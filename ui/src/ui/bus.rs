// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! One bus on the waveform plot. It is on or off, has a label and a vertical position, and the only
//! type is I2C: a clock and a data line. Each line is a channel or a reference slot, with its own threshold.
//! Decoded words are drawn in hex. The read/write bit can be part of the address byte or left out of it.

use super::{
    Canvas, Item, Kind, MenuId, Rect, Ui, BG, BUS_POS_MAX, BUS_POS_MIN, DARK_TEXT, GREEN, HDIV,
    LINE, NCH, NREF, RED, TEXT,
};
use crate::gfx::{mix, rgb, scale, Rgb};
use crate::model::{n_ch, si, BusKind, WaveId};
use oos_dsp::decode::{i2c_at, Kind as DecKind, Packet};

/// The bus trace. Kept off the channel palette so the line stays its own colour.
const BUS: Rgb = rgb(0xc4, 0x8a, 0x2a);

struct Src<'a> {
    t0: f64,
    dt: f64,
    v: &'a [f32],
    thr: f32,
    hyst: f32,
}

impl Ui {
    pub(super) fn bus_items(&self) -> Vec<Item> {
        vec![
            Item::BusOn,
            Item::BusLabel,
            Item::BusPos,
            Item::BusClk,
            Item::BusClkThr,
            Item::BusData,
            Item::BusDataThr,
            Item::BusRw,
        ]
    }

