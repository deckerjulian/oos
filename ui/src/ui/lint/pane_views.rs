// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Checks of the panes: the logger, the trend and the spectrum panes, their text and rows, and their pictures
//! (`pane_pictures`). A check of a pane goes here.

use super::*;
use std::collections::BTreeSet;

#[test]
fn a_formula_is_a_math_channel() {
    let mut u = with_two_channels();
    for _ in 0..24 {
        u.step(0.04);
    }
    u.m.math = MathOp::Expr;
    assert!(u.math_wave().is_empty(), "no formula yet");
    assert_eq!(u.value(Item::MathExprI), "none");
    u.activate(Item::MathExprEdit);
    u.wlan.osk.as_mut().unwrap().text = "CH1*(2+".into();
    u.osk_done();
    assert!(u.wlan.osk.is_some(), "a bad formula stays on the keyboard");
    assert!(u.m.math_expr[0].is_empty());
    u.wlan.osk.as_mut().unwrap().text = "2*CH1-CH2".into();
    u.osk_done();
    assert!(u.wlan.osk.is_none());
    let (rec, w) = (u.last[0].clone().unwrap(), u.math_wave());
    assert_eq!(w.len(), rec.ch[0].len().min(rec.ch[1].len()));
    assert!((w[5] - (2.0 * rec.ch[0][5] - rec.ch[1][5])).abs() < 1e-4);
    // a formula with no channel in it runs along the record
    u.m.math_expr[0] = "t".into();
    assert!((u.math_wave()[1] as f64 - rec.t0 - rec.dt).abs() < rec.dt * 1e-3 + 1e-9);
    u.m.math_expr[0] = "2*CH1-CH2".into();
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    let b = crate::store::parse_setup(&crate::store::setup_text(&u.m)).unwrap();
    assert_eq!((b.math, b.math_expr[0].as_str()), (MathOp::Expr, "2*CH1-CH2"));
    assert_eq!(u.items(MenuId::Math).iter().filter(|i| **i == Item::MathA).count(), 0);
}

#[test]
fn the_logger_writes_rows_at_its_interval() {
    let dir = crate::ui::testkit::Scratch::new("log");
    let mut u = with_two_channels();
    u.files.dir = dir.clone();
    u.m.meas = vec![];
    u.m.meas2 = vec![];
    u.adjust(Item::LogOn, 1);
    assert_eq!(u.value(Item::LogOn), "Off", "nothing to log");
    u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(1))];
    for _ in 0..3 {
        u.step(0.04);
    }
    u.adjust(Item::LogOn, 1);
    assert_eq!(u.value(Item::LogOn), "On");
    for _ in 0..130 {
        u.step(0.04);
    }
    assert!(u.value(Item::LogInfo).contains("rows"));
    u.adjust(Item::LogOn, 1);
    let name = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "csv"))
        .unwrap();
    let t = std::fs::read_to_string(&name).unwrap();
    let l: Vec<&str> = t.lines().collect();
    assert!(l[0].starts_with("Elapsed (s),Unix time (s),") && l[0].contains("CH2 (Hz)"), "{}", l[0]);
    assert!((5..=7).contains(&l.len()), "5.2 s at 1 s: {} lines", l.len());
    assert_eq!(l[1].split(',').count(), 4);
    assert!(l[1].split(',').nth(2).unwrap().parse::<f64>().is_ok());
    // a changed list ends the log, it no longer fits the head
    u.adjust(Item::LogOn, 1);
    u.step(1.1);
    u.m.meas.pop();
    u.step(1.1);
    assert!(u.log.file.is_none());
    assert!(u.notices.toast.as_ref().is_some_and(|t| t.0.contains("changed")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_trend_pane_follows_a_measurement() {
    let mut u = with_two_channels();
    u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(0))];
    u.m.pane = 4;
    for _ in 0..40 {
        u.step(0.04);
    }
    assert_eq!(u.pn.trend.len(), 2);
    assert!(u.pn.trend[0].pts.len() > 5);
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    // the first two of the list side by side; a tap moves a chart on
    assert_eq!(u.trend_charts(), [(0, 0), (1, 1)]);
    let r = u.pane_rect();
    assert!(u.pane_tap(r.x + 100, r.y + 40));
    assert_eq!(u.trend_charts(), [(0, 1), (1, 1)]);
    u.render(&mut cv);
    // a shorter list: the row and the trends follow
    u.m.meas.pop();
    u.mstat.clear();
    u.step(0.04);
    u.step(0.04);
    u.render(&mut cv);
    assert_eq!(u.pn.trend.len(), 1);
    u.m.meas.clear();
    u.render(&mut cv);
}

