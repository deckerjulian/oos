// SPDX-License-Identifier: GPL-2.0-or-later
//! The emulated physical controls live outside the oscilloscope display.

use super::{dispatch, input, key_cmd, panel, PointerState};
use crate::{gfx::Canvas, ui::{self, Ui}};
use minifb::{Key, MouseMode, ScaleMode, Window, WindowOptions};

pub struct FrontWindow {
    // Unregister native input callbacks before destroying their window.
    input: input::Input,
    window: Window,
    pointer: PointerState,
    controls: Vec<panel::Control>,
    stage: Canvas,
    pixels: Vec<u32>,
}

/// Panel-local pixels are translated into the existing hardware-control layout,
/// never into the scope's touch surface. Letterbox margins have no hit targets.
fn control_point(size: (usize, usize), x: f32, y: f32) -> Option<(i32, i32)> {
    let (width, height) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let scale = (width / panel::WIDTH as f32).min(height / ui::H as f32);
    let (left, top) = ((width - panel::WIDTH as f32 * scale) * 0.5, (height - ui::H as f32 * scale) * 0.5);
    let (x, y) = ((x - left) / scale, (y - top) / scale);
    (x >= 0.0 && y >= 0.0 && x < panel::WIDTH as f32 && y < ui::H as f32)
        .then_some((ui::W + x as i32, y as i32))
}

impl FrontWindow {
    pub fn open(scope: &Window) -> Result<Self, String> {
        let options = WindowOptions { resize: true, scale_mode: ScaleMode::AspectRatioStretch, ..WindowOptions::default() };
        let mut window = Window::new("OOS Hardware Front Panel", panel::WIDTH as usize, ui::H as usize, options)
            .map_err(|error| format!("Cannot open hardware controls: {error}"))?;
        let (x, y) = scope.get_position();
        let (width, _) = scope.get_size();
        window.set_position(x + width as isize + 12, y);
        window.set_target_fps(0);
        let input = input::Input::attach(&mut window);
        Ok(Self { input, window, pointer: PointerState::default(), controls: panel::controls(),
            stage: Canvas::new(ui::W + panel::WIDTH, ui::H), pixels: vec![0; (panel::WIDTH * ui::H) as usize] })
    }

    pub fn poll(&mut self, ui: &mut Ui) -> bool {
        if !self.window.is_open() { return false; }
        self.window.update();
        for event in self.input.events() {
            match event {
                input::Event::Pointer { phase, x, y, size } => {
                    self.pointer.apply(ui, phase, control_point(size, x as f32, y as f32), true, &self.controls);
                }
                input::Event::Key { key: Key::F6, repeat: false, .. } => return false,
                input::Event::Key { key: Key::O, ctrl: true, repeat: false } => ui.request_waveform(),
                input::Event::Key { key, ctrl: false, repeat: false } => {
                    if let Some(command) = key_cmd(key) { dispatch(ui, command); }
                }
                _ => {}
            }
        }
        self.window.is_open()
    }

    pub fn draw(&mut self, ui: &Ui) -> bool {
        let hover = self.window.get_unscaled_mouse_pos(MouseMode::Pass)
            .and_then(|(x, y)| control_point(self.window.get_size(), x, y));
        panel::draw(&mut self.stage, ui, hover);
        for y in 0..ui::H as usize {
            let source = y * (ui::W + panel::WIDTH) as usize + ui::W as usize;
            let target = y * panel::WIDTH as usize;
            self.pixels[target..target + panel::WIDTH as usize]
                .copy_from_slice(&self.stage.px[source..source + panel::WIDTH as usize]);
        }
        self.window.update_with_buffer(&self.pixels, panel::WIDTH as usize, ui::H as usize).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detached_controls_map_to_the_same_commands_without_scope_pixels() {
        let control = panel::controls().into_iter().find(|control| control.command == "key single").unwrap();
        let (x, y) = (control.rect.x - ui::W + control.rect.w / 2, control.rect.y + control.rect.h / 2);
        assert_eq!(control_point((232, 600), x as f32, y as f32), Some((ui::W + x, y)));
        assert_eq!(control_point((464, 1200), x as f32 * 2.0, y as f32 * 2.0), Some((ui::W + x, y)));
        assert_eq!(control_point((232, 800), x as f32, y as f32 + 100.0), Some((ui::W + x, y)));
        assert_eq!(control_point((232, 800), 20.0, 99.0), None);
        assert_eq!(control_point((232, 600), 232.0, 100.0), None);
    }
}
