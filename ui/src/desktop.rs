// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The scope screen in a desktop window (Windows host build), with the demo signal or a replayed frame.
//!
//! The mouse is the finger: a left click is a tap, a left drag is a touch drag, and right click opens the touch-hold context menu. The wheel turns the knobs and the
//! keyboard stands in for the front panel:
//!
//!   wheel          horizontal pan; menu selection         Enter        Multi 1 push
//!   Shift+wheel    V scale                                  Up / Down    V position
//!   Ctrl+wheel     time-scale zoom around the pointer       Left / Right H position
//!   Alt+wheel      trigger level                            PgUp / PgDn  trigger level
//!   Space  Run/Stop     S  Single     A  Auto     C  Clear     M  Measure    U  Cursor
//!   1-4    CH1-CH4      F  Math       R  Ref      L  LA        G  Generator  T  Trigger menu
//!   H      Horizontal menu            Q  Quick    Z  zoom (H scale push)     F12  screenshot (PNG)
//!
//! Hardware buttons and knobs are a separate movable window. The scope display
//! always uses its full 1024 x 600 buffer. F6 shows or hides the hardware window.
//! Ctrl+O opens the native waveform picker. LAN services control this same session.

use crate::cmd;
use crate::gfx::Canvas;
use crate::ui::{self, Ev, Phase, Ui};
use minifb::{Key, MouseButton, MouseMode, Scale, ScaleMode, Window, WindowOptions};
use std::time::Instant;

mod lan;
mod panel;
mod picker;
mod shell;
mod input;
mod front_window;

/// One wheel notch as minifb reports it on Windows (WHEEL_DELTA 120 x 0.1).
const NOTCH: f32 = 12.0;

/// The command a key stands for (cmd.rs syntax), None for keys without one.
fn key_cmd(k: Key) -> Option<&'static str> {
    Some(match k {
        Key::Space => "key runstop",
        Key::S => "key single",
        Key::A => "key auto",
        Key::C => "key clear",
        Key::M => "key measure",
        Key::U => "key cursor",
        Key::Key1 => "key ch1",
        Key::Key2 => "key ch2",
        Key::Key3 => "key ch3",
        Key::Key4 => "key ch4",
        Key::F => "key math",
        Key::R => "key ref",
        Key::L => "key la",
        Key::G => "key gi",
        Key::T => "key tmenu",
        Key::H => "key hmenu",
        Key::Q => "key quick",
        Key::Z => "knob hscale push",
        Key::Enter | Key::NumPadEnter => "knob multi1 push",
        Key::Up => "knob vpos +5",
        Key::Down => "knob vpos -5",
        Key::Right => "knob hpos +5",
        Key::Left => "knob hpos -5",
        Key::PageUp => "knob tlevel +5",
        Key::PageDown => "knob tlevel -5",
        _ => return None,
    })
}

fn dispatch(ui: &mut Ui, command: &str) {
    if let Some(action) = command.strip_prefix("workflow ") {
        ui.workflow_command(action);
    } else {
        cmd::run(ui, command);
    }
}

fn open_waveform(ui: &mut Ui, win: &Window, loader: &mut picker::Loader) {
    loader.cancel();
    let Some(slot) = ui.waveform_destination() else { return; };
    match picker::waveform(win.get_window_handle(), slot) {
        Ok(Some(path)) => {
            match loader.submit(path, ui.waveform_epoch(), slot) {
                Ok(name) => ui.toast(&format!("Loading {name}…")),
                Err(error) => ui.toast(&error),
            }
        }
        Ok(None) => ui.toast("Open canceled; the current waveform is unchanged"),
        Err(error) => ui.toast(&error),
    }
}

#[derive(Default)]
struct PointerState {
    touch: Option<(i32, i32)>,
    panel_pressed: Option<usize>,
}

impl PointerState {
    fn apply(&mut self, ui: &mut Ui, phase: input::Pointer, at: Option<(i32, i32)>, panel_open: bool, controls: &[panel::Control]) {
        let on_screen = at.filter(|&(x, _)| x < ui::W);
        match phase {
            input::Pointer::Down => {
                self.panel_pressed = if panel_open { at.and_then(|(x, y)| controls.iter().position(|c| c.rect.contains(x, y))) } else { None };
                self.touch = on_screen;
                if let Some((x, y)) = on_screen { ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y }); }
            }
            input::Pointer::Move => {
                if let Some(previous) = self.touch {
                    if let Some((x, y)) = on_screen {
                        if previous != (x, y) { ui.handle(Ev::Touch { id: 0, phase: Phase::Move, x, y }); }
                        self.touch = Some((x, y));
                    } else {
                        ui.cancel_pointer();
                        self.touch = None;
                    }
                }
            }
            input::Pointer::Up => {
                if let (Some(index), Some((x, y))) = (self.panel_pressed.take(), at) {
                    if panel_open && controls[index].rect.contains(x, y) { dispatch(ui, controls[index].command); }
                }
                if self.touch.take().is_some() {
                    if let Some((x, y)) = on_screen { ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y }); }
                    else { ui.cancel_pointer(); }
                }
            }
            input::Pointer::Context if self.touch.is_none() && self.panel_pressed.is_none() => {
                if let Some((x, y)) = on_screen { ui.context_menu(x, y); }
            }
            input::Pointer::Cancel => {
                self.touch = None;
                self.panel_pressed = None;
                ui.cancel_pointer();
            }
            _ => {}
        }
    }
}

