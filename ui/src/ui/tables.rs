// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Tables over the plot and in the second pane: decoder events, search events, measurements. A tap on an event moves the
//! view to it while the scope stands.

use super::*;

/// What the table window shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum TableKind {
    Decode,
    Search,
}

/// What Split puts under the plot. 0 is traces: two stacked windows, and this pane is not drawn.
pub const PANES: [&str; 5] = ["Traces", "Spectrum", "Results", "Events", "Trend"];
pub(super) const PANE_SPECTRUM: usize = 1;
pub(super) const PANE_RESULTS: usize = 2;
pub(super) const PANE_EVENTS: usize = 3;
pub(super) const PANE_TREND: usize = 4;

const ROW_H: i32 = 26;
const HEAD_H: i32 = 40;

/// One line of an event table.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct EventRow {
    pub t: f64,
    pub who: String,
    pub text: String,
    pub ok: bool,
}

impl Ui {
    /// Everything the decoders found in the record, in time order.
    pub(super) fn decode_events(&self) -> Vec<EventRow> {
        let mut v = Vec::new();
        for k in 0..DECODERS {
            if !self.m.dec_is_on(k) {
                continue;
            }
            let m = self.m.dec_view(k);
            let Ok(p) = self.decode_packets_of(&m) else { continue };
            for x in &p {
                let text = packet_tag(&m, x).map_or_else(|| packet_text(&m, x), |t| t.to_string());
                v.push(EventRow { t: x.t0, who: format!("D{} {}", k + 1, m.dec_proto.name()), text, ok: x.ok });
            }
        }
        v.sort_by(|a, b| a.t.total_cmp(&b.t));
        v
    }

    fn search_rows(&self) -> Vec<EventRow> {
        self.search_events().into_iter().enumerate().map(|(i, t)| EventRow { t, who: format!("{}", i + 1), text: self.m.trig_type.name().to_string(), ok: true }).collect()
    }

    /// Whether the decoder event table is open.
    pub(crate) fn decode_table_on(&self) -> bool {
        self.table == Some(TableKind::Decode)
    }

    /// Opens or closes the decoder event table. A search table stays up until this opens the decoder one.
    pub(crate) fn set_decode_table(&mut self, on: bool) {
        if on {
            self.table = Some(TableKind::Decode);
            self.table_top = 0;
        } else if self.table == Some(TableKind::Decode) {
            self.table = None;
        }
    }

    /// The decoder events as text, one per line: time, decoder, content (SCPI).
    pub fn events_text(&self) -> String {
        self.decode_events().iter().map(|r| format!("{:e},{},{},{}", r.t, r.who, r.text, if r.ok { "ok" } else { "error" })).collect::<Vec<_>>().join("\n")
    }

    pub(super) fn table_rows(&self, k: TableKind) -> Vec<EventRow> {
        match k {
            TableKind::Decode => self.decode_events(),
            TableKind::Search => self.search_rows(),
        }
    }

    /// The table window over the plot: rectangle and the rows that fit.
    fn table_rect(&self, n: usize) -> (Rect, usize) {
        let a = self.over_area();
        let fit = ((a.h - 16 - HEAD_H) / ROW_H).max(1) as usize;
        let rows = n.clamp(1, fit);
        (Rect::new(a.x + 30, a.y + 8, 420, HEAD_H + rows as i32 * ROW_H + 4), rows)
    }

    pub(super) fn table_toggle(&mut self, k: TableKind) {
        self.table = if self.table == Some(k) { None } else { Some(k) };
        self.table_top = 0;
        self.close_menu();
    }

    /// Buttons of the table head: up, down, close.
    fn table_buttons(r: Rect) -> [Rect; 3] {
        let b = |i: i32| Rect::new(r.x + r.w - 46 * i, r.y + 2, 42, HEAD_H - 4);
        [b(3), b(2), b(1)]
    }

    pub(super) fn draw_table(&self, cv: &mut Canvas) {
        let Some(k) = self.table else { return };
        let f = &self.fonts;
        let rows = self.table_rows(k);
        let (r, fit) = self.table_rect(rows.len());
        cv.round(r, 6, CARD_BORDER, 255);
        cv.round(r.inset(1), 5, BAR, 240);
        let title = match k {
            TableKind::Decode => format!("Decoder events \u{b7} {}", rows.len()),
            TableKind::Search => format!("Search events \u{b7} {}", rows.len()),
        };
        cv.text(&f.sb, r.x + 12, r.y + 25, &title, TEXT);
        for (b, t) in Self::table_buttons(r).iter().zip(["Up", "Dn", "\u{d7}"]) {
            btn(cv, *b, BTN, BTN_BORDER);
            cv.text_in(&f.mb, *b, 1, t, TEXT);
        }
        let top = self.table_top.min(rows.len().saturating_sub(fit));
        if rows.is_empty() {
            cv.text(&f.s, r.x + 12, r.y + HEAD_H + 18, "nothing found in this record", DIM);
        }
        for (i, e) in rows.iter().skip(top).take(fit).enumerate() {
            let y = r.y + HEAD_H + i as i32 * ROW_H;
            cv.hline(r.x + 8, r.x + r.w - 9, y, LINE);
            cv.text(&f.s, r.x + 12, y + 18, &si(e.t, "s"), LABEL);
            cv.text(&f.s, r.x + 110, y + 18, &e.who, DIM);
            cv.text(&f.sb, r.x + 220, y + 18, &e.text, if e.ok { TEXT } else { RED });
        }
    }

