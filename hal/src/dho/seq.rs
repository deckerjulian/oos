// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Complete acquisition sequences: settings -> ordered steps (register writes, AFE frames, polls, sleeps, the C2H
//! read), a persistent state with all shadows, the executor for the unit, and frame decoding. Shared by `oos-fpgad`
//! and the hardware source of `oos-ui`. Order (docs/protocols): [ADC mode] -> [AFE] -> TPU edge trigger -> wait FSM
//! stopped -> acquisition configuration -> SCU auto/normal trigger -> arm -> wait done -> play-last read-out -> read.

use crate::dho::{acq, adc, afe, auxout, hz, la, mask, scu, tpu, zone, Write};

#[derive(Clone, Debug)]
pub enum Step {
    Wr(Write),
    Afe(afe::Op),
    /// wait until (reg & bit) != 0, at most `ms`
    Poll { reg: u32, bit: u32, ms: u64, what: &'static str },
    /// read-only diagnostic: log the values of these registers
    Peek(&'static [u32]),
    /// `frames` frames of `frame` bytes each; in the stream every frame is padded to `frame_stride(frame)` (8 bytes),
    /// the read takes the padding too and drops it
    Read { frames: usize, frame: usize },
    Sleep { us: u32 },
    Note(String),
}

/// Input coupling of a channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Couple {
    Dc,
    Ac,
    /// done in the FPGA, the AFE keeps its state
    Gnd,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chan {
    /// V/div at the probe tip, the volts of the samples and the trigger levels (the BNC when `probe` is 1)
    pub scale: f64,
    /// offset (vertical position) in V, at the same place
    pub offset: f64,
    /// bandwidth value (`afe::bw`)
    pub bw: u32,
    /// probe ratio: the AFE works at the BNC and gets scale and offset divided by this (afe-settings.md 3.7)
    pub probe: f64,
    /// zero correction in V at the same place: the AFE offset takes it, samples and trigger levels stay as they are
    pub bias: f64,
    /// deskew in s, into the sample delays (cold-boot.md 3.10)
    pub skew: f64,
    pub couple: Couple,
    /// the FPGA inverts the samples, the AFE offset is mirrored (afe-settings.md 3.5)
    pub invert: bool,
}

/// Hardware trigger type (docs/protocols/trigger.md sections 2 and 10). Source, slope and level of `Settings` are the
/// main source / edge / (upper) level; times in seconds, levels in volts.
#[derive(Clone, Debug, PartialEq)]
pub enum Trig {
    Edge,
    Pulse { pol: tpu::Polarity, when: tpu::When, lower: f64, upper: f64 },
    Slope { when: tpu::When, lower: f64, upper: f64, lo: f64 },
    Runt { pol: tpu::Polarity, when: tpu::When, lower: f64, upper: f64, lo: f64 },
    Window { event: tpu::WindowEvent, time: f64, lo: f64 },
    Timeout { time: f64 },
    Delay { srcb: u32, eb: tpu::Edge, when: tpu::When, lower: f64, upper: f64, levelb: f64 },
    /// `pats` CH1..CH4, `la` D0..D15 (TPU pattern slots 4..19, trigger.md 10.8; all X without the LA)
    Pattern { pats: [tpu::Pat; 4], la: [tpu::Pat; 16], levels: [f64; 4] },
    Duration { pats: [tpu::Pat; 4], la: [tpu::Pat; 16], when: tpu::When, lower: f64, upper: f64, levels: [f64; 4] },
    SetupHold { clk: u32, clk_edge: tpu::Edge, setup: f64, hold: f64, levelc: f64 },
    Nth { count: u32, idle: f64 },
    Video { std: tpu::VideoStd, sync: tpu::VideoSync, line: u32 },
    /// the TPU's UART receiver on the main source at the main level (docs/fork/hwuart.md)
    Uart(crate::dho::uarttrig::Uart),
    /// the TPU's SPI and I2C receivers on their own lines at the main level (docs/fork/hwserial.md)
    Spi(crate::dho::serialtrig::Spi),
    I2c(crate::dho::serialtrig::I2c),
}

/// The holdoff of a trigger set-up without one.
pub const HOLDOFF_MIN: f64 = 8e-9;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// trigger holdoff, seconds (`HOLDOFF_MIN` = none)
    pub holdoff: f64,
    /// time in the middle of the record relative to the trigger, seconds; kept within half a record
    pub delay: f64,
    pub mode: acq::Mode,
    /// enabled channels, bit0 = CH1
    pub mask: u32,
    pub tdiv: f64,
    /// memory depth (points per channel), 0 = Auto
    pub depth: i128,
    pub ch: [Chan; 4],
    /// program the AFEs of the enabled channels (else they keep the stock set-up)
    pub afe: bool,
    pub trig_src: u32,
    pub slope: tpu::Slope,
    pub level: f64,
    pub auto: bool,
    /// AUTO sweep with the shortest auto-trigger time (10 ms) whatever the time base: the auto set-up reads the
    /// levels of a signal its trigger level may not cross yet, and must not wait 20 time/div per record for it
    pub auto_fast: bool,
    /// SINGLE: the UI takes the first frame after the press and stops; a batch of more frames records triggers nobody
    /// sees (oos-ui plans one frame per batch, `hw::batch_plan`)
    pub single: bool,
    pub trigger: Trig,
    /// points per channel to read, from memory point pre + `at` (None = the whole record)
    pub window: Option<(u32, i64)>,
    /// read-out with the stock type-4 play and the pass-through writes before it
    /// (the combination that delivered own frames on the unit, 2026-11-03); false = the play-last read-out of the warm
    /// hand-over, which only works right after a raw read of the stock system
    pub play4: bool,
    /// configuration blocks written before the arm: bit0 trigger, bit1 acquisition, bit2 auto trigger (7 = all;
    /// 0 = arm with the configuration the stock system left behind; diagnostic)
    pub blocks: u8,
    /// read with the 16-byte frame header like the stock live loop (0x1010 bit31 = 0) instead of the export mode
    pub header: bool,
    /// noise reject: 0.7 instead of 0.3 div comparator hysteresis (trigger.md 2.3, 2.7)
    pub noise_reject: bool,
    /// coupling of the edge trigger (trigger.md 2.5); the other types are always DC
    pub trig_couple: tpu::Coupling,
    /// frames per batch: one arm records this many triggered frames, one type-4 play and one read return them all
    /// (limited to `Params::max_frames`; needs `play4`)
    pub frames: u32,
    /// FPGA compression between memory and TX (0x1170, the stock live display path): k > 1 reads
    /// the whole record as ceil(memsel / k) points per channel; 1 = pass-through
    pub compress: u32,
    /// logic analyser on (la.md, UNTESTED on the unit): LA set-up after the acquisition configuration (halved analog rate with a channel
    /// shown, LA doubling, rate block, thresholds), and after the analog read a second, raw LA read (`la_read_steps`).
    /// None = the analog path exactly as without the LA.
    pub la: Option<la::Set>,
    /// hardware segmented recording (docs/fork/specs/03-segmented-timestamps.md): one arm records this many frames
    /// with FPGA re-arm and per-frame trigger timestamps (`seg`). None = the live batches.
    pub segments: Option<Segments>,
    /// what the rear AUX OUT BNC carries (docs/fork/auxout.md); written with the acquisition block when it changed
    pub aux: auxout::AuxOut,
    /// the FPGA mask test (docs/fork/hwmask.md); None = off
    pub hwmask: Option<HwMask>,
    /// the FPGA zone trigger (docs/fork/hwzone.md); None = off. It shares the mask unit's slot and strobe, so it stays
    /// off while `hwmask` is set
    pub hwzone: Option<HwZone>,
}

/// Zone qualification in the FPGA: up to two zones on channel `ch`.
#[derive(Clone, Debug, PartialEq)]
pub struct HwZone {
    pub ch: usize,
    pub zones: Vec<zone::Zone>,
    /// raised to clear the qualified-frame counter
    pub clear: u64,
    /// value and column mapping (shared with the mask unit)
    pub map: mask::Map,
}

/// A mask test in the FPGA: the volt band of each of the `mask::COLUMNS` columns on channel `ch`.
#[derive(Clone, Debug, PartialEq)]
pub struct HwMask {
    /// source channel 0..3; the test is off while it is not acquired
    pub ch: usize,
    /// lower and upper limit per column, volts at the probe tip (infinite = no limit)
    pub lo: Vec<f32>,
    pub hi: Vec<f32>,
    /// the FSM stops on a failed frame
    pub stop_on_fail: bool,
    /// raised to clear the counters
    pub clear: u64,
    /// value and column mapping of the comparator (`mask::Map`, the debug sweep)
    pub map: mask::Map,
}

/// A hardware segmented recording.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segments {
    /// frames to record (limited to `hz::max_record_frames`)
    pub n: u32,
    /// wait after each frame before the next arm, seconds (0x4068; 0 = back to back)
    pub interval: f64,
}

impl Default for Settings {
    /// The warm hand-over of 2026-11-02: CH1 1 V/div, 200 us/div, Auto depth, edge CH1 rising 1.5 V, read the
    /// verified window (4000 points from 1,248,000).
    fn default() -> Settings {
        let c = Chan { scale: 1.0, offset: 0.0, bw: afe::bw::OFF, probe: 1.0, bias: 0.0, skew: 0.0, couple: Couple::Dc, invert: false };
        Settings { delay: 0.0, mode: acq::Mode::Normal, holdoff: HOLDOFF_MIN, mask: 1, tdiv: 200e-6, depth: 0, ch: [c; 4], afe: false, trig_src: 1, slope: tpu::Slope::Rising, level: 1.5, auto: true, auto_fast: false, single: false, trigger: Trig::Edge, window: Some((4000, -2000)), play4: true, blocks: 7, header: false, noise_reject: false, trig_couple: tpu::Coupling::Dc, frames: 1, compress: 1, la: None, segments: None, aux: auxout::AuxOut::TriggerOut, hwmask: None, hwzone: None }
    }
}

/// Everything the FPGA/ADC/AFE do not read back, kept across acquisitions.
pub struct State {
    pub sh: scu::Shadows,
    pub ts: tpu::Shadows,
    pub afe: [afe::Afe; 4],
    pub adc: adc::Adc,
    pub mode: usize,
    /// FPGA digital gain of the trigger source channel (cal_vertical record), 1.0 when OOS did not set the AFE
    pub gain: [f64; 4],
    /// the LA was on in the last set-up: 0x131c goes back when it is off again
    pub la_was_on: bool,
    /// a holdoff other than `HOLDOFF_MIN` is in the TPU
    pub holdoff_set: bool,
    /// the AUX OUT set-up last written; None = not written since the hand-over, so the first set-up writes it
    pub aux: Option<auxout::AuxOut>,
    /// the mask unit: table last loaded, (slot, samples per column, clear, stop on fail) of the running test (None =
    /// off), and the control shadow 0x1190
    pub mask_cols: Option<Vec<mask::Column>>,
    pub mask_run: Option<(u32, u32, u64, bool)>,
    pub s1190: u32,
    /// the zone unit: tables last loaded, and (slot, samples per column, control word, clear) of the running zone
    /// qualification (None = off)
    pub zone_tabs: [Option<Vec<zone::Column>>; 2],
    pub zone_run: Option<(u32, u32, u32, u64)>,
}

impl State {
    /// State after a warm hand-over: the stock values, ADC in the mode of `app_mask`.
    pub fn handover(app_mask: u32, cal_adc: Option<&adc::CalAdc>) -> State {
        let mode = adc::mode_of_mask(app_mask).unwrap_or(0);
        let mut a = adc::Adc::default();
        if let Some(c) = cal_adc {
            a.parameter(c, 0);
            if mode != 0 {
                a.channel_mode(c, mode);
            }
        }
        State { sh: scu::Shadows::HANDOVER, ts: tpu::Shadows::APP, afe: [0, 1, 2, 3].map(afe::Afe::new), adc: a, mode, gain: [1.0; 4], la_was_on: false, holdoff_set: false, aux: None, mask_cols: None, mask_run: None, s1190: 0, zone_tabs: [None, None], zone_run: None }
    }

