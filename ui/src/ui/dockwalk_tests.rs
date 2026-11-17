// SPDX-License-Identifier: GPL-2.0-or-later
//! The docking walk (batch 35): Studio views in tabs, the full states and the way back out of them.
//!
//! A scripted part (the owner's two bugs, the tabs, every exit of a full state) and a random walk over the docking
//! actions with the invariants checked after every step: a deterministic subset runs with the suite, the long walk is
//! `#[ignore]` (`cargo test dockwalk_long -- --ignored`). A failing walk is shortened by removing steps that do not
//! matter and reported as the minimal sequence of actions. OOS_SHOTS=<dir> writes pictures (b35-*).
use super::dock::{Extra, Node, VKind, KINDS, STRIP_H};
use super::sheet::PanelKind;
use super::studio::StudioView;
use super::*;

// ------------------------------------------------------------------------------------------------ the harness

fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    for _ in 0..10 { u.step(0.05); }
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    u.studio_set_view(StudioView::Docked);
    settle(&mut u);
    u
}

fn settle(u: &mut Ui) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    for _ in 0..3 { u.step(0.05); }
}

fn shot(u: &mut Ui, name: &str) {
    settle(u);
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("b35-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn touch(u: &mut Ui, phase: Phase, x: i32, y: i32) {
    u.handle(Ev::Touch { id: 0, phase, x, y });
    u.step(0.01);
}

fn tap(u: &mut Ui, x: i32, y: i32) {
    touch(u, Phase::Down, x, y);
    touch(u, Phase::Up, x, y);
    u.step(0.05);
}

fn drag(u: &mut Ui, a: (i32, i32), b: (i32, i32)) {
    touch(u, Phase::Down, a.0, a.1);
    for k in 1..=8 { touch(u, Phase::Move, a.0 + (b.0 - a.0) * k / 8, a.1 + (b.1 - a.1) * k / 8); }
    touch(u, Phase::Up, b.0, b.1);
    u.step(0.05);
}

fn centre(r: Rect) -> (i32, i32) { (r.x + r.w / 2, r.y + r.h / 2) }

fn screen(u: &Ui) -> Rect { Rect::new(0, 0, u.scr.w, u.scr.h) }

fn inside(r: Rect, outer: Rect) -> bool { r.x >= outer.x && r.y >= outer.y && r.right() <= outer.right() && r.bottom() <= outer.bottom() }

fn overlaps(a: Rect, b: Rect) -> bool { a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom() }

/// Where the views are on the screen: docked (the one in front of a group) and floating, the rectangle of each.
fn visible(u: &Ui) -> Vec<(VKind, Rect, bool)> {
    let mut v: Vec<(VKind, Rect, bool)> = u.dock_layout().leaves.iter().map(|(k, r)| (*k, *r, false)).collect();
    for k in KINDS { if let Some(r) = u.dock_float_rect(k) { v.push((k, r, true)); } }
    v
}

fn open(u: &mut Ui, k: VKind) {
    match k {
        VKind::Wave => {}
        VKind::Measure => u.studio_set_view(StudioView::Docked),
        VKind::Eye => u.studio_apply_panel(PanelKind::Eye),
        VKind::Pane => u.studio_apply_panel(PanelKind::Spectrum),
        VKind::Hist => u.studio_apply_panel(PanelKind::Histogram),
        VKind::Xy => u.select_option(Item::XyMode, "XY"),
        VKind::Table => u.table = Some(tables::TableKind::Search),
        VKind::Sim | VKind::View => {}
    }
    settle(u);
}

/// The views of the walk.
const WALK: [VKind; 7] = [VKind::Wave, VKind::Measure, VKind::Eye, VKind::Pane, VKind::Hist, VKind::Xy, VKind::Table];

fn name_of(k: VKind) -> &'static str {
    match k { VKind::Wave => "Waveform", VKind::Measure => "Measurements", VKind::Eye => "Eye", VKind::Pane => "Spectrum", VKind::Hist => "Histogram", VKind::Xy => "XY", VKind::Table => "Search", _ => "?" }
}

// ------------------------------------------------------------------------------------------------ the invariants

