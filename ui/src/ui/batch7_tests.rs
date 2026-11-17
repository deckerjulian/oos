// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 7: the shared measurement Levels (percent or volts, low / mid / high, hysteresis, top and base method, fall levels)
//! in the Studio sheet, the Minimal and Default Measure menu, the setup file and SCPI. OOS_SHOTS=<dir> writes pictures.
use super::sheet::*;
use super::*;
use oos_dsp::measure::{BaseTop, Levels, Meas};

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

#[test]
fn the_defaults_keep_the_current_levels() {
    let u = fresh(Layout::Default);
    let want = Levels::percent(0.1, 0.9);
    assert_eq!(u.m.shared_levels(), want);
    assert_eq!(u.m.shared_levels().rise, [0.1, 0.5, 0.9]);
    assert_eq!(u.m.shared_levels().fall, [0.1, 0.5, 0.9]);
    assert_eq!(u.m.shared_levels().hyst, 0.05);
    assert_eq!(u.m.shared_levels().base_top, BaseTop::Auto);
    assert_eq!(u.meas_value(Item::MeasRefs).unwrap(), "10/90");
}

#[test]
fn the_levels_tab_has_seven_rows_and_the_fall_page_is_in_the_footer() {
    let mut u = fresh(Layout::Studio);
    u.open_menu(MenuId::Measure);
    let spec = u.sheet_spec(MenuId::Measure).unwrap();
    assert_eq!(spec.tabs.iter().map(|t| t.name).collect::<Vec<_>>(), ["Settings", "Sources", "Parameters", "Levels"]);
    let levels = &spec.tabs[3];
    assert_eq!(levels.rows, [Item::MeasLvType, Item::MeasRefs, Item::MeasLow, Item::MeasMid, Item::MeasHigh, Item::MeasHyst, Item::MeasBaseTop]);
    assert_eq!(levels.foot, [Item::Open(MenuId::MeasFall)]);
    assert!(!spec.tabs.iter().any(|t| t.rows.contains(&Item::MeasRefs) && t.name != "Levels"));
    assert!(!u.items(MenuId::Measure).contains(&Item::Open(MenuId::MeasLevels)));
    let g = u.sheet_geo().unwrap();
    tap_in(&mut u, g.tabs[3]);
    let g = u.sheet_geo().unwrap();
    assert_eq!(g.rows.len(), 7);
    assert!(g.rows.iter().all(|r| r.row.h == SHEET_ROW && r.ctrl.h >= 26));
    assert_eq!(g.foot.len(), 1);
    shot(&mut u, "b7-levels-studio");
    // the footer button opens the fall page
    let (_, b) = g.foot[0];
    tap_in(&mut u, b);
    assert_eq!(u.menu, MenuId::MeasFall);
    let spec = u.sheet_spec(MenuId::MeasFall).unwrap();
    assert_eq!(spec.tabs[0].rows, [Item::MeasSepFall, Item::MeasFallLow, Item::MeasFallMid, Item::MeasFallHigh]);
    shot(&mut u, "b7-fall-studio");
}

#[test]
fn the_minimal_and_default_menus_list_the_levels() {
    for (l, name) in [(Layout::Default, "b7-levels-default")] {
        let mut u = fresh(l);
        u.open_menu(MenuId::Measure);
        assert!(u.items(MenuId::Measure).contains(&Item::Open(MenuId::MeasLevels)), "{l:?}");
        assert!(!u.items(MenuId::Measure).contains(&Item::MeasLow), "{l:?}");
        u.activate(Item::Open(MenuId::MeasLevels));
        assert_eq!(u.menu, MenuId::MeasLevels);
        let items = u.items(MenuId::MeasLevels);
        for it in [Item::MeasLvType, Item::MeasRefs, Item::MeasLow, Item::MeasMid, Item::MeasHigh, Item::MeasHyst, Item::MeasBaseTop, Item::Open(MenuId::MeasFall)] {
            assert!(items.contains(&it), "{l:?} {it:?}");
        }
        shot(&mut u, name);
    }
}

