// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Software renderer: XRGB8888 canvas with rectangles, anti-aliased lines, alpha blending and text.

use crate::font::Font;

pub type Rgb = u32;

pub const fn rgb(r: u32, g: u32, b: u32) -> Rgb {
    (r << 16) | (g << 8) | b
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
    pub fn inset(&self, d: i32) -> Rect {
        Rect::new(self.x + d, self.y + d, self.w - 2 * d, self.h - 2 * d)
    }
}

pub struct Canvas {
    pub px: Vec<u32>,
    pub w: i32,
    pub h: i32,
    clip: (i32, i32, i32, i32), // x0, y0, x1, y1 (exclusive)
}

#[inline]
fn blend1(d: u32, s: u32, a: u32) -> u32 {
    // a: 0..256
    let rb = d & 0xff00ff;
    let g = d & 0x00ff00;
    let srb = s & 0xff00ff;
    let sg = s & 0x00ff00;
    let rb = (rb.wrapping_add((srb.wrapping_sub(rb)).wrapping_mul(a) >> 8)) & 0xff00ff;
    let g = (g.wrapping_add((sg.wrapping_sub(g)).wrapping_mul(a) >> 8)) & 0x00ff00;
    rb | g
}

pub fn mix(a: Rgb, b: Rgb, t: u32) -> Rgb {
    blend1(a, b, t + (t >> 7))
}

