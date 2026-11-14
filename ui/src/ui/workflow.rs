// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Direct file, reference, simulation and acquisition controls shared by the screen and web remote.
use super::*;

#[cfg(test)]
mod slot_loading_tests {
    use super::*;
    fn trace(label: &str) -> crate::store::WaveTrace {
        crate::store::WaveTrace { v: (0..64).map(|i| if i % 8 < 4 { -1.0 } else { 1.0 }).collect(),
            dt: 1e-9, t0: -32e-9, label: label.into() }
    }
    fn tap(ui: &mut Ui, r: Rect) {
        for phase in [Phase::Down, Phase::Up] { ui.handle(Ev::Touch { id: 0, phase, x: r.x + r.w / 2, y: r.y + r.h / 2 }); }
    }
    #[test]
    fn destination_choice_is_modal_and_cancel_changes_no_source_or_model() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.meas.clear(); ui.ref_sel = 3;
        let before = ui.m.clone();
        ui.request_waveform();
        assert!(ui.load_slot_ask && !ui.open_wave_request);
        tap(&mut ui, Rect::new(Ui::load_slot_box().x, Ui::load_slot_box().y, 400, 45));
        assert!(ui.load_slot_ask && !ui.open_wave_request, "the title is not a destination");
        tap(&mut ui, Ui::load_slot_cancel());
        assert!(!ui.load_slot_ask && !ui.open_wave_request && ui.waveform_destination().is_none());
        assert_eq!(ui.m, before); assert_eq!(ui.ref_sel, 3); assert!(ui.refs.iter().all(Option::is_none));
        ui.request_waveform(); ui.dismiss_context();
        assert!(!ui.load_slot_ask && !ui.open_wave_request);
    }
    #[test]
    fn chosen_async_destination_is_transactional_and_live_import_is_an_overlay() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.meas = vec![(Meas::Vpp, WaveId::Ch(0))];
        ui.m.ch[1].on = true; ui.m.trig_mode = TrigMode::Normal; ui.m.delay = 6e-6;
        ui.ref_sel = 3;
        let before = ui.m.clone();
        ui.request_waveform(); tap(&mut ui, Ui::load_slot_button(9));
        assert_eq!(ui.waveform_destination(), Some(9));
        assert!(ui.open_wave_request && !ui.load_slot_ask);
        assert_eq!(ui.ref_sel, 3, "choosing a destination does not change focus before commit");
        assert!(ui.commit_waveform_traces_at(vec![trace("A"), trace("B")], "two", 9).is_err());
        assert_eq!(ui.m, before); assert_eq!(ui.ref_sel, 3);
        assert!(ui.refs.iter().all(Option::is_none), "overflow validates before replacing any slot");
        ui.ref_sel = 7; // A different source may be selected while the worker is reading.
        ui.commit_waveform_traces_at(vec![trace("R10")], "chosen", 9).unwrap();
        assert_eq!(ui.m, before, "live ADC/run/timebase/trigger/catalog are preserved");
        assert!(!ui.record_mode() && ui.m.ch[0].on && ui.m.ch[1].on);
        assert_eq!(ui.ref_sel, 9); assert!(ui.refs[7].is_none() && ui.refs[9].as_ref().unwrap().on);
        assert_eq!(ui.replay.captures, 0);
        assert!(ui.set_trigger_source(WaveId::Ref(9), false));
        assert!(ui.record_mode() && ui.m.ch.iter().all(|ch| !ch.on));
        assert_eq!(ui.m.trigger_wave(), WaveId::Ref(9));
        ui.m.trig_level = 0.0;
        ui.key(key::SINGLE); ui.step(0.2);
        assert_eq!(ui.replay.captures, 1, "an explicit Ref trigger uses stored acquisition, never ADC arming");
        assert_eq!(ui.m.run, RunState::Stop);
    }

    #[test]
    fn live_overlay_fit_reuses_bounded_selected_record_fit_and_keeps_adc_acquisition() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.commit_waveform_traces_at(vec![trace("overlay")], "overlay", 2).unwrap();
        ui.m.deskew_ref[2] = 2e-9;
        ui.m.delay = 10e-3;
        let native = ui.ref_info(2).unwrap();
        let pointer = ui.refs[2].as_ref().unwrap().v.as_ptr();
        let adc = ui.m.ch.clone(); let trigger = ui.m.trigger(); let run = ui.m.run;
        assert!(ui.live_ref_note().contains("outside this window"));
        ui.fit_shown_reference();
        assert_eq!((ui.m.ch.clone(), ui.m.trigger(), ui.m.run), (adc, trigger, run));
        assert!(!ui.record_mode() && !ui.m.zoom);
        let (scale, delay) = ui.view(); let half = scale as f64 * HDIV as f64 * 0.5;
        let first = native.2 - 2e-9; let last = first + (native.0 - 1) as f64 * native.1;
        assert!(delay as f64 - half <= first && delay as f64 + half >= last);
        assert_eq!(ui.ref_info(2).unwrap().2, native.2);
        assert_eq!(ui.refs[2].as_ref().unwrap().v.as_ptr(), pointer);
        assert!(!ui.live_ref_note().contains("outside this window"));
        for ch in &mut ui.m.ch { ch.on = false; }
        assert!(!ui.record_mode(), "reference visibility alone cannot silently choose stored acquisition");
    }
}

pub(super) struct LiveSession {
    model: Model,
    visible: [bool; NREF],
}

#[cfg(test)]
mod import_measurement_tests {
    use super::*;

    const CSV: &[u8] = b"Time (s),Volts\n0,-1\n0.000000001,1\n0.000000002,-1\n0.000000003,1\n";

