// SPDX-License-Identifier: GPL-2.0-or-later
//! SCPI for what a person does with the Studio screen itself: where the views sit and how big they are (`:DOCK`), the
//! plot's right-click menu (`:VIEW`, `:ZOOM`), and the controls on the screen by name and place (`:UI`). A program that
//! drives the scope like a person needs these as much as the settings: it can put the measurement table beside the
//! waveform, drag a divider, float the spectrum, press a button it read off the screen.
//!
//! Every command calls the code a finger reaches: `dock_apply` with the same drops the drag preview offers, the actions of
//! the context menu, and touches through `Ui::handle`. Nothing here keeps state of its own.
use super::dock::{Dir, Drop, Node, Side, VKind, KINDS};
use super::scpiext::{bad, conflict, t, text_arg, Reply, DONE};
use super::studio::{StudioView, ZoomRow, TOP_LABELS};
use super::workspace::Action;
use super::*;
use crate::scpi::{e as en, kw, num};

/// The views by SCPI name; the query answers the short form.
const VIEWS: [(VKind, &str); 9] = [
    (VKind::Wave, "WAVEform"), (VKind::Measure, "MEASurements"), (VKind::Eye, "EYE"), (VKind::Pane, "PANE"), (VKind::Table, "TABLe"),
    (VKind::Sim, "SIM"), (VKind::Hist, "HISTogram"), (VKind::Xy, "XY"), (VKind::View, "VIEW"),
];

/// Headers of this file, for the documentation tests.
#[cfg_attr(not(test), allow(dead_code))] // only the tests use it
pub(super) const LAYOUT_HEADERS: &[&str] = &[
    "DOCK:LIST", "DOCK:MOVE", "DOCK:SIZE", "DOCK:FLOAT", "DOCK:SHOW", "DOCK:CLOSe", "DOCK:ACTivate", "DOCK:MAXimise", "DOCK:RESTore",
    "DOCK:RESet", "DOCK:LAYout",
    "VIEW:TRACes", "VIEW:FIT",
    "ZOOM:HERE", "ZOOM:BOX", "ZOOM:RESet", "ZOOM:CLOSe",
    "UI:SCReen", "UI:ELEMents", "UI:TAP", "UI:DRAG", "UI:CONText", "UI:MENU", "UI:MENU:SELect", "UI:MENU:CLOSe",
];

fn vname(k: VKind) -> &'static str {
    let long = VIEWS.iter().find(|(v, _)| *v == k).map(|(_, n)| *n).unwrap_or("WAVEform");
    match k { VKind::Wave => "WAVE", VKind::Measure => "MEAS", VKind::Table => "TABL", VKind::Hist => "HIST", _ => long }
}

fn view_of(s: &str) -> Option<VKind> {
    let s = text_arg(s);
    VIEWS.iter().find(|(_, n)| kw(s.trim(), n)).map(|(v, _)| *v)
}

/// Comma-separated arguments (a quoted one may hold commas).
fn args(arg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut cur, mut quote) = (String::new(), None);
    for c in arg.chars() {
        match (c, quote) {
            ('"' | '\'', None) => { quote = Some(c); cur.push(c); }
            (c, Some(q)) if c == q => { quote = None; cur.push(c); }
            (',', None) => { out.push(cur.trim().to_string()); cur.clear(); }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() || !out.is_empty() { out.push(cur.trim().to_string()); }
    out
}

fn ints(a: &[String]) -> Option<Vec<i32>> {
    a.iter().map(|s| num(s).filter(|v| v.is_finite() && v.abs() < 100_000.0).map(|v| v.round() as i32)).collect()
}

/// A label as a quoted SCPI string.
fn q(s: &str) -> String { format!("\"{}\"", s.replace('"', "\"\"")) }

fn rect_txt(r: Rect) -> String { format!("{},{},{},{}", r.x, r.y, r.w, r.h) }

/// Where a view is in the tree: false is the first child of a split. A view in a group of tabs: the group's place.
fn path_of(n: &Node, k: VKind) -> Option<Vec<bool>> {
    match n {
        Node::Leaf(l) => (*l == k).then(Vec::new),
        Node::Tabs { kinds, .. } => kinds.contains(&k).then(Vec::new),
        Node::Split { a, b, .. } => {
            if let Some(mut p) = path_of(a, k) { p.insert(0, false); return Some(p); }
            if let Some(mut p) = path_of(b, k) { p.insert(0, true); return Some(p); }
            None
        }
    }
}

fn node_at<'a>(n: &'a Node, path: &[bool]) -> Option<&'a Node> {
    match (n, path.split_first()) {
        (_, None) => Some(n),
        (Node::Split { a, b, .. }, Some((&second, rest))) => node_at(if second { b } else { a }, rest),
        _ => None,
    }
}

/// One control on the screen: what it is, its label, its rectangle.
pub(super) struct Element {
    pub kind: &'static str,
    pub label: String,
    pub rect: Rect,
}

