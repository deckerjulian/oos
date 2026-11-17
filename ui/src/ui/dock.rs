// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio docking: every view of the Studio body (waveform, measurements, eye, second pane, event table, SIM) sits in
//! a small tree of horizontal and vertical splits, or floats as a panel of its own. Each view has a title strip with
//! its name and a close X; dragging the strip (a short drag or a long press) shows where the view would land and
//! moves it: onto an edge of another view splits that view 50/50, onto its centre swaps the two, anywhere else floats
//! it. The splits have draggable dividers. The arrangement is kept in the setup (`Model::studio_dock`).
//!
//! The tree only holds placement. What is on the screen stays decided by the settings the views already have
//! (`dock_shown`): a view that is off is left out of the layout and its neighbour takes the room; when it comes
//! back it gets the place the tree gives it, or a default one.
use super::*;
use super::studio::{chip, StudioView};
use std::rc::Rc;

/// Height of the title strip of a view: the whole strip is the grip, and the X in it is a 44 x 40 target.
pub(super) const STRIP_H: i32 = 26;
/// The buttons of a strip are 40 px targets: they reach down over the top of the content, invisibly.
pub(super) const STRIP_HIT: i32 = 40;
/// Space between two docked views, and the thickness of the divider hit area.
pub(super) const GAP: i32 = 4;
pub(super) const DIV_HIT: i32 = 44;
/// Smallest docked views, strip included.
const MIN_W: i32 = 150;
const MIN_H: i32 = STRIP_H + 80;
/// The measurement list stays usable small: a few rows, and it scrolls.
const MEAS_MIN_W: i32 = 200;
const MEAS_MIN_H: i32 = STRIP_H + 100;
const WAVE_MIN_W: i32 = 300;
const WAVE_MIN_H: i32 = 210;
/// A drag starts when the finger has moved this far.
const DRAG_PX: i32 = 12;
/// Two taps on a strip this close (seconds) are a double tap.
const DOUBLE_TAP: f64 = 0.45;
/// Smallest floating panel, and the resize grip in its corner.
const FLOAT_MIN_W: i32 = 220;
const FLOAT_MIN_H: i32 = 170;
const GRIP: i32 = 44;
/// The edges of a floating panel that resize it.
const EDGE: i32 = 20;
/// Least height of the overview strip: the zoom box and its handles.
const OV_MIN: i32 = 40;
/// The strip of a tab group: the room of the handle and the buttons, and a tab at least this wide.
const TABS_FIXED_W: i32 = 130;
pub(super) const TAB_W: i32 = 30;
const E_L: u8 = 1;
const E_R: u8 = 2;
const E_B: u8 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum VKind {
    Wave,
    Measure,
    Eye,
    Pane,
    Table,
    Sim,
    /// the waveform histogram
    Hist,
    /// the XY picture
    Xy,
    /// the analysers that replace the waveform elsewhere (spectrum, Bode, power, ...)
    View,
}

pub(super) const NK: usize = 9;
pub(super) const KINDS: [VKind; NK] = [VKind::Wave, VKind::Measure, VKind::Eye, VKind::Pane, VKind::Table, VKind::Sim, VKind::Hist, VKind::Xy, VKind::View];

impl VKind {
    pub(super) fn idx(self) -> usize { KINDS.iter().position(|k| *k == self).unwrap_or(0) }
    fn ch(self) -> char {
        match self { VKind::Wave => 'W', VKind::Measure => 'M', VKind::Eye => 'E', VKind::Pane => 'P', VKind::Table => 'T', VKind::Sim => 'S', VKind::Hist => 'I', VKind::Xy => 'X', VKind::View => 'A' }
    }
    fn from_ch(c: char) -> Option<VKind> { KINDS.into_iter().find(|k| k.ch() == c) }
    /// Every view can float; one view always stays docked (`dock_can_float`).
    pub(super) fn floatable(self) -> bool { true }
    fn min_w(self) -> i32 { match self { VKind::Wave => WAVE_MIN_W, VKind::Measure => MEAS_MIN_W, _ => MIN_W } }
    fn min_h(self) -> i32 { match self { VKind::Wave => WAVE_MIN_H, VKind::Measure => MEAS_MIN_H, _ => MIN_H } }
    /// Smallest floating panel.
    fn float_min(self) -> (i32, i32) { if self == VKind::Wave { (WAVE_MIN_W, WAVE_MIN_H + STRIP_H) } else { (FLOAT_MIN_W, FLOAT_MIN_H) } }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Dir {
    /// side by side: the first view at the left
    H,
    /// stacked: the first view on top
    V,
}

#[derive(Clone, PartialEq, Debug)]
pub(super) enum Node {
    Leaf(VKind),
    /// `ratio` is the share of the room the first child takes
    Split { dir: Dir, ratio: f32, a: Box<Node>, b: Box<Node> },
    /// several views in one place, as tabs: `active` is the one shown (a group has two views at least)
    Tabs { kinds: Vec<VKind>, active: usize },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Node {
    pub(super) fn contains(&self, k: VKind) -> bool {
        match self { Node::Leaf(l) => *l == k, Node::Split { a, b, .. } => a.contains(k) || b.contains(k), Node::Tabs { kinds, .. } => kinds.contains(&k) }
    }

    /// Every view of the tree, the hidden tabs too.
    pub(super) fn leaves(&self, out: &mut Vec<VKind>) {
        match self { Node::Leaf(k) => out.push(*k), Node::Split { a, b, .. } => { a.leaves(out); b.leaves(out); } Node::Tabs { kinds, .. } => out.extend(kinds.iter().copied()) }
    }

    /// The tabs: a group of one view is that view, and an empty group is nothing.
    pub(super) fn tabs_of(kinds: Vec<VKind>, active: VKind) -> Option<Node> {
        match kinds.len() {
            0 => None,
            1 => Some(Node::Leaf(kinds[0])),
            _ => { let a = kinds.iter().position(|k| *k == active).unwrap_or(0); Some(Node::Tabs { kinds, active: a }) }
        }
    }

    /// The tab of `k` comes to the front, wherever the group is. False when `k` is in no group.
    pub(super) fn activate(&mut self, k: VKind) -> bool {
        match self {
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => a.activate(k) || b.activate(k),
            Node::Tabs { kinds, active } => match kinds.iter().position(|x| *x == k) { Some(i) => { *active = i; true } None => false },
        }
    }

    /// The tree with `kind` in the group of `at` (a group is made of a lone view).
    pub(super) fn joined(&self, at: VKind, kind: VKind) -> Node {
        match self {
            Node::Leaf(l) if *l == at => Node::Tabs { kinds: vec![at, kind], active: 1 },
            Node::Leaf(_) => self.clone(),
            Node::Split { dir, ratio, a, b } => Node::Split { dir: *dir, ratio: *ratio, a: Box::new(a.joined(at, kind)), b: Box::new(b.joined(at, kind)) },
            Node::Tabs { kinds, .. } if kinds.contains(&at) && !kinds.contains(&kind) => {
                let mut k = kinds.clone();
                k.push(kind);
                Node::Tabs { active: k.len() - 1, kinds: k }
            }
            Node::Tabs { .. } => self.clone(),
        }
    }

    /// The tree without `k`: the sibling takes the place of the split that held it. None when nothing is left.
    pub(super) fn without(&self, k: VKind) -> Option<Node> {
        match self {
            Node::Leaf(l) => (*l != k).then(|| self.clone()),
            Node::Split { dir, ratio, a, b } => match (a.without(k), b.without(k)) {
                (Some(a), Some(b)) => Some(Node::Split { dir: *dir, ratio: *ratio, a: Box::new(a), b: Box::new(b) }),
                (Some(n), None) | (None, Some(n)) => Some(n),
                (None, None) => None,
            },
            Node::Tabs { kinds, active } => {
                let front = kinds[(*active).min(kinds.len() - 1)];
                Self::tabs_of(kinds.iter().copied().filter(|x| *x != k).collect(), front)
            }
        }
    }

    /// The tree with the leaf of `at` replaced by `with`. A view in a group: a lone view takes its tab, a bigger
    /// arrangement takes the place of the whole group.
    pub(super) fn replace(&self, at: VKind, with: &Node) -> Node {
        match self {
            Node::Leaf(l) if *l == at => with.clone(),
            Node::Leaf(_) => self.clone(),
            Node::Tabs { kinds, active } if kinds.contains(&at) => match with {
                Node::Leaf(x) => Node::Tabs { kinds: kinds.iter().map(|k| if *k == at { *x } else { *k }).collect(), active: *active },
                _ => with.replace(at, self),
            },
            Node::Tabs { .. } => self.clone(),
            Node::Split { dir, ratio, a, b } => Node::Split { dir: *dir, ratio: *ratio, a: Box::new(a.replace(at, with)), b: Box::new(b.replace(at, with)) },
        }
    }

    /// `k` and `other` change places.
    pub(super) fn swapped(&self, k: VKind, other: VKind) -> Node {
        match self {
            Node::Leaf(l) if *l == k => Node::Leaf(other),
            Node::Leaf(l) if *l == other => Node::Leaf(k),
            Node::Leaf(_) => self.clone(),
            Node::Tabs { kinds, active } => Node::Tabs { kinds: kinds.iter().map(|x| if *x == k { other } else if *x == other { k } else { *x }).collect(), active: *active },
            Node::Split { dir, ratio, a, b } => {
                // two views side by side trade their sizes with their places
                let pair = matches!((&**a, &**b), (Node::Leaf(x), Node::Leaf(y)) if (*x == k && *y == other) || (*x == other && *y == k));
                Node::Split { dir: *dir, ratio: if pair { 1.0 - *ratio } else { *ratio }, a: Box::new(a.swapped(k, other)), b: Box::new(b.swapped(k, other)) }
            }
        }
    }

    /// The split at `path` (false: first child, true: second) with a new ratio.
    pub(super) fn with_ratio(&self, path: &[bool], r: f32) -> Node {
        match (self, path.split_first()) {
            (Node::Split { dir, a, b, .. }, None) => Node::Split { dir: *dir, ratio: r, a: a.clone(), b: b.clone() },
            (Node::Split { dir, ratio, a, b }, Some((&second, rest))) => {
                if second { Node::Split { dir: *dir, ratio: *ratio, a: a.clone(), b: Box::new(b.with_ratio(rest, r)) } }
                else { Node::Split { dir: *dir, ratio: *ratio, a: Box::new(a.with_ratio(rest, r)), b: b.clone() } }
            }
            _ => self.clone(),
        }
    }

    /// The smallest this node can be along `dir`.
    fn min_along(&self, dir: Dir) -> i32 {
        match self {
            Node::Leaf(k) => if dir == Dir::H { k.min_w() } else { k.min_h() },
            Node::Tabs { kinds, .. } => {
                let m = kinds.iter().map(|k| if dir == Dir::H { k.min_w() } else { k.min_h() }).max().unwrap_or(0);
                // the strip has the room of its buttons and a tab each
                if dir == Dir::H { m.max(TABS_FIXED_W + TAB_W * kinds.len() as i32) } else { m }
            }
            Node::Split { dir: d, a, b, .. } => {
                let (x, y) = (a.min_along(dir), b.min_along(dir));
                if *d == dir { x + GAP + y } else { x.max(y) }
            }
        }
    }

    // ------------------------------------------------------------------------------------------ the setup text

    /// `W`, or `H0.60(W,V0.50(E,M))`: a split and its ratio, then its two children.
    pub(super) fn text(&self) -> String {
        match self {
            Node::Leaf(k) => k.ch().to_string(),
            Node::Tabs { kinds, active } => format!("G{}({})", active, kinds.iter().map(|k| k.ch()).collect::<String>()),
            Node::Split { dir, ratio, a, b } => format!("{}{:.3}({},{})", if *dir == Dir::H { 'H' } else { 'V' }, ratio, a.text(), b.text()),
        }
    }

    pub(super) fn parse(s: &str) -> Option<Node> {
        let b: Vec<char> = s.chars().collect();
        let mut i = 0;
        let n = Self::parse_at(&b, &mut i, 0)?;
        if i != b.len() { return None; }
        let mut v = Vec::new();
        n.leaves(&mut v);
        // every view at most once
        (0..v.len()).all(|x| !v[..x].contains(&v[x])).then_some(n)
    }

    fn parse_at(b: &[char], i: &mut usize, depth: usize) -> Option<Node> {
        if depth > 8 { return None; }
        let c = *b.get(*i)?;
        *i += 1;
        if let Some(k) = VKind::from_ch(c) { return Some(Node::Leaf(k)); }
        if c == 'G' {
            let start = *i;
            while b.get(*i).is_some_and(|c| c.is_ascii_digit()) { *i += 1; }
            let active: usize = b[start..*i].iter().collect::<String>().parse().ok()?;
            if *b.get(*i)? != '(' { return None; }
            *i += 1;
            let mut kinds = Vec::new();
            while let Some(k) = b.get(*i).copied().and_then(VKind::from_ch) { kinds.push(k); *i += 1; }
            if *b.get(*i)? != ')' { return None; }
            *i += 1;
            return (kinds.len() >= 2 && active < kinds.len()).then_some(Node::Tabs { kinds, active });
        }
        let dir = match c { 'H' => Dir::H, 'V' => Dir::V, _ => return None };
        let start = *i;
        while b.get(*i).is_some_and(|c| c.is_ascii_digit() || *c == '.') { *i += 1; }
        let ratio: f32 = b[start..*i].iter().collect::<String>().parse().ok()?;
        if !(0.05..=0.95).contains(&ratio) { return None; }
        if *b.get(*i)? != '(' { return None; }
        *i += 1;
        let a = Self::parse_at(b, i, depth + 1)?;
        if *b.get(*i)? != ',' { return None; }
        *i += 1;
        let c2 = Self::parse_at(b, i, depth + 1)?;
        if *b.get(*i)? != ')' { return None; }
        *i += 1;
        Some(Node::Split { dir, ratio, a: Box::new(a), b: Box::new(c2) })
    }
}

/// What the button left of the X of a strip does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Extra {
    /// the measurements alone fill the area
    Expand,
    /// every view in tabs of one group, this one in front
    Maximise,
    /// back from the measurements alone
    RestoreOnly,
    /// back from one group of tabs
    RestoreTabs,
    /// the eye leaves the whole plot for a pane beside it
    RestoreEye,
}

/// What a press on a title strip may become.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Press {
    /// the strip itself: a drag or a long press moves the view; a lift without either does nothing
    Strip { kind: VKind, x0: i32, y0: i32, x: i32, y: i32, t0: f64, grab: (i32, i32), free: bool },
    /// the X, or the extra button of a strip (maximize, or restore from it); acts on the lift
    Button { kind: VKind, close: bool },
    /// the List / Detailed switch of the measurements panel
    View { kind: VKind },
    /// a control of the measurements bar in the strip (source, gate, search, pinned, add, statistics)
    Bar,
}

