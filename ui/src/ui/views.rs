// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Display > Views: up to four independent workspaces of the one scope to switch between. There is one scope, one
//! acquisition and one set of stored data, so those are shared; everything that is only how it is looked at and
//! analysed belongs to the view (Batch 33).
//!
//! SHARED by every view (the instrument): the channels' on/off and vertical settings, the time base, the trigger,
//! the acquisition (mode, run state), the stored data of the references, the definitions of math and bus channels,
//! the shared measurement set-up of Measure (reference levels, thresholds), the layout skin and the text size.
//!
//! PER VIEW (restored on every switch, kept in the setup as `view<n>`):
//! * the measurement list: the items (`meas`, `meas2`, `cat`), their sources, per-row settings, the statistics
//!   switches and the gate, and what is pinned or floated from it;
//! * the panels: the Studio view (waveform / docked / floating / only), the docking tree and the floating panels
//!   with their place and size, the measurement panel's mode (list, detailed, cards, statistics, search text,
//!   source filter, folded), the zoom overview height, the pinned-badge row, the second pane, the windows, the DVM,
//!   the counter, the analyser page and XY, full screen;
//! * the cursors: mode, positions, the source of the second cursor, the readout place, linked;
//! * the zoom: on, window and delay, the one source that zooms, the horizontal scale of each reference, the
//!   display magnification (Y zoom);
//! * the lanes shown: which references and whether the bus are shown (a reference's samples stay shared);
//! * the analysis set-ups of the page shown: histogram, spectrum, eye, search.
//! Not part of a view: a record session (SIM, a loaded file) with its panel stays as it is when the view changes, and
//! the visibility of a math channel is its definition, so it is shared.
//!
//! A view added by raising the count starts as a copy of the view shown (Batch 30); lowering the count keeps the
//! content of the view shown. The views are part of a setup (`views`, `view1` ... `view4`, `studio_state`); a setup
//! from before has no slots and every other view starts as a copy of the one shown. The measurement SCPI commands
//! act on the list of the view shown, `:DISPlay:VIEW` changes which that is.
//!
//! Switching: the numbered buttons in the top bar, a swipe from the left or right edge of the plot inwards (only
//! while there is more than one view; a vertical drag there still moves a channel or the trigger level), the Quick
//! key set to "Next view", SCPI `:DISPlay:VIEW`.

use super::studio::{MeasKey, StudioView};
use super::workspace::Trace;
use super::*;
use crate::store;

/// The most views.
pub const VIEWS_MAX: usize = 4;
/// Keys of a setup (store.rs) that belong to a view, besides the ones of the Ui (`xy`, `full`, `studio_view`,
/// `studio_dock`, `studio_state`, `lanes`).
const KEYS: [&str; 42] = [
    "windows", "density", "split_pane", "ch_window", "math_window", "la_window", "ref_window", "meas", "meas2", "cat", "row_cfg", "meas_stat", "meas_full_record", "dvm", "counter", "cursor", "cur_a", "cur_b", "cur_c", "cur_d",
    "cur_b_source", "pane", "meas_labels", "trend_sel", "strip", "studio_ov", "studio_meas_view", "studio_badges", "zoom", "zoom_tdiv", "zdelay", "hist", "view", "spec", "spec_peaks", "spec_acpr", "eye", "eye_place", "eye_more", "eye_pll", "math_hidden", "gen_hidden",
];
/// The strip at each edge of the plot where a horizontal swipe switches views (px).
pub(super) const EDGE: i32 = 18;
/// How far the swipe has to go (px).
const SWIPE: i32 = 70;
/// Width of a number in the top bar's view buttons.
const NUM_W: i32 = 28;
/// The same in the Studio top bar (chips of the bar height, smaller numbers).
const NUM_W_STUDIO: i32 = 26;

/// The width of the Studio top bar's view segment for `views` views (0 with fewer than two).
pub(super) fn studio_box_w(views: usize) -> i32 {
    if views < 2 { 0 } else { NUM_W_STUDIO * views as i32 + 2 }
}

/// The keys of a view that are not a key of the model: they come from the Ui.
const UI_KEYS: [&str; 7] = ["xy", "full", "studio_view", "studio_dock", "studio_state", "lanes", "math_def"];

fn known(k: &str) -> bool {
    KEYS.contains(&k) || UI_KEYS.contains(&k)
}

/// `;` separates the keys of a slot, so a value that holds one (the docking text does) is written escaped.
pub(super) fn esc(v: &str) -> String {
    v.replace('%', "%25").replace(';', "%3B")
}

