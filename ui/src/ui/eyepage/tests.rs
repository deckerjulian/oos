// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Tests of the eye page: layout, the figures on clean and noisy signals, pooling over records, the sliding column
//! and the open plot, drawing at every height, and the pictures of `eye_page_shots`. A new test of the page goes
//! here.

use super::*;

/// Deterministic noise for the test signals.
struct Noise(u64);
impl Noise {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    fn gauss(&mut self, sigma: f64) -> f64 {
        let (u, v) = (self.uniform(), self.uniform());
        sigma * (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
}

/// What a test signal carries.
#[derive(Clone, Copy)]
struct Sig {
    rate: f64,
    uis: usize,
    prbs: bool,
    /// sample steps per UI
    spu: usize,
    rj: f64,
    pj: f64,
    volt_noise: f64,
}

/// NRZ of +-1 V with linear edges of a quarter UI.
fn record(s: Sig, n: &mut Noise) -> Record {
    let ui = 1.0 / s.rate;
    let dt = ui / s.spu as f64;
    let start = n.uniform() * ui;
    let (mut lfsr, mut level) = (0x7fu8, false);
    let mut edges = Vec::new();
    for k in 0..s.uis {
        let bit = if s.prbs {
            let b = ((lfsr >> 6) ^ (lfsr >> 5)) & 1;
            lfsr = ((lfsr << 1) | b) & 0x7f;
            b == 1
        } else {
            k % 2 == 0
        };
        if bit != level {
            let t = k as f64 * ui;
            edges.push((t + n.gauss(s.rj) + s.pj * (2.0 * std::f64::consts::PI * t * s.rate / 37.0).sin(), bit));
            level = bit;
        }
    }
    let (tr, mut at) = (0.25 * ui, 0usize);
    let mut v = Vec::with_capacity(s.uis * s.spu);
    for i in 0..s.uis * s.spu {
        let t = i as f64 * dt + start;
        while at + 1 < edges.len() && edges[at + 1].0 - tr / 2.0 <= t {
            at += 1;
        }
        let (te, up) = edges[at];
        let from = if up { -1.0 } else { 1.0 };
        let y = from + (-2.0 * from) * ((t - te) / tr + 0.5).clamp(0.0, 1.0);
        v.push((y + n.gauss(s.volt_noise)) as f32);
    }
    let mut ch: [Vec<f32>; NCH] = Default::default();
    ch[0] = v;
    Record { t0: 0.0, dt, ch, triggered: true, partial: false, raw: None, la: None, pairs: false }
}

fn page(s: Sig, records: usize) -> Ui {
    let mut ui = crate::ui::testkit::demo_ui();
    ui.step(0.05);
    ui.m.run = RunState::Stop;
    ui.m.view = AView::Eye;
    ui.relayout();
    ui.azr.eye = None;
    ui.azr.page = Page::default();
    let mut n = Noise(0x2545_f491_4f6c_dd1d);
    for _ in 0..records {
        ui.last[0] = Some(record(s, &mut n));
        ui.measure_last();
        ui.analyzer_update();
        ui.now += REFRESH;
    }
    ui
}

const CLEAN: Sig = Sig { rate: 10e6, uis: 400, prbs: false, spu: 125, rj: 0.0, pj: 0.0, volt_noise: 0.002 };
const NOISY: Sig = Sig { rate: 100e6, uis: 2000, prbs: true, spu: 12, rj: 0.25e-9, pj: 0.3e-9, volt_noise: 0.04 };

#[test]
fn a_tiny_reference_zoom_keeps_full_record_eye_ssc_and_tie_without_duplicate_folds() {
    let rate = 20e6;
    let modulation = 20e3;
    let dt = 1.0 / (64.0 * rate);
    let edges = oos_dsp::hss::ssc_edge_times(4002, rate, 0.01, modulation, "Down-spread only");
    let mut at = 0;
    let samples: Vec<f32> = (0..256_000)
        .map(|i| {
            let t = i as f64 * dt;
            while at + 1 < edges.len() && edges[at + 1] <= t {
                at += 1;
            }
            let level = if at % 2 == 0 { 1.0 } else { -1.0 };
            // A short linear transition, with all edge times coming from the SSC ramp.
            let fraction = ((t - edges[at]) / (0.1 / rate)).clamp(0.0, 1.0);
            (-level + 2.0 * level * fraction) as f32
        })
        .collect();
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.run = RunState::Stop;
    ui.session_is_sim = false;
    ui.set_offline(true);
    ui.m.sim_slot = 0;
    ui.refs[0] = Some(RefWave { label: "SSC clock".into(), vpd: 0.5, ..RefWave::test(samples, dt, 0.0) });
    ui.m.eye_src = WaveId::Ref(0);
    ui.m.eye_rate = rate;
    ui.m.zoom = true;
    ui.m.zscale = tscales().iter().position(|&scale| scale as f64 >= 0.2 / rate / HDIV as f64).unwrap();
    ui.m.zdelay = 50e-6;
    ui.refresh_stopped_reference();
    assert!(ui.reference_slice(0).unwrap().0.len() < 64, "ordinary measurements still use the tiny view");
    ui.m.view = AView::Eye;
    ui.relayout();
    ui.eye_update(&Ui::blank_record());
    assert_eq!(ui.azr.page.records, 1);
    assert_eq!(ui.azr.eye.as_ref().unwrap().records, 1);
    assert!(ui.azr.page.edges > 3000, "full record TIE: {} edges", ui.azr.page.edges);
    assert!(ui.azr.page.title.contains("full record"), "{}", ui.azr.page.title);
    let reading = |ui: &Ui, name| ui.azr.page.readings.iter().find(|(n, _)| *n == name).unwrap().1;
    let ssc = reading(&ui, "SSCRate");
    assert!((ssc / modulation - 1.0).abs() < 0.25, "SSC {ssc} Hz, expected {modulation}");
    // Downspread has a 1% peak-to-peak local rate span. Same-polarity
    // averaging and interpolation at 64 samples/UI introduce a small error.
    let ppm = reading(&ui, "SSCPpm");
    assert!((ppm / 10_000.0 - 1.0).abs() < 0.03, "SSC depth {ppm} ppm, expected 10000 ppm");
    let hits = ui.azr.eye.as_ref().unwrap().hits.clone();
    let identity = ui.azr.eye_ref;
    for delay in [20e-6, 80e-6, 120e-6] {
        ui.m.zdelay = delay;
        ui.now += REFRESH;
        ui.refresh_stopped_reference();
        assert_eq!(ui.azr.page.records, 1);
        assert_eq!(ui.azr.eye.as_ref().unwrap().hits, hits);
        assert_eq!(ui.azr.eye_ref, identity);
        assert_eq!(reading(&ui, "SSCRate"), ssc);
    }
    // A waveform edit away from the old three fingerprint samples is still a new record.
    std::sync::Arc::make_mut(&mut ui.refs[0].as_mut().unwrap().v)[1000] *= 0.8;
    ui.eye_update(&Ui::blank_record());
    assert_ne!(ui.azr.eye_ref, identity);
    assert_eq!(ui.azr.page.records, 1);
    let waveform_identity = ui.azr.eye_ref;
    ui.m.eye_ber = 1e-6;
    ui.eye_update(&Ui::blank_record());
    assert_ne!(ui.azr.eye_ref, waveform_identity, "settings refresh the cached figures");
    assert_eq!(ui.azr.page.records, 1);
    std::sync::Arc::make_mut(&mut ui.refs[0].as_mut().unwrap().v).fill(0.0);
    ui.eye_update(&Ui::blank_record());
    assert!(ui.azr.eye.is_none() && ui.azr.eye_res.is_none(), "an edgeless replacement cannot keep stale results");
}

#[test]
fn the_page_tiles_the_area_to_its_bottom_edge() {
    for h in [160, 240, 296, 300, 352, 400, 440, 480] {
        let a = Rect::new(0, 40, PANEL_W, h);
        let l = layout(a, false);
        let bottom = a.y + a.h;
        assert_eq!(
            l.charts[2].y + l.charts[2].h + if l.jit_rows > 0 { GAP + l.jit.h } else { 0 },
            bottom,
            "right column, height {}",
            h
        );
        assert_eq!(l.charts[3].y + l.charts[3].h, l.charts[2].y + l.charts[2].h);
        assert_eq!(
            l.eye.y + l.eye.h + if l.fig_rows > 0 { GAP + l.fig.h } else { 0 },
            bottom,
            "left column, height {}",
            h
        );
        assert_eq!(l.charts[1].x + l.charts[1].w, a.x + a.w, "height {}", h);
        assert!(l.plots.iter().all(|p| p.h >= 1 && p.w > 40), "{:?}", l.plots);
        assert_eq!(l.fig_rows > 0, h >= BOXES_MIN_H);
    }
}

#[test]
fn full_screen_gives_the_eye_the_whole_plot() {
    let a = Rect::new(0, 0, W, H);
    let l = layout(a, true);
    assert_eq!((l.fig_rows, l.jit_rows), (0, 0));
    assert!(l.plot.w > 900 && l.plot.h > 500, "{:?}", l.plot);
    assert!(l.charts.iter().all(|c| c.w == 0));
}

#[test]
fn narrow_pane_stacks_the_charts_and_hiding_them_gives_the_eye_its_height() {
    let area = Rect::new(600, 80, 400, 400);
    let home = layout(area, false);
    assert_eq!((home.eye.x, home.eye.w), (area.x, area.w));
    assert!(home.charts.iter().all(|chart| chart.y >= home.eye.y + home.eye.h));
    let hidden = placed(area, 0.0);
    assert_eq!((hidden.eye.y, hidden.eye.h), (area.y, area.h));
    assert!(hidden.charts.iter().all(|chart| chart.x >= area.x + area.w));
    let short = layout(Rect::new(0, 350, 900, 160), false);
    assert!(short.eye.h > 0 && short.plot.h > 0);
}

#[test]
fn the_eye_range_holds_the_signal_on_whole_steps() {
    for (lo, hi) in [(-1.0, 1.0), (0.0, 3.3), (0.1, 0.18), (-0.002, 0.003), (47.0, 53.0)] {
        let (a, b, step) = eye_range(lo, hi);
        assert!(a < lo && b > hi, "{} {}", a, b);
        let n = (b - a) / step;
        assert!((n - n.round()).abs() < 1e-6 && (3.0..=9.0).contains(&n), "{} {} {}", a, b, step);
        assert!((hi - lo) / (b - a) > 0.4, "{} {}", a, b);
    }
}

/// The eye fits the signal, not the scale of the channel, and a change of the signal gets a new range.
#[test]
fn the_eye_follows_the_signal() {
    let mut ui = page(CLEAN, 3);
    let e = ui.azr.eye.as_ref().unwrap();
    assert!(e.v_max >= 1.0 && e.v_max < 2.0 && e.v_min <= -1.0 && e.v_min > -2.0, "{} {}", e.v_min, e.v_max);
    let mut rec = ui.last[0].clone().unwrap();
    rec.ch[0].iter_mut().for_each(|x| *x *= 0.1);
    ui.last[0] = Some(rec);
    ui.measure_last();
    ui.analyzer_update();
    let e = ui.azr.eye.as_ref().unwrap();
    assert!(e.v_max < 0.5, "{}", e.v_max);
}

/// Few edges of a clock, every record on the same polarity: the figures are those of the signal.
#[test]
fn a_clock_with_few_edges_has_a_full_eye() {
    let ui = page(Sig { uis: 10, ..CLEAN }, 6);
    let (m, _, _) = ui.azr.eye_res.clone().expect("eye result");
    assert!((m.amplitude - 2.0).abs() < 0.1, "{:?}", m);
    assert!(m.width.is_finite() && m.crossing_pct.is_finite(), "{:?}", m);
}

/// An idle record neither restarts the page nor moves the range.
#[test]
fn a_record_without_edges_keeps_the_eye() {
    let mut ui = page(CLEAN, 5);
    let (top, n) = (ui.azr.eye.as_ref().unwrap().v_max, ui.azr.page.records);
    let mut rec = ui.last[0].clone().unwrap();
    rec.ch[0].iter_mut().for_each(|x| *x = 0.01);
    ui.last[0] = Some(rec);
    ui.measure_last();
    ui.analyzer_update();
    assert_eq!((ui.azr.eye.as_ref().unwrap().v_max, ui.azr.page.records), (top, n));
    assert!(ui.azr.page.status[1].contains("Under 3 edges"), "{:?}", ui.azr.page.status);
}

#[test]
fn disabled_eye_source_and_clock_ignore_samples_retained_in_the_last_record() {
    let mut ui = page(CLEAN, 2);
    let mut rec = ui.last[0].clone().unwrap();
    assert!(!rec.ch[0].is_empty());
    let records = ui.azr.page.records;
    ui.m.ch[0].on = false;
    ui.eye_update(&rec);
    assert_eq!(ui.azr.page.status[0], "CH1 is off");
    assert_eq!(ui.azr.page.records, records);
    ui.m.ch[0].on = true;
    rec.ch[1] = rec.ch[0].clone();
    ui.m.ch[1].on = false;
    ui.m.eye_b = Some(WaveId::Ch(1));
    ui.eye_update(&rec);
    assert_eq!(ui.azr.page.status[0], "CH2 is off");
    assert_eq!(ui.azr.page.records, records);
}

/// Every figure the page draws has one number for SCPI, including the ones that came out missing.
#[test]
fn every_figure_has_one_number() {
    let ui = page(CLEAN, 2);
    let got: Vec<_> = ui.azr.page.readings.iter().map(|(n, _)| *n).collect();
    for name in EYE_ITEMS {
        assert_eq!(got.iter().filter(|n| *n == name).count(), 1, "{name} in {got:?}");
    }
    assert_eq!(got.len(), EYE_ITEMS.len());
    assert!(ui.azr.page.readings.iter().any(|(n, v)| *n == "HEIGht" && v.is_finite()));
    assert!(ui.azr.page.readings.iter().any(|(n, v)| *n == "WIDTh" && v.is_finite()));
    assert!(ui.azr.page.readings.iter().any(|(n, v)| *n == "UI" && v.is_finite()));
    assert!(ui.azr.page.readings.iter().any(|(n, v)| *n == "TJ" && v.is_finite()));
}

/// Time of the parts on a deep record: `cargo test --release eye_page_timing -- --ignored --nocapture`.
#[test]
#[ignore]
fn eye_page_timing() {
    let s = Sig { rate: 10e6, uis: 8000, prbs: true, spu: 125, rj: 50e-12, pj: 0.0, volt_noise: 0.01 };
    let mut ui = page(s, 3);
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    let t = std::time::Instant::now();
    for _ in 0..20 {
        ui.analyzer_update();
    }
    println!("update, no refresh {:.2} ms", t.elapsed().as_secs_f64() * 50.0);
    let t = std::time::Instant::now();
    for _ in 0..20 {
        ui.now += REFRESH;
        ui.analyzer_update();
    }
    println!("update with refresh {:.2} ms", t.elapsed().as_secs_f64() * 50.0);
    let t = std::time::Instant::now();
    for _ in 0..200 {
        ui.draw_eye(&mut cv);
    }
    println!("draw {:.3} ms", t.elapsed().as_secs_f64() * 5.0);
    let rec = ui.last[0].clone().unwrap();
    let v = &rec.ch[0];
    let clk = eye::recover_clock(v, rec.dt, 0.0, 0.1, None).unwrap();
    for span in [1, 2] {
        let mut e = eye::Eye::new(530, 340, span, -1.5, 1.5);
        let t = std::time::Instant::now();
        for _ in 0..10 {
            e.accumulate(v, rec.dt, &clk);
        }
        println!("accumulate {} samples, span {}: {:.2} ms", v.len(), span, t.elapsed().as_secs_f64() * 100.0);
    }
}

#[test]
fn nothing_sits_under_the_overflow_button() {
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.view = AView::Eye;
    for w in [640, PANEL_W] {
        for h in [160, 352, 480] {
            ui.workspace.eye_float = Some(Rect::new(0, 40, w, h));
            ui.relayout();
            let content = ui.eye_page_rect();
            let close = ui.eye_pane_close();
            assert!(content.y >= close.y + close.h, "workspace header must reserve Close before laying out any charts");
            let l = layout(content, false);
            for r in [l.eye, l.head, l.fig, l.jit].iter().chain(&l.charts) {
                assert!(!overlaps(*r, close), "{:?} sits under actual Close {:?}", r, close);
            }
        }
    }
}

/// Every title and figure inside its box, with the longest values the page can show.
#[test]
fn the_text_fits_its_boxes() {
    for s in [CLEAN, NOISY, Sig { uis: 6, ..CLEAN }] {
        let ui = page(s, 8);
        let (f, p) = (ui.fonts(), &ui.azr.page);
        let l = layout(Rect::new(0, 40, PANEL_W, 400), false);
        for (k, c) in p.charts.iter().enumerate() {
            assert!(f.sb.width(&c.title) + 12 <= l.charts[k].w, "'{}'", c.title);
            assert!(f.s.width(&c.empty) <= l.plots[k].w, "'{}'", c.empty);
            assert!(f.s.width(&c.y.0).max(f.s.width(&c.y.1)) + 3 <= CHART_GUTTER, "{:?}", c.y);
        }
        assert!(f.sb.width(&p.status[0]) + 2 <= l.head.w && f.s.width(&p.status[1]) + 2 <= l.head.w, "{:?}", p.status);
        assert!(f.sb.width(&p.title) + 6 + 196 <= l.eye.w, "'{}'", p.title);
        let room = |r: Rect, n: i32| (r.w - 12) / n - 4;
        for cells in [&p.eye_measured, &p.eye_model, &p.stats] {
            for c in cells {
                assert!(f.s.width(c.label) <= LABEL_W - 2, "'{}'", c.label);
                assert!(LABEL_W + f.s.width(&c.value) <= room(l.fig, 3), "'{}'", c.value);
            }
        }
        for cells in [&p.measured, &p.model] {
            for c in cells {
                assert!(f.s.width(c.label) <= LABEL_W - 2, "'{}'", c.label);
                assert!(LABEL_W + f.s.width(&c.value) <= room(l.jit, 2), "'{}'", c.value);
            }
        }
    }
}

#[test]
fn few_edges_per_record_pool_into_a_result() {
    let ui = page(Sig { uis: 12, ..CLEAN }, 200);
    let (_, j, _) = ui.azr.eye_res.clone().expect("eye result");
    assert!(j.rj_rms.is_finite() && j.tj_pp.is_finite(), "{:?}", j.fit);
    assert_ne!(j.fit, JitterFit::TooFew);
    assert!(ui.azr.page.pool.len() > 1000);
}

/// No number stands in for one that was not computed.
#[test]
fn too_little_data_says_so() {
    let ui = page(Sig { uis: 6, ..CLEAN }, 3);
    let p = &ui.azr.page;
    assert!(p.status[1].contains("needs"), "{:?}", p.status);
    assert!(p.pool.is_empty());
    assert!(!p.charts[1].empty.is_empty() && !p.charts[2].empty.is_empty() && !p.charts[3].empty.is_empty());
    assert!(p.model.iter().all(|c| c.kind == Kind::Missing), "{:?}", p.model);
    for c in p.measured.iter().chain(&p.model).chain(&p.eye_measured).chain(&p.eye_model).chain(&p.stats) {
        assert!(!c.value.contains("---") && !c.value.contains("NaN") && !c.value.contains("inf"), "{:?}", c);
    }
}

#[test]
fn a_noisy_signal_fills_every_chart() {
    let ui = page(NOISY, 30);
    let p = &ui.azr.page;
    let (_, j, period) = ui.azr.eye_res.clone().expect("eye result");
    assert_eq!(j.fit, JitterFit::Tails);
    assert!((period - 10e-9).abs() < 1e-11, "{}", period);
    assert!(p.charts.iter().all(|c| c.empty.is_empty()), "{:?}", p.charts);
    assert!(!p.trend.is_empty() && !p.hist.is_empty() && !p.spec.is_empty() && !p.tub.model.is_empty());
    assert!(
        p.measured.iter().filter(|c| c.label == "DDJ" || c.label == "PJ rms").all(|c| c.kind == Kind::Measured),
        "{:?}",
        p.measured
    );
    assert!(p.model.iter().any(|c| c.label == "TJ 1e-12" && c.kind == Kind::Model));
}

#[test]
fn clear_starts_the_page_over() {
    let mut ui = page(CLEAN, 5);
    assert!(!ui.azr.page.img.is_empty());
    ui.activate(Item::EyeClear);
    assert_eq!(ui.m.view, AView::Eye);
    assert!(ui.azr.eye.is_none() && ui.azr.page.pool.is_empty() && ui.azr.page.img.is_empty());
    ui.render(&mut Canvas::new(PANEL_W, PANEL_H));
}

#[test]
fn another_signal_starts_the_page_over() {
    let mut ui = page(CLEAN, 5);
    let mut n = Noise(77);
    ui.last[0] = Some(record(Sig { rate: 20e6, ..CLEAN }, &mut n));
    ui.analyzer_update();
    assert_eq!(ui.azr.page.records, 1);
}

#[test]
fn it_draws_at_every_height_and_with_the_source_off() {
    let mut ui = page(NOISY, 4);
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    for h in [160, 240, 352, 480] {
        ui.area.h = h;
        ui.analyzer_update();
        ui.draw_eye(&mut cv);
    }
    let mut rec = ui.last[0].clone().unwrap();
    rec.ch[0].clear();
    ui.last[0] = Some(rec);
    ui.analyzer_update();
    assert!(ui.azr.page.status[0].contains("off"));
    ui.draw_eye(&mut cv);
}

fn separated(a: Rect, b: Rect) -> bool {
    a.w > 0
        && b.w > 0
        && a.h > 0
        && b.h > 0
        && !(a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h)
}

#[test]
fn selected_eye_source_keeps_its_identity_and_updates_only_automatic_rate() {
    let mut ui = crate::ui::testkit::demo_ui();
    ui.set_offline(true);
    let mut noise = Noise(3);
    for (slot, signal) in [(0, CLEAN), (1, Sig { rate: CLEAN.rate * 2.0, ..CLEAN })] {
        let rec = record(signal, &mut noise);
        ui.refs[slot] = Some(RefWave {
            color: slot,
            label: format!("Rate {slot}"),
            vpd: 0.2,
            ..RefWave::test(rec.ch[0].clone(), rec.dt, rec.t0)
        });
    }
    ui.select_workspace_source(WaveId::Ref(1));
    ui.eye_make();
    assert_eq!(ui.m.eye_src, WaveId::Ref(1));
    assert!((ui.m.eye_rate - CLEAN.rate * 2.0).abs() / (CLEAN.rate * 2.0) < 0.02);
    ui.adjust(Item::EyeSrc, -1);
    assert_eq!(ui.m.eye_src, WaveId::Ref(0));
    assert!((ui.m.eye_rate - CLEAN.rate).abs() / CLEAN.rate < 0.02);
    ui.m.eye_from_record = false;
    ui.m.eye_rate = 125e6;
    ui.adjust(Item::EyeSrc, 1);
    assert_eq!(ui.m.eye_rate, 125e6, "a deliberately manual rate survives source selection");
    std::sync::Arc::make_mut(&mut ui.refs[1].as_mut().unwrap().v).fill(0.0);
    ui.eye_make();
    assert_eq!(ui.m.eye_src, WaveId::Ref(1));
    assert_eq!(ui.m.eye_rate, 0.0, "a flat selected record does not silently use another source's clock");
}

/// One press reads the bit rate from the record. The eye starts beside the trace; Under stacks it below.
#[test]
fn one_press_reads_the_rate_and_keeps_the_trace() {
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.eye_place = EyePlace::Side;
    ui.m.run = RunState::Stop;
    ui.m.eye_rate = 123.0;
    let mut n = Noise(3);
    ui.last[0] = Some(record(CLEAN, &mut n));
    ui.render(&mut Canvas::new(W, H));
    let g = ui.grid;
    ui.tap(g.x + 40, g.y + 30);
    assert_ne!(ui.m.view, AView::Eye, "the waveform has no Eye shortcut");
    ui.open_menu(MenuId::Eye);
    assert!(ui.items(MenuId::Eye).contains(&Item::EyeAct));
    let b = ui.dlg_layout().unwrap().rows.into_iter().find(|row| row.it == Item::EyeAct).unwrap().ctrl;
    for phase in [Phase::Down, Phase::Up] {
        ui.handle(Ev::Touch { id: 0, phase, x: b.x + b.w / 2, y: b.y + b.h / 2 });
    }
    assert_eq!(ui.m.view, AView::Eye);
    assert!(ui.m.eye_from_record);
    assert_eq!(ui.m.eye_place, EyePlace::Side);
    assert!((ui.m.eye_rate - CLEAN.rate).abs() / CLEAN.rate < 0.02, "rate {}", ui.m.eye_rate);
    ui.render(&mut Canvas::new(W, H));
    let (wave, eye) = (ui.grid, ui.eye_pane_rect());
    assert!(separated(wave, eye), "wave {:?} eye {:?}", wave, eye);
    assert!(eye.x >= wave.x + wave.w);
    assert!(wave.w > eye.w && eye.w > 250, "wave {:?} eye {:?}", wave, eye);
    let (x, y) = (eye.x + 20, eye.y + 12);
    ui.context_menu(x, y);
    let popup = ui.workspace.popup.as_ref().unwrap().rect;
    let popup_x = popup.x;
    let popup_y = popup.y;
    ui.tap(popup_x + 24, popup_y + 4 + 2 * 30 + 15); // Dock Eye below.
    ui.render(&mut Canvas::new(W, H));
    let (wave, eye) = (ui.grid, ui.eye_pane_rect());
    assert_eq!(ui.m.eye_place, EyePlace::Under);
    assert!(separated(wave, eye), "wave {:?} eye {:?}", wave, eye);
    assert!(eye.y >= wave.y + wave.h - 1);
    assert!(eye.w + 2 >= wave.w);
    let c = ui.eye_pane_close();
    ui.tap(c.x + 4, c.y + 4);
    assert_eq!(ui.m.view, AView::Scope);
    assert!(!ui.eye_open());
}

/// The colours come from the names in ui.rs, so a new look reaches this page.
#[test]
fn no_hard_coded_colours() {
    let src = [
        include_str!("../eyepage.rs"),
        include_str!("page.rs"),
        include_str!("update.rs"),
        include_str!("draw.rs"),
        include_str!("tests.rs"),
    ]
    .concat();
    let call = ["rgb", "("].concat();
    assert!(!src.contains(&call));
}

#[test]
fn the_column_slides_off_and_the_eye_takes_its_room() {
    let mut ui = page(CLEAN, 2);
    let home = layout(ui.eye_page_rect(), false);
    assert_eq!(placed(ui.eye_page_rect(), 1.0), home);
    let tab = ui.eye_tab();
    ui.tap(tab.x + 8, tab.y + 8);
    assert_eq!(ui.eye_col.to, 0.0);
    assert!(ui.eye_column_open() > 0.9, "{}", ui.eye_column_open());
    ui.now += SLIDE * 0.5;
    let mid = ui.eye_column_open();
    assert!(mid > 0.35 && mid < 0.65, "{}", mid);
    let l = placed(ui.eye_page_rect(), mid);
    assert!(l.charts[0].x > home.charts[0].x && (l.eye.w > home.eye.w || l.eye.h > home.eye.h), "{:?}", l);
    ui.now += SLIDE;
    assert!(ui.eye_column_open() < 0.001, "{}", ui.eye_column_open());
    let shut = placed(ui.eye_page_rect(), 0.0);
    assert_eq!(shut.eye.x + shut.eye.w, ui.eye_page_rect().x + ui.eye_page_rect().w);
    assert!(shut.charts[0].x >= ui.eye_page_rect().x + ui.eye_page_rect().w);
    if home.fig_rows > 0 {
        assert_eq!((shut.fig.x, shut.fig.w), (shut.eye.x, shut.eye.w));
    }
    let tab = ui.eye_tab();
    ui.tap(tab.x + 8, tab.y + 8);
    ui.now += SLIDE;
    assert!((ui.eye_column_open() - 1.0).abs() < 0.001, "{}", ui.eye_column_open());
    assert_eq!(placed(ui.eye_page_rect(), ui.eye_column_open()), home);
}

/// Hiding the column must not throw away the eye or the pooled edges.
#[test]
fn sliding_the_column_keeps_the_eye() {
    let mut ui = page(CLEAN, 5);
    let (n, pool, w) = (ui.azr.page.records, ui.azr.page.pool.len(), ui.azr.page.img_w);
    assert!(w > 0 && n > 1 && pool > 100, "{} {} {}", n, pool, w);
    let tab = ui.eye_tab();
    ui.tap(tab.x + tab.w / 2, tab.y + tab.h / 2);
    ui.now += SLIDE;
    ui.analyzer_update();
    assert!(
        ui.azr.page.records > 1 && ui.azr.page.pool.len() >= pool,
        "{} {}",
        ui.azr.page.records,
        ui.azr.page.pool.len()
    );
    assert_eq!(ui.azr.page.img_w, w);
    assert!(ui.eye_column_open() < 0.01);
}

#[test]
fn the_column_tab_stays_clear_of_the_overflow_button() {
    let mut ui = page(CLEAN, 1);
    for open in [0.0, 0.35, 1.0] {
        ui.eye_col.from = open;
        ui.eye_col.to = open;
        ui.eye_col.t = ui.now;
        let tab = ui.eye_tab();
        assert!(!overlaps(tab, ui.view_btn()), "open {} tab {:?} btn {:?}", open, tab, ui.view_btn());
        assert!(tab.w >= 40 && tab.h >= 40, "{:?}", tab);
        assert!(
            ui.eye_page_rect().contains(tab.x + 1, tab.y + 1)
                && ui.eye_page_rect().contains(tab.x + tab.w - 2, tab.y + tab.h - 2),
            "{:?}",
            tab
        );
    }
}

#[test]
fn a_tap_on_a_plot_fills_the_chart_area_and_a_second_tap_restores_the_four() {
    let mut ui = page(NOISY, 4);
    let home = layout(ui.eye_page_rect(), false);
    for k in 0..4 {
        let r = home.charts[k];
        ui.tap(r.x + r.w / 2, r.y + r.h / 2);
        assert_eq!(ui.eye_col.big, Some(k as u8));
        let (charts, plots) = charts_shown(&home, Some(k as u8));
        let span = home.charts[1].x + home.charts[1].w - home.charts[0].x;
        assert_eq!(charts[k].w, span);
        assert!(plots[k].w > home.plots[k].w);
        ui.tap(charts[k].x + 12, charts[k].y + 12);
        assert_eq!(ui.eye_col.big, None);
    }
}

/// With the column away, a tap where a plot used to be does not open one.
#[test]
fn a_hidden_column_has_no_plot_to_open() {
    let mut ui = page(CLEAN, 2);
    let home = layout(ui.eye_page_rect(), false);
    ui.eye_col.from = 0.0;
    ui.eye_col.to = 0.0;
    ui.eye_col.t = ui.now;
    let r = home.charts[2];
    ui.tap(r.x + 8, r.y + 8);
    assert_eq!(ui.eye_col.big, None);
}

#[test]
fn the_heat_scale_stays_clear_of_the_top_buttons() {
    let mut ui = page(CLEAN, 2);
    let home = layout(ui.eye_page_rect(), false);
    assert_eq!(ui.heat_x(home.plot), home.plot.x + home.plot.w - 128);
    ui.eye_col.from = 0.0;
    ui.eye_col.to = 0.0;
    ui.eye_col.t = ui.now;
    let r = placed(ui.eye_page_rect(), 0.0).plot;
    let kx = ui.heat_x(r);
    let scale = Rect::new(kx - 30, ui.eye_page_rect().y, 68 + ui.fonts().s.width("dense") + 34, 18);
    if ui.full_btn().0.x >= r.x {
        assert!(scale.x + scale.w <= ui.full_btn().0.x - 8, "{:?} {:?}", scale, ui.full_btn().0);
    }
    assert!(!overlaps(scale, ui.full_btn().0) && !overlaps(scale, ui.view_btn()), "{:?}", scale);
}

#[test]
fn it_draws_while_the_column_moves_and_a_plot_is_open() {
    let mut ui = page(NOISY, 4);
    let mut cv = Canvas::new(W, H);
    for (open, big) in [(1.0, None), (0.6, Some(1)), (0.25, Some(3)), (0.0, None)] {
        ui.eye_col.from = open;
        ui.eye_col.to = open;
        ui.eye_col.t = ui.now;
        ui.eye_col.big = big;
        ui.draw_eye(&mut cv);
    }
}

/// Pictures for a visual check: `OOS_SHOTS=<dir> cargo test eye_page_shots -- --ignored`.
#[test]
#[ignore]
fn eye_page_shots() {
    let dir = std::env::var("OOS_SHOTS").unwrap_or_else(|_| ".".into());
    let few = Sig { uis: 10, ..CLEAN };
    let save = |ui: &mut Ui, name: &str| {
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        ui.render(&mut cv);
        std::fs::write(format!("{}/{}.png", dir, name), crate::store::png_of(&cv)).unwrap();
    };
    for (name, s, n) in [
        ("eye-clean", CLEAN, 40),
        ("eye-noisy", NOISY, 60),
        ("eye-few", few, 6),
        ("eye-tiny", Sig { uis: 6, ..CLEAN }, 3),
    ] {
        let mut ui = page(s, n);
        save(&mut ui, name);
        ui.m.run = RunState::Stop;
    }
    let mut shown = page(NOISY, 40);
    save(&mut shown, "eye-column-shown");
    let tab = shown.eye_tab();
    shown.tap(tab.x + 8, tab.y + 8);
    shown.now += SLIDE;
    save(&mut shown, "eye-column-hidden");
    let mut open = page(NOISY, 40);
    let r = layout(open.eye_page_rect(), false).charts[3];
    open.tap(r.x + r.w / 2, r.y + r.h / 2);
    open.now += REFRESH;
    open.analyzer_update();
    save(&mut open, "eye-plot-expanded");
}
