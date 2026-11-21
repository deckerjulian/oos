// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The eye page at work: the samples of the eye source and its clock, the update per record and the refresh of
//! `Page`, the sliding column, Side / Under placement and the one-press eye setup. A new input or control of the page
//! goes here.

use super::*;

/// What an eye is made for (`eye_key`): source, clock, scale, offset, symbol rate and plot size.
type EyeKey = (u8, usize, u8, usize, u32, u32, u64, i32, i32);

impl Ui {
    /// Generated and imported references are immutable records. Eye analysis reads
    /// the whole record even when ordinary measurements follow the zoom window.
    pub(super) fn eye_samples(&self, rec: &Record, id: WaveId) -> Option<WaveSamp> {
        match id {
            WaveId::Ref(i) => {
                let r = self.refs.get(i)?.as_ref()?;
                (r.v.len() >= 2 && r.dt.is_finite() && r.dt > 0.0).then(|| WaveSamp {
                    v: r.v.as_ref().clone(),
                    t0: self.source_time_origin(id, r.t0),
                    dt: r.dt,
                })
            }
            WaveId::Ch(c) => {
                let (values, dt, t0) = self.eye_channel_samples(rec, c)?;
                Some(WaveSamp { v: values.to_vec(), dt, t0 })
            }
        }
    }