pub fn scale(c: Rgb, t: u32) -> Rgb {
    let r = ((c >> 16) & 255) * t / 255;
    let g = ((c >> 8) & 255) * t / 255;
    let b = (c & 255) * t / 255;
    rgb(r, g, b)
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Canvas {
        Canvas { px: vec![0; (w * h) as usize], w, h, clip: (0, 0, w, h) }
    }

    pub fn set_clip(&mut self, r: Rect) {
        self.clip = (r.x.max(0), r.y.max(0), (r.x + r.w).min(self.w), (r.y + r.h).min(self.h));
    }
    pub fn no_clip(&mut self) {
        self.clip = (0, 0, self.w, self.h);
    }

    fn clipr(&self, r: Rect) -> Option<(i32, i32, i32, i32)> {
        let x0 = r.x.max(self.clip.0);
        let y0 = r.y.max(self.clip.1);
        let x1 = (r.x + r.w).min(self.clip.2);
        let y1 = (r.y + r.h).min(self.clip.3);
        if x1 <= x0 || y1 <= y0 {
            None
        } else {
            Some((x0, y0, x1, y1))
        }
    }

    pub fn clear(&mut self, c: Rgb) {
        self.px.iter_mut().for_each(|p| *p = c);
    }

    pub fn fill(&mut self, r: Rect, c: Rgb) {
        if let Some((x0, y0, x1, y1)) = self.clipr(r) {
            for y in y0..y1 {
                let o = (y * self.w) as usize;
                self.px[o + x0 as usize..o + x1 as usize].iter_mut().for_each(|p| *p = c);
            }
        }
    }

    pub fn blend(&mut self, r: Rect, c: Rgb, a: u32) {
        if a >= 255 {
            return self.fill(r, c);
        }
        if a == 0 {
            return;
        }
        let a = a + (a >> 7);
        if let Some((x0, y0, x1, y1)) = self.clipr(r) {
            for y in y0..y1 {
                let o = (y * self.w) as usize;
                for p in &mut self.px[o + x0 as usize..o + x1 as usize] {
                    *p = blend1(*p, c, a);
                }
            }
        }
    }

    #[inline]
    pub fn pixel(&mut self, x: i32, y: i32, c: Rgb, a: u32) {
        if a == 0 || x < self.clip.0 || y < self.clip.1 || x >= self.clip.2 || y >= self.clip.3 {
            return;
        }
        let p = &mut self.px[(y * self.w + x) as usize];
        *p = if a >= 255 { c } else { blend1(*p, c, a + (a >> 7)) };
    }

    pub fn hline(&mut self, x0: i32, x1: i32, y: i32, c: Rgb) {
        if x1 >= x0 {
            self.fill(Rect::new(x0, y, x1 - x0 + 1, 1), c)
        }
    }
    pub fn vline(&mut self, x: i32, y0: i32, y1: i32, c: Rgb) {
        if y1 >= y0 {
            self.fill(Rect::new(x, y0, 1, y1 - y0 + 1), c)
        }
    }
    pub fn rect(&mut self, r: Rect, c: Rgb) {
        self.hline(r.x, r.x + r.w - 1, r.y, c);
        self.hline(r.x, r.x + r.w - 1, r.y + r.h - 1, c);
        self.vline(r.x, r.y, r.y + r.h - 1, c);
        self.vline(r.x + r.w - 1, r.y, r.y + r.h - 1, c);
    }

    fn corner_cov(px: i32, py: i32, r: i32) -> u32 {
        if r <= 0 {
            return 0;
        }
        let mut n = 0;
        for sy in 0..4 {
            for sx in 0..4 {
                let dx = px as f32 + (sx as f32 + 0.5) / 4.0 - r as f32;
                let dy = py as f32 + (sy as f32 + 0.5) / 4.0 - r as f32;
                if dx * dx + dy * dy <= (r * r) as f32 {
                    n += 1;
                }
            }
        }
        n * 255 / 16
    }

    /// Filled rounded rectangle with alpha.
    pub fn round(&mut self, r: Rect, rad: i32, c: Rgb, a: u32) {
        let rad = rad.min(r.h / 2).min(r.w / 2);
        if rad <= 0 {
            return self.blend(r, c, a);
        }
        self.blend(Rect::new(r.x, r.y + rad, r.w, r.h - 2 * rad), c, a);
        self.blend(Rect::new(r.x + rad, r.y, r.w - 2 * rad, rad), c, a);
        self.blend(Rect::new(r.x + rad, r.y + r.h - rad, r.w - 2 * rad, rad), c, a);
        for j in 0..rad {
            for i in 0..rad {
                let cv = Self::corner_cov(i, j, rad) * a / 255;
                self.pixel(r.x + i, r.y + j, c, cv);
                self.pixel(r.x + r.w - 1 - i, r.y + j, c, cv);
                self.pixel(r.x + i, r.y + r.h - 1 - j, c, cv);
                self.pixel(r.x + r.w - 1 - i, r.y + r.h - 1 - j, c, cv);
            }
        }
    }

    pub fn round_outline(&mut self, r: Rect, rad: i32, c: Rgb) {
        let rad = rad.min(r.h / 2).min(r.w / 2);
        self.hline(r.x + rad, r.x + r.w - 1 - rad, r.y, c);
        self.hline(r.x + rad, r.x + r.w - 1 - rad, r.y + r.h - 1, c);
        self.vline(r.x, r.y + rad, r.y + r.h - 1 - rad, c);
        self.vline(r.x + r.w - 1, r.y + rad, r.y + r.h - 1 - rad, c);
        for j in 0..rad {
            for i in 0..rad {
                let o = Self::corner_cov(i, j, rad);
                let n = Self::corner_cov(i - 1, j - 1, rad - 1);
                let cv = o.saturating_sub(n);
                self.pixel(r.x + i, r.y + j, c, cv);
                self.pixel(r.x + r.w - 1 - i, r.y + j, c, cv);
                self.pixel(r.x + i, r.y + r.h - 1 - j, c, cv);
                self.pixel(r.x + r.w - 1 - i, r.y + r.h - 1 - j, c, cv);
            }
        }
    }

    /// Anti-aliased line (Xiaolin Wu).
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgb) {
        let (mut x0, mut y0, mut x1, mut y1) = (x0, y0, x1, y1);
        let steep = (y1 - y0).abs() > (x1 - x0).abs();
        if steep {
            std::mem::swap(&mut x0, &mut y0);
            std::mem::swap(&mut x1, &mut y1);
        }
        if x0 > x1 {
            std::mem::swap(&mut x0, &mut x1);
            std::mem::swap(&mut y0, &mut y1);
        }
        let dx = x1 - x0;
        let dy = y1 - y0;
        let grad = if dx != 0 { (dy << 16) / dx } else { 1 << 16 };
        let mut yf = y0 << 16;
        for x in x0..=x1 {
            let yi = yf >> 16;
            let fr = ((yf >> 8) & 255) as u32;
            if steep {
                self.pixel(yi, x, c, 255 - fr);
                self.pixel(yi + 1, x, c, fr);
            } else {
                self.pixel(x, yi, c, 255 - fr);
                self.pixel(x, yi + 1, c, fr);
            }
            yf += grad;
        }
    }

    pub fn dashed_hline(&mut self, x0: i32, x1: i32, y: i32, on: i32, off: i32, c: Rgb) {
        let mut x = x0;
        while x <= x1 {
            self.hline(x, (x + on - 1).min(x1), y, c);
            x += on + off;
        }
    }
    pub fn dashed_vline(&mut self, x: i32, y0: i32, y1: i32, on: i32, off: i32, c: Rgb) {
        let mut y = y0;
        while y <= y1 {
            self.vline(x, y, (y + on - 1).min(y1), c);
            y += on + off;
        }
    }

    /// Filled triangle, 4x supersampled edges.
    pub fn triangle(&mut self, p: [(i32, i32); 3], c: Rgb) {
        let minx = p.iter().map(|q| q.0).min().unwrap();
        let maxx = p.iter().map(|q| q.0).max().unwrap();
        let miny = p.iter().map(|q| q.1).min().unwrap();
        let maxy = p.iter().map(|q| q.1).max().unwrap();
        let (x0, y0, x1, y1, x2, y2) = (p[0].0 as f32, p[0].1 as f32, p[1].0 as f32, p[1].1 as f32, p[2].0 as f32, p[2].1 as f32);
        let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
        if area == 0.0 {
            return;
        }
        for y in miny..=maxy {
            for x in minx..=maxx {
                let mut n = 0;
                for s in 0..4 {
                    let px = x as f32 + if s & 1 != 0 { 0.75 } else { 0.25 };
                    let py = y as f32 + if s & 2 != 0 { 0.75 } else { 0.25 };
                    let w0 = (x1 - x0) * (py - y0) - (y1 - y0) * (px - x0);
                    let w1 = (x2 - x1) * (py - y1) - (y2 - y1) * (px - x1);
                    let w2 = (x0 - x2) * (py - y2) - (y0 - y2) * (px - x2);
                    let inside = if area > 0.0 { w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 } else { w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0 };
                    if inside {
                        n += 1;
                    }
                }
                self.pixel(x, y, c, n * 255 / 4);
            }
        }
    }

    pub fn circle(&mut self, cx: i32, cy: i32, r: i32, c: Rgb) {
        self.round(Rect::new(cx - r, cy - r, 2 * r, 2 * r), r, c, 255);
    }

    // ---- text ----

    /// Draws `s` with the baseline at `y`; returns the pen x after the text.
    pub fn text(&mut self, f: &Font, x: i32, y: i32, s: &str, c: Rgb) -> i32 {
        let mut x = x;
        for ch in s.chars() {
            let g = f.glyph(ch);
            let a = &f.alpha[g.off..g.off + (g.w * g.h) as usize];
            let gx = x + g.x0;
            let gy = y + g.y0;
            for j in 0..g.h {
                let yy = gy + j;
                if yy < self.clip.1 || yy >= self.clip.3 {
                    continue;
                }
                let row = (yy * self.w) as usize;
                for i in 0..g.w {
                    let xx = gx + i;
                    let al = a[(j * g.w + i) as usize] as u32;
                    if al == 0 || xx < self.clip.0 || xx >= self.clip.2 {
                        continue;
                    }
                    let p = &mut self.px[row + xx as usize];
                    *p = if al == 255 { c } else { blend1(*p, c, al + (al >> 7)) };
                }
            }
            x += g.adv;
        }
        x
    }

    /// Text vertically centred in `r`; align 0 = left, 1 = centre, 2 = right.
    pub fn text_in(&mut self, f: &Font, r: Rect, align: i32, s: &str, c: Rgb) {
        let tw = f.width(s);
        let tx = match align {
            0 => r.x,
            1 => r.x + (r.w - tw) / 2,
            _ => r.x + r.w - tw,
        };
        let cap = f.size * 72 / 100;
        self.text(f, tx, r.y + (r.h + cap) / 2, s, c);
    }
}

