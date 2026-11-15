// SPDX-License-Identifier: GPL-2.0-or-later
//! Display > Layout > Studio: a top bar with Run, Single, the trigger state and the acquisition chip, a bottom bar with
//! one chip per channel, stacked lanes with pinned measurement badges, a measurement panel and the zoom toolbar.
//! Everything here is drawn and tapped from state the other layouts already have (lanes: `workspace`, measurements:
//! `tools`, zoom: `Ui::split_zoom`); Studio only arranges and re-skins it.
use super::*;

/// One measurement as a badge or a panel row remembers it: by what it measures, so it survives other measurements
/// being added or removed.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum MeasKey {
    One(Meas, WaveId),
    Pair(usize, WaveId, WaveId),
    Cat(u16, WaveId, WaveId, WaveId, WaveId),
}

/// How the measurements share the screen (View menu).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum StudioView {
    /// the lanes and the pinned badges only
    #[default]
    Waveform,
    /// the panel at the right of the lanes
    Docked,
    /// the panel over the lanes
    Floating,
    /// the table of all measurements instead of the lanes
    Only,
    /// analysing a loaded file: the record navigator on top, the references in lanes, the measurements docked
    Offline,
    /// the SIM panel docked at the right of the lanes
    Sim,
}

impl StudioView {
    /// The name kept in the setup: the views of a record session are kept as the plain waveform.
    pub(super) fn token(self) -> &'static str {
        match self {
            StudioView::Docked => "docked",
            StudioView::Floating => "floating",
            StudioView::Only => "only",
            _ => "waveform",
        }
    }
    pub(super) fn from_token(s: &str) -> StudioView {
        match s {
            "docked" => StudioView::Docked,
            "floating" => StudioView::Floating,
            "only" => StudioView::Only,
            _ => StudioView::Waveform,
        }
    }
    /// The views that run on a record session (a loaded file or a SIM): leaving them returns to live.
    pub(super) fn is_record(self) -> bool { matches!(self, StudioView::Offline | StudioView::Sim) }
    /// The views with a panel docked at the right of the lanes.
    pub(super) fn is_docked(self) -> bool { matches!(self, StudioView::Docked | StudioView::Offline | StudioView::Sim) }
}

/// The measurements panel (docked at the right, floating or maximized).
#[derive(Default)]
pub(super) struct StudioPanel {
    /// first row of the page shown
    pub first: usize,
    /// folded to its title
    pub collapsed: bool,
    /// where the floating panel is
    pub at: Option<(i32, i32)>,
    /// the table shows mean, min and max
    pub stats: bool,
}

#[derive(Default)]
pub(super) struct Studio {
    /// the layout was just chosen: the lanes are stacked at the next frame (`studio_sync`)
    pub enter: bool,
    /// the measurements shown as badges; None until one is pinned or unpinned: the first three
    pub pinned: Option<Vec<MeasKey>>,
    /// measurements moved to the floating panel from a badge popup: listed there beside the pinned ones
    pub floated: Vec<MeasKey>,
    /// the badge row is hidden (eye button, View > Show pinned badges)
    pub hide_badges: bool,
    /// the badge whose action popup is open
    pub popup: Option<MeasKey>,
    pub view: StudioView,
    /// the View menu is open
    pub view_menu: bool,
    /// Full was chosen for the eye (Side is the Studio default: the waveform stays in view)
    pub eye_full: bool,
    /// the "+ CH" picker (channels that are off) is open
    pub ch_picker: bool,
    /// Compact view: smaller lane gaps, badge row and measurement panel
    pub compact: bool,
    pub panel: StudioPanel,
    /// the SIM panel lists the knobs instead of the presets
    pub sim_knobs: bool,
    /// the table and the panel list only the measurements of this source
    pub filter: Option<WaveId>,
    pub drag: Option<StudioDrag>,
    /// the zoom toolbar menu is open
    pub zoom_menu: bool,
    /// the display scale popup of this channel's lane is open
    pub yzoom: Option<usize>,
    /// where the lanes drew their channel badges last frame
    pub lane_badges: std::cell::RefCell<Vec<(usize, Rect)>>,
    /// Studio was the layout at the last frame: coming into it applies the entry rules (`studio_sync`)
    pub was: bool,
    /// Search 1 (Add > Search): its own source, level and slope
    pub search: Option<super::addrail::SearchSpec>,
    /// the event the next / previous buttons stand on
    pub search_at: usize,
    pub search_cache: std::cell::RefCell<Option<(u64, std::sync::Arc<Vec<f64>>)>>,
    /// the cursor readouts go to the badge row, not onto the graticule
    pub cur_badge: bool,
    /// moving one cursor moves the other with it
    pub cur_linked: bool,
    /// the docking tree and the floating views (`dock.rs`)
    pub dock: super::dock::Dock,
    /// the divider under the overview strip is held: (finger y, strip height) at the press
    pub ov_drag: Option<(i32, i32)>,
    /// the gestures of the measurements list (`measdrag.rs`)
    pub md: super::measdrag::Md,
}

/// The second group of the "+ CH" popup.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum PickAdd {
    Math,
    Ref,
    Bus,
}
pub(super) const PICK_ADDS: [PickAdd; 3] = [PickAdd::Math, PickAdd::Ref, PickAdd::Bus];
impl PickAdd {
    pub(super) fn label(self) -> &'static str {
        match self { PickAdd::Math => "Add math", PickAdd::Ref => "Add ref", PickAdd::Bus => "Add bus" }
    }
}

/// The rectangles of the Studio top bar.
#[derive(Clone, Copy)]
pub(super) struct StudioTop {
    /// the Add panel button
    pub add: Rect,
    /// Live (the way back from a record session), zero wide in live mode
    pub badge: Rect,
    /// the run / trigger status pill
    pub run: Rect,
    pub single: Rect,
    /// the mini SIM control, zero wide unless the SIM is the source
    pub sim: Rect,
    pub acq: Rect,
    pub measure: Rect,
    pub cursors: Rect,
    pub view: Rect,
    pub gear: Rect,
}

/// The rectangles of the Studio bottom bar.
#[derive(Clone, Copy)]
pub(super) struct StudioBottom {
    pub ch: [Rect; NCH],
    /// a small chip for each shown ref: a tap makes it the active source
    pub refs: [Rect; NREF],
    /// a small chip for each math channel that is on
    pub math: [Rect; NMATH],
    /// "+ CH": the channels that are off, in one chip (zero wide when every channel is on)
    pub add: Rect,
    pub time: Rect,
    pub trig: Rect,
    pub panel: Option<Rect>,
}

const REF_CHIP_W: i32 = 42;

const CHIP_H: i32 = 30;
const CHIP_Y: i32 = 5;

/// A play triangle, 12 px high, left edge at `x`.
pub(super) fn icon_play(cv: &mut Canvas, x: i32, cy: i32, c: Rgb) {
    cv.triangle([(x, cy - 6), (x, cy + 6), (x + 11, cy)], c);
}

pub(super) fn icon_stop(cv: &mut Canvas, x: i32, cy: i32, c: Rgb) {
    cv.fill(Rect::new(x, cy - 5, 10, 10), c);
}

/// Three bars of different height.
fn icon_bars(cv: &mut Canvas, x: i32, cy: i32, c: Rgb) {
    for (i, h) in [7, 13, 10].into_iter().enumerate() {
        cv.fill(Rect::new(x + i as i32 * 5, cy + 6 - h, 3, h), c);
    }
}

/// Two cursor lines.
fn icon_cursors(cv: &mut Canvas, x: i32, cy: i32, c: Rgb) {
    cv.fill(Rect::new(x, cy - 6, 2, 13), c);
    cv.fill(Rect::new(x + 7, cy - 6, 2, 13), c);
}

/// A screen on a foot.
fn icon_view(cv: &mut Canvas, x: i32, cy: i32, c: Rgb) {
    cv.round_outline(Rect::new(x, cy - 6, 15, 10), 1, c);
    cv.hline(x + 4, x + 10, cy + 6, c);
    cv.vline(x + 7, cy + 4, cy + 6, c);
}

/// A ring with teeth.
fn icon_gear(cv: &mut Canvas, cx: i32, cy: i32, c: Rgb, bg: Rgb) {
    for (dx, dy) in [(0, -8), (0, 8), (-8, 0), (8, 0), (-6, -6), (6, -6), (-6, 6), (6, 6)] {
        cv.fill(Rect::new(cx + dx - 1, cy + dy - 1, 3, 3), c);
    }
    cv.circle(cx, cy, 6, c);
    cv.circle(cx, cy, 4, bg);
    cv.circle(cx, cy, 2, c);
}

/// A chip: the rounded body with a 1 px border.
pub(super) fn chip(cv: &mut Canvas, r: Rect, fill: Rgb, border: Rgb) {
    cv.round(r, 6, border, 255);
    cv.round(r.inset(1), 5, fill, 255);
}

impl Ui {
    /// After the settings of the last run were read (main.rs): Studio comes back with its view and its lanes.
    pub fn restore_view_state(&mut self) {
        self.studio.view = StudioView::from_token(&self.m.studio_view);
        let text = self.m.studio_dock.clone();
        self.dock_restore(&text);
        self.studio.enter = self.m.layout == Layout::Studio;
    }

    /// Start of a frame: the lanes draw their badges again, and a layout change made in the Display menu is applied.
    pub(super) fn studio_frame_start(&mut self) {
        self.studio.lane_badges.get_mut().clear();
        // coming into Studio by any road (menu, recalled setup, restart) applies its entry rules: its bars show
        let is = self.m.layout == Layout::Studio;
        if is && !self.studio.was { self.studio.enter = true; }
        self.studio.was = is;
        // the view is kept with the setup like the layout; a record view is not (it needs its session)
        let token = self.studio.view.token();
        if self.m.studio_view != token { self.m.studio_view = token.to_string(); }
        if self.m.layout == Layout::Studio { self.studio_prune_ref_rows(); }
        self.dock_sync_setup();
        self.studio_sync();
    }

    /// Applies a layout change made in the Display menu: Studio always draws its sources as lanes.
    pub(super) fn studio_sync(&mut self) {
        self.studio_sync_views();
        if !std::mem::take(&mut self.studio.enter) { return; }
        if self.m.layout == Layout::Studio {
            self.trace_mode(true);
            // Studio is entered with its bars: a full-screen state left from another layout would hide them
            if self.m.density == Density::Full {
                self.m.density = Density::Scope;
                self.apply_density();
                self.relayout();
            }
        }
    }

    /// What the badge at the left of the top bar says about the source of the waveforms.
    pub(super) fn studio_source(&self) -> (&'static str, Rgb) {
        if self.record_mode() { return (if self.session_is_sim { "SIM" } else { "REF" }, ACCENT); }
        match self.src {
            Source::Demo { .. } => ("DEMO", ACCENT),
            Source::Replay { .. } => ("REPLAY", ACCENT),
            _ => ("LIVE", GREEN),
        }
    }

    /// Offline waveform analysis: the badge is the way back to live.
    pub(super) fn studio_back_badge(&self) -> bool {
        self.m.layout == Layout::Studio && self.studio.view == StudioView::Offline && self.record_mode()
    }