/// Everything that must hold after every step.
fn check(u: &Ui) -> Result<(), String> {
    let lay = u.dock_layout();
    let region = u.dock_region();
    let scr = screen(u);
    let only = u.studio.view == StudioView::Only;
    // every view at most once
    let mut seen: Vec<VKind> = Vec::new();
    for (k, r) in lay.leaves.iter() {
        if seen.contains(k) { return Err(format!("{k:?} is in the dock twice")); }
        seen.push(*k);
        if !u.dock_shown(*k) { return Err(format!("an empty host frame: {k:?} is docked but off")); }
        if u.dock_floating(*k) { return Err(format!("{k:?} is docked and floating")); }
        if r.w < 40 || r.h < STRIP_H + 8 { return Err(format!("{k:?} is only {r:?}")); }
        if !inside(*r, region) { return Err(format!("{k:?} {r:?} is outside the area {region:?}")); }
    }
    for k in KINDS {
        if let Some(r) = u.dock_float_rect(k) {
            if seen.contains(&k) { return Err(format!("{k:?} floats and is docked: a copy over the host")); }
            seen.push(k);
            if !inside(r, region) { return Err(format!("floating {k:?} {r:?} is off the area {region:?}")); }
        }
    }
    for (i, (a, ra)) in lay.leaves.iter().enumerate() {
        for (b, rb) in lay.leaves.iter().skip(i + 1) {
            if overlaps(*ra, *rb) { return Err(format!("{a:?} and {b:?} overlap")); }
        }
    }
    for g in &lay.tabs {
        if g.kinds.len() < 2 { return Err("a group of one".into()); }
        for (i, k) in g.kinds.iter().enumerate() {
            if g.kinds[..i].contains(k) { return Err(format!("{k:?} twice in a group")); }
            if !u.dock_shown(*k) { return Err(format!("a closed view {k:?} is a tab")); }
            if u.dock_floating(*k) { return Err(format!("{k:?} is a tab and floats")); }
            if i != g.active && seen.contains(k) { return Err(format!("{k:?} is a tab behind another and drawn too")); }
        }
    }
    // every view that is on is somewhere
    if !only {
        for k in KINDS {
            if !u.dock_shown(k) { continue; }
            let in_group = lay.tabs.iter().any(|g| g.kinds.contains(&k));
            if !seen.contains(&k) && !in_group { return Err(format!("{k:?} is on and nowhere")); }
        }
    }
    if lay.leaves.is_empty() { return Err("nothing is docked".into()); }
    // the waveform can be reached
    if only {
        let m = u.dock_leaf(VKind::Measure).ok_or("measurements only without a view")?;
        if u.dock_extra_kind(VKind::Measure) != Some(Extra::RestoreOnly) { return Err("measurements only has no restore".into()); }
        let e = u.dock_extra_rect(VKind::Measure, m).ok_or("no restore rectangle")?;
        if !inside(e, scr) || e.w < 24 || e.h < 24 { return Err(format!("restore {e:?} is off the screen or small")); }
    } else {
        if !u.dock_shown(VKind::Wave) { return Err("the waveform is gone".into()); }
        let direct = seen.contains(&VKind::Wave);
        let tab = lay.tabs.iter().find(|g| g.kinds.contains(&VKind::Wave)).is_some_and(|g| {
            let front = g.kinds[g.active];
            u.dock_tab_rects(front, g.rect).iter().any(|(k, t)| *k == VKind::Wave && inside(*t, scr)) || u.dock_tab_more(front, g.rect).is_some_and(|m| inside(m, scr))
        });
        if !direct && !tab { return Err("the waveform is neither on the screen nor a tab".into()); }
    }
    // a full state shows its way out
    if !only && u.dock_tabs_root() {
        let (k, r) = lay.leaves[0];
        if u.dock_extra_kind(k) != Some(Extra::RestoreTabs) { return Err(format!("tabs fill the area but {k:?} has {:?}", u.dock_extra_kind(k))); }
        let e = u.dock_extra_rect(k, r).ok_or("no restore rectangle")?;
        if !inside(e, scr) || e.w < 24 || e.h < 24 { return Err(format!("restore {e:?} is off the screen or small")); }
    }
    if !only && u.eye_open() && u.m.eye_place == EyePlace::Full {
        // (the waveform's place can be a tab behind another: the group's own exit is there)
        if let Some(r) = u.dock_rect(VKind::Wave) {
            if !u.dock_tabs_root() && u.dock_extra_kind(VKind::Wave) != Some(Extra::RestoreEye) { return Err("eye on the whole plot has no restore".into()); }
            if u.dock_extra_rect(VKind::Wave, r).is_none() { return Err("eye on the whole plot: no restore rectangle".into()); }
        }
    }
    if u.dock_full_state() && !u.dock_tabs_root() && !only && !(u.eye_open() && u.m.eye_place == EyePlace::Full) { return Err("a full state without a known exit".into()); }
    // the title bars
    for (k, r, floating) in visible(u) {
        let s = Ui::dock_strip(r);
        if s.h < 24 { return Err(format!("{k:?} strip is {} px", s.h)); }
        let c = Ui::dock_close_rect(r);
        if !inside(c, scr) || c.w < 24 || c.h < 24 { return Err(format!("close of {k:?} {c:?}")); }
        if let Some(e) = u.dock_extra_rect(k, r) {
            if !inside(e, scr) || e.w < 24 || e.h < 24 || overlaps(e, c) { return Err(format!("extra button of {k:?} {e:?}")); }
        }
        if floating { continue; }
        let tabs = u.dock_tab_rects(k, r);
        for (i, (v, t)) in tabs.iter().enumerate() {
            if t.w < 24 || t.h < 24 || !inside(*t, s) { return Err(format!("tab {v:?} of {k:?} {t:?} in strip {s:?}")); }
            if tabs.iter().skip(i + 1).any(|(_, o)| overlaps(*t, *o)) { return Err(format!("tabs of {k:?} overlap")); }
            if let Some(e) = u.dock_extra_rect(k, r) { if overlaps(*t, e) { return Err(format!("tab {v:?} under the extra button")); } }
            if overlaps(*t, c) { return Err(format!("tab {v:?} under the X")); }
        }
        if let Some(g) = u.dock_tab_group(k) {
            if g.kinds[g.active] == k && !tabs.iter().any(|(v, _)| *v == k) { return Err(format!("the tab of {k:?}, in front, is not in its strip")); }
        }
    }
    Ok(())
}

