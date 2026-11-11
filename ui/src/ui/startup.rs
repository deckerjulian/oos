// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Start-up screen: while the acquisition hardware starts, the OOS logo (or the own power-on picture) with the
//! progress of the start-up as a checklist, which is also the start-up self-test (`oos_hal::bootstatus`: rcS and
//! oos-fpgad write the file, `main.rs` reads it every 250 ms). It goes away 2 s after a clean start (5 s with
//! warnings); after a failed start it stays until a tap or a key. The last result is in System > About.

use super::*;
use oos_hal::bootstatus::{State as St, View};

/// Seconds the screen stays after the start-up completed.
const HOLD_OK: f64 = 2.0;
const HOLD_WARN: f64 = 5.0;
/// Without a new line for this long (a hung mount or driver load) a tap leaves the screen.
const STALL_S: f64 = 60.0;

pub struct Screen {
    pub view: View,
    /// uptime at the last update (the running stage's clock)
    pub uptime: f64,
    /// UI time when the start-up finished
    done_at: Option<f64>,
    dismissed: bool,
    /// the own power-on picture as the background (W x H pixels)
    pub splash: Option<Vec<u32>>,
    /// "OOS 0.1.4 · Build ...", read once: which software is starting
    version: String,
}

/// The version of the software and, in an image, its build.
fn version_line(build: Option<&str>) -> String {
    let v = format!("OOS {}", env!("CARGO_PKG_VERSION"));
    match build.map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => format!("{}  \u{b7}  Build {}", v, oos_hal::update::short(b)),
        None => v,
    }
}

impl Ui {
    /// Shows the start-up screen until the file says the start-up is complete.
    pub fn start_up_begin(&mut self, splash: Option<Vec<u32>>) {
        let version = version_line(std::fs::read_to_string("/etc/oos-build").ok().as_deref());
        self.start_up = Some(Screen { view: View::default(), uptime: 0.0, done_at: None, dismissed: false, splash, version });
    }

    /// The start-up file's text and the current uptime. A file that is complete at the first update (oos-ui restarted
    /// later in the boot) shows no screen.
    pub fn start_up_update(&mut self, text: &str, uptime: f64) {
        let now = self.now;
        let Some(s) = self.start_up.as_mut() else { return };
        let first = s.view == View::default() && s.uptime == 0.0;
        s.view = oos_hal::bootstatus::parse(text);
        s.uptime = uptime;
        if s.view.finished.is_some() && s.done_at.is_none() {
            s.done_at = Some(now);
            if first && s.view.finished == Some(Ok(())) {
                s.dismissed = true;
            }
        }
        self.start_up_last = Some(s.view.clone());
    }

    /// The screen is up (the scope screen and its input wait).
    pub fn start_up_active(&self) -> bool {
        let Some(s) = &self.start_up else { return false };
        if s.dismissed {
            return false;
        }
        match (&s.view.finished, s.done_at) {
            (Some(Ok(())), Some(t)) => self.now - t < if s.view.warnings().next().is_some() { HOLD_WARN } else { HOLD_OK },
            _ => true,
        }
    }

    /// No progress line for `STALL_S` seconds: the start-up probably hangs.
    fn start_up_stalled(s: &Screen) -> bool {
        s.view.finished.is_none() && s.uptime > 0.0 && s.uptime - s.view.t.max(s.view.stages.first().map_or(0.0, |x| x.t0)) > STALL_S
    }

    /// Input while the screen is up: the power key and its dialog work, a finished (or stalled) start-up closes on any
    /// tap or key.
    pub(super) fn start_up_input(&mut self, ev: &Ev) -> bool {
        if !self.start_up_active() || self.power_dlg {
            return false;
        }
        if let Ev::Key(k) = ev {
            if *k == key::POWER {
                return false;
            }
        }
        let pressed = match ev {
            Ev::Key(k) => *k != 0,
            Ev::Touch { phase, .. } => *phase == Phase::Up,
        };
        if pressed {
            if let Some(s) = self.start_up.as_mut().filter(|s| s.view.finished.is_some() || Self::start_up_stalled(s)) {
                s.dismissed = true;
            }
        }
        true
    }