impl Ui {
    /// The entry from `scpi_ext_command`: None = not a header of this file.
    pub(super) fn scpi_layout(&mut self, nodes: &[&str], q: bool, arg: &str) -> Option<Reply> {
        let n0 = *nodes.first()?;
        let rest = &nodes[1..];
        let is = |i: usize, p: &str| rest.get(i).is_some_and(|n| kw(n, p));
        let r = if kw(n0, "DOCK") && rest.len() == 1 {
            self.scpi_dock(rest[0], q, arg)?
        } else if kw(n0, "VIEW") && rest.len() == 1 && (is(0, "TRACes") || is(0, "FIT")) {
            if is(0, "FIT") {
                if q { return Some(Err(bad("FIT is an action"))); }
                self.workspace_action(Action::FitRecord);
                DONE
            } else if q {
                t(if self.workspace.stacked { "STAC" } else { "OVER" })
            } else {
                let a = text_arg(arg);
                if kw(&a, "STACked") { self.workspace_action(Action::Stacked); DONE }
                else if kw(&a, "OVERlay") { self.workspace_action(Action::Overlay); DONE }
                else { Err(bad(arg)) }
            }
        } else if kw(n0, "ZOOM") && rest.len() == 1 {
            self.scpi_zoom(rest[0], q, arg)?
        } else if kw(n0, "UI") && !rest.is_empty() {
            self.scpi_ui(rest, q, arg)?
        } else {
            return None;
        };
        Some(r)
    }

    // ------------------------------------------------------------------------------------------------ :DOCK

    fn scpi_dock(&mut self, node: &str, q: bool, arg: &str) -> Option<Reply> {
        let a = args(arg);
        let key = ["LIST", "MOVE", "SIZE", "FLOAT", "SHOW", "CLOSe", "ACTivate", "MAXimise", "RESTore", "RESet", "LAYout"].into_iter().find(|k| kw(node, k))?;
        if !self.dock_on() { return Some(Err(conflict("the views dock in the Studio layout only (:DISPlay:LAYout STUDio)"))); }
        let view = |i: usize| a.get(i).and_then(|s| view_of(s));
        Some(match key {
            "LIST" => t(self.dock_list()),
            // the setup text separates its parts with ';', which ends a SCPI command even inside quotes: '|' on the wire
            "LAYout" if q => t(q_text(&self.dock_text().replace(';', "|"))),
            "LAYout" => { let s = text_arg(arg).replace('|', ";"); self.dock_restore(&s); self.dock_changed(); DONE }
            "RESet" if !q => { self.dock_reset(); DONE }
            "RESTore" if !q => { if !self.dock_exit_full() { self.dock_restore_layout(); } DONE }
            "MOVE" if !q => match (view(0), a.get(1), a.get(2)) {
                (Some(k), Some(tgt), side) => self.scpi_dock_move(k, tgt, side.map(String::as_str).unwrap_or("CENTer")),
                _ => Err(bad(arg)),
            },
            "SIZE" => match view(0) {
                Some(k) if q => self.dock_size_get(k),
                Some(k) => match a.get(1).and_then(|s| num(s)) {
                    Some(f) if (0.02..=0.98).contains(&f) => self.dock_size_set(k, f as f64),
                    _ => Err(bad(arg)),
                },
                None => Err(bad(arg)),
            },
            "FLOAT" => match view(0) {
                Some(k) if q => t(self.dock_float_rect(k).map_or("0".to_string(), rect_txt)),
                Some(k) => {
                    if !self.dock_shown(k) { return Some(Err(conflict(&format!("{} is not on the screen", vname(k))))); }
                    if !self.dock_can_float(k) { return Some(Err(conflict(&format!("{} is the last docked view", vname(k))))); }
                    let want = match ints(&a[1..]) {
                        Some(v) if v.len() == 4 => Rect::new(v[0], v[1], v[2], v[3]),
                        Some(v) if v.is_empty() => self.dock_float_rect(k).unwrap_or_else(|| self.dock_default_float(k)),
                        _ => return Some(Err(bad(arg))),
                    };
                    let r = self.dock_fit_float(k, want, self.dock_float_region());
                    self.dock_apply(k, Drop::Float(r));
                    DONE
                }
                None => Err(bad(arg)),
            },
            "SHOW" => match view(0) {
                Some(k) if q => t(if self.dock_shown(k) { "1" } else { "0" }),
                Some(VKind::Measure) => { if !self.dock_shown(VKind::Measure) { self.studio_set_view(StudioView::Docked); self.dock_changed(); } DONE }
                Some(VKind::Sim) => { self.studio_set_view(StudioView::Sim); self.dock_changed(); DONE }
                Some(k) if self.dock_shown(k) => DONE,
                Some(k) => Err(conflict(&format!("{} opens with its own command (its menu, :EYE, :VIEW, ...)", vname(k)))),
                None => Err(bad(arg)),
            },
            "CLOSe" if !q => match view(0) {
                Some(k) if self.dock_shown(k) => { self.dock_close(k); DONE }
                Some(_) => DONE,
                None => Err(bad(arg)),
            },
            "ACTivate" if !q => match view(0) {
                Some(k) if self.dock_shown(k) => { self.dock_activate(k); DONE }
                Some(k) => Err(conflict(&format!("{} is not on the screen", vname(k)))),
                None => Err(bad(arg)),
            },
            "MAXimise" => {
                if q { return Some(t(if self.dock_full_state() { "1" } else { "0" })); }
                match view(0) {
                    Some(k) if self.dock_shown(k) => { self.dock_maximise(k); DONE }
                    Some(k) => Err(conflict(&format!("{} is not on the screen", vname(k)))),
                    None => Err(bad(arg)),
                }
            }
            _ => Err(bad(if q { "this command has no query" } else { "this is a query only" })),
        })
    }