    #[test]
    fn additive_imports_keep_all_configured_single_pair_and_gate_settings() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.load_waveform_auto(CSV, "first.csv").unwrap();
        ui.m.meas = ALL_MEAS.iter().map(|&kind| (kind, WaveId::Ref(0))).collect();
        ui.m.meas2 = (0..PAIRS_N).map(|kind| (kind, WaveId::Ref(0), WaveId::Ref(1))).collect();
        ui.m.meas_full_record = true;
        ui.m.meas_labels = true;
        ui.m.meas_lpct = 0.2;
        ui.m.meas_hpct = 0.8;
        let expected = (ui.m.meas.clone(), ui.m.meas2.clone());
        ui.load_waveform_auto(CSV, "second.csv").unwrap();
        ui.load_waveform_auto(CSV, "third.csv").unwrap();
        assert!(ui.refs[0].is_some() && ui.refs[1].is_some() && ui.refs[2].is_some());
        assert_eq!((ui.m.meas.clone(), ui.m.meas2.clone()), expected);
        assert!(ui.m.meas_full_record && ui.m.meas_labels);
        assert_eq!((ui.m.meas_lpct, ui.m.meas_hpct), (0.2, 0.8));
        assert_eq!(ui.meas_src, WaveId::Ref(2), "new source is selected without replacing existing readouts");
    }

    #[test]
    fn a_first_import_respects_a_deliberately_empty_or_custom_catalog() {
        for custom in [Vec::new(), vec![(Meas::Vmax, WaveId::Ch(0))]] {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.meas = custom.clone();
            ui.load_waveform_auto(CSV, "custom.csv").unwrap();
            assert_eq!(ui.m.meas, custom);
        }
        let mut untouched = Ui::new(Source::Demo { t: 0.0 });
        untouched.load_waveform_auto(CSV, "default.csv").unwrap();
        assert!(untouched.m.meas.iter().any(|entry| entry.1 == WaveId::Ref(0)), "a first live import adds useful reference defaults");
        assert!(untouched.m.meas.iter().any(|entry| matches!(entry.1, WaveId::Ch(_))), "live channel defaults remain in the overlay workspace");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay() -> Ui {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.remember_live();
        u.offline = true;
        u.offline_ref = 0;
        u.m.trig_ref = Some(0);
        for ch in &mut u.m.ch { ch.on = false; }
        u.m.la.on = false;
        u.m.sim_slot = 0;
        u.m.trig_level = 0.0;
        u.m.trig_mode = TrigMode::Normal;
        u.m.tscale = tscales().iter().position(|&x| (x - 10e-9).abs() < 1e-14).unwrap();
        let values: Vec<f32> = (0..5000).map(|i| {
            let amplitude = if i < 2500 { 0.5 } else { 1.0 };
            if i % 40 < 20 { -amplitude } else { amplitude }
        }).collect();
        u.refs[0] = Some(RefWave { src: 0, color: 0, label: "Test record".into(), t0: 0.0, dt: 1e-9, v: (values).into(), vpd: 0.5, offset: 0.0, on: true });
        u.meas_src = WaveId::Ref(0);
        u.m.meas = vec![(Meas::Vpp, WaveId::Ref(0))];
        u.m.run = RunState::Stop;
        u
    }

    #[test]
    fn single_arms_waits_for_the_condition_and_stops_after_one_capture() {
        let mut u = replay();
        u.m.trig_level = 10.0;
        u.key(key::SINGLE);
        u.step(0.2);
        assert_eq!(u.m.status, TrigStatus::Wait);
        assert_eq!(u.replay.captures, 0);
        assert_eq!(u.m.run, RunState::Run);
        u.m.trig_level = 0.0;
        u.step(0.2);
        assert_eq!(u.replay.captures, 1);
        assert_eq!(u.m.status, TrigStatus::Triggered);
        assert_eq!(u.m.run, RunState::Stop);
        assert!(!u.sim_play && u.last[0].is_none());
        u.step(0.5);
        assert_eq!(u.replay.captures, 1);
    }

    #[test]
    fn run_stop_and_capture_measurements_share_one_state() {
        let mut u = replay();
        u.key(key::RUNSTOP);
        u.step(0.2);
        let first = u.meas_at(WaveId::Ref(0), Meas::Vpp);
        let count = u.replay.captures;
        u.replay.cursor = 3000.0;
        u.step(0.2);
        let second = u.meas_at(WaveId::Ref(0), Meas::Vpp);
        assert!((first - 1.0).abs() < 0.01 && (second - 2.0).abs() < 0.01, "{first} {second}");
        assert!(u.replay.captures > count);
        assert_eq!(u.mstat[0].n, 2);
        u.key(key::RUNSTOP);
        let held = (u.replay.captures, u.view(), second);
        u.step(0.5);
        assert_eq!((u.replay.captures, u.view(), u.meas_at(WaveId::Ref(0), Meas::Vpp)), held);
    }

    #[test]
    fn panning_a_stopped_reference_recomputes_the_visible_measurement_window() {
        let mut u = replay();
        u.m.delay = 1e-6;
        u.step(0.01);
        let first = u.meas_at(WaveId::Ref(0), Meas::Vpp);
        u.m.delay = 4e-6;
        u.step(0.01);
        let second = u.meas_at(WaveId::Ref(0), Meas::Vpp);
        assert!((first - 1.0).abs() < 0.01 && (second - 2.0).abs() < 0.01, "{first} {second}");
        assert_eq!(u.replay.captures, 0, "panning is analysis, not an acquisition");
    }

    fn wave(dt: f64, vpd: f32, offset: f32, color: usize) -> RefWave {
        RefWave {
            src: 0,
            color,
            label: String::new(),
            t0: 0.0,
            dt,
            v: (vec![0.0; 64]).into(),
            vpd,
            offset,
            on: true,
        }
    }

    #[test]
    fn reference_badge_shows_vertical_scale_and_the_record_sample_rate() {
        let fast = wave(20e-12, 0.25, 0.0, 0);
        assert_eq!(Ui::ref_badge_readout(&fast), ("250 mV/div".into(), "50 GSa/s".into()));
        let slow = wave(1e-9, 1.0, 0.0, 1);
        assert_eq!(Ui::ref_badge_readout(&slow), ("1 V/div".into(), "1 GSa/s".into()));
        let (scale, rate) = Ui::ref_badge_readout(&fast);
        assert!(!scale.contains("s/div") && !rate.contains("/div"));
    }

    #[test]
    fn reference_badges_keep_one_horizontal_scale() {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        for ch in &mut u.m.ch {
            ch.on = false;
        }
        let time = u.m.time_per_div();
        u.refs[0] = Some(wave(20e-12, 0.25, 0.0, 1));
        u.refs[1] = Some(wave(1e-9, 1.0, 2.0, 2));
        let mut cv = Canvas::new(W, H);
        u.render(&mut cv);
        assert_eq!(u.m.time_per_div(), time);
        assert_eq!(Ui::ref_badge_readout(u.refs[0].as_ref().unwrap()).1, "50 GSa/s");
        assert_eq!(Ui::ref_badge_readout(u.refs[1].as_ref().unwrap()).1, "1 GSa/s");
        for color in [REF_COLORS[1].1, REF_COLORS[2].1] {
            let mut span = 0;
            for y in 40..520 {
                let mut run = 0;
                let mut best = 0;
                for x in 0..cv.w {
                    if cv.px[(y * cv.w + x) as usize] == color {
                        run += 1;
                        best = best.max(run);
                    } else {
                        run = 0;
                    }
                }
                span = span.max(best);
            }
            assert!(span >= 48, "badge run {span} for {color:#x}");
        }
    }

    #[test]
    fn every_reference_uses_the_same_time_window_at_its_own_sample_interval() {
        let mut u = replay();
        let values: Vec<f32> = (0..2500).map(|i| if i < 1250 { if i % 20 < 10 { -1.0 } else { 1.0 } } else if i % 20 < 10 { -3.0 } else { 3.0 }).collect();
        u.refs[1] = Some(RefWave { src: 0, color: 1, label: "Second source".into(), t0: 0.0, dt: 2e-9, v: (values).into(), vpd: 1.0, offset: 0.0, on: false });
        u.m.delay = 1e-6;
        u.step(0.01);
        assert!((u.meas_at(WaveId::Ref(1), Meas::Vpp) - 2.0).abs() < 0.01);
        u.m.delay = 4e-6;
        u.step(0.01);
        assert!((u.meas_at(WaveId::Ref(0), Meas::Vpp) - 2.0).abs() < 0.01);
        assert!((u.meas_at(WaveId::Ref(1), Meas::Vpp) - 6.0).abs() < 0.01);
        assert_eq!(u.reference_slice(1).unwrap().2, 2e-9);
    }

    #[test]
    fn reference_two_source_triggers_use_secondary_scale_and_rebuild_cached_hits() {
        for kind in [TrigType::Delay, TrigType::SetupHold] {
            for secondary in [WaveId::Ref(1), WaveId::Ch(1)] {
                let mut u = replay();
                u.m.trig_type = kind;
                u.m.trig_level = 0.0;
                u.m.trig_level2 = 0.0;
                u.m.slope = Slope::Rise;
                u.m.trig_slope2 = Slope::Rise;
                u.m.trig_when = WhenKind::Gt;
                u.m.trig_t1 = 0.0;
                u.m.setup = 20e-9;
                u.m.hold = 1e-9;
                u.m.set_trigger_wave(secondary, true);
                let values: Vec<f32> = (0..5000).map(|i| if i % 40 < 30 { -0.05 } else { 0.05 }).collect();
                match secondary {
                    WaveId::Ref(i) => u.refs[i] = Some(RefWave { src: 0, color: 1,
                        label: "Low-amplitude secondary".into(), t0: 0.0, dt: 1e-9,
                        v: (values).into(), vpd: 2.0, offset: 0.0, on: false }),
                    WaveId::Ch(c) => {
                        u.m.ch[c].probe = 1.0;
                        u.m.ch[c].vfine = 2.0 / VSCALES[u.m.ch[c].vscale];
                        let mut rec = Ui::blank_record();
                        rec.dt = 1e-9;
                        rec.ch[c] = values;
                        u.last[0] = Some(rec);
                    }
                }

                u.key(key::SINGLE);
                u.replay_acquire(0.2);
                assert_eq!((u.m.status, u.replay.captures, u.m.run),
                    (TrigStatus::Wait, 0, RunState::Run), "{kind:?} {secondary:?}: large secondary hysteresis should reject the small waveform");

                // Change only its scale. The source buffer, thresholds and type
                // are identical, so this also exercises cache invalidation.
                match secondary {
                    WaveId::Ref(i) => u.refs[i].as_mut().unwrap().vpd = 0.02,
                    WaveId::Ch(c) => u.m.ch[c].vfine = 0.02 / VSCALES[u.m.ch[c].vscale],
                }
                u.now += 0.2;
                u.replay_acquire(0.2);
                assert_eq!((u.m.status, u.replay.captures, u.m.run),
                    (TrigStatus::Triggered, 1, RunState::Stop), "{kind:?} {secondary:?}: secondary scale must control its own comparator");

                match secondary {
                    WaveId::Ref(i) => u.refs[i].as_mut().unwrap().vpd = 2.0,
                    WaveId::Ch(c) => u.m.ch[c].vfine = 2.0 / VSCALES[u.m.ch[c].vscale],
                }
                u.key(key::SINGLE);
                u.now += 0.2;
                u.replay_acquire(0.2);
                assert_eq!((u.m.status, u.replay.captures, u.m.run),
                    (TrigStatus::Wait, 1, RunState::Run), "{kind:?} {secondary:?}: cached hits must be removed when secondary hysteresis grows");
            }
        }
    }

    #[test]
    fn leaving_sim_restores_live_settings_and_keeps_the_saved_record() {
        let mut u = replay();
        u.return_to_live();
        assert!(!u.record_mode());
        assert!(u.m.ch[0].on);
        assert_eq!(u.m.run, RunState::Run);
        assert!(u.refs[0].is_some() && !u.refs[0].as_ref().unwrap().on);
        u.step(0.02);
        assert!(u.last[0].is_some());
    }

    #[test]
    fn invalid_file_does_not_replace_a_working_session() {
        let mut u = replay();
        let old = u.refs[0].as_ref().unwrap().v.clone();
        assert!(u.load_waveform_bytes(b"not a waveform", "bad.csv").is_err());
        assert_eq!(u.refs[0].as_ref().unwrap().v, old);
        let mut header = vec![0u8; 36];
        header[..4].copy_from_slice(b"OOSW");
        header[4..8].copy_from_slice(&1u32.to_le_bytes());
        header[8..12].copy_from_slice(&2u32.to_le_bytes());
        header[12..20].copy_from_slice(&u64::MAX.to_le_bytes());
        header[20..28].copy_from_slice(&1e-9f64.to_le_bytes());
        assert!(u.load_waveform_bytes(&header, "oversize.bin").is_err());
    }

    #[test]
    fn a_file_waits_for_a_slot_and_lands_on_the_one_chosen() {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        let kept: Vec<f32> = (0..40).map(|i| if i < 20 { -0.2 } else { 0.2 }).collect();
        u.refs[0] = Some(RefWave { src: 0, color: 0, label: "keep".into(), t0: 0.0, dt: 1e-9, v: kept.clone().into(), vpd: 0.2, offset: 0.0, on: true });
        u.request_waveform();
        assert!(u.load_slot_ask && !u.open_wave_request);
        let title = Ui::load_slot_box();
        u.tap(title.x + 30, title.y + 24);
        assert!(u.load_slot_ask && !u.open_wave_request, "a tap on the title loads nothing");
        u.tap(Ui::load_slot_cancel().x + 4, Ui::load_slot_cancel().y + 4);
        assert!(!u.load_slot_ask && !u.open_wave_request);
        assert_eq!(u.refs[0].as_ref().unwrap().v.as_ref(), &kept);
        u.request_waveform();
        u.tap(Ui::load_slot_button(9).x + 4, Ui::load_slot_button(9).y + 4);
        assert_eq!(u.waveform_destination(), Some(9));
        assert_eq!(u.ref_slot(), 0, "slot choice does not redirect the current source before commit");
        assert!(u.open_wave_request);
        let mut csv = String::from("Time (s),Volts\n");
        for k in 0..80 {
            csv.push_str(&format!("{},{}\n", k as f64 * 1e-9, if k < 40 { -0.4 } else { 0.4 }));
        }
        u.load_waveform_bytes_at(csv.as_bytes(), "lane.csv", u.waveform_destination().unwrap()).unwrap();
        assert_eq!(u.refs[0].as_ref().unwrap().v.as_ref(), &kept);
        assert!(u.refs[9].is_some() && u.refs[1].is_none());
        assert_eq!(u.ref_slot(), 9);
        assert_eq!(u.m.deskew_ref[9], 0.0);
    }

    fn square(delay: usize) -> Vec<f32> {
        (0..1600).map(|i| if ((i + 8000 - delay) / 80) % 2 == 0 { -0.5 } else { 0.5 }).collect()
    }

    #[test]
    fn auto_deskew_lines_up_the_shown_traces() {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.run = RunState::Stop;
        for ch in &mut u.m.ch { ch.on = false; }
        u.m.ch[0].on = true;
        u.m.ch[1].on = true;
        let mut samples = std::array::from_fn(|_| Vec::new());
        samples[0] = square(0);
        samples[1] = square(40);
        u.last[0] = Some(Record { t0: 0.0, dt: 1e-9, ch: samples, triggered: true, partial: false, raw: None, la: None });
        u.refs[3] = Some(RefWave { src: 0, color: 1, label: String::new(), t0: 0.0, dt: 1e-9, v: square(80).into(), vpd: 0.2, offset: 0.0, on: true });
        u.refs[4] = Some(RefWave { src: 0, color: 2, label: String::new(), t0: 0.0, dt: 1e-9, v: square(200).into(), vpd: 0.2, offset: 0.0, on: false });
        u.measure_last();
        let before = u.pair_at(8, WaveId::Ch(0), WaveId::Ch(1));
        assert!((before - 40e-9).abs() < 2e-9, "{before}");
        assert!(u.items(MenuId::Horizontal).contains(&Item::AutoDeskew));
        assert!(u.items(MenuId::ChMore(0)).contains(&Item::AutoDeskew));
        u.activate(Item::AutoDeskew);
        let after = u.pair_at(8, WaveId::Ch(0), WaveId::Ch(1));
        assert!(after.abs() < 2e-9, "{after}");
        let edge = |id| {
            let (a, t0) = u.meas_analysis(id).unwrap();
            a.rises[0] + t0
        };
        let (ch, rf) = (edge(WaveId::Ch(0)), edge(WaveId::Ref(3)));
        assert!((ch - rf).abs() < 2e-9, "channel {ch} reference {rf}");
        assert_eq!(u.m.deskew_ref[4], 0.0, "a hidden reference is left where it is");
        u.activate(Item::AutoDeskew);
        assert!(u.pair_at(8, WaveId::Ch(0), WaveId::Ch(1)).abs() < 2e-9, "running it again does not walk the traces");
        u.m.ch[1].on = false;
        u.refs[3].as_mut().unwrap().on = false;
        u.m.deskew_ch = [0.0; NCH];
        u.m.deskew_ref = [0.0; NREF];
        u.activate(Item::AutoDeskew);
        assert_eq!(u.m.deskew_ch, [0.0; NCH]);
        assert_eq!(u.m.deskew_ref, [0.0; NREF]);
    }
}

