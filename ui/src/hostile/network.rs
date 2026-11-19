// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hostile input for what arrives in frames: SNTP answers, the answers of a USBTMC device and the frames of the
//! front-panel MCU. VXI-11, HiSLIP, mDNS and HTTP are tested in their own modules, where their private parts are.

use super::run;
use crate::board::Board;

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
