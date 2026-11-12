// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The analysers in the plot area: spectrum analyser (RBW, trace modes, peak markers, waterfall,
//! THD/SNR/SFDR), eye diagram with jitter (eyepage.rs), power analysis, and Identify (names the signals and applies a
//! trigger and decoder). The maths is in oos-dsp (`spectrum`, `eye`, `power`, `identify`); this file holds the menus,
//! the per-record update on every captured sample (`Ui::meas_wave`) and the drawing.

use super::*;
use oos_dsp::{eye, identify, power, spectrum};

/// Analyser state kept between records.
#[derive(Default)]
pub struct State {
    pub spec: Option<spectrum::Spectrum>,
    pub spec_avg: Option<spectrum::Avg>,
    pub wf: Option<spectrum::Waterfall>,
    pub dist: Option<spectrum::Distortion>,
    /// the lines for table and markers, in the order of the table
    pub peaks: Vec<spectrum::Marker>,
    /// top of the band the pane follows (0 = none yet), and when the Results pane last had a spectrum
    pub spec_stop: f64,
    pub spec_t: f64,
    /// the limit line, and of the traces since it was set: how many, how many went over, the worst bin
    pub limit: Option<spectrum::Limit>,
    pub limit_n: u64,
    pub limit_over: u64,
    pub limit_worst: Option<(f64, f32)>,
    pub eye: Option<eye::Eye>,
    /// what the eye was built for (source, scale, offset, rate, raster size), to restart it on a change
    pub eye_key: (usize, u32, u32, u64, i32, i32),
    pub eye_res: Option<(eye::EyeMeas, eye::Jitter, f64)>,
    pub page: super::eyepage::Page,
    pub power: Vec<String>,
    pub findings: Vec<identify::Finding>,
    /// protocol watch: on/off, the bus last named per channel group, time of the last check
    pub watch: bool,
    pub watch_seen: Vec<String>,
    pub watch_t: f64,
    /// bandwidth test (bwtest.rs): result, info line, time of the last update
    pub bw: Option<oos_dsp::bandwidth::StepResponse>,
    pub bw_info: String,
    pub bw_t: f64,
}

/// A bus a finding names ("UART 115200 on CH3"), None for plain signals.
fn bus_name(f: &identify::Finding) -> Option<String> {
    use identify::SignalKind as K;
    let chs = f.channels.iter().map(|c| format!("CH{}", c + 1)).collect::<Vec<_>>().join("+");
    let what = match &f.kind {
        K::Uart { baud, .. } => format!("UART {:.0} Bd", baud),
        K::I2c { .. } => "I2C".to_string(),
        K::Spi { mode, .. } => format!("SPI mode {}", mode),
        K::Can { bitrate, .. } => format!("CAN {}", si_short(*bitrate, "b/s")),
        K::Lin { baud } => format!("LIN {:.0} Bd", baud),
        K::OneWire => "1-Wire".to_string(),
        K::Manchester { bitrate } => format!("Manchester {}", si_short(*bitrate, "b/s")),
        K::Video { .. } => "video".to_string(),
        _ => return None,
    };
    Some(format!("{} on {}", what, chs))
}

const TRACES: [&str; 4] = ["Normal", "Average", "Max hold", "Min hold"];
const POWER_MODES: [PowerMode; 4] = [PowerMode::Quality, PowerMode::Harmonics, PowerMode::Switching, PowerMode::Ripple];
const FIND_LABELS: [&str; 8] = ["1", "2", "3", "4", "5", "6", "7", "8"];

/// 1-2-5 frequency ladder 1 Hz .. 1 GHz (0 = full span / auto RBW in front).
fn freq_ladder() -> Vec<f64> {
    let mut v = vec![0.0];
    let mut d = 1.0;
    while d < 2e9 {
        v.extend([d, 2.0 * d, 5.0 * d]);
        d *= 10.0;
    }
    v
}

fn step_ladder(v: f64, d: i32) -> f64 {
    let l = freq_ladder();
    let i = l.iter().position(|&x| x >= v * 0.999).unwrap_or(0);
    l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize]
}

