// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The analysers' menu entries: labels, values, options, the knob and activation of the items of the spectrum,
//! power, Identify and the other analyser pages (`Ui::ana_*`), with their lists and 1-2-5 ladders. A new item gets
//! its case in each `ana_*` function here.

use super::*;

const TRACES: [&str; 4] = ["Normal", "Average", "Max hold", "Min hold"];
const POWER_MODES: [PowerMode; 6] = [
    PowerMode::Quality,
    PowerMode::Harmonics,
    PowerMode::Switching,
    PowerMode::Ripple,
    PowerMode::Capacitance,
    PowerMode::Rail,
];

fn power_page(p: PowerMode) -> &'static str {
    match p {
        PowerMode::Quality => "Quality",
        PowerMode::Harmonics => "Harmonics",
        PowerMode::Switching => "Switching",
        PowerMode::Ripple => "Ripple",
        PowerMode::Capacitance => "Cin",
        PowerMode::Rail => "Rail",
    }
}
const FIND_LABELS: [&str; 8] = ["1", "2", "3", "4", "5", "6", "7", "8"];

/// 1-2-5 frequency ladder 1 Hz .. 1 GHz (0 = full span / auto RBW in front).
fn freq_ladder() -> Vec<f64> {
    decades(0, 10, true)
}

fn step_ladder(v: f64, d: i32) -> f64 {
    ladder_step(&freq_ladder(), v, d, 0)
}

impl Ui {
    /// Remote view changes must refresh stopped references too: there may be no
    /// next acquisition to build the newly opened analyser's results.
    pub(crate) fn set_analysis_view(&mut self, view: AView) {
        self.m.view = view;
        self.relayout();
        self.analyzer_update();
    }