struct DeferredCaptureStatistics {
    capture: u64,
    context: String,
    rows: Vec<usize>,
}

#[derive(Default)]
pub(super) struct Replay {
    cursor: f64,
    pub(super) next: f64,
    force: bool,
    frame: Option<(usize, usize, usize)>,
    pub(super) viewport: Option<(usize, f32, f32)>,
    pub captures: u64,
    pub(super) trigger_time: Option<f64>,
    pub(super) sim_revision: u64,
    pub(super) sim_attempt: u64,
    pub(super) sim_next: f64,
    pub(super) fresh_sim: bool,
    pub(super) sim_initial_armed: bool,
    pub(super) sim_config_change: bool,
    deferred_statistics: Option<DeferredCaptureStatistics>,
}

impl Replay {
    #[cfg(test)]
    pub(super) fn frame_for_test(&self) -> Option<(usize,usize,usize)> { self.frame }
    pub(super) fn force_requested(&self) -> bool { self.force }
    pub(super) fn request_force(&mut self) { self.force = true; }
    pub(super) fn clear_deferred_statistics(&mut self) { self.deferred_statistics = None; }

    pub(super) fn reset_trigger_source(&mut self) {
        self.cursor=0.0;self.force=false;self.trigger_time=None;
        self.deferred_statistics=None;
    }