    pub(super) fn studio_badge_label(&self) -> &'static str {
        if self.studio_back_badge() { "Live" } else { self.studio_source().0 }
    }

    /// The chip of the acquisition: the sample rate and depth, or while analysing a file its name first.
    pub(super) fn studio_acq_caption(&self) -> String {
        let (caption, _) = self.acq_caption();
        // the source is part of the caption: DEMO and REPLAY here, SIM and REF come with the record caption
        if !self.record_mode() && !matches!(self.src, Source::Hw { .. } | Source::Live { .. }) {
            let name = match self.src { Source::Demo { .. } => "DEMO", _ => "REPLAY" };
            return format!("{name} \u{b7} {caption}");
        }
        // the SIM chip of the bar already names the SIM
        let caption = if self.session_is_sim && self.record_mode() { caption.trim_start_matches("SIM {b7} ").to_string() } else { caption };
        if !self.studio_back_badge() { return caption; }
        match self.refs.get(self.active_reference()).and_then(|r| r.as_ref()) {
            Some(r) => {
                let name = if r.label.is_empty() { format!("R{}", self.active_reference() + 1) } else { r.label.clone() };
                format!("{name} \u{b7} {}", caption.trim_start_matches("REF \u{b7} "))
            }
            None => "No file loaded".to_string(),
        }
    }

    /// Keeps the record views consistent with the session: Offline ends with the record session, and shows
    /// the navigator (the zoom overview) for a loaded file until the user closes it.
    pub(super) fn studio_sync_views(&mut self) {
        if self.m.layout != Layout::Studio || self.studio.view != StudioView::Offline { return; }
        if !self.record_mode() {
            self.studio.view = StudioView::Waveform;
            self.relayout();
        } else if self.refs.iter().any(Option::is_some) && !self.m.zoom && !self.zoom_user_closed {
            self.set_zoom(true);
        }
    }

    /// A record view ends when the session does (`return_to_live`).
    pub(super) fn studio_left_record(&mut self) {
        if self.studio.view.is_record() {
            self.studio.view = StudioView::Waveform;
            self.studio.panel.first = 0;
        }
    }

    pub(super) fn studio_top(&self) -> StudioTop {
        let f = self.fonts();
        let top = self.frame().top;
        // the same sizes as the Default bar: the status pill at the left, small buttons of the bar height
        let btn_w = |label: &str| (f.mb.width(label) + 28).max(40);
        let pill = Rect::new(8, 6, 92.max(30 + f.mb.width("TRIG'D") + 12), 28);
        let single = Rect::new(pill.right() + 4, CHIP_Y, 54, CHIP_H);
        let mut x = single.right() + 6;
        // Live: the way back from a loaded file or a SIM, as in the Default bar
        let badge = if self.record_mode() { Rect::new(x, CHIP_Y, btn_w("Live"), CHIP_H) } else { Rect::new(x, CHIP_Y, 0, CHIP_H) };
        if badge.w > 0 { x = badge.right() + 6; }
        // while the SIM is the source: the preset, a tap opens the SIM panel
        let sim = if self.session_is_sim && self.record_mode() {
            Rect::new(x, CHIP_Y, (f.mb.width(self.sim_preset_name()) + 56).min(220), CHIP_H)
        } else { Rect::new(x, CHIP_Y, 0, CHIP_H) };
        if sim.w > 0 { x = sim.right() + 8; } else { x += 2; }
        // the right group is laid out from the edge
        let mut r = top.right() - 8;
        let gear = Rect::new(r - 40, CHIP_Y, 40, CHIP_H);
        r = gear.x - 6;
        let view = Rect::new(r - btn_w("View"), CHIP_Y, btn_w("View"), CHIP_H);
        r = view.x - 6;
        let cursors = Rect::new(r - btn_w("Cursors"), CHIP_Y, btn_w("Cursors"), CHIP_H);
        r = cursors.x - 6;
        let measure = Rect::new(r - btn_w("Measure"), CHIP_Y, btn_w("Measure"), CHIP_H);
        r = measure.x - 6;
        let add = Rect::new(r - btn_w("+ Add"), CHIP_Y, btn_w("+ Add"), CHIP_H);
        let acq = Rect::new(x, CHIP_Y, (add.x - 10 - x).max(40), CHIP_H);
        StudioTop { add, badge, run: pill, single, sim, acq, measure, cursors, view, gear }
    }

    /// Measure and Cursors as the top-bar buttons (`top_buttons`); the others are Studio's own.
    pub(super) fn studio_top_buttons(&self) -> Vec<(TopBtn, &'static str, Rect)> {
        let t = self.studio_top();
        vec![(TopBtn::Menu(MenuId::Measure), "Measure", t.measure), (TopBtn::Menu(MenuId::Cursor), "Cursors", t.cursors)]
    }

    pub(super) fn draw_studio_top(&self, cv: &mut Canvas) {
        let f = self.fonts();
        let top = self.frame().top;
        cv.fill(top, BAR);
        cv.hline(top.x, top.right() - 1, top.bottom() - 1, LINE);
        let t = self.studio_top();
        let root = self.menu_root();
        let open = |m: MenuId| self.menu_open && root == m;
        // one status pill as in the Default bar; a tap is Run / Stop in every layout
        let (txt, col) = self.run_caption();
        let r = t.run;
        cv.round(r, 6, mix(BAR, col, 80), 255);
        cv.round(r.inset(1), 5, mix(BAR, col, 36), 255);
        cv.circle(r.x + 16, r.y + 14, 4, col);
        cv.text_in(&f.mb, Rect::new(r.x + 30, r.y, r.w - 30, r.h), 0, txt, col);
        btn(cv, t.single, BTN, BTN_BORDER);
        cv.text_in(&f.s, t.single, 1, "Single", TEXT);
        if t.badge.w > 0 {
            btn(cv, t.badge, BTN, ACCENT);
            cv.text_in(&f.mb, t.badge, 1, "Live", ACCENT);
        }
        if t.sim.w > 0 {
            let on = self.studio.view == StudioView::Sim;
            btn(cv, t.sim, BTN, if on { ACCENT } else { BTN_BORDER });
            cv.set_clip(t.sim.inset(2));
            cv.text_in(&f.sb, Rect::new(t.sim.x + 8, t.sim.y, 30, t.sim.h), 0, "SIM", if on { ACCENT } else { DIM });
            cv.text_in(&f.mb, Rect::new(t.sim.x + 40, t.sim.y, (t.sim.w - 44).max(1), t.sim.h), 0, self.sim_preset_name(), if on { ACCENT } else { TEXT });
            cv.no_clip();
        }
        // sample rate and depth as plain text (the source in front of them): a tap opens the page that sets them
        let caption = self.studio_acq_caption();
        let acq_open = self.menu_open && root == if self.record_mode() { MenuId::Ref } else { MenuId::Acquire };
        cv.set_clip(Rect::new(t.acq.x, top.y, t.acq.w, top.h));
        cv.text(&f.mb, t.acq.x, 25, &caption, if acq_open { ACCENT } else { TEXT });
        cv.no_clip();
        // Measure, Cursors, View, settings: the Default button style
        let measuring = open(MenuId::Measure) || self.results_panel();
        let cursors = open(MenuId::Cursor) || self.m.cursor != CursorMode::Off;
        let adding = self.menu_open && matches!(self.menu, MenuId::AddPanel);
        for (r, label, on) in [(t.add, "+ Add", adding), (t.measure, "Measure", measuring), (t.cursors, "Cursors", cursors), (t.view, "View", self.studio.view_menu)] {
            btn(cv, r, BTN, if on { ACCENT } else { BTN_BORDER });
            cv.text_in(&f.mb, r, 1, label, if on { ACCENT } else { TEXT });
        }
        let gear_on = open(MenuId::Main) || open(MenuId::Utility) || self.m.touch_lock;
        btn(cv, t.gear, BTN, if gear_on { ACCENT } else { BTN_BORDER });
        icon_gear(cv, t.gear.x + t.gear.w / 2, t.gear.y + t.gear.h / 2, if gear_on { ACCENT } else { TEXT }, BTN);
    }

    /// The rectangles of the Studio bottom bar: a chip per channel, the time base and the trigger.
    pub(super) fn studio_bottom(&self) -> StudioBottom {
        let bottom = self.frame().bottom;
        let (y, h) = (bottom.y + 3, CHIP_H);
        let n = n_ch();
        let mut right = bottom.right() - 4;
        let panel = self.desktop_panel.map(|_| { right -= 46; let r = Rect::new(right, y, 46, h); right -= 4; r });
        let trig_w = 196;
        let trig = Rect::new(right - trig_w, y, trig_w, h);
        let time = Rect::new(trig.x - 9 - 150, y, 150, h);
        // the sources that are on get a chip; the channels that are off share one "+ CH" chip
        let shown = self.shown_refs();
        let maths: Vec<usize> = (0..NMATH).filter(|&k| self.m.math_ch(k).op != MathOp::Off).collect();
        let off = (0..n).filter(|&c| !self.m.ch[c].on).count();
        // with every channel on the chip stays, as "Add source": math, ref and bus are behind it
        let add_w = if off > 0 { 62 } else { 100 };
        let on = (0..n).filter(|&c| self.m.ch[c].on).count() as i32;
        let small = (shown.len() + maths.len()) as i32 * (REF_CHIP_W + 5) + add_w + 5;
        let room = time.x - 9 - (bottom.x + 4) - small;
        let w_on = if on > 0 { (170).min((room - (on - 1) * 5) / on).max(60) } else { 0 };
        let mut ch = [Rect::new(0, y, 0, h); NCH];
        let mut x = bottom.x + 4;
        for (c, r) in ch.iter_mut().enumerate().take(n) {
            if !self.m.ch[c].on { continue; }
            *r = Rect::new(x, y, w_on, h);
            x += w_on + 5;
        }
        let mut refs = [Rect::new(0, y, 0, h); NREF];
        for &i in &shown {
            refs[i] = Rect::new(x, y, REF_CHIP_W, h);
            x += REF_CHIP_W + 5;
        }
        let mut math = [Rect::new(0, y, 0, h); NMATH];
        for &k in &maths {
            math[k] = Rect::new(x, y, REF_CHIP_W, h);
            x += REF_CHIP_W + 5;
        }
        let add = Rect::new(x, y, add_w, h);
        StudioBottom { ch, refs, math, add, time, trig, panel }
    }

    pub(super) fn studio_cards(&self) -> Cards {
        let b = self.studio_bottom();
        Cards { ch: b.ch, la: None, gen: None, refs: None, time: b.time, trig: b.trig }
    }

    pub(super) fn draw_studio_bottom(&self, cv: &mut Canvas) {
        let f = self.fonts();
        let bottom = self.frame().bottom;
        cv.fill(bottom, BAR);
        self.draw_studio_badges(cv);
        cv.hline(bottom.x, bottom.right() - 1, bottom.y, LINE);
        let b = self.studio_bottom();
        let root = if self.menu_open { Some(self.menu_root()) } else { None };
        for c in 0..n_ch() {
            let r = b.ch[c];
            if r.w == 0 { continue; }
            let (col, ch) = (CH_COLORS[c], &self.m.ch[c]);
            let sel = root == Some(MenuId::Channel(c));
            let border = if sel { ACCENT } else if ch.on { col } else { BTN_BORDER };
            chip(cv, r, if ch.on { BTN } else { BAR }, border);
            // the channel badge: filled while the channel is on
            let badge = Rect::new(r.x + 8, r.y + 5, 42, 20);
            cv.round(badge, 4, if ch.on { col } else { mix(BAR, col, 70) }, 255);
            cv.text_in(&f.sb, badge, 1, &format!("CH{}", c + 1), if ch.on { DARK_TEXT } else { mix(BAR, col, 200) });
            let text = if ch.on { format!("{}/div", si_short(ch.volts_per_div() as f64, ch.unit.name())) } else { "Off".into() };
            cv.set_clip(Rect::new(badge.right() + 2, r.y, (r.right() - 18 - badge.right() - 2).max(1), r.h));
            cv.text_in(&f.mb, Rect::new(badge.right() + 8, r.y, (r.right() - 20 - badge.right() - 8).max(1), r.h), 0, &text, if ch.on { TEXT } else { DIM });
            cv.no_clip();
            chevron_down(cv, r.right() - 16, r.y + r.h / 2 - 2, DIM);
        }
        for i in self.shown_refs() {
            let r = b.refs[i];
            let col = self.wave_color(WaveId::Ref(i));
            let active = self.selected_workspace_source() == Some(WaveId::Ref(i));
            chip(cv, r, BTN, if active { ACCENT } else { col });
            cv.text_in(&f.sb, r, 1, &format!("R{}", i + 1), col);
        }
        for k in 0..NMATH {
            let r = b.math[k];
            if r.w == 0 { continue; }
            let col = MATH_COLORS[k % MATH_COLORS.len()];
            chip(cv, r, BTN, if self.m.math_sel == k && self.selected_workspace_source().is_none() { ACCENT } else { col });
            cv.text_in(&f.sb, r, 1, &format!("M{}", k + 1), col);
        }
        if b.add.w > 0 {
            chip(cv, b.add, BTN, if self.studio.ch_picker { ACCENT } else { BTN_BORDER });
            let label = if (0..n_ch()).all(|c| self.m.ch[c].on) { "Add source" } else { "+ CH" };
            cv.text_in(&f.sb, b.add, 1, label, if self.studio.ch_picker { ACCENT } else { TEXT });
        }
        // the sections of the row are told apart by a line
        for x in [b.time.x - 5, b.trig.x - 5] {
            cv.vline(x, bottom.y + 6, bottom.bottom() - 6, LINE);
        }
        // time base
        let r = b.time;
        chip(cv, r, BTN, if root == Some(MenuId::Horizontal) { ACCENT } else { BTN_BORDER });
        let cy = r.y + r.h / 2;
        cv.hline(r.x + 10, r.x + 24, cy, TEXT);
        cv.line(r.x + 10, cy, r.x + 14, cy - 3, TEXT);
        cv.line(r.x + 10, cy, r.x + 14, cy + 3, TEXT);
        cv.line(r.x + 24, cy, r.x + 20, cy - 3, TEXT);
        cv.line(r.x + 24, cy, r.x + 20, cy + 3, TEXT);
        cv.text_in(&f.mb, Rect::new(r.x + 32, r.y, r.w - 52, r.h), 0, &format!("{}/div", si_short(self.m.time_per_div() as f64, "s")), TEXT);
        chevron_down(cv, r.right() - 16, cy - 2, DIM);
        // trigger: the slope, the source and the level
        let r = b.trig;
        chip(cv, r, BTN, if root == Some(MenuId::Trigger) { ACCENT } else { BTN_BORDER });
        let source = self.m.trigger_wave();
        let col = self.wave_color(source);
        let edge = matches!(self.m.trig_type, TrigType::Edge | TrigType::Slope | TrigType::Timeout | TrigType::NthEdge | TrigType::Delay);
        let mut x = r.x + 9;
        cv.set_clip(r.inset(2));
        if edge {
            draw_slope(cv, x, r.y + 7, self.m.slope, col);
            x += 20;
        } else {
            x = cv.text(&f.sb, x, r.y + 20, &self.m.trig_type.name().to_uppercase(), LABEL) + 6;
        }
        x = cv.text(&f.mb, x, r.y + 20, &self.trigger_source_label(), col) + 6;
        let levels = if matches!(self.m.trig_type, TrigType::Runt | TrigType::Slope | TrigType::Window) {
            format!("{} / {}", si_short(self.m.trig_level2 as f64, "V"), si_short(self.m.trig_level as f64, "V"))
        } else { si_short(self.trig_level_shown() as f64, "V") };
        if edge && self.m.slope != Slope::Either {
            // the arrow of the edge: up for rising, down for falling
            let (top, bot) = (r.y + 9, r.y + 21);
            cv.vline(x + 3, top, bot, TEXT);
            let (tip, base) = if self.m.slope == Slope::Rise { (top, top + 4) } else { (bot, bot - 4) };
            cv.line(x + 3, tip, x, base, TEXT);
            cv.line(x + 3, tip, x + 6, base, TEXT);
            x += 12;
        }
        cv.text(&f.mb, x, r.y + 20, &levels, TEXT);
        cv.no_clip();
        chevron_down(cv, r.right() - 16, r.y + r.h / 2 - 2, DIM);
        if let Some(r) = b.panel {
            let on = self.desktop_panel == Some(true);
            chip(cv, r, BTN, if on { ACCENT } else { BTN_BORDER });
            cv.text_in(&f.s, r, 1, "Panel", if on { ACCENT } else { TEXT });
        }
        self.draw_studio_ch_picker(cv);
    }

    fn studio_bottom_tap(&mut self, x: i32, y: i32) {
        let b = self.studio_bottom();
        if b.add.w > 0 && b.add.contains(x, y) {
            self.studio.ch_picker = !self.studio.ch_picker;
            return;
        }
        if let Some(k) = (0..NMATH).find(|&k| b.math[k].w > 0 && b.math[k].contains(x, y)) {
            self.m.math_select(k);
            return;
        }
        for c in 0..n_ch() {
            if !b.ch[c].contains(x, y) { continue; }
            if !self.m.ch[c].on {
                self.m.ch[c].on = true;
                self.settings_changed();
                self.relayout();
            }
            self.select_workspace_source(WaveId::Ch(c));
            return self.toggle_menu(MenuId::Channel(c));
        }
        if let Some(i) = self.shown_refs().into_iter().find(|&i| b.refs[i].contains(x, y)) {
            return self.select_workspace_source(WaveId::Ref(i));
        }
        if b.time.contains(x, y) {
            self.toggle_menu(MenuId::Horizontal);
        } else if b.trig.contains(x, y) {
            self.toggle_menu(MenuId::Trigger);
        } else if b.panel.is_some_and(|r| r.contains(x, y)) {
            self.desktop_panel = self.desktop_panel.map(|open| !open);
        }
    }

    /// The picker over the "+ CH" chip: one row per channel that is off.
    pub(super) fn studio_ch_picker(&self) -> Option<Vec<(usize, Rect)>> {
        let (chans, _) = self.studio_picker_rows()?;
        (!chans.is_empty()).then_some(chans)
    }

    /// The popup of the "+ CH" chip: the channels that are off, then (set apart by a line) Add math, Add ref and
    /// Add bus. Rows 44 px high, the panel over the chip.
    pub(super) fn studio_picker_rows(&self) -> Option<(Vec<(usize, Rect)>, Vec<(PickAdd, Rect)>)> {
        if !self.studio.ch_picker { return None; }
        let b = self.studio_bottom();
        // the channels that are off; with every channel on, all of them (to turn off)
        let mut off: Vec<usize> = (0..n_ch()).filter(|&c| !self.m.ch[c].on).collect();
        if off.is_empty() { off = (0..n_ch()).collect(); }
        let (w, row) = (170, 44);
        let sep = if off.is_empty() { 0 } else { 10 };
        let total = (off.len() + PICK_ADDS.len()) as i32 * row + sep;
        let top = self.frame().bottom.y - 8 - total - 8;
        let x = b.add.x.min(self.scr.w - w - 4);
        let chans = off.iter().enumerate().map(|(i, &c)| (c, Rect::new(x + 4, top + 4 + i as i32 * row, w - 8, row))).collect();
        let y0 = top + 4 + off.len() as i32 * row + sep;
        let extras = PICK_ADDS.iter().enumerate().map(|(i, &k)| (k, Rect::new(x + 4, y0 + i as i32 * row, w - 8, row))).collect();
        Some((chans, extras))
    }

    /// What a row of the popup's second group opens: the flows of the + Add panel and the analysis menus.
    fn studio_pick_add(&mut self, k: PickAdd) {
        match k {
            PickAdd::Math => self.open_submenu(MenuId::Math),
            PickAdd::Ref => self.open_submenu(MenuId::Ref),
            PickAdd::Bus => self.open_submenu(MenuId::Decode),
        }
    }

    fn draw_studio_ch_picker(&self, cv: &mut Canvas) {
        let Some((rows, extras)) = self.studio_picker_rows() else { return };
        let f = self.fonts();
        let first = rows.first().map_or(extras[0].1, |r| r.1);
        let last = extras[extras.len() - 1].1;
        let p = Rect::new(first.x - 4, first.y - 4, first.w + 8, last.bottom() - first.y + 8 + if rows.is_empty() { 0 } else { 0 });
        cv.round(p, 8, BTN_BORDER, 255);
        cv.round(p.inset(1), 7, PANEL_BG, 255);
        if !rows.is_empty() { cv.hline(extras[0].1.x + 6, extras[0].1.right() - 6, extras[0].1.y - 5, LINE); }
        for (k, r) in &extras {
            let col = match k { PickAdd::Math => MATH_COLORS[0], PickAdd::Ref => REF_COLORS[0].1, PickAdd::Bus => super::bus::BUS };
            let badge = Rect::new(r.x + 8, r.y + 10, 6, 24);
            cv.round(badge, 3, col, 255);
            cv.text_in(&f.m, Rect::new(badge.right() + 10, r.y, r.w - 30, r.h), 0, k.label(), TEXT);
        }
        for (c, r) in rows {
            let badge = Rect::new(r.x + 8, r.y + 10, 42, 24);
            cv.round(badge, 4, mix(BAR, CH_COLORS[c], 200), 255);
            cv.text_in(&f.sb, badge, 1, &format!("CH{}", c + 1), DARK_TEXT);
            cv.text_in(&f.m, Rect::new(badge.right() + 10, r.y, r.w - 64, r.h), 0, if self.m.ch[c].on { "Turn off" } else { "Turn on" }, TEXT);
        }
    }

    /// A tap in Studio's own bars and panels. False: the tap belongs to the plot, a menu or a dialog.
    pub(super) fn studio_tap(&mut self, x: i32, y: i32) -> bool {
        self.studio_sync();
        if self.power_dlg || self.wlan.osk.is_some() || self.dialog.is_some() || self.load_slot_ask { return false; }
        if self.menu_open && self.menu_rect().is_some_and(|d| d.contains(x, y)) { return false; }
        if self.studio_view_tap(x, y) { return true; }
        if self.studio.ch_picker {
            let (rows, extras) = self.studio_picker_rows().unzip();
            self.studio.ch_picker = false;
            if let Some((k, _)) = extras.into_iter().flatten().find(|(_, r): &(PickAdd, Rect)| r.contains(x, y)) {
                self.studio_pick_add(k);
                return true;
            }
            if let Some((c, _)) = rows.into_iter().flatten().find(|(_, r): &(usize, Rect)| r.contains(x, y)) {
                let on = !self.m.ch[c].on;
                self.m.ch[c].on = on;
                self.settings_changed();
                self.relayout();
                if on { self.select_workspace_source(WaveId::Ch(c)); }
                return true;
            }
            // a tap on the chip that opened it only closes it; any other tap closes it and goes on
            if self.studio_bottom().add.contains(x, y) { return true; }
        }
        if self.md_menu_tap(x, y) { return true; }
        if self.studio_popup_tap(x, y) { return true; }
        if let Some(k) = self.dock_float_at(x, y) { return self.dock_float_tap(k, x, y); }
        if self.studio_zoom_tap(x, y) { return true; }
        if self.studio_badge_tap(x, y) { return true; }
        if self.studio_panel_tap(x, y) { return true; }
        if y >= self.frame().bottom.y - 2 {
            self.studio_bottom_tap(x, self.bar_y(y));
            return true;
        }
        if y < self.frame().top.bottom() {
            self.studio_top_tap(x, y);
            return true;
        }
        false
    }

    /// The status pill: Run / Stop as in every layout. While the SIM panel is open and nothing plays yet, Run plays
    /// the SIM source (the same action as the panel's Play).
    pub(super) fn studio_run_stop(&mut self) {
        if self.studio.view == StudioView::Sim && !self.record_mode() { return self.sim_activate_play(); }
        self.key(key::RUNSTOP);
    }

    /// Single: one acquisition, or one SIM frame that is then held.
    pub(super) fn studio_single(&mut self) {
        if self.studio.view == StudioView::Sim && !self.record_mode() {
            if self.sim_generating() { return; }
            return self.sim_request_ui(false);
        }
        self.key(key::SINGLE);
    }

    fn sim_activate_play(&mut self) {
        self.sim_activate(Item::SimPlay);
    }

    fn studio_top_tap(&mut self, x: i32, y: i32) {
        let t = self.studio_top();
        let hit = |r: Rect| self.top_hit(r).contains(x, y);
        if t.badge.w > 0 && hit(t.badge) {
            self.return_to_live();
        } else if hit(t.run) {
            self.studio_run_stop();
        } else if hit(t.single) {
            self.studio_single();
        } else if t.sim.w > 0 && hit(t.sim) {
            if self.menu_open { self.close_menu(); }
            self.studio_set_view(StudioView::Sim);
        } else if hit(t.acq) {
            self.toggle_menu(if self.record_mode() { MenuId::Ref } else { MenuId::Acquire });
        } else if hit(t.add) {
            if self.menu_open && self.menu == MenuId::AddPanel { self.close_menu(); } else { self.open_menu(MenuId::AddPanel); self.sheet.add.kind = None; }
        } else if hit(t.measure) {
            self.open_meas_list();
        } else if hit(t.cursors) {
            self.studio_cursors_tap();
        } else if hit(t.view) {
            if self.menu_open { self.close_menu(); }
            self.studio.popup = None;
            self.studio.view_menu = true;
        } else if hit(t.gear) {
            if self.m.touch_lock { self.key(key::TOUCHLOCK); } else { self.toggle_menu(MenuId::Main); }
        }
    }
}

