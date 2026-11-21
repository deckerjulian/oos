// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What is derived from the channels and drawn over them: the mask (test, building it, saving and loading it), the
//! histogram, the math channels with their scale and the decoded buses with their packet rows, and the mapping of
//! volts and times to screen pixels they share. A new derived trace gets its drawing here and its call in
//! `Ui::render_layers` (`render.rs`).

use super::*;

/// The mask test in software as the screen keeps it (ADR 0062); the mask unit in the FPGA is `hwmask::Unit`.
#[derive(Default)]
pub(super) struct MaskTest {
    /// the reference waveform (volts, screen time) the mask is built from
    pub reference: Option<Vec<f32>>,
    /// the mask built from it with the tolerances of the menu (`build_mask`)
    pub mask: Option<oos_dsp::mask::Mask>,
    /// acquisitions that passed and failed (`mask_test`)
    pub pass: u64,
    pub fail: u64,
}

impl MaskTest {
    /// Drops the mask and its reference: a new source channel, a default set-up or Clear needs a new one.
    pub fn forget(&mut self) {
        self.mask = None;
        self.reference = None;
    }
}

/// The decoded lines of the records shown (ADR 0062): the packets with times in s, by a key of the record and the
/// settings, analog and LA alike; the screen redraws ~30 times a second and decodes a record once.
#[derive(Default)]
pub(super) struct Decoded(std::cell::RefCell<Vec<(u64, Vec<oos_dsp::decode::Packet>)>>);

impl Decoded {
    /// The packets kept under `key`.
    pub fn get(&self, key: u64) -> Option<Vec<oos_dsp::decode::Packet>> {
        self.0.borrow().iter().find(|e| e.0 == key).map(|e| e.1.clone())
    }

    /// Keeps what the decoders of one record need.
    pub fn put(&self, key: u64, p: &[oos_dsp::decode::Packet]) {
        let mut c = self.0.borrow_mut();
        if c.len() >= DECODERS * 2 {
            c.remove(0);
        }
        c.push((key, p.to_vec()));
    }

    /// How many results are kept.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }
}

impl Ui {
    /// Mask test of one acquisition (live records only): counts passed / failed, stops on a failure if set.
    pub(super) fn mask_test(&mut self, rec: &Record) {
        if !self.m.mask_on {
            return;
        }
        let Some(mask) = &self.mask_sw.mask else { return };
        let v = &rec.ch[self.m.mask_src];
        if v.is_empty() {
            return;
        }
        if mask.test(v).pass {
            self.mask_sw.pass += 1;
        } else {
            self.mask_sw.fail += 1;
            // with the hardware test the counters decide (hwmask.rs)
            if self.m.mask_stop && self.m.run == RunState::Run && !self.mask_hw_active() {
                self.m.run = RunState::Stop;
                self.toast("Mask failed: stopped");
            }
        }
    }

    /// (Re)builds the mask from the reference waveform with the tolerances of the menu (X in divisions of the 10-div
    /// window, Y in divisions of the source channel's current V/div).
    pub(super) fn build_mask(&mut self) {
        let Some(r) = &self.mask_sw.reference else { return };
        let tol_x = (self.m.mask_x / HDIV * r.len() as f32).round() as usize;
        let tol_y = self.m.mask_y * self.m.ch[self.m.mask_src].volts_per_div();
        let mut mask = oos_dsp::mask::build(r, tol_x, tol_y);
        mask.invert = self.m.mask_inv;
        self.mask_sw.mask = Some(mask);
        self.mask_cols_update();
    }

    /// Creates the mask from the source channel of the record shown and starts the test.
    pub(super) fn create_mask(&mut self) {
        let c = self.m.mask_src;
        let Some(v) = self.last[0].as_ref().map(|r| r.ch[c].clone()).filter(|v| !v.is_empty()) else {
            return self.toast(&format!("CH{} has no data", c + 1));
        };
        self.mask_sw.reference = Some(v);
        self.build_mask();
        self.m.mask_on = true;
        self.mask_counts_reset();
        self.toast(&format!("Mask created from CH{}", c + 1));
    }

