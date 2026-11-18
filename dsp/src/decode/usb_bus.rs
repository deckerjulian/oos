// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! USB 1.x / 2.0 low speed and full speed: the line states of D+ and D- that the packet decoder in `usb` reads.
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::{UsbSpeed, USB_FULL, USB_LOW};
use crate::edges::{edges, states};

/// Line state of the USB pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ls {
    J,
    K,
    Se0,
    Se1,
}

/// The line as state segments (start in fractional samples, state) and what the decoder knows about the bus.
pub(super) struct UsbBus {
    pub(super) segs: Vec<(f64, Ls)>,
    pub(super) n: f64,
    /// D+ and D- given; with D+ alone there is no SE0 (it looks like D+ low)
    pub(super) two: bool,
    /// J = D+ high (full-speed signalling)
    pub(super) fs_pol: bool,
    /// bit times (samples) the SYNC may have, with the rate in kbit/s
    rates: Vec<(f64, u32)>,
}

impl UsbBus {
    pub(super) fn end(&self, k: usize) -> f64 {
        self.segs.get(k + 1).map_or(self.n, |s| s.0)
    }
    pub(super) fn dur(&self, k: usize) -> f64 {
        self.end(k) - self.segs[k].0
    }
    /// The segment at time `t`.
    pub(super) fn at(&self, t: f64) -> usize {
        self.segs.partition_point(|s| s.0 <= t).saturating_sub(1)
    }
    /// A SYNC from segment `k`: K J K J K J one bit each, then K; its bit time and rate.
    pub(super) fn sync(&self, k: usize) -> Option<(f64, u32)> {
        let s = self.segs.get(k..k + 7)?;
        if s.iter().enumerate().any(|(q, x)| x.1 != if q % 2 == 0 { Ls::K } else { Ls::J }) {
            return None;
        }
        let bit = (s[6].0 - s[0].0) / 6.0;
        if s.windows(2).any(|w| !((0.6..1.4).contains(&((w[1].0 - w[0].0) / bit)))) || bit.is_nan() || bit < 3.0 {
            return None;
        }
        self.rates.iter().find(|r| (bit / r.0 - 1.0).abs() <= 0.25).map(|r| (bit, r.1))
    }
    /// D+ alone: whether the line after `te` (the end of a candidate packet) looks like the EOP (D+ low for two bit
    /// times) and then idle or the next SYNC; the segment to go on from.
    pub(super) fn eop_after(&self, te: f64, bit: f64) -> Option<usize> {
        let q = self.at(te + 0.5 * bit);
        let idle_or_sync = |k: usize| self.end(k) >= self.n || self.dur(k) > 7.0 * bit || self.sync(k + 1).is_some();
        if self.fs_pol {
            // the SE0 looks like K, then J
            let ok = self.segs[q].1 == Ls::K
                && (self.end(q) - te - 2.0 * bit).abs() <= 0.6 * bit
                && q + 1 < self.segs.len()
                && (self.dur(q + 1) >= 1.5 * bit || self.end(q + 1) >= self.n)
                && idle_or_sync(q + 1);
            ok.then_some(q + 1)
        } else {
            // the SE0 looks like J: at least two bit times of it and two of idle
            let ok =
                self.segs[q].1 == Ls::J && (self.end(q) - te >= 3.5 * bit || self.end(q) >= self.n) && idle_or_sync(q);
            ok.then_some(q + 1)
        }
    }
}

pub(super) fn usb_bus(dp: &[f32], dm: Option<&[f32]>, dt: f64, speed: UsbSpeed, thr: f32, hyst: f32) -> UsbBus {
    let len = dm.map_or(dp.len(), |m| m.len().min(dp.len()));
    let n = len as f64;
    let (fs, ls) = (1.0 / USB_FULL / dt, 1.0 / USB_LOW / dt);
    // line code: bit 0 = D+, bit 1 = D-; with D+ alone D- is taken as its inverse
    let lines: Vec<&[f32]> = std::iter::once(&dp[..len]).chain(dm.map(|m| &m[..len])).collect();
    let single = |c: u8| {
        if dm.is_some() {
            c
        } else if c & 1 != 0 {
            1
        } else {
            2
        }
    };
    let mut code = 0u8;
    let mut ev = Vec::new();
    for (k, v) in lines.iter().enumerate() {
        let e = edges(v, thr, hyst);
        let first = match e.first() {
            Some(x) => !x.rising,
            None => states(v, thr, hyst).into_iter().flatten().last().unwrap_or(false),
        };
        code |= (first as u8) << k;
        ev.extend(e.into_iter().map(|x| (x.t, k, x.rising)));
    }
    ev.sort_by(|a, b| a.0.total_cmp(&b.0));
    code = single(code);
    let mut raw = vec![(0.0, code)];
    for (t, k, rising) in ev {
        code = single(if rising { code | 1 << k } else { code & !(1 << k) });
        if raw.last().is_some_and(|x| x.1 != code) {
            raw.push((t, code));
        }
    }
    let rdur = |k: usize| raw.get(k + 1).map_or(n, |s| s.0) - raw[k].0;
    // J: the idle state = the longest D+ / D- high segment
    let jc = match speed {
        UsbSpeed::Low => 2,
        UsbSpeed::Full => 1,
        UsbSpeed::Auto => (0..raw.len())
            .filter(|&k| raw[k].1 == 1 || raw[k].1 == 2)
            .max_by(|&a, &b| rdur(a).total_cmp(&rdur(b)))
            .map_or(1, |k| raw[k].1),
    };
    let fs_pol = jc == 1;
    // SE0 / SE1 shorter than 0.3 bit: the crossover of D+ and D- in a J <-> K transition; the transition is in its
    // middle
    let g = 0.3 * if fs_pol { fs } else { ls };
    let mut segs: Vec<(f64, Ls)> = Vec::new();
    let mut mid = None;
    for k in 0..raw.len() {
        let (t, c) = raw[k];
        if (c == 0 || c == 3) && k > 0 && k + 1 < raw.len() && rdur(k) < g {
            mid = Some(t + 0.5 * rdur(k));
            continue;
        }
        let st = match c {
            0 => Ls::Se0,
            3 => Ls::Se1,
            c if c == jc => Ls::J,
            _ => Ls::K,
        };
        let t = mid.take().unwrap_or(t);
        if segs.last().is_none_or(|s| s.1 != st) {
            segs.push((t, st));
        }
    }
    let rates = if fs_pol { vec![(fs, 12000), (ls, 1500)] } else { vec![(ls, 1500)] };
    UsbBus { segs, n, two: dm.is_some(), fs_pol, rates }
}
