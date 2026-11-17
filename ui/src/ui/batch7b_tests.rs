// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 7b: per-row measurement settings (pair and catalogue rows too), the per-measurement settings card in the Studio
//! sheet and the Minimal / Default dialog, statistics population, and the eye / power cards.
//! OOS_SHOTS=<dir> writes pictures.
use super::*;
use crate::model::{RowCfg, RowKey};
use oos_dsp::measure::{self, BaseTop, Meas};

fn fresh(l: Layout) -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = l;
    u.m.ch[1].on = true;
    if l == Layout::Studio { u.studio.enter = true; }
    for _ in 0..10 { u.step(0.05); }
    u
}

fn shot(u: &mut Ui, name: &str) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    for _ in 0..8 { u.step(0.05); }
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn tap_in(u: &mut Ui, r: Rect) { u.tap(r.x + r.w / 2, r.y + r.h / 2); }

/// A record of pulses whose cycles are `cycles` samples long (low first, then high), delayed by `delay` low samples.
fn pulses(cycles: &[usize], delay: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; delay + 10];
    for &c in cycles {
        v.extend(std::iter::repeat(0.0).take(c / 2));
        v.extend(std::iter::repeat(1.0).take(c - c / 2));
    }
    v.extend(std::iter::repeat(0.0).take(30));
    v
}

fn put(u: &mut Ui, slot: usize, v: Vec<f32>) {
    u.refs[slot] = Some(RefWave { src: 0, color: slot, label: format!("R{}", slot + 1), t0: 0.0, dt: 1e-9, v: v.into(), vpd: 1.0, offset: 0.0, on: true });
}

fn bench() -> Ui {
    let mut u = fresh(Layout::Default);
    u.m.run = RunState::Stop;
    u.m.meas_full_record = true;
    u.m.meas.clear(); u.m.meas2.clear(); u.m.cat.clear(); u.m.row_cfg.clear();
    u.meas_own.clear();
    // cycles of 40, 60, 80, 100, 120 samples
    put(&mut u, 0, pulses(&[40, 60, 80, 100, 120], 0));
    put(&mut u, 1, pulses(&[40, 60, 80, 100, 120], 7));
    u
}

fn analysis(v: &[f32]) -> measure::Analysis { measure::analyze_min(v, 1e-9, 0.0) }

#[test]
fn a_row_follows_the_shared_settings_until_one_of_its_fields_is_set() {
    let mut u = bench();
    u.m.meas = vec![(Meas::Period, WaveId::Ref(0)), (Meas::Freq, WaveId::Ref(0))];
    let all = u.meas_number(WaveId::Ref(0), Meas::Period);
    assert!((all - 80e-9).abs() < 1e-10, "{all}");
    // cycle offset 2: the rising periods 90 and 110
    let key = RowKey::One(Meas::Period, WaveId::Ref(0));
    u.m.set_row_cfg(key, RowCfg { off: Some(2), ..Default::default() });
    let skipped = u.meas_number(WaveId::Ref(0), Meas::Period);
    assert!((skipped - 100e-9).abs() < 1e-10, "{skipped}");
    // the other row on the same source is untouched
    let f = u.meas_number(WaveId::Ref(0), Meas::Freq);
    assert!((f - u.meas_at(WaveId::Ref(0), Meas::Freq)).abs() < 1e-3);
    // rising periods are 50, 70, 90, 110; every second one: 50 and 90
    u.m.set_row_cfg(key, RowCfg { step: Some(2), ..Default::default() });
    let stepped = u.meas_number(WaveId::Ref(0), Meas::Period);
    assert!((stepped - 70e-9).abs() < 1e-10, "{stepped}");
    u.m.set_row_cfg(key, RowCfg { off: Some(1), step: Some(2), ..Default::default() });
    assert!((u.meas_number(WaveId::Ref(0), Meas::Period) - 90e-9).abs() < 1e-10, "70 and 110");
    // all fields back to follow: the shared value, and no entry kept
    u.m.set_row_cfg(key, RowCfg::default());
    assert!(u.m.row_cfg.is_empty());
    assert_eq!(u.meas_number(WaveId::Ref(0), Meas::Period), all);
    // the rows list shows the row's own value
    u.m.set_row_cfg(key, RowCfg { pol: Some(2), off: Some(3), ..Default::default() });
    assert!((u.meas_rows()[0].v - 120e-9).abs() < 1e-10, "{}", u.meas_rows()[0].v);
}