/// The cursor band and its C1 / C2 tags.
const BAND: Rgb = rgb(0x2d, 0xd4, 0xcf);
/// Height of the row above the lanes that carries the T marker and the cursor tags.
const HEAD_H: i32 = 18;

impl Ui {
    /// Studio keeps a row above the lanes for the trigger position and the cursor tags.
    pub(super) fn studio_head_h(&self) -> i32 {
        if self.m.layout == Layout::Studio && !self.m.zoom && !self.full_on() { HEAD_H } else { 0 }
    }

    /// Screen x of a time (relative to the trigger) in the main view of the plot.
    pub(super) fn studio_x_of_time(&self, t: f64) -> i32 {
        let g = self.grid;
        let (tpd, delay) = self.view();
        g.x + ((0.5 + (t - delay as f64) / (HDIV as f64 * tpd as f64)) * g.w as f64).round() as i32
    }

    /// The channel badge at the left edge of a lane, on the channel's ground line (`draw_markers`).
    pub(super) fn studio_lane_badge(&self, cv: &mut Canvas, c: usize, y: i32) {
        let a = self.area;
        let col = CH_COLORS[c];
        let r = Rect::new(a.x + 3, y - 9, 44, 18);
        self.studio.lane_badges.borrow_mut().push((c, r));
        cv.round(r, 4, col, 255);
        cv.round(r.inset(1), 3, BG, 255);
        cv.text_in(&self.fonts().sb, r, 1, &format!("CH{}", c + 1), col);
    }

    /// The T marker over the lanes, the cursor tags and the shaded band between the cursors.
    pub(super) fn draw_studio_overlay(&self, cv: &mut Canvas) {
        let head = self.studio_head_h();
        if head == 0 || self.view_on() || self.eye_open() { return; }
        let f = self.fonts();
        let g = self.grid;
        let row = Rect::new(g.x, g.y - head, g.w, head);
        cv.set_clip(Rect::new(g.x - 12, row.y, g.w + 24, g.h + head));
        // the shaded band between two time cursors
        if matches!(self.m.cursor, CursorMode::Time | CursorMode::Track | CursorMode::Xy) {
            // a cursor keeps its time while zoomed: its fraction may lie outside the view, then it is not drawn
            let fr = [self.m.cur_a, self.m.cur_b];
            let xs = fr.map(|frac| g.x + (frac.clamp(0.0, 1.0) * g.w as f32) as i32);
            let (x0, x1) = (xs[0].min(xs[1]), xs[0].max(xs[1]));
            cv.set_clip(Rect::new(g.x, g.y, g.w, g.h));
            let off_same_side = (fr[0] < 0.0 && fr[1] < 0.0) || (fr[0] > 1.0 && fr[1] > 1.0);
            for &lane in &self.wins {
                if !off_same_side { cv.blend(Rect::new(x0, lane.y, (x1 - x0).max(1), lane.h), BAND, 46); }
            }
            cv.set_clip(Rect::new(g.x - 12, row.y, g.w + 24, g.h + head));
            for (k, (x, name)) in [(xs[0], "C1"), (xs[1], "C2")].into_iter().enumerate() {
                if !(0.0..=1.0).contains(&fr[k]) { continue; }
                let r = Rect::new((x - 11).clamp(g.x, g.right() - 22), row.y + 1, 22, head - 3);
                cv.round(r, 3, BAND, 255);
                cv.text_in(&f.sb, r, 1, name, DARK_TEXT);
                if self.workspace.cursor_sel == Some(k) { cv.round_outline(Rect::new(r.x - 2, r.y - 2, r.w + 4, r.h + 4), 3, ACCENT); }
                cv.vline(x, row.bottom() - 2, g.y, BAND);
            }
        }
        // T: where the trigger is in the record, dashed through every lane; at an edge when it is off the screen
        let tx = self.studio_x_of_time(0.0);
        let r = Rect::new(tx.clamp(g.x + 11, g.right() - 11) - 11, row.y + 1, 22, head - 3);
        cv.round(r, 3, AMBER, 255);
        cv.round(r.inset(1), 2, BG, 255);
        cv.text_in(&f.sb, r, 1, "T", AMBER);
        if (g.x..g.right()).contains(&tx) {
            cv.triangle([(tx - 3, r.bottom()), (tx + 3, r.bottom()), (tx, r.bottom() + 3)], AMBER);
            for &lane in &self.wins {
                cv.dashed_vline(tx, lane.y, lane.y + lane.h - 1, 4, 4, AMBER);
            }
        } else if tx < g.x {
            cv.triangle([(r.x - 1, r.y + r.h / 2), (r.x + 4, r.y + 2), (r.x + 4, r.bottom() - 2)], AMBER);
        } else {
            cv.triangle([(r.right() + 1, r.y + r.h / 2), (r.right() - 4, r.y + 2), (r.right() - 4, r.bottom() - 2)], AMBER);
        }
        cv.no_clip();
    }
}

/// Height of the row of pinned badges under the lanes.
const ROW_H: i32 = 34;
const ROW_H_COMPACT: i32 = 26;
/// Width of the action popup of a badge, and the height of its rows.
const POPUP_W: i32 = 300;
const POPUP_ROW: i32 = 34;

/// Two short stacked bars that move together: the compact view.
fn icon_compact(cv: &mut Canvas, cx: i32, cy: i32, c: Rgb) {
    for dy in [-5, 0, 5] { cv.hline(cx - 7, cx + 7, cy + dy, c); }
}

fn icon_pin(cv: &mut Canvas, x: i32, cy: i32, filled: bool, c: Rgb) {
    // a round head on a needle that leans to the left
    cv.line(x + 6, cy - 1, x + 1, cy + 6, c);
    cv.circle(x + 8, cy - 3, 3, c);
    if filled {
        cv.fill(Rect::new(x + 4, cy - 1, 8, 2), c);
    } else {
        cv.circle(x + 8, cy - 3, 1, BTN);
        cv.hline(x + 4, x + 11, cy - 1, c);
    }
}

fn icon_eye(cv: &mut Canvas, cx: i32, cy: i32, c: Rgb) {
    cv.round_outline(Rect::new(cx - 8, cy - 5, 17, 11), 5, c);
    cv.circle(cx, cy, 2, c);
}

impl Ui {
    /// The measurements in the order of the list: single, pair, catalogue.
    pub(super) fn studio_keys(&self) -> Vec<MeasKey> {
        let m = &self.m;
        m.meas.iter().map(|&(k, id)| MeasKey::One(k, id))
            .chain(m.meas2.iter().map(|&(k, a, b)| MeasKey::Pair(k, a, b)))
            .chain(m.cat.iter().map(|&(i, a, b, c, d)| MeasKey::Cat(i, a, b, c, d)))
            .collect()
    }

    /// A source that shows a signal now: a channel that is on, a ref that is shown.
    pub(super) fn studio_source_live(&self, id: WaveId) -> bool {
        match id {
            WaveId::Ch(c) => c < n_ch() && self.m.ch[c].on,
            WaveId::Ref(i) => self.refs.get(i).and_then(Option::as_ref).is_some_and(|r| r.on),
        }
    }

    /// The source a measurement reads first.
    fn studio_key_source(key: MeasKey) -> WaveId {
        match key { MeasKey::One(_, id) | MeasKey::Pair(_, id, _) | MeasKey::Cat(_, id, _, _, _) => id }
    }

    /// The measurements shown as badges: those pinned, or the first three until the user pins one. Only those of a
    /// source that is live: no "CH1 ----" while CH1 is off and a ref is the signal.
    pub(super) fn studio_pinned(&self) -> Vec<MeasKey> {
        let keys: Vec<MeasKey> = self.studio_keys().into_iter().filter(|k| self.studio_source_live(Self::studio_key_source(*k))).collect();
        match &self.studio.pinned {
            Some(pinned) => keys.into_iter().filter(|k| pinned.contains(k)).collect(),
            None => keys.into_iter().take(3).collect(),
        }
    }

    /// Measurements of a ref that was hidden or removed leave the list with it.
    pub(super) fn studio_prune_ref_rows(&mut self) {
        let gone = |s: &Ui, id: WaveId| matches!(id, WaveId::Ref(_)) && !s.studio_source_live(id);
        if !(self.m.meas.iter().any(|&(_, id)| gone(self, id)) || self.m.meas2.iter().any(|&(_, a, b)| gone(self, a) || gone(self, b))) { return; }
        let (meas, meas2) = (self.m.meas.clone(), self.m.meas2.clone());
        self.m.meas = meas.into_iter().filter(|&(_, id)| !gone(self, id)).collect();
        self.m.meas2 = meas2.into_iter().filter(|&(_, a, b)| !gone(self, a) && !gone(self, b)).collect();
        let keys = self.studio_keys();
        if let Some(p) = &mut self.studio.pinned { p.retain(|k| keys.contains(k)); }
        self.studio.floated.retain(|k| keys.contains(k));
        self.studio.popup = None;
        self.studio.panel.first = 0;
        self.relayout();
    }

    /// The measurements of the floating panel: the pinned ones and those moved there from a badge.
    pub(super) fn studio_float_keys(&self) -> Vec<MeasKey> {
        let pinned = self.studio_pinned();
        self.studio_keys().into_iter().filter(|k| pinned.contains(k) || self.studio.floated.contains(k)).collect()
    }

    pub(super) fn studio_is_pinned(&self, key: MeasKey) -> bool {
        self.studio_pinned().contains(&key)
    }

    pub(super) fn studio_set_pin(&mut self, key: MeasKey, on: bool) {
        let mut pinned = self.studio_pinned();
        pinned.retain(|k| *k != key);
        if on { pinned.push(key); }
        self.studio.pinned = Some(pinned);
        self.relayout();
    }

    /// Index of a measurement in the list (`meas_rows`).
    pub(super) fn studio_index(&self, key: MeasKey) -> Option<usize> {
        self.studio_keys().iter().position(|k| *k == key)
    }

    pub(super) fn studio_owns_measurements(&self) -> bool {
        self.m.layout == Layout::Studio
    }

    /// Whether the row under the lanes is there: pinned measurements, or the cursor read-out, unless hidden.
    pub(super) fn studio_badges_visible(&self) -> bool {
        self.m.layout == Layout::Studio && !self.studio.hide_badges && self.studio.view != StudioView::Only
            && (!self.studio_pinned().is_empty() || self.m.cursor != CursorMode::Off || self.studio.search.is_some())
    }

    /// Height the badge row takes from the lanes.
    pub(super) fn studio_row_h(&self) -> i32 {
        if self.studio_badges_visible() { self.studio_row_px() } else { 0 }
    }

    /// The badge row: under the lanes, as wide as they are.
    pub(super) fn studio_row(&self) -> Rect {
        let (x, w) = self.plot_span();
        let h = self.studio_row_px();
        Rect::new(x, self.dock_wave_rect().bottom() - h, w, h)
    }

