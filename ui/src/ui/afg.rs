// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Built-in generator page (S models): output on/off, wave shape (sine, square, ramp, pulse, noise, DC), frequency,
//! amplitude, offset, duty / symmetry. The acquisition thread applies the settings (`hw.rs`, `oos_hal::dho::afg`) with the
//! factory calibration; limits as the stock system, frequency caps per shape from the platform limits.

use super::*;

impl Ui {
    /// Values of the generator card: frequency, and wave with amplitude.
    pub(super) fn afg_card_text(&self) -> (String, String) {
        (self.value(Item::AfgFreq), format!("{} \u{b7} {}", self.value(Item::AfgWave), self.value(Item::AfgAmpl)))
    }

    pub(super) fn afg_items(&self) -> Vec<Item> {
        let mut v = vec![Item::AfgOn, Item::AfgWave];
        match self.m.afg_wave {
            // DC: the level is the offset
            5 => return [v, vec![Item::AfgOffset]].concat(),
            6 => return [v, vec![Item::AfgFreq, Item::AfgAmpl, Item::AfgOffset, Item::AfgArbSrc, Item::AfgArbGrab, Item::AfgArbFile]].concat(),
            4 => v.extend([Item::AfgAmpl, Item::AfgOffset]),
            _ => v.extend([Item::AfgFreq, Item::AfgAmpl, Item::AfgOffset]),
        }
        match self.m.afg_wave {
            1 | 3 => v.push(Item::AfgDuty),
            2 => v.push(Item::AfgSym),
            _ => {}
        }
        // modulation of sine, square and ramp carriers
        if self.m.afg_wave <= 2 {
            v.push(Item::AfgMod);
            if self.m.afg_mod > 0 {
                v.extend([Item::AfgModRate, Item::AfgModDepth, Item::AfgModShape]);
            }
        }
        v
    }

