// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hostile input for the readers of settings a person, a stick or the network writes: `system.conf`, `net.allow`,
//! time zone strings (also from a DHCP lease), a date typed on the screen, public keys from a stick, JSON of MCP.

use super::run;
use crate::clock::{Local, Tz};
use crate::netacl::{canonical, Acl};
use crate::sysconf::{server_ok, SysConf, DHCP};

/// `SysConf::parse` (`/oos/system.conf` and the image's defaults): returns for any text; what it takes is in range
/// (backlight 10..=100, a server name `server_ok` takes, a time zone `Tz::parse` takes, a name `mdns::name_ok`
/// takes), and written out and read again it is the same settings with nothing refused.
#[test]
fn hostile_system_conf() {
    let mut full = SysConf::default();
    (full.ntp, full.ntp_server, full.tz) = (true, "192.0.2.7".into(), "CET-1CEST,M3.5.0,M10.5.0/3".into());
    (full.net_name, full.backlight, full.remote_ssh) = ("lab-scope-3".into(), 40, true);
    full.features_off = vec!["bode".into(), "eye".into()];
    let files = [
        SysConf::default().text(),
        full.text()
            + "net.allow = 192.0.2.0/24 2001:db8::/32 onlink6\neth.mac = keep  # a comment\ntz.fallback = JST-9\n",
    ];
    let seeds: Vec<&[u8]> = files.iter().map(|f| f.as_bytes()).collect();
    run("sysconf::parse", &seeds, 1, |b| {
        let text = String::from_utf8_lossy(b);
        let (c, bad) = SysConf::parse(&text);
        if bad.len() > text.lines().count() {
            return Err(format!("{} problems in {} lines", bad.len(), text.lines().count()));
        }
        if !(10..=100).contains(&c.backlight) {
            return Err(format!("took the backlight {}", c.backlight));
        }
        if c.ntp_server != DHCP && !server_ok(&c.ntp_server) {
            return Err(format!("took the server {:?}", c.ntp_server));
        }
        for tz in [&c.tz, &c.tz_fallback] {
            if tz != DHCP && Tz::parse(tz).is_err() {
                return Err(format!("took the time zone {tz:?}"));
            }
        }
        if !c.net_name.is_empty() && !crate::mdns::name_ok(&c.net_name) {
            return Err(format!("took the name {:?}", c.net_name));
        }
        // a line this version does not know is kept as it is, but for a carriage return at its end
        let written = c.text().replace("\r\n", "\n");
        let (again, bad) = SysConf::parse(&written);
        if !bad.is_empty() || again.text() != written {
            return Err(format!(
                "written out and read again it differs: {bad:?}\n  first:  {c:?}\n  second: {again:?}"
            ));
        }
        // the one rule for a value: the last line of a key, without its comment
        if let Some(v) = crate::sysconf::value(&text, "net.allow") {
            if v != v.trim() || v.contains('\n') {
                return Err(format!("a value with space around it: {v:?}"));
            }
        }
        Ok(())
    });
}

// Break it catches: a word of a list that becomes a comment once the list is written out (#181)
#[test]
fn hostile_sysconf_list_word_with_hash() {
    let (c, _) = SysConf::parse("features.off =#eye bode\n");
    assert_eq!(SysConf::parse(&c.text()).0.features_off, c.features_off, "written as {:?}", c.text());
    // the word is left out and named, the others stand: the file still loads
    let (c, bad) = SysConf::parse("features.off =#eye bode\ndecoders.off = Uart\u{3000}#x Spi\n");
    assert_eq!((c.features_off, c.decoders_off), (vec!["bode".to_string()], vec!["Uart".to_string(), "Spi".into()]));
    assert_eq!(bad.len(), 2, "{bad:?}");
}

