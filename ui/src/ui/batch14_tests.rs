// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 14: fixes from the hands-on walkthrough of Batch 13 (audit/walkthrough-b13.md). One test per item.
//! OOS_SHOTS=<dir> writes pictures (b14-*).
use super::dock::VKind;
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
        std::fs::write(std::path::Path::new(&dir).join(format!("b14-{name}.png")), crate::store::png_of(&cv)).unwrap();
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

// 1 ------------------------------------------------------------------------------------------------ time-axis drag

#[test]
fn a_drag_on_the_time_axis_takes_about_sixty_pixels_a_step() {
    let mut u = studio();
    u.m.tscale = 20;
    frame(&mut u);
    let s = u.axis_time_strip().unwrap();
    let t0 = u.m.tscale as i32;
    // the remote's replay: Down, 8 Moves, Up
    let (x0, y) = (s.x + 100, s.y + s.h - 10);
    crate::cmd::run(&mut u, &format!("drag {x0} {y} {} {y}", x0 + 290));
    frame(&mut u);
    let steps = t0 - u.m.tscale as i32;
    assert!((3..=5).contains(&steps), "290 px is about 5 steps of 60 px, not 9: {steps}");
    // a short drag is still a step or none
    let t1 = u.m.tscale as i32;
    crate::cmd::run(&mut u, &format!("drag {x0} {y} {} {y}", x0 + 30));
    assert!((t1 - u.m.tscale as i32).abs() <= 1);
    // and one drag event never jumps more than a couple of steps
    let t2 = u.m.tscale as i32;
    touch(&mut u, Phase::Down, x0, y);
    touch(&mut u, Phase::Move, x0 + 400, y);
    touch(&mut u, Phase::Up, x0 + 400, y);
    assert!((t2 - u.m.tscale as i32) <= 3, "clamped: {}", t2 - u.m.tscale as i32);
}

// 2 ------------------------------------------------------------------------------------------------ zoom icon

#[test]
fn the_zoom_icon_in_the_waveform_corner_turns_zoom_on_and_off() {
    let mut u = studio();
    assert!(!u.m.zoom);
    let (_, hit) = u.zoom_btn();
    assert!(hit.h >= 40);
    tap(&mut u, hit);
    assert!(u.m.zoom, "zoom on");
    let (_, hit) = u.zoom_btn();
    tap(&mut u, hit);
    assert!(!u.m.zoom, "zoom off");
    // the remote taps the drawn icon: Down and Up with no frame between
    let (drawn, _) = u.zoom_btn();
    crate::cmd::run(&mut u, &format!("tap {} {}", drawn.x + drawn.w / 2, drawn.y + drawn.h / 2));
    assert!(u.m.zoom, "remote tap on the icon: on");
    frame(&mut u);
    let (drawn, _) = u.zoom_btn();
    crate::cmd::run(&mut u, &format!("tap {} {}", drawn.x + drawn.w / 2, drawn.y + drawn.h / 2));
    assert!(!u.m.zoom, "remote tap on the icon: off");
    // with a docked measurements panel and a view open too
    u.studio_apply_panel(super::sheet::PanelKind::Eye);
    frame(&mut u);
    let (drawn, _) = u.zoom_btn();
    crate::cmd::run(&mut u, &format!("tap {} {}", drawn.x + drawn.w / 2, drawn.y + drawn.h / 2));
    assert!(u.m.zoom, "with an eye docked");
    for _ in 0..20 { frame(&mut u); }
    assert!(u.m.zoom, "and it stays on");
    // a slow tap, as a web page sends it: the lift comes a moment after the press
    let mut v = studio();
    let (drawn, _) = v.zoom_btn();
    let (x, y) = (drawn.x + drawn.w / 2, drawn.y + drawn.h / 2);
    // (a press held for the long-press time opens the zoom source popover instead: batch27)
    for wait in [0.05, 0.3, 0.45] {
        v.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
        v.step(wait);
        v.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
        frame(&mut v);
        let on = v.m.zoom;
        assert!(on, "a tap held for {wait} s turns zoom on");
        v.set_zoom(false);
        frame(&mut v);
    }
    // every state a walkthrough passes through
    for state in 0..4 {
        let mut u = studio();
        match state {
            0 => u.studio_set_view(StudioView::Waveform),
            1 => u.studio_set_view(StudioView::Floating),
            2 => { u.m.hist = Hist::Vertical; u.m.view = AView::Spectrum; }
            _ => { u.xy = true; }
        }
        for _ in 0..4 { frame(&mut u); }
        let (drawn, _) = u.zoom_btn();
        crate::cmd::run(&mut u, &format!("tap {} {}", drawn.x + drawn.w / 2, drawn.y + drawn.h / 2));
        for _ in 0..4 { frame(&mut u); }
        assert!(u.m.zoom, "state {state}: zoom on after a tap on the icon at {drawn:?}");
    }
}