    /// Height of the badge row, and of the chips in it.
    pub(super) fn studio_row_px(&self) -> i32 { if self.studio.compact { ROW_H_COMPACT } else { ROW_H } }
    pub(super) fn studio_chip_h(&self) -> i32 { self.studio_row_px() - 6 }
    /// Width of the measurement panel and height of its rows: smaller in the compact view (not the SIM panel).
    pub(super) fn studio_panel_w(&self) -> i32 {
        if self.studio.compact && self.studio.view != StudioView::Sim { 210 } else { MEAS_PANEL_W_FULL }
    }
    pub(super) fn studio_panel_row(&self) -> i32 {
        if self.studio.compact && self.studio.view != StudioView::Sim { 36 } else { PANEL_ROW_FULL }
    }
    /// The small switch for the compact view, left of the Meas tab in the badge row.
    pub(super) fn studio_compact_chip(&self) -> Rect {
        let eye = self.studio_eye();
        let tab_w = if self.studio_tab().is_some() { 108 } else { 0 };
        Rect::new(eye.x - 6 - tab_w - 34, eye.y, 34, eye.h)
    }

    /// The eye button at the right end of the row.
    pub(super) fn studio_eye(&self) -> Rect {
        let r = self.studio_row();
        let h = self.studio_chip_h();
        Rect::new(r.right() - 38, r.y + (r.h - h) / 2, 34, h)
    }

    /// Where the badges end: before the Meas tab and the eye.
    fn studio_row_limit(&self) -> i32 {
        self.studio_compact_chip().x - 6
    }

    /// The row of badges as (measurement, rectangle), as many as fit.
    pub(super) fn studio_badges(&self) -> Vec<(MeasKey, Rect)> {
        let f = self.fonts();
        let row = self.studio_row();
        let rows = self.meas_rows();
        let limit = self.studio_row_limit();
        let mut x = row.x + 4 + self.studio_search_w();
        let mut out = Vec::new();
        for key in self.studio_pinned() {
            let Some(r) = self.studio_index(key).and_then(|i| rows.get(i)).filter(|r| !self.meas_row_hidden(r)) else { continue };
            let w = 14 + 6 + (f.sb.width("CH4") + 10) + 6 + f.s.width(&r.name) + 8 + f.sb.width(&r.text()).max(f.sb.width("0.000 MHz")) + 10;
            if x + w > limit { break; }
            out.push((key, Rect::new(x, row.y + (row.h - self.studio_chip_h()) / 2, w, self.studio_chip_h())));
            x += w + 5;
        }
        out
    }

    /// What the cursors read, as one chip after the badges: the differences only.
    pub(super) fn studio_cursor_chip(&self, after: i32) -> Option<(Rect, String)> {
        if self.m.cursor == CursorMode::Off { return None; }
        let badge = self.studio.cur_badge;
        let text = self.cursor_values().into_iter().filter(|(l, _)| badge || l.contains('\u{394}'))
            .map(|(l, v)| format!("{l} {v}")).collect::<Vec<_>>().join("   ");
        if text.is_empty() { return None; }
        let w = 30 + self.fonts().sb.width(&text) + 10;
        let row = self.studio_row();
        (after + w <= self.studio_row_limit()).then(|| (Rect::new(after, row.y + (row.h - self.studio_chip_h()) / 2, w, self.studio_chip_h()), text))
    }

    pub(super) fn draw_studio_badges(&self, cv: &mut Canvas) {
        if !self.studio_badges_visible() { return; }
        let f = self.fonts();
        let row = self.studio_row();
        cv.fill(row, BG);
        let rows = self.meas_rows();
        self.draw_studio_search_chip(cv);
        let mut end = row.x + 4 + self.studio_search_w();
        for (key, r) in self.studio_badges() {
            let Some(m) = self.studio_index(key).and_then(|i| rows.get(i)) else { continue };
            let col = self.wave_color(m.src);
            chip(cv, r, BTN, if self.studio.popup == Some(key) { ACCENT } else { BTN_BORDER });
            let cy = r.y + r.h / 2;
            icon_pin(cv, r.x + 8, cy, true, TEXT);
            let tag = Rect::new(r.x + 28, r.y + 5, f.sb.width("CH4") + 10, r.h - 10);
            cv.round(tag, 4, col, 255);
            cv.text_in(&f.sb, tag, 1, &self.wave_name(m.src), DARK_TEXT);
            let x = cv.text(&f.s, tag.right() + 6, cy + 4, &m.name, DIM) + 8;
            cv.set_clip(Rect::new(x, r.y, (r.right() - x - 4).max(1), r.h));
            cv.text(&f.sb, x, cy + 4, &m.text(), TEXT);
            cv.no_clip();
            end = r.right() + 5;
        }
        if let Some((r, text)) = self.studio_cursor_chip(end) {
            chip(cv, r, BAR, BTN_BORDER);
            icon_cursors(cv, r.x + 9, r.y + r.h / 2, BAND);
            cv.text_in(&f.sb, Rect::new(r.x + 28, r.y, r.w - 30, r.h), 0, &text, TEXT);
        }
        self.draw_studio_tab(cv);
        let c = self.studio_compact_chip();
        chip(cv, c, if self.studio.compact { mix(BAR, ACCENT, 60) } else { BTN }, if self.studio.compact { ACCENT } else { BTN_BORDER });
        icon_compact(cv, c.x + c.w / 2, c.y + c.h / 2, if self.studio.compact { ACCENT } else { TEXT });
        let eye = self.studio_eye();
        cv.vline(eye.x - 3, row.y + 6, row.bottom() - 6, LINE);
        icon_eye(cv, eye.x + eye.w / 2, eye.y + eye.h / 2, TEXT);
    }

    /// The popup of a badge: where it is.
    pub(super) fn studio_popup_rect(&self) -> Option<Rect> {
        let key = self.studio.popup?;
        let (_, anchor) = self.studio_badges().into_iter().find(|(k, _)| *k == key)?;
        let h = 40 + 3 * POPUP_ROW + 10 + POPUP_ROW + 6;
        let x = (anchor.x + anchor.w / 2 - POPUP_W / 2).clamp(4, self.scr.w - POPUP_W - 4);
        Some(Rect::new(x, anchor.y - h - 8, POPUP_W, h))
    }

    /// Row `i` of the popup: 0 Configure, 1 Unpin, 2 Float, 3 Remove.
    pub(super) fn studio_popup_row(p: Rect, i: i32) -> Rect {
        let y = p.y + 40 + i * POPUP_ROW + if i == 3 { 10 } else { 0 };
        Rect::new(p.x + 4, y, p.w - 8, POPUP_ROW)
    }

    /// What floats over the lanes and the bars: the badge popup.
    pub(super) fn draw_studio_floating(&self, cv: &mut Canvas) {
        self.draw_studio_zoom_menu(cv);
        self.draw_studio_yzoom(cv);
        let (Some(key), Some(p)) = (self.studio.popup, self.studio_popup_rect()) else { return };
        let f = self.fonts();
        let rows = self.meas_rows();
        let Some(m) = self.studio_index(key).and_then(|i| rows.get(i)) else { return };
        cv.round(p, 8, BTN_BORDER, 255);
        cv.round(p.inset(1), 7, PANEL_BG, 255);
        if let Some((_, a)) = self.studio_badges().into_iter().find(|(k, _)| *k == key) {
            let x = a.x + a.w / 2;
            cv.triangle([(x - 7, p.bottom() - 1), (x + 7, p.bottom() - 1), (x, p.bottom() + 6)], BTN_BORDER);
            cv.triangle([(x - 6, p.bottom() - 2), (x + 6, p.bottom() - 2), (x, p.bottom() + 4)], PANEL_BG);
        }
        let tag = Rect::new(p.x + 10, p.y + 9, f.sb.width("CH4") + 10, 22);
        cv.round(tag, 4, self.wave_color(m.src), 255);
        cv.text_in(&f.sb, tag, 1, &self.wave_name(m.src), DARK_TEXT);
        let x = cv.text(&f.mb, tag.right() + 8, p.y + 25, &m.name, TEXT) + 10;
        cv.text(&f.mb, x, p.y + 25, &m.text(), TEXT);
        let (cx, cy) = (p.right() - 24, p.y + 20);
        cv.line(cx - 5, cy - 5, cx + 5, cy + 5, TEXT);
        cv.line(cx - 5, cy + 5, cx + 5, cy - 5, TEXT);
        cv.hline(p.x + 8, p.right() - 9, p.y + 38, LINE);
        for (i, (label, col)) in [("Configure...", TEXT), ("Unpin from waveform", TEXT), ("Move to floating panel", TEXT), ("Remove measurement", RED)].into_iter().enumerate() {
            let r = Self::studio_popup_row(p, i as i32);
            if i == 3 { cv.hline(p.x + 8, p.right() - 9, r.y - 5, LINE); }
            let (ix, iy) = (r.x + 18, r.y + r.h / 2);
            match i {
                0 => icon_gear(cv, ix, iy, col, PANEL_BG),
                1 => { icon_pin(cv, ix - 6, iy, false, col); cv.line(ix - 8, iy + 8, ix + 8, iy - 8, col); }
                2 => { cv.round_outline(Rect::new(ix - 8, iy - 6, 14, 12), 2, col); cv.line(ix, iy, ix + 8, iy - 8, col); }
                _ => { cv.round_outline(Rect::new(ix - 7, iy - 5, 13, 13), 2, col); cv.hline(ix - 9, ix + 8, iy - 7, col); }
            }
            cv.text_in(&f.mb, Rect::new(r.x + 42, r.y, r.w - 44, r.h), 0, label, col);
        }
    }

    /// A tap while a badge popup is open: it either does one of its actions or closes it. Always taken.
    fn studio_popup_tap(&mut self, x: i32, y: i32) -> bool {
        let Some(key) = self.studio.popup else { return false };
        let Some(p) = self.studio_popup_rect() else { self.studio.popup = None; return false };
        self.studio.popup = None;
        if !p.contains(x, y) { return true; }
        let hit = (0..4).find(|&i| Self::studio_popup_row(p, i).contains(x, y));
        let Some(index) = self.studio_index(key) else { return true };
        match hit {
            Some(0) => self.open_meas_cfg(index),
            Some(1) => self.studio_set_pin(key, false),
            Some(2) => {
                // only this measurement moves: off the badges, into the floating panel; the view stays unless no panel shows
                self.studio_set_pin(key, false);
                if !self.studio.floated.contains(&key) { self.studio.floated.push(key); }
                if self.studio.view == StudioView::Waveform { self.studio.view = StudioView::Floating; }
                self.relayout();
            }
            Some(3) => {
                self.studio_set_pin(key, false);
                self.remove_measurement(index);
            }
            _ => {}
        }
        true
    }

    /// A tap in the badge row: the eye hides it, a badge opens its popup.
    fn studio_badge_tap(&mut self, x: i32, y: i32) -> bool {
        if !self.studio_badges_visible() || !self.studio_row().contains(x, y) { return false; }
        if self.studio_search_chip_tap(x, y) { return true; }
        if self.studio_eye().contains(x, y) {
            self.studio.hide_badges = true;
            self.relayout();
            return true;
        }
        if self.studio_compact_chip().contains(x, y) {
            self.studio_set_compact(!self.studio.compact);
            return true;
        }
        if self.studio_tab().is_some_and(|tab| tab.contains(x, y)) {
            self.studio_set_view(StudioView::Docked);
            return true;
        }
        if let Some((key, _)) = self.studio_badges().into_iter().find(|(_, r)| r.contains(x, y)) {
            self.studio.popup = Some(key);
        }
        true
    }
}

/// The View menu: its width and the height of a row.
const VIEW_W: i32 = 340;
const VIEW_ROW: i32 = 40;

/// What the View menu offers, in order.
pub(super) const STUDIO_VIEWS: [(StudioView, &str); 6] = [
    (StudioView::Waveform, "Waveform focus"),
    (StudioView::Docked, "Docked measurements"),
    (StudioView::Floating, "Floating measurements"),
    (StudioView::Only, "Measurements only"),
    (StudioView::Offline, "Offline waveform analysis"),
    (StudioView::Sim, "SIM"),
];

/// Rows of the View menu after the arrangements: the badges switch and Reset layout.
pub(super) const VIEW_BADGES: usize = STUDIO_VIEWS.len();
pub(super) const VIEW_RESET: usize = STUDIO_VIEWS.len() + 1;
pub(super) const VIEW_COMPACT: usize = STUDIO_VIEWS.len() + 2;
/// The row that opens the add-panel page.
pub(super) const VIEW_ADD: usize = STUDIO_VIEWS.len() + 3;

/// A small square wave, the lanes in the pictures of the View menu.
fn icon_wave(cv: &mut Canvas, r: Rect, c: Rgb) {
    let (lo, hi) = (r.bottom() - 4, r.y + 4);
    let mut x = r.x + 2;
    let mut y = lo;
    for step in [5, 5, 5, 5, 5, 5] {
        cv.hline(x, x + step, y, c);
        x += step;
        let next = if y == lo { hi } else { lo };
        cv.vline(x, y.min(next), y.max(next), c);
        y = next;
        if x >= r.right() - 6 { break; }
    }
}

fn icon_view_mode(cv: &mut Canvas, r: Rect, view: StudioView, c: Rgb) {
    cv.round_outline(r, 3, c);
    match view {
        StudioView::Waveform => icon_wave(cv, r.inset(2), c),
        StudioView::Docked => {
            icon_wave(cv, Rect::new(r.x + 2, r.y + 2, r.w - 14, r.h - 4), c);
            cv.vline(r.right() - 11, r.y, r.bottom() - 1, c);
            for k in 0..3 { cv.hline(r.right() - 9, r.right() - 4, r.y + 5 + k * 5, c); }
        }
        StudioView::Floating => {
            icon_wave(cv, r.inset(2), c);
            cv.round_outline(Rect::new(r.right() - 16, r.bottom() - 12, 13, 9), 1, c);
        }
        StudioView::Only => {
            for k in 0..3 { cv.hline(r.x + 4, r.right() - 5, r.y + 5 + k * 6, c); }
        }
        StudioView::Offline => {
            // the navigator over the record with its window, the lanes under it
            cv.hline(r.x + 3, r.right() - 4, r.y + 8, c);
            cv.round_outline(Rect::new(r.x + 14, r.y + 3, 14, 10), 1, c);
            icon_wave(cv, Rect::new(r.x + 2, r.y + 12, r.w - 4, r.h - 14), c);
        }
        StudioView::Sim => {
            icon_wave(cv, Rect::new(r.x + 2, r.y + 2, r.w - 14, r.h - 4), c);
            cv.vline(r.right() - 11, r.y, r.bottom() - 1, c);
            cv.triangle([(r.right() - 9, r.y + r.h / 2 - 5), (r.right() - 9, r.y + r.h / 2 + 5), (r.right() - 3, r.y + r.h / 2)], c);
        }
    }
}

impl Ui {
    /// The View menu, under the View chip.
    pub(super) fn studio_view_rect(&self) -> Rect {
        let anchor = self.studio_top().view;
        let h = 40 + 10 * VIEW_ROW + 48;
        Rect::new((anchor.right() - VIEW_W).max(4), self.frame().top.bottom() + 4, VIEW_W, h)
    }

    /// Row `i` of the View menu: 0 to 3 the measurement arrangements, 4 and 5 the record views (a line above them),
    /// `VIEW_BADGES` the badges switch, `VIEW_RESET` Reset layout.
    pub(super) fn studio_view_row(&self, i: usize) -> Rect {
        let p = self.studio_view_rect();
        let y = match i {
            0..=3 => p.y + 40 + i as i32 * VIEW_ROW,
            4..=5 => p.y + 40 + i as i32 * VIEW_ROW + 8,
            VIEW_BADGES => p.y + 40 + 6 * VIEW_ROW + 16,
            VIEW_COMPACT => p.y + 40 + 7 * VIEW_ROW + 24,
            VIEW_RESET => p.y + 40 + 8 * VIEW_ROW + 32,
            _ => p.y + 40 + 9 * VIEW_ROW + 40,
        };
        Rect::new(p.x + 6, y, p.w - 12, VIEW_ROW)
    }

    pub(super) fn studio_view_close(&self) -> Rect {
        let p = self.studio_view_rect();
        Rect::new(p.right() - 44, p.y + 4, 38, 32)
    }

    /// Compact view on or off: smaller lane gaps, badge row and measurement panel.
    pub(super) fn studio_set_compact(&mut self, on: bool) {
        self.studio.compact = on;
        self.studio.panel.first = 0;
        self.relayout();
    }

