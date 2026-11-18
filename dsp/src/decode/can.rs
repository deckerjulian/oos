// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! CAN 2.0A/B and CAN FD.
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::{can_crc15, can_fd_crc, Kind, Packet};
use crate::edges::{edges, states, Edge};

/// CAN 2.0A/B data and remote frames at `bitrate`. `invert` = false: logic level as on the RX/TX pin (dominant low,
/// idle high); true for CAN_H probed against ground (dominant high). Hard sync on the start of frame after at least 7
/// recessive bit times, resynchronisation on every edge, sampling at 70 % of the bit, bit de-stuffing, CRC check.
pub fn can(v: &[f32], dt: f64, bitrate: f64, thr: f32, hyst: f32, invert: bool) -> Vec<Packet> {
    let spb = 1.0 / bitrate / dt;
    if spb < 3.0 {
        return Vec::new();
    }
    let st = states(v, thr, hyst);
    // recessive = true
    let rec = |x: f64| -> Option<bool> { st.get(x as usize).copied().flatten().map(|b| b != invert) };
    let e = edges(v, thr, hyst);
    let edge_near = |from: f64, to: f64| -> Option<f64> {
        let i = e.partition_point(|x| x.t < from);
        e.get(i).filter(|x| x.t < to).map(|x| x.t)
    };
    frames(&e, invert, spb, &rec, |sof| can_frame(sof, spb, &rec, &edge_near))
}

/// The frames on the bus: `frame` decodes from each SOF (packets, end), one not inside the frame before.
fn frames(
    e: &[Edge],
    invert: bool,
    bit: f64,
    rec: &dyn Fn(f64) -> Option<bool>,
    mut frame: impl FnMut(f64) -> Option<(Vec<Packet>, f64)>,
) -> Vec<Packet> {
    let mut out = Vec::new();
    let mut busy_until = 0.0;
    let mut last_rec_start: Option<f64> = None;
    for (k, x) in e.iter().enumerate() {
        // a recessive run starts at a recessive-going edge
        let to_rec = x.rising != invert;
        if to_rec {
            last_rec_start = Some(x.t);
            continue;
        }
        if x.t < busy_until {
            continue;
        }
        // SOF: dominant edge after >= 7 recessive bit times (or the record start when the bus was idle before)
        let idle_from =
            last_rec_start.unwrap_or(if k == 0 && rec(x.t - 1.0) == Some(true) { 0.0 } else { f64::INFINITY });
        if x.t - idle_from < 7.0 * bit {
            continue;
        }
        if let Some((pk, end)) = frame(x.t) {
            busy_until = end;
            out.extend(pk);
        }
    }
    out
}