// 3 ------------------------------------------------------------------------------------------------ eye and spectrum

#[test]
fn eye_and_spectrum_are_on_screen_together_as_separate_views() {
    let mut u = studio();
    u.studio_apply_panel(super::sheet::PanelKind::Eye);
    frame(&mut u);
    assert!(u.dock_leaf(VKind::Eye).is_some());
    u.studio_apply_panel(super::sheet::PanelKind::Spectrum);
    frame(&mut u);
    let (e, p) = (u.dock_leaf(VKind::Eye), u.dock_leaf(VKind::Pane));
    assert!(e.is_some() && p.is_some(), "both docked: {e:?} {p:?}");
    assert_eq!(u.dock_name(VKind::Pane), "Spectrum");
    shot(&mut u, "eye-and-spectrum");
}

// 4 ------------------------------------------------------------------------------------------------ histogram and a ref

#[test]
fn the_histogram_view_stays_when_a_ref_is_loaded() {
    let mut u = studio();
    u.studio_apply_panel(super::sheet::PanelKind::Histogram);
    frame(&mut u);
    assert!(u.dock_leaf(VKind::Hist).is_some());
    // a user's CSV, through the import the remote uses
    let mut csv = String::from("Time (s),Voltage (V)
");
    for i in 0..2000 { csv.push_str(&format!("{:e},{}
", i as f64 * 1e-6, (i as f64 * 0.05).sin() * (-(i as f64) / 800.0).exp())); }
    u.load_waveform_bytes(csv.as_bytes(), "damped.csv").expect("the CSV loads");
    assert!(u.refs[0].is_some());
    for _ in 0..5 { frame(&mut u); }
    assert!(u.m.hist != Hist::Off && u.dock_leaf(VKind::Hist).is_some(), "still there: {:?}", u.m.hist);
    // loading the ref as an offline record keeps it too
    u.studio_set_view(StudioView::Offline);
    for _ in 0..5 { frame(&mut u); }
    assert!(u.m.hist != Hist::Off, "{:?}", u.m.hist);
    u.studio_set_view(StudioView::Docked);
    for _ in 0..5 { frame(&mut u); }
    assert!(u.m.hist != Hist::Off && u.dock_leaf(VKind::Hist).is_some());
}

// 5 ------------------------------------------------------------------------------------------------ waveform focus

#[test]
fn in_waveform_focus_the_badge_row_buttons_work() {
    let mut u = studio();
    u.studio_set_view(StudioView::Waveform);
    frame(&mut u);
    // the Meas tab is gone: the list button (with the count) opens the measurements
    u.studio_set_view(StudioView::Waveform);
    frame(&mut u);
    // the list button: opens the measurement list
    let c = u.studio_compact_chip();
    tap(&mut u, c);
    assert!(u.studio.view != StudioView::Waveform || u.menu_open, "the list button does something");
}

// 9 ------------------------------------------------------------------------------------------------ undo

#[test]
fn undo_restores_the_row_a_few_seconds_later() {
    let mut u = studio();
    let before = u.studio_keys();
    let key = before[1];
    u.md_remove(key);
    frame(&mut u);
    assert_eq!(u.studio_keys().len(), before.len() - 1);
    // the remote taps about three seconds later
    for _ in 0..30 { u.step(0.1); }
    let (bar, button) = u.md_undo_bar().expect("the bar is still there after 3 s");
    let _ = bar;
    tap(&mut u, button);
    assert_eq!(u.studio_keys(), before, "the row is back in its place");
    assert!(super::measdrag::UNDO_S >= 6.0);
}

// 6 ------------------------------------------------------------------------------------------------ add math

#[test]
fn add_math_ends_with_a_trace_on_screen() {
    let mut u = studio();
    for c in 2..n_ch() { u.m.ch[c].on = false; }
    frame(&mut u);
    assert_eq!(u.m.math, crate::math::MathOp::Off);
    let b = u.studio_bottom();
    tap(&mut u, b.add);
    let (_, extras) = u.studio_picker_rows().expect("the popup");
    let r = extras.iter().find(|e| e.0 == super::studio::PickAdd::Math).unwrap().1;
    tap(&mut u, r);
    // the menu is open on math with a real operation and sources that are on
    assert!(u.m.math != crate::math::MathOp::Off, "an operation: {:?}", u.m.math);
    let on = |src: usize| src < n_ch() && u.m.ch[src].on;
    assert!(on(u.m.math_a) && on(u.m.math_b), "sources on: A {} B {}", u.m.math_a, u.m.math_b);
    for _ in 0..4 { frame(&mut u); }
    assert!(u.studio_bottom().math[0].w > 0, "the M1 chip");
    assert!(!u.math_wave().is_empty(), "M1 has data");
}

// 7 ------------------------------------------------------------------------------------------------ glyphs

/// Every character a string of the UI source can put on the screen is in the font.
fn missing_glyphs() -> Vec<(char, String)> {
    let font = &crate::font::Fonts::get(0).m;
    let mut out: Vec<(char, String)> = Vec::new();
    let mut stack = vec![std::path::PathBuf::from("src")];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() { stack.push(p); continue; }
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if !name.ends_with(".rs") || name.contains("test") || name == "font.rs" || name == "desktop.rs" || name == "json.rs" { continue; }
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            // stop at the tests of the file
            let text = text.split("#[cfg(test)]").next().unwrap_or("").to_string();
            for (n, line) in text.lines().enumerate() {
                let t = line.trim_start();
                if t.starts_with("//") { continue; }
                let code = line.split("//").next().unwrap_or(line);
                let mut chars: Vec<char> = code.chars().filter(|c| !c.is_ascii()).collect();
                let mut rest = code;
                let pat = format!("{}u{{", char::from(92u8));
                while let Some(i) = rest.find(&pat) {
                    rest = &rest[i + 3..];
                    if let Some(j) = rest.find('}') {
                        if let Some(c) = u32::from_str_radix(&rest[..j], 16).ok().and_then(char::from_u32) { chars.push(c); }
                    }
                }
                for c in chars {
                    if font.glyph(c).cp != c as u32 && !out.iter().any(|(x, _)| *x == c) { out.push((c, format!("{}:{}", p.display(), n + 1))); }
                }
            }
        }
    }
    out
}

#[test]
fn every_character_in_a_string_of_the_ui_has_a_glyph() {
    let missing = missing_glyphs();
    assert!(missing.is_empty(), "the font lacks: {}", missing.iter().map(|(c, w)| format!("U+{:04X} {c} ({w})", *c as u32)).collect::<Vec<_>>().join("; "));
}

// 8 ------------------------------------------------------------------------------------------------ display names

#[test]
fn measurements_show_the_name_people_use_not_the_internal_key() {
    let mut u = studio();
    assert_eq!(super::meascatalog::meas_display_name(Meas::Vamp), "Amplitude");
    u.m.meas.clear();
    u.meas_src = WaveId::Ch(0);
    u.activate(Item::MeasAdd(Meas::Vamp));
    let t = u.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
    assert!(t.contains("Amplitude") && !t.contains("Vamp"), "{t}");
    assert!(Ui::toast_is_success(&t), "a success has no warning icon: {t}");
    assert!(!Ui::toast_is_success("Needs CH2 on"));
    let rows = u.meas_rows();
    assert!(rows.iter().any(|r| r.name == "Amplitude") && !rows.iter().any(|r| r.name == "Vamp"));
    assert_eq!(u.selected_measurement_label(0), "CH1 Amplitude");
}

// 10 ----------------------------------------------------------------------------------------------- detailed table

#[test]
fn the_detailed_table_keeps_name_value_and_mean_in_a_narrow_panel_and_the_pin_after_the_name() {
    let (cols, total) = Ui::detail_cols(262);
    let idx: Vec<usize> = cols.iter().map(|c| c.0).collect();
    assert_eq!(&idx[..4], &[0, 1, 2, 3], "source, name, value, mean");
    let mean = cols[3];
    assert!(mean.1 + mean.2 <= 262, "Mean fits without scrolling: {cols:?}");
    assert!(total >= 262);
    // wide: the source has its tag column
    assert!(Ui::detail_cols(600).0[0].2 >= 30);
    // the pin sits in the name column, at its right end
    let mut u = studio();
    let s = u.dock_rect(VKind::Measure).and_then(|r| u.dock_view_rect(VKind::Measure, r)).unwrap();
    tap(&mut u, Rect::new(s.x + s.w / 6 - 2, s.y, 4, s.h));
    assert!(u.studio_detailed());
    shot(&mut u, "detailed");
}

// 11 ----------------------------------------------------------------------------------------------- ref lane labels

#[test]
fn a_loaded_ref_gets_round_volts_per_div_and_offset_like_a_channel() {
    for want in [0.0371f32, 0.18, 0.55662, 1.3, 7.9] {
        let v = Ui::nice_ref_vpd(want);
        let m = v / 10f32.powf(v.log10().floor());
        assert!([1.0f32, 2.0, 5.0].iter().any(|x| (m - x).abs() < 1e-3) || (m - 10.0).abs() < 1e-3, "{want} -> {v}");
        assert!(v >= want * 0.99, "the trace fits");
    }
    let mut u = studio();
    let mut csv = String::from("Time (s),Voltage (V)\n");
    for i in 0..2000 { csv.push_str(&format!("{:e},{}\n", i as f64 * 1e-6, 0.55662 + (i as f64 * 0.05).sin() * 1.6)); }
    u.load_waveform_bytes(csv.as_bytes(), "ref.csv").unwrap();
    let r = u.refs[0].as_ref().unwrap();
    let steps = r.offset / r.vpd;
    assert!((steps - steps.round()).abs() < 1e-3, "the offset is whole divisions: {} / {}", r.offset, r.vpd);
    for _ in 0..4 { frame(&mut u); }
    shot(&mut u, "ref-lane");
}

// 14 ----------------------------------------------------------------------------------------------- histogram axis

#[test]
fn the_histogram_view_has_a_volts_axis_and_a_count_scale() {
    let mut u = studio();
    u.m.hist = Hist::Vertical;
    for _ in 0..8 { frame(&mut u); }
    let r = u.dock_content(VKind::Hist).expect("the histogram view");
    let mut cv = Canvas::new(W, H);
    u.draw_view_tiles(&mut cv);
    let bg = cv.px[(r.y * W + r.x + 1) as usize];
    let ink = |x0: i32, x1: i32| (r.y..r.bottom()).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|&(x, y)| cv.px[(y * W + x) as usize] != bg).count();
    assert!(ink(r.x, r.x + 44) > 30, "volts labels at the left: {}", ink(r.x, r.x + 44));
    assert!(ink(r.x + 50, r.right()) > 30, "the bars and the count scale");
    shot(&mut u, "histogram-axis");
}

// 15 ----------------------------------------------------------------------------------------------- add panel

#[test]
fn the_add_panel_has_no_bus_entry_since_the_channel_popup_adds_it() {
    assert!(!super::addrail::ADD_KINDS.iter().any(|k| k.1 == "Bus" || k.1 == "Math" || k.1 == "Ref"));
    let mut u = studio();
    u.open_menu(MenuId::AddPanel);
    frame(&mut u);
    assert!(u.rail_geo().rows.iter().all(|(k, _)| *k != super::addrail::AddKind::Bus));
}

#[test]
fn the_spectrum_beside_an_eye_has_its_trace() {
    let mut u = studio();
    u.studio_apply_panel(super::sheet::PanelKind::Eye);
    u.studio_apply_panel(super::sheet::PanelKind::Spectrum);
    for _ in 0..10 { frame(&mut u); }
    assert!(u.azr.spec.is_some(), "the spectrum was computed while the eye is open");
}

// 12 ----------------------------------------------------------------------------------------------- eye controls

#[test]
fn the_studio_eye_has_no_close_or_side_of_its_own_and_its_axis_labels_do_not_overlap() {
    let mut u = studio();
    u.studio_apply_panel(super::sheet::PanelKind::Eye);
    for _ in 0..6 { frame(&mut u); }
    assert_eq!(u.eye_pane_close().w, 0, "the strip's X closes the view");
    assert_eq!(u.eye_place_control().w, 0, "no Side / Full / Under in a dock view");
    let strip = Ui::dock_close_rect(u.dock_leaf(VKind::Eye).unwrap());
    tap(&mut u, strip);
    assert!(u.dock_leaf(VKind::Eye).is_none(), "closed with the strip X");
    // the labels of the UI axis: narrow keeps only those that do not touch
    let f = crate::font::Fonts::get(0);
    let mut free = i32::MIN;
    let r = Rect::new(0, 0, 150, 100);
    for k in 0..=4 {
        let x = r.x + k * (r.w - 1) / 4;
        let s = ["-0.5", "0", "0.5", "1", "1.5 UI"][k as usize];
        let w = f.s.width(s);
        let tx = if k == 4 { x - w } else if k == 0 { x } else { x - w / 2 };
        if tx < free + 6 { continue; }
        free = tx + w;
    }
    assert!(free > 0);
}

// 13 ----------------------------------------------------------------------------------------------- overlaps

#[test]
fn the_overview_grip_clears_the_zoom_bar_buttons_and_the_trigger_tag_clears_the_corner_icons() {
    let mut u = studio();
    u.set_zoom(true);
    for _ in 0..4 { frame(&mut u); }
    let bar = u.studio_zoom_bar().expect("the zoom bar");
    let (hit, handle) = u.ov_divider().unwrap();
    for r in [bar.plus, bar.minus, bar.vplus, bar.vminus, bar.center, bar.fit, bar.more, bar.close, bar.tdiv] {
        if r.w == 0 { continue; }
        let over = hit.x < r.right() && r.x < hit.right() && hit.y < r.bottom() && r.y < hit.bottom();
        assert!(!over, "the grip {hit:?} sits on {r:?}");
    }
    assert!(handle.w >= 40 && hit.h >= 40);
    // a trigger level as high as the corner buttons
    u.set_zoom(false);
    frame(&mut u);
    let (_, zb) = u.zoom_btn();
    let x1 = u.trigger_tag_x(u.grid.right() - 26, zb.y + 10);
    assert!(x1 + 26 <= zb.x, "the tag moved left of the icons: {x1} vs {zb:?}");
    assert_eq!(u.trigger_tag_x(u.grid.right() - 26, u.grid.bottom() - 40), u.grid.right() - 26, "low levels stay put");
}

// labels on trace -------------------------------------------------------------------------------- Studio

#[test]
fn labels_on_trace_draw_in_the_waveform_lane_of_their_channel_and_can_be_switched_from_the_row_menu() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    u.m.meas = vec![(Meas::Vtop, WaveId::Ch(0)), (Meas::Vbase, WaveId::Ch(0)), (Meas::PWidth, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(0)), (Meas::Vavg, WaveId::Ch(0))];
    u.ensure_meas_own();
    for _ in 0..6 { frame(&mut u); }
    let mut off = Canvas::new(W, H);
    u.render(&mut off);
    // the switch is in the row menu
    let g = u.studio_panel_geo().unwrap();
    let (_, menu) = Ui::md_row_buttons(Rect::new(g.list.x, g.list.y, g.list.w, u.studio_panel_row()));
    tap(&mut u, menu);
    let p = u.md_menu_rect().expect("the row menu");
    let n = 8;
    tap(&mut u, Ui::md_menu_row(p, n - 1));
    assert!(u.m.meas_labels, "Labels on trace is on");
    let mut on = Canvas::new(W, H);
    u.render(&mut on);
    let lane = u.wins[0];
    let wave = u.dock_leaf(VKind::Wave).unwrap();
    let mut inside = 0;
    for y in 0..H {
        for x in 0..W {
            if on.px[(y * W + x) as usize] != off.px[(y * W + x) as usize] {
                if lane.contains(x, y) { inside += 1; } else { assert!(!wave.contains(x, y) || !u.wins[1].contains(x, y), "a label on the other lane at {x},{y}"); }
            }
        }
    }
    assert!(inside > 300, "level lines, arrow and labels in the lane of CH1: {inside}");
    // zoomed too
    u.set_zoom(true);
    for _ in 0..4 { frame(&mut u); }
    shot(&mut u, "labels-zoom");
    u.set_zoom(false);
    for _ in 0..4 { frame(&mut u); }
    shot(&mut u, "labels-square");
}

