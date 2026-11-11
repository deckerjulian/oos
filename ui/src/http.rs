// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Web remote (HTTP, port 8080): live screen in the browser, clicks act as touch taps, buttons for keys and knobs.
//!   GET /             page            GET /screen.png   current screen (PNG, 60 to 175 KB; the page's live view and saving;
//!                                     made off the frame loop, with an ETag: If-None-Match gets 304; see `Shots`)
//!   GET /screen.bmp   current screen (24-bit BMP, 1.8 MB; the page can show this instead)
//!   GET /wave.csv, /la.csv   the last record, as Storage > CSV writes it
//!   GET /cmd?c=...    one key, knob or touch command (cmd.rs syntax: "key runstop", "knob vscale +1", "tap 100 200");
//!                     other commands are refused (`cmd::remote_ok`)
//!   GET /lxi/identification   LXI identification XML (LXI Device Specification 2011, 10.2)
//!   POST /mcp         Model Context Protocol (JSON-RPC 2.0, streamable HTTP transport, plain JSON responses): tools
//!                     `scpi` (any SCPI command of scpi.rs, returns the response) and `screenshot` (PNG)
//!   GET /update       update state and upload form; POST /update with a signed package (.oosupd) installs it
//!                     (ui/update.rs: the signature is checked before anything is written)
//! Unauthenticated: bench LAN only (an update must carry a trusted signature, but the community key is public).

use crate::gfx::Canvas;
use crate::ui::Ui;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Longest request: `POST /update` (a package is ~31 MB, the image at most 64 MiB), every other request.
const MAX_UPDATE: usize = 70 << 20;
const MAX_OTHER: usize = 1 << 20;
/// Open connections at once (more are closed at accept).
const MAX_PENDING: usize = 8;
/// Idle time and whole time a request may take (the upload of a package over 10 Mbit/s takes ~30 s).
const IDLE_S: u64 = 10;
const DEADLINE_S: u64 = 120;
/// Answers above this size are sent from a thread of their own: over WLAN a picture takes long enough to stall the
/// frame loop, and with it the scope's own screen and keys. A screen as PNG is 60 to 175 KB (a BMP 1.8 MB), so it
/// goes the same way; the page and the small answers fit into the socket's buffer and go at once.
const SEND_INLINE: usize = 48 << 10;
/// At most this many at once; more are answered 503, and the page tries again.
const MAX_SENDERS: usize = 2;
const SEND_TIMEOUT_S: u64 = 30;
static SENDERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A large answer in the background. False when too many run already, or no thread could be started.
fn send_in_background(s: &TcpStream, head: String, body: impl AsRef<[u8]> + Send + 'static) -> bool {
    use std::sync::atomic::Ordering::SeqCst;
    let Ok(mut c) = s.try_clone() else { return false };
    if SENDERS.fetch_add(1, SeqCst) >= MAX_SENDERS {
        SENDERS.fetch_sub(1, SeqCst);
        return false;
    }
    let t = std::thread::Builder::new().name("web-send".into()).spawn(move || {
        lower_priority();
        let _ = c.set_write_timeout(Some(Duration::from_secs(SEND_TIMEOUT_S)));
        let _ = c.write_all(head.as_bytes());
        let _ = c.write_all(body.as_ref());
        SENDERS.fetch_sub(1, SeqCst);
    });
    if t.is_err() {
        SENDERS.fetch_sub(1, SeqCst);
        return false;
    }
    true
}

fn busy_answer(s: &TcpStream) {
    let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
    let msg = "busy, try again";
    let _ = (&*s).write_all(format!("HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", msg.len(), msg).as_bytes());
}

/// Nice value of the threads of the web remote (Linux; a thread's own value), where `SCHED_IDLE` is not to be had.
const WEB_NICE: i32 = 10;

/// The threads of the web remote (encoder, senders) run only on what the scope leaves over: scheduling class
/// `SCHED_IDLE` (they get a core only while nothing else wants it), and only on the slower cores of a big.LITTLE SoC
/// (RK3399: the four A53; the frame loop keeps the two A72 for itself). Nice 10 where that fails.
fn lower_priority() {
    #[cfg(target_os = "linux")]
    // SAFETY: plain system calls on the calling thread; the parameter and the CPU set are local values.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::pid_t;
        // priority 0, the only one SCHED_IDLE takes (musl's struct has more fields)
        let p: libc::sched_param = std::mem::zeroed();
        // the system call itself: musl's sched_setscheduler() is a stub that always fails
        if libc::syscall(libc::SYS_sched_setscheduler, tid, libc::SCHED_IDLE, &p as *const libc::sched_param) != 0 {
            libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, WEB_NICE);
        }
        let little = little_cores();
        if !little.is_empty() {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            for c in little {
                libc::CPU_SET(c, &mut set);
            }
            libc::sched_setaffinity(tid, std::mem::size_of::<libc::cpu_set_t>(), &set);
        }
    }
}

