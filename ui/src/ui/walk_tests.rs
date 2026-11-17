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

#[test]
#[ignore]
fn walk_lane_pictures() {
    let Ok(dir) = std::env::var("OOS_WALK_SHOTS") else { return };
    let shot = |u: &mut Ui, name: &str| {
        for _ in 0..30 { u.step(0.04); }
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
    // four sources in lanes under a docked measurements panel
    let mut u = studio();
    u.activate(Item::RefSave);
    u.m.math_select(0);
    u.m.math = crate::math::MathOp::Sub;
    u.studio_set_view(super::studio::StudioView::Docked);
    u.m.studio_detail = true;
    u.dock_apply(super::dock::VKind::Measure, super::dock::Drop::Split(super::dock::VKind::Wave, super::dock::Side::Top));
    u.relayout();
    shot(&mut u, "L1-four-lanes-short");
    // the same lanes with the whole body
    let mut u = studio();
    u.activate(Item::RefSave);
    u.m.math_select(0);
    u.m.math = crate::math::MathOp::Sub;
    u.relayout();
    shot(&mut u, "L2-four-lanes");
    // a tap on the Area badge, then on Peak-to-Peak
    let mut u = studio();
    u.m.meas = vec![(Meas::Area, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(1)), (Meas::Period, WaveId::Ch(0)), (Meas::AreaCyc, WaveId::Ch(1))];
    u.relayout();
    for _ in 0..30 { u.step(0.04); }
    let badges = u.studio_badges();
    let b = badges[0].1;
    tap(&mut u, b.x + b.w / 2, b.y + b.h / 2);
    shot(&mut u, "L3-area-mark");
    let b = badges[3].1;
    tap(&mut u, b.x + b.w / 2, b.y + b.h / 2);
    shot(&mut u, "L4-cycle-area-mark");
    let b = badges[1].1;
    tap(&mut u, b.x + b.w / 2, b.y + b.h / 2);
    shot(&mut u, "L5-pkpk-mark");
}

/// A tap on a badge shows its measurement on the waveform (Area shades what the trace encloses), the next tap on
/// it opens its menu, and closing the menu takes the mark away. A tap on another badge moves the mark.
#[test]
fn a_badge_tap_marks_its_measurement_on_the_waveform() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.meas = vec![(Meas::Area, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(0)), (Meas::PEdges, WaveId::Ch(0))];
    u.relayout();
    for _ in 0..30 { u.step(0.04); }
    let badges = u.studio_badges();
    let (area, vpp, edges) = (badges[0], badges[1], badges[2]);
    let plain = sig(&mut u).0;
    tap(&mut u, area.1.x + area.1.w / 2, area.1.y + area.1.h / 2);
    assert_eq!(u.studio.mark, Some(area.0));
    assert!(u.studio.popup.is_none(), "no menu on the first tap");
    assert_ne!(sig(&mut u).0, plain, "the area is drawn");
    tap(&mut u, vpp.1.x + vpp.1.w / 2, vpp.1.y + vpp.1.h / 2);
    assert_eq!(u.studio.mark, Some(vpp.0), "another badge takes the mark");
    tap(&mut u, vpp.1.x + vpp.1.w / 2, vpp.1.y + vpp.1.h / 2);
    assert_eq!(u.studio.popup, Some(vpp.0), "the second tap opens the menu");
    u.studio.popup = None;
    u.step(0.04);
    assert!(u.studio.mark.is_none(), "closing the menu ends the mark");
    // a count has nothing to draw: its badge opens the menu at once
    tap(&mut u, edges.1.x + edges.1.w / 2, edges.1.y + edges.1.h / 2);
    assert_eq!(u.studio.popup, Some(edges.0));
}