/// The arrangement as a text, to compare a view that was left and came back.
fn state_text(u: &Ui) -> String {
    let region = u.dock_region();
    let floating: Vec<VKind> = KINDS.into_iter().filter(|k| u.dock_shown(*k) && u.dock_floating(*k)).collect();
    // the tree the dock keeps (a view that is off or on in the meantime, like the search table, is no part of it)
    let _ = region;
    let _ = floating;
    format!("{}|{:?}", u.dock_text(), u.studio.view)
}

// ------------------------------------------------------------------------------------------------ the steps

#[derive(Clone, Copy, Debug)]
enum Act {
    DockEdge,
    DragFloat,
    Maximise,
    Restore,
    TabSwitch,
    TabDragOut,
    TabJoin,
    Close,
    Reopen,
    SwitchViews,
    Divider,
    LargerText,
    EyePlace,
    Escape,
    DoubleTap,
    ContextMenu,
}

const ACTS: [Act; 16] = [
    Act::DockEdge, Act::DragFloat, Act::Maximise, Act::Restore, Act::TabSwitch, Act::TabDragOut, Act::TabJoin, Act::Close,
    Act::Reopen, Act::SwitchViews, Act::Divider, Act::LargerText, Act::EyePlace, Act::Escape, Act::DoubleTap, Act::ContextMenu,
];

/// An action with two raw numbers; what they pick is decided by the state the action meets, so a shortened sequence
/// still means something.
#[derive(Clone, Copy, Debug)]
struct Step {
    act: Act,
    a: u32,
    b: u32,
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    fn step(&mut self) -> Step {
        // maximising, restoring and tabs come often: they are what the walk is for
        let weights: [u32; 16] = [4, 4, 4, 3, 4, 4, 3, 3, 4, 2, 2, 1, 1, 1, 1, 1];
        let total: u32 = weights.iter().sum();
        let mut n = self.next() % total;
        let mut i = 0;
        while n >= weights[i] { n -= weights[i]; i += 1; }
        Step { act: ACTS[i], a: self.next(), b: self.next() }
    }
}

/// The side of a rectangle a drop aims at.
fn aim(r: Rect, side: u32) -> (i32, i32) {
    match side % 4 {
        0 => (r.x + 20, r.y + r.h / 2),
        1 => (r.right() - 20, r.y + r.h / 2),
        2 => (r.x + r.w / 2, r.y + STRIP_H + 14),
        _ => (r.x + r.w / 2, r.bottom() - 14),
    }
}

/// Which view a walk has on the other numbered view (Views 1 and 2).
struct Walk {
    u: Ui,
    snap: [Option<String>; 2],
    log: Vec<String>,
}

impl Walk {
    fn new(layout: usize) -> Walk {
        let mut u = studio();
        start_layout(&mut u, layout);
        u.views_set(2);
        settle(&mut u);
        Walk { u, snap: [None, None], log: Vec::new() }
    }

    fn quiet(&mut self) {
        let u = &mut self.u;
        if u.menu_open { u.close_menu(); }
        u.studio.ctx = None;
        u.studio.add_menu = false;
        u.studio.view_menu = false;
        u.studio.popup = None;
        u.workspace.popup = None;
        u.dialog = None;
        u.touch.down = false;
    }

    fn apply(&mut self, s: Step) -> Result<(), String> {
        self.quiet();
        let desc = self.run(s);
        self.log.push(format!("{:?} {}", s.act, desc));
        settle(&mut self.u);
        check(&self.u).map_err(|e| format!("{e} (after {:?}: {desc})", s.act))
    }