/// The cores with less than the largest `cpu_capacity` (empty on a SoC whose cores are all alike, or without the
/// file): where the background threads go.
#[cfg(target_os = "linux")]
fn little_cores() -> Vec<usize> {
    static L: std::sync::OnceLock<Vec<usize>> = std::sync::OnceLock::new();
    L.get_or_init(|| {
        let cap: Vec<(usize, u32)> = (0..64)
            .filter_map(|c| std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu{}/cpu_capacity", c)).ok().and_then(|v| v.trim().parse().ok()).map(|v| (c, v)))
            .collect();
        let max = cap.iter().map(|x| x.1).max().unwrap_or(0);
        cap.iter().filter(|x| x.1 < max).map(|x| x.0).collect()
    })
    .clone()
}

/// The fastest the live picture is made: 20 a second.
const SHOT_MIN: Duration = Duration::from_millis(50);
/// The encoder's share of its core at most (half of a slow core, which is idle otherwise): the time between two copies
/// is at least this many times the last encode.
const SHOT_DUTY: u32 = 2;
/// While the screen stands still, it is looked at again only this often.
const SHOT_STILL: Duration = Duration::from_millis(200);
/// After a request, pictures are made ahead for this long (a viewer that left costs no more than this).
const SHOT_AHEAD: Duration = Duration::from_secs(1);
/// A viewer that has the newest picture waits this long for the next one (long poll), then gets 304.
const LONG_POLL: Duration = if cfg!(test) { Duration::from_millis(1000) } else { Duration::from_secs(2) };
/// A picture from a copy at most this old goes out at once to a viewer that does not have it; else a fresh copy first.
const SHOT_FRESH: Duration = Duration::from_millis(300);
/// Connections waiting for a picture at most (more are answered 503).
const SHOT_WAITING: usize = 8;

/// The live picture of the web remote.
///
/// A viewer asks with the ETag of the picture it has (If-None-Match). Has it an older one, or none, it gets the newest
/// at once. Has it the newest, the request waits (long poll) until the screen has changed and a new picture is made,
/// at most `LONG_POLL`, then 304. So every new picture reaches the page without delay, and nothing is asked for in
/// vain.
///
/// The frame loop only copies the screen (2.4 MB), and only while somebody waits, and not more often than the encoder
/// can keep up with on a third of a slow core (`SHOT_DUTY`), at most 20 times a second. The encoder thread compares the
/// copy with the last picture, makes a PNG only of a changed screen and hands it to the waiting viewers; it and the
/// senders get only the time the scope leaves over (`lower_priority`). The PNG used to be made in the frame loop:
/// one viewer at 15 pictures a second took the scope's screen from 27 to 16 frames a second.
pub struct Shots {
    st: Mutex<ShotState>,
    wake: Condvar,
}

#[derive(Default)]
struct ShotState {
    /// a copy of the screen for the encoder: width, height, pixels
    next: Option<(u32, u32, Vec<u32>)>,
    /// the encoder works on a copy
    busy: bool,
    /// connections waiting for a picture newer than the ETag they have, and since when
    waiting: Vec<(TcpStream, Option<String>, Instant)>,
    /// the last picture: its pixels (taken out by the encoder while it compares), size, PNG, ETag
    px: Vec<u32>,
    size: (u32, u32),
    png: Option<Arc<[u8]>>,
    tag: String,
    /// when the last copy was taken, and the time to wait after it before the next
    at: Option<Instant>,
    /// the last request for the live picture: while it is recent, the next picture is made ahead of the next request
    asked: Option<Instant>,
    gap: Duration,
    /// pixel buffer for the next copy
    spare: Vec<u32>,
    /// PNGs made so far
    encodes: u64,
    /// the frame loop hands its screens over (`hand_over`) instead of `tick` copying them
    frames: bool,
    thread: bool,
    quit: bool,
}

impl Shots {
    fn new() -> Arc<Shots> {
        Arc::new(Shots { st: Default::default(), wake: Default::default() })
    }

    fn lock(&self) -> MutexGuard<'_, ShotState> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The answer to one `GET /screen.png` (`have`: the ETag of If-None-Match); the frame loop never encodes here.
    fn request(self: &Arc<Self>, s: &TcpStream, have: Option<String>, cv: &Canvas) {
        let mut st = self.lock();
        st.asked = Some(Instant::now());
        // an older picture or none: the newest at once, when it is fresh (a screenshot must not be an old one)
        let fresh = st.at.is_some_and(|t| t.elapsed() < SHOT_FRESH) && !st.busy && st.next.is_none();
        if let Some(png) = st.png.clone().filter(|_| fresh && have.as_deref() != Some(st.tag.as_str()) && st.size == (cv.w as u32, cv.h as u32)) {
            let tag = st.tag.clone();
            drop(st);
            return answer_shot(s, have.as_deref(), png, &tag);
        }
        if st.waiting.len() >= SHOT_WAITING {
            drop(st);
            return busy_answer(s);
        }
        if !st.thread {
            let me = self.clone();
            if std::thread::Builder::new().name("web-png".into()).spawn(move || me.encoder()).is_err() {
                // no thread: the old way, in the frame loop
                drop(st);
                return answer_shot(s, None, crate::store::png_of(cv).into(), "");
            }
            st.thread = true;
        }
        let Ok(c) = s.try_clone() else { return };
        st.waiting.push((c, have, Instant::now()));
        drop(st);
        self.tick(cv);
    }

    /// Every frame: a copy for the encoder when the time has come and somebody waits, or asked within `SHOT_AHEAD` (the
    /// picture is then ready when the viewer's next request comes); 304 to whoever waited too long.
    fn tick(&self, cv: &Canvas) {
        let mut st = self.lock();
        let watched = st.asked.is_some_and(|t| t.elapsed() < SHOT_AHEAD);
        if st.waiting.is_empty() && !watched {
            return;
        }
        let now = Instant::now();
        let (old, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut st.waiting).into_iter().partition(|w| now - w.2 >= LONG_POLL);
        st.waiting = keep;
        if !old.is_empty() {
            let (png, tag) = (st.png.clone().unwrap_or_else(|| Arc::from(&[][..])), st.tag.clone());
            drop(st);
            for (c, have, _) in old {
                answer_shot(&c, have.as_deref(), png.clone(), &tag);
            }
            st = self.lock();
        }
        if !st.frames && Self::wanted(&st, now, watched) {
            let mut px = std::mem::take(&mut st.spare);
            px.clear();
            px.extend_from_slice(&cv.px);
            st.next = Some((cv.w as u32, cv.h as u32, px));
            st.at = Some(now);
            self.wake.notify_one();
        }
    }

    /// Whether the encoder should get the screen now.
    fn wanted(st: &ShotState, now: Instant, watched: bool) -> bool {
        (!st.waiting.is_empty() || watched) && !st.busy && st.next.is_none() && st.at.map_or(true, |t| now - t >= st.gap)
    }

    /// Right before the frame loop renders the next screen (which clears and draws all of it): the shown screen goes to
    /// the encoder by swapping buffers, no copy. A copy of 2.4 MB in the frame loop pushed everything else of the frame
    /// out of the fast core's cache and made the whole frame about 10 % slower. `whole`: the next render draws every
    /// pixel (not the start-up screen); else nothing is swapped and `tick` copies.
    fn hand_over(&self, cv: &mut Canvas, whole: bool) {
        let mut st = self.lock();
        st.frames = whole;
        let now = Instant::now();
        let watched = st.asked.is_some_and(|t| t.elapsed() < SHOT_AHEAD);
        if !whole || !Self::wanted(&st, now, watched) {
            return;
        }
        let mut px = std::mem::take(&mut st.spare);
        px.resize(cv.px.len(), 0);
        std::mem::swap(&mut px, &mut cv.px);
        st.next = Some((cv.w as u32, cv.h as u32, px));
        st.at = Some(now);
        self.wake.notify_one();
    }

    fn encoder(&self) {
        lower_priority();
        let boot = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64 & 0xffff_ffff);
        loop {
            let mut st = self.lock();
            while st.next.is_none() && !st.quit {
                st = self.wake.wait(st).unwrap_or_else(|e| e.into_inner());
            }
            if st.quit {
                return;
            }
            let Some((w, h, px)) = st.next.take() else { continue };
            st.busy = true;
            let last = std::mem::take(&mut st.px);
            let known = st.png.is_some() && st.size == (w, h);
            drop(st);
            // the comparison and the encoding without the lock: the frame loop may ask meanwhile
            let t0 = Instant::now();
            let png: Option<Arc<[u8]>> = (!known || !same_pixels(&last, &px)).then(|| crate::store::png(w, h, &px).into());
            let took = t0.elapsed();
            let mut st = self.lock();
            st.busy = false;
            let Some(png) = png else {
                // the same screen: who has an older picture gets this one, who has it waits on; the next look later
                st.gap = SHOT_STILL.max(took * SHOT_DUTY);
                st.px = last;
                st.spare = px;
                let tag = st.tag.clone();
                let (lack, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut st.waiting).into_iter().partition(|w| w.1.as_deref() != Some(tag.as_str()));
                st.waiting = keep;
                let png = st.png.clone().unwrap_or_else(|| Arc::from(&[][..]));
                drop(st);
                for (c, have, _) in lack {
                    answer_shot(&c, have.as_deref(), png.clone(), &tag);
                }
                continue;
            };
            st.gap = SHOT_MIN.max(took * SHOT_DUTY);
            st.encodes += 1;
            st.tag = format!("\"{:x}-{}\"", boot, st.encodes);
            st.png = Some(png.clone());
            st.size = (w, h);
            st.px = px;
            st.spare = last;
            let waiting = std::mem::take(&mut st.waiting);
            let tag = st.tag.clone();
            drop(st);
            for (c, have, _) in waiting {
                answer_shot(&c, have.as_deref(), png.clone(), &tag);
            }
        }
    }
}

/// Whether two screens are the same, eight bytes at a step. `==` on the slices ends in musl's memcmp, which goes byte
/// by byte: 12 ms for a screen on an A53, against about 1 ms here.
pub fn same_pixels(a: &[u32], b: &[u32]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let (ca, cb) = (a.chunks_exact(8), b.chunks_exact(8));
    let (ra, rb) = (ca.remainder(), cb.remainder());
    // 32 bytes at a time with an OR of the differences: no early exit per word, which lets the compiler use vectors
    ca.zip(cb).all(|(x, y)| x.iter().zip(y).fold(0u32, |d, (p, q)| d | (p ^ q)) == 0) && ra == rb
}

/// 304 when the viewer has this picture, else the PNG through a sender thread (503 when all are busy).
fn answer_shot(s: &TcpStream, have: Option<&str>, png: Arc<[u8]>, tag: &str) {
    let etag = if tag.is_empty() { String::new() } else { format!("ETag: {}\r\n", tag) };
    if !tag.is_empty() && have == Some(tag) {
        let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
        let _ = (&*s).write_all(format!("HTTP/1.1 304 Not Modified\r\n{}Cache-Control: no-store\r\nConnection: close\r\n\r\n", etag).as_bytes());
        return;
    }
    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\n{}Cache-Control: no-store\r\nConnection: close\r\n\r\n", png.len(), etag);
    if !send_in_background(s, head, png) {
        busy_answer(s);
    }
}