pub fn run(mut ui: Ui, args: &[String]) {
    ui.desktop_panel = Some(true);
    let opts = WindowOptions { resize: true, scale: Scale::X1, scale_mode: ScaleMode::AspectRatioStretch, ..WindowOptions::default() };
    let mut win = match Window::new("OOS Plus — F6: Hardware controls", ui::W as usize, ui::H as usize, opts) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("oos-ui: window: {}", e);
            std::process::exit(1);
        }
    };
    // Poll input faster than drawing: short clicks must not disappear between 30 fps frames.
    win.set_target_fps(120);
    let input = input::Input::attach(&mut win);
    if !input.mouse_captured() { eprintln!("oos-ui: native mouse event capture unavailable; using polling"); }
    let mut cv = Canvas::new(ui::W, ui::H);
    let mut front_window: Option<front_window::FrontWindow> = None;
    let mut services = lan::Services::open(args);
    let mut loader = picker::Loader::new();
    let mut last = Instant::now();
    let mut wheel = 0.0f32;
    let mut wheel_modifiers = (false, false, false);
    let mut pointer = PointerState::default();
    let mut was_down = false;
    let mut was_right = false;
    let controls = panel::controls();
    let mut shot_n = 0u32;
    while win.is_open() && !ui.quit {
        if front_window.as_mut().is_some_and(|panel| !panel.poll(&mut ui)) {
            front_window = None;
            ui.desktop_panel = Some(false);
        }
        // mouse = finger 0; a drag that leaves the picture ends where it left
        let down = win.get_mouse_down(MouseButton::Left);
        let geometry = shell::Geometry { panel_open: false };
        let at = win.get_unscaled_mouse_pos(MouseMode::Pass).and_then(|(x, y)| geometry.pointer(win.get_size(), x, y));
        let right = win.get_mouse_down(MouseButton::Right);
        if !input.mouse_captured() {
            if down && !was_down { pointer.apply(&mut ui, input::Pointer::Down, at, false, &controls); }
            if down && was_down { pointer.apply(&mut ui, input::Pointer::Move, at, false, &controls); }
            if !down && was_down { pointer.apply(&mut ui, input::Pointer::Up, at, false, &controls); }
            if right && !was_right && !down { pointer.apply(&mut ui, input::Pointer::Context, at, false, &controls); }
        }
        was_down = down;
        was_right = right;
        for event in input.events() {
            match event {
                input::Event::Pointer { phase, x, y, size } => {
                    let point = geometry.pointer(size, x as f32, y as f32);
                    pointer.apply(&mut ui, phase, point, false, &controls);
                }
                input::Event::Wheel { x, y, size, delta, ctrl, shift, alt } => {
                    let modifiers = (ctrl, shift, alt);
                    if modifiers != wheel_modifiers { wheel = 0.0; wheel_modifiers = modifiers; }
                    wheel += delta as f32 / 120.0;
                    let steps = wheel.trunc() as i32;
                    wheel -= steps as f32;
                    if steps != 0 {
                        if let Some((x, y)) = geometry.pointer(size, x as f32, y as f32) {
                            ui.handle(Ev::Wheel { x, y, steps, ctrl, shift, alt });
                        }
                    }
                }
                input::Event::Key { key, ctrl, repeat } => {
                    if key == Key::O && ctrl && !repeat { ui.request_waveform(); }
                    else if key == Key::F6 && !repeat { ui.desktop_panel = Some(!ui.desktop_panel.unwrap_or(false)); }
                    else if key == Key::Escape && !repeat {
                        if loader.busy() { loader.cancel(); ui.toast("Waveform load canceled"); }
                        else { ui.dismiss_context(); }
                    }
                    else if !ctrl {
                        if key == Key::M && !repeat { ui.workflow_command("measure"); }
                        else if key == Key::F12 && !repeat {
                            let path = crate::store::next_name(&ui.store_dir, "shot", "png");
                            ui.shot_request = Some(path.to_string_lossy().into_owned());
                        } else if !repeat || matches!(key, Key::Up | Key::Down | Key::Left | Key::Right | Key::PageUp | Key::PageDown) {
                            if let Some(command) = key_cmd(key) { cmd::run(&mut ui, command); }
                        }
                    }
                }
            }
        }
        // Native Windows messages retain the event position and modifiers. Poll
        // only when subclass installation failed, so one notch is never applied twice.
        if !input.mouse_captured() {
            let shift = win.is_key_down(Key::LeftShift) || win.is_key_down(Key::RightShift);
            let ctrl = win.is_key_down(Key::LeftCtrl) || win.is_key_down(Key::RightCtrl);
            let alt = win.is_key_down(Key::LeftAlt) || win.is_key_down(Key::RightAlt);
            let modifiers = (ctrl, shift, alt);
            if modifiers != wheel_modifiers { wheel = 0.0; wheel_modifiers = modifiers; }
            if let Some((_, dy)) = win.get_scroll_wheel() { wheel += dy / NOTCH; }
            let steps = wheel.trunc() as i32;
            if steps != 0 {
                wheel -= steps as f32;
                if let Some((x, y)) = at { ui.handle(Ev::Wheel { x, y, steps, ctrl, shift, alt }); }
            }
        }
        services.poll(&mut ui, &cv);
        if let Some((slot, result)) = loader.poll(ui.waveform_epoch()) {
            match result {
                Ok((traces, name)) => if let Err(error) = ui.commit_waveform_traces_at(traces, &name, slot) { ui.toast(&error); },
                Err(error) => ui.toast(&error),
            }
        }
        if std::mem::take(&mut ui.open_wave_request) {
            open_waveform(&mut ui, &win, &mut loader);
            // The dialog pumps Windows messages independently; discard elapsed acquisition time and mouse state.
            last = Instant::now();
            pointer.apply(&mut ui, input::Pointer::Cancel, None, false, &controls);
            input.discard();
            was_down = false;
            was_right = false;
        }
        if ui.desktop_panel == Some(true) && front_window.is_none() {
            match front_window::FrontWindow::open(&win) {
                Ok(panel) => front_window = Some(panel),
                Err(error) => { ui.desktop_panel = Some(false); ui.toast(&error); }
            }
        } else if ui.desktop_panel != Some(true) { front_window = None; }
        if last.elapsed().as_secs_f64() < 1.0 / 30.0 {
            win.update();
            continue;
        }
        let dt = last.elapsed().as_secs_f64().min(0.2);
        last = Instant::now();
        ui.step(dt);
        ui.render(&mut cv);
        if let Some(p) = ui.shot_request.take() {
            crate::save_shot(&mut ui, &cv, p, &mut shot_n);
        }
        if let Err(e) = win.update_with_buffer(&cv.px, ui::W as usize, ui::H as usize) {
            eprintln!("oos-ui: window: {}", e);
            break;
        }
        if front_window.as_mut().is_some_and(|panel| !panel.draw(&ui)) {
            front_window = None;
            ui.desktop_panel = Some(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RunState, TrigMode};

    #[test]
    fn queued_short_panel_click_dispatches_once_and_canceled_or_crossed_gestures_do_not() {
        let mut ui = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        ui.m.run = RunState::Stop;
        let controls = panel::controls();
        let single = controls.iter().find(|control| control.command == "key single").unwrap().rect;
        let at = Some((single.x + single.w / 2, single.y + single.h / 2));
        let mut state = PointerState::default();
        // Both edges can arrive in one Windows message-pump iteration.
        for phase in [input::Pointer::Down, input::Pointer::Up] { state.apply(&mut ui, phase, at, true, &controls); }
        assert_eq!(ui.m.run, RunState::Run);
        assert_eq!(ui.m.trig_mode, TrigMode::Single);
        ui.m.run = RunState::Stop;
        state.apply(&mut ui, input::Pointer::Down, at, true, &controls);
        state.apply(&mut ui, input::Pointer::Cancel, None, true, &controls);
        state.apply(&mut ui, input::Pointer::Up, at, true, &controls);
        assert_eq!(ui.m.run, RunState::Stop, "focus/capture loss must not activate a control");
        state.apply(&mut ui, input::Pointer::Down, at, true, &controls);
        state.apply(&mut ui, input::Pointer::Move, Some((500, 300)), true, &controls);
        assert!(state.touch.is_none(), "a panel press cannot become a waveform drag");
        state.apply(&mut ui, input::Pointer::Up, Some((500, 300)), true, &controls);
        assert_eq!(ui.m.run, RunState::Stop);
    }
}