    pub(super) fn reset_search(&mut self) {
        self.cursor = 0.0;
    }
}

impl Ui {
    fn capture_statistics_context(&self) -> String {
        let refs: Vec<_> = self.refs.iter().map(|r| r.as_ref().map(|r|
            (r.v.as_ptr() as usize, r.v.len(), r.dt.to_bits(), r.t0.to_bits(), r.vpd.to_bits()))).collect();
        let channels: Vec<_> = self.m.ch.iter().map(|ch| (ch.volts_per_div().to_bits(), ch.unit.name())).collect();
        let gate = (!self.m.meas_full_record).then(|| { let (scale, delay) = self.view(); (scale.to_bits(), delay.to_bits()) });
        format!("{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}", self.m.meas, (&self.m.meas2, &self.m.cat),
            (self.m.meas_full_record, self.m.meas_lpct.to_bits(), self.m.meas_hpct.to_bits(), self.m.meas_idle.to_bits(),
                self.m.meas_n, self.m.meas_data, self.m.meas_out, self.m.meas_tsetup.to_bits(), self.m.meas_thold.to_bits()),
            refs, channels, gate, self.m.trigger(), self.m.trigger_wave(),
            (self.m.trig_type, self.m.trig_bus, self.m.trig_bus_val, self.m.trig_nrej,
                self.m.deskew_ch.map(f64::to_bits), self.m.deskew_ref.map(f64::to_bits)))
    }

