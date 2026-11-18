// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The settings an acquisition sequence is built from: channels (`Chan`, `Couple`), the hardware trigger type
//! (`Trig`), the whole set-up (`Settings`, its default the warm hand-over) and the optional FPGA units (`HwMask`,
//! `HwZone`, `Segments`). A new setting goes here as a field with its default; the steps it causes go into `State` in
//! seq.rs. What the registers do with the values is described in docs/protocols/.

use crate::dho::{acq, afe, auxout, la, mask, tpu, zone};

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
    Pulse {
        pol: tpu::Polarity,
        when: tpu::When,
        lower: f64,
        upper: f64,
    },
    Slope {
        when: tpu::When,
        lower: f64,
        upper: f64,
        lo: f64,
    },
    Runt {
        pol: tpu::Polarity,
        when: tpu::When,
        lower: f64,
        upper: f64,
        lo: f64,
    },
    Window {
        event: tpu::WindowEvent,
        time: f64,
        lo: f64,
    },
    Timeout {
        time: f64,
    },
    Delay {
        srcb: u32,
        eb: tpu::Edge,
        when: tpu::When,
        lower: f64,
        upper: f64,
        levelb: f64,
    },
    /// `pats` CH1..CH4, `la` D0..D15 (TPU pattern slots 4..19, trigger.md 10.8; all X without the LA)
    Pattern {
        pats: [tpu::Pat; 4],
        la: [tpu::Pat; 16],
        levels: [f64; 4],
    },
    Duration {
        pats: [tpu::Pat; 4],
        la: [tpu::Pat; 16],
        when: tpu::When,
        lower: f64,
        upper: f64,
        levels: [f64; 4],
    },
    SetupHold {
        clk: u32,
        clk_edge: tpu::Edge,
        setup: f64,
        hold: f64,
        levelc: f64,
    },
    Nth {
        count: u32,
        idle: f64,
    },
    Video {
        std: tpu::VideoStd,
        sync: tpu::VideoSync,
        line: u32,
    },
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
    /// hardware segmented recording: one arm records this many frames
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
        let c = Chan {
            scale: 1.0,
            offset: 0.0,
            bw: afe::bw::OFF,
            probe: 1.0,
            bias: 0.0,
            skew: 0.0,
            couple: Couple::Dc,
            invert: false,
        };
        Settings {
            delay: 0.0,
            mode: acq::Mode::Normal,
            holdoff: HOLDOFF_MIN,
            mask: 1,
            tdiv: 200e-6,
            depth: 0,
            ch: [c; 4],
            afe: false,
            trig_src: 1,
            slope: tpu::Slope::Rising,
            level: 1.5,
            auto: true,
            auto_fast: false,
            single: false,
            trigger: Trig::Edge,
            window: Some((4000, -2000)),
            play4: true,
            blocks: 7,
            header: false,
            noise_reject: false,
            trig_couple: tpu::Coupling::Dc,
            frames: 1,
            compress: 1,
            la: None,
            segments: None,
            aux: auxout::AuxOut::TriggerOut,
            hwmask: None,
            hwzone: None,
        }
    }
}
