// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hostile input for the parsers of oos-hal that read what comes from outside the program (ADR 0047): the files of
//! the unit (calibration, `vendor.bin`, `limits.conf`), the answers of wpa_supplicant, and the update (in
//! `update/hostile_tests.rs`, next to the builders of valid packages). Each parser has one test: valid inputs,
//! mutated from a fixed seed (`gen.rs`, which dsp and ui take too), go in; it must return, not panic, and what
//! it accepts must obey the contract the test states. `cargo test hostile` runs them all, `OOS_HOSTILE_ITERS` raises
//! the number of inputs. A new parser of outside input comes with its test. A finding is not fixed in the test: its
//! smallest input becomes an ignored test, and `KNOWN` names it so the run steps over that one case until the fix is
//! merged.

mod gen;

pub(crate) use gen::*;

/// The findings that still stand, each with its smallest input as an ignored test. The list only shrinks: a fix
/// removes its line and the `#[ignore]` of its test (`hostile_known_findings_still_stand` fails until it does).
pub(crate) const KNOWN: &[Known] = &[
    Known {
        name: "fdt-nesting-not-bounded",
        what: "update::fdt takes a device tree nested any deep: 30 000 levels overflow the stack and end the process",
        repro: crate::update::hostile_tests::hostile_known_fdt_nesting_not_bounded,
    },
    Known {
        name: "rsce-table-not-bounded",
        what: "update::fdt::rsce_file takes a table longer than its container: entries of no blocks are read up to \
               2^32 times",
        repro: crate::update::hostile_tests::hostile_known_rsce_table_not_bounded,
    },
    Known {
        name: "unhex-takes-a-plus-sign",
        what: "update::hash::unhex takes a plus sign for a hex digit: '+f' is the byte 0x0f",
        repro: crate::update::hostile_tests::hostile_known_unhex_takes_a_plus_sign,
    },
];

// Break it catches: a finding that was fixed and is still stepped over
#[test]
fn hostile_known_findings_still_stand() {
    still_stand(KNOWN);
}

/// `calfile::parse`, `calfile::stamp`, `boot::cal::payload`, `DdrCal::from_file_bytes` and `CalAdc::from_file_bytes`
/// (the calibration files of `/rigol/data` and of OOS's own data partition): return for any bytes; a payload taken
/// lies inside the file, has the length the header names and the checksum it names; a DDR calibration is 48 bytes,
/// an ADC calibration 0x738.
#[test]
fn hostile_calibration_files() {
    use crate::dho::{adc::CalAdc, boot::cal, calfile};
    let files = [
        calfile::build(&[0x11; 48], (2026, 11, 5, 16, 50, 12)),
        calfile::build(&(0..0x738).map(|k| k as u8).collect::<Vec<u8>>(), (2026, 1, 2, 3, 4, 5)),
        calfile::build(&[], (0, 0, 0, 0, 0, 0)),
    ];
    let seeds: Vec<&[u8]> = files.iter().map(|f| f.as_slice()).collect();
    run("dho::calfile::parse", &seeds, 1, |b| {
        let said = b.get(20..24).map(|l| u32::from_le_bytes([l[0], l[1], l[2], l[3]]) as usize);
        for (what, p) in [("calfile::parse", calfile::parse(b).ok()), ("cal::payload", cal::payload(b))] {
            let Some(p) = p else { continue };
            if Some(p.len()) != said || b.len() < 28 + p.len() || p != &b[28..28 + p.len()] {
                return Err(format!("{what}: a payload of {} bytes, the header says {said:?}", p.len()));
            }
            if crate::crc32(p).to_le_bytes() != b[24..28] {
                return Err(format!("{what}: a payload whose checksum is not the header's"));
            }
        }
        if calfile::parse(b).is_ok() && calfile::stamp(b).is_none() {
            return Err("a container without its stamp".into());
        }
        if cal::DdrCal::from_file_bytes(b).is_some() && said != Some(48) {
            return Err(format!("a DDR calibration of {said:?} bytes"));
        }
        if CalAdc::from_file_bytes(b).is_some_and(|c| c.payload().len() != 0x738) {
            return Err("an ADC calibration of another length".into());
        }
        Ok(())
    });
}

