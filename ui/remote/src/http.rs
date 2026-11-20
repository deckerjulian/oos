// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Web remote (HTTP, port 8080): live screen in the browser, clicks act as touch taps, buttons for keys and knobs.
//!   GET /             page            GET /screen.png   current screen (PNG, 60 to 175 KB; the page's live view and saving;
//!                                     made off the frame loop, with an ETag: If-None-Match gets 304; see shots.rs)
//!   GET /screen.bmp   current screen (24-bit BMP, 1.8 MB; the page can show this instead)
//!   GET /screen.jpg   the live picture as JPEG from the VPU (`vpu.rs`; PNG where there is none), as /screen.png
//!   GET /wave.csv, /la.csv   the last record, as Storage > CSV writes it
//!   POST /waveform?name=...&slot=1   load a waveform into a chosen reference (no file written; parsed on a thread of
//!                     its own and put into the slot by the frame loop, see `WaveJob`)
//!   GET /ui-state     source, acquisition state and capture count for the remote controls
//!   GET /cmd?c=...    one key, knob or touch command (cmd.rs syntax: "key runstop", "knob vscale +1", "tap 100 200");
//!                     other commands are refused (`cmd::remote_ok`)
//!   GET /lxi/identification   LXI identification XML (LXI Device Specification 2011, 10.2)
//!   POST /mcp         Model Context Protocol (JSON-RPC 2.0, streamable HTTP transport, plain JSON responses): tools
//!                     `scpi` (any SCPI command of scpi.rs, returns the response) and `screenshot` (PNG)
//!   GET /update       update state and upload form; POST /update with a signed package (.oosupd) installs it
//!                     (ui/update.rs: the signature is checked before anything is written)
//! Unauthenticated: bench LAN only (an update must carry a trusted signature, but the community key is public).
//! `net.allow` (`netacl`, docs/network.md 6) limits `POST /update` and `/mcp` to the listed address prefixes.

use std::net::{TcpListener, TcpStream};
use std::time::Instant;

/// What the web server asks of the program it serves (oos-ui: `site::Ctx`, the screen for one pass of its frame
/// loop). The server reads the requests, keeps the limits and sends the answers; the pages, the pictures, the uploads
/// and the routes of the screen are the program's.
pub trait Site {
    /// The traces of an uploaded waveform file, read on a thread of the server.
    type Upload: Send + 'static;
    /// The longest waveform file `POST /waveform` takes.
    const MAX_WAVEFORM: usize;
    /// Every pass, before the requests (the live picture's turn).
    fn pass(&mut self);
    /// `GET /screen.png` or `/screen.jpg` on `s` (`have`: the ETag of If-None-Match): the program answers it, now or
    /// later.
    fn picture(&mut self, s: &TcpStream, have: Option<String>, jpeg: bool);
    /// Whether `GET /cmd` may run the text command `line`.
    fn remote_ok(&self, line: &str) -> bool;
    /// The answer of a route of the program: (content type, body, whether the body is the reason it failed and not
    /// the data asked for); None for a route that is none of its. An empty content type is 202 with no body.
    fn answer(&mut self, route: &str, post: bool, body: &[u8]) -> Option<(&'static str, Vec<u8>, bool)>;
    /// What `GET /update` shows: what gives the state of the update (it may take a while: the server calls it on a
    /// thread of its own, not in the frame loop) and the result of the last upload.
    fn update_state(&mut self) -> (fn() -> String, String);
    /// The session of the screen: an upload that started in another one is not taken.
    fn waveform_epoch(&self) -> u64;
    /// Reads an uploaded waveform file (on the server's thread for it, not the frame loop).
    fn parse_upload(file: &[u8], name: &str) -> Result<Self::Upload, String>;
    /// Puts the traces of an upload into reference slot `slot` (R1 is 0) when the session is still the one of `epoch`:
    /// (taken, the text for the page).
    fn take_upload(
        &mut self,
        parsed: Result<Self::Upload, String>,
        name: &str,
        slot: usize,
        epoch: u64,
    ) -> (bool, String);
}

/// Longest request: `POST /update` (a package is ~31 MB, the image at most 64 MiB), every other request.
const MAX_UPDATE: usize = 70 << 20;
const MAX_OTHER: usize = 1 << 20;
/// Open connections at once (more are closed at accept).
const MAX_PENDING: usize = 8;
/// Answers waiting for their clients (`Http::sending`); one more closes its connection unanswered.
const MAX_SENDING: usize = 8;
/// A body above this (an update package, a waveform) is taken one at a time: a second one is answered 503 when its
/// head is in, so the requests waiting hold at most one large body and `MAX_PENDING` small ones.
const LARGE_BODY: usize = MAX_OTHER;
/// Idle time and whole time a request may take (the upload of a package over 10 Mbit/s takes ~30 s).
const IDLE_S: u64 = 10;
const DEADLINE_S: u64 = 120;
/// Answers above this size are sent from a thread of their own: over WLAN a picture takes long enough to stall the
/// frame loop, and with it the scope's own screen and keys. A screen as PNG is 60 to 175 KB (a BMP 1.8 MB), so it
/// goes the same way; the page and the small answers fit into the socket's buffer and go at once.
pub const SEND_INLINE: usize = 48 << 10;
/// Which instrument addresses the identification page lists. The default is the scope: VXI-11 and the SCPI socket.
#[derive(Clone, Debug)]
pub struct Offer {
    pub scpi_port: Option<u16>,
    pub vxi11: bool,
    pub hislip_port: Option<u16>,
}

impl Default for Offer {
    fn default() -> Offer {
        Offer { scpi_port: Some(5555), vxi11: true, hislip_port: None }
    }
}

/// A request being read.
struct Pending {
    s: TcpStream,
    /// the request so far
    req: Vec<u8>,
    /// last data, accepted
    t: Instant,
    t_acc: Instant,
    /// the length of the whole request, once its head is in and the body is taken
    need: Option<usize>,
    /// its body is over `LARGE_BODY`: the one large body being read
    large: bool,
}

/// A waveform upload (`POST /waveform`) parsed on a thread of its own: a file
/// near the 128 MiB limit takes seconds to parse, which on the frame loop stopped the scope's screen and keys. The
/// frame loop puts the traces into the slot (`Ui::commit_waveform_traces_at`) and answers the client, which waits.
struct WaveJob<U> {
    s: TcpStream,
    name: String,
    /// R1 is 0
    slot: usize,
    /// `Ui::waveform_epoch` at the upload: a session changed meanwhile does not get the late file
    epoch: u64,
    parsed: std::sync::mpsc::Receiver<Result<U, String>>,
}

impl<U: Send + 'static> WaveJob<U> {
    /// Starts parsing `req[body..]`; None when no thread could be started.
    fn start(
        s: TcpStream,
        req: Vec<u8>,
        body: usize,
        name: String,
        slot: usize,
        epoch: u64,
        parse: fn(&[u8], &str) -> Result<U, String>,
    ) -> Option<WaveJob<U>> {
        let (tx, parsed) = std::sync::mpsc::channel();
        let file = name.clone();
        std::thread::Builder::new()
            .name("web-wave".into())
            .spawn(move || {
                oos_system::system::lower_priority();
                let _ = tx.send(parse(&req[body..], &file));
            })
            .ok()?;
        Some(WaveJob { s, name, slot, epoch, parsed })
    }

