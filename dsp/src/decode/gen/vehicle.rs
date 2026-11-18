// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Signal generators for the CAN FD, FlexRay and SENT decoders.
//!
//! A generator for a new decoder goes next to the generators of its family, in `gen.rs` or a file in `gen/`; the
//! decoder itself goes into the file of its family in `decode/`, and its protocol module is registered in
//! `ui/src/protocols/mod.rs` (ADR 0002).

use super::Wave;
use crate::decode::{can_crc15, can_fd_crc, flexray_frame_crc, flexray_header_crc, sent_crc, FD_LEN, FD_SP};
use crate::sim::Logic;

/// One CAN / CAN FD frame for `can_fd_bits`.
#[derive(Clone, Debug)]
pub struct FdFrame {
    pub id: u32,
    pub ext: bool,
    /// FD frame (FDF recessive); false = classic data frame
    pub fd: bool,
    pub brs: bool,
    pub esi: bool,
    /// payload; FD frames are padded with 0xCC to the next valid length
    pub data: Vec<u8>,
}

/// The bus bits of a frame (true = recessive) with their durations in seconds: SOF .. EOF and 3 bits
/// intermission, data phase at `data_rate` when BRS is set (switch at `FD_SP` of the BRS bit / CRC delimiter).
pub fn can_fd_bits(f: &FdFrame, nominal: f64, data_rate: f64) -> Vec<(bool, f64)> {
    let (tn, td) = (1.0 / nominal, 1.0 / data_rate);
    let push = |h: &mut Vec<bool>, v: u32, n: u32| (0..n).rev().for_each(|i| h.push((v >> i) & 1 == 1));
    let dlc = if f.fd {
        FD_LEN.iter().position(|&l| l >= f.data.len()).unwrap_or(15) as u32
    } else {
        f.data.len().min(8) as u32
    };
    let nb = if f.fd { FD_LEN[dlc as usize] } else { dlc as usize };
    let mut h = vec![false];
    if f.ext {
        push(&mut h, f.id >> 18 & 0x7ff, 11);
        h.push(true); // SRR
        h.push(true); // IDE
        push(&mut h, f.id & 0x3ffff, 18);
        h.push(false); // RTR / RRS
    } else {
        push(&mut h, f.id & 0x7ff, 11);
        h.push(false); // RTR / RRS
        h.push(false); // IDE
    }
    let mut brs_at = usize::MAX;
    if f.fd {
        h.push(true); // FDF
        h.push(false); // res
        brs_at = h.len();
        h.push(f.brs);
        h.push(f.esi);
    } else {
        if f.ext {
            h.push(false); // r1
        }
        h.push(false); // r0
    }
    push(&mut h, dlc, 4);
    for i in 0..nb {
        push(&mut h, *f.data.get(i).unwrap_or(&0xcc) as u32, 8);
    }
    if !f.fd {
        let c = can_crc15(&h) as u32;
        push(&mut h, c, 15);
    }
    let fast = f.fd && f.brs;
    let brs_len = FD_SP * tn + (1.0 - FD_SP) * td;
    let del_len = FD_SP * td + (1.0 - FD_SP) * tn;
    let mut out: Vec<(bool, f64)> = Vec::new();
    let mut raw = Vec::new();
    let (mut run, mut last, mut cnt) = (0, None, 0u32);
    for (k, &x) in h.iter().enumerate() {
        let len = if fast && k == brs_at {
            brs_len
        } else if fast && k > brs_at {
            td
        } else {
            tn
        };
        out.push((x, len));
        raw.push(x);
        if Some(x) == last {
            run += 1;
        } else {
            run = 1;
            last = Some(x);
        }
        // a stuff bit after 5 equal bits; at the end of an FD data field the fixed stuff bit replaces it
        if run == 5 && !(f.fd && k + 1 == h.len()) {
            out.push((!x, if fast && k >= brs_at { td } else { tn }));
            raw.push(!x);
            last = Some(!x);
            run = 1;
            cnt += 1;
        }
    }
    let dd = if fast { td } else { tn };
    if f.fd {
        let g = (cnt % 8) ^ (cnt % 8) >> 1;
        let mut seq = Vec::new();
        push(&mut seq, g, 3);
        seq.push(g.count_ones() % 2 == 1);
        let crc_len = if nb > 16 { 21 } else { 17 };
        let mut input = raw.clone();
        input.extend(&seq);
        push(&mut seq, can_fd_crc(&input, crc_len), crc_len);
        let mut prev = *raw.last().expect("a frame starts with its SOF bit");
        for (i, &b) in seq.iter().enumerate() {
            if i % 4 == 0 {
                out.push((!prev, dd));
            }
            out.push((b, dd));
            prev = b;
        }
    }
    out.push((true, if fast { del_len } else { tn })); // CRC delimiter
    out.push((false, tn)); // ACK slot
    out.push((true, tn)); // ACK delimiter
    out.push((true, 10.0 * tn)); // EOF + intermission
    out
}

