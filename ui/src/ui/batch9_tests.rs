//! Batch 9: the rest of the Studio menus on the Settings Sheet (Spectrum, Eye, Power, Storage, Mask, SIM, LA), source
//! pickers and a Measure page in the Add panel, vertical zoom on the zoom bar, and the minus of Number rows.
//! OOS_SHOTS=<dir> writes pictures (b9-*).
use super::addrail::{AddKind, RailAct};
use super::sheet::RowKind;
use super::*;

fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    for _ in 0..10 { u.step(0.05); }
    u
}

fn shot(u: &mut Ui, name: &str) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    for _ in 0..6 { u.step(0.05); }
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("b9-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn tap_in(u: &mut Ui, r: Rect) { u.tap(r.x + r.w / 2, r.y + r.h / 2); }

fn open(u: &mut Ui, m: MenuId) {
    if u.menu_open { u.close_menu(); }
    u.open_menu(m);
    assert!(u.sheet_active(), "{m:?} is a sheet in Studio");
}

fn tab(u: &mut Ui, i: usize) {
    let g = u.sheet_geo().expect("sheet");
    tap_in(u, g.tabs[i]);
}

/// Every item of the classic menu is on some tab or in some footer of the sheet.
fn all_reachable(u: &Ui, m: MenuId) {
    let spec = u.sheet_spec(m).unwrap_or_else(|| panic!("{m:?} has a sheet"));
    let on_sheet: Vec<Item> = spec.tabs.iter().flat_map(|t| t.rows.iter().chain(t.foot.iter()).copied()).collect();
    for it in u.items(m) {
        assert!(on_sheet.contains(&it), "{m:?}: {it:?} is reachable on the sheet");
    }
    for t in &spec.tabs {
        assert!(t.rows.len() <= 7 && t.foot.len() <= 2, "{m:?} {} fits", t.name);
    }
    assert!(spec.tabs.len() <= 4);
}

const CONVERTED: [MenuId; 10] = [MenuId::Spectrum, MenuId::Eye, MenuId::Power, MenuId::Storage, MenuId::Mask, MenuId::Sim, MenuId::SimAdvanced, MenuId::La, MenuId::LaLines, MenuId::LaPat];

// ------------------------------------------------------------------------------------------------ item 1

#[test]
fn the_remaining_studio_menus_are_sheets_and_every_setting_stays_reachable() {
    let mut u = studio();
    for m in CONVERTED { all_reachable(&u, m); }
    // states that change the list: every power mode, switched-off sources, a user threshold, SIM from a record
    for mode in [PowerMode::Quality, PowerMode::Harmonics, PowerMode::Switching, PowerMode::Ripple, PowerMode::Capacitance, PowerMode::Rail] {
        u.m.pw_mode = mode;
        all_reachable(&u, MenuId::Power);
    }
    u.m.ch[1].on = false;
    u.m.pw_v = WaveId::Ch(1);
    u.m.spec_src = WaveId::Ch(1);
    u.m.eye_src = WaveId::Ch(1);
    u.m.mask_src = 1;
    for m in [MenuId::Power, MenuId::Spectrum, MenuId::Eye, MenuId::Mask] {
        assert!(u.items(m).contains(&Item::SrcOn), "{m:?} says its source is off");
        all_reachable(&u, m);
    }
    u.m.la.preset = [la::USER, la::USER];
    all_reachable(&u, MenuId::La);
    assert!(u.items(MenuId::La).contains(&Item::LaUser(1)));
    u.offline = true;
    assert!(u.items(MenuId::Sim).contains(&Item::SimLive));
    all_reachable(&u, MenuId::Sim);
}

#[test]
fn default_and_minimal_keep_their_menus() {
    for l in [Layout::Default, Layout::Minimal] {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.layout = l;
        for _ in 0..4 { u.step(0.05); }
        for m in CONVERTED {
            assert!(u.sheet_spec(m).is_none(), "{l:?} {m:?} stays a dialog");
            u.open_menu(m);
            assert!(!u.sheet_active());
            assert!(u.dlg_layout().is_some(), "{l:?} {m:?} opens its classic dialog");
            u.close_menu();
        }
    }
}

#[test]
fn rows_are_44_px_and_every_control_is_a_target_of_40_px() {
    let mut u = studio();
    u.m.la.preset = [la::USER, la::USER];
    for m in CONVERTED {
        open(&mut u, m);
        let n = u.sheet_spec(m).unwrap().tabs.len();
        for i in 0..n {
            if n > 1 { tab(&mut u, i); }
            let g = u.sheet_geo().unwrap();
            for t in &g.tabs { assert!(t.h >= 40 && t.w >= 40, "{m:?} tab"); }
            for r in &g.rows {
                assert_eq!(r.row.h, 44, "{m:?} row");
                assert!(r.ctrl.h >= 26 && r.row.bottom() <= g.r.bottom() - 56, "{m:?} {:?} fits above the footer", r.it);
                if r.kind != RowKind::Toggle { assert!(r.ctrl.h >= 40, "{m:?} {:?} control", r.it); }
            }
            for (_, b) in &g.foot { assert!(b.h >= 44 && b.bottom() <= g.r.bottom(), "{m:?} footer"); }
        }
    }
}

#[test]
fn spectrum_has_traces_and_limits_on_their_own_tabs() {
    let mut u = studio();
    open(&mut u, MenuId::Spectrum);
    let spec = u.sheet_spec(MenuId::Spectrum).unwrap();
    assert_eq!(spec.tabs.iter().map(|t| t.name).collect::<Vec<_>>(), ["Spectrum", "Traces", "Limits"]);
    assert!(spec.tabs[2].foot.contains(&Item::SpecLimitSet) && spec.tabs[2].foot.contains(&Item::SpecLimitClear));
    assert_eq!(*spec.tabs[2].foot.last().unwrap(), Item::SpecLimitClear, "the destructive one goes last");
    shot(&mut u, "spectrum-1");
    tab(&mut u, 1);
    shot(&mut u, "spectrum-2");
    tab(&mut u, 2);
    shot(&mut u, "spectrum-3");
    // a row works as it did: the window is a Pick
    tab(&mut u, 0);
    let g = u.sheet_geo().unwrap();
    let win = g.rows.iter().find(|r| r.it == Item::SpecWin).unwrap();
    assert_eq!(win.kind, RowKind::Pick);
    tap_in(&mut u, win.ctrl);
    assert_eq!(u.menu, MenuId::Pick);
}

#[test]
fn eye_power_and_storage_pages() {
    let mut u = studio();
    open(&mut u, MenuId::Eye);
    shot(&mut u, "eye-1");
    tab(&mut u, 1);
    shot(&mut u, "eye-2");
    tab(&mut u, 2);
    shot(&mut u, "eye-3");
    // Power: the analyses are one Pick row, the page never grows past seven rows
    u.m.pw_mode = PowerMode::Rail;
    open(&mut u, MenuId::Power);
    let g = u.sheet_geo().unwrap();
    assert_eq!(g.rows.iter().find(|r| r.it == Item::PowMode).unwrap().kind, RowKind::Pick);
    assert!(g.rows.len() <= 7);
    shot(&mut u, "power-rail");
    u.m.pw_mode = PowerMode::Ripple;
    shot(&mut u, "power-ripple");
    open(&mut u, MenuId::Storage);
    shot(&mut u, "storage-1");
    tab(&mut u, 1);
    shot(&mut u, "storage-2");
    let spec = u.sheet_spec(MenuId::Storage).unwrap();
    assert_eq!(spec.tabs.len(), 2);
}

#[test]
fn mask_sim_and_la_pages() {
    let mut u = studio();
    open(&mut u, MenuId::Mask);
    shot(&mut u, "mask-1");
    tab(&mut u, 1);
    shot(&mut u, "mask-2");
    let spec = u.sheet_spec(MenuId::Mask).unwrap();
    assert_eq!(*spec.tabs[1].foot.last().unwrap(), Item::MaskClear);
    open(&mut u, MenuId::Sim);
    shot(&mut u, "sim-1");
    for i in 1..4 { tab(&mut u, i); shot(&mut u, &format!("sim-{}", i + 1)); }
    // Advanced is a link on the Jitter tab, and it opens as a sheet with Back
    tab(&mut u, 2);
    let g = u.sheet_geo().unwrap();
    let adv = g.rows.iter().find(|r| r.it == Item::Open(MenuId::SimAdvanced)).expect("Advanced link");
    assert_eq!(adv.kind, RowKind::Link);
    tap_in(&mut u, adv.ctrl);
    assert_eq!(u.menu, MenuId::SimAdvanced);
    assert!(u.sheet_active() && u.sheet_chrome().1.is_some());
    shot(&mut u, "sim-advanced");
    let b = u.sheet_chrome().1.unwrap();
    tap_in(&mut u, b);
    assert_eq!(u.menu, MenuId::Sim);
    // LA: lines are four to a tab, and the channel list never scrolls
    open(&mut u, MenuId::La);
    shot(&mut u, "la-1");
    tab(&mut u, 1);
    shot(&mut u, "la-2");
    tab(&mut u, 0);
    let g = u.sheet_geo().unwrap();
    let lines = g.rows.iter().find(|r| r.it == Item::Open(MenuId::LaLines)).expect("lines link");
    tap_in(&mut u, lines.ctrl);
    assert_eq!(u.menu, MenuId::LaLines);
    let g = u.sheet_geo().unwrap();
    assert_eq!(g.tabs.len(), 4);
    assert_eq!(g.rows.len(), 4);
    shot(&mut u, "la-lines");
    let first = g.rows[0].it;
    let before = u.value(first);
    tap_in(&mut u, g.rows[0].ctrl);
    assert_ne!(u.value(first), before, "a tap changes the line");
    tab(&mut u, 3);
    let g = u.sheet_geo().unwrap();
    assert!(g.rows.iter().any(|r| r.it == Item::LaLine(15)));
}

// ------------------------------------------------------------------------------------------------ item 2

fn open_plot(u: &mut Ui, i: usize) {
    if !(u.menu_open && u.menu == MenuId::AddPanel) { let r = u.studio_top().add; tap_in(u, r); }
    if u.sheet.add.kind.is_some() { let b = u.sheet_chrome().1.unwrap(); tap_in(u, b); }
    let (_, r) = u.rail_geo().rows.into_iter().find(|(k, _)| *k == AddKind::Plot).unwrap();
    tap_in(u, r);
    let g = u.rail_geo();
    tap_in(u, g.types[i].1);
}

fn add_btn(u: &Ui) -> Rect { u.rail_geo().foot.into_iter().find(|(a, _)| *a == RailAct::Add).unwrap().1 }

#[test]
fn xy_picks_x_and_y_and_draws_them() {
    let mut u = studio();
    u.m.ch[2].on = true;
    open_plot(&mut u, 0);
    let g = u.rail_geo();
    assert_eq!((g.srcs.len(), g.srcs_b.len()), (3, 3), "X and Y each offer the channels that are on");
    for (_, r) in g.srcs.iter().chain(g.srcs_b.iter()) { assert!(r.h >= 40 && r.w >= 40); }
    assert_eq!(g.src_labels.len(), 2);
    // X = CH3, Y = CH2
    tap_in(&mut u, g.srcs[2].1);
    tap_in(&mut u, g.srcs_b[1].1);
    shot(&mut u, "xy-page");
    let b = add_btn(&u); tap_in(&mut u, b);
    assert!(u.xy);
    assert_eq!(u.xy_src, [2, 1]);
    shot(&mut u, "xy-drawn");
    // changing a chip while it is on changes the plot at once
    let g = u.rail_geo();
    tap_in(&mut u, g.srcs[0].1);
    assert_eq!(u.xy_src, [0, 1]);
}

#[test]
fn histogram_eye_spectrum_and_trend_pick_their_source() {
    let mut u = studio();
    u.m.ch[2].on = true;
    u.m.run = RunState::Stop;
    // histogram: channels only
    open_plot(&mut u, 2);
    let g = u.rail_geo();
    assert_eq!(g.srcs.len(), 3);
    assert!(g.srcs_b.is_empty() && g.meas.is_empty());
    tap_in(&mut u, g.srcs[2].1);
    let b = add_btn(&u); tap_in(&mut u, b);
    assert!(u.panel_shown(super::sheet::PanelKind::Histogram));
    assert_eq!(u.hist_src(), Some(2));
    // eye and spectrum: the waves that are on
    open_plot(&mut u, 1);
    let g = u.rail_geo();
    tap_in(&mut u, g.srcs[1].1);
    let b = add_btn(&u); tap_in(&mut u, b);
    assert_eq!(u.m.eye_src, WaveId::Ch(1));
    open_plot(&mut u, 3);
    let g = u.rail_geo();
    tap_in(&mut u, g.srcs[2].1);
    let b = add_btn(&u); tap_in(&mut u, b);
    assert_eq!(u.m.spec_src, WaveId::Ch(2));
    // trend: a source and a measurement; the first chart follows the measurement that is made
    open_plot(&mut u, 4);
    let g = u.rail_geo();
    assert_eq!((g.srcs.len(), g.meas.len()), (3, 4));
    for (_, r) in g.meas.iter() { assert!(r.h >= 40 && r.w >= 40); }
    tap_in(&mut u, g.srcs[1].1);
    tap_in(&mut u, g.meas[1].1);
    shot(&mut u, "trend-page");
    let b = add_btn(&u); tap_in(&mut u, b);
    let k = Ui::TREND_MEAS[1];
    assert!(u.m.meas.contains(&(k, WaveId::Ch(1))));
    let p = u.m.meas.iter().position(|e| *e == (k, WaveId::Ch(1))).unwrap();
    assert_eq!(u.m.trend_sel[0], 2 + p);
    assert!(u.panel_shown(super::sheet::PanelKind::Trend));
}

#[test]
fn the_plot_pages_keep_their_picture_and_fit_the_sheet() {
    let mut u = studio();
    u.m.ch[2].on = true;
    for i in 0..5 {
        open_plot(&mut u, i);
        let g = u.rail_geo();
        let sheet = u.sheet_rect();
        assert!(g.preview.w > 200 && g.preview.h >= 60, "plot {i} picture {:?}", g.preview);
        let mut all: Vec<Rect> = g.types.iter().map(|t| t.1).chain(g.srcs.iter().map(|t| t.1)).chain(g.srcs_b.iter().map(|t| t.1)).chain(g.meas.iter().map(|t| t.1)).collect();
        all.push(g.preview);
        for r in all { assert!(r.x >= sheet.x && r.right() <= sheet.right() && r.bottom() <= sheet.bottom() - 56, "plot {i} inside above the footer: {r:?}"); }
        assert!(g.note.y + 20 <= g.foot[0].1.y, "plot {i} note clears the footer");
        shot(&mut u, &format!("plot-{i}"));
    }
}

#[test]
fn measure_has_a_picture_a_line_and_a_choose_button() {
    let mut u = studio();
    let r = u.studio_top().add;
    tap_in(&mut u, r);
    let (_, r) = u.rail_geo().rows.into_iter().find(|(k, _)| *k == AddKind::Measure).unwrap();
    tap_in(&mut u, r);
    assert_eq!(u.menu, MenuId::AddPanel, "Measure is a page like the others");
    assert_eq!(u.sheet.add.kind, Some(AddKind::Measure));
    let g = u.rail_geo();
    assert!(g.preview.w > 200 && g.preview.h >= 80 && g.desc.h >= 32);
    assert_eq!(g.foot.len(), 1);
    assert_eq!(g.foot[0].0, RailAct::Choose);
    assert!(g.foot[0].1.h >= 44);
    let mut cv = Canvas::new(W, H);
    cv.clear(BG);
    let r = Rect::new(10, 10, 356, 88);
    u.draw_rail_preview(&mut cv, r, "measure");
    let mut blank = Canvas::new(W, H);
    blank.clear(BG);
    u.draw_rail_preview(&mut blank, r, "none");
    let drawn = (r.y..r.bottom()).flat_map(|y| (r.x..r.right()).map(move |x| (x, y))).filter(|&(x, y)| cv.px[(y * W + x) as usize] != blank.px[(y * W + x) as usize]).count();
    assert!(drawn > 150, "the picture draws a waveform and readouts ({drawn} pixels)");
    shot(&mut u, "measure-page");
    tap_in(&mut u, g.foot[0].1);
    assert_eq!(u.menu, MenuId::MeasAdd);
    let b = u.sheet_chrome().1.expect("Back");
    tap_in(&mut u, b);
    assert_eq!(u.menu, MenuId::AddPanel);
    assert_eq!(u.sheet.add.kind, Some(AddKind::Measure), "Back lands on the Measure page");
}

// ------------------------------------------------------------------------------------------------ item 3

fn zoomed() -> Ui {
    let mut u = studio();
    u.m.run = RunState::Stop;
    u.set_zoom(true);
    u.m.zscale = u.m.tscale.saturating_sub(3);
    u.m.zdelay = 0.0;
    u.relayout();
    for _ in 0..4 { u.step(0.05); }
    u.render(&mut Canvas::new(W, H));
    u
}

#[test]
fn the_zoom_bar_has_vertical_zoom_with_targets_of_40_px() {
    let mut u = zoomed();
    let bar = u.studio_zoom_bar().expect("bar");
    let row = u.zoom_readout();
    for r in [bar.vplus, bar.vminus] {
        assert!(r.w >= 40 && r.h >= 20 && r.x >= row.x && r.right() <= row.right(), "{r:?} in {row:?}");
    }
    assert!(bar.minus.right() <= bar.vplus.x && bar.vplus.right() <= bar.vminus.x && bar.vminus.right() <= bar.center.x);
    assert!(bar.vminus.right() <= bar.readout.x || bar.readout.w == 0);
    shot(&mut u, "zoom-bar");
    assert_eq!(u.zoom_vfactor, 1.0);
    // a touch 19 px off the middle still counts, as with the time chips
    let (x, y) = (bar.vplus.x + bar.vplus.w / 2, bar.vplus.y + bar.vplus.h / 2);
    u.tap(x, y - 19);
    assert_eq!(u.zoom_vfactor, 2.0, "Y+ doubles the display scale");
    u.tap(x, y + 19);
    assert_eq!(u.zoom_vfactor, 4.0);
    let (x, y) = (bar.vminus.x + bar.vminus.w / 2, bar.vminus.y + bar.vminus.h / 2);
    u.tap(x, y);
    u.tap(x, y);
    assert_eq!(u.zoom_vfactor, 1.0, "Y- halves it, never below one");
    u.tap(x, y);
    assert_eq!(u.zoom_vfactor, 1.0);
    // the time zoom is not touched
    let z = u.m.zscale;
    u.tap(bar.vplus.x + 20, bar.vplus.y + 20);
    assert_eq!(u.m.zscale, z);
    assert!(u.zoom_vfactor > 1.0);
}

#[test]
fn a_narrow_plot_drops_the_vertical_chips_instead_of_overlapping() {
    let u = zoomed();
    let bar = u.studio_zoom_bar().unwrap();
    // the chips on the right never run into the ones on the left
    assert!(bar.minus.right() <= bar.center.x);
    if bar.vplus.w == 0 { assert_eq!(bar.vminus.w, 0); } else { assert!(bar.vminus.right() <= bar.center.x); }
}

// ------------------------------------------------------------------------------------------------ item 4

#[test]
fn the_minus_of_a_number_row_is_a_real_minus() {
    // the bundled font has the hyphen only: no U+2212, so the sign is drawn as a stroke as wide as the plus
    let f = Fonts::get(0);
    assert_ne!(f.m.glyph('\u{2212}').cp, 0x2212, "the font has no minus sign; if it gains one, draw it as text");
    let mut cv = Canvas::new(120, 60);
    cv.clear(BG);
    let r = Rect::new(10, 10, 100, 40);
    Ui::draw_sign(&mut cv, r, false);
    let lit = |cv: &Canvas, x: i32, y: i32| cv.px[(y * 120 + x) as usize] == TEXT;
    let cy = 30;
    let bar = (0..120).filter(|&x| lit(&cv, x, cy) && lit(&cv, x, cy - 1)).count();
    assert_eq!(bar, 14, "two pixels thick, 14 wide");
    assert!(!lit(&cv, 60, cy - 6) && !lit(&cv, 60, cy + 6), "no vertical stroke: it is a minus");
    let mut cv2 = Canvas::new(120, 60);
    cv2.clear(BG);
    Ui::draw_sign(&mut cv2, r, true);
    assert!(lit(&cv2, 60, cy - 6) && lit(&cv2, 60, cy + 6), "the plus has its vertical stroke");
    assert_eq!(bar, (0..120).filter(|&x| lit(&cv2, x, cy) && lit(&cv2, x, cy - 1)).count(), "same width as the plus");
}

#[test]
fn number_rows_draw_the_stroke_signs_in_both_buttons() {
    let mut u = studio();
    open(&mut u, MenuId::Sim);
    let g = u.sheet_geo().unwrap();
    let row = g.rows.iter().find(|r| r.kind == RowKind::Number).expect("a Number row on the first tab");
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    let (mi, _, pl) = Ui::value_parts(row.ctrl);
    let (cx, cy) = (mi.x + mi.w / 2, mi.y + mi.h / 2);
    let run = (cx - 7..cx + 7).filter(|&x| cv.px[(cy * W + x) as usize] == TEXT).count();
    assert_eq!(run, 14, "the minus button shows a 14 px stroke");
    let (px_, py) = (pl.x + pl.w / 2, pl.y + pl.h / 2);
    assert_eq!(cv.px[((py - 6) * W + px_) as usize], TEXT, "the plus has its vertical stroke");
    shot(&mut u, "number-row");
}