/// `Acl::parse`, `Acl::from_value` and `canonical` (`net.allow`: who may send commands and updates): return for any
/// text; every word is taken or named as bad; a prefix taken has at most 32 or 128 bits and no bit set below them;
/// the list written out reads as the same list; a value of which nothing parses does not open the scope; what
/// `canonical` gives is a list `canonical` gives again unchanged.
#[test]
fn hostile_net_allow() {
    let seeds: [&[u8]; 4] = [
        b"192.0.2.0/24 198.51.100.7, 2001:db8:5::/48\tonlink6",
        b"203.0.113.9/32",
        b"::ffff:192.0.2.1/128 fe80::/10 0.0.0.0/0",
        b"",
    ];
    run("netacl::parse", &seeds, 1, |b| {
        let text = String::from_utf8_lossy(b);
        let words: Vec<&str> = text.split([' ', ',', '\t']).filter(|w| !w.is_empty()).collect();
        let onlink = words.iter().filter(|w| **w == crate::netacl::ONLINK6).count();
        let (acl, bad) = Acl::parse(&text);
        let shown = acl.to_string();
        let taken = shown.split(' ').filter(|w| !w.is_empty() && *w != crate::netacl::ONLINK6).count();
        if taken + bad.len() + onlink != words.len() || bad.iter().any(|w| !words.contains(&w.as_str())) {
            return Err(format!(
                "{} words gave {taken} prefixes, {onlink} onlink6 and the bad words {bad:?}",
                words.len()
            ));
        }
        for w in shown.split(' ').filter(|w| !w.is_empty() && *w != crate::netacl::ONLINK6) {
            let Some((a, n)) = w.split_once('/') else { return Err(format!("a prefix without its length: {w}")) };
            let (Ok(ip), Ok(n)) = (a.parse::<std::net::IpAddr>(), n.parse::<u32>()) else {
                return Err(format!("not a prefix: {w}"));
            };
            let (bits, max) = match ip {
                std::net::IpAddr::V4(a) => (u32::from(a) as u128, 32),
                std::net::IpAddr::V6(a) => (u128::from(a), 128),
            };
            if n > max || (n < max && bits << (128 - max + n) != 0) {
                return Err(format!("a prefix with bits below its length: {w}"));
            }
        }
        let (again, bad_again) = Acl::parse(&shown);
        if again != acl || !bad_again.is_empty() {
            return Err(format!("{shown:?} read again is {again} with the bad words {bad_again:?}"));
        }
        let (forced, bad) = Acl::from_value(&text);
        if forced.is_open() && !bad.is_empty() {
            return Err(format!("a list with the bad words {bad:?} opens the scope to everyone"));
        }
        if let Ok(c) = canonical(&text) {
            if canonical(&c).as_ref() != Ok(&c) || !Acl::parse(&c).1.is_empty() {
                return Err(format!("canonical gave {c:?}, which it does not give again"));
            }
        }
        Ok(())
    });
}

/// `Tz::parse` (the `tz` of system.conf and of a DHCP lease, option 100): returns for any text; a zone it takes
/// gives, for any time, an offset within 25 hours of UTC and a local time that is a date of the calendar, and the
/// local time brought back is within two days of where it started.
#[test]
fn hostile_time_zones() {
    let zones: Vec<&[u8]> = crate::clock::ZONES.iter().map(|z| z.1.as_bytes()).collect();
    let more: [&[u8]; 4] =
        [b"<+0545>-5:45", b"EST5EDT", b"AAA-24BBB-24,J365/167,0/-167", b"WGT3WGST,M3.5.0/-2,M10.5.0/-1"];
    let seeds: Vec<&[u8]> = zones.into_iter().chain(more).collect();
    run("clock::Tz::parse", &seeds, 1, |b| {
        let Ok(tz) = Tz::parse(&String::from_utf8_lossy(b)) else { return Ok(()) };
        // through a year in steps of 17 days and an odd number of seconds, and at the ends of the 32-bit range
        let year = (0..22).map(|k| 1_794_355_200 + k * 17 * 86_400 + k * 3_601);
        for unix in year.chain([0, -1, i32::MAX as i64, i32::MIN as i64, 4_102_444_799]) {
            let (off, _) = tz.offset_at(unix);
            if off.abs() > 25 * 3600 {
                return Err(format!("an offset of {off} s at {unix}"));
            }
            let l = tz.local(unix);
            let days = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            if !(1..=12).contains(&l.mo)
                || l.d == 0
                || l.d > days[l.mo as usize - 1]
                || l.h > 23
                || l.mi > 59
                || l.s > 59
            {
                return Err(format!("the local time {l:?} at {unix}"));
            }
            let back = tz.to_unix(&l);
            if (back - unix).abs() > 2 * 86_400 {
                return Err(format!("{unix} is {l:?}, which is {back}"));
            }
            if tz.abbr_at(unix).len() < 3 {
                return Err(format!("the abbreviation {:?}", tz.abbr_at(unix)));
            }
        }
        Ok(())
    });
}

/// `Local::parse` (the date and time typed under System > Date and time): returns for any text; a time it takes
/// is a date of the calendar in 2000..=2099 and reads the same when written out.
#[test]
fn hostile_dates() {
    let seeds: [&[u8]; 4] = [b"2026-11-11 14:30", b"2028-02-29T23:59:59", b" 2000-01-01 00:00:00 ", b"2099-12-31 7:5"];
    run("clock::Local::parse", &seeds, 1, |b| {
        let Ok(l) = Local::parse(&String::from_utf8_lossy(b)) else { return Ok(()) };
        if !(2000..=2099).contains(&l.y) || Local::parse(&l.text()) != Ok(l) {
            return Err(format!("took {l:?}, written {:?}", l.text()));
        }
        // a date of the calendar: the day after the day before is the same day
        let day = crate::clock::days_from_civil(l.y, l.mo, l.d);
        if crate::clock::civil_from_days(day) != (l.y, l.mo, l.d) {
            return Err(format!("{l:?} is no day of the calendar"));
        }
        Ok(())
    });
}

