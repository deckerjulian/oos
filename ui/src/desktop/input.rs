// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Queue edges from Windows messages instead of sampling the final button state.
//! A complete click or Ctrl+O chord between two frames must still arrive intact.

use minifb::{InputCallback, Key, Window};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};

// Use the system import libraries already bundled with the Windows toolchain.
// These few declarations avoid requiring an external assembler just to rebuild
// additional windows-sys raw-dylib binding modules on the GNU host.
type SubclassProc = Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM, usize, usize) -> LRESULT>;
#[link(name = "comctl32")]
extern "system" {
    fn SetWindowSubclass(hwnd: HWND, procedure: SubclassProc, id: usize, data: usize) -> i32;
    fn RemoveWindowSubclass(hwnd: HWND, procedure: SubclassProc, id: usize) -> i32;
    fn DefSubclassProc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT;
}
#[link(name = "user32")]
extern "system" {
    fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> i32;
    fn SetCapture(hwnd: HWND) -> HWND;
    fn ReleaseCapture() -> i32;
}

const WM_KILLFOCUS: u32 = 0x0008;
const WM_CANCELMODE: u32 = 0x001f;
const WM_NCDESTROY: u32 = 0x0082;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONDOWN: u32 = 0x0204;
const WM_CAPTURECHANGED: u32 = 0x0215;

const SUBCLASS_ID: usize = 0x4f4f_5349;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pointer {
    Down,
    Move,
    Up,
    Context,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Pointer { phase: Pointer, x: i32, y: i32, size: (usize, usize) },
    Key { key: Key, ctrl: bool, repeat: bool },
}

struct Keys {
    left_ctrl: bool,
    right_ctrl: bool,
    down: [bool; 512],
}

impl Default for Keys {
    fn default() -> Self { Self { left_ctrl: false, right_ctrl: false, down: [false; 512] } }
}

impl Keys {
    fn update(&mut self, key: Key, state: bool) -> Option<Event> {
        match key {
            Key::LeftCtrl => self.left_ctrl = state,
            Key::RightCtrl => self.right_ctrl = state,
            _ => {}
        }
        let repeat = self.down.get(key as usize).copied().unwrap_or(false);
        if let Some(down) = self.down.get_mut(key as usize) { *down = state; }
        state.then_some(Event::Key { key, ctrl: self.left_ctrl || self.right_ctrl, repeat })
    }
}

struct Keyboard {
    keys: Rc<RefCell<Keys>>,
    send: Sender<Event>,
}

impl InputCallback for Keyboard {
    fn add_char(&mut self, _: u32) {}
    fn set_key_state(&mut self, key: Key, state: bool) {
        let event = self.keys.borrow_mut().update(key, state);
        if let Some(event) = event { let _ = self.send.send(event); }
    }
}

struct Capture {
    send: Sender<Event>,
    keys: Rc<RefCell<Keys>>,
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM, _: usize, data: usize) -> LRESULT {
    let capture = &*(data as *const Capture);
    let phase = match message {
        WM_LBUTTONDOWN => { SetCapture(hwnd); Some(Pointer::Down) }
        WM_MOUSEMOVE if w & 1 != 0 => Some(Pointer::Move),
        WM_LBUTTONUP => Some(Pointer::Up),
        WM_RBUTTONDOWN => Some(Pointer::Context),
        WM_KILLFOCUS | WM_CANCELMODE | WM_CAPTURECHANGED => {
            if message == WM_KILLFOCUS { *capture.keys.borrow_mut() = Keys::default(); }
            Some(Pointer::Cancel)
        }
        WM_NCDESTROY => {
            RemoveWindowSubclass(hwnd, Some(procedure), SUBCLASS_ID);
            None
        }
        _ => None,
    };
    if let Some(phase) = phase {
        let mut rect: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rect);
        let x = (l as u16) as i16 as i32;
        let y = ((l as usize >> 16) as u16) as i16 as i32;
        let size = ((rect.right - rect.left).max(1) as usize, (rect.bottom - rect.top).max(1) as usize);
        let _ = capture.send.send(Event::Pointer { phase, x, y, size });
    }
    // Releasing capture synchronously sends WM_CAPTURECHANGED. Queue Up first
    // so that cancellation cannot consume a quick click before its release.
    if message == WM_LBUTTONUP { ReleaseCapture(); }
    DefSubclassProc(hwnd, message, w, l)
}

pub struct Input {
    hwnd: HWND,
    capture: Box<Capture>,
    received: Receiver<Event>,
    mouse_captured: bool,
}

impl Input {
    pub fn attach(window: &mut Window) -> Self {
        let (send, received) = mpsc::channel();
        let keys = Rc::new(RefCell::new(Keys::default()));
        window.set_input_callback(Box::new(Keyboard { keys: keys.clone(), send: send.clone() }));
        let mut capture = Box::new(Capture { send, keys });
        let hwnd = window.get_window_handle();
        // Same window thread for installation, callbacks, and removal. The Box
        // remains stable until Drop unregisters the callback before freeing it.
        let mouse_captured = unsafe { SetWindowSubclass(hwnd, Some(procedure), SUBCLASS_ID, (&mut *capture as *mut Capture) as usize) != 0 };
        Self { hwnd, capture, received, mouse_captured }
    }

    pub fn mouse_captured(&self) -> bool { self.mouse_captured }
    pub fn events(&self) -> Vec<Event> { self.received.try_iter().collect() }
    pub fn discard(&self) {
        for _ in self.received.try_iter() {}
        *self.capture.keys.borrow_mut() = Keys::default();
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        if self.mouse_captured { unsafe { RemoveWindowSubclass(self.hwnd, Some(procedure), SUBCLASS_ID); } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_complete_short_ctrl_o_chord_keeps_its_modifier_at_the_press() {
        let mut keys = Keys::default();
        keys.update(Key::LeftCtrl, true);
        let open = keys.update(Key::O, true);
        keys.update(Key::O, false);
        keys.update(Key::LeftCtrl, false);
        assert_eq!(open, Some(Event::Key { key: Key::O, ctrl: true, repeat: false }));
        assert!(!keys.left_ctrl);
    }

    #[test]
    fn focus_reset_does_not_leave_a_stuck_modifier_or_repeat_after_a_file_dialog() {
        let mut keys = Keys::default();
        keys.update(Key::LeftCtrl, true);
        keys.update(Key::O, true);
        keys = Keys::default();
        assert_eq!(keys.update(Key::O, true), Some(Event::Key { key: Key::O, ctrl: false, repeat: false }));
    }
}