    /// One line for About: how the last start-up went.
    pub(super) fn start_up_summary(&self) -> Option<String> {
        let v = self.start_up_last.as_ref()?;
        let warn: Vec<&str> = v.warnings().map(|s| s.label.as_str()).collect();
        Some(match &v.finished {
            None => format!("Start-up running ({:.1} s)", v.t),
            Some(Err(_)) => format!("Start-up stopped at {} ({:.1} s)", v.failed().map_or("?", |s| s.label.as_str()), v.t),
            Some(Ok(())) if warn.is_empty() => format!("Start-up {:.1} s, self-test passed", v.t),
            Some(Ok(())) => format!("Start-up {:.1} s, warnings: {}", v.t, warn.join(", ")),
        })
    }

    pub(super) fn draw_start_up(&self, cv: &mut Canvas) {
        let Some(s) = &self.start_up else { return };
        let f = self.fonts();
        cv.no_clip();
        match &s.splash {
            Some(px) if px.len() == cv.px.len() => cv.px.copy_from_slice(px),
            _ => {
                cv.px.copy_from_slice(logo());
                spaced(cv, &f.s, 182, "OPEN  OSCILLOSCOPE  SOFTWARE", 3, DIM);
            }
        }
        let v = &s.view;
        let rows = v.stages.len().max(1) as i32;
        let row_h = 26;
        let failed = matches!(v.finished, Some(Err(_)));
        let card = Rect::new(W / 2 - 330, 212, 660, 16 + rows * row_h + 12 + 24 + if failed { 34 } else { 14 });
        cv.round(card, 10, DLG_BG, 235);
        let mut y = card.y + 16;
        let blink = (self.now * 2.0) as i64 % 2 == 0;
        for st in &v.stages {
            let col = match st.state {
                St::Ok => GREEN,
                St::Warn => AMBER,
                St::Fail => RED,
                St::Run => if blink { ACCENT } else { DIM },
            };
            let (cx, cy) = (card.x + 30, y + row_h / 2);
            cv.circle(cx, cy, 8, col);
            // the marks drawn (the UI font has no check mark): tick, "!", cross
            for d in 0..2 {
                match st.state {
                    St::Ok => {
                        cv.line(cx - 5, cy + d - 1, cx - 2, cy + d + 2, DARK_TEXT);
                        cv.line(cx - 2, cy + d + 2, cx + 4, cy + d - 4, DARK_TEXT);
                    }
                    St::Warn => {
                        cv.line(cx + d, cy - 5, cx + d, cy + 1, DARK_TEXT);
                        cv.line(cx + d, cy + 4, cx + d, cy + 5, DARK_TEXT);
                    }
                    St::Fail => {
                        cv.line(cx - 4 + d, cy - 4, cx + 4 + d, cy + 4, DARK_TEXT);
                        cv.line(cx + 4 + d, cy - 4, cx - 4 + d, cy + 4, DARK_TEXT);
                    }
                    St::Run => {}
                }
            }
            cv.text(&f.m, card.x + 52, y + 18, &st.label, if st.state == St::Run { TEXT } else { LABEL });
            let t = match st.t1 {
                Some(t1) if t1 - st.t0 < 0.05 => format!("at {:.1} s", t1),
                Some(t1) => format!("{:.1} s", t1 - st.t0),
                None => format!("{:.1} s", (s.uptime - st.t0).max(0.0)),
            };
            cv.text_in(&f.s, Rect::new(card.x + card.w - 90, y, 74, row_h), 2, &t, DIM);
            if !st.detail.is_empty() {
                let x0 = card.x + 300;
                let room = card.x + card.w - 100 - x0;
                cv.text(&f.s, x0, y + 17, &fit_text(&f.s, &st.detail, room), if st.state == St::Warn { AMBER } else if st.state == St::Fail { RED } else { DIM });
            }
            y += row_h;
        }
        y += 12;
        let (line, col) = match &v.finished {
            None => (format!("Starting the acquisition hardware ...  {:.1} s", s.uptime), TEXT),
            Some(Ok(())) => match v.warnings().count() {
                0 => (format!("Ready after {:.1} s  \u{b7}  self-test passed", v.t), GREEN),
                n => (format!("Ready after {:.1} s  \u{b7}  {} warning{}", v.t, n, if n == 1 { "" } else { "s" }), AMBER),
            },
            Some(Err(_)) => (format!("Start-up stopped: {}", v.failed().map_or("", |s| s.label.as_str())), RED),
        };
        cv.text_in(&f.mb, Rect::new(card.x, y, card.w, 24), 1, &line, col);
        // the version along the bottom edge, under the messages that ask for a tap; also over an own picture
        cv.text_in(&f.s, Rect::new(0, H - 20, W, 16), 1, &s.version, DIM);
        if let Some(Err(d)) = &v.finished {
            cv.text_in(&f.s, Rect::new(card.x, y + 24, card.w, 20), 1, &fit_text(&f.s, d, card.w - 40), DIM);
            cv.text_in(&f.s, Rect::new(0, H - 40, W, 20), 1, "Tap to continue without acquisition", DIM);
        } else if Self::start_up_stalled(s) {
            let quiet = s.uptime - v.t;
            cv.text_in(&f.s, Rect::new(0, H - 40, W, 20), 1, &format!("No progress for {:.0} s (see /tmp/oos-cold.log) \u{b7} tap to continue without acquisition", quiet), AMBER);
        }
    }
}

