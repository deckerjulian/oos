// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Acquisition per frame: `Ui::step` takes what the source delivered since the last frame under the run state (RUN,
//! SINGLE, STOP, segments), starts the measurements of the newest record in the background and keeps the trace
//! brightness and persistence maps; the fast path for the queued hardware frames (`Ui::fast_draw_all`), the record
//! shown after a STOP and the HDMI header line are here too. A new per-frame job goes into `Ui::step`.

use super::*;

/// What the acquisition of one step took: records, times, the pending clear and the newest raw deep record.
struct StepTake {
    got: usize,
    ta: f32,
    td: f32,
    clear_pending: bool,
    deep_raw: bool,
    raw_rec: Option<Record>,
}

impl Ui {
    /// One frame: the polls, the run state, the records of this step, their measurements and the trace brightness.
    pub fn step(&mut self, dt_wall: f64) {
        self.step_polls(dt_wall);
        self.step_guards();
        let (hw_seg, final_done) = self.step_run_state();
        let persist = PERSIST[self.m.persist].1;
        self.step_refresh(dt_wall);
        let live = self.m.n_on() > 0 || self.m.la_on();
        // a source started in STOP (replay) still shows its first record, unless the screen was cleared
        if !self.record_mode()
            && live
            && (self.m.run == RunState::Run || self.stop_final.is_some() || (self.last[0].is_none() && !self.cleared))
        {
            self.step_live(dt_wall, persist, hw_seg);
        } else if !self.record_mode() {
            // stopped: the whole stored record for the view shown (read again by the thread, `stop_replay`), then
            // redraw the stored record with the current scale/offset
            self.step_stopped_redraw();
        }
        if self.analyzed_view != self.m.view {
            self.analyzed_view = self.m.view;
            if self.m.view != AView::Scope {
                self.analyzer_update();
            }
        }
        self.step_meas_rows();
        // the final frame was taken (or did not come in time): stopped
        if self.stop_final.is_some_and(|t| final_done || self.now > t) {
            self.stop_final = None;
        }
        if self.m.run == RunState::Stop {
            if !self.record_mode() {
                self.m.status = TrigStatus::Stop;
            }
            self.wfm_rate = 0.0;
        }
        self.step_grade();
    }

    /// The per-frame polls and ticks, and the wall time.
    fn step_polls(&mut self, dt_wall: f64) {
        // the deposit workers (drawpipe.rs) draw the hardware's batches; the other sources have no batches
        crate::drawpipe::set_enabled(self.src.is_hw());
        // System > Features: what is left out runs nothing
        self.features_enforce();
        self.poll_reference_measurements();
        self.poll_catalog_measurements();
        self.math_fit_tick();
        self.sim_poll();
        self.eye_auto_poll();
        self.relayout();
        self.now += dt_wall;
        self.meas_hold_tick();
        self.workspace_hold_tick();
        self.update_poll();
        self.wlan_poll();
        self.ntp_poll();
        self.ssh_poll();
        self.allow_poll();
        self.usb_poll();
        self.bt_poll();
        self.ch_hold_poll();
        self.counter_poll();
        self.dvm_poll();
        self.mask_hw_poll();
        self.zone_hw_poll();
        self.la_probe_poll();
        self.log_tick();
        self.m.clamp_offsets();
    }

    /// Trigger level changes, SINGLE arming, the channels of the platform and the sources that point at them.
    fn step_guards(&mut self) {
        if (self.m.trig_level, self.m.trig_level2) != self.level_seen {
            // the first look (start-up) is no change
            if !self.level_seen.0.is_nan() {
                self.level_changed = self.now;
            }
            self.level_seen = (self.m.trig_level, self.m.trig_level2);
        }
        self.src.publish_afg(&self.m);
        // SINGLE armed: from here on only what comes after the press counts
        let single = self.m.trig_mode == TrigMode::Single && self.m.run == RunState::Run;
        if single && !self.single_armed {
            self.m.hw_stamp += 1;
            self.hw_avg_restart();
        }
        self.single_armed = single;
        self.bus_candidate_guard_tick();
        // 2-channel models: CH3/CH4 stay off whatever switched them on (setups, autoset, demos, SCPI)
        if self.m.fit_platform() {
            self.mark_changed();
        }
        if let WaveId::Ch(c) = self.meas_src {
            if c >= n_ch() {
                self.meas_src = WaveId::Ch(0);
            }
        }
        if let WaveId::Ch(c) = self.meas_src_b {
            if c >= n_ch() {
                self.meas_src_b = WaveId::Ch(0);
            }
        }
        if self.ref_src >= n_ch() {
            self.ref_src = n_ch() - 1;
        }
        self.auto_step();
        self.sysinfo_poll();
        self.cal_poll();
    }

    /// RUN and STOP edges: segments, average restart, the final frame of a STOP; the segment switch and whether
    /// the final frame is taken.
    fn step_run_state(&mut self) -> (bool, bool) {
        // segmented recording: a new RUN/SINGLE starts a new set of segments
        let running = self.m.run == RunState::Run;
        let hw_seg = self.seg_hw_on();
        // RUN after STOP: the hardware average starts over (stock clears it on every state change)
        if running && !self.was_running {
            self.hw_avg_restart();
        }
        if running && !self.was_running && self.m.segments > 0 {
            self.seg_n = 0;
            self.history.clear();
            self.hist_pos = None;
            if hw_seg {
                self.seg_run += 1;
                (self.seg_pos, self.seg_view) = (None, None);
                self.seg_times.clear();
            }
        }
        // STOP of a deep record on pairs with raw deep measurements: the hardware reads the newest frame raw and
        // stops arming; until it is here the records go on (docs/fork/stop-exact.md)
        if !running && self.was_running && self.stop_wants_final() {
            self.final_req += 1;
            self.stop_final = Some(self.now + STOP_FINAL_WAIT);
        }
        // held while stopped, unless the screen still waits for a first record (a source started in STOP, as the
        // record path below takes it); a Clear in STOP leaves the screen empty, not the thread free to arm
        let final_done = self.src.hw_hold(!running && (self.last[0].is_some() || self.cleared), self.final_req);
        // the protocol watch and XY read every record: no live compression for them (compress.rs)
        self.src.hw_needs_raw(self.needs_raw());
        self.was_running = running;
        if hw_seg {
            self.seg_poll(running);
        }
        if !running {
            self.cancel_demo_acquisition();
        }
        (hw_seg, final_done)
    }

