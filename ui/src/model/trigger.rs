// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The trigger settings: types, coupling, slopes, the serial trigger and its conditions, the aux output, and the
//! methods that turn the model into the trigger the engine runs and tell which channels and LA lines it reads. A new
//! trigger type or condition goes here.

use super::*;

/// Coupling of the edge trigger: a filter in the trigger path only, the trace stays as the channel's coupling shows it
/// (trigger.md 2.5).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrigCoup {
    Dc,
    Ac,
    LfReject,
    HfReject,
}

pub const TRIG_COUPS: [TrigCoup; 4] = [TrigCoup::Dc, TrigCoup::Ac, TrigCoup::LfReject, TrigCoup::HfReject];

/// What the rear AUX OUT BNC carries (System > AUX out, `:SYSTem:AOUTput`, docs/fork/auxout.md).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxOut {
    /// a pulse per trigger: the FPGA's power-up state, so the default
    TrigOut,
    /// constant low
    Off,
    /// a pulse per failed acquisition of the hardware mask test (`mask.hw = on`, docs/fork/hwmask.md); without it the
    /// output stays low
    PassFail,
}

pub const AUX_OUTS: [AuxOut; 3] = [AuxOut::TrigOut, AuxOut::Off, AuxOut::PassFail];

impl AuxOut {
    /// what the menu and SCPI take: pass/fail only where the mask test or the zone trigger can run in hardware
    /// (`Board::mask_hw`, `Board::zone_hw`); a setup file takes every value
    pub fn offered(self, hw_mask: bool) -> bool {
        self != AuxOut::PassFail || hw_mask
    }

    /// the offered choices in menu order
    pub fn choices(hw_mask: bool) -> Vec<AuxOut> {
        AUX_OUTS.iter().copied().filter(|a| a.offered(hw_mask)).collect()
    }

    pub fn name(self) -> &'static str {
        match self {
            AuxOut::TrigOut => "Trigger out",
            AuxOut::Off => "Off",
            AuxOut::PassFail => "Pass/fail",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slope {
    Rise,
    Fall,
    Either,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrigMode {
    Auto,
    Normal,
    Single,
}

/// Trigger types (the software trigger engine of oos-dsp; on the unit later the TPU, docs/protocols/trigger.md).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrigType {
    Edge,
    Pulse,
    Slope,
    Video,
    Pattern,
    Duration,
    Timeout,
    Runt,
    Window,
    Delay,
    SetupHold,
    NthEdge,
    /// protocol trigger: the decoder of the Decode menu finds the event (BusWhat, value)
    Bus,
}

pub const TRIG_TYPES: [TrigType; 13] = [
    TrigType::Edge,
    TrigType::Pulse,
    TrigType::Slope,
    TrigType::Video,
    TrigType::Pattern,
    TrigType::Duration,
    TrigType::Timeout,
    TrigType::Runt,
    TrigType::Window,
    TrigType::Delay,
    TrigType::SetupHold,
    TrigType::NthEdge,
    TrigType::Bus,
];

/// What the protocol trigger looks for in the decoded packets (`trig::find_all`), as oos-dsp matches it.
pub use oos_dsp::protocols::BusWhat;
pub const BUS_WHATS: [BusWhat; 4] = [BusWhat::Frame, BusWhat::Id, BusWhat::Data, BusWhat::Error];

/// How a protocol value compares (`SerialTrig`): with the value (`trig_bus_val`) as min, `max` as the upper limit, all
/// after the mask. The relations of the TPU's serial receivers (docs/fork/serial-ui.md).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rel {
    Eq,
    Ne,
    /// greater than the value
    Gt,
    /// less than max
    Lt,
    /// value <= x <= max
    In,
    /// x < value or x > max
    Out,
}
pub const RELS: [Rel; 6] = [Rel::Eq, Rel::Ne, Rel::Gt, Rel::Lt, Rel::In, Rel::Out];
impl Rel {
    pub fn name(self) -> &'static str {
        match self {
            Rel::Eq => "=",
            Rel::Ne => "\u{2260}",
            Rel::Gt => ">",
            Rel::Lt => "<",
            Rel::In => "In range",
            Rel::Out => "Out of range",
        }
    }