/// Overlay through the real path: a finger held on the plot opens its menu, a tap on "Overlay traces" puts every
/// source on one graticule, and "Stacked traces" brings the lanes back. With channels, a reference and a math
/// channel, the measurements docked above, and more than one view.
#[test]
fn overlay_from_the_plot_menu_by_touch() {
    for (name, setup) in [("plain", 0), ("ref and math", 1), ("docked panel", 2), ("three views", 3)] {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.layout = Layout::Studio;
        u.m.ch[1].on = true;
        u.relayout();
        for _ in 0..12 { u.step(0.04); }
        if setup >= 1 {
            u.activate(Item::RefSave);
            u.m.math_select(0);
            u.m.math = crate::math::MathOp::Sub;
            u.relayout();
        }
        if setup >= 2 {
            u.studio_set_view(super::studio::StudioView::Docked);
            u.dock_apply(super::dock::VKind::Measure, super::dock::Drop::Split(super::dock::VKind::Wave, super::dock::Side::Top));
            u.relayout();
        }
        if setup >= 3 { u.views_set(3); }
        for _ in 0..12 { u.step(0.04); }
        u.toast = None;
        let lanes = u.wins.len();
        assert!(lanes >= 2, "{name}: lanes to begin with");
        let pick = |u: &mut Ui, row: super::studioctx::CtxRow| {
            let g = u.wins[0];
            let (x, y) = (g.x + g.w / 2, g.y + g.h / 2);
            u.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
            for _ in 0..20 { u.step(0.04); }
            u.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
            u.step(0.04);
            let rows = u.studio_ctx_rows();
            let r = rows.iter().find(|(k, _, _)| *k == row).unwrap_or_else(|| panic!("the menu has the row: {} rows", rows.len())).2;
            tap(u, r.x + r.w / 2, r.y + r.h / 2);
            for _ in 0..6 { u.step(0.04); }
        };
        pick(&mut u, super::studioctx::CtxRow::Overlay);
        assert_eq!(u.wins.len(), 1, "{name}: Overlay is one graticule");
        assert!(!u.trace_stacked(), "{name}");
        pick(&mut u, super::studioctx::CtxRow::Stacked);
        assert_eq!(u.wins.len(), lanes, "{name}: Stacked brings the lanes back");
    }
}

#[test]
#[ignore]
fn walk_overlay_pictures() {
    let Ok(dir) = std::env::var("OOS_WALK_SHOTS") else { return };
    let shot = |u: &mut Ui, name: &str| {
        for _ in 0..30 { u.step(0.04); u.wait_sim_generation(); }
        u.toast = None;
        let mut cv = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut cv);
        let _ = crate::store::write(&std::path::Path::new(&dir).join(format!("{name}.png")), &crate::store::png_of(&cv));
    };
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.relayout();
    for _ in 0..12 { u.step(0.04); }
    u.activate(Item::RefSave);
    u.m.math_select(0);
    u.m.math = crate::math::MathOp::Sub;
    u.relayout();
    u.trace_mode(false);
    shot(&mut u, "O1-overlay-ch-ref-math");
    // SIM playing two signals, overlaid
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.relayout();
    for _ in 0..12 { u.step(0.04); }
    u.select_option(Item::SimPreset, "USB 2.0 HS");
    u.sim_add_next();
    u.wait_sim_generation();
    u.select_option(Item::SimPreset, "PCIe 1.0");
    u.sim_add_next();
    u.wait_sim_generation();
    shot(&mut u, "O2-live-two-refs-stacked");
    u.trace_mode(false);
    shot(&mut u, "O3-live-two-refs-overlay");
    u.activate(Item::SimPlay);
    shot(&mut u, "O4-sim-overlay");
}

#[test]
fn overlay_stays_with_a_reference_and_a_math_channel() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.relayout();
    for _ in 0..12 { u.step(0.04); }
    u.activate(Item::RefSave);
    u.m.math_select(0);
    u.m.math = crate::math::MathOp::Sub;
    u.relayout();
    u.trace_mode(false);
    assert!(!u.trace_stacked());
    for i in 0..40 {
        u.step(0.04);
        assert!(!u.trace_stacked(), "frame {i}: overlay flag {} workspace.stacked {}", u.studio.overlay, u.workspace_stacked_flag());
        assert_eq!(u.wins.len(), 1, "frame {i}: one graticule");
        let mut cv = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut cv);
        assert_eq!(u.wins.len(), 1, "frame {i}: one graticule after the picture (overlay {} workspace.stacked {} win_layout {:?} view {:?})", u.studio.overlay, u.workspace_stacked_flag(), u.m.win_layout, u.m.view);
        assert!(!u.trace_stacked(), "frame {i}: after the picture");
    }
    // and with a wait for the SIM worker in between, as the picture tests do
    u.wait_sim_generation();
    u.toast = None;
    u.step(0.04);
    assert_eq!(u.wins.len(), 1);
}

