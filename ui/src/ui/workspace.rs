// SPDX-License-Identifier: GPL-2.0-or-later
//! Compact scope chrome, source lanes and pointer gestures shared by touch and mouse.
use super::*;

pub(super) const HOLD: f64 = LONG_PRESS;
const MOVE: i32 = 8;
pub(super) const HEADER: i32 = 26;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Trace { Channel(usize), Reference(usize), Math(usize) }
#[derive(Clone, Copy, PartialEq)]
enum Panel { Plot, Eye, Measurements }
#[derive(Clone, Copy)]
struct Gesture { panel: Panel, origin: Rect, x: i32, y: i32, at: f64, moved: bool, held: bool }
#[derive(Clone)]
pub(super) enum Action {
    Overlay, Stacked, Sources, Source(WaveId), Trigger(WaveId), Menu(MenuId),
    ZoomReset, ZoomClose, FitRecord, Live, Single, Run, Panel,
    EyeRight, EyeLeft, EyeBelow, EyeFloat, EyeClose, RestoreLayout,
    MeasureFloat, MeasureDock(StripDock), Close,
    /// the quick actions of a channel (its hold popup)
    ChCoupling(usize), ChInvert(usize), ChZero(usize), Autoset,
    /// the quick actions of a math or reference chip in the Studio bottom bar
    MathOff(usize), MathSettings(usize), RefClear(usize), RefSettings(usize),
    /// Studio: the horizontal scale of a reference (Follow main, or its own; `sharedzoom.rs`)
    RefHorizontal(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mixed() -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.run = RunState::Stop;
        ui.m.tscale = tscales().iter().position(|&t| t >= 1e-9).unwrap();
        ui.m.delay = 4e-9;
        for i in 0..3 {
            ui.refs[i] = Some(RefWave { src: 0, color: i, label: format!("source-{i}"), t0: -2e-9, dt: 1e-10,
                v: ((0..128).map(|k| ((k as f32 + i as f32) * 0.2).sin() * 0.2).collect::<Vec<_>>()).into(), vpd: 0.1, offset: 0.0, on: true });
        }
        ui
    }

    #[test]
    fn stacked_zoom_preserves_view_and_puts_all_sources_in_detail_lanes() {
        let mut ui = mixed();
        ui.set_zoom(true);
        ui.m.zdelay = ui.m.delay + 1e-9;
        let before = (ui.m.tscale, ui.m.delay, ui.m.zscale, ui.m.zdelay);
        ui.trace_mode(true);
        assert!(ui.stacked() && ui.m.zoom);
        assert_eq!((ui.m.tscale, ui.m.delay, ui.m.zscale, ui.m.zdelay), before);
        assert_eq!(ui.wins.len(), ui.trace_sources().len());
        let (overview, detail) = ui.split_zoom(ui.grid);
        for &lane in &ui.wins {
            assert_eq!((lane.x, lane.w), (detail.x, detail.w));
            assert!(lane.y >= detail.y && lane.y + lane.h <= detail.y + detail.h);
            assert!(lane.h > 0 && lane.y >= overview.y + overview.h);
        }
        assert_eq!((ui.phz.w, ui.phz.h), (ui.wins[0].w as usize, ui.wins[0].h as usize));
        assert_ne!(ui.source_area(WaveId::Ref(0)), ui.source_area(WaveId::Ref(1)));
        let mut cv = Canvas::new(W, H);
        ui.render(&mut cv);
        // Overview draws all references even when its current detail lane is CH1.
        for i in 0..3 {
            let color = REF_COLORS[i].1;
            assert!((overview.y..overview.y + overview.h).any(|y| cv.px[(y * W + overview.x) as usize..(y * W + overview.x + overview.w) as usize].contains(&color)));
        }
        ui.trace_mode(false);
        assert!(ui.m.zoom && !ui.stacked());
        assert_eq!((ui.m.tscale, ui.m.delay, ui.m.zscale, ui.m.zdelay), before);
        assert_eq!(ui.wins, vec![ui.split_zoom(ui.grid).1]);
        ui.render(&mut cv);
    }

    #[test]
    fn selecting_visible_channel_only_opens_settings_without_acquisition_side_effects() {
        let mut ui = mixed();
        ui.m.set_trigger_wave(WaveId::Ref(1), false);
        ui.m.trig_type = TrigType::Runt;
        ui.m.status = TrigStatus::Stop;
        let epoch = ui.waveform_epoch();
        let (_, rect) = ui.source_pills().into_iter().find(|(id, _)| *id == WaveId::Ch(0)).unwrap();
        ui.workspace_footer_tap(rect.x + rect.w / 2, rect.y + 10);
        assert_eq!(ui.menu, MenuId::Channel(0));
        assert_eq!(ui.m.run, RunState::Stop);
        assert_eq!(ui.m.status, TrigStatus::Stop);
        assert_eq!(ui.m.trigger_wave(), WaveId::Ref(1));
        assert_eq!(ui.m.trig_type, TrigType::Runt);
        assert_eq!(ui.waveform_epoch(), epoch);
    }

    #[test]
    fn stationary_hold_then_move_turns_a_panel_context_menu_into_a_drag() {
        let mut ui = mixed();
        for panel in [Panel::Measurements, Panel::Eye] {
            let origin = match panel {
                Panel::Measurements => { ui.workspace_action(Action::MeasureFloat); ui.results_rects().unwrap().0 },
                Panel::Eye => { ui.m.view = AView::Eye; ui.workspace.eye_float = Some(Rect::new(360, 100, 620, 360)); ui.relayout(); ui.eye_pane_rect() },
                _ => unreachable!(),
            };
            let (x, y) = (origin.x + 20, origin.y + 12);
            ui.now = 0.0;
            assert!(ui.workspace_touch(Phase::Down, x, y));
            ui.now = 0.7;
            ui.workspace_hold_tick();
            assert!(ui.workspace.popup.is_some());
            assert!(ui.workspace_touch(Phase::Move, x - 200, y + 40));
            assert!(ui.workspace.popup.is_none());
            assert!(ui.workspace.gesture.is_some_and(|gesture| gesture.moved && !gesture.held));
            assert!(ui.workspace_touch(Phase::Up, x - 200, y + 40));
            let moved = match panel { Panel::Measurements => ui.results_rects().unwrap().0, Panel::Eye => ui.eye_pane_rect(), _ => unreachable!() };
            assert_eq!((moved.x, moved.y), (origin.x - 200, origin.y + 40));
        }
    }

    #[test]
    fn docking_keeps_statistics_page_and_unfolded_controls_inside_the_workspace() {
        let mut ui = mixed();
        ui.m.meas = (0..73).map(|i| (ALL_MEAS[i % ALL_MEAS.len()], WaveId::Ref(i % 3))).collect();
        ui.m.meas2.push((0, WaveId::Ref(0), WaveId::Ref(1)));
        let rows = ui.meas_rows();
        assert!(ui.results_name(0, &rows[0]).starts_with("R1 "));
        assert!(ui.results_name(73, &rows[73]).starts_with("R1-R2 "), "both pair sources precede any clipped label");
        ui.workspace_action(Action::MeasureFloat);
        ui.meas_page = 2;
        for dock in StripDock::ALL {
            ui.workspace_action(Action::MeasureDock(dock));
            assert!(ui.stats_on && ui.results_panel());
            assert_eq!(ui.meas_page, 2);
            let (header, body) = ui.results_rects().unwrap();
            assert!(header.x >= 0 && header.x + header.w <= W);
            assert!(header.y >= TOP.h && body.y + body.h <= BOTTOM.y);
        }
        ui.results_min = true;
        ui.workspace.measure_float = Some((W - 40, BOTTOM.y - 30));
        ui.results_min = false;
        let (header, body) = ui.results_rects().unwrap();
        assert!(header.x + header.w <= W && body.y + body.h <= BOTTOM.y);
        assert_eq!(Ui::results_pitch(73), 42, "statistics stay available for large selections");
    }

    #[test]
    fn context_and_visible_zoom_reset_have_the_same_distinct_close_behavior() {
        let mut ui = mixed();
        ui.set_zoom(true);
        ui.workspace_action(Action::ZoomReset);
        assert!(ui.m.zoom);
        assert_eq!((ui.m.zscale, ui.m.zdelay), (ui.m.tscale, ui.m.delay));
        ui.workspace_action(Action::ZoomClose);
        assert!(!ui.m.zoom);
    }

    #[test]
    fn detail_content_follows_drag_and_overview_selection_moves_in_record_time() {
        let mut ui = mixed();
        ui.set_zoom(true);
        ui.trace_mode(true);
        let lane = ui.wins[1];
        let (x, y) = (lane.x + lane.w / 2, lane.y + lane.h / 2);
        let before = ui.m.zdelay;
        ui.touch_ev(Phase::Down, x, y);
        ui.touch_ev(Phase::Move, x + 60, y);
        ui.touch_ev(Phase::Up, x + 60, y);
        assert!(ui.m.zdelay < before, "detail content follows the rightward finger");
        let before = ui.m.zdelay;
        let overview = ui.split_zoom(ui.grid).0;
        let (x, y) = (overview.x + overview.w / 2 + 100, overview.y + 45);
        ui.touch_ev(Phase::Down, x, y);
        ui.touch_ev(Phase::Move, x + 40, y);
        ui.touch_ev(Phase::Up, x + 40, y);
        assert!(ui.m.zdelay > before, "overview selection advances to later record time");
    }