/// A picture scaled into another size with its aspect ratio kept (centred, black bars stay untouched): nearest
/// source pixel per destination pixel, the maps computed once (HDMI mirror of the panel picture).
pub struct Fit {
    pub src: (i32, i32),
    /// destination rectangle
    pub x0: usize,
    pub y0: usize,
    xs: Vec<u32>,
    ys: Vec<u32>,
}

impl Fit {
    pub fn new(sw: i32, sh: i32, dw: i32, dh: i32) -> Fit {
        let (sw, sh, dw, dh) = (sw.max(1) as i64, sh.max(1) as i64, dw.max(1) as i64, dh.max(1) as i64);
        // the larger of the two fits: width-limited or height-limited
        let (w, h) = if dw * sh <= dh * sw { (dw, (dw * sh / sw).max(1)) } else { ((dh * sw / sh).max(1), dh) };
        let xs = (0..w).map(|x| ((2 * x + 1) * sw / (2 * w)) as u32).collect();
        let ys = (0..h).map(|y| ((2 * y + 1) * sh / (2 * h)) as u32).collect();
        Fit { src: (sw as i32, sh as i32), x0: ((dw - w) / 2) as usize, y0: ((dh - h) / 2) as usize, xs, ys }
    }

    #[cfg(test)]
    pub fn size(&self) -> (usize, usize) {
        (self.xs.len(), self.ys.len())
    }

