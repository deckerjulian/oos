//! Batch 33: independent views. A view is a workspace of its own for everything that is presentation and analysis (the
//! measurement list and its pins, the panels, the cursors, the zoom, the lanes shown, the analysis set-ups); the scope
//! (channels, time base, trigger, acquisition) is one for all. OOS_SHOTS=<dir> writes pictures (b33-*).
use super::studio::{MeasKey, StudioView};
use super::*;

const LAYOUTS: [Layout; 3] = [Layout::Minimal, Layout::Default, Layout::Studio];

fn frame(u: &mut Ui) {
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    u.step(0.02);
}

fn shot(u: &mut Ui, name: &str) {
    for _ in 0..4 { frame(u); }
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("b33-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

/// A scope with CH1 and CH2 on, CH3 and CH4 off, and no reference.
fn fresh(layout: Layout) -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = layout;
    u.m.ch[1].on = true;
    u.m.ch[2].on = false;
    u.m.ch[3].on = false;
    for r in u.refs.iter_mut() { *r = None; }
    for _ in 0..4 { u.step(0.05); }
    u
}

fn a_reference() -> RefWave {
    RefWave { src: 0, color: 0, label: "R1".into(), t0: 0.0, dt: 1e-9, v: vec![0.0f32; 128].into(), vpd: 0.5, offset: 0.0, on: true }
}

fn one(k: Meas, c: usize) -> (Meas, WaveId) { (k, WaveId::Ch(c)) }

/// A catalogue measurement (not a `Meas` row) on CH1 and CH2.
fn a_cat() -> (u16, WaveId, WaveId, WaveId, WaveId) { (90, WaveId::Ch(0), WaveId::Ch(1), WaveId::Ch(2), WaveId::Ch(3)) }

#[test]
fn removing_a_measurement_in_view_two_leaves_view_one_alone_in_every_layout() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.m.meas = vec![one(Meas::Freq, 0), one(Meas::Vpp, 0)];
        u.m.meas2 = vec![(0, WaveId::Ch(0), WaveId::Ch(1))];
        u.m.cat = vec![a_cat()];
        u.views_set(2);
        u.view_go(1);
        // the new view is a copy; take the table apart in it, one of each kind
        assert_eq!(u.measurement_count(), 4, "{l:?}");
        u.remove_measurement(0);
        u.remove_measurement(u.m.meas.len());
        u.remove_measurement(u.measurement_count() - 1);
        assert_eq!(u.measurement_count(), 1, "{l:?}");
        assert_eq!(u.m.meas, vec![one(Meas::Vpp, 0)], "{l:?}");
        u.view_go(0);
        assert_eq!(u.m.meas, vec![one(Meas::Freq, 0), one(Meas::Vpp, 0)], "{l:?}: view 1 keeps its list");
        assert_eq!(u.m.meas2.len(), 1, "{l:?}");
        assert_eq!(u.m.cat, vec![a_cat()], "{l:?}: and its catalogue rows");
        // and the other way
        u.remove_measurement(0);
        u.view_go(1);
        assert_eq!(u.m.meas, vec![one(Meas::Vpp, 0)], "{l:?}: view 2 is as it was left");
    }
}