/// CAN / CAN FD frames as a logic waveform (dominant = `lo`), 12 idle bits before the first.
pub fn can_fd_logic(frames: &[FdFrame], nominal: f64, data_rate: f64, lo: f64, hi: f64) -> Logic {
    let mut w = Wave::new(lo, hi, 0.02 / data_rate);
    w.level(true, 12.0 / nominal);
    for f in frames {
        for (b, d) in can_fd_bits(f, nominal, data_rate) {
            w.level(b, d);
        }
    }
    w.done()
}

/// One FlexRay frame for `flexray_bits`.
#[derive(Clone, Debug)]
pub struct FrFrame {
    pub ppi: bool,
    /// null frame indicator (false = null frame)
    pub nf: bool,
    pub sync: bool,
    pub startup: bool,
    pub id: u16,
    pub cycle: u8,
    /// payload, padded with 0 to an even length
    pub payload: Vec<u8>,
    pub channel_b: bool,
}

/// Header, payload and CRC bytes of a FlexRay frame.
pub fn flexray_bytes(f: &FrFrame) -> Vec<u8> {
    let mut pl = f.payload.clone();
    pl.truncate(254);
    if pl.len() % 2 == 1 {
        pl.push(0);
    }
    let words = (pl.len() / 2) as u64;
    let id = (f.id & 0x7ff) as u64;
    let hcrc =
        flexray_header_crc(((f.sync as u32) << 19) | ((f.startup as u32) << 18) | (id as u32) << 7 | words as u32)
            as u64;
    let h: u64 = (f.ppi as u64) << 38
        | (f.nf as u64) << 37
        | (f.sync as u64) << 36
        | (f.startup as u64) << 35
        | id << 24
        | words << 17
        | hcrc << 6
        | (f.cycle & 0x3f) as u64;
    let mut b: Vec<u8> = h.to_be_bytes()[3..].to_vec();
    b.extend(pl);
    let crc = flexray_frame_crc(&b, f.channel_b);
    b.extend(&crc.to_be_bytes()[1..]);
    b
}

/// The bits of a FlexRay frame on the bus (true = high / Data_1): TSS of `tss` bits, FSS, BSS + byte for every
/// byte, FES.
pub fn flexray_bits(f: &FrFrame, tss: usize) -> Vec<bool> {
    let mut s = vec![false; tss];
    s.push(true);
    for b in flexray_bytes(f) {
        s.push(true);
        s.push(false);
        (0..8).rev().for_each(|i| s.push((b >> i) & 1 == 1));
    }
    s.push(false);
    s.push(true);
    s
}

/// One SENT frame for `sent_ticks`: status and data nibbles, `crc` = None for the right CRC (recommended method),
/// `pause` = pause pulse ticks (0 = none).
#[derive(Clone, Debug)]
pub struct SentFrame {
    pub status: u8,
    pub data: Vec<u8>,
    pub crc: Option<u8>,
    pub pause: u32,
}

/// The falling-edge intervals of SENT frames in ticks: sync 56, nibbles 12 + value, the pause.
pub fn sent_ticks(frames: &[SentFrame]) -> Vec<u32> {
    let mut t = Vec::new();
    for f in frames {
        t.push(56);
        t.push(12 + (f.status & 15) as u32);
        t.extend(f.data.iter().map(|&x| 12 + (x & 15) as u32));
        t.push(12 + (f.crc.unwrap_or_else(|| sent_crc(&f.data, false)) & 15) as u32);
        if f.pause > 0 {
            t.push(f.pause);
        }
    }
    t
}

/// SENT intervals (ticks) as a logic waveform, idle high: every interval starts with a falling edge and 5 ticks
/// low; a last falling edge (and low pulse) ends the last interval.
pub fn sent_logic(ticks: &[u32], tick: f64, lo: f64, hi: f64) -> Logic {
    let mut l = Logic::new(lo, hi, tick / 4.0);
    l.push(0.0, true);
    let mut t = 20.0 * tick;
    for &n in ticks.iter().chain([&5]) {
        l.push(t, false);
        l.push(t + 5.0 * tick, true);
        t += n as f64 * tick;
    }
    l
}

/// SENT frames as a logic waveform at `tick` (s).
pub fn sent(frames: &[SentFrame], tick: f64, lo: f64, hi: f64) -> Logic {
    sent_logic(&sent_ticks(frames), tick, lo, hi)
}
