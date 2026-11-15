//! Batch 2: hidden side effects and wrong data (trigger type, SIM, ref math, setup refs, ref sources, axes, power).
use super::*;

fn fresh(layout: Layout) -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = layout;
    for _ in 0..6 { u.step(0.05); }
    u
}

fn shot(u: &mut Ui, name: &str) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    for _ in 0..4 { u.step(0.05); }
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn sine_ref(i: usize, n: usize, dt: f64) -> RefWave {
    RefWave { src: 0, color: i, label: format!("R{}", i + 1), t0: 0.0, dt,
        v: (0..n).map(|k| (k as f64 * dt * 5e3 * std::f64::consts::TAU).sin() as f32).collect::<Vec<_>>().into(), vpd: 0.5, offset: 0.0, on: true }
}

fn only_ch1(u: &mut Ui) {
    for c in 1..NCH { u.m.ch[c].on = false; }
    u.m.ch[0].on = true;
}

#[test]
fn trigger_type_change_touches_only_the_trigger() {
    for t in TRIG_TYPES {
        let mut u = fresh(Layout::Minimal);
        only_ch1(&mut u);
        let (on, dec_on, proto, vscale1) = (std::array::from_fn::<bool, NCH, _>(|i| u.m.ch[i].on), u.m.dec_on, u.m.dec_proto, u.m.ch[1].vscale);
        let off = u.m.ch[1].offset;
        let src = u.m.trig_src;
        u.trigger_type_set(t);
        assert_eq!(std::array::from_fn::<bool, NCH, _>(|i| u.m.ch[i].on), on, "{t:?} must not switch channels");
        assert_eq!(u.m.dec_on, dec_on, "{t:?} must not create a decode");
        assert_eq!(u.m.dec_proto, proto);
        assert_eq!((u.m.ch[1].vscale, u.m.ch[1].offset), (vscale1, off), "{t:?} must not rescale a channel");
        if !matches!(t, TrigType::Bus) { assert_eq!(u.m.trig_src, src, "{t:?} keeps the source"); }
    }
}

#[test]
fn trigger_second_source_stays_on_an_enabled_channel() {
    let mut u = fresh(Layout::Minimal);
    u.m.ch[0].on = true; u.m.ch[1].on = true; u.m.ch[2].on = false; u.m.ch[3].on = false;
    u.trigger_type_set(TrigType::SetupHold);
    assert!(u.m.ch[u.m.trig_src].on && u.m.ch[u.m.trig_src2].on);
    assert_ne!(u.m.trig_src, u.m.trig_src2);
}

#[test]
fn choosing_a_decode_protocol_leaves_the_trigger_alone() {
    let mut u = fresh(Layout::Minimal);
    let before = (u.m.trig_type, u.m.trig_src, u.m.trig_level, u.m.slope);
    u.set_dec_proto(Proto::Spi);
    assert_eq!((u.m.trig_type, u.m.trig_src, u.m.trig_level, u.m.slope), before);
}

#[test]
fn shots_trigger_type() {
    for (name, t) in [("pulse", TrigType::Pulse), ("setuphold", TrigType::SetupHold)] {
        let mut u = fresh(Layout::Minimal);
        only_ch1(&mut u);
        u.trigger_type_set(t);
        shot(&mut u, &format!("trig-{name}"));
    }
}

#[test]
fn trigger_pill_shows_the_level_the_trigger_page_shows() {
    for layout in [Layout::Minimal, Layout::Studio] {
        let mut u = fresh(layout);
        for t in [TrigType::Pattern, TrigType::Duration] {
            u.trigger_type_set(t);
            u.m.pat_levels[u.m.trig_src] = 1.25;
            u.m.trig_level = 0.15;
            assert_eq!(u.value(Item::PatLevel), "1.25 V");
            assert_eq!(u.trig_level_shown(), 1.25);
        }
    }
}

