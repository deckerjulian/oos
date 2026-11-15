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

#[test]
fn compact_view_shrinks_lanes_badges_and_panel_and_toggles_from_the_view_menu_and_the_row() {
    let mut u = studio();
    u.studio_set_pin_all_for_test();
    u.studio_set_view(StudioView::Docked);
    for _ in 0..3 { u.step(0.05); }
    let (gap0, row0, panel0, prow0) = (u.wins[1].y - u.wins[0].bottom(), u.studio_row_px(), u.studio_panel_w(), u.studio_panel_row());
    let plot0 = u.wins[0].w;
    shot(&mut u, "compact-off");
    // the View menu has the row
    u.studio.view_menu = true;
    let row = u.studio_view_row(super::studio::VIEW_COMPACT);
    assert!(u.studio_view_rect().contains(row.x, row.y) && u.studio_view_rect().contains(row.right() - 1, row.bottom() - 1));
    tap_in(&mut u, row);
    assert!(u.studio.compact);
    for _ in 0..3 { u.step(0.05); }
    assert!(u.wins[1].y - u.wins[0].bottom() < gap0, "smaller lane gap");
    assert!(u.studio_row_px() < row0, "smaller badge row");
    assert!(u.studio_panel_w() < panel0 && u.studio_panel_row() < prow0, "narrower panel");
    assert!(u.wins[0].w > plot0, "the lanes get the room");
    shot(&mut u, "compact-on");
    // the small toggle in the badge row turns it off again
    u.studio.view_menu = false;
    let c = u.studio_compact_chip();
    assert!(u.studio_row().contains(c.x, c.y));
    tap_in(&mut u, c);
    assert!(!u.studio.compact);
    // Default is not touched by the flag
    let mut d = Ui::new(Source::Demo { t: 0.0 });
    d.studio.compact = true;
    d.step(0.05);
    assert!(d.m.layout != Layout::Studio && d.split_wins(d.grid).len() == 1);
}

#[test]
fn bottom_bar_shows_the_sources_that_are_on_and_one_plus_ch_chip_for_the_rest() {
    let mut u = studio();
    let b = u.studio_bottom();
    // CH1 and CH2 are on: two channel chips, one "+ CH" for CH3 and CH4, no "Off" chips
    assert!(b.ch[0].w > 0 && b.ch[1].w > 0 && b.ch[2].w == 0 && b.ch[3].w == 0);
    assert!(b.add.w > 0);
    shot(&mut u, "bottom-sources");
    // a ref and a math channel that are shown get chips
    u.refs[0] = Some(RefWave { src: 0, color: 0, label: "R1".into(), t0: 0.0, dt: 1e-6, v: vec![0.0f32; 100].into(), vpd: 1.0, offset: 0.0, on: true });
    u.m.math_select(0); u.m.math = MathOp::Add;
    let b = u.studio_bottom();
    assert!(b.refs[0].w > 0 && b.math[0].w > 0 && b.math[1].w == 0, "refs and math that are on");
    let all = [b.ch[0], b.ch[1], b.refs[0], b.math[0], b.add, b.time, b.trig];
    for (i, a) in all.iter().enumerate() { for c in &all[i + 1..] { assert!(!overlaps(*a, *c), "{a:?} {c:?}"); } }
    shot(&mut u, "bottom-sources-ref-math");
    // the chip opens a picker of the channels that are off; a row turns one on
    tap_in(&mut u, b.add);
    let rows = u.studio_ch_picker().expect("the picker");
    assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), [2, 3]);
    shot(&mut u, "bottom-picker");
    assert!(!u.m.ch[2].on);
    tap_in(&mut u, rows[0].1);
    assert!(u.m.ch[2].on && u.studio.ch_picker == false);
    assert!(u.studio_bottom().ch[2].w > 0, "CH3 now has its chip");
    // all channels on: no "+ CH"
    for c in 0..n_ch() { u.m.ch[c].on = true; }
    assert_eq!(u.studio_bottom().add.w, 0);
}