    /// The relation needs the upper limit.
    pub fn uses_max(self) -> bool {
        matches!(self, Rel::Lt | Rel::In | Rel::Out)
    }

    /// `x` against `min`/`max` after `mask`.
    pub fn test(self, x: u64, min: u64, max: u64, mask: u64) -> bool {
        let (x, a, b) = (x & mask, min & mask, max & mask);
        match self {
            Rel::Eq => x == a,
            Rel::Ne => x != a,
            Rel::Gt => x > a,
            Rel::Lt => x < b,
            Rel::In => a <= x && x <= b,
            Rel::Out => x < a || x > b,
        }
    }
}

/// UART stop bits of the trigger.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StopBits {
    One,
    OneHalf,
    Two,
}
pub const STOP_BITS: [StopBits; 3] = [StopBits::One, StopBits::OneHalf, StopBits::Two];

/// Which UART error the Error condition looks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UartErr {
    Either,
    Parity,
    Framing,
}
pub const UART_ERRS: [UartErr; 3] = [UartErr::Either, UartErr::Parity, UartErr::Framing];

/// I2C transfer direction of the address condition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum I2cDir {
    Either,
    Write,
    Read,
}
pub const I2C_DIRS: [I2cDir; 3] = [I2cDir::Either, I2cDir::Write, I2cDir::Read];

/// The protocol trigger's settings beyond condition and value (Trigger > Protocol, `:TRIGger:RS232|SPI|IIC:*`); the
/// TPU's receivers take them all (`trig.serial_hw = on`), the software trigger as far as the decoders show it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SerialTrig {
    pub rel: Rel,
    /// upper limit of `rel` (Lt, In, Out)
    pub max: u64,
    /// bits of the value that count (SPI, I2C data)
    pub mask: u64,
    /// UART: idle low (the line inverted)
    pub uart_inv: bool,
    pub uart_stop: StopBits,
    pub uart_err: UartErr,
    /// SPI: chip select active high
    pub spi_cs_high: bool,
    /// SPI without chip select: a clock pause longer than this starts a frame, s (0 = the decoder's default)
    pub spi_timeout: f32,
    /// I2C address condition: relation and upper limit, direction, 10-bit addresses
    pub addr_rel: Rel,
    pub addr_max: u32,
    pub dir: I2cDir,
    pub ten_bit: bool,
    /// I2C data condition: 0 = any data byte of a frame (the software trigger), 1..5 = the first `bytes` data bytes
    /// after the address, the first the most significant (also the TPU's receiver)
    pub bytes: u8,
}

impl Default for SerialTrig {
    fn default() -> SerialTrig {
        SerialTrig {
            rel: Rel::Eq,
            max: 0xff,
            mask: u64::MAX,
            uart_inv: false,
            uart_stop: StopBits::One,
            uart_err: UartErr::Either,
            spi_cs_high: false,
            spi_timeout: 0.0,
            addr_rel: Rel::Eq,
            addr_max: 0x7f,
            dir: I2cDir::Either,
            ten_bit: false,
            bytes: 0,
        }
    }
}
/// The name of what the protocol trigger looks for, in the menu and in setups.
pub fn bus_what_name(w: BusWhat) -> &'static str {
    match w {
        BusWhat::Frame => "Frame",
        BusWhat::Id => "ID/Addr",
        BusWhat::Data => "Data",
        BusWhat::Error => "Error",
    }
}

impl TrigType {
    pub fn name(self) -> &'static str {
        match self {
            TrigType::Edge => "Edge",
            TrigType::Pulse => "Pulse",
            TrigType::Slope => "Slope",
            TrigType::Video => "Video",
            TrigType::Pattern => "Pattern",
            TrigType::Duration => "Duration",
            TrigType::Timeout => "Timeout",
            TrigType::Runt => "Runt",
            TrigType::Window => "Window",
            TrigType::Delay => "Delay",
            TrigType::SetupHold => "Setup/Hold",
            TrigType::NthEdge => "Nth edge",
            TrigType::Bus => "Bus",
        }
    }

    /// The types with an upper (`trig_level`) and a lower level (`trig_level2`).
    pub fn two_level(self) -> bool {
        matches!(self, TrigType::Slope | TrigType::Runt | TrigType::Window)
    }
}

