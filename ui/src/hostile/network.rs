// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hostile input for what arrives in frames: VXI-11 (ONC RPC records), SNTP answers, the answers of a USBTMC
//! device and the frames of the front-panel MCU. HiSLIP, mDNS and HTTP are tested in their own modules, where their
//! private parts are.

use super::run;
use crate::board::Board;
use crate::vxi11::{core, parse_call, portmap, take_record, Link, CORE_PROG, PMAP_PROG};

/// One RPC call as a record: the last-fragment mark and the length, the call header, the arguments.
fn rpc(xid: u32, prog: u32, proc_: u32, args: &[u32], data: &[u8]) -> Vec<u8> {
    let mut m = Vec::new();
    for v in [xid, 0, 2, prog, 1, proc_, 0, 0, 0, 0].iter().chain(args) {
        m.extend_from_slice(&v.to_be_bytes());
    }
    if !data.is_empty() {
        m.extend_from_slice(&(data.len() as u32).to_be_bytes());
        m.extend_from_slice(data);
        m.resize(m.len().next_multiple_of(4), 0);
    }
    let mut rec = (0x8000_0000 | m.len() as u32).to_be_bytes().to_vec();
    rec.extend(m);
    rec
}

/// `take_record`, `parse_call`, `portmap` and `core` (VXI-11 on TCP 111 and its core port): return for any bytes;
/// a record taken is no longer than the bytes it came from and leaves the buffer shorter; an answer is whole XDR
/// words and starts with the call's number; there are never more than 16 links.
#[test]
fn hostile_vxi11_records() {
    let two_fragments = {
        let whole = rpc(9, CORE_PROG, 11, &[0, 1000, 1000, 8], b"*IDN?\n");
        let body = &whole[4..];
        let mut b = (12u32).to_be_bytes().to_vec();
        b.extend_from_slice(&body[..12]);
        b.extend_from_slice(&(0x8000_0000 | (body.len() - 12) as u32).to_be_bytes());
        b.extend_from_slice(&body[12..]);
        b
    };
    let session: Vec<u8> = [
        rpc(1, PMAP_PROG, 3, &[CORE_PROG, 1, 6, 0], b""),
        rpc(2, CORE_PROG, 10, &[77, 0, 1000], b"inst0"),
        rpc(3, CORE_PROG, 11, &[0, 1000, 1000, 8], b":CHAN1:SCAL 0.5;:CHAN1:SCAL?\n"),
        rpc(4, CORE_PROG, 12, &[0, 1024, 1000, 1000, 0, 10], b""),
        rpc(5, CORE_PROG, 15, &[0, 0, 1000, 1000], b""),
        rpc(6, CORE_PROG, 23, &[0], b""),
    ]
    .concat();
    let read = rpc(7, CORE_PROG, 12, &[0, 1, 1000, 1000, 0, 10], b"");
    let seeds: [&[u8]; 3] = [&session, &two_fragments, &read];
    run("vxi11::core", &seeds, 1, |b| {
        let mut buf = b.to_vec();
        // one link is open, with an answer waiting
        let mut links = vec![Some(Link::default())];
        if let Some(l) = links[0].as_mut() {
            l.out.extend_from_slice(b"OOS,unit\n");
        }
        let mut exec = |c: &str| if c.ends_with('?') { b"1".to_vec() } else { Vec::new() };
        loop {
            let before = buf.len();
            let Some(rec) = take_record(&mut buf) else { break };
            if buf.len() + 4 > before || rec.len() + buf.len() > before {
                return Err(format!("a record of {} bytes left {} of {} bytes", rec.len(), buf.len(), before));
            }
            let Some(c) = parse_call(&rec) else { continue };
            for reply in [portmap(&c), core(&c, &mut links, &mut exec)] {
                if reply.len() % 4 != 0 || reply.len() < 24 || reply[..4] != c.xid.to_be_bytes() {
                    return Err(format!("the answer {reply:02x?} to call {}", c.xid));
                }
            }
            if links.len() > 16 {
                return Err(format!("{} links", links.len()));
            }
        }
        if buf.len() > b.len() {
            return Err("the buffer grew".into());
        }
        Ok(())
    });
}

