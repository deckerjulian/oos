//! Generated bus data must qualify before replacing the accepted SIM record.
//! The controlled worker keeps the pending phase deterministic: no timing sleeps,
//! random PRBS payloads, native GUI, or live SCPI endpoint are involved.
use super::*;
use crate::wave::Source;
use oos_dsp::{decode::Kind as PacketKind, sim::{i2c_logic, sample, I2cXfer}};

const RATE: f64 = 100_000.0;
const DT: f64 = 100e-9;
const POINTS: usize = 20_000;

fn data(payload: u8) -> (Vec<f32>, Vec<f32>) {
    let messages = (0..8).map(|_| I2cXfer { addr: 0x50, read: false,
        data: vec![payload], repeated: false, stop: true }).collect::<Vec<_>>();
    let (sda, scl) = i2c_logic(&messages, RATE, 0.0, 3.3);
    // The stored clock deliberately has a different grid and negative origin.
    // It overlaps the entire generated record, whose native origin is zero.
    (sample(|t| sda.v(t), 0.0, DT, POINTS),
        sample(|t| scl.v(t), -4.0 * DT, 2.0 * DT, POINTS / 2 + 4))
}

fn reference(label: &str, values: Vec<f32>, dt: f64, t0: f64) -> RefWave {
    RefWave { src: 0, color: 0, label: label.into(), t0, dt,
        v: values.into(), vpd: 0.5, offset: -1.65, on: true }
}

fn fixture() -> (Ui, Vec<f32>, Vec<f32>) {
    let mut ui = Ui::new(Source::Demo { t: 0.0 });
    let (old, clock) = data(0x5a);
    let (fresh, same_clock) = data(0xa5);
    assert_eq!(clock, same_clock, "payload changes must not alter the clock fixture");
    ui.refs[0] = Some(reference("old SDA 5A", old.clone(), DT, 0.0));
    ui.refs[1] = Some(reference("stored SCL", clock, 2.0 * DT, -4.0 * DT));
    for channel in &mut ui.m.ch { channel.on = false; }
    ui.m.la.on = false;
    ui.offline = false;
    ui.session_is_sim = true;
    ui.sim_play = true;
    ui.m.sim_slot = 0;
    ui.m.sim_rate = 1.0 / (8.0 * DT);
    ui.m.sim_vhigh = 3.3;
    ui.m.sim_vlow = 0.0;
    ui.m.sim_bw = 2.0 * ui.m.sim_rate;
    ui.m.sim_rise = DT;
    ui.m.sim_deemph = 0.0;
    ui.m.sim_ssc = false;
    ui.m.sim_noise = 0.0;
    ui.m.sim_rj = 0.0;
    ui.m.sim_pj = 0.0;
    ui.m.sim_dcd_pp = 0.0;
    ui.m.sim_corr_rj = 0.0;
    assert_eq!(serial::dt_of(&ui.sim_lane()), DT);
    serial::sample_count(&ui.sim_lane()).expect("the controlled low-rate lane must pass the real Play preflight");
    ui.m.dec_on = true;
    ui.m.dec_la_in = false;
    ui.m.dec_proto = Proto::I2c;
    ui.m.dec_src = DEC_REF_BASE;
    ui.m.dec_src2 = DEC_REF_BASE + 1;
    ui.m.dec_diff = None;
    ui.m.dec_cs = None;
    ui.m.dec_thr = 1.65;
    ui.m.trig_type = TrigType::Bus;
    ui.m.trig_bus = BusWhat::Data;
    ui.m.trig_bus_val = 0x5a;
    ui.m.set_trigger_wave(WaveId::Ref(0), false);
    ui.m.trig_ref2 = None;
    ui.m.trig_mode = TrigMode::Normal;
    ui.m.run = RunState::Run;
    ui.m.status = TrigStatus::Wait;
    ui.m.zoom = false;
    ui.m.tscale = nearest_tscale(50e-6);
    ui.m.delay = 250e-6;
    ui.m.meas_full_record = true;
    ui.m.meas = vec![(Meas::Vpp, WaveId::Ref(0)), (Meas::Vrms, WaveId::Ref(0))];
    ui.m.meas2.clear();
    ui.m.cat.clear();
    ui.ensure_meas_own();
    ui.m.view = AView::Eye;
    ui.m.eye_src = WaveId::Ref(0);
    ui.m.eye_rate = RATE;
    ui.m.eye_from_record = false;
    ui.azr.eye_auto.enabled = false;
    ui.replay.sim_next = f64::INFINITY;
    // Establish genuine accepted measurement statistics and an accumulated Eye,
    // then request the opposite payload. No fabricated counters are seeded.
    let request = ui.sim_capture_request(ui.sim_lane(), 0, true, true, true);
    let generated = Generated::evaluate(&request, old.clone());
    assert!(generated.error.is_none());
    assert!(!generated.hits.as_ref().unwrap().1.is_empty());
    ui.sim_finish(request, generated);
    assert_eq!(ui.replay.captures, 1);
    assert_eq!(ui.mstat.iter().map(|s| s.n).collect::<Vec<_>>(), vec![1, 1]);
    assert_eq!(eye_records(&ui), 1.0);
    assert!(!ui.azr.page.img.is_empty());
    ui.m.trig_bus_val = 0xa5;
    ui.m.status = TrigStatus::Wait;
    (ui, old, fresh)
}