#[test]
fn sim_keeps_a_loaded_ref_and_every_channel() {
    let mut u = fresh(Layout::Default);
    u.refs[0] = Some(sine_ref(0, 2000, 1e-6));
    u.refs[0].as_mut().unwrap().label = "mine.csv".into();
    u.m.ch[0].on = true; u.m.ch[1].on = true;
    u.m.sim_slot = 0;
    u.sim_play_set(true).unwrap();
    u.wait_sim_generation();
    assert_eq!(u.refs[0].as_ref().unwrap().label, "mine.csv", "the loaded ref is not overwritten");
    assert_eq!(u.m.sim_slot, 1, "the SIM took the first free slot");
    assert!(u.refs[1].is_some() && u.session_is_sim);
    assert!(u.m.ch[0].on && u.m.ch[1].on, "the SIM does not turn channels off");
    let toast = u.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
    assert!(toast.ends_with("to R2"), "{toast}");
}

#[test]
fn sim_with_every_slot_full_says_which_ref_it_replaced() {
    let mut u = fresh(Layout::Default);
    for i in 0..NREF { u.refs[i] = Some(sine_ref(i, 200, 1e-6)); u.refs[i].as_mut().unwrap().label = format!("file{i}.csv"); }
    u.m.sim_slot = 0;
    u.sim_play_set(true).unwrap();
    u.wait_sim_generation();
    let toast = u.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
    assert!(toast.contains("to R1, replacing file0.csv"), "{toast}");
}

#[test]
fn restarting_the_sim_reuses_its_own_slot() {
    let mut u = fresh(Layout::Default);
    u.sim_play_set(true).unwrap();
    u.wait_sim_generation();
    assert_eq!(u.m.sim_slot, 0);
    u.return_to_live();
    u.sim_play_set(true).unwrap();
    u.wait_sim_generation();
    assert_eq!(u.m.sim_slot, 0, "the earlier output of the same preset is not a loaded ref");
    assert!(u.refs[1].is_none());
}

#[test]
fn shots_refs_sim_axis_power() {
    for layout in [Layout::Default, Layout::Studio] {
        let name = format!("{layout:?}").to_lowercase();
        let mut u = fresh(layout);
        for c in 1..NCH { u.m.ch[c].on = false; }
        u.refs[0] = Some(sine_ref(0, 6000, 1e-6));
        u.refs[1] = Some(sine_ref(1, 6000, 1e-6));
        u.refs[1].as_mut().unwrap().color = 2;
        u.relayout();
        shot(&mut u, &format!("refs-live-{name}"));
        u.m.ch[0].on = false;
        u.relayout();
        shot(&mut u, &format!("refs-only-{name}"));
        u.m.ch[0].on = true;
        u.m.math = MathOp::Sub; u.m.math_a = 0; u.m.math_b = MATH_REF_BASE;
        for _ in 0..4 { u.step(0.05); }
        shot(&mut u, &format!("math-ref-{name}"));
        let mut s = fresh(layout);
        s.refs[0] = Some(sine_ref(0, 6000, 1e-6));
        s.refs[0].as_mut().unwrap().label = "mine.csv".into();
        s.sim_play_set(true).unwrap();
        s.wait_sim_generation();
        for _ in 0..4 { s.step(0.05); }
        shot(&mut s, &format!("sim-{name}"));
    }
    let mut p = fresh(Layout::Default);
    for c in 0..NCH { p.m.ch[c].on = false; }
    p.refs[0] = Some(sine_ref(0, 4000, 1e-6));
    p.refs[1] = Some(sine_ref(1, 4000, 1e-6));
    p.refs[1].as_mut().unwrap().v = vec![0.0; 4000].into();
    p.m.view = AView::Power;
    (p.m.pw_v, p.m.pw_i) = (WaveId::Ref(0), WaveId::Ref(1));
    p.analyzer_update();
    shot(&mut p, "power-noprobe");
}
