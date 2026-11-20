// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The tests of the transport that need no screen; those that do are in oos-ui (ui/src/site/tests.rs).

use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

/// A program that answers nothing of its own: every route of it is 404, no picture, no command, no upload.
struct Nothing;

impl Site for Nothing {
    type Upload = ();
    const MAX_WAVEFORM: usize = 1 << 20;
    fn pass(&mut self) {}
    fn picture(&mut self, s: &TcpStream, _: Option<String>, _: bool) {
        oos_system::net::busy_answer(s);
    }
    fn remote_ok(&self, _: &str) -> bool {
        false
    }
    fn answer(&mut self, route: &str, _: bool, _: &[u8]) -> Option<(&'static str, Body, bool)> {
        // a body made on a sender thread (as the screenshots of the program)
        (route == "/later").then(|| ("text/plain", Body::Later(Box::new(|| b"made later".to_vec())), false))
    }
    fn update_state(&mut self) -> (fn() -> String, String) {
        (held_state, "last <upload>".into())
    }
    fn waveform_epoch(&self) -> u64 {
        0
    }
    fn parse_upload(_: &[u8], _: &str) -> Result<(), String> {
        Err("no uploads".into())
    }
    fn take_upload(&mut self, _: Result<(), String>, _: &str, _: usize, _: u64) -> (bool, String) {
        (false, String::new())
    }
}

/// The tests that use the senders of `oos_system::net` (shared by the process), one at a time: one that holds both
/// would make the other's answer 503.
fn senders() -> std::sync::MutexGuard<'static, ()> {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ONE.lock().unwrap_or_else(|e| e.into_inner())
}

/// While set, the state of the update is not given (`held_state` waits): the program that gives it is still running.
static HOLD_STATE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Set when `held_state` has given the state.
static STATE_GIVEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The state of the update of `Nothing`, once `HOLD_STATE` is clear (at most 60 s, so a server that waits for it in
/// `poll` fails the test instead of holding it).
fn held_state() -> String {
    let until = std::time::Instant::now() + Duration::from_secs(60);
    while HOLD_STATE.load(std::sync::atomic::Ordering::SeqCst) && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(1));
    }
    STATE_GIVEN.store(true, std::sync::atomic::Ordering::SeqCst);
    "state of the update".into()
}