fn eye_records(ui: &Ui) -> f64 {
    ui.eye_readings().iter().find(|(name, _)| *name == "RECords").expect("accepted Eye").1
}

struct Accepted {
    samples: Arc<Vec<f32>>, captures: u64, revision: u64,
    frame: Option<(usize, usize, usize)>, statistics: Vec<(u64, [u64; 4])>,
    eye: Vec<(&'static str, u64)>, image: Vec<u32>,
}
impl Accepted {
    fn of(ui: &Ui) -> Self {
        Self { samples: Arc::clone(&ui.refs[0].as_ref().unwrap().v), captures: ui.replay.captures,
            revision: ui.replay.sim_revision, frame: ui.replay.frame_for_test(),
            statistics: ui.measurement_statistics_snapshot_for_test(),
            eye: ui.eye_readings().iter().map(|(name, value)| (*name, value.to_bits())).collect(),
            image: ui.azr.page.img.clone() }
    }
    fn assert_held(&self, ui: &Ui) {
        assert!(Arc::ptr_eq(&self.samples, &ui.refs[0].as_ref().unwrap().v), "unaccepted data became visible");
        assert_eq!(ui.replay.captures, self.captures);
        assert_eq!(ui.replay.sim_revision, self.revision);
        assert_eq!(ui.replay.frame_for_test(), self.frame);
        assert_eq!(ui.measurement_statistics_snapshot_for_test(), self.statistics);
        assert_eq!(ui.eye_readings().iter().map(|(n, v)| (*n, v.to_bits())).collect::<Vec<_>>(), self.eye);
        assert_eq!(ui.azr.page.img, self.image);
    }
}

struct Held {
    reply: Sender<(Request, Result<Generated, String>)>,
    request: Option<Request>,
    // Keep the input receiver alive; unexpected follow-up requests remain queued
    // instead of starting a random generator or failing a channel send.
    _requests: Receiver<Request>,
}
impl Held {
    fn channel(ui: &mut Ui) -> Self {
        let (requests, input) = mpsc::channel();
        let (reply, results) = mpsc::channel();
        ui.sim_generation = Some(Generation { requests, results, next: 0, active: None,
            run: false, in_flight: false, queued: None, pending: None });
        Self { reply, request: None, _requests: input }
    }
    fn receive_request(&mut self) {
        let request = self._requests.try_recv().expect("SIM must submit a generation request");
        assert!(request.bus_trigger.as_ref().is_some_and(Result::is_ok));
        assert!(self.request.replace(request).is_none());
    }
    fn submit(ui: &mut Ui) -> Self {
        let mut held = Self::channel(ui);
        let request = ui.sim_capture_request(ui.sim_lane(), 0, true, true, true);
        ui.sim_generation.as_mut().unwrap().submit(request).unwrap();
        held.receive_request();
        held
    }
    fn arm_first(ui: &mut Ui) -> Self {
        let mut held = Self::channel(ui);
        ui.sim_play_set(true).unwrap();
        held.receive_request();
        held
    }
    fn finish(&mut self, ui: &mut Ui, samples: Vec<f32>) {
        let request = self.request.take().unwrap();
        let generated = Generated::evaluate(&request, samples);
        self.reply.send((request, Ok(generated))).unwrap();
        ui.sim_poll();
    }
}

#[test]
fn fresh_sim_i2c_qualifies_generated_sda_before_single_commit_and_preserves_physical_time() {
    let (mut ui, _old, fresh) = fixture();
    assert!(ui.search_events().is_empty(), "the installed 5A record contradicts the requested A5 match");
    let before = Accepted::of(&ui);
    ui.key(key::SINGLE);
    let clock = Arc::clone(&ui.refs[1].as_ref().unwrap().v);
    let mut held = Held::submit(&mut ui);
    for _ in 0..5 { ui.step(0.0); before.assert_held(&ui); }
    assert!(ui.sim_generating());
    assert_eq!(ui.m.status, TrigStatus::Wait);
    held.finish(&mut ui, fresh.clone());
    assert_eq!(ui.replay.captures, before.captures + 1);
    assert_eq!(ui.replay.sim_revision, before.revision + 1);
    assert_eq!(ui.refs[0].as_ref().unwrap().v.as_slice(), fresh.as_slice());
    assert!(!Arc::ptr_eq(&before.samples, &ui.refs[0].as_ref().unwrap().v));
    assert!(Arc::ptr_eq(&clock, &ui.refs[1].as_ref().unwrap().v));
    assert_eq!((ui.refs[0].as_ref().unwrap().t0, ui.refs[0].as_ref().unwrap().dt), (0.0, DT));
    assert_eq!((ui.refs[1].as_ref().unwrap().t0, ui.refs[1].as_ref().unwrap().dt), (-4.0 * DT, 2.0 * DT));
    assert_eq!(ui.m.run, RunState::Stop);
    assert!(!ui.sim_play);
    assert_eq!(ui.m.status, TrigStatus::Triggered);
    assert_eq!(ui.mstat.iter().map(|s| s.n).collect::<Vec<_>>(), vec![2, 2]);
    assert_eq!(eye_records(&ui), 2.0);
    let scans = ui.decoder_worker_state_for_test();
    let packets = ui.decode_packets_of(&ui.m).unwrap();
    let data = packets.iter().filter(|p| p.kind == PacketKind::Data).collect::<Vec<_>>();
    assert_eq!(data.len(), 8);
    assert!(data.iter().all(|p| p.value == 0xa5 && p.ok));
    // First data ACK is at 79 quarter-bit periods plus the sampled edge delay.
    assert!((data[0].t1 - 79.0 * 0.25 / RATE).abs() < 2.0 * DT);
    let hit = ui.replay.trigger_time.unwrap();
    assert!(data.iter().any(|p| p.t1 == hit));
    assert!(hit > 180e-6, "physical packet time must not be rebased to trigger zero");
    let hits = ui.search_events();
    assert_eq!(hits.as_ref(), &data.iter().map(|p| p.t1).collect::<Vec<_>>());
    assert!(ui.decode_events_result().unwrap().contains("A5"));
    assert!(Arc::ptr_eq(&packets, &ui.decode_packets_of(&ui.m).unwrap()));
    assert_eq!(ui.decoder_worker_state_for_test(), scans, "accepted candidate packets should serve Search/export without decoding again");
    let accepted = Accepted::of(&ui);
    for _ in 0..3 { ui.step(0.0); accepted.assert_held(&ui); }
}

#[test]
fn sim_bus_miss_stop_and_force_keep_acceptance_statistics_and_eye_in_sync() {
    let (mut ui, old, fresh) = fixture();
    ui.key(key::SINGLE);
    let before = Accepted::of(&ui);
    let mut miss = Held::submit(&mut ui);
    miss.finish(&mut ui, old.clone());
    before.assert_held(&ui);
    assert_eq!(ui.m.status, TrigStatus::Wait);
    assert_eq!(ui.m.run, RunState::Run);
    // Stop while another matching generation is outstanding: its late reply
    // cannot replace samples or increment the accepted record statistics.
    let mut stopped = Held::submit(&mut ui);
    ui.key(key::RUNSTOP);
    stopped.finish(&mut ui, fresh);
    before.assert_held(&ui);
    assert_eq!(ui.m.run, RunState::Stop);
    assert_eq!(ui.m.status, TrigStatus::Stop);
    // A fresh no-match result may be explicitly forced once. It is AUTO and
    // has no fabricated Bus event time, then Single returns to Stop.
    ui.key(key::FORCE);
    assert!(ui.replay.force_requested());
    let mut forced = Held::submit(&mut ui);
    forced.finish(&mut ui, old);
    assert_eq!(ui.replay.captures, before.captures + 1);
    assert_eq!(ui.m.run, RunState::Stop);
    assert_eq!(ui.m.status, TrigStatus::Auto);
    assert_eq!(ui.replay.trigger_time, None);
    assert!(!ui.replay.force_requested());
    assert_eq!(ui.mstat.iter().map(|s| s.n).collect::<Vec<_>>(), vec![2, 2]);
    assert_eq!(eye_records(&ui), 2.0);
    let accepted = Accepted::of(&ui);
    for _ in 0..3 { ui.step(0.0); accepted.assert_held(&ui); }
}

#[test]
fn inflight_sim_bus_result_rejects_changed_threshold_auxiliary_owner_and_origin() {
    for change in 0..3 {
        let (mut ui, _old, fresh) = fixture();
        ui.key(key::SINGLE);
        let before = Accepted::of(&ui);
        let mut held = Held::submit(&mut ui);
        match change {
            0 => ui.m.dec_thr = 1.8,
            1 => { let r = ui.refs[1].as_mut().unwrap(); r.v = Arc::new(r.v.as_ref().clone()); },
            _ => ui.m.deskew_ref[1] = 3.0 * DT,
        }
        held.finish(&mut ui, fresh);
        before.assert_held(&ui);
        assert_eq!(ui.m.run, RunState::Run);
        assert_eq!(ui.m.status, TrigStatus::Wait);
    }
}

#[test]
fn unsupported_sim_qualification_reports_an_error_without_committing_a_candidate() {
    for kind in [TrigType::Pattern, TrigType::Duration] {
        let (mut ui, _old, fresh) = fixture();
        let before = Accepted::of(&ui);
        ui.m.trig_type = kind;
        let request = ui.sim_capture_request(ui.sim_lane(), 0, true, true, true);
        let generated = Generated::evaluate(&request, fresh);
        ui.sim_finish(request, generated);
        assert!(ui.take_sim_error().unwrap().contains("unavailable"));
        before.assert_held(&ui);
        assert_eq!(ui.m.status, TrigStatus::Wait);
    }
    let (mut ui, _old, fresh) = fixture();
    let before = Accepted::of(&ui);
    ui.m.dec_la_in = true;
    let request = ui.sim_capture_request(ui.sim_lane(), 0, true, true, true);
    let generated = Generated::evaluate(&request, fresh);
    ui.sim_finish(request, generated);
    assert!(ui.take_sim_error().unwrap().contains("logic-analyzer"));
    before.assert_held(&ui);
}

fn live_fixture(mode: TrigMode) -> (Ui, Vec<f32>, Vec<f32>) {
    let (mut ui, old, fresh) = fixture();
    // A live session may already display references and a captured clock. Start
    // the first SIM into an empty R3 while retaining that accepted CH snapshot.
    let clock = ui.refs[1].as_ref().unwrap();
    let mut record = Ui::blank_record();
    record.dt = clock.dt;
    record.t0 = clock.t0;
    record.ch[0] = clock.v.as_ref().clone();
    ui.last[0] = Some(record);
    ui.session_is_sim = false;
    ui.sim_play = false;
    ui.replay = Default::default();
    ui.workflow_hold = None;
    ui.m.sim_slot = 2;
    ui.m.dec_src = DEC_REF_BASE + 2;
    ui.m.dec_src2 = 0;
    ui.m.ch[0].on = true;
    ui.m.ch[0].vscale = VSCALES.iter().position(|&v| v == 0.5).unwrap();
    ui.m.set_trigger_wave(WaveId::Ch(0), false);
    ui.m.trig_mode = mode;
    ui.m.run = RunState::Stop;
    ui.m.status = TrigStatus::Stop;
    assert!(ui.refs[2].is_none());
    (ui, old, fresh)
}

#[test]
fn first_sim_bus_normal_and_single_retry_a_miss_without_replacing_accepted_live_data() {
    for mode in [TrigMode::Normal, TrigMode::Single] {
        let (mut ui, old, fresh) = live_fixture(mode);
        let before = Accepted::of(&ui);
        let clock = ui.last[0].as_ref().unwrap().ch[0].as_ptr();
        let mut held = Held::arm_first(&mut ui);
        assert!(ui.replay.sim_initial_armed);
        assert!(ui.session_is_sim && ui.sim_play);
        assert_eq!(ui.m.status, TrigStatus::Wait);
        assert_eq!(ui.m.run, RunState::Run);
        assert!(!held.request.as_ref().unwrap().continuing);
        before.assert_held(&ui);
        held.finish(&mut ui, old);
        before.assert_held(&ui);
        assert!(ui.refs[2].is_none(), "a first no-hit must not create a SIM reference");
        assert_eq!(ui.last[0].as_ref().unwrap().ch[0].as_ptr(), clock);
        assert!(ui.replay.sim_initial_armed);
        assert_eq!(ui.m.status, TrigStatus::Wait);
        // The main loop, rather than a second Play press, must schedule a fresh
        // initial request. It must not replay any old installed reference.
        ui.step(0.0);
        held.receive_request();
        assert!(!held.request.as_ref().unwrap().continuing);
        before.assert_held(&ui);
        held.finish(&mut ui, fresh.clone());
        assert_eq!(ui.replay.captures, 1);
        assert_eq!(ui.replay.sim_revision, 1);
        assert!(!ui.replay.sim_initial_armed);
        assert_eq!(ui.refs[2].as_ref().unwrap().v.as_slice(), fresh.as_slice());
        assert_eq!(ui.m.status, TrigStatus::Triggered);
        assert_eq!(ui.m.run, if mode == TrigMode::Single { RunState::Stop } else { RunState::Run });
        assert_eq!(ui.last[0].as_ref().unwrap().ch[0].as_ptr(), clock, "required native clock must remain the accepted snapshot");
        assert_eq!((ui.last[0].as_ref().unwrap().dt, ui.last[0].as_ref().unwrap().t0), (2.0 * DT, -4.0 * DT));
        assert!(ui.m.ch[0].on, "the decoder cache key must retain a required CH auxiliary");
        assert!(!ui.search_events().is_empty());
        assert!(ui.decode_events_result().unwrap().contains("A5"));
        assert!(ui.replay.trigger_time.unwrap() > 180e-6);
    }
}

#[test]
fn first_sim_bus_stop_rearm_and_live_cancel_late_results_with_an_empty_target_slot() {
    let (mut ui, _old, fresh) = live_fixture(TrigMode::Single);
    let saved = ui.m.clone();
    let before = Accepted::of(&ui);
    let mut held = Held::arm_first(&mut ui);
    ui.key(key::RUNSTOP);
    assert_eq!(ui.m.run, RunState::Stop);
    assert!(!ui.sim_play);
    held.finish(&mut ui, fresh.clone());
    before.assert_held(&ui);
    assert!(ui.refs[2].is_none());
    assert_eq!(ui.m.status, TrigStatus::Stop);
    // SINGLE must rearm the pending initial SIM even though R3 is still empty.
    ui.key(key::SINGLE);
    assert_eq!(ui.m.run, RunState::Run);
    assert!(ui.sim_play);
    ui.step(0.0);
    held.receive_request();
    assert!(!held.request.as_ref().unwrap().continuing);
    assert!(ui.sim_activate(Item::SimPlay));
    assert_eq!(ui.m.run, RunState::Stop, "busy SIM Play must stop, not restart on the next tick");
    assert!(!ui.sim_play);
    held.finish(&mut ui, fresh.clone());
    ui.step(0.0);
    before.assert_held(&ui);
    assert!(held._requests.try_recv().is_err(), "cancelled first-arm must not silently retry");
    ui.key(key::SINGLE);
    ui.step(0.0);
    held.receive_request();
    ui.return_to_live();
    assert!(!ui.session_is_sim && !ui.sim_play && !ui.sim_generating());
    assert!(!ui.replay.sim_initial_armed);
    assert_eq!(ui.m.run, saved.run);
    assert_eq!(ui.m.ch[0].on, saved.ch[0].on);
    assert_eq!(ui.m.dec_cfg(), saved.dec_cfg());
    assert_eq!(ui.m.trigger_wave(), saved.trigger_wave());
    assert_eq!((ui.m.tscale, ui.m.delay), (saved.tscale, saved.delay));
    let restored = Accepted::of(&ui);
    held.finish(&mut ui, fresh);
    restored.assert_held(&ui);
    assert!(ui.refs[2].is_none(), "a late armed-SIM result cannot install after Live");
}

#[test]
fn first_sim_bus_force_accepts_one_fresh_no_hit_without_inventing_a_packet_time() {
    let (mut ui, old, _fresh) = live_fixture(TrigMode::Single);
    let mut held = Held::arm_first(&mut ui);
    held.finish(&mut ui, old.clone());
    assert_eq!(ui.replay.captures, 0);
    assert!(ui.refs[2].is_none());
    assert_eq!(ui.m.status, TrigStatus::Wait);
    ui.key(key::FORCE);
    assert!(ui.replay.force_requested());
    ui.step(0.0);
    held.receive_request();
    held.finish(&mut ui, old.clone());
    assert_eq!(ui.replay.captures, 1);
    assert_eq!(ui.replay.sim_revision, 1);
    assert_eq!(ui.refs[2].as_ref().unwrap().v.as_slice(), old.as_slice());
    assert_eq!(ui.m.run, RunState::Stop);
    assert_eq!(ui.m.status, TrigStatus::Auto);
    assert_eq!(ui.replay.trigger_time, None);
    assert!(!ui.replay.force_requested());
    assert!(!ui.replay.sim_initial_armed);
    assert!(ui.search_events().is_empty());
    let captures = ui.replay.captures;
    ui.step(0.0);
    assert_eq!(ui.replay.captures, captures);
}

#[test]
fn large_stored_and_native_clock_snapshots_are_shared_without_ui_scans_and_keep_native_time() {
    for native in [false, true] {
        let (mut ui, _old, fresh) = fixture();
        let mut clock = ui.refs[1].as_ref().unwrap().v.as_ref().clone();
        // Only the physical overlap with the generated SDA is needed. Invalid
        // values far outside it must neither poison decode nor trigger a full
        // auxiliary scan on the UI thread.
        clock.resize(3_000_000, f32::NAN);
        let owner: Arc<Vec<f32>>;
        let native_frame: Option<Arc<crate::board::Frame>>;
        if native {
            let frame = Arc::new(crate::board::Frame { seq: 77, dt: 2.0 * DT, pre: 100,
                ch: [clock, Vec::new(), Vec::new(), Vec::new()], ..Default::default() });
            let mut record = Ui::blank_record();
            // Wrong processed data/grid proves the immutable raw frame is used.
            record.ch[0] = vec![0.0; 64];
            record.dt = 32.0 * DT;
            record.t0 = 0.123;
            record.raw = Some(crate::wave::Raw { frame: Arc::clone(&frame), i0: 100, i1: 164,
                t0: -4.0 * DT + 100.0 * frame.dt, conditioned: true });
            ui.last[0] = Some(record);
            ui.m.ch[0].on = true;
            ui.m.ch[0].vscale = VSCALES.iter().position(|&v| v == 0.5).unwrap();
            ui.m.dec_src2 = 0;
            owner = Arc::new(Vec::new()); // unused in the native ownership branch
            native_frame = Some(frame);
        } else {
            owner = Arc::new(clock);
            ui.refs[1].as_mut().unwrap().v = Arc::clone(&owner);
            native_frame = None;
        }
        let before = Accepted::of(&ui);
        let scans = ui.decoder_worker_state_for_test();
        let owners = native_frame.as_ref().map_or_else(|| Arc::strong_count(&owner), Arc::strong_count);
        let start = std::time::Instant::now();
        let request = ui.sim_capture_request(ui.sim_lane(), 0, true, true, true);
        let elapsed = start.elapsed();
        assert!(request.bus_trigger.as_ref().is_some_and(Result::is_ok));
        assert_eq!(native_frame.as_ref().map_or_else(|| Arc::strong_count(&owner), Arc::strong_count), owners + 1,
            "request must share the immutable native allocation instead of copying 3M samples");
        assert_eq!(ui.decoder_worker_state_for_test(), scans, "request creation must not decode the installed old record");
        before.assert_held(&ui);
        let generated = Generated::evaluate(&request, fresh.clone());
        assert!(generated.error.is_none(), "only native physical overlap is decoded");
        let hits = Arc::clone(&generated.hits.as_ref().unwrap().1);
        assert_eq!(hits.len(), 8);
        assert!((hits[0] - 79.0 * 0.25 / RATE).abs() < 2.0 * DT);
        ui.sim_finish(request, generated);
        assert_eq!(ui.replay.captures, before.captures + 1);
        assert_eq!(ui.refs[0].as_ref().unwrap().v.as_slice(), fresh.as_slice());
        assert_eq!(ui.search_events().as_ref(), hits.as_ref());
        assert_eq!(ui.decoder_worker_state_for_test(), scans, "accepted predecoded packets must be reused");
        if let Some(frame) = native_frame.as_ref() {
            assert!(Arc::ptr_eq(frame, &ui.last[0].as_ref().unwrap().raw.as_ref().unwrap().frame));
            assert_eq!(frame.dt, 2.0 * DT);
        } else {
            assert!(Arc::ptr_eq(&owner, &ui.refs[1].as_ref().unwrap().v));
        }
        println!("3M {} auxiliary: request {:?}; shared owner, no UI decode scan, eight original-time hits", if native { "native Frame" } else { "Ref" }, elapsed);
    }
}

#[test]
fn opc_single_waits_after_a_completed_no_hit_generation_until_the_matching_retry_commits() {
    let (mut ui, old, fresh) = live_fixture(TrigMode::Single);
    let before = Accepted::of(&ui);
    let mut held = Held::arm_first(&mut ui);
    let request = held.request.as_ref().unwrap();
    let completed_generation = OperationToken::Sim(ui.workflow_epoch, request.id);
    let fence = ui.scpi_operations();
    let mut scpi = crate::scpi::Scpi::open(0);
    let cv = Canvas::new(W, H);
    let mut query = crate::scpi::Program::new("*OPC?;:TRIG:STAT?");
    assert!(scpi.poll_program(&mut ui, &cv, &mut query).is_none());
    held.finish(&mut ui, old);
    before.assert_held(&ui);
    assert!(!ui.sim_generating(), "the worker request itself is finished");
    assert!(ui.scpi_completed.contains(&completed_generation), "no-hit generation is a completed operation, not an accepted capture");
    assert_eq!((ui.m.run, ui.m.status), (RunState::Run, TrigStatus::Wait));
    assert_eq!(fence.state(&ui), OperationState::Pending, "Single still needs its own accepted-capture receipt");
    assert!(scpi.poll_program(&mut ui, &cv, &mut query).is_none());
    ui.step(0.0);
    held.receive_request();
    assert_ne!(held.request.as_ref().unwrap().id, match completed_generation { OperationToken::Sim(_, id) => id, _ => unreachable!() });
    assert_eq!(fence.state(&ui), OperationState::Pending);
    held.finish(&mut ui, fresh.clone());
    assert_eq!(ui.replay.captures, before.captures + 1);
    assert_eq!(ui.m.run, RunState::Stop);
    assert_eq!(ui.refs[2].as_ref().unwrap().v.as_slice(), fresh.as_slice());
    assert_eq!(fence.state(&ui), OperationState::Complete);
    assert_eq!(scpi.poll_program(&mut ui, &cv, &mut query), Some(b"1;STOP".to_vec()));
    let accepted = Accepted::of(&ui);
    for _ in 0..3 { ui.step(0.0); accepted.assert_held(&ui); }
}

#[test]
fn opc_completed_generation_receipt_survives_the_next_periodic_request_without_waiting_for_it() {
    let (mut ui, _old, fresh) = live_fixture(TrigMode::Normal);
    let mut held = Held::arm_first(&mut ui);
    let first = held.request.as_ref().unwrap().id;
    let fence = ui.scpi_operations();
    let mut scpi = crate::scpi::Scpi::open(0);
    let cv = Canvas::new(W, H);
    let mut query = crate::scpi::Program::new("*OPC?;:SIM:BUSY?");
    assert!(scpi.poll_program(&mut ui, &cv, &mut query).is_none());
    held.finish(&mut ui, fresh);
    assert_eq!(ui.replay.captures, 1);
    assert_eq!(fence.state(&ui), OperationState::Complete);
    let accepted = Accepted::of(&ui);
    ui.step(0.51);
    held.receive_request();
    assert_ne!(held.request.as_ref().unwrap().id, first);
    assert!(held.request.as_ref().unwrap().fresh, "exercise a true periodic capture after initial acceptance");
    assert!(ui.sim_generating());
    accepted.assert_held(&ui);
    assert_eq!(fence.state(&ui), OperationState::Complete, "a later request must not extend the completed fence");
    assert_eq!(scpi.poll_program(&mut ui, &cv, &mut query), Some(b"1;1".to_vec()));
    ui.key(key::RUNSTOP);
}