#[test]
fn a_row_can_set_its_own_levels_base_top_and_outside_level() {
    let mut u = bench();
    // a spike above the high level in the first high span
    let mut v = pulses(&[200, 200, 200], 0);
    v[110] = 1.5;
    v[50] = -0.3;
    put(&mut u, 0, v);
    u.m.meas = vec![(Meas::Vtop, WaveId::Ref(0)), (Meas::Vbase, WaveId::Ref(0)), (Meas::Vamp, WaveId::Ref(0))];
    let auto = u.meas_number(WaveId::Ref(0), Meas::Vtop);
    assert!((auto - 1.0).abs() < 0.05, "{auto}");
    u.m.set_row_cfg(RowKey::One(Meas::Vtop, WaveId::Ref(0)), RowCfg { base_top: Some(BaseTop::MinMax), ..Default::default() });
    assert!((u.meas_number(WaveId::Ref(0), Meas::Vtop) - 1.5).abs() < 1e-6);
    assert!((u.meas_number(WaveId::Ref(0), Meas::Vamp) - auto).abs() < 0.05, "the other rows keep the shared method");
    // overshoot against a manual reference
    u.m.meas = vec![(Meas::Overshoot, WaveId::Ref(0))];
    let shared = u.meas_number(WaveId::Ref(0), Meas::Overshoot);
    u.m.set_row_cfg(RowKey::One(Meas::Overshoot, WaveId::Ref(0)), RowCfg { os_top: Some(1.0), os_base: Some(0.0), ..Default::default() });
    let manual = u.meas_number(WaveId::Ref(0), Meas::Overshoot);
    assert!((manual - 50.0).abs() < 0.1, "{manual} vs {shared}");
    // time outside its own volts: below 0.5 V is the whole low half; above 1.2 V is only the spike
    u.m.meas = vec![(Meas::Outside, WaveId::Ref(0))];
    let key = RowKey::One(Meas::Outside, WaveId::Ref(0));
    u.m.set_row_cfg(key, RowCfg { out_volts: Some(1.2), side: Some(0), ..Default::default() });
    let spike = u.meas_number(WaveId::Ref(0), Meas::Outside);
    assert!(spike.is_finite() && spike < 5e-9, "{spike}");
    u.m.set_row_cfg(key, RowCfg { out_volts: Some(0.5), side: Some(0), ..Default::default() });
    let wide = u.meas_number(WaveId::Ref(0), Meas::Outside);
    assert!(wide > 50e-9, "{wide}");
}

#[test]
fn burst_and_n_periods_rows_keep_their_own_counting() {
    let mut u = bench();
    u.m.meas = vec![(Meas::NPeriods, WaveId::Ref(0))];
    let key = RowKey::One(Meas::NPeriods, WaveId::Ref(0));
    let base = u.meas_number(WaveId::Ref(0), Meas::NPeriods);
    u.m.set_row_cfg(key, RowCfg { n: Some(1), ..Default::default() });
    let one = u.meas_number(WaveId::Ref(0), Meas::NPeriods);
    assert!((one - 80e-9).abs() < 1e-10, "{one} {base}");
    u.m.set_row_cfg(key, RowCfg { n: Some(1), n_off: Some(2), ..Default::default() });
    assert!((u.meas_number(WaveId::Ref(0), Meas::NPeriods) - 100e-9).abs() < 1e-10);
    u.m.set_row_cfg(key, RowCfg { n: Some(1), n_edge: Some(1), ..Default::default() });
    assert!(u.meas_number(WaveId::Ref(0), Meas::NPeriods).is_finite());
}

#[test]
fn a_pair_row_has_its_own_edges_direction_and_window() {
    let mut u = bench();
    // pair 0: delay rise-rise; R2 is R1 seven samples later
    u.m.meas2 = vec![(0, WaveId::Ref(0), WaveId::Ref(1))];
    let (a, b) = (analysis(&pulses(&[40, 60, 80, 100, 120], 0)), analysis(&pulses(&[40, 60, 80, 100, 120], 7)));
    let rr = u.pair_at(0, WaveId::Ref(0), WaveId::Ref(1));
    assert!((rr - 7e-9).abs() < 1e-10, "{rr}");
    let key = RowKey::Pair(0, WaveId::Ref(0), WaveId::Ref(1));
    // from a rise to a fall of the other source
    u.m.set_row_cfg(key, RowCfg { to: Some(1), ..Default::default() });
    let rf = u.pair_at(0, WaveId::Ref(0), WaveId::Ref(1));
    let want = measure::delay_opts(&a, &b, &measure::PairOpts { from: measure::EdgeSel::Rise, to: measure::ToSel::Fall, dir: measure::SearchDir::First, min: None, max: None });
    assert_eq!(rf.to_bits(), want.to_bits());
    assert!(rf > 7e-9 + 10e-9, "{rf}");
    // the last edges, and the same / opposite direction from both
    u.m.set_row_cfg(key, RowCfg { from: Some(2), to: Some(4), dir: Some(1), ..Default::default() });
    let last = u.pair_at(0, WaveId::Ref(0), WaveId::Ref(1));
    let want = measure::delay_opts(&a, &b, &measure::PairOpts { from: measure::EdgeSel::Both, to: measure::ToSel::Opposite, dir: measure::SearchDir::Last, min: None, max: None });
    assert_eq!(last.to_bits(), want.to_bits());
    // a window that excludes the 7 ns delay
    u.m.set_row_cfg(key, RowCfg { wmin: Some(20e-9), ..Default::default() });
    let windowed = u.pair_at(0, WaveId::Ref(0), WaveId::Ref(1));
    assert!(windowed.is_nan() || windowed >= 20e-9, "{windowed}");
    // back to the defaults: the first value again (the cache key includes the pairing)
    u.m.set_row_cfg(key, RowCfg::default());
    assert_eq!(u.pair_at(0, WaveId::Ref(0), WaveId::Ref(1)).to_bits(), rr.to_bits());
}