    /// Back to the arrangement the layout starts with.
    pub(super) fn studio_reset_layout(&mut self) {
        self.studio_set_view(StudioView::Waveform);
        self.dock_reset();
        self.studio.hide_badges = false;
        self.studio.pinned = None;
        self.studio.popup = None;
        self.studio.panel = Default::default();
        self.studio.compact = false;
        self.set_zoom(false);
        self.relayout();
    }

    pub(super) fn draw_studio_view_menu(&self, cv: &mut Canvas) {
        if !self.studio.view_menu { return; }
        let f = self.fonts();
        let p = self.studio_view_rect();
        cv.round(p, 8, BTN_BORDER, 255);
        cv.round(p.inset(1), 7, PANEL_BG, 255);
        let top = self.studio_top().view;
        let x = top.x + top.w / 2;
        cv.triangle([(x - 7, p.y + 1), (x + 7, p.y + 1), (x, p.y - 6)], BTN_BORDER);
        cv.triangle([(x - 6, p.y + 2), (x + 6, p.y + 2), (x, p.y - 4)], PANEL_BG);
        cv.text(&f.lb, p.x + 16, p.y + 28, "View", TEXT);
        let c = self.studio_view_close();
        let (cx, cy) = (c.x + c.w / 2, c.y + c.h / 2);
        cv.line(cx - 6, cy - 6, cx + 6, cy + 6, TEXT);
        cv.line(cx - 6, cy + 6, cx + 6, cy - 6, TEXT);
        cv.hline(p.x + 8, p.right() - 9, p.y + 38, LINE);
        for (i, (view, label)) in STUDIO_VIEWS.into_iter().enumerate() {
            let r = self.studio_view_row(i);
            let on = self.studio.view == view;
            if on {
                cv.round(r, 6, ACCENT, 255);
                cv.round(r.inset(1), 5, mix(PANEL_BG, ACCENT, 28), 255);
            }
            icon_view_mode(cv, Rect::new(r.x + 8, r.y + 8, 42, 24), view, TEXT);
            cv.text_in(&f.m, Rect::new(r.x + 62, r.y, r.w - 100, r.h), 0, label, TEXT);
            if on {
                let (kx, ky) = (r.right() - 24, r.y + r.h / 2);
                cv.line(kx, ky, kx + 4, ky + 4, ACCENT);
                cv.line(kx + 4, ky + 4, kx + 11, ky - 5, ACCENT);
                cv.line(kx, ky + 1, kx + 4, ky + 5, ACCENT);
                cv.line(kx + 4, ky + 5, kx + 11, ky - 4, ACCENT);
            }
        }
        cv.hline(p.x + 8, p.right() - 9, self.studio_view_row(4).y - 4, LINE);
        cv.hline(p.x + 8, p.right() - 9, self.studio_view_row(VIEW_BADGES).y - 4, LINE);
        let r = self.studio_view_row(VIEW_BADGES);
        cv.text_in(&f.m, Rect::new(r.x + 8, r.y, r.w - 70, r.h), 0, "Show pinned badges", TEXT);
        let on = !self.studio.hide_badges;
        let sw = Rect::new(r.right() - 52, r.y + 9, 44, 22);
        cv.round(sw, 11, if on { ACCENT } else { BTN_BORDER }, 255);
        cv.circle(if on { sw.right() - 11 } else { sw.x + 11 }, sw.y + 11, 8, if on { DARK_TEXT } else { DIM });
        let r = self.studio_view_row(VIEW_COMPACT);
        cv.text_in(&f.m, Rect::new(r.x + 8, r.y, r.w - 70, r.h), 0, "Compact view", TEXT);
        let sw = Rect::new(r.right() - 52, r.y + 9, 44, 22);
        cv.round(sw, 11, if self.studio.compact { ACCENT } else { BTN_BORDER }, 255);
        cv.circle(if self.studio.compact { sw.right() - 11 } else { sw.x + 11 }, sw.y + 11, 8, if self.studio.compact { DARK_TEXT } else { DIM });
        cv.hline(p.x + 8, p.right() - 9, self.studio_view_row(VIEW_RESET).y - 4, LINE);
        let r = self.studio_view_row(VIEW_RESET);
        let (ix, iy) = (r.x + 24, r.y + r.h / 2);
        cv.circle(ix, iy, 8, TEXT);
        cv.circle(ix, iy, 6, PANEL_BG);
        cv.fill(Rect::new(ix - 2, iy - 10, 6, 4), PANEL_BG);
        cv.triangle([(ix - 1, iy - 12), (ix - 1, iy - 5), (ix + 5, iy - 9)], TEXT);
        cv.text_in(&f.m, Rect::new(r.x + 48, r.y, r.w - 50, r.h), 0, "Reset layout", TEXT);
        cv.hline(p.x + 8, p.right() - 9, self.studio_view_row(VIEW_ADD).y - 4, LINE);
        let r = self.studio_view_row(VIEW_ADD);
        cv.hline(r.x + 18, r.x + 30, r.y + r.h / 2, TEXT);
        cv.vline(r.x + 24, r.y + r.h / 2 - 6, r.y + r.h / 2 + 6, TEXT);
        cv.text_in(&f.m, Rect::new(r.x + 48, r.y, r.w - 50, r.h), 0, "Add panel", TEXT);
    }

    /// A tap while the View menu is open: a row does its action, a tap outside closes it. Always taken.
    pub(super) fn studio_view_tap(&mut self, x: i32, y: i32) -> bool {
        if !self.studio.view_menu { return false; }
        let p = self.studio_view_rect();
        let chip = self.top_hit(self.studio_top().view);
        if chip.contains(x, y) {
            // the chip that opened it closes it again
            self.studio.view_menu = false;
            return true;
        }
        if !p.contains(x, y) {
            self.studio.view_menu = false;
            return true;
        }
        if self.studio_view_close().contains(x, y) {
            self.studio.view_menu = false;
            return true;
        }
        for (i, (view, _)) in STUDIO_VIEWS.into_iter().enumerate() {
            if self.studio_view_row(i).contains(x, y) {
                self.studio_set_view(view);
                self.studio.view_menu = false;
                return true;
            }
        }
        if self.studio_view_row(VIEW_BADGES).contains(x, y) {
            self.studio.hide_badges = !self.studio.hide_badges;
            self.relayout();
        } else if self.studio_view_row(VIEW_COMPACT).contains(x, y) {
            self.studio_set_compact(!self.studio.compact);
        } else if self.studio_view_row(VIEW_RESET).contains(x, y) {
            self.studio_reset_layout();
            self.studio.view_menu = false;
        } else if self.studio_view_row(VIEW_ADD).contains(x, y) {
            self.studio.view_menu = false;
            self.open_menu(MenuId::AddPanel);
        }
        true
    }

    /// Chooses how the measurements share the screen.
    pub(super) fn studio_set_view(&mut self, view: StudioView) {
        let was = self.studio.view;
        // a record view runs on a session of its own: leaving it returns to the live acquisition
        if was.is_record() && was != view && self.record_mode() { self.return_to_live(); }
        self.studio.view = view;
        if view != StudioView::Offline { self.studio.dock.float_on[super::dock::VKind::Measure.idx()] = false; }
        self.dock_view_changed();
        self.studio.panel.first = 0;
        self.studio.filter = None;
        self.studio.popup = None;
        if view != was {
            match view {
                StudioView::Offline => self.studio_enter_offline(),
                StudioView::Sim => self.studio_enter_sim(),
                _ => {}
            }
        }
        self.relayout();
    }
}

/// Below this width the footer of the panel stacks its two buttons narrower, with shorter labels.
pub(super) const NARROW_PANEL_W: i32 = 250;
/// Width of the measurement panel, docked or floating, and the height of a row in it.
const MEAS_PANEL_W_FULL: i32 = 270;
const PANEL_ROW_FULL: i32 = 46;
/// Rows of the table that has the whole body.
const TABLE_ROW: i32 = 44;

/// A touch that the panel or the table owns, from its first point to its last.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum StudioDrag {
    /// the list moves with the finger
    Scroll { y0: i32, first0: usize, moved: bool },
    /// a touch on a panel or the table that scrolls nothing: it ends in a tap
    Eat { x0: i32, y0: i32 },
    /// the floating panel moves with the finger
    Move { x0: i32, y0: i32, at0: (i32, i32), moved: bool },
    /// a row swiped sideways
    Swipe { key: MeasKey, x0: i32, y0: i32 },
    /// a row held and dragged to reorder or pin
    Reorder { key: MeasKey, x: i32, y: i32 },
    /// a badge held and dragged
    BadgeDrag { key: MeasKey, x: i32, y: i32 },
    /// a tile of the Add measurement panel dragged to a channel
    Tile { k: usize, x0: i32, y0: i32, x: i32, y: i32, moved: bool },
}

/// The parts of the docked panel.
pub(super) struct PanelGeo {
    pub p: Rect,
    pub maximize: Rect,
    pub collapse: Rect,
    pub list: Rect,
    pub page: usize,
    pub up: Rect,
    pub down: Rect,
    pub add: Rect,
    /// the + tile: opens the add-panel page
    pub plus: Rect,
}

pub(super) fn icon_expand(cv: &mut Canvas, cx: i32, cy: i32, c: Rgb) {
    cv.line(cx - 6, cy + 6, cx + 6, cy - 6, c);
    cv.hline(cx + 1, cx + 6, cy - 6, c);
    cv.vline(cx + 6, cy - 6, cy - 1, c);
    cv.hline(cx - 6, cx - 1, cy + 6, c);
    cv.vline(cx - 6, cy + 1, cy + 6, c);
}

pub(super) fn icon_arrow(cv: &mut Canvas, cx: i32, cy: i32, up: bool, c: Rgb) {
    let d = if up { -1 } else { 1 };
    cv.triangle([(cx - 6, cy - 3 * d), (cx + 6, cy - 3 * d), (cx, cy + 3 * d)], c);
}

impl Ui {
    /// Index of the first row of the list that is shown, kept in range.
    pub(super) fn studio_first(&self, total: usize, page: usize) -> usize {
        self.studio.panel.first.min(total.saturating_sub(page))
    }

    /// The measurements the panel lists: all, or those of the chosen source.
    pub(super) fn studio_listed(&self, rows: &[tools::MeasRow]) -> Vec<usize> {
        (0..rows.len()).filter(|&i| !self.meas_row_hidden(&rows[i]) && self.studio.filter.is_none_or(|id| rows[i].src == id)).collect()
    }

    /// The measurement panel (docked or floating) and the SIM panel: the room under their title strip, in the dock.
    pub(super) fn studio_panel_rect(&self) -> Option<Rect> {
        match self.studio.view {
            StudioView::Docked | StudioView::Offline | StudioView::Floating => self.dock_content(super::dock::VKind::Measure),
            StudioView::Sim => self.dock_content(super::dock::VKind::Sim),
            _ => None,
        }
    }

    /// The table of all measurements: the whole body.
    pub(super) fn studio_table_rect(&self) -> Option<Rect> {
        if self.studio.view != StudioView::Only { return None; }
        self.dock_content(super::dock::VKind::Measure)
    }

    /// Plot width less the docked panel: `plot_span` calls this.
    pub(super) fn studio_dock_w(&self) -> i32 {
        if self.dock_on() { return 0; }
        if self.m.layout == Layout::Studio && self.sheet_active() { return self.sheet_dock_w(); }
        if self.m.layout == Layout::Studio && self.studio.view.is_docked() { self.studio_panel_w() + 4 } else { 0 }
    }

    pub(super) fn studio_panel_geo(&self) -> Option<PanelGeo> {
        let p = self.studio_panel_rect().filter(|_| matches!(self.studio.view, StudioView::Docked | StudioView::Offline | StudioView::Floating))?;
        let list_y = p.y + 28;
        let rows_h = (p.bottom() - 48 - 6 - 34 - 4 - list_y).max(self.studio_panel_row());
        let page = (rows_h / self.studio_panel_row()).max(1) as usize;
        let list = Rect::new(p.x + 4, list_y, p.w - 8, page as i32 * self.studio_panel_row());
        let nav_y = list.bottom() + 4;
        Some(PanelGeo {
            p,
            maximize: Rect::new(0, 0, 0, 0),
            collapse: Rect::new(0, 0, 0, 0),
            list, page,
            up: Rect::new(p.right() - 82, nav_y, 36, 30),
            down: Rect::new(p.right() - 42, nav_y, 36, 30),
            // a narrow panel: the + Panel button shrinks to its plus and the other gets a short label (see the draw)
            add: Rect::new(p.x + 6, p.bottom() - 46, p.w - 12 - if p.w < NARROW_PANEL_W { 52 } else { 92 }, 40),
            plus: if p.w < NARROW_PANEL_W { Rect::new(p.right() - 50, p.bottom() - 46, 44, 40) } else { Rect::new(p.right() - 90, p.bottom() - 46, 84, 40) },
        })
    }

    /// Floating panel: its title bar, close, the Dock and View all buttons.
    pub(super) fn studio_float_parts(&self) -> Option<(Rect, Rect, Rect, Rect)> {
        let p = self.studio_panel_rect().filter(|_| false)?;
        Some((Rect::new(p.x, p.y, p.w - 44, 40), Rect::new(p.right() - 42, p.y + 5, 36, 30),
            Rect::new(p.x + 8, p.bottom() - 46, p.w / 2 - 12, 38), Rect::new(p.x + p.w / 2 + 4, p.bottom() - 46, p.w / 2 - 12, 38)))
    }

    /// One row of the docked and floating panel: CH badge, the name over the value, the pin.
    pub(super) fn draw_panel_row(&self, cv: &mut Canvas, r: Rect, row: &tools::MeasRow, pinned: bool) {
        let f = self.fonts();
        let tag = Rect::new(r.x + 8, r.y + 12, f.sb.width("CH4") + 12, 22);
        cv.round(tag, 5, self.wave_color(row.src), 255);
        cv.text_in(&f.sb, tag, 1, &self.wave_name(row.src), DARK_TEXT);
        let x = tag.right() + 12;
        cv.set_clip(Rect::new(x, r.y, (r.right() - 88 - x).max(1), r.h));
        cv.text(&f.s, x, r.y + 18, &row.name, LABEL);
        cv.text(&f.mb, x, r.y + 38, &row.text(), TEXT);
        cv.no_clip();
        let (pin, menu) = Self::md_row_buttons(r);
        icon_pin(cv, pin.x + pin.w / 2 - 6, r.y + r.h / 2 + 1, pinned, if pinned { AMBER } else { DIM });
        // the row's menu: three dots
        for k in -1..=1 { cv.circle(menu.x + menu.w / 2, r.y + r.h / 2 + 7 * k, 2, DIM); }
        cv.hline(r.x + 6, r.right() - 7, r.bottom() - 1, LINE);
    }

    pub(super) fn draw_scrollbar(cv: &mut Canvas, list: Rect, first: usize, page: usize, total: usize) {
        if total <= page { return; }
        let x = list.right() - 4;
        cv.vline(x, list.y, list.bottom() - 1, mix(BG, LINE, 120));
        let h = (list.h as i64 * page as i64 / total as i64).max(16) as i32;
        let y = list.y + ((list.h - h) as i64 * first as i64 / (total - page).max(1) as i64) as i32;
        cv.fill(Rect::new(x - 1, y, 3, h), DIM);
    }