    pub(super) fn bus_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::BusPos | Item::BusClkThr | Item::BusDataThr => Kind::Value,
            Item::BusLabel => Kind::Action,
            Item::BusOn | Item::BusClk | Item::BusData | Item::BusRw => Kind::Choice,
            _ => return None,
        })
    }

    pub(super) fn bus_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Bus) => "Bus",
            Item::BusOn => "Bus",
            Item::BusLabel => "Label",
            Item::BusPos => "Position",
            Item::BusClk => "Clock",
            Item::BusClkThr => "Clock threshold",
            Item::BusData => "Data",
            Item::BusDataThr => "Data threshold",
            Item::BusRw => "R/W bit",
            _ => return None,
        })
    }

    pub(super) fn bus_value(&self, it: Item) -> Option<String> {
        let b = &self.m.bus;
        Some(match it {
            Item::BusOn => {
                if b.on {
                    "On".into()
                } else {
                    "Off".into()
                }
            }
            Item::BusLabel => b.shown_label(),
            Item::BusPos => format!("{:.1} div", b.pos),
            Item::BusClk => self.wave_name(b.clk),
            Item::BusData => self.wave_name(b.data),
            Item::BusClkThr => si(b.clk_thr as f64, "V"),
            Item::BusDataThr => si(b.data_thr as f64, "V"),
            Item::BusRw => {
                if b.addr_rw {
                    "Included".into()
                } else {
                    "Left out".into()
                }
            }
            _ => return None,
        })
    }

    pub(super) fn bus_options(&self, it: Item) -> Option<Vec<String>> {
        Some(match it {
            Item::BusOn => vec!["Off".into(), "On".into()],
            Item::BusRw => vec!["Left out".into(), "Included".into()],
            Item::BusClk | Item::BusData => self
                .bus_ids()
                .iter()
                .map(|&id| self.wave_name(id))
                .collect(),
            _ => return None,
        })
    }

    pub(super) fn bus_adjust(&mut self, it: Item, d: i32) -> bool {
        match it {
            Item::BusOn => {
                if d != 0 {
                    self.m.bus.on = !self.m.bus.on;
                }
            }
            Item::BusRw => {
                if d != 0 {
                    self.m.bus.addr_rw = !self.m.bus.addr_rw;
                }
            }
            Item::BusPos => {
                self.m.bus.pos = (self.m.bus.pos + 0.1 * d as f32).clamp(BUS_POS_MIN, BUS_POS_MAX)
            }
            Item::BusClk | Item::BusData => {
                let ids = self.bus_ids();
                if ids.is_empty() {
                    return true;
                }
                let cur = if it == Item::BusClk {
                    self.m.bus.clk
                } else {
                    self.m.bus.data
                };
                let i = ids.iter().position(|&x| x == cur).unwrap_or(0);
                let next = ids[(i as i32 + d).rem_euclid(ids.len() as i32) as usize];
                if it == Item::BusClk {
                    self.m.bus.clk = next;
                } else {
                    self.m.bus.data = next;
                }
            }
            Item::BusClkThr | Item::BusDataThr => {
                let id = if it == Item::BusClkThr {
                    self.m.bus.clk
                } else {
                    self.m.bus.data
                };
                let step = self.bus_volt_step(id);
                let thr = if it == Item::BusClkThr {
                    &mut self.m.bus.clk_thr
                } else {
                    &mut self.m.bus.data_thr
                };
                *thr = (*thr + step * d as f32).clamp(-40.0, 40.0);
            }
            _ => return false,
        }
        true
    }

    /// Vertical centre of the bus, in pixels. Positive position is up.
    pub(super) fn bus_y(&self) -> i32 {
        let a = self.area;
        let ppy = a.h as f32 / super::VDIV;
        (a.y as f32 + a.h as f32 / 2.0 - self.m.bus.pos * ppy).round() as i32
    }

    pub(super) fn draw_bus(&self, cv: &mut Canvas) {
        if !self.m.bus.on {
            return;
        }
        let f = &self.fonts;
        let a = self.area;
        if a.w < 40 || a.h < 40 {
            return;
        }
        let y = self.bus_y().clamp(a.y + 12, a.y + a.h - 12);
        cv.set_clip(a);
        let name = self.m.bus.shown_label();
        let bw = (f.sb.width(&name) + 10).max(28);
        let badge = Rect::new(a.x + 28, y - 9, bw, 18);
        cv.round(badge, 3, BUS, 255);
        cv.text_in(&f.sb, badge, 1, &name, DARK_TEXT);
        let left = badge.x + badge.w + 6;
        let right = a.x + a.w - 8;
        if right > left {
            cv.hline(left, right, y, BUS);
        }
        self.draw_bus_levels(cv, a);
        let h = 18;
        let top = y - h / 2;
        let mut x_prev = left;
        for p in self.bus_packets() {
            let x0 = self.x_of_time(p.t0 as f32);
            let x1 = self.x_of_time(p.t1 as f32).max(x0);
            if let Some(tag) = bus_tag(p.kind) {
                if x0 < left - 8 || x0 > right {
                    continue;
                }
                let tw = f.s.width(tag);
                let r = Rect::new(x0 - tw / 2 - 3, top + 2, tw + 6, h - 4);
                cv.round(r, 3, mix(BG, if p.ok { GREEN } else { RED }, 160), 255);
                cv.text_in(&f.s, r, 1, tag, TEXT);
                continue;
            }
            let Some(txt) = bus_word(&p, self.m.bus.addr_rw) else {
                continue;
            };
            if x1 < left || x0 > right {
                continue;
            }
            let x0 = x0.max(x_prev + 1);
            if x1 - x0 < 8 {
                continue;
            }
            let base = if p.ok { BUS } else { RED };
            let fill = if p.ok {
                scale(BUS, 70)
            } else {
                rgb(0x70, 0x18, 0x18)
            };
            cv.fill(Rect::new(x0 + 3, top, (x1 - x0 - 6).max(1), h), fill);
            cv.line(x0, top + h / 2, x0 + 3, top, base);
            cv.line(x0, top + h / 2, x0 + 3, top + h - 1, base);
            cv.line(x1, top + h / 2, x1 - 3, top, base);
            cv.line(x1, top + h / 2, x1 - 3, top + h - 1, base);
            cv.hline(x0 + 3, x1 - 3, top, base);
            cv.hline(x0 + 3, x1 - 3, top + h - 1, base);
            if f.s.width(&txt) + 6 < x1 - x0 {
                cv.text_in(&f.s, Rect::new(x0, top, x1 - x0, h), 1, &txt, TEXT);
            }
            x_prev = x1;
        }
        if self.touch.drag == super::Drag::Bus {
            cv.dashed_hline(a.x, a.x + a.w - 1, y, 6, 4, BUS);
            let s = format!("{:.1} div", self.m.bus.pos);
            let r = Rect::new(a.x + 8, (y - 28).max(a.y + 2), f.sb.width(&s) + 16, 20);
            cv.round(r, 4, LINE, 255);
            cv.text_in(&f.sb, r, 1, &s, BUS);
        }
        cv.no_clip();
    }

    fn draw_bus_levels(&self, cv: &mut Canvas, a: Rect) {
        let b = &self.m.bus;
        for (id, thr) in [(b.clk, b.clk_thr), (b.data, b.data_thr)] {
            let Some(y) = self.bus_level_y(id, thr) else {
                continue;
            };
            cv.dashed_hline(
                a.x + 26,
                a.x + a.w - 26,
                y,
                2,
                6,
                scale(self.wave_color(id), 170),
            );
        }
    }

    fn bus_level_y(&self, id: WaveId, thr: f32) -> Option<i32> {
        match id {
            WaveId::Ch(c) if c < n_ch() && self.m.ch[c].on && self.shows(c) => {
                Some(self.y_of_volts(c, thr))
            }
            WaveId::Ref(i)
                if self
                    .refs
                    .get(i)
                    .and_then(|r| r.as_ref())
                    .is_some_and(|r| r.on) =>
            {
                Some(self.y_of_wave(id, thr))
            }
            _ => None,
        }
    }

    fn bus_ids(&self) -> Vec<WaveId> {
        let mut v: Vec<WaveId> = (0..n_ch()).map(WaveId::Ch).collect();
        v.extend((0..NREF).map(WaveId::Ref));
        v
    }

    fn bus_volt_step(&self, id: WaveId) -> f32 {
        match id {
            WaveId::Ch(c) => self.m.ch[c.min(NCH - 1)].volts_per_div() / 25.0,
            WaveId::Ref(i) => self
                .refs
                .get(i)
                .and_then(|r| r.as_ref())
                .map(|r| r.vpd / 25.0)
                .unwrap_or(0.05),
        }
    }

    fn bus_src(&self, id: WaveId, thr: f32) -> Option<Src<'_>> {
        match id {
            WaveId::Ch(c) => {
                if c >= NCH {
                    return None;
                }
                let rec = self.last[0].as_ref()?;
                let v = &rec.ch[c];
                if v.len() < 2 || !(rec.dt > 0.0) {
                    return None;
                }
                Some(Src {
                    t0: rec.t0,
                    dt: rec.dt,
                    v,
                    thr,
                    hyst: (0.1 * self.m.ch[c].volts_per_div()).max(1e-6),
                })
            }
            WaveId::Ref(i) => {
                let r = self.refs.get(i)?.as_ref()?;
                if r.v.len() < 2 || !(r.dt > 0.0) {
                    return None;
                }
                Some(Src {
                    t0: r.t0,
                    dt: r.dt,
                    v: &r.v,
                    thr,
                    hyst: (0.1 * r.vpd).max(1e-6),
                })
            }
        }
    }

    /// Time window a little wider than the screen, so a word that starts just off the edge still decodes.
    fn bus_span(&self) -> (f64, f64) {
        let (tdiv, delay) = self.view();
        let half = (tdiv as f64) * (HDIV as f64) * 0.5;
        let pad = if half > 0.0 { half * 1.5 } else { 1.0 };
        let mid = delay as f64;
        (mid - pad, mid + pad)
    }

    fn bus_packets(&self) -> Vec<Packet> {
        if !self.m.bus.on || self.m.bus.kind != BusKind::I2c {
            return Vec::new();
        }
        let (lo, hi) = self.bus_span();
        let data = self.bus_src(self.m.bus.data, self.m.bus.data_thr);
        let clk = self.bus_src(self.m.bus.clk, self.m.bus.clk_thr);
        let (Some(data), Some(clk)) = (data, clk) else {
            return Vec::new();
        };
        let Some(sd) = cut(data.t0, data.dt, data.v, lo, hi) else {
            return Vec::new();
        };
        let Some(sc) = cut(clk.t0, clk.dt, clk.v, lo, hi) else {
            return Vec::new();
        };
        let Some((sda, scl, t0, dt)) = align(sd, sc) else {
            return Vec::new();
        };
        if !(dt > 0.0) {
            return Vec::new();
        }
        i2c_at(&sda, &scl, data.thr, data.hyst, clk.thr, clk.hyst)
            .into_iter()
            .map(|mut p| {
                p.t0 = t0 + p.t0 * dt;
                p.t1 = t0 + p.t1 * dt;
                p
            })
            .collect()
    }

    /// Hex words the plot draws for the bus, in time order.
    fn bus_words(&self) -> Vec<String> {
        self.bus_packets()
            .iter()
            .filter_map(|p| bus_word(p, self.m.bus.addr_rw))
            .collect()
    }
}