    /// Record mode plays or refreshes its reference; a STOP refreshes the captured view and its measurements.
    fn step_refresh(&mut self, dt_wall: f64) {
        if self.record_mode() {
            if self.m.run == RunState::Run {
                self.sim_play = true;
            }
            self.sim_tick(dt_wall);
            if self.m.run == RunState::Stop {
                self.refresh_stopped_reference();
            }
        }
        if self.m.run == RunState::Stop && !self.record_mode() {
            self.refresh_capture_view();
            self.refresh_stopped_measurements();
        }
    }

    /// The records of this step: the fast path, the record loop, the rates and the measurement of the newest.
    fn step_live(&mut self, dt_wall: f64, persist: f32, hw_seg: bool) {
        if self.m.run == RunState::Run {
            self.stop_ti_off = None;
            (self.stop_raw, self.stop_pic) = (None, None);
        }
        if self.dirty_map {
            self.ph.clear();
            self.phz.clear();
            self.dirty_map = false;
        }
        // minimum persistence: replace the picture when a new record arrives, not every frame (at large depths
        // the hardware delivers fewer records than frames, and clearing each frame made the trace flicker)
        let clear_pending = persist == 0.0;
        if persist > 0.0 {
            let f = (-dt_wall / persist as f64).exp() as f32;
            self.ph.decay(f);
            self.phz.decay(f);
        }
        let fast = self.fast_ok();
        if !fast {
            self.src.hw_take_all();
        }
        let k = match self.m.trig_type {
            // fast path: it draws the queue on several cores, the record is the newest frame only (frames that
            // come in meanwhile wait for the next screen's fast path instead of a full record each, one by one)
            _ if fast => 1,
            // the hardware source delivers batches: draw up to 48 queued records per frame
            _ if matches!(self.src, Source::Hw { .. }) => 48,
            TrigType::Edge | TrigType::Pattern => 6,
            TrigType::Video => 1,
            _ => 2,
        };
        // the newest raw deep record of this step (deepmeas.rs): with one frame per batch (2 ms/div and up at
        // 10 M) the next pair frame is often queued behind it, which drew it on the fast path or replaced it as
        // the record, and the measurement stayed at the last one for good (unit 2026-11-13)
        let mut take = StepTake {
            got: 0,
            ta: 0.0,
            td: 0.0,
            clear_pending,
            deep_raw: crate::deepmeas::want(&self.m).is_some(),
            raw_rec: None,
        };
        if !fast {
            if let Some(p) = crate::drawpipe::pipe() {
                p.set_view(None, 0, 0);
            }
        }
        // fast display path (fastdraw.rs): every queued frame but the newest straight into the phosphor map
        if fast {
            self.step_fast_frames(&mut take);
        }
        self.step_records(&mut take, k, dt_wall, hw_seg);
        self.step_rates(&take, dt_wall, persist);
        self.step_measure(&mut take);
    }

    /// Fast display path: the queued frames straight into the phosphor map, and the batch pipeline's merge.
    fn step_fast_frames(&mut self, take: &mut StepTake) {
        let t0 = std::time::Instant::now();
        let frames = self.src.drain_newest(self.fast_max());
        if !frames.is_empty() && take.clear_pending {
            self.ph.clear();
            take.clear_pending = false;
        }
        let n = frames.len();
        if let Some(f) = frames.iter().rev().find(|f| take.deep_raw && crate::deepmeas::deep_raw_frame(f)) {
            take.raw_rec = Some(crate::wave::hw_record(&self.m, f.clone()));
        }
        if n > 0 && self.fast_draw_all(frames, fast_helpers()) > 0 {
            self.m.status = TrigStatus::Triggered;
        }
        take.got += n;
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        take.td += ms;
        self.t_fast += 0.05 * (ms - self.t_fast);
        if n > 0 && ms > 0.0 {
            // as many as would have filled the budget at this pace: with a fixed part in each screen's time
            // (starting the helpers) this settles where the time meets the budget. Down only when the budget
            // was exceeded, up only when the take was the limit: the hardware delivers in bursts, and a screen
            // with a few frames says nothing about how many fit
            self.fast_take = fast_take_next(self.fast_take, n, ms, self.fast_budget());
        }
        // batch mode: the other frames of the batches, drawn by the deposit workers (drawpipe.rs)
        if let Some(p) = crate::drawpipe::pipe() {
            let t1 = std::time::Instant::now();
            p.set_view(Some(&self.m), self.ph.w, self.ph.h);
            let d = p.merge(&mut self.ph, &mut take.clear_pending, &mut self.fast_edges, &mut self.fast_off);
            if d.triggered > 0 {
                self.m.status = TrigStatus::Triggered;
            }
            take.got += d.frames as usize;
            self.mstat_fold(&d.folds);
            take.td += t1.elapsed().as_secs_f32() * 1000.0;
        }
    }