/// Text centred on the screen with `gap` extra pixels between the letters (baseline `y`).
fn spaced(cv: &mut Canvas, f: &crate::font::Font, y: i32, s: &str, gap: i32, c: Rgb) {
    let w: i32 = s.chars().map(|ch| f.width(&ch.to_string()) + gap).sum::<i32>() - gap;
    let mut x = (W - w) / 2;
    for ch in s.chars() {
        let t = ch.to_string();
        cv.text(f, x, y, &t, c);
        x += f.width(&t) + gap;
    }
}

/// Anti-aliased coverage of strokes over a rectangle of the screen (the logo is geometry, not font glyphs).
struct Layer {
    r: Rect,
    a: Vec<f32>,
}

impl Layer {
    fn new(r: Rect) -> Layer {
        Layer { r, a: vec![0.0; (r.w * r.h) as usize] }
    }

    /// A polyline `hw` pixels either side of the centre line; `glow`: soft falloff instead of a hard edge.
    fn stroke(&mut self, pts: &[(f32, f32)], hw: f32, glow: bool) {
        for s in pts.windows(2) {
            let ((ax, ay), (bx, by)) = (s[0], s[1]);
            let (dx, dy) = (bx - ax, by - ay);
            let len2 = (dx * dx + dy * dy).max(1e-6);
            let m = hw + 1.0;
            let (x0, x1) = ((ax.min(bx) - m).floor() as i32, (ax.max(bx) + m).ceil() as i32);
            let (y0, y1) = ((ay.min(by) - m).floor() as i32, (ay.max(by) + m).ceil() as i32);
            for y in y0.max(self.r.y)..y1.min(self.r.y + self.r.h) {
                for x in x0.max(self.r.x)..x1.min(self.r.x + self.r.w) {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
                    let d = ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt();
                    let c = if glow { (1.0 - d / hw).max(0.0).powi(2) } else { (hw + 0.5 - d).clamp(0.0, 1.0) };
                    let i = ((y - self.r.y) * self.r.w + (x - self.r.x)) as usize;
                    self.a[i] = self.a[i].max(c);
                }
            }
        }
    }

    fn paint(&self, cv: &mut Canvas, c: Rgb, opacity: f32) {
        for y in 0..self.r.h {
            for x in 0..self.r.w {
                let a = self.a[(y * self.r.w + x) as usize] * opacity;
                if a > 0.0 {
                    cv.pixel(self.r.x + x, self.r.y + y, c, (a * 255.0).round() as u32);
                }
            }
        }
    }
}

