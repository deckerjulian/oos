// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hardware averaging (docs/fork/specs/01-hw-averaging.md): the FPGA averages every trigger, the host plays every
//! frame of a batch (the averaging sits on the play side) and shows the newest (`seq::State::read_steps`). The register side is in `acq` (mode, count, clear), `hz` (segment
//! layout) and `scu` (window word). NOT VERIFIED ON THE UNIT: what the frames hold in average mode is not visible in the
//! stock library (spec 01, 6.1). Two readings are supported, chosen by `Kind`:
//!
//! - `Running`: the FPGA keeps a running average and never stops; every batch shows the current average. The stock
//!   driver fits this: it clears only on a change, never during a steady run.
//! - `Hold`: the FPGA averages `count` triggers and then holds the result; the host shows it once complete and
//!   clears for the next one.
//!
//! The data format is assumed unchanged: the stock library decodes average frames like normal ones (spec 01, 1.4), so
//! the averaged words are 16-bit codes around 32768 with 7500 codes per division (`seq::decode_volts`).

use crate::dho::bus::Bus;

/// How the FPGA average behaves once `count` triggers are in (spec 01, 6.1; decided on the unit by
/// docs/fork/unit-dho804.md, "Hardware averaging" step 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// running average: shown after every batch, cleared only on a change
    Running,
    /// cumulative average of `count` triggers that then holds: shown when complete, then cleared
    Hold,
}

/// Counts the hardware averages: powers of two, 2..1024 (the stock UI offers powers of two; the width of 0x10b0 is
/// not known, spec 01, 6.3). Others average in software.
pub fn hw_count(n: u32) -> Option<u32> {
    (n.is_power_of_two() && (2..=1024).contains(&n)).then_some(n)
}

/// What to do after a batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct After {
    /// hand the frame read to the screen
    pub show: bool,
    /// clear the average before the next arm (`seq::State::avg_clear_steps`)
    pub clear: bool,
    /// triggers in the average now (after a clear: 0), at most `count`
    pub triggers: u32,
}

/// Triggers since the last clear, and what each batch means for the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tracker {
    pub kind: Kind,
    pub count: u32,
    acc: u64,
}

impl Tracker {
    pub fn new(kind: Kind, count: u32) -> Tracker {
        Tracker { kind, count: count.max(1), acc: 0 }
    }

    /// The average was cleared (configuration, CLEAR, RUN, SINGLE).
    pub fn reset(&mut self) {
        self.acc = 0;
    }

    /// A batch of `recorded` triggers was read.
    pub fn batch(&mut self, recorded: u32) -> After {
        self.acc += recorded as u64;
        let n = self.count as u64;
        match self.kind {
            Kind::Running => After { show: true, clear: false, triggers: self.acc.min(n) as u32 },
            Kind::Hold if self.acc >= n => {
                self.acc = 0;
                After { show: true, clear: true, triggers: self.count }
            }
            Kind::Hold => After { show: false, clear: false, triggers: self.acc as u32 },
        }
    }
}

/// 0x10b0 read back: the configured count - 1, or the progress (spec 01, 6.2). Diagnostic only.
pub fn progress<B: Bus>(bar: &mut B) -> u32 {
    bar.rd(crate::dho::acq::AVG_COUNT)
}

/// Reference model of an FPGA average of 16-bit words, for the host model and the tests: the sum of the frames in an
/// accumulator of 16 + log2(frames) bits, divided by the number of frames (a shift for powers of two), truncated.
pub fn mean_words(frames: &[&[u16]]) -> Vec<u16> {
    let n = frames.len().max(1) as u64;
    let len = frames.iter().map(|f| f.len()).min().unwrap_or(0);
    (0..len).map(|i| (frames.iter().map(|f| f[i] as u64).sum::<u64>() / n) as u16).collect()
}

