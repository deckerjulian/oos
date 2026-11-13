// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The scope screen in a desktop window (Windows host build), with the demo signal or a replayed frame.
//!
//! The mouse is the finger: a left click is a tap, a left drag is a touch drag. The wheel turns the knobs and the
//! keyboard stands in for the front panel:
//!
//!   wheel          H scale; Multi 1 while a menu is open    Enter        Multi 1 push
//!   Shift+wheel    V scale                                  Up / Down    V position
//!   Ctrl+wheel     H scale                                  Left / Right H position
//!   Alt+wheel      trigger level                            PgUp / PgDn  trigger level
//!   Space  Run/Stop     S  Single     A  Auto     C  Clear     M  Measure    U  Cursor
//!   1-4    CH1-CH4      F  Math       R  Ref      L  LA        G  Generator  T  Trigger menu
//!   H      Horizontal menu            Q  Quick    Z  zoom (H scale push)     F12  screenshot (PNG)
//!
//! The window keeps the screen's 1024 x 600 shape when it is resized; the bars around it take no input.

use crate::cmd;
use crate::gfx::Canvas;
use crate::ui::{self, Ev, Phase, Ui};
use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Scale, ScaleMode, Window, WindowOptions};
use std::time::Instant;

/// One wheel notch as minifb reports it on Windows (WHEEL_DELTA 120 x 0.1).
const NOTCH: f32 = 12.0;

/// Window pixel -> screen pixel, through the centred, aspect-kept stretch; None on the bars beside the picture.
fn to_screen(win: &Window, x: f32, y: f32) -> Option<(i32, i32)> {
    let (ww, wh) = win.get_size();
    let (ww, wh) = (ww.max(1) as f32, wh.max(1) as f32);
    let s = (ww / ui::W as f32).min(wh / ui::H as f32);
    let (ox, oy) = ((ww - ui::W as f32 * s) / 2.0, (wh - ui::H as f32 * s) / 2.0);
    let (sx, sy) = ((x - ox) / s, (y - oy) / s);
    let inside = sx >= 0.0 && sy >= 0.0 && sx < ui::W as f32 && sy < ui::H as f32;
    inside.then_some((sx as i32, sy as i32))
}

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

pub fn run(mut ui: Ui) {
    let opts = WindowOptions { resize: true, scale: Scale::X1, scale_mode: ScaleMode::AspectRatioStretch, ..WindowOptions::default() };
    let mut win = match Window::new("OOS", ui::W as usize, ui::H as usize, opts) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("oos-ui: window: {}", e);
            std::process::exit(1);
        }
    };
    // the loop paces itself at 30 frames/s, as on the scope
    win.set_target_fps(30);
    let mut cv = Canvas::new(ui::W, ui::H);
    let mut last = Instant::now();
    let mut wheel = 0.0f32;
    let mut touch: Option<(i32, i32)> = None;
    let mut shot_n = 0u32;
    while win.is_open() && !ui.quit {
        // mouse = finger 0; a drag that leaves the picture ends where it left
        let down = win.get_mouse_down(MouseButton::Left);
        let at = win.get_unscaled_mouse_pos(MouseMode::Pass).and_then(|(x, y)| to_screen(&win, x, y));
        match (down, touch, at) {
            (true, None, Some((x, y))) => {
                ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
                touch = Some((x, y));
            }
            (true, Some(p), Some((x, y))) if p != (x, y) => {
                ui.handle(Ev::Touch { id: 0, phase: Phase::Move, x, y });
                touch = Some((x, y));
            }
            (false, Some((x, y)), _) => {
                ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
                touch = None;
            }
            _ => {}
        }
        // wheel = a knob; the modifier picks which one
        if let Some((_, dy)) = win.get_scroll_wheel() {
            wheel += dy / NOTCH;
        }
        let steps = wheel.trunc() as i32;
        if steps != 0 {
            wheel -= steps as f32;
            let shift = win.is_key_down(Key::LeftShift) || win.is_key_down(Key::RightShift);
            let ctrl = win.is_key_down(Key::LeftCtrl) || win.is_key_down(Key::RightCtrl);
            let alt = win.is_key_down(Key::LeftAlt) || win.is_key_down(Key::RightAlt);
            let (knob, n) = if shift {
                ("vscale", steps)
            } else if ctrl {
                ("hscale", steps)
            } else if alt {
                ("tlevel", steps * 5)
            } else if ui.menu_is_open() {
                ("multi1", steps)
            } else {
                // Multi 1 would open a menu here; the bare wheel zooms the timebase instead (up = zoom in)
                ("hscale", steps)
            };
            cmd::run(&mut ui, &format!("knob {} {:+}", knob, n));
        }
        for k in win.get_keys_pressed(KeyRepeat::Yes) {
            if k == Key::F12 {
                let p = crate::store::next_name(&ui.store_dir, "shot", "png");
                ui.shot_request = Some(p.to_string_lossy().into_owned());
            } else if let Some(c) = key_cmd(k) {
                cmd::run(&mut ui, c);
            }
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
    }
}
