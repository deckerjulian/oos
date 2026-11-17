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