#[test]
fn the_four_list_rows_map_to_from_and_to() {
    // the existing rows keep their meaning: Delay fall-rise is from Fall to Rise
    for (i, (_, kind, ea, eb)) in tools::PAIRS.iter().enumerate() {
        let u = bench();
        let o = u.pair_opts(i, WaveId::Ref(0), WaveId::Ref(1));
        match kind {
            tools::PairKind::Delay | tools::PairKind::Phase | tools::PairKind::Skew => {
                assert_eq!(o.from, if *ea { measure::EdgeSel::Rise } else { measure::EdgeSel::Fall }, "{i}");
                assert_eq!(o.to, if *eb { measure::ToSel::Rise } else { measure::ToSel::Fall }, "{i}");
            }
            tools::PairKind::SetupBoth | tools::PairKind::HoldBoth | tools::PairKind::SetupAndHoldBoth => assert_eq!(o.from, measure::EdgeSel::Both),
            _ => assert_eq!((o.from, o.to), (if *ea { measure::EdgeSel::Rise } else { measure::EdgeSel::Fall }, measure::ToSel::Both)),
        }
        assert_eq!((o.dir, o.min, o.max), (measure::SearchDir::First, None, None));
    }
}

#[test]
fn row_settings_are_saved_in_the_setup_and_only_for_rows_that_remain() {
    let mut u = bench();
    u.m.meas = vec![(Meas::Period, WaveId::Ref(0))];
    u.m.meas2 = vec![(0, WaveId::Ref(0), WaveId::Ref(1))];
    let one = RowCfg { pol: Some(2), off: Some(3), step: Some(2), base_top: Some(BaseTop::Mean), hyst: Some(0.1), os_top: Some(1.25), ..Default::default() };
    let pair = RowCfg { from: Some(2), to: Some(4), dir: Some(1), wmin: Some(-2e-9), wmax: Some(5e-9), ..Default::default() };
    u.m.set_row_cfg(RowKey::One(Meas::Period, WaveId::Ref(0)), one);
    u.m.set_row_cfg(RowKey::Pair(0, WaveId::Ref(0), WaveId::Ref(1)), pair);
    // a row that is gone is not kept
    u.m.set_row_cfg(RowKey::One(Meas::Vpp, WaveId::Ch(0)), RowCfg { off: Some(1), ..Default::default() });
    let text = crate::store::setup_text(&u.m);
    assert!(text.contains("row_cfg = m:Period:R1|"), "{text}");
    assert!(!text.contains("m:Vpp"));
    let back = crate::store::parse_setup(&text).unwrap();
    assert_eq!(back.row_cfg.len(), 2);
    assert_eq!(back.row_cfg(RowKey::One(Meas::Period, WaveId::Ref(0))), Some(one));
    assert_eq!(back.row_cfg(RowKey::Pair(0, WaveId::Ref(0), WaveId::Ref(1))), Some(pair));
    // a bad entry is refused, an old setup has none
    assert!(crate::store::parse_setup("row_cfg = m:Period:R1|nonsense=1\n").is_err());
    assert!(crate::store::parse_setup("row_cfg = m:NotAMeasurement:1|off=1\n").is_err());
    assert!(crate::store::parse_setup("meas = Vpp:1\n").unwrap().row_cfg.is_empty());
}