    /// Configuration steps for `s` (ADC mode if the channel set changed, AFEs, trigger, acquisition registers).
    pub fn configure(&mut self, s: &Settings, cal: Option<&afe::Cal>, cal_adc: Option<&adc::CalAdc>) -> Result<(Vec<Step>, hz::Params), String> {
        // trigger source: a channel that is acquired, or LA line 9..24 (only with the LA on)
        let on_la = (9..=24).contains(&s.trig_src) && s.la.is_some();
        if !(1..=15).contains(&s.mask) || (!on_la && (!(1..=4).contains(&s.trig_src) || s.mask & (1 << (s.trig_src - 1)) == 0)) {
            return Err("channel mask 1..0xf with the trigger source enabled".into());
        }
        let mut st = Vec::new();
        let mut p = hz::prep(s.mask, tdiv_fs(s.tdiv), s.depth, 0);
        // with the LA on the record stays centred: its rate rule comes after this one and is not worked out for it
        if s.delay != 0.0 && s.la.is_none() {
            let half = p.dot_fs as i128 * p.memsel as i128 / 2;
            p = hz::prep(s.mask, tdiv_fs(s.tdiv), s.depth, time_fs(s.delay).clamp(-half, half));
        }
        if let Some(l) = &s.la {
            p = la::prep(p, l.analog_shown);
        }
        if let acq::Mode::Average { count } = s.mode {
            // spec 01, 6.5: the library does not prevent these combinations, OOS refuses them
            if s.la.is_some() || s.segments.is_some() {
                return Err("hardware average with the LA or hardware segments".into());
            }
            p = hz::avg_params(p);
            if p.max_frames == 0 {
                return Err(format!("hardware average: {} points do not fit the average layout", p.memsel));
            }
            st.push(Step::Note(format!("hardware average of {} (0x10b0 = {})", count, acq::avg_count_reg(count))));
        }
        st.push(Step::Note(format!("mask {:#x}: ratio {}, {} Sa/s, {} points/channel, segment {:#x}, pre {} post {}", s.mask, p.ratio, p.rate, p.memsel, p.seg, p.pre, p.post)));
        let mode = adc::mode_of_mask(s.mask).unwrap();
        if mode != self.mode {
            let c = cal_adc.ok_or("a channel set different from the current ADC mode needs cal_adc.hex")?;
            st.push(Step::Note(format!("ADC mode {} -> {}", self.mode, mode)));
            for w in self.adc.channel_mode(c, mode) {
                if w.pre_us > 0 {
                    st.push(Step::Sleep { us: w.pre_us });
                }
                st.push(Step::Wr((w.reg, w.val)));
                if w.post_us > 0 {
                    st.push(Step::Sleep { us: w.post_us });
                }
            }
            for (_, v) in adc::wpu_gain(s.mask, [1; 4], 0x5555) {
                st.push(Step::Wr((0x105c, v)));
            }
            self.mode = mode;
        }
        if s.afe {
            let cal = cal.ok_or("AFE set-up needs the calibration files (cal_vertical/zero/bandwidth/lsb.hex)")?;
            for c in 0..4 {
                if s.mask & (1 << c) == 0 {
                    continue;
                }
                let k = &s.ch[c];
                // the correction is on the input side, so an inverted channel has it the other way round
                let bias = if k.invert { -k.bias } else { k.bias };
                let dc = match k.couple {
                    Couple::Dc => Some(true),
                    Couple::Ac => Some(false),
                    Couple::Gnd => None,
                };
                let (ops, rec, code) = afe::configure_ch(&mut self.afe[c], cal, (k.scale / k.probe * 1e9) as i64, ((k.offset - bias) / k.probe * 1e9) as i64, k.bw, p.rate, s.mask, k.invert, dc);
                st.push(Step::Note(format!("AFE CH{}: {} V/div, digital gain {:.5}, offset DAC {}", c + 1, k.scale, rec.gain, code)));
                self.gain[c] = rec.gain;
                st.extend(ops.into_iter().map(Step::Afe));
            }
        }
        if s.afe {
            // read-path gain/offset from the digital gains of the AFE records (else they keep the stock values)
            st.push(Step::Note("read-path gain/offset".into()));
            let gnd = s.ch.map(|k| (k.couple == Couple::Gnd).then(|| (k.offset / k.scale * 7500.0 + 32768.0) as i32 as u32));
            st.extend(crate::dho::wpu::analog_gnd(s.mask, self.gain, gnd).into_iter().map(Step::Wr));
        }
        if s.blocks & 7 != 7 {
            st.push(Step::Note(format!("configuration blocks {:#x} (bit0 trigger, bit1 acquisition, bit2 auto trigger)", s.blocks)));
        }
        if s.blocks & 1 != 0 {
            st.extend(self.trigger(s).into_iter().map(Step::Wr));
        }
        if s.blocks & 2 != 0 {
            st.push(Step::Poll { reg: 0x4034, bit: scu::status::STOPPED, ms: 30, what: "FSM stopped (stop poll)" });
            if matches!(s.mode, acq::Mode::Average { .. }) {
                // stock RunConfig: stop, average clear, then the configuration (spec 01, 1.5)
                st.push(Step::Note("average clear".into()));
                st.extend(acq::avg_clear().into_iter().map(Step::Wr));
            }
            st.push(Step::Note("acquisition configuration".into()));
            let deskew = s.la.as_ref().map(|_| la::deskew(la::timing(&p, s.mask).m));
            let skew = s.ch.map(|k| (k.skew * 1e15).round() as i64);
            let invert = (0..4).filter(|&c| s.ch[c].invert).fold(0, |a, c| a | 1 << c);
            st.extend(acq::configure_la(&mut self.sh, &p, s.mask, invert, 0, s.mode, deskew, skew).into_iter().map(Step::Wr));
            if std::mem::replace(&mut self.la_was_on, s.la.is_some()) && s.la.is_none() {
                st.push(Step::Wr(la::wave_early(None)));
            }
            if s.la.is_some() {
                let t = la::timing(&p, s.mask);
                st.push(Step::Note(format!("LA on: m {}, {} Sa/s, {} LA samples, trigger at {}", t.m, t.rate, t.n, t.pre)));
                st.extend(la::config(&mut self.sh, &p, s.mask, s.la.as_ref().is_some_and(|l| l.roll)).into_iter().map(Step::Wr));
            }
            if self.aux != Some(s.aux) {
                st.push(Step::Note(format!("AUX OUT {:?}", s.aux)));
                st.extend(auxout::writes(&s.aux).into_iter().map(Step::Wr));
                self.aux = Some(s.aux);
            }
            st.extend(self.zone_steps(s, &p));
            st.extend(self.mask_steps(s, &p));
        }
        if let Some(l) = &s.la {
            // thresholds with every set-up (la.md 8.1: on change and once after the start)
            st.push(Step::Wr(la::thresholds(&l.cal, l.thr[0], l.thr[1])));
        }
        if s.blocks & 4 != 0 {
            st.push(Step::Note(format!("SCU {} trigger", if s.auto { "auto" } else { "normal" })));
            // a time base of 0 gives the shortest auto times (t1 10 ms, t2 100 ms)
            let tb = if s.auto_fast { 0 } else { tdiv_fs(s.tdiv) };
            st.extend(scu::auto_trigger(&mut self.sh, s.auto, tb, time_fs(s.holdoff)).into_iter().map(Step::Wr));
        }
        Ok((st, p))
    }

    /// The mask unit for `s` (docs/fork/hwmask.md): nothing while it runs as wanted; else off, the table when it
    /// changed, compression, slot (counters cleared on a new slot, a new start or a clear request) and on again; off
    /// when it is not wanted or its channel is not acquired. Called in the acquisition block with the FSM stopped.
    fn mask_steps(&mut self, s: &Settings, p: &hz::Params) -> Vec<Step> {
        let want = s.hwmask.as_ref().and_then(|m| mask::slot(s.mask, m.ch).map(|sl| (m, sl)));
        let Some((m, slot)) = want else {
            if self.mask_run.take().is_none() {
                return vec![];
            }
            self.sh.s4020 = mask::scu_bits(self.sh.s4020, false, false);
            let mut w = vec![Step::Note("mask test off".into()), Step::Wr((0x4020, self.sh.s4020))];
            // the zone trigger keeps the slot and the strobe it set up
            if self.zone_run.is_none() {
                self.s1190 &= !mask::ctrl::SLOT_MASK;
                w.extend([Step::Wr((mask::COMPRESS, mask::compress::OFF)), Step::Wr((mask::CTRL, self.s1190))]);
            }
            return w;
        };
        let k = &s.ch[m.ch];
        // the software mask spreads over the screen: 10 divisions around the delay
        let t = mask::Timing { points: p.memsel, pre: p.pre, rate: p.rate, mask: s.mask, screen: (s.delay - 5.0 * s.tdiv, 10.0 * s.tdiv) };
        let cols = mask::table_map(&m.lo, &m.hi, &t, k.scale, k.offset, self.gain[m.ch], &m.map);
        let run = (slot, m.map.layout(p.memsel, s.mask).1, m.clear, m.stop_on_fail);
        if self.mask_run == Some(run) && self.mask_cols.as_ref() == Some(&cols) {
            return vec![];
        }
        let clear = self.mask_run.map_or(true, |r| r.0 != run.0 || r.2 != run.2);
        let mut w = vec![];
        self.sh.s4020 = mask::scu_bits(self.sh.s4020, false, false);
        w.push((0x4020, self.sh.s4020));
        if self.mask_cols.as_ref() != Some(&cols) {
            w.extend(mask::load(&cols));
            self.mask_cols = Some(cols);
        }
        w.push((mask::COMPRESS, mask::compress_word(run.1)));
        w.extend(mask::select(&mut self.s1190, slot, clear));
        self.sh.s4020 = mask::scu_bits(self.sh.s4020, true, m.stop_on_fail);
        w.push((0x4020, self.sh.s4020));
        self.mask_run = Some(run);
        let mut st = vec![Step::Note(format!("mask test CH{} slot {}, {} samples per column{}", m.ch + 1, slot, run.1, if clear { ", counters cleared" } else { "" }))];
        st.extend(w.into_iter().map(Step::Wr));
        st
    }

    /// The zone unit for `s` (docs/fork/hwzone.md), like `mask_steps`: nothing while it runs as wanted; else off, the
    /// tables that changed, control, compression, slot (the counter cleared on a new slot, a new start or a clear
    /// request) and armed; off when it is not wanted, the mask test is wanted, or its channel is not acquired.
    fn zone_steps(&mut self, s: &Settings, p: &hz::Params) -> Vec<Step> {
        let want = s.hwzone.as_ref().filter(|z| s.hwmask.is_none() && !z.zones.is_empty()).and_then(|z| mask::slot(s.mask, z.ch).map(|sl| (z, sl)));
        let Some((z, slot)) = want else {
            if self.zone_run.take().is_none() {
                return vec![];
            }
            self.sh.s4020 = zone::scu_bit(self.sh.s4020, false);
            let mut w = vec![Step::Note("zone trigger off".into()), Step::Wr((0x4020, self.sh.s4020)), Step::Wr((zone::CTRL, 0))];
            if self.mask_run.is_none() {
                self.s1190 &= !mask::ctrl::SLOT_MASK;
                w.extend([Step::Wr((mask::COMPRESS, mask::compress::OFF)), Step::Wr((mask::CTRL, self.s1190))]);
            }
            return w;
        };
        let k = &s.ch[z.ch];
        let tabs: Vec<Vec<zone::Column>> = z.zones.iter().take(2).map(|x| zone::table(x, p.memsel, p.rate, p.pre, s.mask, k.scale, k.offset, self.gain[z.ch], &z.map)).collect();
        let n = z.map.layout(p.memsel, s.mask).1;
        let run = (slot, n, zone::ctrl_word(&z.zones), z.clear);
        if self.zone_run == Some(run) && tabs.iter().enumerate().all(|(i, t)| self.zone_tabs[i].as_ref() == Some(t)) {
            return vec![];
        }
        let clear = self.zone_run.map_or(true, |r| r.0 != run.0 || r.3 != run.3);
        let mut w = vec![];
        self.sh.s4020 = zone::scu_bit(self.sh.s4020, false);
        w.push((0x4020, self.sh.s4020));
        for (i, t) in tabs.into_iter().enumerate() {
            if self.zone_tabs[i].as_ref() != Some(&t) {
                w.extend(zone::load(i, &t));
                self.zone_tabs[i] = Some(t);
            }
        }
        w.push((zone::CTRL, run.2));
        w.push((mask::COMPRESS, mask::compress_word(n)));
        w.extend(mask::select(&mut self.s1190, slot, clear));
        self.sh.s4020 = zone::scu_bit(self.sh.s4020, true);
        w.push((0x4020, self.sh.s4020));
        self.zone_run = Some(run);
        let mut st = vec![Step::Note(format!("zone trigger CH{} slot {}, control {:#x}{}", z.ch + 1, slot, run.2, if clear { ", counter cleared" } else { "" }))];
        st.extend(w.into_iter().map(Step::Wr));
        st
    }

    /// The control shadow while the zone trigger runs (for `zone::read`), None when it is off.
    pub fn zone_ctrl(&self) -> Option<u32> {
        self.zone_run.map(|_| self.s1190)
    }

    /// The control shadow while the mask test runs (for `mask::read`), None when it is off.
    pub fn mask_ctrl(&self) -> Option<u32> {
        self.mask_run.map(|_| self.s1190)
    }