pub(super) fn unesc(v: &str) -> String {
    v.replace("%3B", ";").replace("%25", "%")
}

/// A view as kept in a setup: `key=value; key=value ...` of its keys only.
pub fn slot_checked(v: &str) -> Result<String, String> {
    for part in v.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let k = part.split_once('=').map(|(k, _)| k.trim()).ok_or_else(|| format!("not key=value: {}", part))?;
        if !known(k) {
            return Err(format!("not part of a view: {}", k));
        }
    }
    Ok(v.trim().to_string())
}

/// The value of `key` in a slot's text.
fn slot_get(slot: &str, key: &str) -> Option<String> {
    slot.split(';').map(str::trim).find_map(|p| p.split_once('=').filter(|(k, _)| k.trim() == key).map(|(_, v)| unesc(v.trim())))
}

fn key_text(k: MeasKey) -> String {
    match k {
        MeasKey::One(m, id) => format!("o/{:?}/{}", m, id.token()),
        MeasKey::Pair(i, a, b) => format!("p/{}/{}/{}", i, a.token(), b.token()),
        MeasKey::Cat(i, a, b, c, d) => format!("c/{}/{}/{}/{}/{}", i, a.token(), b.token(), c.token(), d.token()),
    }
}

fn key_parse(s: &str) -> Option<MeasKey> {
    let f: Vec<&str> = s.split('/').collect();
    match f[..] {
        ["o", m, id] => Some(MeasKey::One(store::parse_meas(m).ok()?, WaveId::parse(id).ok()?)),
        ["p", i, a, b] => Some(MeasKey::Pair(i.parse().ok()?, WaveId::parse(a).ok()?, WaveId::parse(b).ok()?)),
        ["c", i, a, b, c, d] => Some(MeasKey::Cat(i.parse().ok()?, WaveId::parse(a).ok()?, WaveId::parse(b).ok()?, WaveId::parse(c).ok()?, WaveId::parse(d).ok()?)),
        _ => None,
    }
}

fn keys_text(v: &[MeasKey]) -> String {
    v.iter().map(|k| key_text(*k)).collect::<Vec<_>>().join(",")
}

fn keys_parse(s: &str) -> Vec<MeasKey> {
    s.split(',').filter_map(key_parse).collect()
}

fn trace_parse(s: &str) -> Option<Trace> {
    let n = |r: &str, max: usize| r.parse::<usize>().ok().filter(|n| (1..=max).contains(n)).map(|n| n - 1);
    if let Some(r) = s.strip_prefix("CH") { return n(r, NCH).map(Trace::Channel); }
    if let Some(r) = s.strip_prefix('R') { return n(r, NREF).map(Trace::Reference); }
    s.strip_prefix('M').and_then(|r| n(r, NMATH)).map(Trace::Math)
}

/// The view of a model, the Ui's XY and full screen, and what the Ui keeps (`ui`: the Studio view, the docking text,
/// the state text and the lanes shown).
fn snapshot_of(m: &Model, xy: bool, full: bool, ui: &[(&str, String)]) -> String {
    let text = store::setup_text(m);
    let mut parts: Vec<String> = text.lines().filter_map(|l| l.split_once(" = ")).filter(|(k, _)| KEYS.contains(k)).map(|(k, v)| format!("{}={}", k, esc(v))).collect();
    parts.push(format!("xy={}", xy));
    parts.push(format!("full={}", full));
    parts.extend(ui.iter().map(|(k, v)| format!("{}={}", k, esc(v))));
    parts.join("; ")
}

