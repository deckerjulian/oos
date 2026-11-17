// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Waveform sources (demo generator, replay of a captured frame), phosphor (hit-count) maps, measurements.
//! The live FPGA source (oos-fpgad, Phase 4) will plug in as another `Source`.

use crate::model::*;
use crate::board::{board, Board};

pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn uniform(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Approximate standard normal (sum of 4 uniforms).
    pub fn gauss(&mut self) -> f32 {
        let s = self.uniform() + self.uniform() + self.uniform() + self.uniform();
        ((s - 2.0) * 1.7320508) as f32
    }
}

/// One acquisition: samples per channel in volts at the probe tip, sample k at time t0 + k*dt relative to the trigger.
#[derive(Clone)]
pub struct Record {
    pub t0: f64,
    pub dt: f64,
    pub ch: [Vec<f32>; NCH],
    pub triggered: bool,
    /// roll/scan picture before the screen is complete (SINGLE waits for a complete one)
    pub partial: bool,
    /// hardware source: the frame behind the record and the sample range of the screen window, for measurements on
    /// every captured sample (shared, not copied; dropped for the history)
    pub raw: Option<Raw>,
    /// logic analyser lines D0..D15 of the same acquisition (LA on)
    pub la: Option<crate::ui::la::LaRec>,
    /// the points are min/max pairs (`to_record` peak detect: point 2j the minimum, 2j+1 the maximum of one span)
    pub pairs: bool,
}

#[derive(Clone)]
pub struct Raw {
    pub frame: std::sync::Arc<crate::board::Frame>,
    pub i0: usize,
    pub i1: usize,
    /// time of sample i0 relative to the trigger (s)
    pub t0: f64,
    /// Native values already include the acquisition's coupling/filter/invert.
    /// A reconstructed display must not condition those samples a second time.
    pub conditioned: bool,
}

/// State of a hardware segmented recording as the UI sees it (`Source::seg_sync`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SegView {
    pub phase: crate::board::SegPhase,
    pub total: u32,
    pub recorded: u32,
    pub read: u32,
    /// frames of the read-back still in the queue
    pub queued: bool,
}

pub enum Source {
    Demo { t: f64 },
    Replay { words: Vec<u16>, xinc: f64, yinc: f64, yorg: f64, yref: f64 },
    /// frames from the read-out-only debug source of the board (`Board::live`)
    Live { shared: crate::board::LiveShared, seen: u64, words: Vec<u16>, xinc: f64, yinc: f64, yorg: f64, yref: f64 },
    /// acquisitions of the board (`Board::spawn`)
    Hw { shared: crate::board::HwShared, seen: u64 },
}

/// What the demo signals show: plain, or with features for the selected trigger type (runts / missing pulses on CH1,
/// composite video on CH2).
#[derive(Clone, Copy)]
pub struct DemoCfg {
    pub runts: bool,
    pub video: Option<oos_dsp::sim::VideoStd>,
    /// bus traffic for the decoder: I2C on CH3 (SDA) / CH4 (SCL), SPI on CH2 (CS) / CH3 (MOSI) / CH4 (CLK)
    pub bus: Option<Proto>,
    /// the generator looped back on these channels (`OOS_DEMO_LOOP`, host tests)
    pub gen: Option<DemoGen>,
}

/// The generator of the model as a demo signal.
#[derive(Clone, Copy)]
pub struct DemoGen {
    mask: u32,
    on: bool,
    wave: usize,
    freq: f64,
    ampl: f64,
    offset: f64,
    duty: f64,
    /// the cable is at the BNC, the demo signals at the probe tip
    probe: [f32; NCH],
}

impl DemoGen {
    fn of(m: &Model) -> Option<DemoGen> {
        static LOOP: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
        let mask = *LOOP.get_or_init(|| std::env::var("OOS_DEMO_LOOP").ok().and_then(|v| v.parse().ok()).unwrap_or(0));
        (mask != 0).then_some(DemoGen { mask, on: m.afg_on, wave: m.afg_wave, freq: m.afg_freq, ampl: m.afg_ampl, offset: m.afg_offset, duty: m.afg_duty / 100.0, probe: std::array::from_fn(|c| m.ch[c].probe) })
    }

