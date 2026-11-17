// SPDX-License-Identifier: GPL-2.0-or-later
//! A small annotated figure for every measurement of the catalogue, in the style of the figures of a scope manual:
//! a stroke of the waveform and, over it in the accent colour, exactly what is measured (a level, an arrow between two
//! levels, a span between two edges, a shaded area). Our own drawing. Every `Cat` has its own case.
use super::meascatalog::Cat;
use crate::font::Fonts;
use crate::gfx::{Canvas, Rgb};
use super::Rect;

/// The standard pulse train: low and high level, and the x of its edges.
const L: f32 = 0.24;
const H: f32 = 0.68;
const R1: f32 = 0.2;
const F1: f32 = 0.48;
const R2: f32 = 0.66;
const F2: f32 = 0.94;

struct Ic<'a> {
    cv: &'a mut Canvas,
    f: &'a Fonts,
    r: Rect,
    col: Rgb,
    acc: Rgb,
}

type P = (f32, f32);

impl Ic<'_> {
    fn p(&self, x: f32, y: f32) -> (i32, i32) {
        (self.r.x + (x * (self.r.w - 1) as f32).round() as i32, self.r.y + ((1.0 - y) * (self.r.h - 1) as f32).round() as i32)
    }
    fn seg(&mut self, a: P, b: P, c: Rgb) {
        let (p, q) = (self.p(a.0, a.1), self.p(b.0, b.1));
        self.cv.line(p.0, p.1, q.0, q.1, c);
    }
    fn poly(&mut self, pts: &[P], c: Rgb) {
        for w in pts.windows(2) { self.seg(w[0], w[1], c); }
    }
    fn stroke(&mut self, pts: &[P]) { let c = self.col; self.poly(pts, c); }
    fn dash(&mut self, y: f32, x0: f32, x1: f32) {
        let (p, q) = (self.p(x0, y), self.p(x1, y));
        let c = self.acc;
        self.cv.dashed_hline(p.0, q.0, p.1, 3, 2, c);
    }
    fn dash_v(&mut self, x: f32, y0: f32, y1: f32) {
        let (p, q) = (self.p(x, y0), self.p(x, y1));
        let c = self.acc;
        let (a, b) = (p.1.min(q.1), p.1.max(q.1));
        let mut y = a;
        while y < b { self.cv.vline(p.0, y, (y + 2).min(b), c); y += 5; }
    }
    /// A double arrow up and down.
    fn va(&mut self, x: f32, y0: f32, y1: f32) {
        let (p, q) = (self.p(x, y0), self.p(x, y1));
        let c = self.acc;
        self.cv.vline(p.0, p.1.min(q.1), p.1.max(q.1), c);
        let d = if p.1 > q.1 { 1 } else { -1 };
        for (pt, s) in [(q, 1), (p, -1)] {
            self.cv.line(pt.0, pt.1, pt.0 - 2, pt.1 + 3 * d * s, c);
            self.cv.line(pt.0, pt.1, pt.0 + 2, pt.1 + 3 * d * s, c);
        }
    }
    /// A double arrow left and right.
    fn ha(&mut self, x0: f32, x1: f32, y: f32) {
        let (p, q) = (self.p(x0, y), self.p(x1, y));
        let c = self.acc;
        self.cv.hline(p.0, q.0, p.1, c);
        for (pt, s) in [(p, 1), (q, -1)] {
            self.cv.line(pt.0, pt.1, pt.0 + 3 * s, pt.1 - 2, c);
            self.cv.line(pt.0, pt.1, pt.0 + 3 * s, pt.1 + 2, c);
        }
    }
    fn dot(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        let c = self.acc;
        self.cv.circle(p.0, p.1, 2, c);
    }
    fn dots(&mut self, pts: &[P]) { for &(x, y) in pts { self.dot(x, y); } }
    fn tick(&mut self, x: f32, y0: f32, y1: f32) { self.dash_v(x, y0, y1); }
    fn mark(&mut self, a: P, b: P) { let c = self.acc; self.seg(a, b, c); }
    /// Hatching between two x and under a level.
    fn shade(&mut self, x0: f32, x1: f32, lo: f32, hi: f32) {
        let (a, b) = (self.p(x0, hi), self.p(x1, lo));
        let c = self.acc;
        let mut x = a.0;
        while x <= b.0 { self.cv.line(x, b.1, (x + 3).min(b.0), a.1, c); x += 3; }
    }
    fn tag(&mut self, s: &str) {
        let w = self.f.s.width(s);
        // a tag that does not fit is left out rather than cut, and so is one that would cover most of a small figure
        if w > self.r.w - 2 || w > self.r.w * 3 / 5 { return; }
        let (x, y) = (self.r.right() - w - 1, self.r.y + self.f.s.size * 3 / 4);
        let c = self.acc;
        self.cv.text(&self.f.s, x, y, s, c);
    }
    fn tag_l(&mut self, s: &str) {
        if self.f.s.width(s) > self.r.w - 2 { return; }
        let y = self.r.y + self.f.s.size * 3 / 4;
        let c = self.acc;
        self.cv.text(&self.f.s, self.r.x + 1, y, s, c);
    }

    // ------------------------------------------------------------------------------------------ base figures

    fn sq(&mut self) { self.stroke(&[(0.04, L), (R1, L), (R1, H), (F1, H), (F1, L), (R2, L), (R2, H), (F2, H)]); }
    /// A pulse train with slanted edges, for the rise and fall figures.
    fn sqs(&mut self) { self.stroke(&[(0.04, L), (0.14, L), (0.24, H), (0.48, H), (0.58, L), (0.7, L), (0.8, H), (0.96, H)]); }
    fn ring(&mut self, up: bool) {
        let (t, b) = if up { (H, L) } else { (L, H) };
        let k = if up { 1.0 } else { -1.0 };
        self.stroke(&[(0.04, b), (R1, b), (R1, t + 0.0), (R1 + 0.04, t + 0.2 * k), (R1 + 0.09, t - 0.08 * k), (R1 + 0.14, t + 0.05 * k), (R1 + 0.2, t), (F1, t), (F1, b), (F2, b)]);
    }
    fn sine(&mut self, amp: f32, mid: f32) {
        let pts: Vec<P> = (0..=24).map(|i| { let x = 0.04 + 0.92 * i as f32 / 24.0; (x, mid + amp * (i as f32 / 24.0 * std::f32::consts::TAU * 1.5).sin()) }).collect();
        self.stroke(&pts);
    }
    fn sine_at(&mut self, amp: f32, mid: f32, phase: f32) {
        let pts: Vec<P> = (0..=24).map(|i| { let x = 0.04 + 0.92 * i as f32 / 24.0; (x, mid + amp * (i as f32 / 24.0 * std::f32::consts::TAU * 1.5 + phase).sin()) }).collect();
        self.stroke(&pts);
    }
    fn nrz(&mut self) { self.stroke(&[(0.04, H), (0.22, H), (0.22, L), (0.4, L), (0.4, H), (0.58, H), (0.58, L), (0.94, L)]); }
    fn eye(&mut self) {
        let pts_a: Vec<P> = (0..=12).map(|i| { let x = 0.08 + 0.84 * i as f32 / 12.0; (x, 0.5 + 0.28 * (i as f32 / 12.0 * std::f32::consts::PI).cos()) }).collect();
        let pts_b: Vec<P> = pts_a.iter().map(|&(x, y)| (x, 1.0 - y)).collect();
        self.stroke(&pts_a);
        self.stroke(&pts_b);
        self.stroke(&[(0.08, 0.78), (0.92, 0.78)]);
        self.stroke(&[(0.08, 0.22), (0.92, 0.22)]);
    }
    fn bell(&mut self, w: f32, at: f32) {
        let pts: Vec<P> = (0..=20).map(|i| { let x = 0.06 + 0.88 * i as f32 / 20.0; (x, 0.2 + 0.6 * (-((x - at) / w).powi(2)).exp()) }).collect();
        self.stroke(&pts);
    }
    fn bars(&mut self, hs: &[f32]) {
        for (i, h) in hs.iter().enumerate() {
            let x = 0.12 + 0.76 * i as f32 / (hs.len().max(2) - 1) as f32;
            self.seg((x, 0.15), (x, 0.15 + h * 0.7), self.col);
        }
    }
    fn lissa(&mut self, w: f32) {
        let pts: Vec<P> = (0..=24).map(|i| { let a = i as f32 / 24.0 * std::f32::consts::TAU; (0.5 + 0.32 * a.cos(), 0.5 + 0.28 * (a + w).sin()) }).collect();
        self.stroke(&pts);
    }
    fn scatter(&mut self) { for (i, y) in [0.45, 0.58, 0.4, 0.62, 0.5, 0.36, 0.6, 0.47, 0.55, 0.42].iter().enumerate() { let c = self.col; let p = self.p(0.1 + 0.08 * i as f32, *y); self.cv.pixel(p.0, p.1, c, 255); self.cv.pixel(p.0 + 1, p.1, c, 255); } }
    /// Two traces, the second one later.
    fn pair(&mut self, shift: f32) {
        self.stroke(&[(0.04, L), (R1, L), (R1, H), (0.6, H), (0.6, L), (0.94, L)]);
        self.stroke(&[(0.04, L + 0.1), (R1 + shift, L + 0.1), (R1 + shift, H + 0.1 - 0.02), (0.6 + shift, H + 0.1 - 0.02), (0.6 + shift, L + 0.1), (0.94, L + 0.1)]);
    }
    fn switching(&mut self) {
        // drain voltage falling, current rising
        self.stroke(&[(0.04, H), (0.3, H), (0.45, L), (0.94, L)]);
        let c = self.col;
        self.poly(&[(0.04, L - 0.06), (0.3, L - 0.06), (0.45, 0.5), (0.7, 0.5), (0.94, L - 0.06)], c);
    }
    fn motor(&mut self) {
        for k in 0..3 { self.sine_at(0.18, 0.5, k as f32 * 2.09); }
    }
}