    pub(super) fn draw_studio_panel(&self, cv: &mut Canvas) {
        if self.m.layout != Layout::Studio { return; }
        // an analysis view fills the plot area: the docked panel stays beside it, a floating panel and the table do not
        if self.view_on() && !self.studio.view.is_docked() { return; }
        self.draw_studio_offline(cv);
        if self.studio_table_rect().is_some() { return self.draw_studio_table(cv); }
        if self.studio.view == StudioView::Sim { return self.draw_studio_sim(cv); }
        let Some(p) = self.studio_panel_rect() else { return };
        let f = self.fonts();
        let rows = self.meas_rows();
        let keys = self.studio_keys();
        let listed = self.studio_listed(&rows);
        let pinned = self.studio_pinned();
        cv.round(p, 8, if self.studio.view == StudioView::Floating { BTN_BORDER } else { CARD_BORDER }, 255);
        cv.round(p.inset(1), 7, PANEL_BG, 255);
        if let Some(g) = self.studio_panel_geo() {
            cv.text(&f.s, p.x + 14, p.y + 19, &format!("{} total \u{b7} {} pinned", self.meas_shown_count(), pinned.len()), DIM);
            let first = self.studio_first(listed.len(), g.page);
            cv.set_clip(g.list);
            for (k, &i) in listed.iter().skip(first).take(g.page).enumerate() {
                let r = Rect::new(g.list.x, g.list.y + k as i32 * self.studio_panel_row(), g.list.w, self.studio_panel_row());
                // a row that is swiped or springs back slides left over a red Remove
                let md = &self.studio.md;
                let off = match (keys.get(i), md.swipe, md.spring) {
                    (Some(a), Some((b, dx)), _) if *a == b => dx,
                    (Some(a), _, Some((b, dx))) if *a == b => dx as i32,
                    _ => 0,
                };
                if off < 0 {
                    let rev = Rect::new((r.right() + off).max(r.x), r.y, (-off).min(r.w), r.h);
                    cv.fill(rev, mix(PANEL_BG, RED, 150));
                    cv.text_in(&f.mb, rev, 1, "Remove", TEXT);
                }
                self.draw_panel_row(cv, Rect::new(r.x + off, r.y, r.w, r.h), &rows[i], keys.get(i).is_some_and(|k| pinned.contains(k)));
            }
            cv.no_clip();
            Self::draw_scrollbar(cv, g.list, first, g.page, listed.len());
            let end = (first + g.page).min(listed.len());
            cv.text(&f.s, p.x + 14, g.up.y + 20, &if listed.is_empty() { "0 of 0".to_string() } else { format!("{}-{} of {}", first + 1, end, listed.len()) }, DIM);
            for (r, up) in [(g.up, true), (g.down, false)] {
                chip(cv, r, BTN, BTN_BORDER);
                icon_arrow(cv, r.x + r.w / 2, r.y + r.h / 2, up, TEXT);
            }
            chip(cv, g.add, BTN, BTN_BORDER);
            cv.hline(g.add.x + 10, g.add.x + 22, g.add.y + g.add.h / 2, TEXT);
            cv.vline(g.add.x + 16, g.add.y + g.add.h / 2 - 6, g.add.y + g.add.h / 2 + 6, TEXT);
            let add_label = if g.add.w < 168 { "Add" } else { "Add measurement" };
            cv.text_in(&f.m, Rect::new(g.add.x + 28, g.add.y, g.add.w - 30, g.add.h), 1, add_label, TEXT);
            chip(cv, g.plus, BTN, BTN_BORDER);
            cv.text_in(&f.m, g.plus, 1, if g.plus.w < 60 { "+" } else { "+ Panel" }, TEXT);
        } else if let Some((title, close, dock, all)) = self.studio_float_parts() {
            // the dotted handle shows that the panel can be moved
            for (dx, dy) in [(0, 0), (6, 0), (12, 0), (0, 6), (6, 6), (12, 6), (0, 12), (6, 12), (12, 12)] {
                cv.fill(Rect::new(p.x + 14 + dx, p.y + 13 + dy, 2, 2), DIM);
            }
            cv.text(&f.lb, p.x + 44, title.y + 27, "Measurements", TEXT);
            chip(cv, close, BTN, BTN_BORDER);
            let (cx, cy) = (close.x + close.w / 2, close.y + close.h / 2);
            cv.line(cx - 5, cy - 5, cx + 5, cy + 5, TEXT);
            cv.line(cx - 5, cy + 5, cx + 5, cy - 5, TEXT);
            let shown = self.studio_float_keys();
            cv.text(&f.s, p.x + 14, p.y + 56, &format!("{} shown \u{b7} {} total", shown.len(), rows.len()), DIM);
            let mut k = 0;
            for key in &shown {
                let Some(i) = self.studio_index(*key).filter(|&i| i < rows.len()) else { continue };
                if k >= 5 { break; }
                self.draw_panel_row(cv, Rect::new(p.x + 4, p.y + 64 + k * self.studio_panel_row(), p.w - 8, self.studio_panel_row()), &rows[i], true);
                k += 1;
            }
            if k == 0 { cv.text(&f.s, p.x + 14, p.y + 64 + 28, "Nothing shown yet", DIM); }
            chip(cv, dock, BTN, BTN_BORDER);
            cv.round_outline(Rect::new(dock.x + 12, dock.y + 12, 16, 12), 2, TEXT);
            cv.vline(dock.x + 23, dock.y + 12, dock.y + 23, TEXT);
            cv.text_in(&f.m, Rect::new(dock.x + 34, dock.y, dock.w - 34, dock.h), 0, "Dock", TEXT);
            chip(cv, all, BTN, BTN_BORDER);
            cv.text_in(&f.m, Rect::new(all.x + 8, all.y, all.w - 22, all.h), 1, &format!("View all {}", rows.len()), TEXT);
            chevron(cv, all.right() - 14, all.y + all.h / 2, 1, TEXT);
        }
    }

    /// The tab that brings the panel back while only the lanes are shown.
    pub(super) fn studio_tab(&self) -> Option<Rect> {
        if self.m.layout != Layout::Studio || self.studio.view != StudioView::Waveform || !self.studio_badges_visible()
            || self.measurement_count() == 0 { return None; }
        // in the badge row, left of the eye: nothing hangs under the top bar
        let eye = self.studio_eye();
        Some(Rect::new(eye.x - 6 - 102, eye.y, 102, eye.h))
    }

    fn draw_studio_tab(&self, cv: &mut Canvas) {
        let Some(r) = self.studio_tab() else { return };
        let f = self.fonts();
        chip(cv, r, BTN, BTN_BORDER);
        cv.text_in(&f.sb, Rect::new(r.x + 8, r.y, r.w - 22, r.h), 0, &format!("Meas \u{b7} {}", self.measurement_count()), TEXT);
        chevron(cv, r.right() - 12, r.y + r.h / 2, 1, TEXT);
    }

    // ------------------------------------------------------------------------------------------------ the table

    /// The controls of the table view: Waveform, the source filter, Statistics and Add.
    pub(super) fn studio_table_parts(&self) -> Option<[Rect; 4]> {
        let t = self.studio_table_rect()?;
        let y = t.y + 8;
        let add = Rect::new(t.right() - 100, y, 92, 32);
        let stats = Rect::new(add.x - 8 - 126, y, 126, 32);
        let filter = Rect::new(stats.x - 8 - 150, y, 150, 32);
        let back = Rect::new(filter.x - 8 - 120, y, 120, 32);
        Some([back, filter, stats, add])
    }

    /// Rows of the table, the page, and the footer buttons (Reset statistics, up, down).
    pub(super) fn studio_table_geo(&self) -> Option<(Rect, usize, [Rect; 3])> {
        let t = self.studio_table_rect()?;
        let list = Rect::new(t.x + 6, t.y + 48 + 28, t.w - 12, t.h - 48 - 28 - 44);
        let page = (list.h / TABLE_ROW).max(1) as usize;
        let y = t.bottom() - 38;
        let reset = Rect::new(t.right() - 168, y, 160, 32);
        Some((list, page, [reset, Rect::new(reset.x - 44 - 44, y, 38, 32), Rect::new(reset.x - 44, y, 38, 32)]))
    }

    /// The source filter in the order the sources first measure something.
    fn studio_filter_next(&self) -> Option<WaveId> {
        let rows = self.meas_rows();
        let mut sources: Vec<WaveId> = Vec::new();
        for r in rows.iter().filter(|r| !self.meas_row_hidden(r)) { if !sources.contains(&r.src) { sources.push(r.src); } }
        match self.studio.filter {
            None => sources.first().copied(),
            Some(id) => sources.iter().position(|&s| s == id).and_then(|i| sources.get(i + 1)).copied(),
        }
    }

    fn draw_studio_table(&self, cv: &mut Canvas) {
        let (Some(t), Some([back, filter, stats, add]), Some((list, page, [reset, up, down]))) =
            (self.studio_table_rect(), self.studio_table_parts(), self.studio_table_geo()) else { return };
        let f = self.fonts();
        let rows = self.meas_rows();
        let keys = self.studio_keys();
        let listed = self.studio_listed(&rows);
        cv.fill(t, BG);
        cv.text(&f.s, t.x + 12, t.y + 30, &format!("{} total \u{b7} {} pinned", self.meas_shown_count(), self.studio_pinned().len()), DIM);
        chip(cv, back, BTN, BTN_BORDER);
        icon_wave(cv, Rect::new(back.x + 6, back.y + 8, 26, 16), TEXT);
        cv.text_in(&f.m, Rect::new(back.x + 36, back.y, back.w - 38, back.h), 0, "Waveform", TEXT);
        chip(cv, filter, BTN, BTN_BORDER);
        let label = self.studio.filter.map_or("All channels".to_string(), |id| self.wave_name(id));
        cv.text_in(&f.m, Rect::new(filter.x + 10, filter.y, filter.w - 30, filter.h), 0, &label, TEXT);
        chevron_down(cv, filter.right() - 20, filter.y + 14, DIM);
        let on = self.studio.panel.stats;
        chip(cv, stats, if on { mix(BTN, ACCENT, 30) } else { BTN }, if on { ACCENT } else { BTN_BORDER });
        icon_bars(cv, stats.x + 10, stats.y + 16, TEXT);
        cv.text_in(&f.m, Rect::new(stats.x + 30, stats.y, stats.w - 32, stats.h), 0, "Statistics", TEXT);
        chip(cv, add, BTN, BTN_BORDER);
        cv.text_in(&f.m, add, 1, "+ Add", TEXT);
        // columns: pin, source, measurement, current, then mean and the minimum, maximum and deviation the settings show
        let show = self.m.meas_show;
        let cols: Vec<(i32, &str)> = if on {
            let mut v = vec![(52, "Source"), (136, "Measurement"), (380, "Current"), (540, "Mean")];
            let mut x = 540;
            for (name, i) in [("Min", 1usize), ("Max", 2), ("\u{3c3}", 0)] {
                if show[i] { x += 120; v.push((x, name)); }
            }
            v
        } else { vec![(52, "Source"), (136, "Measurement"), (420, "Current")] };
        let head = Rect::new(t.x + 6, t.y + 48, t.w - 12, 28);
        cv.fill(head, BAR);
        for (x, name) in &cols { cv.text_in(&f.mb, Rect::new(head.x + x, head.y, 160, head.h), 0, name, LABEL); }
        let first = self.studio_first(listed.len(), page);
        cv.set_clip(list);
        for (k, &i) in listed.iter().skip(first).take(page).enumerate() {
            let r = Rect::new(list.x, list.y + k as i32 * TABLE_ROW, list.w, TABLE_ROW);
            let row = &rows[i];
            let cy = r.y + r.h / 2;
            let pinned = keys.get(i).is_some_and(|key| self.studio_is_pinned(*key));
            icon_pin(cv, r.x + 14, cy + 1, pinned, if pinned { AMBER } else { DIM });
            let tag = Rect::new(r.x + cols[0].0, r.y + 10, f.sb.width("CH4") + 12, 24);
            cv.round(tag, 5, self.wave_color(row.src), 255);
            cv.text_in(&f.sb, tag, 1, &self.wave_name(row.src), DARK_TEXT);
            cv.text_in(&f.m, Rect::new(r.x + cols[1].0, r.y, 240, r.h), 0, &row.name, TEXT);
            cv.text_in(&f.mb, Rect::new(r.x + cols[2].0, r.y, 170, r.h), 0, &row.text(), TEXT);
            if on {
                let st = self.mstat.get(i).copied().unwrap_or_default();
                let cell = |v: f64| if st.n == 0 { "----".to_string() } else { row.fmt4(v) };
                let mean = st.sum / st.n.max(1) as f64;
                let sd = (st.sumsq / st.n.max(1) as f64 - mean * mean).max(0.0).sqrt();
                let mut values = vec![mean];
                for (v, i) in [(st.min, 1usize), (st.max, 2), (sd, 0)] {
                    if show[i] { values.push(v); }
                }
                for (k, v) in values.into_iter().enumerate() {
                    let Some(col) = cols.get(3 + k) else { break };
                    cv.text_in(&f.m, Rect::new(r.x + col.0, r.y, 118, r.h), 0, &cell(v), TEXT);
                }
            }
            cv.hline(r.x, r.right() - 1, r.bottom() - 1, LINE);
        }
        cv.no_clip();
        Self::draw_scrollbar(cv, list, first, page, listed.len());
        let end = (first + page).min(listed.len());
        cv.text(&f.m, t.x + 14, t.bottom() - 16, &if listed.is_empty() { "0 of 0".to_string() } else { format!("{}-{} of {}", first + 1, end, listed.len()) }, DIM);
        for (r, is_up) in [(up, true), (down, false)] {
            chip(cv, r, BTN, BTN_BORDER);
            icon_arrow(cv, r.x + r.w / 2, r.y + r.h / 2, is_up, TEXT);
        }
        chip(cv, reset, BTN, BTN_BORDER);
        cv.text_in(&f.m, reset, 1, "Reset statistics", TEXT);
    }

    // ------------------------------------------------------------------------------------------------ taps

    /// Opens the settings of measurement `index`: its own page, or the measure menu for the kinds without one.
    pub(super) fn studio_configure(&mut self, index: usize) {
        self.open_meas_cfg(index)
    }

    /// Pages the list by one page.
    pub(super) fn studio_page(&mut self, total: usize, page: usize, down: bool) {
        let first = self.studio_first(total, page);
        self.studio.panel.first = if down { (first + page).min(total.saturating_sub(page)) } else { first.saturating_sub(page) };
    }

    /// A tap on the panel or the table. False: it is not on them.
    pub(super) fn studio_panel_tap(&mut self, x: i32, y: i32) -> bool {
        if self.studio_offline_tap(x, y) { return true; }
        if self.studio.view == StudioView::Sim { return self.studio_sim_tap(x, y); }
        let rows = self.meas_rows();
        let keys = self.studio_keys();
        let listed = self.studio_listed(&rows);
        if let Some(g) = self.studio_panel_geo() {
            if !g.p.contains(x, y) { return false; }
            if self.md_undo_tap(x, y) { return true; }
            if g.maximize.contains(x, y) { self.studio_set_view(StudioView::Only); }
            else if g.collapse.contains(x, y) { self.studio_set_view(StudioView::Waveform); }
            else if g.up.contains(x, y) || g.down.contains(x, y) { self.studio_page(listed.len(), g.page, g.down.contains(x, y)); }
            else if g.add.contains(x, y) { self.open_meas_list(); }
            else if g.plus.contains(x, y) { self.open_menu(MenuId::AddPanel); }
            else if g.list.contains(x, y) {
                let first = self.studio_first(listed.len(), g.page);
                let k = ((y - g.list.y) / self.studio_panel_row()) as usize;
                if let Some(&i) = listed.get(first + k) {
                    let row = Rect::new(g.list.x, g.list.y + k as i32 * self.studio_panel_row(), g.list.w, self.studio_panel_row());
                    let (pin, menu) = Self::md_row_buttons(row);
                    if menu.contains(x, y) {
                        if let Some(&key) = keys.get(i) { self.studio.md.menu = Some(key); }
                    } else if pin.contains(x, y) {
                        if let Some(&key) = keys.get(i) { let on = self.studio_is_pinned(key); self.studio_set_pin(key, !on); }
                    } else { self.studio_configure(i); }
                }
            }
            return true;
        }
        if let Some((title, close, dock, all)) = self.studio_float_parts() {
            let p = self.studio_panel_rect().unwrap();
            if !p.contains(x, y) { return false; }
            if close.contains(x, y) { self.studio_set_view(StudioView::Waveform); }
            else if dock.contains(x, y) { self.studio_set_view(StudioView::Docked); }
            else if all.contains(x, y) { self.studio_set_view(StudioView::Only); }
            else if !title.contains(x, y) && y >= p.y + 64 {
                let shown = self.studio_float_keys();
                let k = ((y - p.y - 64) / self.studio_panel_row()) as usize;
                if let Some(index) = shown.get(k).and_then(|&key| self.studio_index(key)) {
                    if x >= p.right() - 52 {
                        if let Some(&key) = keys.get(index) { self.studio.floated.retain(|f| *f != key); self.studio_set_pin(key, false); }
                    }
                    else { self.studio_configure(index); }
                }
            }
            return true;
        }
        if let (Some(t), Some([back, filter, stats, add]), Some((list, page, [reset, up, down]))) =
            (self.studio_table_rect(), self.studio_table_parts(), self.studio_table_geo()) {
            if !t.contains(x, y) { return false; }
            if back.contains(x, y) { self.studio_set_view(StudioView::Waveform); }
            else if filter.contains(x, y) { self.studio.filter = self.studio_filter_next(); self.studio.panel.first = 0; }
            else if stats.contains(x, y) { self.studio.panel.stats = !self.studio.panel.stats; }
            else if add.contains(x, y) { self.open_meas_list(); }
            else if reset.contains(x, y) { self.clear_measurement_statistics(); }
            else if up.contains(x, y) || down.contains(x, y) { self.studio_page(listed.len(), page, down.contains(x, y)); }
            else if list.contains(x, y) {
                let first = self.studio_first(listed.len(), page);
                let k = ((y - list.y) / TABLE_ROW) as usize;
                if let Some(&i) = listed.get(first + k) {
                    if x < list.x + 52 {
                        if let Some(&key) = keys.get(i) { let on = self.studio_is_pinned(key); self.studio_set_pin(key, !on); }
                    } else { self.studio_configure(i); }
                }
            }
            return true;
        }
        if let Some(tab) = self.studio_tab() {
            if Rect::new(tab.x - 4, tab.y - 2, tab.w + 8, tab.h + 6).contains(x, y) {
                self.studio_set_view(StudioView::Docked);
                return true;
            }
        }
        false
    }