    fn v(&self, t: f64) -> f64 {
        if !self.on {
            return 0.0;
        }
        let ph = (t * self.freq).rem_euclid(1.0);
        let half = self.ampl / 2.0;
        self.offset
            + half
                * match self.wave {
                    0 => (2.0 * std::f64::consts::PI * ph).sin(),
                    1 | 3 => {
                        if ph < self.duty {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                    2 => 1.0 - 4.0 * (ph - 0.5).abs(),
                    4 => ((t * 1e9) as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(17) as f64 / u64::MAX as f64 * 2.0 - 1.0,
                    5 => 0.0,
                    _ => board().arb_at(ph),
                }
    }
}

impl DemoCfg {
    pub fn of(m: &Model) -> DemoCfg {
        DemoCfg {
            runts: matches!(m.trig_type, TrigType::Runt | TrigType::Timeout),
            video: if m.trig_type == TrigType::Video { Some(m.video_std) } else { None },
            bus: if m.dec_on && m.dec_proto != Proto::UART { Some(m.dec_proto) } else { None },
            gen: DemoGen::of(m),
        }
    }
}

/// Demo bus traffic, repeated: I2C 400 kHz (write 0x50 [00 42], repeated start, read [4F 53] = "OS") every 200 us;
/// SPI mode 0, 2 MHz, words 9F/FF 00/1F 00/40 every 50 us.
fn demo_bus() -> &'static (oos_dsp::sim::Logic, oos_dsp::sim::Logic, [oos_dsp::sim::Logic; 4]) {
    use oos_dsp::sim::{i2c_logic, spi_logic, I2cXfer};
    static BUS: std::sync::OnceLock<(oos_dsp::sim::Logic, oos_dsp::sim::Logic, [oos_dsp::sim::Logic; 4])> = std::sync::OnceLock::new();
    BUS.get_or_init(|| {
        let (sda, scl) = i2c_logic(
            &[
                I2cXfer { addr: 0x50, read: false, data: vec![0x00, 0x42], repeated: false, stop: false },
                I2cXfer { addr: 0x50, read: true, data: vec![0x4f, 0x53], repeated: true, stop: true },
            ],
            400e3,
            0.0,
            3.3,
        );
        let spi = spi_logic(&[(0x9f, 0xff), (0x00, 0x1f), (0x00, 0x40)], 8, 0, 2e6, 0.0, 3.3);
        (sda, scl, spi)
    })
}





fn demo_signal(cfg: DemoCfg, ch: usize, t: f64) -> f64 {
    use std::f64::consts::PI;
    if let Some(g) = cfg.gen {
        // the cable is what the channel sees, nothing else
        return if g.mask & 1 << ch != 0 { g.v(t) * g.probe[ch] as f64 } else { 0.0 };
    }
    match (cfg.bus, ch) {
        // a protocol module: its demo lines on the channels of its setup
        (Some(Proto::Module(i)), _) => {
            if let Some(d) = crate::protocols::demo(i) {
                let s = crate::protocols::get(i).setup();
                if let Some(f) = (0..4).find(|&r| s.src[r] == Some(ch)).and_then(|r| d.lines[r].as_ref()) {
                    return f(t.rem_euclid(d.period));
                }
            }
        }
        _ => {}
    }
    match ch {
        0 => {
            // probe compensation square, 1 kHz, 0..2.5 V, RC edges; with runts: every 5th pulse only 45 % high
            let k = (t * 1000.0).floor() as i64;
            let ph = (t * 1000.0).rem_euclid(1.0);
            let tau = 1.2e-6 * 1000.0; // in periods
            let h = if cfg.runts && k.rem_euclid(5) == 2 { 0.45 } else { 1.0 };
            if ph < 0.5 {
                2.5 * h * (1.0 - (-ph / tau).exp())
            } else {
                2.5 * h * (-(ph - 0.5) / tau).exp()
            }
        }
        1 if cfg.video.is_some() => oos_dsp::sim::video(cfg.video.unwrap(), t),
        1 => {
            let w = 2.0 * PI * 10e3 * t;
            1.0 * w.sin() + 0.08 * (3.0 * w).sin()
        }
        2 => {
            // UART 115200 8N1, 0 / 3.3 V, idle high, sending "OOS 0.1 " in a loop with one idle byte between strings
            const MSG: &[u8] = b"OOS 0.1 ";
            let bit = 1.0 / 115200.0;
            let nb = (t / bit).floor() as i64;
            let frame = nb.div_euclid(10);
            let pos = nb.rem_euclid(10);
            let k = frame.rem_euclid(MSG.len() as i64 + 1) as usize;
            let hi = if k == MSG.len() {
                true
            } else {
                match pos {
                    0 => false,
                    9 => true,
                    p => (MSG[k] >> (p - 1)) & 1 == 1,
                }
            };
            // finite edge rate: 150 ns transitions
            let within = (t / bit - nb as f64) * bit;
            let edge = (within / 150e-9).min(1.0);
            let v = if hi { 3.3 } else { 0.0 };
            let prev_hi = {
                let nb2 = nb - 1;
                let fr2 = nb2.div_euclid(10);
                let p2 = nb2.rem_euclid(10);
                let k2 = fr2.rem_euclid(MSG.len() as i64 + 1) as usize;
                if k2 == MSG.len() {
                    true
                } else {
                    match p2 {
                        0 => false,
                        9 => true,
                        p => (MSG[k2] >> (p - 1)) & 1 == 1,
                    }
                }
            };
            let pv = if prev_hi { 3.3 } else { 0.0 };
            pv + (v - pv) * edge
        }
        _ => {
            // PWM 20 kHz with slowly sweeping duty and ringing after the edges
            let f = 20e3;
            let duty = 0.5 + 0.3 * (2.0 * PI * 0.2 * t).sin();
            let ph = (t * f).rem_euclid(1.0);
            let hi = ph < duty;
            let since = if hi { ph } else { ph - duty } / f;
            let ring = 0.35 * (-since / 0.4e-6).exp() * (2.0 * PI * 8e6 * since).cos();
            if hi {
                3.3 + ring
            } else {
                -ring
            }
        }
    }
}

const NOISE: [f32; NCH] = {
    let base = [0.012, 0.006, 0.010, 0.015];
    let mut n = [0.0; NCH];
    let mut i = 0;
    while i < NCH {
        n[i] = base[i % 4];
        i += 1;
    }
    n
};

/// Demo logic lines at time t, bit n = Dn, matching the analog demo: D0-D7 an 8-bit counter at 100 kHz, D8 the UART
/// of CH3 ("OOS 0.1 ", 115200 baud), D9-D11 SPI (CS, CLK, MOSI; mode 0, 2 MHz), D12/D13 I2C (SCL, SDA; 400 kHz), D14
/// the 1 kHz square of CH1, D15 the 20 kHz PWM of CH4. Thresholds at half the logic level.
fn demo_la(t: f64) -> u16 {
    let plain = DemoCfg { runts: false, video: None, bus: None, gen: None };
    let (sda, scl, spi) = demo_bus();
    let mut w = (t * 100e3).floor().rem_euclid(256.0) as u16;
    let ts = t.rem_euclid(50e-6);
    let ti = t.rem_euclid(200e-6);
    for (n, v, thr) in [
        (8, demo_signal(plain, 2, t), 1.65),
        (9, spi[0].v(ts), 1.65),
        (10, spi[1].v(ts), 1.65),
        (11, spi[2].v(ts), 1.65),
        (12, scl.v(ti), 1.65),
        (13, sda.v(ti), 1.65),
        (14, demo_signal(plain, 0, t), 1.25),
        (15, demo_signal(plain, 3, t), 1.65),
    ] {
        if v > thr {
            w |= 1 << n;
        }
    }
    w
}

/// The record `Source::acquire` makes of a hardware frame: 3600 points across the screen.
pub fn hw_record(m: &Model, f: crate::board::Frame) -> Record {
    let window = m.time_per_div() as f64 * HDIV as f64;
    crate::board::to_record(&std::sync::Arc::new(f), m, 3600, window, m.delay as f64 - window / 2.0)
}

impl Source {
    /// The hardware applies inversion, coupling and bandwidth itself; the other sources get them from `condition`.
    pub fn is_hw(&self) -> bool {
        matches!(self, Source::Hw { .. })
    }

    /// The made-up signals of the demo: the starting values of the trigger types are made for them.
    pub fn is_demo(&self) -> bool {
        matches!(self, Source::Demo { .. })
    }

    /// Ask the acquisition thread for one force-trigger pulse. Other sources ignore it.
    pub fn force_hw(&self) {
        if let Source::Hw { shared, .. } = self {
            let mut state = shared.lock().unwrap();
            state.force = true;
            #[cfg(test)]
            { state.force_requests += 1; }
        }
    }

    /// The generator settings for the hardware thread: every step, the output must follow also while stopped.
    /// Readers of the records that only the UI knows of (protocol watch, XY) for the hardware thread
    /// (`Shared::needs_raw`: no live compression).
    pub fn hw_needs_raw(&self, on: bool) {
        if let Source::Hw { shared, .. } = self {
            shared.lock().unwrap().needs_raw = on;
        }
    }

    /// STOP for the hardware thread (`Shared::hold`): whether it may arm, and the newest final-frame request. True when
    /// that request is done and every frame before it was taken (always for other sources). The release (RUN, SINGLE)
    /// drops what was queued during the hold: the thread does so on its next pass, but the UI takes frames at once.
    pub fn hw_hold(&self, hold: bool, req: u64) -> bool {
        if let Source::Hw { shared, .. } = self {
            let mut g = shared.lock().unwrap();
            if g.hold && !hold {
                g.queue.clear();
            }
            (g.hold, g.final_req) = (hold, req);
            return g.final_ack >= req && g.queue.is_empty();
        }
        true
    }

    pub fn publish_afg(&self, m: &Model) {
        if let Source::Hw { shared, .. } = self {
            let mut g = shared.lock().unwrap();
            for i in 0..gen_count() {
                g.afg[i] = Some(board().gen(&m.gen_ch(i)));
            }
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Source::Demo { .. } => "DEMO",
            Source::Replay { .. } => "REPLAY",
            Source::Live { .. } => "FPGA",
            Source::Hw { .. } => "HW",
        }
    }

    /// Signal of channel `ch` sampled n times from t0 with step dt (no trigger), used by auto set-up.
    pub fn probe(&self, ch: usize, t0: f64, dt: f64, n: usize) -> Vec<f32> {
        match self {
            Source::Demo { .. } => (0..n).map(|k| demo_signal(DemoCfg { runts: false, video: None, bus: None, gen: None }, ch, t0 + k as f64 * dt) as f32).collect(),
            Source::Live { words, yinc, yorg, yref, xinc, .. } | Source::Replay { words, yinc, yorg, yref, xinc } => {
                if ch != 0 || words.is_empty() {
                    return vec![0.0; n];
                }
                (0..n)
                    .map(|k| {
                        let i = ((k as f64 * dt) / xinc) as usize % words.len();
                        ((words[i] as f64 - yorg - yref) * yinc) as f32
                    })
                    .collect()
            }
            Source::Hw { shared, .. } => {
                // the latest hardware record of the channel, wrapped around (auto set-up looks at levels and periods)
                let f = shared.lock().unwrap().frame.clone();
                let v = &f.ch[ch];
                if v.is_empty() {
                    return vec![0.0; n];
                }
                (0..n).map(|k| v[((k as f64 * dt) / f.dt) as usize % v.len()]).collect()
            }
        }
    }

    /// Hardware source: hands the counter set-up the UI wants to the acquisition thread (None: off) and returns the
    /// latest reading; None off the hardware (the counter is then worked out from the records).
    pub fn hw_counter(&self, want: Option<crate::board::CounterWant>) -> Option<Option<crate::board::CounterRead>> {
        if let Source::Hw { shared, .. } = self {
            let mut g = shared.lock().unwrap();
            g.counter_want = want;
            return Some(g.counter);
        }
        None
    }

    /// Hardware source: why the mask/zone unit is off for the current settings (`Shared::unit_blocked`).
    pub fn hw_unit_blocked(&self) -> Option<String> {
        if let Source::Hw { shared, .. } = self {
            return shared.lock().unwrap().unit_blocked.clone();
        }
        None
    }

    /// Hardware source: the mask table last loaded (`Shared::mask_table`).
    pub fn hw_mask_table(&self) -> Option<std::sync::Arc<Vec<[u8; 4]>>> {
        if let Source::Hw { shared, .. } = self {
            return shared.lock().unwrap().mask_table.clone();
        }
        None
    }

    /// Hardware source: hands the zone trigger the UI wants to the acquisition thread (None: off) and returns the latest
    /// reading; None off the hardware.
    pub fn hw_zone(&self, want: Option<crate::board::ZoneWant>) -> Option<Option<crate::board::ZoneRead>> {
        if let Source::Hw { shared, .. } = self {
            let mut g = shared.lock().unwrap();
            if g.zone_want != want {
                g.zone_want = want;
            }
            return Some(g.zone);
        }
        None
    }

    /// Hardware source: hands the mask test the UI wants to the acquisition thread (None: off) and returns the latest
    /// counter reading; None off the hardware.
    pub fn hw_mask(&self, want: Option<crate::board::MaskWant>) -> Option<Option<crate::board::MaskRead>> {
        if let Source::Hw { shared, .. } = self {
            let mut g = shared.lock().unwrap();
            if g.mask_want != want {
                g.mask_want = want;
            }
            return Some(g.mask);
        }
        None
    }

    /// Hardware source: hands the DVM set-up the UI wants to the acquisition thread (None: off) and returns the latest
    /// reading; None off the hardware.
    pub fn hw_dvm(&self, want: Option<crate::board::DvmWant>) -> Option<Option<crate::board::DvmRead>> {
        if let Source::Hw { shared, .. } = self {
            let mut g = shared.lock().unwrap();
            g.dvm_want = want;
            return Some(g.dvm);
        }
        None
    }

    /// Hardware source: asks for measure-unit levels (`Shared::levels_want`) and returns the latest answer; None for
    /// other sources.
    pub fn hw_levels(&self, want: Option<(u64, u64, u32)>) -> Option<Option<(u64, [Option<crate::board::ChLevels>; 4])>> {
        if let Source::Hw { shared, .. } = self {
            let mut g = shared.lock().unwrap();
            g.levels_want = want;
            return Some(g.levels);
        }
        None
    }

    /// Hardware source: asks for one raw measure-unit run (`Shared::slots_want`) and returns the latest answer; None
    /// for other sources.
    pub fn hw_slots(&self, want: u64) -> Option<Option<crate::board::SlotDump>> {
        if let Source::Hw { shared, .. } = self {
            let mut g = shared.lock().unwrap();
            g.slots_want = Some(want);
            return Some(g.slots);
        }
        None
    }

    /// Hardware source with `trig.hw_status = on`: the latest trigger status of a batch (`Shared::trig_fired`).
    pub fn hw_trig_fired(&self) -> Option<(u64, Option<bool>)> {
        if let Source::Hw { shared, .. } = self {
            return shared.lock().unwrap().trig_fired;
        }
        None
    }

    /// Hardware source: FPGA version (0 = not reached) and the latest XADC reading (die degC, VCCINT V).
    pub fn hw_board(&self) -> Option<(u32, Option<(f32, f32)>)> {
        if let Source::Hw { shared, .. } = self {
            let g = shared.lock().unwrap();
            return Some((g.fpga_version, g.xadc));
        }
        None
    }

    /// Sample rate and points per channel of the latest hardware frame (the model's numbers are the stock system's
    /// choice for Auto depth; the hardware source reads a shorter live record).
    pub fn hw_rate_depth(&self) -> Option<(f64, usize)> {
        if let Source::Hw { shared, .. } = self {
            let g = shared.lock().unwrap();
            let n = g.frame.ch.iter().map(|c| c.len()).max().unwrap_or(0);
            if g.frame.seq > 0 && g.frame.acq.1 > 0 {
                return Some(g.frame.acq);
            }
            if g.frame.seq > 0 && g.frame.dt > 0.0 && n > 0 {
                return Some((1.0 / g.frame.dt, n));
            }
        }
        None
    }

    /// Sample rate and depth as the top bar shows them: of the hardware when it runs, else the model's choice.
    pub fn rate_depth(&self, m: &Model) -> (f64, u64) {
        self.hw_rate_depth().map(|(r, n)| (r, n as u64)).unwrap_or_else(|| m.srate_depth())
    }

    /// Hardware segments: tells the thread the recording number and whether the acquisition runs; returns phase,
    /// segments asked for, recorded and read back, and whether frames are still queued. None without the hardware
    /// source.
    pub fn seg_sync(&self, run: u64, running: bool) -> Option<SegView> {
        let Source::Hw { shared, .. } = self else { return None };
        let mut g = shared.lock().unwrap();
        (g.seg.run, g.seg.running) = (run, running);
        let current = g.seg.done_run == run;
        Some(SegView { phase: if current { g.seg.phase } else { crate::board::SegPhase::Idle }, total: g.seg.total, recorded: g.seg.recorded, read: g.seg.read, queued: !g.queue.is_empty() })
    }

    /// Hardware segments: the trigger times read back so far (seconds since the first trigger).
    pub fn seg_times(&self) -> Vec<Option<f64>> {
        match self {
            Source::Hw { shared, .. } => shared.lock().unwrap().seg.times.clone(),
            _ => Vec::new(),
        }
    }

    /// Hardware segments: asks the thread to play segment `k`; `seg_shown` delivers it.
    pub fn seg_request(&self, k: u32) {
        if let Source::Hw { shared, .. } = self {
            let mut g = shared.lock().unwrap();
            (g.seg.want, g.seg.shown) = (Some(k), None);
        }
    }

    /// Hardware segments: the segment asked for with `seg_request`, as a record for the screen.
    pub fn seg_shown(&self, m: &Model) -> Option<(u32, Record)> {
        let Source::Hw { shared, .. } = self else { return None };
        let (k, f) = shared.lock().unwrap().seg.shown.take()?;
        let window = m.time_per_div() as f64 * HDIV as f64;
        Some((k, crate::board::to_record(&std::sync::Arc::new(f), m, 3600, window, m.delay as f64 - window / 2.0)))
    }

    /// Hardware source in STOP: the view shown (time per div, delay; None: none to read for), and the frame the
    /// thread has read again from the stored record for it, if a new one came (`board::Shared::stop_view`).
    pub fn hw_stop(&mut self, view: Option<(f64, f64)>) -> Option<crate::board::Frame> {
        if let Source::Hw { shared, .. } = self {
            let mut g = shared.lock().unwrap();
            g.stop_view = view;
            return g.replay.take().filter(|_| view.is_some());
        }
        None
    }

    /// Hardware source: every frame of a batch decoded again (the record path, which takes them one by one).
    pub fn hw_take_all(&mut self) {
        if let Source::Hw { shared, .. } = self {
            shared.lock().unwrap().take = 0;
        }
    }

    /// Hardware source: frames decoded and frames dropped unseen so far (`board::Shared`).
    pub fn hw_frames(&self) -> Option<(u64, u64)> {
        match self {
            Source::Hw { shared, .. } => shared.lock().ok().map(|g| (g.decoded, g.dropped)),
            _ => None,
        }
    }

    /// Hardware source: takes the newest `max` queued frames but the newest one (the fast display path draws them;
    /// the newest stays for `acquire`, which makes the full record for measurements, decoding and history). Older
    /// ones are dropped and counted.
    pub fn drain_newest(&mut self, max: usize) -> Vec<crate::board::Frame> {
        if let Source::Hw { shared, .. } = self {
            let mut g = shared.lock().unwrap();
            // the thread decodes no more than this of a batch (the record takes one more)
            g.take = max + 1;
            let n = g.queue.len().saturating_sub(1);
            let skip = n.saturating_sub(max);
            g.queue.drain(..skip);
            g.dropped += skip as u64;
            return g.queue.drain(..n - skip).collect();
        }
        Vec::new()
    }

    /// One acquisition for the current settings, or None if Normal/Single mode found no trigger.
    pub fn acquire(&mut self, m: &Model, dt_wall: f64, rng: &mut Rng) -> Option<Record> {
        self.acquire_impl(m, dt_wall, rng, false)
    }

    /// Full-depth demo acquisition, called only by the bounded demo worker.
    pub(crate) fn acquire_full_demo(&mut self, m: &Model, dt_wall: f64, rng: &mut Rng) -> Option<Record> {
        self.acquire_impl(m, dt_wall, rng, true)
    }

    fn acquire_impl(&mut self, m: &Model, dt_wall: f64, rng: &mut Rng, full_demo: bool) -> Option<Record> {
        let tdiv = m.hw_time_per_div() as f64;
        let (window, n) = if full_demo && matches!(self, Source::Demo { .. }) {
            let (rate, points) = m.srate_depth();
            (points as f64 / rate, points as usize)
        } else { (tdiv * HDIV as f64, 3600usize) };
        let dt = window / n as f64;
        let t_first = m.delay as f64 - window / 2.0; // relative to the trigger
        let level = m.trig_level as f64;
        if let Source::Hw { shared, seen } = self {
            let mut g = shared.lock().unwrap();
            g.want = Some(board().want(m));
            g.deep_raw = crate::deepmeas::want(m);
            g.slow = board().slow_mode(m);
            // running: no STOP view any more, the thread acquires again
            (g.stop_view, g.replay) = (None, None);
            g.stamp = m.hw_stamp;
            g.cores = m.cores;
            g.avg_clear = m.hw_avg_clear;
            // SINGLE takes nothing from before the key press
            let fresh = loop {
                match g.queue.pop_front() {
                    Some(f) if m.trig_mode == TrigMode::Single && f.stamp < m.hw_stamp => continue,
                    f => break f,
                }
            };
            let Some(f) = fresh else {
                // a SINGLE scan is complete and SINGLE was pressed again: next sweep
                if g.halted {
                    g.halted = false;
                    g.rearm = true;
                }
                return None;
            };
            *seen = f.seq;
            drop(g);
            return Some(hw_record(m, f));
        }
        if let Source::Live { shared, seen, words, xinc, yinc, yorg, yref } = self {
            let g = shared.lock().unwrap();
            if g.0 == *seen || g.1.is_empty() {
                return None; // no new frame
            }
            *seen = g.0;
            *words = g.1.clone();
            drop(g);
            let mut tmp = Source::Replay { words: std::mem::take(words), xinc: *xinc, yinc: *yinc, yorg: *yorg, yref: *yref };
            let r = tmp.acquire(m, dt_wall, rng);
            if let Source::Replay { words: w, .. } = tmp {
                *words = w;
            }
            return r;
        }
        match self {
            Source::Demo { t } => {
                *t += dt_wall;
                let start = *t + rng.uniform() * 1e-3;
                // software trigger (oos-dsp) on the noise-free signals of the channels the trigger reads, sampled finely
                // enough for the trigger's time condition and long enough to decide it
                let cfg = DemoCfg::of(m);
                let used = m.trig_channels();
                let span = window * 1.5 + m.trig_span();
                let mut dts = dt.min(m.trig_feature()).max(1e-9);
                let mut ns = (span / dts).ceil() as usize;
                if ns > 400_000 {
                    ns = 400_000;
                    dts = span / ns as f64;
                }
                let ns = ns.max(64);
                let bufs: Vec<Vec<f32>> = (0..NCH)
                    .map(|c| {
                        if !used[c] {
                            return Vec::new();
                        }
                        let sg = if m.ch[c].invert { -1.0 } else { 1.0 };
                        let (bias, skew) = (m.ch[c].bias as f64, m.ch[c].skew as f64);
                        (0..ns).map(|k| (sg * (demo_signal(cfg, c, start + k as f64 * dts - skew) - bias)) as f32).collect()
                    })
                    .collect();
                let hyst: [f32; NCH] = std::array::from_fn(|c| 0.1 * m.ch[c].volts_per_div());
                let inp = oos_dsp::trigger::Input { ch: std::array::from_fn(|c| &bufs[c][..]), dt: dts, hyst };
                // a pattern with D lines: 1 where the demo lines and the channels match; the same trigger type on this one
                // wave then gives the begin of the pattern, or its time condition for a duration trigger
                let la_pat = m.pat_on_la().then(|| {
                    use oos_dsp::trigger::Trigger;
                    let hit: Vec<f32> = (0..ns)
                        .map(|k| {
                            let w = demo_la(start + k as f64 * dts);
                            let ch = (0..NCH).all(|c| match m.pat[c] {
                                Bit::H => bufs[c].get(k).is_some_and(|&v| v > m.pat_levels[c]),
                                Bit::L => bufs[c].get(k).is_some_and(|&v| v <= m.pat_levels[c]),
                                _ => true,
                            });
                            (ch && crate::ui::la::pattern_is(w, &m.pat_la)) as u8 as f32
                        })
                        .collect();
                    let e: [f32; 0] = [];
                    let inp = oos_dsp::trigger::Input { ch: std::array::from_fn(|c| if c == 0 { &hit[..] } else { &e[..] }), dt: dts, hyst: [0.1; NCH] };
                    let bits = std::array::from_fn(|i| if i == 0 { Bit::H } else { Bit::X });
                    let t = match m.when() {
                        Some(when) if m.trig_type == TrigType::Duration => Trigger::Duration { bits, levels: [0.5; NCH], when },
                        _ => Trigger::Pattern { bits, levels: [0.5; NCH] },
                    };
                    oos_dsp::trigger::find_first(&t, &inp, 0.0)
                });
                let found = match m.trig_line() {
                    _ if la_pat.is_some() => la_pat.flatten(),
                    // protocol trigger on LA lines: the decoder over the demo lines
                    _ if m.bus_on_la() => {
                        let lines: Vec<u16> = (0..ns).map(|k| demo_la(start + k as f64 * dts)).collect();
                        crate::trig::points(m, &crate::trig::decode_la(m, &lines, dts).unwrap_or_default()).first().copied()
                    }
                    // LA line: 0 / 1 samples of the demo line, the same trigger on them at level 0.5
                    Some(n) => {
                        let line: Vec<f32> = (0..ns).map(|k| (demo_la(start + k as f64 * dts) >> n & 1) as f32).collect();
                        let e: [f32; 0] = [];
                        let inp = oos_dsp::trigger::Input { ch: std::array::from_fn(|c| if c == 0 { &line[..] } else { &e[..] }), dt: dts, hyst: [0.1; NCH] };
                        let mut s = m.clone();
                        (s.trig_la, s.trig_src, s.trig_level) = (None, 0, 0.5);
                        oos_dsp::trigger::find_first(&s.trigger(), &inp, 0.0)
                    }
                    None => crate::trig::find_first(m, &inp, 0.0),
                }
                .map(|i| start + i * dts);
                let (ttrig, triggered) = match found {
                    Some(tt) => (tt, true),
                    None => {
                        if m.trig_mode != TrigMode::Auto {
                            return None;
                        }
                        (start, false)
                    }
                };
                // trigger jitter of a real ADC: +-0.3 sample
                let jitter = (rng.uniform() - 0.5) * 0.6 * dt.min(8e-10);
                let mut chs: [Vec<f32>; NCH] = Default::default();
                for c in 0..NCH {
                    if !m.ch[c].on {
                        continue;
                    }
                    let mut v = Vec::with_capacity(n);
                    let noise = NOISE[c] * if m.acq == AcqMode::HiRes { 0.3 } else { 1.0 } * if cfg.gen.is_some() { 0.02 } else { 1.0 };
                    let (bias, skew) = (m.ch[c].bias, m.ch[c].skew as f64);
                    for k in 0..n {
                        let tk = ttrig + jitter + t_first + k as f64 * dt;
                        v.push(demo_signal(cfg, c, tk - skew) as f32 - bias + noise * rng.gauss());
                    }
                    chs[c] = v;
                }
                // the 16 logic lines over the same window, finer than the analog points (bus-style bands show)
                let la = m.la_on().then(|| {
                    let nl = crate::ui::la::DEMO_SAMPLES;
                    let dl = window / nl as f64;
                    let words = (0..nl).map(|k| demo_la(ttrig + jitter + t_first + k as f64 * dl)).collect();
                    crate::ui::la::LaRec { words: std::sync::Arc::new(words), t0: t_first, dt: dl }
                });
                Some(Record { t0: t_first, dt, ch: chs, triggered, partial: false, raw: None, la, pairs: false })
            }
            Source::Replay { words, xinc, yinc, yorg, yref } => {
                let mut volts: Vec<f32> = words.iter().map(|&w| ((w as f64 - *yorg - *yref) * *yinc) as f32).collect();
                condition(m, 0, &mut volts, *xinc);
                // software trigger (oos-dsp) on the stored samples; only CH1 is stored, other sources: edge on CH1
                let trig = if m.trig_channels().iter().enumerate().all(|(c, &u)| !u || c == 0) {
                    m.trigger()
                } else {
                    oos_dsp::trigger::Trigger::Edge { src: 0, slope: oos_dsp::trigger::Slope::Rising, level: level as f32 }
                };
                let e: [f32; 0] = [];
                let inp = oos_dsp::trigger::Input { ch: std::array::from_fn(|c| if c == 0 { &volts[..] } else { &e[..] }), dt: *xinc, hyst: [0.1 * m.ch[0].volts_per_div(); NCH] };
                let ti = oos_dsp::trigger::find_first(&trig, &inp, 0.0);
                let triggered = ti.is_some();
                let ti = ti.unwrap_or(volts.len() as f64 / 2.0);
                // resample the stored record onto the screen grid (linear interpolation)
                let mut v = Vec::with_capacity(n);
                for k in 0..n {
                    let ts = t_first + k as f64 * dt; // relative to trigger
                    let x = ti + ts / *xinc;
                    if x < 0.0 || x >= (volts.len() - 1) as f64 {
                        v.push(f32::NAN);
                    } else {
                        let i = x as usize;
                        let f = (x - i as f64) as f32;
                        v.push(volts[i] * (1.0 - f) + volts[i + 1] * f);
                    }
                }
                let mut chs: [Vec<f32>; NCH] = Default::default();
                chs[0] = v;
                let mut native: [Vec<f32>; NCH] = Default::default();
                native[0] = volts;
                let basic = std::array::from_fn(|c| crate::board::BasicStats::of(&native[c]));
                let native_len = native[0].len();
                let frame = std::sync::Arc::new(crate::board::Frame { seq: 1, dt: *xinc,
                    pre: ti.round().clamp(0.0, native_len.saturating_sub(1) as f64) as usize,
                    ch: native, roll: false, envelope: false, scan: false, partial: false,
                    acq: (1.0 / *xinc, native_len), stamp: 0, basic, la: None, fixed_ti: None, hw_avg: false });
                let raw = Some(Raw { frame, i0: 0, i1: native_len, t0: -ti * *xinc, conditioned: true });
                Some(Record { t0: t_first, dt, ch: chs, triggered, partial: false, raw, la: None, pairs: false })
            }
            Source::Live { .. } | Source::Hw { .. } => None, // handled above
        }
    }
}

/// Applies coupling, bandwidth limit and inversion to a record channel (display path).
pub fn condition(m: &Model, c: usize, v: &mut Vec<f32>, dt: f64) {
    let ch = &m.ch[c];
    match ch.coupling {
        Coupling::Gnd => v.iter_mut().for_each(|x| *x = 0.0),
        Coupling::Ac => {
            let finite: Vec<f32> = v.iter().copied().filter(|x| x.is_finite()).collect();
            if !finite.is_empty() {
                let mean = finite.iter().sum::<f32>() / finite.len() as f32;
                v.iter_mut().for_each(|x| *x -= mean);
            }
        }
        Coupling::Dc | Coupling::Dc50 => {}
    }
    if ch.bw == BW_20M {
        let a = (1.0 - (-2.0 * std::f64::consts::PI * 20e6 * dt).exp()) as f32;
        let mut y = v.first().copied().unwrap_or(0.0);
        for x in v.iter_mut() {
            if x.is_finite() {
                y += a * (*x - y);
                *x = y;
            }
        }
    }
    if ch.invert {
        v.iter_mut().for_each(|x| *x = -*x);
    }
}

/// Hit-count maps (one per channel) of the waveform area: the digital phosphor.
pub struct Phosphor {
    pub w: usize,
    pub h: usize,
    pub map: [Vec<u16>; NCH],
}

impl Phosphor {
    pub fn new(w: usize, h: usize) -> Phosphor {
        Phosphor { w, h, map: Default::default() }.init()
    }
    fn init(mut self) -> Phosphor {
        for c in 0..NCH {
            self.map[c] = vec![0; self.w * self.h];
        }
        self
    }
    pub fn clear(&mut self) {
        for m in self.map.iter_mut() {
            m.iter_mut().for_each(|x| *x = 0);
        }
    }
    /// Multiplies every count by f (0..1).
    pub fn decay(&mut self, f: f32) {
        let k = (f.clamp(0.0, 1.0) * 65536.0) as u32;
        if k >= 65536 {
            return;
        }
        for m in self.map.iter_mut() {
            for x in m.iter_mut() {
                if *x != 0 {
                    *x = ((*x as u32 * k) >> 16) as u16;
                }
            }
        }
    }

    /// Halves every count (non-zero counts stay >= 1, so rare hits survive): keeps the density ratios when the
    /// counts approach saturation under long persistence.
    pub fn halve(&mut self) {
        for m in self.map.iter_mut() {
            for x in m.iter_mut() {
                if *x != 0 {
                    *x = (*x >> 1).max(1);
                }
            }
        }
    }

    /// Highest sum of the maps of the channels in `on` at one pixel (colour grade).
    pub fn merged_max(&self, on: [bool; NCH]) -> u32 {
        let maps: Vec<&[u16]> = (0..NCH).filter(|&c| on[c]).map(|c| self.map[c].as_slice()).collect();
        match maps.len() {
            0 => 0,
            1 => maps[0].iter().copied().max().unwrap_or(0) as u32,
            _ => (0..self.w * self.h).map(|i| maps.iter().map(|m| m[i] as u32).sum::<u32>()).max().unwrap_or(0),
        }
    }

    /// Draws a record channel into the map: every segment between two samples adds `w` to the pixels it covers,
    /// so the brightness follows the dwell time of the trace like on an analog/digital-phosphor screen. Display
    /// options: dots (`Model::dots`, one pixel per sample) and per-sample brightness weights 0..1 (Z input).
    /// `pairs` (`Record::pairs`): with dots, each min/max pair is one vertical span, since it stands for every sample
    /// between its extremes; two dots would drop the edges of a slow time base.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_ex(&mut self, c: usize, m: &Model, view: (f32, f32), rec_t0: f64, dt: f64, v: &[f32], w: u16, z: Option<&[f32]>, pairs: bool) {
        let (pw, ph) = (self.w, self.h);
        draw_trace(&mut self.map[c], pw, ph, 0, c, m, view, rec_t0, dt, v, w, z, pairs);
    }
}

/// `Phosphor::draw_ex` on one band of one channel's map (`pw` x `ph`, row-major): `band` holds the rows `r_lo` ..
/// `r_lo + band.len() / pw` (the whole map: `r_lo` 0).
#[allow(clippy::too_many_arguments)]
pub fn draw_trace(band: &mut [u16], pw: usize, ph: usize, r_lo: usize, c: usize, m: &Model, view: (f32, f32), rec_t0: f64, dt: f64, v: &[f32], w: u16, z: Option<&[f32]>, pairs: bool) {
    let (b0, b1) = (r_lo as i32, (r_lo + band.len() / pw.max(1)).min(ph) as i32);
    let wi = pw as i32;
    trace_columns(pw, ph, c, m, view, rec_t0, dt, v, w, z, pairs, (0, wi), |cx, r0, r1, w| {
        for r in r0.max(b0)..=r1.min(b1 - 1) {
            let idx = ((r - b0) * wi + cx) as usize;
            band[idx] = band[idx].saturating_add(w);
        }
    });
}

/// The pixels of a trace on a `pw` x `ph` map, column by column: `put(column, first row, last row, weight)` for every
/// vertical span (rows within the map, first <= last), only for the columns `cols.0 .. cols.1`. Line segments from
/// point to point, or one pixel a point with `Model::dots`; `z` scales the weight per point (Z input). With `pairs`
/// (`v` holds min/max pairs) and dots, each pair is one span from its minimum to its maximum over the columns it
/// covers: two dots would lose the edges of a square at slow time bases.
#[allow(clippy::too_many_arguments)]
pub fn trace_columns(pw: usize, ph: usize, c: usize, m: &Model, view: (f32, f32), rec_t0: f64, dt: f64, v: &[f32], w: u16, z: Option<&[f32]>, pairs: bool, cols: (i32, i32), mut put: impl FnMut(i32, i32, i32, u16)) {
    let (wd, ht) = (pw as f32, ph as f32);
    let tdiv = view.0 as f64;
    let vdelay = view.1 as f64;
    let ppd_x = wd as f64 / HDIV as f64;
    let ch = &m.ch[c];
    let vpd = ch.volts_per_div();
    let ppd_y = ht / VDIV;
    let cy = ht / 2.0;
    let x_of = |k: usize| -> f32 { (((rec_t0 + k as f64 * dt - vdelay) / tdiv) * ppd_x + wd as f64 / 2.0) as f32 };
    let y_of = |val: f32| -> f32 { cy - (val + ch.offset) / vpd * ppd_y };
    let hi = ph as i32;
    let (c_lo, c_hi) = (cols.0.max(0), cols.1.min(pw as i32));
    let w0 = w;
    let wt = |k: usize| match z {
        Some(z) => (w0 as f32 * z.get(k).copied().unwrap_or(0.0).clamp(0.0, 1.0)).round() as u16,
        None => w0,
    };
    if m.dots && pairs {
        for (j, p) in v.chunks(2).enumerate() {
            let (lo, hi_) = p.iter().copied().filter(|y| y.is_finite()).fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), y| (a.min(y), b.max(y)));
            if lo > hi_ {
                continue;
            }
            // the pair covers points 2j and 2j+1: every column it touches, from the minimum to the maximum
            let k = 2 * j;
            let (x0, x1) = (x_of(k).floor() as i32, x_of(k + p.len() - 1).floor() as i32);
            let (ya, yb) = (y_of(lo), y_of(hi_));
            let r0 = (ya.min(yb).round() as i32).max(0);
            let r1 = (ya.max(yb).round() as i32).min(hi - 1);
            if r0 <= r1 {
                for x in x0.max(c_lo)..=x1.min(c_hi - 1) {
                    put(x, r0, r1, wt(k));
                }
            }
        }
        return;
    }
    let mut prev: Option<(f32, f32)> = None;
    for (k, &val) in v.iter().enumerate() {
        if !val.is_finite() {
            prev = None;
            continue;
        }
        let w = match z {
            Some(z) => (w0 as f32 * z.get(k).copied().unwrap_or(0.0).clamp(0.0, 1.0)).round() as u16,
            None => w0,
        };
        let p = (x_of(k), y_of(val));
        if m.dots {
            let (x, y) = (p.0.floor() as i32, p.1.round() as i32);
            if (c_lo..c_hi).contains(&x) && (0..hi).contains(&y) {
                put(x, y, y, w);
            }
            continue;
        }
        if let Some(q) = prev {
            // rasterize segment q -> p as vertical spans per column
            let (x0, y0, x1, y1) = if q.0 <= p.0 { (q.0, q.1, p.0, p.1) } else { (p.0, p.1, q.0, q.1) };
            let cx0 = x0.floor() as i32;
            let cx1 = x1.floor() as i32;
            if cx1 >= c_lo && cx0 < c_hi {
                for cx in cx0.max(c_lo)..=cx1.min(c_hi - 1) {
                    let (ya, yb) = if cx0 == cx1 {
                        (y0, y1)
                    } else {
                        let ta = ((cx as f32 - x0) / (x1 - x0)).clamp(0.0, 1.0);
                        let tb = ((cx as f32 + 1.0 - x0) / (x1 - x0)).clamp(0.0, 1.0);
                        (y0 + (y1 - y0) * ta, y0 + (y1 - y0) * tb)
                    };
                    let (lo, hi_) = if ya < yb { (ya, yb) } else { (yb, ya) };
                    let r0 = (lo.round() as i32).max(0);
                    let r1 = (hi_.round() as i32).min(hi - 1);
                    if r0 <= r1 {
                        put(cx, r0, r1, w);
                    }
                }
            }
        }
        prev = Some(p);
    }
}

