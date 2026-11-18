// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The scope by name on the local network: a Multicast DNS responder (RFC 6762) with DNS service discovery (RFC 6763).
//! `http://dho924s-123456.local:8080` reaches the web remote, also when the scope only has a link-local address
//! (169.254.x.y, rcS without DHCP), and a computer that browses for web pages, SCPI or VXI-11 instruments lists it.
//!
//! - Answers: A of `<name>.local` (no AAAA: the services listen on IPv4 only) with the address of the interface the question came in on, the reverse
//!   PTR of its IPv4 address, `_services._dns-sd._udp.local`, and per service that is switched on a PTR, SRV and TXT
//!   (`_http._tcp` 8080, `_scpi-raw._tcp` 5555, `_vxi-11._tcp` 111). NSEC says which types a name of the scope has.
//! - Questions with the unicast-response bit (QU) get a unicast answer; a question from a port other than 5353 (a plain
//!   DNS resolver: `dig @224.0.0.251 -p 5353 ...`) gets a legacy unicast answer with its id, its questions, TTL 10.
//! - Probing (3 queries 250 ms apart) and announcing (2 answers 1 s apart) before the scope answers, after each change of
//!   the name, the services or the interfaces. A name another host holds becomes `<name>-2`, `-3` ...; an instance
//!   name `... (2)`. Goodbye (TTL 0) for what goes away, and for everything when switched off.
//! - Its own thread: a blocking `poll` with a timeout of up to a second, the interfaces looked at every 2 s. No load on
//!   the frame loop; the thread does not run while the switch is off.
//!
//! The protocol is in `Responder` (no sockets: the tests drive it with packets and times); `Mdns` runs it on the network.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub const PORT: u16 = 5353;
pub const GROUP4: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
pub const GROUP6: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 0xfb);
/// Longest name of the scope: one DNS label.
pub const NAME_MAX: usize = 63;

const T_A: u16 = 1;
const T_PTR: u16 = 12;
const T_TXT: u16 = 16;
const T_AAAA: u16 = 28;
const T_SRV: u16 = 33;
const T_NSEC: u16 = 47;
const T_ANY: u16 = 255;
const CLASS_IN: u16 = 1;
/// The top bit of a class: cache flush in a record, unicast response (QU) in a question.
const TOP: u16 = 0x8000;
/// QR and AA: an authoritative answer.
const RESPONSE: u16 = 0x8400;
/// TTL of the records that name a host or its address (RFC 6762 §10), and of the others.
const TTL_HOST: u32 = 120;
const TTL_OTHER: u32 = 4500;
/// Most a legacy unicast answer gives (RFC 6762 §6.7).
const TTL_LEGACY: u32 = 10;
/// Payload of one packet: an Ethernet frame without the IPv6 and UDP headers, with room.
const MAX_PKT: usize = 1400;
const PROBES: u32 = 3;
const PROBE_GAP: f64 = 0.25;
const ANNOUNCES: u32 = 2;
const ANNOUNCE_GAP: f64 = 1.0;

// ------------------------------------------------------------------------------------------------ the scope's names

/// A name of the scope: one DNS label of letters, digits and hyphens (RFC 952/1123), not starting or ending with a
/// hyphen, up to 63 characters.
pub fn name_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= NAME_MAX
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

/// The digits that tell this unit from others of its model: the last six of the serial number's final run of digits
/// (`DHO9A000123456` gives `123456`), else its last six letters and digits.
fn serial_tag(serial: Option<&str>) -> Option<String> {
    let s: Vec<char> = serial?.trim().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let digits = s.iter().rev().take_while(|c| c.is_ascii_digit()).count();
    let take = if digits > 0 { digits.min(6) } else { s.len().min(6) };
    let t: String = s[s.len() - take..].iter().collect::<String>().to_ascii_lowercase();
    (!t.is_empty()).then_some(t)
}

/// The name of a unit from its model and serial number: `dho924s-123456`; `oos` without either.
pub fn default_name(model: &str, serial: Option<&str>) -> String {
    let m: String =
        model.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>().to_ascii_lowercase();
    let mut n = match (m.is_empty(), serial_tag(serial)) {
        (true, None) => "oos".to_string(),
        (true, Some(t)) => format!("oos-{}", t),
        (false, None) => m,
        (false, Some(t)) => format!("{}-{}", m, t),
    };
    n.truncate(NAME_MAX);
    let n = n.trim_matches('-').to_string();
    if name_ok(&n) {
        n
    } else {
        "oos".into()
    }
}

/// The instance name a computer lists for the services: `Rigol DHO924S OOS (123456)` with the default name, else with
/// the name in brackets. One DNS label (63 bytes) at most.
pub fn instance_name(model: &str, serial: Option<&str>, name: &str) -> String {
    let tag = match serial_tag(serial) {
        Some(t) if name == default_name(model, serial) => t,
        _ => name.to_string(),
    };
    let mut s = format!("Rigol {} OOS ({})", model.trim(), tag);
    if s.len() > NAME_MAX {
        s = format!("OOS ({})", tag);
        s.truncate(NAME_MAX);
    }
    s
}

mod responder;
mod wire;

pub use responder::*;
pub use wire::*;

// ------------------------------------------------------------------------------------------------ on the network

/// What the page shows: the state and the host label in use.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub state: State,
    pub name: String,
}

static STATUS: std::sync::Mutex<Status> = std::sync::Mutex::new(Status { state: State::Off, name: String::new() });