    /// A tap while the table is open; true when it was on the table.
    pub(super) fn table_tap(&mut self, x: i32, y: i32) -> bool {
        let Some(k) = self.table else { return false };
        let rows = self.table_rows(k);
        let (r, fit) = self.table_rect(rows.len());
        if !r.contains(x, y) {
            return false;
        }
        let [up, down, close] = Self::table_buttons(r);
        let last = rows.len().saturating_sub(fit);
        self.table_top = self.table_top.min(last);
        if close.contains(x, y) {
            self.table = None;
        } else if up.contains(x, y) {
            self.table_top = self.table_top.saturating_sub(fit);
        } else if down.contains(x, y) {
            self.table_top = (self.table_top + fit).min(last);
        } else if y >= r.y + HEAD_H {
            let i = self.table_top + ((y - r.y - HEAD_H) / ROW_H) as usize;
            if let Some(e) = rows.get(i) {
                let t = e.t;
                self.go_to_time(t);
            }
        }
        true
    }

    /// The view centred on a time of the record.
    pub(super) fn go_to_time(&mut self, t: f64) {
        if self.m.run == RunState::Run {
            return self.toast("Stop the scope to go to an event");
        }
        if self.m.zoom {
            self.m.zdelay = t as f32;
            self.clamp_zoom();
        } else {
            self.m.delay = t as f32;
        }
        self.settings_changed();
        self.toast(&format!("At {}", si(t, "s")));
    }

    /// The events as CSV into the storage directory.
    pub(super) fn events_export(&mut self, k: TableKind) {
        let rows = self.table_rows(k);
        if rows.is_empty() {
            return self.toast("No events in this record");
        }
        let mut t = String::from("Time (s),Source,Event,Ok\n");
        for e in &rows {
            t.push_str(&format!("{:.9e},{},{},{}\n", e.t, e.who, e.text.replace(',', ";"), if e.ok { 1 } else { 0 }));
        }
        let p = crate::store::next_name(&self.save_dir(), "events", "csv");
        let r = crate::store::write(&p, t.as_bytes());
        self.toast_saved(&p, r);
    }

    /// The Results pane shows measurements and cursors, so the strip under the plot need not. With LA lanes, which share
    /// that room and may leave none for the pane, the strip stays.
    pub(super) fn results_pane_on(&self) -> bool {
        self.m.pane == PANE_RESULTS && self.m.view == AView::Scope && !self.xy && !self.m.la_on()
    }

    /// Height the second pane takes from the plot: what its content needs, the plot gets the rest.
    pub(super) fn pane_h(&self) -> i32 {
        if self.m.pane == 0 || self.m.view != AView::Scope || self.xy {
            return 0;
        }
        // the plot keeps its height; with many LA lanes there may be no room left
        let room = self.plot_base() - self.la_strip_h() - 160;
        let h = ((BOTTOM.y - PLOT_Y) * 2 / 5).min(room);
        if h < 100 { 0 } else { h.min(self.pane_want() + 4) }
    }

    /// The second pane: under the plot and the lanes.
    pub(super) fn pane_rect(&self) -> Rect {
        let g = self.grid;
        Rect::new(g.x, g.y + g.h + 2 + self.eye_below() + self.la_strip_h(), g.w, self.pane_h() - 4)
    }

    /// The close button of the pane.
    pub(super) fn pane_close(r: Rect) -> Rect {
        Rect::new(r.x + r.w - 46, r.y + 2, 42, 36)
    }

    pub(super) fn draw_pane_events(&self, cv: &mut Canvas, r: Rect) {
        let f = &self.fonts;
        let rows = &self.pn.events;
        if rows.is_empty() {
            cv.text(&f.sb, r.x + 12, r.y + r.h / 2 + 5, "No decoder events in this record", DIM);
            return;
        }
        let (cols, fit) = panes::event_grid(rows.len(), r.h);
        let pitch = r.h / fit as i32;
        let col_w = (r.w - 54) / cols as i32;
        for (i, e) in rows.iter().take(fit * cols).enumerate() {
            let (cx, y) = (r.x + (i / fit) as i32 * col_w, r.y + (i % fit) as i32 * pitch);
            if i % fit % 2 == 1 {
                cv.fill(Rect::new(cx, y, col_w - 4, pitch), ROW_ALT);
            }
            let ty = y + (pitch + 9) / 2;
            cv.text(&f.s, cx + 12, ty, &si(e.t, "s"), LABEL);
            cv.text(&f.s, cx + 102, ty, &e.who, DIM);
            cv.text(&f.sb, cx + 212, ty, &e.text, if e.ok { TEXT } else { RED });
        }
    }

    pub(super) fn draw_pane_close(&self, cv: &mut Canvas) {
        let b = Self::pane_close(self.pane_rect());
        btn(cv, b, BTN, BTN_BORDER);
        cv.text_in(&self.fonts.mb, b, 1, "\u{d7}", TEXT);
    }
}