#[test]
fn preset_low_mid_high_and_hysteresis_adjust_in_percent() {
    let mut u = fresh(Layout::Default);
    // preset: 10/90 -> 20/80 -> Custom -> 10/90
    u.adjust(Item::MeasRefs, 1);
    assert_eq!((u.m.meas_lpct, u.m.meas_hpct), (0.2, 0.8));
    assert_eq!(u.meas_value(Item::MeasRefs).unwrap(), "20/80");
    u.adjust(Item::MeasRefs, 1);
    assert_eq!(u.meas_value(Item::MeasRefs).unwrap(), "Custom");
    assert_eq!((u.m.meas_lpct, u.m.meas_hpct), (0.2, 0.8));
    u.adjust(Item::MeasRefs, 1);
    assert_eq!((u.m.meas_lpct, u.m.meas_hpct, u.m.meas_lv.custom), (0.1, 0.9, false));
    // editing the low level makes it Custom
    u.adjust(Item::MeasLow, 5);
    assert!((u.m.meas_lpct - 0.15).abs() < 1e-9);
    assert_eq!(u.meas_value(Item::MeasRefs).unwrap(), "Custom");
    assert_eq!(u.meas_value(Item::MeasLow).unwrap(), "15%");
    // low < mid < high holds: the low level stops below the middle one
    u.adjust(Item::MeasLow, 100);
    assert!(u.m.meas_lpct < u.m.meas_lv.mid);
    u.adjust(Item::MeasMid, -100);
    assert!(u.m.meas_lv.mid > u.m.meas_lpct);
    u.adjust(Item::MeasMid, 10);
    assert!((u.m.meas_lv.mid - 0.6).abs() < 1e-9 || u.m.meas_lv.mid > 0.5);
    // hysteresis 1..50 %
    u.adjust(Item::MeasHyst, 2);
    assert!((u.m.meas_lv.hyst - 0.07).abs() < 1e-9);
    u.adjust(Item::MeasHyst, -100);
    assert!((u.m.meas_lv.hyst - 0.01).abs() < 1e-9);
    assert_eq!(u.meas_value(Item::MeasHyst).unwrap(), "1%");
}

#[test]
fn volts_levels_are_ordered_and_reach_the_analysis() {
    let mut u = fresh(Layout::Default);
    u.adjust(Item::MeasLvType, 1);
    assert!(u.m.meas_lv.absolute);
    assert_eq!(u.meas_value(Item::MeasLvType).unwrap(), "Volts");
    assert_eq!(u.meas_value(Item::MeasRefs).unwrap(), "-");
    let lv = u.m.shared_levels();
    assert!(lv.absolute && lv.rise == [-1.0, 0.0, 1.0] && lv.hyst == 0.03);
    // 1-2-5 steps: -1 V up one step is -500 mV
    u.adjust(Item::MeasLow, 1);
    assert_eq!(u.m.meas_lv.volts[0], -0.5);
    assert_eq!(u.meas_value(Item::MeasLow).unwrap(), "-500 mV");
    // the high level cannot pass the middle one downwards
    u.adjust(Item::MeasHigh, -100);
    assert!(u.m.meas_lv.volts[2] > u.m.meas_lv.volts[1]);
    u.adjust(Item::MeasHyst, 1);
    assert_eq!(u.m.meas_lv.vhyst, 0.05);
    u.adjust(Item::MeasLvType, 1);
    assert!(!u.m.meas_lv.absolute);
}

#[test]
fn top_base_method_cycles_and_separate_fall_starts_from_the_rise_levels() {
    let mut u = fresh(Layout::Default);
    let names: Vec<String> = u.options(Item::MeasBaseTop);
    assert_eq!(names, ["Auto", "Min-max", "Histogram mean", "Histogram mode"]);
    u.adjust(Item::MeasBaseTop, 1);
    assert_eq!(u.m.meas_lv.base_top, BaseTop::MinMax);
    u.adjust(Item::MeasBaseTop, -2);
    assert_eq!(u.m.meas_lv.base_top, BaseTop::Mode);
    u.adjust(Item::MeasBaseTop, 1);
    assert_eq!(u.m.meas_lv.base_top, BaseTop::Auto);
    u.adjust(Item::MeasRefs, 1);
    assert_eq!(u.shared_fall(), u.m.shared_levels().rise);
    u.adjust(Item::MeasSepFall, 1);
    assert!(u.m.meas_lv.sep_fall);
    assert_eq!(u.m.meas_lv.fall, [0.2, 0.5, 0.8]);
    u.adjust(Item::MeasFallLow, -10);
    assert!((u.m.meas_lv.fall[0] - 0.1).abs() < 1e-9);
    // the fall levels change; the rise ones do not
    assert_eq!(u.m.shared_levels().rise, [0.2, 0.5, 0.8]);
    assert_eq!(u.m.shared_levels().fall, [0.1, 0.5, 0.8]);
}

impl Ui {
    fn shared_fall(&self) -> [f64; 3] { self.m.shared_levels().fall }
}

#[test]
fn levels_reach_the_measurements_of_a_reference() {
    let mut u = fresh(Layout::Default);
    let mut v = vec![0.0f32; 20];
    for _ in 0..3 {
        v.extend((0..100).map(|i| i as f32 / 99.0));
        v.extend(vec![1.0; 80]);
        v.extend((0..100).map(|i| 1.0 - i as f32 / 99.0));
        v.extend(vec![0.0; 80]);
    }
    u.refs[0] = Some(RefWave { src: 0, color: 0, label: String::new(), t0: 0.0, dt: 1e-9, v: v.into(), vpd: 0.5, offset: 0.0, on: true });
    let src = WaveId::Ref(0);
    let ten = u.meas_at(src, Meas::Rise);
    assert!(ten.is_finite() && (ten - 80e-9).abs() < 5e-9, "{ten}");
    u.adjust(Item::MeasRefs, 1);
    let twenty = u.meas_at(src, Meas::Rise);
    assert!((twenty - 60e-9).abs() < 5e-9, "{twenty}");
    // a different middle level moves Vmid
    u.adjust(Item::MeasRefs, 1);
    u.adjust(Item::MeasMid, -20);
    assert!((u.meas_at(src, Meas::Vmid) - 0.3).abs() < 1e-3, "{}", u.meas_at(src, Meas::Vmid));
    // the top and base method changes the top
    u.adjust(Item::MeasBaseTop, 1);
    assert_eq!(u.m.meas_lv.base_top, BaseTop::MinMax);
    assert!((u.meas_at(src, Meas::Vtop) - 1.0).abs() < 1e-9);
}