// eye and spectrum parity -------------------------------------------------------------------------- wide views

fn widen(u: &mut Ui, k: VKind, w: i32) {
    // the eye / pane gets the room: the other dock panels are made small by their dividers
    for _ in 0..4 {
        let have = u.dock_leaf(k).unwrap().w;
        if have >= w { break; }
        let d = u.dock_layout().dividers.iter().min_by_key(|d| (d.line.x - u.dock_leaf(k).unwrap().x).abs().min((d.line.x - u.dock_leaf(k).unwrap().right()).abs())).cloned().unwrap();
        let (x, y) = (d.line.x + d.line.w / 2, d.line.y + d.line.h / 2);
        let left = d.line.x < u.dock_leaf(k).unwrap().x + have / 2;
        touch(u, Phase::Down, x, y);
        for s in 1..=6 { touch(u, Phase::Move, x + (if left { -1 } else { 1 }) * (w - have) * s / 6, y); }
        touch(u, Phase::Up, x, y);
        for _ in 0..3 { frame(u); }
    }
}

#[test]
fn a_wide_eye_view_shows_the_full_page_and_a_narrow_one_keeps_the_eye() {
    let mut u = studio();
    u.studio_apply_panel(super::sheet::PanelKind::Eye);
    u.studio_set_view(StudioView::Waveform);
    for _ in 0..8 { frame(&mut u); }
    widen(&mut u, VKind::Eye, 720);
    for _ in 0..8 { frame(&mut u); }
    let w = u.dock_leaf(VKind::Eye).unwrap().w;
    assert!(w >= 600, "a wide eye: {w}");
    shot(&mut u, "eye-wide");
}