    /// Touches that move the list or the floating panel. Everything else belongs to the plot (`touch_ev`).
    pub(super) fn studio_touch(&mut self, ph: Phase, x: i32, y: i32) -> bool {
        if self.m.layout != Layout::Studio { return false; }
        match ph {
            Phase::Down => {
                self.studio.drag = None;
                if self.dialog.is_some() || self.power_dlg || self.wlan.osk.is_some() || self.m.touch_lock
                    || self.studio.view_menu || self.studio.popup.is_some() || self.studio.md.menu.is_some() || self.load_slot_ask { return false; }
                // the Add measurement panel is open: only its tiles can be dragged
                if self.menu_open { return self.md_down(x, y); }
                if self.ov_divider_down(x, y) { return true; }
                if self.dock_down(x, y) { return true; }
                if self.md_down(x, y) { return true; }
                let rows = self.meas_rows();
                if self.studio.view == StudioView::Sim {
                    if let Some((_, _, page, _)) = self.studio_sim_list().filter(|(list, ..)| list.contains(x, y)) {
                        self.studio.drag = Some(StudioDrag::Scroll { y0: y, first0: self.studio_first(self.studio_sim_list().map_or(0, |l| l.3), page), moved: false });
                        return true;
                    }
                    return self.studio_panel_rect().is_some_and(|p| p.contains(x, y)) && self.studio_eat(x, y);
                }
                if let Some(g) = self.studio_panel_geo() {
                    if g.list.contains(x, y) {
                        self.studio.drag = Some(StudioDrag::Scroll { y0: y, first0: self.studio_first(self.studio_listed(&rows).len(), g.page), moved: false });
                        return true;
                    }
                    return self.dock_float_at(x, y).is_some() && self.studio_eat(x, y);
                }
                if self.dock_float_at(x, y).is_some() { return self.studio_eat(x, y); }
                if let Some((title, ..)) = self.studio_float_parts() {
                    if title.contains(x, y) {
                        let p = self.studio_panel_rect().unwrap();
                        self.studio.drag = Some(StudioDrag::Move { x0: x, y0: y, at0: (p.x, p.y), moved: false });
                        return true;
                    }
                    return self.studio_panel_rect().is_some_and(|p| p.contains(x, y)) && self.studio_eat(x, y);
                }
                if let (Some(t), Some((list, page, _))) = (self.studio_table_rect(), self.studio_table_geo()) {
                    if list.contains(x, y) {
                        self.studio.drag = Some(StudioDrag::Scroll { y0: y, first0: self.studio_first(self.studio_listed(&rows).len(), page), moved: false });
                        return true;
                    }
                    return t.contains(x, y) && self.studio_eat(x, y);
                }
                false
            }
            Phase::Move => {
                if self.ov_divider_move(y) { return true; }
                if self.dock_move(x, y) { return true; }
                if self.md_move(x, y) { return true; }
                let Some(drag) = self.studio.drag else { return false };
                match drag {
                    StudioDrag::Scroll { y0, first0, moved } => {
                        let rows = self.meas_rows();
                        let sim = self.studio_sim_list().filter(|_| self.studio.view == StudioView::Sim);
                        let total = sim.map_or_else(|| self.studio_listed(&rows).len(), |l| l.3);
                        let (h, page) = if let Some((_, h, page, _)) = sim { (h, page) } else if let Some(g) = self.studio_panel_geo() { (self.studio_panel_row(), g.page) } else { (TABLE_ROW, self.studio_table_geo().map_or(1, |g| g.1)) };
                        let moved = moved || (y - y0).abs() > 8;
                        if moved {
                            let delta = (y0 - y) / h;
                            self.studio.panel.first = (first0 as i32 + delta).clamp(0, total.saturating_sub(page) as i32) as usize;
                        }
                        self.studio.drag = Some(StudioDrag::Scroll { y0, first0, moved });
                    }
                    StudioDrag::Eat { .. } => {}
                    StudioDrag::Swipe { .. } | StudioDrag::Reorder { .. } | StudioDrag::BadgeDrag { .. } | StudioDrag::Tile { .. } => {}
                    StudioDrag::Move { x0, y0, at0, moved } => {
                        let moved = moved || (x - x0).abs().max((y - y0).abs()) > 8;
                        if moved { self.studio.panel.at = Some((at0.0 + x - x0, at0.1 + y - y0)); }
                        self.studio.drag = Some(StudioDrag::Move { x0, y0, at0, moved });
                    }
                }
                true
            }
            Phase::Up => {
                if self.ov_divider_up() { return true; }
                if self.dock_up(x, y) { return true; }
                let Some(drag) = self.studio.drag.take() else { return false };
                if self.md_up(drag, x, y) { return true; }
                let moved = match drag {
                    StudioDrag::Swipe { .. } | StudioDrag::Reorder { .. } | StudioDrag::BadgeDrag { .. } => true,
                    StudioDrag::Tile { moved, .. } => moved,
                    StudioDrag::Scroll { moved, .. } | StudioDrag::Move { moved, .. } => moved,
                    StudioDrag::Eat { x0, y0 } => (x - x0).abs().max((y - y0).abs()) > 8,
                };
                if !moved { self.tap(x, y); }
                true
            }
        }
    }

    /// A touch on the table or a floating panel that does nothing by itself: the plot behind must not take it.
    /// The gesture ends in a tap (`Phase::Up` of the owner below).
    pub(super) fn studio_eat(&mut self, x: i32, y: i32) -> bool {
        self.studio.drag = Some(StudioDrag::Eat { x0: x, y0: y });
        true
    }
}

// ---------------------------------------------------------------------------------------------------- zoom

/// The rows of the Zoom & navigation menu.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum ZoomRow {
    Minus,
    Plus,
    /// vertical zoom in and out
    VPlus,
    VMinus,
    Tdiv,
    Fit,
    Center,
    ResetY,
    Close,
}

/// The chips of the zoom toolbar.
#[derive(Clone, Copy)]
pub(super) struct ZoomBar {
    /// the "Zoom" caption
    pub label: Rect,
    /// the time/div field of the zoomed view
    pub tdiv: Rect,
    /// [+] and [-]: one step of the 1-2-5 sequence in and out
    pub plus: Rect,
    pub minus: Rect,
    /// the vertical zoom: [Y+] and [Y-], one step of the display scale of the zoomed lanes (an empty rectangle when there is no room)
    pub vplus: Rect,
    pub vminus: Rect,
    /// "(20x zoom)"
    pub readout: Rect,
    /// [x]: zoom off
    pub close: Rect,
    pub center: Rect,
    pub fit: Rect,
    pub more: Rect,
}

const ZMENU_W: i32 = 300;

/// A zoom factor as the toolbar writes it: "5x", "2.5x", "12x".
fn zoom_text(n: f32) -> String {
    let n = if n.is_finite() { n.max(1.0) } else { 1.0 };
    if n >= 9.95 { format!("{:.0}\u{d7}", n) } else { format!("{:.1}\u{d7}", n).replace(".0\u{d7}", "\u{d7}") }
}

impl Ui {
    /// Studio's zoom bar sits in the strip between the overview and the zoomed lanes.
    pub(super) fn studio_zoom_active(&self) -> bool {
        self.m.layout == Layout::Studio && self.m.zoom && !self.xy_over() && !self.view_on()
    }

    /// How many times the detail view magnifies time against the main view.
    pub(super) fn studio_time_zoom(&self) -> f32 {
        self.m.time_per_div() / tscales()[self.m.zscale].max(1e-12)
    }

    pub(super) fn studio_zoom_bar(&self) -> Option<ZoomBar> {
        if !self.studio_zoom_active() { return None; }
        let row = self.zoom_readout();
        let (y, h) = (row.y + 2, (row.h - 4).max(20));
        let close = Rect::new(row.right() - 46, y, 40, h);
        let more = Rect::new(close.x - 6 - 40, y, 40, h);
        // a narrow plot (the eye or SIM beside it) keeps these two as icons
        let (fit_w, center_w) = if row.w < 760 { (40, 40) } else { (118, 136) };
        let fit = Rect::new(more.x - 6 - fit_w, y, fit_w, h);
        let center = Rect::new(fit.x - 6 - center_w, y, center_w, h);
        let f = self.fonts();
        let label = Rect::new(row.x + 8, y, f.mb.width("Zoom") + 6, h);
        let tdiv = Rect::new(label.right() + 6, y, 118, h);
        let plus = Rect::new(tdiv.right() + 6, y, 40, h);
        let minus = Rect::new(plus.right() + 6, y, 40, h);
        // the vertical zoom comes after the time zoom, when the chips on the right leave the room
        let vplus = Rect::new(minus.right() + 12, y, 40, h);
        let vminus = Rect::new(vplus.right() + 6, y, 40, h);
        let (vplus, vminus) = if vminus.right() + 8 <= center.x { (vplus, vminus) } else { (Rect::new(0, 0, 0, 0), Rect::new(0, 0, 0, 0)) };
        // the readout takes what is left before the chips on the right; it is not drawn when there is no room
        let rx = if vminus.w > 0 { vminus.right() } else { minus.right() } + 8;
        let readout = Rect::new(rx, y, (center.x - 8 - rx).clamp(0, 120), h);
        Some(ZoomBar { label, tdiv, plus, minus, vplus, vminus, readout, close, center, fit, more })
    }

    /// The trigger is outside the zoomed window.
    fn studio_trigger_off_screen(&self) -> bool {
        let (tz, dz) = self.view();
        dz.abs() > tz * HDIV / 2.0
    }

    pub(super) fn studio_zoom_menu_rect(&self) -> Option<Rect> {
        let bar = self.studio_zoom_bar().filter(|_| self.studio.zoom_menu)?;
        let h = 40 + 44 + 44 + 36 + 8 + 4 * 38 + 8;
        Some(Rect::new((bar.more.right() - ZMENU_W).max(4), bar.more.bottom() + 6, ZMENU_W, h))
    }

    /// The rows of the menu with their rectangles.
    pub(super) fn studio_zoom_rows(&self) -> Vec<(ZoomRow, Rect)> {
        let Some(p) = self.studio_zoom_menu_rect() else { return Vec::new() };
        let x = p.x + 8;
        let w = p.w - 16;
        let mut y = p.y + 40;
        let mut out = vec![
            (ZoomRow::Minus, Rect::new(p.right() - 8 - 48 - 100 - 48 - 8, y + 4, 48, 36)),
            (ZoomRow::Plus, Rect::new(p.right() - 8 - 48, y + 4, 48, 36)),
        ];
        y += 44;
        out.push((ZoomRow::Tdiv, Rect::new(x, y + 2, w, 40)));
        y += 44 + 36 + 8;
        for row in [ZoomRow::Fit, ZoomRow::Center, ZoomRow::ResetY, ZoomRow::Close] {
            out.push((row, Rect::new(x, y, w, 38)));
            y += 38;
        }
        out
    }

    pub(super) fn draw_studio_zoom_bar(&self, cv: &mut Canvas) {
        let Some(bar) = self.studio_zoom_bar() else { return };
        let f = self.fonts();
        cv.fill(self.zoom_readout(), BG);
        let open = self.studio.zoom_menu;
        cv.text_in(&f.mb, bar.label, 0, "Zoom", TEXT);
        chip(cv, bar.tdiv, BTN, BTN_BORDER);
        cv.text_in(&f.m, Rect::new(bar.tdiv.x + 10, bar.tdiv.y, bar.tdiv.w - 30, bar.tdiv.h), 0, &format!("{}/div", si_short(tscales()[self.m.zscale] as f64, "s")), TEXT);
        chevron_down(cv, bar.tdiv.right() - 20, bar.tdiv.y + bar.tdiv.h / 2 - 2, DIM);
        for (r, plus) in [(bar.plus, true), (bar.minus, false)] {
            chip(cv, r, BTN, BTN_BORDER);
            let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
            cv.hline(cx - 6, cx + 6, cy, TEXT);
            if plus { cv.vline(cx, cy - 6, cy + 6, TEXT); }
        }
        for (r, plus) in [(bar.vplus, true), (bar.vminus, false)] {
            if r.w == 0 { continue; }
            chip(cv, r, BTN, BTN_BORDER);
            let (cx, cy) = (r.x + r.w / 2 - 7, r.y + r.h / 2);
            cv.hline(cx - 5, cx + 5, cy, TEXT);
            if plus { cv.vline(cx, cy - 5, cy + 5, TEXT); }
            cv.text_in(&f.sb, Rect::new(r.x + r.w / 2 + 4, r.y, r.w / 2 - 6, r.h), 1, "Y", DIM);
        }
        let readout = format!("({} zoom)", zoom_text(self.studio_time_zoom()));
        if bar.readout.w >= f.s.width(&readout) { cv.text_in(&f.s, bar.readout, 0, &readout, DIM); }
        chip(cv, bar.close, BTN, BTN_BORDER);
        let (cx, cy) = (bar.close.x + bar.close.w / 2, bar.close.y + bar.close.h / 2);
        cv.line(cx - 5, cy - 5, cx + 5, cy + 5, TEXT);
        cv.line(cx - 5, cy + 5, cx + 5, cy - 5, TEXT);
        // Center trigger turns amber while the trigger is out of the window
        let off = self.studio_trigger_off_screen();
        chip(cv, bar.center, BTN, if off { AMBER } else { BTN_BORDER });
        let icon_only = bar.center.w < 100;
        let (cx, cy) = (if icon_only { bar.center.x + bar.center.w / 2 } else { bar.center.x + 18 }, bar.center.y + bar.center.h / 2);
        cv.circle(cx, cy, 7, TEXT);
        cv.circle(cx, cy, 5, BTN);
        cv.circle(cx, cy, 2, TEXT);
        if !icon_only { cv.text_in(&f.m, Rect::new(bar.center.x + 32, bar.center.y, bar.center.w - 34, bar.center.h), 0, "Center trigger", TEXT); }
        chip(cv, bar.fit, BTN, BTN_BORDER);
        icon_expand(cv, if icon_only { bar.fit.x + bar.fit.w / 2 } else { bar.fit.x + 18 }, bar.fit.y + bar.fit.h / 2, TEXT);
        if !icon_only { cv.text_in(&f.m, Rect::new(bar.fit.x + 32, bar.fit.y, bar.fit.w - 34, bar.fit.h), 0, "Fit record", TEXT); }
        chip(cv, bar.more, BTN, if open { ACCENT } else { BTN_BORDER });
        for dx in [-8, 0, 8] { cv.circle(bar.more.x + bar.more.w / 2 + dx, bar.more.y + bar.more.h / 2, 2, TEXT); }
    }

