// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The Bus view pane: what a protocol module makes of its decoded packets (`protocols::Protocol::view`), for example
//! the devices on a bus and their last state, under the plot.

use super::*;
use crate::protocols::{Memory, Tone, View};

/// What the Bus view keeps for one decoder.
#[derive(Default)]
pub struct ViewSlot {
    /// the protocol the memory belongs to
    proto: Option<Proto>,
    /// the record the view was made of: (waveforms since clear, history position)
    rec: Option<(u64, Option<usize>)>,
    mem: Memory,
    view: Option<View>,
}

/// Heights of a decoder's head line (name and status), of a column title and of one text row: 30 / 22 / 20 px at the
/// normal text size, growing with the text.
fn heights(f: &crate::font::Fonts) -> (i32, i32, i32) {
    (f.sb.size + 18, f.sb.size + 10, f.s.size + 8)
}

impl Ui {
    /// The views of the decoders that are on and have one: ("D1 name", view), in decoder order. A module sees each
    /// record once; its memory goes when its decoder changes protocol or is switched off.
    pub(super) fn bus_views(&mut self) -> Vec<(String, View)> {
        let mut v = Vec::new();
        let rec = (self.wfm_total as u64, self.hist_pos);
        for k in 0..DECODERS {
            let m = self.m.dec_view(k);
            let module = m.dec_proto.module().filter(|_| self.m.dec_is_on(k));
            let Some(p) = module else {
                self.pn.view_slots[k] = ViewSlot::default();
                continue;
            };
            if self.pn.view_slots[k].proto != Some(m.dec_proto) {
                self.pn.view_slots[k] = ViewSlot { proto: Some(m.dec_proto), ..ViewSlot::default() };
            }
            if self.pn.view_slots[k].rec != Some(rec) {
                let pk = self.decode_packets_of(&m).unwrap_or_default();
                let slot = &mut self.pn.view_slots[k];
                slot.view = p.view(&pk, &m.dec_opts(), &mut slot.mem);
                slot.rec = Some(rec);
            }
            if let Some(view) = &self.pn.view_slots[k].view {
                v.push((format!("D{} {}", k + 1, p.name()), view.clone()));
            }
        }
        v
    }

    /// Forgets what the modules collected (Clear).
    pub(super) fn bus_views_clear(&mut self) {
        self.pn.view_slots = Default::default();
        self.pn.views.clear();
    }

    /// Height of the views: a head line per decoder, its tallest column under it.
    pub(super) fn bus_view_h(&self) -> i32 {
        let (head_h, title_h, row_h) = heights(self.fonts());
        self.pn.views.iter().map(|(_, v)| head_h + v.sections.iter().map(|s| title_h + s.rows.len() as i32 * row_h).max().unwrap_or(0) + 8).sum()
    }

    pub(super) fn draw_pane_bus(&self, cv: &mut Canvas, r: Rect) {
        let f = self.fonts();
        let (head_h, title_h, row_h) = heights(f);
        let views = &self.pn.views;
        if views.is_empty() {
            cv.text(&f.sb, r.x + 12, r.y + r.h / 2 + 5, "No bus view: Decode > Protocol with a view of its own, decoder on", DIM);
            return;
        }
        // room right of the text for the close button
        let w = r.w - 54;
        let mut y = r.y + 4;
        for (who, v) in views {
            cv.text(&f.s, r.x + 12, y + (head_h + f.s.size) / 2 + 1, who, DIM);
            if let Some((s, tone)) = &v.status {
                let col = match tone {
                    Tone::Good => GREEN,
                    Tone::Bad => RED,
                    Tone::Neutral => LABEL,
                };
                let x = r.x + 12 + f.s.width(who) + 14;
                let s = startup::fit_text(&f.sb, s, (r.x + w - x - 20).max(0));
                let pill = Rect::new(x, y + 4, f.sb.width(&s) + 20, head_h - 6);
                cv.round(pill, 6, mix(BG, col, 60), 255);
                cv.text(&f.sb, pill.x + 10, pill.y + (pill.h + f.sb.size) / 2 - 2, &s, col);
            }
            y += head_h;
            let n = v.sections.len().max(1) as i32;
            let col_w = (w - 12) / n;
            for (i, s) in v.sections.iter().enumerate() {
                let x = r.x + 12 + i as i32 * col_w;
                let room = col_w - 12;
                cv.text(&f.sb, x, y + title_h - 6, &startup::fit_text(&f.sb, &s.title, room), LABEL);
                for (j, row) in s.rows.iter().enumerate() {
                    let ry = y + title_h + j as i32 * row_h;
                    if ry + row_h > r.y + r.h {
                        break;
                    }
                    cv.text(&f.s, x, ry + row_h - 5, &startup::fit_text(&f.s, row, room), TEXT);
                }
            }
            y += v.sections.iter().map(|s| title_h + s.rows.len() as i32 * row_h).max().unwrap_or(0) + 8;
            if y >= r.y + r.h {
                break;
            }
            cv.hline(r.x + 8, r.x + w, y - 4, LINE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example module's view comes up in the Bus view pane with its status and rows, and goes when the decoder
    /// switches to a protocol without one.
    #[test]
    fn bus_view_shows_the_module_view() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.set_dec_proto(Proto::from_key("Example").unwrap());
        ui.m.dec_on = true;
        ui.m.tscale = tscales().iter().position(|&x| (x - 1e-3).abs() < 1e-6).unwrap();
        ui.m.pane = tables::PANE_BUS;
        let mut views = Vec::new();
        for _ in 0..6 {
            ui.step(0.3);
            views = ui.pn.views.clone();
            if views.first().is_some_and(|(_, v)| v.sections.first().is_some_and(|s| !s.rows.is_empty())) {
                break;
            }
        }
        let (who, v) = views.first().expect("no view");
        assert_eq!(who, "D1 Example");
        assert_eq!(v.status.as_ref().map(|s| s.1), Some(Tone::Good), "{:?}", v);
        assert!(v.sections[0].rows.iter().any(|r| r == "4F"), "{:?}", v);
        assert!(ui.pane_want() > 60);
        ui.set_dec_proto(Proto::Uart);
        ui.step(0.3);
        assert!(ui.pn.views.is_empty());
    }
}