/// Bits the accumulator of `n` frames of 16-bit words needs.
pub fn acc_bits(n: u32) -> u32 {
    16 + n.max(1).next_power_of_two().trailing_zeros()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dho::seq;

    use crate::dho::acq::Mode;
    use crate::dho::bus::DmaBuf;
    use crate::dho::seg::sim::{engine, Engine};
    use crate::dho::seq::{Settings, State, Step};

    fn settings(mode: Mode) -> Settings {
        Settings { mode, tdiv: 100e-9, depth: 1000, window: None, frames: 10, ..Settings::default() }
    }

    fn writes(st: &[Step]) -> String {
        st.iter().filter_map(|x| if let Step::Wr((o, v)) = x { Some(format!("{:#x} {:#x}\n", o, v)) } else { None }).collect()
    }

    /// Acquisition block, arm and read of one batch of 10 frames with 4 recorded: the register writes.
    fn plan(mode: Mode) -> String {
        let s = Settings { blocks: 2, ..settings(mode) };
        let mut st = State::handover(1, None);
        let (mut v, p) = st.configure(&s, None, None).unwrap();
        v.extend(st.arm_steps(&s, &p));
        v.extend(st.read_steps(&s, &p, 4));
        writes(&v)
    }

    // The fixture hal/testdata/avg-writes.txt (Average 16, CH1, 100 ns/div, 1000 points, a batch of 10 with 4
    // recorded), checked by hand against spec 01: clear pulse after the stop poll and before the configuration, 0x1004
    // bit3, 0x10b0 = 15, 0x404c = 10, 0x10e0 = 512 bursts, 0x1110 after 0x1108 (window from 0: 0), play of frames 0..3
    // as in normal mode. Break it catches: any change of the average sequence, any difference to normal mode beyond
    // these, a play of the newest frame alone (B1: the frames not played never reach the average).
    #[test]
    fn average_writes_match_the_fixture() {
        let a = plan(Mode::Average { count: 16 });
        assert_eq!(a, include_str!("../../testdata/avg-writes.txt"));
        let n = plan(Mode::Normal);
        let (a, n): (Vec<&str>, Vec<&str>) = (a.lines().collect(), n.lines().collect());
        // inserted: the clear pulse first, 0x1110 behind 0x1108
        assert_eq!(&a[..2], ["0x10b4 0x1", "0x10b4 0x0"]);
        let i = a.iter().position(|l| l.starts_with("0x1108 ")).unwrap();
        assert_eq!(a[i + 1], "0x1110 0x0");
        let mut b = a.clone();
        b.drain(i + 1..i + 2);
        b.drain(..2);
        assert_eq!(b.len(), n.len());
        let diff: Vec<(&str, &str)> = b.iter().zip(&n).filter(|(x, y)| x != y).map(|(x, y)| (*x, *y)).collect();
        assert_eq!(
            diff,
            [
                ("0x1004 0x4000008", "0x1004 0x4000000"),
                ("0x10b0 0xf", "0x10b0 0x1"),
                ("0x404c 0xa", "0x404c 0x5"),
                ("0x10e0 0x200", "0x10e0 0x100"),
            ]
        );
    }

    /// One batch against the model; returns (triggers recorded, the step word of point 0 of the newest frame).
    fn batch(st: &mut State, s: &Settings, p: &crate::dho::hz::Params, bar: &mut crate::dho::seg::sim::SimBus, c2h: &mut crate::dho::seg::sim::SimC2h, dma: &mut DmaBuf) -> (u32, u16) {
        let count = seq::acquire_batch(st, s, p, bar, c2h, dma, false, 0, |_| {}).unwrap();
        // every recorded frame is played, 1000 words each; the newest is the last
        let fb = seq::frame_bytes(s, p);
        assert_eq!(dma.data().len(), count as usize * fb);
        let at = (count as usize - 1) * fb;
        let w = u16::from_le_bytes([dma.data()[at], dma.data()[at + 1]]);
        (count, w - Engine::raw(0, 0))
    }

    // End to end against the model: configure (clear), batches of 4 triggers, every frame played, the newest shown, the
    // hold tracker's clear. Break it catches: showing frame 0 instead of the newest, a play of fewer frames than
    // recorded, no clear after a configuration or a complete hold average, the software path seeing raw frames.
    #[test]
    fn averaged_batches_against_the_model() {
        for kind in [Kind::Running, Kind::Hold] {
            let s = settings(Mode::Average { count: 16 });
            let mut st = State::handover(1, None);
            let (cfg, p) = st.configure(&s, None, None).unwrap();
            assert_eq!((p.seg, p.max_frames), (512, 14_336));
            let (eng, mut bar, mut c2h) = engine(Engine::new(0, 1_000_000_000_000, 4, p.ratio));
            eng.borrow_mut().avg_hold = kind == Kind::Hold;
            // triggers from before the configuration do not count
            eng.borrow_mut().triggers = 100;
            seq::execute_bus(&cfg, &mut bar, &mut c2h, &mut DmaBuf::new(), |_| {}).unwrap();
            let mut dma = DmaBuf::new();
            let mut tr = Tracker::new(kind, 16);
            let mut seen = Vec::new();
            for _ in 0..6 {
                let (count, step) = batch(&mut st, &s, &p, &mut bar, &mut c2h, &mut dma);
                assert_eq!(count, 4);
                let a = tr.batch(count);
                seen.push((step, a.show));
                if a.clear {
                    seq::execute_bus(&st.avg_clear_steps(), &mut bar, &mut c2h, &mut DmaBuf::new(), |_| {}).unwrap();
                    tr.reset();
                }
            }
            // the step of trigger g is (g % 16) x 4; the first trigger after the configuration is 100 (step 16)
            let want: Vec<(u16, bool)> = match kind {
                // running: means over 100..103, 100..107, ..., then always 16 consecutive triggers (30)
                Kind::Running => vec![(22, true), (30, true), (38, true), (30, true), (30, true), (30, true)],
                // hold: the same until 16 are in, shown then, cleared; the next average starts at trigger 116
                Kind::Hold => vec![(22, false), (30, false), (38, false), (30, true), (22, false), (30, false)],
            };
            assert_eq!(seen, want, "{:?}", kind);
            // a new configuration (count 4) clears again: the next frame averages only its own batch
            let s4 = settings(Mode::Average { count: 4 });
            let (cfg, p4) = st.configure(&s4, None, None).unwrap();
            seq::execute_bus(&cfg, &mut bar, &mut c2h, &mut DmaBuf::new(), |_| {}).unwrap();
            let first = eng.borrow().triggers;
            let (_, step) = batch(&mut st, &s4, &p4, &mut bar, &mut c2h, &mut dma);
            let g: Vec<u16> = (first..first + 4).map(|g| (g % 16) as u16 * 4).collect();
            assert_eq!(step, g.iter().sum::<u16>() / 4);
        }
    }

    #[test]
    fn combinations_are_refused() {
        let mut st = State::handover(1, None);
        let s = settings(Mode::Average { count: 16 });
        assert!(st.configure(&Settings { la: Some(Default::default()), ..s.clone() }, None, None).is_err());
        assert!(st.configure(&Settings { segments: Some(seq::Segments { n: 10, interval: 0.0 }), ..s.clone() }, None, None).is_err());
        // 125 M points do not fit the average ring
        assert!(st.configure(&Settings { depth: 125_000_000, tdiv: 1.0, ..s }, None, None).is_err());
    }

    #[test]
    fn counts_the_hardware_takes() {
        assert_eq!([1, 2, 3, 16, 1024, 2048].map(hw_count), [None, Some(2), None, Some(16), Some(1024), None]);
    }

    // Break it catches: a hold average shown before it is complete or never cleared, a running one cleared, triggers
    // past the count.
    #[test]
    fn running_and_hold_batches() {
        let mut r = Tracker::new(Kind::Running, 16);
        assert_eq!(r.batch(5), After { show: true, clear: false, triggers: 5 });
        assert_eq!(r.batch(20), After { show: true, clear: false, triggers: 16 });
        r.reset();
        assert_eq!(r.batch(1).triggers, 1);
        let mut h = Tracker::new(Kind::Hold, 16);
        assert_eq!(h.batch(10), After { show: false, clear: false, triggers: 10 });
        assert_eq!(h.batch(10), After { show: true, clear: true, triggers: 16 });
        assert_eq!(h.batch(3), After { show: false, clear: false, triggers: 3 });
        h.reset();
        assert!(!h.batch(15).show);
        assert_eq!(h.batch(1), After { show: true, clear: true, triggers: 16 });
    }

    // Break it catches: an accumulator that overflows at 1024 frames, a shift that loses the sub-LSB steps, a scale
    // other than the normal 7500 codes per division for averaged words.
    #[test]
    fn averaged_words_scale_like_raw_ones() {
        // bit growth: 1024 frames of the largest code need 26 bits; the shift brings them back to 16
        assert_eq!((acc_bits(1), acc_bits(2), acc_bits(16), acc_bits(1024)), (16, 17, 20, 26));
        let full = vec![0xffffu16; 4];
        let many: Vec<&[u16]> = (0..1024).map(|_| &full[..]).collect();
        assert_eq!(mean_words(&many), full);
        assert!(1024u64 * 0xffff < 1 << acc_bits(1024));
        // one ADC LSB is about 14.6 codes (12-bit ADC over ~8 divisions of 7500 codes): an average of codes 15 apart
        // keeps the step between them, truncated to whole codes
        let (a, b) = (vec![32768u16; 3], vec![32783u16; 3]);
        assert_eq!(mean_words(&[&a, &b]), [32775; 3]);
        assert_eq!(mean_words(&[&a, &a, &a, &b]), [32771; 3]);
        // to volts with the normal conversion: the average of the volts within one code (1 / 7500 div)
        let (scale, offset) = (0.5, 0.1);
        let raw: Vec<Vec<u16>> = (0..16u16).map(|k| (0..8u16).map(|i| 30_000 + 37 * k + 101 * i).collect()).collect();
        let refs: Vec<&[u16]> = raw.iter().map(|v| &v[..]).collect();
        let avg = mean_words(&refs);
        let bytes: Vec<u8> = avg.iter().flat_map(|w| w.to_le_bytes()).collect();
        let hw = seq::decode_volts(&bytes, 1, [(scale, offset); 4])[0].clone();
        for (i, v) in hw.iter().enumerate() {
            let sw = raw.iter().map(|f| seq::volts(&f[i..i + 1], scale, offset)[0] as f64).sum::<f64>() / 16.0;
            assert!((*v as f64 - sw).abs() <= scale / 7500.0 + 1e-6, "point {}: {} vs {}", i, v, sw);
        }
    }
}