    /// Every view: name, state (DOCK, TAB = docked behind another tab, FLOAT, OFF) and rectangle.
    fn dock_list(&self) -> String {
        let lay = self.dock_layout();
        let mut out = Vec::new();
        for k in KINDS {
            if !self.dock_shown(k) { out.push(format!("{},OFF,0,0,0,0", vname(k))); continue; }
            if let Some(r) = self.dock_float_rect(k) { out.push(format!("{},FLOAT,{}", vname(k), rect_txt(r))); continue; }
            if let Some((_, r)) = lay.leaves.iter().find(|(l, _)| *l == k) { out.push(format!("{},DOCK,{}", vname(k), rect_txt(*r))); continue; }
            match lay.tabs.iter().find(|g| g.kinds.contains(&k)) {
                Some(g) => out.push(format!("{},TAB,{}", vname(k), rect_txt(g.rect))),
                None => out.push(format!("{},OFF,0,0,0,0", vname(k))),
            }
        }
        out.join(";")
    }

    fn scpi_dock_move(&mut self, k: VKind, target: &str, side: &str) -> Reply {
        if !self.dock_shown(k) { return Err(conflict(&format!("{} is not on the screen", vname(k)))); }
        let side_of = |s: &str| [(Side::Left, "LEFT"), (Side::Right, "RIGHt"), (Side::Top, "TOP"), (Side::Bottom, "BOTTom")].into_iter().find(|(_, n)| kw(s, n)).map(|(v, _)| v);
        if kw(&text_arg(target), "EDGE") {
            let s = side_of(&text_arg(side)).ok_or_else(|| bad(side))?;
            return self.dock_edge(k, s);
        }
        let tk = view_of(target).ok_or_else(|| bad(target))?;
        if tk == k { return Err(bad("a view cannot dock against itself")); }
        if !self.dock_shown(tk) || self.dock_floating(tk) { return Err(conflict(&format!("{} is not docked on the screen", vname(tk)))); }
        let s = text_arg(side);
        let drop = if let Some(sd) = side_of(&s) { Drop::Split(tk, sd) }
            else if kw(&s, "CENTer") { Drop::Swap(tk) }
            else if kw(&s, "TABS") { Drop::Tab(tk) }
            else { return Err(bad(side)); };
        self.dock_apply(k, drop);
        DONE
    }

    /// A view along a whole edge of the area: the rest of the arrangement keeps its shape beside it.
    fn dock_edge(&mut self, k: VKind, s: Side) -> Reply {
        let region = self.dock_region();
        let floating = self.dock_floating(k);
        let tree = self.dock_effective(region);
        let rest = if floating { Some(tree) } else { tree.without(k) };
        let Some(rest) = rest else { return DONE };
        let (dir, first) = match s { Side::Left => (Dir::H, true), Side::Right => (Dir::H, false), Side::Top => (Dir::V, true), Side::Bottom => (Dir::V, false) };
        // the view gets a third of the room (the waveform two thirds)
        let share = if k == VKind::Wave { 0.66 } else { 0.34 };
        let (a, b, ratio) = if first { (Node::Leaf(k), rest, share) } else { (rest, Node::Leaf(k), 1.0 - share) };
        self.studio.dock.restore = None;
        self.studio.dock.tree = Some(Node::Split { dir, ratio, a: Box::new(a), b: Box::new(b) });
        if floating { self.dock_set_float(k, false); }
        self.dock_changed();
        DONE
    }

    /// The share of its split a docked view (or its group of tabs) has, 0..1 along the split.
    fn dock_size_get(&self, k: VKind) -> Reply {
        if self.dock_floating(k) || !self.dock_shown(k) { return t("0"); }
        let tree = self.dock_effective(self.dock_region());
        let Some(p) = path_of(&tree, k) else { return t("0") };
        let Some((&second, parent)) = p.split_last() else { return t("1") };
        let lay = self.dock_layout();
        let Some(d) = lay.dividers.iter().find(|d| d.path == parent) else { return t("1") };
        let first = if d.dir == Dir::H { d.line.x - d.origin } else { d.line.y - d.origin };
        let f = first as f64 / d.avail.max(1) as f64;
        t(en(if second { 1.0 - f } else { f }))
    }