impl Ui {
    /// What the Ui keeps of the view shown: the pinned measurements, the panel, the search, the zoom source and the
    /// like, as `key=value` words (`m.studio_state`, and in a view's slot).
    pub(super) fn view_ui_text(&self) -> String {
        let s = &self.studio;
        let mut p: Vec<String> = Vec::new();
        if let Some(pin) = &s.pinned { p.push(format!("pin={}", keys_text(pin))); }
        if !s.unpinned.is_empty() { p.push(format!("unpin={}", keys_text(&s.unpinned))); }
        if !s.floated.is_empty() { p.push(format!("float={}", keys_text(&s.floated))); }
        for (on, k) in [(s.hide_badges, "hide"), (s.compact, "compact"), (s.cur_badge, "curbadge"), (s.cur_linked, "curlink"), (s.eye_full, "eyefull"), (s.overlay, "overlay")] {
            if on { p.push(format!("{}=1", k)); }
        }
        let pa = &s.panel;
        if pa.collapsed || pa.stats || pa.cards || pa.pinned_only || pa.stats_mode != 0 || pa.hx != 0 {
            p.push(format!("panel={}/{}/{}/{}/{}/{}", pa.stats_mode, pa.cards as u8, pa.pinned_only as u8, pa.collapsed as u8, pa.stats as u8, pa.hx));
        }
        if let Some((x, y)) = pa.at { p.push(format!("panelat={}/{}", x, y)); }
        if pa.first != 0 || pa.sub != 0 { p.push(format!("scroll={}/{}", pa.first, pa.sub)); }
        if s.zoom_open.is_some() { p.push("zoomtab=1".into()); }
        if !pa.query.is_empty() { p.push(format!("query={}", pa.query.replace(' ', "_"))); }
        if let Some(f) = s.filter { p.push(format!("filter={}", f.token())); }
        if let Some(q) = &s.search { p.push(format!("search={}/{}/{}", q.src.token(), q.level, q.slope)); }
        if let Some(t) = s.sz.lane { p.push(format!("zlane={}", super::sharedzoom::trace_name(t))); }
        if !s.sz.ref_h.is_empty() {
            p.push(format!("refh={}", s.sz.ref_h.iter().map(|(i, (t, d))| format!("{}/{}/{}", i, t, d)).collect::<Vec<_>>().join(",")));
        }
        if self.zoom_vfactor != 1.0 || self.zoom_vcenter != 0.0 { p.push(format!("yzoom={}/{}", self.zoom_vfactor, self.zoom_vcenter)); }
        let w = &self.workspace;
        if w.stacked { p.push("stacked=1".into()); }
        if w.measure_docked { p.push("mdock=1".into()); }
        if let Some((x, y)) = w.measure_float { p.push(format!("mfloat={}/{}", x, y)); }
        if let Some(r) = w.eye_float { p.push(format!("efloat={}/{}/{}/{}", r.x, r.y, r.w, r.h)); }
        if w.eye_left { p.push("eleft=1".into()); }
        p.join(" ")
    }

    /// Shows what `text` (`view_ui_text`) says: every part not named goes back to its default.
    pub(super) fn view_ui_apply(&mut self, text: &str) {
        let num = |v: &str| -> Vec<i32> { v.split('/').filter_map(|x| x.parse().ok()).collect() };
        self.studio.pinned = None;
        self.studio.unpinned.clear();
        self.studio.floated.clear();
        self.studio.hide_badges = false;
        self.studio.compact = false;
        self.studio.cur_badge = false;
        self.studio.cur_linked = false;
        self.studio.overlay = false;
        self.studio.eye_full = false;
        self.studio.panel = Default::default();
        self.studio.zoom_open = None;
        self.studio.zoom_pull = None;
        self.studio.add_menu = false;
        self.studio.ctx = None;
        self.studio.filter = None;
        self.studio.search = None;
        self.studio.search_at = 0;
        self.studio.popup = None;
        self.studio.sz = Default::default();
        self.studio.badge_first = 0;
        self.zoom_vfactor = 1.0;
        self.zoom_vcenter = 0.0;
        self.workspace.stacked = false;
        self.workspace.measure_docked = false;
        self.workspace.measure_float = None;
        self.workspace.eye_float = None;
        self.workspace.eye_left = false;
        for w in text.split_whitespace() {
            let Some((k, v)) = w.split_once('=') else { continue };
            match k {
                "pin" => self.studio.pinned = Some(keys_parse(v)),
                "unpin" => self.studio.unpinned = keys_parse(v),
                "float" => self.studio.floated = keys_parse(v),
                "hide" => self.studio.hide_badges = v == "1",
                "compact" => self.studio.compact = v == "1",
                "curbadge" => self.studio.cur_badge = v == "1",
                "curlink" => self.studio.cur_linked = v == "1",
                "overlay" => self.studio.overlay = v == "1",
                "eyefull" => self.studio.eye_full = v == "1",
                "panel" => {
                    let n = num(v);
                    if let [mode, cards, pinned, folded, stats, hx] = n[..] {
                        let p = &mut self.studio.panel;
                        (p.stats_mode, p.cards, p.pinned_only, p.collapsed, p.stats, p.hx) = (mode.clamp(0, 2) as u8, cards != 0, pinned != 0, folded != 0, stats != 0, hx.clamp(0, 10_000));
                    }
                }
                "panelat" => if let [x, y] = num(v)[..] { self.studio.panel.at = Some((x, y)); },
                "scroll" => if let [first, sub] = num(v)[..] { self.studio.panel.first = first.clamp(0, 100_000) as usize; self.studio.panel.sub = sub.clamp(0, 1000); },
                "zoomtab" => self.studio.zoom_open = (v == "1").then_some(self.now),
                "query" => { self.studio.panel.query = v.replace('_', " "); self.studio.panel.searching = !self.studio.panel.query.is_empty(); }
                "filter" => self.studio.filter = WaveId::parse(v).ok(),
                "search" => {
                    let f: Vec<&str> = v.split('/').collect();
                    if let [src, level, slope] = f[..] {
                        if let (Ok(src), Ok(level), Ok(slope)) = (WaveId::parse(src), level.parse::<f32>(), slope.parse::<u8>()) {
                            if level.is_finite() { self.studio.search = Some(super::addrail::SearchSpec { src, level, slope: slope.min(2) }); }
                        }
                    }
                }
                "zlane" => self.studio.sz.lane = trace_parse(v),
                "refh" => {
                    for e in v.split(',') {
                        let f: Vec<&str> = e.split('/').collect();
                        if let [i, t, d] = f[..] {
                            if let (Ok(i), Ok(t), Ok(d)) = (i.parse::<usize>(), t.parse::<f32>(), d.parse::<f32>()) {
                                if i < NREF && t.is_finite() && d.is_finite() && t > 0.0 { self.studio.sz.ref_h.insert(i, (t, d)); }
                            }
                        }
                    }
                }
                "yzoom" => {
                    let f: Vec<&str> = v.split('/').collect();
                    if let [a, b] = f[..] {
                        if let (Ok(a), Ok(b)) = (a.parse::<f32>(), b.parse::<f32>()) {
                            if a.is_finite() && b.is_finite() { (self.zoom_vfactor, self.zoom_vcenter) = (a.clamp(1.0, 1024.0), b); }
                        }
                    }
                }
                "stacked" => self.workspace.stacked = v == "1",
                "mdock" => self.workspace.measure_docked = v == "1",
                "mfloat" => if let [x, y] = num(v)[..] { self.workspace.measure_float = Some((x, y)); },
                "efloat" => if let [x, y, w, h] = num(v)[..] { if w > 0 && h > 0 { self.workspace.eye_float = Some(Rect::new(x, y, w, h)); } },
                "eleft" => self.workspace.eye_left = v == "1",
                _ => {}
            }
        }
    }