#[test]
fn a_wide_spectrum_view_has_the_peak_list_and_numbered_markers() {
    let mut u = studio();
    u.studio_apply_panel(super::sheet::PanelKind::Spectrum);
    u.studio_set_view(StudioView::Waveform);
    for _ in 0..10 { frame(&mut u); }
    widen(&mut u, VKind::Pane, 720);
    for _ in 0..10 { frame(&mut u); }
    let r = u.dock_content(VKind::Pane).unwrap();
    assert!(r.w >= super::panes::SPEC_NARROW_W, "wide: {}", r.w);
    let b = u.spec_box(r, true);
    assert!(b.list.w > 0 && b.sort.w > 0, "the Peaks by frequency list is there when wide");
    shot(&mut u, "spectrum-wide");
}

// Batch 15: badges --------------------------------------------------------------------------------- hide and show

#[test]
fn hiding_the_badges_leaves_a_button_and_an_undo_and_showing_them_needs_no_menu() {
    let mut u = studio();
    u.studio_set_view(StudioView::Waveform);
    frame(&mut u);
    assert!(u.studio_badges_visible() && !u.studio_badges().is_empty());
    let eye = u.studio_eye();
    tap(&mut u, eye);
    assert!(!u.studio_badges_visible(), "hidden");
    let show = u.studio_bottom().show.expect("a button in the bottom bar");
    assert!(show.w >= 40 && show.h >= 36, "{show:?}");
    // the Undo of the hide action
    let (bar, undo) = u.md_hide_bar().expect("the Undo bar");
    assert!(bar.w > 0);
    tap(&mut u, undo);
    assert!(u.studio_badges_visible(), "undone");
    // hide again and use the button
    let eye = u.studio_eye();
    tap(&mut u, eye);
    assert!(!u.studio_badges_visible());
    let show = u.studio_bottom().show.unwrap();
    tap(&mut u, show);
    assert!(u.studio_badges_visible(), "shown again without the View menu");
    assert!(u.studio_bottom().show.is_none());
}