    /// Creates the mask from the selected reference slot.
    pub(super) fn create_mask_from_ref(&mut self) {
        let Some(r) = self.refs[self.ref_page.sel].as_ref().filter(|r| r.v.len() >= 2) else {
            return self.toast(&format!("R{} is empty", self.ref_page.sel + 1));
        };
        self.mask_sw.reference = Some(r.v.as_ref().clone());
        self.m.mask_src = r.src.min(NCH - 1);
        self.build_mask();
        self.m.mask_on = true;
        self.mask_sw.pass = 0;
        self.mask_sw.fail = 0;
        self.toast(&format!("Mask created from R{}", self.ref_page.sel + 1));
    }

    /// Off plus the operations of the selected Math category.
    pub(super) fn math_ops_in_family(&self) -> Vec<MathOp> {
        let mut v = vec![MathOp::Off];
        v.extend_from_slice(self.math.family.ops());
        v
    }

    pub(super) fn ff_save(&mut self) {
        let recs: Vec<Record> = if !self.history.recs.is_empty() {
            self.history.recs.iter().cloned().collect()
        } else if let Some(rec) = self.last[0].as_ref() {
            vec![rec.clone()]
        } else {
            return self.toast("No frames to save");
        };
        let p = crate::store::next_name(&self.save_dir(), "frame", "bin");
        let r = crate::store::write(&p, &crate::store::ff_bin(&recs, &self.m));
        self.toast_saved(&p, r);
    }

    pub(super) fn ff_load(&mut self) {
        let p = match crate::store::latest_name(&self.save_dir(), "frame", "bin") {
            Some(p) => p,
            None => return self.toast("No frame-NNN.bin in the save folder"),
        };
        let data = match std::fs::read(&p) {
            Ok(b) => b,
            Err(e) => return self.toast(&format!("Load failed: {}", e)),
        };
        match crate::store::parse_ff(&data) {
            Ok(recs) if !recs.is_empty() => {
                self.history.recs = recs.into();
                let n = self.history.recs.len();
                self.history.pos = Some(n - 1);
                self.last[0] = Some(self.history.recs[n - 1].clone());
                self.m.run = RunState::Stop;
                self.measure_last();
                self.toast(&format!("Loaded {} frames from {}", n, crate::store::file_name(&p)));
            }
            Ok(_) => self.toast("That file holds no frames"),
            Err(e) => self.toast(&format!("Load failed: {}", e)),
        }
    }

    /// Forbidden region of the mask (outside the envelope) tinted red, the envelope edges a little stronger.
    pub(super) fn draw_mask(&self, cv: &mut Canvas) {
        let (Some(mask), true, false) = (&self.mask_sw.mask, self.m.mask_on, self.xy) else { return };
        if !self.shows(self.m.mask_src) {
            return;
        }
        let a = self.plot.area;
        let ch = &self.m.ch[self.m.mask_src];
        let ppd = a.h as f32 / VDIV;
        let cy = a.y as f32 + a.h as f32 / 2.0;
        let y_of =
            |v: f32| -> i32 { (cy - (v + ch.offset) / ch.volts_per_div() * ppd).round().clamp(-1e6, 1e6) as i32 };
        let (top, bot) = (a.y, a.y + a.h);
        for x in 0..a.w {
            let (lo, hi) = mask.band(x as f64 / a.w as f64, (x + 1) as f64 / a.w as f64);
            let yh = y_of(hi).clamp(top, bot);
            let yl = (y_of(lo) + 1).clamp(top, bot);
            let px = a.x + x;
            if self.m.mask_inv {
                if yl > yh {
                    cv.blend(Rect::new(px, yh, 1, yl - yh), RED, 58);
                    cv.blend(Rect::new(px, yh, 1, 1), RED, 120);
                    cv.blend(Rect::new(px, yl - 1, 1, 1), RED, 120);
                }
            } else {
                if yh > top {
                    cv.blend(Rect::new(px, top, 1, yh - top), RED, 58);
                    cv.blend(Rect::new(px, yh - 1, 1, 1), RED, 120);
                }
                if yl < bot {
                    cv.blend(Rect::new(px, yl, 1, bot - yl), RED, 58);
                    cv.blend(Rect::new(px, yl, 1, 1), RED, 120);
                }
            }
        }
    }