/// One CAN frame from its SOF edge at `sof`: the packets and the time after the ACK delimiter.
fn can_frame(
    sof: f64,
    spb: f64,
    rec: &dyn Fn(f64) -> Option<bool>,
    edge_near: &dyn Fn(f64, f64) -> Option<f64>,
) -> Option<(Vec<Packet>, f64)> {
    let mut start = sof;
    let mut raw: Vec<(bool, f64, f64)> = Vec::new(); // (level, bit start, bit end)
    let mut next = || -> Option<(bool, f64, f64)> {
        let b = rec(start + 0.7 * spb)?;
        let s = start;
        let nominal = start + spb;
        start = match edge_near(start + 0.5 * spb, start + 1.5 * spb) {
            Some(te) if (te - nominal).abs() < 0.5 * spb => te,
            _ => nominal,
        };
        Some((b, s, start))
    };
    // de-stuffed bits with their times
    let mut bits: Vec<(bool, f64, f64)> = Vec::new();
    let (mut run, mut last) = (0, None::<bool>);
    let mut need = 19usize; // SOF + ID 11 + RTR/SRR + IDE + r0 + DLC 4, grows when the header is known
    let (mut ext, mut known) = (false, false);
    while bits.len() < need {
        let b = next()?;
        raw.push(b);
        if run == 5 {
            // stuff bit: must be the complement, not part of the data
            if Some(b.0) == last {
                return None; // stuff error
            }
            run = 1;
            last = Some(b.0);
            continue;
        }
        if Some(b.0) == last {
            run += 1;
        } else {
            run = 1;
            last = Some(b.0);
        }
        bits.push(b);
        if !known && bits.len() == 14 {
            // bit 13 = IDE (recessive = extended)
            ext = bits[13].0;
            if ext {
                need = 1 + 11 + 1 + 1 + 18 + 1 + 2 + 4;
            }
        }
        if !known && bits.len() == need {
            known = true;
            let dlc_at = need - 4;
            let dlc = bits[dlc_at..need].iter().fold(0u32, |a, b| a << 1 | b.0 as u32);
            let rtr = if ext { bits[32].0 } else { bits[12].0 };
            let n = if rtr { 0 } else { dlc.min(8) as usize };
            need += 8 * n + 15;
        }
    }
    let val = |r: std::ops::Range<usize>| bits[r].iter().fold(0u32, |a, b| a << 1 | b.0 as u32);
    let (id, rtr, dlc_at) =
        if ext { (val(1..12) << 18 | val(14..32), bits[32].0, 35) } else { (val(1..12), bits[12].0, 15) };
    let dlc = val(dlc_at..dlc_at + 4);
    let n = if rtr { 0 } else { dlc.min(8) as usize };
    let data_at = dlc_at + 4;
    let crc_at = data_at + 8 * n;
    let crc_calc = can_crc15(&bits[..crc_at].iter().map(|b| b.0).collect::<Vec<_>>());
    let crc_rx = val(crc_at..crc_at + 15) as u16;
    // stuffing covers SOF .. CRC sequence: a run of five at the end of the CRC is followed by one more stuff bit
    if run == 5 {
        let (_, _, _) = next()?;
    }
    // CRC delimiter, ACK slot, ACK delimiter are not stuffed
    let (_, _, _) = next()?;
    let ack = next()?;
    let ack_del = next()?;
    let pk = |kind, a: usize, b: usize, value: u32, ok: bool| Packet::new(kind, bits[a].1, bits[b].2, value, ok);
    let mut out = vec![Packet::new(Kind::Start, bits[0].1, bits[0].2, 0, true)];
    let id_end = if ext { 31 } else { 11 };
    out.push(pk(Kind::Id { ext, rtr }, 1, id_end, id, true));
    for i in 0..n {
        out.push(pk(Kind::Data, data_at + 8 * i, data_at + 8 * i + 7, val(data_at + 8 * i..data_at + 8 * i + 8), true));
    }
    out.push(pk(Kind::Crc, crc_at, crc_at + 14, crc_rx as u32, crc_rx == crc_calc));
    out.push(Packet::new(Kind::Ack, ack.1, ack.2, 0, !ack.0));
    Some((out, ack_del.2))
}

/// Payload length per DLC in CAN FD frames.
pub const FD_LEN: [usize; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64];

/// Sample point (fraction of the bit) where the bit rate switches (BRS bit, CRC delimiter); assumed by `can_fd`
/// and used by `gen::can_fd_bits`.
pub const FD_SP: f64 = 0.75;

/// Bit timing reader: one bit at a time from the current position, resynchronised on edges near the bit end.
struct BitClock<'a> {
    t: f64,
    rec: &'a dyn Fn(f64) -> Option<bool>,
    e: &'a [Edge],
}

impl BitClock<'_> {
    /// One bit sampled `smp` samples after its start; `f(level)` = (bit length, resync tolerance). The next bit starts
    /// at an edge within the tolerance of the nominal end, else at the nominal end. (level, start, end)
    fn bit(&mut self, smp: f64, f: impl Fn(bool) -> (f64, f64)) -> Option<(bool, f64, f64)> {
        let s = self.t;
        let b = (self.rec)(s + smp)?;
        let (len, tol) = f(b);
        let nominal = s + len;
        let i = self.e.partition_point(|x| x.t < nominal - tol);
        self.t = match self.e.get(i) {
            Some(x) if x.t < nominal + tol => x.t,
            _ => nominal,
        };
        Some((b, s, self.t))
    }
}