/// `sntp::parse` (the answer of a time server, UDP): returns for any bytes; an answer it takes is at least 48
/// bytes from a server (mode 4, version 1..=4) to this request, of stratum 1..=15, synchronised, with a transmit
/// time, and both its times are numbers.
#[test]
fn hostile_sntp_answers() {
    let t1 = crate::sntp::to_ntp(1_356_998_400.0);
    let mut answer = [0u8; 48];
    (answer[0], answer[1]) = (4 << 3 | 4, 2);
    answer[12..16].copy_from_slice(b"GPS\0");
    answer[24..32].copy_from_slice(&t1.to_be_bytes());
    answer[32..40].copy_from_slice(&crate::sntp::to_ntp(1_794_398_400.010).to_be_bytes());
    answer[40..48].copy_from_slice(&crate::sntp::to_ntp(1_794_398_400.011).to_be_bytes());
    let mut kiss = answer;
    kiss[1] = 0;
    kiss[12..16].copy_from_slice(b"RATE");
    let mut long = answer.to_vec();
    long.extend_from_slice(&[0; 20]);
    let seeds: [&[u8]; 3] = [&answer, &kiss, &long];
    run("sntp::parse", &seeds, 1, |b| match crate::sntp::parse(b, t1) {
        Ok(r) => {
            let header = b.len() >= 48 && b[0] & 7 == 4 && (1..=4).contains(&(b[0] >> 3 & 7)) && b[0] >> 6 != 3;
            if !header || b[24..32] != t1.to_be_bytes() || b[40..48] == [0; 8] || !(1..=15).contains(&r.stratum) {
                return Err(format!("took an answer of stratum {} that is none to this request", r.stratum));
            }
            if !(r.t2.is_finite() && r.t3.is_finite()) || r.t3 < -2_208_988_800.0 || r.t3 > 4_294_967_296.0 {
                return Err(format!("took the times {} and {}", r.t2, r.t3));
            }
            Ok(())
        }
        Err(e) if e.is_empty() => Err("refused without a message".into()),
        Err(_) => Ok(()),
    });
}

/// A USBTMC device that answers with the transfers of the input, again and again: each transfer is a length byte
/// and that many bytes (twice the length byte when it is odd, so a transfer can be long). It echoes the tag of the
/// request, as a device does. `data` counts the bytes the reader takes as data: those after the header of the
/// transfer that answers a request, and all of a transfer that continues it.
struct Device {
    transfers: Vec<Vec<u8>>,
    asked: usize,
    tag: u8,
    data: usize,
    requested: bool,
}

impl crate::usbtmc::Bulk for Device {
    fn bulk_out(&mut self, data: &[u8], _: u32) -> std::io::Result<usize> {
        self.tag = data.get(1).copied().unwrap_or(0);
        self.requested = true;
        Ok(data.len())
    }

    fn bulk_in(&mut self, buf: &mut [u8], _: u32) -> std::io::Result<usize> {
        if self.transfers.is_empty() || self.asked >= 300 {
            return Err(std::io::Error::from_raw_os_error(libc::ETIMEDOUT));
        }
        let at = self.asked % self.transfers.len();
        let t = &mut self.transfers[at];
        self.asked += 1;
        if t.len() >= 3 {
            (t[1], t[2]) = (self.tag, !self.tag);
        }
        let n = t.len().min(buf.len());
        buf[..n].copy_from_slice(&t[..n]);
        self.data += if std::mem::take(&mut self.requested) { n.saturating_sub(crate::usbtmc::HEADER) } else { n };
        Ok(n)
    }

    fn clear_halt(&mut self, _: bool) -> std::io::Result<()> {
        Ok(())
    }
}