fn bus_word(p: &Packet, addr_rw: bool) -> Option<String> {
    match p.kind {
        DecKind::Addr { read } => {
            let v = if addr_rw {
                (p.value << 1) | u32::from(read)
            } else {
                p.value
            };
            Some(format!("{:02X}", v))
        }
        DecKind::Data => Some(format!("{:02X}", p.value & 0xff)),
        _ => None,
    }
}

fn bus_tag(kind: DecKind) -> Option<&'static str> {
    match kind {
        DecKind::Start => Some("S"),
        DecKind::RepStart => Some("Sr"),
        DecKind::Stop => Some("P"),
        _ => None,
    }
}

fn cut<'a>(t0: f64, dt: f64, v: &'a [f32], lo: f64, hi: f64) -> Option<(f64, f64, &'a [f32])> {
    if v.len() < 2 || !(dt > 0.0) || !(hi > lo) {
        return None;
    }
    let end = t0 + (v.len() - 1) as f64 * dt;
    let lo = lo.max(t0);
    let hi = hi.min(end);
    if !(hi > lo) {
        return None;
    }
    let i0 = ((lo - t0) / dt).floor().clamp(0.0, (v.len() - 2) as f64) as usize;
    let i1 = (((hi - t0) / dt).ceil() as usize)
        .saturating_add(1)
        .clamp(i0 + 2, v.len());
    Some((t0 + i0 as f64 * dt, dt, &v[i0..i1]))
}