    /// "Mask CH1 · Passed n · Failed m" pill at the top left of the plot, under the status line.
    pub(super) fn draw_mask_pill(&self, cv: &mut Canvas) {
        if !self.m.mask_on || self.mask_sw.mask.is_none() || !self.shows(self.m.mask_src) {
            return;
        }
        let f = self.fonts();
        let c = self.m.mask_src;
        let (pass, fail) = self.mask_counts();
        let parts: [(String, Rgb); 6] = [
            (if self.mask_hw_active() { "Mask (HW) ".into() } else { "Mask ".into() }, TEXT),
            (format!("CH{}", c + 1), CH_COLORS[c]),
            ("  \u{b7}  Passed ".into(), DIM),
            (fmt_count(pass as f64), GREEN),
            ("  \u{b7}  Failed ".into(), DIM),
            (fmt_count(fail as f64), if fail > 0 { RED } else { TEXT }),
        ];
        let w: i32 = parts.iter().map(|(s, _)| f.sb.width(s)).sum::<i32>() + 24;
        let a = self.plot.area;
        let r = Rect::new(a.x + 8, a.y + 24, w, 24);
        let col = if fail > 0 { RED } else { GREEN };
        cv.round(r, 12, mix(BAR, col, 110), 255);
        cv.round(r.inset(1), 11, mix(BAR, col, 26), 255);
        let mut x = r.x + 12;
        for (s, c) in &parts {
            x = cv.text(&f.sb, x, r.y + 17, s, *c);
        }
    }

    /// Z input: brightness 0..1 per sample from the Z channel's screen position (bottom = dark, top = full).
    pub(super) fn z_weights(&self, rec: &Record) -> Option<Vec<f32>> {
        let z = self.m.z_src?;
        let v = &rec.ch[z];
        if v.is_empty() {
            return None;
        }
        let ch = &self.m.ch[z];
        let vpd = ch.volts_per_div();
        Some(v.iter().map(|&x| (((x + ch.offset) / vpd + VDIV / 2.0) / VDIV).clamp(0.0, 1.0)).collect())
    }

    /// Histogram source: the measurement source if it is on, else the first channel on.
    pub(super) fn hist_src(&self) -> Option<usize> {
        if let WaveId::Ch(c) = self.sources.a {
            if c < n_ch() && self.m.ch[c].on {
                return Some(c);
            }
        }
        (0..n_ch()).find(|&c| self.m.ch[c].on)
    }

    /// Histogram bins from the phosphor map: rows (vertical) or columns within +-0.5 div of the trigger level
    /// (horizontal); with the mean and standard deviation in pixels.
    pub(super) fn hist_bins(&self) -> Option<(Vec<u64>, f64, f64)> {
        let c = self.hist_src()?;
        let area = self.source_area(WaveId::Ch(c)).unwrap_or(self.wave_area());
        let phosphor = if self.m.zoom { &self.phz } else { &self.ph };
        let (w, h) = (phosphor.w, phosphor.h);
        let map = &phosphor.map[c];
        let bins: Vec<u64> = match self.m.hist {
            Hist::Off => return None,
            Hist::Vertical => (0..h).map(|r| map[r * w..(r + 1) * w].iter().map(|&x| x as u64).sum()).collect(),
            Hist::Horizontal => {
                let ch = &self.m.ch[c];
                let ppd = h as f32 / VDIV;
                let (factor, center) = self.zoom_vertical_transform(area);
                let yl =
                    h as f32 / 2.0 - ((self.m.trig_level + ch.offset) / ch.volts_per_div() - center) * factor * ppd;
                let (r0, r1) = (((yl - ppd / 2.0).max(0.0)) as usize, ((yl + ppd / 2.0).max(0.0) as usize).min(h));
                (0..w).map(|x| (r0..r1).map(|r| map[r * w + x] as u64).sum()).collect()
            }
        };
        let n: u64 = bins.iter().sum();
        if n == 0 {
            return Some((bins, f64::NAN, f64::NAN));
        }
        let mean = bins.iter().enumerate().map(|(i, &b)| i as f64 * b as f64).sum::<f64>() / n as f64;
        let var = bins.iter().enumerate().map(|(i, &b)| (i as f64 - mean).powi(2) * b as f64).sum::<f64>() / n as f64;
        Some((bins, mean, var.sqrt()))
    }