#[test]
fn the_gate_of_a_row_is_its_own() {
    let mut u = bench();
    u.m.meas_full_record = true;
    u.m.meas = vec![(Meas::Vtop, WaveId::Ref(0))];
    let shared = u.meas_number(WaveId::Ref(0), Meas::Vtop);
    assert!(u.own_params(Meas::Vtop, WaveId::Ref(0)).is_none(), "a row that follows needs no analysis of its own");
    u.m.set_row_cfg(RowKey::One(Meas::Vtop, WaveId::Ref(0)), RowCfg { gate_full: Some(false), ..Default::default() });
    let own = u.own_params(Meas::Vtop, WaveId::Ref(0)).expect("a gate is an analysis setting");
    assert_eq!(own.cfg.gate_full, Some(false));
    // the screen gate of this record covers part of it at the default time base; the value is still a level
    assert!(u.meas_number(WaveId::Ref(0), Meas::Vtop).is_finite() && shared.is_finite());
    assert!(u.m.meas_full_record && u.row_gate.get().is_none(), "the gate override does not leak out of a reading");
    assert_eq!(u.gate_full(), true);
}

// ------------------------------------------------------------------------------------------------ the card

use super::measrow::RF;
use super::studio::StudioView;
use super::sheet::{RowKind, SHEET_MAX_ROWS, SHEET_ROW};

fn card(l: Layout, rows: &[(Meas, WaveId)], pairs: &[(usize, WaveId, WaveId)]) -> Ui {
    let mut u = fresh(l);
    u.m.run = RunState::Stop;
    u.m.meas_full_record = true;
    u.m.meas = rows.to_vec();
    u.m.meas2 = pairs.to_vec();
    u.m.cat.clear();
    u.m.row_cfg.clear();
    u.meas_own.clear();
    put(&mut u, 0, pulses(&[40, 60, 80, 100, 120], 0));
    put(&mut u, 1, pulses(&[40, 60, 80, 100, 120], 7));
    u.studio.panel.first = 0;
    u
}

fn rows_of(u: &Ui) -> Vec<Item> { u.measrow_plan().into_iter().flat_map(|(_, r)| r).collect() }

#[test]
fn every_group_has_a_card_of_at_most_seven_rows_per_tab() {
    let r0 = WaveId::Ref(0);
    let single = [Meas::Vpp, Meas::Vtop, Meas::Overshoot, Meas::Preshoot, Meas::Period, Meas::Freq, Meas::PWidth, Meas::PDuty, Meas::Rise, Meas::Fall,
        Meas::PSlew, Meas::HighTime, Meas::Burst, Meas::NPeriods, Meas::Outside];
    let rows: Vec<(Meas, WaveId)> = single.iter().map(|&k| (k, r0)).collect();
    let pairs: Vec<(usize, WaveId, WaveId)> = (0..tools::PAIRS.len()).map(|k| (k, r0, WaveId::Ref(1))).collect();
    let mut u = card(Layout::Studio, &rows, &pairs);
    for i in 0..u.measurement_count() {
        u.open_meas_cfg(i);
        assert_eq!(u.menu, MenuId::MeasRow);
        let spec = u.sheet_spec(MenuId::MeasRow).unwrap_or_else(|| panic!("row {i} has a card"));
        assert!(spec.tabs.len() <= 4 && spec.tabs.iter().all(|t| !t.rows.is_empty() && t.rows.len() <= SHEET_MAX_ROWS && t.foot == [Item::RowF(RF::Reset)]), "row {i}");
        assert!(u.sheet_active());
        let g = u.sheet_geo().unwrap();
        assert!(g.rows.iter().all(|r| r.row.h == SHEET_ROW));
        // every item answers its label, value and options
        for it in rows_of(&u) {
            assert!(!u.item_label(it).is_empty(), "{it:?}");
            let opts = u.options(it);
            if !opts.is_empty() { assert!(opts.contains(&u.value(it)), "row {i} {it:?}: {:?} in {opts:?}", u.value(it)); }
        }
        u.close_menu();
        assert!(u.meas_cfg.is_none());
    }
}