#[test]
fn measurements_are_added_pinned_and_set_per_view() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.m.meas = vec![one(Meas::Freq, 0), one(Meas::Vpp, 0)];
        u.views_set(2);
        u.view_go(1);
        u.m.meas.push(one(Meas::Vrms, 1));
        u.m.meas_show = [false, true, true];
        u.m.meas_pop_current = true;
        u.m.meas_full_record = !u.m.meas_full_record;
        let gate = u.m.meas_full_record;
        u.studio.pinned = Some(vec![MeasKey::One(Meas::Vrms, WaveId::Ch(1))]);
        u.studio.floated = vec![MeasKey::One(Meas::Vpp, WaveId::Ch(0))];
        u.studio.panel.stats_mode = 2;
        u.studio.panel.pinned_only = true;
        u.studio.panel.query = "v r".into();
        u.view_go(0);
        assert_eq!(u.m.meas.len(), 2, "{l:?}: nothing added in view 1");
        assert_eq!(u.m.meas_show, [true; 3], "{l:?}: statistics switches are per view");
        assert!(!u.m.meas_pop_current, "{l:?}");
        assert_ne!(u.m.meas_full_record, gate, "{l:?}: the gate too");
        assert!(u.studio.floated.is_empty() && u.studio.pinned.as_ref().is_none_or(|p| !p.contains(&MeasKey::One(Meas::Vrms, WaveId::Ch(1)))), "{l:?}: no pins of view 2 in view 1");
        assert!(u.studio.panel.stats_mode == 0 && !u.studio.panel.pinned_only && u.studio.panel.query.is_empty(), "{l:?}");
        // pin in view 1: view 2 does not follow
        u.studio_set_pin(MeasKey::One(Meas::Freq, WaveId::Ch(0)), true);
        u.view_go(1);
        assert_eq!(u.studio.pinned, Some(vec![MeasKey::One(Meas::Vrms, WaveId::Ch(1))]), "{l:?}");
        assert_eq!(u.studio.floated, vec![MeasKey::One(Meas::Vpp, WaveId::Ch(0))], "{l:?}");
        assert!(u.studio.panel.stats_mode == 2 && u.studio.panel.pinned_only && u.studio.panel.query == "v r", "{l:?}");
        assert_eq!(u.m.meas.len(), 3, "{l:?}");
        assert_eq!(u.m.meas_show, [false, true, true], "{l:?}");
        assert_eq!(u.m.meas_full_record, gate, "{l:?}");
    }
}

#[test]
fn cursors_and_zoom_are_per_view() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.m.cursor = CursorMode::Time;
        u.m.cur_a = 0.3;
        u.m.cur_b = 0.6;
        u.m.zoom = true;
        u.m.zdelay = 1e-6;
        u.studio.sz.lane = Some(super::workspace::Trace::Channel(1));
        u.studio.sz.ref_h.insert(0, (1e-6, 0.0));
        u.zoom_vfactor = 4.0;
        u.views_set(2);
        u.view_go(1);
        // a copy ...
        assert!(u.m.zoom && u.m.cursor == CursorMode::Time, "{l:?}");
        assert_eq!(u.studio.sz.lane, Some(super::workspace::Trace::Channel(1)), "{l:?}");
        // ... that is changed
        u.m.cursor = CursorMode::Volt;
        u.m.cur_a = 0.1;
        u.m.cur_b = 0.9;
        u.m.cur_b_source = Some(WaveId::Ch(1));
        u.m.zoom = false;
        u.m.zdelay = -2e-6;
        u.studio.sz.lane = None;
        u.studio.sz.ref_h.clear();
        u.zoom_vfactor = 1.0;
        u.view_go(0);
        assert_eq!(u.m.cursor, CursorMode::Time, "{l:?}");
        assert!((u.m.cur_a - 0.3).abs() < 1e-6 && (u.m.cur_b - 0.6).abs() < 1e-6, "{l:?}");
        assert_eq!(u.m.cur_b_source, None, "{l:?}: the source of the second cursor too");
        assert!(u.m.zoom && (u.m.zdelay - 1e-6).abs() < 1e-12, "{l:?}");
        assert_eq!(u.studio.sz.lane, Some(super::workspace::Trace::Channel(1)), "{l:?}");
        assert_eq!(u.studio.sz.ref_h.get(&0), Some(&(1e-6, 0.0)), "{l:?}: the horizontal scale of a reference");
        assert_eq!(u.zoom_vfactor, 4.0, "{l:?}: Y zoom");
        u.view_go(1);
        assert!(u.m.cursor == CursorMode::Volt && !u.m.zoom && u.studio.sz.lane.is_none() && u.studio.sz.ref_h.is_empty() && u.zoom_vfactor == 1.0, "{l:?}");
        assert_eq!(u.m.cur_b_source, Some(WaveId::Ch(1)), "{l:?}");
    }
}