/// GET /update: the state comes from a program that takes a while (`oos-update status --quick`); `poll`, which runs
/// in the frame loop, hands the request on and returns while that program still runs, and the page follows (#205).
/// While both senders hold such a request, the next one is 503 at once.
#[test]
fn the_update_page_is_made_off_the_frame_loop() {
    use std::sync::atomic::Ordering::SeqCst;
    let _one_at_a_time = senders();
    let mut h = Http::<()>::open(0);
    HOLD_STATE.store(true, SeqCst);
    let until = std::time::Instant::now() + Duration::from_secs(60);
    let ask = || {
        let mut c = TcpStream::connect(("127.0.0.1", h.port().unwrap())).unwrap();
        c.write_all(b"GET /update HTTP/1.1\r\n\r\n").unwrap();
        c
    };
    let (mut first, mut second) = (ask(), ask());
    // until a sender thread has each request (it holds one of the senders while the state is held)
    while oos_system::net::SENDERS.load(SeqCst) < oos_system::net::MAX_SENDERS && std::time::Instant::now() < until {
        h.poll(&mut Nothing);
        assert!(!STATE_GIVEN.load(SeqCst), "poll waited for the state of the update");
    }
    assert!(h.pending.is_empty(), "the requests were handed on");
    for c in [&mut first, &mut second] {
        c.set_nonblocking(true).unwrap();
        let mut got = Vec::new();
        assert!(c.read_to_end(&mut got).is_err() && got.is_empty(), "an answer before the state: {got:?}");
        c.set_nonblocking(false).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    }
    // both senders busy: the third is answered 503 in poll, and its state is not asked for
    let mut third = TcpStream::connect(("127.0.0.1", h.port().unwrap())).unwrap();
    third.write_all(b"GET /update HTTP/1.1\r\n\r\n").unwrap();
    third.set_nonblocking(true).unwrap();
    let mut got = Vec::new();
    while std::time::Instant::now() < until {
        h.poll(&mut Nothing);
        match third.read_to_end(&mut got) {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(e) => panic!("{e}"),
        }
    }
    let r = String::from_utf8_lossy(&got);
    assert!(r.starts_with("HTTP/1.1 503 Service Unavailable\r\n"), "{r}");
    assert!(!STATE_GIVEN.load(SeqCst), "the state was given before it was let go");
    HOLD_STATE.store(false, SeqCst);
    for mut c in [first, second] {
        let mut got = Vec::new();
        c.read_to_end(&mut got).unwrap();
        let r = String::from_utf8_lossy(&got);
        assert!(r.starts_with("HTTP/1.1 200 OK\r\n") && r.contains("Content-Type: text/html"), "{r}");
        assert!(r.contains("<pre>state of the update</pre>") && r.contains("last &lt;upload>"), "{r}");
        let (head, body) = r.split_once("\r\n\r\n").unwrap();
        assert!(head.contains(&format!("Content-Length: {}\r\n", body.len())), "{head}");
    }
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

/// Web remote off: the answers still waiting for their clients are dropped with the connections, as the requests
/// being read are; and while `MAX_SENDING` answers wait, one more closes without a byte of it.
#[test]
fn switching_the_web_remote_off_drops_the_answers_that_wait() {
    let mut h = Http::<()>::open(0);
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let pair = || {
        let c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        (c, l.accept().unwrap().0)
    };
    let mut clients = Vec::new();
    for _ in 0..MAX_SENDING {
        let (c, s) = pair();
        let mut out = oos_system::net::Outbox::default();
        out.push(b"an answer still waiting");
        h.sending.push((s, out));
        clients.push(c);
    }
    let (mut ninth, s) = pair();
    h.send(s, b"HTTP/1.1 200 OK\r\n\r\nshort".to_vec());
    assert_eq!(h.sending.len(), MAX_SENDING, "the ninth is not kept");
    ninth.set_read_timeout(Some(Duration::from_secs(300))).unwrap();
    let mut got = Vec::new();
    ninth.read_to_end(&mut got).unwrap();
    assert!(got.is_empty(), "the ninth gets no part of its answer: {:?}", String::from_utf8_lossy(&got));
    h.set_open(None);
    assert!(h.sending.is_empty() && h.pending.is_empty());
}

#[test]
fn identification_names_custom_hislip_port_without_advertising_the_default() {
    let xml = lxi_identification(&Offer { scpi_port: Some(5555), vxi11: false, hislip_port: Some(14880) }, "DHO924S");
    assert!(xml.contains("TCPIP::hislip0,14880::INSTR"), "{xml}");
    assert!(!xml.contains("TCPIP::hislip0::INSTR"), "{xml}");
    let closed = lxi_identification(&Offer { scpi_port: None, vxi11: false, hislip_port: None }, "DHO924S");
    assert!(!closed.contains("InstrumentAddressString"), "{closed}");
}

// Break it catches: the identification named every unit a DHO924S (#205); a DHO804 answered so on LXI discovery.
#[test]
fn identification_names_the_units_model() {
    let offer = Offer { scpi_port: Some(5555), vxi11: true, hislip_port: None };
    let xml = lxi_identification(&offer, "DHO804");
    assert!(xml.contains("<Model>DHO804</Model>") && !xml.contains("DHO924S"), "{xml}");
    assert!(xml.contains("<SerialNumber>0</SerialNumber>"), "the serial stays out: {xml}");
    let odd = lxi_identification(&offer, "A<B&C");
    assert!(odd.contains("<Model>A&lt;B&amp;C</Model>"), "{odd}");
}

#[test]
fn unescape_survives_any_bytes() {
    assert_eq!(unescape("key%20runstop+x"), "key runstop x");
    // two-byte characters right after the %: the slice used to fall inside one
    assert_eq!(unescape("a%é1"), "a1");
    assert_eq!(unescape("%€"), String::from_utf8_lossy(&[0xac]));
    assert_eq!(unescape("x%4"), "x%4");
}

/// ADR 0066: an answer the client does not take yet waits in `sending`, the frame loop goes on, and it goes out whole
/// once the client reads.
#[test]
fn an_answer_the_client_does_not_take_waits_and_does_not_hold_the_loop() {
    const HANG: Duration = Duration::from_secs(300);
    let mut h = Http::<()>::open(0);
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
    let (s, _) = l.accept().unwrap();
    // far more than the socket buffers hold, also on Linux
    let answer: Vec<u8> = (0..32u32 << 20).map(|i| i as u8).collect();
    h.send(s, answer.clone());
    assert_eq!(h.sending.len(), 1, "the rest waits");
    h.poll(&mut Nothing);
    assert_eq!(h.sending.len(), 1, "a pass takes what the socket takes and goes on");
    let reader = std::thread::spawn(move || {
        let mut got = Vec::new();
        c.read_to_end(&mut got).unwrap();
        got
    });
    let t = std::time::Instant::now();
    while !h.sending.is_empty() {
        assert!(t.elapsed() < HANG);
        h.poll(&mut Nothing);
    }
    assert!(reader.join().unwrap() == answer, "the answer arrives whole, and the connection closes after it");
}

/// The page /update: the state and the last result, escaped, above the upload form, in the style of the remote.
#[test]
fn the_update_page_shows_the_state_the_last_result_and_the_form() {
    let page = update_page("state <a> & b", "last <x>");
    assert!(page.contains("<pre>state &lt;a> &amp; b</pre>") && page.contains(">last &lt;x></pre>"), "{page}");
    assert!(page.contains(r#"<input type="file" id="f" accept=".oosupd">"#), "{page}");
    assert!(page.contains(&format!("<style>{STYLE}</style>")), "the page has the style of the remote");
}

/// The answers `poll` sends before a request is read whole (403, 413, 503): the text with its length, and after a
/// HEAD the same head without the text. A HEAD reaches 503 only here: its body may be at most `MAX_OTHER`, which is
/// `LARGE_BODY`, so a HEAD that would wait for the large body's turn is 413 first.
#[test]
fn an_early_answer_to_a_head_has_no_body() {
    for (status, msg) in [
        ("403 Forbidden", "refused"),
        ("413 Payload Too Large", "request too large"),
        ("503 Service Unavailable", "busy, try again"),
    ] {
        let get = String::from_utf8(early_answer(status, msg, false)).unwrap();
        let head = String::from_utf8(early_answer(status, msg, true)).unwrap();
        assert!(
            get.starts_with(&format!("HTTP/1.1 {status}\r\n")) && get.ends_with(&format!("\r\n\r\n{msg}")),
            "{get}"
        );
        assert!(get.contains(&format!("Content-Length: {}\r\n", msg.len())), "{get}");
        assert_eq!(head, get.strip_suffix(msg).unwrap(), "the head of a HEAD is the head of the GET");
    }
    assert_eq!(LARGE_BODY, MAX_OTHER, "a HEAD with a large body is 413 before it could be 503");
}

/// A body the program makes later (`Body::Later`) is made and sent on a sender thread, with its length; a HEAD gets the
/// head alone.
#[test]
fn a_body_made_later_is_sent_whole() {
    let _one_at_a_time = senders();
    let mut h = Http::<()>::open(0);
    for (method, body) in [("GET", "made later"), ("HEAD", "")] {
        let mut c = TcpStream::connect(("127.0.0.1", h.port().unwrap())).unwrap();
        c.write_all(format!("{method} /later HTTP/1.1\r\n\r\n").as_bytes()).unwrap();
        c.set_read_timeout(Some(Duration::from_millis(1))).unwrap();
        let mut got = Vec::new();
        let until = std::time::Instant::now() + Duration::from_secs(60);
        while std::time::Instant::now() < until {
            h.poll(&mut Nothing);
            match c.read_to_end(&mut got) {
                Ok(_) => break,
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => panic!("{e}"),
            }
        }
        let r = String::from_utf8_lossy(&got);
        assert!(r.starts_with("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 10\r\n"), "{r}");
        assert!(r.ends_with(&format!("\r\n\r\n{body}")), "{method}: {r}");
    }
}