#[test]
fn the_card_content_follows_the_row() {
    let r0 = WaveId::Ref(0);
    let mut u = card(Layout::Studio, &[(Meas::Overshoot, r0), (Meas::Period, r0), (Meas::PWidth, r0), (Meas::Rise, r0), (Meas::Burst, r0), (Meas::NPeriods, r0), (Meas::Outside, r0)],
        &[(0, r0, WaveId::Ref(1)), (8, r0, WaveId::Ref(1)), (12, r0, WaveId::Ref(1))]);
    let has = |u: &Ui, f: RF| rows_of(u).contains(&Item::RowF(f));
    u.open_meas_cfg(0);
    assert!(has(&u, RF::OsRef) && has(&u, RF::BaseTop) && has(&u, RF::Gate) && !has(&u, RF::OsTop), "overshoot: reference Auto until Manual");
    u.adjust(Item::RowF(RF::OsRef), 1);
    assert!(has(&u, RF::OsTop) && has(&u, RF::OsBase));
    u.adjust(Item::RowF(RF::OsRef), 1);
    assert!(!has(&u, RF::OsTop) && u.m.row_cfg(RowKey::One(Meas::Overshoot, r0)).is_none(), "back to Auto removes the entry");
    u.open_meas_cfg(1);
    assert!(has(&u, RF::Pol) && has(&u, RF::Off) && has(&u, RF::Step));
    u.open_meas_cfg(2);
    assert!(!has(&u, RF::Pol) && has(&u, RF::Off), "a width has no edge polarity");
    u.open_meas_cfg(3);
    assert!(has(&u, RF::Low) && has(&u, RF::High) && has(&u, RF::Hyst) && has(&u, RF::Used));
    u.open_meas_cfg(4);
    assert!(has(&u, RF::Idle) && has(&u, RF::BurstPol));
    u.open_meas_cfg(5);
    assert!(has(&u, RF::N) && has(&u, RF::NOff) && has(&u, RF::NStep) && has(&u, RF::NEdge));
    u.open_meas_cfg(6);
    assert!(has(&u, RF::Side) && has(&u, RF::OutVolts));
    // pairs: delay has a direction, skew and setup do not; setup has no Same or Opposite
    u.open_meas_cfg(7);
    assert!(has(&u, RF::From) && has(&u, RF::To) && has(&u, RF::Dir) && has(&u, RF::WMin) && has(&u, RF::WMax));
    assert_eq!(u.options(Item::RowF(RF::To)), ["Rise", "Fall", "Both", "Same", "Opposite"]);
    u.open_meas_cfg(8);
    assert!(!has(&u, RF::Dir) && has(&u, RF::WMin));
    assert_eq!(u.value(Item::RowF(RF::WMax)), "10 ns");
    assert_eq!(u.value(Item::RowF(RF::WMin)), "-10 ns");
    u.open_meas_cfg(9);
    assert_eq!(u.options(Item::RowF(RF::To)), ["Rise", "Fall", "Both"]);
    assert_eq!(u.value(Item::RowF(RF::WMin)), "0 s");
}

#[test]
fn taps_on_the_sheet_change_the_row_and_the_reset_footer_follows_shared_again() {
    let r0 = WaveId::Ref(0);
    let mut u = card(Layout::Studio, &[(Meas::Period, r0)], &[(0, r0, WaveId::Ref(1))]);
    u.open_meas_cfg(0);
    let g = u.sheet_geo().unwrap();
    let off = g.rows.iter().find(|r| r.it == Item::RowF(RF::Off)).unwrap();
    // the + of a Number row
    u.tap(off.ctrl.right() - 8, off.ctrl.y + off.ctrl.h / 2);
    u.tap(off.ctrl.right() - 8, off.ctrl.y + off.ctrl.h / 2);
    assert_eq!(u.m.row_cfg(RowKey::One(Meas::Period, r0)).unwrap().off, Some(2));
    assert_eq!(u.value(Item::RowF(RF::Off)), "2");
    // a segment of a Choice row: Falling
    let g = u.sheet_geo().unwrap();
    let pol = g.rows.iter().find(|r| r.it == Item::RowF(RF::Pol)).unwrap();
    let w = pol.ctrl.w / 3;
    u.tap(pol.ctrl.x + w * 2 + 4, pol.ctrl.y + pol.ctrl.h / 2);
    assert_eq!(u.m.row_cfg(RowKey::One(Meas::Period, r0)).unwrap().pol, Some(2));
    // falling periods are 60, 80, 100, 120: offset 2 leaves 100 and 120
    assert!((u.meas_number(r0, Meas::Period) - 110e-9).abs() < 1e-10, "{}", u.meas_number(r0, Meas::Period));
    shot(&mut u, "b7b-test-period");
    // Follow shared in the footer
    let g = u.sheet_geo().unwrap();
    assert_eq!(g.foot.len(), 1);
    tap_in(&mut u, g.foot[0].1);
    assert!(u.m.row_cfg.is_empty());
    assert!((u.meas_number(r0, Meas::Period) - 80e-9).abs() < 1e-10);
    // a pair row through the sheet
    u.open_meas_cfg(1);
    u.adjust(Item::RowF(RF::To), 1);
    assert_eq!(u.m.row_cfg(RowKey::Pair(0, r0, WaveId::Ref(1))).unwrap().to, Some(1));
    // choosing the reading's own edge again follows it
    u.adjust(Item::RowF(RF::To), -1);
    assert!(u.m.row_cfg.is_empty());
    // the window steps from the reading's own and returns to it
    u.adjust(Item::RowF(RF::WMax), 1);
    assert!(u.m.row_cfg(RowKey::Pair(0, r0, WaveId::Ref(1))).is_some());
}

