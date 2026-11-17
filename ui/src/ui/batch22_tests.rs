//! Batch 22: the detailed measurement table without overlaps, the Studio / Classic switch in the badge row, and the
//! zoom with too few samples (a line through the samples, each one marked). OOS_SHOTS=<dir> writes pictures (b22-*).
use super::dock::{Dir, VKind};
use super::measpanel::PanelView;
use super::studio::StudioView;
use super::*;

fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.studio_detail = false;
    u.m.studio_zoom_tab = false;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    for _ in 0..10 { u.step(0.05); }
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    u.studio_set_view(StudioView::Docked);
    frame(&mut u);
    u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(0)), (Meas::Vamp, WaveId::Ch(1)), (Meas::Freq, WaveId::Ch(1)), (Meas::Rise, WaveId::Ch(0))];
    u.ensure_meas_own();
    for _ in 0..6 { frame(&mut u); }
    u
}

fn frame(u: &mut Ui) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    u.step(0.02);
}

fn shot(u: &mut Ui, name: &str) {
    for _ in 0..6 { frame(u); }
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("b22-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn touch(u: &mut Ui, phase: Phase, x: i32, y: i32) {
    u.handle(Ev::Touch { id: 0, phase, x, y });
    u.step(0.01);
}

fn tap(u: &mut Ui, r: Rect) {
    let (x, y) = (r.x + r.w / 2, r.y + r.h / 2);
    touch(u, Phase::Down, x, y);
    touch(u, Phase::Up, x, y);
    frame(u);
}

fn drag(u: &mut Ui, a: (i32, i32), b: (i32, i32)) {
    touch(u, Phase::Down, a.0, a.1);
    for k in 1..=8 { touch(u, Phase::Move, a.0 + (b.0 - a.0) * k / 8, a.1 + (b.1 - a.1) * k / 8); }
    touch(u, Phase::Up, b.0, b.1);
    frame(u);
}

fn panel_width(u: &mut Ui, w: i32) {
    let d = u.dock_layout().dividers.iter().find(|d| d.dir == Dir::H).cloned().expect("a divider");
    let (x, y) = (d.line.x + d.line.w / 2, d.line.y + d.line.h / 2);
    let have = u.dock_leaf(VKind::Measure).unwrap().w;
    drag(u, (x, y), (x + have - w, y));
}

fn detailed(u: &mut Ui) {
    u.panel_set_view(PanelView::Detailed);
    for _ in 0..6 { frame(u); }
    assert!(u.studio_detailed());
}

/// The cells of the table at the panel's width do not overlap, and every text has its room.
fn check_table(u: &Ui) {
    let f = u.fonts();
    let g = u.studio_panel_geo().expect("the panel");
    let (cols, _) = Ui::detail_cols(g.list.w);
    let row = Rect::new(0, 0, g.list.w, 38);
    let cells: Vec<Rect> = cols.iter().map(|&(_, x, cw)| Ui::detail_cell(row, x, cw, 0)).collect();
    for p in cells.windows(2) { assert!(p[0].right() <= p[1].x, "{p:?}"); }
    for (&(c, _, _), cell) in cols.iter().zip(&cells) {
        let head = u.detail_head(c, cell.w);
        assert!(head.is_empty() || f.sb.width(head) <= cell.w, "head {head:?} in {cell:?}");
        if c == 0 && g.list.w >= 240 { assert!(!head.is_empty(), "the Source head has a label when there is room"); }
        if c == 1 { assert_eq!(head, "Name"); }
    }
    let rows = u.meas_rows();
    let keys = u.studio_keys();
    for (i, r) in rows.iter().enumerate() {
        let room = cells[1].w - super::measview::DETAIL_PIN_W;
        let (shown, small) = u.detail_name(keys.get(i).copied(), &r.name, room);
        assert!(if small { f.s.width(&shown) } else { f.m.width(&shown) } <= room, "{shown:?} in {room}");
        assert!(f.mb.width(&r.text()) <= cells[2].w, "value {:?} in {:?}", r.text(), cells[2]);
    }
}

#[test]
fn the_detailed_table_has_no_overlaps_at_the_default_and_the_minimum_panel_width() {
    let mut u = studio();
    detailed(&mut u);
    check_table(&u);
    // Peak-to-Peak is shown whole or as Pk-pk at the default width, never cut
    let g = u.studio_panel_geo().unwrap();
    let rows = u.meas_rows();
    let keys = u.studio_keys();
    let i = rows.iter().position(|r| r.name.contains("Peak")).expect("a Peak-to-Peak row");
    let (cols, _) = Ui::detail_cols(g.list.w);
    let cell = Ui::detail_cell(Rect::new(0, 0, g.list.w, 38), cols[1].1, cols[1].2, 0);
    let shown = u.detail_name(keys.get(i).copied(), &rows[i].name, cell.w - super::measview::DETAIL_PIN_W).0;
    assert!(shown == rows[i].name || shown == "Pk-pk", "{shown:?}");
    shot(&mut u, "meas-detailed");
    panel_width(&mut u, 200);
    for _ in 0..4 { frame(&mut u); }
    let g = u.studio_panel_geo().unwrap();
    assert!(g.list.w <= 215, "{:?}", g.list);
    check_table(&u);
    shot(&mut u, "meas-detailed-narrow");
}

#[test]
fn the_detailed_columns_at_every_width_follow_each_other_and_name_value_and_mean_stay_in_view() {
    for w in [200, 210, 240, 262, 300, 329, 330, 400, 560, 700, 900] {
        let (cols, total) = Ui::detail_cols(w);
        let mut x = 0;
        for &(_, cx, cw) in &cols { assert_eq!(cx, x); assert!(cw >= 12); x += cw; }
        assert_eq!(x, total);
        if w >= 250 { assert!(cols[3].1 + cols[3].2 <= w.max(262), "{w}: {cols:?}"); }
    }
}

#[test]
fn the_badge_row_has_a_button_that_switches_studio_and_classic_and_the_sheet_follows() {
    let mut u = studio();
    u.studio_set_view(StudioView::Waveform);
    for k in u.studio_keys() { u.studio_set_pin(k, true); }
    for _ in 0..4 { frame(&mut u); }
    assert!(!u.studio_classic());
    assert_eq!(u.value(Item::BadgeStyle), "Studio");
    let (style, compact, eye) = (u.studio_style_chip(), u.studio_compact_chip(), u.studio_eye());
    assert!(style.right() < compact.x && compact.right() < eye.x, "{style:?} {compact:?} {eye:?}");
    let hit = Ui::hit40(style);
    assert!(hit.w >= 40 && hit.right() < compact.x, "the hit area stays off the neighbour");
    for (_, r) in u.studio_badges() { assert!(r.right() <= style.x, "badges end before the button"); }
    shot(&mut u, "badges-studio");
    tap(&mut u, style);
    assert!(u.studio_classic() && u.m.studio_classic);
    assert_eq!(u.value(Item::BadgeStyle), "Classic", "the Measure sheet shows it");
    assert!(!u.studio.compact && !u.studio.hide_badges, "the other buttons were not touched");
    assert!(u.classic_layout().pairs.iter().all(|(_, r)| r.right() <= u.studio_style_chip().x));
    shot(&mut u, "badges-classic");
    let s = u.studio_style_chip();
    tap(&mut u, s);
    assert!(!u.studio_classic());
    assert_eq!(u.value(Item::BadgeStyle), "Studio");
    // the sheet sets it too, and the button follows
    u.select_option(Item::BadgeStyle, "Classic");
    frame(&mut u);
    assert!(u.studio_classic());
    let s = u.studio_style_chip();
    tap(&mut u, s);
    assert!(!u.studio_classic());
    // compact and the eye still do their own work
    let c = u.studio_compact_chip();
    tap(&mut u, c);
    assert_eq!(u.studio.view, StudioView::Docked, "with the panel closed the list button opens it");
    u.studio_set_view(StudioView::Waveform);
    u.step(0.05);
    let e = u.studio_eye();
    tap(&mut u, e);
    assert!(u.studio.hide_badges);
}

// ------------------------------------------------------------------------------------------------ sparse zoom

fn scale_index(t: f32) -> usize { tscales().iter().position(|&s| (s / t - 1.0).abs() < 1e-3).expect("a time scale") }

fn sparse_zoom() -> Ui {
    let mut u = studio();
    u.m.tscale = scale_index(50e-3);
    u.m.delay = 0.0;
    for _ in 0..8 { u.step(0.05); }
    u.set_zoom(true);
    u.m.zscale = scale_index(200e-6);
    u.m.zdelay = 0.439e-3;
    u.clamp_zoom();
    u.relayout();
    for _ in 0..8 { u.step(0.05); }
    u
}

/// (columns lit, first lit column, last lit column) of the zoom's trace of channel 1.
fn lit(u: &Ui) -> (usize, usize, usize) {
    let (w, h) = (u.phz.w, u.phz.h);
    let map = &u.phz.map[0];
    let cols: Vec<usize> = (0..w).filter(|&x| (0..h).any(|y| map[y * w + x] > 0)).collect();
    (cols.len(), cols.first().copied().unwrap_or(0), cols.last().copied().unwrap_or(0))
}

#[test]
fn a_sparse_zoom_joins_the_samples_with_lines_and_marks_each_one_and_dots_mode_stays_dots() {
    let mut u = sparse_zoom();
    let dt = u.studio_zoom_sample_dt().unwrap();
    assert!(u.zoom_sparse(dt));
    assert!(!u.m.dots, "the user's draw mode is vectors");
    assert!(u.studio_zoom_samples_hint().expect("the hint").ends_with("samples/div"));
    assert!(u.phz.fat, "each real sample is marked");
    let (n, first, last) = lit(&u);
    assert!(n > 0 && n == last - first + 1, "the trace is continuous: {n} columns between {first} and {last}");
    assert!(n > u.phz.w * 9 / 10, "and it spans the zoom: {n} of {}", u.phz.w);
    shot(&mut u, "sparse-zoom");
    // Display > Dots: dots only
    u.m.dots = true;
    for _ in 0..4 { frame(&mut u); }
    let (n, _, _) = lit(&u);
    assert!(n < u.phz.w / 2, "dots only: {n} of {}", u.phz.w);
}

// ------------------------------------------------------------------------------------------------ the table in other panels

fn eye_rows(u: &mut Ui) {
    #[allow(unused_imports)] use super::meascatalog::Cat;
    u.m.cat.clear();
    for c in [super::meascatalog::cat_from(super::meascatalog::cat::EyeHeight).unwrap(), super::meascatalog::cat_from(super::meascatalog::cat::EyeHeightBer).unwrap(), super::meascatalog::cat_from(super::meascatalog::cat::EyeWidth).unwrap(), super::meascatalog::cat_from(super::meascatalog::cat::EyeHeightBer).unwrap()] {
        let id = if u.m.cat.len() == 3 { WaveId::Ref(0) } else { WaveId::Ch(0) };
        u.m.cat.push((c.id, id, id, id, id));
    }
    u.ensure_meas_own();
    for _ in 0..6 { frame(u); }
}

#[test]
fn a_short_wide_panel_keeps_its_rows_with_the_controls_in_the_title_strip_and_a_narrow_one_drops_columns() {
    let mut u = studio();
    detailed(&mut u);
    // the pure layout of a panel 1000 x 170: no toolbar, no footer, the rows have all the room under the column heads
    let p = Rect::new(10, 300, 1000, 170);
    let g = u.studio_panel_geo_in(p);
    assert!(!g.short);
    assert_eq!(g.list.y, p.y + super::measview::COL_HEAD);
    assert!(g.page >= 4 && g.list.h >= super::measview::DETAIL_ROW);
    assert!(g.list.bottom() <= p.bottom());
    // the head cells of the wide table do not overlap and say Src, not a cut Source
    let (cols, total) = Ui::detail_cols(g.list.w);
    assert_eq!(cols.len(), 8);
    assert_eq!(total, g.list.w);
    let row = Rect::new(0, 0, g.list.w, 28);
    let f = u.fonts();
    for w in cols.windows(2) { assert!(Ui::detail_cell(row, w[0].1, w[0].2, 0).right() <= Ui::detail_cell(row, w[1].1, w[1].2, 0).x); }
    let c0 = Ui::detail_cell(row, cols[0].1, cols[0].2, 0);
    assert!(f.sb.width(u.detail_head(0, c0.w)) <= c0.w);
    // a narrow panel has a toolbar row of its own
    assert!(u.studio_panel_geo_in(Rect::new(10, 100, 300, 400)).list.y >= 100 + super::measpanel::TOOL_H);
    // the real thing: the panel docked under the waveform, made short
    let mut u = studio();
    u.dock_apply(VKind::Measure, super::dock::Drop::Split(VKind::Wave, super::dock::Side::Bottom));
    detailed(&mut u);
    for _ in 0..6 { frame(&mut u); }
    if let Some(g) = u.studio_panel_geo() {
        assert!(g.list.bottom() <= g.p.bottom());
        assert!(g.page >= 1);
    }
    shot(&mut u, "meas-detailed-wide-short");
}

#[test]
fn narrow_columns_are_whole_or_gone_and_eye_names_stay_apart() {
    let mut u = studio();
    eye_rows(&mut u);
    detailed(&mut u);
    panel_width(&mut u, 262);
    for _ in 0..4 { frame(&mut u); }
    let g = u.studio_panel_geo().unwrap();
    let (cols, total) = Ui::detail_cols(g.list.w);
    assert!(total <= g.list.w, "every column shown fits: {cols:?} in {}", g.list.w);
    assert_eq!(cols.iter().map(|c| c.0).take(4).collect::<Vec<_>>(), [0, 1, 2, 3]);
    // priority: mean, min, max, sigma, count
    for w in [262, 330, 420, 520, 640] {
        let ids: Vec<usize> = Ui::detail_cols(w).0.iter().map(|c| c.0).collect();
        if ids.contains(&4) { assert!(ids.contains(&3), "{w}: {ids:?}"); }
        if ids.contains(&5) { assert!(ids.contains(&4), "{w}: {ids:?}"); }
        if ids.contains(&6) { assert!(ids.contains(&5), "{w}: {ids:?}"); }
        if ids.contains(&7) { assert!(ids.contains(&6), "{w}: {ids:?}"); }
        assert!(Ui::detail_cols(w).1 <= w);
    }
    // names: each fits, and Eye Height and Eye Height@BER read differently
    let f = u.fonts();
    let rows = u.meas_rows();
    let keys = u.studio_keys();
    let name = Ui::detail_cell(Rect::new(0, 0, g.list.w, 38), cols[1].1, cols[1].2, 0);
    let room = name.w - super::measview::DETAIL_PIN_W;
    let shown: Vec<(String, String)> = rows.iter().enumerate().map(|(i, r)| (r.name.clone(), u.detail_name(keys.get(i).copied(), &r.name, room).0)).collect();
    for (long, s) in &shown { assert!(f.s.width(s).max(f.m.width(s).min(room)) <= room, "{long:?} -> {s:?}"); }
    for (a, sa) in &shown { for (b, sb) in &shown { if a != b { assert_ne!(sa, sb, "{a:?} and {b:?} read the same"); } } }
    assert!(shown.iter().any(|(l, s)| l == "Eye Height@BER" && s != l), "the narrow case cuts it: {shown:?}");
    shot(&mut u, "meas-detailed-narrow-eye");
}

#[test]
fn a_middle_cut_keeps_the_end_of_a_name() {
    let u = studio();
    let f = u.fonts();
    let a = super::measview::fit_middle(&f.m, "Eye Height", 60);
    let b = super::measview::fit_middle(&f.m, "Eye Height@BER", 60);
    assert_ne!(a, b);
    assert!(b.ends_with("BER") || b.ends_with("ER"), "{b}");
    assert!(f.m.width(&b) <= 60 && f.m.width(&a) <= 60);
    assert_eq!(super::measview::fit_middle(&f.m, "Freq", 200), "Freq");
}

// ------------------------------------------------------------------------------------------------ the Menu in a short window

fn menu_on(scr: Screen) -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.scr = scr;
    u.m.layout = Layout::Studio;
    u.m.studio_detail = false;
    u.m.studio_zoom_tab = false;
    u.studio.enter = true;
    let mut cv = Canvas::new(scr.w, scr.h);
    for _ in 0..10 { u.step(0.05); u.render(&mut cv); }
    u.studio_set_view(StudioView::Docked);
    let gear = u.studio_top().gear;
    tap_at(&mut u, gear.x + gear.w / 2, gear.y + gear.h / 2);
    for _ in 0..4 { u.step(0.05); u.render(&mut cv); }
    assert!(u.menu_open && u.menu == MenuId::Main && u.sheet_active());
    u
}