    #[test]
    fn pointer_pan_moves_reference_content_with_the_finger_in_every_display_format() {
        for zoom in [false, true] {
            for stacked in [false, true] {
                let mut ui = mixed();
                ui.set_zoom(zoom);
                ui.trace_mode(stacked);
                ui.select_workspace_source(WaveId::Ref(1));
                let lane = ui.source_area(WaveId::Ref(1)).unwrap();
                let (x, y) = (lane.x + lane.w / 2, lane.y + lane.h / 2);
                let time = ui.view().1;
                for (phase, x) in [(Phase::Down, x), (Phase::Move, x + 60), (Phase::Up, x + 60)] {
                    ui.handle(Ev::Touch { id: 0, phase, x, y });
                }
                let (scale, delay) = ui.view();
                let delta_x = (time - delay) / scale * lane.w as f32 / HDIV;
                assert!((delta_x - 60.0).abs() < 0.1, "zoom {zoom} stacked {stacked}: content moved {delta_x}px");
                let before = ui.refs[1].as_ref().unwrap().offset;
                let screen_before = ui.zoom_y_of_divs(lane, before / ui.refs[1].as_ref().unwrap().vpd);
                for (phase, y) in [(Phase::Down, y), (Phase::Move, y + 24), (Phase::Up, y + 24)] {
                    ui.handle(Ev::Touch { id: 0, phase, x, y });
                }
                let r = ui.refs[1].as_ref().unwrap();
                let delta_y = ui.zoom_y_of_divs(lane, r.offset / r.vpd) - screen_before;
                assert!((delta_y - 24.0).abs() < 0.1, "zoom {zoom} stacked {stacked}: content moved {delta_y}px");
                if zoom { assert_eq!(r.offset, before, "zoom pans the display, not source settings"); }
                else { assert!(r.offset < before); }
            }
        }
    }

    #[test]
    fn reference_source_pill_routes_vertical_hardware_knobs_without_changing_replay_or_trigger() {
        let mut ui = mixed();
        ui.set_offline(true);
        ui.offline_ref = 0;
        ui.m.set_trigger_wave(WaveId::Ref(0), false);
        ui.m.trig_type = TrigType::Runt;
        let (_, pill) = ui.source_pills().into_iter().find(|(id, _)| *id == WaveId::Ref(1)).unwrap();
        ui.workspace_footer_tap(pill.x + 12, pill.y + 12);
        assert_eq!(ui.ref_sel, 1);
        let channel = (ui.m.ch[0].vscale, ui.m.ch[0].offset);
        let other = (ui.refs[0].as_ref().unwrap().vpd, ui.refs[0].as_ref().unwrap().offset);
        let scale = ui.refs[1].as_ref().unwrap().vpd;
        ui.key(0xda); // V scale clockwise: more sensitive.
        assert!(ui.refs[1].as_ref().unwrap().vpd < scale);
        ui.key(0x9a); // V position clockwise.
        assert!(ui.refs[1].as_ref().unwrap().offset > 0.0);
        ui.key(0x9c); // Position push: ground the selected reference.
        assert_eq!(ui.refs[1].as_ref().unwrap().offset, 0.0);
        ui.close_menu();
        ui.key(0xdc); // Scale push: selected reference settings.
        assert_eq!(ui.menu, MenuId::Ref);
        assert_eq!((ui.m.ch[0].vscale, ui.m.ch[0].offset), channel);
        assert_eq!((ui.refs[0].as_ref().unwrap().vpd, ui.refs[0].as_ref().unwrap().offset), other);
        assert_eq!(ui.active_reference(), 0, "selection is separate from replay source identity");
        assert_eq!(ui.m.trigger_wave(), WaveId::Ref(0));
        assert_eq!(ui.m.trig_type, TrigType::Runt);
        assert_eq!(ui.m.run, RunState::Stop);
    }

    fn pointer_tap(ui: &mut Ui, rect: Rect) {
        let (x, y) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
        ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
        ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
    }

    #[test]
    fn native_pointer_menu_back_and_close_win_over_overlapping_statistics_and_eye_headers() {
        for panel in [Panel::Measurements, Panel::Eye] {
            let mut ui = mixed();
            ui.m.layout = Layout::Default; // the stock dialogs: Minimal and Studio open sheets
            ui.stats_on = true;
            ui.results_min = false;
            if panel == Panel::Eye { ui.m.view = AView::Eye; }
            ui.open_meas_list();
            ui.activate(Item::Open(MenuId::Measure));
            for back in [true, false] {
                let dialog = ui.dlg_layout().unwrap();
                let target = if back { dialog.back.expect("settings has a Back route") } else { dialog.close };
                let (x, y) = (target.x + target.w / 2, target.y + target.h / 2);
                let header = match panel {
                    Panel::Measurements => {
                        ui.workspace.measure_float = Some((x - 24, y - 15));
                        ui.results_rects().unwrap().0
                    }
                    Panel::Eye => {
                        ui.workspace.eye_float = Some(Rect::new((x - 24).min(W - 320), y - HEADER / 2, 320, 200));
                        ui.eye_pane_rect()
                    }
                    _ => unreachable!(),
                };
                assert!(header.contains(x, y), "the test must overlap the actual background header");
                let position = (ui.workspace.measure_float, ui.workspace.eye_float);
                pointer_tap(&mut ui, target);
                assert!(!ui.results_min, "Back/Close must not fold the statistics panel behind the menu");
                assert_eq!((ui.workspace.measure_float, ui.workspace.eye_float), position);
                assert!(ui.workspace.gesture.is_none());
                if back {
                    assert!(ui.menu_open && ui.menu == MenuId::MeasAdd, "native Back must return to the chooser");
                    ui.activate(Item::Open(MenuId::Measure));
                } else { assert!(!ui.menu_open, "native Close must dismiss the menu"); }
            }
        }
    }

    #[test]
    fn native_pointer_dialog_power_and_keyboard_do_not_activate_background_panel_headers() {
        for modal in 0..3 {
            let mut ui = mixed();
            ui.workspace_action(Action::MeasureFloat);
            let header = ui.results_rects().unwrap().0;
            let position = ui.workspace.measure_float;
            match modal {
                0 => ui.dialog = Some(vec!["Modal information".into()]),
                1 => ui.power_dlg = true,
                _ => ui.wlan.osk = Some(wlan::Osk::for_ref(0, "Reference label")),
            }
            let (x, y) = (header.x + 24, header.y + 12);
            ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
            ui.now += 0.7;
            ui.workspace_hold_tick();
            ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
            assert!(!ui.results_min);
            assert_eq!(ui.workspace.measure_float, position);
            assert!(ui.workspace.gesture.is_none() && ui.workspace.popup.is_none());
            if modal == 0 { assert!(ui.dialog.is_none(), "the modal receives the dismiss tap"); }
            if modal == 1 { assert!(!ui.power_dlg, "the power dialog receives its cancel tap"); }
            if modal == 2 { assert!(ui.wlan.osk.is_some(), "an outside keyboard tap must not reach the panel"); }
        }
    }

    #[test]
    fn stacked_reference_drag_selects_its_knob_context_without_retargeting_acquisition() {
        for zoom in [false, true] {
            for live in [false, true] {
                let mut ui = mixed();
                ui.refs[3] = ui.refs[0].clone();
                if !live { ui.set_offline(true); }
                ui.offline_ref = 3;
                ui.m.set_trigger_wave(WaveId::Ref(0), false);
                ui.select_workspace_source(WaveId::Ref(3));
                ui.trace_mode(true);
                ui.set_zoom(zoom);
                let area = ui.source_area(WaveId::Ref(1)).unwrap();
                let (x, y) = (area.x + area.w / 2, area.y + area.h / 2);
                let before = ui.refs[1].as_ref().unwrap().offset;
                for (phase, y) in [(Phase::Down, y), (Phase::Move, y + 20), (Phase::Up, y + 20)] {
                    ui.handle(Ev::Touch { id: 0, phase, x, y });
                }
                assert_eq!(ui.selected_workspace_source(), Some(WaveId::Ref(1)));
                assert_eq!(ui.ref_sel, 1);
                if zoom { assert_eq!(ui.refs[1].as_ref().unwrap().offset, before); assert!(ui.zoom_vcenter > 0.0); }
                else { assert!(ui.refs[1].as_ref().unwrap().offset < before); }
                let scale = ui.refs[1].as_ref().unwrap().vpd;
                let other = ui.refs[3].as_ref().unwrap().vpd;
                ui.key(0xda);
                assert!(ui.refs[1].as_ref().unwrap().vpd < scale);
                assert_eq!(ui.refs[3].as_ref().unwrap().vpd, other);
                assert_eq!(ui.offline_ref, 3);
                assert_eq!(ui.m.trigger_wave(), WaveId::Ref(0));
                assert_eq!(ui.m.run, RunState::Stop);
                assert_eq!(ui.replay.captures, 0);
            }
        }
    }

    #[test]
    fn pair_header_has_real_source_status_and_picker_routes_without_extra_rows() {
        let mut ui = mixed();
        ui.m.layout = Layout::Default; // the header of the stock dialog; the sheet pins the sources under its tabs
        ui.select_workspace_source(WaveId::Ref(1));
        ui.meas_src = WaveId::Ref(1);
        ui.meas_src_b = WaveId::Ch(1);
        ui.open_meas_list();
        ui.activate(Item::Open(MenuId::MeasPair));
        let dialog = ui.dlg_layout().unwrap();
        assert_eq!(dialog.rows.len(), tools::PAIRS.len());
        assert!(ui.menu_title(MenuId::MeasPair).contains("R2 to CH2 (off)"));
        for (item, rect) in ui.pair_header_sources(&dialog) {
            assert!(rect.x >= dialog.r.x && rect.x + rect.w < dialog.back.unwrap().x);
            pointer_tap(&mut ui, rect);
            assert_eq!(ui.menu, MenuId::Pick);
            assert_eq!(ui.pick.unwrap().it, item);
            let target = if item == Item::MeasSrc { "R1" } else { "R3" };
            ui.select_option(item, target);
            ui.close_picker();
            assert_eq!(ui.menu, MenuId::MeasPair);
        }
        assert_eq!(ui.meas_src, WaveId::Ref(0));
        assert_eq!(ui.meas_src_b, WaveId::Ref(2));
        assert_eq!(ui.selected_workspace_source(), Some(WaveId::Ref(1)), "analysis A/B choices do not redirect workspace knobs");
        assert!(ui.parent.is_some());
        assert_eq!(ui.item_label(Item::RefSrc), "Save from");
        assert!(ui.menu_title(MenuId::Ref).contains("R2"));
    }