fn align(a: (f64, f64, &[f32]), b: (f64, f64, &[f32])) -> Option<(Vec<f32>, Vec<f32>, f64, f64)> {
    let (at0, adt, av) = a;
    let (bt0, bdt, bv) = b;
    if av.len() < 2 || bv.len() < 2 || !(adt > 0.0) || !(bdt > 0.0) {
        return None;
    }
    let same =
        (at0 - bt0).abs() <= adt * 1e-6 && (adt - bdt).abs() <= adt * 1e-6 && av.len() == bv.len();
    if same {
        return Some((av.to_vec(), bv.to_vec(), at0, adt));
    }
    let t0 = at0.max(bt0);
    let t1 = (at0 + (av.len() - 1) as f64 * adt).min(bt0 + (bv.len() - 1) as f64 * bdt);
    if !(t1 > t0) {
        return None;
    }
    let mut dt = adt.min(bdt);
    let mut n = ((t1 - t0) / dt).floor() as usize + 1;
    const MAX_N: usize = 2_000_000;
    if n > MAX_N {
        n = MAX_N;
        dt = (t1 - t0) / (n.saturating_sub(1).max(1) as f64);
    }
    if n < 8 {
        return None;
    }
    let mut sa = Vec::with_capacity(n);
    let mut sb = Vec::with_capacity(n);
    for i in 0..n {
        let t = t0 + i as f64 * dt;
        sa.push(nearest(at0, adt, av, t));
        sb.push(nearest(bt0, bdt, bv, t));
    }
    Some((sa, sb, t0, dt))
}