/// Time condition kind of the "When" setting (Off only for runt).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WhenKind {
    Off,
    Gt,
    Lt,
    In,
    Out,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinMode {
    Enter,
    Exit,
    Inside,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VSync {
    AllLines,
    Line,
    Odd,
    Even,
    AllFields,
}

/// 1-2-5 ladder for trigger times: 1 ns .. 10 s.
pub fn time_ladder() -> Vec<f32> {
    decades(-9, 2, false).into_iter().filter(|&x| x <= 10.0).map(|x| x as f32).collect()
}

impl Model {
    fn dsp_slope(s: Slope) -> oos_dsp::trigger::Slope {
        match s {
            Slope::Rise => oos_dsp::trigger::Slope::Rising,
            Slope::Fall => oos_dsp::trigger::Slope::Falling,
            Slope::Either => oos_dsp::trigger::Slope::Either,
        }
    }

    /// The time condition of the "When" setting (None = Off).
    pub fn when(&self) -> Option<oos_dsp::trigger::When> {
        use oos_dsp::trigger::When;
        let (a, b) = (self.trig_t1 as f64, self.trig_t2 as f64);
        match self.trig_when {
            WhenKind::Off => None,
            WhenKind::Gt => Some(When::Gt(a)),
            WhenKind::Lt => Some(When::Lt(a)),
            WhenKind::In => Some(When::In(a.min(b), a.max(b))),
            WhenKind::Out => Some(When::Out(a.min(b), a.max(b))),
        }
    }

    pub fn trigger_wave(&self) -> WaveId {
        self.trig_ref.map_or(WaveId::Ch(self.trig_src), WaveId::Ref)
    }

    pub fn trigger_wave2(&self) -> WaveId {
        self.trig_ref2.map_or(WaveId::Ch(self.trig_src2), WaveId::Ref)
    }

    pub fn set_trigger_wave(&mut self, source: WaveId, secondary: bool) {
        let (channel, reference) = if secondary {
            (&mut self.trig_src2, &mut self.trig_ref2)
        } else {
            (&mut self.trig_src, &mut self.trig_ref)
        };
        match source {
            WaveId::Ch(c) => {
                *channel = c.min(NCH - 1);
                *reference = None;
            }
            WaveId::Ref(i) => {
                *reference = Some(i.min(NREF - 1));
            }
        }
        if !secondary {
            self.trig_la = None;
        }
    }

    /// Keeps the lower level of a two-level type at or below the upper one. The lower level gives way: the upper is
    /// the level every type shares, the lower one is also Delay's level B and Setup/Hold's clock level.
    pub fn order_trigger_levels(&mut self) {
        if self.trig_type.two_level() && self.trig_level2 > self.trig_level {
            self.trig_level2 = self.trig_level;
        }
    }

    /// The configured trigger for the software trigger engine.
    pub fn trigger(&self) -> oos_dsp::trigger::Trigger {
        use oos_dsp::trigger::{Trigger, VideoSync, WinWhen};
        let (src, lv, lv2) = (self.trig_src, self.trig_level, self.trig_level2);
        let (hi, lo) = (lv.max(lv2), lv.min(lv2));
        let when = self.when().unwrap_or(oos_dsp::trigger::When::Gt(0.0));
        let sl = Self::dsp_slope(self.slope);
        match self.trig_type {
            TrigType::Edge => Trigger::Edge { src, slope: sl, level: lv },
            // the protocol trigger is evaluated by trig::find_all; this edge only stands for it elsewhere
            TrigType::Bus => {
                Trigger::Edge { src: self.dec_src, slope: oos_dsp::trigger::Slope::Falling, level: self.dec_thr }
            }
            TrigType::Pulse => Trigger::Pulse { src, pol: self.trig_pol, level: lv, when },
            TrigType::Slope => Trigger::Slope { src, slope: sl, lo, hi, when },
            TrigType::Video => Trigger::Video {
                src,
                std: self.video_std,
                level: lv,
                sync: match self.video_sync {
                    VSync::AllLines => VideoSync::AllLines,
                    VSync::Line => VideoSync::Line(self.video_line),
                    VSync::Odd => VideoSync::OddField,
                    VSync::Even => VideoSync::EvenField,
                    VSync::AllFields => VideoSync::AllFields,
                },
            },
            TrigType::Pattern => Trigger::Pattern { bits: self.pat, levels: self.pat_levels },
            TrigType::Duration => Trigger::Duration {
                bits: self.pat.map(|b| if matches!(b, Bit::R | Bit::F) { Bit::X } else { b }),
                levels: self.pat_levels,
                when,
            },
            TrigType::Timeout => Trigger::Timeout { src, slope: sl, level: lv, time: self.trig_t1 as f64 },
            TrigType::Runt => Trigger::Runt { src, pol: self.trig_pol, lo, hi, when: self.when() },
            TrigType::Window => Trigger::Window {
                src,
                lo,
                hi,
                when: match self.win_mode {
                    WinMode::Enter => WinWhen::Enter,
                    WinMode::Exit => WinWhen::Exit,
                    WinMode::Inside => WinWhen::Inside(when),
                },
            },
            TrigType::Delay => Trigger::Delay {
                a: src,
                slope_a: sl,
                level_a: lv,
                b: self.trig_src2,
                slope_b: Self::dsp_slope(self.trig_slope2),
                level_b: lv2,
                when,
            },
            TrigType::SetupHold => Trigger::SetupHold {
                data: src,
                clk: self.trig_src2,
                clk_slope: Self::dsp_slope(self.trig_slope2),
                data_level: lv,
                clk_level: lv2,
                setup: self.setup as f64,
                hold: self.hold as f64,
            },
            TrigType::NthEdge => {
                Trigger::NthEdge { src, slope: sl, level: lv, idle: self.nth_idle as f64, n: self.nth_n }
            }
        }
    }

    /// Channels the trigger reads.
    pub fn trig_channels(&self) -> [bool; NCH] {
        let mut u = [false; NCH];
        if self.trig_line().is_some() || self.bus_on_la() {
            return u;
        }
        match self.trig_type {
            TrigType::Pattern | TrigType::Duration => {
                for c in 0..NCH {
                    u[c] = self.pat[c] != Bit::X;
                }
            }
            TrigType::Delay | TrigType::SetupHold => {
                u[self.trig_src] = true;
                u[self.trig_src2] = true;
            }
            TrigType::Bus => {
                let r = self.dec_proto.roles();
                if let Some(v) = u.get_mut(self.dec_src) {
                    *v = true;
                }
                if r[1].is_some() {
                    if let Some(v) = u.get_mut(self.dec_src2) {
                        *v = true;
                    }
                }
                if let (Some(_), Some(c)) = (r[2], self.dec_cs) {
                    if let Some(v) = u.get_mut(c) {
                        *v = true;
                    }
                }
                if let (Some(_), Some(c)) = (r[3], self.dec_src4) {
                    if let Some(v) = u.get_mut(c) {
                        *v = true;
                    }
                }
                if let (true, Some(c)) = (self.dec_proto.diff_ok(), self.dec_diff) {
                    if let Some(v) = u.get_mut(c) {
                        *v = true;
                    }
                }
            }
            _ => u[self.trig_src] = true,
        }
        u
    }

    /// Time scale of the trigger condition (s): the search resolution must resolve it.
    pub fn trig_feature(&self) -> f64 {
        let t = |x: f32| x as f64;
        match self.trig_type {
            TrigType::Edge | TrigType::Pattern => f64::INFINITY,
            TrigType::Bus => self.bus_bit() / 20.0,
            TrigType::Video => 0.25e-6,
            TrigType::SetupHold => t(self.setup.min(self.hold)) / 4.0,
            TrigType::Timeout => t(self.trig_t1) / 20.0,
            TrigType::NthEdge => t(self.nth_idle) / 20.0,
            TrigType::Runt if self.trig_when == WhenKind::Off => f64::INFINITY,
            TrigType::Window if self.win_mode != WinMode::Inside => f64::INFINITY,
            _ => {
                t(self.trig_t1.min(if matches!(self.trig_when, WhenKind::In | WhenKind::Out) {
                    self.trig_t2
                } else {
                    f32::MAX
                })) / 20.0
            }
        }
    }

    /// Record length the trigger needs to decide (s).
    pub fn trig_span(&self) -> f64 {
        let t = |x: f32| x as f64;
        match self.trig_type {
            TrigType::Video => match self.video_sync {
                VSync::AllLines => 4.0 * self.video_std.line(),
                _ => 1.1 * self.video_std.lines() as f64 * self.video_std.line(),
            },
            TrigType::Timeout | TrigType::Duration => 3.0 * t(self.trig_t1.max(self.trig_t2)) + 6e-3,
            TrigType::NthEdge => 4.0 * t(self.nth_idle) + 2e-3,
            // room for a whole frame of the protocol before the trigger point (a module may say: USB auto, low-speed packets)
            TrigType::Bus => self.bus_frame_span(),
            TrigType::Edge | TrigType::Pattern => 0.0,
            _ => 3.0 * t(self.trig_t1.max(self.trig_t2)) + 3e-3,
        }
    }

    /// The trigger coupling in effect: the set one for the edge trigger on a channel, DC for every other type and
    /// for an LA line.
    pub fn trig_coupling(&self) -> TrigCoup {
        if self.trig_type == TrigType::Edge && self.trig_line().is_none() {
            self.trig_coup
        } else {
            TrigCoup::Dc
        }
    }

    /// The trigger types that can use an LA line (the stock system also has them for D0..D15; trigger.md 5).
    pub fn trig_la_ok(&self) -> bool {
        self.la_on() && matches!(self.trig_type, TrigType::Edge | TrigType::Pulse)
    }
    /// The pattern or duration trigger looks at LA lines.
    pub fn pat_on_la(&self) -> bool {
        self.la_on()
            && matches!(self.trig_type, TrigType::Pattern | TrigType::Duration)
            && self.pat_la.iter().any(|&b| b != Bit::X)
    }
    /// The protocol trigger on LA lines (the decoder's input is the LA).
    pub fn bus_on_la(&self) -> bool {
        self.trig_type == TrigType::Bus && self.dec_on_la()
    }
    /// The hardware trigger cannot be refined on the analog samples (it is on LA lines).
    pub fn trig_on_la(&self) -> bool {
        self.trig_line().is_some() || self.pat_on_la() || self.bus_on_la()
    }
    /// The LA line the trigger is on: only while the LA is on and with a type that can use one.
    pub fn trig_line(&self) -> Option<u8> {
        self.trig_la.filter(|_| self.trig_la_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::testkit::{document, exempt_once, names, q, scpi_session, section};

    /// Every trigger type with its words: the one a setup stores (`trig_type = <word>`, the variant's name), the one
    /// `:TRIGger:MODE` takes and the one its query answers. A word that was released never changes: setups and
    /// scripts hold it. A new type gets its row here; `row` does not compile without it.
    const TYPES: [(TrigType, &str, &str, &str); 13] = [
        (TrigType::Edge, "Edge", "EDGE", "EDGE"),
        (TrigType::Pulse, "Pulse", "PULSE", "PULS"),
        (TrigType::Slope, "Slope", "SLOPE", "SLOP"),
        (TrigType::Video, "Video", "VIDEO", "VID"),
        (TrigType::Pattern, "Pattern", "PATTERN", "PATT"),
        (TrigType::Duration, "Duration", "DURATION", "DUR"),
        (TrigType::Timeout, "Timeout", "TIMEOUT", "TIM"),
        (TrigType::Runt, "Runt", "RUNT", "RUNT"),
        (TrigType::Window, "Window", "WINDOW", "WIND"),
        (TrigType::Delay, "Delay", "DELAY", "DEL"),
        (TrigType::SetupHold, "SetupHold", "SETUPHOLD", "SET"),
        (TrigType::NthEdge, "NthEdge", "NTHEDGE", "NEDG"),
        (TrigType::Bus, "Bus", "BUS", "BUS"),
    ];

    /// The row of `t` in `TYPES`.
    fn row(t: TrigType) -> usize {
        match t {
            TrigType::Edge => 0,
            TrigType::Pulse => 1,
            TrigType::Slope => 2,
            TrigType::Video => 3,
            TrigType::Pattern => 4,
            TrigType::Duration => 5,
            TrigType::Timeout => 6,
            TrigType::Runt => 7,
            TrigType::Window => 8,
            TrigType::Delay => 9,
            TrigType::SetupHold => 10,
            TrigType::NthEdge => 11,
            TrigType::Bus => 12,
        }
    }

    /// The types the pictures of the comparison (ui/tests/triggers.txt) do not show, each with the reason. A type in
    /// this list that the script shows fails the test, so the list only gets shorter.
    const NOT_SHOT: [(TrigType, &str); 1] = [(
        TrigType::Bus,
        "the script walks the types of the hardware trigger; the protocol trigger has no picture yet",
    )];

    /// What a trigger type needs besides its variant, each failure with its name and the part that is missing: its
    /// place in `TRIG_TYPES` (the menu's list), a name, its stored word and its SCPI words as `TYPES` has them, the
    /// lines of docs/scpi.md and of the wiki that name them, and its pictures in the comparison.
    #[test]
    fn every_trigger_type_is_complete() {
        let (scpi_md, wiki_scpi) = (document("docs/scpi.md"), document("wiki/SCPI-and-VXI-11.md"));
        let words = scpi_md.lines().find(|l| l.starts_with("Trigger type names are")).expect("the line of the words");
        let (long_words, short_words) = words.split_once("MODE replies").expect("the long and the short words");
        let wiki_row = wiki_scpi.lines().find(|l| l.starts_with("| `:TRIGger:MODE[?]`")).expect("the wiki's row");
        let features = section(&document("wiki/Features.md"), "**Trigger**");
        let shots = include_str!("../../tests/triggers.txt");
        assert_eq!(TRIG_TYPES.len(), TYPES.len(), "TRIG_TYPES and TYPES list the same types");
        exempt_once("NOT_SHOT", &NOT_SHOT.map(|x| x.0), &TRIG_TYPES);
        let mut seen = std::collections::HashSet::new();
        for (k, (t, stored, long, short)) in TYPES.into_iter().enumerate() {
            assert_eq!(row(t), k, "{:?}: its row in TYPES", t);
            assert!(TRIG_TYPES.contains(&t), "{:?}: not in TRIG_TYPES, so in no menu", t);
            assert!(!t.name().is_empty() && seen.insert(t.name()), "{:?}: a name of its own", t);
            // the setup file
            assert_eq!(format!("{:?}", t), stored, "{:?}: the word a setup stores never changes", t);
            let m = Model { trig_type: t, ..Default::default() };
            let text = crate::store::setup_text(&m);
            assert!(text.lines().any(|l| l == format!("trig_type = {}", stored)), "{:?}: its setup line", t);
            assert_eq!(crate::store::parse_setup(&text).map(|m| m.trig_type), Ok(t), "{:?}: a setup reads it back", t);
            // the remote commands
            for word in [long, short] {
                let (mut sc, mut ui, cv) = scpi_session();
                assert_eq!(q(&mut sc, &mut ui, &cv, &format!(":TRIGger:MODE {}", word)), "", "{:?}", t);
                assert_eq!(ui.m.trig_type, t, ":TRIGger:MODE {} sets {:?}", word, t);
                assert_eq!(q(&mut sc, &mut ui, &cv, ":TRIGger:MODE?"), short, "{:?}: the answer of :TRIGger:MODE?", t);
                assert_eq!(q(&mut sc, &mut ui, &cv, ":TRIGger:TYPe?"), t.name(), "{:?}", t);
            }
            // the texts
            let tick = |w: &str| format!("`{}`", w);
            assert!(long_words.contains(&tick(long)), "{:?}: {} is not among the type names of docs/scpi.md", t, long);
            assert!(
                short_words.contains(&tick(short)),
                "{:?}: {} is not among the MODE replies of docs/scpi.md",
                t,
                short
            );
            assert!(wiki_row.contains(&tick(long)), "{:?}: {} is not in the wiki's row of :TRIGger:MODE", t, long);
            assert!(names(&features, t.name()), "{:?}: {} is not under Trigger in wiki/Features.md", t, t.name());
            // the pictures
            let shot = format!("-trig-{}.ppm", stored.to_lowercase());
            let shown = shots.lines().any(|l| l.starts_with("shot ") && l.ends_with(&shot));
            match NOT_SHOT.iter().find(|x| x.0 == t) {
                Some(x) => assert!(!shown && !x.1.is_empty(), "{:?}: it has its picture, take it out of NOT_SHOT", t),
                None => {
                    assert!(shown, "{:?}: no picture *{} in ui/tests/triggers.txt, and no reason in NOT_SHOT", t, shot)
                }
            }
        }
    }
}