/// A record as the hardware delivers it: a 200 kHz square, 70 k points at 1.25 GSa/s behind the screen record.
fn square_on_ch1(u: &mut Ui) {
    let dt = 0.8e-9;
    let n = 70_000;
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5
    };
    let v: Vec<f32> = (0..n)
        .map(|i| if (i as f64 * dt * 200e3).rem_euclid(1.0) < 0.5 { 2.0 } else { 0.0 } + 4e-3 * noise())
        .collect();
    u.m.tscale = tscales().iter().position(|&x| (x - 5e-6).abs() < 1e-9).unwrap();
    u.m.meas = vec![(Meas::Freq, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(0))];
    u.m.run = RunState::Stop;
    u.step(0.04);
    let mut r = u.last[0].clone().expect("a record");
    r.dt = dt;
    r.t0 = -28e-6;
    r.ch[0] = v;
    u.last[0] = Some(r);
    u.measure_last();
}

/// The list as the table shows it, in Hz.
fn peak_hz(u: &Ui) -> Vec<f64> {
    u.azr.peaks.iter().map(|p| p.f).collect()
}

#[test]
fn the_pane_follows_a_square_wave_and_finds_its_harmonics() {
    let mut u = crate::ui::testkit::demo_ui();
    u.m.pane = tables::PANE_SPECTRUM;
    u.step(0.04);
    square_on_ch1(&mut u);
    u.relayout();
    u.analyzer_update();
    let sp = u.azr.spec.as_ref().unwrap();
    assert_eq!(sp.freq(sp.dbv.len() - 1).round(), 2.5e6);
    assert!(sp.rbw < 35e3, "RBW {}", sp.rbw);
    assert_eq!(u.value(Item::SpecSpan), "Auto 2.5 MHz");
    let f = peak_hz(&u);
    assert!(f.len() >= 6 && (f[0] - 200e3).abs() < 1e3, "{:?}", f);
    for (k, x) in f.iter().take(6).enumerate() {
        assert!((x / ((2 * k + 1) as f64 * 200e3) - 1.0).abs() < 0.01, "{:?}", f);
    }
    // the user's span wins; the full view keeps the whole band and still finds the fundamental
    u.m.spec_span = 1e6;
    u.m.spec_center = 5e5;
    u.analyzer_update();
    assert_eq!(u.azr.spec.as_ref().unwrap().freq(0), 0.0);
    assert!(u.azr.spec.as_ref().unwrap().freq(u.azr.spec.as_ref().unwrap().dbv.len() - 1) <= 1e6 + 1.0);
    u.m.spec_span = 0.0;
    u.m.view = AView::Spectrum;
    u.relayout();
    u.analyzer_update();
    // the full view follows the signal too, All shows the whole band
    assert!(u.value(Item::SpecSpan).starts_with("Auto"), "{}", u.value(Item::SpecSpan));
    assert!((u.strongest_line().unwrap().f - 200e3).abs() < 1e3);
    u.m.spec_harm = PANE_HARMS.iter().position(|&h| h == 0).unwrap();
    u.analyzer_update();
    assert_eq!(u.value(Item::SpecSpan), "Full");
    assert!((u.strongest_line().unwrap().f - 200e3).abs() < 1e3);
}

#[test]
fn the_sort_button_works_by_touch_in_pane_and_view() {
    let mut u = crate::ui::testkit::demo_ui();
    u.m.pane = tables::PANE_SPECTRUM;
    u.step(0.04);
    square_on_ch1(&mut u);
    u.relayout();
    u.analyzer_update();
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    let by_f = peak_hz(&u);
    assert!(by_f.windows(2).all(|w| w[0] < w[1]));
    let b = u.spec_box(u.pane_rect(), true).sort;
    u.tap(b.x + 5, b.y + 5);
    assert_eq!(u.m.spec_sort, 1);
    assert!(u.azr.peaks.windows(2).all(|w| w[0].dbv >= w[1].dbv));
    // the view: under the overflow button, which stays free
    u.m.view = AView::Spectrum;
    u.relayout();
    u.analyzer_update();
    u.render(&mut cv);
    let b = u.spec_box(u.plot.area, false);
    assert!(
        !overlaps(b.sort, u.view_btn())
            && !overlaps(b.list, u.view_btn())
            && !overlaps(b.plot, u.view_btn())
            && !overlaps(b.title, u.view_btn())
    );
    u.tap(b.sort.x + 5, b.sort.y + 5);
    assert_eq!(u.m.spec_sort, 0);
    let v = u.view_btn();
    u.tap(v.x + 5, v.y + 5);
    assert!(u.menu_open && u.menu == MenuId::ViewMore);
    // the close button of the pane too
    let mut u = crate::ui::testkit::demo_ui();
    u.m.pane = tables::PANE_SPECTRUM;
    u.step(0.04);
    u.relayout();
    let r = u.pane_rect();
    assert!(!overlaps(u.spec_box(r, true).sort, Ui::pane_close(r)));
    u.tap(r.x + r.w - 30, r.y + 12);
    assert_eq!(u.m.pane, 0);
}