fn nearest(t0: f64, dt: f64, v: &[f32], t: f64) -> f32 {
    if v.is_empty() || !(dt > 0.0) {
        return 0.0;
    }
    let i = ((t - t0) / dt).round().clamp(0.0, (v.len() - 1) as f64) as usize;
    v[i]
}

#[cfg(test)]
mod tests {
    use super::{Item, MenuId, Ui, BUS};
    use crate::gfx::{Canvas, Rgb};
    use crate::model::{bus_label, n_ch, WaveId, NCH};
    use crate::wave::{Record, Source};
    use oos_dsp::sim::{i2c_logic, sample, I2cXfer};

    fn ui() -> Ui {
        Ui::new(Source::Demo { t: 0.0 })
    }

    fn xfer() -> (Vec<f32>, Vec<f32>, f64) {
        let rate = 100e3;
        let dt = 0.25e-6;
        let (sda, scl) = i2c_logic(
            &[
                I2cXfer {
                    addr: 0x50,
                    read: false,
                    data: vec![0x00, 0x12],
                    repeated: false,
                    stop: false,
                },
                I2cXfer {
                    addr: 0x50,
                    read: true,
                    data: vec![0xab, 0xcd],
                    repeated: true,
                    stop: true,
                },
            ],
            rate,
            0.0,
            3.3,
        );
        let n = 2400;
        (
            sample(|t| scl.v(t), 0.0, dt, n),
            sample(|t| sda.v(t), 0.0, dt, n),
            dt,
        )
    }

    fn put_channels(u: &mut Ui, scl: Vec<f32>, sda: Vec<f32>, dt: f64) {
        let mut ch: [Vec<f32>; NCH] = std::array::from_fn(|_| Vec::new());
        ch[0] = scl;
        ch[1] = sda;
        u.last[0] = Some(Record {
            t0: 0.0,
            dt,
            ch,
            triggered: true,
            partial: false,
            raw: None,
            la: None,
        });
        u.m.bus.on = true;
        u.m.bus.clk = WaveId::Ch(0);
        u.m.bus.data = WaveId::Ch(1);
        u.m.bus.clk_thr = 1.65;
        u.m.bus.data_thr = 1.65;
        u.m.delay = 300e-6;
    }

    #[test]
    fn words_are_hex_and_the_read_write_bit_is_optional() {
        let (scl, sda, dt) = xfer();
        let mut u = ui();
        put_channels(&mut u, scl, sda, dt);
        assert_eq!(u.bus_words(), ["50", "00", "12", "50", "AB", "CD"]);
        u.m.bus.addr_rw = true;
        assert_eq!(u.bus_words(), ["A0", "00", "12", "A1", "AB", "CD"]);
        u.m.bus.clk_thr = 5.0;
        assert!(
            u.bus_words().is_empty(),
            "a clock threshold above the swing drops the words"
        );
        u.m.bus.on = false;
        u.m.bus.clk_thr = 1.65;
        assert!(u.bus_words().is_empty());
    }

    #[test]
    fn a_reference_keeps_its_own_threshold_and_sample_interval() {
        let (scl, sda, dt) = xfer();
        let mut u = ui();
        let mut ch: [Vec<f32>; NCH] = std::array::from_fn(|_| Vec::new());
        ch[0] = scl;
        u.last[0] = Some(Record {
            t0: 0.0,
            dt,
            ch,
            triggered: true,
            partial: false,
            raw: None,
            la: None,
        });
        let slow: Vec<f32> = sda.iter().step_by(2).copied().collect();
        u.refs[2] = Some(super::super::RefWave {
            src: 1,
            color: 2,
            label: String::new(),
            t0: 0.0,
            dt: dt * 2.0,
            v: slow,
            vpd: 2.0,
            offset: 0.0,
            on: true,
        });
        u.m.bus.on = true;
        u.m.bus.clk = WaveId::Ch(0);
        u.m.bus.data = WaveId::Ref(2);
        u.m.bus.clk_thr = 1.65;
        u.m.bus.data_thr = 1.2;
        u.m.delay = 300e-6;
        assert_eq!(u.bus_words(), ["50", "00", "12", "50", "AB", "CD"]);
        u.m.bus.data_thr = 4.0;
        assert!(u.bus_words().is_empty());
    }