    #[test]
    fn eye_header_source_and_settings_taps_preserve_drag_and_show_record_scope() {
        let mut ui = mixed();
        ui.select_workspace_source(WaveId::Ref(1));
        ui.eye_make();
        assert_eq!(ui.m.eye_src, WaveId::Ref(1));
        assert_eq!(ui.eye_source_mode_label(), "Full record");
        let source = ui.eye_header_source();
        pointer_tap(&mut ui, source);
        assert_eq!(ui.menu, MenuId::Pick);
        assert_eq!(ui.pick.unwrap().it, Item::EyeSrc);
        ui.select_option(Item::EyeSrc, "R3");
        ui.close_picker();
        assert_eq!(ui.m.eye_src, WaveId::Ref(2));
        assert_eq!(ui.menu, MenuId::Eye);
        ui.close_menu();
        let placement = ui.eye_place_control();
        pointer_tap(&mut ui, placement);
        assert_eq!(ui.menu, MenuId::Pick);
        assert_eq!(ui.pick.unwrap().it, Item::EyePlace, "the separate placement target opens its own picker");
        ui.select_option(Item::EyePlace, "Side");
        ui.close_picker();
        assert_eq!(ui.m.eye_place, EyePlace::Side);
        ui.close_menu();
        let pane = ui.eye_pane_rect();
        let place = ui.eye_place_control();
        let settings = Rect::new(place.x + place.w + 8, pane.y + 1, 30, HEADER - 2);
        pointer_tap(&mut ui, settings);
        assert_eq!(ui.menu, MenuId::Eye);
        ui.close_menu();
        ui.session_is_sim = true;
        ui.m.sim_slot = 0;
        assert_eq!(ui.eye_source_mode_label(), "Full record", "a different SIM source does not make R3 live");
        ui.m.eye_src = WaveId::Ref(0);
        assert_eq!(ui.eye_source_mode_label(), "SIM captures");
        ui.m.eye_src = WaveId::Ch(0);
        assert_eq!(ui.eye_source_mode_label(), "Stopped");
        ui.m.run = RunState::Run;
        assert_eq!(ui.eye_source_mode_label(), "Live");
    }

    #[test]
    fn reference_cursor_modes_render_only_in_selected_lane_and_read_its_physical_samples() {
        for live in [false, true] {
            let mut ui = mixed();
            if !live { ui.set_offline(true); }
            let r = ui.refs[1].as_mut().unwrap();
            r.t0 = -7e-6;
            r.dt = 2e-9;
            r.v = (0..128).map(|i| i as f32 * 0.1).collect::<Vec<_>>().into();
            r.vpd = 0.2;
            r.offset = 0.3;
            ui.select_workspace_source(WaveId::Ref(1));
            assert!((ui.wave_at(-7e-6 + 3.5 * 2e-9).unwrap() - 0.35).abs() < 1e-5);
            assert!(ui.wave_at(-7e-6 - 2e-9).is_none());
            ui.trace_mode(true);
            assert!(ui.menu_title(MenuId::Cursor).contains("R2"));
            for mode in [CursorMode::Time, CursorMode::Volt, CursorMode::Track, CursorMode::Xy] {
                ui.m.cursor = mode;
                ui.m.cur_a = 0.25;
                ui.m.cur_b = 0.75;
                ui.m.cur_c = 0.25;
                ui.m.cur_d = 0.75;
                let mut cv = Canvas::new(W, H);
                cv.clear(BG);
                for index in 0..ui.wins.len() {
                    ui.cur_win = index;
                    ui.area = ui.wins[index];
                    ui.draw_cursors(&mut cv);
                }
                let selected = ui.source_area(WaveId::Ref(1)).unwrap();
                for &area in &ui.wins {
                    let marked = (area.y..area.y + area.h).any(|y| cv.px[(y * W + area.x) as usize..(y * W + area.x + area.w) as usize].contains(&CURSOR));
                    assert_eq!(marked, area == selected, "{mode:?}: cursor must stay in the selected source lane");
                }
                if mode == CursorMode::Volt || mode == CursorMode::Xy {
                    let values = ui.cursor_values();
                    let y1 = values.iter().find(|(name, _)| name == "Y1").unwrap();
                    assert_eq!(y1.1, si(((0.5f32 - 0.25) * VDIV * 0.2 - 0.3) as f64, "V"));
                }
            }
            assert_eq!(ui.m.run, RunState::Stop);
            assert_eq!(ui.replay.captures, 0);
            assert!(ui.last[0].is_none());
        }
    }

    #[test]
    fn native_pointer_drags_selected_reference_time_voltage_track_and_xy_cursors() {
        for mode in [CursorMode::Time, CursorMode::Volt, CursorMode::Track, CursorMode::Xy] {
            let mut ui = mixed();
            ui.trace_mode(true);
            ui.set_zoom(true);
            ui.select_workspace_source(WaveId::Ref(1));
            ui.m.cursor = mode;
            ui.m.cur_a = 0.25;
            ui.m.cur_b = 0.75;
            ui.m.cur_c = 0.3;
            ui.m.cur_d = 0.7;
            let area = ui.source_area(WaveId::Ref(1)).unwrap();
            let offset = ui.refs[1].as_ref().unwrap().offset;
            let horizontal = matches!(mode, CursorMode::Time | CursorMode::Track);
            let (x, y) = if horizontal {
                (area.x + (area.w as f32 * ui.m.cur_a) as i32, area.y + area.h * 4 / 5)
            } else {
                (area.x + area.w / 2, area.y + (area.h as f32 * if mode == CursorMode::Xy { ui.m.cur_c } else { ui.m.cur_a }) as i32)
            };
            let delta = if horizontal { 40 } else { 16 };
            for (phase, shift) in [(Phase::Down, 0), (Phase::Move, delta), (Phase::Up, delta)] {
                ui.handle(Ev::Touch { id: 0, phase, x: x + if horizontal { shift } else { 0 }, y: y + if horizontal { 0 } else { shift } });
            }
            if mode == CursorMode::Xy { assert!(ui.m.cur_c > 0.3); }
            else { assert!(ui.m.cur_a > 0.25, "{mode:?}: the selected cursor must move"); }
            assert_eq!(ui.refs[1].as_ref().unwrap().offset, offset, "cursor gestures do not drag the waveform");
            assert_eq!(ui.m.run, RunState::Stop);
            assert_eq!(ui.replay.captures, 0);
        }
    }

    #[test]
    fn cursor_strip_badges_follow_selected_source_identity_and_color_in_all_modes() {
        let mut ui = mixed();
        ui.m.meas.clear();
        ui.m.meas2.clear();
        ui.refs[1].as_mut().unwrap().color = 2;
        ui.select_workspace_source(WaveId::Ref(1));
        let color = REF_COLORS[2].1;
        for mode in [CursorMode::Time, CursorMode::Volt, CursorMode::Track, CursorMode::Xy] {
            ui.m.cursor = mode;
            assert_eq!(ui.cursor_badge(), Some(("R2".into(), color)));
            let lines = ui.strip_side_lines();
            assert_eq!(lines[0].pill, Some(("R2".into(), color)));
            let mut cv = Canvas::new(W, H);
            ui.draw_strip_band(&mut cv, 100);
            assert_eq!(cv.px[(105 * W + 10) as usize], color, "{mode:?}: the horizontal source pill uses the reference color");
        }
        ui.m.meas = ALL_MEAS.iter().map(|&kind| (kind, WaveId::Ref(1))).collect();
        ui.m.strip_dock = StripDock::Below;
        ui.relayout();
        let mut cv = Canvas::new(W, H);
        assert!(ui.draw_paged_measurement_strip(&mut cv));
        // The cursor row follows the three readout rows: the page controls are in the band's last column.
        let y = ui.strip_band_y() + 3 * STRIP_ROW;
        assert_eq!(cv.px[((y + 5) * W + 10) as usize], color, "paged readouts retain the same source badge");
        ui.select_workspace_source(WaveId::Ch(0));
        assert_eq!(ui.cursor_badge(), Some(("CH1".into(), CH_COLORS[0])));
        ui.m.ch[0].on = false;
        assert!(ui.cursor_badge().is_none(), "an unavailable source cannot claim the old CH1 label");
    }

