// SPDX-License-Identifier: GPL-2.0-or-later
//! The functional walk: every control of the bars and of every sheet is pressed through the touch path, and what
//! shows no response is listed. `OOS_WALK=1 cargo test walk -- --ignored --nocapture` prints the list.
#![cfg(test)]
use super::sheet::RowKind;
use super::*;
use crate::gfx::Canvas;

fn fresh(layout: Layout) -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = layout;
    u.relayout();
    for _ in 0..12 { u.step(0.04); }
    u.m.run = RunState::Stop;
    for _ in 0..4 { u.step(0.04); }
    u
}

/// What the screen and the menu say: the picture, the menu and the values of its items.
fn sig(u: &mut Ui) -> (u64, String) {
    use std::hash::{Hash, Hasher};
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    cv.px.hash(&mut h);
    let vals: Vec<String> = if u.menu_open { u.items(u.menu).into_iter().map(|it| u.value(it)).collect() } else { Vec::new() };
    (h.finish(), format!("{:?} {} {:?} {:?} {}", u.menu, u.menu_open, vals, u.toast.as_ref().map(|t| t.0.clone()), u.numpad.is_some()))
}

fn tap(u: &mut Ui, x: i32, y: i32) {
    crate::cmd::run(u, &format!("tap {x} {y}"));
    u.step(0.04);
}

fn mid(r: Rect) -> (i32, i32) { (r.x + r.w / 2, r.y + r.h / 2) }

/// The points a row is pressed at: each segment, minus / value / plus, or its middle.
fn points(u: &Ui, row: &super::sheet::SheetRowGeo) -> Vec<(i32, i32)> {
    match row.kind {
        RowKind::Choice => {
            let n = u.sheet_choice(row.it).0.len().max(1) as i32;
            (0..n).map(|k| (row.ctrl.x + row.ctrl.w * (2 * k + 1) / (2 * n), row.ctrl.y + row.ctrl.h / 2)).collect()
        }
        RowKind::Number => { let (a, b, c) = Ui::value_parts(row.ctrl); vec![mid(a), mid(b), mid(c)] }
        RowKind::Info => Vec::new(),
        _ => vec![mid(row.ctrl)],
    }
}

fn menus(u: &Ui) -> Vec<MenuId> {
    let mut v = vec![MenuId::Display, MenuId::Utility, MenuId::Storage, MenuId::Measure, MenuId::Cursor, MenuId::Horizontal, MenuId::Trigger, MenuId::Acquire, MenuId::Channel(0), MenuId::Channel(1)];
    for (_, items) in u.main_groups() {
        for it in items { if let Item::Open(id) = it { if !v.contains(&id) { v.push(id); } } }
    }
    v
}

#[test]
#[ignore]
fn walk_every_sheet_row() {
    if std::env::var("OOS_WALK").is_err() { return; }
    for layout in [Layout::Minimal, Layout::Studio] {
        let (mut rows, mut dead, mut classic) = (0, Vec::new(), Vec::new());
        let list = menus(&fresh(layout));
        // sub menus are found while walking: a Link that opens one adds it
        let mut queue: Vec<MenuId> = list.clone();
        let mut seen: Vec<MenuId> = Vec::new();
        while let Some(menu) = queue.pop() {
            if seen.contains(&menu) { continue; }
            seen.push(menu);
            let mut u = fresh(layout);
            u.open_menu(menu);
            u.step(0.04);
            if !u.sheet_active() { classic.push(format!("{menu:?}")); continue; }
            let Some(spec) = u.sheet_spec(menu) else { classic.push(format!("{menu:?} (special sheet)")); continue };
            for t in 0..spec.tabs.len() {
                let count = { let mut u = fresh(layout); u.open_menu(menu); u.step(0.04); if let Some(g) = u.sheet_geo() { if let Some(r) = g.tabs.get(t) { let (x, y) = mid(*r); tap(&mut u, x, y); } } u.sheet_geo().map_or(0, |g| g.rows.len() + g.foot.len()) };
                for i in 0..count {
                    let mut any = false;
                    let mut what = String::new();
                    let mut n_points = 1;
                    let mut p = 0;
                    while p < n_points {
                        let mut u = fresh(layout);
                        u.open_menu(menu);
                        u.step(0.04);
                        let g = u.sheet_geo().unwrap();
                        if let Some(r) = g.tabs.get(t) { let (x, y) = mid(*r); tap(&mut u, x, y); }
                        let g = u.sheet_geo().unwrap();
                        let pts = if i < g.rows.len() { what = format!("{:?} {:?}", g.rows[i].it, g.rows[i].kind); points(&u, &g.rows[i]) } else { let f = &g.foot[i - g.rows.len()]; what = format!("{:?} Foot", f.0); vec![mid(f.1)] };
                        n_points = pts.len();
                        if pts.is_empty() { any = true; break; }
                        let before = sig(&mut u);
                        tap(&mut u, pts[p].0, pts[p].1);
                        let after = sig(&mut u);
                        if before != after { any = true; }
                        if u.menu_open && u.menu != menu && u.menu != MenuId::Pick && !seen.contains(&u.menu) { queue.push(u.menu); }
                        p += 1;
                    }
                    rows += 1;
                    if !any { dead.push(format!("{menu:?} / {} / {what}", spec.tabs[t].name)); }
                }
            }
        }
        println!("== {layout:?}: {rows} rows pressed over {} menus; {} without any response", seen.len(), dead.len());
        for d in &dead { println!("   DEAD {d}"); }
        println!("   still classic dialogs: {}", classic.join(", "));
    }
}