    /// TPU writes of the trigger in `s` (trigger.md: section 6 edge, section 10 the other types).
    fn trigger(&mut self, s: &Settings) -> Vec<Write> {
        let fs = time_fs;
        let nv = |v: f64| v * 1e9;
        let (src, ts) = (s.trig_src, &mut self.ts);
        // LA lines have no level: any entry will do
        let tc = if src <= 4 { (src - 1) as usize } else { 0 };
        // the hysteresis comes from the scale of the source at the BNC, and multiplies the scale of each channel
        let noise = tpu::noise_hysteresis(nv(s.ch[tc].scale / s.ch[tc].probe), s.noise_reject);
        let vt: [tpu::Vert; 4] = [0, 1, 2, 3].map(|c| tpu::Vert { scale: nv(s.ch[c].scale), offset: nv(s.ch[c].offset), g: self.gain[c], noise });
        let edge = if s.slope == tpu::Slope::Falling { tpu::Edge::Falling } else { tpu::Edge::Rising };
        let mut w = match &s.trigger {
            Trig::Edge => tpu::edge_coupled(ts, src, s.slope, s.trig_couple, nv(s.level), vt[tc].offset, vt[tc].scale, vt[tc].g, noise, 8_000_000),
            Trig::Pulse { pol, when, lower, upper } => tpu::pulse(ts, src, *pol, *when, fs(*lower), fs(*upper), nv(s.level), &vt),
            Trig::Slope { when, lower, upper, lo } => tpu::slope(ts, src, edge, *when, fs(*lower), fs(*upper), nv(s.level), nv(*lo), &vt),
            Trig::Runt { pol, when, lower, upper, lo } => tpu::runt(ts, src, *pol, *when, fs(*lower), fs(*upper), nv(s.level), nv(*lo), &vt, true),
            Trig::Window { event, time, lo } => tpu::window(ts, src, s.slope, *event, fs(*time), nv(s.level), nv(*lo), &vt, true),
            Trig::Timeout { time } => tpu::timeout(ts, src, s.slope, fs(*time), nv(s.level), &vt),
            Trig::Delay { srcb, eb, when, lower, upper, levelb } => tpu::delay(ts, src, edge, *srcb, *eb, *when, fs(*lower), fs(*upper), nv(s.level), nv(*levelb), &vt),
            Trig::Pattern { pats, la, levels } => tpu::pattern(ts, &[&pats[..], &la[..]].concat(), tpu::Logic::And, &levels.map(nv), &vt),
            Trig::Duration { pats, la, when, lower, upper, levels } => tpu::duration(ts, &[&pats[..], &la[..]].concat(), tpu::Logic::And, *when, fs(*lower), fs(*upper), &levels.map(nv), &vt),
            Trig::SetupHold { clk, clk_edge, setup, hold, levelc } => {
                tpu::setup_hold(ts, *clk, src, *clk_edge, tpu::DataLevel::H, tpu::ShEvent::Both, fs(*setup), fs(*hold), nv(*levelc), nv(s.level), &vt)
            }
            Trig::Nth { count, idle } => tpu::nth(ts, src, edge, *count, fs(*idle), nv(s.level), &vt),
            Trig::Video { std, sync, line } => tpu::video(ts, src, *std, true, *sync, *line, nv(s.level), &vt, tpu::VideoClk::Fast),
            Trig::Uart(u) => tpu::uart(ts, src, u, nv(s.level), &vt),
            Trig::Spi(x) => tpu::spi(ts, x, nv(s.level), &vt),
            Trig::I2c(x) => tpu::i2c(ts, x, nv(s.level), &vt),
        };
        // the shortest holdoff is what every trigger type leaves behind; once changed, it is written until it is back
        let set = (s.holdoff - HOLDOFF_MIN).abs() > 1e-12;
        if set || self.holdoff_set {
            w.extend(tpu::holdoff(&mut self.ts, fs(s.holdoff)));
        }
        self.holdoff_set = set;
        w
    }

    /// One acquisition with the configured registers: arm, wait, read-out window, read.
    pub fn acquire(&mut self, s: &Settings, p: &hz::Params) -> Vec<Step> {
        let nf = frames(s, p);
        let mut st = self.arm_steps(s, p);
        st.extend(self.read_steps(s, p, nf));
        if s.la.is_some() {
            st.extend(self.la_read_steps(s, p));
        }
        st
    }

    /// The raw LA read after the analog read of the same acquisition (la.md 4.2, UNTESTED): pass-through LA chain,
    /// window of `la::window` words, play-last, one read, analog stream back on. Only the last frame of a batch has
    /// its LA (la.md 4.1), so the LA runs with one frame per batch.
    pub fn la_read_steps(&mut self, s: &Settings, p: &hz::Params) -> Vec<Step> {
        let t = la::timing(p, s.mask);
        let (n, start) = la::window(&t);
        let mut st = vec![Step::Note(format!("LA read-out: {} words from LA sample {} (trigger at {})", n, start, t.pre))];
        // wait for the stop before touching the play engine (at least 1.5 ms)
        st.push(Step::Poll { reg: 0x4034, bit: scu::status::STOPPED, ms: 30, what: "FSM stopped before the LA read" });
        st.extend(la::passthrough().into_iter().map(Step::Wr));
        st.extend(la::readout(&mut self.sh, n, start, p.ratio, s.mask).into_iter().map(Step::Wr));
        st.push(Step::Read { frames: 1, frame: la::read_bytes(n) });
        st.extend(la::restore(&mut self.sh).into_iter().map(Step::Wr));
        st
    }

    /// Arm a batch of `frames(s, p)` frames and wait for the done flag (= the first frame is complete; the FPGA goes
    /// on recording into the ring until the last frame or a stop, unit 2026-11-03).
    pub fn arm_steps(&mut self, s: &Settings, p: &hz::Params) -> Vec<Step> {
        let mut st = self.arm_only_steps(s, p);
        st.extend(wait_steps(p));
        st
    }

    /// The arm of `arm_steps` without the wait for done: what `prearm` issues ahead, before the host decodes the batch
    /// just read.
    pub fn arm_only_steps(&mut self, s: &Settings, p: &hz::Params) -> Vec<Step> {
        let nf = frames(s, p);
        let mut st = vec![Step::Note(format!("arm (frame 0, {} frame{})", nf, if nf == 1 { "" } else { "s" }))];
        st.extend(scu::arm(s.mask, &mut self.sh, 0, nf).into_iter().map(Step::Wr));
        st
    }

    /// Stops an acquisition armed ahead (`prearm`) that will not be read, before anything else writes the FPGA: the
    /// stock StopScope (wait up to 10 ms for 0x4034 bit13, then the stop sequence; the same 17 writes every arm starts
    /// with).
    pub fn disarm_steps(&mut self) -> Vec<Step> {
        let mut w = Vec::new();
        scu::stop_sequence(&mut w, &mut self.sh);
        let mut st = vec![Step::Note("disarm".into()), Step::Poll { reg: 0x4034, bit: scu::status::STOPPED, ms: 10, what: "FSM stopped before the disarm" }];
        st.extend(w.into_iter().map(Step::Wr));
        st
    }

    /// Restarts a hardware average (CLEAR, RUN after STOP, SINGLE, a complete hold average): wait for the FSM to stop,
    /// then the clear pulse (stock StopScope + setAvgClr). Between batches the read-out has already stopped the FSM.
    pub fn avg_clear_steps(&mut self) -> Vec<Step> {
        let mut st = vec![Step::Note("average clear".into()), Step::Poll { reg: 0x4034, bit: scu::status::STOPPED, ms: 10, what: "FSM stopped before the average clear" }];
        st.extend(acq::avg_clear().into_iter().map(Step::Wr));
        st
    }

    /// Read-out of the `count` frames a batch recorded, all of them in every mode: pass-through, window, stop sequence,
    /// type-4 play, one read. In average mode too, as the stock driver does: the FPGA's averaging sits on the play
    /// side (netlist: the next frame waits for the play-out of the one before), so a frame that is not played does not
    /// reach the average (B1, unit 2026-11-13: reading the newest frame alone left the noise near 0.5 mV at any
    /// count). The caller shows the newest.
    pub fn read_steps(&mut self, s: &Settings, p: &hz::Params, count: u32) -> Vec<Step> {
        self.read_steps_at(s, p, 0, count)
    }

    /// `read_steps` of `count` frames from frame `first` of the ring (the segments of a hardware segmented record).
    pub fn read_steps_at(&mut self, s: &Settings, p: &hz::Params, first: u32, count: u32) -> Vec<Step> {
        let nf = if s.play4 { count.max(1) } else { 1 };
        let mut st = Vec::new();
        let (n, start) = window(s, p);
        // the window counts words of all interleaved channels: n x ratio from point x ratio (Phase 0.5 step D)
        let win = scu::Window { n: n * p.ratio, start: start * p.ratio, ratio: p.ratio, fsm: s.mask, la: s.la.is_some(), avg: matches!(s.mode, acq::Mode::Average { .. }) };
        st.push(Step::Note(format!("read-out: {} points from memory point {} (trigger at {})", n, start, p.pre)));
        let mut ro = if s.play4 {
            let mut v = scu::passthrough();
            if s.compress > 1 {
                // main compression on, factor k (live value 0xc0000004 at k = 4); TX length = compressed points
                for w in v.iter_mut().filter(|w| w.0 == 0x1170) {
                    w.1 = compress_word(compress_k(s, p));
                }
            }
            let mut r = scu::readout_play4(&win, &mut self.sh, if s.play4 { first } else { 0 }, nf);
            if s.compress > 1 {
                let len = compressed_len(s, p);
                for w in r.iter_mut().filter(|w| w.0 == 0x11f0) {
                    w.1 = len;
                }
            }
            v.extend(r);
            v
        } else {
            scu::readout(&win, &mut self.sh)
        };
        if s.header {
            // frame header on: the FPGA puts a 16-byte header in front of the frame
            for w in ro.iter_mut().filter(|w| w.0 == 0x1010) {
                w.1 &= !(1 << 31);
            }
            self.sh.s1010 &= !(1 << 31);
        }
        st.extend(ro.into_iter().map(Step::Wr));
        st.push(Step::Peek(&[0x4034, 0x4080]));
        st.push(Step::Read { frames: nf as usize, frame: frame_bytes(s, p) });
        st
    }
}

impl State {
    /// Start of a hardware segmented recording (spec 03, 1.2): clear the first-trigger stamp, then the record arm of
    /// `record_frames(s, p)` frames with the interval of `s.segments`. Poll the progress with `seg::status`.
    pub fn record_steps(&mut self, s: &Settings, p: &hz::Params) -> Vec<Step> {
        let n = record_frames(s, p);
        let iv = s.segments.map_or(0, |g| scu::interval_ticks(time_fs(g.interval)));
        let mut st = vec![Step::Note(format!("segmented record: {} frames, interval {} x 6.4 ns", n, iv))];
        st.extend(scu::ts_first_clear(&mut self.sh).into_iter().map(Step::Wr));
        st.extend(scu::record_arm(s.mask, &mut self.sh, n, iv).into_iter().map(Step::Wr));
        st
    }

    /// The stamp play of segment `k` (spec 03, 1.4): a type-4 play of frame `k` alone with the smallest read-out window
    /// (`STAMP_WORDS` words of all channels, from the start of the record window, header on), its read, then the latch
    /// pulse. Read the stamp from 0x40b0/0x40b4 after it (`seg::read_latched`).
    pub fn segment_stamp_steps(&mut self, s: &Settings, p: &hz::Params, k: u32) -> Vec<Step> {
        let (_, start) = window(s, p);
        let win = scu::Window { n: STAMP_WORDS, start: start * p.ratio, ratio: p.ratio, fsm: s.mask, la: false, avg: false };
        let mut w = scu::passthrough();
        w.extend(scu::readout_play4(&win, &mut self.sh, k, 1));
        for x in w.iter_mut().filter(|x| x.0 == 0x1010) {
            x.1 &= !(1 << 31);
        }
        self.sh.s1010 &= !(1 << 31);
        let mut st = vec![Step::Note(format!("stamp play of segment {}", k))];
        st.extend(w.into_iter().map(Step::Wr));
        st.push(Step::Read { frames: 1, frame: STAMP_BYTES });
        st.extend(scu::ts_latch(&mut self.sh).into_iter().map(Step::Wr));
        st
    }
}

/// Words (all channels) of the stamp play: 128, the smallest window of the stock raw export (record-play.md 7.1: a
/// TX length of 8 points becomes 128 / ratio).
pub const STAMP_WORDS: u32 = 128;
/// Bytes of the stamp play's read: the 16-byte frame header and `STAMP_WORDS` words.
pub const STAMP_BYTES: usize = 16 + 2 * STAMP_WORDS as usize;

/// Frames of a hardware segmented recording: `Segments::n`, at least 1, at most `hz::max_record_frames`; 0 without
/// segments.
pub fn record_frames(s: &Settings, p: &hz::Params) -> u32 {
    s.segments.map_or(0, |g| g.n.clamp(1, hz::max_record_frames(p.seg)))
}

/// The wait of a batch after its arm: done (= the first frame is complete), then the ring registers for the log.
pub fn wait_steps(p: &hz::Params) -> Vec<Step> {
    // at least the record itself, twice over: slow time bases with the rate halved by the LA take seconds
    let record_ms = p.memsel as u64 * 1000 / p.rate.max(1);
    vec![Step::Poll { reg: 0x4080, bit: scu::PLAYINFO_DONE, ms: 2000.max(2 * record_ms + 500), what: "acquisition done" }, Step::Peek(&[0x4034, 0x4010, 0x4014])]
}

/// Frames of one batch: `Settings::frames`, at least 1, at most the frame ring (`max_frames`); 1 for play-last.
pub fn frames(s: &Settings, p: &hz::Params) -> u32 {
    if !s.play4 || s.la.is_some() {
        return 1;
    }
    s.frames.clamp(1, p.max_frames.max(1))
}

/// Bytes one frame of `frame` bytes takes in the C2H stream: the FPGA pads every frame of a play-out to 8 bytes (4
/// points at ratio 1). Unit 2026-11-04, 10 MHz sine, batches of 4..6 frames: 1250 points (100 ns/div) come at a
/// stride of 1252, 625 points at 628 (the phase is the same in every frame at that stride and walks by 2 / 3 samples a
/// frame at the unpadded one); 2500 points need no padding. Reading the unpadded length shifted every later frame
/// and left the padding in the stream for the next read.
pub fn frame_stride(frame: usize) -> usize {
    frame.div_ceil(8) * 8
}