impl Ui {
    pub(super) fn ana_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::SpecCenter | Item::SpecSpan | Item::SpecRbw | Item::SpecRef | Item::EyeRate => Kind::Value,
            Item::SpecPeak | Item::SpecLimitSet | Item::SpecLimitClear | Item::EyeClear | Item::IdentifyAct => Kind::Action,
            Item::SpecMargin => Kind::Value,
            Item::ProtoWatch => Kind::Choice,
            // a finding is a row with its summary as the only option: tapping it applies it
            Item::IdFind(_) => Kind::Choice,
            Item::SpecOn | Item::SpecSrc | Item::SpecHarm | Item::SpecWin | Item::SpecTrace | Item::SpecWf | Item::SpecDist | Item::EyeOn | Item::EyeSrc | Item::PowOn | Item::PowV | Item::PowI | Item::PowMode => Kind::Choice,
            _ => return None,
        })
    }

    pub(super) fn ana_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Spectrum) => "Spectrum",
            Item::Open(MenuId::Eye) => "Eye / jitter",
            Item::Open(MenuId::Power) => "Power analysis",
            Item::IdentifyAct => "Identify",
            Item::ProtoWatch => "Protocol watch",
            Item::SpecOn | Item::EyeOn | Item::PowOn => "View",
            Item::SpecSrc | Item::EyeSrc => "Source",
            Item::SpecCenter => "Centre",
            Item::SpecSpan => "Span",
            Item::SpecHarm => "Harmonics",
            Item::SpecRbw => "RBW",
            Item::SpecWin => "Window",
            Item::SpecTrace => "Trace",
            Item::SpecRef => "Ref level",
            Item::SpecWf => "Waterfall",
            Item::SpecDist => "THD / SNR",
            Item::SpecPeak => "Centre on peak",
            Item::SpecLimitSet => "Limit from trace",
            Item::SpecLimitClear => "Clear limit",
            Item::SpecMargin => "Limit margin",
            Item::EyeRate => "Bit rate",
            Item::EyeClear => "Clear eye",
            Item::PowV => "Voltage",
            Item::PowI => "Current",
            Item::PowMode => "Page",
            Item::IdFind(k) => FIND_LABELS[k.min(7)],
            _ => return None,
        })
    }

    pub(super) fn ana_value(&self, it: Item) -> Option<String> {
        let m = &self.m;
        let onoff = |b: bool| if b { "On".to_string() } else { "Off".to_string() };
        Some(match it {
            Item::SpecOn => onoff(m.view == AView::Spectrum),
            Item::ProtoWatch => onoff(self.azr.watch),
            Item::EyeOn => onoff(m.view == AView::Eye),
            Item::PowOn => onoff(m.view == AView::Power),
            Item::SpecSrc => format!("CH{}", m.spec_src + 1),
            Item::EyeSrc => match self.eye_ref {
                Some(i) => format!("R{}", i + 1),
                None => format!("CH{}", m.eye_src + 1),
            },
            Item::PowV => format!("CH{}", m.pw_v + 1),
            Item::PowI => format!("CH{}", m.pw_i + 1),
            Item::SpecCenter => si_short(m.spec_center, "Hz"),
            Item::SpecSpan if m.spec_span > 0.0 => si_short(m.spec_span, "Hz"),
            Item::SpecSpan if self.spec_follows() && self.azr.spec_stop > 0.0 => format!("Auto {}", si_short(self.azr.spec_stop, "Hz")),
            Item::SpecSpan => "Full".into(),
            Item::SpecHarm => panes::harm_names()[m.spec_harm.min(PANE_HARMS.len() - 1)].clone(),
            Item::SpecRbw => match &self.azr.spec {
                Some(s) if m.spec_rbw == 0.0 => format!("Auto {}", si_short(s.rbw, "Hz")),
                _ if m.spec_rbw == 0.0 => "Auto".into(),
                _ => si_short(m.spec_rbw, "Hz"),
            },
            Item::SpecWin => m.spec_win.name().into(),
            Item::SpecTrace => TRACES[m.spec_trace.min(3)].into(),
            Item::SpecRef => format!("{:.0} dBV", m.spec_ref),
            Item::SpecWf => onoff(m.spec_wf),
            Item::SpecDist => onoff(m.spec_dist),
            Item::EyeRate => if m.eye_rate == 0.0 { "Auto".into() } else { si_short(m.eye_rate, "b/s") },
            Item::PowMode => format!("{:?}", m.pw_mode),
            Item::IdFind(k) => self.azr.findings.get(k).map_or(String::new(), |f| {
                // short enough for the row's button (longer text would turn it into a - value + picker)
                let mut s = format!("CH{} {}", f.channels.first().map_or(0, |c| c + 1), f.summary);
                if s.chars().count() > 34 {
                    s = s.chars().take(32).collect::<String>() + "..";
                }
                s
            }),
            Item::SpecPeak | Item::SpecLimitSet | Item::SpecLimitClear | Item::EyeClear | Item::IdentifyAct => String::new(),
            Item::SpecMargin => format!("{:.0} dB", self.m.spec_margin),
            _ => return None,
        })
    }

    pub(super) fn ana_options(&self, it: Item) -> Option<Vec<String>> {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect();
        Some(match it {
            Item::SpecOn | Item::EyeOn | Item::PowOn | Item::SpecWf | Item::SpecDist | Item::ProtoWatch => v(&["Off", "On"]),
            Item::SpecSrc | Item::PowV | Item::PowI => ch_names(),
            Item::EyeSrc => self.eye_source_names(),
            Item::SpecHarm => panes::harm_names(),
            Item::SpecWin => crate::math::WINDOWS.iter().map(|w| w.name().to_string()).collect(),
            Item::SpecTrace => v(&TRACES),
            Item::PowMode => POWER_MODES.iter().map(|p| format!("{:?}", p)).collect(),
            Item::IdFind(_) => vec![self.ana_value(it).unwrap_or_default()],
            _ => return None,
        })
    }

    /// Adjusts an analyser item; false = not an analyser item.
    pub(super) fn ana_adjust(&mut self, it: Item, d: i32) -> bool {
        if let Item::IdFind(k) = it {
            if let Some(f) = self.azr.findings.get(k).cloned() {
                self.apply_finding(&f);
                self.close_menu();
                self.toast(&format!("Applied: {}", f.summary));
            }
            return true;
        }
        if it == Item::ProtoWatch {
            self.azr.watch = !self.azr.watch;
            self.azr.watch_seen.clear();
            self.azr.watch_t = 0.0;
            return true;
        }
        if it == Item::EyeSrc {
            self.step_eye_source(d);
            return true;
        }
        let cyc = |i: usize, n: usize| (i as i32 + d).rem_euclid(n as i32) as usize;
        let view = |m: &mut Model, v: AView| m.view = if m.view == v { AView::Scope } else { v };
        let m = &mut self.m;
        match it {
            Item::SpecOn => view(m, AView::Spectrum),
            Item::EyeOn => view(m, AView::Eye),
            Item::PowOn => view(m, AView::Power),
            Item::SpecSrc => m.spec_src = src_step(m.spec_src, NCH, d),
            Item::PowV => m.pw_v = src_step(m.pw_v, NCH, d),
            Item::PowI => m.pw_i = src_step(m.pw_i, NCH, d),
            Item::SpecCenter => m.spec_center = step_ladder(m.spec_center, d),
            Item::SpecSpan => m.spec_span = step_ladder(m.spec_span, d),
            Item::SpecRbw => m.spec_rbw = step_ladder(m.spec_rbw, d),
            Item::SpecHarm => {
                m.spec_harm = cyc(m.spec_harm, PANE_HARMS.len());
                self.azr.spec_stop = 0.0;
            }
            Item::SpecMargin => m.spec_margin = (m.spec_margin + d as f32).clamp(0.0, 60.0),
            Item::SpecWin => {
                let w = crate::math::WINDOWS;
                m.spec_win = w[cyc(w.iter().position(|&x| x == m.spec_win).unwrap_or(0), w.len())];
            }
            Item::SpecTrace => m.spec_trace = cyc(m.spec_trace, 4),
            Item::SpecRef => m.spec_ref += 5.0 * d as f32,
            Item::SpecWf => m.spec_wf = !m.spec_wf,
            Item::SpecDist => m.spec_dist = !m.spec_dist,
            Item::EyeRate => m.eye_rate = step_ladder(m.eye_rate, d),
            Item::PowMode => m.pw_mode = POWER_MODES[cyc(POWER_MODES.iter().position(|&p| p == m.pw_mode).unwrap_or(0), 4)],
            _ => return false,
        }
        // the averaging/waterfall/eye restart on any analyser change
        self.azr.spec_avg = None;
        if let Some(w) = &mut self.azr.wf {
            w.clear();
        }
        self.analyzer_update();
        true
    }

    /// Runs an analyser action; false = not one.
    pub(super) fn ana_activate(&mut self, it: Item) -> bool {
        match it {
            Item::SpecLimitSet => {
                let Some(sp) = &self.azr.spec else {
                    self.toast("Spectrum: no data on the source");
                    return true;
                };
                self.azr.limit = Some(spectrum::limit_from(sp, self.m.spec_margin, 8));
                (self.azr.limit_n, self.azr.limit_over, self.azr.limit_worst) = (0, 0, None);
                return true;
            }
            Item::SpecLimitClear => {
                self.azr.limit = None;
                return true;
            }
            Item::SpecPeak => {
                if let Some(p) = self.strongest_line() {
                    self.m.spec_center = p.f;
                    if self.m.spec_span == 0.0 {
                        self.m.spec_span = (p.f / 5.0).max(1.0);
                    }
                    self.azr.spec_avg = None;
                }
            }
            Item::EyeClear => {
                self.azr.eye = None;
                self.azr.eye_res = None;
                self.azr.page = Default::default();
            }
            Item::IdentifyAct => {
                self.identify_now();
                self.open_menu(MenuId::Identify);
            }
            _ => return false,
        }
        true
    }

    /// The strongest spectral line above DC.
    pub(super) fn strongest_line(&self) -> Option<spectrum::Marker> {
        self.azr.peaks.iter().copied().max_by(|a, b| a.dbv.total_cmp(&b.dbv))
    }

    /// The spectrum is on the screen and its span is left to the signal.
    fn spec_follows(&self) -> bool {
        let shown = self.m.view == AView::Spectrum || self.m.view == AView::Scope && matches!(self.m.pane, tables::PANE_SPECTRUM | tables::PANE_RESULTS);
        shown && self.m.spec_span == 0.0 && PANE_HARMS[self.m.spec_harm.min(PANE_HARMS.len() - 1)] > 0
    }

    /// Puts the lines into the order of the table: by frequency or by level.
    pub(super) fn peaks_sort(&mut self) {
        if self.m.spec_sort == 0 {
            self.azr.peaks.sort_by(|a, b| a.f.total_cmp(&b.f));
        } else {
            self.azr.peaks.sort_by(|a, b| b.dbv.total_cmp(&a.dbv));
        }
    }

    /// The spectrum of the source from the record: the view, or `pane` beside the waveform.
    fn spectrum_update(&mut self, rec: &Record, pane: bool) {
        let c = self.m.spec_src;
        if rec.ch[c].is_empty() {
            self.azr.spec = None;
            self.azr.peaks.clear();
            return;
        }
        let (v, dt) = self.meas_wave(rec, c);
        let f1 = self.ana[c].get(Meas::Freq);
        let (mut center, mut span) = (self.m.spec_center, self.m.spec_span);
        if self.spec_follows() {
            // no frequency this record: the span stays
            self.azr.spec_stop = spectrum::auto_stop(f1, PANE_HARMS[self.m.spec_harm.min(PANE_HARMS.len() - 1)], self.azr.spec_stop);
            (center, span) = (self.azr.spec_stop / 2.0, self.azr.spec_stop);
        }
        let area = if pane { self.pane_rect() } else { self.area };
        // Auto RBW: the finest the record allows (window ENBW / record time)
        let finest = spectrum::enbw(self.m.spec_win) / (v.len().max(1) as f64 * dt);
        let s = spectrum::Settings {
            center,
            span,
            rbw: if self.m.spec_rbw > 0.0 { self.m.spec_rbw } else { finest },
            window: self.m.spec_win,
            averaging: 16,
            trace: [spectrum::Trace::Normal, spectrum::Trace::Average, spectrum::Trace::MaxHold, spectrum::Trace::MinHold][self.m.spec_trace.min(3)],
            detector: spectrum::Detector::Peak,
            points: self.spec_box(area, pane).plot.w.max(100) as usize,
            ref_level_dbv: self.m.spec_ref as f64,
        };
        let sp = spectrum::analyze(&v, dt, &s, &mut self.azr.spec_avg);
        self.azr.peaks = spectrum::rank_lines(&sp, Some(f1), panes::PEAKS_MAX);
        self.peaks_sort();
        if self.m.spec_wf && !pane {
            self.azr.wf.get_or_insert_with(|| spectrum::Waterfall::new(120)).push(&sp);
        }
        self.azr.dist = if self.m.spec_dist && !pane { spectrum::distortion(&v, dt, None, 10) } else { None };
        if let Some(l) = &self.azr.limit {
            self.azr.limit_n += 1;
            if let Some(w) = spectrum::limit_check(&sp, l) {
                self.azr.limit_over += 1;
                if self.azr.limit_worst.is_none_or(|x| w.1 > x.1) {
                    self.azr.limit_worst = Some(w);
                }
            }
        }
        self.azr.spec = Some(sp);
    }

    pub(super) fn identify_items(&self) -> Vec<Item> {
        let mut v = vec![Item::ProtoWatch];
        v.extend((0..self.azr.findings.len().min(8)).map(Item::IdFind));
        v
    }

    /// Protocol watch: every 2 s Identify on the screen record of the channels that are on; a bus that was
    /// not there before is announced, the findings are ready in the Identify menu.
    pub(super) fn protocol_watch(&mut self) {
        if !self.azr.watch || self.now - self.azr.watch_t < 2.0 {
            return;
        }
        self.azr.watch_t = self.now;
        let Some(rec) = self.last[0].clone() else { return };
        // every captured sample of the window (the screen record undersamples fast buses), as the measurements do
        let mut dt = rec.dt;
        let owned: Vec<Vec<f32>> = (0..NCH)
            .map(|c| {
                if !self.m.ch[c].on || rec.ch[c].is_empty() {
                    return Vec::new();
                }
                let (v, d) = self.meas_wave(&rec, c);
                dt = d;
                v
            })
            .collect();
        let ws: Vec<&[f32]> = owned.iter().map(|v| v.as_slice()).collect();
        let found = identify::identify(&ws, dt);
        let buses: Vec<String> = found.iter().filter(|f| f.confidence >= 0.6).filter_map(bus_name).collect();
        for b in &buses {
            if !self.azr.watch_seen.contains(b) {
                self.toast(&format!("Protocol watch: {} found (Identify to apply)", b));
            }
        }
        self.azr.watch_seen = buses;
        self.azr.findings = found;
    }

    /// SCPI / MCP: Identify now, one finding per line ("CH1 Clock 1.000 kHz ... (90 %)").
    pub fn identify_text(&mut self) -> String {
        self.identify_now();
        self.azr.findings.iter().map(|f| format!("{} {} ({:.0} %)", f.channels.iter().map(|c| format!("CH{}", c + 1)).collect::<Vec<_>>().join("+"), f.summary, f.confidence * 100.0)).collect::<Vec<_>>().join(";")
    }

    /// Identify on every captured sample of the channels that are on.
    fn identify_now(&mut self) {
        let Some(rec) = self.last[0].clone() else {
            self.azr.findings.clear();
            return;
        };
        let mut dt = rec.dt;
        let ws: Vec<Vec<f32>> = (0..NCH)
            .map(|c| {
                if self.m.ch[c].on && !rec.ch[c].is_empty() {
                    let (v, d) = self.meas_wave(&rec, c);
                    dt = d;
                    v
                } else {
                    Vec::new()
                }
            })
            .collect();
        let refs: Vec<&[f32]> = ws.iter().map(|v| v.as_slice()).collect();
        self.azr.findings = identify::identify(&refs, dt);
        if self.azr.findings.is_empty() {
            self.toast("Identify: nothing found");
        }
    }

    /// Trigger and decoder of an Identify finding.
    fn apply_finding(&mut self, f: &identify::Finding) {
        use identify::{DecoderHint as D, TriggerHint as T};
        let m = &mut self.m;
        match f.trigger {
            T::Edge { source, level, rising, .. } => {
                (m.trig_type, m.trig_src, m.trig_level, m.slope) = (TrigType::Edge, source, level, if rising { Slope::Rise } else { Slope::Fall });
            }
            T::PulseWidth { source, level, positive, less_than, greater_than } => {
                (m.trig_type, m.trig_src, m.trig_level) = (TrigType::Pulse, source, level);
                m.trig_pol = if positive { Pol::Pos } else { Pol::Neg };
                match (less_than, greater_than) {
                    (Some(t), _) => (m.trig_when, m.trig_t1) = (WhenKind::Lt, t as f32),
                    (None, Some(t)) => (m.trig_when, m.trig_t1) = (WhenKind::Gt, t as f32),
                    _ => {}
                }
            }
            T::Video { source, ntsc } => {
                (m.trig_type, m.trig_src) = (TrigType::Video, source);
                m.video_std = if ntsc { VideoStd::Ntsc } else { VideoStd::Pal };
            }
            T::None => {}
        }
        let nearest = |l: &[f64], x: f64| l.iter().enumerate().min_by(|a, b| (a.1 - x).abs().partial_cmp(&(b.1 - x).abs()).unwrap()).map_or(0, |e| e.0);
        let Some(d) = &f.decoder else { return };
        m.dec_on = true;
        match d {
            D::Uart { ch, baud, bits, parity, thr, .. } => {
                (m.dec_proto, m.dec_src, m.dec_bits, m.dec_parity, m.dec_thr) = (Proto::Uart, *ch, *bits, *parity, *thr);
                m.dec_baud = nearest(&crate::decode::BAUDS, *baud);
            }
            D::I2c { sda, scl, thr } => (m.dec_proto, m.dec_src, m.dec_src2, m.dec_thr) = (Proto::I2c, *sda, *scl, *thr),
            D::Spi { clk, data, cs, mode, bits, thr } => {
                (m.dec_proto, m.dec_src2, m.dec_cs, m.dec_mode, m.dec_bits, m.dec_thr) = (Proto::Spi, *clk, *cs, *mode, *bits, *thr);
                m.dec_src = data.first().copied().unwrap_or(0);
            }
            D::Can { ch, bitrate, thr, invert } => {
                (m.dec_proto, m.dec_src, m.dec_thr, m.dec_canh) = (Proto::Can, *ch, *thr, *invert);
                m.dec_canrate = nearest(&crate::decode::CAN_RATES, *bitrate);
            }
            D::Lin { ch, baud, thr } => {
                (m.dec_proto, m.dec_src, m.dec_thr) = (Proto::Lin, *ch, *thr);
                m.dec_baud = nearest(&crate::decode::BAUDS, *baud);
            }
            D::OneWire { ch, thr } => (m.dec_proto, m.dec_src, m.dec_thr) = (Proto::OneWire, *ch, *thr),
            D::Manchester { .. } => m.dec_on = false,
        }
    }

    /// Recomputes the active analyser from the last record (every captured sample of the window).
    pub(super) fn analyzer_update(&mut self) {
        // a source that is off shows nothing: the first channel that is on takes over
            if let Some(on) = (0..NCH).find(|&c| self.m.ch[c].on) {
            if !self.m.ch[self.m.spec_src].on {
                self.m.spec_src = on;
            }
            if self.eye_ref.is_none() && !self.m.ch[self.m.eye_src].on {
                self.m.eye_src = on;
            }
        }
        // the second pane runs the spectrum beside the scope; the peak list of the Results pane needs it less often
        let beside = self.m.view == AView::Scope && self.pane_h() > 0 && matches!(self.m.pane, tables::PANE_SPECTRUM | tables::PANE_RESULTS);
        if beside && self.m.pane == tables::PANE_RESULTS {
            if self.azr.spec.is_some() && (self.now - self.azr.spec_t).abs() < 0.5 {
                return;
            }
            self.azr.spec_t = self.now;
        }
        let view = if beside { AView::Spectrum } else { self.m.view };
        if view == AView::Scope {
            return;
        }
        let Some(rec) = self.last[0].clone() else { return };
        match view {
            AView::Spectrum => self.spectrum_update(&rec, beside),
            AView::Eye => self.eye_update(&rec),
            AView::Power => {
                let (cv, ci) = (self.m.pw_v, self.m.pw_i);
                if rec.ch[cv].is_empty() || (rec.ch[ci].is_empty() && self.m.pw_mode != PowerMode::Ripple) {
                    self.azr.power = vec!["Voltage / current channel off: Switch on source in the menu".into()];
                    return;
                }
                let (v, dt) = self.meas_wave(&rec, cv);
                let i = if rec.ch[ci].is_empty() { Vec::new() } else { self.meas_wave(&rec, ci).0 };
                self.azr.power = match self.m.pw_mode {
                    PowerMode::Quality => {
                        let q = power::quality(&v, &i, dt, None);
                        vec![
                            format!("Vrms {}   Irms {}", si(q.vrms, "V"), si(q.irms, "A")),
                            format!("P {}   S {}   Q {}", si(q.p_real, "W"), si(q.s_apparent, "VA"), si(q.q_reactive, "var")),
                            format!("PF {:.3}   phase {:.1}\u{b0}   f {}", q.pf, q.phase_deg, si(q.freq, "Hz")),
                            format!("crest V {:.2}   crest I {:.2}   {} cycles", q.crest_v, q.crest_i, q.cycles),
                        ]
                    }
                    PowerMode::Harmonics => {
                        let h = power::harmonics(&i, dt, None, 15);
                        let mut out = vec!["Current harmonics vs. IEC 61000-3-2 class A".to_string()];
                        out.extend(h.iter().map(|(n, a, lim, pass)| format!("H{:<2} {:>10}  limit {:>10}  {}", n, si(*a, "A"), if lim.is_finite() { si(*lim, "A") } else { "-".into() }, if *pass { "pass" } else { "FAIL" })));
                        out
                    }
                    PowerMode::Switching => {
                        let s = power::switching(&v, &i, dt, 10.0);
                        vec![
                            format!("E on {}   E off {}", si(s.e_on, "J"), si(s.e_off, "J")),
                            format!("E cond {}   P avg {}", si(s.e_cond, "J"), si(s.p_avg, "W")),
                            format!("{} transitions", s.transitions.len()),
                        ]
                    }
                    PowerMode::Ripple => {
                        let (pp, ac) = power::ripple(&v);
                        let (pk, tpk, i2t) = if i.is_empty() { (f64::NAN, f64::NAN, f64::NAN) } else { power::inrush(&i, dt) };
                        vec![format!("Ripple {} pp   {} rms AC", si(pp, "V"), si(ac, "V")), format!("Inrush peak {} at {}   I\u{b2}t {}", si(pk, "A"), si(tpk, "s"), si(i2t, "A\u{b2}s"))]
                    }
                };
            }
            AView::Step => self.bw_update(&rec),
            AView::Scope | AView::Bode => {}
        }
    }

    /// The active analyser in the plot area.
    pub(super) fn draw_analyzer(&self, cv: &mut Canvas) {
        match self.m.view {
            AView::Spectrum => self.draw_spectrum(cv, self.area, false),
            AView::Eye => self.draw_eye(cv),
            AView::Power => self.draw_power(cv),
            AView::Bode => self.draw_bode(cv),
            AView::Step => self.draw_bw(cv),
            AView::Scope => {}
        }
    }

    fn panel(&self, cv: &mut Canvas, x: i32, y: i32, lines: &[String], col: Rgb) {
        let f = &self.fonts;
        let w = lines.iter().map(|l| f.s.width(l)).max().unwrap_or(0) + 16;
        let r = Rect::new(x, y, w, lines.len() as i32 * 18 + 8);
        cv.round(r, 4, PANEL_BG, 230);
        for (k, l) in lines.iter().enumerate() {
            cv.text(&f.s, r.x + 8, r.y + 17 + 18 * k as i32, l, col);
        }
    }

    /// The spectrum in `a`: the plot area, or the second pane (`pane`).
    pub(super) fn draw_spectrum(&self, cv: &mut Canvas, a: Rect, pane: bool) {
        let f = &self.fonts;
        let col = CH_COLORS[self.m.spec_src];
        let Some(sp) = &self.azr.spec else {
            let t = if self.m.ch[self.m.spec_src].on { "Spectrum: waiting for a record".to_string() } else { format!("Spectrum: {} is off", self.ch_name(self.m.spec_src)) };
            cv.text(&f.sb, a.x + 20, a.y + a.h / 2 + 5, &t, DIM);
            return;
        };
        cv.fill(a, BG);
        let b = self.spec_box(a, pane);
        let g = b.plot;
        let wf_h = self.spec_wf_h(a, pane);
        let n = sp.dbv.len().max(2);
        let (top, dbdiv) = (self.m.spec_ref, panes::DB_PER_DIV);
        let y_of = |db: f32| (g.y as f32 + (top - db) / dbdiv * (g.h as f32 / VDIV)).clamp(g.y as f32 - 2.0, (g.y + g.h) as f32 + 2.0) as i32;
        let x_of = |i: usize| g.x + (i as i64 * (g.w as i64 - 1) / (n as i64 - 1)) as i32;
        self.draw_spec_frame(cv, &b, sp);
        cv.set_clip(g);
        let mut prev: Option<(i32, i32)> = None;
        for (i, &db) in sp.dbv.iter().enumerate() {
            let p = (x_of(i), y_of(db));
            if let Some(q) = prev {
                cv.line(q.0, q.1, p.0, p.1, col);
            }
            prev = Some(p);
        }
        // the limit, over the frequencies of the trace on the screen
        if let Some(l) = &self.azr.limit {
            let mut prev: Option<(i32, i32)> = None;
            for i in 0..n {
                let Some(db) = spectrum::limit_at(l, sp.freq(i)) else {
                    prev = None;
                    continue;
                };
                let p = (x_of(i), y_of(db));
                if let Some(q) = prev {
                    cv.line(q.0, q.1, p.0, p.1, RED);
                }
                prev = Some(p);
            }
        }
        cv.no_clip();
        self.draw_spec_marks(cv, &b, sp, top);
        self.draw_peaks(cv, &b, sp, pane);
        if let (true, Some(w)) = (wf_h > 0, &self.azr.wf) {
            let wr = Rect::new(g.x, a.y + a.h - wf_h, g.w, wf_h);
            let rows = w.rows.len().max(1);
            let rh = (wf_h as usize / rows.max(wf_h as usize / 3)).max(1) as i32;
            for (k, row) in w.rows.iter().rev().enumerate() {
                let y = wr.y + k as i32 * rh;
                if y >= wr.y + wr.h {
                    break;
                }
                let m = row.len().max(2);
                for px in 0..wr.w {
                    let i = (px as usize * (m - 1)) / (wr.w as usize - 1).max(1);
                    let (r, gg, b) = spectrum::color(row[i.min(m - 1)], top, 90.0);
                    cv.fill(Rect::new(wr.x + px, y, 1, rh), rgb(r as u32, gg as u32, b as u32));
                }
            }
            cv.hline(wr.x, wr.x + wr.w, wr.y, DIM);
        }
    }

    fn draw_power(&self, cv: &mut Canvas) {
        let a = self.area;
        let mut l = vec![format!("Power  V = CH{}  I = CH{}  ({:?})", self.m.pw_v + 1, self.m.pw_i + 1, self.m.pw_mode)];
        l.extend(self.azr.power.iter().cloned());
        self.panel(cv, a.x + 20, a.y + 30, &l, TEXT);
    }
}
