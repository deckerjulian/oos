//! Batch 3A: the Studio top bar, SIM follows Run / Stop / Single, sources shown, compact view, saved desktop settings, zoom.
use super::studio::StudioView;
use super::*;

fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    for _ in 0..8 { u.step(0.05); }
    u
}

fn shot(u: &mut Ui, name: &str) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    for _ in 0..6 { u.step(0.05); }
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn tap_in(u: &mut Ui, r: Rect) { u.tap(r.x + r.w / 2, r.y + r.h / 2); }

#[test]
fn shots_top_bar_vs_default() {
    let mut d = Ui::new(Source::Demo { t: 0.0 });
    d.m.layout = Layout::Default;
    for _ in 0..8 { d.step(0.05); }
    shot(&mut d, "topbar-default");
    let mut s = studio();
    shot(&mut s, "topbar-studio");
}

fn sim_ui() -> Ui {
    let mut u = studio();
    u.store_dir = std::env::temp_dir().join("oos-batch3a-sim");
    u.studio_set_view(StudioView::Sim);
    for _ in 0..3 { u.step(0.05); }
    u
}

#[test]
fn top_bar_is_one_status_pill_a_single_button_plain_acquisition_and_the_default_buttons() {
    let mut u = studio();
    let t = u.studio_top();
    // the pill and Single have the Default bar's size and place
    assert_eq!((t.run.x, t.run.y, t.run.h), (8, 6, 28));
    assert_eq!((t.single.h, t.single.w), (30, 54));
    assert_eq!(t.badge.w, 0, "no Live button while live");
    assert_eq!(t.sim.w, 0, "no SIM control while the SIM is not the source");
    // the source is in the caption, not a badge of its own
    assert!(u.studio_acq_caption().starts_with("DEMO \u{b7} "), "{}", u.studio_acq_caption());
    // acquisition is plain text over the room before Measure and opens Acquire
    assert!(t.acq.right() < t.measure.x);
    tap_in(&mut u, t.acq);
    assert_eq!(u.menu, MenuId::Acquire);
    u.close_menu();
    // a tap on the pill is Run / Stop, as in Default
    let run = u.m.run;
    tap_in(&mut u, t.run);
    assert_ne!(u.m.run, run);
    // nothing hangs under the bar: the Meas tab is in the badge row at the bottom
    u.studio_set_pin_all_for_test();
    if let Some(tab) = u.studio_tab() { assert!(tab.y >= u.grid.bottom(), "the Meas tab is under the lanes"); }
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    assert!(u.studio_head_h() <= 18 && u.grid.y - u.frame().top.bottom() <= 18, "only the T row sits under the bar");
}

#[test]
fn run_stop_and_single_play_hold_and_capture_the_sim() {
    let mut u = sim_ui();
    let t = u.studio_top();
    assert!(!u.record_mode());
    // Run on the open SIM panel plays the SIM source
    tap_in(&mut u, t.run);
    u.wait_sim_generation();
    for _ in 0..8 { u.step(0.05); }
    assert!(u.session_is_sim && u.sim_playing() && u.m.run == RunState::Run);
    // the bar now has the mini SIM control with the preset, and Live
    let t = u.studio_top();
    assert!(t.sim.w > 0 && t.badge.w > 0);
    assert!(t.acq.x >= t.sim.right());
    shot(&mut u, "sim-bar-playing");
    // Stop holds the current frame
    let held = u.refs[u.m.sim_slot].as_ref().unwrap().v.clone();
    tap_in(&mut u, t.run);
    for _ in 0..20 { u.step(0.05); }
    u.wait_sim_generation();
    assert!(!u.sim_playing() && u.m.run == RunState::Stop);
    assert_eq!(u.refs[u.m.sim_slot].as_ref().unwrap().v.len(), held.len());
    assert_eq!(u.run_caption().0, "STOP");
    // Run plays again
    tap_in(&mut u, t.run);
    for _ in 0..4 { u.step(0.05); }
    assert!(u.sim_playing() && u.m.run == RunState::Run);
    // Single captures one frame and holds
    tap_in(&mut u, t.single);
    u.wait_sim_generation();
    for _ in 0..40 { u.step(0.05); u.wait_sim_generation(); }
    assert_eq!(u.m.trig_mode, TrigMode::Single);
    assert!(!u.sim_playing() || u.m.run == RunState::Stop || u.m.status != TrigStatus::Auto, "one frame, then held");
    // a SIM started elsewhere (the SIM menu) shows the chip too; a tap opens the SIM panel
    let mut u = studio();
    u.store_dir = std::env::temp_dir().join("oos-batch3a-sim2");
    u.sim_activate(Item::SimPlay);
    u.wait_sim_generation();
    for _ in 0..8 { u.step(0.05); }
    assert!(u.session_is_sim && u.studio.view != StudioView::Sim);
    let t = u.studio_top();
    assert!(t.sim.w > 0);
    tap_in(&mut u, t.sim);
    assert_eq!(u.studio.view, StudioView::Sim);
}