#[derive(Clone, Copy, Default)]
#[allow(dead_code)] // the measurements come from oos-dsp; these serve auto set-up and level 50 %
pub struct Stats {
    pub vmax: f32,
    pub vmin: f32,
    pub vavg: f32,
    pub vrms: f32,
    pub top: f32,
    pub base: f32,
    pub period: f64,
    pub pwidth: f64,
    pub rise: f64,
    pub fall: f64,
    pub valid: bool,
}

pub fn stats(v: &[f32], dt: f64) -> Stats {
    // the finite samples: the record itself when it has no gap
    let own: Vec<f32>;
    let f: &[f32] = if v.iter().all(|x| x.is_finite()) {
        v
    } else {
        own = v.iter().copied().filter(|x| x.is_finite()).collect();
        &own
    };
    if f.len() < 4 {
        return Stats::default();
    }
    let vmax = f.iter().copied().fold(f32::MIN, f32::max);
    let vmin = f.iter().copied().fold(f32::MAX, f32::min);
    let n = f.len() as f32;
    let vavg = f.iter().sum::<f32>() / n;
    let vrms = (f.iter().map(|x| x * x).sum::<f32>() / n).sqrt();
    // top/base: histogram modes of the upper and lower 40 %
    let span = (vmax - vmin).max(1e-9);
    let mut hist = [0u32; 256];
    for &x in f {
        hist[(((x - vmin) / span) * 255.0) as usize] += 1;
    }
    let mode = |a: usize, b: usize| -> usize { (a..b).max_by_key(|&i| hist[i]).unwrap_or(a) };
    let lo = mode(0, 102);
    let hi = mode(154, 256);
    let mut base = vmin + span * lo as f32 / 255.0;
    let mut top = vmin + span * hi as f32 / 255.0;
    if hist[lo] < (f.len() / 20) as u32 || hist[hi] < (f.len() / 20) as u32 {
        base = vmin; // no flat levels (sine, triangle): use the extremes
        top = vmax;
    }
    let amp = top - base;
    let mut s = Stats { vmax, vmin, vavg, vrms, top, base, period: f64::NAN, pwidth: f64::NAN, rise: f64::NAN, fall: f64::NAN, valid: true };
    if amp <= 0.0 {
        return s;
    }
    let mid = base + amp * 0.5;
    let hy = amp * 0.1;
    // crossings with hysteresis at the mid level
    let mut rises = Vec::new();
    let mut falls = Vec::new();
    let mut state = v.iter().find(|x| x.is_finite()).map(|&x| x > mid).unwrap_or(false);
    for i in 1..v.len() {
        let (a, b) = (v[i - 1], v[i]);
        if !a.is_finite() || !b.is_finite() {
            continue;
        }
        if !state && b > mid + hy {
            state = true;
            // interpolate the mid crossing backwards
            let mut j = i;
            while j > 0 && v[j - 1].is_finite() && v[j - 1] > mid {
                j -= 1;
            }
            if j > 0 && v[j - 1].is_finite() {
                let (p, q) = (v[j - 1], v[j]);
                rises.push((j - 1) as f64 + ((mid - p) / (q - p)) as f64);
            }
        } else if state && b < mid - hy {
            state = false;
            let mut j = i;
            while j > 0 && v[j - 1].is_finite() && v[j - 1] < mid {
                j -= 1;
            }
            if j > 0 && v[j - 1].is_finite() {
                let (p, q) = (v[j - 1], v[j]);
                falls.push((j - 1) as f64 + ((p - mid) / (p - q)) as f64);
            }
        }
    }
    if rises.len() >= 2 {
        s.period = (rises[rises.len() - 1] - rises[0]) / (rises.len() - 1) as f64 * dt;
    } else if falls.len() >= 2 {
        s.period = (falls[falls.len() - 1] - falls[0]) / (falls.len() - 1) as f64 * dt;
    }
    if let Some(&r) = rises.first() {
        if let Some(&fl) = falls.iter().find(|&&x| x > r) {
            s.pwidth = (fl - r) * dt;
        }
    }
    // 10-90 % rise/fall around the first edges
    let l10 = base + amp * 0.1;
    let l90 = base + amp * 0.9;
    let cross = |from: usize, level: f32, up: bool, back: bool| -> Option<f64> {
        if back {
            let mut i = from;
            while i > 0 {
                let (a, b) = (v[i - 1], v[i]);
                if a.is_finite() && b.is_finite() && ((up && a < level && b >= level) || (!up && a > level && b <= level)) {
                    return Some((i - 1) as f64 + ((level - a) / (b - a)) as f64);
                }
                i -= 1;
            }
            None
        } else {
            for i in from + 1..v.len() {
                let (a, b) = (v[i - 1], v[i]);
                if a.is_finite() && b.is_finite() && ((up && a < level && b >= level) || (!up && a > level && b <= level)) {
                    return Some((i - 1) as f64 + ((level - a) / (b - a)) as f64);
                }
            }
            None
        }
    };
    if let Some(&r) = rises.first() {
        let ri = r as usize;
        if let (Some(a), Some(b)) = (cross(ri + 1, l10, true, true), cross(ri, l90, true, false)) {
            s.rise = (b - a) * dt;
        }
    }
    if let Some(&fl) = falls.first() {
        let fi = fl as usize;
        if let (Some(a), Some(b)) = (cross(fi + 1, l90, false, true), cross(fi, l10, false, false)) {
            s.fall = (b - a) * dt;
        }
    }
    s
}