/// Every IPv4 address with its netmask of the interfaces that are up (not loopback), in the order the kernel lists
/// them; an interface can have several (a DHCP or set address and a link-local one).
pub fn ipv4_networks() -> Vec<(Ipv4Addr, Ipv4Addr)> {
    net::ipv4_all()
}

/// The responder's state, for the page (Off until it runs).
pub fn status() -> Status {
    STATUS.lock().map(|s| s.clone()).unwrap_or_default()
}

fn set_status(s: Status) {
    if let Ok(mut g) = STATUS.lock() {
        *g = s;
    }
}

mod net;

/// What the frame loop tells the thread.
enum Ctl {
    Set(Config),
    Stop,
}

/// The responder on the network, in its own thread (started when first switched on).
pub struct Mdns {
    tx: Option<std::sync::mpsc::Sender<Ctl>>,
    th: Option<std::thread::JoinHandle<()>>,
    /// interface names to use (a host test), else all but loopback
    only: Option<Vec<String>>,
}

impl Mdns {
    pub fn new(only: Option<Vec<String>>) -> Mdns {
        Mdns { tx: None, th: None, only }
    }

    /// The configuration to announce (on or off).
    pub fn set(&mut self, cfg: Config) {
        if self.tx.is_none() {
            if !cfg.on {
                return;
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let only = self.only.clone();
            match std::thread::Builder::new().name("mdns".into()).spawn(move || run(rx, only)) {
                Ok(h) => {
                    self.tx = Some(tx);
                    self.th = Some(h);
                }
                Err(e) => {
                    eprintln!("oos-ui: mDNS: no thread: {}", e);
                    return;
                }
            }
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(Ctl::Set(cfg));
        }
    }

    /// Goodbyes and the end of the thread (at the end of oos-ui).
    pub fn stop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Ctl::Stop);
        }
        if let Some(h) = self.th.take() {
            let _ = h.join();
        }
    }
}