    /// Moves the divider beside a view: the view gets `f` of the room of its split, as far as the least sizes allow.
    fn dock_size_set(&mut self, k: VKind, f: f64) -> Reply {
        if !self.dock_shown(k) || self.dock_floating(k) { return Err(conflict(&format!("{} is not docked (a floating view takes :DOCK:FLOAT)", vname(k)))); }
        let region = self.dock_region();
        let tree = self.dock_effective(region);
        let Some(p) = path_of(&tree, k) else { return Err(conflict("not in the arrangement")) };
        let Some((&second, parent)) = p.split_last() else { return Err(conflict(&format!("{} fills the area: there is no divider", vname(k)))) };
        let parent = parent.to_vec();
        if !matches!(node_at(&tree, &parent), Some(Node::Split { .. })) { return Err(conflict("no divider")); }
        let lay = self.dock_layout();
        let Some(d) = lay.dividers.iter().find(|d| d.path == parent) else { return Err(conflict("no divider")) };
        let want = if second { 1.0 - f } else { f };
        let a = ((want * d.avail as f64).round() as i32).clamp(d.lo, d.hi);
        let t2 = tree.with_ratio(&parent, a as f32 / d.avail.max(1) as f32);
        self.studio.dock.restore = None;
        self.studio.dock.tree = Some(t2);
        self.dock_changed();
        DONE
    }

    // ------------------------------------------------------------------------------------------------ :ZOOM

    fn scpi_zoom(&mut self, node: &str, q: bool, arg: &str) -> Option<Reply> {
        let key = ["HERE", "BOX", "RESet", "CLOSe"].into_iter().find(|k| kw(node, k))?;
        let scales = tscales();
        Some(match key {
            "HERE" if q => t(en(self.m.zdelay as f64)),
            "HERE" => match num(arg).filter(|v| v.is_finite()) {
                Some(v) => {
                    if self.view_on() { return Some(Err(conflict("not on this view"))); }
                    if !self.m.zoom { self.set_zoom(true); }
                    self.m.zdelay = v;
                    self.clamp_zoom();
                    self.relayout();
                    DONE
                }
                None => Err(bad(arg)),
            },
            "BOX" if q => {
                if !self.m.zoom { return Some(t("0,0")); }
                let half = scales[self.m.zscale] * HDIV / 2.0;
                t(format!("{},{}", en((self.m.zdelay - half) as f64), en((self.m.zdelay + half) as f64)))
            }
            "BOX" => {
                let a = args(arg);
                let v: Option<Vec<f32>> = a.iter().map(|s| num(s).filter(|v| v.is_finite())).collect();
                match v.as_deref() {
                    Some([t0, t1]) if t1 != t0 => {
                        if self.view_on() { return Some(Err(conflict("not on this view"))); }
                        self.zoom_time_box(t0.min(*t1), t0.max(*t1));
                        DONE
                    }
                    _ => Err(bad(arg)),
                }
            }
            "RESet" if !q => { self.workspace_action(Action::ZoomReset); DONE }
            "CLOSe" if !q => { self.workspace_action(Action::ZoomClose); DONE }
            _ => Err(bad("this command has no query")),
        })
    }

    /// The zoom window on a span of time: the finest step that holds it, centred on it (the time half of a drawn box).
    fn zoom_time_box(&mut self, t0: f32, t1: f32) {
        let span = t1 - t0;
        let scales = tscales();
        let mut z = 0;
        while z < self.m.tscale && scales[z] * HDIV < span * 0.999 { z += 1; }
        let on = self.m.zoom;
        if !on { self.zoom_saved = Some((self.m.tscale, self.m.tfine, self.m.delay)); }
        self.zoom_user_closed = false;
        self.m.zscale = z;
        self.m.zdelay = (t0 + t1) * 0.5;
        self.m.zoom = true;
        self.clamp_zoom();
        self.dirty_map = true;
        self.relayout();
    }

    // ------------------------------------------------------------------------------------------------ :UI

    fn scpi_ui(&mut self, rest: &[&str], q: bool, arg: &str) -> Option<Reply> {
        let is = |i: usize, p: &str| rest.get(i).is_some_and(|n| kw(n, p));
        let a = args(arg);
        Some(match rest.len() {
            1 if is(0, "SCReen") => t(format!("{},{}", self.scr.w, self.scr.h)),
            1 if is(0, "ELEMents") => {
                let filter = text_arg(arg).to_ascii_uppercase();
                let all = self.ui_elements();
                t(all.iter().filter(|e| filter.is_empty() || e.kind == filter).map(|e| format!("{},{},{}", e.kind, q_str(&e.label), rect_txt(e.rect))).collect::<Vec<_>>().join(";"))
            }
            1 if is(0, "TAP") && !q => match ints(&a).as_deref() {
                Some([x, y]) => { self.ui_tap(*x, *y); DONE }
                _ => Err(bad(arg)),
            },
            1 if is(0, "DRAG") && !q => match ints(&a).as_deref() {
                Some([x0, y0, x1, y1]) => { self.ui_drag(*x0, *y0, *x1, *y1); DONE }
                _ => Err(bad(arg)),
            },
            1 if is(0, "CONText") && !q => match ints(&a).as_deref() {
                Some([x, y]) => { self.context_menu(*x, *y); DONE }
                _ => Err(bad(arg)),
            },
            1 if is(0, "MENU") && q => t(self.ui_menu_list()),
            2 if is(0, "MENU") && is(1, "SELect") && !q => self.ui_menu_select(&text_arg(arg)),
            2 if is(0, "MENU") && is(1, "CLOSe") && !q => { self.ui_menu_close(); DONE }
            _ => return None,
        })
    }