#[test]
fn panels_are_per_view_and_closing_the_table_in_one_leaves_the_other() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.m.meas = vec![one(Meas::Freq, 0)];
        u.studio_set_view(StudioView::Docked);
        u.m.pane = 1;
        u.m.studio_detail = false; // view 1 shows the List: the table is the default now, so this is the view that differs
        u.m.hist = Hist::Off;
        u.views_set(2);
        u.view_go(1);
        assert_eq!(u.studio.view, StudioView::Docked, "{l:?}: a copy");
        // the owner's case: remove the measurement table in view 2
        u.studio_set_view(StudioView::Waveform);
        u.m.pane = 0;
        u.m.hist = Hist::Vertical;
        u.studio.compact = true;
        u.studio.panel.collapsed = true;
        u.m.studio_ov = 150;
        u.m.studio_detail = true; // view 1 shows the List (the table is the default now), this view the table
        u.m.eye_pll_bw = 1e5;
        u.view_go(0);
        assert_eq!(u.studio.view, StudioView::Docked, "{l:?}: the table is still there in view 1");
        assert_eq!((u.m.pane, u.m.hist), (1, Hist::Off), "{l:?}");
        assert!(!u.studio.compact && !u.studio.panel.collapsed && u.m.studio_ov == 0 && !u.m.studio_detail && u.m.eye_pll_bw == 0.0, "{l:?}");
        assert_eq!(u.m.studio_view, "docked", "{l:?}");
        u.view_go(1);
        assert_eq!(u.studio.view, StudioView::Waveform, "{l:?}");
        assert!(u.studio.compact && u.studio.panel.collapsed && u.m.studio_ov == 150 && u.m.studio_detail, "{l:?}");
        assert_eq!((u.m.pane, u.m.hist), (0, Hist::Vertical), "{l:?}");
        assert_eq!(u.m.eye_pll_bw, 1e5, "{l:?}");
    }
}

#[test]
fn the_docking_arrangement_and_floating_panels_are_per_view() {
    let mut u = fresh(Layout::Studio);
    u.m.meas = vec![one(Meas::Freq, 0)];
    u.studio_set_view(StudioView::Floating);
    frame(&mut u);
    u.views_set(2);
    u.view_go(1);
    // floating panel with a place of its own in view 2 (its text holds `;`: the slot escapes it)
    u.studio.dock.float_rect[super::dock::VKind::Hist.idx()] = Some(Rect::new(30, 40, 300, 220));
    u.studio.dock.float_on[super::dock::VKind::Hist.idx()] = true;
    let text = u.dock_text();
    assert!(text.contains(';'), "{text}");
    u.view_go(0);
    assert!(u.studio.dock.float_rect[super::dock::VKind::Hist.idx()].is_none() && !u.studio.dock.float_on[super::dock::VKind::Hist.idx()]);
    u.view_go(1);
    assert_eq!(u.studio.dock.float_rect[super::dock::VKind::Hist.idx()], Some(Rect::new(30, 40, 300, 220)));
    assert!(u.studio.dock.float_on[super::dock::VKind::Hist.idx()]);
    // the floating measurement card of the other layouts, too
    u.workspace.measure_float = Some((100, 120));
    u.workspace.eye_float = Some(Rect::new(10, 20, 200, 150));
    u.view_go(0);
    assert!(u.workspace.measure_float.is_none() && u.workspace.eye_float.is_none());
    u.view_go(1);
    assert_eq!(u.workspace.measure_float, Some((100, 120)));
    assert_eq!(u.workspace.eye_float, Some(Rect::new(10, 20, 200, 150)));
}