/// CAN FD (ISO 11898-1:2015) and classic CAN frames (base and extended format) on one line, `invert` as in
/// `decode::can`. Nominal bit rate `nominal`, data phase `data_rate` (from the sample point of a recessive BRS bit to
/// the sample point of the CRC delimiter, `FD_SP`). Dynamic stuffing up to the end of the data field, then the stuff
/// count and CRC with fixed stuff bits (complement of the preceding bit, before the stuff count and after every 4
/// bits; it replaces a dynamic stuff bit due at the data field end).
///
/// Emits Start (SOF), Id { ext, rtr } (rtr only for classic remote frames), FD only: Field("BRS"), Field("ESI");
/// Field("DLC") (value = DLC, value2 = data bytes), Data per byte, FD only: Field("SBC") (stuff count mod 8 from the
/// Gray code; `ok` = parity and count right), Crc (value = received CRC-15/17/21, `ok` = it matches and the fixed
/// stuff bits are right), Ack (`ok` = dominant). A stuff error ends the frame with Field("!ERR").
pub fn can_fd(v: &[f32], dt: f64, nominal: f64, data_rate: f64, thr: f32, hyst: f32, invert: bool) -> Vec<Packet> {
    let tn = 1.0 / nominal / dt;
    let td = 1.0 / data_rate / dt;
    if !(tn >= 3.0 && td >= 3.0) {
        return Vec::new();
    }
    let st = states(v, thr, hyst);
    let rec = |x: f64| -> Option<bool> { st.get(x as usize).copied().flatten().map(|b| b != invert) };
    let e = edges(v, thr, hyst);
    frames(&e, invert, tn, &rec, |sof| can_fd_frame(&mut BitClock { t: sof, rec: &rec, e: &e }, tn, td))
}

