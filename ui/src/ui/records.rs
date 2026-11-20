// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The records behind the picture: the last record for the web page and SCPI (CSV, binary, LA lines, the views of a
//! channel or a reference in pages), hardware and software segments, browsing the history, measuring a record in the
//! background (the worker, its statistics folds), the search events and the trigger level shown. A new way to read a
//! record goes here.

use super::*;

/// The measurements in the background (`Ui::measure_in_background`, ADR 0062): the worker, while there is one; the
/// record shown is still to be measured (`want`); new results for the statistics, the trend and the counter (`new`);
/// per channel, whether the measurement sent (`env_sent`) and the one shown (`env`) ran on min/max pairs
/// (`framestats::needs_samples` are then not available).
#[derive(Default)]
pub(super) struct MeasBackground {
    pub worker: Option<MeasWorker>,
    pub want: bool,
    pub new: bool,
    pub env_sent: [bool; NCH],
    pub env: [bool; NCH],
    /// the record a background measurement is wanted of (`want`), when it must not be the newest at the time the
    /// worker is free: with raw deep reads (deepmeas.rs) the newest is then often a pair record, held
    pub pending: Option<Record>,
}

/// Segmented recording (ADR 0062): the records since the acquisition started (in software); for hardware segments the
/// recording number told to the thread, its state, the segment shown (browsing) and the trigger times read back.
#[derive(Default)]
pub(super) struct Segments {
    pub n: usize,
    pub run: u64,
    pub view: Option<crate::wave::SegView>,
    pub pos: Option<usize>,
    pub times: Vec<Option<f64>>,
}

impl Ui {
    /// The last record for the web page, as Storage > CSV writes it. A long record is refused rather than cut: making
    /// the text runs in the frame loop, and a file that looks complete but is not would be worse.
    pub fn web_wave_csv(&self) -> Result<String, String> {
        let rec = self.last[0].as_ref().ok_or("No waveform yet")?;
        let n = rec.ch.iter().map(|c| c.len()).max().unwrap_or(0);
        if n > WEB_CSV_MAX {
            return Err(format!("{} points are too many for the browser: use Storage on the scope, or SCPI", n));
        }
        Ok(crate::store::wave_csv(rec, &self.m))
    }

    /// The last record in the binary form of Storage.
    pub fn web_wave_bin(&self) -> Result<Vec<u8>, String> {
        let rec = self.last[0].as_ref().ok_or("No waveform yet")?;
        Ok(crate::store::wave_bin(rec, &self.m))
    }

    /// The LA lines of the last record, one row per change.
    pub fn web_la_csv(&self) -> Result<String, String> {
        let la = self.last[0]
            .as_ref()
            .and_then(|r| r.la.as_ref())
            .filter(|_| self.m.la_on())
            .ok_or("No logic analyser record")?;
        Ok(crate::store::la_csv(la, &self.m.la.lines()))
    }

    /// Logic analyser record of the last acquisition (SCPI).
    pub fn la_record(&self) -> Option<&la::LaRec> {
        self.last[0].as_ref().and_then(|r| r.la.as_ref())
    }

    #[cfg(test)]
    pub(crate) fn measurement_statistics_snapshot_for_test(&self) -> Vec<(u64, [u64; 4])> {
        self.mstat
            .iter()
            .map(|stat| (stat.n, [stat.sum.to_bits(), stat.sumsq.to_bits(), stat.min.to_bits(), stat.max.to_bits()]))
            .collect()
    }

    /// Measurement `k` of channel `c` (NaN when off or not measurable).
    pub fn meas(&self, c: usize, k: Meas) -> f64 {
        self.meas_at(WaveId::Ch(c), k)
    }