    /// Up to `k` records from the source, each through the zone and protocol triggers before it is kept.
    fn step_records(&mut self, take: &mut StepTake, k: usize, dt_wall: f64, hw_seg: bool) {
        for _ in 0..k {
            let t0 = std::time::Instant::now();
            let r = self.acquire_bus_candidate(dt_wall / k as f64);
            take.ta += t0.elapsed().as_secs_f32() * 1000.0;
            if r.is_none() && self.bus_candidate_waiting() {
                break;
            }
            match r {
                Some(mut rec) if !crate::zone::pass(&self.m.zones, &rec.ch, rec.t0, rec.dt) => {
                    // zone trigger: not a qualifying record
                    let _ = &mut rec;
                    self.zone_drops += 1;
                }
                // protocol trigger, Normal and Single: the hardware fires on every edge of the line, only the decoder
                // decides; Single waits on for the record that holds the event
                Some(rec)
                    if self.m.trig_type == TrigType::Bus
                        && !self.mixed_bus_mode()
                        && self.m.trig_mode != TrigMode::Auto
                        && !rec.triggered
                        && !rec.partial
                        && self.bus_can_match(&rec) =>
                {
                    self.m.status = TrigStatus::Wait;
                }
                Some(rec) => {
                    if self.step_commit(rec, take, hw_seg) {
                        break;
                    }
                }
                None => {}
            }
            if self.m.run == RunState::Stop && self.stop_final.is_none() {
                break;
            }
        }
    }

    /// One record kept: conditioned, drawn, into the history, and the end of segments or SINGLE; true stops the
    /// loop.
    fn step_commit(&mut self, mut rec: Record, take: &mut StepTake, hw_seg: bool) -> bool {
        self.bus_candidate_committed();
        self.m.status = if rec.triggered { TrigStatus::Triggered } else { TrigStatus::Auto };
        self.condition_record(&mut rec);
        // minimum persistence: the picture of this step is every frame of it (the fast path's and this
        // record), cleared once before the first of them
        if take.clear_pending {
            self.ph.clear();
            self.phz.clear();
            take.clear_pending = false;
        }
        let zv = (tscales()[self.m.zscale], self.m.zdelay);
        let detail = self.captured_record_view(&rec, zv);
        let t1 = std::time::Instant::now();
        let zw = self.z_weights(&rec);
        // the fast path draws the other frames of the picture from their samples (`draw_frame`): the
        // newest one the same way, or it adds about 3.6 times their counts per pixel (3600 record points
        // against one span per column) and the grading of the picture follows how many of each it holds:
        // the trace flickered with the batch sizes (docs/fork/flicker.md)
        let frame = rec.raw.as_ref().filter(|_| self.fast_ok()).map(|r| r.frame.clone());
        if let Some(f) = frame {
            let stats = crate::drawpipe::Stats { edges: &mut self.fast_edges, off: &mut self.fast_off };
            crate::drawpipe::draw_frame(&mut self.ph, &self.m, &f, Some(stats));
        } else {
            self.draw_channels(&rec, detail.as_deref(), zw.as_deref(), 24);
        }
        take.td += t1.elapsed().as_secs_f32() * 1000.0;
        if self.history.len() >= if hw_seg { HISTORY } else { HISTORY.max(self.seg_count()) } {
            self.history.pop_front();
        }
        // a deep LA record is tens of MB: the history keeps the LA of short records only
        let la = rec.la.clone().filter(|l| l.words.len() <= HISTORY_LA_WORDS);
        self.history.push_back(Record { raw: None, la, ..rec.clone() });
        self.hist_pos = None;
        self.mask_test(&rec);
        if take.deep_raw && rec.raw.as_ref().is_some_and(|r| crate::deepmeas::deep_raw_frame(&r.frame)) {
            take.raw_rec = Some(rec.clone());
        }
        self.last[0] = Some(rec);
        self.cleared = false;
        take.got += 1;
        self.records += 1;
        if self.m.segments > 0 {
            self.seg_n += 1;
            // hardware segments end when the thread has read them all back (`seg_poll`)
            if !hw_seg && self.seg_n >= self.seg_count() {
                if self.m.trig_mode == TrigMode::Single {
                    self.scpi_finish_operation(OperationToken::Single(self.single_operation));
                }
                self.m.run = RunState::Stop;
                self.toast(&format!("{} segments recorded", self.seg_n));
                return true;
            }
        } else if self.m.trig_mode == TrigMode::Single && !self.last[0].as_ref().is_some_and(|r| r.partial) {
            self.scpi_finish_operation(OperationToken::Single(self.single_operation));
            self.m.run = RunState::Stop;
            return true;
        }
        false
    }