    /// Histogram overlay: bars along the left edge (vertical) or the bottom (horizontal), mean and sigma in units.
    pub(super) fn draw_hist(&self, cv: &mut Canvas) {
        let Some((bins, mean, sd)) = self.hist_bins() else { return };
        let Some(c) = self.hist_src().filter(|&c| self.shows(c)) else { return };
        let a = self.plot.area;
        let col = CH_COLORS[c];
        let peak = bins.iter().copied().max().unwrap_or(0).max(1) as f64;
        let depth = (a.w / 5).max(40) as f64;
        cv.set_clip(a);
        let ch = &self.m.ch[c];
        let txt = match self.m.hist {
            Hist::Vertical => {
                // a bin per row of the map; a larger plot (HDMI layout) gives each bin its share of rows
                let n = bins.len().max(1) as i32;
                for (r, &b) in bins.iter().enumerate() {
                    let len = (b as f64 / peak * depth).round() as i32;
                    let (y0, y1) = (r as i32 * a.h / n, (r as i32 + 1) * a.h / n);
                    if len > 0 {
                        cv.blend(Rect::new(a.x, a.y + y0, len, (y1 - y0).max(1)), col, 150);
                    }
                }
                // pixel row -> volts
                let (factor, center) = self.zoom_vertical_transform(a);
                let h = bins.len().max(1) as f64;
                let vpp = ch.volts_per_div() as f64 * VDIV as f64 / h / factor as f64;
                let mv = (h / 2.0 - mean) * vpp + center as f64 * ch.volts_per_div() as f64 - ch.offset as f64;
                format!("Hist CH{}  mean {}  \u{3c3} {}", c + 1, si(mv, self.m.unit(c)), si(sd * vpp, self.m.unit(c)))
            }
            _ => {
                let n = bins.len().max(1) as i32;
                for (x, &b) in bins.iter().enumerate() {
                    let len = (b as f64 / peak * depth * 0.6).round() as i32;
                    let (x0, x1) = (x as i32 * a.w / n, (x as i32 + 1) * a.w / n);
                    if len > 0 {
                        cv.blend(Rect::new(a.x + x0, a.y + a.h - len, (x1 - x0).max(1), len), col, 150);
                    }
                }
                let (tdiv, delay) = self.view();
                let spp = tdiv as f64 / (n as f64 / HDIV as f64);
                let mt = (mean - n as f64 / 2.0) * spp + delay as f64;
                format!("Hist CH{} @ level  mean {}  \u{3c3} {}", c + 1, si(mt, "s"), si(sd * spp, "s"))
            }
        };
        cv.no_clip();
        let f = self.fonts();
        // above the decoder band and the time labels, right of the volt labels
        let r = Rect::new(a.x + 14 + f.s.width("-0.00 mV"), a.y + a.h - 76, f.s.width(&txt) + 16, 20);
        cv.round(r, 4, PANEL_BG, 230);
        cv.text_in(&f.s, r, 1, &txt, col);
    }

    pub(super) fn math_scale_text(&self) -> String {
        Self::math_scale_text_of(&self.m.math_ch(self.m.math_sel))
    }

    fn math_scale_text_of(c: &MathCh) -> String {
        if c.op == MathOp::Fft {
            format!("{} dB", FFT_DB[c.scale.min(FFT_DB.len() - 1)])
        } else {
            let sc = math_scales();
            si_short(sc[c.scale.min(sc.len() - 1)], c.op.unit())
        }
    }

    /// Test/legacy adapter; production rendering reuses immutable timed results.
    #[cfg(test)]
    pub(super) fn math_source(&self, s: usize, below: usize) -> Vec<f32> {
        self.math_source_record(s, below).map_or_else(Vec::new, |w| w.v.as_ref().clone())
    }

    /// The waveform of math channel `k` (empty for FFT/Off or missing sources).
    pub(super) fn math_wave_of(&self, k: usize) -> Vec<f32> {
        if self.m.math_ch(k).op == MathOp::Fft {
            return Vec::new();
        }
        self.math_record(k).ok().flatten().map_or_else(Vec::new, |w| w.v.as_ref().clone())
    }