/// The thread: sockets while on, a blocking wait between packets and timers.
fn run(rx: std::sync::mpsc::Receiver<Ctl>, only: Option<Vec<String>>) {
    use std::sync::mpsc::TryRecvError;
    let t0 = std::time::Instant::now();
    let now = || t0.elapsed().as_secs_f64();
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)
        ^ std::process::id() as u64;
    let mut r = Responder::new(seed);
    let mut sock: Option<net::Net> = None;
    let mut ifs: Vec<Iface> = Vec::new();
    let mut if_t = f64::NEG_INFINITY;
    let mut buf = vec![0u8; 9000];
    let flush = |r: &mut Responder| {
        for l in r.log.drain(..) {
            eprintln!("oos-ui: {}", l);
        }
    };
    loop {
        // the frame loop's wishes: without sockets the thread sleeps until the next one
        let msg = if sock.is_some() {
            match rx.try_recv() {
                Ok(m) => Some(m),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Ctl::Stop),
            }
        } else {
            Some(rx.recv().unwrap_or(Ctl::Stop))
        };
        match msg {
            Some(Ctl::Set(c)) => {
                let on = c.on;
                if on && sock.is_none() {
                    match net::Net::open() {
                        Ok(s) => {
                            sock = Some(s);
                            if_t = f64::NEG_INFINITY;
                        }
                        Err(e) => {
                            eprintln!("oos-ui: mDNS: port 5353: {}", e);
                            set_status(Status { state: State::Failed, name: c.name.clone() });
                            continue;
                        }
                    }
                }
                let outs = r.set_config(now(), c);
                if let Some(s) = &sock {
                    outs.iter().for_each(|o| s.send(o, &ifs));
                }
                if !on {
                    sock = None;
                    ifs.clear();
                    let _ = r.set_ifaces(now(), Vec::new());
                }
            }
            Some(Ctl::Stop) => {
                let mut c = r.cfg.clone();
                c.on = false;
                let outs = r.set_config(now(), c);
                if let Some(s) = &sock {
                    outs.iter().for_each(|o| s.send(o, &ifs));
                }
                set_status(Status::default());
                return;
            }
            None => {}
        }
        let Some(s) = sock.as_mut() else {
            set_status(Status { state: r.state(), name: r.host_label() });
            continue;
        };
        if now() - if_t >= 2.0 {
            if_t = now();
            let new = net::interfaces(only.as_deref());
            if new != ifs {
                s.join(&new);
                // goodbyes for an old address go out on an interface that is still there, from its new address
                ifs = new;
                for o in r.set_ifaces(now(), ifs.clone()) {
                    s.send(&o, &ifs);
                }
            }
        }
        while let Some((n, src, idx, group)) = s.recv(&mut buf) {
            for o in r.receive(now(), &buf[..n], src, idx, group) {
                s.send(&o, &ifs);
            }
        }
        for o in r.tick(now()) {
            s.send(&o, &ifs);
        }
        flush(&mut r);
        set_status(Status { state: r.state(), name: r.host_label() });
        let t = now();
        let wake = r.next_wake().map_or(1.0, |w| w - t).min(if_t + 2.0 - t).clamp(0.0, 1.0);
        s.wait(wake);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ETH: u32 = 2;

    fn eth(a: [u8; 4]) -> Iface {
        Iface {
            index: ETH,
            name: "eth0".into(),
            v4: Some((Ipv4Addr::from(a), Ipv4Addr::new(255, 255, 255, 0))),
            v6: Vec::new(),
        }
    }

    fn cfg() -> Config {
        Config {
            on: true,
            name: "dho924s-123456".into(),
            instance: "Rigol DHO924S OOS (123456)".into(),
            services: vec![
                Service { kind: "_http._tcp".into(), port: 8080, txt: vec!["path=/".into()] },
                Service { kind: "_scpi-raw._tcp".into(), port: 5555, txt: Vec::new() },
                Service { kind: "_vxi-11._tcp".into(), port: 111, txt: Vec::new() },
            ],
            v6: false,
        }
    }

    /// A responder through probing and announcing on eth0 at 192.168.1.20.
    fn running() -> Responder {
        let mut r = Responder::new(7);
        r.set_config(0.0, cfg());
        r.set_ifaces(0.0, vec![eth([192, 168, 1, 20])]);
        let mut t = 0.0;
        while r.phase != Phase::Run {
            t += 0.05;
            r.tick(t);
            assert!(t < 5.0);
        }
        r
    }

    fn query(id: u16, qs: &[(&[&str], u16, bool)]) -> Vec<u8> {
        let mut w = Wr::new(id, 0);
        for (n, t, qu) in qs {
            w.question(&dname(n), *t, CLASS_IN | if *qu { TOP } else { 0 });
        }
        w.counts([qs.len(), 0, 0, 0]);
        w.b
    }

    fn peer(port: u16) -> SocketAddr {
        SocketAddr::from(([192, 168, 1, 50], port))
    }

    fn one(out: &[Out]) -> Msg {
        assert_eq!(out.len(), 1, "{:?}", out);
        parse(&out[0].pkt).expect("our own packet parses")
    }

    fn has(rs: &[Rr], name: &str, t: u16) -> bool {
        rs.iter().any(|r| r.rtype == t && r.name.join(".").eq_ignore_ascii_case(name))
    }

    #[test]
    fn names_of_the_scope() {
        for s in ["dho924s-123456", "a", "lab-3", &"x".repeat(63)] {
            assert!(name_ok(s), "{}", s);
        }
        for s in ["", "-a", "a-", "a b", "a.b", "ä", &"x".repeat(64), "a_b"] {
            assert!(!name_ok(s), "{}", s);
        }
        assert_eq!(default_name("DHO924S", Some("DHO9A000123456")), "dho924s-123456");
        assert_eq!(default_name("DHO804", Some("DHO8A000000001")), "dho804-000001");
        assert_eq!(default_name("DHO924S", None), "dho924s");
        assert_eq!(default_name("DHO924S", Some("ABCDEFGH")), "dho924s-cdefgh");
        assert_eq!(default_name("", None), "oos");
        assert_eq!(default_name("DHO 924/S", Some(" 12 ")), "dho924s-12");
        assert!(name_ok(&default_name(&"M".repeat(80), Some("1234567"))));
        assert_eq!(instance_name("DHO924S", Some("DHO9A000123456"), "dho924s-123456"), "Rigol DHO924S OOS (123456)");
        assert_eq!(instance_name("DHO924S", Some("DHO9A000123456"), "lab-scope-3"), "Rigol DHO924S OOS (lab-scope-3)");
        assert_eq!(instance_name("DHO924S", None, "dho924s"), "Rigol DHO924S OOS (dho924s)");
        assert!(instance_name("DHO924S", None, &"x".repeat(63)).len() <= NAME_MAX);
    }

    #[test]
    fn compressed_names_are_read_and_loops_refused() {
        // a question "a.local" at 12, then a record "b" + pointer to "local" (offset 14)
        let mut b = vec![0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0];
        b.extend([1, b'a', 5, b'l', b'o', b'c', b'a', b'l', 0, 0, 1, 0, 1]);
        b.extend([1, b'b', 0xc0, 14, 0, 1, 0, 1, 0, 0, 0, 120, 0, 4, 10, 0, 0, 1]);
        let m = parse(&b).unwrap();
        assert_eq!(m.qs[0].name, dname(&["a", "local"]));
        assert_eq!(m.an[0].name, dname(&["b", "local"]));
        assert_eq!(m.an[0].canon, vec![10, 0, 0, 1]);
        // a pointer to itself, one forward, two pointing at each other
        let head = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        for name in [vec![0xc0, 12], vec![0xc0, 14, 0], vec![1, b'x', 0xc0, 12], vec![0xc0, 14, 0xc0, 12]] {
            let mut p = head.clone();
            p.extend(&name);
            p.extend([0, 1, 0, 1]);
            assert!(parse(&p).is_none(), "{:?}", name);
        }
        // 0x40 and 0x80 label types, a name over 255 bytes
        for bad in [0x40u8, 0x80] {
            let mut p = head.clone();
            p.extend([bad, 0, 0, 1, 0, 1]);
            assert!(parse(&p).is_none());
        }
        let mut p = head.clone();
        for _ in 0..5 {
            p.push(60);
            p.extend([b'x'; 60]);
        }
        p.extend([0, 0, 1, 0, 1]);
        assert!(parse(&p).is_none());
        // names in record data: a PTR whose target is compressed
        let mut w = Wr::new(0, RESPONSE);
        let r = Rec::new(
            dname(&["_http", "_tcp", "local"]),
            Data::Ptr(dname(&["Scope", "_http", "_tcp", "local"])),
            4500,
            false,
        );
        w.rec(&r, 4500, false);
        w.counts([0, 1, 0, 0]);
        assert_eq!(w.b.len(), 12 + 18 + 10 + 8, "the target points back");
        let m = parse(&w.b).unwrap();
        assert!(r.matches(&m.an[0]));
    }

    /// Malformed bytes never panic: every cut of real packets, random bytes, and random changes of real packets.
    #[test]
    fn malformed_packets_never_panic() {
        let mut r = running();
        let mut seeds: Vec<Vec<u8>> = vec![r.probe(&eth([10, 0, 0, 1]))];
        let q = query(1, &[(&["_http", "_tcp", "local"], T_PTR, false)]);
        seeds.extend(r.receive(1.0, &q, peer(PORT), ETH, true).into_iter().map(|o| o.pkt));
        seeds.extend(responses(0, &[], &r.records(&eth([10, 0, 0, 1])), &[], Style::Normal));
        seeds.push(q);
        for s in &seeds {
            for n in 0..s.len() {
                let _ = parse(&s[..n]);
                let _ = r.receive(2.0, &s[..n], peer(PORT), ETH, true);
            }
        }
        let mut x = 0x9e3779b97f4a7c15u64;
        let mut rnd = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for i in 0..20_000 {
            let mut p = if i % 2 == 0 {
                (0..(rnd() % 300)).map(|_| rnd() as u8).collect::<Vec<u8>>()
            } else {
                seeds[i % seeds.len()].clone()
            };
            if !p.is_empty() {
                for _ in 0..1 + rnd() % 4 {
                    let k = (rnd() as usize) % p.len();
                    p[k] = rnd() as u8;
                }
            }
            let _ = parse(&p);
            let _ = r.receive(3.0, &p, peer(if i % 3 == 0 { 40000 } else { PORT }), ETH, i % 5 != 0);
        }
    }

    #[test]
    fn probes_then_announces_then_answers() {
        let mut r = Responder::new(3);
        r.set_config(0.0, cfg());
        assert_eq!(r.state(), State::NoNetwork);
        r.set_ifaces(0.0, vec![eth([169, 254, 7, 9])]);
        assert_eq!(r.state(), State::Probing);
        let q = query(0, &[(&["dho924s-123456", "local"], T_A, false)]);
        let (mut probes, mut ann) = (Vec::new(), Vec::new());
        let mut t = 0.0;
        while r.phase != Phase::Run {
            for o in r.tick(t) {
                assert_eq!(o.dest, Dest::Group4);
                let m = parse(&o.pkt).unwrap();
                if m.is_response() { &mut ann } else { &mut probes }.push((t, m));
            }
            // no answers while probing
            if matches!(r.phase, Phase::Probe { .. }) {
                assert!(r.receive(t, &q, peer(PORT), ETH, true).is_empty());
            }
            t += 0.01;
        }
        assert_eq!(probes.len(), 3);
        assert!(probes[0].0 <= 0.25 + 1e-9);
        for w in probes.windows(2) {
            assert!((w[1].0 - w[0].0 - 0.25).abs() < 0.011, "{} {}", w[0].0, w[1].0);
        }
        let p = &probes[0].1;
        assert!(!p.is_response());
        assert!(p.qs.iter().all(|q| q.qtype == T_ANY && q.qu));
        assert!(p.qs.iter().any(|q| q.name == dname(&["dho924s-123456", "local"])));
        assert!(p.qs.iter().any(|q| q.name.join(".") == "Rigol DHO924S OOS (123456)._http._tcp.local"));
        assert!(
            has(&p.ns, "dho924s-123456.local", T_A)
                && has(&p.ns, "Rigol DHO924S OOS (123456)._scpi-raw._tcp.local", T_SRV)
        );
        // announcements: two, a second apart, every record with cache flush on the unique ones
        assert_eq!(ann.len(), 2);
        assert!(ann[0].0 - probes[2].0 >= 0.25 - 1e-9);
        assert!((ann[1].0 - ann[0].0 - 1.0).abs() < 0.011);
        let a = &ann[0].1;
        assert!(a.is_response() && a.flags & 0x0400 != 0);
        let addr = a.an.iter().find(|r| r.rtype == T_A).unwrap();
        assert!(addr.flush && addr.ttl == 120 && addr.canon == vec![169, 254, 7, 9]);
        assert!(a
            .an
            .iter()
            .any(|r| r.rtype == T_PTR && !r.flush && r.name.join(".") == "_services._dns-sd._udp.local"));
        assert!(has(&a.an, "9.7.254.169.in-addr.arpa", T_PTR));
        assert_eq!(r.log.len(), 1, "{:?}", r.log);
        assert!(r.log[0].contains("dho924s-123456.local on eth0"));
        // now it answers
        assert!(!r.receive(t, &q, peer(PORT), ETH, true).is_empty());
        assert_eq!(r.next_wake(), None);
    }

    #[test]
    fn answers_a_ptr_srv_txt() {
        let mut r = running();
        // A, multicast, with the cache-flush bit and an NSEC for the missing AAAA
        let m = one(&r.receive(10.0, &query(0, &[(&["DHO924S-123456", "local"], T_A, false)]), peer(PORT), ETH, true));
        assert_eq!((m.id, m.qs.len()), (0, 0));
        assert_eq!(m.an.len(), 1);
        assert!(m.an[0].flush && m.an[0].canon == vec![192, 168, 1, 20]);
        assert!(m.ar.iter().any(|r| r.rtype == T_NSEC));
        // the same again within a second: not multicast twice
        assert!(r
            .receive(10.5, &query(0, &[(&["dho924s-123456", "local"], T_A, false)]), peer(PORT), ETH, true)
            .is_empty());
        // browsing: the PTR with SRV, TXT and the address in the additional section
        let m = one(&r.receive(11.0, &query(0, &[(&["_http", "_tcp", "local"], T_PTR, false)]), peer(PORT), ETH, true));
        assert_eq!(m.an.len(), 1);
        let want = Rec::new(
            dname(&["_http", "_tcp", "local"]),
            Data::Ptr(dname(&["Rigol DHO924S OOS (123456)", "_http", "_tcp", "local"])),
            4500,
            false,
        );
        assert!(want.matches(&m.an[0]) && !m.an[0].flush);
        let inst = "Rigol DHO924S OOS (123456)._http._tcp.local";
        assert!(has(&m.ar, inst, T_SRV) && has(&m.ar, inst, T_TXT) && has(&m.ar, "dho924s-123456.local", T_A));
        let srv = m.ar.iter().find(|r| r.rtype == T_SRV).unwrap();
        let mut c = vec![0, 0, 0, 0, 0x1f, 0x90];
        canon_name(&dname(&["dho924s-123456", "local"]), &mut c);
        assert_eq!(srv.canon, c, "port 8080, the host as target");
        let txt = m.ar.iter().find(|r| r.rtype == T_TXT).unwrap();
        assert_eq!(txt.canon, b"\x06path=/".to_vec());
        // SRV and TXT asked directly; an empty TXT is one empty string
        let m = one(&r.receive(
            12.0,
            &query(0, &[(&["Rigol DHO924S OOS (123456)", "_scpi-raw", "_tcp", "local"], T_ANY, false)]),
            peer(PORT),
            ETH,
            true,
        ));
        assert!(has(&m.an, "Rigol DHO924S OOS (123456)._scpi-raw._tcp.local", T_SRV));
        assert_eq!(m.an.iter().find(|r| r.rtype == T_TXT).unwrap().canon, vec![0]);
        // the service types
        let m = one(&r.receive(
            13.0,
            &query(0, &[(&["_services", "_dns-sd", "_udp", "local"], T_PTR, false)]),
            peer(PORT),
            ETH,
            true,
        ));
        assert_eq!(m.an.len(), 3);
        // the reverse name
        let m = one(&r.receive(
            14.0,
            &query(0, &[(&["20", "1", "168", "192", "in-addr", "arpa"], T_PTR, false)]),
            peer(PORT),
            ETH,
            true,
        ));
        assert_eq!(m.an[0].canon[1..15].to_vec(), b"dho924s-123456".to_vec());
        // AAAA of a host without IPv6: NSEC that lists A only
        let m =
            one(&r.receive(15.0, &query(0, &[(&["dho924s-123456", "local"], T_AAAA, false)]), peer(PORT), ETH, true));
        assert_eq!(m.an[0].rtype, T_NSEC);
        assert_eq!(m.an[0].canon[m.an[0].canon.len() - 3..].to_vec(), vec![0, 1, 0x40]);
        // other names and other hosts' services: nothing
        assert!(r.receive(16.0, &query(0, &[(&["other", "local"], T_A, false)]), peer(PORT), ETH, true).is_empty());
        assert!(r
            .receive(16.0, &query(0, &[(&["_ipp", "_tcp", "local"], T_PTR, false)]), peer(PORT), ETH, true)
            .is_empty());
        // an interface the responder does not know: by the sender's network (the host's own query on loopback), else
        // nothing
        let q = query(9, &[(&["dho924s-123456", "local"], T_A, false)]);
        assert!(r.receive(17.0, &q, SocketAddr::from(([10, 9, 9, 9], 40000)), 99, true).is_empty());
        let out = r.receive(17.0, &q, SocketAddr::from(([192, 168, 1, 20], 40000)), 1, true);
        assert_eq!(one(&out).an[0].canon, vec![192, 168, 1, 20]);
        assert_eq!(out[0].iface, ETH);
    }

    #[test]
    fn qu_and_legacy_unicast() {
        let mut r = running();
        // QU: to the asker, port 5353
        let out = r.receive(10.0, &query(0, &[(&["dho924s-123456", "local"], T_A, true)]), peer(PORT), ETH, true);
        assert_eq!(out[0].dest, Dest::To(peer(PORT)));
        assert!(parse(&out[0].pkt).unwrap().an[0].flush);
        // a plain resolver (dig): its id, its question, TTL 10 at most, no cache flush, to its port
        let out =
            r.receive(10.1, &query(0x4242, &[(&["_http", "_tcp", "local"], T_PTR, false)]), peer(53000), ETH, true);
        assert_eq!(out[0].dest, Dest::To(peer(53000)));
        let m = one(&out);
        assert_eq!(m.id, 0x4242);
        assert_eq!(m.qs.len(), 1);
        assert_eq!(m.qs[0].name, dname(&["_http", "_tcp", "local"]));
        assert!(m.an.iter().chain(&m.ar).all(|r| r.ttl <= 10 && !r.flush));
        // again at once: a legacy query is always answered
        assert_eq!(
            r.receive(10.2, &query(0x4243, &[(&["dho924s-123456", "local"], T_A, false)]), peer(53000), ETH, true)
                .len(),
            1
        );
        // sent to the scope's own address: only from the same link
        let q = query(1, &[(&["dho924s-123456", "local"], T_A, false)]);
        assert_eq!(r.receive(11.0, &q, peer(PORT), ETH, false)[0].dest, Dest::To(peer(PORT)));
        assert!(r.receive(11.0, &q, SocketAddr::from(([8, 8, 8, 8], PORT)), ETH, false).is_empty());
        assert!(!r.receive(11.0, &q, SocketAddr::from(([169, 254, 3, 4], 40000)), ETH, false).is_empty());
    }

    #[test]
    fn known_answers_are_left_out() {
        let mut r = running();
        let mut w = Wr::new(0, 0);
        w.question(&dname(&["_services", "_dns-sd", "_udp", "local"]), T_PTR, CLASS_IN);
        let ty = dname(&["_services", "_dns-sd", "_udp", "local"]);
        w.rec(&Rec::new(ty.clone(), Data::Ptr(dname(&["_http", "_tcp", "local"])), 4500, false), 3000, false);
        w.rec(&Rec::new(ty.clone(), Data::Ptr(dname(&["_scpi-raw", "_tcp", "local"])), 4500, false), 100, false);
        w.counts([1, 2, 0, 0]);
        let m = one(&r.receive(10.0, &w.b, peer(PORT), ETH, true));
        // _http known with more than half the TTL, _scpi-raw with too little
        assert_eq!(m.an.len(), 2);
        assert!(!m.an.iter().any(|a| a.canon.starts_with(b"\x05_http")));
    }

    fn response_from(name: &[&str], data: Data) -> Vec<u8> {
        let mut w = Wr::new(0, RESPONSE);
        w.rec(&Rec::new(dname(name), data, 120, true), 120, true);
        w.counts([0, 1, 0, 0]);
        w.b
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let mut r = Responder::new(5);
        r.set_config(0.0, cfg());
        r.set_ifaces(0.0, vec![eth([192, 168, 1, 20])]);
        r.tick(0.3);
        // the scope's own probe and announcement come back: no conflict
        let own = SocketAddr::from(([192, 168, 1, 20], PORT));
        for o in r.tick(0.6) {
            r.receive(0.6, &o.pkt, own, ETH, true);
        }
        let same = response_from(&["dho924s-123456", "local"], Data::A(Ipv4Addr::new(192, 168, 1, 20)));
        r.receive(0.6, &same, peer(PORT), ETH, true);
        assert_eq!(r.host_label(), "dho924s-123456");
        // a response from another host on port 40000 does not count
        let other = response_from(&["dho924s-123456", "local"], Data::A(Ipv4Addr::new(192, 168, 1, 77)));
        r.receive(0.7, &other, peer(40000), ETH, true);
        assert_eq!(r.host_label(), "dho924s-123456");
        // another host answers the probe: -2, and probing again
        r.receive(0.7, &other, peer(PORT), ETH, true);
        assert_eq!(r.host_label(), "dho924s-123456-2");
        assert!(matches!(r.phase, Phase::Probe { sent: 0, .. }));
        assert!(r.log.last().unwrap().contains("dho924s-123456.local is taken"), "{:?}", r.log);
        let other2 = response_from(&["DHO924S-123456-2", "local"], Data::A(Ipv4Addr::new(192, 168, 1, 78)));
        r.tick(1.0);
        r.receive(1.0, &other2, peer(PORT), ETH, true);
        assert_eq!(r.host_label(), "dho924s-123456-3");
        // an instance name: (2)
        let srv = response_from(
            &["Rigol DHO924S OOS (123456)", "_http", "_tcp", "local"],
            Data::Srv { port: 80, target: dname(&["x", "local"]) },
        );
        r.receive(1.1, &srv, peer(PORT), ETH, true);
        assert_eq!(r.inst_label(), "Rigol DHO924S OOS (123456) (2)");
        // probes and announcements under the new names
        let mut t = 1.1;
        while r.phase != Phase::Run {
            t += 0.05;
            r.tick(t);
        }
        let m =
            one(&r.receive(t + 2.0, &query(0, &[(&["dho924s-123456-3", "local"], T_A, false)]), peer(PORT), ETH, true));
        assert_eq!(m.an[0].canon, vec![192, 168, 1, 20]);
        assert!(r
            .receive(t + 2.0, &query(0, &[(&["dho924s-123456", "local"], T_A, false)]), peer(PORT), ETH, true)
            .is_empty());
        // a new name set by hand starts without a number
        let mut c = cfg();
        c.name = "lab-3".into();
        r.set_config(t + 3.0, c);
        assert_eq!(r.host_label(), "lab-3");
    }

    #[test]
    fn a_conflict_after_announcing_probes_again() {
        let mut r = running();
        let other = response_from(&["dho924s-123456", "local"], Data::A(Ipv4Addr::new(192, 168, 1, 77)));
        r.receive(20.0, &other, peer(PORT), ETH, true);
        // not renamed yet: the probe finds out whether the other host still holds the name
        assert_eq!(r.host_label(), "dho924s-123456");
        assert_eq!(r.state(), State::Probing);
        // a goodbye of another host for the name is no conflict
        let mut r = running();
        let mut w = Wr::new(0, RESPONSE);
        w.rec(
            &Rec::new(dname(&["dho924s-123456", "local"]), Data::A(Ipv4Addr::new(192, 168, 1, 77)), 120, true),
            0,
            true,
        );
        w.counts([0, 1, 0, 0]);
        r.receive(20.0, &w.b, peer(PORT), ETH, true);
        assert_eq!(r.state(), State::Running);
    }

    #[test]
    fn simultaneous_probes_are_broken_by_the_data() {
        let probe_with = |a: [u8; 4]| {
            let mut w = Wr::new(0, 0);
            w.question(&dname(&["dho924s-123456", "local"]), T_ANY, CLASS_IN | TOP);
            w.rec(&Rec::new(dname(&["dho924s-123456", "local"]), Data::A(Ipv4Addr::from(a)), 120, true), 120, false);
            w.counts([1, 0, 1, 0]);
            w.b
        };
        let mut r = Responder::new(9);
        r.set_config(0.0, cfg());
        r.set_ifaces(0.0, vec![eth([192, 168, 1, 20])]);
        r.tick(0.3);
        // the other host's address is lower: the scope wins and goes on
        r.receive(0.4, &probe_with([192, 168, 1, 10]), peer(PORT), ETH, true);
        assert!(matches!(r.phase, Phase::Probe { sent: 1, .. }));
        // higher: the scope loses, waits a second and probes again
        r.receive(0.4, &probe_with([192, 168, 1, 30]), peer(PORT), ETH, true);
        assert!(matches!(r.phase, Phase::Probe { sent: 0, at } if (at - 1.4).abs() < 1e-9));
        assert_eq!(r.host_label(), "dho924s-123456");
    }

    #[test]
    fn goodbyes_when_a_service_or_everything_goes() {
        let mut r = running();
        let mut c = cfg();
        c.services.retain(|s| s.kind != "_vxi-11._tcp");
        let out = r.set_config(30.0, c.clone());
        let m = one(&out);
        assert!(m.an.iter().all(|a| a.ttl == 0));
        assert!(has(&m.an, "_services._dns-sd._udp.local", T_PTR) && has(&m.an, "_vxi-11._tcp.local", T_PTR));
        assert!(!has(&m.an, "_http._tcp.local", T_PTR) && !has(&m.an, "dho924s-123456.local", T_A));
        assert_eq!(r.state(), State::Probing);
        // switched off: every record with TTL 0, then nothing more
        let mut t = 30.0;
        while r.phase != Phase::Run {
            t += 0.05;
            r.tick(t);
        }
        c.on = false;
        let m = one(&r.set_config(t, c));
        assert!(has(&m.an, "dho924s-123456.local", T_A) && m.an.iter().all(|a| a.ttl == 0));
        assert_eq!(r.state(), State::Off);
        assert!(r.tick(t + 10.0).is_empty());
        assert!(r
            .receive(t, &query(0, &[(&["dho924s-123456", "local"], T_A, false)]), peer(PORT), ETH, true)
            .is_empty());
    }

    #[test]
    fn a_new_address_is_probed_and_announced() {
        let mut r = Responder::new(11);
        r.set_config(0.0, cfg());
        r.set_ifaces(0.0, vec![eth([169, 254, 7, 9])]);
        let mut t = 0.0;
        while r.phase != Phase::Run {
            t += 0.05;
            r.tick(t);
        }
        // the same list again: nothing
        assert!(r.set_ifaces(t, vec![eth([169, 254, 7, 9])]).is_empty());
        assert_eq!(r.state(), State::Running);
        // DHCP after the link-local address: a goodbye for the old address, then probing
        let m = one(&r.set_ifaces(t, vec![eth([192, 168, 1, 20])]));
        assert!(m.an.iter().all(|a| a.ttl == 0) && m.an.iter().any(|a| a.canon == vec![169, 254, 7, 9]));
        assert_eq!(r.state(), State::Probing);
        // a WLAN stick: both interfaces, each with its own address
        let mut w = eth([10, 0, 0, 5]);
        (w.index, w.name) = (5, "wlan0".into());
        w.v6 = vec!["fe80::1234".parse().unwrap()];
        r.set_ifaces(t, vec![w, eth([192, 168, 1, 20])]);
        let mut outs = Vec::new();
        while r.phase != Phase::Run {
            t += 0.05;
            outs.extend(r.tick(t));
        }
        for (i, d) in [(5, Dest::Group4), (5, Dest::Group6), (ETH, Dest::Group4)] {
            assert!(outs.iter().any(|o| o.iface == i && o.dest == d), "{} {:?}", i, d);
        }
        assert!(!outs.iter().any(|o| o.iface == ETH && o.dest == Dest::Group6), "eth0 has no IPv6 address");
        let m = one(&r.receive(
            t + 2.0,
            &query(0, &[(&["dho924s-123456", "local"], T_ANY, false)]),
            SocketAddr::from(([10, 0, 0, 9], PORT)),
            5,
            true,
        ));
        assert!(m.an.iter().any(|a| a.rtype == T_A && a.canon == vec![10, 0, 0, 5]));
        assert!(!m.an.iter().any(|a| a.rtype == T_AAAA), "the services have no IPv6");
        assert!(!m.an.iter().any(|a| a.canon == vec![192, 168, 1, 20]), "only the address of the interface asked on");
        // all interfaces gone: waiting for one
        r.set_ifaces(t + 3.0, Vec::new());
        assert_eq!(r.state(), State::NoNetwork);
    }

    // net.ipv6 = on: AAAA for every IPv6 address of the interface asked on, no NSEC for it, a SRV brings both families
    #[test]
    fn aaaa_when_the_services_listen_on_ipv6() {
        let mut r = Responder::new(7);
        r.set_config(0.0, Config { v6: true, ..cfg() });
        let mut e = eth([192, 168, 1, 20]);
        let (ll, global): (Ipv6Addr, Ipv6Addr) =
            ("fe80::0:ff:fe00:1".parse().unwrap(), "2001:db8:5:7:1d2e:3f4a:5b6c:7d8e".parse().unwrap());
        e.v6 = vec![ll, global];
        r.set_ifaces(0.0, vec![e]);
        let mut t = 0.0;
        while r.phase != Phase::Run {
            t += 0.05;
            r.tick(t);
        }
        let from6 = SocketAddr::from((Ipv6Addr::from(0xfe80_0000_0000_0000_0000_0000_0000_0099u128), PORT));
        let m = one(&r.receive(t + 1.0, &query(0, &[(&["dho924s-123456", "local"], T_AAAA, false)]), from6, ETH, true));
        let got: Vec<Vec<u8>> = m.an.iter().filter(|a| a.rtype == T_AAAA).map(|a| a.canon.clone()).collect();
        assert_eq!(got, vec![ll.octets().to_vec(), global.octets().to_vec()]);
        assert!(m.an.iter().all(|a| a.flush) && !m.ar.iter().any(|r| r.rtype == T_NSEC));
        assert!(has(&m.ar, "dho924s-123456.local", T_A), "the other family as additional");
        let m =
            one(&r.receive(t + 2.0, &query(0, &[(&["_http", "_tcp", "local"], T_PTR, false)]), peer(PORT), ETH, true));
        assert!(has(&m.ar, "dho924s-123456.local", T_A) && has(&m.ar, "dho924s-123456.local", T_AAAA));
    }

    #[test]
    fn a_long_announcement_is_split() {
        let mut r = Responder::new(1);
        let mut c = cfg();
        for i in 0..20 {
            c.services.push(Service {
                kind: format!("_x{}._tcp", i),
                port: 1000 + i,
                txt: vec!["k=".to_string() + &"v".repeat(200)],
            });
        }
        r.set_config(0.0, c);
        r.set_ifaces(0.0, vec![eth([192, 168, 1, 20])]);
        let mut pkts = Vec::new();
        let mut t = 0.0;
        while r.phase != Phase::Run {
            t += 0.05;
            pkts.extend(r.tick(t).into_iter().filter(|o| parse(&o.pkt).unwrap().is_response()));
        }
        assert!(pkts.len() > 2, "{}", pkts.len());
        let total: usize = pkts.iter().map(|o| parse(&o.pkt).unwrap().an.len()).sum();
        assert_eq!(total, 2 * r.records(&eth([192, 168, 1, 20])).len());
        assert!(pkts.iter().all(|o| o.pkt.len() <= MAX_PKT + 300));
    }

    /// On a host: the responder on the interfaces in OOS_MDNS_IF (default lo0, or lo on Linux) with a test name, asked
    /// like `dig @224.0.0.251 -p 5353` would. With OOS_MDNS_HOLD=<s> it keeps running for `dns-sd -B _http._tcp` or
    /// `dns-sd -G v4v6 <name>.local` by hand. (On macOS a legacy query from the same host only comes back on lo0: on
    /// en0 the kernel does not loop it to the socket, so the answer check needs lo0 there; mDNSResponder's own
    /// queries and the announcements do arrive on en0. Verified 2026-11-13 with dns-sd -G and -B.)
    #[test]
    #[ignore]
    fn host_responder() {
        let lo = if cfg!(target_os = "linux") { "lo" } else { "lo0" };
        let only: Vec<String> =
            std::env::var("OOS_MDNS_IF").unwrap_or(lo.into()).split(',').map(str::to_string).collect();
        let ifs = net::interfaces(Some(&only));
        assert!(!ifs.is_empty(), "no interface {:?}", only);
        let name = format!("oos-test-{}", std::process::id());
        let mut m = Mdns::new(Some(only));
        let mut c = cfg();
        c.name = name.clone();
        c.instance = format!("OOS test ({})", std::process::id());
        m.set(c);
        std::thread::sleep(std::time::Duration::from_millis(2500));
        assert_eq!(status().state, State::Running, "{:?}", status());
        let s = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        let (a, _) = ifs[0].v4.expect("an IPv4 address");
        let ia = libc::in_addr { s_addr: u32::from(a).to_be() };
        use std::os::fd::AsRawFd;
        // SAFETY: a pointer to an in_addr of the size given
        unsafe {
            libc::setsockopt(
                s.as_raw_fd(),
                libc::IPPROTO_IP,
                libc::IP_MULTICAST_IF,
                &ia as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::in_addr>() as libc::socklen_t,
            )
        };
        s.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        s.send_to(&query(0x1234, &[(&[&name, "local"], T_A, false)]), (GROUP4, PORT)).unwrap();
        let mut buf = [0u8; 1500];
        match s.recv_from(&mut buf) {
            Ok((n, from)) => {
                let ans = parse(&buf[..n]).unwrap();
                println!("answer from {}: {:?}", from, ans);
                assert_eq!(ans.id, 0x1234);
                assert!(ans.an.iter().any(|r| r.rtype == T_A && r.canon == a.octets().to_vec()));
            }
            Err(e) if a.is_loopback() => panic!("no answer: {}", e),
            Err(e) => println!("no legacy answer on {} ({}): see above, check with dns-sd", ifs[0].name, e),
        }
        if let Some(hold) = std::env::var("OOS_MDNS_HOLD").ok().and_then(|s| s.parse::<u64>().ok()) {
            println!("{}.local for {} s", name, hold);
            std::thread::sleep(std::time::Duration::from_secs(hold));
        }
        m.stop();
        assert_eq!(status().state, State::Off);
    }
}
