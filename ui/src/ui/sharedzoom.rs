// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio: one shared zoom window for every lane, a source choice for it (tap and hold the Zoom button), and a
//! horizontal scale of their own for references recorded at another time base (hold an R pill).
//!
//! The zoom window itself is the one of `Model` (`zscale`, `zdelay`): `Ui::view` gives it to every lane. A lane
//! picked as the zoom source is the only one that uses it; the others stay at the main time base. A reference with
//! its own horizontal scale keeps it whatever the zoom does. Live channels always share the acquisition time base.
use super::studio::chip;
use super::workspace::Trace;
use super::*;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct SharedZoom {
    /// the zoom source popover is open
    pub pop: bool,
    /// the one lane that zooms (None: all lanes)
    pub lane: Option<Trace>,
    /// references with a horizontal scale of their own: slot -> (s/div, delay of the lane's centre)
    pub ref_h: BTreeMap<usize, (f32, f32)>,
    /// the horizontal popover of this reference is open
    pub ref_pop: Option<usize>,
    /// the finger that is down: when and where (the long-press clock of `studio_hold_tick`), and whether a hold fired
    pub press: Option<(f64, i32, i32)>,
    pub fired: bool,
}

const POP_W: i32 = 300;
const ROW: i32 = 40;

pub(super) fn trace_name(t: Trace) -> String {
    match t {
        Trace::Channel(c) => format!("CH{}", c + 1),
        Trace::Reference(i) => format!("R{}", i + 1),
        Trace::Math(k) => format!("M{}", k + 1),
    }
}

/// What a tap in the zoom source popover hit.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum ZPart {
    All,
    Lane(Trace),
    YMinus,
    YPlus,
    Reset,
}

/// What a tap in the reference horizontal popover hit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum RPart {
    Follow,
    Own,
    TMinus,
    TPlus,
    PMinus,
    PPlus,
    Fit,
}

fn zoom_text_of(n: f32) -> String {
    let n = if n.is_finite() { n.max(1.0) } else { 1.0 };
    if n >= 9.95 { format!("{:.0}\u{d7}", n) } else { format!("{:.1}\u{d7}", n).replace(".0\u{d7}", "\u{d7}") }
}

impl Ui {
    // ------------------------------------------------------------------------------------------ views

    /// The lane being drawn (`cur_win`) as a source, in Studio's stacked lanes.
    fn studio_cur_trace(&self) -> Option<Trace> {
        if self.m.layout != Layout::Studio || !self.stacked() { return None; }
        self.trace_sources().get(self.cur_win).copied()
    }

    /// The single zoom lane, while it is still a lane.
    pub(super) fn studio_zoom_lane(&self) -> Option<Trace> {
        if self.m.layout != Layout::Studio { return None; }
        let t = self.studio.sz.lane?;
        self.trace_sources().contains(&t).then_some(t)
    }

    /// Whether the zoom window applies to live channel `c` (all lanes, or `c` is the chosen one).
    pub(super) fn studio_zoom_covers(&self, c: usize) -> bool {
        match self.studio_zoom_lane() {
            None => true,
            Some(t) => t == Trace::Channel(c),
        }
    }

    /// The time view of the lane being drawn, given the shared one. A lane of a reference with its own scale
    /// has it; with one lane chosen to zoom, the other lanes keep the main time base.
    pub(super) fn studio_lane_view(&self, base: (f32, f32)) -> (f32, f32) {
        if self.m.layout != Layout::Studio || (self.studio.sz.lane.is_none() && self.studio.sz.ref_h.is_empty()) { return base; }
        let Some(t) = self.studio_cur_trace() else { return base };
        if let Trace::Reference(i) = t {
            if let Some(&own) = self.studio.sz.ref_h.get(&i) { return own; }
        }
        if self.m.zoom {
            if let Some(z) = self.studio_zoom_lane() {
                if z != t { return (self.m.time_per_div(), self.m.delay); }
            }
        }
        base
    }

    /// The own horizontal scale of reference `i` (s/div, delay), if it has one.
    pub(super) fn studio_ref_own(&self, i: usize) -> Option<(f32, f32)> {
        if self.m.layout != Layout::Studio { return None; }
        self.studio.sz.ref_h.get(&i).copied()
    }

