// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio: a tap on the trigger level tag at the right edge of the plot ("1.5 V" with the T marker, or "R1 276 mV" for a
//! ref) opens a vertical slider for the level. The thumb is a large touch target and follows the finger; the value on top
//! opens the numeric pad; "50%" sets the level to the middle of the signal. The range is the one of the numeric pad
//! (plus and minus 5 div of the source). A tap outside closes it.
use super::*;

/// What a finger that went down on the slider is doing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SliderPart {
    Thumb,
    Value,
    Fifty,
    /// the panel's padding, or a tap that closed it: nothing more comes of the gesture
    Dead,
}

/// The parts of the slider.
#[derive(Clone, Copy, Debug)]
pub(super) struct SliderGeo {
    pub panel: Rect,
    pub value: Rect,
    pub fifty: Rect,
    /// the track: x of its centre, the y of the top (highest level) and of the bottom (lowest level)
    pub x: i32,
    pub top: i32,
    pub bottom: i32,
}

pub(super) const SLIDER_W: i32 = 150;
const THUMB: i32 = 44;

impl Ui {
    pub(super) fn trig_slider_open(&self) -> bool { self.m.layout == Layout::Studio && self.studio.trig_slider }

    /// The hit area of the trigger level tag on the plot (value and T marker), at least 40 px high.
    pub(super) fn trigger_tag_hit(&self) -> Option<Rect> {
        let f = self.fonts();
        let a = self.area;
        let ts = self.m.trig_src;
        let clamp = |y: i32, a: Rect| { let m = (a.h / 2).min(9); y.clamp(a.y + m, a.y + a.h - m) };
        match self.m.trigger_wave() {
            WaveId::Ch(_) => {
                if !(self.m.ch[ts].on && self.shows(ts) && self.m.trig_line().is_none() && self.trigger_absolute_level_visible(false)) { return None; }
                let lvl = self.trig_level_shown();
                let y = clamp(self.y_of_volts(ts, lvl), a);
                let x1 = self.trigger_tag_x(a.x + a.w - MARK_W, y);
                let s = si_short(lvl as f64, self.m.unit(ts));
                let x0 = x1 - 4 - f.sb.width(&s) - 8;
                Some(Rect::new(x0, y - 20, x1 + 28 - x0, 40))
            }
            id @ WaveId::Ref(i) => {
                self.refs[i].as_ref().filter(|r| r.on)?;
                let a = self.source_area(id)?;
                let r = self.refs[i].as_ref()?;
                let y = clamp(self.zoom_y_of_divs(a, (self.m.trig_level + r.offset) / r.vpd).round() as i32, a);
                let x = a.x + a.w - 22;
                let text = format!("{} {}", Self::trigger_wave_label(id), si_short(self.m.trig_level as f64, "V"));
                let x0 = x - f.s.width(&text) - 4 - 8;
                Some(Rect::new(x0, y - 20, x + 10 + f.sb.width("T") + 8 - x0, 40))
            }
        }
    }

    pub(super) fn trig_slider_geo(&self) -> SliderGeo {
        let g = self.grid;
        let w = SLIDER_W;
        let h = (g.h - 16).clamp(260, 440).min(self.scr.h - 8);
        let x = (g.right() - w - 96).max(g.x + 4);
        let panel = Rect::new(x, (g.y + (g.h - h) / 2).max(4), w, h);
        let value = Rect::new(panel.x + 8, panel.y + 8, w - 16, 52);
        let fifty = Rect::new(panel.x + 8, panel.bottom() - 8 - 52, w - 16, 52);
        SliderGeo { panel, value, fifty, x: panel.x + w / 2, top: value.bottom() + 16 + THUMB / 2, bottom: fifty.y - 16 - THUMB / 2 }
    }

    /// The level range of the slider: that of the numeric pad.
    fn trig_slider_range(&mut self) -> (f64, f64) {
        self.numpad_limits(Item::TrigLevel).unwrap_or((-1.0, 1.0))
    }

    /// The thumb's y for the level now.
    pub(super) fn trig_slider_thumb_y(&self) -> i32 {
        let g = self.trig_slider_geo();
        let r = self.trigger_vpd(false) as f64 * 5.0;
        let frac = if r > 0.0 { ((self.m.trig_level as f64 + r) / (2.0 * r)).clamp(0.0, 1.0) } else { 0.5 };
        g.bottom - (frac * (g.bottom - g.top) as f64).round() as i32
    }

    fn trig_slider_set_at(&mut self, y: i32) {
        let g = self.trig_slider_geo();
        let (lo, hi) = self.trig_slider_range();
        let frac = ((g.bottom - y) as f64 / (g.bottom - g.top).max(1) as f64).clamp(0.0, 1.0);
        self.numpad_apply(Item::TrigLevel, lo + (hi - lo) * frac);
    }

