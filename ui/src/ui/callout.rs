// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later
//! Callouts: a short text pinned to a point of a signal. A callout belongs to a source and a time; it is drawn at
//! the value the source has there, with a line to its text, and moves with the signal when the view is scrolled or
//! scaled. Added from the hold menu of the plot ("Add note here"), edited by a tap on its text, removed by emptying
//! it. They are part of the setup text (and so of a session).

use super::wlan::{Osk, OskFor};
use super::*;

/// Most callouts on the screen: more would hide the signal they explain.
pub(super) const CALLOUT_MAX: usize = 8;
/// Longest text of a callout.
pub(super) const CALLOUT_LEN: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Callout {
    pub src: WaveId,
    /// seconds, on the time axis of the record (0 is the trigger)
    pub t: f64,
    pub text: String,
}

impl Ui {
    /// The time under the screen column `x` of the lane of `src`.
    fn callout_time_at(&self, src: WaveId, x: i32) -> f64 {
        let a = self.source_area(src).unwrap_or(self.area);
        let (tdiv, delay) = self.view();
        delay as f64 + ((x - a.x) as f64 / a.w.max(1) as f64 - 0.5) * HDIV as f64 * tdiv as f64
    }

    /// A callout on `src` at the time under `x`, and the keyboard for its text. An empty text takes it away again.
    pub(super) fn callout_add(&mut self, src: WaveId, x: i32) {
        if self.callouts.len() >= CALLOUT_MAX {
            return self.toast(&format!("{CALLOUT_MAX} notes at most: remove one first"));
        }
        let t = self.callout_time_at(src, x);
        self.callouts.push(Callout { src, t, text: String::new() });
        self.wlan.osk = Some(Osk::with_text(OskFor::Callout(self.callouts.len() - 1), ""));
    }

    /// The keyboard is done: the text of callout `i`. Empty removes the callout.
    pub(super) fn callout_text_done(&mut self, i: usize, text: &str) {
        let text: String = text.trim().chars().take(CALLOUT_LEN).collect();
        if i >= self.callouts.len() {
            return;
        }
        if text.is_empty() {
            self.callouts.remove(i);
            self.toast("Note removed");
        } else {
            self.callouts[i].text = text;
        }
    }

    /// The keyboard was closed without a text: a callout that never had one does not stay as an empty box.
    pub(super) fn callout_drop_empty(&mut self) {
        self.callouts.retain(|c| !c.text.is_empty());
    }

    /// Where callout `i` is on the screen: (the point on the signal, the box of its text). None when its source is not
    /// drawn, its time is outside the view, or the source has no value there.
    pub(super) fn callout_place(&self, i: usize) -> Option<((i32, i32), Rect)> {
        let c = self.callouts.get(i)?;
        let lane = self.source_area(c.src)?;
        let (tdiv, delay) = self.view();
        let fx = 0.5 + (c.t - delay as f64) / (HDIV as f64 * tdiv as f64);
        if !(0.0..=1.0).contains(&fx) {
            return None;
        }
        let x = lane.x + (fx * lane.w as f64) as i32;
        let v = self.wave_at_source(c.src, c.t)?;
        let y = (self.y_of_wave(c.src, v)).clamp(lane.y + 2, lane.bottom() - 2);
        let f = self.fonts();
        let w = f.s.width(if c.text.is_empty() { "note" } else { &c.text }) + 14;
        // the text stands up and to the right of the point, or to the left and below where that would leave the lane
        let bx = if x + 18 + w <= lane.right() - 2 { x + 18 } else { (x - 18 - w).max(lane.x + 2) };
        let by = if y - 34 >= lane.y + 2 { y - 34 } else { (y + 14).min(lane.bottom() - 22) };
        Some(((x, y), Rect::new(bx, by, w, 20)))
    }

    pub(super) fn draw_callouts(&self, cv: &mut Canvas) {
        let f = self.fonts();
        for i in 0..self.callouts.len() {
            let Some(((x, y), r)) = self.callout_place(i) else { continue };
            let color = self.wave_color(self.callouts[i].src);
            // the line ends at the nearest corner of the box
            let (cx, cy) = (if r.x > x { r.x } else { r.right() }, if r.y > y { r.y } else { r.bottom() });
            cv.line(x, y, cx, cy, color);
            cv.circle(x, y, 3, color);
            cv.round(r, 4, PANEL_BG, 240);
            cv.round_outline(r, 4, color);
            cv.text_in(&f.s, r, 1, &self.callouts[i].text, TEXT);
        }
    }

    /// A tap on the text of a callout opens the keyboard on it. True when it took the tap.
    pub(super) fn callout_tap(&mut self, x: i32, y: i32) -> bool {
        let hit = (0..self.callouts.len()).find(|&i| self.callout_place(i).is_some_and(|(_, r)| Rect::new(r.x - 4, r.y - 8, r.w + 8, r.h + 16).contains(x, y)));
        let Some(i) = hit else { return false };
        let now = self.callouts[i].text.clone();
        self.wlan.osk = Some(Osk::with_text(OskFor::Callout(i), &now));
        true
    }

    /// The `callout<N>` lines of a setup: source, time and text.
    pub(super) fn callout_setup_text(&self) -> String {
        self.callouts.iter().enumerate().map(|(i, c)| {
            let src = match c.src { WaveId::Ch(k) => format!("CH{}", k + 1), WaveId::Ref(k) => format!("R{}", k + 1) };
            format!("callout{} = {},{:e},{}\n", i + 1, src, c.t, c.text.replace('\n', " "))
        }).collect()
    }

    /// The callouts of a setup text; lines that do not read are left out.
    pub(super) fn callouts_from_setup(text: &str) -> Vec<Callout> {
        let mut out = Vec::new();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            if !key.trim().strip_prefix("callout").is_some_and(|n| n.parse::<usize>().is_ok()) {
                continue;
            }
            let mut parts = value.trim().splitn(3, ',');
            let (Some(src), Some(t), Some(text)) = (parts.next(), parts.next(), parts.next()) else { continue };
            let src = match src.trim() {
                s if s.starts_with("CH") => s[2..].parse::<usize>().ok().filter(|k| (1..=NCH).contains(k)).map(|k| WaveId::Ch(k - 1)),
                s if s.starts_with('R') => s[1..].parse::<usize>().ok().filter(|k| (1..=NREF).contains(k)).map(|k| WaveId::Ref(k - 1)),
                _ => None,
            };
            let (Some(src), Ok(t)) = (src, t.trim().parse::<f64>()) else { continue };
            let text: String = text.trim().chars().take(CALLOUT_LEN).collect();
            if t.is_finite() && !text.is_empty() && out.len() < CALLOUT_MAX {
                out.push(Callout { src, t, text });
            }
        }
        out
    }
}