/// `model::decode_vendor` and `Platform::build` (`vendor.bin` of the unit, `/oos/limits.conf` and the limits file of
/// the image): return for any bytes and text; a vendor file is taken only whole (a multiple of four bytes with its
/// checksum); the platform has 2 or 4 channels, and every limit is a number above zero.
#[test]
fn hostile_vendor_and_limits() {
    use crate::model::{decode_vendor, encode_vendor, Platform};
    let vendor = encode_vendor(&[(5, "DHO924S"), (7, "DHO8A000000000"), (9, "02:00:00:00:21"), (11, "")]);
    let limits =
        "# limits\nmodel = DHO814\nchannels = 4\nafg = yes\nla = off\nmem.1ch = 50_000_000\nmem.la_4ch = 1e6\n\
                  adc.clock_hz = 1.25e9\nafg.sine_max_hz = 7e7\nafg.arb_max_hz = 1e6\next_trig = 0\n";
    let seeds: [&[u8]; 2] = [&vendor, limits.as_bytes()];
    run("model::Platform::build", &seeds, 2, |b| {
        if let Ok(v) = decode_vendor(b) {
            if b.len() < 8 || b.len() % 4 != 0 || v.items.len() > b.len() / 12 {
                return Err(format!("{} items from a vendor file of {} bytes", v.items.len(), b.len()));
            }
        }
        let text = String::from_utf8_lossy(b);
        for p in [
            Platform::build(Some(b), Some(&text), None),
            Platform::build_unit(None, Some(&text), Some(&text), Some(&text)),
        ] {
            let l = &p.limits;
            let afg = &l.afg_max_hz;
            let hz =
                [l.adc_clock_hz, l.afg_clock_hz, afg.sine, afg.square, afg.ramp, afg.pulse, afg.noise, afg.dc, afg.arb];
            if !matches!(p.caps.channels, 2 | 4) || hz.iter().chain(&l.max_srate).any(|x| !(x.is_finite() && *x > 0.0))
            {
                return Err(format!("{} channels with the limits {l:?}", p.caps.channels));
            }
            if l.max_mem.iter().chain(&l.max_mem_la).any(|&m| m < 1000) {
                return Err(format!("a memory depth below 1000 points: {l:?}"));
            }
        }
        Ok(())
    });
}

/// `wlan::parse_scan`, `parse_networks`, `parse_status`, `parse_signal` and `unescape` (the answers of
/// wpa_supplicant, with the names of the networks in the air): return for any text; a network of a scan has a name
/// of 1..=32 bytes that is not all zeros, each name once, the strongest first; an unescaped name is no longer than
/// its text.
#[test]
fn hostile_wlan_answers() {
    use crate::wlan;
    let scan = "bssid / frequency / signal level / flags / ssid\n\
                02:00:00:00:01\t2437\t-48\t[WPA2-PSK-CCMP][ESS]\tlab\n\
                02:00:00:00:02\t5180\t-60\t[WPA2-PSK-CCMP][ESS]\tlab\n\
                02:00:00:00:03\t2412\t-71\t[ESS]\tcaf\\xc3\\xa9 \\\"guest\\\"\\t\\\\\n\
                02:00:00:00:04\t2462\t-80\t[WPA2-SAE-CCMP][ESS]\t\n";
    let networks = "network id / ssid / bssid / flags\n0\tlab\tany\t[CURRENT]\n1\told\\x00net\tany\t[TEMP-DISABLED]\n";
    let status = "bssid=02:00:00:00:01\nfreq=2437\nssid=lab\nwpa_state=COMPLETED\nip_address=192.0.2.44\n\
                  address=02:00:00:00:10\nRSSI=-48\nLINKSPEED=72\n";
    let seeds: [&[u8]; 3] = [scan.as_bytes(), networks.as_bytes(), status.as_bytes()];
    run("wlan::parse_scan", &seeds, 1, |b| {
        let text = String::from_utf8_lossy(b);
        let nets = wlan::parse_scan(&text);
        for (k, n) in nets.iter().enumerate() {
            if n.ssid.is_empty() || n.ssid.len() > wlan::SSID_MAX || n.ssid.iter().all(|c| *c == 0) {
                return Err(format!("a network named {:?}", n.ssid));
            }
            if nets[..k].iter().any(|o| o.ssid == n.ssid || o.signal < n.signal) {
                return Err(format!("{:?} twice or after a weaker one", wlan::name(&n.ssid)));
            }
            if wlan::name(&n.ssid).chars().any(char::is_control) {
                return Err("a name for the screen with a control character".into());
            }
        }
        if wlan::parse_networks(&text).len() > text.lines().count() || wlan::unescape(&text).len() > text.len() {
            return Err("more out than in".into());
        }
        let s = wlan::parse_status(&text);
        if s.connected() != (s.state == "COMPLETED") || s.ssid.len() > text.len() {
            return Err(format!("the status {s:?}"));
        }
        let _ = wlan::parse_signal(&text);
        Ok(())
    });
}