    /// Eye timing always uses the entire retained native acquisition, never the
    /// interpolated screen record or its current measurement/view gate.
    pub(super) fn eye_channel_samples<'a>(&self, rec: &'a Record, c: usize) -> Option<(&'a [f32], f64, f64)> {
        if c >= n_ch() || !self.m.ch[c].on {
            return None;
        }
        let (values, dt, t0) = if let Some(raw) = &rec.raw {
            if raw.frame.envelope || self.m.acq == AcqMode::HiRes {
                return None;
            }
            let values = raw.frame.ch.get(c)?;
            (values.as_slice(), raw.frame.dt, raw.t0 - raw.i0 as f64 * raw.frame.dt)
        } else {
            (rec.ch.get(c)?.as_slice(), rec.dt, rec.t0)
        };
        (values.len() >= 2 && dt.is_finite() && dt > 0.0 && t0.is_finite()).then_some((
            values,
            dt,
            self.source_time_origin(WaveId::Ch(c), t0),
        ))
    }

    fn eye_input_problem(&self, rec: &Record, id: WaveId) -> Option<&'static str> {
        if let WaveId::Ch(c) = id {
            if c < n_ch() && self.m.ch[c].on && rec.raw.is_some() && self.m.acq == AcqMode::HiRes {
                return Some("Native HiRes samples are not retained for Eye/Jitter; use Normal acquisition");
            }
            if c < n_ch() && self.m.ch[c].on && rec.raw.as_ref().is_some_and(|raw| raw.frame.envelope) {
                return Some("Envelope acquisition has no native time samples for Eye/Jitter");
            }
        }
        None
    }

    fn eye_source_ready(&self, rec: &Record, id: WaveId) -> bool {
        match id {
            WaveId::Ref(i) => self
                .refs
                .get(i)
                .and_then(Option::as_ref)
                .is_some_and(|r| r.v.len() >= 2 && r.dt.is_finite() && r.dt > 0.0),
            WaveId::Ch(i) => self.eye_channel_samples(rec, i).is_some(),
        }
    }

    /// Data and clock share the finest native grid within their physical overlap.
    pub(super) fn eye_pair(&self, rec: &Record, id: WaveId, clock: WaveId) -> Option<(Vec<f32>, Vec<f32>, f64, f64)> {
        let a = self.eye_samples(rec, id)?;
        let b = self.eye_samples(rec, clock)?;
        let (dt, t0, n) = eye_common_grid(&a, &b)?;
        Some((eye_on_grid(a, t0, dt, n), eye_on_grid(b, t0, dt, n), dt, t0))
    }

    /// Called after replacing a generated reference, including a same-grid record.
    pub(in crate::ui) fn invalidate_reference_eye(&mut self) {
        self.azr.eye = None;
        self.azr.eye_ref = None;
        self.azr.eye_sim_revision = None;
        self.azr.eye_res = None;
        self.azr.page = Page::default();
        self.azr.eye_auto.invalidate();
    }

    /// Hash every reference sample before making copies or computing statistics.
    /// Live companion waves still accumulate once per live capture as before.
    fn eye_reference_fingerprint(&self, id: WaveId, clock: Option<WaveId>) -> Option<(usize, u64)> {
        let WaveId::Ref(slot) = id else { return None };
        let mut h = 0u64;
        for source in [Some(id), clock, self.m.eye_rail].into_iter().flatten() {
            let WaveId::Ref(i) = source else { return None };
            let Some(r) = self.refs.get(i).and_then(Option::as_ref) else {
                h = h.rotate_left(13) ^ i as u64 ^ u64::MAX;
                continue;
            };
            let t0 = self.source_time_origin(source, r.t0);
            let data_identity = if self.session_is_sim && i == self.m.sim_slot {
                self.replay.sim_revision ^ r.dt.to_bits() ^ t0.to_bits()
            } else {
                ref_fp(&r.v, r.dt, t0)
            };
            h = h.rotate_left(13) ^ data_identity ^ i as u64;
            h = h.rotate_left(13) ^ r.vpd.to_bits() as u64 ^ ((r.offset.to_bits() as u64) << 32);
        }
        for setting in [self.m.eye_ber, self.m.eye_pn_lo, self.m.eye_pn_hi, self.m.eye_notch] {
            h = h.rotate_left(13) ^ setting.to_bits();
        }
        h ^= (self.m.eye_npj as u64) | ((self.m.eye_hp as u64) << 1) | ((self.m.eye_from_record as u64) << 2);
        Some((slot, h))
    }

    fn eye_scale(&self, id: WaveId) -> (f32, f32) {
        match id {
            WaveId::Ch(c) => {
                let ch = &self.m.ch[c.min(NCH - 1)];
                (ch.volts_per_div(), ch.offset)
            }
            // an empty slot does not reach the page (`eye_source_ready`); any scale for one
            WaveId::Ref(i) => self.refs[i].as_ref().map_or((1.0, 0.0), |r| (r.vpd, r.offset)),
        }
    }

    /// Eye page, per record. Clock, when set, is the strobe: its edges fold the source (DQ vs DQS).
    pub(in crate::ui) fn eye_update(&mut self, rec: &Record) {
        if self.eye_input_blocked(rec) {
            return;
        }
        if self.eye_auto_update(rec) {
            return;
        }
        let id = self.m.eye_src;
        let clk_id = self.m.eye_b;
        if !self.eye_source_ready(rec, id) {
            self.azr.page.status = [missing_wave(id), "Switch the source on".into()];
            return;
        }
        if let Some(b) = clk_id {
            if !self.eye_source_ready(rec, b) {
                self.azr.page.status = [missing_wave(b), "Switch the clock on".into()];
                return;
            }
        }
        let (vpd, offset) = self.eye_scale(id);
        let lay = layout(self.eye_page_rect(), self.full_on());
        let (pw, ph) = (lay.plot.w.max(64), lay.plot.h.max(64));
        let tag = match id {
            WaveId::Ch(c) => (0u8, c),
            WaveId::Ref(i) => (1, i),
        };
        let ctag = match clk_id {
            None => (0u8, 0usize),
            Some(WaveId::Ch(c)) => (1, c),
            Some(WaveId::Ref(i)) => (2, i),
        };
        let key = (tag.0, tag.1, ctag.0, ctag.1, vpd.to_bits(), offset.to_bits(), self.m.eye_rate.to_bits(), pw, ph);
        let fp = self.eye_reference_fingerprint(id, clk_id);
        if fp.is_some() && fp == self.azr.eye_ref && self.azr.eye.is_some() && self.azr.eye_key == key {
            return;
        }
        let sim_capture = self.session_is_sim && id == WaveId::Ref(self.m.sim_slot) && self.replay.sim_revision > 0;
        let reference_changed = fp.is_some() && fp != self.azr.eye_ref && !sim_capture;
        let (v, clk_v, dt, t0) = if let Some(b) = clk_id {
            let Some((v, clock, dt, t0)) = self.eye_pair(rec, id, b) else {
                self.azr.eye = None;
                self.azr.eye_res = None;
                self.azr.page = Page::default();
                self.azr.page.status =
                    ["No overlapping Eye data/clock interval".into(), "Check source timing and deskew".into()];
                return;
            };
            (v, Some(clock), dt, t0)
        } else {
            let Some(s) = self.eye_samples(rec, id) else {
                self.azr.page.status = [missing_wave(id), "Switch the source on".into()];
                return;
            };
            (s.v, None, s.dt, s.t0)
        };
        let recov = clk_v.as_deref().unwrap_or(&v);
        let recov_id = clk_id.unwrap_or(id);
        let st = match recov_id {
            WaveId::Ch(c) if clk_id.is_none() && rec.raw.is_none() => self.measured.stats[c],
            _ => stats(recov, dt),
        };
        let (clk_vpd, _) = self.eye_scale(recov_id);
        // A strobe idles near one rail between bursts; histogram top/base sit on that rail and the
        // comparator chatters. Peak-to-peak with a wide hysteresis keeps the burst edges.
        let (thr, hyst) = self.eye_threshold(&st, clk_id, clk_vpd);
        let mut rate = (self.m.eye_rate > 0.0).then_some(self.m.eye_rate);
        // the rate is of the record it was set or found on. At a slower time base its bit is shorter than two
        // samples: a rate found from a record is found again from this one, a rate that was set is said to need
        // a faster record (as `automatic::infer` says it). Recovered with it, the clock had a bit far below a
        // sample, and the page took the memory of the scope (seen on a unit)
        if rate.is_some_and(|r| 1.0 / r < 2.0 * dt) {
            if self.m.eye_from_record {
                self.m.eye_rate = 0.0;
                rate = None;
            } else {
                // the eye of the faster time base goes, and the page says why (the old eye stayed, with nothing said)
                self.azr.eye = None;
                self.azr.eye_res = None;
                self.azr.page = Page::default();
                self.azr.page.status = [
                    format!("{} needs a faster time base", si_short(self.m.eye_rate, "b/s")),
                    "Under 2 samples a bit in this record".into(),
                ];
                return;
            }
        }
        // An idle live bus keeps its accumulated eye. A replacement reference
        // without edges must not display the previous reference's results.
        let recovered = if self.azr.eye_auto.enabled && clk_id.is_none() {
            eye::recover_clock_pll(recov, dt, thr, hyst, rate, 0.0, true)
        } else {
            eye::recover_clock(recov, dt, thr, hyst, rate)
        };
        let Some(clk) = recovered else {
            if reference_changed {
                self.invalidate_reference_eye();
            }
            self.azr.page.status[1] = format!("Under 3 edges on {} in the last record", self.wave_name(recov_id));
            return;
        };
        self.eye_restart(&v, dt, key, reference_changed, pw, ph);
        // made or kept by eye_restart
        let Some(range) = self.azr.eye.as_ref().map(|e| (e.v_min, e.v_max)) else { return };
        let last = self.azr.page.ui.mean();
        if last.is_finite() && (clk.period / last - 1.0).abs() > UI_CHANGE {
            if let Some(eye) = self.azr.eye.as_mut() {
                eye.clear();
            }
            self.azr.page = Page { vstep: self.azr.page.vstep, ..Page::default() };
        }
        let (fold, data) =
            if clk_id.is_some() { Self::eye_strobe_fold(&v, dt, &clk, range) } else { (clk.clone(), v.clone()) };
        if sim_capture && self.azr.eye_sim_revision == Some((self.m.sim_slot, self.replay.sim_revision)) {
            let tie = eye::tie(&clk.edges, &clk);
            self.azr.eye_ref = fp;
            self.eye_refresh(&clk, &tie, (&v, dt, t0), &lay, rec);
            return;
        }
        if sim_capture {
            self.azr.eye_sim_revision = Some((self.m.sim_slot, self.replay.sim_revision));
        }
        if let Some(eye) = self.azr.eye.as_mut() {
            eye.accumulate(&data, dt, &fold);
        }
        self.azr.eye_ref = fp;
        let tie = eye::tie(&clk.edges, &clk);
        self.azr.page.add(&tie, &clk);
        if fp.is_some() || self.now - self.azr.page.t >= REFRESH {
            self.azr.page.t = self.now;
            self.eye_refresh(&clk, &tie, (&v, dt, t0), &lay, rec);
        }
    }

    /// An input the eye cannot read (native HiRes, envelope): the page is cleared and says why; true then.
    fn eye_input_blocked(&mut self, rec: &Record) -> bool {
        for id in [Some(self.m.eye_src), self.m.eye_b, self.m.eye_rail].into_iter().flatten() {
            if let Some(problem) = self.eye_input_problem(rec, id) {
                self.azr.eye = None;
                self.azr.eye_res = None;
                self.azr.eye_auto.summary = None;
                self.azr.eye_auto.invalidate();
                self.azr.page = Page::default();
                self.azr.page.status = [self.wave_name(id), problem.into()];
                return true;
            }
        }
        false
    }

    /// The threshold and hysteresis that recover the clock: the auto setup's, the strobe's swing or the source's level.
    fn eye_threshold(&self, st: &Stats, clk_id: Option<WaveId>, clk_vpd: f32) -> (f32, f32) {
        if let Some(summary) = self.azr.eye_auto.summary.as_ref().filter(|_| self.azr.eye_auto.enabled) {
            (summary.threshold, summary.hysteresis)
        } else if clk_id.is_some() {
            let (lo, hi) = (st.vmin, st.vmax);
            (0.5 * (lo + hi), (0.2 * (hi - lo)).max(0.1 * clk_vpd))
        } else {
            let thr = if st.valid { 0.5 * (st.top + st.base) } else { 0.5 * (st.vmax + st.vmin) };
            (thr, 0.1 * clk_vpd)
        }
    }

    /// A new eye when there is none, its key changed, the signal left or shrank in its range or the reference changed.
    fn eye_restart(&mut self, v: &[f32], dt: f64, key: EyeKey, reference_changed: bool, pw: i32, ph: i32) {
        let data_st = stats(v, dt);
        let left = self.azr.eye.as_ref().is_some_and(|e| {
            let (lo, hi) = (e.v_min as f64, e.v_max as f64);
            let (span, swing) = (hi - lo, (data_st.vmax - data_st.vmin) as f64);
            data_st.vmax as f64 > hi + EYE_OUT * span
                || (data_st.vmin as f64) < lo - EYE_OUT * span
                || swing < EYE_SHRUNK * span
        });
        if self.azr.eye.is_none() || self.azr.eye_key != key || left || reference_changed {
            // the eye fills its plot whatever the scale of the channel; the signal itself sets the range
            let (lo, hi) = v
                .iter()
                .filter(|x| x.is_finite())
                .fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x as f64), b.max(x as f64)));
            let (lo, hi, step) = eye_range(lo, hi);
            self.azr.eye = Some(eye::Eye::new(pw as usize, ph as usize, 2, lo as f32, hi as f32));
            self.azr.eye_key = key;
            self.azr.eye_sim_revision = None;
            self.azr.eye_res = None;
            self.azr.page = Page { vstep: step, ..Page::default() };
        }
    }

    /// With a strobe: the record gated around its edges, folded on the strobe or on it shifted by half a period,
    /// whichever gives the higher eye.
    /// `range`: the voltage range of the page's eye (`Eye::v_min`, `v_max`).
    fn eye_strobe_fold(v: &[f32], dt: f64, clk: &eye::Clock, range: (f32, f32)) -> (eye::Clock, Vec<f32>) {
        let gated = eye::gate_around_clock(v, dt, clk, 1.5);
        let height = |c: &eye::Clock| {
            let mut t = eye::Eye::new(48, 48, 2, range.0, range.1);
            t.accumulate(&gated, dt, c);
            let h = t.measure(c.period).height;
            if h.is_finite() {
                h
            } else {
                f64::NEG_INFINITY
            }
        };
        let shifted = eye::center_strobe(clk);
        let fold = if height(&shifted) > height(clk) { shifted } else { clk.clone() };
        (fold, gated)
    }

    /// The heavy part of the page, from the pool and the last record. `wave` is the selected eye source: its
    /// samples (on the common grid when a clock source is chosen), `dt` and `t0`.
    fn eye_refresh(&mut self, clk: &eye::Clock, tie: &[f64], wave: (&[f32], f64, f64), lay: &Layout, rec: &Record) {
        let (wave, dt, t0) = wave;
        let clock = if let Some(b) = self.m.eye_b {
            format!("clock from {}", self.wave_name(b))
        } else if self.m.eye_from_record {
            "bit rate from the record".into()
        } else if self.m.eye_rate > 0.0 {
            "clock fitted near the set rate".into()
        } else {
            "clock fitted to the edges".into()
        };
        let source = if let WaveId::Ref(i) = self.m.eye_src {
            format!(
                "{} · {}",
                self.wave_name(self.m.eye_src),
                if self.session_is_sim && i == self.m.sim_slot { "live SIM captures" } else { "fixed full record" }
            )
        } else {
            self.wave_name(self.m.eye_src)
        };
        let title = format!("{} \u{b7} {} \u{b7} {}", source, si(1.0 / clk.period, "b/s"), clock);
        let Some(e) = self.azr.eye.as_ref() else { return };
        let meas = e.measure(clk.period);
        let p = &mut self.azr.page;
        p.paint(e);
        p.levels = (meas.one.is_finite() && meas.zero.is_finite()).then_some((meas.one, meas.zero));
        let mut sorted = p.pool.clone();
        sorted.sort_by(f64::total_cmp);
        let mut j = eye::jitter(&sorted);
        // steps between edges only mean something inside a record: those come from the sums
        let rms = |s: (f64, u64)| if s.1 == 0 { f64::NAN } else { (s.0 / s.1 as f64).sqrt() };
        (j.period_jitter_rms, j.c2c_rms) = (rms(p.step), rms(p.step2));
        let trend = eye::tie_trend(&clk.edges, tie, clk);
        let (_, plots) = charts_shown(lay, self.eye_col.big);
        p.chart_trend(&trend, clk, plots[0].w as usize);
        p.chart_hist(&j);
        let (pj, why) = p.chart_spec(&trend, clk, plots[2].w as usize);
        p.chart_tub(&sorted, &j, clk.period, plots[3].w as usize);
        p.text_jitter(&j, pj, &why);
        p.text_eye(e, &meas, clk.period, dt);
        p.text_status(&j);
        p.title = title;
        let means = self.azr.page.means.clone();
        let input = FactInput { clk, tie, trend: &trend, j: &j, meas: &meas, wave, dt, means: &means };
        let (measured, model, eye_model) = self.eye_facts(input, t0, rec);
        self.azr.page.measured.extend(measured);
        self.azr.page.model.extend(model);
        self.azr.page.eye_model.extend(eye_model);
        self.azr.eye_res = Some((meas, j, clk.period));
        self.eye_auto_finish_status();
    }

    /// Gather optional companions; the DSP/figure builder is also used off-thread.
    fn eye_facts(&mut self, input: FactInput, t0: f64, rec: &Record) -> (Vec<Cell>, Vec<Cell>, Vec<Cell>) {
        let (wave, dt) = (input.wave, input.dt);
        let settings = FactSettings::from(&self.m);
        let rail = self.m.eye_rail.and_then(|id| self.eye_samples(rec, id)).map(|s| eye_on_grid(s, t0, dt, wave.len()));
        let companion = self.m.eye_b.and_then(|id| self.eye_pair(rec, self.m.eye_src, id));
        populate_eye_facts(
            &mut self.azr.page,
            settings,
            input,
            rail.as_ref().map(|v| (v.as_slice(), dt)),
            companion.as_ref().map(|(a, b, dt, _)| (a.as_slice(), b.as_slice(), *dt)),
        )
    }

    /// Where the column sits this frame: 0 has left the screen, 1 is home.
    pub(super) fn eye_column_open(&self) -> f32 {
        let u = ((self.now - self.eye_col.t) / SLIDE).clamp(0.0, 1.0) as f32;
        let e = u * u * (3.0 - 2.0 * u);
        self.eye_col.from + (self.eye_col.to - self.eye_col.from) * e
    }

    pub(in crate::ui) fn eye_tab(&self) -> Rect {
        eye_tab_at(self.eye_page_rect(), &placed(self.eye_page_rect(), self.eye_column_open()))
    }

    /// Slide the column the other way. A tap in the middle of a move reverses it.
    fn eye_slide(&mut self) {
        self.eye_col.from = self.eye_column_open();
        self.eye_col.to = if self.eye_col.to > 0.5 { 0.0 } else { 1.0 };
        self.eye_col.t = self.now;
    }

    /// The column tab, or a plot. A plot that is already open goes back to the four. Full screen has neither.
    pub(in crate::ui) fn eye_column_tap(&mut self, x: i32, y: i32) -> bool {
        let area = self.eye_page_rect();
        if self.full_on() || !area.contains(x, y) || layout(area, false).charts[0].w == 0 {
            return false;
        }
        if self.eye_tab().contains(x, y) {
            self.eye_slide();
            return true;
        }
        if self.eye_column_open() < 0.5 {
            return false;
        }
        let l = placed(self.eye_page_rect(), self.eye_column_open());
        let (charts, _) = charts_shown(&l, self.eye_col.big);
        if let Some(k) = self.eye_col.big {
            if charts[k as usize].contains(x, y) {
                self.eye_col.big = None;
                self.azr.page.t = f64::NEG_INFINITY;
                return true;
            }
            return false;
        }
        for (k, r) in charts.iter().enumerate() {
            if r.contains(x, y) {
                self.eye_col.big = Some(k as u8);
                self.azr.page.t = f64::NEG_INFINITY;
                return true;
            }
        }
        false
    }

    pub(in crate::ui) fn eye_pane_close(&self) -> Rect {
        let a = self.eye_pane_rect();
        let full = self.full_btn().0;
        let right = if full.y < a.y + workspace::HEADER && full.y + full.h > a.y && full.x > a.x && full.x < a.x + a.w {
            full.x - 8
        } else {
            a.x + a.w - 2
        };
        Rect::new(right - 48, a.y + 1, 48, workspace::HEADER - 2)
    }

    pub(in crate::ui) fn eye_set_place(&mut self, place: EyePlace) {
        let previous = self.m.eye_place;
        if place == EyePlace::Full {
            self.workspace.eye_float = None;
            self.workspace.eye_left = false;
        }
        self.m.eye_place = place;
        if place == EyePlace::Under && self.eye_below() == 0 {
            self.m.eye_place = EyePlace::Side;
            self.toast(
                "Eye stays beside the trace: not enough room below. Hide the logic lanes or measurements to use Under.",
            );
        }
        if self.m.eye_place == previous {
            return;
        }
        if !self.azr.eye_auto.enabled && !self.auto_eye_large() {
            self.azr.eye = None;
        }
        self.relayout();
        self.analyzer_update();
    }

    /// Bit rate of `id` from its edges. Nothing was typed.
    pub(in crate::ui) fn waveform_bit_rate(&self, id: WaveId) -> Option<f64> {
        match id {
            WaveId::Ref(i) => {
                let r = self.refs.get(i)?.as_ref()?;
                automatic::probe_rate(&r.v, r.dt)
            }
            WaveId::Ch(c) => {
                let rec = self.last[0].as_ref()?;
                let (values, dt, _) = self.eye_channel_samples(rec, c)?;
                automatic::probe_rate(values, dt)
            }
        }
    }

    /// One press: look at the waveform, take the bit rate from that record, and open the eye.
    pub(in crate::ui) fn eye_make(&mut self) {
        // Selection is intentional: never switch away from a flat/weak selected
        // signal merely because another visible channel has an easier clock.
        let id = self.selected_workspace_source().or_else(|| {
            (0..n_ch())
                .filter(|&c| self.m.ch[c].on)
                .map(WaveId::Ch)
                .chain((0..NREF).filter(|&i| self.refs[i].as_ref().is_some_and(|r| r.on)).map(WaveId::Ref))
                .next()
        });
        self.close_menu();
        if let Some(id) = id {
            self.m.eye_src = id;
        }
        self.m.eye_from_record = true;
        self.m.eye_rate = 0.0;
        self.m.eye_b = None;
        self.m.eye_rail = None;
        self.m.eye_notch = 0.0;
        self.m.eye_hp = false;
        self.m.view = AView::Eye;
        self.eye_col.big = None;
        self.eye_col.to = 1.0;
        self.eye_col.from = 1.0;
        self.invalidate_reference_eye();
        self.azr.eye_auto.enabled = true;
        self.relayout();
        let rec = self.last[0].clone().unwrap_or_else(Self::blank_record);
        self.eye_update(&rec);
        self.toast(if id.is_some() {
            "Automatic Eye/Jitter on the selected source"
        } else {
            "No active or loaded waveform: select a source"
        });
    }
}