#[derive(Clone, PartialEq, Debug)]
pub(super) enum DockDrag {
    /// a view follows the finger; the drop preview shows where it would land
    Move { kind: VKind, x: i32, y: i32, grab: (i32, i32), free: bool },
    /// a divider moves: the split at `path`, the room it divides along `dir` from `origin`
    Divider { path: Vec<bool>, dir: Dir, origin: i32, avail: i32, lo: i32, hi: i32 },
    /// the corner of a floating panel
    Resize { kind: VKind, x0: i32, y0: i32, r0: Rect, edges: u8 },
}

/// Where a dragged view would land.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Drop {
    /// nowhere: the view stays
    Stay,
    Swap(VKind),
    Split(VKind, Side),
    Float(Rect),
    /// into the tabs of that view
    Tab(VKind),
}

#[derive(Default)]
pub(super) struct Dock {
    /// the arrangement the user made; None is the default one
    pub tree: Option<Node>,
    /// where each floating view is (kept while it is docked)
    pub float_rect: [Option<Rect>; NK],
    /// which views float (the measurements follow the View menu: `StudioView::Floating`)
    pub float_on: [bool; NK],
    /// the floating views, the one on top last
    pub z: Vec<VKind>,
    /// counts every change of the arrangement: the layout cache and the setup text follow it
    pub ver: u32,
    pub synced: Option<u32>,
    pub press: Option<Press>,
    pub drag: Option<DockDrag>,
    /// the list of tabs that did not fit is open under the strip of this view
    pub tab_list: Option<VKind>,
    /// the arrangement before the views were put in tabs to fill the area (the way back of the restore button)
    pub restore: Option<Saved>,
    /// the views that were on at the last frame: one that comes back is brought to the front of its tabs
    pub seen: Option<u16>,
    /// the last tap on a strip: a second one on the same view is a double tap
    pub last_tap: Option<(f64, VKind)>,
    pub cache: std::cell::RefCell<Option<(DockKey, Rc<DockLayout>)>>,
}

/// The arrangement kept while the views are tabs of one group.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Saved {
    pub tree: Option<Node>,
    pub floats: [bool; NK],
    /// the measurements were floating (they follow the View menu)
    pub meas_floating: bool,
}

#[derive(Clone, PartialEq)]
pub(super) struct DockKey {
    region: Rect,
    shown: u16,
    floating: u16,
    ver: u32,
    panel_w: i32,
    under: bool,
}

#[derive(Clone, Debug)]
pub(super) struct Divider {
    pub path: Vec<bool>,
    pub dir: Dir,
    /// the touch area (44 px thick) and the line that is drawn
    pub hit: Rect,
    pub line: Rect,
    pub origin: i32,
    pub avail: i32,
    pub lo: i32,
    pub hi: i32,
}

/// Views in one place as tabs: the rectangle they share, their order and the one in front.
#[derive(Clone, Debug)]
pub(super) struct TabGroup {
    pub rect: Rect,
    pub kinds: Vec<VKind>,
    pub active: usize,
}

#[derive(Clone, Debug, Default)]
pub(super) struct DockLayout {
    pub region: Rect,
    /// the views that are drawn: one per place (the tab in front for a group)
    pub leaves: Vec<(VKind, Rect)>,
    pub dividers: Vec<Divider>,
    pub tabs: Vec<TabGroup>,
}

impl Default for Rect {
    fn default() -> Rect { Rect::new(0, 0, 0, 0) }
}

fn lay(n: &Node, r: Rect, path: &mut Vec<bool>, out: &mut DockLayout) {
    match n {
        Node::Leaf(k) => out.leaves.push((*k, r)),
        Node::Tabs { kinds, active } => {
            let i = (*active).min(kinds.len() - 1);
            out.leaves.push((kinds[i], r));
            out.tabs.push(TabGroup { rect: r, kinds: kinds.clone(), active: i });
        }
        Node::Split { dir, ratio, a, b } => {
            let total = if *dir == Dir::H { r.w } else { r.h };
            let avail = (total - GAP).max(2);
            let (ma, mb) = (a.min_along(*dir), b.min_along(*dir));
            let want = (*ratio * avail as f32).round() as i32;
            // the mins win unless there is no room for both: then the ratio is kept
            let (lo, hi) = if ma + mb <= avail { (ma, avail - mb) } else { (1, avail - 1) };
            let sa = want.clamp(lo, hi.max(lo));
            let (ra, line, rb, origin) = if *dir == Dir::H {
                (Rect::new(r.x, r.y, sa, r.h), Rect::new(r.x + sa, r.y, GAP, r.h), Rect::new(r.x + sa + GAP, r.y, avail - sa, r.h), r.x)
            } else {
                (Rect::new(r.x, r.y, r.w, sa), Rect::new(r.x, r.y + sa, r.w, GAP), Rect::new(r.x, r.y + sa + GAP, r.w, avail - sa), r.y)
            };
            let hit = if *dir == Dir::H {
                Rect::new(line.x + GAP / 2 - DIV_HIT / 2, line.y, DIV_HIT, line.h)
            } else {
                Rect::new(line.x, line.y + GAP / 2 - DIV_HIT / 2, line.w, DIV_HIT)
            };
            out.dividers.push(Divider { path: path.clone(), dir: *dir, hit, line, origin, avail, lo, hi: hi.max(lo) });
            path.push(false);
            lay(a, ra, path, out);
            path.pop();
            path.push(true);
            lay(b, rb, path, out);
            path.pop();
        }
    }
}

/// Where a view sits in a default arrangement before the user moved anything.
fn insert_default(base: Node, k: VKind, region: Rect, panel_w: i32, under: bool) -> Node {
    let leaf = Box::new(Node::Leaf(k));
    match k {
        VKind::Wave => Node::Split { dir: Dir::H, ratio: 0.5, a: leaf, b: Box::new(base) },
        VKind::Measure | VKind::Sim => {
            let ratio = ((region.w - panel_w - GAP) as f32 / (region.w - GAP).max(1) as f32).clamp(0.3, 0.85);
            Node::Split { dir: Dir::H, ratio, a: Box::new(base), b: leaf }
        }
        VKind::Hist | VKind::Xy | VKind::View => {
            let n = Node::Split { dir: Dir::H, ratio: 0.58, a: Box::new(Node::Leaf(VKind::Wave)), b: leaf };
            if base.contains(VKind::Wave) { base.replace(VKind::Wave, &n) } else { Node::Split { dir: Dir::H, ratio: 0.58, a: Box::new(base), b: Box::new(Node::Leaf(k)) } }
        }
        VKind::Eye => {
            let n = Node::Split { dir: if under { Dir::V } else { Dir::H }, ratio: 0.58, a: Box::new(Node::Leaf(VKind::Wave)), b: leaf };
            if base.contains(VKind::Wave) { base.replace(VKind::Wave, &n) } else { Node::Split { dir: Dir::H, ratio: 0.58, a: Box::new(base), b: Box::new(Node::Leaf(k)) } }
        }
        VKind::Pane | VKind::Table => {
            let n = Node::Split { dir: Dir::V, ratio: 0.6, a: Box::new(Node::Leaf(VKind::Wave)), b: leaf };
            if base.contains(VKind::Wave) { base.replace(VKind::Wave, &n) } else { Node::Split { dir: Dir::V, ratio: 0.6, a: Box::new(base), b: Box::new(Node::Leaf(k)) } }
        }
    }
}

