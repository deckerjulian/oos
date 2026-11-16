//! Batch 3A: the Studio top bar, SIM follows Run / Stop / Single, sources shown, compact view, saved desktop settings, zoom.
use super::studio::StudioView;
use super::*;
use super::{axis_label, studio_time_ticks};

fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.studio_detail = false;
    u.m.studio_zoom_tab = false;
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
    assert!(t.acq.right() < t.add.x);
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
    assert!(u.grid.y - u.frame().top.bottom() <= 40, "only the strip sits under the bar; T is inside the scale");
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
    // batch 22: with the panel open there is no row; the View menu has the switch
    assert_eq!(u.studio_row_h(), 0);
    u.studio.view_menu = true;
    let c = u.studio_view_row(super::studio::VIEW_COMPACT);
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
    // all channels on: the chip stays, as "Add source" (math, ref and bus are behind it)
    for c in 0..n_ch() { u.m.ch[c].on = true; }
    assert!(u.studio_bottom().add.w > 0);
}

#[test]
fn pinned_badges_follow_the_live_source_and_a_hidden_ref_leaves_no_rows() {
    let mut u = studio();
    u.refs[0] = Some(RefWave { src: 0, color: 0, label: "R1".into(), t0: 0.0, dt: 1e-6, v: (0..1000).map(|i| (i as f32 * 0.05).sin()).collect::<Vec<_>>().into(), vpd: 1.0, offset: 0.0, on: true });
    u.m.meas.push((Meas::Vpp, WaveId::Ref(0)));
    let badge_sources = |u: &Ui| u.studio_pinned().into_iter().map(|k| match k { super::studio::MeasKey::One(_, id) => id, _ => unreachable!() }).collect::<Vec<_>>();
    assert!(badge_sources(&u).contains(&WaveId::Ch(0)));
    // CH1 off, the ref is the signal: no CH1 badges
    u.m.ch[0].on = false;
    u.step(0.05);
    let s = badge_sources(&u);
    assert!(!s.is_empty() && s.iter().all(|id| *id == WaveId::Ref(0)), "{s:?}");
    shot(&mut u, "badges-follow-ref");
    // the ref is hidden: its rows go, and CH1 comes back with its channel
    u.refs[0].as_mut().unwrap().on = false;
    u.step(0.05);
    u.render(&mut Canvas::new(W, H));
    assert!(!u.m.meas.iter().any(|&(_, id)| id == WaveId::Ref(0)), "leftover ref rows: {:?}", u.m.meas);
    u.m.ch[0].on = true;
    assert!(badge_sources(&u).iter().all(|id| *id == WaveId::Ch(0)));
}

#[test]
fn trigger_chip_shows_the_real_trigger_source_in_sim() {
    let mut u = sim_ui();
    u.sim_activate(Item::SimPlay);
    u.wait_sim_generation();
    for _ in 0..8 { u.step(0.05); }
    assert!(u.session_is_sim);
    assert_eq!(u.m.trigger_wave(), WaveId::Ref(u.m.sim_slot));
    assert_eq!(u.trigger_source_label(), format!("R{}", u.m.sim_slot + 1));
    shot(&mut u, "trigger-chip-sim");
}