/// `dbus::Message::decode`, `dbus::total_len` and `bluez::objects` (what bluetoothd sends on the system bus: replies,
/// errors, the agent's calls and the managed objects): return for any bytes; a length is at most `dbus::MAX_LEN`; a
/// message that decodes encodes to one that decodes the same (the sender is not sent back); every device of the
/// objects has its path.
#[test]
fn hostile_dbus_messages() {
    use crate::bluez;
    use crate::dbus::{self, Message, Value};
    let s = |t: &str| Value::Str(t.into());
    let var = |v: Value| Value::Variant(Box::new(v));
    let dict = |kv: Vec<(&str, Value)>| {
        Value::Array(
            "{sv}".into(),
            kv.into_iter().map(|(k, v)| Value::Entry(Box::new(s(k)), Box::new(var(v)))).collect(),
        )
    };
    let ifaces = |name: &str, props: Value| {
        Value::Array("{sa{sv}}".into(), vec![Value::Entry(Box::new(s(name)), Box::new(props))])
    };
    let objects = Value::Array(
        "{oa{sa{sv}}}".into(),
        vec![
            Value::Entry(
                Box::new(Value::Path("/org/bluez/hci0".into())),
                Box::new(ifaces(
                    "org.bluez.Adapter1",
                    dict(vec![("Alias", s("scope")), ("Powered", Value::Bool(true))]),
                )),
            ),
            Value::Entry(
                Box::new(Value::Path("/org/bluez/hci0/dev_1".into())),
                Box::new(ifaces(
                    "org.bluez.Device1",
                    dict(vec![("Name", s("Keys")), ("Icon", s("input-keyboard")), ("RSSI", Value::I16(-60))]),
                )),
            ),
        ],
    );
    let mut reply = Message::call("org.bluez", "/", "org.freedesktop.DBus.ObjectManager", "GetManagedObjects", vec![]);
    reply.kind = 2;
    reply.reply_serial = Some(3);
    reply.body = vec![objects];
    let mut call = Message::call("x", "/oos/agent", "org.bluez.Agent1", "DisplayPasskey", vec![]);
    call.body = vec![Value::Path("/org/bluez/hci0/dev_1".into()), Value::U32(123456), Value::U16(2)];
    let mut error = Message::error(&call, "org.bluez.Error.Failed", "no");
    error.serial = 9;
    let seeds: Vec<Vec<u8>> = [reply, call, error].iter().map(Message::encode).collect();
    let seeds: Vec<&[u8]> = seeds.iter().map(Vec::as_slice).collect();
    run("dbus::Message::decode", &seeds, 1, |b| {
        if let Ok(head) = <[u8; 16]>::try_from(b.get(..16).unwrap_or_default()) {
            if dbus::total_len(&head).is_ok_and(|n| n > dbus::MAX_LEN) {
                return Err("a length over MAX_LEN".into());
            }
        }
        let Ok(m) = Message::decode(b) else { return Ok(()) };
        let again = Message::decode(&m.encode()).map_err(|e| format!("decoded, encoded and refused: {e}"))?;
        if again != (Message { sender: None, ..m.clone() }) {
            return Err(format!("decoded, encoded and decoded otherwise: {m:?} -> {again:?}"));
        }
        let (_, devices) = bluez::objects(&m.body);
        if devices.iter().any(|d| d.path.is_empty()) {
            return Err("a device without its path".into());
        }
        Ok(())
    });
}