    fn run(&mut self, s: Step) -> String {
        let u = &mut self.u;
        let vis = visible(u);
        let pick = |n: usize, x: u32| (x as usize) % n.max(1);
        match s.act {
            Act::DockEdge => {
                let docked: Vec<(VKind, Rect, bool)> = vis.iter().copied().filter(|v| !v.2).collect();
                if vis.is_empty() || docked.is_empty() { return "none".into(); }
                let (k, r, floating) = vis[pick(vis.len(), s.a)];
                let (t, tr, _) = docked[pick(docked.len(), s.b)];
                if t == k { return "self".into(); }
                let g = (r.x + 24, r.y + STRIP_H / 2);
                let to = aim(tr, s.a >> 8);
                drag(u, g, to);
                format!("{k:?}{} to side {} of {t:?}", if floating { " (floating)" } else { "" }, (s.a >> 8) % 4)
            }
            Act::DragFloat => {
                if vis.is_empty() { return "none".into(); }
                let (k, r, floating) = vis[pick(vis.len(), s.a)];
                let reg = u.dock_region();
                let to = (reg.x + 40 + (s.b % 600) as i32, reg.y + 60 + ((s.b >> 10) % 240) as i32);
                if floating { drag(u, (r.x + 50, r.y + STRIP_H / 2), to); } else { drag(u, (r.x + 24, r.y + STRIP_H / 2), (r.x + r.w * 31 / 100, r.y + r.h * 60 / 100)); }
                format!("{k:?}{}", if floating { " by its title" } else { " to the ring" })
            }
            Act::Maximise => {
                let with: Vec<(VKind, Rect, bool)> = vis.iter().copied().filter(|(k, r, _)| u.dock_extra_kind(*k).is_some_and(|e| matches!(e, Extra::Maximise | Extra::Expand)) && u.dock_extra_rect(*k, *r).is_some()).collect();
                if with.is_empty() { return "none".into(); }
                let (k, r, _) = with[pick(with.len(), s.a)];
                let e = u.dock_extra_rect(k, r).unwrap();
                tap(u, e.x + e.w / 2, r.y + STRIP_H / 2);
                format!("{k:?} button")
            }
            Act::Restore => {
                let with: Vec<(VKind, Rect, bool)> = vis.iter().copied().filter(|(k, _, _)| matches!(u.dock_extra_kind(*k), Some(Extra::RestoreOnly | Extra::RestoreTabs | Extra::RestoreEye))).collect();
                if with.is_empty() { return "none".into(); }
                let (k, r, _) = with[pick(with.len(), s.a)];
                let e = u.dock_extra_rect(k, r).unwrap();
                tap(u, e.x + e.w / 2, r.y + STRIP_H / 2);
                format!("{k:?} restore button")
            }
            Act::TabSwitch => {
                let lay = u.dock_layout();
                if lay.tabs.is_empty() { return "none".into(); }
                let g = lay.tabs[pick(lay.tabs.len(), s.a)].clone();
                let front = g.kinds[g.active];
                let rect = lay.leaves.iter().find(|(k, _)| *k == front).map(|(_, r)| *r).unwrap();
                let tabs = u.dock_tab_rects(front, rect);
                if tabs.is_empty() { return "no tabs".into(); }
                let (k, t) = tabs[pick(tabs.len(), s.b)];
                let c = centre(t);
                tap(u, c.0, c.1);
                format!("tab {k:?}")
            }
            Act::TabDragOut => {
                let lay = u.dock_layout();
                if lay.tabs.is_empty() { return "none".into(); }
                let g = lay.tabs[pick(lay.tabs.len(), s.a)].clone();
                let front = g.kinds[g.active];
                let rect = lay.leaves.iter().find(|(k, _)| *k == front).map(|(_, r)| *r).unwrap();
                let tabs = u.dock_tab_rects(front, rect);
                if tabs.is_empty() { return "no tabs".into(); }
                let (k, t) = tabs[pick(tabs.len(), s.b)];
                let to = match (s.b >> 8) % 5 { 4 => (rect.x + rect.w * 31 / 100, rect.y + rect.h * 60 / 100), n => aim(rect, n) };
                drag(u, centre(t), to);
                format!("tab {k:?} out to {}", (s.b >> 8) % 5)
            }
            Act::TabJoin => {
                let docked: Vec<(VKind, Rect, bool)> = vis.iter().copied().filter(|v| !v.2).collect();
                if vis.len() < 2 || docked.is_empty() { return "none".into(); }
                let (k, r, _) = vis[pick(vis.len(), s.a)];
                let (t, tr, _) = docked[pick(docked.len(), s.b)];
                if t == k { return "self".into(); }
                drag(u, (r.x + 24, r.y + STRIP_H / 2), (tr.x + tr.w * if s.b & 0x100 != 0 { 22 } else { 78 } / 100, tr.y + STRIP_H / 2));
                format!("{k:?} onto the title bar of {t:?}")
            }
            Act::Close => {
                if vis.is_empty() { return "none".into(); }
                let (k, r, _) = vis[pick(vis.len(), s.a)];
                let c = Ui::dock_close_rect(r);
                tap(u, c.x + c.w / 2, r.y + STRIP_H / 2);
                format!("{k:?} X")
            }
            Act::Reopen => {
                let off: Vec<VKind> = WALK.into_iter().filter(|k| !u.dock_shown(*k)).collect();
                if off.is_empty() { return "all on".into(); }
                let k = off[pick(off.len(), s.a)];
                open(u, k);
                format!("{k:?}")
            }
            Act::SwitchViews => {
                let cur = u.m.view_cur;
                self.snap[cur] = Some(state_text(u));
                let u = &mut self.u;
                u.view_go(1 - cur);
                settle(u);
                if let Some(before) = self.snap[1 - cur].clone() {
                    let now = state_text(u);
                    if before != now { return format!("MISMATCH view {} came back as {now} not {before}", 2 - cur); }
                }
                format!("to view {}", 2 - cur)
            }
            Act::Divider => {
                let lay = u.dock_layout();
                if lay.dividers.is_empty() { return "none".into(); }
                let d = &lay.dividers[pick(lay.dividers.len(), s.a)];
                let c = centre(d.line);
                let dx = (s.b % 241) as i32 - 120;
                drag(u, c, if d.dir == dock::Dir::H { (c.0 + dx, c.1) } else { (c.0, c.1 + dx / 2) });
                format!("divider by {dx}")
            }
            Act::LargerText => {
                u.m.text_size = if u.m.text_size == crate::model::TextSize::Larger { crate::model::TextSize::Normal } else { crate::model::TextSize::Larger };
                u.dock_changed();
                format!("{:?}", u.m.text_size)
            }
            Act::EyePlace => {
                if !u.eye_open() { return "no eye".into(); }
                let to = ["Full", "Side", "Under"][pick(3, s.a)];
                u.select_option(Item::EyePlace, to);
                to.into()
            }
            Act::Escape => { u.dismiss_context(); "Esc".into() }
            Act::DoubleTap => {
                if vis.is_empty() { return "none".into(); }
                let (k, r, floating) = vis[pick(vis.len(), s.a)];
                let x = if floating { r.x + 50 } else { r.x + 24 };
                tap(u, x, r.y + STRIP_H / 2);
                tap(u, x, r.y + STRIP_H / 2);
                format!("{k:?} strip")
            }
            Act::ContextMenu => {
                let reg = u.dock_region();
                u.studio_ctx_open(reg.x + reg.w / 2, reg.y + reg.h / 2);
                let rows = u.studio_ctx_rows();
                let Some((_, _, r)) = rows.into_iter().find(|(row, _, _)| matches!(row, super::studioctx::CtxRow::Layout)) else { return "no row".into() };
                let c = centre(r);
                tap(u, c.0, c.1);
                "layout row".into()
            }
        }
    }
}