    #[test]
    fn fit_record_context_action_stays_on_screen_with_all_sources() {
        let mut ui=mixed();
        for i in 0..NREF {if ui.refs[i].is_none(){ui.refs[i]=ui.refs[0].clone();}}
        ui.select_workspace_source(WaveId::Ref(1));ui.m.delay=1.0;ui.set_zoom(true);
        ui.context_menu(900,550);let popup=ui.workspace.popup.as_ref().unwrap();
        assert!(popup.rect.y>=0 && popup.rect.y+popup.rect.h<=H);
        let index=popup.rows.iter().position(|(label,_)|label=="Fit record").unwrap();
        let r=popup.row_rect(index);let (x,y)=(r.x+30,r.y+r.h/2);
        for phase in [Phase::Down,Phase::Up]{ui.handle(Ev::Touch{id:0,phase,x,y});}
        assert!(!ui.m.zoom && ui.m.delay.abs()<1e-3);
        assert_eq!((ui.m.run,ui.replay.captures),(RunState::Stop,0));
    }
}
/// Rows of every popup are one finger tall; the title row carries the close.
pub(super) const POPUP_ROW: i32 = 40;
pub(super) const POPUP_TITLE: i32 = 40;
pub(super) const POPUP_COL_W: i32 = 244;
/// The smallest touch target: what is drawn smaller gets a hit area of this size (`Ui::hit40`).
pub(super) const TOUCH_MIN: i32 = 40;
pub(super) struct Popup { pub(super) rect: Rect, pub(super) rows: Vec<(String, Action)>, pub(super) title: String, pub(super) per_col: usize }
impl Popup {
    pub(super) fn close_rect(&self) -> Rect { Rect::new(self.rect.right() - POPUP_TITLE - 3, self.rect.y + 3, POPUP_TITLE, POPUP_TITLE) }
    pub(super) fn cols(&self) -> usize { self.rows.len().div_ceil(self.per_col.max(1)).max(1) }
    /// Rectangle of row `i`: rows fill a column top to bottom, then the next column.
    pub(super) fn row_rect(&self, i: usize) -> Rect {
        let (per, cols) = (self.per_col.max(1), self.cols() as i32);
        let w = (self.rect.w - 10) / cols;
        Rect::new(self.rect.x + 5 + (i / per) as i32 * w, self.rect.y + 3 + POPUP_TITLE + (i % per) as i32 * POPUP_ROW, w, POPUP_ROW)
    }
}
#[derive(Default)]
pub(super) struct Workspace {
    selected_source: Option<WaveId>,
    pub stacked: bool,
    pub eye_float: Option<Rect>,
    pub eye_left: bool,
    pub measure_float: Option<(i32, i32)>,
    /// the measurement card sits in the strip dock (`m.strip_dock`) instead of floating
    pub measure_docked: bool,
    gesture: Option<Gesture>,
    pub(super) popup: Option<Popup>,
    /// a hold opened the popup while the finger is still down: its lift is not a tap on a row
    pub(super) lift_pending: bool,
    /// the cursor a tap on its tag picked (`Ui::select_cursor`)
    pub(super) cursor_sel: Option<usize>,
}