    #[test]
    fn the_menu_is_one_i2c_bus() {
        let mut u = ui();
        assert!(u
            .main_groups()
            .iter()
            .any(|(_, items)| items.contains(&Item::Open(MenuId::Bus))));
        assert_eq!(u.menu_title(MenuId::Bus), "Bus · I2C");
        let items = u.bus_items();
        for want in [
            Item::BusOn,
            Item::BusLabel,
            Item::BusPos,
            Item::BusClk,
            Item::BusClkThr,
            Item::BusData,
            Item::BusDataThr,
            Item::BusRw,
        ] {
            assert!(items.contains(&want));
        }
        let text: String = items
            .iter()
            .map(|&it| {
                format!(
                    "{} {}",
                    Ui::bus_label(it).unwrap_or(""),
                    u.bus_value(it).unwrap_or_default()
                )
            })
            .collect();
        let low = text.to_ascii_lowercase();
        for banned in ["spi", "uart", "can", "rigol", "tek", "keysight", "siglent"] {
            assert!(!low.contains(banned), "{text}");
        }
        assert_eq!(u.bus_value(Item::BusRw), Some("Left out".into()));
        u.bus_adjust(Item::BusRw, 1);
        assert!(u.m.bus.addr_rw);
        u.bus_adjust(Item::BusPos, 5);
        assert!((u.m.bus.pos - -2.5).abs() < 1e-4);
        u.select_option(Item::BusClk, "R4");
        let ch = format!("CH{}", n_ch());
        u.select_option(Item::BusData, &ch);
        assert_eq!(
            (u.m.bus.clk, u.m.bus.data),
            (WaveId::Ref(3), WaveId::Ch(n_ch() - 1))
        );
        u.m.bus.label.clear();
        u.activate(Item::BusLabel);
        u.osk_type('S');
        u.osk_ok();
        assert_eq!(u.m.bus.label, "S");
        assert_eq!(u.m.bus.shown_label(), "S");
        u.m.bus.label.clear();
        assert_eq!(u.m.bus.shown_label(), "I2C");
    }

    #[test]
    fn the_plot_draws_the_bus_and_the_setup_keeps_it() {
        let (scl, sda, dt) = xfer();
        let mut u = ui();
        put_channels(&mut u, scl, sda, dt);
        u.m.bus.label = "Sense".into();
        u.m.bus.pos = -2.4;
        let mut off = Canvas::new(super::super::W, super::super::H);
        u.m.bus.on = false;
        u.render(&mut off);
        assert_eq!(count(&off, BUS), 0);
        u.m.bus.on = true;
        let mut on = Canvas::new(super::super::W, super::super::H);
        u.render(&mut on);
        assert!(
            count(&on, BUS) > 40,
            "the bus line and its label sit on the plot"
        );
        assert_eq!(u.bus_words(), ["50", "00", "12", "50", "AB", "CD"]);
        let back = crate::store::parse_setup(&crate::store::setup_text(&u.m)).unwrap();
        assert_eq!(back.bus, u.m.bus);
        assert!(crate::store::parse_setup("bus_kind = Spi\n").is_err());
        assert_eq!(bus_label("ab\ncd"), "abcd");
        assert_eq!(bus_label("0123456789EXTRA"), "0123456789");
    }

    fn count(cv: &Canvas, c: Rgb) -> usize {
        cv.px.iter().filter(|&&p| p == c).count()
    }
}