#[test]
#[ignore]
fn walk_the_bars() {
    if std::env::var("OOS_WALK").is_err() { return; }
    for layout in [Layout::Minimal, Layout::Studio] {
        let base = fresh(layout);
        let f = base.frame();
        let mut ys = vec![f.top.y + f.top.h / 2, f.bottom.y + f.bottom.h / 2];
        if layout == Layout::Studio { ys.push(f.bottom.y - 22); }
        for y in ys {
            let mut runs: Vec<(i32, i32, String)> = Vec::new();
            for x in (4..base.scr.w).step_by(8) {
                let mut u = fresh(layout);
                let before = sig(&mut u);
                tap(&mut u, x, y);
                let after = sig(&mut u);
                let what = if before == after { "-".to_string() } else if u.menu_open { format!("menu {}", u.menu_title(u.menu)) } else if let Some(t) = &u.toast { format!("toast {:?}", t.0) } else { "change".to_string() };
                match runs.last_mut() { Some(r) if r.2 == what => r.1 = x, _ => runs.push((x, x, what)) }
            }
            println!("== {layout:?} y={y}");
            for (a, b, w) in runs { println!("   x {a:4}..{b:4}  {w}"); }
        }
    }
}

#[test]
#[ignore]
fn walk_list_classic() {
    if std::env::var("OOS_WALK").is_err() { return; }
    let mut u = fresh(Layout::Studio);
    for menu in [MenuId::Remote] {
        u.open_menu(menu);
        let items = u.items(menu);
        println!("## {menu:?} ({} items)", items.len());
        for it in items { println!("   {:?} | {} | {:?} | {}", it, u.item_label(it), u.sheet_row_kind(it), u.value(it)); }
        u.close_menu();
    }
}