#[test]
fn the_references_and_the_bus_shown_are_per_view_but_their_samples_are_shared() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.refs[0] = Some(a_reference());
        u.refs[1] = Some(RefWave { label: "R2".into(), ..a_reference() });
        u.views_set(2);
        u.view_go(1);
        u.refs[1].as_mut().unwrap().on = false;
        u.m.bus.on = true;
        u.view_go(0);
        assert!(u.refs[0].as_ref().unwrap().on && u.refs[1].as_ref().unwrap().on, "{l:?}");
        assert!(!u.m.bus.on, "{l:?}");
        u.view_go(1);
        assert!(u.refs[0].as_ref().unwrap().on && !u.refs[1].as_ref().unwrap().on, "{l:?}");
        assert!(u.m.bus.on, "{l:?}");
        // the stored data is the same, a hidden lane keeps its samples
        assert_eq!(u.refs[1].as_ref().unwrap().v.len(), 128, "{l:?}");
    }
}

#[test]
fn the_scope_is_shared_by_every_view() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.views_set(3);
        u.view_go(1);
        let (vs, tdiv, lvl) = (u.m.ch[0].vscale, u.m.tscale, u.m.trig_level);
        u.m.ch[0].vscale = (vs + 2).min(VSCALES.len() - 1);
        u.m.tscale = (tdiv + 2).min(tscales().len() - 1);
        u.m.trig_level = lvl + 0.25;
        u.m.ch[2].on = true;
        u.m.acq = AcqMode::Average;
        u.view_go(2);
        assert_eq!(u.m.ch[0].vscale, (vs + 2).min(VSCALES.len() - 1), "{l:?}");
        assert_eq!(u.m.tscale, (tdiv + 2).min(tscales().len() - 1), "{l:?}");
        assert_eq!(u.m.trig_level, lvl + 0.25, "{l:?}");
        assert!(u.m.ch[2].on && u.m.acq == AcqMode::Average, "{l:?}");
        u.view_go(0);
        assert!(u.m.ch[2].on && u.m.acq == AcqMode::Average && u.m.trig_level == lvl + 0.25, "{l:?}");
        // a change made in view 3 shows in view 1 as well
        u.view_go(2);
        u.m.ch[1].on = false;
        u.m.delay += 1e-6;
        let d = u.m.delay;
        u.view_go(0);
        assert!(!u.m.ch[1].on && u.m.delay == d, "{l:?}");
    }
}

/// Three views that differ in everything a view owns.
fn three_views(l: Layout) -> Ui {
    let mut u = fresh(l);
    u.m.meas = vec![one(Meas::Freq, 0)];
    u.views_set(3);
    u.studio_set_view(StudioView::Docked);
    u.studio_set_pin(MeasKey::One(Meas::Freq, WaveId::Ch(0)), true);
    u.view_go(1);
    u.m.meas = vec![one(Meas::Vpp, 1), one(Meas::Vrms, 1)];
    u.m.cat = vec![a_cat()];
    u.m.cursor = CursorMode::Time;
    u.m.zoom = true;
    u.studio_set_view(StudioView::Floating);
    u.studio.pinned = Some(vec![MeasKey::One(Meas::Vrms, WaveId::Ch(1))]);
    u.studio.panel.stats_mode = 1;
    u.view_go(2);
    u.m.meas.clear();
    u.m.cat.clear();
    u.m.pane = 1;
    u.studio_set_view(StudioView::Waveform);
    u.studio.search = Some(super::addrail::SearchSpec { src: WaveId::Ch(1), level: 0.5, slope: 1 });
    u
}