/// Auto set-up: per channel find amplitude and frequency, choose V/div, offset, timebase and trigger.
/// Live channels only. A shown reference uses `autoset_shown`.
pub fn autoset(m: &mut Model, src: &Source) {
    // pass 1: which channels carry a signal, their levels and periods
    let mut found: Vec<(usize, Stats, f64)> = Vec::new();
    for c in (0..NCH).filter(|&c| m.ch[c].on) {
        let v = src.probe(c, 0.0137, 1e-6, 40000);
        let s = stats(&v, 1e-6);
        if s.vmax - s.vmin < 0.05 {
            continue;
        }
        let mut period = s.period;
        if period.is_finite() && period < 50e-6 {
            let v2 = src.probe(c, 0.0137, period / 200.0, 4000);
            let s2 = stats(&v2, period / 200.0);
            if s2.period.is_finite() {
                period = s2.period;
            }
        }
        found.push((c, s, period));
    }
    place(m, &found);
}

/// Auto set-up of stored records. Each entry is one shown reference: its samples and its own sample interval.
/// Vertical scale and position come from that record's levels, the same band rule as a live channel (the peak-to-peak
/// fills 80 % of its band, the midpoint sits on the band centre). The time base shows about three periods of the
/// slowest record. Analog channels, the trigger and the run state stay as they are.
/// `None` where the record is flatter than the live set-up's floor (50 mV).
#[cfg(test)]
pub fn autoset_shown(m: &mut Model, waves: &[(&[f32], f64)]) -> Vec<Option<(f32, f32)>> {
    let measured: Vec<Stats> = waves.iter().map(|(v, dt)| autoset_summary(v, *dt)).collect();
    let hot: Vec<bool> = measured.iter().map(|s| s.vmax - s.vmin >= 0.05).collect();
    let n = hot.iter().filter(|&&h| h).count();
    if n == 0 {
        return vec![None; waves.len()];
    }
    let band = VDIV / n as f32;
    let mut k = 0usize;
    let mut slowest = 0.0f64;
    let mut out = Vec::with_capacity(waves.len());
    for (s, &h) in measured.iter().zip(&hot) {
        if !h {
            out.push(None);
            continue;
        }
        let y_div = VDIV / 2.0 - band * (k as f32 + 0.5);
        let vpp = s.vmax - s.vmin;
        let want = vpp / (band * 0.8);
        let vpd = VSCALES.iter().copied().find(|&x| x >= want).unwrap_or(VSCALES[VSCALES.len() - 1]);
        let mid = (s.vmax + s.vmin) / 2.0;
        out.push(Some((vpd, y_div * vpd - mid)));
        if s.period.is_finite() && s.period > slowest {
            slowest = s.period;
        }
        k += 1;
    }
    if slowest > 0.0 {
        let ts = tscales();
        let want = (slowest * 3.0 / HDIV as f64) as f32;
        m.tscale = ts.iter().position(|&t| t >= want).unwrap_or(ts.len() - 1);
        m.tfine = 1.0;
        m.delay = 0.0;
    }
    out
}