    /// The waveform of the selected math channel.
    #[cfg(test)]
    pub(super) fn math_wave(&self) -> Vec<f32> {
        self.math_wave_of(self.m.math_sel)
    }

    /// Picks the math scale so that the waveform spans about 6 divisions.
    pub(super) fn math_autoscale(&mut self) {
        self.math_fit(self.m.math_sel);
    }

    /// Fits the scale and offset of math channel `k` to its result. A result that is flat (a source minus itself) is
    /// not stretched to nanovolts: the scale stays within 1/100 of the V/div of the sources it was made from.
    pub(super) fn math_fit(&mut self, k: usize) {
        let mut c = self.m.math_ch(k);
        if c.op == MathOp::Fft {
            (c.scale, c.offset) = (3, 0.0);
        } else {
            let w = self.math_wave_of(k);
            let (mn, mx) =
                w.iter().filter(|x| x.is_finite()).fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
            let sc = math_scales();
            if mx < mn {
                c.scale = sc.iter().position(|&x| x >= 1.0).unwrap_or(0);
            } else {
                let floor = if c.op.unit() == "V" { self.math_source_vpd(k) as f64 / 100.0 } else { 0.0 };
                let want = ((mx - mn) as f64 / 6.0).max(1e-9).max(floor);
                c.scale = sc.iter().position(|&x| x >= want).unwrap_or(sc.len() - 1);
                c.offset = -((mx + mn) as f64 / 2.0 / sc[c.scale]) as f32;
            }
        }
        if k == self.m.math_sel {
            (self.m.math_scale, self.m.math_offset) = (c.scale, c.offset);
        }
        self.m.maths[k] = c;
    }

    pub(super) fn draw_math(&self, cv: &mut Canvas) {
        // all traces first, then the labels on top of them
        let on: Vec<usize> = (0..NMATH)
            .filter(|&k| self.m.math_ch(k).op != MathOp::Off && self.math_win_of(k) == self.plot.cur_win)
            .collect();
        let labels: Vec<Option<String>> =
            on.iter().enumerate().map(|(row, &k)| self.draw_math_ch(cv, k, row as i32)).collect();
        let f = self.fonts();
        for (row, (label, &k)) in labels.iter().zip(&on).enumerate() {
            if let Some(label) = label {
                // top right, under the status line, one row per math channel (the results panel goes below them)
                let r = Rect::new(
                    self.plot.area.x + self.plot.area.w - 12 - f.s.width(label) - 16,
                    self.plot.area.y + 22 + 26 * row as i32,
                    f.s.width(label) + 16,
                    20,
                );
                cv.round(r, 4, rgb(0x1c, 0x16, 0x28), 230);
                cv.text_in(&f.s, r, 1, label, MATH_COLORS[k % MATH_COLORS.len()]);
            }
        }
    }