    pub(super) fn ui_tap(&mut self, x: i32, y: i32) {
        self.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
        self.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
    }

    /// A finger from one point to another in steps, as `drag` of the control commands.
    pub(super) fn ui_drag(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        self.handle(Ev::Touch { id: 0, phase: Phase::Down, x: x0, y: y0 });
        for s in 1..=12 {
            self.handle(Ev::Touch { id: 0, phase: Phase::Move, x: x0 + (x1 - x0) * s / 12, y: y0 + (y1 - y0) * s / 12 });
        }
        self.handle(Ev::Touch { id: 0, phase: Phase::Up, x: x1, y: y1 });
    }

    /// The controls on the screen now, in the order a person reads them: the bars, the views and their strips, the
    /// dividers, then whatever menu is open (it is on top, so it comes last).
    pub(super) fn ui_elements(&self) -> Vec<Element> {
        let mut out = Vec::new();
        let mut push = |kind: &'static str, label: String, rect: Rect| { if rect.w > 0 && rect.h > 0 { out.push(Element { kind, label, rect }); } };
        if self.m.layout == Layout::Studio {
            let top = self.studio_top();
            let l = &TOP_LABELS[top.tier];
            let run = if self.m.run == RunState::Run { "Stop" } else { "Run" };
            for (label, r) in [
                ("Views".to_string(), top.vbox), (l[3].to_string(), top.add), ("Live".to_string(), top.badge), ("SIM".to_string(), top.simbtn),
                (run.to_string(), top.run), ("Single".to_string(), top.single), ("SIM control".to_string(), top.sim), ("Acquire".to_string(), top.acq),
                ("Measure".to_string(), top.measure), (l[5].to_string(), top.cursors), (l[6].to_string(), top.display), (l[7].to_string(), top.storage),
                (l[8].to_string(), top.view), ("Settings".to_string(), top.gear), (l[0].to_string(), top.autoset), (l[1].to_string(), top.clear), (l[2].to_string(), top.force),
            ] { push("TOP", label, r); }
            let b = self.studio_bottom();
            for c in 0..n_ch() { push("BOTTOM", format!("CH{}", c + 1), b.ch[c]); }
            for i in 0..NREF { push("BOTTOM", format!("R{}", i + 1), b.refs[i]); }
            for i in 0..NMATH { push("BOTTOM", format!("M{}", i + 1), b.math[i]); }
            push("BOTTOM", "+ CH".into(), b.add);
            if let Some(r) = b.show { push("BOTTOM", "Show badges".into(), r); }
            if let Some(r) = b.la { push("BOTTOM", "LA".into(), r); }
            if let Some(r) = b.gen { push("BOTTOM", "GEN".into(), r); }
            push("BOTTOM", "Time".into(), b.time);
            push("BOTTOM", "Trigger".into(), b.trig);
            if let Some(r) = b.panel { push("BOTTOM", "Panel".into(), r); }
            // the views: docked, then floating; each with its strip's buttons and tabs
            let lay = self.dock_layout();
            let mut views: Vec<(VKind, Rect)> = lay.leaves.clone();
            for k in KINDS { if let Some(r) = self.dock_float_rect(k) { views.push((k, r)); } }
            for (k, r) in views {
                let name = self.dock_name(k);
                push("VIEW", format!("{} ({})", name, vname(k)), r);
                push("CLOSE", format!("Close {name}"), Self::dock_close_rect(r));
                if let Some(e) = self.dock_extra_rect(k, r) { push("EXPAND", format!("Expand {name}"), e); }
                for (v, tr) in self.dock_tab_rects(k, r) { push("TAB", self.dock_name(v), tr); }
                if self.dock_floating(k) { push("RESIZE", format!("Resize {name}"), Self::dock_grip_rect(r)); }
            }
            for (i, d) in lay.dividers.iter().enumerate() {
                push("DIVIDER", format!("{} {}", if d.dir == Dir::H { "Vertical divider" } else { "Horizontal divider" }, i + 1), d.hit);
            }
            for (key, r) in self.studio_badges() { push("BADGE", format!("{key:?}"), r); }
            for (c, r) in self.cursor_tags() { push("CURSOR", ["A", "B", "C", "D"].get(c).copied().unwrap_or("?").to_string(), r); }
        }
        for (label, r) in self.ui_menu_rows() { push("MENU", label, r); }
        out
    }

