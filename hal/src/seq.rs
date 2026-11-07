// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Complete acquisition sequences: settings -> ordered steps (register writes, AFE frames, polls, sleeps, the C2H
//! read), a persistent state with all shadows, the executor for the unit, and frame decoding. Shared by `oos-fpgad`
//! and the hardware source of `oos-ui`. Order (docs/protocols): [ADC mode] -> [AFE] -> TPU edge trigger -> wait FSM
//! stopped -> acquisition configuration -> SCU auto/normal trigger -> arm -> wait done -> play-last read-out -> read.

use crate::{acq, adc, afe, hz, la, scu, tpu, Write};

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
}

impl Default for Settings {
    /// The warm hand-over of 2026-11-02: CH1 1 V/div, 200 us/div, Auto depth, edge CH1 rising 1.5 V, read the
    /// verified window (4000 points from 1,248,000).
    fn default() -> Settings {
        let c = Chan { scale: 1.0, offset: 0.0, bw: afe::bw::OFF, probe: 1.0, bias: 0.0, skew: 0.0, couple: Couple::Dc, invert: false };
        Settings { delay: 0.0, mode: acq::Mode::Normal, holdoff: HOLDOFF_MIN, mask: 1, tdiv: 200e-6, depth: 0, ch: [c; 4], afe: false, trig_src: 1, slope: tpu::Slope::Rising, level: 1.5, auto: true, trigger: Trig::Edge, window: Some((4000, -2000)), play4: true, blocks: 7, header: false, frames: 1, compress: 1, la: None }
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
        State { sh: scu::Shadows::HANDOVER, ts: tpu::Shadows::APP, afe: [0, 1, 2, 3].map(afe::Afe::new), adc: a, mode, gain: [1.0; 4], la_was_on: false, holdoff_set: false }
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
            st.extend(crate::wpu::analog_gnd(s.mask, self.gain, gnd).into_iter().map(Step::Wr));
        }
        if s.blocks & 7 != 7 {
            st.push(Step::Note(format!("configuration blocks {:#x} (bit0 trigger, bit1 acquisition, bit2 auto trigger)", s.blocks)));
        }
        if s.blocks & 1 != 0 {
            st.extend(self.trigger(s).into_iter().map(Step::Wr));
        }
        if s.blocks & 2 != 0 {
            st.push(Step::Poll { reg: 0x4034, bit: scu::status::STOPPED, ms: 30, what: "FSM stopped (stop poll)" });
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
        }
        if let Some(l) = &s.la {
            // thresholds with every set-up (la.md 8.1: on change and once after the start)
            st.push(Step::Wr(la::thresholds(&l.cal, l.thr[0], l.thr[1])));
        }
        if s.blocks & 4 != 0 {
            st.push(Step::Note(format!("SCU {} trigger", if s.auto { "auto" } else { "normal" })));
            st.extend(scu::auto_trigger(&mut self.sh, s.auto, tdiv_fs(s.tdiv), time_fs(s.holdoff)).into_iter().map(Step::Wr));
        }
        Ok((st, p))
    }

    /// TPU writes of the trigger in `s` (trigger.md: section 6 edge, section 10 the other types).
    fn trigger(&mut self, s: &Settings) -> Vec<Write> {
        let fs = time_fs;
        let nv = |v: f64| v * 1e9;
        let (src, ts) = (s.trig_src, &mut self.ts);
        // LA lines have no level: any entry will do
        let tc = if src <= 4 { (src - 1) as usize } else { 0 };
        // the hysteresis comes from the scale of the source at the BNC, and multiplies the scale of each channel
        let noise = tpu::noise_hysteresis(nv(s.ch[tc].scale / s.ch[tc].probe), false);
        let vt: [tpu::Vert; 4] = [0, 1, 2, 3].map(|c| tpu::Vert { scale: nv(s.ch[c].scale), offset: nv(s.ch[c].offset), g: self.gain[c], noise });
        let edge = if s.slope == tpu::Slope::Falling { tpu::Edge::Falling } else { tpu::Edge::Rising };
        let mut w = match &s.trigger {
            Trig::Edge => tpu::edge(ts, src, s.slope, nv(s.level), vt[tc].offset, vt[tc].scale, vt[tc].g, noise, 8_000_000),
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
        let nf = frames(s, p);
        let mut st = vec![Step::Note(format!("arm (frame 0, {} frame{})", nf, if nf == 1 { "" } else { "s" }))];
        st.extend(scu::arm(s.mask, &mut self.sh, 0, nf).into_iter().map(Step::Wr));
        // at least the record itself, twice over: slow time bases with the rate halved by the LA take seconds
        let record_ms = p.memsel as u64 * 1000 / p.rate.max(1);
        st.push(Step::Poll { reg: 0x4080, bit: scu::PLAYINFO_DONE, ms: 2000.max(2 * record_ms + 500), what: "acquisition done" });
        st.push(Step::Peek(&[0x4034, 0x4010, 0x4014]));
        st
    }

    /// Read-out of the first `count` frames of the batch: pass-through, window, stop sequence, type-4 play, one read.
    pub fn read_steps(&mut self, s: &Settings, p: &hz::Params, count: u32) -> Vec<Step> {
        let nf = if s.play4 { count.max(1) } else { 1 };
        let mut st = Vec::new();
        let (n, start) = window(s, p);
        // the window counts words of all interleaved channels: n x ratio from point x ratio (Phase 0.5 step D)
        let win = scu::Window { n: n * p.ratio, start: start * p.ratio, ratio: p.ratio, fsm: s.mask, la: s.la.is_some() };
        st.push(Step::Note(format!("read-out: {} points from memory point {} (trigger at {})", n, start, p.pre)));
        let mut ro = if s.play4 {
            let mut v = scu::passthrough();
            if s.compress > 1 {
                // main compression on, factor k (live value 0xc0000004 at k = 4); TX length = compressed points
                for w in v.iter_mut().filter(|w| w.0 == 0x1170) {
                    w.1 = 0xc000_0000 | (s.compress & 0x0fff_ffff);
                }
            }
            let mut r = scu::readout_play4(&win, &mut self.sh, 0, nf);
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

/// Points per channel after the FPGA compression: the whole record / k, rounded up to 8 like the stock TX
/// length (roundup8(len)).
pub fn compressed_len(s: &Settings, p: &hz::Params) -> u32 {
    let k = s.compress.max(1);
    (p.memsel.div_ceil(k) + 7) / 8 * 8
}

/// Drops the stream padding behind every frame (`frame_stride`) in place: the frames end up back to back.
pub fn unpad(buf: &mut Vec<u8>, frame: usize) {
    let stride = frame_stride(frame);
    if frame == 0 || stride == frame {
        return;
    }
    let n = buf.len() / stride;
    for k in 1..n {
        buf.copy_within(k * stride..k * stride + frame, k * frame);
    }
    buf.truncate(n * frame);
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
    w.iter().map(|&x| ((x as f64 - 32768.0) / 7500.0 * scale - offset) as f32).collect()
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
/// `0x4010 - first + 1`), then play and read exactly those frames.
/// Returns the bytes (count x `frame_bytes`) and the count.
#[cfg(target_os = "linux")]
pub fn acquire_batch(st: &mut State, s: &Settings, p: &hz::Params, bar: &mut crate::bus::Bar1, c2h: &mut std::fs::File, wait_ms: u64, mut log: impl FnMut(String)) -> Result<(Vec<u8>, u32), String> {
    use crate::bus::Bus;
    let nf = frames(s, p);
    execute(&st.arm_steps(s, p), bar, None, c2h, &mut log)?;
    let t = std::time::Instant::now();
    let mut count = bar.rd(0x4010).wrapping_add(1).min(nf);
    while count < nf && t.elapsed().as_millis() < wait_ms as u128 {
        std::thread::sleep(std::time::Duration::from_micros(200));
        count = bar.rd(0x4010).wrapping_add(1).clamp(1, nf);
    }
    log(format!("{} of {} frames after {:.1} ms", count, nf, t.elapsed().as_secs_f64() * 1e3));
    let bytes = execute(&st.read_steps(s, p, count), bar, None, c2h, &mut log)?;
    Ok((bytes, count))
}

/// The LA read of the acquisition just read out (`la_read_steps`) on the unit: LA words, bit n = Dn, word `start` of
/// `la::window` first.
#[cfg(target_os = "linux")]
pub fn acquire_la(st: &mut State, s: &Settings, p: &hz::Params, bar: &mut crate::bus::Bar1, c2h: &mut std::fs::File, log: impl FnMut(String)) -> Result<Vec<u16>, String> {
    let (n, _) = la::window(&la::timing(p, s.mask));
    let steps = st.la_read_steps(s, p);
    match execute(&steps, bar, None, c2h, log) {
        Ok(bytes) => Ok(la::words(&bytes, n as usize)),
        Err(e) => {
            // the read failed before `la::restore` ran: without it the stream stays on the LA and the analog is gone
            use crate::bus::Bus;
            for (off, v) in la::restore(&mut st.sh) {
                bar.wr(off, v);
            }
            Err(e)
        }
    }
}

/// Runs steps on the unit; returns the bytes of the Read step. `log` gets notes and poll results.
#[cfg(target_os = "linux")]
pub fn execute(steps: &[Step], bar: &mut crate::bus::Bar1, afe_bus: Option<&mut afe::GpioAfe>, c2h: &mut std::fs::File, mut log: impl FnMut(String)) -> Result<Vec<u8>, String> {
    use crate::bus::Bus;
    let mut afe_bus = afe_bus;
    let mut out = Vec::new();
    for s in steps {
        match s {
            Step::Wr((off, v)) => bar.wr(*off, *v),
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
            Step::Poll { reg, bit, ms, what } => {
                let t = std::time::Instant::now();
                let mut last = bar.rd(*reg);
                while last & bit == 0 && t.elapsed().as_millis() < *ms as u128 {
                    std::thread::sleep(std::time::Duration::from_micros(500));
                    last = bar.rd(*reg);
                }
                let ok = last & bit != 0;
                log(format!("{} {} ({:#x} = {:#010x}, {:.1} ms)", what, if ok { "ok" } else { "TIMEOUT" }, reg, last, t.elapsed().as_secs_f64() * 1e3));
                if !ok && *reg == 0x4080 {
                    return Err("no acquisition (0x4080 bit30 not set)".into());
                }
            }
            Step::Read { frames, frame } => {
                let bytes = read_len(*frames, *frame);
                out = crate::bus::read_dma(c2h, bytes, |got, k| log(format!("c2h: short read of {} bytes (at {} of {})", k, got, bytes)))
                    .map_err(|e| format!("c2h read of {} bytes: {} (0x4034 = {:#x})", bytes, e, bar.rd(0x4034)))?;
                unpad(&mut out, *frame);
            }
            Step::Peek(regs) => log(regs.iter().map(|r| format!("{:#06x} = {:#010x}", r, bar.rd(*r))).collect::<Vec<_>>().join(", ")),
            Step::Sleep { us } => std::thread::sleep(std::time::Duration::from_micros(*us as u64)),
            Step::Note(n) => log(n.clone()),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    /// Holdoff writes only when one is set, and once more when it goes back.
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
        let has = |w: &[crate::Write]| w.iter().any(|x| x.0 == 0x2084);
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
        // trigger 17, acquisition 22, auto trigger 3, arm 29, pass-through 10, read-out window 7 + stop sequence 17 +
        // type-4 play 12 (the sequence that delivered own frames on the unit, 2026-11-03)
        assert_eq!(w.len(), 17 + 22 + 3 + 29 + 10 + 7 + 17 + 12);
        let k = w.iter().position(|x| *x == (0x1170, 0x4000_0001)).unwrap();
        assert!(k > 17 + 22 + 3 + 29 - 1, "pass-through comes after the arm");
        assert_eq!(*w.last().unwrap(), (0x4004, 0x1));
        // the play-last read-out of the warm hand-over is still available: 28 writes
        let mut state = State::handover(1, None);
        let (st, _) = plan(&mut state, &Settings { play4: false, ..Settings::default() }, None, None).unwrap();
        assert_eq!(writes(&st).len(), 17 + 22 + 3 + 29 + 28);
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
}