fn sim_eye(preset: &str) -> Ui {
    let mut u = studio();
    u.store_dir = std::env::temp_dir().join("oos-batch3a-eye");
    assert!(u.sim_preset_set(preset));
    u.studio_set_view(StudioView::Sim);
    u.sim_activate(Item::SimPlay);
    u.wait_sim_generation();
    for _ in 0..10 { u.step(0.05); u.wait_sim_generation(); }
    u
}

fn wait_eye(u: &mut Ui) -> bool {
    for _ in 0..600 {
        u.step(0.05); u.wait_sim_generation(); std::thread::sleep(std::time::Duration::from_millis(50));
        if u.azr.eye.is_some() { return true; }
    }
    false
}

#[test]
fn eye_on_a_stopped_sim_ref_computes_instead_of_waiting_for_edges() {
    // PCIe 5.0 at 32 Gb/s into R1, then Stop holds the frame: no new record arrives to trigger the analysis
    let mut u = sim_eye("PCIe 5.0");
    assert_eq!(u.m.eye_src, WaveId::Ref(u.m.sim_slot));
    u.key(key::RUNSTOP);
    for _ in 0..5 { u.step(0.05); }
    assert!(!u.sim_playing());
    u.m.view = AView::Eye;
    u.step(0.05);
    assert!(wait_eye(&mut u), "the eye of a held SIM frame is computed: {:?}", u.azr.page.status);
    assert!(!u.azr.page.status[0].starts_with("Waiting"), "{:?}", u.azr.page.status);
}

#[test]
fn studio_eye_opens_beside_the_plot_and_full_is_an_explicit_choice() {
    let mut u = sim_eye("PCIe 3.0");
    u.key(key::RUNSTOP);
    u.m.eye_place = EyePlace::Full;
    u.m.view = AView::Eye;
    for _ in 0..3 { u.step(0.05); }
    assert_eq!(u.m.eye_place, EyePlace::Side, "Studio opens the eye next to the plot");
    let pane = u.eye_pane_rect();
    assert!(!u.wins.is_empty() && u.wins[0].w > 200, "the waveform lane stays: {:?}", u.wins);
    assert!(u.wins[0].right() <= pane.x + 1 || u.wins[0].bottom() <= pane.y + 1, "beside or under, not over it");
    shot(&mut u, "eye-side");
    // Full by choice stays Full, also when the eye is opened again
    u.eye_set_place(EyePlace::Full);
    assert_eq!(u.m.eye_place, EyePlace::Full);
    u.m.view = AView::Scope; u.step(0.05);
    u.m.view = AView::Eye; u.step(0.05);
    assert_eq!(u.m.eye_place, EyePlace::Full);
    // Default and Minimal are not touched
    let mut d = Ui::new(Source::Demo { t: 0.0 });
    d.m.layout = Layout::Default;
    d.m.eye_place = EyePlace::Full;
    d.m.view = AView::Eye;
    for _ in 0..3 { d.step(0.05); }
    assert_eq!(d.m.eye_place, EyePlace::Full);
}