/// Everything the panes write stays inside its pane, and in the width it was given.
#[test]
fn pane_text_fits() {
    let f = crate::font::Fonts::get(0);
    // the widest cell against the narrowest column, with the side column open
    let x = panes::tests_result_cols(1024 - 308 - 16);
    let w = |k: usize| x[k + 1] - x[k] - 8;
    assert!(f.sb.width("CH1-2") <= w(0));
    for &k in ALL_MEAS.iter() {
        assert!(f.s.width(k.name()) <= w(1), "{}", k.name());
    }
    for (n, ..) in tools::PAIRS {
        assert!(f.s.width(&format!("{} 1-2", n)) <= x[2] - x[1] - 6, "{}", n);
    }
    for k in 2..7 {
        assert!(f.sb.width("-1.250 mV") <= w(k) && f.sb.width("199.9 kHz") <= w(k));
    }
    assert!(f.s.width("12.34 M") <= w(7));
    // the peak list
    assert!(f.sb.width("625.00 MHz") + f.s.width("-123.4 dBV") + 26 <= 206);
    // a trend card
    assert!(f.sb.width("-1.250 mV") + f.s.width("Max") + 16 <= 128);
}

/// A pane uses its height: a short list makes it lower and the plot larger, and the rows reach its end.
#[test]
fn panes_leave_no_empty_rows() {
    let mut u = crate::ui::testkit::demo_ui();
    u.m.pane = tables::PANE_RESULTS;
    u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0))];
    u.m.ch[0].on = false;
    u.step(0.04);
    u.relayout();
    let few = u.pane_h();
    let plot = u.plot.grid.h;
    let (fit, pitch) = Ui::results_fit(u.pane_rect(), 1);
    assert_eq!(fit, 1);
    assert!(u.pane_rect().h - 40 - pitch < 4);
    for &k in ALL_MEAS.iter().take(12) {
        u.m.meas.push((k, WaveId::Ch(0)));
    }
    u.relayout();
    assert!(u.pane_h() > few && u.plot.grid.h < plot);
    let (fit, pitch) = Ui::results_fit(u.pane_rect(), 13);
    assert!(fit < 13 && u.pane_rect().h - 40 - fit as i32 * pitch < 8, "{} rows of {}", fit, pitch);
    // nothing to show: a line of text, not a black box
    u.m.meas.clear();
    u.relayout();
    assert!(u.pane_h() <= 70);
    u.m.pane = tables::PANE_TREND;
    u.relayout();
    assert!(u.pane_h() <= 70);
    u.m.pane = tables::PANE_EVENTS;
    u.relayout();
    assert!(u.pane_h() <= 70);
}

#[test]
fn a_long_results_list_scrolls_by_touch() {
    let mut u = crate::ui::testkit::demo_ui();
    u.m.pane = tables::PANE_RESULTS;
    u.m.meas = ALL_MEAS.iter().take(14).map(|&k| (k, WaveId::Ch(0))).collect();
    for _ in 0..3 {
        u.step(0.04);
    }
    u.relayout();
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    let r = u.pane_rect();
    let (fit, _) = Ui::results_fit(r, 14);
    // the scroll bar: the thumb to the bottom is the end of the list, a tap at the top of the track the start
    let (bar, _) = u.results_bar().expect("Results pane");
    assert!(bar.needed());
    let th = bar.thumb();
    crate::cmd::run(
        &mut u,
        &format!("drag {} {} {} {}", th.x + th.w / 2, th.y + 5, th.x + th.w / 2, bar.track.y + bar.track.h + 40),
    );
    assert_eq!(u.pn.top, 14 - fit);
    u.render(&mut cv);
    crate::cmd::run(&mut u, &format!("drag {0} {1} {0} {1}", bar.track.x + bar.track.w / 2, bar.track.y + 2));
    assert_eq!(u.pn.top, 0, "a press on the track jumps there");
    // a finger up the table goes down the list
    crate::cmd::run(&mut u, &format!("drag 300 {} 300 {}", r.y + r.h - 10, r.y + 50));
    assert!(u.pn.top > 0);
    // still there after the list got shorter
    u.m.meas.truncate(2);
    u.relayout();
    u.render(&mut cv);
}