    /// The rows of the menu or popup that is open (the one on top), with their rectangles.
    fn ui_menu_rows(&self) -> Vec<(String, Rect)> {
        if self.studio.ctx.is_some() { return self.studio_ctx_rows().into_iter().map(|(_, l, r)| (l, r)).collect(); }
        if let Some(p) = &self.workspace.popup { return p.rows.iter().enumerate().map(|(i, (l, _))| (l.clone(), p.row_rect(i))).collect(); }
        if self.studio.add_menu { return self.studio_add_rows().into_iter().map(|(row, r)| (self.add_row_label(row).to_string(), r)).collect(); }
        if self.studio.zoom_menu {
            return self.studio_zoom_rows().into_iter().map(|(row, r)| (match row {
                ZoomRow::Minus => "Zoom out", ZoomRow::Plus => "Zoom in", ZoomRow::VPlus => "Vertical zoom in", ZoomRow::VMinus => "Vertical zoom out",
                ZoomRow::Tdiv => "Time/div", ZoomRow::Fit => "Fit", ZoomRow::Center => "Center", ZoomRow::ResetY => "Reset vertical", ZoomRow::Close => "Close zoom",
            }.to_string(), r)).collect();
        }
        if self.menu_open && self.menu_is_wide() {
            return self.menu_geo().groups.iter().flat_map(|g| g.items.iter()).map(|(it, r)| (self.item_label(*it).to_string(), *r)).collect();
        }
        Vec::new()
    }

    /// The open menu: its title, then each row as "label" (with its value for a setting of a side menu).
    fn ui_menu_list(&self) -> String {
        let rows = self.ui_menu_rows();
        let title = if self.studio.ctx.is_some() { "Context".to_string() }
            else if let Some(p) = &self.workspace.popup { p.title.clone() }
            else if self.studio.add_menu { "Add".to_string() }
            else if self.studio.zoom_menu { "Zoom".to_string() }
            else if self.menu_open { self.menu_title(self.menu).to_string() }
            else { return "\"\"".to_string(); };
        let mut out = vec![q_str(&title)];
        if !rows.is_empty() {
            out.extend(rows.into_iter().map(|(l, _)| q_str(&l)));
        } else if self.menu_open {
            for it in self.items(self.menu) {
                let v = self.value(it);
                out.push(q_str(&if v.is_empty() { self.item_label(it).to_string() } else { format!("{} = {}", self.item_label(it), v) }));
            }
        }
        out.join(",")
    }

    /// Presses the row of the open menu with that label (exactly, else the one label that starts with it).
    fn ui_menu_select(&mut self, label: &str) -> Reply {
        let want = label.trim().to_ascii_lowercase();
        if want.is_empty() { return Err(bad(label)); }
        let rows = self.ui_menu_rows();
        if !rows.is_empty() {
            let exact = rows.iter().find(|(l, _)| l.to_ascii_lowercase() == want);
            let starts: Vec<&(String, Rect)> = rows.iter().filter(|(l, _)| l.to_ascii_lowercase().starts_with(&want)).collect();
            let hit = exact.or(if starts.len() == 1 { Some(starts[0]) } else { None });
            return match hit {
                Some((_, r)) => { let (x, y) = (r.x + r.w / 2, r.y + r.h / 2); self.ui_tap(x, y); DONE }
                None if starts.len() > 1 => Err(bad(&format!("{label}: {} rows start so", starts.len()))),
                None => Err(bad(&format!("no row {label:?} in the open menu"))),
            };
        }
        if self.menu_open { return self.script_do(label).map(|_| None).map_err(|e| bad(&e)); }
        Err(conflict("no menu is open"))
    }

    fn ui_menu_close(&mut self) {
        self.studio.ctx = None;
        self.workspace.popup = None;
        self.studio.add_menu = false;
        self.studio.zoom_menu = false;
        if self.menu_open { self.close_menu(); }
        self.relayout();
    }
}

/// A label as a SCPI string.
fn q_str(s: &str) -> String { q(s) }
/// The setup text of the arrangement, quoted.
fn q_text(s: &str) -> String { q(s) }

#[cfg(test)]
mod tests {
    use super::*;

    fn studio() -> Ui {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.layout = Layout::Studio;
        u.studio_set_view(StudioView::Docked);
        u.relayout();
        u
    }

    fn cmd(u: &mut Ui, h: &str, arg: &str) -> Reply {
        let q = h.ends_with('?');
        let nodes: Vec<&str> = h.trim_end_matches('?').trim_start_matches(':').split(':').collect();
        u.scpi_ext_command(&nodes, q, arg).expect("a header of this file")
    }

    fn text(r: Reply) -> String { String::from_utf8(r.unwrap().unwrap()).unwrap() }

    fn rect_of(u: &Ui, k: VKind) -> Rect { u.dock_rect(k).expect("on the screen") }