    /// 50%: the middle of the signal (of the ref, for a ref trigger); without a measurement, zero.
    pub(super) fn trig_slider_fifty(&mut self) {
        if self.m.trig_ref.is_none() && !self.stats[self.m.trig_src].valid {
            self.m.trig_level = 0.0;
            self.replay.reset_search();
            self.settings_changed();
        } else {
            self.level_50();
            self.replay.reset_search();
        }
    }

    pub(super) fn trig_slider_down(&mut self, x: i32, y: i32) -> bool {
        if !self.trig_slider_open() { return false; }
        let g = self.trig_slider_geo();
        let part = if !g.panel.contains(x, y) {
            self.studio.trig_slider = false;
            SliderPart::Dead
        } else if g.value.contains(x, y) { SliderPart::Value }
        else if g.fifty.contains(x, y) { SliderPart::Fifty }
        else if y >= g.top - THUMB / 2 && y <= g.bottom + THUMB / 2 && (x - g.x).abs() <= 56 {
            self.trig_slider_set_at(y);
            SliderPart::Thumb
        } else { SliderPart::Dead };
        self.studio.slider_part = Some(part);
        true
    }

    pub(super) fn trig_slider_move(&mut self, y: i32) -> bool {
        match self.studio.slider_part {
            None => false,
            Some(SliderPart::Thumb) => { self.trig_slider_set_at(y); true }
            Some(_) => true,
        }
    }

    pub(super) fn trig_slider_up(&mut self, x: i32, y: i32) -> bool {
        let Some(part) = self.studio.slider_part.take() else { return false };
        if !self.trig_slider_open() { return true; }
        let g = self.trig_slider_geo();
        match part {
            SliderPart::Value if g.value.contains(x, y) => { self.open_numpad(Item::TrigLevel); }
            SliderPart::Fifty if g.fifty.contains(x, y) => self.trig_slider_fifty(),
            _ => {}
        }
        true
    }

    pub(super) fn draw_trig_slider(&self, cv: &mut Canvas) {
        if !self.trig_slider_open() { return; }
        let f = self.fonts();
        let g = self.trig_slider_geo();
        cv.round(g.panel, 10, ACCENT, 255);
        cv.round(g.panel.inset(2), 9, DLG_BG, 255);
        // the value: a tap types it
        let pressed = self.studio.slider_part == Some(SliderPart::Value);
        btn(cv, g.value, if pressed { mix(BTN, ACCENT, 60) } else { BTN }, BTN_BORDER);
        cv.text_in(&f.s, Rect::new(g.value.x, g.value.y + 3, g.value.w, 16), 1, &format!("{} level", Self::trigger_wave_label(self.m.trigger_wave())), DIM);
        cv.text_in(&f.mb, Rect::new(g.value.x, g.value.y + 18, g.value.w, g.value.h - 20), 1, &self.value(Item::TrigLevel), ACCENT);
        // the track, filled up to the thumb
        let r = self.trigger_vpd(false) as f64 * 5.0;
        let unit = self.numpad_value(Item::TrigLevel).map(|v| v.1).unwrap_or_default();
        cv.round(Rect::new(g.x - 4, g.top, 8, g.bottom - g.top), 4, BTN_BORDER, 255);
        let ty = self.trig_slider_thumb_y();
        cv.round(Rect::new(g.x - 4, ty, 8, g.bottom - ty), 4, ACCENT, 255);
        // the middle of the range, and its ends
        cv.hline(g.x - 14, g.x + 14, (g.top + g.bottom) / 2, DIM);
        cv.text(&f.s, g.panel.x + 8, g.top + 4, &si_short(r, &unit), DIM);
        cv.text(&f.s, g.panel.x + 8, g.bottom - 2, &si_short(-r, &unit), DIM);
        let held = self.studio.slider_part == Some(SliderPart::Thumb);
        let thumb = Rect::new(g.x - THUMB / 2, ty - THUMB / 2, THUMB, THUMB);
        cv.round(thumb, THUMB / 2, ACCENT, 255);
        cv.round(thumb.inset(5), THUMB / 2 - 5, if held { ACCENT } else { DLG_BG }, 255);
        cv.text_in(&f.sb, thumb, 1, "T", if held { DARK_TEXT } else { ACCENT });
        let pressed = self.studio.slider_part == Some(SliderPart::Fifty);
        btn(cv, g.fifty, if pressed { mix(BTN, ACCENT, 60) } else { BTN }, BTN_BORDER);
        cv.text_in(&f.mb, g.fifty, 1, "50%", TEXT);
    }
}
