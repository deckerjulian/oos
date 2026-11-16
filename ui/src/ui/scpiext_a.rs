// SPDX-License-Identifier: GPL-2.0-or-later
//! SCPI coverage extensions, tier 1 (continued): measurement copy and remove, channel zero bias, analysis deskew,
//! math auto scale and linear, the Eye and Spectrum actions. See `scpiext.rs`.

use super::scpiext::*;
use super::*;
use crate::scpi::{e as en, num, parse_wave};

pub(super) const T1A: &[&str] = &[
    "MEASure:COPY", "MEASure:ITEM:REMove",
    "CHANnel<n>:BIAS:ZERO",
    "CALibrate:DESKew", "CALibrate:DESKew:AUTO", "CALibrate:DESKew:CLEar",
    "MATH<n>:AUTO", "MATH<n>:LINear:A", "MATH<n>:LINear:B",
    "EYE:AUTO", "EYE:CLEar",
    "SPECtrum:PEAK:CENTer", "SPECtrum:LIMit:SET", "SPECtrum:LIMit:CLEar", "SPECtrum:LIMit:MARGin",
];

impl Ui {
    pub(super) fn ext_a(&mut self, key: &str, idx: Option<usize>, q: bool, arg: &str) -> Option<Reply> {
        let action = |q: bool| if q { Err(undefined(key)) } else { DONE };
        Some(match key {
            // Measure > Copy to: the menu's select_option (the measurements of the Source A move to the other source)
            "MEASure:COPY" => {
                if q { return Some(Err(undefined(key))); }
                let Some(to) = parse_wave(arg).filter(|w| self.options(Item::MeasCopyTo).contains(&self.wave_name(*w))) else { return Some(Err(bad(arg))) };
                let name = self.wave_name(to);
                self.select_option(Item::MeasCopyTo, &name);
                DONE
            }
            // Measure: remove one row, by its number in :MEASure:STRip? order or by item and source
            "MEASure:ITEM:REMove" => {
                if q { return Some(Err(undefined(key))); }
                let mut it = arg.split(',').map(str::trim);
                let first = it.next().unwrap_or("");
                let (a, b) = (it.next().and_then(parse_wave), it.next().and_then(parse_wave));
                let index = if let Ok(n) = first.parse::<usize>() {
                    (1..=self.measurement_count()).contains(&n).then(|| n - 1)
                } else if let Some(k) = crate::scpi::meas_kind(first) {
                    let src = a.unwrap_or(self.meas_src);
                    self.m.meas.iter().position(|&e| e == (k, src))
                } else if let Some(k) = crate::scpi::pair_kind(first) {
                    let (a, b) = (a.unwrap_or(self.meas_src), b.unwrap_or(self.meas_src_b));
                    self.m.meas2.iter().position(|&e| e == (k, a, b)).map(|i| self.m.meas.len() + i)
                } else { None };
                match index {
                    Some(i) => { self.activate(Item::MeasRemove(i as u16)); DONE }
                    None => Err(bad(arg)),
                }
            }
            "CHANnel<n>:BIAS:ZERO" => {
                let c = idx.and_then(|n| n.checked_sub(1)).filter(|&c| c < n_ch())?;
                if q { return Some(Err(undefined(key))); }
                self.activate(Item::ChBiasZero(c));
                DONE
            }
            // analysis deskew of the shown traces (Horizontal > Auto deskew / Clear deskew)
            "CALibrate:DESKew:AUTO" => { self.activate(Item::AutoDeskew); action(q) }
            "CALibrate:DESKew:CLEar" => { self.activate(Item::ClearDeskew); action(q) }
            "CALibrate:DESKew" => {
                let Some(id) = arg.split(',').next().and_then(parse_wave).filter(|w| match w { WaveId::Ch(c) => *c < n_ch(), _ => true }) else { return Some(Err(bad(arg))) };
                let slot: &mut f64 = match id { WaveId::Ch(c) => &mut self.m.deskew_ch[c], WaveId::Ref(i) => &mut self.m.deskew_ref[i] };
                if q { return Some(t(en(*slot))); }
                let Some(v) = arg.split(',').nth(1).and_then(|s| s.trim().parse::<f64>().ok()).filter(|x| x.is_finite() && x.abs() <= 1000.0) else { return Some(Err(bad(arg))) };
                *slot = v;
                self.deskew_changed();
                DONE
            }
            "MATH<n>:AUTO" => {
                let k = idx.and_then(|n| n.checked_sub(1)).filter(|&k| k < NMATH)?;
                if q { return Some(Err(undefined(key))); }
                self.math_fit(k);
                self.mark_changed();
                DONE
            }
            "MATH<n>:LINear:A" | "MATH<n>:LINear:B" => {
                let k = idx.and_then(|n| n.checked_sub(1)).filter(|&k| k < NMATH)?;
                let mut c = self.m.math_ch(k);
                let a = key.ends_with(":A");
                if q { return Some(t(en(if a { c.lin.0 } else { c.lin.1 } as f64))); }
                let Some(v) = num(arg) else { return Some(Err(bad(arg))) };
                if a { c.lin.0 = v } else { c.lin.1 = v }
                if k == self.m.math_sel { self.m.math_lin = c.lin; } else { self.m.maths[k] = c; }
                self.mark_changed();
                DONE
            }
            // Eye / Jitter: Auto Eye/Jitter and Clear eye
            "EYE:AUTO" => { self.ana_activate(Item::EyeAct); action(q) }
            "EYE:CLEar" => { self.ana_activate(Item::EyeClear); action(q) }
            // Spectrum: centre on the strongest line, limit line from the trace / clear, its margin
            "SPECtrum:PEAK:CENTer" => { self.ana_activate(Item::SpecPeak); action(q) }
            "SPECtrum:LIMit:SET" => { self.ana_activate(Item::SpecLimitSet); action(q) }
            "SPECtrum:LIMit:CLEar" => { self.ana_activate(Item::SpecLimitClear); action(q) }
            "SPECtrum:LIMit:MARGin" => numf(q, arg, &mut self.m.spec_margin, 0.0, 60.0),
            _ => return None,
        })
    }
}