/// A finger held on the plot, then a tap on a row of its menu.
fn pick_ctx(u: &mut Ui, row: super::studioctx::CtxRow) {
    let g = u.wins[0];
    let (x, y) = (g.x + g.w / 3, g.y + g.h / 2);
    u.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
    for _ in 0..20 { u.step(0.04); }
    u.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
    u.step(0.04);
    let rows = u.studio_ctx_rows();
    let r = rows.iter().find(|(k, _, _)| *k == row).unwrap_or_else(|| panic!("the menu has the row: {} rows", rows.len())).2;
    tap(u, r.x + r.w / 2, r.y + r.h / 2);
    for _ in 0..6 { u.step(0.04); }
}

/// Fit all lanes from the plot menu: every shown source gets a scale that holds its signal and is centred in its
/// lane; the time base and the trigger source stay. Pictures with `OOS_WALK_SHOTS`.
#[test]
fn fit_all_lanes_scales_every_source_to_its_lane() {
    let shot = |u: &mut Ui, name: &str| {
        let Ok(dir) = std::env::var("OOS_WALK_SHOTS") else { return };
        for _ in 0..20 { u.step(0.04); }
        u.toast = None;
        let mut cv = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut cv);
        let _ = crate::store::write(&std::path::Path::new(&dir).join(format!("{name}.png")), &crate::store::png_of(&cv));
    };
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.relayout();
    for _ in 0..12 { u.step(0.04); u.render(&mut Canvas::new(1024, 600)); }
    u.activate(Item::RefSave);
    u.m.math_select(0);
    u.m.math = crate::math::MathOp::Sub;
    // badly set: CH1 far too coarse and off centre, CH2 clipped, the reference tiny
    u.m.ch[0].vscale = VSCALES.len() - 2;
    u.m.ch[0].offset = 20.0;
    u.m.ch[1].vscale = 2;
    u.refs[0].as_mut().unwrap().vpd = 10.0;
    u.relayout();
    for _ in 0..12 { u.step(0.04); }
    shot(&mut u, "F1-before-fit");
    let (tscale, trig) = (u.m.tscale, u.m.trig_src);
    pick_ctx(&mut u, super::studioctx::CtxRow::FitLanes);
    for _ in 0..12 { u.step(0.04); }
    shot(&mut u, "F2-after-fit");
    assert_eq!((u.m.tscale, u.m.trig_src), (tscale, trig), "the time base and the trigger stay");
    let rec = u.last[0].as_ref().unwrap().clone();
    for c in 0..2 {
        let (lo, hi) = rec.ch[c].iter().fold((f32::MAX, f32::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
        let ch = &u.m.ch[c];
        let divs = (hi - lo) / ch.volts_per_div();
        assert!((2.0..=6.01).contains(&divs), "CH{}: {divs} divisions of 8", c + 1);
        assert!(((hi + lo) / 2.0 + ch.offset).abs() <= ch.volts_per_div() * 0.6, "CH{} is centred", c + 1);
    }
    let r = u.refs[0].as_ref().unwrap();
    let (lo, hi) = r.v.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
    assert!((2.0..=6.01).contains(&((hi - lo) / r.vpd)), "R1: {} divisions", (hi - lo) / r.vpd);
    // Overlay is kept with the setup and comes back after a restart
    pick_ctx(&mut u, super::studioctx::CtxRow::Overlay);
    assert!(u.m.studio_overlay && u.wins.len() == 1);
    shot(&mut u, "F3-overlay-after-fit");
    let text = crate::store::setup_text(&u.m);
    assert!(text.contains("studio_traces = overlay"), "the setup has the trace mode");
}