    /// Eye figures from the last refresh, in the order the page stores them. Empty until the eye has run.
    pub fn eye_readings(&self) -> &[(&'static str, f64)] {
        &self.azr.page.readings
    }

    /// Records read by the UI beyond what the model shows the board (`Shared::needs_raw`): the protocol watch and XY.
    pub(super) fn needs_raw(&self) -> bool {
        self.azr.watch || self.xy
    }

    /// A STOP that waits for the hardware's final frame: a plain STOP (not the end of Single or segments, not a mask
    /// stop or a history view) of a normal acquisition on the hardware. The records go on until the thread holds: the
    /// record shown is then the newest it read, the one it reads again for the views after the STOP (#24; before, a
    /// batch finished after the STOP was read again, another acquisition than the one shown). For live-compression
    /// pairs or a deep record's pairs with raw deep measurements on, that last frame is read once more raw.
    pub(super) fn stop_wants_final(&self) -> bool {
        let m = &self.m;
        self.src.is_hw()
            && board().slow_mode(m) == crate::board::Slow::Off
            && m.trig_mode != TrigMode::Single
            && m.segments == 0
            && !(m.mask_on && m.mask_stop)
            && self.hist_pos.is_none()
            && self.seg.pos.is_none()
            && self.last[0].as_ref().is_some_and(|r| r.raw.is_some())
    }

    /// Measurement `k` of channel `c` as last measured; NaN for a mean-type measurement made on min/max pairs.
    pub(crate) fn ana_get(&self, c: usize, k: Meas) -> f64 {
        if self.meas_bg.env[c] && crate::framestats::needs_samples(k) {
            f64::NAN
        } else {
            self.ana[c].get(k)
        }
    }

    /// Whether channel `c` of record `rec` is measured on min/max pairs (`meas_slice`).
    pub(super) fn meas_on_pairs(&self, rec: &Record, c: usize) -> bool {
        match self.meas_frame(rec, c) {
            Some((f, _, _)) => f.envelope,
            None => rec.pairs || rec.raw.as_ref().is_some_and(|r| r.frame.envelope),
        }
    }

    /// (points, dt, time of the first point relative to the trigger) of the last record of channel c.
    #[cfg(test)]
    pub fn record_info(&self, c: usize) -> Option<(usize, f64, f64)> {
        let r = self.last[0].as_ref()?;
        if r.ch[c].is_empty() {
            return None;
        }
        Some((r.ch[c].len(), r.dt, r.t0))
    }

    /// A record the protocol trigger can judge: not roll or scan (they have no trigger point), and with the decoder's
    /// input in it; otherwise it is shown as it comes rather than held back for ever.
    pub(super) fn bus_can_match(&self, rec: &Record) -> bool {
        if board().slow_mode(&self.m) != crate::board::Slow::Off {
            return false;
        }
        if self.m.dec_on_la() {
            rec.la.is_some()
        } else {
            rec.ch.get(self.m.dec_src).is_some_and(|v| !v.is_empty())
        }
    }

    /// RAW read-out: (points, dt, t0) of the entire retained native acquisition; `None` when the
    /// screen record is all there is.
    pub fn raw_info(&self, c: usize) -> Option<(usize, f64, f64)> {
        let r = self.last[0].as_ref()?.raw.as_ref()?;
        let v = r.frame.ch.get(c)?;
        // a compressed frame holds min/max pairs, not samples: then the screen record is the answer
        (!v.is_empty() && !r.frame.envelope).then(|| (v.len(), r.frame.dt, r.t0 - r.i0 as f64 * r.frame.dt))
    }

    /// The samples of `raw_info`.
    #[cfg(test)]
    pub fn raw_volts(&self, c: usize) -> Option<Vec<f32>> {
        let r = self.last[0].as_ref()?.raw.as_ref()?;
        let v = r.frame.ch.get(c)?;
        (!r.frame.envelope).then(|| v.iter().map(|x| if x.is_finite() { *x } else { 0.0 }).collect())
    }

    pub fn record_volts(&self, c: usize) -> Vec<f32> {
        self.last[0]
            .as_ref()
            .map(|r| r.ch[c].iter().map(|x| if x.is_finite() { *x } else { 0.0 }).collect())
            .unwrap_or_default()
    }

    /// The samples a `:WAVeform` read uses. `memory` is RAW, or MAXimum while stopped: the deep record when the
    /// acquisition kept one, otherwise the screen record.
    fn channel_view(&self, c: usize, memory: bool) -> Option<(&[f32], f64, f64)> {
        let rec = self.last[0].as_ref()?;
        if memory {
            if let Some(raw) = rec.raw.as_ref().filter(|r| !r.frame.envelope) {
                if let Some(v) = raw.frame.ch.get(c) {
                    if !v.is_empty() && raw.frame.dt.is_finite() && raw.frame.dt > 0.0 {
                        return Some((v.as_slice(), raw.frame.dt, raw.t0 - raw.i0 as f64 * raw.frame.dt));
                    }
                }
            }
        }
        let v = rec.ch.get(c)?;
        (!v.is_empty()).then_some((v.as_slice(), rec.dt, rec.t0))
    }

    /// How many points `channel_page` can walk. Zero when that source has no record.
    pub fn channel_points(&self, c: usize, memory: bool) -> usize {
        self.channel_view(c, memory).map(|(v, _, _)| v.len()).unwrap_or(0)
    }

    /// `(dt, t0)` of that same record. None when it is empty.
    pub fn channel_origin(&self, c: usize, memory: bool) -> Option<(f64, f64)> {
        self.channel_view(c, memory).map(|(_, dt, t0)| (dt, t0))
    }

    /// One page of a channel, 1-based inclusive, the same numbering as `:WAVeform:STARt` and `:WAVeform:STOP`.
    /// `stop == 0` reads through the last point. The time is the first sample of the page.
    pub fn channel_page(&self, c: usize, memory: bool, start: u64, stop: u64) -> Option<(Vec<f32>, f64, f64)> {
        let (v, dt, t0) = self.channel_view(c, memory)?;
        Some(copy_page(v, dt, t0, start, stop))
    }

    /// Samples of a reference slot, finite volts. Empty when that slot holds nothing.
    #[cfg(test)]
    pub(crate) fn ref_volts(&self, slot: usize) -> Vec<f32> {
        self.ref_page(slot, 1, 0).map(|(v, _, _, _, _)| v).unwrap_or_default()
    }

    /// One page of a reference slot. `None` when the slot is empty. Volts per division and offset travel with it.
    pub fn ref_page(&self, slot: usize, start: u64, stop: u64) -> Option<(Vec<f32>, f64, f64, f64, f64)> {
        let r = self.refs.get(slot)?.as_ref()?;
        if r.v.len() < 2 || !(r.dt > 0.0) {
            return None;
        }
        let (page, dt, t0) = copy_page(&r.v, r.dt, r.t0, start, stop);
        let vpd = if r.vpd > 0.0 { r.vpd as f64 } else { 1.0 };
        Some((page, dt, t0, vpd, r.offset as f64))
    }

    /// (points, dt, t0, volts per division, offset) of a slot that has a record.
    pub(crate) fn ref_info(&self, slot: usize) -> Option<(usize, f64, f64, f64, f64)> {
        let r = self.refs.get(slot)?.as_ref()?;
        if r.v.len() < 2 || !(r.dt > 0.0) {
            return None;
        }
        let vpd = if r.vpd > 0.0 { r.vpd as f64 } else { 1.0 };
        Some((r.v.len(), r.dt, r.t0, vpd, r.offset as f64))
    }

    /// Hardware segments are recorded for the current set-up (else the software history records them).
    pub(super) fn seg_hw_on(&self) -> bool {
        self.src.is_hw() && self.m.seg_hw && self.m.segments > 0 && board().seg_max(&self.m).is_some()
    }

    /// Most segments for the current set-up: the hardware limit with hardware segments, else the history limit.
    fn seg_limit(&self) -> usize {
        match board().seg_max(&self.m) {
            Some(x) if self.m.seg_hw && self.src.is_hw() => x.min(SEG_HW_MAX),
            _ => SEG_MAX,
        }
    }

    /// Segments one recording takes.
    pub(super) fn seg_count(&self) -> usize {
        self.m.segments.min(self.seg_limit())
    }

    /// The segment counts of the menu: the steps within the limit, and the limit itself.
    pub(super) fn seg_options(&self) -> Vec<usize> {
        let lim = self.seg_limit();
        let mut v: Vec<usize> = SEGMENTS.iter().copied().filter(|&n| n <= lim).collect();
        if v.last() != Some(&lim) {
            v.push(lim);
        }
        v
    }

    /// Hardware segments: tells the thread about RUN/STOP, takes the times when the read-back is done, stops when every
    /// segment is on the screen, shows a segment asked for.
    pub(super) fn seg_poll(&mut self, running: bool) {
        let Some(v) = self.src.seg_sync(self.seg.run, running) else { return };
        let was = self.seg.view.map(|x| x.phase);
        self.seg.view = Some(v);
        if v.phase == crate::board::SegPhase::Done && was != Some(crate::board::SegPhase::Done) {
            self.seg.times = self.src.seg_times();
            if running && !v.queued {
                self.m.run = RunState::Stop;
                self.toast(&format!("{} segments recorded", v.recorded));
            }
        } else if running && v.phase == crate::board::SegPhase::Done && !v.queued {
            self.m.run = RunState::Stop;
            self.toast(&format!("{} segments recorded", v.recorded));
        }
        if let Some((k, rec)) = self.src.seg_shown(&self.m) {
            self.seg.pos = Some(k as usize);
            self.last[0] = Some(rec);
            self.cleared = false;
            self.measure_last();
        }
    }

    /// "Segment k/N", the trigger time since the first trigger and the step from the previous segment.
    pub(super) fn seg_label(&self, i: usize) -> String {
        let n = self.seg.view.map_or(0, |v| v.recorded);
        let t = |k: usize| self.seg.times.get(k).copied().flatten();
        let mut s = format!("Segment {}/{}", i + 1, n);
        if let Some(x) = t(i) {
            s += &format!(" \u{b7} {}", si(x, "s"));
            if let Some(p) = i.checked_sub(1).and_then(t) {
                s += &format!(" \u{b7} \u{394}t {}", si(x - p, "s"));
            }
        }
        s
    }

    /// Hardware segments: the trigger times since the first trigger, comma separated, in seconds (9.9E37: no stamp).
    pub fn seg_times_text(&self) -> String {
        self.seg
            .times
            .iter()
            .map(|t| t.map_or("9.9E37".to_string(), |x| format!("{:e}", x)))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Hardware segments: shows segment `i` (played from the FPGA memory), stopping the acquisition.
    pub(super) fn seg_show(&mut self, i: usize) {
        self.m.run = RunState::Stop;
        self.seg.pos = Some(i);
        self.src.seg_request(i as u32);
        self.toast(&self.seg_label(i));
    }

    /// Shows record `hist_pos + d` of the history (d = 0: the newest), stopping the acquisition.
    pub(super) fn history_step(&mut self, d: i32) {
        // hardware segments: any of them, played on demand
        if let Some(v) =
            self.seg.view.filter(|v| self.seg_hw_on() && v.phase == crate::board::SegPhase::Done && v.recorded > 0)
        {
            let n = v.recorded as i32;
            let cur = self.seg.pos.unwrap_or(n as usize - 1) as i32;
            let i = if d == 0 { n - 1 } else { (cur + d).clamp(0, n - 1) };
            return self.seg_show(i as usize);
        }
        let n = self.history.len();
        if n == 0 {
            return self.toast("History empty");
        }
        self.m.run = RunState::Stop;
        let cur = self.hist_pos.unwrap_or(n - 1) as i32;
        let i = if d == 0 { n as i32 - 1 } else { (cur + d).clamp(0, n as i32 - 1) } as usize;
        self.hist_pos = Some(i);
        self.last[0] = Some(self.history[i].clone());
        self.measure_last();
        self.toast(&format!("History {}/{}", i + 1, n));
    }

    /// Recomputes the measurements of the record shown, and of every loaded ref, at once (a result from the
    /// background for an older record is dropped).
    pub(crate) fn measure_last(&mut self) {
        self.measure_channel_sources();
        self.measure_refs();
    }

    /// The measurements of `rec` now, on this thread.
    pub(super) fn measure_rec(&mut self, rec: &Record) {
        let work: Vec<super::MeasWork> = (0..NCH)
            .filter(|&c| self.m.ch[c].on)
            .map(|c| {
                let (v, dt) = self.meas_slice(rec, c);
                (c, v, dt, 0.4 * self.m.ch[c].volts_per_div() as f64, self.meas_on_pairs(rec, c))
            })
            .collect();
        let done = measure_channels(&work);
        self.meas_bg.env_sent = std::array::from_fn(|c| self.meas_on_pairs(rec, c));
        self.meas_set(done);
        self.meas_n += 1;
        self.meas_bg.want = false;
    }

    /// Statistics of measurements made on every frame by the deposit workers (framestats.rs) into the Measure page's
    /// statistics, once these exist (the first measured record starts them).
    pub(super) fn mstat_fold(&mut self, folds: &crate::framestats::Folds) {
        if self.mstat.len() < self.m.meas.len() {
            return;
        }
        for (key, f) in folds.iter().filter(|x| x.1.n > 0) {
            if let Some(i) = self.m.meas.iter().position(|e| *e == (key.0, WaveId::Ch(key.1))) {
                let st = &mut self.mstat[i];
                if st.n == 0 {
                    (st.min, st.max) = (f.min, f.max);
                }
                st.n += f.n;
                st.sum += f.sum;
                st.sumsq += f.sumsq;
                st.min = st.min.min(f.min);
                st.max = st.max.max(f.max);
            }
        }
    }

    /// Each loaded ref, at the slot's own sample interval. Coupling and the bandwidth limit are not applied.
    pub(super) fn measure_refs(&mut self) {
        self.measure_reference_sources();
    }

    /// A record with no samples, for an analyser whose sources are all stored refs.
    pub(super) fn blank_record() -> Record {
        Record {
            t0: 0.0,
            dt: 1.0,
            ch: Default::default(),
            triggered: false,
            partial: false,
            raw: None,
            la: None,
            pairs: false,
        }
    }

    fn meas_set(&mut self, done: Vec<(usize, Stats, oos_dsp::measure::Analysis)>) {
        self.meas_bg.env = self.meas_bg.env_sent;
        for c in 0..NCH {
            (self.stats[c], self.ana[c]) = (Stats::default(), Default::default());
        }
        for (c, st, an) in done {
            (self.stats[c], self.ana[c]) = (st, std::sync::Arc::new(an));
        }
    }

    /// From now on the measurements of new records are made in the background (the frame loop on the unit).
    pub fn measure_in_background(&mut self) {
        self.meas_bg.worker = MeasWorker::start();
    }

    /// The background's side, every frame: takes a finished result, and gives it the record shown when it wants one.
    pub(super) fn meas_poll(&mut self) {
        let Some(w) = self.meas_bg.worker.as_mut() else { return };
        if let Ok((n, done)) = w.rx.try_recv() {
            w.busy = false;
            if n == self.meas_n {
                // of the record shown when it was sent: in Run a few frames old, the newest result there is
                self.meas_set(done);
                self.meas_bg.new = true;
            }
        }
        let w = self.meas_bg.worker.as_mut().expect("worker");
        if w.busy || !self.meas_bg.want {
            return;
        }
        self.meas_bg.want = false;
        let pending = self.meas_bg.pending.take();
        let Some(rec) = pending.as_ref().or(self.last[0].as_ref()) else { return };
        // a held pair record (deepmeas.rs) is never measured: the measurement stays that of the last raw one
        if crate::deepmeas::hold(&self.m, rec.raw.as_ref().map(|r| &*r.frame)) {
            return;
        }
        let env: [bool; NCH] = std::array::from_fn(|c| self.meas_on_pairs(rec, c));
        let job: Vec<(usize, MeasSrc, f64, f64, bool)> = (0..NCH)
            .filter(|&c| self.m.ch[c].on)
            .map(|c| {
                let min = 0.4 * self.m.ch[c].volts_per_div() as f64;
                match self.meas_frame(rec, c) {
                    Some((f, i0, i1)) => {
                        let dt = f.dt;
                        (c, MeasSrc::Frame(f, c, i0, i1), dt, min, env[c])
                    }
                    None => (c, MeasSrc::Own(rec.ch[c].clone()), rec.dt, min, env[c]),
                }
            })
            .collect();
        let w = self.meas_bg.worker.as_mut().expect("worker");
        if w.tx.send((self.meas_n, job)).is_ok() {
            w.busy = true;
            self.meas_bg.env_sent = env;
        } else {
            // the thread is gone: measure here again
            self.meas_bg.worker = None;
            self.measure_last();
            self.meas_bg.new = true;
        }
    }

    /// The window of the hardware frame `meas_slice` measures channel `c` on, if it does.
    pub(super) fn meas_frame(
        &self,
        rec: &Record,
        c: usize,
    ) -> Option<(std::sync::Arc<crate::board::Frame>, usize, usize)> {
        let r = rec.raw.as_ref().filter(|_| self.m.acq != AcqMode::HiRes)?;
        (r.i1 <= r.frame.ch[c].len() && r.i1 - r.i0 > rec.ch[c].len()).then(|| (r.frame.clone(), r.i0, r.i1))
    }

    /// Samples to measure channel `c` on: every captured sample of the screen window when the hardware frame has more
    /// than the screen record (conditioned like the record), else the screen record.
    pub(super) fn meas_slice<'a>(&self, rec: &'a Record, c: usize) -> (&'a [f32], f64) {
        // high res: the screen record is the mean of those samples, the measurements follow it
        if let Some(r) = rec.raw.as_ref().filter(|_| self.m.acq != AcqMode::HiRes) {
            if self.meas_frame(rec, c).is_some() {
                return (&r.frame.ch[c][r.i0..r.i1], r.frame.dt);
            }
        }
        (&rec.ch[c], rec.dt)
    }

    /// `meas_slice` as an own copy.
    pub(super) fn meas_wave(&self, rec: &Record, c: usize) -> (Vec<f32>, f64) {
        let (v, dt) = self.meas_slice(rec, c);
        (v.to_vec(), dt)
    }

    /// Search: times (s, relative to the trigger) of all events of the trigger condition in the record shown.
    pub(super) fn search_events(&self) -> std::sync::Arc<Vec<f64>> {
        if self.m.trig_type == TrigType::Bus
            && (Self::decoder_uses_refs(&self.m) || self.m.deskew_ch.iter().any(|&shift| shift != 0.0))
        {
            return self.decoder_trigger_hits(&self.m).unwrap_or_default();
        }
        if let Some(slot) = self.m.trig_ref {
            return self.reference_trigger_hits(slot).unwrap_or_else(|| std::sync::Arc::new(Vec::new()));
        }
        let Some(rec) = &self.last[0] else { return std::sync::Arc::new(Vec::new()) };
        if !self.m.trig_channels().iter().enumerate().all(|(c, &u)| !u || !rec.ch[c].is_empty()) {
            return std::sync::Arc::new(Vec::new());
        }
        let refs: [&[f32]; NCH] = std::array::from_fn(|c| &rec.ch[c][..]);
        let hyst: [f32; NCH] = std::array::from_fn(|c| 0.1 * self.m.ch[c].volts_per_div());
        let inp = oos_dsp::trigger::Input { ch: refs, dt: rec.dt, hyst };
        let t0 = self.source_time_origin(self.m.trigger_wave(), rec.t0);
        let mut times: Vec<_> = crate::trig::find_all(&self.m, &inp).into_iter().map(|i| t0 + i * rec.dt).collect();
        times.sort_by(f64::total_cmp);
        std::sync::Arc::new(times)
    }

    /// Half the sample rate while it is below the ADC's and the samples are taken one in so many (Normal, Average, High
    /// res): the hardware keeps every n-th sample and filters nothing before, so a signal above it shows at a wrong,
    /// lower frequency. None at the full rate and in Peak, whose min/max see every sample.
    pub fn alias_above(&self) -> Option<f64> {
        let (sr, _) = self.m.srate_depth();
        (self.m.acq != AcqMode::Peak && sr < self.m.max_srate() * 0.999).then_some(sr / 2.0)
    }

    /// The level line is on: shortly after the trigger level changed (`LEVEL_LINE_S`).
    pub(super) fn level_line_on(&self) -> bool {
        self.seen.level_changed > 0.0 && self.now - self.seen.level_changed < LEVEL_LINE_S
    }

    /// Level shown at the trigger marker: the pattern level of the source for pattern/duration, else the level.
    pub(super) fn trig_level_shown(&self) -> f32 {
        if matches!(self.m.trig_type, TrigType::Pattern | TrigType::Duration) {
            self.m.pat_levels[self.m.trig_src]
        } else {
            self.m.trig_level
        }
    }
}

/// A 1-based inclusive page. `stop == 0` means the last sample. A start past the end is an empty page.
fn copy_page(v: &[f32], dt: f64, t0: f64, start: u64, stop: u64) -> (Vec<f32>, f64, f64) {
    let n = v.len() as u64;
    if n == 0 || start > n {
        return (Vec::new(), dt, t0);
    }
    let a = start.max(1);
    let b = if stop == 0 { n } else { stop.clamp(a, n) };
    let lo = (a - 1) as usize;
    let hi = b as usize;
    let page = v[lo..hi].iter().map(|x| if x.is_finite() { *x } else { 0.0 }).collect();
    (page, dt, t0 + lo as f64 * dt)
}