    /// Scales `src` (src.0 x src.1 pixels) into `dst` (rows of `stride` pixels). A repeated source row is copied.
    pub fn scale(&self, src: &[u32], dst: &mut [u32], stride: usize) {
        let sw = self.src.0 as usize;
        let w = self.xs.len();
        let mut last: Option<(u32, usize)> = None;
        for (y, &sy) in self.ys.iter().enumerate() {
            let at = (self.y0 + y) * stride + self.x0;
            if at + w > dst.len() {
                break;
            }
            match last {
                Some((ly, from)) if ly == sy => dst.copy_within(from..from + w, at),
                _ => {
                    let row = &src[sy as usize * sw..sy as usize * sw + sw];
                    for (d, &sx) in dst[at..at + w].iter_mut().zip(&self.xs) {
                        *d = row[sx as usize];
                    }
                }
            }
            last = Some((sy, at));
        }
    }
}

/// The mode for a monitor from (width, height, refresh Hz, preferred, interlaced): never an interlaced one; the
/// preferred one if it is at most
/// `max` (the mirror is scaled by the CPU, so a 4K mode would cost four times the work for the same picture), else
/// the largest one within `max`, at the refresh nearest 60 Hz.
pub fn pick_mode(modes: &[(u16, u16, u32, bool, bool)], max: (u16, u16)) -> Option<usize> {
    let fits = |&(w, h, _, _, il): &(u16, u16, u32, bool, bool)| w <= max.0 && h <= max.1 && !il;
    if let Some(i) = modes.iter().position(|m| m.3 && fits(m)) {
        return Some(i);
    }
    // the first of equal candidates (the kernel lists the higher clock first)
    let key = |i: usize| {
        let (w, h, r, _, _) = modes[i];
        (w as u32 * h as u32, std::cmp::Reverse((r as i64 - 60).unsigned_abs()))
    };
    (0..modes.len()).filter(|&i| fits(&modes[i])).fold(None, |b: Option<usize>, i| if b.is_some_and(|b| key(b) >= key(i)) { b } else { Some(i) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_modes() {
        let m = [(3840, 2160, 60, true, false), (1920, 1080, 50, false, false), (1920, 1080, 60, false, false), (1920, 1080, 60, false, true), (1280, 720, 60, false, false)];
        assert_eq!(pick_mode(&m, (1920, 1080)), Some(2));
        // 1080i60 ties with 1080p60 on size and refresh: never interlaced, the first of equal ones
        let m = [(3840, 2160, 60, true, false), (1920, 1080, 60, false, true), (1920, 1080, 60, false, false)];
        assert_eq!(pick_mode(&m, (1920, 1080)), Some(2));
        let m = [(1680, 1050, 60, true, false), (1920, 1080, 60, false, false)];
        assert_eq!(pick_mode(&m, (1920, 1080)), Some(0));
        assert_eq!(pick_mode(&[(4096, 2160, 30, true, false)], (1920, 1080)), None);
    }

    #[test]
    fn fit_keeps_the_aspect_ratio() {
        // the 1024 x 600 panel on 1920 x 1080: height-limited, 1843 x 1080 with bars of 38 / 39 pixels
        let f = Fit::new(1024, 600, 1920, 1080);
        assert_eq!((f.size(), f.x0, f.y0), ((1843, 1080), 38, 0));
        // on 1280 x 1024: width-limited
        let f = Fit::new(1024, 600, 1280, 1024);
        assert_eq!((f.size(), f.x0, f.y0), ((1280, 750), 0, 137));
        // 2 x 2 -> 4 x 4: every source pixel twice in both directions, the first row copied
        let f = Fit::new(2, 2, 4, 4);
        let mut d = vec![0u32; 16];
        f.scale(&[1, 2, 3, 4], &mut d, 4);
        assert_eq!(d, [1, 1, 2, 2, 1, 1, 2, 2, 3, 3, 4, 4, 3, 3, 4, 4]);
        // bars stay as they are; a short destination is not overrun
        let f = Fit::new(2, 1, 4, 4);
        let mut d = vec![9u32; 16];
        f.scale(&[5, 6], &mut d, 4);
        assert_eq!(d, [9, 9, 9, 9, 5, 5, 6, 6, 5, 5, 6, 6, 9, 9, 9, 9]);
        f.scale(&[5, 6], &mut d[..6], 4);
    }
}