#[test]
fn minimal_and_default_open_the_card_as_a_dialog_with_forty_pixel_controls() {
    for l in [Layout::Minimal, Layout::Default] {
        let r0 = WaveId::Ref(0);
        let mut u = card(l, &[(Meas::Period, r0), (Meas::Rise, r0)], &[(0, r0, WaveId::Ref(1))]);
        for i in 0..3 {
            u.open_meas_cfg(i);
            assert_eq!(u.menu, MenuId::MeasRow);
            assert!(!u.sheet_active());
            let d = u.dlg_layout().expect("a dialog");
            assert!(d.close.h >= 40 && d.close.w >= 40, "{l:?} close {:?}", d.close);
            assert!(d.rows.iter().all(|r| r.ctrl.h >= 40), "{l:?} row {i}: controls are 40 px");
            assert_eq!(d.rows.len(), u.measrow_items().len());
            assert!(d.r.x >= 0 && d.r.right() <= W && d.r.y >= 0 && d.r.bottom() <= H);
        }
        shot(&mut u, &format!("b7b-test-dialog-{l:?}"));
        // the close button closes it and forgets the row
        let d = u.dlg_layout().unwrap();
        tap_in(&mut u, d.close);
        assert!(!u.menu_open && u.meas_cfg.is_none());
    }
}

#[test]
fn the_gear_paths_open_the_card_for_every_kind_of_row() {
    let r0 = WaveId::Ref(0);
    let mut u = card(Layout::Studio, &[(Meas::Vpp, r0)], &[(0, r0, WaveId::Ref(1))]);
    u.studio_set_view(StudioView::Docked);
    assert_eq!(u.studio_keys().len(), 2);
    u.studio_configure(1);
    assert_eq!(u.meas_cfg, Some(1));
    assert_eq!(u.menu, MenuId::MeasRow);
    assert_eq!(u.rowf_key(), Some(RowKey::Pair(0, r0, WaveId::Ref(1))));
    assert!(u.menu_title(MenuId::MeasRow).contains("settings"));
}

#[test]
fn pictures_of_the_cards() {
    if std::env::var_os("OOS_SHOTS").is_none() { return; }
    let r0 = WaveId::Ref(0);
    let r1 = WaveId::Ref(1);
    let kinds = [("amplitude", Meas::Overshoot), ("timing", Meas::Period), ("edges", Meas::Rise), ("burst", Meas::Burst), ("nperiods", Meas::NPeriods), ("outside", Meas::Outside)];
    let rows: Vec<(Meas, WaveId)> = kinds.iter().map(|k| (k.1, r0)).collect();
    let mut u = card(Layout::Studio, &rows, &[(0, r0, r1), (8, r0, r1), (12, r0, r1)]);
    u.m.row_cfg.clear();
    for (i, (name, _)) in kinds.iter().enumerate() {
        u.open_meas_cfg(i);
        shot(&mut u, &format!("b7b-card-{name}"));
    }
    for (i, name) in ["delay", "skew", "setup"].iter().enumerate() {
        u.open_meas_cfg(kinds.len() + i);
        shot(&mut u, &format!("b7b-card-{name}"));
    }
    u.open_meas_cfg(1);
    u.adjust(Item::RowF(RF::Off), 2);
    u.adjust(Item::RowF(RF::Pol), 1);
    let g = u.sheet_geo().unwrap();
    tap_in(&mut u, g.tabs[1]);
    shot(&mut u, "b7b-card-timing-levels");
    let mut m = card(Layout::Minimal, &[(Meas::NPeriods, r0), (Meas::Period, r0)], &[]);
    m.open_meas_cfg(0);
    shot(&mut m, "b7b-dialog-minimal");
    let mut d = card(Layout::Default, &[(Meas::Rise, r0)], &[(0, r0, r1)]);
    d.open_meas_cfg(1);
    shot(&mut d, "b7b-dialog-default");
}

// ------------------------------------------------------------------------------------------------ statistics

#[test]
fn the_population_decides_whether_a_new_acquisition_adds_to_the_history() {
    let mut acc = MStat::default();
    let mut cur = MStat::default();
    for v in [1.0, 2.0, 6.0] {
        acc.push(v, false);
        cur.push(v, true);
    }
    assert_eq!((acc.n, acc.sum, acc.min, acc.max), (3, 9.0, 1.0, 6.0));
    assert_eq!((cur.n, cur.sum, cur.min, cur.max), (1, 6.0, 6.0, 6.0), "current is the latest acquisition only");
}