    /// The references and the bus shown, for a view's slot: `R1 R3 B`. None while a record session owns the refs.
    fn view_lanes_text(&self) -> Option<String> {
        if self.record_mode() { return None; }
        let mut v: Vec<String> = (0..NREF).filter(|&i| self.refs[i].as_ref().is_some_and(|r| r.on)).map(|i| format!("R{}", i + 1)).collect();
        if self.m.bus.on { v.push("B".into()); }
        Some(v.join(" "))
    }

    /// Whether math channel `k` is shown in view `j` (0-based) as that view's slot says; a slot from before the
    /// views kept their math lanes counts as showing it.
    fn math_shown_in_slot(&self, j: usize, k: usize) -> bool {
        let s = &self.m.view_slots[j];
        let words = |key: &str| slot_get(s, key).map(|v| v.split_whitespace().filter_map(|w| w.parse::<usize>().ok()).collect::<Vec<_>>());
        match words("math_def") {
            None => true,
            Some(def) => def.contains(&(k + 1)) && !words("math_hidden").unwrap_or_default().contains(&(k + 1)),
        }
    }

    /// Shows or hides math channel `k` in the view shown only; its definition stays. A channel that no other view
    /// shows goes off altogether when it is hidden (the slot is free again). Showing one that is off defines it as an
    /// addition, like the display switch of `:MATH<n>:DISPlay`.
    pub(crate) fn math_set_shown(&mut self, k: usize, on: bool) {
        if k >= NMATH { return; }
        if on {
            if self.m.math_ch(k).op == MathOp::Off {
                let mut c = self.m.math_ch(k);
                c.op = MathOp::Add;
                if k == self.m.math_sel { self.m.math = c.op; } else { self.m.maths[k] = c; }
            }
            self.m.math_hidden[k] = false;
            return;
        }
        let elsewhere = self.m.views >= 2 && (0..self.m.views).any(|j| j != self.m.view_cur && self.math_shown_in_slot(j, k));
        if elsewhere {
            self.m.math_hidden[k] = true;
        } else {
            if k == self.m.math_sel { self.m.math = MathOp::Off; } else { self.m.maths[k].op = MathOp::Off; }
            self.m.math_hidden[k] = false;
        }
    }