    #[test]
    fn move_puts_the_measurements_on_each_side_of_the_waveform() {
        let mut u = studio();
        for (side, check) in [
            ("LEFT", (|m: Rect, w: Rect| m.right() <= w.x) as fn(Rect, Rect) -> bool),
            ("RIGHT", |m, w| m.x >= w.right()),
            ("TOP", |m, w| m.bottom() <= w.y),
            ("BOTTOM", |m, w| m.y >= w.bottom()),
        ] {
            cmd(&mut u, ":DOCK:MOVE", &format!("MEAS,WAVE,{side}")).unwrap();
            let (m, w) = (rect_of(&u, VKind::Measure), rect_of(&u, VKind::Wave));
            assert!(check(m, w), "{side}: meas {m:?} wave {w:?}");
            let list = text(cmd(&mut u, ":DOCK:LIST?", ""));
            assert!(list.contains("MEAS,DOCK,") && list.contains("WAVE,DOCK,"), "{list}");
        }
    }

    #[test]
    fn size_moves_the_divider_and_reads_back() {
        let mut u = studio();
        cmd(&mut u, ":DOCK:MOVE", "MEAS,WAVE,RIGHT").unwrap();
        cmd(&mut u, ":DOCK:SIZE", "MEAS,0.4").unwrap();
        let f: f64 = text(cmd(&mut u, ":DOCK:SIZE?", "MEAS")).parse().unwrap();
        assert!((f - 0.4).abs() < 0.02, "{f}");
        let (m, w) = (rect_of(&u, VKind::Measure), rect_of(&u, VKind::Wave));
        assert!(m.w < w.w && m.w > 200, "{m:?} {w:?}");
        let g: f64 = text(cmd(&mut u, ":DOCK:SIZE?", "WAVE")).parse().unwrap();
        assert!((f + g - 1.0).abs() < 0.02);
        // beyond the least size of the waveform: limited, not refused
        cmd(&mut u, ":DOCK:SIZE", "MEAS,0.95").unwrap();
        assert!(rect_of(&u, VKind::Wave).w >= 300);
    }

    #[test]
    fn float_tabs_edge_and_layout_round_trip() {
        let mut u = studio();
        cmd(&mut u, ":DOCK:FLOAT", "MEAS,100,120,400,300").unwrap();
        let r = text(cmd(&mut u, ":DOCK:FLOAT?", "MEAS"));
        assert!(u.dock_floating(VKind::Measure), "{r}");
        assert!(text(cmd(&mut u, ":DOCK:LIST?", "")).contains("MEAS,FLOAT,"));
        cmd(&mut u, ":DOCK:MOVE", "MEAS,WAVE,TABS").unwrap();
        assert!(!u.dock_floating(VKind::Measure));
        assert!(text(cmd(&mut u, ":DOCK:LIST?", "")).contains(",TAB,"));
        cmd(&mut u, ":DOCK:ACTivate", "WAVE").unwrap();
        assert!(text(cmd(&mut u, ":DOCK:LIST?", "")).contains("WAVE,DOCK,"));
        cmd(&mut u, ":DOCK:MOVE", "MEAS,EDGE,BOTTOM").unwrap();
        assert!(rect_of(&u, VKind::Measure).y >= rect_of(&u, VKind::Wave).bottom());
        let saved = text(cmd(&mut u, ":DOCK:LAYout?", ""));
        cmd(&mut u, ":DOCK:MOVE", "MEAS,WAVE,LEFT").unwrap();
        cmd(&mut u, ":DOCK:LAYout", &saved).unwrap();
        assert!(rect_of(&u, VKind::Measure).y >= rect_of(&u, VKind::Wave).bottom());
        assert!(cmd(&mut u, ":DOCK:MOVE", "MEAS,MEAS,LEFT").is_err());
        assert!(cmd(&mut u, ":DOCK:MOVE", "MEAS,WAVE,SIDEWAYS").is_err());
        assert!(cmd(&mut u, ":DOCK:MOVE", "EYE,WAVE,LEFT").is_err(), "the eye is off");
    }

    #[test]
    fn maximise_and_restore() {
        let mut u = studio();
        cmd(&mut u, ":DOCK:MOVE", "MEAS,WAVE,RIGHT").unwrap();
        cmd(&mut u, ":DOCK:MAXimise", "MEAS").unwrap();
        assert_eq!(text(cmd(&mut u, ":DOCK:MAXimise?", "")), "1");
        cmd(&mut u, ":DOCK:RESTore", "").unwrap();
        assert_eq!(text(cmd(&mut u, ":DOCK:MAXimise?", "")), "0");
        assert!(rect_of(&u, VKind::Measure).x >= rect_of(&u, VKind::Wave).right());
    }

