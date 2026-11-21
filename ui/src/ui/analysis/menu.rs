// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The analysers' menu entries: labels, values, options, the knob and activation of the items of the spectrum,
//! power, Identify and the other analyser pages (`Ui::ana_*`), with their lists and 1-2-5 ladders. A new item gets
//! its case in each `ana_*` function here.

use super::*;
use crate::ui::menu_text::on_off;

const FIND_LABELS: [&str; 8] = ["1", "2", "3", "4", "5", "6", "7", "8"];

use crate::settings::spectrum::freq_step as step_ladder;

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
            Item::SpecSpan | Item::SpecRbw | Item::EyeRate | Item::PowRef => Kind::Value,
            Item::SpecPeak
            | Item::SpecLimitSet
            | Item::SpecLimitClear
            | Item::EyeClear
            | Item::EyeAct
            | Item::IdentifyAct => Kind::Action,
            Item::ProtoWatch => Kind::Choice,
            // a finding is a row with its summary as the only option: tapping it applies it
            Item::IdFind(_) => Kind::Choice,
            Item::SpecOn
            | Item::SpecSrc
            | Item::SpecHarm
            | Item::EyeOn
            | Item::EyeSrc
            | Item::EyeB
            | Item::EyeRail
            | Item::PowOn
            | Item::PowV
            | Item::PowI
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
            Item::SpecSpan => "Span",
            Item::SpecHarm => "Harmonics",
            Item::SpecRbw => "RBW",
            Item::SpecPeak => "Centre on peak",
            Item::SpecLimitSet => "Limit from trace",
            Item::SpecLimitClear => "Clear limit",
            Item::EyeAct => "Auto Eye/Jitter",
            Item::EyeRate => "Bit rate",
            Item::EyeB => "Clock",
            Item::EyeRail => "Rail",
            Item::EyeClear => "Clear eye",
            Item::PowV => "Voltage",
            Item::PowI => "Current",
            Item::PowVin => "Input",
            Item::PowRef => "Reference",
            Item::PowClk => "Clock",
            Item::IdFind(k) => FIND_LABELS[k.min(7)],
            _ => return None,
        })
    }

    pub(in crate::ui) fn ana_value(&self, it: Item) -> Option<String> {
        let m = &self.m;
        Some(match it {
            Item::SpecOn => on_off(m.view == AView::Spectrum),
            Item::ProtoWatch => on_off(self.azr.watch),
            Item::EyeOn => on_off(m.view == AView::Eye),
            Item::PowOn => on_off(m.view == AView::Power),
            Item::SpecSrc => self.wave_name(m.spec_src),
            Item::EyeSrc => self.wave_name(m.eye_src),
            Item::PowV => self.wave_name(m.pw_v),
            Item::PowI => self.wave_name(m.pw_i),
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
            Item::EyeRate => {
                if m.eye_from_record && m.eye_rate > 0.0 {
                    format!("{} from record", si_short(m.eye_rate, "b/s"))
                } else if m.eye_rate == 0.0 {
                    "Auto".into()
                } else {
                    si_short(m.eye_rate, "b/s")
                }
            }
            Item::EyeB => m.eye_b.map(|id| self.wave_name(id)).unwrap_or_else(|| "Off".into()),
            Item::EyeRail => m.eye_rail.map(|id| self.wave_name(id)).unwrap_or_else(|| "Off".into()),
            Item::PowVin => m.pw_vin.map(|id| self.wave_name(id)).unwrap_or_else(|| "Off".into()),
            Item::PowRef => m.pw_ref.map(|v| si_short(v, "V")).unwrap_or_else(|| "Top".into()),
            Item::PowClk => m.pw_clk.map(|id| self.wave_name(id)).unwrap_or_else(|| "Off".into()),
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
            _ => return None,
        })
    }

    pub(in crate::ui) fn ana_options(&self, it: Item) -> Option<Vec<String>> {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect();
        Some(match it {
            Item::SpecOn | Item::EyeOn | Item::PowOn | Item::ProtoWatch => v(&["Off", "On"]),
            Item::SpecSrc | Item::EyeSrc | Item::PowV | Item::PowI => self.wave_names(),
            Item::EyeB | Item::EyeRail | Item::PowVin | Item::PowClk => self.wave_names_opt(),
            Item::SpecHarm => panes::harm_names(),
            Item::IdFind(_) => vec![self.ana_value(it).unwrap_or_default()],
            _ => return None,
        })
    }

    /// Adjusts an analyser item; false = not an analyser item.
    pub(in crate::ui) fn ana_adjust(&mut self, it: Item, d: i32) -> bool {
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
            self.ana_restart();
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
        let view = crate::ui::analysers::toggle_view;
        let m = &mut self.m;
        match it {
            Item::SpecOn => view(m, AView::Spectrum),
            Item::EyeOn => view(m, AView::Eye),
            Item::PowOn => view(m, AView::Power),
            Item::SpecSpan => m.spec_span = step_ladder(m.spec_span, d),
            Item::SpecRbw => m.spec_rbw = step_ladder(m.spec_rbw, d),
            Item::SpecHarm => {
                m.spec_harm = cycle(m.spec_harm, PANE_HARMS.len(), d);
                self.azr.spec_stop = 0.0;
            }
            Item::EyeRate => {
                m.eye_rate = step_ladder(m.eye_rate, d);
                m.eye_from_record = false;
            }
            Item::PowRef => {
                let cur = m.pw_ref.unwrap_or(0.0);
                let next = step_ladder(cur, d);
                m.pw_ref = if next == 0.0 { None } else { Some(next) };
            }
            _ => return false,
        }
        self.ana_restart();
        true
    }

    /// After a change of an analyser's setting: the averaging and the waterfall start again, the page updates.
    pub(in crate::ui) fn ana_restart(&mut self) {
        self.azr.spec_avg = None;
        if let Some(w) = &mut self.azr.wf {
            w.clear();
        }
        self.analyzer_update();
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