#[test]
fn the_desktop_setup_file_brings_back_the_layout_and_the_studio_view() {
    let dir = std::env::temp_dir().join(format!("oos-setup-file-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // nothing saved yet: a normal start
    let mut fresh = Ui::new(Source::Demo { t: 0.0 });
    let mut file = crate::store::SetupFile::new(&dir);
    assert_eq!(file.restore(&mut fresh), Ok(false));
    // Studio with the measurements docked, a time base, then saved (the folder is created)
    let mut u = studio();
    u.studio_set_view(StudioView::Docked);
    u.m.tscale = 12;
    u.render(&mut Canvas::new(W, H));
    file.save(&u, true);
    assert!(dir.join(crate::store::SetupFile::NAME).is_file());
    // a restart: a new interface reads it
    let mut again = Ui::new(Source::Demo { t: 0.0 });
    assert_eq!(again.m.layout, Layout::Minimal);
    let mut file2 = crate::store::SetupFile::new(&dir);
    assert_eq!(file2.restore(&mut again), Ok(true));
    assert_eq!(again.m.layout, Layout::Studio);
    assert_eq!(again.studio.view, StudioView::Docked);
    assert_eq!(again.m.tscale, 12);
    for _ in 0..6 { again.step(0.05); }
    again.render(&mut Canvas::new(W, H));
    assert!(again.stacked() && again.studio.view == StudioView::Docked);
    // an unchanged setup writes nothing; a change is written by the next save
    let before = std::fs::metadata(dir.join(crate::store::SetupFile::NAME)).unwrap().modified().unwrap();
    file2.save(&again, true);
    assert_eq!(std::fs::metadata(dir.join(crate::store::SetupFile::NAME)).unwrap().modified().unwrap(), before);
    again.m.tscale = 10;
    file2.save(&again, true);
    assert_eq!(std::fs::read_to_string(dir.join(crate::store::SetupFile::NAME)).unwrap(), crate::store::setup_text(&again.m));
    // a damaged file is reported, not applied
    std::fs::write(dir.join(crate::store::SetupFile::NAME), "layout = wide\n").unwrap();
    let mut third = Ui::new(Source::Demo { t: 0.0 });
    assert!(crate::store::SetupFile::new(&dir).restore(&mut third).is_err());
    assert_eq!(third.m.layout, Layout::Minimal);
    let _ = std::fs::remove_dir_all(&dir);
}

fn scale_index(t: f32) -> usize { tscales().iter().position(|&s| (s / t - 1.0).abs() < 1e-3).expect("a time scale") }

/// The owner's two cases on the demo source: main 500 us/div with zoom 2.5x, main 50 ms/div with zoom 250x.
fn zoomed_demo(main: f32, zoom: f32, zdelay: f32) -> Ui {
    let mut u = studio();
    u.m.tscale = scale_index(main);
    u.m.delay = 0.0;
    for _ in 0..8 { u.step(0.05); }
    u.set_zoom(true);
    u.m.zscale = scale_index(zoom);
    u.m.zdelay = zdelay;
    u.clamp_zoom();
    u.relayout();
    for _ in 0..8 { u.step(0.05); }
    u
}

#[test]
fn shots_zoom_cases() {
    let mut a = zoomed_demo(500e-6, 200e-6, 0.439e-3);
    shot(&mut a, "zoom-a");
    let mut b = zoomed_demo(50e-3, 200e-6, 0.439e-3);
    shot(&mut b, "zoom-b");
}

#[test]
fn zoom_axis_labels_are_round_at_any_delay() {
    // the owner's case: 200 us/div in a window put at 0.439 ms
    let ticks = studio_time_ticks(200e-6, 0.439e-3, 1);
    assert_eq!(ticks.first().map(|t| t.0), Some(-2), "first label -0.4 ms, not -0.561 ms");
    for (k, t) in &ticks { assert!((t - *k as f64 * 200e-6).abs() < 1e-9, "{k} {t}"); }
    let labels: Vec<String> = ticks.iter().map(|&(k, t)| if k == 0 { "0 s".into() } else { axis_label(t as f32, 200e-6, 0.0, 1.439e-3, "s") }).collect();
    assert_eq!(labels, ["-0.4 ms", "-0.2 ms", "0 s", "0.2 ms", "0.4 ms", "0.6 ms", "0.8 ms", "1.0 ms", "1.2 ms", "1.4 ms"]);
    // a window with the trigger in the middle has the labels the main axis has
    let c = studio_time_ticks(200e-6, 0.0, 1);
    assert_eq!(c.len(), 11);
    assert_eq!((c[0].0, c[10].0), (-5, 5));
    // 2.5x at 500 us/div and 250x at 50 ms/div read the same way
    assert!(studio_time_ticks(50e-3, 0.439e-3, 1).iter().all(|&(k, t)| (t - k as f64 * 50e-3).abs() < 1e-5));
}

#[test]
fn zoom_detail_shows_real_samples_not_invented_ramps() {
    // 50 ms/div main: the record has 3.6 kpts at 7.2 kSa/s; the zoom at 200 us/div has 1.4 samples per division
    let mut u = zoomed_demo(50e-3, 200e-6, 0.439e-3);
    assert!(u.studio_zoom_active());
    let dt = u.studio_zoom_sample_dt().unwrap();
    assert!((dt - 50e-3 * 10.0 / 3600.0).abs() < 1e-9, "dt {dt}");
    assert!(u.zoom_sparse(dt), "the zoom asks for more than the record has");
    let hint = u.studio_zoom_samples_hint().expect("the hint");
    assert!(hint.ends_with("samples/div") && hint.starts_with("1.4"), "{hint}");
    assert!(u.phz.fat, "each sample is marked with a dot");
    // the samples are joined: the pixel columns between two samples are lit
    let lane = u.wins[0];
    let map = &u.phz.map[0];
    let lit_cols = (0..u.phz.w).filter(|&x| (0..u.phz.h).any(|y| map[y * u.phz.w + x] > 0)).count();
    assert!(lit_cols > u.phz.w / 2, "the samples are joined by a line: {lit_cols} of {}", u.phz.w);
    shot(&mut u, "zoom-b");
    // the zoom of a record with enough samples is a line and has no hint
    let a = zoomed_demo(500e-6, 200e-6, 0.439e-3);
    assert!(!a.zoom_sparse(a.studio_zoom_sample_dt().unwrap()) && a.studio_zoom_samples_hint().is_none() && !a.phz.fat);
    let _ = lane;
}

#[test]
fn zoom_factor_time_per_div_and_the_overview_window_agree() {
    for (main, zoom, factor) in [(500e-6, 200e-6, 2.5f32), (50e-3, 200e-6, 250.0), (50e-3, 5e-3, 10.0)] {
        let u = zoomed_demo(main, zoom, 0.0);
        assert!((u.studio_time_zoom() / factor - 1.0).abs() < 0.01, "{} vs {factor}", u.studio_time_zoom());
        let (pct, f) = u.zoom_box();
        assert!((f / factor - 1.0).abs() < 0.01);
        assert!((pct - 50.0).abs() < 0.5, "centered box is at 50 %: {pct}");
        let (o, _) = u.split_zoom(u.grid);
        let (xa, xb) = u.zoom_window_x(o);
        let want = o.w as f32 / factor;
        assert!(((xb - xa) as f32 - want).abs() <= 1.5, "window {} px, {} expected", xb - xa, want);
    }
    // a 250x window is a thin line in the overview: drawn at least as a visible bar
    let mut u = zoomed_demo(50e-3, 200e-6, 0.0);
    shot(&mut u, "zoom-thin-window");
}

#[test]
fn center_trigger_puts_t_in_the_middle_of_the_zoom_window() {
    for (main, zoom) in [(500e-6, 200e-6), (50e-3, 200e-6)] {
        let mut u = zoomed_demo(main, zoom, 0.439e-3);
        assert!(u.studio_trigger_off_screen_for_test() || u.m.zdelay != 0.0);
        let g = u.grid;
        assert!((u.studio_x_of_time(0.0) - (g.x + g.w / 2)).abs() > 20, "T starts off centre");
        u.studio_center_trigger();
        assert_eq!(u.m.zdelay, 0.0);
        assert!((u.studio_x_of_time(0.0) - (g.x + g.w / 2)).abs() <= 1, "T at {} of {}", u.studio_x_of_time(0.0), g.w);
    }
    // also when the main window was moved away from the trigger: the box can only follow the window
    let mut u = zoomed_demo(500e-6, 200e-6, 0.0);
    u.m.delay = 4e-3;
    u.m.zdelay = 4e-3;
    u.clamp_zoom();
    u.studio_center_trigger();
    assert_eq!(u.m.zdelay, 0.0);
    let (pct, _) = u.zoom_box();
    assert!((pct - 50.0).abs() < 0.5);
}

#[test]
fn panning_the_zoom_never_changes_the_main_time_base() {
    let mut u = zoomed_demo(500e-6, 200e-6, 0.0);
    let (tscale, tfine, delay) = (u.m.tscale, u.m.tfine, u.m.delay);
    let (o, z) = u.split_zoom(u.grid);
    let same = |u: &Ui| (u.m.tscale, u.m.tfine, u.m.delay) == (tscale, tfine, delay);
    // a drag in the zoom view pans the box
    let zdelay = u.m.zdelay;
    touch(&mut u, Phase::Down, z.x + 300, z.y + 100);
    touch(&mut u, Phase::Move, z.x + 400, z.y + 100);
    touch(&mut u, Phase::Move, z.x + 500, z.y + 100);
    touch(&mut u, Phase::Up, z.x + 500, z.y + 100);
    assert!(u.m.zdelay != zdelay, "the box moved");
    assert!(same(&u), "a drag in the zoom view: main {} {}", u.m.tscale, u.m.delay);
    // a drag of the box in the overview
    let (xa, xb) = u.zoom_window_x(o);
    let mid = (xa + xb) / 2;
    let before = u.m.zdelay;
    touch(&mut u, Phase::Down, mid, o.y + 20);
    touch(&mut u, Phase::Move, mid + 40, o.y + 20);
    touch(&mut u, Phase::Move, mid + 80, o.y + 20);
    touch(&mut u, Phase::Up, mid + 80, o.y + 20);
    assert!(u.m.zdelay != before, "the box moved in the overview");
    assert!(same(&u), "a drag of the box in the overview");
    // the wheel over the zoom view leaves the zoom alone (Studio); on the overview it changes the box width
    let (zs, zd0) = (u.m.zscale, u.m.zdelay);
    assert!(!u.pointer_wheel(z.x + 200, z.y + 100, 1, false));
    assert!((u.m.zscale, u.m.zdelay) == (zs, zd0) && same(&u), "the wheel over the zoom view does nothing");
    for (x, y) in [(o.x + 200, o.y + 40)] {
        let zs = u.m.zscale;
        assert!(u.pointer_wheel(x, y, 2, false));
        assert!(u.m.zscale != zs, "the wheel changes the box width");
        assert!(same(&u), "wheel at {x},{y} must not touch the main time base");
    }
    // the toolbar buttons, the knobs: zoom width and pan
    u.adjust(Item::ZDelay, 3);
    assert!(same(&u));
    u.adjust(Item::ZScale, -1);
    assert!(same(&u), "the zoom width knob changes only the zoom");
    // the zoom view does not magnify by the wheel, Ctrl included
    let z0 = u.m.zscale;
    assert!(!u.pointer_wheel(z.x + 300, z.y + 100, 1, true));
    assert!(u.m.zscale == z0 && same(&u));
    // only a pinch / Ctrl-wheel on the overview changes the main time/div
    assert!(u.pointer_wheel(o.x + 300, o.y + 40, 1, true));
    assert!(u.m.tscale != tscale, "the overview magnifies the main time base");
}

#[test]
fn drawing_a_box_turns_the_zoom_on_framed_to_it() {
    let mut u = studio();
    for _ in 0..8 { u.step(0.05); }
    assert!(!u.m.zoom);
    let g = u.grid;
    u.arm_zoom_area();
    touch(&mut u, Phase::Down, g.x + 400, g.y + 100);
    touch(&mut u, Phase::Move, g.x + 500, g.y + 200);
    touch(&mut u, Phase::Move, g.x + 600, g.y + 300);
    touch(&mut u, Phase::Up, g.x + 600, g.y + 300);
    assert!(u.m.zoom, "the box turned the zoom on");
    let (tdiv, zdiv) = (u.m.time_per_div(), tscales()[u.m.zscale]);
    assert!(zdiv < tdiv);
}

fn touch(u: &mut Ui, phase: Phase, x: i32, y: i32) {
    u.handle(Ev::Touch { id: 0, phase, x, y });
    for _ in 0..1 { u.step(0.01); }
}