    /// The lane being drawn is a reference with its own time scale: its labels show it.
    pub(super) fn studio_lane_has_own_time(&self) -> bool {
        matches!(self.studio_cur_trace(), Some(Trace::Reference(i)) if self.studio.sz.ref_h.contains_key(&i))
    }

    /// The vertical zoom applies to this lane rectangle (every detail lane, or the chosen one).
    pub(super) fn studio_vzoom_covers(&self, area: Rect) -> bool {
        let Some(z) = self.studio_zoom_lane() else { return true };
        self.trace_sources().iter().position(|t| *t == z).and_then(|i| self.wins.get(i)).is_some_and(|w| *w == area)
    }

    // ------------------------------------------------------------------------------------------ own horizontal

    /// s/div and delay at which the whole record of reference `slot` fills a lane.
    pub(super) fn studio_ref_fit_view(&self, slot: usize) -> Option<(f32, f32)> {
        let r = self.refs.get(slot)?.as_ref()?;
        if r.v.len() < 2 || !(r.dt > 0.0) { return None; }
        let t0 = self.source_time_origin(WaveId::Ref(slot), r.t0);
        let span = (r.v.len() - 1) as f64 * r.dt;
        Some(((span / HDIV as f64) as f32, (t0 + span / 2.0) as f32))
    }

    /// Fit in Studio: the lane of the reference gets its own time/div so the whole record fills it (and its volts
    /// fit); the live time base and the other lanes stay. False: not Studio or nothing to fit.
    pub(super) fn studio_ref_fit(&mut self, slot: usize) -> bool {
        if self.m.layout != Layout::Studio || !self.stacked() { return false; }
        let Some(view) = self.studio_ref_fit_view(slot) else { return false };
        if let Some(r) = self.refs[slot].as_ref() {
            let summary = crate::wave::autoset_summary(&r.v, r.dt);
            if summary.valid {
                let want = ((summary.vmax - summary.vmin) / (VDIV * 0.8)).max(VSCALES[0]);
                let vpd = VSCALES.iter().copied().find(|&s| s >= want).unwrap_or(VSCALES[VSCALES.len() - 1]);
                let offset = -0.5 * (summary.vmax + summary.vmin);
                let r = self.refs[slot].as_mut().unwrap();
                r.vpd = vpd;
                r.offset = offset;
            }
        }
        self.studio.sz.ref_h.insert(slot, view);
        self.dirty_map = true;
        self.toast("Fit record");
        true
    }

    /// "Follow main" or "Own" for a reference.
    pub(super) fn studio_ref_set_own(&mut self, slot: usize, own: bool) {
        if !own { self.studio.sz.ref_h.remove(&slot); }
        else if !self.studio.sz.ref_h.contains_key(&slot) {
            let seed = self.studio_ref_fit_view(slot).unwrap_or((self.m.time_per_div(), self.m.delay));
            self.studio.sz.ref_h.insert(slot, seed);
        }
        self.dirty_map = true;
    }

    /// One step of the own s/div along the ladder (dir > 0: slower, wider window).
    pub(super) fn studio_ref_step_tdiv(&mut self, slot: usize, dir: i32) {
        self.studio_ref_set_own(slot, true);
        let Some(&(t, d)) = self.studio.sz.ref_h.get(&slot) else { return };
        let ladder = tscales();
        let next = if dir > 0 { ladder.iter().copied().find(|&s| s > t * 1.001) } else { ladder.iter().copied().rev().find(|&s| s < t * 0.999) };
        if let Some(n) = next { self.studio.sz.ref_h.insert(slot, (n, d)); }
        self.dirty_map = true;
    }

    /// One step of the own position: a division.
    pub(super) fn studio_ref_step_pos(&mut self, slot: usize, dir: i32) {
        self.studio_ref_set_own(slot, true);
        if let Some(h) = self.studio.sz.ref_h.get_mut(&slot) { h.1 += dir as f32 * h.0; }
        self.dirty_map = true;
    }

    // ------------------------------------------------------------------------------------------ overview