    /// The answer once the parse is done: the traces in the slot, or why not. None while it runs.
    fn finish<S: Site<Upload = U>>(&self, site: &mut S) -> Option<(bool, String)> {
        let parsed = match self.parsed.try_recv() {
            Ok(parsed) => parsed,
            Err(std::sync::mpsc::TryRecvError::Empty) => return None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("The waveform reader stopped; try again.".into()),
        };
        Some(site.take_upload(parsed, &self.name, self.slot, self.epoch))
    }
}

pub struct Http<U> {
    l: Option<TcpListener>,
    pending: Vec<Pending>,
    /// the waveform upload being parsed
    wave: Option<WaveJob<U>>,
    /// answers the client has not taken yet, written as it takes them (ADR 0066); the connection closes after
    sending: Vec<(TcpStream, oos_system::net::Outbox)>,
    /// who may install an update or use MCP (empty: everyone); checked per request, so a change counts at once
    pub acl: oos_system::netacl::Shared,
    /// the addresses the identification page names
    offer: Offer,
}

/// The head of an answer of `Http::answer`: status, type (none for an answer without a body: the 202 of an MCP
/// notification), length, the `extra` lines, no caching, and the connection closes after it.
fn answer_head(status: &str, ctype: &str, length: usize, extra: &str) -> String {
    let ctype = if ctype.is_empty() { String::new() } else { format!("Content-Type: {ctype}\r\n") };
    format!("HTTP/1.1 {status}\r\n{ctype}Content-Length: {length}\r\n{extra}Cache-Control: no-store\r\nConnection: close\r\n\r\n")
}

/// The web page /update: the state of `Ui::update_status` and an upload form.
fn update_page(status: &str, last: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;");
    format!(
        r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>OOS update</title>
<style>{}</style></head>
<body><header><b>OOS update</b><nav><a href="/">Remote</a></nav></header>
<pre>{}</pre>
<p>Package (<code>.oosupd</code>, signed): <input type="file" id="f" accept=".oosupd"> <button onclick="up()">Install</button></p>
<pre id="r">{}</pre>
<p>The new build starts on trial at the next power-on; a failed start writes the previous build back.
Command line: <code>curl --data-binary @oos-dev.oosupd http://&lt;scope&gt;:8080/update</code></p>
<script>
function up(){{const f=document.getElementById('f').files[0];if(!f)return;const r=document.getElementById('r');
r.textContent='uploading '+f.size+' bytes ...';
fetch('/update',{{method:'POST',body:f}}).then(x=>x.text()).then(t=>{{r.textContent=t;setTimeout(()=>location.reload(),15000)}})}}
</script></body></html>"#,
        STYLE,
        esc(status),
        esc(last)
    )
}

