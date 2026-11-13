// SPDX-License-Identifier: GPL-2.0-or-later
//! A live channel plus stored bus roles is judged before accepting the capture.
//! Keep one candidate, share the existing decoder worker, and leave the last
//! accepted picture/readouts untouched while a long candidate is computing.
use super::*;

#[derive(Default)]
pub(super) struct State {
    pub(super) record: Option<Record>,
    pub(super) epoch: u64,
    guard: String,
    force: bool,
    minimum_stamp: u64,
    ready_hit: Option<f64>,
    last_hit: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::{Arc, Mutex}, time::{Duration, Instant}};

    fn data(points: usize, byte: u8) -> Vec<f32> {
        let mut data = vec![0.0; points];
        for frame in 0..32 {
            let start = points / 2 + 100 + frame * 100;
            for bit in 0..8 {
                data[start + bit * 8..start + (bit + 1) * 8].fill(if byte >> (7 - bit) & 1 != 0 { 3.3 } else { 0.0 });
            }
        }
        data
    }
    fn frame(points: usize, byte: u8, sequence: u64, stamp: u64) -> Arc<crate::board::Frame> {
        Arc::new(crate::board::Frame { seq: sequence, stamp, dt: 1e-9, pre: points / 2,
            ch: [data(points, byte), Vec::new(), Vec::new(), Vec::new()], ..Default::default() })
    }
    fn fixture(points: usize) -> (Ui, crate::board::HwShared) {
        let shared = Arc::new(Mutex::new(crate::board::Shared::default()));
        let mut ui = Ui::new(Source::Hw { shared: Arc::clone(&shared), seen: 0 });
        ui.m.tscale = tscales().iter().position(|&scale| scale >= 10e-9).unwrap();
        ui.m.view = AView::Eye;
        ui.m.eye_src = WaveId::Ch(0); ui.m.eye_rate = 125e6; ui.m.eye_from_record = false;
        ui.m.ch[0].bw = BW_FULL; ui.m.ch[0].coupling = Coupling::Dc;
        let old = Arc::new(crate::board::Frame { seq: 1, stamp: 0, dt: 1e-9, pre: 2048,
            ch: [(0..4096).map(|i| if i % 16 < 8 { 0.0 } else { 3.3 }).collect(), Vec::new(), Vec::new(), Vec::new()], ..Default::default() });
        shared.lock().unwrap().queue.push_back(Arc::clone(&old));
        shared.lock().unwrap().frame = old;
        ui.step(0.05);
        assert_eq!(ui.records, 1);
        assert!(ui.azr.eye.as_ref().is_some_and(|eye| eye.records > 0), "seed a genuine accepted Eye record");
        let mut clock = vec![0.0; points];
        for frame in 0..32 {
            let start = points / 2 + 100 + frame * 100;
            for bit in 0..8 { clock[start + bit * 8 + 2..start + bit * 8 + 6].fill(3.3); }
        }
        ui.refs[0] = Some(RefWave { src: 0, color: 0, label: "stored SCK".into(), v: (clock).into(),
            t0: -(points as f64 / 2.0) * 1e-9, dt: 1e-9, vpd: 0.5, offset: 0.0, on: true });
        ui.m.dec_on = true; ui.m.dec_proto = Proto::Spi; ui.m.dec_src = 0; ui.m.dec_src2 = DEC_REF_BASE;
        ui.m.dec_cs = None; ui.m.dec_bits = 8; ui.m.dec_mode = 0; ui.m.dec_thr = 1.65;
        ui.m.trig_type = TrigType::Bus; ui.m.trig_mode = TrigMode::Normal;
        ui.m.trig_bus = BusWhat::Data; ui.m.trig_bus_val = 0xa5;
        ui.relayout(); ui.dirty_map = false;
        assert!(ui.mixed_bus_mode());
        (ui, shared)
    }
    fn push(ui: &Ui, shared: &crate::board::HwShared, points: usize, byte: u8, sequence: u64) {
        let frame = frame(points, byte, sequence, ui.m.hw_stamp);
        let mut state = shared.lock().unwrap();
        state.frame = Arc::clone(&frame); state.queue.push_back(frame);
    }
    fn until(ui: &mut Ui, condition: impl Fn(&Ui) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !condition(ui) {
            assert!(Instant::now() < deadline, "bounded mixed Bus acquisition did not resolve");
            ui.step(0.002); std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn single_preserves_picture_statistics_eye_and_fifo_until_one_matched_candidate_commits() {
        let points = 300_000;
        let (mut ui, shared) = fixture(points);
        assert!(ui.mixed_bus_command(2)); ui.step(0.001); // establish the fresh hardware arm stamp
        let before = (ui.records, ui.wfm_total, ui.azr.eye.as_ref().unwrap().records);
        let statistics = ui.measurement_statistics_snapshot_for_test();
        let picture = ui.ph.map.clone();
        let old = Arc::clone(&ui.last[0].as_ref().unwrap().raw.as_ref().unwrap().frame);
        push(&ui, &shared, points, 0xa5, 2); push(&ui, &shared, points, 0xa5, 3);
        ui.step(0.001);
        assert!(ui.bus_candidate_waiting());
        assert_eq!(ui.records, before.0); assert_eq!(ui.wfm_total, before.1);
        assert_eq!(ui.azr.eye.as_ref().unwrap().records, before.2);
        assert_eq!(ui.measurement_statistics_snapshot_for_test(), statistics);
        assert_eq!(ui.ph.map, picture);
        assert!(Arc::ptr_eq(&old, &ui.last[0].as_ref().unwrap().raw.as_ref().unwrap().frame));
        assert_eq!(shared.lock().unwrap().queue.len(), 1, "only one candidate may leave the FIFO while decoding");
        until(&mut ui, |ui| ui.records > before.0);
        assert_eq!(ui.records, before.0 + 1); assert_eq!(ui.wfm_total, before.1 + 1);
        assert_eq!(ui.m.run, RunState::Stop); assert_eq!(ui.m.status, TrigStatus::Stop);
        assert!(ui.last[0].as_ref().unwrap().triggered, "accepted record must carry the real Bus trigger decision");
        let raw = ui.last[0].as_ref().unwrap().raw.as_ref().unwrap();
        assert_eq!(raw.frame.seq, 2);
        assert!((raw.t0 - raw.i0 as f64 * raw.frame.dt - ui.refs[0].as_ref().unwrap().t0).abs() < 1e-15);
        assert_eq!(shared.lock().unwrap().queue.len(), 1);
        assert_eq!(ui.decoder_worker_state_for_test(), (1, 1), "one worker and no accepted-frame rescan");
        let hit = ui.bus_candidate.last_hit.unwrap();
        assert!(ui.search_events().iter().any(|time| (*time - hit).abs() < 1e-15));
        let stopped = (ui.records, ui.wfm_total, ui.azr.eye.as_ref().unwrap().records);
        for _ in 0..4 { ui.step(0.02); }
        assert_eq!((ui.records, ui.wfm_total, ui.azr.eye.as_ref().unwrap().records), stopped);
    }

    #[test]
    fn no_match_waits_and_force_accepts_one_without_fabricating_a_bus_match() {
        let points = 300_000;
        let (mut ui, shared) = fixture(points);
        ui.m.trig_bus_val = 0x5a;
        ui.mixed_bus_command(2); ui.step(0.001);
        let before = ui.records;
        let eye = ui.azr.eye.as_ref().unwrap().records;
        push(&ui, &shared, points, 0xa5, 2); ui.step(0.001);
        assert!(ui.bus_candidate_waiting());
        until(&mut ui, |ui| !ui.bus_candidate_waiting());
        assert_eq!(ui.records, before); assert_eq!(ui.m.run, RunState::Run);
        assert_eq!(ui.m.status, TrigStatus::Wait); assert_eq!(ui.azr.eye.as_ref().unwrap().records, eye);
        push(&ui, &shared, points, 0xa5, 3); ui.step(0.001);
        assert!(ui.bus_candidate_waiting());
        ui.key(key::FORCE); ui.step(0.001);
        assert_eq!(ui.records, before + 1); assert_eq!(ui.m.run, RunState::Stop);
        assert_eq!(ui.m.status, TrigStatus::Stop); assert_eq!(ui.m.trig_bus_val, 0x5a);
        assert!(!ui.last[0].as_ref().unwrap().triggered, "Force must not fabricate a Bus match");
        assert_eq!(ui.last[0].as_ref().unwrap().raw.as_ref().unwrap().frame.seq, 3);
        assert!(ui.bus_candidate.last_hit.is_none()); assert!(!ui.bus_candidate.force);
    }

    #[test]
    fn changed_condition_stop_and_rearm_reject_stale_candidates_and_old_stamp_frames() {
        let points = 300_000;
        let (mut ui, shared) = fixture(points);
        ui.mixed_bus_command(2); ui.step(0.001);
        let before = ui.records;
        push(&ui, &shared, points, 0xa5, 2); push(&ui, &shared, points, 0x5a, 3); ui.step(0.001);
        assert!(ui.bus_candidate_waiting());
        ui.m.trig_bus_val = 0x5a; ui.mark_changed();
        assert!(!ui.bus_candidate_waiting());
        ui.step(0.001);
        assert_eq!(ui.records, before); assert!(shared.lock().unwrap().queue.is_empty(), "old arm's queued record must be rejected");
        push(&ui, &shared, points, 0x5a, 4); push(&ui, &shared, points, 0x5a, 5); ui.step(0.001);
        assert!(ui.bus_candidate_waiting());
        ui.key(key::RUNSTOP); assert_eq!(ui.m.run, RunState::Stop); assert!(!ui.bus_candidate_waiting());
        ui.step(0.001); assert_eq!(ui.records, before);
        ui.key(key::SINGLE); ui.step(0.001);
        assert!(shared.lock().unwrap().queue.is_empty()); assert_eq!(ui.records, before);
        push(&ui, &shared, points, 0x5a, 6); ui.step(0.001);
        until(&mut ui, |ui| ui.records > before);
        assert_eq!(ui.records, before + 1); assert_eq!(ui.m.run, RunState::Stop);
        assert_eq!(ui.last[0].as_ref().unwrap().raw.as_ref().unwrap().frame.seq, 6);
        assert_eq!(ui.decoder_worker_state_for_test().0, 1);
    }

    #[test]
    fn real_scpi_queries_keep_pending_candidate_and_force_uses_the_live_bus_path() {
        use std::io::{BufRead, Write};
        let points = 300_000;
        let (mut ui, shared) = fixture(points);
        let before = ui.records;
        push(&ui, &shared, points, 0xa5, 2); ui.step(0.001);
        assert!(ui.bus_candidate_waiting());
        let epoch = ui.bus_candidate.epoch;
        let statistics = ui.measurement_statistics_snapshot_for_test();
        let mut scpi = crate::scpi::Scpi::open(0);
        let mut socket = std::net::TcpStream::connect(("127.0.0.1", scpi.port().unwrap())).unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        socket.write_all(b":RUN;:TRIG:STAT?;:TIM:SCAL?;:CHAN1:DISP?\n").unwrap();
        scpi.poll(&mut ui, &Canvas::new(W, H));
        let mut response = String::new(); std::io::BufReader::new(socket.try_clone().unwrap()).read_line(&mut response).unwrap();
        assert_eq!(response.trim().split(';').count(), 3);
        assert!(ui.bus_candidate_waiting()); assert_eq!(ui.bus_candidate.epoch, epoch);
        assert_eq!(ui.measurement_statistics_snapshot_for_test(), statistics); assert_eq!(ui.records, before);
        socket.write_all(b":STOP;:TRIG:STAT?\n").unwrap(); scpi.poll(&mut ui, &Canvas::new(W, H));
        response.clear(); std::io::BufReader::new(socket.try_clone().unwrap()).read_line(&mut response).unwrap();
        assert_eq!(ui.m.run, RunState::Stop); assert!(!ui.bus_candidate_waiting());
        ui.m.trig_bus_val = 0x5a;
        socket.write_all(b":SINGLE;*OPC?\n").unwrap(); scpi.poll(&mut ui, &Canvas::new(W, H));
        response.clear(); std::io::BufReader::new(socket.try_clone().unwrap()).read_line(&mut response).unwrap(); ui.step(0.001);
        push(&ui, &shared, points, 0xa5, 3);
        socket.write_all(b":TRIG:FORC;*OPC?\n").unwrap(); scpi.poll(&mut ui, &Canvas::new(W, H));
        response.clear(); std::io::BufReader::new(socket.try_clone().unwrap()).read_line(&mut response).unwrap(); ui.step(0.001);
        assert_eq!(ui.records, before + 1); assert_eq!(ui.m.run, RunState::Stop); assert_eq!(ui.m.status, TrigStatus::Stop);
        assert!(!ui.last[0].as_ref().unwrap().triggered);
        assert_eq!(ui.last[0].as_ref().unwrap().raw.as_ref().unwrap().frame.seq, 3);
    }

    #[test]
    fn bus_primary_source_switches_between_stored_and_live_without_losing_auxiliary_refs() {
        let (mut ui, _) = fixture(10_000);
        let metadata = { let reference = ui.refs[0].as_ref().unwrap(); (reference.dt, reference.t0, reference.v.len()) };
        assert!(ui.configure_decoder_source(0, Some(DEC_REF_BASE)));
        assert!(ui.record_mode()); assert_eq!(ui.m.trig_ref, Some(0));
        assert!(ui.configure_decoder_source(0, Some(0)));
        assert!(!ui.record_mode()); assert!(ui.mixed_bus_mode()); assert_eq!(ui.m.trig_ref, None);
        assert_eq!(ui.m.dec_src2, DEC_REF_BASE);
        let reference = ui.refs[0].as_ref().unwrap();
        assert_eq!((reference.dt, reference.t0, reference.v.len()), metadata);
        ui.set_offline(true); ui.m.set_trigger_wave(WaveId::Ref(0), false);
        ui.trigger_type_set(TrigType::Bus);
        assert!(!ui.record_mode(), "Bus activation must follow its CH primary rather than a stale reference workspace");
    }
}

impl Ui {
    pub(super) fn mixed_bus_mode(&self) -> bool {
        !self.record_mode() && self.m.trig_type == TrigType::Bus && self.m.dec_src < NCH
            && Self::decoder_uses_refs(&self.m) && board().slow_mode(&self.m) == crate::board::Slow::Off
    }
    fn candidate_guard(&self, record: &Record) -> String {
        format!("{}|{}|{}|{:?}|{}|{:?}|{}", self.bus_candidate.epoch, self.workflow_epoch, self.m.hw_stamp,
            (self.m.trig_bus, self.m.trig_bus_val, self.m.trig_mode),
            crate::store::setup_text(&self.m), self.m.run, self.decoder_key_at(&self.m, Some(record)).unwrap_or_default())
    }
    pub(super) fn cancel_bus_candidate(&mut self, clear_event: bool) {
        let active = self.bus_candidate.record.is_some() || self.mixed_bus_mode();
        self.bus_candidate.record = None;
        self.bus_candidate.guard.clear();
        self.bus_candidate.force = false;
        self.bus_candidate.ready_hit = None;
        self.bus_candidate.epoch = self.bus_candidate.epoch.wrapping_add(1);
        if clear_event { self.bus_candidate.last_hit = None; }
        if active {
            self.m.hw_stamp = self.m.hw_stamp.wrapping_add(1);
            self.bus_candidate.minimum_stamp = self.m.hw_stamp;
        }
    }
    pub(super) fn bus_candidate_guard_tick(&mut self) {
        let mixed = self.mixed_bus_mode();
        let stale = self.bus_candidate.record.as_ref().is_some_and(|record|
            !mixed || self.m.run != RunState::Run || self.candidate_guard(record) != self.bus_candidate.guard);
        if stale { self.cancel_bus_candidate(!mixed || self.m.run == RunState::Run); }
        if mixed { self.bus_candidate.minimum_stamp = self.bus_candidate.minimum_stamp.max(self.m.hw_stamp); }
        else { self.bus_candidate.last_hit = None; self.bus_candidate.ready_hit = None; self.bus_candidate.force = false; }
    }
    pub(super) fn bus_candidate_waiting(&self) -> bool { self.bus_candidate.record.is_some() }
    pub(super) fn bus_candidate_key(&mut self, key_code: u8) {
        if matches!(key_code, key::DEFAULT | 0x15..=0x18) { self.cancel_bus_candidate(true); return; }
        if !self.mixed_bus_mode() { return; }
        match key_code {
            key::RUNSTOP => self.cancel_bus_candidate(false),
            key::SINGLE => { self.cancel_bus_candidate(true); self.single_armed = false; self.m.status = TrigStatus::Wait; }
            key::FORCE => { self.bus_candidate.force = true; self.m.status = TrigStatus::Wait; }
            _ => {}
        }
    }
    /// Remote acquisition commands bypass modal UI input but retain assignment
    /// semantics, including a repeated RUN leaving the pending candidate alone.
    pub(crate) fn mixed_bus_command(&mut self, command: u8) -> bool {
        if !self.mixed_bus_mode() { return false; }
        match command {
            0 => {
                if self.m.run != RunState::Stop { self.cancel_bus_candidate(false); }
                self.m.run = RunState::Stop; self.m.status = TrigStatus::Stop;
            }
            1 => {
                if self.m.run != RunState::Run { self.cancel_bus_candidate(false); }
                if self.m.trig_mode == TrigMode::Single { self.m.trig_mode = TrigMode::Normal; }
                self.m.run = RunState::Run; self.m.status = TrigStatus::Wait;
            }
            2 => {
                self.cancel_bus_candidate(true);
                self.single_armed = false;
                self.m.trig_mode = TrigMode::Single; self.m.run = RunState::Run; self.m.status = TrigStatus::Wait;
                self.m.floor_live_tdiv();
                self.dirty_map = true;
            }
            3 => {
                self.bus_candidate.force = true; self.m.run = RunState::Run; self.m.status = TrigStatus::Wait;
            }
            _ => return false,
        }
        true
    }
    fn acquire_mixed_bus_record(&mut self, elapsed: f64) -> Option<Record> {
        if let Source::Hw { shared, seen } = &mut self.src {
            let mut candidate_model = self.m.clone();
            candidate_model.trig_type = TrigType::Edge; candidate_model.trig_mode = TrigMode::Auto;
            candidate_model.trig_src = candidate_model.dec_src; candidate_model.trig_ref = None;
            candidate_model.trig_level = candidate_model.dec_thr; candidate_model.slope = Slope::Rise;
            let mut state = shared.lock().unwrap();
            state.want = Some(board().want(&candidate_model));
            state.slow = crate::board::Slow::Off; state.stamp = self.m.hw_stamp;
            let frame = state.queue.pop_front()?;
            *seen = frame.seq;
            drop(state);
            if frame.stamp < self.bus_candidate.minimum_stamp || !frame.dt.is_finite() || frame.dt <= 0.0 { return None; }
            let window = self.m.time_per_div() as f64 * HDIV as f64;
            // Preserve the hardware's original time zero. Refining an Edge on
            // CH alone here would shift it relative to the fixed R roles.
            return Some(crate::board::resample_record(&frame, &self.m, 3600, window,
                self.m.delay as f64 - window * 0.5, frame.pre as f64, false));
        }
        let previous = (self.m.trig_type, self.m.trig_mode, self.m.trig_src, self.m.trig_ref, self.m.trig_level, self.m.slope);
        self.m.trig_type = TrigType::Edge; self.m.trig_mode = TrigMode::Auto;
        self.m.trig_src = self.m.dec_src; self.m.trig_ref = None;
        self.m.trig_level = self.m.dec_thr; self.m.slope = Slope::Rise;
        let record = self.acquire_display_record(elapsed);
        (self.m.trig_type, self.m.trig_mode, self.m.trig_src, self.m.trig_ref, self.m.trig_level, self.m.slope) = previous;
        record
    }
    pub(super) fn acquire_bus_candidate(&mut self, elapsed: f64) -> Option<Record> {
        if !self.mixed_bus_mode() { return self.acquire_display_record(elapsed); }
        self.bus_candidate_guard_tick();
        if self.m.run != RunState::Run { return None; }
        if self.bus_candidate.record.is_none() {
            // The hardware cannot read reference memory. Acquire edge/auto
            // candidates, then apply the actual Bus condition here. SINGLE's
            // arm stamp is checked explicitly before a candidate is retained.
            let record = self.acquire_mixed_bus_record(elapsed)?;
            if record.raw.as_ref().is_some_and(|raw| raw.frame.stamp < self.bus_candidate.minimum_stamp) {
                return None;
            }
            self.bus_candidate.guard = self.candidate_guard(&record);
            self.bus_candidate.record = Some(record);
        }
        if self.bus_candidate.force {
            self.bus_candidate.force = false;
            self.bus_candidate.ready_hit = None;
            let mut record = self.bus_candidate.record.take()?;
            record.triggered = false;
            return Some(record);
        }
        let record = self.bus_candidate.record.as_ref()?;
        let packets = self.reference_decode_packets_at(&self.m, Some(record));
        let packets = match packets {
            Err(super::reference_decode::COMPUTING) => { self.m.status = TrigStatus::Wait; return None; }
            result => result,
        };
        let hit = packets.as_ref().ok().and_then(|packets| packets.iter().filter(|packet| crate::trig::matches(&self.m, packet))
            .map(|packet| packet.t1).min_by(|a, b| a.abs().total_cmp(&b.abs())));
        if hit.is_none() && self.m.trig_mode != TrigMode::Auto {
            self.bus_candidate.record = None;
            self.m.status = TrigStatus::Wait;
            return None;
        }
        let mut record = self.bus_candidate.record.take()?;
        record.triggered = hit.is_some();
        self.bus_candidate.ready_hit = hit;
        if let Some(time) = hit {
            let half = self.m.time_per_div() as f64 * HDIV as f64 * 0.5;
            if time < self.m.delay as f64 - half || time >= self.m.delay as f64 + half {
                self.m.delay = time as f32;
                if let Some(display) = self.captured_record_view(&record, (self.m.time_per_div(), self.m.delay)) {
                    record = display.as_ref().clone();
                    record.triggered = true;
                }
            }
        }
        // Retain original hardware/ref origins. Packet times therefore remain
        // aligned across mixed sources and can be reused by bands and Search.
        if let Ok(packets) = packets { self.seed_accepted_decode(&record, packets); }
        Some(record)
    }
    pub(super) fn bus_candidate_committed(&mut self) {
        if self.mixed_bus_mode() { self.bus_candidate.last_hit = self.bus_candidate.ready_hit.take(); }
    }
    pub(super) fn draw_bus_event(&self, canvas: &mut Canvas) {
        if !self.mixed_bus_mode() { return; }
        let Some(time) = self.bus_candidate.last_hit else { return; };
        if self.source_area(WaveId::Ch(self.m.dec_src)).is_some_and(|area| area != self.area) { return; }
        let area = self.area;
        let x = self.x_of_time(time as f32);
        if x < area.x || x >= area.x + area.w { return; }
        canvas.set_clip(area);
        canvas.dashed_vline(x, area.y + 21, area.y + area.h - 1, 2, 6, AMBER);
        canvas.text(&self.fonts.sb, (x + 4).min(area.x + area.w - 66), area.y + 31, "Bus event", AMBER);
        canvas.no_clip();
    }
}