/// One CAN / CAN FD frame from its SOF: the packets and the time after the ACK delimiter.
fn can_fd_frame(clk: &mut BitClock, tn: f64, td: f64) -> Option<(Vec<Packet>, f64)> {
    let sof = clk.t;
    let mut raw: Vec<bool> = Vec::new(); // SOF .. data end with the dynamic stuff bits (FD CRC input)
    let mut bits: Vec<(bool, f64, f64)> = Vec::new(); // de-stuffed
    let (mut run, mut last) = (0u32, None::<bool>);
    let mut stuff_cnt = 0u32;
    let mut need = 14usize; // up to IDE first
    let (mut ext, mut fd, mut fast) = (false, false, false);
    let (mut fdf_at, mut dlc_at, mut data_end, mut brs_at) = (usize::MAX, usize::MAX, usize::MAX, usize::MAX);
    let nominal = |_: bool| (tn, 0.4 * tn);
    let data = |_: bool| (td, 0.4 * td);
    // the dynamically stuffed part; classic frames include the CRC-15
    while bits.len() < need {
        let idx = bits.len();
        let stuff = run == 5;
        let b = if fast {
            clk.bit(0.65 * td, data)?
        } else if !stuff && idx == brs_at {
            // BRS: a recessive BRS switches to the data bit rate at its sample point
            clk.bit(0.65 * tn, |l| if l { (FD_SP * tn + (1.0 - FD_SP) * td, 0.4 * td) } else { (tn, 0.4 * tn) })?
        } else {
            clk.bit(0.65 * tn, nominal)?
        };
        raw.push(b.0);
        if stuff {
            if Some(b.0) == last {
                let start = Packet::new(Kind::Start, sof, bits[0].2, 0, true);
                return Some((vec![start, Packet::tag("!ERR", b.1, b.2, false)], b.2));
            }
            run = 1;
            last = Some(b.0);
            stuff_cnt += 1;
            continue;
        }
        if Some(b.0) == last {
            run += 1;
        } else {
            run = 1;
            last = Some(b.0);
        }
        bits.push(b);
        if idx == brs_at && b.0 {
            fast = true;
        }
        let n = bits.len();
        if n == 14 && fdf_at == usize::MAX {
            ext = bits[13].0;
            fdf_at = if ext { 33 } else { 14 };
            need = fdf_at + 1;
        } else if dlc_at == usize::MAX && fdf_at != usize::MAX && n == fdf_at + 1 {
            fd = bits[fdf_at].0;
            dlc_at = if fd {
                fdf_at + 4
            } else if ext {
                fdf_at + 2
            } else {
                fdf_at + 1
            };
            if fd {
                brs_at = fdf_at + 2;
            }
            need = dlc_at + 4;
        } else if data_end == usize::MAX && dlc_at != usize::MAX && n == dlc_at + 4 {
            let dlc = bits[dlc_at..dlc_at + 4].iter().fold(0usize, |a, b| a << 1 | b.0 as usize);
            let rtr = !fd && bits[if ext { 32 } else { 12 }].0;
            let nb = if fd {
                FD_LEN[dlc]
            } else if rtr {
                0
            } else {
                dlc.min(8)
            };
            data_end = n + 8 * nb;
            need = if fd { data_end } else { data_end + 15 };
        }
    }
    let val = |r: std::ops::Range<usize>| bits[r].iter().fold(0u32, |a, b| a << 1 | b.0 as u32);
    let span = |kind, a: usize, b: usize, value: u32, ok| Packet::new(kind, bits[a].1, bits[b].2, value, ok);
    let rtr = !fd && bits[if ext { 32 } else { 12 }].0;
    let id = if ext { val(1..12) << 18 | val(14..32) } else { val(1..12) };
    let dlc = val(dlc_at..dlc_at + 4);
    let nb = (data_end - dlc_at - 4) / 8;
    let mut out =
        vec![span(Kind::Start, 0, 0, 0, true), span(Kind::Id { ext, rtr }, 1, if ext { 31 } else { 11 }, id, true)];
    if fd {
        out.push(span(Kind::Field("BRS"), brs_at, brs_at, bits[brs_at].0 as u32, true));
        out.push(span(Kind::Field("ESI"), brs_at + 1, brs_at + 1, bits[brs_at + 1].0 as u32, true));
    }
    out.push(span(Kind::Field("DLC"), dlc_at, dlc_at + 3, dlc, true).with_value2(nb as u32));
    for i in 0..nb {
        let a = dlc_at + 4 + 8 * i;
        out.push(span(Kind::Data, a, a + 7, val(a..a + 8), true));
    }
    let dd = |_: bool| if fast { (td, 0.4 * td) } else { (tn, 0.4 * tn) };
    let ds = if fast { 0.65 * td } else { 0.65 * tn };
    if fd {
        let crc_len = if nb > 16 { 21 } else { 17 };
        let mut prev = raw.last().copied().unwrap_or(false);
        let mut fixed_ok = true;
        let mut seq: Vec<(bool, f64, f64)> = Vec::new();
        for i in 0..4 + crc_len {
            if i % 4 == 0 {
                let f = clk.bit(ds, dd)?;
                fixed_ok &= f.0 != prev;
            }
            let b = clk.bit(ds, dd)?;
            prev = b.0;
            seq.push(b);
        }
        let sv = |r: std::ops::Range<usize>| seq[r].iter().fold(0u32, |a, b| a << 1 | b.0 as u32);
        let gray = sv(0..3);
        let cnt = (gray ^ gray >> 1 ^ gray >> 2) & 7;
        let par_ok = (gray.count_ones() + seq[3].0 as u32).is_multiple_of(2);
        out.push(Packet::field("SBC", seq[0].1, seq[3].2, cnt, par_ok && cnt == stuff_cnt % 8));
        let rx = sv(4..4 + crc_len);
        let mut input = raw.clone();
        input.extend(seq[..4].iter().map(|b| b.0));
        let calc = can_fd_crc(&input, crc_len as u32);
        out.push(Packet::new(Kind::Crc, seq[4].1, seq[3 + crc_len].2, rx, rx == calc && fixed_ok));
    } else {
        // a stuff bit after the last CRC bit
        if run == 5 {
            clk.bit(0.65 * tn, nominal)?;
        }
        let crc_at = data_end;
        let calc = can_crc15(&bits[..crc_at].iter().map(|b| b.0).collect::<Vec<_>>());
        let rx = val(crc_at..crc_at + 15);
        out.push(span(Kind::Crc, crc_at, crc_at + 14, rx, rx == calc as u32));
    }
    // CRC delimiter (back to the nominal rate at its sample point), ACK slot, ACK delimiter
    if fast {
        clk.bit(0.65 * td, |_| (FD_SP * td + (1.0 - FD_SP) * tn, 0.4 * td))?;
    } else {
        clk.bit(0.65 * tn, nominal)?;
    }
    let ack = clk.bit(0.65 * tn, nominal)?;
    let del = clk.bit(0.65 * tn, nominal)?;
    out.push(Packet::new(Kind::Ack, ack.1, ack.2, 0, !ack.0));
    Some((out, del.2))
}