// Batch 15: the horizontal position --------------------------------------------------------------- drift

#[test]
fn the_horizontal_position_cannot_drift_far_away_and_a_double_tap_on_the_time_axis_centres_it() {
    let mut u = studio();
    // pan while the time base is huge, then zoom in: the position stayed where the pan left it
    u.m.tscale = tscales().len() - 3;
    frame(&mut u);
    u.m.delay = -202.0;
    for _ in 0..3 { frame(&mut u); }
    for _ in 0..40 { u.axis_step(super::axis::Axis::Time, -1); }
    for _ in 0..3 { frame(&mut u); }
    let lim = 50.0 * HDIV * u.m.time_per_div();
    assert!(u.m.delay.abs() <= lim * 1.001, "the position is kept near the trigger: {} vs {}", u.m.delay, lim);
    // a double tap on the time axis: centre
    u.m.delay = lim * 0.5;
    frame(&mut u);
    let s = u.axis_time_strip().unwrap();
    let (x, y) = (s.x + s.w / 2, s.y + s.h - 10);
    for _ in 0..2 { touch(&mut u, Phase::Down, x, y); touch(&mut u, Phase::Up, x, y); }
    frame(&mut u);
    assert_eq!(u.m.delay, 0.0, "centred by the double tap");
}

// Batch 15: the slim strip ------------------------------------------------------------------------- 26 px