#[test]
fn a_setup_with_three_views_round_trips_through_save_recall_and_the_scpi_block() {
    for l in LAYOUTS {
        let mut u = three_views(l);
        assert_eq!((u.m.views, u.m.view_cur), (3, 2), "{l:?}");
        let text = u.setup_text_full();
        assert!(text.contains("views = 3 3") && text.contains("view1 =") && text.contains("view2 ="), "{l:?}");
        let mut v = fresh(l);
        v.apply_setup_text(&text).unwrap();
        assert_eq!((v.m.views, v.m.view_cur), (3, 2), "{l:?}");
        assert!(v.m.meas.is_empty() && v.m.pane == 1 && v.studio.view == StudioView::Waveform, "{l:?}");
        assert_eq!(v.studio.search.map(|s| (s.src, s.slope)), Some((WaveId::Ch(1), 1)), "{l:?}");
        v.view_go(0);
        assert_eq!(v.m.meas, vec![one(Meas::Freq, 0)], "{l:?}");
        assert_eq!(v.studio.view, StudioView::Docked, "{l:?}");
        assert_eq!(v.studio_pinned(), vec![MeasKey::One(Meas::Freq, WaveId::Ch(0))], "{l:?}");
        assert_eq!(v.m.pane, 0, "{l:?}");
        v.view_go(1);
        assert_eq!(v.m.meas, vec![one(Meas::Vpp, 1), one(Meas::Vrms, 1)], "{l:?}");
        assert_eq!(v.m.cat, vec![a_cat()], "{l:?}");
        assert!(v.m.cursor == CursorMode::Time && v.m.zoom && v.studio.view == StudioView::Floating && v.studio.panel.stats_mode == 1, "{l:?}");
        assert_eq!(v.studio.pinned, Some(vec![MeasKey::One(Meas::Vrms, WaveId::Ch(1))]), "{l:?}");
        assert!(v.studio.search.is_none(), "{l:?}");
        // the text written again is the one read
        v.view_go(2);
        let again = v.setup_text_full();
        let mut w = fresh(l);
        w.apply_setup_text(&again).unwrap();
        w.view_go(1);
        assert_eq!(w.m.meas.len(), 2, "{l:?}");
        // the block of :SYSTem:SETup? does the same
        let cv = Canvas::new(u.scr.w, u.scr.h);
        let mut scpi = crate::scpi::Scpi::open(0);
        let block = scpi.exec_line(&mut u, &cv, ":SYST:SETup?");
        assert!(!block.is_empty(), "{l:?}");
        let mut x = fresh(l);
        let mut s2 = crate::scpi::Scpi::open(0);
        let mut cmd = b":SYSTem:SETup ".to_vec();
        cmd.extend_from_slice(&block);
        s2.exec_line(&mut x, &cv, &String::from_utf8_lossy(&cmd));
        assert_eq!((x.m.views, x.m.view_cur), (3, 2), "{l:?}");
        x.view_go(1);
        assert_eq!(x.m.meas.len(), 2, "{l:?}: the block carries every view");
    }
}

#[test]
fn an_old_setup_loads_with_every_view_a_copy_of_the_current_one() {
    for l in LAYOUTS {
        // views counted but no slots (a setup from before the views kept anything)
        let mut u = fresh(l);
        u.apply_setup_text("views = 3 1\nmeas = Freq:1 Vpp:1\nzoom = true\n").unwrap();
        assert_eq!((u.m.views, u.m.view_cur), (3, 0), "{l:?}");
        u.view_go(1);
        assert_eq!(u.m.meas, vec![one(Meas::Freq, 0), one(Meas::Vpp, 0)], "{l:?}: a copy, not an empty plot");
        assert!(u.m.zoom, "{l:?}");
        u.m.meas.pop();
        u.view_go(2);
        assert_eq!(u.m.meas.len(), 2, "{l:?}");
        // a setup with no views at all
        let mut u = fresh(l);
        u.apply_setup_text("meas = Freq:1\n").unwrap();
        assert_eq!(u.m.views, 1, "{l:?}");
        assert_eq!(u.m.meas.len(), 1, "{l:?}");
        // slots of Batch 30 (no state of the Ui): the keys they have are applied, the panels stay as they are
        let mut u = fresh(l);
        u.studio_set_view(StudioView::Docked);
        u.apply_setup_text("views = 2 1\nview2 = meas=Vpp:2; xy=false; full=false\nmeas = Freq:1\n").unwrap();
        let before = u.studio.view;
        u.view_go(1);
        assert_eq!(u.m.meas, vec![one(Meas::Vpp, 1)], "{l:?}");
        assert_eq!(u.studio.view, before, "{l:?}");
        u.view_go(0);
        assert_eq!(u.m.meas, vec![one(Meas::Freq, 0)], "{l:?}");
    }
}