    /// The window in the overview: shaded, framed, with a grip at each end.
    pub(super) fn draw_studio_window(&self, cv: &mut Canvas, o: Rect, xa: i32, xb: i32) {
        // a thin window (a large zoom of a long record) stays a visible bar
        let (xa, xb) = if xb - xa < 6 { let m = (xa + xb) / 2; (m - 3, m + 3) } else { (xa, xb) };
        let w = Rect::new(xa, o.y, (xb - xa).max(2), o.h);
        cv.blend(w, BAND, 50);
        cv.rect(w, BAND);
        for edge in [xa, xb] {
            let r = Rect::new(edge - 7, o.y + o.h / 2 - 14, 14, 28);
            cv.round(r, 4, BAND, 255);
            for k in [-3, 0, 3] { cv.vline(edge + k, r.y + 7, r.bottom() - 8, DARK_TEXT); }
        }
    }

    /// The menu of the zoom toolbar's last chip.
    pub(super) fn draw_studio_zoom_menu(&self, cv: &mut Canvas) {
        let Some(p) = self.studio_zoom_menu_rect() else { return };
        let f = self.fonts();
        cv.round(p, 8, BTN_BORDER, 255);
        cv.round(p.inset(1), 7, PANEL_BG, 255);
        if let Some(bar) = self.studio_zoom_bar() {
            let x = bar.more.x + bar.more.w / 2;
            cv.triangle([(x - 7, p.y + 1), (x + 7, p.y + 1), (x, p.y - 6)], BTN_BORDER);
            cv.triangle([(x - 6, p.y + 2), (x + 6, p.y + 2), (x, p.y - 4)], PANEL_BG);
        }
        cv.text(&f.lb, p.x + 14, p.y + 28, "Zoom & navigation", TEXT);
        cv.hline(p.x + 8, p.right() - 9, p.y + 38, LINE);
        let mut linked_y = p.y + 40 + 44 + 44;
        for (row, r) in self.studio_zoom_rows() {
            let cy = r.y + r.h / 2;
            match row {
                ZoomRow::Minus | ZoomRow::Plus => {
                    chip(cv, r, BTN, BTN_BORDER);
                    cv.hline(r.x + r.w / 2 - 6, r.x + r.w / 2 + 6, cy, TEXT);
                    if row == ZoomRow::Plus { cv.vline(r.x + r.w / 2, cy - 6, cy + 6, TEXT); }
                    if row == ZoomRow::Minus {
                        cv.text(&f.m, p.x + 14, cy + 5, "Time zoom", TEXT);
                        let v = Rect::new(r.right() + 6, r.y, 100, r.h);
                        chip(cv, v, BTN, BTN_BORDER);
                        cv.text_in(&f.mb, v, 1, &zoom_text(self.studio_time_zoom()), TEXT);
                    }
                }
                ZoomRow::Tdiv => {
                    chip(cv, r, BTN, BTN_BORDER);
                    cv.text_in(&f.m, Rect::new(r.x + 12, r.y, r.w - 40, r.h), 0, &format!("{}/div", si_short(tscales()[self.m.zscale] as f64, "s")), TEXT);
                    chevron_down(cv, r.right() - 22, cy - 2, DIM);
                    linked_y = r.bottom() + 2;
                }
                _ => {
                    if row == ZoomRow::Fit { cv.hline(p.x + 8, p.right() - 9, r.y - 4, LINE); }
                    let label = match row { ZoomRow::Fit => "Fit record", ZoomRow::Center => "Center trigger", ZoomRow::ResetY => "Reset vertical zoom", _ => "Close zoom" };
                    let (ix, iy) = (r.x + 18, cy);
                    match row {
                        ZoomRow::Fit => icon_expand(cv, ix, iy, TEXT),
                        ZoomRow::Center => { cv.circle(ix, iy, 7, TEXT); cv.circle(ix, iy, 5, PANEL_BG); cv.circle(ix, iy, 2, TEXT); }
                        ZoomRow::ResetY => { cv.vline(ix, iy - 7, iy + 7, TEXT); icon_arrow(cv, ix, iy - 8, true, TEXT); icon_arrow(cv, ix, iy + 8, false, TEXT); }
                        _ => { cv.line(ix - 5, iy - 5, ix + 5, iy + 5, TEXT); cv.line(ix - 5, iy + 5, ix + 5, iy - 5, TEXT); }
                    }
                    cv.text_in(&f.m, Rect::new(r.x + 42, r.y, r.w - 44, r.h), 0, label, TEXT);
                }
            }
        }
        // the lanes of Studio always share one time axis
        cv.circle(p.x + 28, linked_y + 18, 5, DIM);
        cv.circle(p.x + 36, linked_y + 18, 5, DIM);
        cv.text(&f.m, p.x + 52, linked_y + 23, "Time axes linked", LABEL);
    }

    /// The display scale popup of a lane (a tap on its channel badge while zoomed).
    pub(super) fn studio_yzoom_rect(&self) -> Option<Rect> {
        let c = self.studio.yzoom?;
        let (_, badge) = self.studio.lane_badges.borrow().iter().find(|(k, _)| *k == c).copied()?;
        let h = 40 + 46 + 44;
        let y = badge.y.min(self.frame().bottom.y - h - 4).max(self.frame().top.bottom() + 4);
        Some(Rect::new(badge.right() + 8, y, 250, h))
    }

    /// Minus, plus and Reset Y of that popup.
    pub(super) fn studio_yzoom_parts(&self) -> Option<(Rect, Rect, Rect, Rect)> {
        let p = self.studio_yzoom_rect()?;
        Some((Rect::new(p.right() - 38, p.y + 4, 32, 32), Rect::new(p.x + 10, p.y + 44, 48, 36),
            Rect::new(p.right() - 58, p.y + 44, 48, 36), Rect::new(p.x + 10, p.y + 40 + 46, p.w - 20, 36)))
    }

    pub(super) fn draw_studio_yzoom(&self, cv: &mut Canvas) {
        let (Some(p), Some((close, minus, plus, reset)), Some(c)) = (self.studio_yzoom_rect(), self.studio_yzoom_parts(), self.studio.yzoom) else { return };
        let f = self.fonts();
        cv.round(p, 8, BTN_BORDER, 255);
        cv.round(p.inset(1), 7, PANEL_BG, 255);
        cv.text(&f.mb, p.x + 12, p.y + 26, &format!("CH{} display scale", c + 1), TEXT);
        let (cx, cy) = (close.x + close.w / 2, close.y + close.h / 2);
        cv.line(cx - 5, cy - 5, cx + 5, cy + 5, TEXT);
        cv.line(cx - 5, cy + 5, cx + 5, cy - 5, TEXT);
        for (r, plus_sign) in [(minus, false), (plus, true)] {
            chip(cv, r, BTN, BTN_BORDER);
            cv.hline(r.x + r.w / 2 - 6, r.x + r.w / 2 + 6, r.y + r.h / 2, TEXT);
            if plus_sign { cv.vline(r.x + r.w / 2, r.y + r.h / 2 - 6, r.y + r.h / 2 + 6, TEXT); }
        }
        cv.text_in(&f.mb, Rect::new(minus.right(), minus.y, plus.x - minus.right(), minus.h), 1, &format!("Y zoom {}", zoom_text(self.zoom_vfactor)), TEXT);
        chip(cv, reset, BTN, BTN_BORDER);
        cv.text_in(&f.m, reset, 1, "Reset Y", TEXT);
    }

    /// What the zoomed lanes say about their own state: the vertical zoom, and a hint while a gesture is on.
    pub(super) fn draw_studio_zoom_overlay(&self, cv: &mut Canvas) {
        if !self.studio_zoom_active() { return; }
        let f = self.fonts();
        let lane_tag = |cv: &mut Canvas, lane: Rect, text: &str| {
            let w = f.sb.width(text) + 16;
            let r = Rect::new(self.right_tag_x(lane, lane.y + 6, w) - w + 26, lane.y + 6, w, 20);
            cv.round(r, 5, BTN_BORDER, 255);
            cv.round(r.inset(1), 4, PANEL_BG, 255);
            cv.text_in(&f.sb, r, 1, text, TEXT);
        };
        if (self.zoom_vfactor - 1.0).abs() > 1e-3 {
            for &lane in &self.wins { lane_tag(cv, lane, &format!("Y zoom {}", zoom_text(self.zoom_vfactor))); }
        }
        if let (Some(hint), Some(&lane)) = (self.studio_zoom_samples_hint(), self.wins.last()) {
            let w = f.sb.width(&hint) + 16;
            let r = Rect::new(lane.x + 52, lane.bottom() - 52, w, 20);
            cv.round(r, 5, BTN_BORDER, 255);
            cv.round(r.inset(1), 4, PANEL_BG, 255);
            cv.text_in(&f.sb, r, 1, &hint, AMBER);
        }
        // the hint of the gesture under the finger
        let lane = self.wave_area();
        let (cx, cy) = (lane.x + lane.w / 2, lane.y + lane.h / 3);
        let hint = if self.pinch.active { Some(format!("Time zoom \u{b7} {}", zoom_text(self.studio_time_zoom()))) }
            else if self.touch.down && self.touch.drag == Drag::Delay { Some("Pan time".to_string()) } else { None };
        if let Some(text) = hint {
            let r = Rect::new(cx - f.m.width(&text) / 2 - 12, cy - 14, f.m.width(&text) + 24, 28);
            cv.round(r, 6, BTN_BORDER, 255);
            cv.round(r.inset(1), 5, PANEL_BG, 255);
            cv.text_in(&f.m, r, 1, &text, TEXT);
        }
    }

    pub(super) fn studio_zoom_do(&mut self, row: ZoomRow) {
        let area = self.wave_area();
        match row {
            ZoomRow::Minus => self.zoom_horizontal_step(1, area, 0.5),
            ZoomRow::Plus => self.zoom_horizontal_step(-1, area, 0.5),
            ZoomRow::VPlus => self.zoom_vertical_step(1, area, 0.5),
            ZoomRow::VMinus => self.zoom_vertical_step(-1, area, 0.5),
            ZoomRow::Tdiv => { self.studio.zoom_menu = false; self.open_menu(MenuId::Horizontal); }
            ZoomRow::Fit => self.reset_zoom(),
            ZoomRow::Center => self.studio_center_trigger(),
            ZoomRow::ResetY => { self.zoom_vfactor = 1.0; self.zoom_vcenter = 0.0; self.dirty_map = true; }
            ZoomRow::Close => { self.studio.zoom_menu = false; self.set_zoom(false); }
        }
    }

    /// Puts the trigger in the middle of the zoomed window.
    pub(super) fn studio_center_trigger(&mut self) {
        self.m.zdelay = 0.0;
        self.clamp_zoom();
        if self.m.zdelay != 0.0 {
            // the trigger is outside the main window: the window comes to it, so the box can too
            self.m.delay = 0.0;
            self.m.zdelay = 0.0;
            self.clamp_zoom();
        }
        self.dirty_map = true;
        self.settings_changed();
    }

    /// Seconds between the samples of the record the zoom shows (the held or captured record, or the ref of a SIM or file).
    pub(super) fn studio_zoom_sample_dt(&self) -> Option<f64> {
        if self.record_mode() {
            return self.refs.get(self.active_reference()).and_then(Option::as_ref).map(|r| r.dt).filter(|dt| *dt > 0.0);
        }
        let rec = self.last[0].as_ref()?;
        let dt = rec.raw.as_ref().map_or(rec.dt, |raw| raw.frame.dt);
        (dt > 0.0).then_some(dt)
    }

    /// The hint when the zoom asks for more than the record has: how many samples there are in a division.
    pub(super) fn studio_zoom_samples_hint(&self) -> Option<String> {
        if !self.studio_zoom_active() { return None; }
        let per_div = tscales()[self.m.zscale] as f64 / self.studio_zoom_sample_dt()?;
        // below about one sample in three pixels of a division the picture is the samples themselves
        (per_div < (self.wins.first().map_or(self.grid.w, |w| w.w) as f64 / HDIV as f64) / 3.0)
            .then(|| if per_div < 10.0 { format!("{per_div:.1} samples/div") } else { format!("{per_div:.0} samples/div") })
    }

    /// Taps on the zoom bar, its menu, the display scale popup and the lane badges. False: none of them.
    pub(super) fn studio_zoom_tap(&mut self, x: i32, y: i32) -> bool {
        if self.studio.yzoom.is_some() {
            let parts = self.studio_yzoom_parts();
            let inside = self.studio_yzoom_rect().is_some_and(|p| p.contains(x, y));
            if let (Some((close, minus, plus, reset)), true) = (parts, inside) {
                let area = self.wave_area();
                if close.contains(x, y) { self.studio.yzoom = None; }
                else if minus.contains(x, y) { self.zoom_vertical_step(-1, area, 0.5); }
                else if plus.contains(x, y) { self.zoom_vertical_step(1, area, 0.5); }
                else if reset.contains(x, y) { self.studio_zoom_do(ZoomRow::ResetY); }
                return true;
            }
            self.studio.yzoom = None;
            return true;
        }
        if self.studio.zoom_menu {
            let inside = self.studio_zoom_menu_rect().is_some_and(|p| p.contains(x, y));
            if inside {
                if let Some((row, _)) = self.studio_zoom_rows().into_iter().find(|(_, r)| r.contains(x, y)) { self.studio_zoom_do(row); }
                return true;
            }
            self.studio.zoom_menu = false;
            // a tap on the chip that opened it only closes it
            if self.studio_zoom_bar().is_some_and(|b| b.more.contains(x, y)) { return true; }
        }
        if let Some(bar) = self.studio_zoom_bar() {
            let y = self.zoom_row_y(x, y);
            if self.zoom_readout().contains(x, y) {
                if bar.readout.contains(x, y) { self.studio.zoom_menu = true; }
                else if bar.plus.contains(x, y) { self.studio_zoom_do(ZoomRow::Plus); }
                else if bar.minus.contains(x, y) { self.studio_zoom_do(ZoomRow::Minus); }
                else if bar.vplus.contains(x, y) { self.studio_zoom_do(ZoomRow::VPlus); }
                else if bar.vminus.contains(x, y) { self.studio_zoom_do(ZoomRow::VMinus); }
                else if bar.close.contains(x, y) { self.studio_zoom_do(ZoomRow::Close); }
                else if bar.tdiv.contains(x, y) { self.open_menu(MenuId::Horizontal); }
                else if bar.center.contains(x, y) { self.studio_center_trigger(); }
                else if bar.fit.contains(x, y) { self.reset_zoom(); }
                else if bar.more.contains(x, y) { self.studio.zoom_menu = true; }
                return true;
            }
        }
        // a lane's channel badge: its display scale while zoomed, its menu otherwise
        let hit = self.studio.lane_badges.borrow().iter().find(|(_, r)| Self::hit40(*r).contains(x, y)).map(|&(c, _)| c);
        if let Some(c) = hit {
            self.select_workspace_source(WaveId::Ch(c));
            if self.studio_zoom_active() { self.studio.yzoom = Some(c); } else { self.toggle_menu(MenuId::Channel(c)); }
            return true;
        }
        false
    }
}

#[cfg(test)]
impl Ui {
    pub(super) fn studio_trigger_off_screen_for_test(&self) -> bool {
        self.studio_trigger_off_screen()
    }
}

#[cfg(test)]
impl Ui {
    pub(super) fn studio_set_pin_all_for_test(&mut self) {
        let keys = self.studio_keys();
        for k in keys.into_iter().take(3) { self.studio_set_pin(k, true); }
    }
}