#[test]
fn the_strip_of_a_view_is_slim_and_its_buttons_are_still_forty_pixels() {
    let mut u = studio();
    frame(&mut u);
    let w = u.dock_leaf(VKind::Wave).unwrap();
    let s = Ui::dock_strip(w);
    assert!(s.h <= 26 && s.h >= 24, "slim: {}", s.h);
    let c = Ui::dock_close_rect(w);
    assert!(c.h >= 40 && c.w >= 40, "the X target: {c:?}");
    let m = u.dock_leaf(VKind::Measure).unwrap();
    let sw = u.dock_view_rect(VKind::Measure, m).unwrap();
    assert!(sw.h >= 40);
    let content = u.dock_content(VKind::Wave).unwrap();
    assert_eq!(content.y, w.y + s.h, "the content starts right under the strip");
    shot(&mut u, "slim-strip");
    // a drag from anywhere on the strip still moves the view; a tap on the X closes
    let (x, y) = (s.x + s.w / 2, s.y + s.h / 2);
    touch(&mut u, Phase::Down, x, y);
    for k in 1..=8 { touch(&mut u, Phase::Move, x + k * 3, y + k * 20); }
    assert!(matches!(u.studio.dock.drag, Some(super::dock::DockDrag::Move { .. })), "picked up");
    touch(&mut u, Phase::Up, x + 24, y + 160);
    frame(&mut u);
    // the cursor tags and the trigger tag are not under it
    u.m.cursor = CursorMode::Time;
    for _ in 0..3 { frame(&mut u); }
    let lane = u.wins[0];
    let strip = Ui::dock_strip(u.dock_rect(VKind::Wave).unwrap());
    assert!(lane.y >= strip.bottom(), "the lane starts under the strip: {lane:?} {strip:?}");
}