/// An SSH wire string: the length, then the bytes.
fn wire(b: &[u8]) -> Vec<u8> {
    let mut v = (b.len() as u32).to_be_bytes().to_vec();
    v.extend_from_slice(b);
    v
}

/// An `authorized_keys` line of made-up key material: `kind`, then the parts of its blob.
fn key_line(kind: &str, parts: &[&[u8]], comment: &str) -> String {
    let mut blob = wire(kind.as_bytes());
    for p in parts {
        blob.extend(wire(p));
    }
    format!("{} {} {}", kind, crate::http::base64(&blob), comment)
}

/// `ssh::parse_line` and `Keys::import` (public keys from a stick, the trusted keys file): return for any text; a
/// key they take names one of the known types in its blob too, an RSA key has at least 2048 bits, and the line
/// written into the trusted keys file reads as the same key.
#[test]
fn hostile_public_keys() {
    let modulus: Vec<u8> = std::iter::once(0).chain((0..256).map(|k| 0x80 | k as u8)).collect();
    let files = [
        key_line("ssh-ed25519", &[&[7; 32]], "tester@host"),
        key_line("ssh-rsa", &[&[1, 0, 1], &modulus], "old key"),
        format!("from=\"192.0.2.0/24\",command=\"echo a b\" {}", key_line("ssh-ed25519", &[&[9; 32]], "")),
        format!("# keys\n\n{}\n{}\n", key_line("ecdsa-sha2-nistp256", &[b"nistp256", &[4; 65]], "a b c"), "not a key"),
    ];
    let seeds: Vec<&[u8]> = files.iter().map(|f| f.as_bytes()).collect();
    run("ssh::parse_line", &seeds, 1, |b| {
        let text = String::from_utf8_lossy(b);
        for line in text.lines() {
            let Ok(k) = crate::ssh::parse_line(line) else { continue };
            let named = k.blob.get(4..4 + k.kind.len());
            if named != Some(k.kind.as_bytes()) || k.short_kind() == "key" {
                return Err(format!("took a key of type {:?} whose data names {:?}", k.kind, named));
            }
            if k.kind == "ssh-rsa" && k.blob.len() < 4 + 7 + 4 + 4 + 256 {
                return Err(format!("took an RSA key of {} bytes", k.blob.len()));
            }
            if k.fingerprint().len() != 50 || k.label().is_empty() {
                return Err(format!("the fingerprint {:?}", k.fingerprint()));
            }
        }
        let mut keys = crate::ssh::Keys::default();
        let r = keys.import(&text);
        let kept = keys.keys();
        if kept.len() != r.added || r.added > crate::ssh::KEYS_MAX || kept.iter().any(|k| k.options) {
            return Err(format!("{} keys kept after {:?}", kept.len(), r));
        }
        if crate::ssh::Keys::parse(&keys.text()).keys() != kept {
            return Err("the trusted keys file reads as other keys than were added".into());
        }
        Ok(())
    });
}

// Break it catches: the bits of an RSA modulus counted from its first byte when it has none (#181)
#[test]
fn hostile_ssh_rsa_empty_modulus() {
    // in a build without overflow checks the key is taken as one of 2^64 bits
    assert!(crate::ssh::parse_line(&key_line("ssh-rsa", &[&[1, 0, 1], &[]], "no modulus")).is_err());
}

/// `json::parse` (the body of an MCP request): returns for any text; a value it takes is written as JSON it takes
/// again, and that is written the same way.
#[test]
fn hostile_json() {
    let seeds: [&[u8]; 3] = [
        br#"{"id":3,"method":"tools/call","params":{"name":"scpi","arguments":{"command":":TIM:SCAL 1e-3"}}}"#,
        br#"[true,null,-1.5e3,"a\"\u00e9\ud83d\ude00\n",{"a":[],"b":{}},0.1,12345678901234567890]"#,
        b" \"text\" ",
    ];
    run("json::parse", &seeds, 1, |b| {
        let Some(v) = crate::json::parse(&String::from_utf8_lossy(b)) else { return Ok(()) };
        let written = v.to_string();
        match crate::json::parse(&written) {
            Some(again) if again.to_string() == written => Ok(()),
            Some(again) => Err(format!("written as {written:?}, read and written again as {:?}", again.to_string())),
            None => Err(format!("written as {written:?}, which is refused")),
        }
    });
}