/// Draws the figure of `c` in `r`: the waveform in `col`, what is measured in `acc`. Nothing leaves `r`.
pub(super) fn draw_cat_icon(cv: &mut Canvas, f: &Fonts, r: Rect, c: Cat, col: Rgb, acc: Rgb) {
    let outer = cv.save_clip();
    cv.clip_within(r);
    let mut g = Ic { cv, f, r, col, acc };
    use Cat::*;
    match c {
        // levels
        Amplitude => { g.sq(); g.dash(H, 0.1, 0.9); g.dash(L, 0.1, 0.9); g.va(0.34, L, H); }
        Maximum => { g.sine(0.28, 0.5); g.dash(0.78, 0.06, 0.94); g.dot(0.2, 0.78); g.tag("max"); }
        Minimum => { g.sine(0.28, 0.5); g.dash(0.22, 0.06, 0.94); g.dot(0.37, 0.22); g.tag("min"); }
        PeakToPeak => { g.sine(0.28, 0.5); g.dash(0.78, 0.06, 0.94); g.dash(0.22, 0.06, 0.94); g.va(0.2, 0.22, 0.78); }
        PosOvershoot => { g.ring(true); g.dash(H, 0.1, 0.9); g.dot(R1 + 0.04, H + 0.2); g.va(0.12, H, H + 0.2); g.tag("+%"); }
        NegOvershoot => { g.ring(false); g.dash(H, 0.1, 0.9); g.dot(R1 + 0.04, L - 0.2); g.va(0.12, L - 0.2, L); g.tag("-%"); }
        Mean => { g.sine(0.28, 0.5); g.dash(0.5, 0.04, 0.96); g.tag("avg"); }
        Rms => { g.sine(0.28, 0.5); g.dash(0.5 + 0.2, 0.04, 0.96); g.dash(0.5 - 0.2, 0.04, 0.96); g.tag("rms"); }
        AcRms => { g.sine(0.2, 0.42); g.dash(0.42, 0.04, 0.96); g.va(0.5, 0.42, 0.62); g.tag("ac"); }
        Top => { g.sq(); g.dash(H, 0.04, 0.96); g.tick(0.34, H, H + 0.1); g.tag("top"); }
        Base => { g.sq(); g.dash(L, 0.04, 0.96); g.tick(0.56, L, L - 0.1); g.tag("base"); }
        Area => { g.sine(0.28, 0.5); g.shade(0.04, 0.3, 0.5, 0.78); g.dash(0.5, 0.04, 0.96); }
        // time
        Period => { g.sq(); g.tick(R1, H, 0.9); g.tick(R2, H, 0.9); g.ha(R1, R2, 0.9); }
        Frequency => { g.sq(); g.tick(R1, H, 0.9); g.tick(R2, H, 0.9); g.ha(R1, R2, 0.9); g.tag("1/T"); }
        UnitInterval => { g.nrz(); g.tick(0.22, L, 0.9); g.tick(0.4, L, 0.9); g.ha(0.22, 0.4, 0.88); g.tag("UI"); }
        DataRate => { g.nrz(); g.tick(0.22, L, 0.9); g.tick(0.4, L, 0.9); g.ha(0.22, 0.4, 0.88); g.tag("b/s"); }
        PosWidth => { g.sq(); g.tick(R1, H, 0.9); g.tick(F1, H, 0.9); g.ha(R1, F1, 0.88); g.tag("+"); }
        NegWidth => { g.sq(); g.tick(F1, L, 0.9); g.tick(R2, L, 0.9); g.ha(F1, R2, 0.88); g.tag("-"); }
        Skew => { g.pair(0.1); g.ha(R1, R1 + 0.1, 0.92); g.tag("skew"); }
        Delay => { g.pair(0.14); g.ha(R1, R1 + 0.14, 0.92); g.tick(R1, L, 0.92); g.tick(R1 + 0.14, L, 0.92); }
        Rise => { g.sqs(); g.dash(L + 0.1 * (H - L) * 0.0 + (H - L) * 0.1, 0.1, 0.3); g.dash(L + (H - L) * 0.9, 0.1, 0.3); g.tick(0.16, L, 0.9); g.tick(0.22, L, 0.9); g.ha(0.16, 0.22, 0.9); g.tag("10-90"); }
        Fall => { g.sqs(); g.tick(0.5, L, 0.9); g.tick(0.56, L, 0.9); g.ha(0.5, 0.56, 0.9); g.dash(L + (H - L) * 0.9, 0.44, 0.64); g.dash(L + (H - L) * 0.1, 0.44, 0.64); g.tag("90-10"); }
        Phase => { g.sine_at(0.2, 0.62, 0.0); g.sine_at(0.2, 0.3, 0.9); g.ha(0.0 + 0.04 + 0.0, 0.04 + 0.14, 0.5); g.tag("\u{b0}"); }
        RiseSlew => { g.sqs(); g.mark((0.14, L), (0.24, L)); g.mark((0.24, L), (0.24, H)); g.tag("V/s"); }
        FallSlew => { g.sqs(); g.mark((0.48, H), (0.58, H)); g.mark((0.48, H), (0.48, L)); g.tag("-V/s"); }
        Burst => { g.stroke(&[(0.04, L), (0.14, L), (0.14, H), (0.22, H), (0.22, L), (0.3, L), (0.3, H), (0.38, H), (0.38, L), (0.46, L), (0.46, H), (0.54, H), (0.54, L), (0.94, L)]); g.tick(0.14, H, 0.9); g.tick(0.54, H, 0.9); g.ha(0.14, 0.54, 0.88); }
        PosDuty => { g.sq(); g.ha(R1, F1, 0.9); g.ha(R1, R2, 0.8); g.tag("+%"); }
        NegDuty => { g.sq(); g.ha(F1, R2, 0.9); g.ha(R1, R2, 0.8); g.tag("-%"); }
        Outside => { g.sine(0.3, 0.5); g.dash(0.72, 0.04, 0.96); g.dash(0.28, 0.04, 0.96); g.shade(0.1, 0.3, 0.72, 0.8); g.tag("out"); }
        Setup => { g.pair(0.0); g.mark((0.3, 0.1), (0.3, 0.95)); g.ha(R1, 0.3, 0.14); g.tag("su"); }
        Hold => { g.pair(0.0); g.mark((0.1, 0.1), (0.1, 0.95)); g.ha(0.1, R1, 0.14); g.tag("hold"); }
        NPeriods => { g.sq(); g.ha(R1, F2, 0.9); g.tick(R1, H, 0.9); g.tick(F2, H, 0.9); g.tag("N"); }
        HighTime => { g.sq(); g.dash(L + (H - L) * 0.9, 0.04, 0.96); g.ha(R1, F1, 0.88); g.tag("hi"); }
        LowTime => { g.sq(); g.dash(L + (H - L) * 0.1, 0.04, 0.96); g.ha(F1, R2, 0.88); g.tag("lo"); }
        // jitter
        JitterSummary => { g.scatter(); g.va(0.5, 0.3, 0.7); g.tag("Tj"); }
        Tie => { g.scatter(); g.dash(0.5, 0.04, 0.96); g.tag("TIE"); }
        PhaseNoise => { g.stroke(&[(0.06, 0.85), (0.16, 0.5), (0.4, 0.34), (0.94, 0.22)]); g.dash(0.22, 0.06, 0.94); g.tag("PN"); }
        TjBer => { g.stroke(&[(0.06, 0.85), (0.2, 0.3), (0.5, 0.2), (0.8, 0.3), (0.94, 0.85)]); g.dash(0.4, 0.06, 0.94); g.tag("Tj"); }
        RjDd => { g.bell(0.18, 0.5); g.tag("Rj"); }
        DjDd => { g.bell(0.08, 0.3); g.bell(0.08, 0.7); g.tag("Dj"); }
        SjFreq => { g.sine(0.2, 0.5); g.scatter(); g.tag("Sj"); }
        Pj => { g.sine(0.28, 0.5); g.va(0.2, 0.22, 0.78); g.tag("Pj"); }
        Ddj => { g.bars(&[0.4, 0.8, 0.3, 0.7]); g.tag("DDJ"); }
        Dcd => { g.stroke(&[(0.04, L), (0.2, L), (0.2, H), (0.56, H), (0.56, L), (0.7, L), (0.7, H), (0.94, H)]); g.ha(0.2, 0.56, 0.88); g.ha(0.56, 0.7, 0.8); g.tag("DCD"); }
        Srj => { g.bell(0.3, 0.5); g.tag("SRJ"); }
        J2 => { g.bell(0.18, 0.5); g.dash_v(0.32, 0.2, 0.8); g.dash_v(0.68, 0.2, 0.8); g.tag("J2"); }
        // eye
        EyeHeight => { g.eye(); g.va(0.5, 0.28, 0.72); }
        EyeWidth => { g.eye(); g.ha(0.22, 0.78, 0.5); }
        EyeHigh => { g.eye(); g.dash(0.78, 0.08, 0.92); g.tag("1"); }
        EyeHeightBer => { g.eye(); g.va(0.5, 0.34, 0.66); g.tag("BER"); }
        EyeWidthBer => { g.eye(); g.ha(0.3, 0.7, 0.5); g.tag("BER"); }
        EyeLow => { g.eye(); g.dash(0.22, 0.08, 0.92); g.tag("0"); }
        QFactor => { g.eye(); g.va(0.5, 0.28, 0.72); g.tag("Q"); }
        BitHigh => { g.nrz(); g.dash(H, 0.04, 0.96); g.tag("1"); }
        BitLow => { g.nrz(); g.dash(L, 0.04, 0.96); g.tag("0"); }
        BitAmplitude => { g.nrz(); g.dash(H, 0.04, 0.96); g.dash(L, 0.04, 0.96); g.va(0.3, L, H); }
        DcCommon => { g.pair(0.0); g.dash(0.5, 0.04, 0.96); g.tag("DC"); }
        AcCommon => { g.sine_at(0.18, 0.62, 0.0); g.sine_at(0.18, 0.3, 0.0); g.dash(0.46, 0.04, 0.96); g.tag("AC"); }
        DiffCross => { g.sine_at(0.25, 0.5, 0.0); g.sine_at(0.25, 0.5, std::f32::consts::PI); g.dot(0.04 + 0.92 / 3.0 * 0.0 + 0.153, 0.5); g.dot(0.46, 0.5); g.tag("X"); }
        Tnt => { g.sq(); g.ha(R1, R2, 0.9); g.ha(R1, F2, 0.78); g.tag("T/nT"); }
        BitRate => { g.nrz(); g.ha(0.22, 0.58, 0.88); g.tag("bps"); }
        SscDev => { g.stroke(&[(0.04, 0.5), (0.2, 0.4), (0.36, 0.5), (0.52, 0.62), (0.68, 0.5), (0.84, 0.38), (0.94, 0.5)]); g.va(0.5, 0.38, 0.62); g.tag("SSC"); }
        SscRate => { g.stroke(&[(0.04, 0.5), (0.2, 0.4), (0.36, 0.5), (0.52, 0.62), (0.68, 0.5), (0.84, 0.38), (0.94, 0.5)]); g.ha(0.2, 0.84, 0.9); g.tag("kHz"); }
        // power
        PowerQuality => { g.sine_at(0.2, 0.5, 0.0); g.sine_at(0.12, 0.5, 0.8); g.tag("PQ"); }
        Harmonics => { g.bars(&[0.9, 0.2, 0.5, 0.15, 0.3]); g.tag("THD"); }
        Inrush => { g.stroke(&[(0.04, 0.2), (0.2, 0.2), (0.22, 0.85), (0.4, 0.5), (0.7, 0.3), (0.94, 0.26)]); g.dot(0.22, 0.85); g.dash(0.85, 0.1, 0.4); g.tag("Ipk"); }
        InputC => { g.stroke(&[(0.04, 0.2), (0.5, 0.75), (0.94, 0.75)]); g.stroke(&[(0.04, 0.5), (0.1, 0.9), (0.5, 0.3), (0.94, 0.28)]); g.tag("C"); }
        CycAmp => { g.sq(); g.tick(R1, L, 0.92); g.tick(R2, L, 0.92); g.dash(H, R1, R2); g.dash(L, R1, R2); g.va(0.4, L, H); g.tag("cyc"); }
        CycTop => { g.sq(); g.tick(R1, L, 0.92); g.tick(R2, L, 0.92); g.dash(H, R1, R2); g.tag("cTop"); }
        CycBase => { g.sq(); g.tick(R1, L, 0.92); g.tick(R2, L, 0.92); g.dash(L, R1, R2); g.tag("cBase"); }
        CycPp => { g.sine(0.28, 0.5); g.tick(0.04, 0.1, 0.9); g.tick(0.36, 0.1, 0.9); g.va(0.2, 0.22, 0.78); g.tag("cPp"); }
        CycMax => { g.sine(0.28, 0.5); g.tick(0.04, 0.1, 0.9); g.tick(0.36, 0.1, 0.9); g.dot(0.2, 0.78); g.tag("cMax"); }
        CycMin => { g.sine(0.28, 0.5); g.tick(0.04, 0.1, 0.9); g.tick(0.36, 0.1, 0.9); g.dot(0.04 + 0.92 / 3.0 * 0.75 * 0.0 + 0.3, 0.22); g.tag("cMin"); }
        TurnOn => { g.switching(); g.ha(0.3, 0.45, 0.9); g.tag("on"); }
        TurnOff => { g.switching(); g.ha(0.7, 0.94, 0.9); g.tag("off"); }
        EOn => { g.switching(); g.shade(0.3, 0.45, L, 0.5); g.tag("Eon"); }
        EOff => { g.switching(); g.shade(0.7, 0.94, L - 0.06, 0.5); g.tag("Eoff"); }
        ECond => { g.switching(); g.shade(0.45, 0.7, L, 0.5); g.tag("Ec"); }
        DvdtRise => { g.sqs(); g.mark((0.14, L), (0.24, L)); g.mark((0.24, L), (0.24, H)); g.tag("dv/dt"); }
        DvdtFall => { g.sqs(); g.mark((0.48, H), (0.58, H)); g.mark((0.58, H), (0.58, L)); g.tag("-dv"); }
        DidtRise => { g.stroke(&[(0.04, L), (0.3, L), (0.6, H), (0.94, H)]); g.mark((0.3, L), (0.6, L)); g.mark((0.6, L), (0.6, H)); g.tag("di/dt"); }
        DidtFall => { g.stroke(&[(0.04, H), (0.3, H), (0.6, L), (0.94, L)]); g.mark((0.3, H), (0.6, H)); g.mark((0.6, H), (0.6, L)); g.tag("-di"); }
        RdsOn => { g.stroke(&[(0.04, L), (0.3, L), (0.3, 0.7), (0.9, 0.7)]); g.stroke(&[(0.04, 0.3), (0.3, 0.5), (0.9, 0.52)]); g.tag("Rds"); }
        Inductance => { g.lissa(0.0); g.tag("L"); }
        MagProp => { g.lissa(0.6); g.tag("B-H"); }
        MagLoss => { g.lissa(0.6); g.shade(0.3, 0.7, 0.4, 0.6); g.tag("Pc"); }
        Ripple => { g.stroke(&[(0.04, 0.5), (0.14, 0.58), (0.24, 0.44), (0.34, 0.58), (0.44, 0.44), (0.54, 0.58), (0.64, 0.44), (0.74, 0.58), (0.84, 0.44), (0.94, 0.5)]); g.dash(0.44, 0.04, 0.96); g.dash(0.58, 0.04, 0.96); g.va(0.3, 0.44, 0.58); }
        LineRipple => { g.sine(0.12, 0.5); g.va(0.3, 0.38, 0.62); g.tag("line"); }
        SwRipple => { g.stroke(&[(0.04, 0.5), (0.1, 0.6), (0.16, 0.4), (0.22, 0.6), (0.28, 0.4), (0.34, 0.6), (0.4, 0.4), (0.94, 0.5)]); g.va(0.2, 0.4, 0.6); g.tag("sw"); }
        Psrr => { g.sine(0.28, 0.66); g.sine(0.08, 0.28); g.tag("PSRR"); }
        Impedance => { g.sine_at(0.2, 0.62, 0.0); g.sine_at(0.14, 0.3, 0.3); g.tag("Z"); }
        PowerFactor => { g.sine_at(0.2, 0.62, 0.0); g.sine_at(0.2, 0.3, 0.7); g.ha(0.04 + 0.0, 0.04 + 0.13, 0.5); g.tag("PF"); }
        // motor
        Dq0 => { g.motor(); g.tag("dq0"); }
        Speed => { g.motor(); g.ha(0.2, 0.7, 0.92); g.tag("rpm"); }
        Accel => { g.motor(); g.mark((0.2, 0.2), (0.8, 0.2)); g.tag("acc"); }
        Direction => { g.motor(); g.mark((0.3, 0.9), (0.7, 0.9)); g.mark((0.7, 0.9), (0.64, 0.84)); g.tag("dir"); }
        Angle => { g.motor(); g.tick(0.5, 0.0, 1.0); g.tag("ang"); }
        Torque => { g.lissa(0.0); g.tag("Nm"); }
        MechPower => { g.lissa(0.0); g.shade(0.3, 0.7, 0.35, 0.65); g.tag("Pm"); }
        SysEff => { g.sine_at(0.22, 0.62, 0.0); g.sine_at(0.14, 0.3, 0.0); g.tag("eff"); }
        // rails
        RailOver => { g.stroke(&[(0.04, 0.5), (0.3, 0.5), (0.34, 0.84), (0.46, 0.56), (0.56, 0.52), (0.94, 0.5)]); g.dash(0.5, 0.04, 0.96); g.dot(0.34, 0.84); g.tag("over"); }
        RailUnder => { g.stroke(&[(0.04, 0.5), (0.3, 0.5), (0.34, 0.16), (0.46, 0.44), (0.56, 0.48), (0.94, 0.5)]); g.dash(0.5, 0.04, 0.96); g.dot(0.34, 0.16); g.tag("under"); }
        RailTurn => { g.stroke(&[(0.04, 0.2), (0.3, 0.2), (0.6, 0.7), (0.94, 0.7)]); g.dash(0.7, 0.3, 0.96); g.ha(0.3, 0.6, 0.9); g.tag("on"); }
        DcRail => { g.stroke(&[(0.04, 0.6), (0.94, 0.6)]); g.dash(0.6, 0.04, 0.96); g.tag("DC"); }
        // double pulse
        Vpeak => { g.switching(); g.dot(0.3, H); g.dash(H, 0.1, 0.5); g.tag("Vpk"); }
        Ipeak => { g.switching(); g.dot(0.58, 0.5); g.dash(0.5, 0.4, 0.8); g.tag("Ipk"); }
        TdOn => { g.switching(); g.ha(0.2, 0.3, 0.9); g.tag("td"); }
        Tr => { g.switching(); g.ha(0.3, 0.45, 0.9); g.tag("tr"); }
        Ton => { g.switching(); g.ha(0.2, 0.45, 0.9); g.tag("ton"); }
        TdOff => { g.switching(); g.ha(0.64, 0.74, 0.9); g.tag("tdf"); }
        Tf => { g.switching(); g.ha(0.7, 0.8, 0.9); g.tag("tf"); }
        Toff => { g.switching(); g.ha(0.64, 0.94, 0.9); g.tag("toff"); }
        DpdtV => { g.switching(); g.mark((0.3, H), (0.45, H)); g.mark((0.45, H), (0.45, L)); g.tag("dV"); }
        DpdtI => { g.switching(); g.mark((0.3, L - 0.06), (0.45, L - 0.06)); g.mark((0.45, L - 0.06), (0.45, 0.5)); g.tag("dI"); }
        Tdt => { g.switching(); g.ha(0.3, 0.7, 0.9); g.tag("tdt"); }
        Trr => { g.stroke(&[(0.04, 0.6), (0.3, 0.6), (0.4, 0.16), (0.55, 0.3), (0.7, 0.5), (0.94, 0.5)]); g.ha(0.3, 0.7, 0.9); g.tag("trr"); }
        Qrr => { g.stroke(&[(0.04, 0.6), (0.3, 0.6), (0.4, 0.16), (0.55, 0.3), (0.7, 0.5), (0.94, 0.5)]); g.shade(0.34, 0.6, 0.16, 0.5); g.tag("Qrr"); }
        Err => { g.stroke(&[(0.04, 0.6), (0.3, 0.6), (0.4, 0.16), (0.55, 0.3), (0.7, 0.5), (0.94, 0.5)]); g.shade(0.4, 0.7, 0.16, 0.4); g.tag("Err"); }
        Irrm => { g.stroke(&[(0.04, 0.6), (0.3, 0.6), (0.4, 0.16), (0.55, 0.3), (0.7, 0.5), (0.94, 0.5)]); g.dot(0.4, 0.16); g.tag("Irm"); }
        DiodeDdt => { g.stroke(&[(0.04, 0.6), (0.3, 0.6), (0.4, 0.16), (0.55, 0.3), (0.7, 0.5), (0.94, 0.5)]); g.mark((0.3, 0.6), (0.4, 0.6)); g.mark((0.4, 0.6), (0.4, 0.16)); g.tag("dI/dt"); }
        Qoss => { g.stroke(&[(0.04, 0.15), (0.5, 0.8), (0.94, 0.8)]); g.shade(0.06, 0.5, 0.15, 0.5); g.tag("Qoss"); }
    }
    g.cv.restore_clip(outer);
}
