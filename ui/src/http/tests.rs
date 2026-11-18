// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Tests of the web remote: the routes, the screen pictures and their ETags, uploads and their limits, MCP, the page
//! script (run in node where there is one) and the `net.allow` checks. A new test of `http` goes here.

use super::*;
use std::io::Read;

mod hostile;

/// The tests that fetch large answers one after the other: `SENDERS` is shared by the whole process and lets
/// `MAX_SENDERS` through, and under load (tests in parallel, CI) a sender at `SCHED_IDLE` holds its place long
/// enough that another test is answered 503 busy
static SENDS: Mutex<()> = Mutex::new(());

fn sends() -> MutexGuard<'static, ()> {
    SENDS.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn a_foreign_site_gets_nowhere() {
    let f =
        |h: &str, route: &str, post: bool| foreign(&format!("GET {} HTTP/1.1\r\n{}\r\n\r\n", route, h), route, post);
    // tools and the page of the scope itself
    assert!(!f("Host: 192.168.177.64:8080", "/cmd", false));
    assert!(!f("Host: x", "/cmd", false));
    assert!(!f("Host: dho924.local:8080", "/cmd", false));
    assert!(!f("Host: [fe80::1]:8080", "/cmd", false));
    // the scope by its global IPv6 address (net.ipv6 = on), with and without the port, and the page's own requests
    assert!(!f("Host: [2001:db8:5:7:1d2e:3f4a:5b6c:7d8e]:8080", "/cmd", false));
    assert!(!f("Host: [2001:db8:5:7::7d8e]", "/cmd", false));
    assert!(!f(
        "Host: [2001:DB8:5:7::7D8E]:8080\r\nOrigin: http://[2001:db8:5:7::7d8e]:8080\r\nSec-Fetch-Site: same-origin",
        "/update",
        true
    ));
    assert!(f("Host: [2001:db8:5:7::7d8e]:8080\r\nOrigin: http://[2001:db8:5:7::1]:8080", "/update", true));
    assert!(f("Host: [2001:db8::1].evil.example:8080", "/cmd", false));
    assert!(!f("", "/cmd", false));
    assert!(!f(
        "Host: 192.168.177.64:8080\r\nOrigin: http://192.168.177.64:8080\r\nSec-Fetch-Site: same-origin",
        "/update",
        true
    ));
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
    assert!(f("Host: 192.168.177.64:8080\r\nOrigin: https://evil.example", "/waveform", true));
    assert!(f("Host: 192.168.177.64:8080\r\nSec-Fetch-Site: cross-site", "/ui-state", false));
}