/// Points of an elliptic arc (angles in degrees, y down: 0 = right, 90 = down).
fn arc(cx: f32, cy: f32, rx: f32, ry: f32, a0: f32, a1: f32) -> Vec<(f32, f32)> {
    let n = ((a1 - a0).abs() / 4.0).ceil().max(2.0) as usize;
    (0..=n).map(|k| (a0 + (a1 - a0) * k as f32 / n as f32).to_radians()).map(|a| (cx + rx * a.cos(), cy + ry * a.sin())).collect()
}

/// Intersection of the segments a-b and c-d: (point, position along a-b in 0..1).
fn cross(a: (f32, f32), b: (f32, f32), c: (f32, f32), d: (f32, f32)) -> Option<((f32, f32), f32)> {
    let (rx, ry, sx, sy) = (b.0 - a.0, b.1 - a.1, d.0 - c.0, d.1 - c.1);
    let den = rx * sy - ry * sx;
    if den.abs() < 1e-6 {
        return None;
    }
    let t = ((c.0 - a.0) * sy - (c.1 - a.1) * sx) / den;
    let u = ((c.0 - a.0) * ry - (c.1 - a.1) * rx) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| ((a.0 + t * rx, a.1 + t * ry), t))
}

/// Strokes the letters (polylines) so that they weave: at every crossing one letter passes over the other, which
/// gets a gap around it. Between the first two letters the upper crossing goes to the first one; along any later
/// letter the crossings alternate over / under, starting over.
fn interlace(cv: &mut Canvas, area: Rect, paths: &[Vec<(f32, f32)>], hw: f32, c: Rgb) {
    let mut layers: Vec<Layer> = paths
        .iter()
        .map(|p| {
            let mut l = Layer::new(area);
            l.stroke(p, hw, false);
            l
        })
        .collect();
    for i in 0..paths.len() {
        for j in i + 1..paths.len() {
            // crossings as (point, index along path j + fraction), in the order of path j
            let mut xs: Vec<((f32, f32), f32)> = Vec::new();
            for (kj, sj) in paths[j].windows(2).enumerate() {
                for si in paths[i].windows(2) {
                    if let Some((p, t)) = cross(sj[0], sj[1], si[0], si[1]) {
                        if !xs.iter().any(|(q, _)| (q.0 - p.0).hypot(q.1 - p.1) < 3.0) {
                            xs.push((p, kj as f32 + t));
                        }
                    }
                }
            }
            xs.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
            let upper = xs.iter().map(|x| x.0 .1).fold(f32::INFINITY, f32::min);
            for (n, (p, _)) in xs.iter().enumerate() {
                let j_over = if i == 0 && j == 1 { p.1 > upper } else { n % 2 == 0 };
                let (over, under) = if j_over { (j, i) } else { (i, j) };
                // the over letter's run near the crossing, walked from its nearest point both ways
                let path = &paths[over];
                let near = (0..path.len()).min_by(|&a, &b| {
                    let d = |k: usize| (path[k].0 - p.0).hypot(path[k].1 - p.1);
                    d(a).partial_cmp(&d(b)).unwrap()
                });
                let Some(k0) = near else { continue };
                let reach = 3.0 * hw;
                let inside = |k: usize| (path[k].0 - p.0).hypot(path[k].1 - p.1) < reach;
                let (mut a, mut b) = (k0, k0);
                while a > 0 && inside(a - 1) {
                    a -= 1;
                }
                while b + 1 < path.len() && inside(b + 1) {
                    b += 1;
                }
                let mut halo = Layer::new(area);
                halo.stroke(&path[a..=b], hw + 3.5, false);
                for (u, h) in layers[under].a.iter_mut().zip(&halo.a) {
                    *u *= 1.0 - h;
                }
            }
        }
    }
    let mut all = Layer::new(area);
    for l in &layers {
        for (o, a) in all.a.iter_mut().zip(&l.a) {
            *o = o.max(*a);
        }
    }
    all.paint(cv, c, 1.0);
}