#[test]
fn the_results_pane_shows_cursors_and_peaks() {
    let mut u = crate::ui::testkit::demo_ui();
    u.m.pane = tables::PANE_RESULTS;
    u.m.cursor = CursorMode::Time;
    u.step(0.04);
    square_on_ch1(&mut u);
    u.relayout();
    u.analyzer_update();
    assert!(!u.azr.peaks.is_empty());
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    // the pane's own close button still works
    let r = u.pane_rect();
    u.tap(r.x + r.w - 30, r.y + 12);
    assert_eq!(u.m.pane, 0);
}

#[test]
fn a_trend_scale_holds_on_noise_and_widens_for_a_step() {
    let mut t = panes::Trend::default();
    let mut seed = 1u32;
    let mut now = 0.0;
    let mut scales = BTreeSet::new();
    let mut run = |t: &mut panes::Trend, now: &mut f64, level: f64, secs: f64, scales: &mut BTreeSet<String>| {
        for _ in 0..(secs / 0.05) as usize {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = (seed >> 8) as f64 / (1u32 << 24) as f64 - 0.5;
            t.push(*now, level + noise * 10.0);
            *now += 0.05;
            if *now > 3.0 {
                scales.insert(format!("{:?}", t.scale));
            }
        }
    };
    run(&mut t, &mut now, 1000.0, 20.0, &mut scales);
    assert!(scales.len() <= 3, "{:?}", scales);
    let (lo, hi, tick) = t.scale.unwrap();
    assert!(lo < 995.0 && hi > 1005.0 && hi - lo < 60.0 && tick > 0.0, "{:?}", t.scale);
    // one stray value does not set the scale
    t.push(now, 1e6);
    now += 0.2;
    run(&mut t, &mut now, 1000.0, 2.0, &mut scales);
    assert!(t.scale.unwrap().1 < 2000.0, "{:?}", t.scale);
    // a step does, at once
    run(&mut t, &mut now, 2000.0, 1.0, &mut scales);
    assert!(t.scale.unwrap().1 > 2000.0, "{:?}", t.scale);
}

#[test]
fn a_trend_keeps_its_length_and_reaches_back() {
    let mut t = panes::Trend::default();
    for i in 0..200_000 {
        t.push(i as f64 * 0.05, 1.0);
    }
    assert!(t.pts.len() <= 480);
    let age = t.pts.back().unwrap().t - t.pts.front().unwrap().t;
    assert!(age > 9000.0, "{} s", age);
    // a gap stays a gap
    let mut t = panes::Trend::default();
    for i in 0..40 {
        t.push(i as f64 * 0.05, if i < 20 { 1.0 } else { f64::NAN });
    }
    assert!(t.pts.iter().any(|p| p.v.is_nan()) && t.range == Some((1.0, 1.0)));
}

#[test]
fn trend_charts_are_chosen_in_the_display_menu() {
    let mut u = with_two_channels();
    u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(0)), (Meas::Vavg, WaveId::Ch(1))];
    assert!(!u.items(MenuId::Display).contains(&Item::TrendSel(0)));
    u.m.pane = tables::PANE_TREND;
    assert!(u.items(MenuId::Display).contains(&Item::TrendSel(1)));
    assert_eq!(u.value(Item::TrendSel(0)), "Auto");
    u.adjust(Item::TrendSel(1), 1);
    assert_eq!(u.value(Item::TrendSel(1)), "Off");
    assert_eq!(u.trend_charts(), [(0, 0)]);
    u.adjust(Item::TrendSel(1), 3);
    assert_eq!(u.value(Item::TrendSel(1)), "CH2 Vavg".replace("Vavg", Meas::Vavg.name()));
    assert_eq!(u.trend_charts(), [(0, 0), (1, 2)]);
    // the list gets shorter: back to the default
    u.m.meas.truncate(1);
    assert_eq!(u.trend_charts(), [(0, 0)]);
    assert_eq!(u.value(Item::TrendSel(1)), "Auto");
}