    /// The channel conditioning of a software source and the software average.
    fn condition_record(&self, rec: &mut Record) {
        for c in 0..NCH {
            if self.m.ch[c].on
                && !rec.ch[c].is_empty()
                && !self.src.is_hw()
                && !rec.raw.as_ref().is_some_and(|raw| raw.conditioned)
            {
                let dt = rec.dt;
                condition(&self.m, c, &mut rec.ch[c], dt);
            }
        }
        // the FPGA averaged the frame already: blending would average the average
        let hw_avg = rec.raw.as_ref().is_some_and(|r| r.frame.hw_avg);
        if self.m.acq == AcqMode::Average && !hw_avg {
            if let Some(prev) = &self.last[0] {
                let a = 1.0 / self.m.avg_count as f32;
                for c in 0..NCH {
                    if rec.ch[c].len() == prev.ch[c].len() {
                        for (x, p) in rec.ch[c].iter_mut().zip(prev.ch[c].iter()) {
                            if p.is_finite() {
                                *x = p + (*x - p) * a;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Trigger status, picture frames, record interval, timings and waveform rate of this step.
    fn step_rates(&mut self, take: &StepTake, dt_wall: f64, persist: f32) {
        // WAIT only when the whole frame brought nothing (the hardware source delivers a new record only every
        // few attempts; an empty attempt must not override TRIG'D)
        // at large depths the hardware delivers fewer records than frames: WAIT only after 0.5 s without one, or
        // 1.5 record lengths at slow time bases (a 1 s record at 100 ms/div comes once per second), or 2.5 usual
        // record intervals (a 125 M record takes ~1 s to read)
        let wait_after = 0.5f64.max(1.5 * HDIV as f64 * self.m.time_per_div() as f64).max(2.5 * self.rec_iv);
        // `trig.hw_status = on`: the SCU says whether the TPU fired or the auto timeout forced the batch; it
        // overrides what the software refinement concluded (docs/fork/trigger-lockin.md)
        if take.got > 0 {
            if let Some((_, Some(tpu))) = self.src.hw_trig_fired() {
                self.m.status = if tpu { TrigStatus::Triggered } else { TrigStatus::Auto };
            }
        }
        // minimum persistence shows the frames of this step alone: their number grades the picture (`picture_ref`)
        if persist != 0.0 {
            self.pic_frames = 0;
        } else if take.got > 0 {
            self.pic_frames = take.got as u32;
        }
        if take.got > 0 {
            let iv = (self.now - self.last_rec_t).min(2.0);
            self.rec_iv += 0.3 * (iv - self.rec_iv);
            self.last_rec_t = self.now;
        } else if self.m.run == RunState::Run && self.now - self.last_rec_t > wait_after {
            self.m.status = TrigStatus::Wait;
        }
        self.t_acq += 0.05 * (take.ta - self.t_acq);
        self.t_draw += 0.05 * (take.td - self.t_draw);
        let rate = take.got as f32 / dt_wall.max(1e-3) as f32;
        self.wfm_rate += 0.1 * (rate - self.wfm_rate);
        self.wfm_total += take.got as u64;
        crate::drawpipe::Counters::add(&crate::drawpipe::counters().used, take.got as u64);
    }

    /// The measurement of this step's record, now or in the background, and the analyser updates.
    fn step_measure(&mut self, take: &mut StepTake) {
        // deep record on min/max pairs while raw reads of its screen window come (deepmeas.rs): the
        // measurement stays that of the last raw record
        let shown = self.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| &*r.frame);
        let hold = crate::deepmeas::hold(&self.m, shown) || crate::compress::hold(&self.m, shown);
        if take.got > 0 {
            let p0 = std::time::Instant::now();
            if hold {
                // the measurement shown stays that of the last raw deep record, or of this step's
                if let Some(r) = take.raw_rec.take() {
                    if self.meas_bg.is_some() {
                        self.meas_want = true;
                        self.meas_pending = Some(r);
                    } else {
                        self.measure_rec(&r);
                        self.meas_new = true;
                    }
                }
            } else if self.meas_bg.is_some() {
                // the result for the last record is still good when it comes after this one: `meas_n` only
                // changes where a result out becomes wrong (a clear, measuring here at once)
                self.meas_want = true;
                // raw deep reads: this raw record is the one to measure, even when pair records (held) follow
                // before the worker is free; measuring the newest then measured a pair record (Mean NaN live)
                // (only a raw deep record: a clone of every record cost the screen rate, 26 instead of 27 frames a
                // second with GPU and HDMI Layout, unit 2026-11-15)
                let raw_deep = self.last[0]
                    .as_ref()
                    .and_then(|r| r.raw.as_ref())
                    .is_some_and(|r| crate::deepmeas::deep_raw_frame(&r.frame));
                if raw_deep && crate::deepmeas::want(&self.m).is_some() {
                    self.meas_pending = self.last[0].clone();
                }
            } else {
                self.measure_last();
                self.meas_new = true;
            }
            let p1 = std::time::Instant::now();
            self.analyzer_update();
            self.monitor_analyzer_update();
            self.protocol_watch();
            self.bode_update();
            let p2 = std::time::Instant::now();
            self.prof_add(0, p1 - p0);
            self.prof_add(1, p2 - p1);
        }
    }

    /// Stopped: the stored record for the view shown, redrawn with the current scale and offset.
    fn step_stopped_redraw(&mut self) {
        self.stop_replay();
        self.ph.clear();
        self.phz.clear();
        let zv = (tscales()[self.m.zscale], self.m.zdelay);
        let detail = self.captured_view(zv);
        // the view read again as pairs while the raw record of the STOP is the one measured: the pairs are drawn
        let shown = self.stop_picture().cloned().or_else(|| self.last[0].clone());
        if let Some(rec) = shown {
            let zw = self.z_weights(&rec);
            self.draw_channels(&rec, detail.as_deref(), zw.as_deref(), 24 * 6);
        }
    }

    /// The rows of the newest measurement into the trends, the counter and the statistics.
    fn step_meas_rows(&mut self) {
        // the measurements of the newest record (in the background: when they are done, also after a stop)
        self.meas_poll();
        if std::mem::take(&mut self.meas_new) {
            let rows = self.meas_rows();
            self.trend_update(self.mstat.is_empty(), &rows);
            self.mstat.resize(rows.len(), MStat::default());
            self.counter_update();
            for (i, row) in rows.iter().enumerate() {
                if row.computing {
                    if let Some(entry) =
                        i.checked_sub(self.m.meas.len() + self.m.meas2.len()).and_then(|i| self.m.cat.get(i)).copied()
                    {
                        self.queue_catalog_statistics(entry, self.wfm_total);
                    }
                }
                if row.v.is_finite() {
                    self.mstat[i].add(row.v);
                }
            }
        }
    }

    /// Pane tick, trace brightness references, halving of saturating maps and the end of a toast.
    fn step_grade(&mut self) {
        let p3 = std::time::Instant::now();
        self.pane_tick();
        let p4 = std::time::Instant::now();
        self.prof_add(2, p4 - p3);
        let p5 = std::time::Instant::now();
        // brightest pixel per channel: the trace brightness is graded relative to it
        // (follow upwards at once, downwards slowly: no flicker)
        let (full, fullz) = self.grade_refs();
        self.prof_add(3, p5.elapsed());
        // long or infinite persistence: halve the maps before the counts saturate (keeps the density ratios)
        if full {
            self.ph.halve();
            self.vmax.iter_mut().for_each(|v| *v = (*v / 2).max(1));
            self.vmax_cg = (self.vmax_cg / 2).max(1);
        }
        if fullz {
            self.phz.halve();
            self.vmaxz.iter_mut().for_each(|v| *v = (*v / 2).max(1));
            self.vmaxz_cg = (self.vmaxz_cg / 2).max(1);
        }
        if let Some((_, t)) = &self.toast {
            if self.now > *t {
                self.toast = None;
            }
        }
    }

    /// One line of settings for the HDMI workspace: channels, time base, trigger, state, rate.
    pub fn hdmi_header(&self) -> String {
        let m = &self.m;
        let mut parts: Vec<String> = (0..NCH)
            .filter(|&c| m.ch[c].on)
            .map(|c| format!("CH{} {}/div", c + 1, si_short(m.ch[c].volts_per_div() as f64, "V")))
            .collect();
        parts.push(format!("{}/div", si_short(m.time_per_div() as f64, "s")));
        if self.offline {
            let (name, sub) = self.ref_card_lines();
            parts.push(if sub == "empty" || sub.ends_with("pts") { name } else { format!("{} {}", name, sub) });
            parts.push(self.run_caption().0.into());
        } else {
            parts.push(match m.trig_line() {
                Some(n) => format!("Trig D{}", n),
                None => format!("Trig {} {}", self.trigger_source_label(), si_short(m.trig_level as f64, "V")),
            });
            let st = match m.status {
                TrigStatus::Triggered => "TRIG'D",
                TrigStatus::Auto => "AUTO",
                TrigStatus::Wait => "WAIT",
                TrigStatus::Stop => "STOP",
            };
            parts.push(st.into());
        }
        parts.push(format!("{:.0} wfm/s", self.wfm_rate));
        parts.join("   \u{b7}   ")
    }

    /// Frames the fast path takes in one screen: as many as fit into `fast_budget` at the pace of the last screens.
    /// Beyond that the frame rate would drop (and with it the time the newest frame waits to be seen).
    pub(super) fn fast_max(&self) -> usize {
        (self.fast_take as usize).clamp(4, crate::board::QUEUE_MAX)
    }

    /// Time of a screen the fast path may spend (ms): what a frame of the 30 a second (33 ms) leaves after the rest of
    /// the step, drawing and showing the screen, less 2 ms of slack; 5 to 28 ms.
    pub(super) fn fast_budget(&self) -> f32 {
        let rest = (self.t_step - self.t_fast).max(0.0) + self.t_render + self.t_present;
        (31.0 - rest).clamp(5.0, 28.0)
    }

    /// The channels of `rec` into the main picture and, with zoom, into the zoom picture; every point adds `w`.
    /// `detail` is the zoom window read from the capture memory, `zw` the Z input's weights of `rec`: a deskewed
    /// channel is drawn from its own view of the record, without them.
    fn draw_channels(&mut self, rec: &Record, detail: Option<&Record>, zw: Option<&[f32]>, w: u16) {
        let mainv = (self.m.time_per_div(), self.m.delay);
        let zv = (tscales()[self.m.zscale], self.m.zdelay);
        let zoom_model = self.zoom_display_model();
        for c in 0..NCH {
            if !self.m.ch[c].on || rec.ch[c].is_empty() || self.m.z_src == Some(c) {
                continue;
            }
            let shifted = self.deskew_channel_view(rec, c, mainv);
            let main = shifted.as_deref().unwrap_or(rec);
            let main_zw = if shifted.is_some() { None } else { zw };
            let t0 = main.t0 - self.m.deskew_ch[c];
            self.ph.draw_ex(c, &self.m, mainv, t0, main.dt, &main.ch[c], w, main_zw, main.pairs);
            if self.m.zoom {
                let shifted = self.deskew_channel_view(rec, c, zv);
                let z = shifted.as_deref().or(detail).unwrap_or(rec);
                let z_zw = if shifted.is_some() || detail.is_some() { None } else { zw };
                self.phz.draw_ex(c, &zoom_model, zv, z.t0 - self.m.deskew_ch[c], z.dt, &z.ch[c], w, z_zw, z.pairs);
            }
        }
    }

    /// STOP on the hardware: the thread acquires nothing and reads the stored record again for every view (time base,
    /// delay), as much of it as the view shows: zoomed out the whole record, zoomed in every sample of the part on
    /// screen. The trigger stays where the last record had it (software trigger included).
    fn stop_replay(&mut self) {
        let ok = self.src.is_hw()
            && board().slow_mode(&self.m) == crate::board::Slow::Off
            && !self.m.la_on()
            && self.m.segments == 0
            && self.hist_pos.is_none()
            && self.last[0].as_ref().is_some_and(|r| r.raw.is_some());
        if !ok {
            self.src.hw_stop(None);
            self.stop_ti_off = None;
            (self.stop_raw, self.stop_pic) = (None, None);
            return;
        }
        // the software trigger of the record shown at the stop, as a time from the hardware trigger point
        if self.stop_ti_off.is_none() {
            // and that record itself when it is of samples: measured while the views come as pairs
            self.stop_raw = self.last[0].clone().filter(|r| r.raw.as_ref().is_some_and(|x| !x.frame.envelope));
            self.stop_pic = None;
            self.stop_ti_off = self.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|raw| {
                let f = &raw.frame;
                let ti = raw.i0 as f64 - raw.t0 / f.dt;
                (ti - f.fixed_ti.unwrap_or(f.pre as f64)) * f.dt
            });
        }
        let view = (self.m.time_per_div() as f64, self.m.delay as f64);
        let Some(mut f) = self.src.hw_stop(Some(view)) else { return };
        if let (Some(t), Some(off)) = (f.fixed_ti, self.stop_ti_off) {
            f.fixed_ti = Some(t + off / f.dt);
        }
        let window = view.0 * HDIV as f64;
        let rec = crate::board::to_record(&std::sync::Arc::new(f), &self.m, 3600, window, view.1 - window / 2.0);
        // a view read as min/max pairs (zoomed out on a record above `LIVE_READ`: the whole record compressed) after a
        // STOP on a raw record (the final frame of a deep record): the pairs are drawn, the raw record stays the one
        // measured and read out; pairs hold no Mean, RMS or Std dev (A3 of the 2026-11-16 review). A view read raw (far
        // zoomed in) is the record shown and measured, as before
        match self.stop_raw.as_ref().filter(|_| rec.raw.as_ref().is_some_and(|r| r.frame.envelope)) {
            Some(raw) => (self.last[0], self.stop_pic) = (Some(raw.clone()), Some(rec)),
            None => (self.last[0], self.stop_pic) = (Some(rec), None),
        }
        // the record counted when it came in RUN: its values for the view, not a new one for the statistics, the trend
        // and the catalog (as `refresh_stopped_measurements`)
        self.measure_last();
    }

    /// STOP: the pair record to draw for the view (`stop_pic`) while the raw record of the stop is the record shown.
    pub(super) fn stop_picture(&self) -> Option<&Record> {
        let frame = |r: Option<&Record>| r.and_then(|r| r.raw.as_ref()).map(|r| std::sync::Arc::as_ptr(&r.frame));
        let same = frame(self.last[0].as_ref()).is_some_and(|f| Some(f) == frame(self.stop_raw.as_ref()));
        self.stop_pic.as_ref().filter(|_| same)
    }

    /// The fast display path applies: hardware frames with a plain edge trigger and nothing that needs every record
    /// as a full record (averaging, a software mask test, zones, segments, zoom, XY, Z input, software coupling or filter).
    pub(super) fn fast_ok(&self) -> bool {
        let m = &self.m;
        matches!(self.src, Source::Hw { .. })
            && m.trig_type == TrigType::Edge
            && m.acq == AcqMode::Normal
            && m.trig_mode != TrigMode::Single
            && m.segments == 0
            && (!m.mask_on || self.mask_hw_active())
            && !m.zoom
            && !self.xy
            && m.z_src.is_none()
            && (!m.zones.iter().any(|z| z.usable()) || self.zone_hw_active())
            && board().slow_mode(m) == crate::board::Slow::Off
            && (0..NCH).all(|c| {
                !m.ch[c].on || (matches!(m.ch[c].coupling, Coupling::Dc | Coupling::Dc50) && m.ch[c].bw != BW_20M)
            })
    }

    /// The fast path for the queued frames (every one but the newest): placed on several cores (trigger, columns or
    /// screen record, statistics of each frame), then the maps filled in bands, channel by band on several cores.
    /// `helpers`: threads beside the frame loop's (`fast_helpers`). Returns the frames that counted as triggered.
    pub(super) fn fast_draw_all(&mut self, frames: Vec<crate::board::Frame>, helpers: usize) -> usize {
        let frames: Vec<std::sync::Arc<crate::board::Frame>> = frames.into_iter().map(std::sync::Arc::new).collect();
        let (w, h) = (self.ph.w, self.ph.h);
        let m = &self.m;
        // the statistics of every frame for the voltage measurements, once the measurements of the record have set
        // up their rows
        let rows_ready = self.mstat.len() == m.meas.len() + m.meas2.len();
        let fold: [bool; NCH] = std::array::from_fn(|c| {
            rows_ready
                && m.ch[c].on
                && m.meas.iter().any(|&(k, mc)| mc == WaveId::Ch(c) && crate::fastdraw::Fold::covers(k))
        });
        let t0 = std::time::Instant::now();
        let placed = crate::fastdraw::par_map(helpers, &frames, |f| fast_plan(m, w, h, f, fold));
        let t1 = std::time::Instant::now();
        let mut trig = 0;
        for (p, f) in placed.iter().zip(&frames) {
            trig += p.triggered as usize;
            if let FastPlan::Raw { class: Some(k), off, .. } = p.plan {
                self.fast_edges[k] += 1;
                if k == 2 {
                    self.fast_off = (self.fast_off.0 + off, self.fast_off.1.max(off));
                }
            }
            // as the deposit workers (`framestats::fold_frame`): nothing of live-compression pairs (the raw newest
            // frame of the batch counts), of other min/max pairs only what pairs give (Max, Min, Pk-pk)
            if rows_ready && !crate::compress::live(f) {
                for (i, &(k, src)) in self.m.meas.iter().enumerate() {
                    let Some(c) = src.ch() else { continue };
                    if f.envelope && crate::framestats::needs_samples(k) {
                        continue;
                    }
                    if let Some(v) = p.fold[c].and_then(|f| f.get(k)).filter(|v| v.is_finite()) {
                        self.mstat[i].add(v);
                    }
                }
            }
        }
        // the maps: each channel's frames collected as differences (`fastdraw::diff_span`), in bands of columns so one
        // channel alone also spreads over the cores, then summed into the map once
        let m = &self.m;
        let on: Vec<usize> = (0..NCH).filter(|&c| m.ch[c].on).collect();
        for &c in &on {
            if self.acc_t[c].len() != w * (h + 1) {
                self.acc_t[c] = vec![0; w * (h + 1)];
            }
        }
        let bands = if frames.len() < 2 || on.is_empty() { 1 } else { (helpers + 1).div_ceil(on.len()) };
        let band_cols = w.div_ceil(bands).max(1);
        let mut jobs: Vec<(usize, usize, &mut [u16])> = Vec::new();
        for (c, acc) in self.acc_t.iter_mut().enumerate() {
            if on.contains(&c) {
                jobs.extend(acc.chunks_mut(band_cols * (h + 1)).enumerate().map(|(b, band)| (c, b * band_cols, band)));
            }
        }
        crate::fastdraw::par_each(helpers, jobs, |(c, c_lo, band)| {
            for (f, p) in frames.iter().zip(&placed) {
                fast_deposit_t(m, (w, h), c, band, c_lo, f, &p.plan);
            }
        });
        // the rows the frames touched, per channel: only those are added (a trace covers a part of the height)
        let rows: [(usize, usize); NCH] = std::array::from_fn(|c| {
            placed.iter().fold((usize::MAX, 0), |(a, b), p| {
                let (x, y) = match &p.plan {
                    FastPlan::Raw { spans, .. } => spans[c].as_ref().map_or((0, h), crate::fastdraw::span_rows),
                    // a screen record: its points' rows (a segment between two points stays between them)
                    FastPlan::Rec(rec) => rec_rows(m, h, c, &rec.ch[c]),
                };
                (a.min(x), b.max(y))
            })
        });
        let t2 = std::time::Instant::now();
        let pairs: Vec<(usize, &mut Vec<u16>, &mut Vec<u16>)> = self
            .ph
            .map
            .iter_mut()
            .zip(self.acc_t.iter_mut())
            .enumerate()
            .filter(|(c, _)| on.contains(c))
            .map(|(c, (a, b))| (c, a, b))
            .collect();
        crate::fastdraw::par_each(helpers, pairs, |(c, map, acc)| {
            crate::fastdraw::fold_t(map, acc, w, h, band_cols, rows[c].0..rows[c].1)
        });
        let (a, b) = ((t1 - t0).as_secs_f32() * 1e3, t1.elapsed().as_secs_f32() * 1e3);
        self.t_fast_ab.0 += 0.05 * (a - self.t_fast_ab.0);
        self.t_fast_ab.1 += 0.05 * (b - self.t_fast_ab.1);
        self.t_fast_fold += 0.05 * (t2.elapsed().as_secs_f32() * 1e3 - self.t_fast_fold);
        self.fast_n += 0.05 * (frames.len() as f32 - self.fast_n);
        trig
    }
}

/// The fast path's take for the next screen after drawing `n` frames in `ms` with `budget` ms (`Ui::fast_max`).
pub(super) fn fast_take_next(take: f32, n: usize, ms: f32, budget: f32) -> f32 {
    let est = n as f32 * budget / ms;
    let limited = n >= (take as usize).clamp(4, crate::board::QUEUE_MAX);
    if ms > budget || (limited && est > take) {
        (take + 0.5 * (est - take)).clamp(4.0, crate::board::QUEUE_MAX as f32)
    } else {
        take
    }
}

/// What the fast display path does with one frame (`fast_plan`).
pub enum FastPlan {
    /// sparse views (fewer than 2 samples a column), envelopes, roll and scan: a screen record, drawn as a trace
    /// (made here, so the interpolation is spread over the frames, not the channels)
    Rec(crate::wave::Record),
    /// dense views: the raw samples with the trigger at sample `ti`; `class`: the `Ui::fast_edges` index (None: the
    /// source is an LA line), `off`: how far the edge was from the hardware point (class 2); per channel the columns'
    /// spans (None: dots, drawn sample by sample)
    Raw { ti: f64, class: Option<usize>, off: f64, spans: [Option<crate::fastdraw::Spans>; NCH] },
}

/// One frame placed by `fast_plan`: how to draw it, whether it counts as triggered (false if its trigger edge was not
/// found near the hardware trigger point), and per channel the voltage values of the statistics (where wanted).
pub struct FastFrame {
    pub plan: FastPlan,
    pub triggered: bool,
    pub fold: [Option<crate::fastdraw::Fold>; NCH],
}

/// Helper threads of the fast path (`Ui::fast_draw_all`): none while the deposit workers of batch mode have batches
/// to draw, where they keep the small cores busy: a helper there waited for a core, and the frame loop for the helper
/// (issue #186); the queue then holds only the newest frame of each batch, one or two a screen. Without batches to
/// draw (batches of one frame, at slow time bases) every frame comes through the queue, and the helpers draw them as
/// in the frames mode: the frame loop alone took fewer of them a screen and left the others out (unit 2026-11-18).
pub(super) fn fast_helpers() -> usize {
    if crate::drawpipe::pipe().is_some_and(|p| p.busy()) {
        0
    } else {
        crate::fastdraw::helpers()
    }
}

/// Where channel `c` of a dense frame lands on the `w` x `h` map with the trigger at sample `ti`.
fn raw_map(m: &Model, w: usize, h: usize, c: usize, f: &crate::board::Frame, ti: f64) -> crate::fastdraw::Map {
    let x_per_sample = f.dt / m.time_per_div() as f64 * w as f64 / HDIV as f64;
    crate::drawpipe::screen_map(m, w, h, c, x_per_sample, ti)
}

/// One frame of the fast path placed on the screen (`w` x `h` map): the trigger, the columns of a dense frame or the
/// screen record of a sparse one, and the statistics of the channels in `fold`. Touches nothing but the frame, so the
/// frames of a batch are placed on several cores; this is where most of the work is.
pub fn fast_plan(
    m: &Model,
    w: usize,
    h: usize,
    f: &std::sync::Arc<crate::board::Frame>,
    fold: [bool; NCH],
) -> FastFrame {
    let tdiv = m.time_per_div() as f64;
    let window = tdiv * HDIV as f64;
    let t_first = m.delay as f64 - window / 2.0;
    let x_per_sample = f.dt / tdiv * w as f64 / HDIV as f64;
    if x_per_sample > 0.5 || f.envelope || f.roll || f.scan {
        // the record's 3600 points: fewer thin out the picture of a fast signal (at 8 samples a period, two points a
        // column lit a fifth fewer pixels than 3600, and those a sixth fewer than a nearly continuous curve)
        let rec = crate::board::to_record(f, m, 3600, window, t_first);
        // the statistics on the screen record, as the measurements of a sparse record
        let fold = std::array::from_fn(|c| (fold[c] && m.ch[c].on).then(|| crate::fastdraw::Fold::of(&rec.ch[c])));
        let triggered = rec.triggered;
        return FastFrame { plan: FastPlan::Rec(rec), triggered, fold };
    }
    let src = &f.ch[m.trig_src];
    let col = (1.0 / x_per_sample).floor().clamp(1.0, 256.0) as usize;
    // the full search of `fastdraw::edge_smooth` (the Batch path takes `trignear::edge_smooth_near`)
    let t = crate::drawpipe::refine_edge(m, f, col, |s| crate::fastdraw::edge_smooth(src, f.pre, s, |_| true));
    let (class, off) = t.class.map_or((None, 0.0), |(k, d)| (Some(k), d));
    let ti = t.ti;
    let used = |c: usize| m.ch[c].on && !f.ch[c].is_empty();
    let spans = std::array::from_fn(|c| {
        (used(c) && !m.dots).then(|| crate::fastdraw::spans(w, h, &f.ch[c], &raw_map(m, w, h, c, f, ti)))
    });
    // the statistics on the samples of the screen window, as the measurements of a dense record
    let fold = std::array::from_fn(|c| {
        (fold[c] && used(c)).then(|| {
            let v = &f.ch[c];
            let i0 = ((ti + t_first / f.dt).floor().max(0.0) as usize).min(v.len());
            let i1 = ((ti + (t_first + window) / f.dt).ceil().max(0.0) as usize).min(v.len());
            crate::fastdraw::Fold::of(&v[i0..i1.max(i0)])
        })
    });
    FastFrame { plan: FastPlan::Raw { ti, class, off, spans }, triggered: t.triggered, fold }
}

/// The rows (first, last + 1) the points `v` of channel `c` of a screen record fall on, within a map `h` high.
fn rec_rows(m: &Model, h: usize, c: usize, v: &[f32]) -> (usize, usize) {
    let ch = &m.ch[c];
    let (vpd, ppd_y) = (ch.volts_per_div(), h as f32 / VDIV);
    let (lo, hi) =
        v.iter().filter(|x| x.is_finite()).fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
    if lo > hi {
        return (usize::MAX, 0);
    }
    let y = |val: f32| h as f32 / 2.0 - (val + ch.offset) / vpd * ppd_y;
    // rounded as `wave::trace_columns`, one row of slack each side
    let (a, b) = (y(hi).round() - 1.0, y(lo).round() + 1.0);
    (a.max(0.0) as usize, (b.max(-1.0) + 1.0).min(h as f32) as usize)
}

/// Channel `c` of a placed frame into a band of its collected differences (`fastdraw::diff_span`): the columns
/// `c_lo ..` of a map `w` x `h`, `h + 1` rows of them.
pub fn fast_deposit_t(
    m: &Model,
    (w, h): (usize, usize),
    c: usize,
    band: &mut [u16],
    c_lo: usize,
    f: &crate::board::Frame,
    p: &FastPlan,
) {
    if !m.ch[c].on {
        return;
    }
    match p {
        FastPlan::Rec(rec) => {
            if !rec.ch[c].is_empty() {
                let bw = band.len() / (h + 1);
                let c_hi = (c_lo + bw) as i32;
                let trace = crate::wave::Trace::record(c, m, rec);
                crate::wave::trace_columns(w, h, &trace, (c_lo as i32, c_hi), |cx, r0, r1, wt| {
                    crate::fastdraw::diff_span(band, bw, cx as usize - c_lo, r0 as usize, r1 as usize, wt);
                });
            }
        }
        FastPlan::Raw { ti, spans, .. } => match &spans[c] {
            Some(s) => crate::fastdraw::add_spans_t(band, h, c_lo, s, 24),
            None if !f.ch[c].is_empty() => {
                crate::fastdraw::deposit_dots_t(band, h, c_lo, &f.ch[c], &raw_map(m, w, h, c, f, *ti), 24)
            }
            None => {}
        },
    }
}

/// Channel `c` of a placed frame into that channel's whole map `map` (`w` x `h`): one frame after the other straight
/// into the row-major map, the tests' reference for the fast path's drawing.
#[cfg(test)]
pub fn fast_deposit(m: &Model, w: usize, h: usize, c: usize, map: &mut [u16], f: &crate::board::Frame, p: &FastPlan) {
    if !m.ch[c].on {
        return;
    }
    match p {
        FastPlan::Rec(rec) => {
            if !rec.ch[c].is_empty() {
                crate::wave::draw_trace(map, w, h, 0, &crate::wave::Trace::record(c, m, rec));
            }
        }
        FastPlan::Raw { ti, spans, .. } => match &spans[c] {
            Some(s) => crate::fastdraw::add_spans(map, w, 0, s, 24),
            None if !f.ch[c].is_empty() => {
                crate::fastdraw::deposit_dots_band(map, w, 0, h, &f.ch[c], &raw_map(m, w, h, c, f, *ti), 24)
            }
            None => {}
        },
    }
}