/// The logo on the background, drawn once: a small scope screen with graticule and a glowing sine, next to the
/// geometric "OOS" (two linked rings and an S of two arcs).
fn logo() -> &'static [u32] {
    static PX: std::sync::OnceLock<Vec<u32>> = std::sync::OnceLock::new();
    PX.get_or_init(|| {
        let mut cv = Canvas::new(W, H);
        cv.clear(BG);
        let (mark, gap, word_w, top) = (112, 30, 159, 44);
        let x0 = (W - (mark + gap + word_w)) / 2;
        // the screen: bezel, graticule (4 x 4), centre axes
        let m = Rect::new(x0, top, mark, mark);
        cv.round(m, 20, BAR, 255);
        cv.round_outline(m, 20, LINE);
        let g = m.inset(14);
        for k in 1..4 {
            let (gx, gy) = (g.x + g.w * k / 4, g.y + g.h * k / 4);
            let col = if k == 2 { GRID_AXIS } else { GRID_LINE };
            cv.line(gx, g.y, gx, g.y + g.h, col);
            cv.line(g.x, gy, g.x + g.w, gy, col);
        }
        // the trace: one and a quarter periods, glow and core
        let tr: Vec<(f32, f32)> = (0..=96)
            .map(|i| {
                let t = i as f32 / 96.0;
                (g.x as f32 + t * g.w as f32, (g.y + g.h / 2) as f32 - 0.34 * g.h as f32 * (std::f32::consts::TAU * 1.25 * t).sin())
            })
            .collect();
        let mut glow = Layer::new(m);
        glow.stroke(&tr, 7.0, true);
        glow.paint(&mut cv, ACCENT, 0.45);
        let mut core = Layer::new(m);
        core.stroke(&tr, 1.4, false);
        core.paint(&mut cv, ACCENT, 1.0);
        // the word: letters 64 px high, 10 px strokes, centred on the screen's middle, interlaced (`interlace`)
        let (h, hw) = (64.0f32, 5.0f32);
        let cy = (top + mark / 2) as f32;
        let wx = (x0 + mark + gap) as f32;
        let r = h / 2.0 - hw;
        // the two O link like chain rings (upper crossing: first O over, lower: second O over); the S stands free
        let o1 = arc(wx + 32.0, cy, r, r, 90.0, 450.0);
        let o2 = arc(wx + 72.0, cy, r, r, 90.0, 450.0);
        let (sx, sr, srx) = (wx + 131.0, h / 4.0 - hw / 2.0, 22.0);
        let mut s = arc(sx, cy - sr, srx, sr, -25.0, -270.0);
        s.extend(arc(sx, cy + sr, srx, sr, -90.0, 155.0));
        let area = Rect::new(x0 + mark + gap - 4, top, word_w + 8, mark);
        interlace(&mut cv, area, &[o1, o2, s], hw, TEXT);
        cv.px
    })
}

/// `s` cut with "..." to `room` pixels.
pub(super) fn fit_text(f: &crate::font::Font, s: &str, room: i32) -> String {
    if f.width(s) <= room {
        return s.to_string();
    }
    let mut t: String = s.to_string();
    while !t.is_empty() && f.width(&(t.clone() + "...")) > room {
        t.pop();
    }
    t + "..."
}

#[cfg(test)]
mod tests {
    use super::*;
    use oos_hal::bootstatus::line;

    fn ui() -> Ui {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.start_up_begin(None);
        u
    }