/// SIM mode behaves like a live acquisition: Run / Stop, Single, the trigger slope and level act on the simulated
/// record, and its measurements have values. Prints what it sees; asserts the parts that must hold.
#[test]
fn sim_mode_runs_stops_and_triggers_like_live() {
    for layout in [Layout::Studio, Layout::Minimal] {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.layout = layout;
        u.relayout();
        for _ in 0..8 { u.step(0.04); u.wait_sim_generation(); }
        u.select_option(Item::SimPreset, "USB 2.0 HS");
        u.activate(Item::SimPlay);
        u.wait_sim_generation();
        for _ in 0..30 { u.step(0.04); u.wait_sim_generation(); }
        assert!(u.sim_mode() && u.record_mode(), "{layout:?}: SIM plays");
        assert_eq!(u.m.run, RunState::Run, "{layout:?}: running after Play");
        assert_eq!(u.m.status, TrigStatus::Triggered, "{layout:?}: triggered on the simulated signal");
        let slot = u.m.sim_slot;
        assert_eq!(u.m.trig_ref, Some(slot), "{layout:?}: the trigger source is the simulated ref");
        // Run / Stop
        u.key(key::RUNSTOP);
        for _ in 0..4 { u.step(0.04); u.wait_sim_generation(); }
        assert_eq!(u.m.run, RunState::Stop, "{layout:?}: Stop");
        assert!(u.sim_mode(), "{layout:?}: Stop keeps the SIM session");
        u.key(key::RUNSTOP);
        for _ in 0..10 { u.step(0.04); u.wait_sim_generation(); }
        assert_eq!(u.m.run, RunState::Run, "{layout:?}: Run again");
        assert_eq!(u.m.status, TrigStatus::Triggered, "{layout:?}: triggered again");
        // a level outside the signal in Normal mode waits; back inside it triggers
        let level = u.m.trig_level;
        u.m.trig_mode = TrigMode::Normal;
        u.m.trig_level = 50.0;
        for _ in 0..20 { u.step(0.04); u.wait_sim_generation(); }
        assert_eq!(u.m.status, TrigStatus::Wait, "{layout:?}: no trigger at 50 V");
        u.m.trig_level = level;
        for _ in 0..20 { u.step(0.04); u.wait_sim_generation(); }
        assert_eq!(u.m.status, TrigStatus::Triggered, "{layout:?}: triggers at the level again");
        // the slope is a setting of the simulated trigger too
        u.open_menu(MenuId::Trigger);
        u.select_option(Item::TrigSlope, "Falling");
        u.close_menu();
        for _ in 0..20 { u.step(0.04); u.wait_sim_generation(); }
        assert_eq!(u.value(Item::TrigSlope), "Falling");
        assert_eq!(u.m.status, TrigStatus::Triggered, "{layout:?}: triggers on the falling edge");
        // Single takes one record and stops
        u.key(key::SINGLE);
        for _ in 0..30 { u.step(0.04); u.wait_sim_generation(); }
        assert_eq!(u.m.run, RunState::Stop, "{layout:?}: Single stops after its record");
        assert!(u.sim_mode(), "{layout:?}: still the SIM session");
        // measurements of the simulated ref have values
        let rows = u.meas_rows();
        assert!(rows.iter().any(|r| r.src == WaveId::Ref(slot)), "{layout:?}: measurements on the SIM source: {}", rows.len());
        // Live returns, and adding a signal while live keeps the live acquisition
        u.return_to_live();
        for _ in 0..10 { u.step(0.04); u.wait_sim_generation(); }
        assert!(!u.sim_mode() && !u.record_mode() && u.m.ch[0].on, "{layout:?}: Live");
        let before = u.refs.iter().filter(|r| r.is_some()).count();
        u.sim_add_next();
        u.wait_sim_generation();
        for _ in 0..6 { u.step(0.04); u.wait_sim_generation(); }
        assert_eq!(u.refs.iter().filter(|r| r.is_some()).count(), before + 1, "{layout:?}: a SIM signal as one more ref while live");
        assert!(!u.sim_mode() && u.m.ch[0].on && u.m.run == RunState::Run, "{layout:?}: the live acquisition goes on");
    }
}

