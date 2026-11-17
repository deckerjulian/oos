// SPDX-License-Identifier: GPL-2.0-or-later

//! Leaving a reference, Take over or SIM session gives back the live set-up from before it, whichever way it is left,
//! and holds nothing of it for a later Live.
use super::*;
use crate::wave::Source;

fn square_ref() -> RefWave {
    let v = (0..1000).map(|i| (i % 50) as f32 / 50.0).collect::<Vec<_>>();
    RefWave { src: 1, color: 0, label: "r".into(), t0: 0.0, dt: 1e-9, v: v.into(), vpd: 1.0, offset: 0.0, on: true }
}

/// What a session must not take with it: the channels, the time base and the measurements.
fn live_setup(u: &Ui) -> (Vec<bool>, usize, Vec<(Meas, WaveId)>) {
    (u.m.ch.iter().map(|c| c.on).collect(), u.m.tscale, u.m.meas.clone())
}

#[test]
fn trigger_source_back_on_a_channel_brings_back_the_live_setup() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    for c in 0..3 { u.m.ch[c].on = true; }
    u.m.tscale = 10;
    u.m.meas = vec![(Meas::Freq, WaveId::Ch(1)), (Meas::Rise, WaveId::Ch(2))];
    u.step(0.05);
    u.refs[0] = Some(square_ref());
    let before = live_setup(&u);
    // Trigger > Source: R1, then CH2
    assert!(u.set_trigger_source(WaveId::Ref(0), false));
    u.step(0.05);
    assert!(u.record_mode());
    assert!(u.set_trigger_source(WaveId::Ch(1), false));
    u.step(0.05);
    assert!(!u.record_mode() && u.workflow_hold.is_none());
    assert_eq!(live_setup(&u), before, "the channels, time base and measurements of before R1");
    assert_eq!(u.m.trigger_wave(), WaveId::Ch(1));
    // settings made since are what the next session gives back, not those of the first
    u.m.tscale = 14;
    u.m.meas = vec![(Meas::Freq, WaveId::Ch(1))];
    let later = live_setup(&u);
    assert!(u.set_trigger_source(WaveId::Ref(0), false));
    u.step(0.05);
    u.return_to_live();
    u.step(0.05);
    assert_eq!(live_setup(&u), later);
}

#[test]
fn a_channel_key_in_sim_brings_back_the_live_setup() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.ch[0].on = true;
    u.m.ch[1].on = true;
    u.m.tscale = 20;
    u.m.meas = vec![(Meas::Freq, WaveId::Ch(1))];
    u.step(0.05);
    let before = live_setup(&u);
    u.sim_play_set(true).unwrap();
    u.wait_sim_generation();
    u.step(0.05);
    assert!(u.record_mode());
    u.key(key::CH1 + 1);
    u.step(0.05);
    assert!(!u.record_mode() && u.workflow_hold.is_none());
    assert_eq!(live_setup(&u), before, "not SIM's time base and measurements");
    assert_eq!((u.m.trigger_wave(), u.m.run), (WaveId::Ch(1), RunState::Run), "CH2 acquires");
}

#[test]
fn take_over_and_live_bring_back_the_live_setup() {
    // left by the top bar's Live, and by Record = With live in the Reference menu
    for with_live in [false, true] {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.ch[0].on = false;
        u.m.ch[1].on = true;
        u.m.active = 1;
        u.m.meas = vec![(Meas::Freq, WaveId::Ch(1)), (Meas::Rise, WaveId::Ch(1))];
        u.m.view = AView::Spectrum;
        u.m.tscale = 10;
        u.step(0.05);
        u.refs[0] = Some(square_ref());
        let before = (live_setup(&u), u.m.view);
        // Reference menu > Record: Take over
        u.adjust(Item::Offline, 1);
        u.step(0.05);
        assert!(u.record_mode());
        if with_live { u.adjust(Item::Offline, -1); } else { u.return_to_live(); }
        u.step(0.05);
        assert!(!u.record_mode() && u.workflow_hold.is_none());
        assert_eq!((live_setup(&u), u.m.view), before, "with live: {with_live}");
    }
}

/// A storage directory of this test alone.
fn store(u: &mut Ui, name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("oos-session-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    u.store_dir = dir.clone();
    dir
}

#[test]
fn a_setup_recalled_in_sim_is_what_live_keeps() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    let dir = store(&mut u, "recall");
    u.m.tscale = 20;
    u.step(0.05);
    let mut saved = u.m.clone();
    saved.tscale = 25;
    saved.ch[3].on = true;
    saved.meas = vec![(Meas::Rise, WaveId::Ch(3))];
    std::fs::write(u.setup_path(), crate::store::setup_text(&saved)).unwrap();
    u.sim_play_set(true).unwrap();
    u.wait_sim_generation();
    u.step(0.05);
    assert!(u.record_mode());
    u.recall_setup();
    u.step(0.05);
    let recalled = live_setup(&u);
    assert_eq!((recalled.1, recalled.2.clone()), (25, saved.meas.clone()));
    u.return_to_live();
    u.step(0.05);
    assert_eq!(live_setup(&u), recalled, "Live keeps the recalled set-up");
    let _ = std::fs::remove_dir_all(&dir);
}