/// Total bytes of a read of `frames` frames (`frame_stride` each).
pub fn read_len(frames: usize, frame: usize) -> usize {
    frames * frame_stride(frame)
}

/// Bytes of one frame in the read: the window of all interleaved channels (+ the 16-byte header in header mode).
pub fn frame_bytes(s: &Settings, p: &hz::Params) -> usize {
    let n = if s.compress > 1 && s.play4 { compressed_len(s, p) } else { window(s, p).0 };
    2 * (p.ratio * n) as usize + if s.header { 16 } else { 0 }
}

/// Largest compression factor: 0x1170 holds k in bits 13:0, a larger value wraps (netlist simulation, private RE notes
/// docs/re/netlist/blocks/datapath.md).
pub const COMPRESS_K_MAX: u32 = 16383;

/// The compression factor `s.compress` as it is written: 1..=`COMPRESS_K_MAX`, and at most memsel / 8, so a read has at
/// least 8 points.
pub fn compress_k(s: &Settings, p: &hz::Params) -> u32 {
    s.compress.clamp(1, COMPRESS_K_MAX).min((p.memsel / 8).max(1))
}

/// The 0x1170 word for factor `k`: k > 1 enables the min/max path with k in bits 13:0; k <= 1 is the pass-through
/// word. Enable with k = 0 stalls the read (no C2H word at all, netlist simulation), so it is never written.
pub fn compress_word(k: u32) -> u32 {
    match k.min(COMPRESS_K_MAX) {
        0 | 1 => 0x4000_0001,
        k => 0xc000_0000 | k,
    }
}

/// Points per channel after the FPGA compression (`compress_k`): the most whole multiples of 8 points (so whole min/max
/// pairs) whose samples lie inside the record, points x k <= memsel. The FPGA delivers exactly 0x11f0 points and waits
/// for samples until it has them: asking for more than the window holds stalls the read. The stock rounding up to 8
/// asked for 3912 points at k = 256 at 1 M (1 001 472 samples) and the c2h read timed out on the unit, while k = 128
/// (7816 points, 1 000 448 samples) still fitted the 0x248 samples the window reads beyond the record (unitlab round 2).
pub fn compressed_len(s: &Settings, p: &hz::Params) -> u32 {
    let k = compress_k(s, p);
    (p.memsel / k / 8 * 8).max(8)
}

/// Drops the stream padding behind every frame (`frame_stride`) in place: the frames end up back to back.
pub fn unpad(buf: &mut Vec<u8>, frame: usize) {
    let n = unpad_in(buf, frame);
    buf.truncate(n);
}

/// `unpad` on a slice: returns the bytes left at the front. Frames without padding (all live set-ups of oos-ui) cost
/// nothing.
pub fn unpad_in(buf: &mut [u8], frame: usize) -> usize {
    let stride = frame_stride(frame);
    if frame == 0 || stride == frame {
        return buf.len();
    }
    let n = buf.len() / stride;
    for k in 1..n {
        buf.copy_within(k * stride..k * stride + frame, k * frame);
    }
    n * frame
}

/// (points per channel, first memory point) of the read-out window.
pub fn window(s: &Settings, p: &hz::Params) -> (u32, u32) {
    match s.window {
        Some((n, at)) => {
            let n = n.min(p.memsel);
            (n, (p.pre as i64 + at).clamp(0, (p.memsel - n) as i64) as u32)
        }
        None => (p.memsel, 0),
    }
}

/// Splits a read-out (16-bit words, `ratio` channels interleaved) into channels: ratio 1 = the one enabled
/// channel; ratio 2 = lower channel on even words, higher on odd; ratio 4 = word 4i+k is CH k+1 (horizontal-config.md
/// 2.2). Channels not enabled stay empty.
pub fn deinterleave(bytes: &[u8], mask: u32) -> [Vec<u16>; 4] {
    let w: Vec<u16> = bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
    let on: Vec<usize> = (0..4).filter(|c| mask & (1 << c) != 0).collect();
    let mut out: [Vec<u16>; 4] = Default::default();
    match hz::ratio(mask) {
        1 => out[on[0]] = w,
        2 => {
            out[on[0]] = w.iter().step_by(2).copied().collect();
            out[on[1]] = w.iter().skip(1).step_by(2).copied().collect();
        }
        _ => {
            for c in on {
                out[c] = w.iter().skip(c).step_by(4).copied().collect();
            }
        }
    }
    out
}

/// `deinterleave` and `volts` in one pass, straight from the read-out bytes: `cal[c]` = (scale, offset) of CH c+1.
/// The same mapping and lengths as the two passes, in f32 (`volt32`), without the per-channel `u16` vectors. One
/// exact-size vector per enabled channel.
pub fn decode_volts(bytes: &[u8], mask: u32, cal: [(f64, f64); 4]) -> [Vec<f32>; 4] {
    let mut out: [Vec<f32>; 4] = Default::default();
    decode_volts_into(bytes, mask, cal, &mut out);
    out
}

/// `decode_volts` into kept vectors: the enabled channels are refilled in place, the others left empty. A frame that
/// fits their capacity allocates nothing (musl's malloc maps every block of 128 kB or more anew, with page faults).
pub fn decode_volts_into(bytes: &[u8], mask: u32, cal: [(f64, f64); 4], out: &mut [Vec<f32>; 4]) {
    let ratio = hz::ratio(mask) as usize;
    let slot = slots(mask, ratio);
    let group = 2 * ratio;
    let whole = bytes.len() / group;
    let tail = &bytes[whole * group..];
    for o in out.iter_mut() {
        o.clear();
    }
    for (k, c) in slot.iter().enumerate().take(ratio) {
        let Some(c) = *c else { continue };
        // the word of an incomplete last group, as deinterleave hands it out
        let last = tail.get(2 * k..2 * k + 2);
        let (a, off) = affine(cal[c]);
        let o = &mut out[c];
        o.reserve_exact(whole + last.is_some() as usize);
        // one pass per channel over its word of every group; the group size as a constant keeps it a vectorised loop
        match ratio {
            1 => lane::<2>(bytes, 0, a, off, o),
            2 => lane::<4>(bytes, k, a, off, o),
            _ => lane::<8>(bytes, k, a, off, o),
        }
        if let Some(b) = last {
            o.push(volt32(u16::from_le_bytes([b[0], b[1]]), a, off));
        }
    }
}

/// Where a read-out holds each channel without decoding it (fork, docs/fork/perf-plan.md "codes"): words per group,
/// the word of each channel in its group (`decode_volts_into`'s mapping) and `volt_code`'s (volts per code, offset).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CodeMap {
    pub ratio: usize,
    pub word: [Option<usize>; 4],
    pub affine: [(f32, f32); 4],
}

/// `CodeMap` of a read-out of channels `mask` with `cal[c]` = (scale, offset) of CH c+1.
pub fn code_map(mask: u32, cal: [(f64, f64); 4]) -> CodeMap {
    let ratio = hz::ratio(mask) as usize;
    let slot = slots(mask, ratio);
    let mut word = [None; 4];
    for (k, c) in slot.iter().enumerate().take(ratio) {
        if let Some(c) = *c {
            word[c] = Some(k);
        }
    }
    CodeMap { ratio, word, affine: cal.map(affine) }
}

/// The volts `decode_volts_into` gives for read-out word `x` of a channel with `affine` (the same f32 operations).
#[inline]
pub fn volt_code(x: u16, (a, off): (f32, f32)) -> f32 {
    volt32(x, a, off)
}

/// Volts of samples `a..b` of the channel at `word` of every group of `ratio` words in `bytes`, into `out` (cleared
/// first): `volt_code` of each, in one loop.
pub fn lane_volts_into(bytes: &[u8], ratio: usize, word: usize, a: usize, b: usize, (sa, off): (f32, f32), out: &mut Vec<f32>) {
    out.clear();
    out.extend((a..b).map(|i| {
        let j = 2 * (i * ratio + word);
        volt32(u16::from_le_bytes([bytes[j], bytes[j + 1]]), sa, off)
    }));
}

/// Slot k of a group of `ratio` words goes to channel `slot[k]` (deinterleave's mapping): ratio 1 and 2 = the enabled
/// channels in order, ratio 4 = word k is CH k+1 when it is enabled. A fixed table: no allocation per frame.
fn slots(mask: u32, ratio: usize) -> [Option<usize>; 4] {
    let mut slot = [None; 4];
    let mut k = 0;
    for c in (0..4).filter(|c| mask & (1 << c) != 0) {
        if ratio == 4 {
            slot[c] = Some(c);
        } else if k < ratio {
            slot[k] = Some(c);
            k += 1;
        }
    }
    slot
}

/// Volts of word `k` of every whole group of `G` bytes, appended to `o` (`decode_volts_into`).
fn lane<const G: usize>(bytes: &[u8], k: usize, a: f32, off: f32, o: &mut Vec<f32>) {
    let at = o.len();
    o.resize(at + bytes.len() / G, 0.0);
    for (d, g) in o[at..].iter_mut().zip(bytes.chunks_exact(G)) {
        *d = volt32(u16::from_le_bytes([g[2 * k], g[2 * k + 1]]), a, off);
    }
}

/// (volts per code, offset) of a channel in f32 for `volt32`.
fn affine((scale, offset): (f64, f64)) -> (f32, f32) {
    ((scale / 7500.0) as f32, offset as f32)
}

/// `volt` in f32: the code minus 32768 is exact, then one multiply and one subtract. The error against the f64 `volt`
/// is a few f32 ulps of the full-scale value, under 1e-6 of a division: below 1 % of one code (1 / 7500 div).
#[inline]
fn volt32(x: u16, a: f32, off: f32) -> f32 {
    (x as i32 - 32768) as f32 * a - off
}

/// Volts of read-out words: 7500 codes per division around 32768 (preamble yinc = V/div / 7500), minus the offset.
/// A time in femtoseconds, rounded to 6 significant digits: the UI keeps times as f32, and 1 us as 999.999997 ns is one
/// trigger tick short (the same as `tdiv_fs` for time bases).
pub fn time_fs(t: f64) -> i128 {
    let x = t.abs() * 1e15;
    if x < 1.0 {
        return 0;
    }
    let q = 10f64.powi(x.log10().floor() as i32 - 5);
    let v = ((x / q).round() * q).round() as i128;
    if t < 0.0 {
        -v
    } else {
        v
    }
}

pub fn volts(w: &[u16], scale: f64, offset: f64) -> Vec<f32> {
    w.iter().map(|&x| volt(x, scale, offset)).collect()
}

/// Volts of one read-out word (`volts`).
#[inline]
fn volt(x: u16, scale: f64, offset: f64) -> f32 {
    ((x as f64 - 32768.0) / 7500.0 * scale - offset) as f32
}

/// Time per division in femtoseconds, rounded to 4 significant digits: the UI keeps time/div as f32 (200 us ->
/// 199.99999 us), and a rate of 50 000 001 instead of 50 M misses the rate remap table (the FPGA then ran at
/// 62.5 MSa/s instead of 31.25 MSa/s, unit 2026-11-03).
pub fn tdiv_fs(tdiv: f64) -> i128 {
    if tdiv <= 0.0 {
        return 0;
    }
    let e = tdiv.log10().floor() as i32 - 3;
    let q = 10f64.powi(e);
    ((tdiv / q).round() * q * 1e15).round() as i128
}

/// Configure + acquire in one list (oos-fpgad plan/run).
pub fn plan(st: &mut State, s: &Settings, cal: Option<&afe::Cal>, cal_adc: Option<&adc::CalAdc>) -> Result<(Vec<Step>, hz::Params), String> {
    let (mut v, p) = st.configure(s, cal, cal_adc)?;
    v.extend(st.acquire(s, &p));
    Ok((v, p))
}

/// One batch on the unit: arm `frames(s, p)` frames, wait for the first, then let the ring fill until all frames are
/// recorded or `wait_ms` have passed since the first one (0x4010 reads back the last recorded frame, so the count is
/// `0x4010 - first + 1`), then play and read exactly those frames into `dma`.
/// `armed`: `prearm` already armed this batch with the same `s` and `p`; only the wait is left.
/// Returns the count; the bytes (`count` x `frame_bytes`) are `dma.data()`.
/// Generic over the bus, so the host model (`seg::sim::Engine`) runs it too.
#[allow(clippy::too_many_arguments)]
pub fn acquire_batch<B: crate::dho::bus::Bus, R: std::io::Read>(st: &mut State, s: &Settings, p: &hz::Params, bar: &mut B, c2h: &mut R, dma: &mut crate::dho::bus::DmaBuf, armed: bool, wait_ms: u64, log: impl FnMut(String)) -> Result<u32, String> {
    acquire_batch_until(st, s, p, bar, c2h, dma, armed, wait_ms, log, || false)
}

/// `acquire_batch` whose wait for a trigger ends with `Err(NEW_SETTINGS)` as soon as `stop()` says so (checked every
/// 10 ms): in NORMAL without a trigger the wait lasts 2 s, and new settings would wait for it.
#[allow(clippy::too_many_arguments)]
pub fn acquire_batch_until<B: crate::dho::bus::Bus, R: std::io::Read>(st: &mut State, s: &Settings, p: &hz::Params, bar: &mut B, c2h: &mut R, dma: &mut crate::dho::bus::DmaBuf, armed: bool, wait_ms: u64, log: impl FnMut(String), stop: impl Fn() -> bool) -> Result<u32, String> {
    acquire_batch_idle(st, s, p, bar, c2h, dma, armed, wait_ms, log, stop, || false)
}