/// While live, the Reference sheet loads a file from storage into the chosen slot, through its list, and the live
/// acquisition goes on. Minimal and Studio share the flow.
#[test]
fn a_file_loads_into_a_ref_while_live() {
    for layout in [Layout::Studio, Layout::Minimal] {
        let dir = std::env::temp_dir().join(format!("oos-walk-{}-{layout:?}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut csv = String::from("Time (s),Volts\n");
        for i in 0..400 { csv.push_str(&format!("{:.12e},{}\n", i as f64 * 1e-9, if (i / 50) % 2 == 0 { 0.0 } else { 1.0 })); }
        std::fs::write(dir.join("capture.csv"), csv).unwrap();
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.layout = layout;
        u.store_dir = dir.clone();
        u.relayout();
        for _ in 0..8 { u.step(0.04); }
        u.open_menu(MenuId::Ref);
        u.ref_sel = 2;
        u.activate(Item::RefImport);
        assert_eq!(u.menu, MenuId::Pick, "{layout:?}: the files of storage as a list");
        assert_eq!(u.options(Item::RefImport), ["capture.csv"], "{layout:?}");
        u.activate(Item::PickOpt(0));
        for _ in 0..6 { u.step(0.04); }
        let r = u.refs[2].as_ref().unwrap_or_else(|| panic!("{layout:?}: R3 is loaded: {:?}", u.toast));
        assert!(r.on && r.v.len() == 400, "{layout:?}: shown, {} points", r.v.len());
        assert!(!u.record_mode() && u.m.ch[0].on && u.m.run == RunState::Run, "{layout:?}: the live acquisition goes on");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Two fingers are a pinch, never a long press: resting them for longer than the hold time opens no menu, and the
/// pinch that follows changes the time scale. One finger held still opens the context menu.
#[test]
fn two_fingers_never_open_the_hold_menu_and_one_finger_does() {
    for layout in [Layout::Studio, Layout::Minimal] {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.layout = layout;
        u.relayout();
        for _ in 0..8 { u.step(0.04); }
        let g = u.grid;
        let (cx, cy) = (g.x + g.w / 2, g.y + g.h / 2);
        let menu = |u: &Ui| u.workspace_popup_open() || u.studio.ctx.is_some() || u.menu_open;
        // both fingers land and rest
        u.handle(Ev::Touch { id: 0, phase: Phase::Down, x: cx - 60, y: cy });
        for _ in 0..3 { u.step(0.04); }
        u.handle(Ev::Touch { id: 1, phase: Phase::Down, x: cx + 60, y: cy });
        for _ in 0..25 { u.step(0.04); }
        assert!(!menu(&u), "{layout:?}: no menu while two fingers rest");
        // then they spread: a pinch
        let before = u.m.tscale;
        for s in 1..=8 {
            u.handle(Ev::Touch { id: 0, phase: Phase::Move, x: cx - 60 - 20 * s, y: cy });
            u.handle(Ev::Touch { id: 1, phase: Phase::Move, x: cx + 60 + 20 * s, y: cy });
            u.step(0.04);
        }
        u.handle(Ev::Touch { id: 1, phase: Phase::Up, x: cx + 220, y: cy });
        u.handle(Ev::Touch { id: 0, phase: Phase::Up, x: cx - 220, y: cy });
        for _ in 0..3 { u.step(0.04); }
        assert!(!menu(&u), "{layout:?}: no menu after the pinch");
        assert!(u.m.tscale != before || u.m.zoom, "{layout:?}: the pinch zoomed the time axis ({} -> {})", before, u.m.tscale);
        // the second finger arrives late, after the menu of the first finger's hold: the menu goes away
        u.handle(Ev::Touch { id: 0, phase: Phase::Down, x: cx - 60, y: cy });
        for _ in 0..20 { u.step(0.04); }
        assert!(menu(&u), "{layout:?}: one finger held opens the context menu");
        u.handle(Ev::Touch { id: 1, phase: Phase::Down, x: cx + 60, y: cy });
        u.step(0.04);
        assert!(!menu(&u), "{layout:?}: a second finger takes the menu away");
        u.handle(Ev::Touch { id: 1, phase: Phase::Up, x: cx + 60, y: cy });
        u.handle(Ev::Touch { id: 0, phase: Phase::Up, x: cx - 60, y: cy });
        for _ in 0..3 { u.step(0.04); }
        assert!(!menu(&u), "{layout:?}: and the lift is not a tap");
    }
}

#[test]
#[ignore]
fn walk_channel_taps() {
    if std::env::var("OOS_WALK").is_err() { return; }
    for layout in [Layout::Studio, Layout::Minimal] {
        let mut u = fresh(layout);
        u.m.run = RunState::Run;
        let on = |u: &Ui| (0..4).map(|c| if u.m.ch[c].on { '1' } else { '0' }).collect::<String>();
        let y = u.frame().bottom.y + u.frame().bottom.h / 2;
        println!("== {layout:?} start on={} bottom y={y}", on(&u));
        for x in (10..520).step_by(30) {
            let mut v = fresh(layout);
            v.m.ch[1].on = true; v.relayout(); v.step(0.04);
            let b = on(&v);
            tap(&mut v, x, y);
            let menu = if v.menu_open { format!("menu {}", v.menu_title(v.menu)) } else { String::new() };
            println!("   CH1+CH2 on, tap x={x}: on {b} -> {} {menu} popup={}", on(&v), v.workspace_popup_open());
        }
    }
}

/// Studio: the coloured badge of a channel pill switches the channel off, the rest of the pill opens its sheet, and
/// the "+ CH" list switches it on again.
#[test]
fn the_badge_of_a_channel_pill_is_its_switch() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.relayout();
    for _ in 0..8 { u.step(0.04); }
    let b = u.studio_bottom();
    let pill = b.ch[1];
    assert!(pill.w >= 110, "{pill:?}");
    // the body: the sheet, and the channel stays on
    tap(&mut u, pill.right() - 30, pill.y + pill.h / 2);
    assert!(u.menu_open && u.menu == MenuId::Channel(1) && u.m.ch[1].on);
    u.close_menu();
    // the badge: off, without a menu
    tap(&mut u, pill.x + 24, pill.y + pill.h / 2);
    assert!(!u.m.ch[1].on && !u.menu_open, "the badge switches CH2 off");
    assert!(u.m.ch[0].on, "CH1 is not touched");
    // its pill stays as a small one, and a tap on that switches the channel on again without a sheet
    let small = u.studio_bottom().ch[1];
    assert_eq!(small.w, 44, "a small pill while it is off");
    tap(&mut u, small.x + small.w / 2, small.y + small.h / 2);
    assert!(u.m.ch[1].on && !u.menu_open, "the small pill switches CH2 on");
    assert!(u.studio_bottom().ch[1].w >= 110);
}

/// Sliding the waveform with one finger is a pan, not a long press: no menu while the finger moves, however slowly,
/// nor when it rests after it has moved.
#[test]
fn a_one_finger_slide_never_opens_the_hold_menu() {
    for layout in [Layout::Studio, Layout::Minimal] {
        for (name, step_px, pause_first, rest_after) in [("slow slide", 1, 0, 0), ("fast slide", 12, 0, 0), ("short pause, then slide", 4, 8, 0), ("slide, then rest", 6, 0, 30)] {
            let mut u = Ui::new(Source::Demo { t: 0.0 });
            u.m.layout = layout;
            u.relayout();
            for _ in 0..8 { u.step(0.04); }
            let g = u.grid;
            let (x0, y) = (g.x + g.w / 2, g.y + g.h / 2);
            let menu = |u: &Ui| u.workspace_popup_open() || u.studio.ctx.is_some() || u.menu_open;
            let delay = u.m.delay;
            u.handle(Ev::Touch { id: 0, phase: Phase::Down, x: x0, y });
            for _ in 0..pause_first { u.step(0.04); }
            let mut x = x0;
            for _ in 0..40 {
                x += step_px;
                u.handle(Ev::Touch { id: 0, phase: Phase::Move, x, y });
                u.step(0.04);
                assert!(!menu(&u), "{layout:?} {name}: a menu opened {} px into the slide", x - x0);
            }
            for _ in 0..rest_after { u.step(0.04); assert!(!menu(&u), "{layout:?} {name}: a menu opened while resting after the slide"); }
            u.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
            for _ in 0..3 { u.step(0.04); }
            assert!(!menu(&u), "{layout:?} {name}: a menu after the lift");
            if step_px >= 4 { assert!(u.m.delay != delay, "{layout:?} {name}: the slide moved the waveform"); }
        }
    }
}

/// Studio: Overlay traces puts every source on one graticule and Stacked traces gives each its lane again; the
/// plot menu marks the mode in force. The zoom bar style is a row of Horizontal > Zoom.
#[test]
fn studio_overlay_and_stacked_switch_the_lanes() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.relayout();
    for _ in 0..8 { u.step(0.04); }
    assert_eq!(u.wins.len(), 2, "a lane each to begin with");
    assert!(u.trace_stacked());
    u.trace_mode(false);
    for _ in 0..4 { u.step(0.04); }
    assert_eq!(u.wins.len(), 1, "Overlay: one graticule");
    assert!(!u.trace_stacked());
    u.trace_mode(true);
    for _ in 0..4 { u.step(0.04); }
    assert_eq!(u.wins.len(), 2, "Stacked: the lanes are back");
    assert!(u.items(MenuId::Horizontal).contains(&Item::ZoomBar));
    let tab = u.m.studio_zoom_tab;
    u.select_option(Item::ZoomBar, if tab { "Title bar" } else { "Pull-down tab" });
    assert_ne!(u.m.studio_zoom_tab, tab);
    // every measurement has a badge while none is pinned by hand and there is room
    assert!(u.studio.pinned.is_none());
    assert!(u.studio_badges().len() > 3, "{} badges", u.studio_badges().len());
}

#[test]
#[ignore]
fn walk_pictures() {
    let Ok(dir) = std::env::var("OOS_WALK_SHOTS") else { return };
    let shot = |u: &mut Ui, name: &str| {
        for _ in 0..6 { u.step(0.04); }
        let mut cv = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut cv);
        let _ = crate::store::write(&std::path::Path::new(&dir).join(format!("{name}.png")), &crate::store::png_of(&cv));
    };
    let studio = || {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.layout = Layout::Studio;
        u.m.ch[1].on = true;
        u.relayout();
        for _ in 0..12 { u.step(0.04); }
        u
    };
    // the Add menu
    let mut u = studio();
    let add = u.studio_top().add;
    tap(&mut u, add.x + add.w / 2, add.y + add.h / 2);
    shot(&mut u, "1-add-menu");
    // the plot menu, then Overlay
    let mut u = studio();
    let g = u.grid;
    u.context_menu(g.x + 300, g.y + 120);
    shot(&mut u, "2-plot-menu");
    u.trace_mode(false);
    shot(&mut u, "3-overlay");
    // Display > Grid and views: the Layout row
    let mut u = studio();
    u.open_menu(MenuId::Display);
    u.step(0.04);
    let t = u.sheet_geo().unwrap().tabs[1];
    tap(&mut u, t.x + t.w / 2, t.y + t.h / 2);
    shot(&mut u, "4-display-layout");
    // a reference and a math channel beside the channels, six measurements
    let mut u = studio();
    u.activate(Item::RefSave);
    u.m.math_select(0);
    u.m.math = crate::math::MathOp::Add;
    u.relayout();
    shot(&mut u, "5-ref-and-math-pills");
    // Horizontal > Zoom with the zoom bar row
    let mut u = studio();
    u.open_menu(MenuId::Horizontal);
    u.step(0.04);
    let t = u.sheet_geo().unwrap().tabs[1];
    tap(&mut u, t.x + t.w / 2, t.y + t.h / 2);
    shot(&mut u, "6-horizontal-zoom");
}

#[test]
#[ignore]
fn walk_table_pictures() {
    let Ok(dir) = std::env::var("OOS_WALK_SHOTS") else { return };
    let shot = |u: &mut Ui, name: &str| {
        for _ in 0..30 { u.step(0.04); }
        let mut cv = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut cv);
        let _ = crate::store::write(&std::path::Path::new(&dir).join(format!("{name}.png")), &crate::store::png_of(&cv));
    };
    let studio = |view: super::studio::StudioView, detail: bool, stats: bool| {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.layout = Layout::Studio;
        u.m.ch[1].on = true;
        u.m.meas.extend([(Meas::Freq, WaveId::Ch(1)), (Meas::Vpp, WaveId::Ch(1)), (Meas::Vrms, WaveId::Ch(1))]);
        u.relayout();
        for _ in 0..12 { u.step(0.04); }
        u.studio_set_view(view);
        u.m.studio_detail = detail;
        u.studio.panel.stats = stats;
        u.relayout();
        u
    };
    use super::studio::StudioView as V;
    shot(&mut studio(V::Only, true, false), "t1-only");
    shot(&mut studio(V::Only, true, true), "t2-only-stats");
    shot(&mut studio(V::Docked, true, true), "t3-docked-detail");
    shot(&mut studio(V::Docked, false, false), "t4-docked-list");
    // the cards in a panel docked above the waveform, as wide as the screen
    let mut u = studio(V::Docked, false, false);
    u.studio.panel.cards = true;
    u.dock_apply(super::dock::VKind::Measure, super::dock::Drop::Split(super::dock::VKind::Wave, super::dock::Side::Top));
    u.relayout();
    for _ in 0..200 { u.step(0.04); }
    shot(&mut u, "t5-cards-wide");
    let mut u = studio(V::Docked, false, false);
    u.studio.panel.cards = true;
    u.relayout();
    for _ in 0..100 { u.step(0.04); }
    shot(&mut u, "t6-cards-narrow");
    // the detailed table in a panel docked above the waveform
    let mut u = studio(V::Docked, true, true);
    u.dock_apply(super::dock::VKind::Measure, super::dock::Drop::Split(super::dock::VKind::Wave, super::dock::Side::Top));
    u.relayout();
    for _ in 0..100 { u.step(0.04); }
    shot(&mut u, "t7-detail-wide");
}