    /// Writes what the Ui keeps of the shown view into the model, so a setup written now has it.
    pub fn sync_view_state(&mut self) {
        let t = self.view_ui_text();
        if self.m.studio_state != t { self.m.studio_state = t; }
        // a channel that is off is not "hidden": defining it again shows it here
        for k in 0..NMATH {
            if self.m.math_hidden[k] && self.m.math_ch(k).op == MathOp::Off { self.m.math_hidden[k] = false; }
        }
    }

    /// After a setup was read: the shown view's Ui state, and every other view that has no slot starts as a copy of
    /// the shown one (a setup from before the views kept their own state).
    pub(crate) fn view_state_restore(&mut self, text: &str) {
        let has = text.lines().any(|l| l.split('=').next().is_some_and(|k| k.trim() == "studio_state"));
        if has || !self.m.studio_state.is_empty() {
            let t = self.m.studio_state.clone();
            self.view_ui_apply(&t);
        }
        self.view_own = Default::default();
        let n = self.m.views;
        if n >= 2 {
            let snap = self.view_snapshot();
            for k in 0..n {
                if k != self.m.view_cur && self.m.view_slots[k].is_empty() { self.m.view_slots[k] = snap.clone(); }
            }
        }
    }

    /// What the shown view is now.
    pub(super) fn view_snapshot(&self) -> String {
        let mut ui = vec![("studio_view", self.studio.view.token().to_string()), ("studio_dock", self.dock_text()), ("studio_state", self.view_ui_text())];
        if let Some(l) = self.view_lanes_text() { ui.push(("lanes", l)); }
        ui.push(("math_def", (0..NMATH).filter(|&k| self.m.math_ch(k).op != MathOp::Off).map(|k| (k + 1).to_string()).collect::<Vec<_>>().join(" ")));
        snapshot_of(&self.m, self.xy, self.full, &ui)
    }