#[test]
fn waveform_names_are_display_names() {
    assert_eq!(waveform_name("name=C%3A%5Ccaptures%5Cclock.wfm"), "clock.wfm");
    assert_eq!(waveform_name("name=..%2F..%2Fclock%0A.csv"), "clock.csv");
    assert_eq!(waveform_name("slot=2"), "waveform.csv");
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
    let (mut scpi, mut ui, mut cv) = crate::ui::testkit::scpi_session();
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

/// An explicitly started, private backend for tools/scpi_validate.py.
/// It opens no display, hardware device or LAN discovery announcer.
#[test]
#[ignore = "Owned headless backend for explicit local SCPI acceptance"]
fn scpi_validation_fixture() {
    let var = |name: &str, default: u64| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default);
    let mut h = Http::open(var("OOS_VALIDATION_WEB_PORT", 18881) as u16);
    let mut scpi = crate::scpi::Scpi::open(var("OOS_VALIDATION_SCPI_PORT", 15556) as u16);
    assert!(h.port().is_some() && scpi.port().is_some(), "private acceptance ports must bind");
    let mut ui = crate::ui::testkit::demo_ui();
    ui.set_offline(true);
    ui.store_dir = std::env::var_os("OOS_VALIDATION_STORE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("oos-scpi-validation-{}", std::process::id())));
    let mut cv = Canvas::new(crate::ui::W, crate::ui::H);
    let end = std::time::Instant::now() + Duration::from_secs(var("OOS_VALIDATION_SECONDS", 90));
    while !ui.quit && std::time::Instant::now() < end {
        ui.step(0.04);
        ui.render(&mut cv);
        scpi.poll(&mut ui, &cv);
        for command in h.poll(&mut ui, &cv, &mut scpi) {
            crate::cmd::run(&mut ui, &command);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn get(h: &mut Http, ui: &mut Ui, cv: &Canvas, scpi: &mut crate::scpi::Scpi, path: &str) -> String {
    request(h, ui, cv, scpi, format!("GET {} HTTP/1.1\r\nHost: x\r\n\r\n", path).into_bytes())
}

#[test]
fn waveform_upload_loads_the_chosen_reference_and_preserves_it_on_error() {
    let mut h = Http::open(0);
    let mut ui = crate::ui::testkit::demo_ui();
    let store = crate::ui::testkit::Scratch::new("http-no-write");
    ui.store_dir = store.clone();
    let cv = Canvas::new(crate::ui::W, crate::ui::H);
    let mut scpi = crate::scpi::Scpi::open(0);
    let post = |path: &str, body: &[u8]| {
        let mut data =
            format!("POST {path} HTTP/1.1\r\nHost: x\r\nOrigin: http://x\r\nContent-Length: {}\r\n\r\n", body.len())
                .into_bytes();
        data.extend_from_slice(body);
        data
    };
    let mut csv = String::from("Time (s),Volts\n");
    for k in 0..1000 {
        csv.push_str(&format!("{},{}\n", k as f64 * 1e-9, (k as f64 * std::f64::consts::TAU / 20.0).sin()));
    }
    let reply = request(&mut h, &mut ui, &cv, &mut scpi, post("/waveform?name=clock.csv&slot=3", csv.as_bytes()));
    assert!(reply.starts_with("HTTP/1.1 200") && reply.contains("Loaded clock.csv into R3"), "{reply}");
    assert!(!ui.record_mode(), "loading into a live session adds a reference overlay");
    assert_eq!(ui.ref_slot(), 2);
    let reply = request(&mut h, &mut ui, &cv, &mut scpi, post("/waveform?name=no-slot.csv", csv.as_bytes()));
    assert!(reply.starts_with("HTTP/1.1 409") && reply.contains("R1 to R10"), "{reply}");
    assert_eq!(ui.ref_slot(), 2, "an omitted destination must not replace R1");
    assert_eq!(ui.ref_shown(), Some(true));
    assert!(!store.exists(), "an upload is read into memory, not written to a server-selected path");
    let reply = request(&mut h, &mut ui, &cv, &mut scpi, post("/waveform?name=broken.csv&slot=4", b"not a waveform"));
    assert!(reply.starts_with("HTTP/1.1 409"), "{reply}");
    assert_eq!(ui.ref_slot(), 2, "failed upload keeps the previous selection");
    assert_eq!(ui.ref_shown(), Some(true));
    let mut malformed = Vec::from(&b"OOSW"[..]);
    malformed.extend_from_slice(&1u32.to_le_bytes());
    malformed.extend_from_slice(&1u32.to_le_bytes());
    malformed.extend_from_slice(&u64::MAX.to_le_bytes());
    malformed.extend_from_slice(&1e-9f64.to_le_bytes());
    malformed.extend_from_slice(&0f64.to_le_bytes());
    let reply = request(&mut h, &mut ui, &cv, &mut scpi, post("/waveform?name=broken.bin&slot=4", &malformed));
    assert!(reply.starts_with("HTTP/1.1 409") && reply.contains("32-million-sample import limit"), "{reply}");
    assert_eq!(ui.ref_slot(), 2);
    let reply = request(&mut h, &mut ui, &cv, &mut scpi, post("/waveform?name=clock.csv", csv.as_bytes()));
    assert!(reply.starts_with("HTTP/1.1 409") && reply.contains("R1 to R10"), "{reply}");
    assert_eq!(ui.ref_slot(), 2, "a file with no slot does not replace R1");
    assert!(ui.meas_at(crate::model::WaveId::Ref(0), oos_dsp::measure::Meas::Vpp).is_nan());
    assert!(ui.meas_at(crate::model::WaveId::Ref(2), oos_dsp::measure::Meas::Vpp).is_finite());
    for slot in ["0", "11", "nope", ""] {
        let reply = request(&mut h, &mut ui, &cv, &mut scpi, post(&format!("/waveform?slot={slot}"), csv.as_bytes()));
        assert!(reply.starts_with("HTTP/1.1 409") && reply.contains("R1 to R10"), "{reply}");
    }
    let state = get(&mut h, &mut ui, &cv, &mut scpi, "/ui-state");
    let body = state.split("\r\n\r\n").nth(1).unwrap();
    let parsed = crate::json::parse(body).unwrap();
    assert_eq!(parsed.get("mode").and_then(crate::json::J::str), Some("live"));
}

/// A waveform upload is parsed off the frame loop: the poll that takes the request only starts the parse, and a
/// later poll puts the traces into the slot and answers. Before, the poll parsed it (a CSV near 128 MiB took
/// seconds, with the scope's screen and keys stopped). A session changed meanwhile does not get the late file.
#[test]
fn waveform_upload_is_parsed_off_the_frame_loop_and_committed_by_it() {
    let mut h = Http::open(0);
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
    let port = h.port().unwrap();
    let csv = "Time (s),Volts\n0,0.1\n1e-9,0.4\n2e-9,-0.2\n";
    let upload = |slot: usize| {
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        let head = format!(
            "POST /waveform?name=lane.csv&slot={slot} HTTP/1.1\r\nHost: x\r\n\
                Content-Length: {}\r\n\r\n",
            csv.len()
        );
        c.write_all(head.as_bytes()).unwrap();
        c.write_all(csv.as_bytes()).unwrap();
        c
    };
    for (slot, change) in [(3, false), (5, true)] {
        let mut c = upload(slot);
        let until = Instant::now() + Duration::from_secs(60);
        while h.wave.is_none() {
            assert!(Instant::now() < until, "the upload was not taken");
            h.poll(&mut ui, &cv, &mut scpi);
            assert!(ui.ref_shown_slot(slot - 1).is_none(), "the poll that takes the upload does not parse it");
        }
        if change {
            ui.return_to_live();
        }
        while h.wave.is_some() {
            assert!(Instant::now() < until, "the parse did not end");
            h.poll(&mut ui, &cv, &mut scpi);
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut reply = String::new();
        c.read_to_string(&mut reply).unwrap();
        if change {
            assert!(reply.starts_with("HTTP/1.1 409") && reply.contains("session changed"), "{reply}");
            assert!(ui.ref_shown_slot(slot - 1).is_none());
        } else {
            assert!(reply.starts_with("HTTP/1.1 200") && reply.contains("Loaded lane.csv into R3"), "{reply}");
            assert_eq!(ui.ref_shown_slot(slot - 1), Some(true));
        }
    }
}

#[test]
fn waveform_upload_has_its_own_limit_and_origin_protection() {
    assert_eq!(MAX_WAVEFORM, 128 << 20, "native and browser imports share the parser byte limit");
    assert!(PAGE.contains("up to 128 MiB") && PAGE.contains("file.size>128*1024*1024"));
    let mut h = Http::open(0);
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
    let reply = request(
        &mut h,
        &mut ui,
        &cv,
        &mut scpi,
        format!("POST /waveform?name=x.bin HTTP/1.1\r\nContent-Length: {}\r\n\r\n", MAX_WAVEFORM + 1).into_bytes(),
    );
    assert!(reply.starts_with("HTTP/1.1 413"), "{reply}");
    let reply = request(
        &mut h,
        &mut ui,
        &cv,
        &mut scpi,
        b"POST /waveform HTTP/1.1\r\nHost: x\r\nOrigin: https://evil.example\r\nContent-Length: 0\r\n\r\n".to_vec(),
    );
    assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
    assert!(!ui.record_mode());
    assert!(crate::cmd::remote_ok("workflow live"));
    assert!(crate::cmd::remote_ok("workflow sim"));
    assert!(!crate::cmd::remote_ok("workflow quit"));
    assert!(!crate::cmd::remote_ok("workflow load C:/private/file"));
}

/// Runs the page's script under Node with a small stand-in for the browser (skipped without `node`) and returns the
/// picture addresses it asked for: first before and then after `setfmt('bmp')`, and what it kept in localStorage.
fn run_page_script(script: &str, preset: &str, storage_throws: bool) -> Option<String> {
    if !std::process::Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success()) {
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
  // the remote controls' state poll (/ui-state) is not picture traffic: answered, not logged
  if (!u.startsWith('/screen')) return Promise.resolve({ ok: true, status: 200, headers: { get: () => null }, json: () => Promise.resolve({}), text: () => Promise.resolve(''), blob: () => Promise.resolve({}) });
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
    let dir = std::env::temp_dir().join(format!(
        "oos-ui-page-{}-{}",
        std::process::id(),
        preset.len() + storage_throws as usize
    ));
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join("page.js"), script).unwrap();
    std::fs::write(dir.join("run.cjs"), HARNESS).unwrap();
    let mut cmd = std::process::Command::new("node");
    cmd.arg(dir.join("run.cjs")).arg(dir.join("page.js")).env("PRESET", preset);
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
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
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
    let _sends = sends();
    let mut h = Http::open(0);
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
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
    assert!(
        r.starts_with("HTTP/1.1 200") && r.contains("text/csv") && r.contains("Time (s),CH1 (V)"),
        "{}",
        &r[..300.min(r.len())]
    );
    // download names: nothing while the clock is not set (the page takes the browser's time), then the scope's
    // local time in its own zone
    let body = |r: &str| r.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    assert_eq!(body(&get(&mut h, &mut ui, &cv, &mut scpi, "/stamp")), "");
    ui.clock_known = true;
    ui.sys.tz = "JST-9".into();
    let before = ui.local_now().stamp();
    let st = body(&get(&mut h, &mut ui, &cv, &mut scpi, "/stamp"));
    let after = ui.local_now().stamp();
    // the clock goes on during the request (over a minute or a day too): the stamp lies between the readings
    // around it, and the fixed-width form sorts as text
    assert!(st.len() == 15 && before <= st && st <= after, "{} {} {}", before, st, after);
}

/// Large answers leave the frame loop: the whole body arrives, also several at once, and the count goes back.
#[test]
fn large_answers_go_through_a_thread() {
    let _sends = sends();
    use std::sync::atomic::Ordering::SeqCst;
    let mut h = Http::open(0);
    let mut ui = crate::ui::testkit::demo_ui();
    // a real screen: a blank canvas would be a few KB as a PNG, which is not a large answer
    let mut cv = Canvas::new(crate::ui::PANEL_W, crate::ui::PANEL_H);
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
    // the client has every byte, the sender only lets go of its place: a minute is a guard against a hang only
    let until = Instant::now() + Duration::from_secs(60);
    while SENDERS.load(SeqCst) != 0 {
        assert!(Instant::now() < until, "a sender did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// With a frame loop that hands its screens over before each render, the picture is the screen that was shown,
/// taken without a copy, and the loop's canvas gets a buffer of the right size to draw into. The hand-over is told
/// by the buffer, not its content: a blank canvas swapped for a zeroed spare has equal pixels.
///
/// Break it catches as well: the test's own race. A request in before the first frame is answered with a copy of
/// the screen (`Shots::request` -> `tick`: the loop has not yet said that it renders whole frames), and frames
/// handed over afterwards are pictures made ahead. Run in one order or the other by the client's timing, that
/// failed about 1 in 7 under load; both orders now run on purpose.
#[test]
fn live_picture_from_the_frame_loop_without_a_copy() {
    let _sends = sends();
    for frames_first in [true, false] {
        live_picture_hand_over(frames_first);
    }
}

/// The long poll as a hang guard, a minute instead of the tests' second: a debug PNG takes most of a second, more
/// on a busy computer, and a request that waited out the long poll before the first picture was made got an empty
/// 200. The clients of these tests wait as long (`PICTURE_WAIT`).
fn slow_encoder_guard(h: &Http) {
    h.shots.lock().long_poll = Some(PICTURE_WAIT);
}

/// How long a test's client waits for its picture: a hang guard, not a time the test depends on.
const PICTURE_WAIT: Duration = Duration::from_secs(60);

fn live_picture_hand_over(frames_first: bool) {
    let mut h = Http::open(0);
    // the request waits for its picture however long the encoder takes (`slow_encoder_guard`)
    slow_encoder_guard(&h);
    let (mut scpi, mut ui, mut cv) = crate::ui::testkit::scpi_session();
    let port = h.l.as_ref().unwrap().local_addr().unwrap().port();
    let mut frame = 0u32;
    // the first screen handed over (the canvas got another buffer): the one before the render
    let mut shown: Option<Vec<u32>> = None;
    let mut one_frame = |h: &mut Http, cv: &mut Canvas, shown: &mut Option<Vec<u32>>| {
        let (before, buf) = (cv.px.clone(), cv.px.as_ptr());
        h.before_render(cv, true);
        if cv.px.as_ptr() != buf && shown.is_none() {
            *shown = Some(before);
        }
        assert_eq!(cv.px.len(), (cv.w * cv.h) as usize);
        frame += 1;
        cv.clear(crate::gfx::rgb(frame % 256, 40, 90));
    };
    if frames_first {
        for _ in 0..3 {
            h.poll(&mut ui, &cv, &mut scpi);
            one_frame(&mut h, &mut cv, &mut shown);
        }
        assert!(shown.is_none(), "nothing handed over before a request");
    }
    let t = std::thread::spawn(move || {
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(PICTURE_WAIT)).unwrap();
        let _ = c.write_all(b"GET /screen.png HTTP/1.1\r\nHost: x\r\n\r\n");
        let mut out = Vec::new();
        let _ = c.read_to_end(&mut out);
        out
    });
    // the request is in before the next frame, whatever the load; the screen at that moment
    while h.shots.lock().waiting.is_empty() && !t.is_finished() {
        h.poll(&mut ui, &cv, &mut scpi);
        std::thread::sleep(Duration::from_millis(1));
    }
    let at_request = cv.px.clone();
    while !t.is_finished() {
        one_frame(&mut h, &mut cv, &mut shown);
        h.poll(&mut ui, &cv, &mut scpi);
        std::thread::sleep(Duration::from_millis(5));
    }
    let out = t.join().unwrap();
    let e = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    assert!(out.starts_with(b"HTTP/1.1 200"), "{}", String::from_utf8_lossy(&out[..e]));
    let png = |px: &[u32]| crate::store::png(cv.w as u32, cv.h as u32, px);
    if frames_first {
        // the frame loop runs: the next screen goes over by a buffer swap, and it is the picture
        let shown = shown.expect("a screen was handed over");
        assert!(out[e..] == png(&shown), "the PNG of the screen that was shown");
    } else {
        // before the first frame: the screen of that moment, copied
        assert!(out[e..] == png(&at_request), "the PNG of the screen at the request");
    }
}

/// A screenshot asked for as PNG never gets JPEG bytes, a JPEG viewer gets PNG only once the VPU failed.
#[test]
fn a_picture_answers_only_its_own_format() {
    assert!(Shots::fits(false, false, false) && Shots::fits(true, true, false));
    assert!(!Shots::fits(false, true, false) && !Shots::fits(false, true, true), "PNG asked, JPEG made");
    assert!(!Shots::fits(true, false, false), "JPEG asked, PNG made while the VPU works: wait for the JPEG");
    assert!(Shots::fits(true, false, true), "the VPU failed: PNG for everyone");
}

/// System > Graphics = GPU: the canvas changes only when a picture comes back from the GPU (here every third
/// frame). Nothing is swapped out of it (a swapped-in buffer stayed until the next read: the web page went back
/// and forth between the new and an older screen), and the picture is one that came back.
///
/// In both orders on purpose, like `live_picture_from_the_frame_loop_without_a_copy`: a request that is in before
/// the frame loop has said that it runs gets a copy of the canvas of that moment, which is no picture read back.
/// Which order ran was the client's timing, and the test failed now and then on a busy computer.
#[test]
fn live_picture_with_the_gpu_is_a_picture_read_back() {
    let _sends = sends();
    for frames_first in [true, false] {
        live_picture_gpu(frames_first);
    }
}

fn live_picture_gpu(frames_first: bool) {
    let mut h = Http::open(0);
    slow_encoder_guard(&h);
    let (mut scpi, mut ui, mut cv) = crate::ui::testkit::scpi_session();
    let port = h.l.as_ref().unwrap().local_addr().unwrap().port();
    if frames_first {
        // the frame loop runs before anybody asks
        h.poll(&mut ui, &cv, &mut scpi);
        h.before_gpu_frame();
    }
    let t = std::thread::spawn(move || {
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(PICTURE_WAIT)).unwrap();
        let _ = c.write_all(b"GET /screen.png HTTP/1.1\r\nHost: x\r\n\r\n");
        let mut out = Vec::new();
        let _ = c.read_to_end(&mut out);
        out
    });
    // the request is in before the next frame, whatever the load; the screen at that moment
    while h.shots.lock().waiting.is_empty() && !t.is_finished() {
        h.poll(&mut ui, &cv, &mut scpi);
        std::thread::sleep(Duration::from_millis(1));
    }
    let at_request = cv.px.clone();
    let mut read_back = Vec::new();
    let mut frame = 0u32;
    while !t.is_finished() {
        h.poll(&mut ui, &cv, &mut scpi);
        let before = cv.px.clone();
        h.before_gpu_frame();
        assert!(cv.px == before, "the canvas keeps the last picture read back");
        frame += 1;
        if frame.is_multiple_of(3) {
            cv.clear(crate::gfx::rgb(frame % 256, 40, 90));
            read_back.push(cv.px.clone());
            h.picture_read_back(&cv);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let out = t.join().unwrap();
    let e = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    assert!(out.starts_with(b"HTTP/1.1 200"), "{}", String::from_utf8_lossy(&out[..e]));
    let png = |px: &[u32]| crate::store::png(cv.w as u32, cv.h as u32, px);
    if frames_first {
        assert!(read_back.iter().any(|p| out[e..] == png(p)), "the PNG of a picture read back");
    } else {
        // before the first frame: the screen of that moment, copied
        assert!(out[e..] == png(&at_request), "the PNG of the screen at the request");
    }
}

/// The live picture: one encode per screen; a viewer with the newest picture waits (long poll) and gets 304 when
/// nothing changes, or the new picture as soon as the screen changes; a viewer without it gets it at once.
#[test]
fn live_picture_once_per_screen_and_long_poll() {
    let _sends = sends();
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
    let mut ui = crate::ui::testkit::demo_ui();
    let mut cv = Canvas::new(crate::ui::PANEL_W, crate::ui::PANEL_H);
    ui.render(&mut cv);
    let mut scpi = crate::scpi::Scpi::open(0);
    let port = h.l.as_ref().unwrap().local_addr().unwrap().port();
    // a viewer in a thread; the server is polled meanwhile, and `change` alters the screen after 100 ms
    let shot =
        |h: &mut Http, ui: &mut Ui, scpi: &mut crate::scpi::Scpi, cv: &mut Canvas, have: Option<&str>, change: bool| {
            let inm = have.map_or(String::new(), |t| format!("If-None-Match: {}\r\n", t));
            let req = format!("GET /screen.png?t=1 HTTP/1.1\r\nHost: x\r\n{}\r\n", inm).into_bytes();
            let t0 = Instant::now();
            let t = std::thread::spawn(move || {
                let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
                c.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
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
    // the long poll is a minute but where it is waited out: a debug PNG takes most of a second, and the encoder
    // waits twice its last encode before the next copy (`SHOT_DUTY`), so the test's one second could end before a
    // picture is made on a slow or busy computer (a viewer without a picture then got an empty one)
    let long_poll = |h: &Http, d: Duration| h.shots.lock().long_poll = Some(d);
    long_poll(&h, Duration::from_secs(60));
    let (head, tag1, body, _) = shot(&mut h, &mut ui, &mut scpi, &mut cv, None, false);
    assert!(head.starts_with("HTTP/1.1 200") && head.contains("Content-Type: image/png"), "{}", head);
    assert!(tag1.starts_with('"') && tag1.ends_with('"') && tag1.len() > 2, "{}", head);
    assert!(body == png_of(&cv), "the PNG of the screen");
    // the screen unchanged, the viewer has it: it waits the long poll out, then 304 without a body, no encode
    long_poll(&h, LONG_POLL);
    let (head, tag, body, took) = shot(&mut h, &mut ui, &mut scpi, &mut cv, Some(&tag1), false);
    assert!(head.starts_with("HTTP/1.1 304") && body.is_empty() && tag == tag1, "{}", head);
    assert!(took >= LONG_POLL, "answered after {:?}", took);
    long_poll(&h, Duration::from_secs(60));
    // a viewer without it gets the same picture, without a new encode
    let (head, tag, body, _) = shot(&mut h, &mut ui, &mut scpi, &mut cv, None, false);
    assert!(head.starts_with("HTTP/1.1 200") && tag == tag1 && body == png_of(&cv), "{}", head);
    assert_eq!(encodes(&h), 1);
    // the screen changes while the viewer waits: the new picture comes before the long poll ends (a 304 would come
    // after a minute)
    let (head, tag2, body, _) = shot(&mut h, &mut ui, &mut scpi, &mut cv, Some(&tag1), true);
    assert!(head.starts_with("HTTP/1.1 200") && tag2 != tag1 && body == png_of(&cv), "{}", head);
    assert_eq!(encodes(&h), 2);
    // a foreign page still gets nowhere
    let r = request(
        &mut h,
        &mut ui,
        &cv,
        &mut scpi,
        b"GET /screen.png HTTP/1.1\r\nHost: x\r\nSec-Fetch-Site: cross-site\r\n\r\n".to_vec(),
    );
    assert!(r.starts_with("HTTP/1.1 403"), "{}", r);
}

#[test]
fn update_upload_limits() {
    let mut h = Http::open(0);
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
    let post = |path: &str, body: &[u8]| {
        let mut r = format!("POST {} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n", path, body.len()).into_bytes();
        r.extend_from_slice(body);
        r
    };
    // a 5 MB body is taken on /update (and refused for its content, nothing installed) ...
    let big = vec![0xffu8; 5 << 20];
    let r = request(&mut h, &mut ui, &cv, &mut scpi, post("/update", &big));
    assert!(
        r.starts_with("HTTP/1.1 200") && r.contains("REFUSED: not an OOS update package"),
        "{}",
        &r[..r.len().min(300)]
    );
    // ... but not elsewhere
    let r = request(&mut h, &mut ui, &cv, &mut scpi, post("/mcp", &big));
    assert!(r.starts_with("HTTP/1.1 413"), "{}", &r[..r.len().min(200)]);
    // an announced length beyond the limit is answered at once, without waiting for the body
    let r = request(
        &mut h,
        &mut ui,
        &cv,
        &mut scpi,
        b"POST /update HTTP/1.1\r\nContent-Length: 999999999999\r\n\r\n".to_vec(),
    );
    assert!(r.starts_with("HTTP/1.1 413"), "{}", r);
    let r = request(
        &mut h,
        &mut ui,
        &cv,
        &mut scpi,
        b"POST /update HTTP/1.1\r\nContent-Length: 18446744073709551615\r\n\r\n".to_vec(),
    );
    assert!(r.starts_with("HTTP/1.1 413"), "{}", r);
    // ordinary requests still work
    let r = request(&mut h, &mut ui, &cv, &mut scpi, b"GET /lxi/identification HTTP/1.1\r\n\r\n".to_vec());
    assert!(r.starts_with("HTTP/1.1 200") && r.contains("<Manufacturer>OOS"), "{}", r);
    assert!(r.contains("TCPIP::INSTR") && r.contains("TCPIP::5555::SOCKET"), "{}", r);
}

/// The requests waiting hold at most one large body: before, eight connections could each buffer up to 128 MiB
/// (the Vec doubling to 256 MiB), and `POST /update` or `/mcp` from outside `net.allow` was refused only after
/// its whole body was in. Both are now decided when the head is in; only a head is sent here, so the answer must
/// not wait for a body.
#[test]
fn a_large_body_at_a_time_and_net_allow_before_the_body() {
    let mut h = Http::open(0);
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
    let head = |path: &str, n: usize| format!("POST {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {n}\r\n\r\n");
    // a large upload in progress: its head and a part of its body
    let port = h.port().unwrap();
    let mut first = TcpStream::connect(("127.0.0.1", port)).unwrap();
    first.write_all(head("/update", 8 << 20).as_bytes()).unwrap();
    first.write_all(&vec![0u8; 64 << 10]).unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while !h.pending.iter().any(|p| p.req.len() >= head("/update", 8 << 20).len() + (64 << 10)) {
        assert!(Instant::now() < until, "the first upload was not taken");
        h.poll(&mut ui, &cv, &mut scpi);
        std::thread::sleep(Duration::from_millis(1));
    }
    let taken = h.pending.iter().find(|p| p.large).expect("the first upload is the large body");
    assert_eq!(taken.need, Some(head("/update", 8 << 20).len() + (8 << 20)));
    assert!(taken.req.capacity() <= taken.need.unwrap(), "reserved for the request, not doubled past it");
    for path in ["/waveform?slot=1", "/update"] {
        let r = request(&mut h, &mut ui, &cv, &mut scpi, head(path, 2 << 20).into_bytes());
        assert!(r.starts_with("HTTP/1.1 503"), "{path}: {r}");
    }
    // a small body still goes through beside it
    let r = request(&mut h, &mut ui, &cv, &mut scpi, b"GET /lxi/identification HTTP/1.1\r\n\r\n".to_vec());
    assert!(r.starts_with("HTTP/1.1 200"), "{r}");
    drop(first);
    // outside net.allow: refused at the head, the body never asked for
    h.acl.set(crate::netacl::Acl::parse("192.0.2.0/24").0);
    h.acl.peers_as(Some("198.51.100.7".parse().unwrap()));
    for path in ["/update", "/mcp"] {
        let r = request(&mut h, &mut ui, &cv, &mut scpi, head(path, 1 << 20).into_bytes());
        assert!(r.starts_with("HTTP/1.1 403") && r.ends_with("refused"), "{path}: {r}");
    }
    let r = request(&mut h, &mut ui, &cv, &mut scpi, b"GET /lxi/identification HTTP/1.1\r\n\r\n".to_vec());
    assert!(r.starts_with("HTTP/1.1 200"), "{r}");
}

// Break it catches: the web server keeping the net.allow of its start, so a list changed on the screen
// (Remote services > Allowed) refused or let in an update only after a restart of oos-ui
#[test]
fn net_allow_follows_a_change_without_a_restart() {
    let mut h = Http::open(0);
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
    // the list the screen sets, shared with the server; the test's connections come from 198.51.100.7
    let acl = crate::netacl::Shared::default();
    h.acl = acl.clone();
    acl.peers_as(Some("198.51.100.7".parse().unwrap()));
    let update = b"POST /update HTTP/1.1\r\nHost: x\r\nContent-Length: 4\r\n\r\njunk".to_vec();
    let taken = |r: &str| r.starts_with("HTTP/1.1 200") && r.contains("REFUSED: not an OOS update package");
    let r = request(&mut h, &mut ui, &cv, &mut scpi, update.clone());
    assert!(taken(&r), "no list: everyone: {r}");
    acl.set(crate::netacl::Acl::parse("192.0.2.0/24").0);
    let r = request(&mut h, &mut ui, &cv, &mut scpi, update.clone());
    assert!(r.starts_with("HTTP/1.1 403") && r.ends_with("refused"), "outside the new list: {r}");
    acl.set(crate::netacl::Acl::parse("192.0.2.0/24 198.51.100.0/24").0);
    let r = request(&mut h, &mut ui, &cv, &mut scpi, update.clone());
    assert!(taken(&r), "inside the next one: {r}");
    acl.set(crate::netacl::Acl::from_value("").0);
    let r = request(&mut h, &mut ui, &cv, &mut scpi, update);
    assert!(taken(&r), "the list removed: {r}");
}

#[test]
fn identification_names_the_listeners() {
    let mut h = Http::open(0);
    h.offer(Offer { scpi_port: Some(5555), vxi11: false, hislip_port: Some(crate::hislip::PORT) });
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
    let r = request(&mut h, &mut ui, &cv, &mut scpi, b"GET /lxi/identification HTTP/1.1\r\n\r\n".to_vec());
    assert!(r.contains("TCPIP::hislip0::INSTR"), "{}", r);
    assert!(r.contains("TCPIP::5555::SOCKET"), "{}", r);
    assert!(!r.contains("TCPIP::INSTR<"), "{}", r);
    assert!(r.contains("<IdentificationURL>/lxi/identification</IdentificationURL>"), "{}", r);
}

#[test]
fn identification_names_custom_hislip_port_without_advertising_the_default() {
    let xml = lxi_identification(&Offer { scpi_port: Some(5555), vxi11: false, hislip_port: Some(14880) });
    assert!(xml.contains("TCPIP::hislip0,14880::INSTR"), "{xml}");
    assert!(!xml.contains("TCPIP::hislip0::INSTR"), "{xml}");
    let closed = lxi_identification(&Offer { scpi_port: None, vxi11: false, hislip_port: None });
    assert!(!closed.contains("InstrumentAddressString"), "{closed}");
}

#[test]
fn cmd_takes_keys_knobs_and_touch_only() {
    let mut h = Http::open(0);
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
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
fn mcp_pending_opc_is_an_explicit_tool_error_not_success() {
    let mut ui = crate::ui::Ui::new(crate::wave::Source::Demo { t: 0.0 });
    ui.set_offline(true);
    let mut csv = String::from("Time (s),Volts\n");
    for i in 0..4000 {
        csv.push_str(&format!("{},{}\n", i as f64 * 1e-9, if i % 40 < 20 { -0.5 } else { 0.5 }));
    }
    ui.load_waveform_bytes(csv.as_bytes(), "opc.csv").unwrap();
    ui.m.trig_level = 10.0;
    let cv = crate::gfx::Canvas::new(crate::ui::W, crate::ui::H);
    let mut scpi = crate::scpi::Scpi::open(0);
    let reply=mcp(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"scpi","arguments":{"command":":SINGLE;*OPC?;:RUN"}}}"#,&mut ui,&cv,&mut scpi).unwrap();
    let reply = crate::json::parse(&reply).unwrap();
    assert_eq!(reply.get("result").unwrap().get("isError"), Some(&crate::json::J::Bool(true)));
    assert_eq!(
        ui.m.trig_mode,
        crate::model::TrigMode::Single,
        "commands after an unsupported immediate barrier must not execute"
    );
}

#[test]
fn mcp_empty_query_is_empty_text_while_an_empty_write_is_ok() {
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
    let empty = mcp(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"scpi","arguments":{"command":":MEAS:CLEAR;:MEAS:STRIP?"}}}"#,
            &mut ui, &cv, &mut scpi).unwrap();
    assert!(empty.contains("\"text\":\"\""), "{empty}");
    assert!(!empty.contains("\"text\":\"ok\""));
    let write = mcp(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"scpi","arguments":{"command":":MEAS:CLEAR"}}}"#,
            &mut ui, &cv, &mut scpi).unwrap();
    assert!(write.contains("\"text\":\"ok\""), "{write}");
    let compound = mcp(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"scpi","arguments":{"command":":MEAS:STRIP?;*OPC?"}}}"#,
            &mut ui, &cv, &mut scpi).unwrap();
    assert!(compound.contains("\"text\":\";1\""), "{compound}");
}

#[test]
fn mcp_session() {
    use crate::json::{parse, J};
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
    let mut call = |b: &str| mcp(b, &mut ui, &cv, &mut scpi).map(|r| parse(&r).unwrap());
    let r =
        call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#).unwrap();
    assert_eq!(r.get("result").and_then(|x| x.get("protocolVersion")).and_then(J::str), Some("2025-06-18"));
    assert!(call(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    let r = call(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
    assert!(r.to_string().contains("\"screenshot\""));
    let r = call(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"scpi","arguments":{"command":":TIM:SCAL 0.001;*IDN?"}}}"#).unwrap();
    assert!(r.to_string().contains("OOS,DHO924S"), "{}", r.to_string());
    let r = call(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"screenshot","arguments":{}}}"#)
        .unwrap();
    assert!(r.to_string().contains("iVBORw0KGgo")); // PNG signature in base64
    assert!(call(r#"{"jsonrpc":"2.0","id":5,"method":"nope"}"#).unwrap().get("error").is_some());
    drop(call);
    assert!((ui.m.time_per_div() - 1e-3).abs() < 1e-9);
}

#[test]
fn scpi_extensions() {
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
    for _ in 0..3 {
        ui.step(0.05);
    }
    let mut q = |c: &str| crate::ui::testkit::q(&mut scpi, &mut ui, &cv, c);
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
    assert_eq!(
        (q(":SOUR:FREQ?"), q(":SOUR:VOLT?"), q(":SOUR:VOLT:OFFS?"), q(":SOUR:OUTP?"), q(":SOUR:FUNC?")),
        ("1.234560E+04".into(), "5.000000E-01".into(), "2.500000E-01".into(), "1".into(), "SIN".into())
    );
    // other wave shapes: a ramp caps the frequency at 5 MHz; square duty, ramp symmetry
    q(":SOUR:FREQ 1e7");
    q(":SOUR:FUNC RAMP");
    q(":SOUR:FUNC:RAMP:SYMM 20");
    assert_eq!(
        (q(":SOUR:FUNC?"), q(":SOUR:FREQ?"), q(":SOUR:FUNC:RAMP:SYMM?")),
        ("RAMP".into(), "5.000000E+06".into(), "2.000000E+01".into())
    );
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
    assert_eq!(
        (
            q(":SOUR:MOD:TYP?"),
            q(":SOUR:MOD:FM:DEV?"),
            q(":SOUR:MOD:FM:INT:FREQ?"),
            q(":SOUR:MOD:FM:INT:FUNC?"),
            q(":SOUR:MOD?")
        ),
        ("FM".into(), "1.000000E+06".into(), "1.000000E+03".into(), "SQU".into(), "1".into())
    );
    q(":SOUR:MOD OFF");
    assert_eq!(q(":SOUR:MOD:TYP?"), "NONE");
    q(":SOUR:FREQ 1e9");
    assert_eq!(q(":SOUR:FREQ?"), "6.000000E+07");
}

#[test]
fn scpi_refuses_what_is_not_a_number() {
    let (mut scpi, mut ui, mut cv) = crate::ui::testkit::scpi_session();
    let (delay, offset, probe) = (ui.m.delay, ui.m.ch[0].offset, ui.m.ch[0].probe);
    for c in [
        ":TIM:OFFS nan",
        ":TIM:OFFS inf",
        ":TIM:OFFS 1e39",
        ":CHAN1:OFFS NaN",
        ":CHAN1:PROB -inf",
        ":CHAN1:PROB 0",
        ":CHAN1:PROB -10",
        ":TRIG:EDGE:LEV nan",
        ":SOUR:FREQ nan",
        ":SOUR:VOLT inf",
    ] {
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
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
    scpi.exec_line(&mut ui, &cv, ":BODE:GEN \"\"");
    assert_eq!(ui.m.bode_addr, "");
    let m = crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap();
    assert_eq!(m.bode_addr, "");
    // two words would not fit the line of the setup file
    scpi.exec_line(&mut ui, &cv, ":BODE:GEN \"10.0.0.9:5025\"");
    scpi.exec_line(&mut ui, &cv, ":BODE:GEN \"a b\"");
    assert_eq!(ui.m.bode_addr, "10.0.0.9:5025");
    assert_eq!(crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap().bode_addr, "10.0.0.9:5025");
    // "usb": the generator on the USB port, the LAN address kept for later
    scpi.exec_line(&mut ui, &cv, ":BODE:GEN \"usb\"");
    assert!(ui.m.bode_usb && ui.m.bode_addr == "10.0.0.9:5025");
    assert_eq!(scpi.exec_line(&mut ui, &cv, ":BODE:GEN?"), b"usb".to_vec());
    let m = crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap();
    assert!(m.bode_usb && m.bode_addr == "10.0.0.9:5025");
    scpi.exec_line(&mut ui, &cv, ":BODE:GEN 10.0.0.10");
    assert!(!ui.m.bode_usb && ui.m.bode_addr == "10.0.0.10");
}

/// A port still taken when the web page and SCPI open theirs (the oos-ui before this one, ending; seen on the unit,
/// #212): neither listens, and once the port is free `listen_again` takes it. Before, they stayed off until the next
/// start.
#[test]
fn a_port_taken_at_the_start_is_taken_once_it_is_free() {
    for web in [true, false] {
        let holder = crate::net6::tcp(0).expect("a free port");
        let port = holder.local_addr().unwrap().port();
        let (mut h, mut scpi) = (Http::open(0), crate::scpi::Scpi::open(0));
        let listens = |h: &Http, s: &crate::scpi::Scpi| if web { h.port() } else { s.port() };
        if web {
            h = Http::open(port);
        } else {
            scpi = crate::scpi::Scpi::open(port);
        }
        assert_eq!(listens(&h, &scpi), None, "web {web}: the port is taken");
        let again =
            |h: &mut Http, s: &mut crate::scpi::Scpi| if web { h.listen_again(port) } else { s.listen_again(port) };
        assert!(!again(&mut h, &mut scpi), "web {web}: still taken");
        drop(holder);
        assert!(again(&mut h, &mut scpi), "web {web}: free now");
        assert_eq!(listens(&h, &scpi), Some(port));
        assert!(!again(&mut h, &mut scpi), "web {web}: listening already, nothing to do");
    }
}