    /// Draws the trace of math channel `k`; returns its label.
    fn draw_math_ch(&self, cv: &mut Canvas, k: usize, row: i32) -> Option<String> {
        let c = self.m.math_ch(k);
        let col = MATH_COLORS[k % MATH_COLORS.len()];
        let f = self.fonts();
        // the last stacked lane keeps its time labels clear: the trace is clipped above them
        let mut lane = self.plot.area;
        if self.stacked() && self.plot.cur_win + 1 == self.plot.wins.len() {
            lane.h = (lane.h - (f.s.size + 8)).max(f.s.size);
        }
        cv.set_clip(lane);
        let ppy = self.plot.area.h as f32 / VDIV;
        let cy = self.plot.area.y as f32 + self.plot.area.h as f32 / 2.0;

        let label = if c.op == MathOp::Fft {
            let Some(w) = self.math_record(k).ok().flatten() else {
                cv.no_clip();
                return None;
            };
            let df = w.dt;
            let db = w.v.as_ref();
            if db.is_empty() {
                cv.no_clip();
                return None;
            }
            let dbdiv = FFT_DB[c.scale.min(FFT_DB.len() - 1)];
            // top of the grid = +20 dBV (moved by the offset), x: 0 .. fs/2 over the full width
            let top = 20.0 + c.offset * dbdiv;
            let span = df * (db.len() - 1) as f64;
            let n = db.len();
            let mut prev: Option<(i32, i32)> = None;
            for px in 0..self.plot.area.w {
                // peak of the bins that fall into this pixel column
                let k0 = (px as usize * (n - 1)) / self.plot.area.w as usize;
                let k1 = (((px + 1) as usize * (n - 1)) / self.plot.area.w as usize).max(k0 + 1).min(n);
                let v = db[k0..k1].iter().copied().fold(f32::MIN, f32::max);
                let (zoom, center) = self.zoom_vertical_transform(self.plot.area);
                let d = VDIV * 0.5 - (top - v) / dbdiv;
                let y = (cy - (d - center) * zoom * ppy).round() as i32;
                let x = self.plot.area.x + px;
                if let Some((x0, y0)) = prev {
                    cv.line(x0, y0, x, y, col);
                }
                prev = Some((x, y));
            }
            format!(
                "M{} FFT {} {}  {} dB/div  {}/div",
                k + 1,
                src_name(c.a),
                c.win.name(),
                dbdiv,
                si_short(span / HDIV as f64, "Hz")
            )
        } else {
            let result = self.math_record(k);
            let Some(rec) = result.as_ref().ok().and_then(|w| w.as_ref()) else {
                cv.no_clip();
                let status = match result {
                    Err(e) => e,
                    _ => "Computing",
                };
                cv.text(
                    &f.s,
                    self.plot.area.x + 60,
                    self.plot.area.y + 40 + 18 * row,
                    &format!("M{}: {}", k + 1, status),
                    col,
                );
                return None;
            };
            let w = rec.v.as_ref();
            let sc = math_scales();
            let vpd = sc[c.scale.min(sc.len() - 1)] as f32;
            let tdiv = self.view().0 as f64;
            let ppx = self.plot.area.w as f64 / HDIV as f64;
            let mut prev: Option<(i32, i32)> = None;
            let left = self.view().1 as f64 - tdiv * HDIV as f64 * 0.5;
            let right = left + tdiv * HDIV as f64;
            let at = |t: f64| ((t - rec.t0) / rec.dt).floor().clamp(-1.0, w.len() as f64) as isize;
            let begin = (at(left) - 1).clamp(0, w.len() as isize) as usize;
            let end = (at(right) + 2).clamp(0, w.len() as isize) as usize;
            let step = ((end - begin) / (self.plot.area.w as usize * 2)).max(1);
            for i in (begin..end).step_by(step) {
                let v = w[i];
                if !v.is_finite() {
                    prev = None;
                    continue;
                }
                let t = rec.t0 + i as f64 * rec.dt - self.view().1 as f64;
                let x = (self.plot.area.x as f64 + self.plot.area.w as f64 / 2.0 + t / tdiv * ppx).round() as i32;
                let (zoom, center) = self.zoom_vertical_transform(self.plot.area);
                let y = (cy - (v / vpd + c.offset - center) * zoom * ppy).clamp(-1e4, 1e4).round() as i32;
                if let Some((x0, y0)) = prev {
                    cv.line(x0, y0, x, y, col);
                }
                prev = Some((x, y));
            }
            let name = if c.op == MathOp::Expr { self.m.math_expr[k].clone() } else { c.op.formula(c.a, c.b) };
            format!("M{} = {}  {}/div", k + 1, name, Self::math_scale_text_of(&c))
        };
        cv.no_clip();
        Some(label)
    }

    pub(super) fn y_of_volts(&self, c: usize, v: f32) -> i32 {
        let a = self.ch_area(c);
        let ch = &self.m.ch[c];
        self.zoom_y_of_divs(a, (v + ch.offset) / ch.volts_per_div()).round() as i32
    }

    pub(super) fn y_of_wave(&self, id: WaveId, v: f32) -> i32 {
        match id {
            WaveId::Ch(c) => self.y_of_volts(c, v),
            WaveId::Ref(i) => {
                let a = self.plot.area;
                let Some(r) = self.refs.get(i).and_then(|x| x.as_ref()) else {
                    return a.y;
                };
                self.zoom_y_of_divs(a, (v + r.offset) / r.vpd.max(1e-12)).round() as i32
            }
        }
    }