    /// Shows a kept view. A slot from before the views kept their own state (no `studio_state`) leaves the Ui's part as it is.
    fn view_apply(&mut self, slot: &str) {
        let new = slot.contains("studio_state=");
        self.m.meas.clear();
        self.m.meas2.clear();
        self.m.cat.clear();
        self.m.row_cfg.clear();
        if new {
            // keys the setup writes only when they differ from the default
            self.m.studio_ov = 0;
            // the table is the default view of the measurement panel (only the List is written down)
            self.m.studio_detail = true;
            self.m.studio_classic = false;
            self.m.meas_pop_current = false;
            self.m.meas_show = [true; 3];
            self.m.cur_b_source = None;
            self.m.eye_pll_bw = 0.0;
            self.m.math_hidden = [false; NMATH];
            self.m.gen_hidden = false;
        }
        for part in slot.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let Some((k, v)) = part.split_once('=') else { continue };
            let (k, v) = (k.trim(), unesc(v.trim()));
            match k {
                "xy" => self.xy = v == "true",
                "full" => self.full = v == "true",
                // a record session (SIM, a loaded file) has its own panel and goes with the session, not the view
                "studio_view" => if !self.studio.view.is_record() { self.studio.view = StudioView::from_token(&v); },
                "studio_dock" => {
                    let ver = self.studio.dock.ver;
                    self.dock_restore(&v);
                    self.studio.dock.ver = ver.wrapping_add(1);
                }
                "studio_state" => self.view_ui_apply(&v),
                // math channels defined since this view was left are not shown in it: a new one shows in the view it was made in
                "math_def" => {
                    let def: Vec<usize> = v.split_whitespace().filter_map(|w| w.parse().ok()).collect();
                    for k in 0..NMATH {
                        if self.m.math_ch(k).op != MathOp::Off && !def.contains(&(k + 1)) { self.m.math_hidden[k] = true; }
                    }
                }
                "lanes" => {
                    if !self.record_mode() {
                        let shown: Vec<&str> = v.split_whitespace().collect();
                        for i in 0..NREF {
                            if let Some(r) = self.refs[i].as_mut() { r.on = shown.contains(&format!("R{}", i + 1).as_str()); }
                        }
                        self.m.bus.on = shown.contains(&"B");
                    }
                }
                _ if KEYS.contains(&k) => {
                    if let Err(e) = store::set(&mut self.m, k, &v) {
                        eprintln!("oos-ui: view: {}: {}", k, e);
                    }
                }
                _ => {}
            }
        }
        if self.m.zscale > self.m.tscale {
            self.m.zscale = self.m.tscale;
        }
        // what pointed into the list of the view that was shown
        self.meas_cfg = None;
        self.meas_page = 0;
        self.meas_catalog_page = 0;
        // the scroll of the panel is part of the view (`scroll=` of its state); a slot from before keeps no state: top
        if !new { self.studio.panel.scroll_top(); }
        self.studio.md = Default::default();
        self.m.studio_view = self.studio.view.token().to_string();
        self.dock_sync_setup();
        self.settings_changed();
        self.relayout();
    }

    /// Shows view `j` (0-based); the one shown so far keeps its state.
    pub(crate) fn view_go(&mut self, j: usize) {
        let n = self.m.views;
        if n < 2 || j >= n || j == self.m.view_cur {
            return;
        }
        let cur = self.m.view_cur.min(n - 1);
        let snap = self.view_snapshot();
        self.m.view_slots[cur] = snap.clone();
        self.view_own[cur] = std::mem::take(&mut self.meas_own);
        let mut s = std::mem::take(&mut self.m.view_slots[j]);
        if s.is_empty() {
            // never set up (a setup from before the views): a copy of the one that was shown
            s = snap;
            self.view_own[j] = self.view_own[cur].clone();
        }
        self.meas_own = std::mem::take(&mut self.view_own[j]);
        self.view_apply(&s);
        self.m.view_cur = j;
        self.sync_view_state();
        self.toast(&format!("View {} of {}", j + 1, n));
    }

    /// The next (`d` = 1) or previous view, round.
    pub(crate) fn view_step(&mut self, d: i32) {
        let n = self.m.views;
        if n >= 2 {
            self.view_go((self.m.view_cur as i32 + d).rem_euclid(n as i32) as usize);
        }
    }

    /// Display > Views: Off, 2, 3, 4. More views: each new one starts as a copy of the shown view (its windows, analysis
    /// views and layout), not as an empty plot. Fewer views: the shown view keeps its content, whatever number it
    /// then has; the ones dropped are forgotten.
    pub(crate) fn views_set(&mut self, n: usize) {
        let n = n.clamp(1, VIEWS_MAX);
        let old = self.m.views;
        if n == old {
            return;
        }
        if n > old {
            let snap = self.view_snapshot();
            for k in old.max(1)..n {
                self.m.view_slots[k] = snap.clone();
                self.view_own[k] = self.meas_own.clone();
            }
        } else if self.m.view_cur >= n {
            self.m.view_cur = n - 1;
        }
        for s in self.m.view_slots.iter_mut().skip(n) {
            s.clear();
        }
        for o in self.view_own.iter_mut().skip(n) {
            o.clear();
        }
        if n == 1 {
            self.m.view_cur = 0;
            self.m.view_slots[0].clear();
        }
        self.m.views = n;
        if n >= 2 {
            self.toast(&format!("View {} of {}", self.m.view_cur + 1, n));
        } else {
            self.toast("Views off");
        }
    }
    // ------------------------------------------------------------------------------------------------ the menu item

    pub(super) fn views_kind(it: Item) -> Option<Kind> {
        (it == Item::Views).then_some(Kind::Choice)
    }

    pub(super) fn views_label(it: Item) -> Option<&'static str> {
        (it == Item::Views).then_some("Views")
    }

    pub(super) fn views_value(&self, it: Item) -> Option<String> {
        (it == Item::Views).then(|| if self.m.views < 2 { "Off".to_string() } else { self.m.views.to_string() })
    }

    pub(super) fn views_options(&self, it: Item) -> Option<Vec<String>> {
        (it == Item::Views).then(|| (1..=VIEWS_MAX).map(|n| if n == 1 { "Off".to_string() } else { n.to_string() }).collect())
    }

    pub(super) fn views_adjust(&mut self, it: Item, d: i32) -> bool {
        if it != Item::Views {
            return false;
        }
        self.views_set((self.m.views as i32 + d).clamp(1, VIEWS_MAX as i32) as usize);
        true
    }

    // ------------------------------------------------------------------------------------------------ the top bar

    fn num_w(&self) -> i32 {
        if self.m.layout == Layout::Studio { NUM_W_STUDIO } else { NUM_W }
    }

    /// The numbered buttons in the top bar, left of the menu buttons, while there is more than one view.
    pub(super) fn view_box(&self) -> Option<Rect> {
        if self.m.views < 2 {
            return None;
        }
        // Studio: a segment of its own in the right group of its top bar (`studio_top`)
        if self.m.layout == Layout::Studio {
            let r = self.studio_top().vbox;
            return (r.w > 0).then_some(r);
        }
        let first = self.top_buttons().first().map_or(self.top_right(), |b| b.2.x);
        let w = NUM_W * self.m.views as i32 + 2;
        Some(Rect::new(first - 8 - w, 5, w, 30))
    }

    pub(super) fn draw_view_box(&self, cv: &mut Canvas) {
        let Some(r) = self.view_box() else { return };
        let f = self.fonts();
        let nw = self.num_w();
        cv.round(r, 5, BTN_BORDER, 255);
        cv.round(r.inset(1), 4, BTN, 255);
        for i in 0..self.m.views {
            let c = Rect::new(r.x + 1 + nw * i as i32, r.y + 1, nw, r.h - 2);
            let on = i == self.m.view_cur;
            if on {
                cv.round(c.inset(2), 3, ACCENT, 255);
            }
            cv.text_in(&f.mb, c, 1, &(i + 1).to_string(), if on { BG } else { TEXT });
        }
    }

    /// A tap at (x, y) on the view buttons: that view.
    pub(super) fn view_box_tap(&mut self, x: i32, y: i32) -> bool {
        let Some(r) = self.view_box() else { return false };
        if !self.top_hit(r).contains(x, y) {
            return false;
        }
        let i = ((x - r.x - 1) / self.num_w()).clamp(0, self.m.views as i32 - 1) as usize;
        self.view_go(i);
        true
    }

    // ------------------------------------------------------------------------------------------------ the swipe

    /// A finger went down at x in the plot: whether it is in an edge strip where a swipe switches views.
    pub(super) fn view_edge(&self, x: i32) -> bool {
        self.m.views >= 2 && (x < self.grid.x + EDGE || x >= self.grid.x + self.grid.w - EDGE)
    }

    /// The finger moved by (dx, dy) from x0 in an edge strip: Some(true) switched, Some(false) still open, None it is
    /// not a swipe (a vertical drag: the usual drags take it).
    pub(super) fn view_swipe(&mut self, x0: i32, dx: i32, dy: i32) -> Option<bool> {
        if dy.abs() > 10 && dy.abs() >= dx.abs() {
            return None;
        }
        if dx.abs() < SWIPE {
            return Some(false);
        }
        let from_left = x0 < self.grid.x + EDGE;
        // inwards only: from the left edge to the right shows the view before, from the right edge to the left the next
        match (from_left, dx > 0) {
            (true, true) => self.view_step(-1),
            (false, false) => self.view_step(1),
            _ => return None,
        }
        Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui() -> Ui {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.step(1.0 / 30.0);
        u
    }

    #[test]
    fn each_view_keeps_its_arrangement_and_shares_the_rest() {
        let mut u = ui();
        u.m.meas = vec![(Meas::Freq, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(0))];
        u.m.zoom = true;
        let tdiv = u.m.tscale;
        u.views_set(3);
        u.view_go(1);
        // a new view: a copy of the one it was made from, the time base as it was
        assert!(u.m.meas.len() == 2 && u.m.zoom && u.m.view_cur == 1 && u.m.tscale == tdiv);
        u.m.meas.clear();
        u.m.zoom = false;
        u.m.pane = 1;
        u.xy = true;
        u.view_go(0);
        assert_eq!(u.m.meas, vec![(Meas::Freq, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(0))]);
        assert!(u.m.zoom && u.m.pane == 0 && !u.xy);
        u.view_step(1);
        assert!(u.m.pane == 1 && u.xy && u.m.meas.is_empty());
        // round: from the last to the first
        u.view_step(1);
        u.view_step(1);
        assert_eq!(u.m.view_cur, 0);
    }

    #[test]
    fn views_go_with_a_setup() {
        let mut u = ui();
        u.views_set(2);
        u.m.meas = vec![(Meas::Freq, WaveId::Ch(0))];
        u.view_go(1);
        u.m.cursor = CursorMode::Time;
        let text = store::setup_text(&u.m);
        assert!(text.contains("views = 2 2"), "{}", text);
        let m = store::parse_setup(&text).unwrap();
        assert_eq!((m.views, m.view_cur), (2, 1));
        assert!(m.view_slots[0].contains("meas=Freq:1"), "{}", m.view_slots[0]);
        assert!(m.view_slots[1].is_empty());
        // a slot with a key that is not part of a view is refused
        assert!(store::parse_setup("view1 = tdiv=3").is_err());
        assert!(store::parse_setup("views = 5 1").is_err());
    }

    #[test]
    fn fewer_views_keep_the_screen() {
        let mut u = ui();
        u.views_set(3);
        u.view_go(2);
        u.m.zoom = true;
        u.views_set(2);
        // view 3 is gone: what was on screen stays, as view 2
        assert_eq!(u.m.view_cur, 1);
        assert!(u.m.zoom);
        u.views_set(1);
        assert_eq!((u.m.views, u.m.view_cur), (1, 0));
        assert!(u.m.view_slots.iter().all(|s| s.is_empty()));
        // off: nothing switches
        u.view_step(1);
        assert_eq!(u.m.view_cur, 0);
    }

    #[test]
    fn edge_swipes_and_buttons_switch_views() {
        let mut u = ui();
        let g = u.grid;
        let y = g.y + g.h / 2;
        // off: a swipe from the right edge moves the waveform as always
        let d0 = u.m.delay;
        u.handle(Ev::Touch { id: 0, phase: Phase::Down, x: g.x + g.w - 5, y });
        u.handle(Ev::Touch { id: 0, phase: Phase::Move, x: g.x + g.w - 200, y });
        u.handle(Ev::Touch { id: 0, phase: Phase::Up, x: g.x + g.w - 200, y });
        assert!(u.m.delay != d0 && u.m.view_cur == 0);
        u.m.delay = d0;
        u.views_set(3);
        let swipe = |u: &mut Ui, x0: i32, x1: i32| {
            u.handle(Ev::Touch { id: 0, phase: Phase::Down, x: x0, y });
            for s in 1..=4 {
                u.handle(Ev::Touch { id: 0, phase: Phase::Move, x: x0 + (x1 - x0) * s / 4, y });
            }
            u.handle(Ev::Touch { id: 0, phase: Phase::Up, x: x1, y });
        };
        swipe(&mut u, g.x + g.w - 5, g.x + g.w - 200);
        assert_eq!(u.m.view_cur, 1);
        assert_eq!(u.m.delay, d0);
        swipe(&mut u, g.x + 4, g.x + 200);
        assert_eq!(u.m.view_cur, 0);
        // in the middle it is still the time position
        swipe(&mut u, g.x + g.w / 2, g.x + g.w / 2 - 150);
        assert!(u.m.delay != d0 && u.m.view_cur == 0);
        // a vertical drag at the left edge still moves a channel
        let c = u.m.active;
        let off = u.m.ch[c].offset;
        u.handle(Ev::Touch { id: 0, phase: Phase::Down, x: g.x + 4, y: u.y_of_volts(c, 0.0) });
        u.handle(Ev::Touch { id: 0, phase: Phase::Move, x: g.x + 6, y: u.y_of_volts(c, 0.0) + 60 });
        u.handle(Ev::Touch { id: 0, phase: Phase::Up, x: g.x + 6, y: u.y_of_volts(c, 0.0) + 60 });
        assert!(u.m.ch[u.m.active].offset != off || u.m.active != c);
        // the buttons in the top bar
        let r = u.view_box().unwrap();
        u.handle(Ev::Touch { id: 0, phase: Phase::Down, x: r.x + 2 * NUM_W + 10, y: r.y + 15 });
        u.handle(Ev::Touch { id: 0, phase: Phase::Up, x: r.x + 2 * NUM_W + 10, y: r.y + 15 });
        assert_eq!(u.m.view_cur, 2);
        // they leave room for the menu buttons
        assert!(r.x + r.w <= u.top_buttons()[0].2.x);
    }

    #[test]
    fn views_by_scpi() {
        let mut u = ui();
        let cv = Canvas::new(PANEL_W, PANEL_H);
        let mut scpi = crate::scpi::Scpi::open(0);
        assert_eq!(scpi.exec_line(&mut u, &cv, ":DISP:VIEW:COUN?"), b"1");
        scpi.exec_line(&mut u, &cv, ":DISP:VIEW:COUN 3");
        scpi.exec_line(&mut u, &cv, ":DISPlay:VIEW 3");
        assert_eq!(scpi.exec_line(&mut u, &cv, ":DISP:VIEW?"), b"3");
        assert_eq!(u.m.view_cur, 2);
        scpi.exec_line(&mut u, &cv, ":DISP:VIEW 4");
        assert_eq!(u.m.view_cur, 2);
        assert!(scpi.exec_line(&mut u, &cv, ":SYST:ERR?").starts_with(b"-224"));
    }

    #[test]
    fn the_quick_key_steps_through_the_views() {
        let mut u = ui();
        u.views_set(2);
        u.m.quick = Quick::NextView;
        u.key(key::QUICK);
        assert_eq!(u.m.view_cur, 1);
        u.key(key::QUICK);
        assert_eq!(u.m.view_cur, 0);
    }
}