/// Display setup is bounded independently of file size. Native contiguous windows
/// retain fast edges; a separate record-wide sample supplies slow-signal context.
/// This is an autoset estimate, never a replacement for measurement analysis.
pub(crate) fn autoset_summary(v: &[f32], dt: f64) -> Stats {
    const WINDOW: usize = 16_384;
    if v.len() <= WINDOW * 4 { return stats(v, dt); }
    if !dt.is_finite() || dt <= 0.0 { return Stats::default(); }
    let mut summary = Stats { vmin: f32::INFINITY, vmax: f32::NEG_INFINITY, period: f64::NAN, ..Stats::default() };
    let mut candidates = [(0.0f32, f64::NAN); 3];
    for (index, start) in [0, (v.len() - WINDOW) / 2, v.len() - WINDOW].into_iter().enumerate() {
        let window = stats(&v[start..start + WINDOW], dt);
        if !window.valid { continue; }
        summary.valid = true;
        summary.vmin = summary.vmin.min(window.vmin);
        summary.vmax = summary.vmax.max(window.vmax);
        candidates[index] = (window.vmax - window.vmin, window.period);
    }
    let stride = v.len().div_ceil(WINDOW);
    let coarse: Vec<f32> = v.iter().step_by(stride).copied().collect();
    let wide = stats(&coarse, dt * stride as f64);
    if wide.valid {
        summary.valid = true;
        summary.vmin = summary.vmin.min(wide.vmin);
        summary.vmax = summary.vmax.max(wide.vmax);
    }
    // Small fast noise on a slow waveform is not its period. A candidate must
    // carry a substantial part of the observed swing, as in hardware autoset.
    let swing = summary.vmax - summary.vmin;
    summary.period = candidates.into_iter().filter(|&(height, period)| height >= 0.4 * swing && period.is_finite() && period > 0.0)
        .map(|(_, period)| period).min_by(f64::total_cmp).unwrap_or(wide.period);
    summary
}