    pub(super) fn afg_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::AfgOn | Item::AfgWave | Item::AfgMod | Item::AfgModShape | Item::AfgArbSrc | Item::AfgFreqEntry => Kind::Choice,
            Item::AfgArbGrab | Item::AfgArbFile => Kind::Action,
            Item::AfgFreq | Item::AfgAmpl | Item::AfgOffset | Item::AfgDuty | Item::AfgSym | Item::AfgModRate | Item::AfgModDepth => Kind::Value,
            _ => return None,
        })
    }

    pub(super) fn afg_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Afg) => "Generator",
            Item::AfgOn => "Output",
            Item::AfgFreq => "Frequency",
            Item::AfgFreqEntry => "Enter frequency",
            Item::AfgAmpl => "Amplitude",
            Item::AfgOffset => "Offset",
            Item::AfgWave => "Wave",
            Item::AfgDuty => "Duty",
            Item::AfgSym => "Symmetry",
            Item::AfgMod => "Modulation",
            Item::AfgModRate => "Mod. rate",
            // depth (AM) or deviation (FM, PM): the label follows the value
            Item::AfgModDepth => "Depth / dev.",
            Item::AfgModShape => "Mod. shape",
            Item::AfgArbSrc => "Capture from",
            Item::AfgArbGrab => "Capture the screen",
            Item::AfgArbFile => "Load arb.csv",
            _ => return None,
        })
    }

    pub(super) fn afg_value(&self, it: Item) -> Option<String> {
        let m = &self.m;
        Some(match it {
            Item::AfgOn => if m.afg_on { "On".into() } else { "Off".into() },
            Item::AfgFreq => si_short(m.afg_freq, "Hz"),
            Item::AfgFreqEntry => "Keypad".into(),
            Item::AfgAmpl => si_short(m.afg_ampl, "Vpp"),
            Item::AfgOffset => si(m.afg_offset, "V"),
            Item::AfgWave => AFG_WAVES[m.afg_wave].into(),
            Item::AfgDuty => format!("{:.0} %", m.afg_duty),
            Item::AfgSym => format!("{:.0} %", m.afg_symmetry),
            Item::AfgMod => AFG_MODS[m.afg_mod].into(),
            Item::AfgModRate => si_short(m.afg_mod_rate, "Hz"),
            Item::AfgModDepth => match m.afg_mod {
                2 => format!("dev {}", si_short(m.afg_fm_dev, "Hz")),
                3 => format!("dev {:.0}\u{b0}", m.afg_pm_dev),
                _ => format!("{:.0} %", m.afg_mod_depth),
            },
            Item::AfgModShape => AFG_MOD_SHAPES[m.afg_mod_shape].into(),
            Item::AfgArbSrc => format!("CH{}", m.afg_arb_src + 1),
            Item::AfgArbGrab | Item::AfgArbFile => String::new(),
            _ => return None,
        })
    }

    pub(super) fn afg_options(&self, it: Item) -> Option<Vec<String>> {
        match it {
            Item::AfgOn => Some(vec!["Off".into(), "On".into()]),
            Item::AfgWave => Some(AFG_WAVES.iter().map(|s| s.to_string()).collect()),
            Item::AfgMod => Some(AFG_MODS.iter().map(|s| s.to_string()).collect()),
            Item::AfgModShape => Some(AFG_MOD_SHAPES.iter().map(|s| s.to_string()).collect()),
            Item::AfgArbSrc => Some(ch_names()),
            _ => None,
        }
    }

    pub(super) fn afg_adjust(&mut self, it: Item, d: i32) -> bool {
        let m = &mut self.m;
        let ladder = |x: f64, d: i32, lo: f64, hi: f64| {
            let mut l: Vec<f64> = (-3..9).flat_map(|e| [1.0, 2.0, 5.0].map(|k| k * 10f64.powi(e))).filter(|&v| v >= lo && v <= hi).collect();
            // the cap itself is the last step (60 MHz, or an experimental limit off the 1-2-5 ladder)
            if l.last().is_some_and(|&v| v < hi * 0.999) {
                l.push(hi);
            }
            let i = l.iter().position(|&v| v >= x * 0.999).unwrap_or(l.len() - 1);
            l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize]
        };
        match it {
            Item::AfgOn => m.afg_on = !m.afg_on,
            Item::AfgArbSrc => m.afg_arb_src = src_step(m.afg_arb_src, NCH, d),
            // sine up to 60 MHz (platform limits, limits.conf `afg.*_max_hz`)
            Item::AfgFreq => m.afg_freq = ladder(m.afg_freq, d, 0.01, m.afg_wave_hal().max_hz()),
            Item::AfgAmpl => m.afg_ampl = ladder(m.afg_ampl, d, 0.002, 10.0),
            Item::AfgOffset => m.afg_offset = ((m.afg_offset + 0.1 * d as f64) * 10.0).round() / 10.0,
            Item::AfgWave => m.afg_wave = (m.afg_wave as i32 + d).rem_euclid(AFG_WAVES.len() as i32) as usize,
            Item::AfgDuty => m.afg_duty = (m.afg_duty + 5.0 * d as f64).clamp(5.0, 95.0),
            Item::AfgSym => m.afg_symmetry = (m.afg_symmetry + 5.0 * d as f64).clamp(0.0, 100.0),
            Item::AfgMod => m.afg_mod = (m.afg_mod as i32 + d).rem_euclid(AFG_MODS.len() as i32) as usize,
            Item::AfgModShape => m.afg_mod_shape = (m.afg_mod_shape as i32 + d).rem_euclid(AFG_MOD_SHAPES.len() as i32) as usize,
            // stock limits: rate 2 mHz .. 1 MHz
            Item::AfgModRate => m.afg_mod_rate = ladder(m.afg_mod_rate, d, 0.01, 1e6),
            Item::AfgModDepth => match m.afg_mod {
                2 => m.afg_fm_dev = ladder(m.afg_fm_dev, d, 0.01, 30e6),
                3 => m.afg_pm_dev = (m.afg_pm_dev + 15.0 * d as f64).clamp(0.0, 360.0),
                _ => m.afg_mod_depth = (m.afg_mod_depth + 10.0 * d as f64).clamp(0.0, 120.0),
            },
            _ => return false,
        }
        self.afg_clamp();
        true
    }

    /// A key of the frequency keypad: digits edit the entry in `keypad`, a unit sets the value and goes back. What is
    /// outside the range of the wave comes to its limit, and the message says so.
    pub(super) fn freq_key(&mut self, k: usize) {
        let Some(&key) = FREQ_KEYS.get(k) else { return };
        let unit = match key {
            "mHz" => Some(1e-3),
            "Hz" => Some(1.0),
            "kHz" => Some(1e3),
            "MHz" => Some(1e6),
            _ => None,
        };
        if let Some(u) = unit {
            if let Ok(v) = self.keypad.parse::<f64>() {
                let (want, lo, hi) = (v * u, 0.01, self.m.afg_wave_hal().max_hz());
                self.m.afg_freq = want;
                self.afg_clamp();
                if want > hi {
                    self.toast(&format!("Highest frequency of this wave: {}", si_short(hi, "Hz")));
                } else if want < lo {
                    self.toast(&format!("Lowest frequency: {}", si_short(lo, "Hz")));
                }
                self.mark_changed();
            }
            return self.close_picker();
        }
        match key {
            "Cancel" => self.close_picker(),
            "Clear" => self.keypad.clear(),
            "Del" => {
                self.keypad.pop();
            }
            "." if self.keypad.contains('.') => {}
            d if self.keypad.len() < 12 => self.keypad.push_str(d),
            _ => {}
        }
    }

    /// The stock limits: amplitude by frequency, offset within (Vmax - A) / 2.
    fn afg_clamp(&mut self) {
        let m = &mut self.m;
        m.afg_freq = m.afg_freq.clamp(0.01, m.afg_wave_hal().max_hz());
        // FM: the deviation stays below the carrier and inside the frequency range
        m.afg_fm_dev = m.afg_fm_dev.min(m.afg_freq).min(m.afg_wave_hal().max_hz() - m.afg_freq).max(0.01);
        // DC has only its level, noise no frequency: the hidden amplitude or frequency must not limit them
        let f = if matches!(m.afg_wave_hal(), oos_hal::dho::afg::Wave::Noise) { 1e6 } else { m.afg_freq * 1e6 };
        if matches!(m.afg_wave_hal(), oos_hal::dho::afg::Wave::Dc) {
            m.afg_offset = m.afg_offset.clamp(-5.0, 5.0);
            return;
        }
        let (a, o) = oos_hal::dho::afg::clamp(m.afg_ampl * 1e9, m.afg_offset * 1e9, f);
        (m.afg_ampl, m.afg_offset) = (a / 1e9, o / 1e9);
    }

    /// The arbitrary wave from samples (one period); false without enough samples.
    pub fn afg_arb_load(&mut self, v: &[f32]) -> bool {
        match oos_hal::dho::afg::arb_set(v) {
            Some(id) => {
                (self.m.afg_arb, self.m.afg_wave) = (id, 6);
                self.afg_clamp();
                true
            }
            None => false,
        }
    }

    /// The screen of channel `c` as the arbitrary wave (one period = the screen), kept as arb.csv in the storage
    /// directory: the table itself is not part of a setup.
    pub fn afg_arb_capture(&mut self, c: usize) -> Result<usize, String> {
        let Some(rec) = self.last[0].clone() else { return Err("No waveform to capture".into()) };
        let c = c.min(NCH - 1);
        if rec.ch[c].is_empty() {
            return Err(format!("CH{} has no data", c + 1));
        }
        let (v, _) = self.meas_wave(&rec, c);
        if !self.afg_arb_load(&v) {
            return Err(format!("CH{} has no data", c + 1));
        }
        let text: String = v.iter().filter(|x| x.is_finite()).map(|x| format!("{}\n", x)).collect();
        let _ = crate::store::write(&self.store_dir.join("arb.csv"), text.as_bytes());
        Ok(v.len())
    }

    /// Actions of the generator page.
    pub(super) fn afg_activate(&mut self, it: Item) -> bool {
        match it {
            Item::AfgArbGrab => match self.afg_arb_capture(self.m.afg_arb_src) {
                Ok(n) => self.toast(&format!("Arb wave: {} points of CH{}", n, self.m.afg_arb_src + 1)),
                Err(e) => self.toast(&e),
            },
            Item::AfgArbFile => {
                let p = self.store_dir.join("arb.csv");
                let v = std::fs::read_to_string(&p).map(|t| oos_hal::dho::afg::arb_parse(&t)).unwrap_or_default();
                if self.afg_arb_load(&v) {
                    self.toast(&format!("Arb wave: {} points from arb.csv", v.len()));
                } else {
                    self.toast(&format!("No wave in {}", p.display()));
                }
            }
            _ => return false,
        }
        true
    }

    /// SCPI `:SOURce`: changes the given settings (frequency in Hz, amplitude in Vpp, offset in V) within the limits.
    pub fn afg_set(&mut self, on: Option<bool>, f: Option<f64>, a: Option<f64>, o: Option<f64>) {
        let m = &mut self.m;
        m.afg_on = on.unwrap_or(m.afg_on);
        m.afg_freq = f.unwrap_or(m.afg_freq);
        m.afg_ampl = a.unwrap_or(m.afg_ampl);
        m.afg_offset = o.unwrap_or(m.afg_offset);
        self.afg_clamp();
    }
}