/// The starting layouts of the walk.
fn start_layout(u: &mut Ui, n: usize) {
    match n {
        0 => {}
        1 => u.studio_set_view(StudioView::Waveform),
        2 => open(u, VKind::Eye),
        3 => { open(u, VKind::Eye); open(u, VKind::Pane); }
        4 => { open(u, VKind::Hist); u.studio_set_view(StudioView::Floating); }
        5 => { u.studio_set_view(StudioView::Waveform); open(u, VKind::Table); open(u, VKind::Xy); }
        6 => {
            open(u, VKind::Eye);
            settle(u);
            // the eye under the waveform, the measurements at the left
            let (w, e) = (u.dock_leaf(VKind::Wave).unwrap(), u.dock_leaf(VKind::Eye).unwrap());
            drag(u, (e.x + 24, e.y + STRIP_H / 2), aim(w, 3));
            let (w, m) = (u.dock_leaf(VKind::Wave).unwrap(), u.dock_leaf(VKind::Measure).unwrap());
            drag(u, (m.x + 24, m.y + STRIP_H / 2), aim(w, 0));
        }
        _ => { for k in [VKind::Eye, VKind::Pane, VKind::Hist, VKind::Table] { open(u, k); } }
    }
    settle(u);
}

const LAYOUTS: usize = 8;

/// Runs the steps from a layout; the first failure.
fn replay(layout: usize, steps: &[Step]) -> Result<(), String> {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut w = Walk::new(layout);
        check(&w.u).map_err(|e| format!("start: {e}"))?;
        for (i, s) in steps.iter().enumerate() {
            w.apply(*s).map_err(|e| format!("step {}: {e}", i + 1))?;
            if w.log.last().is_some_and(|l| l.contains("MISMATCH")) { return Err(format!("step {}: {}", i + 1, w.log.last().unwrap())); }
        }
        Ok(())
    }));
    match r { Ok(v) => v, Err(_) => Err("panic".into()) }
}

/// The steps without the ones that do not matter (the failure is still there).
fn shorten(layout: usize, mut steps: Vec<Step>) -> Vec<Step> {
    let mut i = steps.len();
    while i > 0 {
        i -= 1;
        let mut t = steps.clone();
        t.remove(i);
        if replay(layout, &t).is_err() { steps = t; }
    }
    steps
}