    fn add_capture_statistic(stat: &mut MStat, value: f64) {
        if !value.is_finite() { return; }
        if stat.n == 0 { stat.min = value; stat.max = value; }
        stat.n += 1; stat.sum += value; stat.sumsq += value * value;
        stat.min = stat.min.min(value); stat.max = stat.max.max(value);
    }

    /// Finish only rows pending for this exact accepted record and measurement
    /// gate/configuration. A worker completion or repeated read is not a capture.
    pub(super) fn finish_deferred_capture_statistics(&mut self) {
        let Some(mut pending) = self.replay.deferred_statistics.take() else { return };
        if pending.capture != self.replay.captures || pending.context != self.capture_statistics_context() { return; }
        let rows = self.meas_rows();
        self.mstat.resize(rows.len(), MStat::default());
        pending.rows.retain(|&index| {
            let Some(row) = rows.get(index) else { return false };
            if row.computing { return true; }
            Self::add_capture_statistic(&mut self.mstat[index], row.v);
            false
        });
        if !pending.rows.is_empty() { self.replay.deferred_statistics = Some(pending); }
    }
    pub fn record_mode(&self) -> bool {
        // A reference drawn with the live channels is not a session. Take over, Offline waveforms, and SIM are.
        self.offline || self.session_is_sim || self.sim_play
    }

    pub(super) fn active_reference(&self) -> usize {
        if self.session_is_sim {
            self.m.sim_slot
        } else if self.offline {
            self.offline_ref
        } else if self.refs.get(self.ref_sel).is_some_and(Option::is_some) {
            self.ref_sel
        } else {
            self.refs.iter().position(Option::is_some).unwrap_or(0)
        }
    }

    pub(super) fn remember_live(&mut self) {
        if self.workflow_hold.is_none() && !self.record_mode() {
            self.workflow_hold = Some(LiveSession { model: self.m.clone(), visible: std::array::from_fn(|i| self.refs[i].as_ref().is_some_and(|r| r.on)) });
        }
    }

    pub fn waveform_epoch(&self) -> u64 { self.workflow_epoch }

    pub fn return_to_live(&mut self) {
        self.workflow_epoch = self.workflow_epoch.wrapping_add(1);
        self.sim_cancel_generation();
        self.set_offline(false);
        self.session_is_sim = false;
        self.sim_play = false;
        self.replay = Replay::default();
        if let Some(saved) = self.workflow_hold.take() {
            self.m = saved.model;
            for (i, r) in self.refs.iter_mut().enumerate() {
                if let Some(r) = r { r.on = saved.visible[i]; }
            }
        } else {
            for r in self.refs.iter_mut().flatten() { r.on = false; }
            self.m.ch[0].on = true;
            self.m.run = RunState::Run;
            self.m.zoom = false;
            self.m.view = AView::Scope;
            self.m.meas = Self::default_meas();
        }
        self.m.floor_live_tdiv();
        self.select_workspace_source(WaveId::Ch(self.m.active));
        self.last[0] = None;
        self.cleared = false;
        self.ph.clear();
        self.phz.clear();
        self.close_menu();
        self.relayout();
        self.settings_changed();
        self.toast("Returned to live acquisition");
    }

    pub fn request_waveform(&mut self) {
        self.cancel_bus_candidate(true);
        self.workflow_epoch = self.workflow_epoch.wrapping_add(1);
        self.close_menu();
        self.open_wave_request = false;
        self.load_destination = None;
        self.load_slot_ask = true;
        self.power_dlg = false;
        self.dialog = None;
        self.cancel_pointer();
    }

    pub fn waveform_destination(&self) -> Option<usize> { self.load_destination }

    pub(super) fn load_slot_box() -> Rect { Rect::new((W - 560) / 2, (H - 248) / 2, 560, 248) }
    pub(super) fn load_slot_button(slot: usize) -> Rect {
        let r = Self::load_slot_box();
        Rect::new(r.x + 20 + (slot % 5) as i32 * 106, r.y + 78 + (slot / 5) as i32 * 52, 98, 44)
    }
    pub(super) fn load_slot_cancel() -> Rect {
        let r = Self::load_slot_box(); Rect::new(r.x + r.w / 2 - 70, r.y + r.h - 52, 140, 36)
    }
    pub(super) fn cancel_load_slot(&mut self) {
        self.load_slot_ask = false; self.load_destination = None; self.open_wave_request = false;
    }
    pub(super) fn load_slot_tap(&mut self, x: i32, y: i32) -> bool {
        if !self.load_slot_ask { return false; }
        if let Some(slot) = (0..NREF).find(|&slot| Self::load_slot_button(slot).contains(x, y)) {
            self.load_slot_ask = false;
            self.load_destination = Some(slot);
            self.open_wave_request = true;
        } else if Self::load_slot_cancel().contains(x, y) || !Self::load_slot_box().contains(x, y) {
            self.cancel_load_slot();
        }
        true
    }
    pub(super) fn draw_load_slot(&self, cv: &mut Canvas) {
        if !self.load_slot_ask { return; }
        cv.blend(Rect::new(0, 0, W, H), rgb(0, 0, 0), 150);
        let r = Self::load_slot_box();
        cv.round(r, 8, ACCENT, 255); cv.round(r.inset(2), 7, DLG_BG, 255);
        cv.text_in(&self.fonts().lb, Rect::new(r.x, r.y + 16, r.w, 28), 1, "Load into", ACCENT);
        cv.text_in(&self.fonts().m, Rect::new(r.x, r.y + 46, r.w, 22), 1, "Choose a destination before opening the file.", DIM);
        for slot in 0..NREF {
            let b = Self::load_slot_button(slot);
            btn(cv, b, BTN, if self.refs[slot].is_some() { AMBER } else { BTN_BORDER });
            let label = format!("R{}{}", slot + 1, if self.refs[slot].is_some() { " replace" } else { "" });
            cv.text_in(&self.fonts().mb, b, 1, &label, TEXT);
        }
        let cancel = Self::load_slot_cancel(); btn(cv, cancel, BTN, BTN_BORDER);
        cv.text_in(&self.fonts().mb, cancel, 1, "Cancel", TEXT);
    }