impl Ui {
    /// Display/knob focus is independent of acquisition and measurement A/B.
    pub(super) fn selected_workspace_source(&self) -> Option<WaveId> {
        let available = |id| match id {
            WaveId::Ch(c) => c < n_ch() && self.m.ch[c].on,
            WaveId::Ref(i) => self.refs.get(i).is_some_and(Option::is_some),
        };
        if let Some(id) = self.workspace.selected_source { return available(id).then_some(id); }
        let id = if self.record_mode() { WaveId::Ref(self.ref_sel) } else { WaveId::Ch(self.m.active) };
        available(id).then_some(id)
    }
    /// The trace the cursors measure: the one picked in the cursor menu, else the selected one.
    pub(super) fn cursor_wave(&self) -> Option<WaveId> {
        let available = |id| match id {
            WaveId::Ch(c) => c < n_ch() && self.m.ch[c].on,
            WaveId::Ref(i) => self.refs.get(i).is_some_and(Option::is_some),
        };
        self.cursor_pin.filter(|&id| available(id)).or_else(|| self.selected_workspace_source())
    }
    pub(super) fn select_workspace_source(&mut self, id: WaveId) {
        self.cursor_pin = None;
        self.workspace.selected_source = Some(id);
        match id { WaveId::Ch(c) => self.m.active = c, WaveId::Ref(i) => self.ref_sel = i }
        self.meas_src = id;
    }
    pub(super) fn workspace_source_visible_here(&self, id: WaveId) -> bool {
        match id {
            WaveId::Ch(c) => self.m.ch[c].on && self.shows(c),
            WaveId::Ref(i) => self.refs.get(i).and_then(Option::as_ref).is_some_and(|r| r.on)
                && (!self.stacked() || self.trace_sources().get(self.cur_win) == Some(&Trace::Reference(i))),
        }
    }
    pub(super) fn cursor_badge(&self) -> Option<(String, Rgb)> {
        self.cursor_wave().map(|id| (self.wave_name(id), self.wave_color(id)))
    }
    pub(super) fn cursor_source_context(&self) -> Option<(WaveId, f32, f32, &'static str)> {
        let id = self.cursor_wave()?;
        let (scale, offset, unit) = match id {
            WaveId::Ch(c) => (self.m.ch[c].volts_per_div(), self.m.ch[c].offset, self.m.unit(c)),
            WaveId::Ref(i) => { let r = self.refs[i].as_ref()?; (r.vpd, r.offset, "V") },
        };
        Some((id, scale, offset, unit))
    }
    pub(super) fn pair_source_caption(&self, id: WaveId) -> String {
        let status = match id {
            WaveId::Ch(c) if !self.m.ch[c].on => " (off)",
            WaveId::Ch(c) if self.last[0].as_ref().is_none_or(|r| r.ch[c].is_empty()) => " (no data)",
            WaveId::Ref(i) if self.refs.get(i).is_none_or(Option::is_none) => " (empty)",
            _ => "",
        };
        format!("{}{}", self.wave_name(id), status)
    }
    pub(super) fn pair_header_sources(&self, dialog: &Dlg) -> [(Item, Rect); 2] {
        let start = dialog.r.x + 70;
        let end = dialog.back.unwrap_or(dialog.close).x - 8;
        let width = ((end - start - 26) / 2).max(44);
        [(Item::MeasSrc, Rect::new(start, dialog.r.y + 6, width, DLG_CTRL)),
         (Item::MeasSrcB, Rect::new(start + width + 26, dialog.r.y + 6, width, DLG_CTRL))]
    }
    pub(super) fn draw_pair_header(&self, cv: &mut Canvas, dialog: &Dlg) {
        cv.text(&self.fonts().mb, dialog.r.x + 14, dialog.r.y + 31, "Pairs", TEXT);
        let targets = self.pair_header_sources(dialog);
        for ((_, rect), id) in targets.into_iter().zip([self.meas_src, self.meas_src_b]) {
            btn(cv, rect, BTN, BTN_BORDER);
            cv.set_clip(Rect::new(rect.x + 5, rect.y, (rect.w - 27).max(1), rect.h));
            cv.text_in(&self.fonts().sb, Rect::new(rect.x + 5, rect.y, (rect.w - 27).max(1), rect.h), 1, &self.pair_source_caption(id), self.wave_color(id));
            cv.no_clip();
            chevron_down(cv, rect.x + rect.w - 16, rect.y + rect.h / 2 - 3, DIM);
        }
        let a = targets[0].1;
        cv.text_in(&self.fonts().mb, Rect::new(a.x + a.w, a.y, 26, a.h), 1, "to", TEXT);
    }
    pub(super) fn stacked(&self) -> bool {
        self.trace_stacked() && !self.xy_over() && matches!(self.m.view, AView::Scope | AView::Eye)
    }
    /// The trace mode that is chosen: Stacked, which Studio has until Overlay is picked from its plot menu.
    pub(super) fn trace_stacked(&self) -> bool {
        self.workspace.stacked || (self.m.layout == Layout::Studio && !self.studio.overlay)
    }
    pub(super) fn trace_sources(&self) -> Vec<Trace> {
        // a record session (SIM, offline) has no live data: its inputs keep their state but get no lane
        let mut sources: Vec<_> = (0..n_ch()).filter(|&c| self.m.ch[c].on && !self.record_mode()).map(Trace::Channel).collect();
        sources.extend((0..NREF).filter(|&i| self.refs[i].as_ref().is_some_and(|r| r.on)).map(Trace::Reference));
        sources.extend((0..NMATH).filter(|&k| self.m.math_shown(k)).map(Trace::Math));
        sources
    }
    pub(super) fn source_area(&self, id: WaveId) -> Option<Rect> {
        if !self.stacked() {
            if let WaveId::Ref(i) = id {
                if self.wins.len() > 1 { return self.wins.get(self.ref_win_of(i)).copied(); }
            }
            return Some(self.wave_area());
        }
        let wanted = match id { WaveId::Ch(c) => Trace::Channel(c), WaveId::Ref(i) => Trace::Reference(i) };
        let index = self.trace_sources().iter().position(|source| *source == wanted)?;
        self.wins.get(index).copied()
    }
    pub(super) fn drag_reference(&self) -> Option<usize> {
        if self.stacked() {
            return match self.trace_sources().get(self.cur_win) { Some(Trace::Reference(i)) => Some(*i), _ => None };
        }
        match self.selected_workspace_source() {
            Some(WaveId::Ref(i)) if self.refs[i].as_ref().is_some_and(|reference| reference.on) => Some(i),
            _ => None,
        }
    }
    pub(super) fn trace_mode(&mut self, stacked: bool) {
        self.workspace.popup = None;
        if self.m.layout == Layout::Studio { self.studio.overlay = !stacked; self.m.studio_overlay = !stacked; }
        // a split already gives every source its own window: Overlay keeps it, Stacked replaces it
        if !stacked && self.m.win_layout != WinLayout::One { self.workspace.stacked = false; return; }
        self.workspace.stacked = stacked;
        if stacked && matches!(self.m.density, Density::Split | Density::Four) {
            self.m.density = Density::Scope;
            self.apply_density();
        }
        self.m.win_layout = WinLayout::One;
        self.relayout();
        self.dirty_map = true;
    }
    pub(super) fn footer_rects(&self) -> (Rect, Rect, Rect, Rect, Option<Rect>, Option<Rect>, Rect) {
        let y = BOTTOM.y + 3;
        let mut right = W - 4;
        let panel = self.desktop_panel.map(|_| { right -= 48; let r = Rect::new(right, y, 46, 30); right -= 4; r });
        let top_live = !self.full_on() && self.top_buttons().iter().any(|button| button.0 == TopBtn::Live);
        let live = (self.record_mode() && !top_live).then(|| { right -= 44; let r = Rect::new(right, y, 42, 30); right -= 4; r });
        right -= 222;
        let trigger = Rect::new(right, y, 222, 30);
        right -= 128;
        let time = Rect::new(right, y, 124, 30);
        right -= 124;
        let overlay = Rect::new(right, y, 58, 30);
        let stacked = Rect::new(right + 62, y, 58, 30);
        (overlay, stacked, time, trigger, live, panel, Rect::new(126, y, (right - 132).max(0), 30))
    }
    /// Sources that get a pill: every live channel and every loaded or selected reference.
    fn pill_sources(&self) -> Vec<WaveId> {
        let mut sources: Vec<_> = (0..n_ch()).filter(|&c| self.m.ch[c].on).map(WaveId::Ch).collect();
        sources.extend((0..NREF).filter(|&i| self.refs[i].as_ref().is_some_and(|reference| reference.on || self.ref_sel == i)).map(WaveId::Ref));
        sources
    }
    /// How many pills fit, and whether the rest goes behind a "+N" pill.
    fn pill_count(&self, total: usize) -> (usize, bool) {
        let fit = (self.footer_rects().6.w / 42).max(1) as usize;
        if total > fit && fit > 1 { (fit - 1, true) } else { (total.min(fit), false) }
    }
    pub(super) fn source_pills(&self) -> Vec<(WaveId, Rect)> {
        let r = self.footer_rects().6;
        let mut sources = self.pill_sources();
        let (count, more) = self.pill_count(sources.len());
        let active = self.selected_workspace_source();
        if let Some(index) = sources.iter().position(|id| Some(*id) == active) {
            if index >= count && count > 0 { sources.swap(index, count - 1); }
        }
        let width = (r.w / (count + more as usize).max(1) as i32).clamp(38, 78);
        sources.into_iter().take(count).enumerate().map(|(i, id)| (id, Rect::new(r.x + i as i32 * width, r.y, width - 4, r.h))).collect()
    }
    /// The "+N" pill after the last source pill that opens the Sources popup, when not every source has a pill.
    pub(super) fn source_overflow(&self) -> Option<(usize, Rect)> {
        let r = self.footer_rects().6;
        let total = self.pill_sources().len();
        let (count, more) = self.pill_count(total);
        if !more { return None; }
        let width = (r.w / (count + 1) as i32).clamp(38, 78);
        Some((total - count, Rect::new(r.x + count as i32 * width, r.y, width - 4, r.h)))
    }
    pub(super) fn compact_cards(&self) -> Cards {
        let mut ch = [Rect::new(0, BOTTOM.y, 0, 30); NCH];
        for (id, rect) in self.source_pills() { if let WaveId::Ch(c) = id { ch[c] = rect; } }
        let (_, _, time, trig, _, _, _) = self.footer_rects();
        Cards { ch, la: None, gen: None, refs: None, time, trig }
    }
    pub(super) fn draw_workspace_footer(&self, cv: &mut Canvas) {
        cv.fill(BOTTOM, BAR);
        cv.hline(0, W - 1, BOTTOM.y, LINE);
        let draw = |cv: &mut Canvas, r, label: &str, on| {
            btn(cv, r, BTN, if on { ACCENT } else { BTN_BORDER });
            cv.text_in(&self.fonts().s, r, 1, label, if on { ACCENT } else { TEXT });
        };
        draw(cv, Self::MENU_BTN, if self.m.touch_lock { "Unlock" } else { "Menu" }, false);
        draw(cv, Rect::new(56, BOTTOM.y + 3, 64, 30), "Sources", false);
        for (id, rect) in self.source_pills() {
            let (label, selected) = match id {
                WaveId::Ch(c) => (format!("CH{}", c + 1), self.selected_workspace_source() == Some(id)),
                WaveId::Ref(i) => (format!("R{}", i + 1), self.selected_workspace_source() == Some(id)),
            };
            let color = self.wave_color(id);
            // the active source is filled in its colour, the others only carry it in their label
            btn(cv, rect, if selected { color } else { BAR }, if selected { color } else { BTN_BORDER });
            cv.text_in(&self.fonts().sb, rect, 1, &label, if selected { BAR } else { color });
        }
        if let Some((n, rect)) = self.source_overflow() { draw(cv, rect, &format!("+{n}"), false); }
        let (overlay, stacked, time, trigger, live, panel, _) = self.footer_rects();
        draw(cv, overlay, "Overlay", !self.workspace.stacked);
        draw(cv, stacked, "Stacked", self.workspace.stacked);
        let (scale, _) = self.view();
        draw(cv, time, &format!("Time {}/div", si_short(scale as f64, "s")), false);
        self.draw_compact_trigger(cv, trigger);
        if let Some(r) = live { draw(cv, r, "Live", false); }
        if let Some(r) = panel { draw(cv, r, "Panel", self.desktop_panel == Some(true)); }
    }
    pub(super) fn workspace_footer_tap(&mut self, x: i32, y: i32) -> bool {
        if !BOTTOM.contains(x, y) { return false; }
        if Self::MENU_BTN.contains(x, y) {
            if self.m.touch_lock { self.key(key::TOUCHLOCK); } else { self.toggle_menu(MenuId::Main); }
            return true;
        }
        if Rect::new(56, BOTTOM.y + 3, 64, 30).contains(x, y) { self.open_sources(x, BOTTOM.y); return true; }
        for (id, rect) in self.source_pills() {
            if !rect.contains(x, y) { continue; }
            self.select_workspace_source(id);
            match id {
                WaveId::Ch(c) => self.toggle_menu(MenuId::Channel(c)),
                WaveId::Ref(_) => self.toggle_menu(MenuId::Ref),
            }
            return true;
        }
        if let Some((_, rect)) = self.source_overflow() { if rect.contains(x, y) { self.open_sources(rect.x, BOTTOM.y); return true; } }
        let (overlay, stacked, time, trigger, live, panel, _) = self.footer_rects();
        if overlay.contains(x, y) { self.trace_mode(false); }
        else if stacked.contains(x, y) { self.trace_mode(true); }
        else if time.contains(x, y) { self.toggle_menu(MenuId::Horizontal); }
        else if live.is_some_and(|r| r.contains(x, y)) { self.return_to_live(); }
        else if panel.is_some_and(|r| r.contains(x, y)) { self.desktop_panel = self.desktop_panel.map(|open| !open); }
        else { self.compact_trigger_tap(x, y, trigger); }
        true
    }
    /// A touch just above or below a chip of the compact bottom bar counts as on it: the chips are 30 px high, the finger
    /// target is 40 (the bar is 36 px; the rest is the 2 px above it and the screen's edge).
    pub(super) fn bar_y(&self, y: i32) -> i32 {
        let bar = self.frame().bottom.y;
        if self.m.layout != Layout::Default && y >= bar - 2 { y.clamp(bar + 3, bar + 32) } else { y }
    }
    /// The zoom bar's buttons are 28 px high; a touch within 20 px of the bar's middle counts as on the buttons (40 px).
    pub(super) fn zoom_row_y(&self, x: i32, y: i32) -> i32 {
        let row = self.zoom_readout();
        let mid = row.y + row.h / 2;
        if !self.m.zoom || !(row.x..row.right()).contains(&x) || (y - mid).abs() > TOUCH_MIN / 2 { return y; }
        y.clamp(row.y + 4, row.bottom() - 5)
    }
    /// A rectangle grown about its centre to at least `TOUCH_MIN` square: what is drawn small keeps a finger-sized hit area.
    pub(super) fn hit40(r: Rect) -> Rect {
        if r.w <= 0 || r.h <= 0 { return r; }
        let (w, h) = (r.w.max(TOUCH_MIN), r.h.max(TOUCH_MIN));
        Rect::new(r.x + (r.w - w) / 2, r.y + (r.h - h) / 2, w, h)
    }
    /// The channel whose chip, lane badge or ground marker is at the point (a hit area of at least 40 px): the target
    /// of a hold in every layout.
    pub(super) fn channel_hotspot(&self, x: i32, y: i32) -> Option<usize> {
        if self.full_on() || self.view_on() { return None; }
        if self.m.layout == Layout::Studio {
            if let Some(c) = self.studio.lane_badges.borrow().iter().find(|(_, r)| Self::hit40(*r).contains(x, y)).map(|&(c, _)| c) { return Some(c); }
        } else {
            for c in 0..n_ch() {
                if !self.m.ch[c].on { continue; }
                let a = self.ch_area(c);
                let my = self.y_of_volts(c, 0.0).clamp(a.y + (a.h / 2).min(9), a.y + a.h - (a.h / 2).min(9));
                if Self::hit40(Rect::new(a.x + 2, my - 8, 24, 16)).contains(x, y) { return Some(c); }
            }
        }
        None
    }
    /// Where a finger that is being held stands and how far its ring has filled (0 to 1), when a hold would act.
    pub(super) fn press_ring(&self) -> Option<(i32, i32, f32)> {
        if let Some(r) = self.md_ring() { return Some(r); }
        if self.m.touch_lock || self.workspace.popup.is_some() || self.menu_open && self.menu != MenuId::MeasHold && self.workspace.gesture.is_none() { return None; }
        let ring = |x, y, at: f64| { let f = ((self.now - at) / LONG_PRESS) as f32; (f > 0.2 && f < 1.0).then_some((x, y, f)) };
        if let Some(g) = self.workspace.gesture.filter(|g| !g.moved && !g.held) { return ring(g.x, g.y, g.at); }
        let t = &self.touch;
        let near = (t.x - t.x0).abs() <= 12 && (t.y - t.y0).abs() <= 12;
        if t.down && !t.held && near {
            if let Some((_, at)) = self.card_hold { return ring(t.x0, t.y0, at); }
            if self.on_measure_btn(t.x0, t.y0) { return ring(t.x0, t.y0, t.t0); }
        }
        None
    }
    /// The ring that fills under a held finger: the hold is there to be found, and it shows when it will act.
    pub(super) fn draw_press_ring(&self, cv: &mut Canvas) {
        let Some((x, y, f)) = self.press_ring() else { return };
        let r = 24.0f32;
        let at = |a: f32| ((x as f32 + r * a.sin()).round() as i32, (y as f32 - r * a.cos()).round() as i32);
        let steps = (f * 48.0).ceil() as i32;
        for k in 0..steps {
            let (a0, a1) = (k as f32 / 48.0 * std::f32::consts::TAU, ((k + 1) as f32 / 48.0).min(f) * std::f32::consts::TAU);
            let ((x0, y0), (x1, y1)) = (at(a0), at(a1));
            for d in [-1, 0, 1] { cv.line(x0 + d, y0, x1 + d, y1, ACCENT); cv.line(x0, y0 + d, x1, y1 + d, ACCENT); }
        }
    }
    /// A titled popup with a close in its title row, rows one finger tall, in columns when the plot is not tall enough.
    fn popup(&mut self, title: &str, x: i32, y: i32, rows: Vec<(String, Action)>) {
        // inside the plot, so the popup never covers the bars or runs off the screen
        let g = self.grid;
        let per_col = (((g.h - POPUP_TITLE - 10) / POPUP_ROW).max(1) as usize).min(rows.len().max(1));
        let cols = rows.len().div_ceil(per_col).max(1) as i32;
        let width = (POPUP_COL_W * cols).min(g.w - 8);
        let height = per_col as i32 * POPUP_ROW + POPUP_TITLE + 6;
        let (x0, y0) = (g.x + 4, g.y.max(TOP.h));
        let rect = Rect::new(x.clamp(x0, (g.right() - width - 4).max(x0)), y.clamp(y0, (g.bottom() - height).max(y0)), width, height);
        self.workspace.popup = Some(Popup { rect, rows, title: title.into(), per_col });
    }
    fn open_sources(&mut self, x: i32, _y: i32) {
        let mut rows = Vec::new();
        for c in 0..n_ch() { rows.push((format!("CH{}  {}", c + 1, if self.m.ch[c].on { "On" } else { "Off" }), Action::Source(WaveId::Ch(c)))); }
        for i in 0..NREF {
            if let Some(reference) = &self.refs[i] { rows.push((format!("R{}  {}  {}", i + 1, if reference.on { "On" } else { "Off" }, reference.label), Action::Source(WaveId::Ref(i)))); }
        }
        rows.push(("Logic analyser".into(), Action::Menu(MenuId::La)));
        rows.push(("Generator".into(), Action::Menu(MenuId::Afg)));
        // sits on the bottom of the plot, above the bar that opened it
        self.popup("Sources", x, self.grid.bottom(), rows);
    }
    /// The quick actions of a math or reference chip of the Studio bottom bar: what a hold or a right-click opens.
    pub(super) fn studio_chip_popup(&mut self, chip: super::measdrag::HoldOn, x: i32, y: i32) {
        use super::measdrag::HoldOn;
        match chip {
            HoldOn::Math(k) => {
                let rows = vec![("Turn off".to_string(), Action::MathOff(k)), (format!("M{} settings", k + 1), Action::MathSettings(k))];
                self.popup(&format!("M{}", k + 1), x, y, rows);
            }
            HoldOn::Chan(c) => self.channel_popup(c, x, y),
            HoldOn::Ref(i) => {
                let on = self.refs.get(i).and_then(Option::as_ref).is_some_and(|r| r.on);
                let mut rows = vec![(if on { "Hide" } else { "Show" }.to_string(), Action::Source(WaveId::Ref(i))), ("Clear slot".to_string(), Action::RefClear(i)),
                    (format!("R{} settings", i + 1), Action::RefSettings(i))];
                if self.m.layout == Layout::Studio && on {
                    let own = self.studio_ref_own(i).map_or("Horizontal: Follow main".to_string(), |(t, _)| format!("Horizontal: Own {}/div", si_short(t as f64, "s")));
                    rows.insert(2, (own, Action::RefHorizontal(i)));
                }
                self.popup(&format!("R{}", i + 1), x, y, rows);
            }
            _ => {}
        }
    }
    /// The quick actions of a channel: what a hold on its chip, card, lane badge or marker opens, in every layout.
    pub(super) fn channel_popup(&mut self, c: usize, x: i32, y: i32) {
        if c >= n_ch() || self.m.touch_lock { return; }
        let ch = &self.m.ch[c];
        let to = if ch.coupling == Coupling::Ac { Coupling::Dc } else { Coupling::Ac };
        let mut rows = Vec::new();
        if board().couplings().contains(&to) { rows.push((format!("Coupling {}, set {}", ch.coupling.name(), to.name()), Action::ChCoupling(c))); }
        rows.push((format!("Invert {}", if ch.invert { "On, set Off" } else { "Off, set On" }), Action::ChInvert(c)));
        rows.push(("Zero offset".into(), Action::ChZero(c)));
        // Studio: the first row is the one people look for
        if ch.on { if self.m.layout == Layout::Studio { rows.insert(0, ("Turn off".into(), Action::Source(WaveId::Ch(c)))); } else { rows.push(("Turn off".into(), Action::Source(WaveId::Ch(c)))); } }
        rows.push((format!("CH{} settings", c + 1), Action::Menu(MenuId::Channel(c))));
        self.popup(&format!("CH{}", c + 1), x, y, rows);
    }
    /// Mouse right-click and a stationary touch hold take the exact same path.
    pub fn context_menu(&mut self, x: i32, y: i32) {
        if self.m.touch_lock { return; }
        if let Some(chip) = self.md_chip_at(x, y) { return self.studio_chip_popup(chip, x, y); }
        let eye = self.eye_open() && Rect::new(self.eye_pane_rect().x, self.eye_pane_rect().y, self.eye_pane_rect().w, HEADER).contains(x, y);
        let measurements = self.results_rects().is_some_and(|(head, _)| head.contains(x, y));
        if !eye && !measurements {
            if let Some(c) = self.channel_hotspot(x, y) { return self.channel_popup(c, x, y); }
        }
        // Studio: the plot has a context menu of its own, at the press point (studioctx.rs)
        if self.m.layout == Layout::Studio && !eye && !measurements { return self.studio_ctx_open(x, y); }
        let rows = if eye {
            vec![("Dock Eye right".into(), Action::EyeRight), ("Dock Eye left".into(), Action::EyeLeft), ("Dock Eye below".into(), Action::EyeBelow),
                 ("Float Eye".into(), Action::EyeFloat), ("Eye settings".into(), Action::Menu(MenuId::Eye)), ("Close Eye".into(), Action::EyeClose)]
        } else if measurements {
            vec![("Float measurements".into(), Action::MeasureFloat), ("Dock right".into(), Action::MeasureDock(StripDock::Right)),
                 ("Dock below".into(), Action::MeasureDock(StripDock::Below)), ("Dock left".into(), Action::MeasureDock(StripDock::Left)),
                 ("Add measurements".into(), Action::Menu(MenuId::MeasAdd))]
        } else {
            let mut rows = vec![("Overlay traces".into(), Action::Overlay), ("Stacked traces".into(), Action::Stacked),
                ("Fit record".into(), Action::FitRecord),
                ("Reset zoom".into(), Action::ZoomReset), ("Close zoom".into(), Action::ZoomClose),
                ("Single acquisition".into(), Action::Single), ("Run / Stop".into(), Action::Run), ("Autoset".into(), Action::Autoset),
                ("Float measurements".into(), Action::MeasureFloat)];
            for source in self.trace_sources().into_iter().take(5) {
                let id = match source { Trace::Channel(c) => WaveId::Ch(c), Trace::Reference(i) => WaveId::Ref(i), Trace::Math(_) => continue };
                rows.push((format!("Trigger on {}", match id { WaveId::Ch(c) => format!("CH{}", c + 1), WaveId::Ref(i) => format!("R{}", i + 1) }), Action::Trigger(id)));
            }
            rows.push(("Trigger settings".into(), Action::Menu(MenuId::Trigger)));
            rows.push(("Sources".into(), Action::Sources));
            rows
        };
        let mut rows = rows;
        // a view that fills the area has a way back in every menu of it
        if self.dock_full_state() { rows.insert(0, ("Restore layout".into(), Action::RestoreLayout)); }
        self.popup(if eye { "Eye" } else if measurements { "Measurements" } else { "Plot" }, x, y, rows);
    }
    pub(super) fn workspace_popup_tap(&mut self, x: i32, y: i32) -> bool {
        let Some(popup) = self.workspace.popup.take() else { return false };
        if popup.rect.contains(x, y) && !popup.close_rect().contains(x, y) {
            if let Some(i) = (0..popup.rows.len()).find(|&i| popup.row_rect(i).contains(x, y)) { self.workspace_action(popup.rows[i].1.clone()); }
        }
        true
    }
    pub(super) fn workspace_action(&mut self, action: Action) {
        match action {
            Action::Overlay => self.trace_mode(false), Action::Stacked => self.trace_mode(true),
            Action::Sources => self.open_sources(56, BOTTOM.y),
            Action::Source(WaveId::Ch(c)) => {
                if self.m.ch[c].on && self.trace_sources().len() <= 1 { self.toast("Keep one source on"); return; }
                if self.m.ch[c].on { self.m.ch[c].on = false; self.settings_changed(); }
                else { self.activate_live_channel(c); }
                self.relayout();
            }
            Action::Source(WaveId::Ref(i)) => {
                if self.refs[i].as_ref().is_some_and(|r| r.on) && self.trace_sources().len() <= 1 { self.toast("Keep one source on"); return; }
                if let Some(reference) = &mut self.refs[i] { reference.on = !reference.on; }
                self.select_workspace_source(WaveId::Ref(i));
                self.relayout();
            }
            Action::Trigger(id) => { self.set_trigger_source(id, false); }
            Action::Menu(MenuId::La) => self.key(key::LA),
            Action::Menu(menu) => self.open_menu(menu),
            Action::ZoomReset => { self.zoom_arm = false; self.zoom_path.clear(); self.reset_zoom(); }
            Action::ZoomClose => { self.zoom_arm = false; self.zoom_path.clear(); self.set_zoom(false); }
            Action::FitRecord => { self.fit_selected_record(); }
            Action::Live => self.return_to_live(), Action::Single => self.key(key::SINGLE), Action::Run => self.key(key::RUNSTOP),
            Action::Panel => self.desktop_panel = self.desktop_panel.map(|open| !open),
            Action::EyeRight => { self.workspace.eye_float = None; self.workspace.eye_left = false; self.eye_set_place(EyePlace::Side); self.relayout(); }
            Action::EyeLeft => { self.workspace.eye_float = None; self.workspace.eye_left = true; self.eye_set_place(EyePlace::Side); self.relayout(); }
            Action::EyeBelow => { self.workspace.eye_float = None; self.workspace.eye_left = false; self.eye_set_place(EyePlace::Under); self.relayout(); }
            Action::EyeFloat => { self.workspace.eye_float = Some(Rect::new(360, 100, 620, 360)); self.relayout(); }
            Action::MathOff(k) => self.math_turn_off(k),
            Action::MathSettings(k) => { self.m.math_select(k); self.open_menu(MenuId::Math); }
            Action::RefClear(i) => self.ref_clear_slot(i),
            Action::RefHorizontal(i) => { self.select_workspace_source(WaveId::Ref(i)); self.studio.sz.pop = false; self.studio.sz.ref_pop = Some(i); }
            Action::RefSettings(i) => { self.select_workspace_source(WaveId::Ref(i)); self.open_menu(MenuId::Ref); }
            Action::RestoreLayout => { self.dock_exit_full(); }
            Action::EyeClose => { self.m.view = AView::Scope; self.workspace.eye_float = None; self.relayout(); }
            Action::MeasureFloat => {
                self.workspace.measure_float = Some((W - RESULTS_W - 28, TOP.h + 32));
                self.workspace.measure_docked = false;
                self.stats_on = true; self.results_min = false; self.relayout();
            }
            Action::MeasureDock(dock) => {
                self.workspace.measure_float = None;
                self.workspace.measure_docked = true;
                self.m.strip_dock = dock;
                self.stats_on = true;
                self.relayout();
            }
            Action::ChCoupling(c) => self.toggle_ac_dc(c),
            Action::ChInvert(c) => { self.m.ch[c].invert = !self.m.ch[c].invert; self.toast(&format!("CH{}: invert {}", c + 1, if self.m.ch[c].invert { "on" } else { "off" })); self.settings_changed(); }
            Action::ChZero(c) => { self.m.ch[c].offset = 0.0; self.toast(&format!("CH{}: offset 0", c + 1)); self.settings_changed(); }
            Action::Autoset => self.key(key::AUTO),
            Action::Close => {},
        }
    }
    pub(super) fn workspace_draw(&self, cv: &mut Canvas) {
        if let Some(popup) = &self.workspace.popup {
            cv.round(popup.rect, 5, BTN_BORDER, 255);
            cv.round(popup.rect.inset(1), 4, BAR, 255);
            let title = Rect::new(popup.rect.x + 10, popup.rect.y + 3, popup.rect.w - POPUP_TITLE - 16, POPUP_TITLE);
            cv.set_clip(title); cv.text_in(&self.fonts().sb, title, 0, &popup.title, ACCENT); cv.no_clip();
            let x = popup.close_rect();
            let (cx, cy) = (x.x + x.w / 2, x.y + x.h / 2);
            cv.line(cx - 6, cy - 6, cx + 6, cy + 6, DIM); cv.line(cx - 6, cy + 6, cx + 6, cy - 6, DIM);
            cv.hline(popup.rect.x + 5, popup.rect.right() - 6, popup.rect.y + 3 + POPUP_TITLE - 1, LINE);
            for (index, (label, action)) in popup.rows.iter().enumerate() {
                let r = popup.row_rect(index);
                cv.set_clip(r); cv.text_in(&self.fonts().s, Rect::new(r.x + 7, r.y, r.w - 34, r.h), 0, label, TEXT); cv.no_clip();
                let current = match action { Action::Overlay => !self.trace_stacked(), Action::Stacked => self.trace_stacked(), _ => false };
                if current {
                    // a check at the right end marks the mode in force
                    let (cx, cy) = (r.x + r.w - 40, r.y + r.h / 2);
                    cv.line(cx, cy, cx + 4, cy + 4, ACCENT); cv.line(cx + 4, cy + 4, cx + 11, cy - 5, ACCENT);
                }
            }
        }
        self.draw_press_ring(cv);
        if self.workspace.gesture.is_some_and(|gesture| gesture.moved && gesture.panel != Panel::Plot) {
            let (left, width) = self.plot_span();
            let (top, bottom) = (self.plot_top(), self.plot_bottom());
            for r in [Rect::new(left + 4, top, 68, (bottom - top).max(1)),
                      Rect::new(left + width - 72, top, 68, (bottom - top).max(1)),
                      Rect::new(left + 76, bottom - 52, (width - 152).max(1), 48)] {
                cv.round_outline(r, 4, ACCENT);
                cv.text_in(&self.fonts().s, Rect::new(r.x, r.y + 8, r.w, 24), 1, "Dock", ACCENT);
            }
            if self.workspace.gesture.is_some_and(|gesture| gesture.panel == Panel::Measurements) {
                let r = Rect::new(left + 76, top, (width - 152).max(1), 32);
                cv.round_outline(r, 4, ACCENT);
                cv.text_in(&self.fonts().s, r, 1, "Dock above", ACCENT);
            }
        }
    }
    /// Another hold took the finger: the plot gesture under it must not open its own menu too.
    pub(super) fn workspace_drop_gesture(&mut self) { self.workspace.gesture = None; }
    #[cfg(test)]
    pub(super) fn workspace_popup_open(&self) -> bool { self.workspace.popup.is_some() }
    /// Fit all lanes: the vertical scale and position of every source that is shown, so that each fills about
    /// three quarters of its lane (or of the graticule, in Overlay). Channels step to the V/div that holds their
    /// signal, references and math channels likewise. The time base, the trigger and what is on stay as they are.
    pub(super) fn fit_all_lanes(&mut self) {
        self.workspace.popup = None;
        let range = |v: &[f32]| {
            let (lo, hi) = v.iter().filter(|x| x.is_finite()).fold((f32::MAX, f32::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
            (hi >= lo).then_some((lo, hi))
        };
        let mut fitted = 0;
        // channels: from the record on screen
        let spans: Vec<Option<(f32, f32)>> = (0..n_ch()).map(|c| {
            if !self.m.ch[c].on || self.record_mode() { return None; }
            self.last[0].as_ref().and_then(|r| r.ch.get(c)).and_then(|v| range(v))
        }).collect();
        for (c, span) in spans.into_iter().enumerate() {
            let Some((lo, hi)) = span else { continue };
            let ch = &mut self.m.ch[c];
            let want = ((hi - lo) / 6.0).max(1e-6);
            ch.vfine = 1.0;
            ch.vscale = (0..VSCALES.len()).find(|&i| VSCALES[i] * ch.probe >= want).unwrap_or(VSCALES.len() - 1);
            // the position on half a division, so the lane's labels are round values
            let half = ch.volts_per_div() / 2.0;
            ch.offset = -(((hi + lo) / 2.0) / half).round() * half;
            fitted += 1;
        }
        self.m.clamp_offsets();
        // references that are shown
        for i in 0..NREF {
            let Some(r) = self.refs[i].as_mut().filter(|r| r.on) else { continue };
            let Some((lo, hi)) = range(&r.v) else { continue };
            let want = ((hi - lo) / 6.0).max(1e-6);
            r.vpd = VSCALES.iter().copied().find(|&x| x >= want).unwrap_or(VSCALES[VSCALES.len() - 1]);
            let half = r.vpd / 2.0;
            r.offset = -(((hi + lo) / 2.0) / half).round() * half;
            fitted += 1;
        }
        // math channels that are shown
        for k in 0..NMATH {
            if self.m.math_shown(k) { self.math_fit(k); fitted += 1; }
        }
        if fitted == 0 { return self.toast("Fit all lanes: nothing is shown"); }
        self.settings_changed();
        self.clear_measurement_statistics();
        self.dirty_map = true;
        self.relayout();
        self.toast(&format!("Fitted {fitted} {}", if fitted == 1 { "source" } else { "sources" }));
    }

    /// Turns a source on or off, as its row of the Sources menu does.
    pub(super) fn source_toggle(&mut self, id: WaveId) { self.workspace_action(Action::Source(id)); }
    #[cfg(test)]
    pub(super) fn workspace_stacked_flag(&self) -> bool { self.workspace.stacked }
    /// Stops the hold of the plot gesture. True when it had already opened its menu, which is closed with it.
    pub(super) fn workspace_cancel_hold(&mut self) -> bool {
        let opened = self.workspace.gesture.is_some_and(|gesture| gesture.held);
        if opened { self.workspace.popup = None; }
        self.workspace.gesture = None;
        opened
    }
    pub(super) fn workspace_hold_tick(&mut self) {
        let Some(mut gesture) = self.workspace.gesture else { return };
        if gesture.moved || gesture.held || self.now - gesture.at < HOLD { return; }
        if self.menu_open || self.dialog.is_some() || self.power_dlg || self.wlan.osk.is_some() || self.m.touch_lock { return; }
        gesture.held = true;
        self.workspace.gesture = Some(gesture);
        self.context_menu(gesture.x, gesture.y);
    }
    pub(super) fn workspace_touch(&mut self, phase: Phase, x: i32, y: i32) -> bool {
        // Menus and dialogs own the pointer before anything behind them can
        // fold, drag or open a context menu on a panel header.
        if self.load_slot_ask || self.menu_open || self.dialog.is_some() || self.power_dlg || self.wlan.osk.is_some() {
            self.workspace.gesture = None;
            return false;
        }
        if self.workspace.popup.is_some() && !self.workspace.gesture.is_some_and(|gesture| gesture.held) {
            if phase == Phase::Up { if std::mem::take(&mut self.workspace.lift_pending) { /* the lift of the hold */ } else { self.workspace_popup_tap(x, y); } }
            else if phase == Phase::Down { self.workspace.lift_pending = false; }
            return true;
        }
        if phase == Phase::Down && (self.full_btn().1.contains(x, y) || self.zoom_btn().1.contains(x, y)
            || (self.m.zoom && self.zoom_readout().contains(x, y))) {
            self.workspace.gesture = None;
            return false;
        }
        match phase {
            Phase::Down => {
                if self.workspace.popup.is_some() { return false; }
                let panel = if self.eye_open() && self.m.layout != Layout::Studio && Rect::new(self.eye_pane_rect().x, self.eye_pane_rect().y, self.eye_pane_rect().w, HEADER).contains(x, y)
                    && !self.eye_pane_close().contains(x, y) { Panel::Eye }
                else if self.results_rects().is_some_and(|(header, _)| header.contains(x, y)) { Panel::Measurements }
                else if self.grid.contains(x, y) && !self.menu_open { Panel::Plot }
                else { return false };
                let origin = match panel { Panel::Eye => self.eye_pane_rect(), Panel::Measurements => { let (head, body) = self.results_rects().unwrap(); Rect::new(head.x, head.y, head.w, head.h + body.h) }, Panel::Plot => self.grid };
                self.workspace.gesture = Some(Gesture { panel, origin, x, y, at: self.now, moved: false, held: false });
                panel != Panel::Plot
            }
            Phase::Move => {
                let Some(mut gesture) = self.workspace.gesture else { return false };
                if (x - gesture.x).abs().max((y - gesture.y).abs()) > MOVE { gesture.moved = true; }
                if gesture.held {
                    if !gesture.moved || gesture.panel == Panel::Plot { return true; }
                    // A stationary hold opens context actions, but moving the same
                    // finger can still take the header into a drag.
                    gesture.held = false;
                    self.workspace.popup = None;
                }
                self.workspace.gesture = Some(gesture);
                if !gesture.moved || gesture.panel == Panel::Plot { return gesture.panel != Panel::Plot; }
                let r = gesture.origin;
                let left = (r.x + x - gesture.x).clamp(0, W - r.w.min(W));
                let top = (r.y + y - gesture.y).clamp(TOP.h, (BOTTOM.y - r.h.min(BOTTOM.y - TOP.h)).max(TOP.h));
                match gesture.panel {
                    Panel::Eye => { self.workspace.eye_float = Some(Rect::new(left, top, r.w.max(320).min(W), r.h.max(180).min(BOTTOM.y - top))); self.relayout(); }
                    Panel::Measurements => { self.workspace.measure_float = Some((left, top)); self.workspace.measure_docked = false; self.relayout(); }
                    _ => {},
                }
                true
            }
            Phase::Up => {
                let Some(gesture) = self.workspace.gesture.take() else { return false };
                if gesture.held { self.touch.down = false; self.touch.drag = Drag::None; return true; }
                if gesture.panel == Panel::Plot { return false; }
                if gesture.moved {
                    let right = x >= self.plot_span().0 + self.plot_span().1 - 76;
                    let left = x <= self.plot_span().0 + 76;
                    let bottom = y >= self.plot_bottom() - 56;
                    let top = y <= self.plot_top() + 36;
                    match gesture.panel {
                        Panel::Eye if right => self.workspace_action(Action::EyeRight),
                        Panel::Eye if left => self.workspace_action(Action::EyeLeft),
                        Panel::Eye if bottom => self.workspace_action(Action::EyeBelow),
                        Panel::Measurements if right => self.workspace_action(Action::MeasureDock(StripDock::Right)),
                        Panel::Measurements if left => self.workspace_action(Action::MeasureDock(StripDock::Left)),
                        Panel::Measurements if bottom => self.workspace_action(Action::MeasureDock(StripDock::Below)),
                        Panel::Measurements if top => self.workspace_action(Action::MeasureDock(StripDock::Above)),
                        _ => {},
                    }
                } else if gesture.panel == Panel::Measurements { self.results_min = !self.results_min; }
                else if gesture.panel == Panel::Eye { self.eye_header_tap(x, y); }
                true
            }
        }
    }
    /// Escape closes transient surfaces and leaves box/zoom modes immediately.
    pub fn dismiss_context(&mut self) {
        if self.load_slot_ask { self.cancel_load_slot(); return; }
        let was_drawing = self.zoom_arm;
        self.workspace.popup = None; self.workspace.gesture = None;
        self.zoom_arm = false; self.zoom_path.clear(); self.zoom_map = None;
        self.touch.down = false; self.touch.drag = Drag::None;
        if self.menu_open { self.close_menu(); }
        // a view that fills the area: Escape is the way back
        else if self.dock_exit_full() {}
        else if self.m.zoom && !was_drawing { self.set_zoom(false); }
    }
    pub fn cancel_pointer(&mut self) {
        self.workspace.gesture = None;
        self.touch.down = false; self.touch.held = false; self.touch.drag = Drag::None;
        self.pinch.active = false;
    }

    pub(super) fn fit_selected_record(&mut self) -> bool {
        let Some(source) = self.selected_workspace_source() else { self.toast("No retained waveform to fit"); return false };
        let metadata = match source {
            WaveId::Ref(i) => self.refs[i].as_ref().map(|r| (r.v.len(), r.dt, self.source_time_origin(source, r.t0))),
            WaveId::Ch(c) => self.raw_info(c).or_else(|| self.last[0].as_ref().map(|r| (r.ch[c].len(), r.dt, r.t0)))
                .map(|(n, dt, t0)| (n, dt, self.source_time_origin(source, t0))),
        };
        let Some((points, dt, t0)) = metadata.filter(|&(n, dt, t0)| n >= 2 && dt.is_finite() && dt > 0.0 && t0.is_finite()) else {
            self.toast("No retained waveform to fit"); return false;
        };
        if let WaveId::Ref(i) = source {
            let r = self.refs[i].as_ref().unwrap();
            let summary = crate::wave::autoset_summary(&r.v, r.dt);
            if summary.valid {
                let want = ((summary.vmax - summary.vmin) / (VDIV * 0.8)).max(VSCALES[0]);
                let vpd = VSCALES.iter().copied().find(|&scale| scale >= want).unwrap_or(VSCALES[VSCALES.len()-1]);
                let offset = -0.5 * (summary.vmax + summary.vmin);
                let r = self.refs[i].as_mut().unwrap(); r.vpd = vpd; r.offset = offset;
            }
        }
        let end = t0 + points.saturating_sub(1) as f64 * dt;
        let center = (t0 + (end - t0) * 0.5) as f32;
        let required = 2.0 * (center as f64 - t0).max(end - center as f64) / HDIV as f64;
        let scales = tscales();
        self.m.tscale = scales.iter().position(|&scale| scale as f64 >= required).unwrap_or(scales.len()-1);
        self.m.tfine = if required > scales[self.m.tscale] as f64 { (required / scales[self.m.tscale] as f64) as f32 } else { 1.0 };
        self.m.delay = center;
        self.set_zoom(false);
        self.zoom_vfactor = 1.0; self.zoom_vcenter = 0.0;
        self.dirty_map = true;
        self.toast("Fit record");
        true
    }
    pub(crate) fn reset_workspace_for_import(&mut self) {
        self.cancel_pointer(); self.workspace.popup = None;
        self.workspace.eye_float = None; self.workspace.eye_left = false;
        self.workspace.measure_docked = false;
        self.workspace.measure_float = None; self.stats_on = false;
        self.m.pane = 0; self.m.win_layout = WinLayout::One;
        self.m.density = if self.full_on() { Density::Full } else { Density::Scope };
        self.zoom_arm = false; self.zoom_path.clear(); self.zoom_map = None;
    }
}