/// A steady period in `v`, sample distance `dt`: at least three rising edges with hysteresis, spaced within 30 %.
/// Neighbouring points are averaged first, the min/max pairs of a dense record become its middle. Noise and spikes
/// have no steady spacing.
pub fn periodic(v: &[f32], dt: f64) -> Option<f64> {
    let p: Vec<f32> = v.chunks_exact(2).map(|x| (x[0] + x[1]) / 2.0).collect();
    let (lo, hi) = (p.iter().copied().fold(f32::MAX, f32::min), p.iter().copied().fold(f32::MIN, f32::max));
    if !(hi > lo) {
        return None;
    }
    let (mid, h) = ((hi + lo) / 2.0, (hi - lo) * 0.2);
    let mut rises = Vec::new();
    let mut low = false;
    for (i, &x) in p.iter().enumerate() {
        if x < mid - h {
            low = true;
        } else if low && x > mid + h {
            low = false;
            rises.push(i);
        }
    }
    if rises.len() < 3 {
        return None;
    }
    let gaps: Vec<usize> = rises.windows(2).map(|w| w[1] - w[0]).collect();
    let (min, max) = (*gaps.iter().min()?, *gaps.iter().max()?);
    if (max as f64) > min.max(1) as f64 * 1.3 + 1.0 {
        return None;
    }
    Some((rises[rises.len() - 1] - rises[0]) as f64 / gaps.len() as f64 * 2.0 * dt)
}

