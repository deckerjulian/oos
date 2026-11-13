// SPDX-License-Identifier: GPL-2.0-or-later
//! Compact scope chrome, source lanes and pointer gestures shared by touch and mouse.
use super::*;

const HOLD: f64 = 0.55;
const MOVE: i32 = 8;
pub(super) const HEADER: i32 = 26;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Trace { Channel(usize), Reference(usize), Math(usize) }
#[derive(Clone, Copy, PartialEq)]
enum Panel { Plot, Eye, Measurements }
#[derive(Clone, Copy)]
struct Gesture { panel: Panel, origin: Rect, x: i32, y: i32, at: f64, moved: bool, held: bool }
#[derive(Clone)]
enum Action {
    Overlay, Stacked, Sources, Source(WaveId), Trigger(WaveId), Menu(MenuId),
    ZoomReset, ZoomClose, FitRecord, Live, Single, Run, Panel,
    EyeRight, EyeLeft, EyeBelow, EyeFloat, EyeClose,
    MeasureFloat, MeasureDock(StripDock), Close,
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
        let (overview, detail) = Ui::split_zoom(ui.grid);
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
        assert_eq!(ui.wins, vec![Ui::split_zoom(ui.grid).1]);
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
        let overview = Ui::split_zoom(ui.grid).0;
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
        ui.m.strip_dock = StripDock::Under;
        ui.relayout();
        let mut cv = Canvas::new(W, H);
        assert!(ui.draw_paged_measurement_strip(&mut cv));
        // The cursor row follows the three readout rows and two navigation rows.
        let y = ui.strip_band_y() + 5 * STRIP_ROW;
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
        let (x,y)=(popup.rect.x+30,popup.rect.y+4+index as i32*30+15);
        for phase in [Phase::Down,Phase::Up]{ui.handle(Ev::Touch{id:0,phase,x,y});}
        assert!(!ui.m.zoom && ui.m.delay.abs()<1e-3);
        assert_eq!((ui.m.run,ui.replay.captures),(RunState::Stop,0));
    }
}
struct Popup { rect: Rect, rows: Vec<(String, Action)> }
#[derive(Default)]
pub(super) struct Workspace {
    selected_source: Option<WaveId>,
    pub stacked: bool,
    pub eye_float: Option<Rect>,
    pub eye_left: bool,
    pub measure_float: Option<(i32, i32)>,
    pub measure_dock: Option<StripDock>,
    gesture: Option<Gesture>,
    popup: Option<Popup>,
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
    pub(super) fn select_workspace_source(&mut self, id: WaveId) {
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
        self.selected_workspace_source().map(|id| (self.wave_name(id), self.wave_color(id)))
    }
    pub(super) fn cursor_source_context(&self) -> Option<(WaveId, f32, f32, &'static str)> {
        let id = self.selected_workspace_source()?;
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
        cv.text(&self.fonts.mb, dialog.r.x + 14, dialog.r.y + 31, "Pairs", TEXT);
        let targets = self.pair_header_sources(dialog);
        for ((_, rect), id) in targets.into_iter().zip([self.meas_src, self.meas_src_b]) {
            btn(cv, rect, BTN, BTN_BORDER);
            cv.set_clip(Rect::new(rect.x + 5, rect.y, (rect.w - 27).max(1), rect.h));
            cv.text_in(&self.fonts.sb, Rect::new(rect.x + 5, rect.y, (rect.w - 27).max(1), rect.h), 1, &self.pair_source_caption(id), self.wave_color(id));
            cv.no_clip();
            chevron_down(cv, rect.x + rect.w - 16, rect.y + rect.h / 2 - 3, DIM);
        }
        let a = targets[0].1;
        cv.text_in(&self.fonts.mb, Rect::new(a.x + a.w, a.y, 26, a.h), 1, "to", TEXT);
    }
    pub(super) fn stacked(&self) -> bool {
        self.workspace.stacked && !self.xy && matches!(self.m.view, AView::Scope | AView::Eye)
    }
    pub(super) fn trace_sources(&self) -> Vec<Trace> {
        let mut sources: Vec<_> = (0..n_ch()).filter(|&c| self.m.ch[c].on).map(Trace::Channel).collect();
        sources.extend((0..NREF).filter(|&i| self.refs[i].as_ref().is_some_and(|r| r.on)).map(Trace::Reference));
        sources.extend((0..NMATH).filter(|&k| self.m.math_ch(k).op != MathOp::Off).map(Trace::Math));
        sources
    }
    pub(super) fn source_area(&self, id: WaveId) -> Option<Rect> {
        if !self.stacked() { return Some(self.wave_area()); }
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
    fn trace_mode(&mut self, stacked: bool) {
        self.workspace.stacked = stacked;
        self.m.win_layout = WinLayout::One;
        self.workspace.popup = None;
        self.relayout();
        self.dirty_map = true;
    }
    fn footer_rects(&self) -> (Rect, Rect, Rect, Rect, Option<Rect>, Option<Rect>, Rect) {
        let y = BOTTOM.y + 3;
        let mut right = W - 4;
        let panel = self.desktop_panel.map(|_| { right -= 48; let r = Rect::new(right, y, 46, 30); right -= 4; r });
        let live = self.record_mode().then(|| { right -= 44; let r = Rect::new(right, y, 42, 30); right -= 4; r });
        right -= 222;
        let trigger = Rect::new(right, y, 222, 30);
        right -= 128;
        let time = Rect::new(right, y, 124, 30);
        right -= 124;
        let overlay = Rect::new(right, y, 58, 30);
        let stacked = Rect::new(right + 62, y, 58, 30);
        (overlay, stacked, time, trigger, live, panel, Rect::new(126, y, (right - 132).max(0), 30))
    }
    fn source_pills(&self) -> Vec<(WaveId, Rect)> {
        let r = self.footer_rects().6;
        let mut sources: Vec<_> = (0..n_ch()).filter(|&c| self.m.ch[c].on).map(WaveId::Ch).collect();
        sources.extend((0..NREF).filter(|&i| self.refs[i].as_ref().is_some_and(|reference| reference.on || self.ref_sel == i)).map(WaveId::Ref));
        let count = sources.len().min((r.w / 42).max(1) as usize);
        let active = self.selected_workspace_source();
        if let Some(index) = sources.iter().position(|id| Some(*id) == active) {
            if index >= count && count > 0 { sources.swap(index, count - 1); }
        }
        let width = (r.w / count.max(1) as i32).clamp(38, 78);
        sources.into_iter().take(count).enumerate().map(|(i, id)| (id, Rect::new(r.x + i as i32 * width, r.y, width - 4, r.h))).collect()
    }
    pub(super) fn compact_cards(&self) -> Cards {
        let mut ch = [Rect::new(0, BOTTOM.y, 0, 30); NCH];
        for (id, rect) in self.source_pills() { if let WaveId::Ch(c) = id { ch[c] = rect; } }
        let (_, _, time, trig, _, _, _) = self.footer_rects();
        Cards { ch, la: None, gen: None, time, trig }
    }
    pub(super) fn draw_workspace_footer(&self, cv: &mut Canvas) {
        cv.fill(BOTTOM, BAR);
        cv.hline(0, W - 1, BOTTOM.y, LINE);
        let draw = |cv: &mut Canvas, r, label: &str, on| {
            btn(cv, r, BTN, if on { ACCENT } else { BTN_BORDER });
            cv.text_in(&self.fonts.s, r, 1, label, if on { ACCENT } else { TEXT });
        };
        draw(cv, Self::MENU_BTN, if self.m.touch_lock { "Unlock" } else { "Menu" }, false);
        draw(cv, Rect::new(56, BOTTOM.y + 3, 64, 30), "Sources", false);
        for (id, rect) in self.source_pills() {
            let (label, selected) = match id {
                WaveId::Ch(c) => (format!("CH{}", c + 1), self.selected_workspace_source() == Some(id)),
                WaveId::Ref(i) => (format!("R{}", i + 1), self.selected_workspace_source() == Some(id)),
            };
            let color = self.wave_color(id);
            btn(cv, rect, BAR, if selected { color } else { BTN_BORDER });
            cv.text_in(&self.fonts.sb, rect, 1, &label, color);
        }
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
        let (overlay, stacked, time, trigger, live, panel, _) = self.footer_rects();
        if overlay.contains(x, y) { self.trace_mode(false); }
        else if stacked.contains(x, y) { self.trace_mode(true); }
        else if time.contains(x, y) { self.toggle_menu(MenuId::Horizontal); }
        else if live.is_some_and(|r| r.contains(x, y)) { self.return_to_live(); }
        else if panel.is_some_and(|r| r.contains(x, y)) { self.desktop_panel = self.desktop_panel.map(|open| !open); }
        else { self.compact_trigger_tap(x, y, trigger); }
        true
    }
    fn popup(&mut self, x: i32, y: i32, rows: Vec<(String, Action)>) {
        let width = 244;
        let height = rows.len() as i32 * 30 + 8;
        let rect = Rect::new(x.clamp(4, W - width - 4), y.clamp(TOP.h, (BOTTOM.y - height).max(TOP.h)), width, height);
        self.workspace.popup = Some(Popup { rect, rows });
    }
    fn open_sources(&mut self, x: i32, y: i32) {
        let mut rows = Vec::new();
        for c in 0..n_ch() { rows.push((format!("CH{}  {}", c + 1, if self.m.ch[c].on { "On" } else { "Off" }), Action::Source(WaveId::Ch(c)))); }
        for i in 0..NREF {
            if let Some(reference) = &self.refs[i] { rows.push((format!("R{}  {}  {}", i + 1, if reference.on { "On" } else { "Off" }, reference.label), Action::Source(WaveId::Ref(i)))); }
        }
        rows.push(("Logic analyser".into(), Action::Menu(MenuId::La)));
        rows.push(("Generator".into(), Action::Menu(MenuId::Afg)));
        self.popup(x, y - rows.len() as i32 * 30 - 8, rows);
    }
    /// Mouse right-click and a stationary touch hold take the exact same path.
    pub fn context_menu(&mut self, x: i32, y: i32) {
        if self.m.touch_lock { return; }
        let eye = self.eye_open() && Rect::new(self.eye_pane_rect().x, self.eye_pane_rect().y, self.eye_pane_rect().w, HEADER).contains(x, y);
        let measurements = self.results_rects().is_some_and(|(head, _)| head.contains(x, y));
        let rows = if eye {
            vec![("Dock Eye right".into(), Action::EyeRight), ("Dock Eye left".into(), Action::EyeLeft), ("Dock Eye below".into(), Action::EyeBelow),
                 ("Float Eye".into(), Action::EyeFloat), ("Eye settings".into(), Action::Menu(MenuId::Eye)), ("Close Eye".into(), Action::EyeClose)]
        } else if measurements {
            vec![("Float measurements".into(), Action::MeasureFloat), ("Dock right".into(), Action::MeasureDock(StripDock::Right)),
                 ("Dock below".into(), Action::MeasureDock(StripDock::Under)), ("Dock left".into(), Action::MeasureDock(StripDock::Left)),
                 ("Add measurements".into(), Action::Menu(MenuId::MeasAdd))]
        } else {
            let mut rows = vec![("Overlay traces".into(), Action::Overlay), ("Stacked traces".into(), Action::Stacked),
                ("Fit record".into(), Action::FitRecord),
                ("Reset zoom".into(), Action::ZoomReset), ("Close zoom".into(), Action::ZoomClose),
                ("Single acquisition".into(), Action::Single), ("Run / Stop".into(), Action::Run),
                ("Float measurements".into(), Action::MeasureFloat)];
            for source in self.trace_sources().into_iter().take(5) {
                let id = match source { Trace::Channel(c) => WaveId::Ch(c), Trace::Reference(i) => WaveId::Ref(i), Trace::Math(_) => continue };
                rows.push((format!("Trigger on {}", match id { WaveId::Ch(c) => format!("CH{}", c + 1), WaveId::Ref(i) => format!("R{}", i + 1) }), Action::Trigger(id)));
            }
            rows.push(("Trigger settings".into(), Action::Menu(MenuId::Trigger)));
            rows.push(("Sources".into(), Action::Sources));
            rows
        };
        self.popup(x, y, rows);
    }
    pub(super) fn workspace_popup_tap(&mut self, x: i32, y: i32) -> bool {
        let Some(popup) = self.workspace.popup.take() else { return false };
        if popup.rect.contains(x, y) {
            let index = ((y - popup.rect.y - 4).max(0) / 30) as usize;
            if let Some((_, action)) = popup.rows.get(index) { self.workspace_action(action.clone()); }
        }
        true
    }
    fn workspace_action(&mut self, action: Action) {
        match action {
            Action::Overlay => self.trace_mode(false), Action::Stacked => self.trace_mode(true),
            Action::Sources => self.open_sources(56, BOTTOM.y),
            Action::Source(WaveId::Ch(c)) => {
                if self.m.ch[c].on { self.m.ch[c].on = false; self.settings_changed(); }
                else { self.activate_live_channel(c); }
                self.relayout();
            }
            Action::Source(WaveId::Ref(i)) => {
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
            Action::EyeClose => { self.m.view = AView::Scope; self.workspace.eye_float = None; self.relayout(); }
            Action::MeasureFloat => {
                self.workspace.measure_float = Some((W - RESULTS_W - 28, TOP.h + 32));
                self.workspace.measure_dock = None;
                self.stats_on = true; self.results_min = false; self.relayout();
            }
            Action::MeasureDock(dock) => {
                self.workspace.measure_float = None;
                self.workspace.measure_dock = Some(dock);
                self.stats_on = true;
                self.relayout();
            }
            Action::Close => {},
        }
    }
    pub(super) fn workspace_draw(&self, cv: &mut Canvas) {
        if let Some(popup) = &self.workspace.popup {
            cv.round(popup.rect, 5, BTN_BORDER, 255);
            cv.round(popup.rect.inset(1), 4, BAR, 255);
            for (index, (label, _)) in popup.rows.iter().enumerate() {
                let r = Rect::new(popup.rect.x + 5, popup.rect.y + 4 + index as i32 * 30, popup.rect.w - 10, 30);
                cv.set_clip(r); cv.text(&self.fonts.s, r.x + 7, r.y + 20, label, TEXT); cv.no_clip();
            }
        }
        if self.workspace.gesture.is_some_and(|gesture| gesture.moved && gesture.panel != Panel::Plot) {
            let (left, width) = self.plot_span();
            let (top, bottom) = (self.plot_top(), self.plot_bottom());
            for r in [Rect::new(left + 4, top, 68, (bottom - top).max(1)),
                      Rect::new(left + width - 72, top, 68, (bottom - top).max(1)),
                      Rect::new(left + 76, bottom - 52, (width - 152).max(1), 48)] {
                cv.round_outline(r, 4, ACCENT);
                cv.text_in(&self.fonts.s, Rect::new(r.x, r.y + 8, r.w, 24), 1, "Dock", ACCENT);
            }
            if self.workspace.gesture.is_some_and(|gesture| gesture.panel == Panel::Measurements) {
                let r = Rect::new(left + 76, top, (width - 152).max(1), 32);
                cv.round_outline(r, 4, ACCENT);
                cv.text_in(&self.fonts.s, r, 1, "Dock above", ACCENT);
            }
        }
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
            if phase == Phase::Up { self.workspace_popup_tap(x, y); }
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
                let panel = if self.eye_open() && Rect::new(self.eye_pane_rect().x, self.eye_pane_rect().y, self.eye_pane_rect().w, HEADER).contains(x, y)
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
                    Panel::Measurements => { self.workspace.measure_float = Some((left, top)); self.workspace.measure_dock = None; self.relayout(); }
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
                        Panel::Measurements if bottom => self.workspace_action(Action::MeasureDock(StripDock::Under)),
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
        self.workspace.measure_dock = None;
        self.workspace.measure_float = None; self.stats_on = false;
        self.m.pane = 0; self.m.win_layout = WinLayout::One;
        self.m.density = if self.full_on() { Density::Full } else { Density::Scope };
        self.zoom_arm = false; self.zoom_path.clear(); self.zoom_map = None;
    }
}