fn walk(layout: usize, seed: u64, n: usize) -> usize {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let steps: Vec<Step> = (0..n).map(|_| rng.step()).collect();
    let mut w = Walk::new(layout);
    check(&w.u).unwrap_or_else(|e| panic!("layout {layout}, start: {e}"));
    let mut cv = Canvas::new(W, H);
    for (i, s) in steps.iter().enumerate() {
        let r = w.apply(*s);
        let r = r.and_then(|_| if w.log.last().is_some_and(|l| l.contains("MISMATCH")) { Err(w.log.last().unwrap().clone()) } else { Ok(()) });
        if let Err(e) = r {
            let tail: Vec<&String> = w.log.iter().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect();
            eprintln!("DOCKWALK FAIL layout {layout} seed {seed} step {}: {e}
  last actions: {tail:?}", i + 1);
            let min = shorten(layout, steps[..=i].to_vec());
            let again = replay(layout, &min).err().unwrap_or_default();
            let mut m = Walk::new(layout);
            let mut text = Vec::new();
            for s in &min { let _ = m.apply(*s); }
            text.extend(m.log.iter().cloned());
            panic!("layout {layout}, seed {seed}, step {}: {e}\nminimal sequence ({} steps, fails with: {again}):\n  {}", i + 1, min.len(), text.join("\n  "));
        }
        // a picture now and then: drawing the arrangement must not fail either
        if i % 20 == 0 { w.u.render(&mut cv); }
    }
    eprintln!("dockwalk: layout {layout} seed {seed}: {} steps clean", steps.len());
    steps.len()
}

// ------------------------------------------------------------------------------------------------ the walks

/// A short walk from every layout with the suite.
#[test]
fn dockwalk_fast_subset() {
    let mut total = 0;
    for layout in 0..LAYOUTS { total += walk(layout, 7 + layout as u64, 60); }
    assert!(total >= 480);
}

/// Thousands of steps from every layout: `cargo test dockwalk_long -- --ignored`.
#[test]
#[ignore]
fn dockwalk_long() {
    let mut total = 0;
    for seed in 0..3u64 {
        for layout in 0..LAYOUTS { total += walk(layout, 1000 + seed * 31 + layout as u64, 250); }
    }
    eprintln!("dockwalk_long: {total} steps");
    assert!(total >= 2000);
}

// ------------------------------------------------------------------------------------------------ the scripted cases

fn extra_tap(u: &mut Ui, k: VKind) {
    let r = u.dock_rect(k).unwrap_or_else(|| panic!("{k:?} on the screen"));
    let e = u.dock_extra_rect(k, r).unwrap_or_else(|| panic!("{k:?} has a button left of its X"));
    tap(u, e.x + e.w / 2, r.y + STRIP_H / 2);
}

fn tab_tap(u: &mut Ui, front: VKind, k: VKind) {
    let r = u.dock_rect(front).expect("the group");
    let t = u.dock_tab_rects(front, r).into_iter().find(|(v, _)| *v == k).unwrap_or_else(|| panic!("a tab for {k:?}")).1;
    let c = centre(t);
    tap(u, c.0, c.1);
}

fn eye_beside_waveform() -> Ui {
    let mut u = studio();
    u.studio_set_view(StudioView::Waveform);
    u.m.ch[2].on = true;
    u.m.ch[3].on = true;
    open(&mut u, VKind::Eye);
    u
}

#[test]
fn maximising_the_eye_puts_the_waveform_in_a_tab_not_in_a_float() {
    let mut u = eye_beside_waveform();
    assert!(u.dock_leaf(VKind::Wave).is_some() && u.dock_leaf(VKind::Eye).is_some());
    extra_tap(&mut u, VKind::Eye);
    // the eye fills the area, the waveform is a tab in its title bar, and nothing floats
    assert!(u.dock_tabs_root(), "one group fills the area");
    assert!(u.dock_leaf(VKind::Eye).is_some() && u.dock_leaf(VKind::Wave).is_none());
    assert!(u.dock_float_rect(VKind::Wave).is_none(), "no small window over the eye");
    let r = u.dock_rect(VKind::Eye).unwrap();
    assert!(r.w >= u.dock_region().w - 2 && r.h >= u.dock_region().h - 2, "{r:?}");
    let tabs = u.dock_tab_rects(VKind::Eye, r);
    assert_eq!(tabs.iter().map(|(k, _)| *k).collect::<Vec<_>>(), vec![VKind::Wave, VKind::Eye]);
    assert_eq!(u.dock_extra_kind(VKind::Eye), Some(Extra::RestoreTabs));
    check(&u).unwrap();
    shot(&mut u, "tabs-eye-maximised");
    // a tap on the Waveform tab brings the waveform to the front
    tab_tap(&mut u, VKind::Eye, VKind::Wave);
    assert!(u.dock_leaf(VKind::Wave).is_some() && u.dock_leaf(VKind::Eye).is_none());
    assert!(u.dock_tabbed_out(VKind::Eye) && !u.dock_tabbed_out(VKind::Wave));
    check(&u).unwrap();
    shot(&mut u, "tab-switched-to-waveform");
    // the restore button puts the views back where they were
    shot(&mut u, "restore-button");
    extra_tap(&mut u, VKind::Wave);
    assert!(!u.dock_tabs_root() && u.dock_leaf(VKind::Wave).is_some() && u.dock_leaf(VKind::Eye).is_some());
    let (w, e) = (u.dock_leaf(VKind::Wave).unwrap(), u.dock_leaf(VKind::Eye).unwrap());
    assert!(w.right() <= e.x || e.right() <= w.x || w.bottom() <= e.y || e.bottom() <= w.y, "beside each other again");
    check(&u).unwrap();
}

#[test]
fn a_group_of_three_tabs_and_a_tab_dragged_out() {
    let mut u = eye_beside_waveform();
    open(&mut u, VKind::Measure);
    assert_eq!(KINDS.into_iter().filter(|k| u.dock_shown(*k)).count(), 3);
    extra_tap(&mut u, VKind::Eye);
    let front = u.dock_layout().leaves[0].0;
    let tabs = u.dock_tab_rects(front, u.dock_rect(front).unwrap());
    assert_eq!(tabs.len(), 3, "{tabs:?}");
    assert!(tabs.iter().all(|(_, t)| t.w >= 24 && t.h >= 24));
    check(&u).unwrap();
    shot(&mut u, "three-tabs");
    // drag the Measurements tab out to the right edge: it docks beside the group
    tab_tap(&mut u, front, VKind::Measure);
    let front = u.dock_layout().leaves[0].0;
    let r = u.dock_rect(front).unwrap();
    let t = u.dock_tab_rects(front, r).into_iter().find(|(v, _)| *v == VKind::Measure).unwrap().1;
    drag(&mut u, centre(t), (r.right() - 20, r.y + r.h / 2));
    check(&u).unwrap();
    assert!(u.dock_leaf(VKind::Measure).is_some() && u.dock_layout().tabs.len() == 1, "the group of two stays");
    assert!(!u.dock_tabs_root(), "no longer one group alone");
    // a tab dragged to the middle floats it
    let g = u.dock_layout().tabs[0].clone();
    let front = g.kinds[g.active];
    let r = u.dock_rect(front).unwrap();
    let other = g.kinds.iter().copied().find(|k| *k != front).unwrap();
    let t = u.dock_tab_rects(front, r).into_iter().find(|(v, _)| *v == other).unwrap().1;
    drag(&mut u, centre(t), (r.x + r.w * 31 / 100, r.y + r.h * 60 / 100));
    check(&u).unwrap();
    assert!(u.dock_float_rect(other).is_some(), "{other:?} floats");
    assert!(u.dock_layout().tabs.is_empty(), "a group of one is its view");
}

#[test]
fn a_view_dropped_on_a_title_bar_joins_its_tabs() {
    let mut u = eye_beside_waveform();
    let (w, e) = (u.dock_leaf(VKind::Wave).unwrap(), u.dock_leaf(VKind::Eye).unwrap());
    drag(&mut u, (e.x + 24, e.y + STRIP_H / 2), (w.x + w.w * 22 / 100, w.y + STRIP_H / 2));
    assert_eq!(u.dock_layout().tabs.len(), 1, "{:?}", u.dock_layout());
    let g = u.dock_layout().tabs[0].clone();
    assert_eq!(g.kinds, vec![VKind::Wave, VKind::Eye]);
    assert_eq!(g.kinds[g.active], VKind::Eye, "the view that came is in front");
    check(&u).unwrap();
    // the setup keeps the group and comes back with it
    let text = u.dock_text();
    assert!(text.starts_with("G1(WE)"), "{text}");
    u.dock_restore(&text);
    assert_eq!(u.studio.dock.tree, Node::parse("G1(WE)"));
}

/// The state of the owner's second bug: the eye on the whole plot while the waveform floats over an empty frame.
#[test]
fn no_empty_host_frame_and_no_floating_copy_of_the_waveform() {
    let mut u = eye_beside_waveform();
    // the waveform floats, the eye fills the area
    let w = u.dock_leaf(VKind::Wave).unwrap();
    drag(&mut u, (w.x + 24, w.y + STRIP_H / 2), (w.x + w.w * 31 / 100, w.y + w.h * 60 / 100));
    assert!(u.dock_floating(VKind::Wave) && u.dock_leaf(VKind::Eye).is_some());
    check(&u).unwrap();
    // the eye goes to the whole plot: nothing is left to dock but the waveform's place
    u.select_option(Item::EyePlace, "Full");
    settle(&mut u);
    check(&u).unwrap();
    assert!(!u.dock_floating(VKind::Wave), "the waveform is docked again");
    assert!(u.dock_leaf(VKind::Wave).is_some(), "one host, no copy");
    assert_eq!(u.dock_extra_kind(VKind::Wave), Some(Extra::RestoreEye));
    // the eye is closed with the waveform floating: it goes back too
    u.select_option(Item::EyePlace, "Side");
    settle(&mut u);
    let w = u.dock_leaf(VKind::Wave).unwrap();
    drag(&mut u, (w.x + 24, w.y + STRIP_H / 2), (w.x + w.w * 31 / 100, w.y + w.h * 60 / 100));
    let e = Ui::dock_close_rect(u.dock_leaf(VKind::Eye).unwrap());
    tap(&mut u, e.x + e.w / 2, e.y + STRIP_H / 2);
    settle(&mut u);
    check(&u).unwrap();
    assert!(u.dock_leaf(VKind::Wave).is_some() && u.dock_float_rect(VKind::Wave).is_none());
}

#[test]
fn every_full_state_has_an_exit_on_the_button_escape_double_tap_and_the_menu() {
    // tabs: the button
    let mut u = eye_beside_waveform();
    extra_tap(&mut u, VKind::Eye);
    assert!(u.dock_full_state());
    extra_tap(&mut u, VKind::Eye);
    assert!(!u.dock_full_state());
    // Escape
    extra_tap(&mut u, VKind::Eye);
    u.dismiss_context();
    settle(&mut u);
    assert!(!u.dock_full_state(), "Esc");
    // a double tap on the title bar, to the full state and back
    let r = u.dock_rect(VKind::Eye).unwrap();
    tap(&mut u, r.x + 24, r.y + 13);
    tap(&mut u, r.x + 24, r.y + 13);
    assert!(u.dock_tabs_root(), "a double tap maximises");
    let r = u.dock_rect(VKind::Eye).unwrap();
    tap(&mut u, r.x + 24, r.y + 13);
    tap(&mut u, r.x + 24, r.y + 13);
    assert!(!u.dock_full_state(), "a double tap restores");
    // the long-press menu
    extra_tap(&mut u, VKind::Eye);
    let reg = u.dock_region();
    u.studio_ctx_open(reg.x + 200, reg.y + 200);
    let row = u.studio_ctx_rows().into_iter().find(|(row, _, _)| matches!(row, super::studioctx::CtxRow::Layout)).expect("the layout row");
    assert_eq!(row.1, "Restore layout");
    let c = centre(row.2);
    tap(&mut u, c.0, c.1);
    assert!(!u.dock_full_state(), "the menu");
    // measurements only
    open(&mut u, VKind::Measure);
    extra_tap(&mut u, VKind::Measure);
    assert_eq!(u.studio.view, StudioView::Only);
    check(&u).unwrap();
    u.dismiss_context();
    assert_eq!(u.studio.view, StudioView::Docked, "Esc leaves measurements only");
    extra_tap(&mut u, VKind::Measure);
    let r = u.dock_rect(VKind::Measure).unwrap();
    tap(&mut u, r.x + 60, r.y + 13);
    tap(&mut u, r.x + 60, r.y + 13);
    assert_ne!(u.studio.view, StudioView::Only, "a double tap on the title");
    // the eye on the whole plot
    u.select_option(Item::EyePlace, "Full");
    settle(&mut u);
    assert!(u.dock_full_state());
    extra_tap(&mut u, VKind::Wave);
    assert_eq!(u.m.eye_place, EyePlace::Side);
    check(&u).unwrap();
}

#[test]
fn closing_down_to_the_last_view_keeps_the_waveform() {
    let mut u = eye_beside_waveform();
    open(&mut u, VKind::Measure);
    extra_tap(&mut u, VKind::Eye);
    // close the views one by one from the front: the group shrinks to the waveform
    for k in [VKind::Eye, VKind::Measure] {
        let front = u.dock_layout().leaves[0].0;
        tab_tap(&mut u, front, k);
        let r = u.dock_rect(k).unwrap();
        let c = Ui::dock_close_rect(r);
        tap(&mut u, c.x + c.w / 2, r.y + STRIP_H / 2);
        settle(&mut u);
        check(&u).unwrap();
    }
    assert!(u.dock_leaf(VKind::Wave).is_some() && u.dock_layout().tabs.is_empty());
    // the waveform stays when it is the last one
    let r = u.dock_rect(VKind::Wave).unwrap();
    let c = Ui::dock_close_rect(r);
    tap(&mut u, c.x + c.w / 2, r.y + STRIP_H / 2);
    check(&u).unwrap();
    assert!(u.dock_leaf(VKind::Wave).is_some());
}

#[test]
fn views_one_and_two_keep_their_tabs() {
    let mut u = eye_beside_waveform();
    u.views_set(2);
    extra_tap(&mut u, VKind::Eye);
    let before = state_text(&u);
    u.view_go(1);
    settle(&mut u);
    check(&u).unwrap();
    u.view_go(0);
    settle(&mut u);
    assert_eq!(state_text(&u), before);
    assert!(u.dock_tabs_root());
    check(&u).unwrap();
}

#[test]
fn the_tree_text_of_a_group_round_trips() {
    for t in ["G0(WE)", "H0.500(G1(WME),P)", "V0.600(W,G2(MIX))"] {
        let n = Node::parse(t).unwrap_or_else(|| panic!("{t}"));
        assert_eq!(n.text(), t.replace("0.500", "0.500"));
    }
    assert!(Node::parse("G2(WE)").is_none() && Node::parse("G0(W)").is_none() && Node::parse("G0(WW)").is_none() && Node::parse("H0.500(G0(WE),E)").is_none());
}