/// Pictures of the panes for looking at: `OOS_PANES=dir cargo test pane_pictures -- --ignored`.
#[test]
#[ignore]
fn pane_pictures() {
    let Ok(dir) = std::env::var("OOS_PANES") else { return };
    let dir = std::path::PathBuf::from(dir);
    let shot = |u: &mut Ui, n: &str| {
        u.relayout();
        u.analyzer_update();
        let mut cv = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut cv);
        let _ = crate::store::write(&dir.join(format!("{}.png", n)), &crate::store::png_of(&cv));
    };
    let square = |pane: usize, f: &dyn Fn(&mut Ui)| {
        let mut u = crate::ui::testkit::demo_ui();
        u.m.pane = pane;
        u.step(0.04);
        square_on_ch1(&mut u);
        f(&mut u);
        u
    };
    let mut u = square(1, &|_| {});
    shot(&mut u, "spectrum-pane");
    let mut u = square(1, &|u| u.m.spec_sort = 1);
    shot(&mut u, "spectrum-pane-by-level");
    let mut u = square(0, &|u| u.m.view = AView::Spectrum);
    shot(&mut u, "spectrum-view-full-band");
    let mut u = square(0, &|u| {
        u.m.view = AView::Spectrum;
        u.m.spec_span = 2.5e6;
        u.m.spec_center = 1.25e6;
        u.m.spec_dist = true;
    });
    shot(&mut u, "spectrum-view-span");
    let mut u = square(0, &|u| {
        u.m.view = AView::Spectrum;
        u.m.spec_span = 2.5e6;
        u.m.spec_center = 1.25e6;
        u.m.spec_wf = true;
    });
    for _ in 0..10 {
        u.analyzer_update();
    }
    shot(&mut u, "spectrum-view-waterfall");
    let mut u = square(2, &|_| {});
    shot(&mut u, "results-two-rows");
    let mut u = square(2, &|u| {
        u.m.spec_src = WaveId::Ch(1);
    });
    shot(&mut u, "results-table-only");
    let live = |pane: usize, f: &dyn Fn(&mut Ui), secs: usize| {
        let mut u = crate::ui::testkit::demo_ui();
        u.m.pane = pane;
        u.m.ch[1].on = true;
        f(&mut u);
        for _ in 0..secs * 25 {
            u.step(0.04);
        }
        u
    };
    let mut u = live(
        2,
        &|u| {
            u.m.cursor = CursorMode::Track;
            u.m.meas = ALL_MEAS.iter().take(12).map(|&k| (k, WaveId::Ch(0))).collect();
            u.m.meas2 = vec![(0, WaveId::Ch(0), WaveId::Ch(1)), (4, WaveId::Ch(0), WaveId::Ch(1))];
        },
        2,
    );
    shot(&mut u, "results-many");
    // a protocol module's own view (the example module, compiled in the tests)
    let mut u = live(
        tables::PANE_BUS,
        &|u| {
            u.set_dec_proto(Proto::from_key("Example").unwrap());
            u.m.dec_on = true;
            u.m.tscale = tscales().iter().position(|&x| (x - 1e-3).abs() < 1e-6).unwrap();
        },
        1,
    );
    shot(&mut u, "bus-view");
    let mut u = live(tables::PANE_BUS, &|_| {}, 1);
    shot(&mut u, "bus-view-empty");
    // DCC: the demo layout, collected over two seconds of records
    let mut u = live(
        0,
        &|u| {
            u.set_dec_proto(Proto::from_key("Dcc").unwrap());
            u.m.dec_on = true;
            u.m.tscale = tscales().iter().position(|&x| (x - 2e-3).abs() < 1e-6).unwrap();
        },
        3,
    );
    shot(&mut u, "bus-view-dcc");
    let mut u = live(
        2,
        &|u| u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(1)), (Meas::Vrms, WaveId::Ch(1))],
        2,
    );
    shot(&mut u, "results-three");
    let mut u = live(
        2,
        &|u| {
            u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0))];
            u.m.ch[0].on = true;
            u.m.spec_src = WaveId::Ch(3);
        },
        2,
    );
    shot(&mut u, "results-one-no-side");
    let mut u = live(4, &|u| u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(1))], 40);
    shot(&mut u, "trend-two");
    let mut u = live(4, &|u| u.m.meas = vec![(Meas::Vrms, WaveId::Ch(1))], 15);
    shot(&mut u, "trend-one");
    let mut u = live(
        3,
        &|u| {
            u.m.ch[2].on = true;
            u.m.dec_on = true;
            u.m.dec_src = 2;
        },
        2,
    );
    shot(&mut u, "events");
    let mut u = live(4, &|_| {}, 1);
    u.m.meas.clear();
    shot(&mut u, "trend-empty");
}