#[test]
fn the_slot_escapes_what_it_separates_with() {
    let raw = "tree;H@1:2:3:4;H!;100%3B";
    assert_eq!(super::views::unesc(&super::views::esc(raw)), raw);
    assert!(!super::views::esc(raw).contains(';'));
    assert!(crate::ui::views::slot_checked("meas=Freq:1; studio_dock=x%3By; studio_state=pin=; lanes=R1 B").is_ok());
    assert!(crate::ui::views::slot_checked("tdiv=3").is_err());
    assert!(crate::ui::views::slot_checked("ch1.on=true").is_err(), "the scope is not part of a view");
}


#[test]
fn views_follow_the_scpi_commands() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        let cv = Canvas::new(u.scr.w, u.scr.h);
        let mut s = crate::scpi::Scpi::open(0);
        let q = |s: &mut crate::scpi::Scpi, u: &mut Ui, c: &str| String::from_utf8(s.exec_line(u, &cv, c)).unwrap();
        q(&mut s, &mut u, ":MEAS:CLE;:MEAS:ITEM VPP,CHAN1");
        q(&mut s, &mut u, ":DISP:VIEW:COUN 2");
        q(&mut s, &mut u, ":DISP:VIEW 2");
        assert_eq!(q(&mut s, &mut u, ":DISP:VIEW?"), "2", "{l:?}");
        // a copy; the commands now work on the list of view 2
        q(&mut s, &mut u, ":MEAS:CLE;:MEAS:ITEM VRMS,CHAN2");
        let two = q(&mut s, &mut u, ":MEAS:STR?");
        assert!(two.contains("VRMS,CHAN2") && !two.contains("VPP"), "{l:?}: {two}");
        q(&mut s, &mut u, ":DISP:VIEW 1");
        let one_ = q(&mut s, &mut u, ":MEAS:STR?");
        assert!(one_.contains("VPP,CHAN1") && !one_.contains("VRMS"), "{l:?}: {one_}");
        q(&mut s, &mut u, ":MEAS:CLE");
        assert_eq!(q(&mut s, &mut u, ":MEAS:STR?"), "", "{l:?}");
        q(&mut s, &mut u, ":DISP:VIEW 2");
        assert!(q(&mut s, &mut u, ":MEAS:STR?").contains("VRMS,CHAN2"), "{l:?}: clearing in view 1 leaves view 2");
    }
}

#[test]
fn pictures_of_two_views_with_different_measurements() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.m.meas = vec![one(Meas::Freq, 0), one(Meas::Vpp, 0), one(Meas::Vrms, 0)];
        u.studio_set_view(StudioView::Docked);
        u.views_set(2);
        u.view_go(1);
        u.m.meas = vec![one(Meas::Vmax, 1), one(Meas::Vmin, 1)];
        u.studio_set_view(StudioView::Docked);
        let n = format!("{l:?}").to_lowercase();
        shot(&mut u, &format!("view2-{n}"));
        u.view_go(0);
        shot(&mut u, &format!("view1-{n}"));
        assert_eq!(u.m.meas.len(), 3, "{l:?}");
    }
}

fn define_math(u: &mut Ui) {
    u.m.math = MathOp::Add;
    u.m.math_a = 0;
    u.m.math_b = 1;
}

fn math_lane(u: &Ui) -> bool { u.trace_sources().contains(&super::workspace::Trace::Math(u.m.math_sel)) }