#[cfg(test)]
mod arb_tests {
    #[test]
    fn the_frequency_value_opens_the_keypad() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.open_menu(MenuId::Afg);
        assert!(!ui.items(MenuId::Afg).contains(&Item::AfgFreqEntry));
        let d = ui.dlg_layout().unwrap();
        let (mi, mid, _) = Ui::value_parts(d.rows.iter().find(|r| r.it == Item::AfgFreq).unwrap().ctrl);
        let f = ui.m.afg_freq;
        ui.tap(mi.x + 4, mi.y + 4);
        assert!(ui.m.afg_freq < f && ui.menu == MenuId::Afg);
        ui.tap(mid.x + mid.w / 2, mid.y + mid.h / 2);
        assert!(ui.menu == MenuId::Pick && ui.pick.unwrap().it == Item::AfgFreqEntry);
    }

    #[test]
    fn frequency_keypad_ends_with_the_unit_and_keeps_the_limit() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.open_menu(MenuId::Afg);
        let type_in = |ui: &mut Ui, keys: &[&str]| {
            ui.open_picker(Item::AfgFreqEntry);
            for k in keys {
                ui.activate(Item::PickOpt(FREQ_KEYS.iter().position(|x| x == k).unwrap()));
            }
        };
        type_in(&mut ui, &["1", "2", ".", "5", ".", "kHz"]);
        assert_eq!(ui.m.afg_freq, 12.5e3);
        assert_eq!(ui.menu, MenuId::Afg);
        // above the limit of the wave: the limit, with a message
        type_in(&mut ui, &["9", "9", "9", "MHz"]);
        assert_eq!(ui.m.afg_freq, ui.m.afg_wave_hal().max_hz());
        assert!(ui.toast.as_ref().is_some_and(|t| t.0.contains("Highest")));
        // a ramp has a lower limit than a sine
        ui.m.afg_wave = 2;
        type_in(&mut ui, &["2", "0", "MHz"]);
        assert_eq!(ui.m.afg_freq, 5e6);
        // cancelled, or nothing typed: the value stays
        type_in(&mut ui, &["7", "Cancel"]);
        assert_eq!(ui.m.afg_freq, 5e6);
        type_in(&mut ui, &["Hz"]);
        assert_eq!(ui.m.afg_freq, 5e6);
        type_in(&mut ui, &["1", "Del", "2", "Hz"]);
        assert_eq!(ui.m.afg_freq, 2.0);
    }

    use super::*;

    #[test]
    fn capture_load_and_scpi() {
        let dir = std::env::temp_dir().join(format!("oos-ui-arb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.store_dir = dir.clone();
        assert!(ui.afg_arb_capture(0).is_err(), "no record yet");
        for _ in 0..3 {
            ui.step(0.05);
        }
        // the screen of CH1 (1 kHz square) becomes the wave, kept as arb.csv
        let n = ui.afg_arb_capture(0).unwrap();
        assert!(n > 100);
        assert_eq!(ui.m.afg_wave, 6);
        assert!(matches!(ui.m.afg_wave_hal(), oos_hal::dho::afg::Wave::Arb { id } if id == ui.m.afg_arb));
        let t = ui.m.afg_wave_hal().table(1e3);
        assert!(t.iter().any(|&v| v > 30000) && t.iter().any(|&v| v < -30000));
        assert!(ui.afg_items().contains(&Item::AfgArbGrab));
        let id = ui.m.afg_arb;
        ui.activate(Item::AfgArbFile);
        assert_ne!(ui.m.afg_arb, id, "loaded from arb.csv");
        // an empty channel is refused and the frequency cap is the arb one
        assert!(ui.afg_arb_capture(3).is_err());
        ui.m.afg_freq = 50e6;
        ui.afg_set(None, None, None, None);
        assert_eq!(ui.m.afg_freq, 10e6);

        let cv = Canvas::new(W, H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let id = ui.m.afg_arb;
        scpi.exec_line(&mut ui, &cv, ":SOUR:ARB:DATA 0,1,0,-1");
        assert_ne!(ui.m.afg_arb, id);
        assert_eq!(scpi.exec_line(&mut ui, &cv, ":SOUR:FUNC?"), b"ARB");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn grid_settings() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.adjust(Item::GridBright, -3);
        ui.adjust(Item::GridFrame, 1);
        assert_eq!((ui.m.grid_bright, ui.m.grid_frame), (70, false));
        let back = crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap();
        assert_eq!((back.grid_bright, back.grid_frame), (70, false));
        let mut cv = Canvas::new(W, H);
        ui.step(0.05);
        ui.render(&mut cv);
    }
}