    pub fn load_waveform_bytes(&mut self, data: &[u8], name: &str) -> Result<(), String> {
        let traces = crate::store::parse_wave(data, name)?;
        self.commit_waveform_traces(traces, name, false)
    }

    pub fn load_waveform_bytes_at(&mut self, data: &[u8], name: &str, slot: usize) -> Result<(), String> {
        let traces = crate::store::parse_wave(data, name)?;
        self.commit_waveform_traces_at(traces, name, slot)
    }

    /// Native/tool Load adds a file without silently replacing the prior source.
    pub fn load_waveform_auto(&mut self, data: &[u8], name: &str) -> Result<(), String> {
        let traces = crate::store::parse_wave(data, name)?;
        self.commit_waveform_traces(traces, name, true)
    }

    /// Main-thread commit for a native file parsed by the import worker. Validate
    /// before mutating the session; failure leaves the working screen intact.
    pub fn commit_waveform_traces(&mut self, traces: Vec<crate::store::WaveTrace>, name: &str, auto: bool) -> Result<(), String> {
        let destination = if auto { None } else { Some(self.ref_sel.min(NREF - 1)) };
        self.commit_waveform_traces_to(traces, name, destination)
    }

    pub fn commit_waveform_traces_at(&mut self, traces: Vec<crate::store::WaveTrace>, name: &str, slot: usize) -> Result<(), String> {
        if slot >= NREF { return Err("Choose a reference slot from R1 to R10".into()); }
        self.commit_waveform_traces_to(traces, name, Some(slot))
    }

    fn commit_waveform_traces_to(&mut self, traces: Vec<crate::store::WaveTrace>, name: &str, destination: Option<usize>) -> Result<(), String> {
        if traces.is_empty() || traces.iter().any(|t| t.v.len() < 2 || !t.dt.is_finite() || t.dt <= 0.0 || !t.t0.is_finite()) {
            return Err("The file needs at least two samples and a valid sample interval.".into());
        }
        let first = if destination.is_none() {
            (0..NREF).find(|&start| start + traces.len() <= NREF && self.refs[start..start + traces.len()].iter().all(Option::is_none))
                .ok_or("No empty reference slots; select a slot and use Reference > Import to replace it")?
        } else { destination.unwrap() };
        if first + traces.len() > NREF {
            return Err("The file has more traces than the remaining reference slots".into());
        }
        // Adding a source must not discard a user's catalog or channel-pair setup.
        // A fresh, untouched live default gets useful reference readouts on its first import.
        let initialize_measurements = self.refs.iter().all(Option::is_none)
            && self.m.meas == Self::default_meas() && self.m.meas2.is_empty();
        let reference_session = self.record_mode();
        self.cancel_bus_candidate(true);
        self.workflow_epoch = self.workflow_epoch.wrapping_add(1);
        // Initial SIM generation is still in Live until its first result. Any
        // successful import supersedes that pending request as well.
        self.sim_cancel_generation();
        if reference_session {
            self.set_offline(true);
            self.session_is_sim = false;
            self.sim_play = false;
            self.replay = Replay::default();
            self.reset_workspace_for_import();
            self.m.la.on = false;
            self.m.view = AView::Scope;
            self.xy = false;
            self.m.eye_b = None;
            self.m.eye_rail = None;
        }
        self.invalidate_reference_eye();
        for (k, trace) in traces.into_iter().enumerate() {
            let slot = first + k;
            self.m.deskew_ref[slot] = 0.0;
            // Bounded native windows, not a synchronous 20M-point scan on commit.
            let summary = crate::wave::autoset_summary(&trace.v, trace.dt);
            let (lo, hi) = (summary.vmin, summary.vmax);
            let (lo, hi) = if lo.is_finite() && hi.is_finite() { (lo, hi) } else { (0.0, 1.0) };
            self.refs[slot] = Some(RefWave { src: 0, color: slot % REF_COLORS.len(), label: if trace.label.is_empty() { name.into() } else { trace.label }, t0: trace.t0, dt: trace.dt, v: (trace.v).into(),
                vpd: ((hi - lo).abs() / 6.0).max(1e-6), offset: -0.5 * (hi + lo), on: true });
        }
        self.ref_sel = first;
        if reference_session {
            self.m.set_trigger_wave(WaveId::Ref(first), false);
            self.m.trig_ref2 = None;
        }
        if initialize_measurements {
            let defaults = [Meas::Vpp, Meas::Freq, Meas::Vrms, Meas::Rise].into_iter().map(|k| (k, WaveId::Ref(first)));
            if reference_session { self.m.meas = defaults.collect(); }
            else { self.m.meas.extend(defaults); }
        }
        if reference_session { self.clear_measurement_statistics(); }
        self.meas_src = WaveId::Ref(first);
        if self.look == reflook::RefLook::TakeOver { self.solo_reference(first); }
        self.ensure_meas_own();
        self.close_menu();
        if reference_session {
            self.present_record(first);
            self.fit_reference_trigger_levels();
        } else {
            self.select_workspace_source(WaveId::Ref(first));
            self.dirty_map = true;
        }
        self.relayout();
        self.toast(&format!("Loaded {} into R{}", name, first + 1));
        Ok(())
    }

    /// Sample rate and length of the references drawn on the live screen, and whether each one meets the current time window.
    pub(super) fn live_ref_note(&self) -> String {
        if self.record_mode() {
            return String::new();
        }
        let (tdiv, delay) = self.view();
        let half = tdiv as f64 * HDIV as f64 / 2.0;
        let (left, right) = (delay as f64 - half, delay as f64 + half);
        let mut parts = Vec::new();
        for (i, r) in self.refs.iter().enumerate() {
            let Some(r) = r.as_ref() else { continue };
            if !r.on || r.v.len() < 2 || !(r.dt > 0.0) {
                continue;
            }
            let t0 = self.source_time_origin(WaveId::Ref(i), r.t0);
            let end = t0 + r.v.len().saturating_sub(1) as f64 * r.dt;
            let outside = end < left || t0 > right;
            let where_ = if outside { " · outside this window" } else { "" };
            parts.push(format!("R{} {} · {}{}", i + 1, si_digits(1.0 / r.dt, "Sa/s", 3), si_digits(r.v.len() as f64, "pts", 3), where_));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("With live · {}", parts.join("   "))
        }
    }