#[test]
fn statistics_settings_are_a_page_with_a_reset_and_follow_the_toggles() {
    let mut u = fresh(Layout::Studio);
    u.open_menu(MenuId::Measure);
    let spec = u.sheet_spec(MenuId::Measure).unwrap();
    assert_eq!(spec.tabs[0].rows, [Item::MeasGate, Item::BadgeStyle, Item::Open(MenuId::MeasPair), Item::Open(MenuId::Dvm), Item::Open(MenuId::MeasStat), Item::MeasLabels]);
    u.open_menu(MenuId::MeasStat);
    let spec = u.sheet_spec(MenuId::MeasStat).unwrap();
    assert_eq!(spec.tabs[0].rows, [Item::MeasStats, Item::MeasPop, Item::MeasShowSd, Item::MeasShowMin, Item::MeasShowMax]);
    assert_eq!(spec.tabs[0].foot, [Item::MeasStatReset]);
    assert_eq!(u.value(Item::MeasPop), "Accumulated");
    assert_eq!((u.value(Item::MeasShowSd), u.value(Item::MeasShowMin), u.value(Item::MeasShowMax)), ("On".into(), "On".into(), "On".into()));
    // the population: Current starts a new history
    u.mstat = vec![MStat { n: 5, sum: 5.0, ..Default::default() }];
    u.adjust(Item::MeasPop, 1);
    assert!(u.m.meas_pop_current && u.mstat.is_empty(), "the history of the other population is dropped");
    // the toggles are Toggle rows on the sheet
    let g = u.sheet_geo().unwrap();
    assert!(g.rows.iter().filter(|r| matches!(r.it, Item::MeasShowSd | Item::MeasShowMin | Item::MeasShowMax)).all(|r| r.kind == RowKind::Toggle));
    let sd = g.rows.iter().find(|r| r.it == Item::MeasShowSd).unwrap().ctrl;
    tap_in(&mut u, sd);
    assert_eq!(u.m.meas_show, [false, true, true]);
    // the reset button of the footer
    u.mstat = vec![MStat { n: 9, ..Default::default() }];
    let g = u.sheet_geo().unwrap();
    tap_in(&mut u, g.foot[0].1);
    assert!(u.mstat.is_empty());
    shot(&mut u, "b7b-card-statistics");
}

#[test]
fn statistics_settings_are_saved_and_have_scpi() {
    let mut u = fresh(Layout::Default);
    let cv = Canvas::new(u.scr.w, u.scr.h);
    let mut s = crate::scpi::Scpi::open(0);
    let mut q = |u: &mut Ui, c: &str| String::from_utf8(s.exec_line(u, &cv, c)).unwrap().trim().to_string();
    assert_eq!(q(&mut u, ":MEASure:STATistic:POPulation?"), "ACC");
    assert_eq!(q(&mut u, ":MEASure:STATistic:SHOW?"), "SDEV,MIN,MAX");
    q(&mut u, ":MEASure:STATistic:POPulation CURRent;:MEASure:STATistic:SHOW MIN,MAX");
    assert_eq!(q(&mut u, ":MEASure:STATistic:POPulation?;:MEASure:STATistic:SHOW?"), "CURR;MIN,MAX");
    assert!(u.m.meas_pop_current && u.m.meas_show == [false, true, true]);
    let text = crate::store::setup_text(&u.m);
    assert!(text.contains("meas_stat = 1 0 1 1"), "{text}");
    let back = crate::store::parse_setup(&text).unwrap();
    assert!(back.meas_pop_current && back.meas_show == [false, true, true]);
    // the defaults write nothing
    assert!(!crate::store::setup_text(&Model::default()).contains("meas_stat"));
    q(&mut u, ":MEASure:STATistic:SHOW NONE");
    assert_eq!(q(&mut u, ":MEASure:STATistic:SHOW?"), "NONE");
    q(&mut u, ":MEASure:STATistic:POPulation BOGUS");
    assert!(u.m.meas_pop_current, "a bad value is refused");
}

#[test]
fn the_studio_table_and_the_results_pane_follow_the_shown_columns() {
    let r0 = WaveId::Ref(0);
    let mut u = card(Layout::Studio, &[(Meas::Period, r0), (Meas::Vpp, r0)], &[]);
    u.studio_set_view(StudioView::Only);
    u.studio.panel.stats = true;
    u.mstat = vec![MStat { n: 3, sum: 3.0, sumsq: 4.0, min: 0.5, max: 2.0 }; 2];
    shot(&mut u, "b7b-table-all");
    u.m.meas_show = [false, true, false];
    shot(&mut u, "b7b-table-min");
    // the results pane of Default hides the cells of columns that are off
    let mut d = card(Layout::Default, &[(Meas::Vpp, r0)], &[]);
    d.stats_on = true;
    d.mstat = vec![MStat { n: 3, sum: 3.0, sumsq: 4.0, min: 0.5, max: 2.0 }];
    let all = d.result_rows();
    assert!(all[0].cells.iter().all(|c| !c.is_empty()), "{:?}", all[0].cells);
    d.m.meas_show = [false, true, false];
    let rows = d.result_rows();
    assert!(rows[0].cells[6].is_empty() && rows[0].cells[5].is_empty() && !rows[0].cells[4].is_empty());
}