#[test]
fn a_math_channel_is_shown_per_view_and_defined_for_all() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.views_set(3);
        u.view_go(1);
        // view 2 defines a math channel: it shows there and in no other view
        define_math(&mut u);
        let k = u.m.math_sel;
        assert!(u.m.math_shown(k) && math_lane(&u), "{l:?}");
        u.view_go(0);
        assert!(!u.m.math_shown(k) && !math_lane(&u), "{l:?}: not in view 1");
        assert_eq!(u.m.math, MathOp::Add, "{l:?}: the definition is shared");
        u.view_go(2);
        assert!(!math_lane(&u), "{l:?}: nor in view 3");
        u.view_go(1);
        assert!(math_lane(&u), "{l:?}: still in view 2");
        // measuring a hidden math channel still works
        u.view_go(0);
        assert!(matches!(u.math_record(k), Ok(Some(_))), "{l:?}: a hidden channel still computes");
        // show it in view 1 too, then hide it in view 2 only
        u.math_set_shown(k, true);
        assert!(math_lane(&u), "{l:?}");
        u.view_go(1);
        u.math_turn_off(k);
        assert!(!math_lane(&u) && u.m.math == MathOp::Add, "{l:?}: hidden in view 2, still defined for view 1");
        u.view_go(0);
        assert!(math_lane(&u), "{l:?}: view 1 keeps it");
        // hidden in the last view that showed it: it is off altogether
        u.math_turn_off(k);
        assert_eq!(u.m.math, MathOp::Off, "{l:?}");
        // by SCPI: the display switch acts on the view shown
        let cv = Canvas::new(u.scr.w, u.scr.h);
        let mut s = crate::scpi::Scpi::open(0);
        s.exec_line(&mut u, &cv, ":DISP:VIEW 2");
        s.exec_line(&mut u, &cv, ":MATH1:DISP ON");
        assert_eq!(s.exec_line(&mut u, &cv, ":MATH1:DISP?"), b"1", "{l:?}");
        s.exec_line(&mut u, &cv, ":DISP:VIEW 1");
        assert_eq!(s.exec_line(&mut u, &cv, ":MATH1:DISP?"), b"0", "{l:?}: new channel is in view 2 only");
        s.exec_line(&mut u, &cv, ":MATH1:DISP ON");
        assert_eq!(s.exec_line(&mut u, &cv, ":MATH1:DISP?"), b"1", "{l:?}");
    }
}

#[test]
fn the_generator_chip_and_the_bode_page_are_per_view_but_the_output_is_shared() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.views_set(3);
        u.view_go(1);
        u.m.view = AView::Bode;
        u.m.gen_hidden = true;
        u.m.afg_on = true;
        u.view_go(0);
        assert!(u.m.view == AView::Scope && !u.m.gen_hidden, "{l:?}: view 1 has no Bode page and shows GEN");
        assert!(u.m.afg_on, "{l:?}: the output is the scope's, not the view's");
        u.view_go(2);
        assert!(u.m.view == AView::Scope && !u.m.gen_hidden, "{l:?}");
        u.view_go(1);
        assert!(u.m.view == AView::Bode && u.m.gen_hidden, "{l:?}");
        let cv = Canvas::new(u.scr.w, u.scr.h);
        let mut s = crate::scpi::Scpi::open(0);
        assert_eq!(s.exec_line(&mut u, &cv, ":DISP:GEN?"), b"0", "{l:?}");
        s.exec_line(&mut u, &cv, ":DISP:GEN ON");
        assert!(!u.m.gen_hidden, "{l:?}");
        s.exec_line(&mut u, &cv, ":DISP:GEN OFF");
        u.view_go(0);
        assert_eq!(s.exec_line(&mut u, &cv, ":DISP:GEN?"), b"1", "{l:?}");
    }
}

#[test]
fn math_gen_and_analysis_pages_round_trip_with_the_setup() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.views_set(3);
        u.view_go(1);
        define_math(&mut u);
        u.m.view = AView::Spectrum;
        u.view_go(2);
        u.m.view = AView::Bode;
        u.m.gen_hidden = true;
        let text = u.setup_text_full();
        let mut v = fresh(l);
        v.apply_setup_text(&text).unwrap();
        assert_eq!((v.m.views, v.m.view_cur), (3, 2), "{l:?}");
        assert!(v.m.view == AView::Bode && v.m.gen_hidden && !math_lane(&v), "{l:?}");
        v.view_go(1);
        assert!(v.m.view == AView::Spectrum && !v.m.gen_hidden && math_lane(&v), "{l:?}");
        v.view_go(0);
        assert!(v.m.view == AView::Scope && !math_lane(&v) && v.m.math == MathOp::Add, "{l:?}");
    }
}