    pub(super) fn x_of_time(&self, t: f32) -> i32 {
        let a = self.plot.area;
        let (tdiv, delay) = self.view();
        let ppd = a.w as f32 / HDIV;
        (a.x as f32 + a.w as f32 / 2.0 + (t - delay) / tdiv * ppd).round() as i32
    }

    /// Decoded packets of the last record (times in s relative to the trigger), of the decoder the menu edits.
    #[cfg(test)]
    pub(super) fn decode_packets(&self) -> Result<Vec<oos_dsp::decode::Packet>, &'static str> {
        self.decode_packets_of(&self.m).map(|packets| packets.as_ref().clone())
    }

    /// The same for the decoder that m is the view of; cached per record and settings, up to four decoders run every frame.
    pub(super) fn decode_packets_of(
        &self,
        m: &Model,
    ) -> Result<std::sync::Arc<Vec<oos_dsp::decode::Packet>>, &'static str> {
        use std::hash::{Hash, Hasher};
        if Self::decoder_uses_refs(m) || (!m.dec_on_la() && m.deskew_ch.iter().any(|&shift| shift != 0.0)) {
            return self.reference_decode_packets(m);
        }
        let Some(rec) = &self.last[0] else { return Ok(std::sync::Arc::new(Vec::new())) };
        if m.dec_on_la() {
            return self.la_packets(m, rec).map(std::sync::Arc::new);
        }
        // the screen record is peak-detect or resampled; the acquisition's own samples where the record has them
        if self.decoder_uses_raw(m) {
            return self.reference_decode_packets(m);
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.counts.wfm_total, self.history.pos, rec.t0.to_bits(), rec.dt.to_bits()).hash(&mut h);
        for c in 0..NCH {
            (rec.ch[c].as_ptr() as usize, rec.ch[c].len(), m.ch[c].volts_per_div().to_bits()).hash(&mut h);
        }
        format!("{:?}", m.dec_cfg()).hash(&mut h);
        let key = h.finish();
        if let Some(p) = self.decoded.get(key) {
            return Ok(std::sync::Arc::new(p));
        }
        let mut p = crate::trig::decode(m, std::array::from_fn(|c| &rec.ch[c][..]), rec.dt)?;
        for x in p.iter_mut() {
            x.t0 = rec.t0 + x.t0 * rec.dt;
            x.t1 = rec.t0 + x.t1 * rec.dt;
        }
        self.decoded.put(key, &p);
        Ok(std::sync::Arc::new(p))
    }

    /// All decoders that are on: bands at the bottom of the plot, or rows under the lanes for those on LA lines.
    pub(super) fn draw_decode(&self, cv: &mut Canvas) {
        if self.last[0].is_none() && !self.refs.iter().any(Option::is_some) {
            return;
        }
        let (mut band, mut lane) = (0, 0);
        let bottom = self.plot.area.y + self.plot.area.h - 46;
        for k in 0..DECODERS {
            if !self.m.dec_is_on(k) {
                continue;
            }
            let m = self.m.dec_view(k);
            // rows under the lanes go with the LA's window, bands with their source's
            let win = self.dec_band_win(k);
            let row = if win.is_none() { self.la_dec_row_n(lane) } else { None };
            if row.is_some() {
                lane += 1;
            }
            if win.unwrap_or(self.la_win_of()) != self.plot.cur_win {
                continue;
            }
            let y = match row {
                Some(r) => r.y + 3,
                None => {
                    band += 1;
                    bottom - (band - 1) * DEC_BAND
                }
            };
            self.draw_decoder(cv, &m, k, y, row);
        }
    }

    /// One decoder's band at height y; row = its place under the lanes.
    fn draw_decoder(&self, cv: &mut Canvas, m: &Model, k: usize, y: i32, row: Option<Rect>) {
        use oos_dsp::decode::Kind;
        let f = self.fonts();
        let a = self.plot.area;
        let col = if m.dec_on_la() {
            la::line_color(m.dec_la[0].unwrap_or(0))
        } else {
            decode_wave(m.dec_src).map_or(DIM, |source| self.wave_color(source))
        };
        // rows under the lanes span all windows, like the lanes
        let a = if row.is_some() { self.lane_area() } else { a };
        let h = 20;
        cv.set_clip(row.unwrap_or(a));
        if row.is_none() {
            cv.blend(Rect::new(a.x, y - 3, a.w, h + 6), BG, 200);
        }
        let name = if m.dec_on_la() {
            format!("{} {}", k + 1, m.dec_proto.name())
        } else {
            format!("{} {} {}", k + 1, m.dec_proto.name(), decode_source_name(m.dec_src))
        };
        cv.text(&f.sb, a.x + 4, y + 15, &name, DIM);
        let packets = match self.decode_packets_of(m) {
            Ok(p) => p,
            Err(e) => {
                cv.text(&f.s, a.x + 16 + f.sb.width(&name), y + 15, e, DIM);
                cv.no_clip();
                return;
            }
        };
        let (tdiv, delay) = self.view();
        let ppx = a.w as f64 / HDIV as f64;
        let xt = |t: f64| (a.x as f64 + a.w as f64 / 2.0 + (t - delay as f64) / tdiv as f64 * ppx).round() as i32;
        let left = a.x + 10 + f.sb.width(&name);
        let mut x_prev_end = left;
        for p in packets.iter() {
            let (x0, x1) = (xt(p.t0), xt(p.t1));
            if x1 < left || x0 > a.x + a.w {
                continue;
            }
            let event = matches!(p.kind, Kind::Field(l) if l.starts_with('!'));
            if let Some(tag) = packet_tag(m, p) {
                let r = Rect::new(x0 - f.s.width(tag) / 2 - 3, y + 3, f.s.width(tag) + 6, h - 6);
                let bad = p.kind == Kind::Stop || ((p.kind == Kind::Ack || event) && !p.ok);
                cv.round(r, 3, if bad { mix(BG, RED, 140) } else { mix(BG, GREEN, 140) }, 255);
                cv.text_in(&f.s, r, 1, tag, TEXT);
                continue;
            }
            let x0 = x0.max(x_prev_end + 1);
            if x1 - x0 < 3 {
                continue;
            }
            let is_addr =
                matches!(p.kind, Kind::Addr { .. } | Kind::Id { .. } | Kind::Word { cmd: true } | Kind::Field(_));
            let base = if is_addr { ACCENT } else { col };
            let fill = if p.ok { scale(base, 70) } else { rgb(0x70, 0x18, 0x18) };
            cv.fill(Rect::new(x0 + 3, y, x1 - x0 - 6, h), fill);
            cv.line(x0, y + h / 2, x0 + 3, y, base);
            cv.line(x0, y + h / 2, x0 + 3, y + h - 1, base);
            cv.line(x1, y + h / 2, x1 - 3, y, base);
            cv.line(x1, y + h / 2, x1 - 3, y + h - 1, base);
            cv.hline(x0 + 3, x1 - 3, y, base);
            cv.hline(x0 + 3, x1 - 3, y + h - 1, base);
            let txt = packet_text(m, p);
            if f.s.width(&txt) + 6 < x1 - x0 {
                cv.text_in(&f.s, Rect::new(x0, y, x1 - x0, h), 1, &txt, TEXT);
            }
            x_prev_end = x1;
        }
        // threshold line on the source trace (LA lines have no analog level)
        if !m.dec_on_la() && m.dec_proto != Proto::CPHY {
            let Some(source) = decode_wave(m.dec_src) else {
                cv.no_clip();
                return;
            };
            let ty = self.y_of_wave(source, m.dec_thr);
            cv.dashed_hline(a.x + 26, a.x + a.w - 26, ty, 2, 6, scale(col, 160));
            if m.dec_proto.module().is_some_and(|p| p.key() == "Arinc429") {
                // the LO level of ARINC 429
                let ty = self.y_of_wave(source, -m.dec_thr);
                cv.dashed_hline(a.x + 26, a.x + a.w - 26, ty, 2, 6, scale(col, 160));
            }
        }
        cv.no_clip();
    }
}