// ------------------------------------------------------------------------------------------------ eye, jitter and power rows

use super::meascatalog::cat as Cat;

fn cat_row(c: u16) -> (u16, WaveId, WaveId, WaveId, WaveId) {
    (c as u16, WaveId::Ref(0), WaveId::Ref(0), WaveId::Ref(0), WaveId::Ref(0))
}

#[test]
fn eye_jitter_and_power_rows_open_a_card_of_the_settings_they_use() {
    let mut u = card(Layout::Studio, &[], &[]);
    u.m.cat = vec![cat_row(Cat::TjBer), cat_row(Cat::PowerQuality), cat_row(Cat::DcCommon), cat_row(Cat::Speed)];
    u.open_meas_cfg(0);
    assert_eq!(u.menu, MenuId::MeasRow);
    assert_eq!(rows_of(&u), [Item::EyeRate, Item::EyeBer, Item::EyePll, Item::EyePnLo, Item::EyePnHi, Item::EyeNotch, Item::EyeHp]);
    let spec = u.sheet_spec(MenuId::MeasRow).unwrap();
    assert_eq!(spec.tabs.len(), 1);
    assert!(spec.tabs[0].rows.len() <= SHEET_MAX_ROWS && spec.tabs[0].foot.is_empty(), "the card edits the shared settings: no Follow shared");
    shot(&mut u, "b7b-card-jitter");
    // a change on the card is the change of the Eye page
    let before = u.m.eye_ber;
    u.adjust(Item::EyeBer, 1);
    assert_ne!(u.m.eye_ber, before);
    u.open_meas_cfg(1);
    assert_eq!(rows_of(&u), [Item::PowFline, Item::PowFsw, Item::PowRef]);
    shot(&mut u, "b7b-card-power");
    u.adjust(Item::PowFline, 1);
    assert!(u.m.pw_fline > 0.0);
    u.open_meas_cfg(2);
    assert_eq!(rows_of(&u), [Item::EyeHp]);
    // a row with no setting says so and opens nothing
    u.close_menu();
    u.open_meas_cfg(3);
    assert!(!u.menu_open && u.meas_cfg.is_none());
    // the dialog of Minimal has them too
    let mut m = card(Layout::Minimal, &[], &[]);
    m.m.cat = vec![cat_row(Cat::Tie)];
    m.open_meas_cfg(0);
    let d = m.dlg_layout().unwrap();
    assert_eq!(d.rows.len(), 7);
    assert!(d.rows.iter().all(|r| r.ctrl.h >= 40) && d.close.h >= 40);
}

#[test]
fn the_pll_loop_bandwidth_is_a_setting_and_reaches_the_clock_recovery() {
    let mut u = fresh(Layout::Default);
    assert_eq!(u.m.eye_pll_bw, 0.0);
    assert_eq!(u.value(Item::EyePll), "auto");
    assert_eq!(u.item_label(Item::EyePll), "PLL bandwidth");
    u.adjust(Item::EyePll, 1);
    assert!(u.m.eye_pll_bw > 0.0);
    assert_ne!(u.value(Item::EyePll), "auto");
    assert!(u.items(MenuId::Eye).contains(&Item::EyePll));
    // a clock with a slow drift: a narrow loop follows it less than a wide one
    let n = 4000;
    let mut v = Vec::with_capacity(n);
    let mut phase = 0.0f64;
    for i in 0..n {
        phase += 1.0 / 20.0 * (1.0 + 0.002 * (i as f64 * 0.01).sin());
        v.push(if phase.fract() < 0.5 { 0.0 } else { 1.0 });
    }
    let ties = |bw: f64| {
        let c = oos_dsp::eye::recover_clock_pll(&v, 1e-10, 0.5, 0.1, None, bw, true).unwrap();
        let e: Vec<f64> = oos_dsp::eye::tie(&c.edges, &c);
        e.iter().fold(0.0f64, |a, x| a.max(x.abs()))
    };
    assert!(ties(1e3) > ties(1e8), "a narrow loop leaves more error: {} vs {}", ties(1e3), ties(1e8));
    // it is saved
    u.m.eye_pll_bw = 2.5e6;
    let text = crate::store::setup_text(&u.m);
    assert!(text.contains("eye_pll = 2500000"), "{text}");
    assert_eq!(crate::store::parse_setup(&text).unwrap().eye_pll_bw, 2.5e6);
    assert!(!crate::store::setup_text(&Model::default()).contains("eye_pll"));
}