    /// The math channels in the overview, thin and decimated like the other sources.
    pub(super) fn draw_studio_overview_math(&self, cv: &mut Canvas, o: Rect) {
        if self.m.layout != Layout::Studio { return; }
        let (tdiv, delay) = (self.m.time_per_div() as f64, self.m.delay as f64);
        if !(tdiv > 0.0) { return; }
        cv.set_clip(o);
        let sc = math_scales();
        for k in 0..NMATH {
            let c = self.m.math_ch(k);
            if !self.m.math_shown(k) || c.op == MathOp::Fft { continue; }
            let Ok(Some(rec)) = self.math_record(k) else { continue };
            let w = rec.v.as_ref();
            if w.is_empty() || !(rec.dt > 0.0) { continue; }
            let col = MATH_COLORS[k % MATH_COLORS.len()];
            let vpd = sc[c.scale.min(sc.len() - 1)] as f32;
            let half = tdiv * HDIV as f64 / 2.0;
            let at = |t: f64| ((t - rec.t0) / rec.dt).floor().clamp(-1.0, w.len() as f64) as isize;
            let begin = (at(delay - half) - 1).clamp(0, w.len() as isize) as usize;
            let end = (at(delay + half) + 2).clamp(0, w.len() as isize) as usize;
            if end <= begin { continue; }
            let step = ((end - begin) / (o.w as usize * 2)).max(1);
            let mut prev: Option<(i32, i32)> = None;
            for i in (begin..end).step_by(step) {
                let v = w[i];
                if !v.is_finite() { prev = None; continue; }
                let t = rec.t0 + i as f64 * rec.dt - delay;
                let x = (o.x as f64 + o.w as f64 / 2.0 + t / tdiv * o.w as f64 / HDIV as f64).round() as i32;
                let y = (o.y as f32 + o.h as f32 / 2.0 - (v / vpd + c.offset) * o.h as f32 / VDIV).clamp(-1e4, 1e4).round() as i32;
                if let Some((x0, y0)) = prev { cv.line(x0, y0, x, y, col); }
                prev = Some((x, y));
            }
        }
        cv.no_clip();
    }

    // ------------------------------------------------------------------------------------------ hold

    /// A finger held on the Zoom button opens the zoom source popover (an R chip's hold menu has the horizontal one). The lift of a hold is not a tap (`touch.held`).
    pub(super) fn studio_hold_tick(&mut self) {
        if self.m.layout != Layout::Studio { return; }
        let Some((t0, x, y)) = self.studio.sz.press else { return };
        if self.studio.sz.fired || self.now - t0 < LONG_PRESS { return; }
        if self.m.touch_lock || self.full_on() || self.dialog.is_some() || self.power_dlg || self.wlan.osk.is_some() { return; }
        if self.touch.down && self.touch.drag != Drag::None && self.touch.drag != Drag::Pending { return; }
        if !self.view_on() && self.zoom_btn().1.contains(x, y) {
            self.studio.sz.fired = true;
            self.workspace_drop_gesture();
            if self.touch.down { self.touch.held = true; }
            self.close_menu();
            self.studio.ch_picker = false;
            self.studio.sz.ref_pop = None;
            self.studio.sz.pop = true;
        }
    }

    // ------------------------------------------------------------------------------------------ zoom popover

    /// The zoom source popover: its rectangle and what each part is.
    pub(super) fn studio_sz_parts(&self) -> Option<(Rect, Vec<(ZPart, Rect)>)> {
        if !self.studio.sz.pop || self.m.layout != Layout::Studio { return None; }
        let btn = self.zoom_btn().0;
        let lanes = self.trace_sources();
        let cols = 3;
        let cw = (POP_W - 16 - 2 * 6) / cols;
        let lane_rows = (lanes.len() as i32 + cols - 1) / cols;
        let h = 36 + (ROW + 2) + lane_rows * (ROW + 2) + 8 + ROW + 8 + ROW + 8;
        let x = (btn.right() + 5 - POP_W).max(4);
        let y = (btn.bottom() + 10).min(self.scr.h - h - 4);
        let p = Rect::new(x, y, POP_W, h);
        let mut parts = Vec::new();
        let mut cy = p.y + 36;
        parts.push((ZPart::All, Rect::new(p.x + 8, cy, POP_W - 16, ROW - 2)));
        cy += ROW + 2;
        for (n, t) in lanes.iter().enumerate() {
            let (col, row) = (n as i32 % cols, n as i32 / cols);
            parts.push((ZPart::Lane(*t), Rect::new(p.x + 8 + col * (cw + 6), cy + row * (ROW + 2), cw, ROW - 2)));
        }
        cy += lane_rows * (ROW + 2) + 8;
        parts.push((ZPart::YMinus, Rect::new(p.x + 8, cy, 56, ROW)));
        parts.push((ZPart::YPlus, Rect::new(p.right() - 8 - 56, cy, 56, ROW)));
        cy += ROW + 8;
        parts.push((ZPart::Reset, Rect::new(p.x + 8, cy, POP_W - 16, ROW)));
        Some((p, parts))
    }