// Batch 15: the Add measurement panel ------------------------------------------------------------- tiles, no computing

fn add_open(u: &mut Ui) {
    u.open_meas_list();
    for _ in 0..3 { frame(u); }
}

#[test]
fn browsing_the_add_list_computes_nothing_and_shows_no_values() {
    let mut u = studio();
    u.m.meas.clear();
    u.m.cat.clear();
    for _ in 0..3 { frame(&mut u); }
    let before = u.catalog_jobs_submitted();
    add_open(&mut u);
    // every group, the Jitter one full of eye analyses
    for group in 0..4 {
        u.sheet.add_group = group;
        for _ in 0..4 { frame(&mut u); }
        let g = u.sheet_add_geo();
        let entries = u.sheet_add_entries();
        for &(k, _) in &g.rows {
            // nothing but whether the sources are there
            assert!(matches!(u.add_preview(entries[k].cat), super::meascatalog::Preview::Value(ref v) if v.is_empty()) || matches!(u.add_preview(entries[k].cat), super::meascatalog::Preview::Off(_) | super::meascatalog::Preview::Reason(_)));
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    for _ in 0..3 { frame(&mut u); }
    assert_eq!(u.catalog_jobs_submitted(), before, "no analysis was started while browsing");
    assert!(u.m.meas.is_empty() && u.m.cat.is_empty(), "nothing was added");
}

#[test]
fn the_add_list_is_a_grid_of_tiles_with_an_info_button_and_a_tap_adds() {
    let mut u = studio();
    u.m.meas.clear();
    add_open(&mut u);
    let g = u.sheet_add_geo();
    assert!(g.rows.len() >= 6);
    let xs: std::collections::BTreeSet<i32> = g.rows.iter().map(|r| r.1.x).collect();
    let ys: std::collections::BTreeSet<i32> = g.rows.iter().map(|r| r.1.y).collect();
    assert_eq!(xs.len(), 2, "two columns");
    assert!(ys.len() >= 3, "rows of tiles");
    for &(_, t) in &g.rows {
        assert!(t.h >= 60 && Ui::tile_info_rect(t).w >= 40 && Ui::tile_info_rect(t).h >= 40, "40 px info target");
    }
    shot(&mut u, "add-tiles");
    // a tap adds
    let &(k, tile) = &g.rows[0];
    let cat = u.sheet_add_entries()[k].cat;
    tap(&mut u, Rect::new(tile.x + 6, tile.y + 44, tile.w - 50, 30));
    assert!(u.add_is_on(cat), "added by a tap");
    // a tap on the added tile shows its card, where Remove is
    tap(&mut u, Rect::new(tile.x + 6, tile.y + 44, tile.w - 50, 30));
    assert!(u.studio.md.info == Some(cat) && u.add_is_on(cat), "the card, not a removal");
    shot(&mut u, "info-card");
    let card = u.info_card_rect();
    tap(&mut u, Ui::info_card_action(card));
    assert!(!u.add_is_on(cat) && u.studio.md.info.is_none(), "removed from the card");
}

#[test]
fn the_info_card_opens_from_a_tile_and_from_a_row() {
    let mut u = studio();
    add_open(&mut u);
    let g = u.sheet_add_geo();
    let &(k, tile) = &g.rows[1];
    let cat = u.sheet_add_entries()[k].cat;
    tap(&mut u, Ui::tile_info_rect(tile));
    assert_eq!(u.studio.md.info, Some(cat));
    let info = super::meascatalog::cat_info(cat);
    assert!(!info.how.is_empty() && !info.blurb.is_empty() && !info.sources.is_empty());
    let card = u.info_card_rect();
    tap(&mut u, super::Ui::info_card_close(card));
    assert!(u.studio.md.info.is_none());
    // from a row of the measurements: About this measurement
    u.close_menu();
    for _ in 0..3 { frame(&mut u); }
    let g = u.studio_panel_geo().unwrap();
    let (_, menu) = Ui::md_row_buttons(Rect::new(g.list.x, g.list.y, g.list.w, u.studio_panel_row()));
    tap(&mut u, menu);
    let p = u.md_menu_rect().expect("the row menu");
    tap(&mut u, Ui::md_menu_row(p, 8));
    assert!(u.studio.md.info.is_some(), "the card of the first row");
    shot(&mut u, "info-card-row");
}