    /// Move the live time base onto the first shown reference. The channels stay on.
    pub(super) fn fit_shown_reference(&mut self) {
        if self.record_mode() {
            return;
        }
        let slot = if self.refs.get(self.ref_sel).and_then(|r| r.as_ref()).is_some_and(|r| r.on && r.v.len() >= 2) {
            self.ref_sel
        } else {
            match (0..NREF).find(|&i| self.refs[i].as_ref().is_some_and(|r| r.on && r.v.len() >= 2 && r.dt > 0.0)) {
                Some(i) => i,
                None => return,
            }
        };
        self.select_workspace_source(WaveId::Ref(slot));
        self.fit_selected_record();
    }

    pub(super) fn live_ref_fit_rect(&self) -> Option<Rect> {
        let note = self.live_ref_note();
        if note.is_empty() || self.view_on() || self.eye_open() {
            return None;
        }
        let g = self.grid;
        Some(Rect::new(g.x + 8, g.y + g.h - 28, 52, 22))
    }

    pub(super) fn reference_slice(&self, slot: usize) -> Option<(&[f32], f64, f64)> {
        let r = self.refs.get(slot)?.as_ref()?;
        let t0 = self.source_time_origin(WaveId::Ref(slot), r.t0);
        if let Some((source, lo, hi)) = self.replay.frame.filter(|_| self.record_mode()) {
            let origin = self.refs.get(source)?.as_ref()?;
            let origin_t0 = self.source_time_origin(WaveId::Ref(source), origin.t0);
            let start = origin_t0 + lo as f64 * origin.dt;
            let end = origin_t0 + hi as f64 * origin.dt;
            let lo = (((start - t0) / r.dt).floor().max(0.0) as usize).min(r.v.len());
            let hi = (((end - t0) / r.dt).ceil().max(0.0) as usize).min(r.v.len()).max(lo);
            return Some((&r.v[lo..hi], t0 + lo as f64 * r.dt, r.dt));
        }
        Some((&r.v, t0, r.dt))
    }

    pub(crate) fn record_key(&mut self, key_code: u8) -> bool {
        if matches!(key_code, key::RUNSTOP | key::SINGLE | key::FORCE) { self.sim_cancel_generation(); }
        if !self.record_mode() { return false; }
        if key_code == key::DEFAULT {
            self.return_to_live();
            self.key(key_code);
            return true;
        }
        if (0x15..=0x18).contains(&key_code) {
            self.activate_live_channel((key_code - 0x15) as usize);
            return true;
        }
        if matches!(key_code, key::RUNSTOP | key::SINGLE | key::FORCE) && !self.replay.sim_initial_armed && self.refs.get(self.active_reference()).and_then(Option::as_ref).is_none() {
            self.m.run = RunState::Stop;
            self.sim_play = false;
            self.m.status = TrigStatus::Stop;
            self.toast("Load a waveform or start a simulation first");
            return true;
        }
        match key_code {
            key::RUNSTOP => {
                if self.m.run == RunState::Run {
                    self.m.run = RunState::Stop;
                    self.sim_play = false;
                    self.m.status = TrigStatus::Stop;
                } else {
                    if self.m.trig_mode == TrigMode::Single { self.m.trig_mode = TrigMode::Normal; }
                    self.m.run = RunState::Run;
                    self.sim_play = true;
                    self.m.status = TrigStatus::Wait;
                    self.replay.next = 0.0;
                }
            }
            key::SINGLE => {
                self.operation_serial = self.operation_serial.wrapping_add(1);
                self.single_operation = self.operation_serial;
                self.m.trig_mode = TrigMode::Single;
                self.m.run = RunState::Run;
                self.sim_play = true;
                self.m.status = TrigStatus::Wait;
                self.replay.next = 0.0;
            }
            key::FORCE => {
                self.replay.force = true;
                self.m.run = RunState::Run;
                self.sim_play = true;
                self.replay.next = 0.0;
            }
            _ => return false,
        }
        // A user arm/force starts immediately; the prior capture's periodic
        // cadence cannot delay a new Single or Run after Stop/cancellation.
        if self.m.run==RunState::Run {self.replay.sim_next=self.now;}
        true
    }

    pub(super) fn replay_acquire(&mut self, _dt_wall: f64) {
        if self.m.run != RunState::Run || !self.sim_play || self.now < self.replay.next { return; }
        self.replay.next = self.now + 0.1;
        let slot = self.m.trig_ref.unwrap_or_else(|| self.active_reference());
        let Some(r) = self.refs.get(slot).and_then(Option::as_ref) else { return; };
        if r.v.len() < 8 || r.dt <= 0.0 { return; }
        let t0 = self.source_time_origin(WaveId::Ref(slot), r.t0);
        let count = ((self.view().0 as f64 * HDIV as f64 / r.dt).ceil() as usize).clamp(8, r.v.len());
        let Some(hits) = self.reference_trigger_hits(slot) else { self.m.status = TrigStatus::Wait; return; };
        let after = t0 + self.replay.cursor * r.dt;
        let next = hits.partition_point(|&time| time <= after);
        let hit = hits.get(next).or_else(|| hits.first()).copied();
        if hit.is_none() && self.m.trig_mode != TrigMode::Auto && !self.replay.force {
            self.m.status = TrigStatus::Wait;
            return;
        }
        let middle = hit.map(|time| (time - t0) / r.dt).unwrap_or((self.replay.cursor + count as f64 * 0.5).min((r.v.len() - 1) as f64));
        let lo = (middle.max(count as f64 * 0.5) as usize).saturating_sub(count / 2).min(r.v.len() - count);
        let hi = lo + count;
        let centre = t0 + middle * r.dt;
        self.replay.cursor = (middle + (count / 2).max(1) as f64) % r.v.len() as f64;
        self.replay.frame = Some((slot, lo, hi));
        self.replay.force = false;
        self.replay.captures += 1;
        self.replay.trigger_time = hit;
        if self.session_is_sim && self.replay.fresh_sim {
            self.replay.sim_revision = self.replay.sim_revision.wrapping_add(1);
            self.replay.fresh_sim = false;
        }
        if self.m.zoom { self.m.zdelay = centre as f32; } else { self.m.delay = centre as f32; }
        self.replay.viewport = Some((slot, self.view().0, self.view().1));
        self.m.status = if hit.is_some() { TrigStatus::Triggered } else { TrigStatus::Auto };
        if self.m.trig_mode == TrigMode::Single {
            self.scpi_finish_operation(OperationToken::Single(self.single_operation));
            self.m.run = RunState::Stop;
            self.sim_play = false;
        }
        if self.replay.sim_config_change {
            self.invalidate_reference_eye();
            self.clear_measurement_statistics();
            self.replay.sim_config_change = false;
        }
        self.measure_refs();
        self.analyzer_update();
        let rows = self.meas_rows();
        self.mstat.resize(rows.len(), MStat::default());
        let mut pending = Vec::new();
        for (index, (row, stat)) in rows.iter().zip(&mut self.mstat).enumerate() {
            if row.computing { pending.push(index); }
            else { Self::add_capture_statistic(stat, row.v); }
        }
        self.replay.deferred_statistics = (!pending.is_empty()).then(|| DeferredCaptureStatistics {
            capture: self.replay.captures, context: self.capture_statistics_context(), rows: pending });
    }