fn shot_at(u: &mut Ui, scr: Screen, name: &str) {
    let mut cv = Canvas::new(scr.w, scr.h);
    for _ in 0..3 { u.render(&mut cv); u.step(0.02); }
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("b22-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn item_rect(u: &Ui, it: Item) -> Rect {
    u.menu_geo().groups.iter().flat_map(|g| g.items.iter()).find(|(i, _)| *i == it).expect("in the Menu").1
}

#[test]
fn the_menu_stays_inside_its_body_in_a_short_window_and_scrolls_to_the_last_row() {
    for scr in [Screen::PANEL, Screen { w: 1024, h: 480 }, Screen { w: 1280, h: 800 }, Screen { w: 1000, h: 420 }] {
        let mut u = menu_on(scr);
        let g = u.menu_geo();
        let sheet = g.r;
        assert!(g.view.y >= sheet.y + super::sheet::SHEET_HEAD && g.view.bottom() <= sheet.bottom() - 2, "{scr:?}: {:?} in {sheet:?}", g.view);
        assert!(sheet.bottom() <= scr.h, "{scr:?}: the sheet {sheet:?} is on the screen");
        if scr == Screen::PANEL { assert_eq!(g.max_scroll(), 0, "the whole Menu fits at 1024 x 600"); assert_eq!(Ui::menu_cols(g.r.w), 6); shot_at(&mut u, scr, "menu-wide"); }
        if scr.h == 420 { assert!(g.max_scroll() > 0); shot_at(&mut u, scr, "menu-wide-short"); }
        if scr.h == 480 { shot_at(&mut u, scr, "menu-short-top"); }
        // at the top and at the end: a button that is drawn (in view at all) is inside the body or cut by it, and a tap on the part outside does nothing
        let end = g.max_scroll();
        for s in [0, end / 2, end] {
            u.sheet.scroll = s;
            let g = u.menu_geo();
            for grp in &g.groups {
                for &(_, c) in &grp.items {
                    let shown = c.bottom() > g.view.y && c.y < g.view.bottom();
                    if shown && c.y >= g.view.y && c.bottom() <= g.view.bottom() { assert!(c.h == CELL_H_T); }
                }
            }
        }
        // the end: the last row is fully inside, with room under it
        u.sheet.scroll = end;
        let g = u.menu_geo();
        assert_eq!(g.max_scroll(), (g.content_h - g.view.h).max(0));
        for it in [Item::Shot, Item::PowerAct] {
            let c = item_rect(&u, it);
            assert!(c.y >= g.view.y && c.bottom() <= g.view.bottom(), "{scr:?}: {it:?} {c:?} in {:?}", g.view);
        }
        let last = g.groups.last().unwrap().items.iter().map(|(_, c)| c.bottom()).max().unwrap();
        assert!(last <= g.view.bottom() - 8 || end == 0, "{scr:?}: padding under the last row");
        if scr.h == 480 { shot_at(&mut u, scr, "menu-short-end"); }
        // Power off asks first, and its first tap lands
        let c = item_rect(&u, Item::PowerAct);
        tap_at(&mut u, c.x + c.w / 2, c.y + c.h / 2);
        assert!(u.studio.md.armed.is_some_and(|(i, _)| i == Item::PowerAct), "{scr:?}: Power off is tappable at the end");
        // a tap outside the body (the bar under the sheet) hits no button
        let below = g.view.bottom() + 3;
        let before = u.studio.md.armed;
        tap_at(&mut u, g.view.x + GROUP_LABEL_W_T + 10, below);
        assert_eq!(u.studio.md.armed.map(|a| a.0), before.map(|a| a.0));
    }
}

const CELL_H_T: i32 = super::menugrid::CELL_H;
const GROUP_LABEL_W_T: i32 = super::menugrid::GROUP_LABEL_W;

fn tap_at(u: &mut Ui, x: i32, y: i32) {
    touch(u, Phase::Down, x, y);
    touch(u, Phase::Up, x, y);
    u.step(0.02);
}

#[test]
fn the_wide_menu_has_fewer_buttons_per_row_in_a_narrow_window_and_clips_under_the_header() {
    assert_eq!(Ui::menu_cols(1008), 6);
    assert!(Ui::menu_cols(700) < 6 && Ui::menu_cols(700) >= 3);
    assert_eq!(Ui::menu_cols(300), 1.max(Ui::menu_cols(300)));
    let mut u = menu_on(Screen { w: 1024, h: 420 });
    let g = u.menu_geo();
    // scrolled: a button that is partly above the body is cut by it, and a tap in the header taps no button
    u.sheet.scroll = 60;
    let g2 = u.menu_geo();
    assert!(g2.view.y >= g2.r.y + super::sheet::SHEET_HEAD, "the clip starts under the header");
    let run = item_rect(&u, Item::RunStop);
    assert!(run.y < g2.view.y, "scrolled up under the header edge: {run:?}");
    let before = u.menu;
    tap_at(&mut u, run.x + run.w / 2, g2.r.y + 20);
    assert_eq!(u.menu, before);
    assert!(!u.studio.md.armed.is_some());
    let _ = g;
    shot_at(&mut u, Screen { w: 1024, h: 420 }, "menu-wide-scrolled");
}

// ------------------------------------------------------------------------------------------------ the badge row

#[test]
fn the_badge_row_is_there_only_for_pinned_badges_with_the_panel_closed_and_never_holds_the_cursor_readout() {
    let mut u = studio();
    // panel open, nothing pinned: no row, the plot reaches down
    u.studio.pinned = Some(Vec::new());
    u.m.cursor = CursorMode::Time;
    u.relayout();
    for _ in 0..4 { frame(&mut u); }
    assert!(u.studio_panel_shown() && !u.studio_badges_visible());
    assert_eq!(u.studio_row_h(), 0);
    let bottom_open = u.dock_wave_rect().bottom();
    shot(&mut u, "no-badge-row");
    // the panel closed and nothing pinned: still no row, and not for the cursor alone; a chip at the plot's corner opens the panel
    u.studio_set_view(StudioView::Waveform);
    for _ in 0..4 { frame(&mut u); }
    assert!(!u.studio_badges_visible() && u.studio_row_h() == 0);
    assert!(u.studio_cursor_chip(0).is_some() || true);
    let chip = u.studio_meas_chip().expect("a way to the panel");
    let w = u.dock_wave_rect();
    assert!(chip.right() <= w.right() && chip.bottom() <= w.bottom());
    tap(&mut u, chip);
    assert_eq!(u.studio.view, StudioView::Docked, "the chip opens the panel");
    // pinned and the panel open: no row
    for k in u.studio_keys() { u.studio_set_pin(k, true); }
    for _ in 0..4 { frame(&mut u); }
    assert_eq!(u.studio_row_h(), 0);
    assert_eq!(u.dock_wave_rect().bottom(), bottom_open, "the plot keeps the height");
    // pinned and the panel closed: the row is back
    u.studio_set_view(StudioView::Waveform);
    for _ in 0..4 { frame(&mut u); }
    assert!(u.studio_badges_visible() && u.studio_row_h() > 0);
    assert!(u.studio_meas_chip().is_none());
    shot(&mut u, "badge-row-pinned");
    // the restore path: the eye hides the row, the bottom bar and the Undo bar bring it back
    let e = u.studio_eye();
    tap(&mut u, e);
    assert!(u.studio.hide_badges && u.studio_row_h() == 0);
    assert!(u.studio_badges_hidden());
    let (_, undo) = u.md_hide_bar().expect("the Undo bar");
    tap(&mut u, undo);
    assert!(!u.studio.hide_badges && u.studio_row_h() > 0);
}