fn drop_zone(l: Rect, x: i32, y: i32) -> Zone {
    let band_x = (l.w * 22 / 100).clamp(1, (l.w / 3).max(48));
    let band_y = (l.h * 22 / 100).clamp(1, (l.h / 3).max(48));
    let (dl, dr, dt, db) = (x - l.x, l.right() - 1 - x, y - l.y, l.bottom() - 1 - y);
    // the nearest edge in the units of its band
    let mut best: Option<(f32, Side)> = None;
    for (d, band, side) in [(dl, band_x, Side::Left), (dr, band_x, Side::Right), (dt, band_y, Side::Top), (db, band_y, Side::Bottom)] {
        if d < band {
            let n = d as f32 / band as f32;
            if best.is_none_or(|(m, _)| n < m) { best = Some((n, side)); }
        }
    }
    if let Some((_, s)) = best { return Zone::Edge(s); }
    let (cx, cy) = (l.x + l.w / 2, l.y + l.h / 2);
    if (x - cx).abs() <= l.w * 14 / 100 && (y - cy).abs() <= l.h * 14 / 100 { Zone::Centre } else { Zone::Ring }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Zone {
    Edge(Side),
    Centre,
    Ring,
}

/// The half of `l` a split puts the dragged view in.
pub(super) fn half_of(l: Rect, s: Side) -> Rect {
    let (hw, hh) = ((l.w - GAP) / 2, (l.h - GAP) / 2);
    match s {
        Side::Left => Rect::new(l.x, l.y, hw, l.h),
        Side::Right => Rect::new(l.right() - hw, l.y, hw, l.h),
        Side::Top => Rect::new(l.x, l.y, l.w, hh),
        Side::Bottom => Rect::new(l.x, l.bottom() - hh, l.w, hh),
    }
}

impl Ui {
    // ------------------------------------------------------------------------------------------------ what is shown

    pub(super) fn dock_on(&self) -> bool { self.m.layout == Layout::Studio }

    /// Whether a view is on the screen (docked or floating).
    pub(super) fn dock_shown(&self, k: VKind) -> bool {
        let v = self.studio.view;
        match k {
            VKind::Wave => v != StudioView::Only,
            VKind::Measure => matches!(v, StudioView::Docked | StudioView::Floating | StudioView::Only | StudioView::Offline),
            VKind::Sim => v == StudioView::Sim,
            VKind::Eye => v != StudioView::Only && self.eye_open() && self.m.eye_place != EyePlace::Full,
            VKind::Pane => v != StudioView::Only && self.m.pane != 0 && matches!(self.m.view, AView::Scope | AView::Eye),
            VKind::Hist => v != StudioView::Only && self.m.hist != Hist::Off,
            VKind::Xy => v != StudioView::Only && self.xy,
            VKind::View => v != StudioView::Only && !self.xy_over() && !matches!(self.m.view, AView::Scope | AView::Eye),
            VKind::Table => v != StudioView::Only && self.table.is_some(),
        }
    }

    pub(super) fn dock_floating(&self, k: VKind) -> bool { self.dock_raw_floating(k) && self.dock_forced() != Some(k) }

    fn dock_raw_floating(&self, k: VKind) -> bool {
        if k == VKind::Measure {
            return self.studio.view == StudioView::Floating || (self.studio.view == StudioView::Offline && self.studio.dock.float_on[k.idx()]);
        }
        self.studio.dock.float_on[k.idx()]
    }

    /// The view that is docked although it floats: when every view on the screen floats the dock would be an empty
    /// frame with a copy of the waveform over it. The waveform (else the first view) goes back in the dock.
    pub(super) fn dock_forced(&self) -> Option<VKind> {
        if self.studio.view == StudioView::Only { return None; }
        let mut shown = KINDS.into_iter().filter(|k| self.dock_shown(*k)).peekable();
        shown.peek()?;
        let all: Vec<VKind> = shown.collect();
        if all.iter().any(|k| !self.dock_raw_floating(*k)) { return None; }
        Some(if all.contains(&VKind::Wave) { VKind::Wave } else { all[0] })
    }

    /// The views that are on and docked (in a split or a group of tabs).
    pub(super) fn dock_docked_count(&self) -> usize { KINDS.into_iter().filter(|k| self.dock_docked(*k)).count() }

    /// May this view float: not when it is the last one docked, so the tree is never empty.
    pub(super) fn dock_can_float(&self, k: VKind) -> bool {
        if self.studio.view == StudioView::Only { return false; }
        if k == VKind::Measure && !matches!(self.studio.view, StudioView::Docked | StudioView::Floating | StudioView::Offline) { return false; }
        self.dock_floating(k) || self.dock_docked_count() > 1
    }

    fn dock_docked(&self, k: VKind) -> bool { self.dock_shown(k) && !self.dock_floating(k) }

    /// The room of the views: between the bars, less the settings sheet.
    pub(super) fn dock_region(&self) -> Rect {
        let fr = self.frame();
        let (y0, y1) = if self.full_on() { (0, self.scr.h) } else { (fr.body.y, fr.bottom.y) };
        // the side sheet floats over the docks: the waveform keeps its size
        let w = fr.body.w.max(1);
        Rect::new(fr.body.x, y0, w, (y1 - y0).max(1))
    }

    fn dock_under(&self) -> bool { self.m.eye_place == EyePlace::Under }

    /// The arrangement as it is drawn now: the user's tree (or the default), the views that are on and docked put in
    /// when it has no place for them, the others left out.
    pub(super) fn dock_effective(&self, region: Rect) -> Node {
        let mut t = self.studio.dock.tree.clone().unwrap_or(Node::Leaf(VKind::Wave));
        let docked: Vec<VKind> = if self.studio.view == StudioView::Only { vec![VKind::Measure] }
            else { KINDS.into_iter().filter(|k| self.dock_docked(*k)).collect() };
        // the waveform first, then the others in the order they appear in `KINDS`
        for k in docked.iter().copied() {
            if !t.contains(k) { t = insert_default(t, k, region, self.studio_panel_w(), self.dock_under()); }
        }
        for k in KINDS {
            if !docked.contains(&k) { if let Some(n) = t.without(k) { t = n; } }
        }
        if docked.is_empty() { Node::Leaf(VKind::Wave) } else { t }
    }

    pub(super) fn dock_layout(&self) -> Rc<DockLayout> {
        let region = self.dock_region();
        let mask = |f: &dyn Fn(VKind) -> bool| KINDS.iter().fold(0u16, |m, k| m | ((f(*k) as u16) << k.idx()));
        let key = DockKey {
            region,
            shown: mask(&|k| self.dock_shown(k)),
            floating: mask(&|k| self.dock_floating(k)),
            ver: self.studio.dock.ver,
            panel_w: self.studio_panel_w(),
            under: self.dock_under(),
        };
        if let Some((k, l)) = self.studio.dock.cache.borrow().as_ref() {
            if *k == key { return l.clone(); }
        }
        let mut out = DockLayout { region, ..Default::default() };
        let tree = self.dock_effective(region);
        lay(&tree, region, &mut Vec::new(), &mut out);
        let l = Rc::new(out);
        *self.studio.dock.cache.borrow_mut() = Some((key, l.clone()));
        l
    }

    /// The docked rectangle of a view, its strip included.
    pub(super) fn dock_leaf(&self, k: VKind) -> Option<Rect> {
        self.dock_layout().leaves.iter().find(|(l, _)| *l == k).map(|(_, r)| *r)
    }

    /// Where the floating views may be: the body, not the row of badges under the lanes.
    pub(super) fn dock_float_region(&self) -> Rect {
        let r = self.dock_region();
        Rect::new(r.x, r.y, r.w, (r.h - self.studio_row_h()).max(FLOAT_MIN_H))
    }

    /// A floating view's rectangle, kept inside the body and clear of the zoom bar and the corner buttons.
    pub(super) fn dock_float_rect(&self, k: VKind) -> Option<Rect> {
        if !self.dock_shown(k) || !self.dock_floating(k) { return None; }
        let reg = self.dock_float_region();
        let r = self.studio.dock.float_rect[k.idx()].unwrap_or_else(|| self.dock_default_float(k));
        Some(self.dock_fit_float(k, r, reg))
    }

    pub(super) fn dock_default_float(&self, k: VKind) -> Rect {
        let reg = self.dock_float_region();
        let (w, h) = match k { VKind::Measure => (self.studio_panel_w() + 40, 300), VKind::Eye => (400, 300), VKind::Wave => (reg.w * 6 / 10, reg.h * 7 / 10), _ => (420, 240) };
        // the old position of the floating measurements keeps meaning
        let (x, y) = if k == VKind::Measure { self.studio.panel.at.unwrap_or((reg.right() - w - 8, reg.y + 56)) } else { (reg.right() - w - 8, reg.y + 56) };
        Rect::new(x, y, w, h)
    }

    pub(super) fn dock_fit_float(&self, k: VKind, r: Rect, reg: Rect) -> Rect {
        let (mw, mh) = k.float_min();
        let w = r.w.clamp(mw, reg.w.max(mw));
        let h = r.h.clamp(mh, reg.h.max(mh));
        let x = r.x.clamp(reg.x, (reg.right() - w).max(reg.x));
        let mut y = r.y.clamp(reg.y, (reg.bottom() - h).max(reg.y));
        // never over the corner buttons of the plot: beside them
        let mut x = x;
        // the waveform's own buttons are in it, not under it
        if k != VKind::Wave {
            for b in [self.full_btn().0, self.zoom_btn().0] {
                let b = Rect::new(b.x - 4, b.y - 4, b.w + 8, b.h + 8);
                if x + w > b.x && x < b.right() && y < b.bottom() && y + h > b.y { x = (b.x - w).max(reg.x); }
            }
        }
        // never over the zoom bar: under it
        if k != VKind::Wave && self.studio_zoom_active() && !self.view_on() {
            let bar = self.zoom_readout();
            if x < bar.right() && x + w > bar.x && y < bar.bottom() + 2 && y + h > bar.y {
                y = (bar.bottom() + 2).min((reg.bottom() - h).max(reg.y));
            }
        }
        Rect::new(x, y, w, h)
    }

    /// The rectangle of a view on the screen, strip included.
    pub(super) fn dock_rect(&self, k: VKind) -> Option<Rect> {
        self.dock_float_rect(k).or_else(|| self.dock_leaf(k))
    }

    /// The dotted box at the left of a title: the handle that docks a floating view. Dragged by the rest of its title a
    /// floating view only moves.
    pub(super) fn dock_handle_rect(r: Rect) -> Rect { Rect::new(r.x, r.y, 32.min(r.w), STRIP_H.min(r.h)) }

    pub(super) fn dock_strip(r: Rect) -> Rect { Rect::new(r.x, r.y, r.w, STRIP_H.min(r.h)) }

    /// What the view itself draws in: under its strip.
    pub(super) fn dock_content(&self, k: VKind) -> Option<Rect> {
        let r = self.dock_rect(k)?;
        Some(Rect::new(r.x, r.y + STRIP_H, r.w, (r.h - STRIP_H).max(1)))
    }

    /// The waveform's rectangle, strip included. The table of the measurements alone has none: the whole room.
    pub(super) fn dock_wave_rect(&self) -> Rect {
        let region = self.dock_region();
        // a tab behind another: the waveform is not on the screen, its place is beside it (nothing there takes a touch)
        if self.dock_tabbed_out(VKind::Wave) { return Rect::new(region.x - region.w - 100, region.y, region.w, region.h); }
        self.dock_rect(VKind::Wave).unwrap_or(region)
    }

    /// A docked view that is a tab behind another one of its group.
    pub(super) fn dock_tabbed_out(&self, k: VKind) -> bool {
        self.dock_docked(k) && self.studio.view != StudioView::Only && self.dock_layout().tabs.iter().any(|g| g.kinds.contains(&k) && g.kinds[g.active] != k)
    }

    /// The group of tabs a view is in.
    pub(super) fn dock_tab_group(&self, k: VKind) -> Option<TabGroup> {
        if self.studio.view == StudioView::Only || !self.dock_docked(k) { return None; }
        self.dock_layout().tabs.iter().find(|g| g.kinds.contains(&k)).cloned()
    }

    pub(super) fn dock_close_rect(r: Rect) -> Rect { Rect::new(r.right() - 48, r.y, 48, STRIP_HIT.min(r.h)) }

    /// The button left of the X: what it does depends on the view and the arrangement.
    pub(super) fn dock_extra_kind(&self, k: VKind) -> Option<Extra> {
        if !self.dock_on() { return None; }
        if self.studio.view == StudioView::Only { return (k == VKind::Measure).then_some(Extra::RestoreOnly); }
        if !self.dock_floating(k) && self.dock_tabs_root() { return Some(Extra::RestoreTabs); }
        if k == VKind::Wave && self.eye_open() && self.m.eye_place == EyePlace::Full { return Some(Extra::RestoreEye); }
        if k == VKind::Measure && self.studio.view != StudioView::Offline { return Some(Extra::Expand); }
        (KINDS.into_iter().filter(|v| self.dock_shown(*v)).count() > 1).then_some(Extra::Maximise)
    }

    pub(super) fn dock_extra_rect(&self, k: VKind, r: Rect) -> Option<Rect> {
        self.dock_extra_kind(k).map(|_| Rect::new(r.right() - 96, r.y, 48, STRIP_HIT.min(r.h)))
    }

    /// One group of tabs holds every view that is docked: it fills the area.
    pub(super) fn dock_tabs_root(&self) -> bool {
        self.dock_on() && self.studio.view != StudioView::Only && { let l = self.dock_layout(); l.tabs.len() == 1 && l.leaves.len() == 1 }
    }

    /// A way back out of a full state is on the screen: a view that fills the area (measurements only, the eye on the
    /// whole plot, everything in one group of tabs).
    pub(super) fn dock_full_state(&self) -> bool {
        self.dock_on() && (self.studio.view == StudioView::Only || self.dock_tabs_root() || (self.eye_open() && self.m.eye_place == EyePlace::Full))
    }

    /// Leaves a full state: the way back every exit (the button, Escape, the double tap, the menu) takes. False when
    /// there was none.
    pub(super) fn dock_exit_full(&mut self) -> bool {
        if !self.dock_on() { return false; }
        if self.studio.view == StudioView::Only { self.studio_set_view(StudioView::Docked); self.dock_changed(); return true; }
        if self.dock_tabs_root() { self.dock_restore_layout(); return true; }
        if self.eye_open() && self.m.eye_place == EyePlace::Full {
            self.eye_set_place(EyePlace::Side);
            self.dock_changed();
            return true;
        }
        false
    }

    // ------------------------------------------------------------------------------------------------ tabs

    /// The space of the title in a strip: its width, and whether the search field of the measurements replaces it.
    fn dock_title_geo(&self, k: VKind, r: Rect) -> (i32, bool) {
        let s = Self::dock_strip(r);
        let close = Self::dock_close_rect(r);
        let extra = self.dock_extra_rect(k, r);
        let view = self.dock_view_rect(k, r);
        let right = view.map_or(extra.map_or(close.x, |e| e.x), |v| v.x);
        let bar = self.dock_bar_content(k, r).map(|p| self.panel_bar(p));
        let right_title = bar.as_ref().map_or(right, |b| b.left - 6);
        let zoom_in_strip = k == VKind::Wave && self.studio_zoom_active() && !self.zoom_tab_mode();
        let right_title = if zoom_in_strip { self.zoom_readout().x - 4 } else { right_title };
        ((right_title - s.x - 30).max(1), bar.as_ref().is_some_and(|b| b.field.is_some()))
    }

    /// The tabs in the strip of the view in front of its group: left-aligned, each as wide as its name (28 to 200 px).
    /// What does not fit goes in the list behind the arrow (`dock_tab_more`); the one in front always stays.
    pub(super) fn dock_tab_rects(&self, k: VKind, r: Rect) -> Vec<(VKind, Rect)> { self.dock_tab_layout(k, r).0 }

    /// The arrow that opens the list of the tabs that did not fit.
    pub(super) fn dock_tab_more(&self, k: VKind, r: Rect) -> Option<Rect> { self.dock_tab_layout(k, r).1 }

    fn dock_tab_layout(&self, k: VKind, r: Rect) -> (Vec<(VKind, Rect)>, Option<Rect>) {
        let none = (Vec::new(), None);
        let Some(g) = self.dock_tab_group(k) else { return none };
        if g.kinds[g.active] != k { return none; }
        let (avail, field) = self.dock_title_geo(k, r);
        if field { return none; }
        let s = Self::dock_strip(r);
        let f = self.fonts();
        let want: Vec<i32> = g.kinds.iter().map(|v| (f.mb.width(&self.dock_name(*v)) + 20).clamp(TAB_W - 2, 200)).collect();
        let total: i32 = want.iter().sum::<i32>() + 2 * want.len() as i32;
        let mut keep: Vec<usize> = (0..g.kinds.len()).collect();
        let mut more = false;
        if total > avail {
            // the arrow takes its place; the tab in front first, then the others in order while they fit
            more = true;
            let room = avail - TAB_W - 2;
            keep = vec![g.active];
            let mut used = want[g.active] + 2;
            for i in 0..g.kinds.len() {
                if i != g.active && used + want[i] + 2 <= room { keep.push(i); used += want[i] + 2; }
            }
            keep.sort();
        }
        let mut x = s.x + 26;
        let room_end = s.x + 26 + avail;
        let mut out = Vec::new();
        for i in keep {
            let w = want[i].min((room_end - x - if more { TAB_W + 2 } else { 0 }).max(TAB_W - 2));
            out.push((g.kinds[i], Rect::new(x, s.y, w, s.h)));
            x += w + 2;
        }
        let arrow = more.then(|| Rect::new((room_end - TAB_W).max(x), s.y, TAB_W, s.h));
        (out, arrow)
    }

    /// The rows of the list of tabs (the views of the group that are not in the strip) under the arrow.
    pub(super) fn dock_tab_list_rows(&self, front: VKind) -> Vec<(VKind, Rect)> {
        let Some(r) = self.dock_rect(front) else { return Vec::new() };
        let Some(g) = self.dock_tab_group(front) else { return Vec::new() };
        let Some(arrow) = self.dock_tab_more(front, r) else { return Vec::new() };
        let shown: Vec<VKind> = self.dock_tab_rects(front, r).into_iter().map(|(v, _)| v).collect();
        let hidden: Vec<VKind> = g.kinds.iter().copied().filter(|v| !shown.contains(v)).collect();
        let w = 200;
        let x = (arrow.right() - w).max(r.x);
        hidden.into_iter().enumerate().map(|(i, v)| (v, Rect::new(x, r.y + STRIP_H + 2 + i as i32 * 36, w, 36))).collect()
    }

    /// The view whose tab is under a point of a strip.
    fn dock_tab_at(&self, k: VKind, r: Rect, x: i32, y: i32) -> Option<VKind> {
        self.dock_tab_rects(k, r).into_iter().find(|(_, t)| t.contains(x, y)).map(|(v, _)| v)
    }

    /// The views of a group as they are, put in tabs with `k` in front: floating views too, so no window floats over
    /// the one that fills the area. Where they were is kept for the way back.
    pub(super) fn dock_maximise(&mut self, k: VKind) {
        if self.studio.view == StudioView::Only || !self.dock_shown(k) { return; }
        let region = self.dock_region();
        let tree = self.dock_effective(region);
        let mut kinds = Vec::new();
        tree.leaves(&mut kinds);
        for v in KINDS { if self.dock_shown(v) && !kinds.contains(&v) { kinds.push(v); } }
        if kinds.len() < 2 { return; }
        if self.dock_tabs_root() { self.dock_activate(k); return; }
        let saved = Saved { tree: self.studio.dock.tree.clone(), floats: self.studio.dock.float_on, meas_floating: self.studio.view == StudioView::Floating };
        for v in kinds.iter().copied() { if self.dock_floating(v) { self.dock_set_float(v, false); } }
        self.studio.dock.tree = Node::tabs_of(kinds, k);
        self.studio.dock.restore = Some(saved);
        self.dock_changed();
    }

    /// The tab of a view comes to the front.
    pub(super) fn dock_activate(&mut self, k: VKind) {
        let region = self.dock_region();
        let mut tree = self.dock_effective(region);
        if tree.activate(k) {
            self.studio.dock.tree = Some(tree);
            self.dock_changed();
        }
    }

    /// The way back of the views in one group of tabs: where they were, or (nothing was kept) side by side again.
    pub(super) fn dock_restore_layout(&mut self) {
        let region = self.dock_region();
        if let Some(s) = self.studio.dock.restore.take() {
            self.studio.dock.tree = s.tree;
            self.studio.dock.float_on = s.floats;
            if s.meas_floating && self.studio.view == StudioView::Docked { self.studio.view = StudioView::Floating; }
            self.dock_changed();
            return;
        }
        let tree = self.dock_effective(region);
        let Node::Tabs { kinds, .. } = tree else { return };
        let mut order = kinds;
        order.sort_by_key(|k| (*k != VKind::Wave, k.idx()));
        let mut t = Node::Leaf(order[0]);
        for k in order.into_iter().skip(1) { t = insert_default(t, k, region, self.studio_panel_w(), self.dock_under()); }
        self.studio.dock.tree = Some(t);
        self.dock_changed();
    }

    /// Every frame: the dock keeps itself sane. A view that all the others float over is brought back into the dock,
    /// and a view that is on again comes to the front of its tabs.
    fn dock_normalize(&mut self) {
        if !self.dock_on() { return; }
        let mut changed = false;
        if let Some(f) = self.dock_forced() {
            if f != VKind::Measure { self.studio.dock.float_on[f.idx()] = false; } else { self.dock_set_float(f, false); }
            changed = true;
        }
        let mask = KINDS.iter().fold(0u16, |m, k| m | ((self.dock_shown(*k) as u16) << k.idx()));
        // the first frame of an arrangement only looks
        let new = self.studio.dock.seen.map_or(0, |seen| mask & !seen);
        self.studio.dock.seen = Some(mask);
        if new != 0 {
            let region = self.dock_region();
            let mut tree = self.dock_effective(region);
            let mut any = false;
            for k in KINDS { if new & (1 << k.idx()) != 0 && !self.dock_floating(k) { any |= tree.activate(k); } }
            if any { self.studio.dock.tree = Some(tree); changed = true; }
        }
        // a stale way back
        if self.studio.dock.restore.is_some() && !self.dock_tabs_root() && self.studio.dock.drag.is_none() { self.studio.dock.restore = None; }
        if changed { self.dock_changed(); }
    }

    /// The edges of a floating panel a finger at (x, y) would drag: the corner, the sides and the bottom. The top is
    /// the strip, which moves the panel.
    pub(super) fn dock_edges_at(r: Rect, x: i32, y: i32) -> Option<u8> {
        if Self::dock_grip_rect(r).contains(x, y) { return Some(E_R | E_B); }
        if y < r.y + STRIP_H { return None; }
        let mut e = 0;
        if x >= r.right() - EDGE { e |= E_R; } else if x < r.x + EDGE { e |= E_L; }
        if y >= r.bottom() - EDGE { e |= E_B; }
        (e != 0).then_some(e)
    }

    /// The handle in the middle of a divider: a 44 px square, wider along the divider.
    pub(super) fn divider_handle_hit(d: &Divider) -> Rect {
        let (cx, cy) = (d.line.x + d.line.w / 2, d.line.y + d.line.h / 2);
        if d.dir == Dir::H { Rect::new(cx - 22, cy - 30, 44, 60) } else { Rect::new(cx - 30, cy - 22, 60, 44) }
    }
    /// The switch between the list and the table, left of the expand button of the measurements panel.
    pub(super) fn dock_view_rect(&self, k: VKind, r: Rect) -> Option<Rect> {
        if k != VKind::Measure || !matches!(self.studio.view, StudioView::Docked | StudioView::Floating | StudioView::Offline) { return None; }
        let base = self.dock_extra_rect(k, r).map_or(Self::dock_close_rect(r).x, |e| e.x);
        Some(Rect::new(base - 76, r.y, 76, STRIP_HIT.min(r.h)))
    }
    /// The strip of the measurements panel has its controls in it (wide panels): the content under the strip.
    fn dock_bar_content(&self, k: VKind, r: Rect) -> Option<Rect> {
        if k != VKind::Measure || self.m.layout != Layout::Studio || !matches!(self.studio.view, StudioView::Docked | StudioView::Floating | StudioView::Offline) { return None; }
        let p = Rect::new(r.x, r.y + STRIP_H, r.w, (r.h - STRIP_H).max(1));
        Self::panel_bar_wide(p).then_some(p)
    }
    /// A point on a control of that bar: its hit area reaches 14 px down over the head row of the table.
    fn dock_bar_at(&self, k: VKind, r: Rect, x: i32, y: i32) -> bool {
        self.dock_bar_content(k, r).is_some_and(|p| self.panel_bar_hit(p, x, y).is_some())
    }
    pub(super) fn dock_grip_rect(r: Rect) -> Rect { Rect::new(r.right() - GRIP, r.bottom() - GRIP, GRIP, GRIP) }

    /// The title that fits a narrow strip: the name, else its short form, else the short form with an ellipsis.
    pub(super) fn dock_title_fit(&self, k: VKind, name: &str, avail: i32) -> String {
        let f = self.fonts();
        if f.lb.width(name) <= avail { return name.to_string(); }
        let short = match k {
            VKind::Measure => "Meas".to_string(),
            _ => name.split([' ', '(']).next().unwrap_or(name).to_string(),
        };
        if f.lb.width(&short) <= avail { return short; }
        let mut s: String = short.chars().collect();
        while !s.is_empty() && f.lb.width(&format!("{s}\u{2026}")) > avail { s.pop(); }
        if s.is_empty() { String::new() } else { format!("{s}\u{2026}") }
    }

    pub(super) fn dock_name(&self, k: VKind) -> String {
        match k {
            VKind::Wave => if self.view_on() || self.eye_open() && self.m.eye_place == EyePlace::Full { self.view_name().to_string() } else { "Waveform".to_string() },
            VKind::Measure => if self.studio.view == StudioView::Offline { "Measurements (file)".to_string() } else { "Measurements".to_string() },
            VKind::Eye => "Eye diagram".to_string(),
            VKind::Pane => match self.m.pane {
                tables::PANE_SPECTRUM => "Spectrum",
                tables::PANE_TREND => "Trend",
                tables::PANE_RESULTS => "Results",
                tables::PANE_EVENTS => "Events",
                tables::PANE_BUS => "Bus",
                _ => "Pane",
            }.to_string(),
            VKind::Table => if self.table == Some(tables::TableKind::Search) { "Search events".to_string() } else { "Decode table".to_string() },
            VKind::Sim => "SIM".to_string(),
            VKind::Hist => "Histogram".to_string(),
            VKind::Xy => "XY".to_string(),
            VKind::View => self.view_name().to_string(),
        }
    }

    /// The floating view under a point, the one on top first.
    pub(super) fn dock_float_at(&self, x: i32, y: i32) -> Option<VKind> {
        let z = &self.studio.dock.z;
        let mut order: Vec<VKind> = KINDS.into_iter().filter(|k| !z.contains(k)).collect();
        order.extend(z.iter().copied());
        order.into_iter().rev().find(|k| self.dock_float_rect(*k).is_some_and(|r| r.contains(x, y)))
    }

    fn dock_raise(&mut self, k: VKind) {
        self.studio.dock.z.retain(|z| *z != k);
        self.studio.dock.z.push(k);
    }

    pub(super) fn dock_changed(&mut self) {
        self.studio.dock.ver = self.studio.dock.ver.wrapping_add(1);
        self.relayout();
    }

    // ------------------------------------------------------------------------------------------------ setup

    pub(super) fn dock_text(&self) -> String {
        let d = &self.studio.dock;
        let mut s = d.tree.as_ref().map(Node::text).unwrap_or_default();
        for k in KINDS {
            if let Some(r) = d.float_rect[k.idx()] { s.push_str(&format!(";{}@{}:{}:{}:{}", k.ch(), r.x, r.y, r.w, r.h)); }
        }
        for k in KINDS {
            if d.float_on[k.idx()] && k != VKind::Measure { s.push_str(&format!(";{}!", k.ch())); }
        }
        s
    }

    /// An unreadable or empty text is the default arrangement.
    pub(super) fn dock_restore(&mut self, text: &str) {
        let mut d = Dock::default();
        let mut parts = text.split(';');
        if let Some(t) = parts.next().filter(|t| !t.is_empty()) { d.tree = Node::parse(t); }
        for p in parts {
            let c = p.chars().next().and_then(VKind::from_ch);
            let Some(k) = c.filter(|k| k.floatable()) else { continue };
            if p.ends_with('!') { d.float_on[k.idx()] = k != VKind::Measure; continue; }
            if let Some(v) = p[1..].strip_prefix('@') {
                let n: Vec<i32> = v.split(':').filter_map(|x| x.parse().ok()).collect();
                if n.len() == 4 && n[2] >= FLOAT_MIN_W && n[3] >= FLOAT_MIN_H && n.iter().all(|x| x.abs() < 10_000) { d.float_rect[k.idx()] = Some(Rect::new(n[0], n[1], n[2], n[3])); }
            }
        }
        self.studio.dock = d;
    }

    // ------------------------------------------------------------------------------------------------ overview height

    /// Height of the overview strip: what the user dragged it to, between the box (40 px) and 60 % of the plot.
    pub(super) fn studio_ov_h(&self, g: Rect) -> i32 {
        let h = if self.m.studio_ov == 0 { 96 } else { self.m.studio_ov };
        h.clamp(OV_MIN, (g.h * 6 / 10).max(OV_MIN))
    }

    /// The divider under the overview strip: its 44 px hit area and the handle in the middle of it.
    pub(super) fn ov_divider(&self) -> Option<(Rect, Rect)> {
        if !self.dock_on() || !self.m.zoom || self.view_on() || self.studio.view == StudioView::Only || self.dock_floating(VKind::Wave) { return None; }
        let o = self.split_zoom(self.grid).0;
        let y = o.bottom();
        // the grip stays clear of the buttons of the zoom bar under it
        let bar = self.studio_zoom_bar();
        let busy: Vec<Rect> = bar.map(|b| vec![b.label, b.tdiv, b.plus, b.minus, b.vplus, b.vminus, b.readout, b.center, b.fit, b.more, b.close]).unwrap_or_default();
        let clear = |cx: i32| { let hit = Rect::new(cx - 30, y - 40, 60, 42); !busy.iter().any(|r| r.w > 0 && hit.x < r.right() && r.x < hit.right() && hit.y < r.bottom() && r.y < hit.bottom()) };
        let cx = [o.x + o.w / 2, o.x + o.w * 62 / 100, o.x + o.w * 38 / 100, o.x + o.w * 25 / 100, o.x + o.w * 75 / 100].into_iter().find(|&c| clear(c)).unwrap_or(o.x + o.w / 2);
        // the grip lies in the bottom of the overview strip, above the buttons of the zoom bar
        Some((Rect::new(cx - 30, y - 40, 60, 42), Rect::new(cx - 22, y - 26, 44, 24)))
    }

    pub(super) fn ov_divider_down(&mut self, x: i32, y: i32) -> bool {
        self.studio.ov_drag = None;
        let Some((hit, _)) = self.ov_divider() else { return false };
        if !hit.contains(x, y) { return false; }
        self.studio.ov_drag = Some((y, self.studio_ov_h(self.grid)));
        true
    }

    pub(super) fn ov_divider_move(&mut self, y: i32) -> bool {
        let Some((y0, h0)) = self.studio.ov_drag else { return false };
        let want = h0 + y - y0;
        self.m.studio_ov = self.studio_ov_h_of(want);
        self.relayout();
        true
    }

    fn studio_ov_h_of(&self, want: i32) -> i32 { want.clamp(OV_MIN, (self.grid.h * 6 / 10).max(OV_MIN)) }

    pub(super) fn ov_divider_up(&mut self) -> bool { self.studio.ov_drag.take().is_some() }

    /// Keeps the setup text current: called every frame, it only writes when the arrangement changed.
    pub(super) fn dock_sync_setup(&mut self) {
        if self.studio.dock.synced == Some(self.studio.dock.ver) { return; }
        self.studio.dock.synced = Some(self.studio.dock.ver);
        let t = self.dock_text();
        if self.m.studio_dock != t { self.m.studio_dock = t; }
    }

    /// Back to the default arrangement: no tree, nothing floating.
    pub(super) fn dock_reset(&mut self) {
        let v = self.studio.dock.ver;
        self.studio.dock = Dock::default();
        self.studio.dock.ver = v.wrapping_add(1);
        self.studio.panel.at = None;
        self.relayout();
    }

    // ------------------------------------------------------------------------------------------------ drop targets

    /// Where a view dragged to a point would land, and the rectangle that shows it.
    pub(super) fn dock_drop_at(&self, kind: VKind, x: i32, y: i32, grab: (i32, i32)) -> (Drop, Rect) {
        let lay = self.dock_layout();
        let own = self.dock_rect(kind).unwrap_or_else(|| lay.region);
        let floating = self.dock_floating(kind);
        let reg = self.dock_float_region();
        let float_at = || {
            let r = self.dock_float_rect(kind).unwrap_or_else(|| self.dock_default_float(kind));
            let want = Rect::new(x - grab.0, y - grab.1, if floating { r.w } else { own.w.clamp(FLOAT_MIN_W, 420) }, if floating { r.h } else { own.h.clamp(FLOAT_MIN_H, 320) });
            self.dock_fit_float(kind, want, reg)
        };
        let can_float = self.dock_can_float(kind);
        let leaf_at = lay.leaves.iter().find(|(_, r)| r.contains(x, y)).copied();
        let Some((tk, tr)) = leaf_at.filter(|_| self.studio.view != StudioView::Only) else {
            return if can_float { let r = float_at(); (Drop::Float(r), r) } else { (Drop::Stay, own) };
        };
        let group = self.dock_tab_group(tk);
        let own_group = group.as_ref().is_some_and(|g| g.kinds.contains(&kind));
        // a floating panel over the point takes it: the point is on the panel, not on the view under it
        if tk == kind || own_group {
            // a tab of the group it is in: an edge of the group splits it off at that side, the rest floats it
            let other = group.as_ref().and_then(|g| g.kinds.iter().copied().find(|v| *v != kind));
            return match (drop_zone(tr, x, y), other) {
                (Zone::Edge(s), Some(t)) => {
                    let half = half_of(tr, s);
                    let (rest_w, rest_h) = match s { Side::Left | Side::Right => (tr.w - half.w - GAP, tr.h), _ => (tr.w, tr.h - half.h - GAP) };
                    if rest_w < t.min_w() || rest_h < t.min_h() || half.w < kind.min_w() || half.h < kind.min_h() { (Drop::Stay, own) } else { (Drop::Split(t, s), half) }
                }
                (Zone::Ring | Zone::Centre, _) if can_float => { let r = float_at(); (Drop::Float(r), r) }
                _ => (Drop::Stay, tr),
            };
        }
        // the title bar of another view: into its tabs (the middle of its top edge stays the edge that docks above)
        if y < tr.y + STRIP_H && (x - (tr.x + tr.w / 2)).abs() > tr.w * 15 / 100 && self.dock_can_tab(kind, tk) {
            return (Drop::Tab(tk), tr);
        }
        // a floating view can take the place of a view that can float, not of the waveform
        let swap_ok = !floating || (tk != VKind::Measure || matches!(self.studio.view, StudioView::Docked | StudioView::Floating | StudioView::Offline));
        match drop_zone(tr, x, y) {
            Zone::Edge(s) => {
                // refused when either view would be left smaller than its least size (a short waveform above all)
                let half = half_of(tr, s);
                let (rest_w, rest_h) = match s { Side::Left | Side::Right => (tr.w - half.w - GAP, tr.h), _ => (tr.w, tr.h - half.h - GAP) };
                if rest_w < tk.min_w() || rest_h < tk.min_h() || half.w < kind.min_w() || half.h < kind.min_h() { (Drop::Stay, own) }
                else { (Drop::Split(tk, s), half) }
            }
            Zone::Centre if swap_ok => (Drop::Swap(tk), tr),
            Zone::Centre if can_float => { let r = float_at(); (Drop::Float(r), r) }
            Zone::Centre => (Drop::Stay, own),
            Zone::Ring => if can_float { let r = float_at(); (Drop::Float(r), r) } else { (Drop::Stay, own) },
        }
    }

    /// May `kind` join the tabs of `target`: a group holds seven views at most.
    fn dock_can_tab(&self, kind: VKind, target: VKind) -> bool {
        if self.studio.view == StudioView::Only { return false; }
        if kind == VKind::Measure && !matches!(self.studio.view, StudioView::Docked | StudioView::Floating | StudioView::Offline) { return false; }
        self.dock_tab_group(target).map_or(1, |g| g.kinds.len()) < 7
    }

    /// Does what the preview showed.
    pub(super) fn dock_apply(&mut self, kind: VKind, drop: Drop) {
        let region = self.dock_region();
        let floating = self.dock_floating(kind);
        if drop != Drop::Stay { self.studio.dock.restore = None; }
        match drop {
            Drop::Stay => {}
            Drop::Float(r) => {
                if !self.dock_can_float(kind) { return; }
                // the others rearrange around the hole
                let tree = self.dock_effective(region).without(kind);
                self.studio.dock.tree = tree.or(Some(Node::Leaf(VKind::Wave)));
                self.studio.dock.float_rect[kind.idx()] = Some(r);
                self.dock_set_float(kind, true);
                self.dock_raise(kind);
            }
            Drop::Swap(t) => {
                let tree = self.dock_effective(region);
                if !floating {
                    self.studio.dock.tree = Some(tree.swapped(kind, t));
                } else {
                    // the floating view takes the place of the docked one, which floats where the other was
                    if t == VKind::Measure && !matches!(self.studio.view, StudioView::Docked | StudioView::Floating | StudioView::Offline) { return; }
                    let rect = self.dock_float_rect(kind).unwrap_or_else(|| self.dock_default_float(kind));
                    self.studio.dock.tree = Some(tree.replace(t, &Node::Leaf(kind)));
                    self.dock_set_float(kind, false);
                    self.studio.dock.float_rect[t.idx()] = Some(rect);
                    self.dock_set_float(t, true);
                    self.dock_raise(t);
                }
            }
            Drop::Tab(t) => {
                let mut tree = self.dock_effective(region);
                if !floating { tree = tree.without(kind).unwrap_or(Node::Leaf(t)); }
                self.studio.dock.tree = Some(tree.joined(t, kind));
                if floating { self.dock_set_float(kind, false); }
            }
            Drop::Split(t, side) => {
                let mut tree = self.dock_effective(region);
                if !floating { tree = tree.without(kind).unwrap_or(Node::Leaf(t)); }
                let (dir, first) = match side { Side::Left => (Dir::H, true), Side::Right => (Dir::H, false), Side::Top => (Dir::V, true), Side::Bottom => (Dir::V, false) };
                let (a, b) = if first { (Node::Leaf(kind), Node::Leaf(t)) } else { (Node::Leaf(t), Node::Leaf(kind)) };
                let n = Node::Split { dir, ratio: 0.5, a: Box::new(a), b: Box::new(b) };
                self.studio.dock.tree = Some(tree.replace(t, &n));
                if floating { self.dock_set_float(kind, false); }
            }
        }
        self.dock_changed();
    }

    /// Floats or docks a view; the measurements follow the View menu setting.
    pub(super) fn dock_set_float(&mut self, k: VKind, on: bool) {
        if k == VKind::Measure {
            if on && self.studio.view == StudioView::Docked { self.studio.view = StudioView::Floating; }
            if !on && self.studio.view == StudioView::Floating { self.studio.view = StudioView::Docked; }
            if self.studio.view == StudioView::Offline { self.studio.dock.float_on[k.idx()] = on; }
        } else {
            self.studio.dock.float_on[k.idx()] = on;
        }
    }

    /// The view menu set the measurements to Floating or Docked: the dock follows.
    pub(super) fn dock_view_changed(&mut self) {
        self.studio.dock.ver = self.studio.dock.ver.wrapping_add(1);
    }

    // ------------------------------------------------------------------------------------------------ close

    pub(super) fn dock_close(&mut self, k: VKind) {
        match k {
            VKind::Wave => {
                if self.xy {
                    self.select_option(Item::XyMode, "YT");
                } else if self.m.view != AView::Scope {
                    self.m.view = AView::Scope;
                } else {
                    return self.toast("The waveform stays");
                }
            }
            VKind::Measure | VKind::Sim => self.studio_set_view(StudioView::Waveform),
            VKind::Eye => { if self.m.view == AView::Eye { self.adjust(Item::EyeOn, 1); } }
            VKind::Pane => { self.m.pane = 0; self.m.split_pane = 0; }
            VKind::Table => self.table = None,
            VKind::Hist => self.m.hist = Hist::Off,
            VKind::Xy => { self.xy = false; self.close_menu(); }
            VKind::View => self.view_exit(),
        }
        self.studio.dock.float_on[k.idx()] = false;
        self.dock_changed();
    }

    // ------------------------------------------------------------------------------------------------ touch

    /// A finger went down. True when the dock takes it: a strip, a divider or a resize corner.
    pub(super) fn dock_down(&mut self, x: i32, y: i32) -> bool {
        self.studio.dock.press = None;
        self.studio.dock.drag = None;
        // the list of tabs: a row brings its view to the front, a touch elsewhere closes it
        if let Some(front) = self.studio.dock.tab_list.take() {
            if let Some((v, _)) = self.dock_tab_list_rows(front).into_iter().find(|(_, r)| r.contains(x, y)) { self.dock_activate(v); return true; }
        }
        if self.studio.view == StudioView::Only {
            // one view only: its strip closes it, nothing moves
            if let Some(r) = self.dock_leaf(VKind::Measure) { return self.dock_down_strip(VKind::Measure, r, x, y, false); }
            return false;
        }
        // floating views first, the one on top first
        let z = &self.studio.dock.z;
        let mut order: Vec<VKind> = KINDS.into_iter().filter(|k| !z.contains(k)).collect();
        order.extend(z.iter().copied());
        for k in order.into_iter().rev() {
            let Some(r) = self.dock_float_rect(k) else { continue };
            if !r.contains(x, y) { continue; }
            self.dock_raise(k);
            if let Some(edges) = Self::dock_edges_at(r, x, y) {
                self.studio.dock.drag = Some(DockDrag::Resize { kind: k, x0: x, y0: y, r0: r, edges });
                return true;
            }
            return self.dock_down_strip(k, r, x, y, true);
        }
        let lay = self.dock_layout();
        // the strips (with their buttons) first: a divider's wide hit area reaches into the strip under it, and a
        // finger on a title means the view; the divider keeps the gap and the room above it
        // the handle in the middle of a divider is a target of its own, over the strips beside it
        if let Some(d) = lay.dividers.iter().find(|d| Self::divider_handle_hit(d).contains(x, y)) {
            self.studio.dock.drag = Some(DockDrag::Divider { path: d.path.clone(), dir: d.dir, origin: d.origin, avail: d.avail, lo: d.lo, hi: d.hi });
            return true;
        }
        for (k, r) in lay.leaves.iter() {
            if Self::dock_strip(*r).contains(x, y) || self.dock_bar_at(*k, *r, x, y) { return self.dock_down_strip(*k, *r, x, y, false); }
        }
        if let Some(d) = lay.dividers.iter().find(|d| d.hit.contains(x, y)) {
            self.studio.dock.drag = Some(DockDrag::Divider { path: d.path.clone(), dir: d.dir, origin: d.origin, avail: d.avail, lo: d.lo, hi: d.hi });
            return true;
        }
        false
    }

    fn dock_down_button(&mut self, k: VKind, r: Rect, x: i32, y: i32) -> bool {
        if Self::dock_close_rect(r).contains(x, y) {
            self.studio.dock.press = Some(Press::Button { kind: k, close: true });
            return true;
        }
        if self.dock_extra_rect(k, r).is_some_and(|e| e.contains(x, y)) {
            self.studio.dock.press = Some(Press::Button { kind: k, close: false });
            return true;
        }
        if self.dock_view_rect(k, r).is_some_and(|e| e.contains(x, y)) {
            self.studio.dock.press = Some(Press::View { kind: k });
            return true;
        }
        false
    }

    fn dock_down_strip(&mut self, k: VKind, r: Rect, x: i32, y: i32, floating: bool) -> bool {
        // the zoom controls in the strip of the waveform are the plot's own taps
        if k == VKind::Wave && self.studio_zoom_active() && !self.zoom_tab_mode() && self.zoom_readout().contains(x, y) { return false; }
        if self.dock_bar_at(k, r, x, y) {
            self.studio.dock.press = Some(Press::Bar);
            return true;
        }
        if !Self::dock_strip(r).contains(x, y) { return false; }
        if self.dock_down_button(k, r, x, y) { return true; }
        if !floating && self.dock_tab_more(k, r).is_some_and(|m| m.contains(x, y)) {
            self.studio.dock.tab_list = Some(k);
            self.studio.dock.press = None;
            return true;
        }
        // a tab of a group is the view it names
        let kind = if floating { k } else { self.dock_tab_at(k, r, x, y).unwrap_or(k) };
        self.studio.dock.press = Some(Press::Strip { kind, x0: x, y0: y, x, y, t0: self.now, grab: (x - r.x, y - r.y), free: floating && !Self::dock_handle_rect(r).contains(x, y) });
        true
    }

    /// The finger moved. True when the dock has the touch.
    pub(super) fn dock_move(&mut self, x: i32, y: i32) -> bool {
        if let Some(Press::Strip { kind, x0, y0, t0, grab, free, .. }) = self.studio.dock.press {
            self.studio.dock.press = Some(Press::Strip { kind, x0, y0, x, y, t0, grab, free });
            if (x - x0).abs().max((y - y0).abs()) > DRAG_PX && self.studio.view != StudioView::Only {
                self.dock_begin_drag(kind, x, y, grab, free);
            }
            return true;
        }
        if matches!(self.studio.dock.press, Some(Press::Button { .. } | Press::View { .. } | Press::Bar)) { return true; }
        let Some(d) = self.studio.dock.drag.clone() else { return false };
        match d {
            DockDrag::Move { kind, grab, free, .. } => {
                self.studio.dock.drag = Some(DockDrag::Move { kind, x, y, grab, free });
                if self.dock_floating(kind) {
                    // a floating panel follows the finger at once
                    let reg = self.dock_float_region();
                    let r = self.dock_float_rect(kind).unwrap_or_else(|| self.dock_default_float(kind));
                    let moved = self.dock_fit_float(kind, Rect::new(x - grab.0, y - grab.1, r.w, r.h), reg);
                    self.studio.dock.float_rect[kind.idx()] = Some(moved);
                    self.studio.dock.ver = self.studio.dock.ver.wrapping_add(1);
                }
            }
            DockDrag::Divider { path, dir, origin, avail, lo, hi } => {
                let pos = if dir == Dir::H { x } else { y };
                let a = (pos - origin - GAP / 2).clamp(lo, hi);
                let region = self.dock_region();
                let t = self.dock_effective(region).with_ratio(&path, a as f32 / avail.max(1) as f32);
                self.studio.dock.tree = Some(t);
                self.dock_changed();
            }
            DockDrag::Resize { kind, x0, y0, r0, edges } => {
                let reg = self.dock_float_region();
                let (mw, mh) = kind.float_min();
                let (dx, dy) = (x - x0, y - y0);
                let w = (r0.w + if edges & E_R != 0 { dx } else if edges & E_L != 0 { -dx } else { 0 }).max(mw);
                let h = (r0.h + if edges & E_B != 0 { dy } else { 0 }).max(mh);
                let nx = if edges & E_L != 0 { r0.right() - w } else { r0.x };
                self.studio.dock.float_rect[kind.idx()] = Some(self.dock_fit_float(kind, Rect::new(nx, r0.y, w, h), reg));
                self.dock_changed();
            }
        }
        true
    }

    fn dock_begin_drag(&mut self, kind: VKind, x: i32, y: i32, grab: (i32, i32), free: bool) {
        self.studio.dock.press = None;
        self.studio.dock.drag = Some(DockDrag::Move { kind, x, y, grab, free });
    }

    /// The finger lifted. True when the dock had the touch.
    pub(super) fn dock_up(&mut self, x: i32, y: i32) -> bool {
        let press = self.studio.dock.press.take();
        let drag = self.studio.dock.drag.take();
        match (press, drag) {
            // a floating view dragged by its title only moves: it already follows the finger, nothing docks
            (_, Some(DockDrag::Move { free: true, .. })) => { self.dock_changed(); true }
            (_, Some(DockDrag::Move { kind, grab, .. })) => {
                let (drop, _) = self.dock_drop_at(kind, x, y, grab);
                if self.dock_floating(kind) {
                    // it already follows the finger: only a dock changes anything more
                    if matches!(drop, Drop::Split(..) | Drop::Swap(_) | Drop::Tab(_)) { self.dock_apply(kind, drop); } else { self.dock_changed(); }
                } else {
                    self.dock_apply(kind, drop);
                }
                true
            }
            (_, Some(_)) => true,
            (Some(Press::Button { kind, close }), None) => {
                let r = self.dock_rect(kind);
                let hit = r.is_some_and(|r| if close { Self::dock_close_rect(r).contains(x, y) } else { self.dock_extra_rect(kind, r).is_some_and(|e| e.contains(x, y)) });
                if hit {
                    if close { self.dock_close(kind); } else { self.dock_extra_do(kind); }
                }
                true
            }
            (Some(Press::View { kind }), None) => {
                if let Some(e) = self.dock_rect(kind).and_then(|r| self.dock_view_rect(kind, r)).filter(|e| e.contains(x, y)) {
                    // three segments: Table, List, Cards
                    let k = ((x - e.x) * 3 / e.w.max(1)).clamp(0, 2);
                    self.panel_set_view([super::measpanel::PanelView::Detailed, super::measpanel::PanelView::List, super::measpanel::PanelView::Cards][k as usize]);
                }
                true
            }
            (Some(Press::Bar), None) => { self.panel_strip_tap(x, y); true }
            (Some(Press::Strip { kind, .. }), None) => { self.dock_strip_tap(kind); true }
            (None, None) => false,
        }
    }

    /// The button left of the X.
    fn dock_extra_do(&mut self, k: VKind) {
        match self.dock_extra_kind(k) {
            Some(Extra::Expand) => self.studio_set_view(StudioView::Only),
            Some(Extra::Maximise) => self.dock_maximise(k),
            Some(Extra::RestoreOnly | Extra::RestoreTabs | Extra::RestoreEye) => { self.dock_exit_full(); }
            None => {}
        }
    }

    /// A tap on a strip that did not drag: the tab of a view comes to the front, and a second tap on the same view
    /// puts every view in tabs (or, from a full state, back).
    fn dock_strip_tap(&mut self, k: VKind) {
        let dbl = self.studio.dock.last_tap.is_some_and(|(t, v)| v == k && self.now - t < DOUBLE_TAP);
        if dbl {
            self.studio.dock.last_tap = None;
            if !self.dock_exit_full() { self.dock_maximise(k); }
            return;
        }
        self.studio.dock.last_tap = Some((self.now, k));
        if self.dock_tabbed_out(k) { self.dock_activate(k); }
    }

    /// Every frame: a finger held on a strip for `LONG_PRESS` picks the view up.
    pub(super) fn dock_poll(&mut self) {
        self.dock_normalize();
        self.dock_sync_setup();
        if let Some(Press::Strip { kind, x0, y0, x, y, t0, grab, free }) = self.studio.dock.press {
            if self.now - t0 >= LONG_PRESS && (x - x0).abs().max((y - y0).abs()) <= DRAG_PX && self.studio.view != StudioView::Only {
                self.dock_begin_drag(kind, x, y, grab, free);
            }
        }
    }

    // ------------------------------------------------------------------------------------------------ drawing

    fn draw_dock_strip(&self, cv: &mut Canvas, k: VKind, r: Rect, floating: bool, lifted: bool) {
        let f = self.fonts();
        let s = Self::dock_strip(r);
        cv.fill(s, if lifted { mix(BAR, ACCENT, 18) } else { BAR });
        cv.hline(s.x, s.right() - 1, s.bottom() - 1, LINE);
        // the dotted grip shows that the strip moves the view; on a floating view it is a box of its own: the dock handle
        if floating { chip(cv, Rect::new(s.x + 3, s.y + 2, 24, s.h - 4), if lifted { mix(BTN, ACCENT, 60) } else { BTN }, BTN_BORDER); }
        for (dx, dy) in [(0, 0), (5, 0), (10, 0), (0, 5), (5, 5), (10, 5), (0, 10), (5, 10), (10, 10)] {
            cv.fill(Rect::new(s.x + 8 + dx, s.y + 6 + dy, 2, 2), DIM);
        }
        let name = self.dock_name(k);
        let close = Self::dock_close_rect(r);
        let extra = self.dock_extra_rect(k, r);
        let view = self.dock_view_rect(k, r);
        let right = view.map_or(extra.map_or(close.x, |e| e.x), |v| v.x);
        // the controls of the measurements panel sit in the strip when it is wide: they take the room left of the switch
        let bar = self.dock_bar_content(k, r).map(|p| self.panel_bar(p));
        let right_title = bar.as_ref().map_or(right, |b| b.left - 6);
        let zoom_in_strip = k == VKind::Wave && self.studio_zoom_active() && !self.zoom_tab_mode();
        let right_title = if zoom_in_strip { self.zoom_readout().x - 4 } else { right_title };
        let avail = (right_title - s.x - 30).max(1);
        let tabs = if floating { Vec::new() } else { self.dock_tab_rects(k, r) };
        if bar.as_ref().is_some_and(|b| b.field.is_some()) {
            // the search field replaces the title
        } else if !tabs.is_empty() {
            // the views of the group: the one in front is raised
            for (v, t) in &tabs {
                let on = *v == k;
                let box_ = Rect::new(t.x, t.y + 2, t.w, t.h - 4);
                chip(cv, box_, if on { mix(BTN, ACCENT, 70) } else { BAR }, if on { ACCENT } else { BTN_BORDER });
                let full = self.dock_name(*v);
                let label = if f.mb.width(&full) <= box_.w - 16 { full } else { self.dock_title_fit(*v, &full, (box_.w - 16).max(1)) };
                cv.set_clip(box_);
                cv.text_in(&f.mb, Rect::new(box_.x + 8, box_.y, box_.w - 8, box_.h), 0, &label, if on { TEXT } else { DIM });
                cv.no_clip();
            }
            if let Some(m) = self.dock_tab_more(k, r) {
                let box_ = Rect::new(m.x, m.y + 2, m.w, m.h - 4);
                chip(cv, box_, BTN, BTN_BORDER);
                let (cx, cy) = (box_.x + box_.w / 2, box_.y + box_.h / 2);
                cv.line(cx - 5, cy - 2, cx, cy + 3, TEXT);
                cv.line(cx, cy + 3, cx + 5, cy - 2, TEXT);
            }
        } else {
            let name = self.dock_title_fit(k, &name, avail);
            cv.set_clip(Rect::new(s.x + 26, s.y, avail, s.h));
            cv.text_in(&f.mb, Rect::new(s.x + 26, s.y, avail, s.h), 0, &name, TEXT);
            // a small caption while something filters the list
            let used = f.mb.width(&name) + 12;
            if let Some(cap) = bar.as_ref().and_then(|_| self.panel_caption()) {
                if used + f.s.width(&cap) <= avail {
                    cv.text_in(&f.s, Rect::new(s.x + 26 + used, s.y, avail - used, s.h), 0, &cap, DIM);
                }
            }
            cv.no_clip();
        }
        if let Some(b) = &bar { self.draw_panel_bar(cv, b); }
        if zoom_in_strip { self.draw_studio_zoom_bar(cv); }
        if let Some(v) = view { self.draw_view_switch(cv, v); }
        if let Some(e) = extra {
            chip(cv, Rect::new(e.x + 12, s.y + 1, 26, 24), BTN, BTN_BORDER);
            let (cx, cy) = (e.x + e.w / 2, s.y + s.h / 2);
            match self.dock_extra_kind(k) {
                // two squares, one behind the other: back to where the views were
                Some(Extra::RestoreOnly | Extra::RestoreTabs | Extra::RestoreEye) => {
                    cv.rect(Rect::new(cx - 6, cy - 2, 8, 8), TEXT);
                    cv.rect(Rect::new(cx - 2, cy - 6, 8, 8), TEXT);
                }
                _ => super::studio::icon_expand(cv, cx, cy, TEXT),
            }
        }
        let dim = k == VKind::Wave && !self.view_on() && !(self.eye_open() && self.m.eye_place == EyePlace::Full);
        chip(cv, Rect::new(close.x + 12, s.y + 1, 26, 24), BTN, BTN_BORDER);
        let (cx, cy) = (close.x + close.w / 2, s.y + s.h / 2);
        let c = if dim { DIM } else { TEXT };
        cv.line(cx - 4, cy - 4, cx + 4, cy + 4, c);
        cv.line(cx - 4, cy + 4, cx + 4, cy - 4, c);
        if floating {
            // the resize corner, and the edges beside it
            cv.fill(Rect::new(r.right() - 3, r.y + STRIP_H + 24, 2, (r.h - STRIP_H - 60).max(0)), DIM);
            cv.fill(Rect::new(r.x + 1, r.y + STRIP_H + 24, 2, (r.h - STRIP_H - 60).max(0)), DIM);
            cv.fill(Rect::new(r.x + 24, r.bottom() - 3, (r.w - 70).max(0), 2), DIM);
            let g = Self::dock_grip_rect(r);
            for i in 0..3 { cv.line(g.right() - 8 - i * 5, g.bottom() - 6, g.right() - 6, g.bottom() - 8 - i * 5, DIM); }
        }
    }

    /// The strips of the docked views, the dividers, the floating views' frames and strips, and the drop preview.
    pub(super) fn draw_dock(&self, cv: &mut Canvas) {
        if !self.dock_on() { return; }
        if let Some((_, h)) = self.ov_divider() {
            let on = self.studio.ov_drag.is_some();
            cv.round(h, 5, if on { ACCENT } else { BTN_BORDER }, 255);
            cv.round(h.inset(1), 4, BTN, 255);
            for i in -1..=1 { cv.fill(Rect::new(h.x + h.w / 2 + i * 8 - 1, h.y + h.h / 2 - 1, 2, 2), DIM); }
        }
        let lay = self.dock_layout();
        let moving = match &self.studio.dock.drag { Some(DockDrag::Move { kind, .. }) => Some(*kind), _ => None };
        for d in &lay.dividers { cv.fill(d.line, BG); }
        for (k, r) in &lay.leaves {
            self.draw_dock_strip(cv, *k, *r, false, moving == Some(*k));
            cv.rect(*r, if moving == Some(*k) { ACCENT } else { CARD_BORDER });
        }
        // the divider handles: a small grip in the middle of the 44 px touch area
        let dragging = matches!(self.studio.dock.drag, Some(DockDrag::Divider { .. }));
        for d in &lay.dividers {
            let (cx, cy) = (d.line.x + d.line.w / 2, d.line.y + d.line.h / 2);
            let h = if d.dir == Dir::H { Rect::new(cx - 12, cy - 22, 24, 44) } else { Rect::new(cx - 22, cy - 12, 44, 24) };
            cv.round(h, 5, if dragging { ACCENT } else { BTN_BORDER }, 255);
            cv.round(h.inset(1), 4, BTN, 255);
            for i in -1..=1 {
                if d.dir == Dir::H { cv.fill(Rect::new(cx - 1, cy + i * 8 - 1, 2, 2), DIM); } else { cv.fill(Rect::new(cx + i * 8 - 1, cy - 1, 2, 2), DIM); }
            }
        }
        let z = &self.studio.dock.z;
        let mut order: Vec<VKind> = KINDS.into_iter().filter(|k| !z.contains(k)).collect();
        order.extend(z.iter().copied());
        for k in order {
            let Some(r) = self.dock_float_rect(k) else { continue };
            cv.round_outline(r.inset(-1), 6, BTN_BORDER);
            self.draw_dock_strip(cv, k, r, true, moving == Some(k));
        }
        if let Some(front) = self.studio.dock.tab_list {
            let rows = self.dock_tab_list_rows(front);
            if let Some(f0) = rows.first() {
                let f = self.fonts();
                let h = rows.len() as i32 * 36;
                let p = Rect::new(f0.1.x, f0.1.y, f0.1.w, h);
                cv.round(p.inset(-1), 6, BTN_BORDER, 255);
                cv.round(p, 5, PANEL_BG, 255);
                for (v, rr) in &rows { cv.text_in(&f.mb, Rect::new(rr.x + 10, rr.y, rr.w - 10, rr.h), 0, &self.dock_name(*v), TEXT); }
            }
        }
        if let Some(DockDrag::Move { kind, x, y, grab, free: false }) = &self.studio.dock.drag {
            let (drop, prev) = self.dock_drop_at(*kind, *x, *y, *grab);
            if drop != Drop::Stay {
                cv.blend(prev, ACCENT, 70);
                cv.rect(prev, ACCENT);
                cv.rect(prev.inset(1), ACCENT);
                let label = match drop {
                    Drop::Swap(_) => "Swap",
                    Drop::Float(_) => "Float",
                    Drop::Tab(_) => "Tab",
                    _ => "Dock",
                };
                cv.text_in(&self.fonts().lb, prev, 1, label, TEXT);
            }
            // the picked-up view: its name under the finger
            let f = self.fonts();
            let name = self.dock_name(*kind);
            let w = f.mb.width(&name) + 28;
            let tag = Rect::new((*x - w / 2).clamp(0, self.scr.w - w), (*y - 52).max(0), w, 34);
            cv.round(tag, 6, ACCENT, 255);
            cv.round(tag.inset(1), 5, PANEL_BG, 250);
            cv.text_in(&f.mb, tag, 1, &name, TEXT);
        }
    }
}

impl Ui {
    /// A tap on a floating view: to that view, not to what is under it. Its strip is handled by the touch.
    pub(super) fn dock_float_tap(&mut self, k: VKind, x: i32, y: i32) -> bool {
        let Some(r) = self.dock_float_rect(k) else { return false };
        if Self::dock_strip(r).contains(x, y) { return true; }
        match k {
            VKind::Measure => { self.studio_panel_tap(x, y); }
            VKind::Eye => {
                if !(self.eye_tap(x, y) || self.eye_column_tap(x, y)) && y < r.y + STRIP_H + workspace::HEADER { self.eye_header_tap(x, y); }
            }
            VKind::Pane => { self.pane_tap(x, y); }
            VKind::Table => { self.table_tap(x, y); }
            VKind::Xy | VKind::View => { if self.view_btn().contains(x, y) { self.toggle_menu(MenuId::ViewMore); } }
            _ => {}
        }
        true
    }
}