    fn draw_studio_sz_pop(&self, cv: &mut Canvas) {
        let Some((p, parts)) = self.studio_sz_parts() else { return };
        let f = self.fonts();
        cv.round(p, 8, BTN_BORDER, 255);
        cv.round(p.inset(1), 7, PANEL_BG, 255);
        cv.text(&f.mb, p.x + 12, p.y + 25, "Zoom source", TEXT);
        let lane = self.studio_zoom_lane();
        for (part, r) in parts {
            match part {
                ZPart::All | ZPart::Lane(_) => {
                    let on = match part { ZPart::All => lane.is_none(), ZPart::Lane(t) => lane == Some(t), _ => false };
                    chip(cv, r, BTN, if on { AMBER } else { BTN_BORDER });
                    let text = match part { ZPart::Lane(t) => trace_name(t), _ => "Zoom: All lanes".to_string() };
                    cv.text_in(&f.m, r, 1, &text, if on { AMBER } else { TEXT });
                }
                ZPart::YMinus | ZPart::YPlus => {
                    chip(cv, r, BTN, BTN_BORDER);
                    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
                    cv.hline(cx - 6, cx + 6, cy, TEXT);
                    if part == ZPart::YPlus { cv.vline(cx, cy - 6, cy + 6, TEXT); }
                    if part == ZPart::YMinus {
                        let mid = Rect::new(r.right(), r.y, p.right() - 8 - 56 - r.right(), r.h);
                        cv.text_in(&f.mb, mid, 1, &format!("Y zoom {}", zoom_text_of(self.zoom_vfactor)), TEXT);
                    }
                }
                ZPart::Reset => {
                    chip(cv, r, BTN, BTN_BORDER);
                    cv.text_in(&f.m, r, 1, "Reset zoom", TEXT);
                }
            }
        }
    }

    /// A tap in the zoom source popover. True: the tap was the popover's (or closed it).
    fn studio_sz_tap(&mut self, x: i32, y: i32) -> bool {
        let Some((p, parts)) = self.studio_sz_parts() else { return false };
        if !p.contains(x, y) {
            self.studio.sz.pop = false;
            return true;
        }
        let Some((part, _)) = parts.into_iter().find(|(_, r)| r.contains(x, y)) else { return true };
        let area = self.wave_area();
        match part {
            ZPart::All => { self.studio.sz.lane = None; self.studio_sz_zoom_on(); }
            ZPart::Lane(t) => { self.studio.sz.lane = Some(t); self.studio_sz_zoom_on(); }
            ZPart::YMinus => { self.studio_sz_zoom_on(); self.zoom_vertical_step(-1, area, 0.5); }
            ZPart::YPlus => { self.studio_sz_zoom_on(); self.zoom_vertical_step(1, area, 0.5); }
            ZPart::Reset => {
                self.studio_sz_zoom_on();
                self.studio.sz.lane = None;
                self.reset_zoom();
            }
        }
        self.dirty_map = true;
        true
    }

    fn studio_sz_zoom_on(&mut self) {
        if !self.m.zoom { self.set_zoom(true); }
    }

    // ------------------------------------------------------------------------------------------ reference popover

    pub(super) fn studio_ref_pop_parts(&self) -> Option<(Rect, Vec<(RPart, Rect)>)> {
        let i = self.studio.sz.ref_pop?;
        if self.m.layout != Layout::Studio { return None; }
        let pill = self.studio_bottom().refs[i];
        if pill.w == 0 { return None; }
        let h = 34 + 4 * (ROW + 4) + 6;
        let x = pill.x.clamp(4, self.scr.w - POP_W - 4);
        let p = Rect::new(x, pill.y - h - 8, POP_W, h);
        let half = (POP_W - 16 - 6) / 2;
        let mut y = p.y + 34;
        let mut parts = vec![
            (RPart::Follow, Rect::new(p.x + 8, y, half, ROW)),
            (RPart::Own, Rect::new(p.x + 8 + half + 6, y, half, ROW)),
        ];
        y += ROW + 4;
        parts.push((RPart::TMinus, Rect::new(p.right() - 8 - 56 - 100 - 56, y, 56, ROW)));
        parts.push((RPart::TPlus, Rect::new(p.right() - 8 - 56, y, 56, ROW)));
        y += ROW + 4;
        parts.push((RPart::PMinus, Rect::new(p.right() - 8 - 56 - 100 - 56, y, 56, ROW)));
        parts.push((RPart::PPlus, Rect::new(p.right() - 8 - 56, y, 56, ROW)));
        y += ROW + 4;
        parts.push((RPart::Fit, Rect::new(p.x + 8, y, POP_W - 16, ROW)));
        Some((p, parts))
    }