    #[test]
    fn screen_follows_the_start_up() {
        let mut u = ui();
        let mut text = line(3.9, "ok", "Linux", "6.12.115") + &line(4.0, "run", "FPGA image", "loading the image from flash");
        u.start_up_update(&text, 6.0);
        assert!(u.start_up_active());
        // input waits while it runs
        assert!(u.start_up_input(&Ev::Touch { id: 0, phase: Phase::Up, x: 10, y: 10 }));
        assert!(u.start_up_active());
        let mut cv = Canvas::new(W, H);
        u.draw_start_up(&mut cv);
        if let Ok(d) = std::env::var("OOS_SHOT_DIR") {
            let _ = std::fs::write(format!("{}/start-up-running.png", d), crate::store::png_of(&cv));
        }
        text += &(line(14.2, "ok", "FPGA image", "") + &line(14.3, "warn", "Calibration data", "missing: cal_afg.hex") + &line(18.0, "end", "", ""));
        u.start_up_update(&text, 18.1);
        assert!(u.start_up_active());
        u.draw_start_up(&mut cv);
        if let Ok(d) = std::env::var("OOS_SHOT_DIR") {
            let _ = std::fs::write(format!("{}/start-up-done.png", d), crate::store::png_of(&cv));
        }
        assert_eq!(u.start_up_summary().unwrap(), "Start-up 18.0 s, warnings: Calibration data");
        // with a warning it stays 5 s, then the scope screen
        u.now += 4.0;
        assert!(u.start_up_active());
        u.now += 1.5;
        assert!(!u.start_up_active());
        assert!(!u.start_up_input(&Ev::Touch { id: 0, phase: Phase::Up, x: 10, y: 10 }));
    }

    #[test]
    fn version_line_names_the_release_and_the_build() {
        let v = env!("CARGO_PKG_VERSION");
        assert_eq!(version_line(None), format!("OOS {}", v));
        assert_eq!(version_line(Some(" \n")), format!("OOS {}", v));
        assert_eq!(version_line(Some("20261110-121314 abc\n")), format!("OOS {}  \u{b7}  Build 20261110-121314", v));
    }

    #[test]
    fn failed_start_up_stays_until_a_tap() {
        let mut u = ui();
        let text = line(4.0, "run", "FPGA image", "") + &line(14.0, "fail", "FPGA image", "PCIe / BAR1: FPGA did not appear on PCIe") + &line(14.1, "abort", "", "see /tmp/oos-cold.log");
        u.start_up_update(&text, 14.2);
        u.now += 60.0;
        assert!(u.start_up_active());
        let mut cv = Canvas::new(W, H);
        u.draw_start_up(&mut cv);
        if let Ok(d) = std::env::var("OOS_SHOT_DIR") {
            let _ = std::fs::write(format!("{}/start-up-failed.png", d), crate::store::png_of(&cv));
        }
        assert!(u.start_up_input(&Ev::Touch { id: 0, phase: Phase::Up, x: 10, y: 10 }));
        assert!(!u.start_up_active());
        assert_eq!(u.start_up_summary().unwrap(), "Start-up stopped at FPGA image (14.1 s)");
    }

    #[test]
    fn stalled_start_up_can_be_left_and_power_works() {
        let mut u = ui();
        let text = line(4.0, "run", "FPGA image", "") + &line(14.0, "run", "Sample clock (PLL lock)", "");
        u.start_up_update(&text, 30.0);
        assert!(u.start_up_input(&Ev::Touch { id: 0, phase: Phase::Up, x: 10, y: 10 }) && u.start_up_active());
        // the power dialog is up: its input goes to the scope screen's handling
        u.power_dlg = true;
        assert!(!u.start_up_input(&Ev::Touch { id: 0, phase: Phase::Up, x: 10, y: 10 }));
        u.power_dlg = false;
        // 61 s without a line: a tap leaves
        u.start_up_update(&text, 75.5);
        let mut cv = Canvas::new(W, H);
        u.draw_start_up(&mut cv);
        assert!(u.start_up_input(&Ev::Touch { id: 0, phase: Phase::Up, x: 10, y: 10 }));
        assert!(!u.start_up_active());
    }

    #[test]
    fn complete_at_the_first_look_shows_nothing() {
        // oos-ui restarted after a crash later in the boot
        let mut u = ui();
        u.start_up_update(&(line(4.0, "run", "FPGA image", "") + &line(18.0, "end", "", "")), 300.0);
        assert!(!u.start_up_active());
    }
}