    #[test]
    fn zoom_box_here_and_traces() {
        let mut u = studio();
        cmd(&mut u, ":ZOOM:BOX", "-1e-6,1e-6").unwrap();
        assert!(u.m.zoom);
        let b = text(cmd(&mut u, ":ZOOM:BOX?", ""));
        let v: Vec<f64> = b.split(',').map(|s| s.parse().unwrap()).collect();
        assert!(v[0] <= -1e-6 * 0.999 && v[1] >= 1e-6 * 0.999 && v[1] - v[0] < 2e-6 * 3.0, "{b}");
        cmd(&mut u, ":ZOOM:HERE", "5e-7").unwrap();
        let h: f64 = text(cmd(&mut u, ":ZOOM:HERE?", "")).parse().unwrap();
        assert!((h - 5e-7).abs() < 1e-9, "{h}");
        cmd(&mut u, ":ZOOM:CLOSe", "").unwrap();
        assert!(!u.m.zoom);
        cmd(&mut u, ":VIEW:TRACes", "STACked").unwrap();
        assert_eq!(text(cmd(&mut u, ":VIEW:TRACes?", "")), "STAC");
        cmd(&mut u, ":VIEW:TRACes", "OVER").unwrap();
        assert_eq!(text(cmd(&mut u, ":VIEW:TRACes?", "")), "OVER");
    }

    #[test]
    fn elements_list_the_bars_and_views_and_the_context_menu_by_name() {
        let mut u = studio();
        let e = text(cmd(&mut u, ":UI:ELEMents?", ""));
        assert!(e.contains("TOP,\"Single\","), "{e}");
        assert!(e.contains("BOTTOM,\"CH1\","), "{e}");
        assert!(e.contains("VIEW,\"Waveform (WAVE)\","), "{e}");
        let only_top = text(cmd(&mut u, ":UI:ELEMents?", "TOP"));
        assert!(only_top.split(';').all(|r| r.starts_with("TOP,")));
        // right-click on the plot, then a row by name
        let a = u.area;
        cmd(&mut u, ":UI:CONText", &format!("{},{}", a.x + a.w / 2, a.y + a.h / 2)).unwrap();
        let m = text(cmd(&mut u, ":UI:MENU?", ""));
        assert!(m.starts_with("\"Context\"") && m.contains("\"Stacked traces\""), "{m}");
        assert!(text(cmd(&mut u, ":UI:ELEMents?", "MENU")).contains("MENU,\"Stacked traces\","));
        cmd(&mut u, ":UI:MENU:SELect", "\"Stacked traces\"").unwrap();
        assert!(u.workspace.stacked);
        assert!(u.studio.ctx.is_none());
        assert!(cmd(&mut u, ":UI:MENU:SELect", "\"Stacked traces\"").is_err(), "no menu open");
    }

    #[test]
    fn a_tap_on_a_listed_element_does_what_a_finger_does() {
        let mut u = studio();
        let e = text(cmd(&mut u, ":UI:ELEMents?", "TOP"));
        let display = e.split(';').find(|r| r.starts_with("TOP,\"Display\",")).expect("Display listed");
        let n: Vec<i32> = display.split(',').skip(2).map(|s| s.parse().unwrap()).collect();
        assert!(!u.menu_open);
        cmd(&mut u, ":UI:TAP", &format!("{},{}", n[0] + n[2] / 2, n[1] + n[3] / 2)).unwrap();
        assert!(u.menu_open && u.menu == MenuId::Display, "{:?}", u.menu);
        // the menu that opened is listed by name, and closes
        assert!(text(cmd(&mut u, ":UI:MENU?", "")).starts_with('"'));
        cmd(&mut u, ":UI:MENU:CLOSe", "").unwrap();
        assert!(!u.menu_open);
    }

    #[test]
    fn a_drag_on_a_divider_resizes_like_size() {
        let mut u = studio();
        cmd(&mut u, ":DOCK:MOVE", "MEAS,WAVE,RIGHT").unwrap();
        let before = rect_of(&u, VKind::Measure).w;
        let d = u.dock_layout().dividers[0].clone();
        let h = Ui::divider_handle_hit(&d);
        let (x, y) = (h.x + h.w / 2, h.y + h.h / 2);
        cmd(&mut u, ":UI:DRAG", &format!("{x},{y},{},{y}", x - 120)).unwrap();
        let after = rect_of(&u, VKind::Measure).w;
        assert!(after > before + 80, "{before} -> {after}");
    }

    #[test]
    fn the_headers_are_answered_and_the_rest_left_alone() {
        let mut u = studio();
        for h in LAYOUT_HEADERS {
            let nodes: Vec<&str> = h.split(':').collect();
            assert!(u.scpi_layout(&nodes, true, "WAVE").is_some() || u.scpi_layout(&nodes, false, "").is_some(), "{h}");
        }
        assert!(u.scpi_layout(&["VIEW"], false, "SCOPe").is_none(), ":VIEW <analyser> stays with scpi.rs");
        assert!(u.scpi_layout(&["CHANnel1", "SCALe"], false, "1").is_none());
    }

    #[test]
    fn every_header_has_a_row_in_the_layout_page() {
        let page = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/scpi/layout.md")).unwrap();
        for h in LAYOUT_HEADERS { assert!(page.contains(&format!("| `:{h}` |")), "{h} has no row in docs/scpi/layout.md"); }
    }
}