    fn draw_studio_ref_pop(&self, cv: &mut Canvas) {
        let (Some((p, parts)), Some(i)) = (self.studio_ref_pop_parts(), self.studio.sz.ref_pop) else { return };
        let f = self.fonts();
        cv.round(p, 8, BTN_BORDER, 255);
        cv.round(p.inset(1), 7, PANEL_BG, 255);
        cv.text(&f.mb, p.x + 12, p.y + 25, &format!("R{} horizontal", i + 1), TEXT);
        let own = self.studio.sz.ref_h.get(&i).copied();
        let (tdiv, pos) = own.unwrap_or((self.m.time_per_div(), self.m.delay));
        let shade = if own.is_some() { TEXT } else { DIM };
        for (part, r) in parts {
            match part {
                RPart::Follow | RPart::Own => {
                    let on = (part == RPart::Own) == own.is_some();
                    chip(cv, r, BTN, if on { AMBER } else { BTN_BORDER });
                    cv.text_in(&f.m, r, 1, if part == RPart::Own { "Own" } else { "Follow main" }, if on { AMBER } else { TEXT });
                }
                RPart::TMinus | RPart::TPlus | RPart::PMinus | RPart::PPlus => {
                    chip(cv, r, BTN, BTN_BORDER);
                    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
                    cv.hline(cx - 6, cx + 6, cy, TEXT);
                    if matches!(part, RPart::TPlus | RPart::PPlus) { cv.vline(cx, cy - 6, cy + 6, TEXT); }
                    if matches!(part, RPart::TMinus | RPart::PMinus) {
                        let time = part == RPart::TMinus;
                        cv.text_in(&f.m, Rect::new(p.x + 12, r.y, 100, r.h), 0, if time { "Time/div" } else { "Position" }, shade);
                        let mid = Rect::new(r.right(), r.y, 100, r.h);
                        let text = if time { format!("{}/div", si_short(tdiv as f64, "s")) } else { si(pos as f64, "s") };
                        cv.text_in(&f.mb, mid, 1, &text, shade);
                    }
                }
                RPart::Fit => {
                    chip(cv, r, BTN, BTN_BORDER);
                    cv.text_in(&f.m, r, 1, "Fit", TEXT);
                }
            }
        }
    }

    fn studio_ref_pop_tap(&mut self, x: i32, y: i32) -> bool {
        let (Some((p, parts)), Some(i)) = (self.studio_ref_pop_parts(), self.studio.sz.ref_pop) else {
            self.studio.sz.ref_pop = None;
            return false;
        };
        if !p.contains(x, y) {
            self.studio.sz.ref_pop = None;
            return true;
        }
        let Some((part, _)) = parts.into_iter().find(|(_, r)| r.contains(x, y)) else { return true };
        match part {
            RPart::Follow => self.studio_ref_set_own(i, false),
            RPart::Own => self.studio_ref_set_own(i, true),
            RPart::TMinus => self.studio_ref_step_tdiv(i, -1),
            RPart::TPlus => self.studio_ref_step_tdiv(i, 1),
            RPart::PMinus => self.studio_ref_step_pos(i, -1),
            RPart::PPlus => self.studio_ref_step_pos(i, 1),
            RPart::Fit => { self.studio_ref_fit(i); }
        }
        true
    }

    /// The popovers: drawn over the lanes and the bars.
    pub(super) fn draw_studio_sz_pops(&self, cv: &mut Canvas) {
        self.draw_studio_sz_pop(cv);
        self.draw_studio_ref_pop(cv);
    }

    /// Taps for both popovers. False: none is open.
    pub(super) fn studio_sz_taps(&mut self, x: i32, y: i32) -> bool {
        if self.studio.sz.ref_pop.is_some() { return self.studio_ref_pop_tap(x, y); }
        if self.studio.sz.pop { return self.studio_sz_tap(x, y); }
        false
    }
}
