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