/// `Usbtmc::read` and `parse_in_header` (the answers of a generator on the USB port): return for any transfers;
/// an answer taken is at most 1 MiB and a transfer; a device that keeps sending transfers without data and without
/// an end is given up.
#[test]
fn hostile_usbtmc_answers() {
    let transfer = |data: &[u8], eom: bool| {
        let mut t = vec![2, 1, !1, 0];
        t.extend_from_slice(&(data.len() as u32).to_le_bytes());
        t.extend_from_slice(&[eom as u8, 0, 0, 0]);
        t.extend_from_slice(data);
        t.resize(t.len().next_multiple_of(4), 0);
        let mut framed = vec![t.len() as u8 & !1];
        framed.extend(t);
        framed
    };
    let whole = transfer(b"OOS,generator,0,1.0\n", true);
    let parts = [transfer(b"1.0000", false), transfer(b"00E+03\n", true)].concat();
    let seeds: [&[u8]; 2] = [&whole, &parts];
    run("usbtmc::read", &seeds, 1, |b| {
        let mut transfers = Vec::new();
        let mut rest = b;
        while let Some((&n, tail)) = rest.split_first() {
            let n = (if n & 1 == 1 { 2 * n as usize } else { n as usize }).min(tail.len());
            transfers.push(tail[..n].to_vec());
            rest = &tail[n..];
        }
        if let Some(t) = transfers.first() {
            if let Ok(h) = crate::usbtmc::parse_in_header(t, 1) {
                if t.len() < crate::usbtmc::HEADER || t[0] != 2 || (t[1], t[2]) != (1, !1) {
                    return Err(format!("took the header {:02x?} as {h:?}", &t[..t.len().min(12)]));
                }
            }
        }
        let mut dev = crate::usbtmc::Usbtmc::new(Device { transfers, asked: 0, tag: 0, data: 0, requested: false });
        let answer = dev.read();
        if let Ok(a) = &answer {
            if a.len() > (1 << 20) + 4096 {
                return Err(format!("an answer of {} bytes", a.len()));
            }
        }
        if dev.bus.asked >= 300 && dev.bus.data == 0 {
            return Err("still asking after 300 transfers without a byte of data and without an end".into());
        }
        Ok(())
    });
}

// Break it catches: a read that asks for ever while the device sends neither data nor the end (#181)
#[test]
fn hostile_usbtmc_empty_transfers_forever() {
    // one transfer: the header of an answer of no bytes that is not the end of the message
    let empty = vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut dev =
        crate::usbtmc::Usbtmc::new(Device { transfers: vec![empty], asked: 0, tag: 0, data: 0, requested: false });
    let answer = dev.read();
    assert!(dev.bus.asked < 300, "still asking after {} empty transfers: {:?}", dev.bus.asked, answer);
}

/// CRC-7/MMC of a frame's command and data byte (polynomial x^7 + x^3 + 1, docs/protocols/frontpanel.md), bit by
/// bit: the check of the test, written apart from the one the parser uses.
fn crc7(cmd: u8, data: u8) -> u8 {
    let mut c = 0u8;
    for bit in (0..16).rev() {
        let top = (c >> 6 & 1) ^ ((u16::from_be_bytes([cmd, data]) >> bit) as u8 & 1);
        c = (c << 1) & 0x7f;
        if top == 1 {
            c ^= 0x09;
        }
    }
    c
}

/// `Board::panel_keys` (the frames of the front-panel MCU on its UART, `FA cmd data crc7 AF`): returns for any
/// bytes; every key it gives is the data byte of a whole frame with a matching checksum in the input, in order;
/// fewer than five bytes stay behind.
#[test]
fn hostile_panel_frames() {
    // frames with a checksum the board made: the LED frames carry any data byte
    let frames = crate::board::board().panel_leds(0x0005_a521).concat();
    let noisy = [&[0xfa, 0xfa, 0x00][..], &frames[..5], &[0xaf, 0xfa]].concat();
    let seeds: [&[u8]; 2] = [&frames, &noisy];
    run("board::panel_keys", &seeds, 1, |b| {
        let mut buf = b.to_vec();
        let got = crate::board::board().panel_keys(&mut buf);
        if buf.len() >= 5 || got.len() > b.len() / 5 {
            return Err(format!("{} keys from {} bytes, {} left", got.len(), b.len(), buf.len()));
        }
        // the frames of the input, read the slow way: at each place, a whole frame or one byte to skip
        let (mut want, mut i) = (Vec::new(), 0);
        while i + 5 <= b.len() {
            let f = &b[i..i + 5];
            if f[0] == 0xfa && f[4] == 0xaf && f[3] == crc7(f[1], f[2]) {
                want.push(f[2]);
                i += 5;
            } else {
                i += 1;
            }
        }
        if got != want {
            return Err(format!("the keys {got:02x?}, the frames of the input say {want:02x?}"));
        }
        Ok(())
    });
}