/// `acquire_batch_until` that lets the caller work while the ring fills: `idle()` is called instead of the sleep
/// between two looks at the frame count, and the count is read after every call, so the read waits at most one call.
/// `idle` returns false when it has nothing to do (the wait then sleeps as without it).
#[allow(clippy::too_many_arguments)]
pub fn acquire_batch_idle<B: crate::dho::bus::Bus, R: std::io::Read>(st: &mut State, s: &Settings, p: &hz::Params, bar: &mut B, c2h: &mut R, dma: &mut crate::dho::bus::DmaBuf, armed: bool, wait_ms: u64, mut log: impl FnMut(String), stop: impl Fn() -> bool, mut idle: impl FnMut() -> bool) -> Result<u32, String> {
    let nf = frames(s, p);
    let arm = if armed { wait_steps(p) } else { st.arm_steps(s, p) };
    execute_bus_until(&arm, bar, c2h, dma, &mut log, &stop)?;
    let t = std::time::Instant::now();
    let mut count = bar.rd(0x4010).wrapping_add(1).min(nf);
    while count < nf && t.elapsed().as_millis() < wait_ms as u128 {
        if !idle() {
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
        count = bar.rd(0x4010).wrapping_add(1).clamp(1, nf);
    }
    log(format!("{} of {} frames after {:.1} ms", count, nf, t.elapsed().as_secs_f64() * 1e3));
    execute_bus(&st.read_steps(s, p, count), bar, c2h, dma, &mut log)?;
    Ok(count)
}

/// A second read of frame `index` of the batch just read with FPGA compression (`s.compress` > 1), this time raw: the
/// same record, every sample (fork, docs/fork/compress.md). The ring is played again as for the LA read and the stamp
/// plays of hardware segments; nothing may arm or write the FPGA in between. The frame lands in `dma`
/// (`frame_bytes` of `raw_settings(s)`).
#[allow(clippy::too_many_arguments)]
pub fn read_raw_again<B: crate::dho::bus::Bus, R: std::io::Read>(st: &mut State, s: &Settings, p: &hz::Params, bar: &mut B, c2h: &mut R, dma: &mut crate::dho::bus::DmaBuf, index: u32, log: impl FnMut(String)) -> Result<(), String> {
    execute_bus(&st.read_steps_at(&raw_settings(s), p, index, 1), bar, c2h, dma, log)
}

/// `s` without the FPGA compression: what `read_raw_again` reads with.
pub fn raw_settings(s: &Settings) -> Settings {
    Settings { compress: 1, frames: 1, ..s.clone() }
}

/// Arms the next batch right after a read (and the LA read of the same record), so the FPGA records while the host
/// decodes: the same writes as the arm `acquire_batch` would issue next, earlier. Read it with `acquire_batch(..,
/// armed = true, ..)` and the same `s` and `p`; anything else that writes the FPGA first needs `disarm`.
#[cfg(target_os = "linux")]
pub fn prearm(st: &mut State, s: &Settings, p: &hz::Params, bar: &mut crate::dho::bus::Bar1, c2h: &mut std::fs::File, log: impl FnMut(String)) -> Result<(), String> {
    execute(&st.arm_only_steps(s, p), bar, None, c2h, log).map(|_| ())
}

/// Stops an acquisition armed by `prearm` that will not be read (`State::disarm_steps`).
#[cfg(target_os = "linux")]
pub fn disarm(st: &mut State, bar: &mut crate::dho::bus::Bar1, c2h: &mut std::fs::File, log: impl FnMut(String)) -> Result<(), String> {
    execute(&st.disarm_steps(), bar, None, c2h, log).map(|_| ())
}

/// The LA read of the acquisition just read out (`la_read_steps`) on the unit: LA words, bit n = Dn, word `start` of
/// `la::window` first.
#[cfg(target_os = "linux")]
pub fn acquire_la(st: &mut State, s: &Settings, p: &hz::Params, bar: &mut crate::dho::bus::Bar1, c2h: &mut std::fs::File, log: impl FnMut(String)) -> Result<Vec<u16>, String> {
    let (n, _) = la::window(&la::timing(p, s.mask));
    let steps = st.la_read_steps(s, p);
    match execute(&steps, bar, None, c2h, log) {
        Ok(bytes) => Ok(la::words(&bytes, n as usize)),
        Err(e) => {
            // the read failed before `la::restore` ran: without it the stream stays on the LA and the analog is gone
            use crate::dho::bus::Bus;
            for (off, v) in la::restore(&mut st.sh) {
                bar.wr(off, v);
            }
            Err(e)
        }
    }
}

/// Runs steps that need only the register window and the C2H stream (no AFE step) on any `Bus`: the unit, or a model
/// of it in the tests. The Read step goes into `dma` like `execute_into_until`.
pub fn execute_bus<B: crate::dho::bus::Bus, R: std::io::Read>(steps: &[Step], bar: &mut B, c2h: &mut R, dma: &mut crate::dho::bus::DmaBuf, mut log: impl FnMut(String)) -> Result<(), String> {
    execute_bus_until(steps, bar, c2h, dma, &mut log, &|| false)
}

/// `execute_bus` whose polls end early with `Err(NEW_SETTINGS)` when `stop()` says so (checked every 10 ms).
pub fn execute_bus_until<B: crate::dho::bus::Bus, R: std::io::Read>(steps: &[Step], bar: &mut B, c2h: &mut R, dma: &mut crate::dho::bus::DmaBuf, log: &mut impl FnMut(String), stop: &impl Fn() -> bool) -> Result<(), String> {
    for s in steps {
        plain_step(s, bar, c2h, dma, log, stop)?;
    }
    Ok(())
}

/// One step of `execute_bus` / `execute_into_until` other than the AFE steps.
fn plain_step<B: crate::dho::bus::Bus, R: std::io::Read>(s: &Step, bar: &mut B, c2h: &mut R, dma: &mut crate::dho::bus::DmaBuf, log: &mut impl FnMut(String), stop: &impl Fn() -> bool) -> Result<(), String> {
    match s {
        Step::Wr((off, v)) => bar.wr(*off, *v),
        Step::Afe(_) => return Err("AFE step without AFE bus".into()),
        Step::Poll { reg, bit, ms, what } => {
            let t = std::time::Instant::now();
            let mut last = bar.rd(*reg);
            let mut k = 0u32;
            while last & bit == 0 && t.elapsed().as_millis() < *ms as u128 {
                std::thread::sleep(std::time::Duration::from_micros(500));
                last = bar.rd(*reg);
                k += 1;
                if k % 20 == 0 && last & bit == 0 && stop() {
                    log(format!("{} {} ({:.1} ms)", what, NEW_SETTINGS, t.elapsed().as_secs_f64() * 1e3));
                    return Err(NEW_SETTINGS.into());
                }
            }
            let ok = last & bit != 0;
            log(format!("{} {} ({:#x} = {:#010x}, {:.1} ms)", what, if ok { "ok" } else { "TIMEOUT" }, reg, last, t.elapsed().as_secs_f64() * 1e3));
            if !ok && *reg == 0x4080 {
                return Err("no acquisition (0x4080 bit30 not set)".into());
            }
        }
        Step::Read { frames, frame } => {
            let bytes = read_len(*frames, *frame);
            let buf = dma
                .read(c2h, bytes, |got, k| log(format!("c2h: short read of {} bytes (at {} of {})", k, got, bytes)))
                .map_err(|e| format!("c2h read of {} bytes: {} (0x4034 = {:#x})", bytes, e, bar.rd(0x4034)))?;
            let n = unpad_in(buf, *frame);
            dma.truncate(n);
        }
        Step::Peek(regs) => log(regs.iter().map(|r| format!("{:#06x} = {:#010x}", r, bar.rd(*r))).collect::<Vec<_>>().join(", ")),
        Step::Sleep { us } => std::thread::sleep(std::time::Duration::from_micros(*us as u64)),
        Step::Note(n) => log(n.clone()),
    }
    Ok(())
}

/// Runs steps on the unit; returns the bytes of the Read step. `log` gets notes and poll results.
#[cfg(target_os = "linux")]
pub fn execute(steps: &[Step], bar: &mut crate::dho::bus::Bar1, afe_bus: Option<&mut afe::GpioAfe>, c2h: &mut std::fs::File, log: impl FnMut(String)) -> Result<Vec<u8>, String> {
    let mut dma = crate::dho::bus::DmaBuf::new();
    execute_into_until(steps, bar, afe_bus, c2h, &mut dma, log, || false)?;
    Ok(dma.into_vec())
}

/// What `execute_until` returns when `stop` ended a poll: the same state of the FPGA as a poll that timed out.
pub const NEW_SETTINGS: &str = "stopped for new settings";

/// `execute` whose polls end early with `Err(NEW_SETTINGS)` when `stop()` says so (checked every 10 ms).
#[cfg(target_os = "linux")]
pub fn execute_until(steps: &[Step], bar: &mut crate::dho::bus::Bar1, afe_bus: Option<&mut afe::GpioAfe>, c2h: &mut std::fs::File, log: impl FnMut(String), stop: impl Fn() -> bool) -> Result<Vec<u8>, String> {
    let mut dma = crate::dho::bus::DmaBuf::new();
    execute_into_until(steps, bar, afe_bus, c2h, &mut dma, log, stop)?;
    Ok(dma.into_vec())
}

/// `execute` with the Read step into `dma` (kept across batches): the bytes are `dma.data()`, without padding. Polls
/// end early like `execute_until`.
#[cfg(target_os = "linux")]
pub fn execute_into_until(steps: &[Step], bar: &mut crate::dho::bus::Bar1, afe_bus: Option<&mut afe::GpioAfe>, c2h: &mut std::fs::File, dma: &mut crate::dho::bus::DmaBuf, mut log: impl FnMut(String), stop: impl Fn() -> bool) -> Result<(), String> {
    use crate::dho::bus::Bus;
    let mut afe_bus = afe_bus;
    for s in steps {
        match s {
            Step::Afe(afe::Op::Afe { ch, reg, val }) => {
                use afe::AfeBus;
                afe_bus.as_deref_mut().ok_or("AFE step without AFE bus")?.write(*ch, *reg, *val)
            }
            Step::Afe(afe::Op::Gear { ch, gear }) => afe::set_gear(*ch, *gear).map_err(|e| format!("gear: {}", e))?,
            Step::Afe(afe::Op::Adc(w)) => {
                std::thread::sleep(std::time::Duration::from_micros(100));
                bar.wr(0x1034, *w);
                std::thread::sleep(std::time::Duration::from_micros(100));
            }
            _ => plain_step(s, bar, c2h, dma, &mut log, &stop)?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Holdoff writes only when one is set, and once more when it goes back.

    // Break it catches: a code map that puts a channel at another word than the decode, or volts that differ from it
    #[test]
    fn code_map_reads_what_the_decode_reads() {
        let cal = [(1.0, 0.1), (0.5, -0.2), (2.0, 0.0), (0.2, 0.05)];
        for mask in 1u32..16 {
            let bytes: Vec<u8> = (0..4 * 37 + 3u32).flat_map(|i| ((30_000 + i * 97 % 5000) as u16).to_le_bytes()).collect();
            let d = decode_volts(&bytes, mask, cal);
            let m = code_map(mask, cal);
            for c in 0..4 {
                let Some(k) = m.word[c] else {
                    assert!(d[c].is_empty(), "mask {} CH{}", mask, c + 1);
                    continue;
                };
                let n = (bytes.len() / 2 + m.ratio - 1 - k) / m.ratio;
                let v: Vec<f32> = (0..n).map(|i| volt_code(u16::from_le_bytes([bytes[2 * (i * m.ratio + k)], bytes[2 * (i * m.ratio + k) + 1]]), m.affine[c])).collect();
                assert_eq!(v, d[c], "mask {} CH{}", mask, c + 1);
            }
        }
    }

    // Break it catches: an enable bit written with k = 0 (the read stalls), k beyond the 14-bit field (it wraps), a
    // TX length that asks for samples beyond the record (the c2h read of k = 256 at 1 M timed out).
    #[test]
    fn compression_word_and_length_stay_inside_the_hardware() {
        assert_eq!(compress_word(0), 0x4000_0001);
        assert_eq!(compress_word(1), 0x4000_0001);
        assert_eq!(compress_word(4), 0xc000_0004);
        assert_eq!(compress_word(16383), 0xc000_3fff);
        assert_eq!(compress_word(16384), 0xc000_3fff);
        assert_eq!(compress_word(u32::MAX), 0xc000_3fff);
        for memsel in [1_000u32, 14_064, 62_504, 70_312, 100_000, 999_992, 1_000_000, 10_000_000, 125_000_000] {
            for k in [0u32, 1, 2, 3, 4, 15, 16, 25, 64, 128, 200, 256, 1000, 16383, 20000, u32::MAX] {
                let p = hz::Params { ratio: 1, rate: 1_250_000_000, memsel, dot_fs: 800_000, seg: 0, max_frames: 1, pre: memsel / 2, post: memsel / 2 };
                let s = Settings { compress: k, ..Settings::default() };
                let (kk, n) = (compress_k(&s, &p), compressed_len(&s, &p));
                assert!((1..=COMPRESS_K_MAX).contains(&kk) && n % 8 == 0 && n >= 8, "{} {}: k {} n {}", memsel, k, kk, n);
                assert!(n as u64 * kk as u64 <= memsel as u64, "{} at k {}: {} points x {}", memsel, k, n, kk);
                // at most 8 points (4 pairs) short of the record
                assert!(memsel as u64 - (n as u64 * kk as u64) < 8 * kk as u64, "{} at k {}", memsel, k);
            }
        }
        // the unit's cases: k 256 at 1 M asked 3912 points, now 3904; k 128 7816 -> 7808, k 4 250 000 unchanged
        let at = |k: u32| compressed_len(&Settings { compress: k, ..Settings::default() }, &hz::Params { ratio: 1, rate: 1_250_000_000, memsel: 1_000_000, dot_fs: 800_000, seg: 0, max_frames: 1, pre: 500_000, post: 500_000 });
        assert_eq!((at(256), at(128), at(4)), (3904, 7808, 250_000));
    }
    #[test]
    fn the_afe_sees_the_bnc_behind_a_probe() {
        let dir = std::env::var("OOS_CAL_DIR").unwrap_or(concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal").into());
        let Ok(cal) = afe::Cal::load(&dir) else {
            eprintln!("calibration files not found, skipped");
            return;
        };
        let afe_ops = |k: Chan| {
            let s = Settings { afe: true, ch: [k; 4], ..Settings::default() };
            let (st, _) = plan(&mut State::handover(1, None), &s, Some(&cal), None).unwrap();
            st.iter().filter(|x| matches!(x, Step::Afe(_))).map(|x| format!("{:?}", x)).collect::<Vec<_>>()
        };
        let k = Settings::default().ch[0];
        let k = Chan { scale: 0.5, offset: 0.25, ..k };
        let tip = Chan { scale: 5.0, offset: 2.5, probe: 10.0, ..k };
        assert!(!afe_ops(k).is_empty());
        assert_eq!(afe_ops(k), afe_ops(tip));
        // the zero correction moves the AFE offset and nothing else
        assert_eq!(afe_ops(Chan { offset: 0.35, bias: 0.1, ..k }), afe_ops(k));
    }

    fn cal() -> Option<afe::Cal> {
        afe::Cal::load(&std::env::var("OOS_CAL_DIR").unwrap_or(concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal").into())).ok()
    }

    fn afe_writes(st: &[Step], reg: u8) -> Vec<u16> {
        st.iter().filter_map(|x| if let Step::Afe(afe::Op::Afe { reg: r, val, .. }) = x { (*r == reg).then_some(*val) } else { None }).collect()
    }

    #[test]
    fn coupling_goes_to_the_afe_and_gnd_to_the_fpga() {
        let Some(cal) = cal() else { return };
        let mut st = State::handover(1, None);
        let k = Settings::default().ch[0];
        let run = |st: &mut State, couple: Couple| {
            let s = Settings { afe: true, ch: [Chan { couple, offset: 0.5, ..k }; 4], ..Settings::default() };
            st.configure(&s, Some(&cal), None).unwrap().0
        };
        let dc = run(&mut st, Couple::Dc);
        assert!(afe_writes(&dc, 0x07).iter().all(|v| v & 0x8000 != 0));
        let ac = run(&mut st, Couple::Ac);
        assert_eq!(afe_writes(&ac, 0x07).last().unwrap() & 0x8000, 0);
        // GND leaves the AFE alone and flattens the channel in the FPGA: 0.5 V at 1 V/div
        let g = run(&mut st, Couple::Gnd);
        assert_eq!(afe_writes(&g, 0x07).last().unwrap() & 0x8000, 0);
        let w = writes(&g);
        assert!(w.contains(&(0x14c8, 0)) && w.contains(&(0x14d0, 32768 + 3750)), "{:?}", &w[..4]);
        assert!(afe_writes(&run(&mut st, Couple::Dc), 0x07).last().unwrap() & 0x8000 != 0);
    }

    #[test]
    fn invert_is_done_in_the_fpga_with_a_mirrored_offset() {
        let Some(cal) = cal() else { return };
        let k = Settings::default().ch[0];
        let run = |invert: bool, offset: f64| {
            let s = Settings { afe: true, ch: [Chan { invert, offset, ..k }; 4], ..Settings::default() };
            State::handover(1, None).configure(&s, Some(&cal), None).unwrap().0
        };
        let (a, b) = (run(false, 0.5), run(true, -0.5));
        assert_eq!(afe_writes(&a, 0x28), afe_writes(&b, 0x28));
        let inv = |st: &[Step]| writes(st).iter().rev().find(|w| w.0 == 0x1004).unwrap().1 >> 16 & 0xf;
        assert_eq!((inv(&a), inv(&b)), (0, 0xf));
    }

    #[test]
    fn peak_mode_delay_and_holdoff_reach_the_hardware() {
        let peak = |mode: acq::Mode, tdiv: f64| {
            let s = Settings { mode, tdiv, depth: 100_000, ..Settings::default() };
            let st = State::handover(1, None).configure(&s, None, None).unwrap().0;
            writes(&st).iter().rev().find(|w| w.0 == 0x1004).unwrap().1 & 2
        };
        assert_eq!((peak(acq::Mode::Normal, 1e-3), peak(acq::Mode::Peak, 1e-3)), (0, 2));
        // the record follows the delay, and stops at half a record
        let pre = |delay: f64| {
            let s = Settings { delay, depth: 100_000, tdiv: 1e-6, ..Settings::default() };
            State::handover(1, None).configure(&s, None, None).unwrap().1
        };
        let (a, b) = (pre(0.0), pre(3e-6));
        assert_eq!(a.pre as i64 - b.pre as i64, (3e-6 * a.rate as f64) as i64);
        assert_eq!((pre(1.0).pre, pre(-1.0).pre), (0, a.memsel));
        // the auto time waits out a long holdoff (trigger.md 2.6): 1 s are 250 M ticks of the 4 ns timer
        let t2 = |holdoff: f64| {
            let s = Settings { holdoff, ..Settings::default() };
            writes(&State::handover(1, None).configure(&s, None, None).unwrap().0).iter().rev().find(|w| w.0 == 0x4064).unwrap().1
        };
        assert_eq!((t2(HOLDOFF_MIN), t2(1.0)), (25_000_000, 250_000_000));
    }

    // Break it catches: the auto set-up's fast auto trigger not reaching the SCU, so at 20 ms/div every record of a
    // signal the level does not cross waits 400 ms (docs/fork/trigger-lockin.md)
    #[test]
    fn fast_auto_trigger_takes_the_shortest_times() {
        let t = |auto_fast: bool| {
            let s = Settings { tdiv: 20e-3, auto_fast, ..Settings::default() };
            let w = writes(&State::handover(1, None).configure(&s, None, None).unwrap().0);
            let last = |r: u32| w.iter().rev().find(|x| x.0 == r).unwrap().1;
            (last(0x4060), last(0x4064))
        };
        // 4 ns ticks: 400 ms / 200 ms at 20 ms/div, 10 ms / 100 ms fast
        assert_eq!(t(false), (100_000_000, 50_000_000));
        assert_eq!(t(true), (2_500_000, 25_000_000));
    }

    #[test]
    fn hysteresis_follows_the_table_below_2_mv() {
        let h = |scale: f64, probe: f64| tpu::noise_hysteresis(scale / probe * 1e9, false);
        assert_eq!((h(1.0, 1.0), h(0.002, 1.0), h(0.00204, 1.0), h(0.001, 1.0), h(0.0005, 1.0), h(0.0002, 1.0), h(0.0001, 1.0)), (0.3, 0.5, 0.3, 1.0, 1.25, 1.15, 2.3));
        assert_eq!(h(0.01, 10.0), 1.0, "the BNC scale counts");
        assert_eq!(h(0.002f32 as f64, 1.0), 0.5, "through f32");
        assert_eq!(tpu::noise_hysteresis(1e9, true), 0.7);
    }

    #[test]
    fn times_survive_f32() {
        assert_eq!(time_fs(1e-6f32 as f64), 1_000_000_000);
        assert_eq!(time_fs(5e-3f32 as f64), 5_000_000_000_000);
        assert_eq!(time_fs(200e-3f32 as f64), 200_000_000_000_000);
        assert_eq!(time_fs(-2e-6f32 as f64), -2_000_000_000);
        assert_eq!(time_fs(8e-9), 8_000_000);
        assert_eq!(time_fs(1.23456e-6), 1_234_560_000);
        assert_eq!(time_fs(0.0), 0);
    }

    #[test]
    fn holdoff_is_written_when_it_matters() {
        let mut st = State::handover(1, None);
        let has = |w: &[crate::dho::Write]| w.iter().any(|x| x.0 == 0x2084);
        let base = Settings { trigger: Trig::Pulse { pol: tpu::Polarity::Pos, when: tpu::When::Gt, lower: 1e-6, upper: 2e-6 }, ..Settings::default() };
        assert!(!has(&st.trigger(&base)));
        let on = Settings { holdoff: 1e-3, ..base.clone() };
        assert!(has(&st.trigger(&on)));
        assert!(has(&st.trigger(&base)), "back to none must reach the hardware");
        assert!(!has(&st.trigger(&base)));
    }

    use super::*;

    fn writes(st: &[Step]) -> Vec<Write> {
        st.iter().filter_map(|s| if let Step::Wr(w) = s { Some(*w) } else { None }).collect()
    }

    #[test]
    fn aux_out_is_written_once_and_on_change() {
        use auxout::{AuxOut, PassFail};
        let aux = |st: &mut State, s: &Settings| -> Vec<Write> {
            writes(&st.configure(s, None, None).unwrap().0).into_iter().filter(|w| w.0 == auxout::SELECT || w.0 == auxout::PASSFAIL).collect()
        };
        let mut st = State::handover(1, None);
        let base = Settings::default();
        assert_eq!(aux(&mut st, &base), vec![(0x1430, 0)], "the first set-up states the power-up source");
        assert_eq!(aux(&mut st, &base), vec![]);
        let off = Settings { aux: AuxOut::Off, ..base.clone() };
        assert_eq!(aux(&mut st, &off), vec![(0x1430, 2)]);
        assert_eq!(aux(&mut st, &off), vec![]);
        let pf = Settings { aux: AuxOut::PassFail(PassFail::DEFAULT), ..base.clone() };
        assert_eq!(aux(&mut st, &pf), vec![(0x119c, 156), (0x1430, 3)]);
        assert_eq!(aux(&mut st, &base), vec![(0x1430, 0)]);
        // the diagnostic arm without the acquisition block leaves AUX OUT alone
        let mut st = State::handover(1, None);
        assert_eq!(aux(&mut st, &Settings { blocks: 5, ..off }), vec![]);
    }

    #[test]
    fn hardware_mask_starts_reloads_clears_and_stops() {
        let only = |st: &mut State, s: &Settings| -> Vec<Write> {
            writes(&st.configure(s, None, None).unwrap().0).into_iter().filter(|w| matches!(w.0, 0x1184 | 0x1190 | 0x1194) || w.0 == 0x4020).collect()
        };
        let band = |h: f32| HwMask { ch: 0, lo: vec![-h; mask::COLUMNS], hi: vec![h; mask::COLUMNS], stop_on_fail: false, clear: 0, map: mask::Map::default() };
        let mut st = State::handover(1, None);
        let base = Settings { window: None, ..Settings::default() };
        let on = Settings { hwmask: Some(band(1.0)), ..base.clone() };
        let w = only(&mut st, &on);
        // off, 1000 columns + write enable dropped, compression for 2.5 Mpts, slot 0 with the counter clear, on; then
        // the auto-trigger block writes 0x4020 from the same shadow
        assert_eq!(w.len(), 1 + 1001 + 1 + 2 + 1 + 1, "{:x?}", &w[..4]);
        assert_eq!(w[0], (0x4020, 0x2001));
        assert_eq!(w[1], (0x1194, 0x1c00_0000 | 158 << 8 | 98));
        assert_eq!(w[1001], (0x1194, 0));
        assert_eq!(w[1002..1006], [(0x1184, 0xc000_03ff), (0x1190, 0x6010_0000), (0x1190, 0x0010_0000), (0x4020, 0xe001)]);
        assert_eq!(*w.last().unwrap(), (0x4020, 0xe001), "auto trigger keeps the mask bits");
        assert_eq!(st.mask_ctrl(), Some(0x0010_0000));
        // unchanged: nothing but the auto-trigger write
        assert_eq!(only(&mut st, &on), vec![(0x4020, 0xe001)]);
        // a tighter band: the table again, the counters keep counting
        let w = only(&mut st, &Settings { hwmask: Some(band(0.5)), ..base.clone() });
        assert_eq!(w.len(), 1 + 1001 + 1 + 1 + 1 + 1);
        assert_eq!(w[1003], (0x1190, 0x0010_0000));
        // a clear request: no table, the counters cleared
        let w = only(&mut st, &Settings { hwmask: Some(HwMask { clear: 1, ..band(0.5) }), ..base.clone() });
        assert_eq!(w, vec![(0x4020, 0x6001), (0x1184, 0xc000_03ff), (0x1190, 0x6010_0000), (0x1190, 0x0010_0000), (0x4020, 0xe001), (0x4020, 0xe001)]);
        // stop on fail sets bit 12
        let w = only(&mut st, &Settings { hwmask: Some(HwMask { clear: 1, stop_on_fail: true, ..band(0.5) }), ..base.clone() });
        assert_eq!(*w.last().unwrap(), (0x4020, 0xf001));
        // off: enable and stop cleared, compression back, slot dropped
        let w = only(&mut st, &base);
        assert_eq!(w, vec![(0x4020, 0x6001), (0x1184, 0x4000_0001), (0x1190, 0), (0x4020, 0x6001)]);
        assert_eq!(st.mask_ctrl(), None);
        assert_eq!(only(&mut st, &base), vec![(0x4020, 0x6001)]);
        // a source that is not acquired leaves the test off
        let w = only(&mut st, &Settings { hwmask: Some(HwMask { ch: 1, ..band(1.0) }), ..base.clone() });
        assert_eq!(w, vec![(0x4020, 0x6001)]);
        // CH2 of CH1+CH2: slot 1 (stream slot)
        let two = Settings { mask: 0b11, hwmask: Some(HwMask { ch: 1, ..band(1.0) }), ..base.clone() };
        let mut st = State::handover(0b11, None);
        assert!(only(&mut st, &two).contains(&(0x1190, 0x0020_0000)));
    }

    #[test]
    fn hardware_zone_starts_hands_over_to_the_mask_and_stops() {
        let only = |st: &mut State, s: &Settings| -> Vec<Write> {
            writes(&st.configure(s, None, None).unwrap().0).into_iter().filter(|w| matches!(w.0, 0x1184 | 0x1190 | 0x1194 | 0x11d0 | 0x11d8 | 0x11dc | 0x4020)).collect()
        };
        let bx = |inside| zone::Zone { poly: vec![(-1e-4, 0.5), (1e-4, 1.5)], inside };
        let z1 = HwZone { ch: 0, zones: vec![bx(true)], clear: 0, map: mask::Map::default() };
        let base = Settings { window: None, ..Settings::default() };
        let on = Settings { hwzone: Some(z1.clone()), ..base.clone() };
        let mut st = State::handover(1, None);
        let w = only(&mut st, &on);
        // off, table 1 (1000 + 1), control, compression, slot with clear, armed; then the auto-trigger write
        assert_eq!(w.len(), 1 + 1001 + 1 + 1 + 2 + 1 + 1);
        assert_eq!(w[0], (0x4020, 0x2001));
        assert!(w[1..1001].iter().all(|x| x.0 == 0x11d8));
        assert_eq!(w[1002..1008], [(0x11d0, 0x0500_0000), (0x1184, 0xc000_03ff), (0x1190, 0x6010_0000), (0x1190, 0x0010_0000), (0x4020, 0x2003), (0x4020, 0x2003)]);
        assert_eq!(st.zone_ctrl(), Some(0x0010_0000));
        assert_eq!(only(&mut st, &on), vec![(0x4020, 0x2003)]);
        // a second zone that must not be touched: its table, the new control word, no clear
        let two = Settings { hwzone: Some(HwZone { zones: vec![bx(true), bx(false)], ..z1.clone() }), ..base.clone() };
        let w = only(&mut st, &two);
        assert!(w.iter().filter(|x| x.0 == 0x11dc).count() == 1001 && w.contains(&(0x11d0, 0x0700_0000)) && w.contains(&(0x1190, 0x0010_0000)) && !w.contains(&(0x1190, 0x6010_0000)));
        // the mask test wins: the zone logic off, the mask keeps the slot it sets
        let mut m = Settings { hwmask: Some(HwMask { ch: 0, lo: vec![-1.0; mask::COLUMNS], hi: vec![1.0; mask::COLUMNS], stop_on_fail: false, clear: 0, map: mask::Map::default() }), ..two.clone() };
        let w = only(&mut st, &m);
        assert_eq!(&w[..2], [(0x4020, 0x2001), (0x11d0, 0)]);
        assert_eq!(st.zone_ctrl(), None);
        assert!(st.mask_ctrl().is_some() && *w.last().unwrap() == (0x4020, 0xe001));
        // back to the zone: the mask goes off without taking the zone's slot and strobe
        m.hwmask = None;
        let w = only(&mut st, &m);
        assert_eq!(st.zone_ctrl(), Some(0x0010_0000));
        assert!(!w.contains(&(0x1184, 0x4000_0001)), "{:x?}", w);
        assert_eq!(*w.last().unwrap(), (0x4020, 0x6003));
        // off
        let w = only(&mut st, &base);
        assert_eq!(w, vec![(0x4020, 0x6001), (0x11d0, 0), (0x1184, 0x4000_0001), (0x1190, 0), (0x4020, 0x6001)]);
    }

    #[test]
    fn uart_trigger_is_written_with_the_trigger_block() {
        use crate::dho::uarttrig::{Cond, Parity, Stop, Uart};
        let u = Uart { baud: 115_200.0, bits: 8, parity: Parity::None, stop: Stop::One, cond: Cond::Start, invert: false };
        let s = Settings { trig_src: 2, mask: 0b11, slope: tpu::Slope::Falling, trigger: Trig::Uart(u), ..Settings::default() };
        let mut st = State::handover(0b11, None);
        let w = writes(&st.configure(&s, None, None).unwrap().0);
        let k = w.iter().position(|x| *x == (0x205c, 4)).expect("mux on the UART event");
        assert!(w[..k].contains(&(0x2100, 1356)) && w[..k].contains(&(0x2104, 7)));
        // the trigger block comes before the FSM stop poll and the acquisition block, like every trigger type
        assert!(k < w.iter().position(|x| x.0 == 0x4020).unwrap());
    }

    #[test]
    fn batch_of_frames() {
        // 1 us/div, CH1: 12 500 points per frame, ring of max_frames frames; 100 frames in one arm and one read
        let s = Settings { tdiv: 1e-6, frames: 100, window: None, ..Settings::default() };
        let mut state = State::handover(1, None);
        let (st, p) = plan(&mut state, &s, None, None).unwrap();
        assert!(p.max_frames >= 100, "max_frames {}", p.max_frames);
        let w = writes(&st);
        // arm: 0x4018 = first + n - 1 twice (record and play)
        assert_eq!(w.iter().filter(|x| **x == (0x4018, 99)).count(), 2);
        assert!(matches!(st.last(), Some(&Step::Read { frames: 100, frame }) if frame == 2 * p.memsel as usize));
        // more than the ring is clamped, play-last stays at one frame
        let (_, p2) = plan(&mut State::handover(1, None), &Settings { frames: 1_000_000, ..s.clone() }, None, None).unwrap();
        assert_eq!(frames(&Settings { frames: 1_000_000, ..s.clone() }, &p2), p2.max_frames);
        assert_eq!(frames(&Settings { play4: false, ..s }, &p2), 1);
    }

    #[test]
    fn f32_time_base_still_hits_the_rate_table() {
        assert_eq!(tdiv_fs(200e-6_f32 as f64), 200_000_000_000);
        assert_eq!(tdiv_fs(2e-9), 2_000_000);
        assert_eq!(tdiv_fs(500e-3_f32 as f64), 500_000_000_000_000);
        // 100 kpts at 200 us/div: 50 MSa/s is remapped to 31.25 MSa/s like the stock system does
        let p = hz::prep(1, tdiv_fs(200e-6_f32 as f64), 100_000, 0);
        assert_eq!(p.rate, 31_250_000);
    }

    #[test]
    fn default_plan_ends_with_export_init_and_type4_play() {
        let mut state = State::handover(1, None);
        let (st, p) = plan(&mut state, &Settings::default(), None, None).unwrap();
        assert_eq!(p.memsel, 2_500_000);
        let w = writes(&st);
        assert!(w.contains(&(0x1108, 1_248_000)) && w.contains(&(0x1100, 0x11e8)));
        assert!(matches!(st.last(), Some(Step::Read { frames: 1, frame: 8000 })));
        // trigger 17, acquisition 22 + AUX OUT 1 (first set-up only), auto trigger 3, arm 29, pass-through 10, read-out
        // window 7 + stop sequence 17 + type-4 play 12 (the sequence that delivered own frames on the unit, 2026-11-03)
        assert_eq!(w.len(), 17 + 22 + 1 + 3 + 29 + 10 + 7 + 17 + 12);
        let k = w.iter().position(|x| *x == (0x1170, 0x4000_0001)).unwrap();
        assert!(k > 17 + 22 + 1 + 3 + 29 - 1, "pass-through comes after the arm");
        assert_eq!(*w.last().unwrap(), (0x4004, 0x1));
        // the play-last read-out of the warm hand-over is still available: 28 writes
        let mut state = State::handover(1, None);
        let (st, _) = plan(&mut state, &Settings { play4: false, ..Settings::default() }, None, None).unwrap();
        assert_eq!(writes(&st).len(), 17 + 22 + 1 + 3 + 29 + 28);
    }

    #[test]
    fn la_off_writes_nothing_new_and_la_on_adds_the_set_up_and_a_second_read() {
        // LA off: no LA register in the plan
        let mut state = State::handover(1, None);
        let s = Settings { tdiv: 1e-3, depth: 100_000, window: None, ..Settings::default() };
        let (st, p) = plan(&mut state, &s, None, None).unwrap();
        let la_regs = [0x128c, 0x12a4, 0x12a8, 0x12ac, 0x12c0, 0x12d4, 0x1298, 0x1280];
        assert!(!writes(&st).iter().any(|w| la_regs.contains(&w.0)));
        assert!(writes(&st).iter().all(|w| w.0 != 0x4008 || w.1 & (1 << 26) == 0));
        // LA on with CH1 shown: halved rate, doubled record registers, enable, rate block, thresholds, second read
        let mut state = State::handover(1, None);
        let sl = Settings { la: Some(la::Set::default()), frames: 50, ..s.clone() };
        let (st, pl) = plan(&mut state, &sl, None, None).unwrap();
        assert_eq!((pl.rate * 2, pl.memsel, pl.seg), (p.rate, p.memsel, 2 * p.seg));
        let w = writes(&st);
        assert!(w.contains(&(0x10e4, 2 * pl.memsel)) && w.contains(&(0x4050, 2 * pl.pre)) && w.contains(&(0x12d4, 0x2336_2353)));
        assert!(w.iter().any(|x| x.0 == 0x4008 && x.1 & (1 << 26) != 0));
        let t = la::timing(&pl, 1);
        assert!(w.contains(&la::rate_block(t.rate)[2]) && w.contains(&(0x1100, 10 * t.n)));
        // one frame per batch, the analog read, then the LA read of 2 x roundup4(n) bytes, then the stream restore
        assert_eq!(frames(&sl, &pl), 1);
        let reads: Vec<&Step> = st.iter().filter(|x| matches!(x, Step::Read { .. })).collect();
        assert_eq!(reads.len(), 2);
        assert!(matches!(reads[1], Step::Read { frames: 1, frame } if *frame == la::read_bytes(t.n)));
        assert_eq!(state.sh.s4008 & (1 << 23 | 1 << 24 | 1 << 26), 1 << 23 | 1 << 26);
        // LA alone (no analog channel shown): the rate is not halved
        let mut state = State::handover(1, None);
        let (_, pa) = plan(&mut state, &Settings { la: Some(la::Set { analog_shown: false, ..la::Set::default() }), ..s }, None, None).unwrap();
        assert_eq!(pa.rate, p.rate);
    }

    #[test]
    fn frames_padded_to_8_bytes() {
        use super::{frame_stride, read_len, unpad};
        assert_eq!((frame_stride(2500), frame_stride(1250), frame_stride(5000), frame_stride(500)), (2504, 1256, 5000, 504));
        assert_eq!(read_len(4, 2500), 10016);
        // three frames of 5 bytes at a stride of 8
        let mut b: Vec<u8> = vec![1, 2, 3, 4, 5, 0, 0, 0, 6, 7, 8, 9, 10, 0, 0, 0, 11, 12, 13, 14, 15, 0, 0, 0];
        unpad(&mut b, 5);
        assert_eq!(b, (1..=15).collect::<Vec<u8>>());
    }

    #[test]
    fn four_channel_window_like_phase05_step_d() {
        // step D (2026-10-09, word-identical): 4 channels, 4000 points from point 0 -> 0x1100 = 0x17e8, 0x1108 = 0,
        // 0x11f0 = 0xfa0, one read of 32000 bytes
        let s = Settings { mask: 0xf, tdiv: 2e-6, window: Some((4000, -1_000_000)), ..Settings::default() };
        let mut state = State::handover(0xf, None);
        let (st, p) = plan(&mut state, &s, None, None).unwrap();
        assert_eq!(p.ratio, 4);
        let w = writes(&st);
        assert!(w.contains(&(0x1100, 0x17e8)) && w.contains(&(0x1108, 0)) && w.contains(&(0x11f0, 0xfa0)));
        assert!(matches!(st.last(), Some(Step::Read { frames: 1, frame: 32000 })));
    }

    #[test]
    fn channel_set_change_switches_the_adc_once() {
        let s = Settings { mask: 3, ..Settings::default() };
        let mut state = State::handover(1, None);
        assert!(state.configure(&s, None, None).is_err());
        if let Ok(cal) = adc::CalAdc::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal")) {
            let mut state = State::handover(1, Some(&cal));
            let (st, _) = state.configure(&s, None, Some(&cal)).unwrap();
            let w = writes(&st);
            assert_eq!(&w[..2], &[(0x1034, 0x0154_1860), (0x1034, 0x0155_0005)]);
            // second configure with the same set: no ADC writes
            let (st2, _) = state.configure(&s, None, Some(&cal)).unwrap();
            assert!(!writes(&st2).iter().any(|w| w.0 == 0x1034));
        }
    }

    #[test]
    fn every_hardware_trigger_type_configures() {
        use tpu::*;
        let pats = [Pat::H, Pat::X, Pat::X, Pat::Rise];
        for t in [
            Trig::Edge,
            Trig::Pulse { pol: Polarity::Pos, when: When::Lt, lower: 0.0, upper: 20e-6 },
            Trig::Slope { when: When::Gt, lower: 1e-6, upper: 0.0, lo: 0.5 },
            Trig::Runt { pol: Polarity::Pos, when: When::None, lower: 0.0, upper: 0.0, lo: 0.5 },
            Trig::Window { event: WindowEvent::Enter, time: 0.0, lo: -0.5 },
            Trig::Timeout { time: 1.2e-3 },
            Trig::Delay { srcb: 4, eb: Edge::Rising, when: When::Gt, lower: 40e-6, upper: 0.0, levelb: 1.65 },
            Trig::Pattern { pats, la: [Pat::X; 16], levels: [1.25, 0.0, 1.65, 1.65] },
            Trig::Duration { pats: [Pat::H, Pat::X, Pat::X, Pat::X], la: [Pat::X; 16], when: When::Gt, lower: 200e-6, upper: 0.0, levels: [1.25; 4] },
            Trig::SetupHold { clk: 4, clk_edge: Edge::Rising, setup: 2e-6, hold: 2e-6, levelc: 1.65 },
            Trig::Nth { count: 3, idle: 50e-6 },
            Trig::Video { std: VideoStd::Pal, sync: VideoSync::OddField, line: 1 },
        ] {
            let s = Settings { mask: 0xf, trigger: t.clone(), ..Settings::default() };
            let mut state = State::handover(0xf, None);
            let (st, _) = state.configure(&s, None, None).unwrap();
            // every type ends with the type select in 0x205c[4:0] written
            assert!(writes(&st).iter().any(|w| w.0 == 0x205c), "{:?}", t);
        }
        // Nth edge: ABR flags (1,1,1,0,1) + type 3 (trigger.md 10.11)
        let s = Settings { trigger: Trig::Nth { count: 3, idle: 50e-6 }, ..Settings::default() };
        let mut state = State::handover(1, None);
        let (st, _) = state.configure(&s, None, None).unwrap();
        assert_eq!(writes(&st).iter().rev().find(|w| w.0 == 0x205c).unwrap().1 & 0x7df, 0x5c3);
    }

    #[test]
    fn deinterleave_and_volts() {
        let words: Vec<u16> = (0..16).collect();
        let b: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let d = deinterleave(&b, 0b1010);
        assert_eq!(d[1], [0, 2, 4, 6, 8, 10, 12, 14]);
        assert_eq!(d[3], [1, 3, 5, 7, 9, 11, 13, 15]);
        let d = deinterleave(&b, 0b0111); // 3 channels run in 4-channel mode
        assert_eq!(d[2], [2, 6, 10, 14]);
        assert!(d[3].is_empty());
        // the replayed frame: 21518 = yref + yorg at 1 V/div, offset -1.5 V -> 0 V
        assert!((volts(&[21518], 1.0, -1.5)[0]).abs() < 1e-6);
        assert!((volts(&[32768 + 7500], 2.0, 0.0)[0] - 2.0).abs() < 1e-6);
    }

    /// Pseudo-random read-out bytes (a fixed LCG, so failures repeat).
    fn noise(n: usize, seed: u32) -> Vec<u8> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (x >> 24) as u8
            })
            .collect()
    }

    /// The bound of `volt32` against the f64 `volt` of code `x`: 4 f32 ulps of the larger of the two terms.
    fn f32_bound(x: u16, (scale, offset): (f64, f64)) -> f64 {
        4.0 * f32::EPSILON as f64 * ((x as f64 - 32768.0).abs() / 7500.0 * scale.abs() + offset.abs()) + f32::MIN_POSITIVE as f64
    }

    // Break it catches: a fused decode that maps a word to the wrong channel, drops the words of an incomplete last
    // group, or moves a value by more than the f32 rounding (the display and the measurements would move).
    #[test]
    fn fused_decode_equals_deinterleave_and_volts() {
        let cal = [(1.0, -1.5), (0.02, 0.013), (5.0, 2.5), (0.5, 0.0)];
        let mut worst = 0f64;
        for mask in 1..16u32 {
            for len in [0usize, 2, 6, 8, 14, 16, 2816, 8000, 12_502] {
                let b = noise(len, mask * 7919 + len as u32);
                let w = deinterleave(&b, mask);
                let got = decode_volts(&b, mask, cal);
                for c in 0..4 {
                    assert_eq!(got[c].len(), w[c].len(), "mask {:#x}, {} bytes, CH{}", mask, len, c + 1);
                    let want = volts(&w[c], cal[c].0, cal[c].1);
                    for (i, (&g, &x)) in got[c].iter().zip(&w[c]).enumerate() {
                        let d = (g as f64 - want[i] as f64).abs();
                        worst = worst.max(d / (cal[c].0 / 7500.0));
                        assert!(d <= f32_bound(x, cal[c]), "mask {:#x}, {} bytes, CH{} word {}: {} vs {}", mask, len, c + 1, i, g, want[i]);
                    }
                }
            }
        }
        // in codes: far below one
        assert!(worst < 0.01, "{} codes", worst);
    }

    // Break it catches: kept vectors that keep stale samples or a channel that is no longer enabled, or that differ from
    // the allocating decode.
    #[test]
    fn decode_into_kept_vectors_equals_decode() {
        let cal = [(1.0, -1.5), (0.02, 0.013), (5.0, 2.5), (0.5, 0.0)];
        let mut out: [Vec<f32>; 4] = Default::default();
        for (mask, len) in [(0xfu32, 8000usize), (1, 2816), (0b1010, 4002), (0b0111, 802), (8, 6)] {
            let b = noise(len, mask);
            decode_volts_into(&b, mask, cal, &mut out);
            let want = decode_volts(&b, mask, cal);
            for c in 0..4 {
                let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<u32>>();
                assert_eq!(bits(&out[c]), bits(&want[c]), "mask {:#x}, CH{}", mask, c + 1);
            }
        }
    }

    // Break it catches: an unpad on the reused DMA buffer that differs from the vector one.
    #[test]
    fn unpad_in_place_equals_unpad() {
        for frame in [5usize, 2500, 2504, 1250, 16] {
            let b = noise(7 * frame_stride(frame) + 3, frame as u32);
            let mut v = b.clone();
            unpad(&mut v, frame);
            let mut s = b.clone();
            let n = unpad_in(&mut s, frame);
            assert_eq!(&s[..n], &v[..], "frame {}", frame);
        }
    }

    // Break it catches: an arm ahead of the decode that writes anything else, or in another order, than the batch loop
    // without it (stop sequence, record, play and their shadows must stay exactly as verified on the unit). Two batches
    // of 100 frames; the pre-armed loop issues arm 2 right after read 1 and only waits before read 2.
    #[test]
    fn prearm_writes_the_same_sequence_as_the_plain_batch_loop() {
        let s = Settings { tdiv: 1e-6, frames: 100, window: None, ..Settings::default() };
        let p = State::handover(1, None).configure(&s, None, None).unwrap().1;
        let batch = |st: &mut State, pre: bool, armed: bool| -> Vec<Step> {
            let mut v = if armed { wait_steps(&p) } else { st.arm_steps(&s, &p) };
            v.extend(st.read_steps(&s, &p, 100));
            if pre {
                v.extend(st.arm_only_steps(&s, &p));
            }
            v
        };
        let mut a = State::handover(1, None);
        a.configure(&s, None, None).unwrap();
        let plain: Vec<Step> = (0..3).flat_map(|_| batch(&mut a, false, false)).collect();
        let mut b = State::handover(1, None);
        b.configure(&s, None, None).unwrap();
        let mut pre = batch(&mut b, true, false);
        pre.extend(batch(&mut b, true, true));
        pre.extend(batch(&mut b, false, true));
        assert_eq!(writes(&pre), writes(&plain));
        assert_eq!(writes(&pre).len(), 3 * (29 + 10 + 7 + 17 + 12));
        // the waits and reads are the same steps too, only the arm moved before the decode
        let polls = |v: &[Step]| v.iter().filter(|x| matches!(x, Step::Poll { .. } | Step::Read { .. })).count();
        assert_eq!(polls(&pre), polls(&plain));
        assert_eq!((a.sh.s4004, a.sh.s4008, a.sh.s400c, a.sh.s4080), (b.sh.s4004, b.sh.s4008, b.sh.s400c, b.sh.s4080));
        // arm_steps is still arm + wait
        let mut c = State::handover(1, None);
        let mut d = State::handover(1, None);
        let mut split = c.arm_only_steps(&s, &p);
        split.extend(wait_steps(&p));
        assert_eq!(format!("{:?}", d.arm_steps(&s, &p)), format!("{:?}", split));
    }

    // Break it catches: a segmented record that is not stamp clear + the batch arm with N and the interval, a count
    // beyond the whole-memory limit, segments that leak into the live batch path.
    #[test]
    fn record_steps_clear_the_stamp_and_arm_n_frames() {
        let seg = |n: u32, interval: f64| Settings { tdiv: 1e-6, depth: 1000, window: None, segments: Some(Segments { n, interval }), ..Settings::default() };
        let mut st = State::handover(1, None);
        let p = st.configure(&seg(100, 1e-3), None, None).unwrap().1;
        let w = writes(&st.record_steps(&seg(100, 1e-3), &p));
        assert_eq!(&w[..2], &[(0x40b8, 4), (0x40b8, 0)]);
        assert_eq!(w.len(), 2 + 29);
        assert!(w.contains(&(0x4068, 156_250)) && w.contains(&(0x4018, 99)));
        // the stock limit for 1,000 points: 61,440 frames
        assert_eq!(record_frames(&seg(1_000_000, 0.0), &p), 61_440);
        assert_eq!(record_frames(&seg(0, 0.0), &p), 1);
        assert_eq!(record_frames(&Settings::default(), &p), 0);
        // the live plan does not change with segments set
        let live = |s: &Settings| writes(&plan(&mut State::handover(1, None), s, None, None).unwrap().0);
        assert_eq!(live(&seg(100, 1e-3)), live(&Settings { segments: None, ..seg(100, 1e-3) }));
    }

    // Break it catches: a segment read that plays from frame 0, a stamp play with the 8-point TX length the stock
    // export never uses, a stamp play without the header.
    #[test]
    fn segment_reads_play_from_their_frame() {
        let s = Settings { mask: 3, tdiv: 1e-6, depth: 1000, window: None, segments: Some(Segments { n: 500, interval: 0.0 }), header: true, ..Settings::default() };
        let mut a = State::handover(3, None);
        let p = a.configure(&s, None, None).unwrap().1;
        let mut b = State::handover(3, None);
        b.configure(&s, None, None).unwrap();
        let at = writes(&a.read_steps_at(&s, &p, 200, 40));
        let zero = writes(&b.read_steps(&s, &p, 40));
        let diff: Vec<(Write, Write)> = at.iter().zip(&zero).filter(|(x, y)| x != y).map(|(x, y)| (*x, *y)).collect();
        assert_eq!(diff, [((0x4010, 200), (0x4010, 0)), ((0x4014, 200), (0x4014, 0)), ((0x4018, 239), (0x4018, 39))]);
        assert!(at.contains(&(0x1010, a.sh.s1010)) && a.sh.s1010 & 1 << 31 == 0, "header on");
        let st = a.segment_stamp_steps(&s, &p, 7);
        let w = writes(&st);
        // 128 words of two channels: TX length 64 per channel, one 16 + 256 byte read, then the latch pulse
        assert!(w.contains(&(0x11f0, 64)) && w.contains(&(0x4010, 7)) && w.contains(&(0x4018, 7)));
        assert!(w.iter().any(|x| x.0 == 0x1010 && x.1 & 1 << 31 == 0));
        assert_eq!(&w[w.len() - 3..], &[(0x40b8, 0), (0x40b8, 1), (0x40b8, 0)]);
        assert!(st.iter().any(|x| matches!(x, Step::Read { frames: 1, frame: 272 })));
        assert!(w.iter().position(|x| x.0 == 0x40b8).unwrap() > w.iter().position(|x| *x == (0x4004, a.sh.s4004)).unwrap());
    }

    // Break it catches: a disarm that is not the plain stop sequence every arm starts with.
    #[test]
    fn disarm_is_the_stop_sequence_of_an_arm() {
        let s = Settings::default();
        let p = hz::prep(1, tdiv_fs(s.tdiv), 0, 0);
        let mut a = State::handover(1, None);
        let d = writes(&a.disarm_steps());
        assert_eq!(d.len(), 17);
        let arm = writes(&State::handover(1, None).arm_only_steps(&s, &p));
        assert_eq!(&arm[..17], &d[..]);
        assert!(matches!(a.disarm_steps()[1], Step::Poll { reg: 0x4034, ms: 10, .. }));
    }
}