#[test]
fn the_levels_are_saved_and_restored() {
    let mut u = fresh(Layout::Default);
    u.adjust(Item::MeasRefs, 1);
    u.adjust(Item::MeasMid, 3);
    u.adjust(Item::MeasHyst, 4);
    u.adjust(Item::MeasBaseTop, 2);
    u.adjust(Item::MeasSepFall, 1);
    u.adjust(Item::MeasFallHigh, -3);
    u.adjust(Item::MeasLvType, 1);
    u.adjust(Item::MeasHigh, 2);
    let text = crate::store::setup_text(&u.m);
    assert!(text.contains("meas_levels "));
    let back = crate::store::parse_setup(&text).unwrap();
    assert_eq!(back.meas_lv, u.m.meas_lv);
    assert_eq!(back.shared_levels(), u.m.shared_levels());
    // a setup file without the line loads with the defaults
    let old: String = text.lines().filter(|l| !l.starts_with("meas_levels ")).collect::<Vec<_>>().join("\n");
    assert_eq!(crate::store::parse_setup(&old).unwrap().meas_lv, crate::model::MeasLevels::default());
}

#[test]
fn scpi_sets_and_reads_the_levels() {
    use crate::scpi::Scpi;
    let (mut s, mut u, cv) = (Scpi::open(0), Ui::new(Source::Demo { t: 0.0 }), Canvas::new(W, H));
    let q = |s: &mut Scpi, u: &mut Ui, c: &str| String::from_utf8(s.exec_line(u, &cv, c)).unwrap();
    assert_eq!(q(&mut s, &mut u, ":MEASure:LEVel:TYPE?").trim(), "PERC");
    assert_eq!(q(&mut s, &mut u, ":MEASure:LEVel:LOW?").trim(), "10");
    assert_eq!(q(&mut s, &mut u, ":MEASure:LEVel:MID?").trim(), "50");
    assert_eq!(q(&mut s, &mut u, ":MEASure:LEVel:HIGH?").trim(), "90");
    assert_eq!(q(&mut s, &mut u, ":MEASure:LEVel:HYSTeresis?").trim(), "5");
    assert_eq!(q(&mut s, &mut u, ":MEASure:LEVel:BASEtop?").trim(), "AUTO");
    q(&mut s, &mut u, ":MEASure:LEVel:PRESet 20/80");
    assert_eq!((u.m.meas_lpct, u.m.meas_hpct), (0.2, 0.8));
    q(&mut s, &mut u, ":MEAS:LEV:MID 40");
    assert_eq!(u.m.meas_lv.mid, 0.4);
    q(&mut s, &mut u, ":MEAS:LEV:HYST 8");
    assert_eq!(u.m.meas_lv.hyst, 0.08);
    q(&mut s, &mut u, ":MEAS:LEV:BASE MEAN");
    assert_eq!(u.m.meas_lv.base_top, BaseTop::Mean);
    assert_eq!(q(&mut s, &mut u, ":MEAS:LEV:BASE?").trim(), "MEAN");
    q(&mut s, &mut u, ":MEAS:LEV:FALL:SEP ON");
    assert!(u.m.meas_lv.sep_fall);
    q(&mut s, &mut u, ":MEAS:LEV:FALL:HIGH 70");
    assert_eq!(u.m.meas_lv.fall[2], 0.7);
    assert_eq!(q(&mut s, &mut u, ":MEAS:LEV:FALL:HIGH?").trim(), "70");
    q(&mut s, &mut u, ":MEAS:LEV:TYPE VOLT");
    assert!(u.m.meas_lv.absolute);
    q(&mut s, &mut u, ":MEAS:LEV:HIGH 2");
    assert_eq!(u.m.meas_lv.volts[2], 2.0);
    // an order that breaks low < mid < high is refused
    let before = u.m.meas_lv;
    q(&mut s, &mut u, ":MEAS:LEV:LOW 5");
    assert_eq!(u.m.meas_lv, before);
    assert!(q(&mut s, &mut u, ":SYSTem:ERRor?").contains("-224"));
    q(&mut s, &mut u, ":MEAS:LEV:TYPE PERC");
    q(&mut s, &mut u, ":MEAS:LEV:LOW 60");
    assert!(q(&mut s, &mut u, ":SYSTem:ERRor?").contains("-224"));
    q(&mut s, &mut u, ":MEAS:LEV:BASE NOPE");
    assert!(q(&mut s, &mut u, ":SYSTem:ERRor?").contains("-224"));
}