/// The trigger level halfway through a signal: between top and base where it has flat levels (a spike or overshoot
/// moves the extremes, not these), else between the extremes. Used by level 50 % and the auto set-up.
pub fn mid_level(s: &Stats) -> f32 {
    if s.top > s.base {
        (s.top + s.base) / 2.0
    } else {
        (s.vmax + s.vmin) / 2.0
    }
}

/// Noise of a two-level signal (square, logic) as a standard deviation: from the median absolute deviation of each
/// half about its median (split halfway between the extremes), so the samples on the edges do not count. None when the
/// halves are not clearly apart (6 sigma), as for a sine or a triangle, whose halves spread by the signal itself.
/// `stats`' histogram top and base are no help here: noise of a fifth of the swing flattens their modes.
pub fn flat_noise(v: &[f32]) -> Option<f32> {
    let f: Vec<f32> = v.iter().copied().filter(|x| x.is_finite()).collect();
    let (lo, hi) = f.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    if f.len() < 32 || hi <= lo {
        return None;
    }
    let mid = (lo + hi) / 2.0;
    let median = |v: &mut Vec<f32>| {
        v.sort_by(|a, b| a.total_cmp(b));
        v[v.len() / 2]
    };
    let half = |up: bool| -> Option<(f32, f32)> {
        let mut h: Vec<f32> = f.iter().copied().filter(|&x| (x >= mid) == up).collect();
        if h.len() < 8 {
            return None;
        }
        let m = median(&mut h);
        let mut dev: Vec<f32> = h.iter().map(|x| (x - m).abs()).collect();
        Some((m, 1.4826 * median(&mut dev)))
    };
    let ((m1, s1), (m0, s0)) = (half(true)?, half(false)?);
    let sigma = s1.max(s0);
    (m1 - m0 > 6.0 * sigma).then_some(sigma)
}

/// The end of an auto set-up: `found` = channel, levels, period; the rest of the set-up stays.
pub fn place(m: &mut Model, found: &[(usize, Stats, f64)]) {
    // one horizontal band per channel (8 div / n), signal fills 80 % of its band; in a split plot per window: n and the
    // band count the channels of the window
    let nw = if m.zoom || m.view != AView::Scope { 1 } else { m.win_layout.count() };
    let win_of = |c: usize| m.ch_win[c].min(nw - 1);
    let wins: Vec<usize> = found.iter().map(|f| win_of(f.0)).collect();
    for (i, (c, s, _)) in found.iter().enumerate() {
        let n = wins.iter().filter(|&&w| w == wins[i]).count().max(1) as f32;
        let k = wins[..i].iter().filter(|&&w| w == wins[i]).count();
        let band = VDIV / n;
        let ch = &mut m.ch[*c];
        ch.on = true;
        ch.coupling = Coupling::Dc;
        let vpp = s.vmax - s.vmin;
        let want = vpp / (band * 0.8);
        ch.vscale = VSCALES.iter().position(|&x| x * ch.probe >= want).unwrap_or(VSCALES.len() - 1);
        ch.vfine = 1.0;
        let vpd = ch.volts_per_div();
        let y_div = VDIV / 2.0 - band * (k as f32 + 0.5);
        let mid = (s.vmax + s.vmin) / 2.0;
        ch.offset = y_div * vpd - mid;
    }
    if let Some((c, s, _)) = found.first() {
        m.trig_src = *c;
        m.trig_level = mid_level(s);
        m.slope = Slope::Rise;
        m.trig_mode = TrigMode::Auto;
        m.active = *c;
    }
    let slowest = found.iter().map(|f| f.2).filter(|p| p.is_finite()).fold(0.0, f64::max);
    if slowest > 0.0 {
        let ts = tscales();
        let want = (slowest * 3.0 / HDIV as f64) as f32;
        m.tscale = ts.iter().position(|&t| t >= want).unwrap_or(ts.len() - 1);
        m.tfine = 1.0;
    }
    m.delay = 0.0;
    m.run = RunState::Run;
}