/// Style of both pages, the remote and /update.
const STYLE: &str = r#"*{box-sizing:border-box}
body{background:#14171c;color:#e4e7ea;font:15px -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;margin:0 auto;max-width:1048px;padding:12px}
header{display:flex;align-items:center;gap:14px;margin-bottom:10px}header b{font-size:17px}header nav{margin-left:auto;display:flex;gap:14px}
a{color:#5cb2f0;text-decoration:none}a:hover{text-decoration:underline}
#st{color:#9aa4af;font-size:13px}#st.bad{color:#f0a93b}
button{background:#242a33;color:#e4e7ea;border:1px solid #3a424d;border-radius:6px;padding:7px 11px;font:inherit;cursor:pointer;touch-action:manipulation}
button:hover{background:#2f3743}button:active{background:#3a4452}button.on{border-color:#f0a93b;color:#f0a93b}
button:focus-visible,a:focus-visible,select:focus-visible{outline:2px solid #f0a93b;outline-offset:3px}button:disabled{opacity:.55;cursor:wait}
select{background:#242a33;color:#e4e7ea;border:1px solid #3a424d;border-radius:6px;padding:7px;font:inherit}
pre{background:#0d0f13;border:1px solid #2a3038;border-radius:6px;padding:12px;overflow-x:auto;white-space:pre-wrap}
code{font:13px ui-monospace,Menlo,Consolas,monospace}"#;

pub const PAGE: &str = r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>OOS remote</title>
<link rel="stylesheet" href="/style.css">
<style>img{display:block;width:100%;height:auto;border:1px solid #3a424d;border-radius:6px;cursor:crosshair;touch-action:none;user-select:none;-webkit-user-select:none}
.g{display:flex;flex-wrap:wrap;gap:12px 22px;margin-top:12px}.k span{display:block;font-size:12px;color:#9aa4af;margin-bottom:3px}.k div{display:flex;flex-wrap:wrap;gap:4px}
.workflow{display:flex;flex-wrap:wrap;align-items:center;gap:8px;margin:0 0 8px}.workflow label{display:flex;align-items:center;gap:6px;color:#b8c0ca;font-size:13px}.workflow .primary{border-color:#f0a93b;color:#f0a93b}.session{margin-left:auto;color:#b8c0ca;font-size:13px;font-variant-numeric:tabular-nums}.file-hint{min-height:20px;margin:0 0 8px;color:#b8c0ca;font-size:13px}.file-hint.bad{color:#ffab91}header{flex-wrap:wrap}@media(max-width:620px){.session{margin-left:0;flex-basis:100%}.workflow{gap:6px}button{min-height:40px}}</style></head>
<body><header><b>OOS remote</b><span id="st">connecting ...</span><nav><a href="/update">Update</a></nav></header>
<div class="workflow" aria-label="Waveform and source actions">
<button id="load-wave" class="primary" onclick="askFile()">Load waveform</button>
<input id="wave-file" type="file" accept=".csv,.bin,.wfm,.trc,.isf,.tr0,.wfmx" hidden>
<label>Into <select id="wave-slot" aria-label="Reference slot"><option value="" selected>Choose slot</option><option value="1">R1</option><option value="2">R2</option><option value="3">R3</option><option value="4">R4</option><option value="5">R5</option><option value="6">R6</option><option value="7">R7</option><option value="8">R8</option><option value="9">R9</option><option value="10">R10</option></select></label>
<button id="sim-mode" onclick="c('workflow sim')">SIM</button><button id="return-live" onclick="c('workflow live')" hidden>Return to live</button>
<button id="acquire-toggle" onclick="c('key runstop')" aria-label="Run or stop acquisition">Run/Stop</button><button onclick="c('key single')">Single</button>
<button onclick="c('workflow measure')">Add measurement</button><span id="session-state" class="session" aria-live="polite">Reading source...</span></div>
<p id="wave-status" class="file-hint" role="status">Choose a slot, then CSV, BIN, WFM, TRC, ISF, TR0 or WFMX · up to 128 MiB. You can also drop a file on the waveform.</p>
<img id="s" width="1024" height="600" alt="scope screen" src="/screen.png">
<div class="g"><div class="k"><span>Acquisition</span><div>
<button onclick="c('key runstop')">Run/Stop</button><button onclick="c('key single')">Single</button><button onclick="c('key auto')">Autoset</button><button onclick="c('key clear')">Clear</button><button onclick="c('key default')">Default</button></div></div>
<div class="k"><span>Channels</span><div>
<button onclick="c('key ch1')">CH1</button><button onclick="c('key ch2')">CH2</button><button onclick="c('key ch3')">CH3</button><button onclick="c('key ch4')">CH4</button><button onclick="c('key math')">Math</button><button onclick="c('key ref')">Ref</button><button onclick="c('key la')">LA</button><button onclick="c('key gi')">Gen</button></div></div>
<div class="k"><span>Tools</span><div>
<button onclick="c('workflow settings')">Measure settings</button><button onclick="c('key cursor')">Cursor</button><button onclick="c('key analyse')">Decode</button><button onclick="c('key tmenu')">Trigger</button><button onclick="c('key force')">Force trigger</button></div></div></div>
<div class="g">
<div class="k"><span>V scale</span><div><button onclick="c('knob vscale -1')">-</button><button onclick="c('knob vscale +1')">+</button></div></div>
<div class="k"><span>V pos</span><div><button onclick="c('knob vpos -5')">-</button><button onclick="c('knob vpos +5')">+</button></div></div>
<div class="k"><span>H scale</span><div><button onclick="c('knob hscale -1')">-</button><button onclick="c('knob hscale +1')">+</button><button onclick="c('knob hscale push')">zoom</button></div></div>
<div class="k"><span>H pos</span><div><button onclick="c('knob hpos -5')">-</button><button onclick="c('knob hpos +5')">+</button></div></div>
<div class="k"><span>Trig level</span><div><button onclick="c('knob tlevel -5')">-</button><button onclick="c('knob tlevel +5')">+</button><button onclick="c('knob tlevel push')">50%</button></div></div>
<div class="k"><span>Multi 1</span><div><button onclick="c('knob multi1 -1')">-</button><button onclick="c('knob multi1 +1')">+</button><button onclick="c('knob multi1 push')">push</button></div></div>
<div class="k"><span>Multi 2</span><div><button onclick="c('knob multi2 -1')">-</button><button onclick="c('knob multi2 +1')">+</button></div></div></div>
<div class="g"><div class="k"><span>Save to this computer</span><div>
<button onclick="dl('/screen.png','png')">Screenshot (PNG)</button><button onclick="dl('/wave.csv','csv')">Waveform (CSV)</button><button onclick="dl('/wave.bin','bin')">Waveform (binary)</button><button onclick="dl('/la.csv','la.csv')">LA lines (CSV)</button></div></div>
<div class="k"><span>Browser picture only</span><div><button id="p" onclick="pause()">Pause picture</button></div></div>
<div class="k"><span>Live picture format</span><div><button id="fp" onclick="setfmt('png')" title="compressed, about 90 KB per picture">PNG</button><button id="fb" onclick="setfmt('bmp')" title="uncompressed, 1.8 MB per picture">BMP</button><button id="fj" onclick="setfmt('jpg')" title="made by the scope's video encoder, about 70 KB per picture: up to 20 a second, but text a little less sharp">JPEG</button></div></div></div>
<script>
const s=document.getElementById('s'),st=document.getElementById('st'),pb=document.getElementById('p'),fp=document.getElementById('fp'),fb=document.getElementById('fb'),fj=document.getElementById('fj');
let paused=false,running=false,n=0,msg='',until=0,fmt='png';
try{const f=localStorage.getItem('fmt');if(f=='bmp'||f=='jpg')fmt=f;}catch(e){}
function setfmt(f){fmt=f;try{localStorage.setItem('fmt',f);}catch(e){}fp.className=f=='png'?'on':'';fb.className=f=='bmp'?'on':'';fj.className=f=='jpg'?'on':'';}
setfmt(fmt);
function say(t){msg=t;until=Date.now()+5000;show();}
function show(bad){st.textContent=Date.now()<until?msg:(paused?'picture paused':bad?'no connection':'connected, '+n+' pictures/s');st.className=bad?'bad':'';}
function c(x){return fetch('/cmd?c='+encodeURIComponent(x)).then(async r=>{const text=await r.text();if(!r.ok||text==='refused')throw Error(text);refreshState();}).catch(()=>say('Command not sent. Check the connection.'));}
let statePending=false,uploading=false,workflowState=null;
function askFile(){const slot=document.getElementById('wave-slot'),status=document.getElementById('wave-status');if(!slot.value){status.textContent='Choose a slot from R1 to R10 before the file.';status.className='file-hint bad';return;}document.getElementById('wave-file').click();}
function refreshState(){if(statePending||document.hidden)return;statePending=true;fetch('/ui-state').then(r=>{if(!r.ok)throw Error();return r.json();}).then(v=>{
const mode=v.mode==='sim'?'SIM':v.mode==='reference'?'Stored waveform':'Live';
workflowState=v;
document.getElementById('session-state').textContent=[mode,v.source,v.status,(v.captures>0?'Capture '+v.captures:'')].filter(Boolean).join(' · ');
document.getElementById('sim-mode').className=v.mode==='sim'?'on':'';document.getElementById('return-live').hidden=v.mode==='live';
document.getElementById('acquire-toggle').textContent=v.running?'Stop':'Run';
}).catch(()=>{}).finally(()=>{statePending=false;});}
async function uploadWave(file){if(!file||uploading)return;const status=document.getElementById('wave-status'),button=document.getElementById('load-wave'),slot=document.getElementById('wave-slot');
if(!slot.value){status.textContent='Choose a slot from R1 to R10 before the file.';status.className='file-hint bad';document.getElementById('wave-file').value='';return;}
if(file.size>128*1024*1024){status.textContent='That file is larger than 128 MiB. Export a shorter waveform and try again.';status.className='file-hint bad';return;}
uploading=true;button.disabled=true;slot.disabled=true;button.textContent='Loading...';status.className='file-hint';status.textContent='Loading '+file.name+' into R'+slot.value+'...';
try{const response=await fetch('/waveform?name='+encodeURIComponent(file.name)+'&slot='+encodeURIComponent(slot.value),{method:'POST',headers:{'Content-Type':'application/octet-stream'},body:file});const text=await response.text();if(!response.ok)throw Error(text);status.textContent=text;refreshState();}
catch(error){status.textContent='Could not load waveform: '+(error.message||'check the connection and try again.');status.className='file-hint bad';}
finally{uploading=false;button.disabled=false;slot.disabled=false;button.textContent='Load waveform';document.getElementById('wave-file').value='';}}
document.getElementById('wave-file').addEventListener('change',event=>uploadWave(event.target.files[0]));
s.addEventListener('dragover',event=>{if(Array.from(event.dataTransfer?.types||[]).includes('Files'))event.preventDefault();});
s.addEventListener('drop',event=>{event.preventDefault();uploadWave(event.dataTransfer.files[0]);});
document.addEventListener('keydown',event=>{if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='o'){event.preventDefault();askFile();}});
refreshState();
function dl(u,ext){say('saving ...');Promise.all([fetch(u).then(r=>r.ok?r.blob():r.text().then(t=>Promise.reject(t))),fetch('/stamp').then(r=>r.ok?r.text():'').catch(()=>'')]).then(([b,t])=>{
const a=document.createElement('a'),d=t.trim()||new Date(Date.now()-new Date().getTimezoneOffset()*60000).toISOString().slice(0,19).replace(/-|:/g,'').replace('T','-');
a.href=URL.createObjectURL(b);a.download='oos-'+d+'.'+ext;document.body.appendChild(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(a.href),10000);say('saved');
}).catch(e=>say(typeof e=='string'?e:'not saved: no connection'));}
function pause(){paused=!paused;pb.textContent=paused?'Resume picture':'Pause picture';pb.className=paused?'on':'';show();go();}
function pt(e){const r=s.getBoundingClientRect();const p=e.changedTouches?e.changedTouches[0]:e;return [Math.round((p.clientX-r.left)*(s.naturalWidth||1024)/r.width),Math.round((p.clientY-r.top)*(s.naturalHeight||600)/r.height)];}
let d0=null;
function down(e){e.preventDefault();d0=pt(e);}
function up(e){if(!d0)return;e.preventDefault();const p=pt(e),start=d0;d0=null;if(Math.abs(p[0]-start[0])+Math.abs(p[1]-start[1])<8){c('tap '+start[0]+' '+start[1]);}else c('drag '+start[0]+' '+start[1]+' '+p[0]+' '+p[1]);}
s.addEventListener('mousedown',down);s.addEventListener('mouseup',up);s.addEventListener('touchstart',down,{passive:false});s.addEventListener('touchend',up,{passive:false});
s.ondragstart=()=>false;
let count=0;setInterval(()=>{n=count;count=0;if(!document.hidden){refreshState();if(!paused)show(st.className=='bad');}},1000);
// no pictures while paused or in a background tab: each one is 90 KB as PNG, 1.8 MB as BMP, which the scope has to send
// a PNG comes with an ETag: sent back, the scope holds the request until its screen has changed (304 after 2 s), so
// the next request can go at once; a BMP has none, so a pause between them
let tag='',shown='';
function next(ok){running=false;show(!ok);setTimeout(go,!ok?1000:fmt!='bmp'?0:150);}
function go(){if(running||paused||document.hidden)return;running=true;const f=fmt;
fetch('/screen.'+f+'?t='+Date.now(),{cache:'no-store',headers:tag&&f!='bmp'?{'If-None-Match':tag}:{}}).then(r=>{
if(r.status==304)return next(true);
if(!r.ok)return next(false);
const t=f!='bmp'&&r.headers.get('ETag')||'';
return r.blob().then(b=>{const u=URL.createObjectURL(b),i=new Image();
i.onload=()=>{s.src=u;if(shown)URL.revokeObjectURL(shown);shown=u;tag=t;count++;next(true);};
i.onerror=()=>{URL.revokeObjectURL(u);next(false);};i.src=u;});
}).catch(()=>next(false));}
document.addEventListener('visibilitychange',go);go();
</script></body></html>"#;

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                // on the bytes: the two characters after % need not be ASCII
                let hex = |c: u8| (c as char).to_digit(16);
                if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                    out.push((h * 16 + l) as u8);
                }
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A browser supplies only a display name. It never selects a path on the instrument.
fn waveform_name(query: &str) -> String {
    let decoded = query.split('&').find_map(|part| part.strip_prefix("name=")).map(unescape).unwrap_or_default();
    let leaf = decoded.rsplit(['/', '\\']).next().unwrap_or("");
    let name: String = leaf.chars().filter(|c| !c.is_control()).take(128).collect();
    if name.trim().is_empty() {
        "waveform.csv".into()
    } else {
        name
    }
}

impl<U: Send + 'static> Http<U> {
    pub fn open(port: u16) -> Http<U> {
        Http {
            l: Self::bind(port),
            pending: Vec::new(),
            wave: None,
            sending: Vec::new(),
            acl: oos_system::netacl::Shared::default(),
            offer: Offer::default(),
        }
    }

    fn bind(port: u16) -> Option<TcpListener> {
        let l = oos_system::net6::tcp_service(port, "web remote");
        if l.is_none() {
            eprintln!("oos-ui: web port {} not available", port);
        }
        l
    }

    /// A port that was still taken when it was opened (the oos-ui before this one, ending, #212): taken now if it is
    /// free. True when that happened.
    pub fn listen_again(&mut self, port: u16) -> bool {
        if self.l.is_some() {
            return false;
        }
        self.l = oos_system::net6::tcp_service(port, "web remote");
        self.l.is_some()
    }

    pub fn port(&self) -> Option<u16> {
        self.l.as_ref().and_then(|l| l.local_addr().ok()).map(|a| a.port())
    }

    /// The addresses the identification page names. Set from the listeners that actually came up.
    pub fn offer(&mut self, o: Offer) {
        self.offer = o;
    }

    /// System > Network > Remote services: the port open (`Some`), or closed with the requests waiting on it.
    pub fn set_open(&mut self, port: Option<u16>) {
        match port {
            Some(p) if self.l.is_none() => self.l = Self::bind(p),
            Some(_) => {}
            None => {
                if self.l.take().is_some() {
                    eprintln!("oos-ui: web remote off");
                }
                self.pending.clear();
                self.sending.clear();
            }
        }
    }

    /// Accepts connections, answers complete requests; text commands are returned for the caller to run.
    pub fn poll<S: Site<Upload = U>>(&mut self, site: &mut S) -> Vec<String> {
        let mut cmds = Vec::new();
        self.sending.retain_mut(|(s, out)| out.flush(s) == oos_system::net::Sent::Later);
        site.pass();
        self.finish_wave(site);
        self.accept_new();
        let mut i = 0;
        while i < self.pending.len() {
            let mut buf = [0u8; 65536];
            // a waveform being parsed holds its body too
            let large_taken = self.pending.iter().any(|p| p.large) || self.wave.is_some();
            let Pending { s, req, t, t_acc, need, large } = &mut self.pending[i];
            let mut dead = read_some(s, req, t, *need, &mut buf);
            // headers complete, and the body when there is a Content-Length
            let hdr_end = req.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4);
            if hdr_end.is_none() && req.len() > 16384 {
                dead = true; // no header end in 16 KiB
            }
            let mut too_big = false;
            // answered when the head is in, before any of the body is kept: 403 refused, 503 busy
            let mut early = None;
            // what goes back before the connection closes
            let mut last = None;
            let complete = hdr_end.is_some_and(|e| {
                let h = String::from_utf8_lossy(&req[..e]).to_ascii_lowercase();
                let n = h
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                let first = h.lines().next().unwrap_or("");
                let upload_path = first.split_whitespace().nth(1).unwrap_or("").split('?').next().unwrap_or("");
                let limit = if first.starts_with("post ") && upload_path == "/update" {
                    MAX_UPDATE
                } else if first.starts_with("post ") && upload_path == "/waveform" {
                    S::MAX_WAVEFORM
                } else {
                    MAX_OTHER
                };
                too_big = n > limit || req.len() > limit + e;
                if !too_big && need.is_none() {
                    // what only `net.allow` may send is refused before its body is read, not after
                    let guarded = upload_path == "/mcp" || (first.starts_with("post ") && upload_path == "/update");
                    if guarded && !self.acl.permits_peer(s.peer_addr()) {
                        early = Some("403 Forbidden");
                        return false;
                    }
                    if n > LARGE_BODY && large_taken {
                        early = Some("503 Service Unavailable");
                        return false;
                    }
                    // the whole request in one allocation, not a doubling past it
                    req.reserve_exact((e + n).saturating_sub(req.len()));
                    *need = Some(e + n);
                    *large = n > LARGE_BODY;
                }
                too_big || e.checked_add(n).is_some_and(|need| req.len() >= need)
            });
            // a HEAD gets no body here either
            let head_only = req.starts_with(b"HEAD ");
            if let Some(status) = early {
                let msg = if status.starts_with("403") { "refused" } else { "busy, try again" };
                last = Some(early_answer(status, msg, head_only));
                dead = true;
            } else if too_big {
                last = Some(early_answer("413 Payload Too Large", "request too large", head_only));
                dead = true;
            } else if complete {
                if !self.answer(i, hdr_end, site, &mut cmds) {
                    continue;
                }
                dead = true;
            } else if t.elapsed().as_secs() > IDLE_S || t_acc.elapsed().as_secs() > DEADLINE_S {
                dead = true;
            }
            if dead {
                let p = self.pending.remove(i);
                if let Some(bytes) = last {
                    self.send(p.s, bytes);
                }
            } else {
                i += 1;
            }
        }
        cmds
    }

    /// Answers the waveform upload whose parsing has finished.
    fn finish_wave<S: Site<Upload = U>>(&mut self, site: &mut S) {
        if let Some((ok, text)) = self.wave.as_ref().and_then(|job| job.finish(site)) {
            let job = self.wave.take().unwrap();
            let status = if ok { "200 OK" } else { "409 Conflict" };
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\
                Cache-Control: no-store\r\nConnection: close\r\n\r\n{text}",
                text.len()
            );
            self.send(job.s, head.into_bytes());
        }
    }

    /// Writes `bytes` to `s` as far as it takes them now; the rest goes out on the next passes (`sending`), and the
    /// connection closes when all is out, or when the client takes nothing for `net::SEND_STALL` or has not taken all
    /// after `net::SEND_LIMIT`. While `MAX_SENDING` answers already wait, it closes unanswered: no byte of an answer
    /// that could not be finished.
    fn send(&mut self, s: TcpStream, bytes: Vec<u8>) {
        if self.sending.len() >= MAX_SENDING {
            return;
        }
        let mut out = oos_system::net::Outbox::default();
        out.push(&bytes);
        if out.flush(&s) == oos_system::net::Sent::Later {
            self.sending.push((s, out));
        }
    }

    /// Takes the connections waiting at the listener, up to `MAX_PENDING`.
    fn accept_new(&mut self) {
        if let Some(l) = &self.l {
            while let Ok((s, _)) = l.accept() {
                if self.pending.len() >= MAX_PENDING {
                    continue; // dropped: closes the connection
                }
                let _ = s.set_nonblocking(false);
                let now = Instant::now();
                self.pending.push(Pending { s, req: Vec::new(), t: now, t_acc: now, need: None, large: false });
            }
        }
    }

    /// Answers the complete request of connection `i`; false when the connection was handed on and is gone.
    fn answer<S: Site<Upload = U>>(
        &mut self,
        i: usize,
        hdr_end: Option<usize>,
        site: &mut S,
        cmds: &mut Vec<String>,
    ) -> bool {
        let Pending { s, req, .. } = &mut self.pending[i];
        // only the header as text: the body may be a 31 MB binary package
        let text = String::from_utf8_lossy(&req[..hdr_end.unwrap_or(req.len())]).into_owned();
        let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
        let (route, query) = path.split_once('?').unwrap_or((&path, ""));
        let req_body = &req[hdr_end.unwrap_or(req.len())..];
        let post = text.starts_with("POST ");
        let method = text.split_whitespace().next().unwrap_or("");
        // HEAD: the head GET would have, without its body (#205)
        let head_only = method == "HEAD";
        let allow = methods_of(route);
        // what changes the scope beyond keys and knobs: only from the allowed prefixes (also at the head)
        let peer_ok = self.acl.permits_peer(s.peer_addr());
        // each answer with its status, chosen where the answer is made (#205: not read back from the body's text)
        const OK: &str = "200 OK";
        const FORBIDDEN: &str = "403 Forbidden";
        let refused = || (FORBIDDEN, "text/plain", b"refused".to_vec());
        let (status, ctype, body): (&str, &str, Vec<u8>) = match route {
            _ if foreign(&text, route, post) => refused(),
            "/update" | "/mcp" if (post || route == "/mcp") && !peer_ok => refused(),
            // a route asked with a method it does not take: 405 with the ones it takes (#205)
            _ if allow.is_some_and(|a| !a.split(", ").any(|m| m == method)) => {
                ("405 Method Not Allowed", "text/plain", b"method not allowed".to_vec())
            }
            "/" => (OK, "text/html; charset=utf-8", PAGE.as_bytes().to_vec()),
            "/style.css" => (OK, "text/css", STYLE.as_bytes().to_vec()),
            "/waveform" if post => {
                let slot = query
                    .split('&')
                    .find_map(|part| part.strip_prefix("slot="))
                    .and_then(|value| value.parse::<usize>().ok());
                match slot.filter(|s| (1..=10).contains(s)) {
                    // the request itself is wrong, not the file or the state of the scope
                    None => (
                        "400 Bad Request",
                        "text/plain; charset=utf-8",
                        b"Choose a reference slot from R1 to R10.".to_vec(),
                    ),
                    Some(slot) => {
                        self.waveform_take(i, slot, query, hdr_end, site);
                        return false;
                    }
                }
            }
            "/screen.png" | "/screen.jpg" => {
                let have = header(&text, "if-none-match").map(str::to_string);
                site.picture(s, have, route == "/screen.jpg");
                self.pending.remove(i);
                return false;
            }
            // the state comes from a program that takes a while: the page is made and sent on a thread of the senders
            // (#205), 503 when they are all busy
            "/update" if !post => {
                let (status, last) = site.update_state();
                let made = oos_system::net::answer_in_background(s, move || {
                    let page = update_page(&status(), &last).into_bytes();
                    let head = answer_head(OK, "text/html; charset=utf-8", page.len(), "");
                    (head, if head_only { Vec::new() } else { page })
                });
                if !made {
                    oos_system::net::busy_answer(s);
                }
                self.pending.remove(i);
                return false;
            }
            "/lxi/identification" => {
                (OK, "text/xml", lxi_identification(&self.offer, &oos_hal::model::platform().caps.name).into_bytes())
            }
            // what the screen answers: its state, the last record, an update package, MCP
            "/cmd" => match remote_cmds(query, cmds, site) {
                true => (OK, "text/plain", b"ok".to_vec()),
                false => refused(),
            },
            _ => match site.answer(route, post, req_body) {
                // the route says the body is why it failed, not the data asked for
                Some((ctype, body, true)) => ("409 Conflict", ctype, body),
                // an MCP notification: no body
                Some(("", body, false)) => ("202 Accepted", "", body),
                Some((ctype, body, false)) => (OK, ctype, body),
                None => ("404 Not Found", "text/plain", b"not found".to_vec()),
            },
        };
        let allowed = match allow {
            Some(a) if status.starts_with("405") => format!("Allow: {}\r\n", a),
            _ => String::new(),
        };
        let head = answer_head(status, ctype, body.len(), &allowed);
        let body = if head_only { Vec::new() } else { body };
        if body.len() > SEND_INLINE {
            if !oos_system::net::send_in_background(s, head, body) {
                oos_system::net::busy_answer(s);
            }
            return true;
        }
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(&body);
        let p = self.pending.remove(i);
        self.send(p.s, bytes);
        false
    }

    /// POST /waveform to `slot`: the request goes whole to the parser, or is answered busy while one is parsed; the
    /// connection leaves `pending` either way.
    fn waveform_take<S: Site<Upload = U>>(
        &mut self,
        i: usize,
        slot: usize,
        query: &str,
        hdr_end: Option<usize>,
        site: &mut S,
    ) {
        let Pending { s, req, .. } = &mut self.pending[i];
        if self.wave.is_some() {
            oos_system::net::busy_answer(s);
            self.pending.remove(i);
        } else {
            // the request goes to the parser whole, its body is not copied out
            let (body, epoch) = (hdr_end.unwrap_or(req.len()), site.waveform_epoch());
            let Pending { s, req, .. } = self.pending.remove(i);
            match WaveJob::start(s, req, body, waveform_name(query), slot - 1, epoch, S::parse_upload) {
                Some(job) => self.wave = Some(job),
                None => eprintln!("oos-ui: no thread for a waveform upload"),
            }
        }
    }
}

/// Reads what connection `s` has now into `req`; true when it closed or failed.
fn read_some(s: &TcpStream, req: &mut Vec<u8>, t: &mut Instant, need: Option<usize>, buf: &mut [u8]) -> bool {
    let mut dead = false;
    // up to 2 MB per connection and frame (an update package is ~31 MB: a few seconds at 100 Mbit/s), and once
    // the length is known not beyond the request: the buffer reserved for it is not outgrown
    let mut got = 0;
    while got < 2 << 20 {
        let room = need.map_or(buf.len(), |n| n.saturating_sub(req.len()).min(buf.len()));
        if room == 0 {
            break;
        }
        match oos_system::net::read_now(s, &mut buf[..room]) {
            Ok(0) => {
                dead = true;
                break;
            }
            Ok(n) => {
                req.extend_from_slice(&buf[..n]);
                got += n;
                *t = Instant::now();
            }
            Err(e) if oos_system::net::later(&e) => break,
            Err(_) => {
                dead = true;
                break;
            }
        }
    }
    dead
}

/// The methods a route takes, as its `Allow` header names them; None for a path that is no route (404 whatever the
/// method). A HEAD is taken where a GET is, except on `/cmd`, whose GET does something, and on the live pictures,
/// whose status, type and length are known only once the program answers one, after this.
fn methods_of(route: &str) -> Option<&'static str> {
    match route {
        "/waveform" | "/mcp" => Some("POST"),
        "/update" => Some("GET, HEAD, POST"),
        "/cmd" | "/screen.png" | "/screen.jpg" => Some("GET"),
        "/"
        | "/style.css"
        | "/screen.bmp"
        | "/wave.csv"
        | "/wave.bin"
        | "/la.csv"
        | "/ui-state"
        | "/stamp"
        | "/lxi/identification" => Some("GET, HEAD"),
        _ => None,
    }
}

/// GET /cmd: the commands `c=` of the query go to `cmds` when a remote may give every one of them; false and none of
/// them when one may not (#205: a request that is refused does nothing, not the half of it that was allowed).
fn remote_cmds<S: Site>(query: &str, cmds: &mut Vec<String>, site: &S) -> bool {
    let given: Vec<String> = query.split('&').filter_map(|kv| kv.strip_prefix("c=")).map(unescape).collect();
    if !given.iter().all(|c| site.remote_ok(c)) {
        return false;
    }
    cmds.extend(given);
    true
}

/// The value of a header of the request head (name without case).
fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// The LXI identification of `model` (the unit's, as `*IDN?` and the mDNS announcement name it) with the
/// instrument addresses of `o`. The serial number stays 0 (#205).
pub fn lxi_identification(o: &Offer, model: &str) -> String {
    let mut addrs = String::new();
    if o.vxi11 {
        addrs.push_str("<InstrumentAddressString>TCPIP::INSTR</InstrumentAddressString>\n");
    }
    if let Some(p) = o.scpi_port {
        addrs.push_str(&format!("<InstrumentAddressString>TCPIP::{p}::SOCKET</InstrumentAddressString>\n"));
    }
    if let Some(port) = o.hislip_port {
        let device = if port == crate::hislip::PORT { "hislip0".into() } else { format!("hislip0,{port}") };
        addrs.push_str(&format!("<InstrumentAddressString>TCPIP::{device}::INSTR</InstrumentAddressString>\n"));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<LXIDevice xmlns="http://www.lxistandard.org/InstrumentIdentification/1.0">
<Manufacturer>OOS</Manufacturer><Model>{}</Model><SerialNumber>0</SerialNumber>
<FirmwareRevision>oos-ui {}</FirmwareRevision><ManufacturerDescription>OOS oscilloscope</ManufacturerDescription>
<HomepageURL>/</HomepageURL><DriverURLs/>
<UserDescription>OOS</UserDescription>
<IdentificationURL>/lxi/identification</IdentificationURL>
<Interface InterfaceType="LXI" IPType="IPv4" xsi:type="NetworkInformation" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
{addrs}</Interface>
<LXIVersion>1.5</LXIVersion>
</LXIDevice>
"#,
        xml_text(model),
        env!("CARGO_PKG_VERSION")
    )
}

/// `s` with the five characters XML gives a meaning escaped.
fn xml_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// A request a web page on another site made through the browser of the bench PC, not the page of the scope or a tool:
/// it may press keys or install a package without the owner's doing. The Host names the scope by address or local
/// name (a foreign name that resolves here is DNS rebinding), an Origin matches the Host, and the browser's own
/// Sec-Fetch-Site says the rest, also for the GET of an image tag, which has no Origin. A link to the page itself may
/// come from anywhere. curl and the like send neither header.
fn foreign(head: &str, route: &str, post: bool) -> bool {
    let header = |name: &str| {
        head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_ascii_lowercase())
        })
    };
    let name_of = |h: &str| {
        h.rsplit_once(':')
            .filter(|(n, p)| !n.ends_with(':') && p.bytes().all(|b| b.is_ascii_digit()))
            .map_or(h.to_string(), |(n, _)| n.to_string())
    };
    let host = header("host");
    if let Some(h) = &host {
        let n = name_of(h);
        let n = n.trim_matches(['[', ']']);
        let local =
            n.parse::<std::net::IpAddr>().is_ok() || n == "localhost" || !n.contains('.') || n.ends_with(".local");
        if !local {
            return true;
        }
    }
    if let Some(o) = header("origin") {
        let o = o.split_once("://").map_or(o.as_str(), |x| x.1).to_string();
        if host.as_deref() != Some(o.as_str()) {
            return true;
        }
    }
    let page = !post && matches!(route, "/" | "/style.css" | "/update" | "/lxi/identification");
    !page && matches!(header("sec-fetch-site").as_deref(), Some("cross-site" | "same-site"))
}

/// What the tests of oos-ui see of a server they drive through the screen (ui/src/site/tests.rs).
#[cfg(feature = "testkit")]
impl<U> Http<U> {
    /// A waveform upload is being read.
    pub fn uploading(&self) -> bool {
        self.wave.is_some()
    }

    /// The requests being read: (bytes so far, the whole length once its head is in, the room kept for it, whether its
    /// body is the one large body).
    pub fn reading(&self) -> Vec<(usize, Option<usize>, usize, bool)> {
        self.pending.iter().map(|p| (p.req.len(), p.need, p.req.capacity(), p.large)).collect()
    }
}

#[cfg(test)]
mod tests;

/// An answer of `poll` before the request is read whole (403, 413, 503): the text `msg`, or only its head after a HEAD.
fn early_answer(status: &str, msg: &str, head_only: bool) -> Vec<u8> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
        Connection: close\r\n\r\n",
        msg.len()
    );
    let body = if head_only { "" } else { msg };
    format!("{head}{body}").into_bytes()
}