    pub(in crate::ui) fn ana_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::SpecCenter
            | Item::SpecSpan
            | Item::SpecRbw
            | Item::SpecRef
            | Item::SpecAcprW
            | Item::SpecAcprS
            | Item::EyeRate
            | Item::EyeBer
            | Item::EyePnLo
            | Item::EyePnHi
            | Item::EyeNotch
            | Item::PowFline
            | Item::PowFsw
            | Item::PowRef => Kind::Value,
            Item::SpecPeak
            | Item::SpecLimitSet
            | Item::SpecLimitClear
            | Item::EyeClear
            | Item::EyeAct
            | Item::IdentifyAct => Kind::Action,
            Item::SpecMargin => Kind::Value,
            Item::ProtoWatch => Kind::Choice,
            // a finding is a row with its summary as the only option: tapping it applies it
            Item::IdFind(_) => Kind::Choice,
            Item::SpecOn
            | Item::SpecSrc
            | Item::SpecHarm
            | Item::SpecWin
            | Item::SpecTrace
            | Item::SpecWf
            | Item::SpecDist
            | Item::EyeOn
            | Item::EyePlace
            | Item::EyeSrc
            | Item::EyeB
            | Item::EyeHp
            | Item::EyeNpj
            | Item::EyeRail
            | Item::PowOn
            | Item::PowV
            | Item::PowI
            | Item::PowMode
            | Item::PowVin
            | Item::PowClk => Kind::Choice,
            _ => return None,
        })
    }

    pub(in crate::ui) fn ana_label(it: Item) -> Option<&'static str> {
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
            Item::EyeAct => "Auto Eye/Jitter",
            Item::EyePlace => "Place",
            Item::EyeRate => "Bit rate",
            Item::EyeBer => "BER",
            Item::EyeB => "Clock",
            Item::EyeHp => "Highpass",
            Item::EyePnLo => "Pn low",
            Item::EyePnHi => "Pn high",
            Item::EyeNpj => "NPJ",
            Item::EyeRail => "Rail",
            Item::EyeNotch => "Notch",
            Item::EyeClear => "Clear eye",
            Item::PowV => "Voltage",
            Item::PowI => "Current",
            Item::PowMode => "Page",
            Item::PowFline => "Line f",
            Item::PowFsw => "Switch f",
            Item::PowVin => "Input",
            Item::PowRef => "Reference",
            Item::PowClk => "Clock",
            Item::SpecAcprW => "Ch width",
            Item::SpecAcprS => "Ch space",
            Item::IdFind(k) => FIND_LABELS[k.min(7)],
            _ => return None,
        })
    }

    pub(in crate::ui) fn ana_value(&self, it: Item) -> Option<String> {
        let m = &self.m;
        let onoff = |b: bool| if b { "On".to_string() } else { "Off".to_string() };
        Some(match it {
            Item::SpecOn => onoff(m.view == AView::Spectrum),
            Item::ProtoWatch => onoff(self.azr.watch),
            Item::EyeOn => onoff(m.view == AView::Eye),
            Item::EyePlace => match m.eye_place {
                EyePlace::Full => "Full".into(),
                EyePlace::Side => "Side".into(),
                EyePlace::Under => "Under".into(),
            },
            Item::PowOn => onoff(m.view == AView::Power),
            Item::SpecSrc => self.wave_name(m.spec_src),
            Item::EyeSrc => self.wave_name(m.eye_src),
            Item::PowV => self.wave_name(m.pw_v),
            Item::PowI => self.wave_name(m.pw_i),
            Item::SpecCenter => si_short(m.spec_center, "Hz"),
            Item::SpecSpan if m.spec_span > 0.0 => si_short(m.spec_span, "Hz"),
            Item::SpecSpan if self.spec_follows() && self.azr.spec_stop > 0.0 => {
                format!("Auto {}", si_short(self.azr.spec_stop, "Hz"))
            }
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
            Item::EyeRate => {
                if m.eye_from_record && m.eye_rate > 0.0 {
                    format!("{} from record", si_short(m.eye_rate, "b/s"))
                } else if m.eye_rate == 0.0 {
                    "Auto".into()
                } else {
                    si_short(m.eye_rate, "b/s")
                }
            }
            Item::EyeBer => format!("{:.0e}", m.eye_ber),
            Item::EyeB => m.eye_b.map(|id| self.wave_name(id)).unwrap_or_else(|| "Off".into()),
            Item::EyeHp => onoff(m.eye_hp),
            Item::EyePnLo => si_short(m.eye_pn_lo, "Hz"),
            Item::EyePnHi => si_short(m.eye_pn_hi, "Hz"),
            Item::EyeNpj => onoff(m.eye_npj),
            Item::EyeRail => m.eye_rail.map(|id| self.wave_name(id)).unwrap_or_else(|| "Off".into()),
            Item::EyeNotch => {
                if m.eye_notch > 0.0 {
                    si_short(m.eye_notch, "Hz")
                } else {
                    "off".into()
                }
            }
            Item::PowMode => power_page(m.pw_mode).into(),
            Item::PowFline => {
                if m.pw_fline > 0.0 {
                    si_short(m.pw_fline, "Hz")
                } else {
                    "auto".into()
                }
            }
            Item::PowFsw => {
                if m.pw_fsw > 0.0 {
                    si_short(m.pw_fsw, "Hz")
                } else {
                    "auto".into()
                }
            }
            Item::PowVin => m.pw_vin.map(|id| self.wave_name(id)).unwrap_or_else(|| "Off".into()),
            Item::PowRef => m.pw_ref.map(|v| si_short(v, "V")).unwrap_or_else(|| "Top".into()),
            Item::PowClk => m.pw_clk.map(|id| self.wave_name(id)).unwrap_or_else(|| "Off".into()),
            Item::SpecAcprW => si_short(m.spec_acpr_w, "Hz"),
            Item::SpecAcprS => si_short(m.spec_acpr_s, "Hz"),
            Item::IdFind(k) => self.azr.findings.get(k).map_or(String::new(), |f| {
                // the whole text: the row's button draws it in the largest font that fits
                format!("CH{} {}", f.channels.first().map_or(0, |c| c + 1), f.summary)
            }),
            Item::SpecPeak
            | Item::SpecLimitSet
            | Item::SpecLimitClear
            | Item::EyeClear
            | Item::EyeAct
            | Item::IdentifyAct => String::new(),
            Item::SpecMargin => format!("{:.0} dB", self.m.spec_margin),
            _ => return None,
        })
    }

    pub(in crate::ui) fn ana_options(&self, it: Item) -> Option<Vec<String>> {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect();
        Some(match it {
            Item::SpecOn
            | Item::EyeOn
            | Item::PowOn
            | Item::SpecWf
            | Item::SpecDist
            | Item::EyeHp
            | Item::EyeNpj
            | Item::ProtoWatch => v(&["Off", "On"]),
            Item::EyePlace => v(&["Full", "Side", "Under"]),
            Item::SpecSrc | Item::EyeSrc | Item::PowV | Item::PowI => self.wave_names(),
            Item::EyeB | Item::EyeRail | Item::PowVin | Item::PowClk => self.wave_names_opt(),
            Item::SpecHarm => panes::harm_names(),
            Item::SpecWin => crate::math::WINDOWS.iter().map(|w| w.name().to_string()).collect(),
            Item::SpecTrace => v(&TRACES),
            Item::PowMode => POWER_MODES.iter().map(|p| power_page(*p).to_string()).collect(),
            Item::IdFind(_) => vec![self.ana_value(it).unwrap_or_default()],
            _ => return None,
        })
    }

    /// Adjusts an analyser item; false = not an analyser item.
    pub(in crate::ui) fn ana_adjust(&mut self, it: Item, d: i32) -> bool {
        if it == Item::EyePlace {
            // Full, Side and Under also bring a floated eye back into its place (`eye_set_place` only clears it for Full)
            let all = [EyePlace::Full, EyePlace::Side, EyePlace::Under];
            let i = all.iter().position(|&p| p == self.m.eye_place).unwrap_or(0);
            self.workspace.eye_float = None;
            self.eye_set_place(all[(i as i32 + d).rem_euclid(all.len() as i32) as usize]);
            return true;
        }
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
        if matches!(it, Item::SpecSrc | Item::EyeSrc | Item::PowV | Item::PowI) {
            let cur = match it {
                Item::SpecSrc => self.m.spec_src,
                Item::EyeSrc => self.m.eye_src,
                Item::PowV => self.m.pw_v,
                _ => self.m.pw_i,
            };
            let next = self.step_wave(cur, d);
            match it {
                Item::SpecSrc => self.m.spec_src = next,
                Item::EyeSrc => {
                    self.m.eye_src = next;
                    if self.m.eye_from_record {
                        self.m.eye_rate = self.waveform_bit_rate(next).unwrap_or(0.0);
                    }
                    self.azr.eye = None;
                    self.azr.eye_ref = None;
                    self.azr.eye_res = None;
                    self.azr.page = super::eyepage::Page::default();
                }
                Item::PowV => self.m.pw_v = next,
                _ => self.m.pw_i = next,
            }
            self.azr.spec_avg = None;
            if let Some(w) = &mut self.azr.wf {
                w.clear();
            }
            self.analyzer_update();
            return true;
        }
        if matches!(it, Item::EyeB | Item::EyeRail | Item::PowVin | Item::PowClk) {
            let cur = match it {
                Item::EyeB => self.m.eye_b,
                Item::EyeRail => self.m.eye_rail,
                Item::PowVin => self.m.pw_vin,
                _ => self.m.pw_clk,
            };
            let next = self.step_wave_opt(cur, d);
            match it {
                Item::EyeB => self.m.eye_b = next,
                Item::EyeRail => self.m.eye_rail = next,
                Item::PowVin => self.m.pw_vin = next,
                _ => self.m.pw_clk = next,
            }
            self.analyzer_update();
            return true;
        }
        let view = |m: &mut Model, v: AView| m.view = if m.view == v { AView::Scope } else { v };
        let m = &mut self.m;
        match it {
            Item::SpecOn => view(m, AView::Spectrum),
            Item::EyeOn => view(m, AView::Eye),
            Item::PowOn => view(m, AView::Power),
            Item::SpecCenter => m.spec_center = step_ladder(m.spec_center, d),
            Item::SpecSpan => m.spec_span = step_ladder(m.spec_span, d),
            Item::SpecRbw => m.spec_rbw = step_ladder(m.spec_rbw, d),
            Item::SpecHarm => {
                m.spec_harm = cycle(m.spec_harm, PANE_HARMS.len(), d);
                self.azr.spec_stop = 0.0;
            }
            Item::SpecMargin => m.spec_margin = (m.spec_margin + d as f32).clamp(0.0, 60.0),
            Item::SpecWin => {
                let w = crate::math::WINDOWS;
                m.spec_win = w[cycle(w.iter().position(|&x| x == m.spec_win).unwrap_or(0), w.len(), d)];
            }
            Item::SpecTrace => m.spec_trace = cycle(m.spec_trace, 4, d),
            Item::SpecRef => m.spec_ref += 5.0 * d as f32,
            Item::SpecWf => m.spec_wf = !m.spec_wf,
            Item::SpecDist => m.spec_dist = !m.spec_dist,
            Item::EyeRate => {
                m.eye_rate = step_ladder(m.eye_rate, d);
                m.eye_from_record = false;
            }
            Item::EyePlace => {
                let all = [EyePlace::Full, EyePlace::Side, EyePlace::Under];
                let i = all.iter().position(|&p| p == m.eye_place).unwrap_or(0);
                m.eye_place = all[(i as i32 + d).rem_euclid(all.len() as i32) as usize];
            }
            Item::EyeBer => {
                const BERS: [f64; 5] = [1e-3, 1e-6, 1e-9, 1e-12, 1e-15];
                let i = BERS.iter().position(|&x| (x - m.eye_ber).abs() < x * 0.01).unwrap_or(3);
                m.eye_ber = BERS[step_index(i, BERS.len(), d)];
            }
            Item::EyeHp => m.eye_hp = !m.eye_hp,
            Item::EyePnLo => m.eye_pn_lo = step_ladder(m.eye_pn_lo, d).max(1.0),
            Item::EyePnHi => m.eye_pn_hi = step_ladder(m.eye_pn_hi, d).max(1.0),
            Item::EyeNpj => m.eye_npj = !m.eye_npj,
            Item::EyeNotch => m.eye_notch = step_ladder(m.eye_notch, d),
            Item::SpecAcprW => m.spec_acpr_w = step_ladder(m.spec_acpr_w.max(1.0), d).max(1.0),
            Item::SpecAcprS => m.spec_acpr_s = step_ladder(m.spec_acpr_s.max(1.0), d).max(1.0),
            Item::PowFline => m.pw_fline = step_ladder(m.pw_fline, d),
            Item::PowFsw => m.pw_fsw = step_ladder(m.pw_fsw, d),
            Item::PowRef => {
                let cur = m.pw_ref.unwrap_or(0.0);
                let next = step_ladder(cur, d);
                m.pw_ref = if next == 0.0 { None } else { Some(next) };
            }
            Item::PowMode => {
                m.pw_mode = POWER_MODES
                    [cycle(POWER_MODES.iter().position(|&p| p == m.pw_mode).unwrap_or(0), POWER_MODES.len(), d)]
            }
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
    pub(in crate::ui) fn ana_activate(&mut self, it: Item) -> bool {
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
            Item::EyeAct => {
                self.eye_make();
                return true;
            }
            Item::IdentifyAct => {
                self.identify_now();
                self.open_menu(MenuId::Identify);
            }
            _ => return false,
        }
        true
    }
}