    pub fn workflow_command(&mut self, action: &str) {
        match action {
            "live" => self.return_to_live(),
            "sim" => self.toggle_menu(MenuId::Sim),
            "load" => self.request_waveform(),
            "measure" => self.open_meas_list(),
            "settings" => self.toggle_menu(MenuId::Measure),
            _ => {}
        }
    }

    pub(super) fn refresh_stopped_reference(&mut self) {
        let slot = self.active_reference();
        let (scale, delay) = self.view();
        let viewport = (slot, scale, delay);
        if self.replay.viewport == Some(viewport) { return; }
        self.replay.viewport = Some(viewport);
        let Some(r) = self.refs.get(slot).and_then(Option::as_ref) else { return; };
        if r.dt <= 0.0 || r.v.len() < 2 { return; }
        let t0 = self.source_time_origin(WaveId::Ref(slot), r.t0);
        let half = scale as f64 * HDIV as f64 * 0.5;
        let lo = (((delay as f64 - half - t0) / r.dt).floor().max(0.0) as usize).min(r.v.len());
        let hi = (((delay as f64 + half - t0) / r.dt).ceil().max(0.0) as usize).min(r.v.len());
        self.replay.frame = Some((slot, lo, hi.max(lo)));
        self.measure_refs();
        self.analyzer_update();
    }

    pub(super) fn record_caption(&self) -> String {
        let slot = self.active_reference();
        format!("{} R{} · {} measurements · #{}", if self.session_is_sim { "SIM" } else { "Reference" }, slot + 1, self.measurement_gate_label(), self.replay.captures)
    }

    pub fn workflow_state(&self) -> String {
        use crate::json::J;
        let status = match self.m.status {
            TrigStatus::Triggered => "TRIGGERED", TrigStatus::Wait => "WAIT", TrigStatus::Auto => "AUTO", TrigStatus::Stop => "STOP",
        };
        J::obj(&[
            ("mode", J::s(if self.session_is_sim { "sim" } else if self.record_mode() { "reference" } else { "live" })),
            ("running", J::Bool(self.m.run == RunState::Run)),
            ("busy", J::Bool(self.sim_generating())),
            ("measurementBusy", J::Bool(self.measurement_busy())),
            ("source", J::s(&if self.record_mode() { format!("R{}", self.active_reference() + 1) } else { format!("CH{}", self.m.active + 1) })),
            ("captures", J::Num(self.replay.captures as f64)),
            ("status", J::s(status)),
            ("gate", J::s(if self.m.meas_full_record { "full-record" } else { "screen" })),
            ("loadRect", self.top_buttons().iter().find(|button| button.0 == TopBtn::Load).map_or(J::Null, |button| {
                let r = button.2;
                J::Arr(vec![J::Num(r.x as f64), J::Num(r.y as f64), J::Num(r.w as f64), J::Num(r.h as f64)])
            })),
        ]).to_string()
    }

    pub(super) fn record_cards(&self) -> Vec<(usize, Rect)> {
        let slots: Vec<_> = (0..NREF).filter(|&i| self.refs[i].is_some()).collect();
        let mut shown: Vec<_> = slots.iter().copied().take(4).collect();
        let active = self.active_reference();
        if !shown.contains(&active) && self.refs[active].is_some() {
            if shown.len() == 4 { shown.pop(); }
            shown.push(active);
        }
        let width = 574 / shown.len().max(1) as i32 - 7;
        shown.into_iter().enumerate().map(|(n, i)| (i, Rect::new(75 + n as i32 * (width + 7), 530, width, 66))).collect()
    }

    pub(super) fn draw_record_cards(&self, cv: &mut Canvas) {
        for (slot, rect) in self.record_cards() {
            let r = self.refs[slot].as_ref().unwrap();
            let col = REF_COLORS[r.color.min(REF_COLORS.len() - 1)].1;
            btn(cv, rect, BAR, if r.on { col } else { CARD_BORDER });
            cv.set_clip(rect.inset(7));
            cv.text(&self.fonts().sb, rect.x + 10, rect.y + 18, &format!("R{}", slot + 1), col);
            cv.text(&self.fonts().s, rect.x + rect.w - 34, rect.y + 18, if r.on { "On" } else { "Off" }, if r.on { col } else { DIM });
            let (scale, rate) = Self::ref_badge_readout(r);
            cv.text(Self::card_font(&self.fonts(), &scale, rect.w - 24), rect.x + 10, rect.y + 42, &scale, TEXT);
            let sub = if r.label.is_empty() { rate } else { format!("{}  \u{b7}  {}", rate, r.label) };
            cv.text(&self.fonts().s, rect.x + 10, rect.y + 58, &sub, DIM);
            cv.no_clip();
        }
    }

    pub(super) fn session_buttons(r: Rect) -> (Rect, Rect) {
        (Rect::new(r.x + 6, r.y + 22, r.w / 2 - 9, 40), Rect::new(r.x + r.w / 2 + 3, r.y + 22, r.w / 2 - 9, 40))
    }

    pub(super) fn draw_session_card(&self, cv: &mut Canvas, r: Rect) {
        btn(cv, r, BAR, CARD_BORDER);
        let title = if self.session_is_sim { "SIMULATION" } else { "STORED WAVEFORM" };
        cv.text(&self.fonts().sb, r.x + 10, r.y + 16, title, ACCENT);
        let (pause, exit) = Self::session_buttons(r);
        btn(cv, pause, BTN, BTN_BORDER);
        cv.text_in(&self.fonts().sb, pause, 1, if self.m.run == RunState::Run { "Stop" } else { "Run" }, TEXT);
        btn(cv, exit, BTN, BTN_BORDER);
        cv.text_in(&self.fonts().sb, exit, 1, "Live", TEXT);
    }
}