#[cfg(test)]
mod tests {
    #[test]
    fn autoset_summary_handles_native_fast_edges_and_slow_signal_with_fast_noise() {
        let count=1_000_000;
        let fast:Vec<f32>=(0..count).map(|k|(k as f64*std::f64::consts::TAU/32.0).sin() as f32).collect();
        let summary=super::autoset_summary(&fast,1e-9);
        assert!((summary.period/32e-9-1.0).abs()<0.01);
        let slow:Vec<f32>=(0..count).map(|k|((k as f64*std::f64::consts::TAU/100_000.0).sin()+0.005*(k as f64*std::f64::consts::TAU/32.0).sin()) as f32).collect();
        let summary=super::autoset_summary(&slow,1e-9);
        assert!((summary.period/100e-6-1.0).abs()<0.02,"period {}",summary.period);
        assert!(summary.vmax-summary.vmin>1.9);
    }
    use super::*;

    // Break it catches: level 50 % on the extremes, which a spike or overshoot pulls off the middle of the swing
    #[test]
    fn mid_level_ignores_spikes_on_a_square_and_takes_the_extremes_of_a_sine() {
        let mut sq: Vec<f32> = (0..4000).map(|k| if (k / 500) % 2 == 0 { 0.0 } else { 5.0 }).collect();
        for k in [499, 1499, 2499] {
            sq[k + 1] = 9.0;
        }
        sq[1000] = -2.0;
        let s = stats(&sq, 1e-6);
        assert_eq!((s.vmax + s.vmin) / 2.0, 3.5);
        assert!((mid_level(&s) - 2.5).abs() < 0.1, "{}", mid_level(&s));
        let sine: Vec<f32> = (0..4000).map(|k| 1.0 + (k as f32 / 400.0 * std::f32::consts::TAU).sin()).collect();
        assert!((mid_level(&stats(&sine, 1e-6)) - 1.0).abs() < 0.01);
    }

    // Break it catches: noise reject chosen for a clean sine (its halves spread by the signal) or missed on a noisy square
    #[test]
    fn flat_noise_of_two_levels_only() {
        let mut x = 5u32;
        let mut r = || {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            (x >> 8) as f32 / (1 << 24) as f32 - 0.5
        };
        let sq: Vec<f32> = (0..4000).map(|k| if (k / 500) % 2 == 0 { 0.0 } else { 5.0 } + 1.2 * r()).collect();
        let s = flat_noise(&sq).unwrap();
        // uniform noise of sigma 0.35; the MAD estimate is exact for normal noise and reads 0.44 here
        assert!((0.3..0.5).contains(&s), "{}", s);
        let sine: Vec<f32> = (0..4000).map(|k| (k as f32 / 400.0 * std::f32::consts::TAU).sin()).collect();
        assert_eq!(flat_noise(&sine), None);
        assert_eq!(flat_noise(&[1.0; 100]), None);
    }

    #[test]
    fn shown_record_uses_its_own_interval_and_leaves_the_channels() {
        let sine: Vec<f32> = (0..2000).map(|k| (k as f32 / 200.0 * std::f32::consts::TAU).sin()).collect();
        let mut m = Model::default();
        let on = m.ch.each_ref().map(|c| c.on);
        let (trig, run) = ((m.trig_src, m.trig_level), m.run);
        let views = autoset_shown(&mut m, &[(&sine, 1e-7)]);
        let (vpd, off) = views[0].expect("level");
        assert!((vpd - 0.5).abs() < 1e-6, "v/div {}", vpd);
        assert!(off.abs() < 1e-3, "offset {}", off);
        assert!((m.time_per_div() - 10e-6).abs() < 1e-12, "s/div {}", m.time_per_div());
        assert_eq!(m.ch.each_ref().map(|c| c.on), on);
        assert_eq!((m.trig_src, m.trig_level), trig);
        assert_eq!(m.run, run);
        // the same samples a decade slower are a decade slower on screen
        let mut slow = Model::default();
        autoset_shown(&mut slow, &[(&sine, 1e-6)]);
        assert!((slow.time_per_div() - 100e-6).abs() < 1e-12, "s/div {}", slow.time_per_div());
        // two records share the screen; a flat one is not a band and does not move the time base
        let mut both = Model::default();
        let flat = vec![0.01f32; 2000];
        let views = autoset_shown(&mut both, &[(&sine, 1e-7), (&flat, 1e-7)]);
        assert!(views[1].is_none());
        assert!((views[0].unwrap().1).abs() < 1e-3);
        assert!((both.time_per_div() - 10e-6).abs() < 1e-12);
        let mut pair = Model::default();
        let views = autoset_shown(&mut pair, &[(&sine, 1e-7), (&sine, 1e-7)]);
        let (a, b) = (views[0].unwrap(), views[1].unwrap());
        assert!((a.0 - 1.0).abs() < 1e-6 && (b.0 - 1.0).abs() < 1e-6, "{:?} {:?}", a, b);
        assert!((a.1 - 2.0).abs() < 1e-3 && (b.1 + 2.0).abs() < 1e-3, "{:?} {:?}", a, b);
    }

    #[test]
    fn periodic_takes_a_sine_and_refuses_noise_and_dc() {
        let sine: Vec<f32> = (0..3600).map(|k| (k as f32 / 100.0 * std::f32::consts::TAU).sin()).collect();
        let p = periodic(&sine, 1e-6).unwrap();
        assert!((p - 100e-6).abs() < 3e-6, "{}", p);
        assert_eq!(periodic(&vec![0.5; 3600], 1e-6), None);
        // spikes at uneven distances, as on the generator cable
        let mut spikes = vec![0f32; 3600];
        for (k, i) in [40, 95, 700, 730, 2000, 3100].iter().enumerate() {
            spikes[*i] = 0.05 + k as f32 * 0.001;
            spikes[*i + 1] = 0.05;
        }
        assert_eq!(periodic(&spikes, 1e-6), None);
        // min/max pairs of a dense record around a slow sine
        let dense: Vec<f32> = (0..3600).map(|k| ((k / 2) as f32 / 300.0 * std::f32::consts::TAU).sin() + if k % 2 == 0 { -0.1 } else { 0.1 }).collect();
        assert!((periodic(&dense, 1e-6).unwrap() - 600e-6).abs() < 10e-6);
    }

    /// Record value at time t (relative to the trigger).
    fn at(r: &Record, c: usize, t: f64) -> f32 {
        r.ch[c][((t - r.t0) / r.dt) as usize]
    }

    fn acquire_with(f: impl Fn(&mut Model)) -> Record {
        let mut m = Model::default();
        f(&mut m);
        let mut src = Source::Demo { t: 0.0 };
        let mut rng = Rng::new(1);
        for _ in 0..50 {
            if let Some(r) = src.acquire(&m, 0.0123, &mut rng) {
                if r.triggered {
                    return r;
                }
            }
        }
        panic!("never triggered");
    }

    #[test]
    fn runt_trigger_on_the_demo() {
        let r = acquire_with(|m| {
            m.trig_type = TrigType::Runt;
            (m.trig_src, m.trig_pol, m.trig_when, m.trig_level, m.trig_level2) = (0, Pol::Pos, WhenKind::Off, 2.0, 0.5);
        });
        // the runt (1.1 V) ends at the trigger point
        let before = at(&r, 0, -0.1e-3);
        let after = at(&r, 0, 0.1e-3);
        assert!(before > 0.8 && before < 1.4, "before {}", before);
        assert!(after.abs() < 0.2, "after {}", after);
    }

    #[test]
    fn timeout_trigger_on_the_demo() {
        let r = acquire_with(|m| {
            m.trig_type = TrigType::Timeout;
            (m.trig_src, m.slope, m.trig_t1, m.trig_level) = (0, Slope::Rise, 1.2e-3, 1.5);
        });
        // 1.2 ms after the last rising edge: the normal pulse fell at -0.7 ms, the runt starts at -0.2 ms
        assert!(at(&r, 0, -0.9e-3) > 2.0);
        let mid = at(&r, 0, 0.0);
        assert!(mid > 0.8 && mid < 1.4, "{}", mid);
    }
}