pub struct Http {
    l: Option<TcpListener>,
    /// connection, request so far, last data, accepted
    pending: Vec<(TcpStream, Vec<u8>, Instant, Instant)>,
    shots: Arc<Shots>,
}

impl Drop for Http {
    fn drop(&mut self) {
        self.shots.lock().quit = true;
        self.shots.wake.notify_all();
    }
}

/// Style of both pages (the update page uses it too).
pub const STYLE: &str = r#"*{box-sizing:border-box}
body{background:#14171c;color:#e4e7ea;font:15px -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;margin:0 auto;max-width:1048px;padding:12px}
header{display:flex;align-items:center;gap:14px;margin-bottom:10px}header b{font-size:17px}header nav{margin-left:auto;display:flex;gap:14px}
a{color:#5cb2f0;text-decoration:none}a:hover{text-decoration:underline}
#st{color:#9aa4af;font-size:13px}#st.bad{color:#f0a93b}
button{background:#242a33;color:#e4e7ea;border:1px solid #3a424d;border-radius:6px;padding:7px 11px;font:inherit;cursor:pointer;touch-action:manipulation}
button:hover{background:#2f3743}button:active{background:#3a4452}button.on{border-color:#f0a93b;color:#f0a93b}
pre{background:#0d0f13;border:1px solid #2a3038;border-radius:6px;padding:12px;overflow-x:auto;white-space:pre-wrap}
code{font:13px ui-monospace,Menlo,Consolas,monospace}"#;

const PAGE: &str = r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>OOS remote</title>
<link rel="stylesheet" href="/style.css">
<style>img{display:block;width:100%;height:auto;border:1px solid #3a424d;border-radius:6px;cursor:crosshair;touch-action:none;user-select:none;-webkit-user-select:none}
.g{display:flex;flex-wrap:wrap;gap:12px 22px;margin-top:12px}.k span{display:block;font-size:12px;color:#9aa4af;margin-bottom:3px}.k div{display:flex;flex-wrap:wrap;gap:4px}</style></head>
<body><header><b>OOS remote</b><span id="st">connecting ...</span><nav><a href="/update">Update</a></nav></header>
<img id="s" width="1024" height="600" alt="scope screen" src="/screen.png">
<div class="g"><div class="k"><span>Acquisition</span><div>
<button onclick="c('key runstop')">Run/Stop</button><button onclick="c('key single')">Single</button><button onclick="c('key auto')">Auto</button><button onclick="c('key clear')">Clear</button><button onclick="c('key default')">Default</button></div></div>
<div class="k"><span>Channels</span><div>
<button onclick="c('key ch1')">CH1</button><button onclick="c('key ch2')">CH2</button><button onclick="c('key ch3')">CH3</button><button onclick="c('key ch4')">CH4</button><button onclick="c('key math')">Math</button><button onclick="c('key ref')">Ref</button><button onclick="c('key la')">LA</button><button onclick="c('key gi')">Gen</button></div></div>
<div class="k"><span>Tools</span><div>
<button onclick="c('key measure')">Measure</button><button onclick="c('key cursor')">Cursor</button><button onclick="c('key analyse')">Decode</button><button onclick="c('key tmenu')">Trigger</button></div></div></div>
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
<div class="k"><span>Picture</span><div><button id="p" onclick="pause()">Pause</button></div></div>
<div class="k"><span>Live picture format</span><div><button id="fp" onclick="setfmt('png')" title="compressed, about 90 KB per picture">PNG</button><button id="fb" onclick="setfmt('bmp')" title="uncompressed, 1.8 MB per picture">BMP</button></div></div></div>
<script>
const s=document.getElementById('s'),st=document.getElementById('st'),pb=document.getElementById('p'),fp=document.getElementById('fp'),fb=document.getElementById('fb');
let paused=false,running=false,n=0,msg='',until=0,fmt='png';
try{if(localStorage.getItem('fmt')=='bmp')fmt='bmp';}catch(e){}
function setfmt(f){fmt=f;try{localStorage.setItem('fmt',f);}catch(e){}fp.className=f=='png'?'on':'';fb.className=f=='bmp'?'on':'';}
setfmt(fmt);
function say(t){msg=t;until=Date.now()+5000;show();}
function show(bad){st.textContent=Date.now()<until?msg:(paused?'picture paused':bad?'no connection':'connected, '+n+' pictures/s');st.className=bad?'bad':'';}
function c(x){fetch('/cmd?c='+encodeURIComponent(x)).catch(()=>say('command not sent'));}
function dl(u,ext){say('saving ...');fetch(u).then(r=>r.ok?r.blob():r.text().then(t=>Promise.reject(t))).then(b=>{
const a=document.createElement('a'),d=new Date(Date.now()-new Date().getTimezoneOffset()*60000).toISOString().slice(0,19).replace(/[:T]/g,'-');
a.href=URL.createObjectURL(b);a.download='oos-'+d+'.'+ext;document.body.appendChild(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(a.href),10000);say('saved');
}).catch(e=>say(typeof e=='string'?e:'not saved: no connection'));}
function pause(){paused=!paused;pb.textContent=paused?'Resume':'Pause';pb.className=paused?'on':'';show();go();}
function pt(e){const r=s.getBoundingClientRect();const p=e.changedTouches?e.changedTouches[0]:e;return [Math.round((p.clientX-r.left)*1024/r.width),Math.round((p.clientY-r.top)*600/r.height)];}
let d0=null;
function down(e){e.preventDefault();d0=pt(e);}
function up(e){if(!d0)return;e.preventDefault();const p=pt(e);if(Math.abs(p[0]-d0[0])+Math.abs(p[1]-d0[1])<8)c('tap '+d0[0]+' '+d0[1]);else c('drag '+d0[0]+' '+d0[1]+' '+p[0]+' '+p[1]);d0=null;}
s.addEventListener('mousedown',down);s.addEventListener('mouseup',up);s.addEventListener('touchstart',down,{passive:false});s.addEventListener('touchend',up,{passive:false});
s.ondragstart=()=>false;
let count=0;setInterval(()=>{n=count;count=0;if(!paused&&!document.hidden)show(st.className=='bad');},1000);
// no pictures while paused or in a background tab: each one is 90 KB as PNG, 1.8 MB as BMP, which the scope has to send
// a PNG comes with an ETag: sent back, the scope holds the request until its screen has changed (304 after 2 s), so
// the next request can go at once; a BMP has none, so a pause between them
let tag='',shown='';
function next(ok){running=false;show(!ok);setTimeout(go,!ok?1000:fmt=='png'?0:150);}
function go(){if(running||paused||document.hidden)return;running=true;const f=fmt;
fetch('/screen.'+f+'?t='+Date.now(),{cache:'no-store',headers:tag&&f=='png'?{'If-None-Match':tag}:{}}).then(r=>{
if(r.status==304)return next(true);
if(!r.ok)return next(false);
const t=f=='png'&&r.headers.get('ETag')||'';
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

impl Http {
    pub fn open(port: u16) -> Http {
        let l = TcpListener::bind(("0.0.0.0", port)).ok();
        match &l {
            Some(l) => {
                let _ = l.set_nonblocking(true);
                eprintln!("oos-ui: web remote on port {}", port);
            }
            None => eprintln!("oos-ui: web port {} not available", port),
        }
        Http { l, pending: Vec::new(), shots: Shots::new() }
    }

    /// Call right before rendering the next screen: hands the shown one to the live picture (`Shots::hand_over`).
    /// `whole`: the render draws every pixel.
    pub fn before_render(&mut self, cv: &mut Canvas, whole: bool) {
        self.shots.hand_over(cv, whole);
    }

    /// Accepts connections, answers complete requests; text commands are returned for the caller to run.
    pub fn poll(&mut self, ui: &mut Ui, cv: &Canvas, scpi: &mut crate::scpi::Scpi) -> Vec<String> {
        let mut cmds = Vec::new();
        self.shots.tick(cv);
        if let Some(l) = &self.l {
            while let Ok((s, _)) = l.accept() {
                if self.pending.len() >= MAX_PENDING {
                    continue; // dropped: closes the connection
                }
                let _ = s.set_nonblocking(false);
                let now = Instant::now();
                self.pending.push((s, Vec::new(), now, now));
            }
        }
        let mut i = 0;
        while i < self.pending.len() {
            let mut buf = [0u8; 65536];
            let (s, req, t, t_acc) = &mut self.pending[i];
            let mut dead = false;
            // up to 2 MB per connection and frame (an update package is ~31 MB: a few seconds at 100 Mbit/s)
            let mut got = 0;
            while got < 2 << 20 {
                match crate::net::read_now(s, &mut buf) {
                    Ok(0) => {
                        dead = true;
                        break;
                    }
                    Ok(n) => {
                        req.extend_from_slice(&buf[..n]);
                        got += n;
                        *t = Instant::now();
                    }
                    Err(e) if crate::net::later(&e) => break,
                    Err(_) => {
                        dead = true;
                        break;
                    }
                }
            }
            // headers complete, and the body when there is a Content-Length
            let hdr_end = req.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4);
            if hdr_end.is_none() && req.len() > 16384 {
                dead = true; // no header end in 16 KiB
            }
            let mut too_big = false;
            let complete = hdr_end.is_some_and(|e| {
                let h = String::from_utf8_lossy(&req[..e]).to_ascii_lowercase();
                let n = h.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                let limit = if h.starts_with("post /update") { MAX_UPDATE } else { MAX_OTHER };
                too_big = n > limit || req.len() > limit + e;
                too_big || e.checked_add(n).is_some_and(|need| req.len() >= need)
            });
            if too_big {
                let msg = b"request too large";
                let head = format!("HTTP/1.1 413 Payload Too Large\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", msg.len());
                let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(msg);
                dead = true;
            } else if complete {
                // only the header as text: the body may be a 31 MB binary package
                let text = String::from_utf8_lossy(&req[..hdr_end.unwrap_or(req.len())]).into_owned();
                let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (route, query) = path.split_once('?').unwrap_or((&path, ""));
                let req_body = &req[hdr_end.unwrap_or(req.len())..];
                let post = text.starts_with("POST ");
                // an answer that is text for the page to show, not the data asked for
                let mut failed = false;
                let mut csv = |r: Result<String, String>| match r {
                    Ok(t) => ("text/csv; charset=utf-8", t.into_bytes()),
                    Err(e) => {
                        failed = true;
                        ("text/plain", e.into_bytes())
                    }
                };
                let (ctype, body): (&str, Vec<u8>) = match route {
                    _ if foreign(&text, route, post) => ("text/plain", b"refused".to_vec()),
                    "/" => ("text/html; charset=utf-8", PAGE.as_bytes().to_vec()),
                    "/style.css" => ("text/css", STYLE.as_bytes().to_vec()),
                    "/screen.png" => {
                        let have = header(&text, "if-none-match").map(str::to_string);
                        self.shots.request(s, have, cv);
                        self.pending.remove(i);
                        continue;
                    }
                    "/wave.csv" => csv(ui.web_wave_csv()),
                    "/la.csv" => csv(ui.web_la_csv()),
                    "/wave.bin" => match ui.web_wave_bin() {
                        Ok(b) => ("application/octet-stream", b),
                        Err(e) => {
                            failed = true;
                            ("text/plain", e.into_bytes())
                        }
                    },
                    "/update" if post => match ui.update_upload(req_body) {
                        Ok(m) => ("text/plain", m.into_bytes()),
                        Err(e) => ("text/plain", format!("REFUSED: {}", e).into_bytes()),
                    },
                    "/update" => ("text/html; charset=utf-8", ui.update_page().into_bytes()),
                    "/lxi/identification" => ("text/xml", lxi_identification().into_bytes()),
                    "/mcp" => match mcp(&String::from_utf8_lossy(req_body), ui, cv, scpi) {
                        Some(r) => ("application/json", r.into_bytes()),
                        None => ("", Vec::new()),
                    },
                    "/screen.bmp" => ("image/bmp", crate::scpi::bmp(cv)),
                    "/cmd" => {
                        let mut refused = false;
                        for kv in query.split('&') {
                            if let Some(v) = kv.strip_prefix("c=") {
                                let c = unescape(v);
                                if crate::cmd::remote_ok(&c) {
                                    cmds.push(c);
                                } else {
                                    refused = true;
                                }
                            }
                        }
                        ("text/plain", if refused { b"refused".to_vec() } else { b"ok".to_vec() })
                    }
                    _ => ("text/plain", b"not found".to_vec()),
                };
                let status = if failed { "409 Conflict" } else if body == b"not found" { "404 Not Found" } else if body == b"refused" { "403 Forbidden" } else if ctype.is_empty() { "202 Accepted" } else { "200 OK" };
                let head = format!("HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", status, ctype, body.len());
                if body.len() > SEND_INLINE {
                    if !send_in_background(s, head, body) {
                        busy_answer(s);
                    }
                } else {
                    let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
                    let _ = s.write_all(head.as_bytes());
                    let _ = s.write_all(&body);
                }
                dead = true;
            } else if t.elapsed().as_secs() > IDLE_S || t_acc.elapsed().as_secs() > DEADLINE_S {
                dead = true;
            }
            if dead {
                self.pending.remove(i);
            } else {
                i += 1;
            }
        }
        cmds
    }
}

/// The value of a header of the request head (name without case).
fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

fn lxi_identification() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<LXIDevice xmlns="http://www.lxistandard.org/InstrumentIdentification/1.0">
<Manufacturer>OOS</Manufacturer><Model>DHO924S</Model><SerialNumber>0</SerialNumber>
<FirmwareRevision>oos-ui {}</FirmwareRevision><ManufacturerDescription>OOS oscilloscope</ManufacturerDescription>
<HomepageURL>/</HomepageURL><DriverURLs/>
<UserDescription>OOS</UserDescription>
<IdentificationURL>/lxi/identification</IdentificationURL>
<Interface InterfaceType="LXI" IPType="IPv4" xsi:type="NetworkInformation" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
<InstrumentAddressString>TCPIP::INSTR</InstrumentAddressString>
<InstrumentAddressString>TCPIP::5555::SOCKET</InstrumentAddressString>
</Interface>
<LXIVersion>1.5</LXIVersion>
</LXIDevice>
"#,
        env!("CARGO_PKG_VERSION")
    )
}

pub fn base64(d: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::with_capacity(d.len().div_ceil(3) * 4);
    for c in d.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            o.push(if k <= c.len() { T[(n >> (18 - 6 * k) & 63) as usize] as char } else { '=' });
        }
    }
    o
}

/// One MCP JSON-RPC message; None for notifications (answered with 202 and no body).
pub fn mcp(body: &str, ui: &mut Ui, cv: &Canvas, scpi: &mut crate::scpi::Scpi) -> Option<String> {
    use crate::json::{parse, J};
    let err = |id: J, code: f64, msg: &str| J::obj(&[("jsonrpc", J::s("2.0")), ("id", id), ("error", J::obj(&[("code", J::Num(code)), ("message", J::s(msg))]))]).to_string();
    let Some(req) = parse(body) else { return Some(err(J::Null, -32700.0, "parse error")) };
    let id = req.get("id").cloned()?;
    let method = req.get("method").and_then(J::str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(J::Null);
    let text = |s: &str| J::obj(&[("content", J::Arr(vec![J::obj(&[("type", J::s("text")), ("text", J::s(s))])]))]);
    let result = match method {
        "initialize" => {
            let ver = params.get("protocolVersion").and_then(J::str).unwrap_or("2025-06-18").to_string();
            J::obj(&[
                ("protocolVersion", J::Str(ver)),
                ("capabilities", J::obj(&[("tools", J::obj(&[]))])),
                ("serverInfo", J::obj(&[("name", J::s("oos-ui")), ("version", J::s(env!("CARGO_PKG_VERSION")))])),
                ("instructions", J::s("Rigol DHO924S running OOS. Use the scpi tool with Rigol-style commands (e.g. :TIM:SCAL 1e-3, :CHAN1:SCAL 0.5, :MEAS:ITEM? VPP,CHAN1, :WAV:DATA?) and screenshot to see the screen.")),
            ])
        }
        "ping" => J::obj(&[]),
        "tools/list" => {
            let schema = |props: J, req: Vec<J>| J::obj(&[("type", J::s("object")), ("properties", props), ("required", J::Arr(req))]);
            J::obj(&[(
                "tools",
                J::Arr(vec![
                    J::obj(&[
                        ("name", J::s("scpi")),
                        ("description", J::s("Run one SCPI program message on the oscilloscope (commands separated by ';'); returns the response of the queries.")),
                        ("inputSchema", schema(J::obj(&[("command", J::obj(&[("type", J::s("string"))]))]), vec![J::s("command")])),
                    ]),
                    J::obj(&[
                        ("name", J::s("screenshot")),
                        ("description", J::s("The current oscilloscope screen as a PNG image.")),
                        ("inputSchema", schema(J::obj(&[]), vec![])),
                    ]),
                ]),
            )])
        }
        "tools/call" => match params.get("name").and_then(J::str) {
            Some("scpi") => {
                let c = params.get("arguments").and_then(|a| a.get("command")).and_then(J::str).unwrap_or("");
                let r = scpi.exec_line(ui, cv, c);
                text(if r.is_empty() { "ok" } else { std::str::from_utf8(&r).unwrap_or("(binary response)") })
            }
            Some("screenshot") => J::obj(&[("content", J::Arr(vec![J::obj(&[("type", J::s("image")), ("mimeType", J::s("image/png")), ("data", J::Str(base64(&crate::store::png_of(cv))))])]))]),
            _ => return Some(err(id, -32602.0, "unknown tool")),
        },
        _ => return Some(err(id, -32601.0, "method not found")),
    };
    Some(J::obj(&[("jsonrpc", J::s("2.0")), ("id", id), ("result", result)]).to_string())
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
    let name_of = |h: &str| h.rsplit_once(':').filter(|(n, p)| !n.ends_with(':') && p.bytes().all(|b| b.is_ascii_digit())).map_or(h.to_string(), |(n, _)| n.to_string());
    let host = header("host");
    if let Some(h) = &host {
        let n = name_of(h);
        let n = n.trim_matches(['[', ']']);
        let local = n.parse::<std::net::IpAddr>().is_ok() || n == "localhost" || !n.contains('.') || n.ends_with(".local");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn a_foreign_site_gets_nowhere() {
        let f = |h: &str, route: &str, post: bool| foreign(&format!("GET {} HTTP/1.1\r\n{}\r\n\r\n", route, h), route, post);
        // tools and the page of the scope itself
        assert!(!f("Host: 192.168.177.64:8080", "/cmd", false));
        assert!(!f("Host: x", "/cmd", false));
        assert!(!f("Host: dho924.local:8080", "/cmd", false));
        assert!(!f("Host: [fe80::1]:8080", "/cmd", false));
        assert!(!f("", "/cmd", false));
        assert!(!f("Host: 192.168.177.64:8080\r\nOrigin: http://192.168.177.64:8080\r\nSec-Fetch-Site: same-origin", "/update", true));
        // a page elsewhere
        assert!(f("Host: 192.168.177.64:8080\r\nOrigin: https://evil.example", "/update", true));
        assert!(f("Host: 192.168.177.64:8080\r\nOrigin: null", "/mcp", true));
        assert!(f("Host: 192.168.177.64:8080\r\nSec-Fetch-Site: cross-site", "/cmd", false));
        assert!(f("Host: 192.168.177.64:8080\r\nSec-Fetch-Site: cross-site", "/screen.png", false));
        // rebinding: a foreign name that resolves to the scope
        assert!(f("Host: evil.example:8080", "/wave.csv", false));
        // a link to the page
        assert!(!f("Host: 192.168.177.64:8080\r\nSec-Fetch-Site: cross-site", "/", false));
        assert!(f("Host: 192.168.177.64:8080\r\nSec-Fetch-Site: cross-site", "/update", true));
    }

    /// One request over a real socket; the server is polled until the answer is complete.
    fn request(h: &mut Http, ui: &mut Ui, cv: &Canvas, scpi: &mut crate::scpi::Scpi, req: Vec<u8>) -> String {
        String::from_utf8_lossy(&request_raw(h, ui, cv, scpi, req)).into_owned()
    }

    fn request_raw(h: &mut Http, ui: &mut Ui, cv: &Canvas, scpi: &mut crate::scpi::Scpi, req: Vec<u8>) -> Vec<u8> {
        let addr = h.l.as_ref().unwrap().local_addr().unwrap();
        let t = std::thread::spawn(move || {
            let mut c = TcpStream::connect(("127.0.0.1", addr.port())).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            let _ = c.write_all(&req);
            let mut out = Vec::new();
            let _ = c.read_to_end(&mut out);
            out
        });
        while !t.is_finished() {
            h.poll(ui, cv, scpi);
            std::thread::sleep(Duration::from_millis(1));
        }
        t.join().unwrap()
    }

    /// The web remote with the demo signals, to try it without a scope: `cargo test web_demo -- --ignored --nocapture`,
    /// then open http://localhost:8080/. Runs for OOS_WEB_DEMO_S seconds (default 300), on OOS_WEB_DEMO_PORT.
    #[test]
    #[ignore]
    fn web_demo() {
        let var = |k: &str, d: u64| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        let mut h = Http::open(var("OOS_WEB_DEMO_PORT", 8080) as u16);
        let mut ui = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        let mut cv = Canvas::new(crate::ui::W, crate::ui::H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let end = Instant::now() + Duration::from_secs(var("OOS_WEB_DEMO_S", 300));
        while Instant::now() < end {
            ui.step(0.04);
            ui.render(&mut cv);
            for c in h.poll(&mut ui, &cv, &mut scpi) {
                println!("web: {}", c);
                crate::cmd::run(&mut ui, &c);
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    }

    fn get(h: &mut Http, ui: &mut Ui, cv: &Canvas, scpi: &mut crate::scpi::Scpi, path: &str) -> String {
        request(h, ui, cv, scpi, format!("GET {} HTTP/1.1\r\nHost: x\r\n\r\n", path).into_bytes())
    }

    /// Runs the page's script under Node with a small stand-in for the browser (skipped without `node`) and returns the
    /// picture addresses it asked for: first before and then after `setfmt('bmp')`, and what it kept in localStorage.
    fn run_page_script(script: &str, preset: &str, storage_throws: bool) -> Option<String> {
        if !std::process::Command::new("node").arg("--version").output().map_or(false, |o| o.status.success()) {
            eprintln!("node not found: page script check skipped");
            return None;
        }
        const HARNESS: &str = r#"
const fs = require('fs'), vm = require('vm');
const log = [], inm = [], store = {};
if (process.env.PRESET) store.fmt = process.env.PRESET;
const els = {};
const el = id => els[id] || (els[id] = { id, textContent: '', className: '', addEventListener() {}, getBoundingClientRect: () => ({ left: 0, top: 0, width: 1024, height: 600 }) });
global.localStorage = process.env.THROWS
  ? { getItem() { throw new Error('blocked'); }, setItem() { throw new Error('blocked'); } }
  : { getItem: k => (k in store ? store[k] : null), setItem: (k, v) => { store[k] = v; } };
global.document = { getElementById: el, hidden: false, addEventListener() {}, createElement: () => ({}), body: { appendChild() {} } };
global.Image = class { set src(v) { this._s = v; setTimeout(() => this.onload && this.onload(), 0); } get src() { return this._s; } };
global.URL = { createObjectURL: () => 'blob:x', revokeObjectURL() {} };
// the scope's side: a PNG has the ETag "t", which If-None-Match then gets 304 for
global.fetch = (u, o) => {
  const have = (o && o.headers && o.headers['If-None-Match']) || '';
  log.push(u); inm.push(have);
  const png = u.startsWith('/screen.png'), status = png && have == '"t"' ? 304 : 200;
  return Promise.resolve({ ok: status == 200, status, headers: { get: k => (png && k == 'ETag' ? '"t"' : null) }, blob: () => Promise.resolve({}) });
};
global.setInterval = () => 0;
vm.runInThisContext(fs.readFileSync(process.argv[2], 'utf8'));
setTimeout(() => {
  const first = log.slice(), inm0 = inm.slice(), cls0 = [el('fp').className, el('fb').className];
  let err = '';
  try { setfmt('bmp'); } catch (e) { err = String(e); }
  const mark = log.length;
  setTimeout(() => {
    console.log(JSON.stringify({ first, inm0, cls0, cls1: [el('fp').className, el('fb').className], after: log.slice(mark), inm1: inm.slice(mark), store, err }));
    process.exit(0);
  }, 500);
}, 400);
"#;
        let dir = std::env::temp_dir().join(format!("oos-ui-page-{}-{}", std::process::id(), preset.len() + storage_throws as usize));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("page.js"), script).unwrap();
        std::fs::write(dir.join("run.js"), HARNESS).unwrap();
        let mut cmd = std::process::Command::new("node");
        cmd.arg(dir.join("run.js")).arg(dir.join("page.js")).env("PRESET", preset);
        if storage_throws {
            cmd.env("THROWS", "1");
        }
        let out = cmd.output().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.status.success(), "node: {}", String::from_utf8_lossy(&out.stderr));
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    #[test]
    fn page_lets_the_viewer_pick_png_or_bmp() {
        let mut h = Http::open(0);
        let mut ui = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        let cv = Canvas::new(crate::ui::W, crate::ui::H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let page = get(&mut h, &mut ui, &cv, &mut scpi, "/");
        let script = page.split("<script>").nth(1).and_then(|s| s.split("</script>").next()).expect("script in the page");
        // the picture the browser loads before the script runs is the small one, too
        assert!(page.contains(r#"id="s" width="1024" height="600" alt="scope screen" src="/screen.png""#), "first picture");
        assert!(page.contains("setfmt('png')") && page.contains("setfmt('bmp')"), "buttons for both formats");
        let Some(r) = run_page_script(script, "", false) else { return };
        // default: PNG, then BMP after the switch, and the choice is kept
        assert!(r.contains(r#""first":["/screen.png?t="#), "{}", r);
        assert!(!r.contains(r#""first":["/screen.bmp"#), "{}", r);
        // the PNG's ETag goes back with the next request, the 304 keeps the loop going; never with a BMP
        assert!(r.contains(r#""inm0":["","\"t\"","#) || r.contains(r#""inm0":["","\"t\""]"#), "{}", r);
        let inm1 = r.split(r#""inm1":["#).nth(1).unwrap().split(']').next().unwrap();
        assert!(inm1.split(',').all(|v| v == r#""""#), "{}", r);
        let after: Vec<&str> = r.split(r#""after":["#).nth(1).unwrap().split(']').next().unwrap().split(',').collect();
        assert!(!after.is_empty() && after.iter().all(|a| a.contains("/screen.bmp?t=")), "{}", r);
        assert!(r.contains(r#""store":{"fmt":"bmp"}"#), "{}", r);
        // the button of the format in use is lit
        assert!(r.contains(r#""cls0":["on",""],"cls1":["","on"]"#), "{}", r);
        // a kept choice comes back at the next visit
        let r = run_page_script(script, "bmp", false).unwrap();
        assert!(r.contains(r#""first":["/screen.bmp?t="#), "{}", r);
        // storage blocked (private window): still PNG, and switching does not fail
        let r = run_page_script(script, "", true).unwrap();
        assert!(r.contains(r#""first":["/screen.png?t="#) && r.contains(r#""err":"""#), "{}", r);
        assert!(r.split(r#""after":["#).nth(1).unwrap().contains("/screen.bmp?t="), "{}", r);
    }

    #[test]
    fn pages_and_downloads() {
        let mut h = Http::open(0);
        let mut ui = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        let cv = Canvas::new(crate::ui::W, crate::ui::H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let r = get(&mut h, &mut ui, &cv, &mut scpi, "/");
        assert!(r.starts_with("HTTP/1.1 200") && r.contains("/screen.png") && r.contains("/update"), "{}", &r[..200]);
        assert!(get(&mut h, &mut ui, &cv, &mut scpi, "/style.css").contains("text/css"));
        // no record yet: the page gets the reason, not an empty file
        let r = get(&mut h, &mut ui, &cv, &mut scpi, "/wave.csv");
        assert!(r.starts_with("HTTP/1.1 409") && r.contains("No waveform"), "{}", r);
        let r = get(&mut h, &mut ui, &cv, &mut scpi, "/la.csv");
        assert!(r.starts_with("HTTP/1.1 409"), "{}", r);
        for _ in 0..30 {
            ui.step(0.02);
        }
        let r = get(&mut h, &mut ui, &cv, &mut scpi, "/wave.csv");
        assert!(r.starts_with("HTTP/1.1 200") && r.contains("text/csv") && r.contains("Time (s),CH1 (V)"), "{}", &r[..300.min(r.len())]);
    }

    /// Large answers leave the frame loop: the whole body arrives, also several at once, and the count goes back.
    #[test]
    fn large_answers_go_through_a_thread() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Http::open(0);
        let mut ui = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        // a real screen: a blank canvas would be a few KB as a PNG, which is not a large answer
        let mut cv = Canvas::new(crate::ui::W, crate::ui::H);
        ui.render(&mut cv);
        let mut scpi = crate::scpi::Scpi::open(0);
        for path in ["/screen.bmp", "/screen.png"] {
            let addr = h.l.as_ref().unwrap().local_addr().unwrap();
            let t = std::thread::spawn(move || {
                let mut c = TcpStream::connect(("127.0.0.1", addr.port())).unwrap();
                c.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
                c.write_all(format!("GET {} HTTP/1.1\r\nHost: x\r\n\r\n", path).as_bytes()).unwrap();
                let mut out = Vec::new();
                let _ = c.read_to_end(&mut out);
                out
            });
            while !t.is_finished() {
                h.poll(&mut ui, &cv, &mut scpi);
                std::thread::sleep(Duration::from_millis(1));
            }
            let out = t.join().unwrap();
            let e = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
            let head = String::from_utf8_lossy(&out[..e]).to_string();
            let n: usize = head.lines().find_map(|l| l.strip_prefix("Content-Length: ")).unwrap().trim().parse().unwrap();
            assert!(n > SEND_INLINE, "{}", path);
            assert_eq!(out.len() - e, n, "{}", path);
            if path.ends_with("png") {
                assert_eq!(&out[e..e + 4], b"\x89PNG");
            }
        }
        for _ in 0..200 {
            if SENDERS.load(SeqCst) == 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("a sender did not finish");
    }

    /// With a frame loop that hands its screens over before each render, the picture is the screen that was shown,
    /// taken without a copy, and the loop's canvas gets a buffer of the right size to draw into.
    #[test]
    fn live_picture_from_the_frame_loop_without_a_copy() {
        let mut h = Http::open(0);
        let mut ui = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        let mut cv = Canvas::new(crate::ui::W, crate::ui::H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let port = h.l.as_ref().unwrap().local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            let _ = c.write_all(b"GET /screen.png HTTP/1.1\r\nHost: x\r\n\r\n");
            let mut out = Vec::new();
            let _ = c.read_to_end(&mut out);
            out
        });
        let mut shown = None;
        let mut frame = 0u32;
        while !t.is_finished() {
            h.poll(&mut ui, &cv, &mut scpi);
            // a frame: hand over, then render a screen of its own (each frame another colour)
            let before = cv.px.clone();
            h.before_render(&mut cv, true);
            if cv.px != before && shown.is_none() {
                shown = Some(before);
            }
            assert_eq!(cv.px.len(), (cv.w * cv.h) as usize);
            frame += 1;
            cv.clear(crate::gfx::rgb(frame % 256, 40, 90));
            std::thread::sleep(Duration::from_millis(5));
        }
        let out = t.join().unwrap();
        let e = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        assert!(out.starts_with(b"HTTP/1.1 200"), "{}", String::from_utf8_lossy(&out[..e]));
        let shown = shown.expect("a screen was handed over");
        assert!(out[e..] == crate::store::png(cv.w as u32, cv.h as u32, &shown), "the PNG of the screen that was shown");
    }

    /// The live picture: one encode per screen; a viewer with the newest picture waits (long poll) and gets 304 when
    /// nothing changes, or the new picture as soon as the screen changes; a viewer without it gets it at once.
    #[test]
    fn live_picture_once_per_screen_and_long_poll() {
        // the comparison of two screens, at every length and a difference at every place
        for n in [0usize, 1, 7, 8, 9, 63, 64, 65] {
            let a: Vec<u32> = (0..n as u32).map(|i| i.wrapping_mul(2654435761)).collect();
            assert!(same_pixels(&a, &a.clone()));
            for k in 0..n {
                let mut b = a.clone();
                b[k] ^= 1 << (k % 32);
                assert!(!same_pixels(&a, &b), "{} {}", n, k);
            }
            assert!(!same_pixels(&a, &a[..n.saturating_sub(1)]) || n == 0);
        }
        let mut h = Http::open(0);
        let mut ui = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        let mut cv = Canvas::new(crate::ui::W, crate::ui::H);
        ui.render(&mut cv);
        let mut scpi = crate::scpi::Scpi::open(0);
        let port = h.l.as_ref().unwrap().local_addr().unwrap().port();
        // a viewer in a thread; the server is polled meanwhile, and `change` alters the screen after 100 ms
        let shot = |h: &mut Http, ui: &mut Ui, scpi: &mut crate::scpi::Scpi, cv: &mut Canvas, have: Option<&str>, change: bool| {
            let inm = have.map_or(String::new(), |t| format!("If-None-Match: {}\r\n", t));
            let req = format!("GET /screen.png?t=1 HTTP/1.1\r\nHost: x\r\n{}\r\n", inm).into_bytes();
            let t0 = Instant::now();
            let t = std::thread::spawn(move || {
                let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
                c.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
                let _ = c.write_all(&req);
                let mut out = Vec::new();
                let _ = c.read_to_end(&mut out);
                out
            });
            let mut changed = false;
            while !t.is_finished() {
                if change && !changed && t0.elapsed() > Duration::from_millis(100) {
                    cv.fill(crate::gfx::Rect::new(10, 60, 300, 40), crate::gfx::rgb(30, 250, 30));
                    changed = true;
                }
                h.poll(ui, cv, scpi);
                std::thread::sleep(Duration::from_millis(1));
            }
            let out = t.join().unwrap();
            let e = out.windows(4).position(|w| w == b"\r\n\r\n").expect("a head") + 4;
            let head = String::from_utf8_lossy(&out[..e]).to_string();
            let tag = head.lines().find_map(|l| l.strip_prefix("ETag: ")).unwrap_or("").to_string();
            (head, tag, out[e..].to_vec(), t0.elapsed())
        };
        let encodes = |h: &Http| h.shots.lock().encodes;
        let png_of = |cv: &Canvas| crate::store::png(cv.w as u32, cv.h as u32, &cv.px);
        let (head, tag1, body, _) = shot(&mut h, &mut ui, &mut scpi, &mut cv, None, false);
        assert!(head.starts_with("HTTP/1.1 200") && head.contains("Content-Type: image/png"), "{}", head);
        assert!(tag1.starts_with('"') && tag1.ends_with('"') && tag1.len() > 2, "{}", head);
        assert!(body == png_of(&cv), "the PNG of the screen");
        // the screen unchanged, the viewer has it: it waits the long poll out, then 304 without a body, no encode
        let (head, tag, body, took) = shot(&mut h, &mut ui, &mut scpi, &mut cv, Some(&tag1), false);
        assert!(head.starts_with("HTTP/1.1 304") && body.is_empty() && tag == tag1, "{}", head);
        assert!(took >= LONG_POLL, "answered after {:?}", took);
        // a viewer without it gets the same picture, without a new encode
        let (head, tag, body, _) = shot(&mut h, &mut ui, &mut scpi, &mut cv, None, false);
        assert!(head.starts_with("HTTP/1.1 200") && tag == tag1 && body == png_of(&cv), "{}", head);
        assert_eq!(encodes(&h), 1);
        // the screen changes while the viewer waits: the new picture comes before the long poll ends
        let (head, tag2, body, took) = shot(&mut h, &mut ui, &mut scpi, &mut cv, Some(&tag1), true);
        assert!(head.starts_with("HTTP/1.1 200") && tag2 != tag1 && body == png_of(&cv), "{}", head);
        assert!(took < LONG_POLL, "answered after {:?}", took);
        assert_eq!(encodes(&h), 2);
        // a foreign page still gets nowhere
        let r = request(&mut h, &mut ui, &cv, &mut scpi, b"GET /screen.png HTTP/1.1\r\nHost: x\r\nSec-Fetch-Site: cross-site\r\n\r\n".to_vec());
        assert!(r.starts_with("HTTP/1.1 403"), "{}", r);
    }

    #[test]
    fn update_upload_limits() {
        let mut h = Http::open(0);
        let mut ui = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        let cv = Canvas::new(crate::ui::W, crate::ui::H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let post = |path: &str, body: &[u8]| {
            let mut r = format!("POST {} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n", path, body.len()).into_bytes();
            r.extend_from_slice(body);
            r
        };
        // a 5 MB body is taken on /update (and refused for its content, nothing installed) ...
        let big = vec![0xffu8; 5 << 20];
        let r = request(&mut h, &mut ui, &cv, &mut scpi, post("/update", &big));
        assert!(r.starts_with("HTTP/1.1 200") && r.contains("REFUSED: not an OOS update package"), "{}", &r[..r.len().min(300)]);
        // ... but not elsewhere
        let r = request(&mut h, &mut ui, &cv, &mut scpi, post("/mcp", &big));
        assert!(r.starts_with("HTTP/1.1 413"), "{}", &r[..r.len().min(200)]);
        // an announced length beyond the limit is answered at once, without waiting for the body
        let r = request(&mut h, &mut ui, &cv, &mut scpi, b"POST /update HTTP/1.1\r\nContent-Length: 999999999999\r\n\r\n".to_vec());
        assert!(r.starts_with("HTTP/1.1 413"), "{}", r);
        let r = request(&mut h, &mut ui, &cv, &mut scpi, b"POST /update HTTP/1.1\r\nContent-Length: 18446744073709551615\r\n\r\n".to_vec());
        assert!(r.starts_with("HTTP/1.1 413"), "{}", r);
        // ordinary requests still work
        let r = request(&mut h, &mut ui, &cv, &mut scpi, b"GET /lxi/identification HTTP/1.1\r\n\r\n".to_vec());
        assert!(r.starts_with("HTTP/1.1 200") && r.contains("<Manufacturer>OOS"), "{}", r);
    }

    #[test]
    fn cmd_takes_keys_knobs_and_touch_only() {
        let mut h = Http::open(0);
        let mut ui = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        let cv = Canvas::new(crate::ui::W, crate::ui::H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let get = |c: &str| format!("GET /cmd?c={} HTTP/1.1\r\n\r\n", c).into_bytes();
        for c in ["shot%20/dev/mmcblk1", "storedir%20/", "stat%20/etc/passwd", "quit", "frames%2010", "selfcal%20x", ""] {
            let r = request(&mut h, &mut ui, &cv, &mut scpi, get(c));
            assert!(r.starts_with("HTTP/1.1 403"), "{}: {}", c, r);
        }
        let r = request(&mut h, &mut ui, &cv, &mut scpi, get("key%20runstop"));
        assert!(r.starts_with("HTTP/1.1 200"), "{}", r);
        for c in ["key runstop", "knob vscale +1", "tap 100 200", "drag 1 2 3 4"] {
            assert!(crate::cmd::remote_ok(c), "{}", c);
        }
    }

    #[test]
    fn unescape_survives_any_bytes() {
        assert_eq!(unescape("key%20runstop+x"), "key runstop x");
        // two-byte characters right after the %: the slice used to fall inside one
        assert_eq!(unescape("a%é1"), "a1");
        assert_eq!(unescape("%€"), String::from_utf8_lossy(&[0xac]));
        assert_eq!(unescape("x%4"), "x%4");
    }

    #[test]
    fn mcp_session() {
        use crate::json::{parse, J};
        let mut ui = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        let cv = Canvas::new(crate::ui::W, crate::ui::H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let mut call = |b: &str| mcp(b, &mut ui, &cv, &mut scpi).map(|r| parse(&r).unwrap());
        let r = call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#).unwrap();
        assert_eq!(r.get("result").and_then(|x| x.get("protocolVersion")).and_then(J::str), Some("2025-06-18"));
        assert!(call(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        let r = call(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
        assert!(r.to_string().contains("\"screenshot\""));
        let r = call(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"scpi","arguments":{"command":":TIM:SCAL 0.001;*IDN?"}}}"#).unwrap();
        assert!(r.to_string().contains("OOS,DHO924S"), "{}", r.to_string());
        let r = call(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"screenshot","arguments":{}}}"#).unwrap();
        assert!(r.to_string().contains("iVBORw0KGgo")); // PNG signature in base64
        assert!(call(r#"{"jsonrpc":"2.0","id":5,"method":"nope"}"#).unwrap().get("error").is_some());
        drop(call);
        assert!((ui.m.time_per_div() - 1e-3).abs() < 1e-9);
    }

    #[test]
    fn scpi_extensions() {
        let mut ui = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        let cv = Canvas::new(crate::ui::W, crate::ui::H);
        let mut scpi = crate::scpi::Scpi::open(0);
        for _ in 0..3 {
            ui.step(0.05);
        }
        let mut q = |c: &str| String::from_utf8(scpi.exec_line(&mut ui, &cv, c)).unwrap();
        q(":VIEW SPEC");
        assert_eq!(q(":VIEW?"), "SPECTRUM");
        assert!(q(":IDEN?").starts_with("CH1 "), "{}", q(":IDEN?"));
        assert_eq!(q(":CAL:STAT?"), "needs the hardware source");
        q(":BODE:GEN \"10.0.0.9:5025\"");
        assert_eq!(q(":BODE:GEN?"), "10.0.0.9:5025");
        assert_eq!(q(":BODE:DATA?"), "");
        // built-in generator, Rigol forms
        q(":SOUR:FREQ 12345.6");
        q(":SOURce1:VOLTage:LEVel:IMMediate:AMPLitude 0.5");
        q(":SOUR:VOLT:OFFS 0.25");
        q(":SOUR:OUTP:STAT ON");
        assert_eq!((q(":SOUR:FREQ?"), q(":SOUR:VOLT?"), q(":SOUR:VOLT:OFFS?"), q(":SOUR:OUTP?"), q(":SOUR:FUNC?")), ("1.234560E+04".into(), "5.000000E-01".into(), "2.500000E-01".into(), "1".into(), "SIN".into()));
        // other wave shapes: a ramp caps the frequency at 5 MHz; square duty, ramp symmetry
        q(":SOUR:FREQ 1e7");
        q(":SOUR:FUNC RAMP");
        q(":SOUR:FUNC:RAMP:SYMM 20");
        assert_eq!((q(":SOUR:FUNC?"), q(":SOUR:FREQ?"), q(":SOUR:FUNC:RAMP:SYMM?")), ("RAMP".into(), "5.000000E+06".into(), "2.000000E+01".into()));
        q(":SOUR:FUNC SQU");
        q(":SOUR:FUNC:SQU:DUTY 25");
        assert_eq!((q(":SOUR:FUNC?"), q(":SOUR:FUNC:SQU:DUTY?")), ("SQU".into(), "2.500000E+01".into()));
        q(":SOUR:FUNC SIN");
        // modulation
        q(":SOUR:FREQ 1e6");
        q(":SOUR:MOD:TYP FM");
        q(":SOUR:MOD:FM:DEV 2e6");
        q(":SOUR:MOD:FM:INT:FREQ 1000");
        q(":SOUR:MOD:FM:INT:FUNC SQU");
        // the deviation stays below the carrier
        assert_eq!((q(":SOUR:MOD:TYP?"), q(":SOUR:MOD:FM:DEV?"), q(":SOUR:MOD:FM:INT:FREQ?"), q(":SOUR:MOD:FM:INT:FUNC?"), q(":SOUR:MOD?")), ("FM".into(), "1.000000E+06".into(), "1.000000E+03".into(), "SQU".into(), "1".into()));
        q(":SOUR:MOD OFF");
        assert_eq!(q(":SOUR:MOD:TYP?"), "NONE");
        q(":SOUR:FREQ 1e9");
        assert_eq!(q(":SOUR:FREQ?"), "6.000000E+07");
    }

    #[test]
    fn scpi_refuses_what_is_not_a_number() {
        let mut ui = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        let mut cv = Canvas::new(crate::ui::W, crate::ui::H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let (delay, offset, probe) = (ui.m.delay, ui.m.ch[0].offset, ui.m.ch[0].probe);
        for c in [":TIM:OFFS nan", ":TIM:OFFS inf", ":TIM:OFFS 1e39", ":CHAN1:OFFS NaN", ":CHAN1:PROB -inf", ":CHAN1:PROB 0", ":CHAN1:PROB -10", ":TRIG:EDGE:LEV nan", ":SOUR:FREQ nan", ":SOUR:VOLT inf"] {
            scpi.exec_line(&mut ui, &cv, c);
        }
        assert_eq!((ui.m.delay, ui.m.ch[0].offset, ui.m.ch[0].probe), (delay, offset, probe));
        // large but finite values run through the screen and a saved setup
        scpi.exec_line(&mut ui, &cv, ":TIM:OFFS 3e38");
        scpi.exec_line(&mut ui, &cv, ":CHAN1:OFFS -3e38");
        ui.key(crate::ui::key::CLEAR);
        ui.mark_changed();
        for _ in 0..3 {
            ui.step(0.05);
        }
        ui.render(&mut cv);
        assert!(crate::store::parse_setup(&crate::store::setup_text(&ui.m)).is_ok());
    }

    #[test]
    fn empty_bode_address_keeps_the_setup_readable() {
        let mut ui = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        let cv = Canvas::new(crate::ui::W, crate::ui::H);
        let mut scpi = crate::scpi::Scpi::open(0);
        scpi.exec_line(&mut ui, &cv, ":BODE:GEN \"\"");
        assert_eq!(ui.m.bode_addr, "");
        let m = crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap();
        assert_eq!(m.bode_addr, "");
        // two words would not fit the line of the setup file
        scpi.exec_line(&mut ui, &cv, ":BODE:GEN \"10.0.0.9:5025\"");
        scpi.exec_line(&mut ui, &cv, ":BODE:GEN \"a b\"");
        assert_eq!(ui.m.bode_addr, "10.0.0.9:5025");
        assert_eq!(crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap().bode_addr, "10.0.0.9:5025");
    }

    #[test]
    fn base64_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
