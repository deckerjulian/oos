// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Mice and keyboards on evdev, whatever brought them: USB, Bluetooth through `hidp` (classic) or `uhid` (LE, from
//! bluetoothd), docs/bluetooth.md. Which input devices they are (`devices`, from the text of /proc/bus/input/devices),
//! what one event means (`decode`), and which UI command a key stands for (`command`: a key for each front-panel
//! key).
//! Pure: `input.rs` opens the devices and reads the events, `ui/pointer.rs` acts on them.

pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;
const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const REL_HWHEEL: u16 = 0x06;
const REL_WHEEL: u16 = 0x08;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;

pub const KEY_ESC: u16 = 1;
pub const KEY_BACKSPACE: u16 = 14;
pub const KEY_ENTER: u16 = 28;
pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_RIGHTSHIFT: u16 = 54;
pub const KEY_LEFTALT: u16 = 56;
pub const KEY_KPENTER: u16 = 96;
pub const KEY_RIGHTCTRL: u16 = 97;
pub const KEY_RIGHTALT: u16 = 100;
pub const KEY_UP: u16 = 103;
pub const KEY_PAGEUP: u16 = 104;
pub const KEY_LEFT: u16 = 105;
pub const KEY_RIGHT: u16 = 106;
pub const KEY_DOWN: u16 = 108;
pub const KEY_PAGEDOWN: u16 = 109;
pub const KEY_F12: u16 = 88;

/// One mouse or keyboard: its event node (`event3`) and what it can do. A device can be both (a receiver with a
/// mouse and a keyboard behind it).
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub event: String,
    pub name: String,
    pub mouse: bool,
    pub keyboard: bool,
}

/// A capability bitmap of /proc/bus/input/devices (`B: KEY=...`): hex words of the kernel's `long`, the highest first.
fn bits(words: &str) -> Vec<u64> {
    let mut v: Vec<u64> = words.split_whitespace().filter_map(|w| u64::from_str_radix(w, 16).ok()).collect();
    v.reverse();
    v
}

/// `event` and its number, the name of an evdev node and nothing else.
pub fn is_event(w: &str) -> bool {
    w.strip_prefix("event").is_some_and(|n| !n.is_empty() && n.len() <= 4 && n.bytes().all(|c| c.is_ascii_digit()))
}

fn has(b: &[u64], n: usize) -> bool {
    b.get(n / 64).is_some_and(|w| w >> (n % 64) & 1 != 0)
}

/// What the input node `event` is, from what the kernel writes for it in sysfs (`/sys/class/input/<event>/device/`:
/// `name` and the capability bitmaps `capabilities/ev`, `rel`, `key`), not from /proc/bus/input/devices, where a
/// device's name stands in the same text and a Bluetooth name with a line break could bring lines of its own. A mouse
/// moves along x and y; a keyboard has the letters and the space bar (a device with media keys only, or the power
/// button, is none). None for neither, and for a node that is not `eventN`.
pub fn classify(event: &str, name: &str, ev: &str, rel: &str, key: &str) -> Option<Device> {
    if !is_event(event) {
        return None;
    }
    let (ev, rel, key) = (bits(ev), bits(rel), bits(key));
    let mouse = has(&ev, EV_REL as usize) && has(&rel, REL_X as usize) && has(&rel, REL_Y as usize);
    let keyboard = has(&ev, EV_KEY as usize) && [16usize, 30, 44, 57].iter().all(|&k| has(&key, k));
    let name = name.trim().chars().filter(|c| !c.is_control()).take(64).collect();
    (mouse || keyboard).then(|| Device { event: event.to_string(), name, mouse, keyboard })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// What one evdev event of a mouse or keyboard means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// the pointer moved by this many device units
    Move {
        dx: i32,
        dy: i32,
    },
    Button {
        button: Button,
        down: bool,
    },
    /// wheel notches: up or right is positive
    Wheel {
        steps: i32,
    },
    /// a key: `repeat` while it is held (the kernel's auto-repeat)
    Key {
        code: u16,
        down: bool,
        repeat: bool,
    },
}

/// One event of `struct input_event` (type, code, value) as the UI sees it; None for what it does not use (SYN, MSC,
/// the high-resolution wheel next to the notched one, LEDs).
pub fn decode(ty: u16, code: u16, value: i32) -> Option<Event> {
    match (ty, code) {
        (EV_REL, REL_X) => Some(Event::Move { dx: value, dy: 0 }),
        (EV_REL, REL_Y) => Some(Event::Move { dx: 0, dy: value }),
        (EV_REL, REL_WHEEL) | (EV_REL, REL_HWHEEL) => Some(Event::Wheel { steps: value }),
        (EV_KEY, BTN_LEFT) => Some(Event::Button { button: Button::Left, down: value != 0 }),
        (EV_KEY, BTN_RIGHT) => Some(Event::Button { button: Button::Right, down: value != 0 }),
        (EV_KEY, BTN_MIDDLE) => Some(Event::Button { button: Button::Middle, down: value != 0 }),
        (EV_KEY, c) if c < 0x100 => Some(Event::Key { code: c, down: value != 0, repeat: value == 2 }),
        _ => None,
    }
}

/// The UI command a key stands for (cmd.rs syntax).
pub fn command(code: u16) -> Option<&'static str> {
    Some(match code {
        57 => "key runstop",      // Space
        31 => "key single",       // S
        30 => "key auto",         // A
        46 => "key clear",        // C
        50 => "workflow measure", // M: the measurement page
        22 => "key cursor",       // U
        2 => "key ch1",
        3 => "key ch2",
        4 => "key ch3",
        5 => "key ch4",
        33 => "key math",         // F
        19 => "key ref",          // R
        38 => "key la",           // L
        34 => "key gi",           // G
        20 => "key tmenu",        // T
        35 => "key hmenu",        // H
        16 => "key quick",        // Q
        44 => "knob hscale push", // Z
        KEY_ENTER | KEY_KPENTER => "knob multi1 push",
        KEY_UP => "knob vpos +5",
        KEY_DOWN => "knob vpos -5",
        KEY_RIGHT => "knob hpos +5",
        KEY_LEFT => "knob hpos -5",
        KEY_PAGEUP => "knob tlevel +5",
        KEY_PAGEDOWN => "knob tlevel -5",
        _ => return None,
    })
}

/// Keys that act again while held (the knob turns on).
pub fn repeats(code: u16) -> bool {
    matches!(code, KEY_UP | KEY_DOWN | KEY_LEFT | KEY_RIGHT | KEY_PAGEUP | KEY_PAGEDOWN | KEY_BACKSPACE)
}

/// The character of a key on a US layout, with Shift or without; None for keys without one.
pub fn char_of(code: u16, shift: bool) -> Option<char> {
    const LOWER: &str = "\0\x1b1234567890-=\x08\tqwertyuiop[]\n\0asdfghjkl;'`\0\\zxcvbnm,./";
    const UPPER: &str = "\0\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\n\0ASDFGHJKL:\"~\0|ZXCVBNM<>?";
    if code == 57 {
        return Some(' ');
    }
    let c = (if shift { UPPER } else { LOWER }).chars().nth(code as usize)?;
    (c >= ' ').then_some(c)
}

/// What one device holds down: its mouse buttons and modifier keys. A device that goes while one is down (unplugged,
/// the Bluetooth link lost) sends no release, so the UI would keep a finger on the screen or Ctrl held; `release`
/// gives the events that let them go.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Held {
    buttons: Vec<Button>,
    keys: Vec<u16>,
}

fn hold<T: PartialEq>(v: &mut Vec<T>, x: T, down: bool) {
    let there = v.contains(&x);
    if down && !there {
        v.push(x);
    } else if !down && there {
        v.retain(|y| *y != x);
    }
}

impl Held {
    /// Notes an event of this device.
    pub fn track(&mut self, e: &Event) {
        match *e {
            Event::Button { button, down } => hold(&mut self.buttons, button, down),
            Event::Key { code, down, .. } if is_modifier(code) => hold(&mut self.keys, code, down),
            _ => {}
        }
    }

    /// The releases of what is held, for a device that went, except what `others` (the devices still there) hold:
    /// a second keyboard's Ctrl or a second mouse's button stays down.
    pub fn release(&self, others: &[&Held]) -> Vec<Event> {
        let held = |f: &dyn Fn(&Held) -> bool| others.iter().any(|o| f(o));
        let buttons = self.buttons.iter().filter(|b| !held(&|o| o.buttons.contains(b)));
        let keys = self.keys.iter().filter(|k| !held(&|o| o.keys.contains(k)));
        let buttons = buttons.map(|&button| Event::Button { button, down: false });
        buttons.chain(keys.map(|&code| Event::Key { code, down: false, repeat: false })).collect()
    }
}

/// Shift, Ctrl and Alt, left and right: the keys the UI keeps as held.
pub fn is_modifier(code: u16) -> bool {
    matches!(code, KEY_LEFTSHIFT | KEY_RIGHTSHIFT | KEY_LEFTCTRL | KEY_RIGHTCTRL | KEY_LEFTALT | KEY_RIGHTALT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As the kernel writes them on a 64-bit build: a BLE mouse through uhid, a USB keyboard, its consumer keys, the
    /// touch screen.
    #[test]
    fn mice_and_keyboards_are_found_others_left_out() {
        let mouse = classify("event2", "MX Anywhere 3 Mouse\n", "17", "1943", "1f0000 0 0 0 0").unwrap();
        assert_eq!(
            mouse,
            Device { event: "event2".into(), name: "MX Anywhere 3 Mouse".into(), mouse: true, keyboard: false }
        );
        let kb = "1000000000007 ff9f207ac14057ff febeffdfffefffff fffffffffffffffe";
        let k = classify("event3", "Dell KB216 Wired Keyboard", "120013", "0", kb).unwrap();
        assert!(k.keyboard && !k.mouse);
        let consumer =
            "3f000303ff 0 0 483ffff17aff32d bfd4444600000000 1 130c730b17c000 267bfad9415fed 9e168000004400 10000002";
        assert_eq!(classify("event4", "Consumer Control", "1f", "1040", consumer), None);
        assert_eq!(classify("event0", "edt-ft5x06", "b", "0", "400 0 0 0 0 0"), None, "the touch screen");
        assert_eq!(classify("event/../mem", "x", "17", "1943", "0"), None);
    }

    #[test]
    fn events_decode() {
        assert_eq!(decode(EV_REL, REL_X, -3), Some(Event::Move { dx: -3, dy: 0 }));
        assert_eq!(decode(EV_REL, REL_Y, 5), Some(Event::Move { dx: 0, dy: 5 }));
        assert_eq!(decode(EV_REL, REL_WHEEL, -1), Some(Event::Wheel { steps: -1 }));
        assert_eq!(decode(EV_REL, 0x0b, 120), None, "the high-resolution wheel is left out");
        assert_eq!(decode(EV_KEY, BTN_LEFT, 1), Some(Event::Button { button: Button::Left, down: true }));
        assert_eq!(decode(EV_KEY, BTN_RIGHT, 0), Some(Event::Button { button: Button::Right, down: false }));
        assert_eq!(decode(EV_KEY, 30, 2), Some(Event::Key { code: 30, down: true, repeat: true }));
        assert_eq!(decode(0, 0, 0), None);
    }

    #[test]
    fn keys_map_to_commands_and_characters() {
        assert_eq!(command(57), Some("key runstop"));
        assert_eq!(command(3), Some("key ch2"));
        assert_eq!(command(KEY_UP), Some("knob vpos +5"));
        assert_eq!(command(KEY_F12), None);
        assert!(repeats(KEY_PAGEDOWN) && !repeats(57));
        assert_eq!(char_of(30, false), Some('a'));
        assert_eq!(char_of(30, true), Some('A'));
        assert_eq!(char_of(2, true), Some('!'));
        assert_eq!(char_of(57, false), Some(' '));
        assert_eq!(char_of(53, true), Some('?'));
        assert_eq!(char_of(KEY_ENTER, false), None);
        assert_eq!(char_of(KEY_BACKSPACE, false), None);
        assert_eq!(char_of(200, false), None);
    }

    #[test]
    fn what_a_device_holds_is_released_when_it_goes() {
        let mut h = Held::default();
        for e in [
            Event::Button { button: Button::Left, down: true },
            Event::Key { code: KEY_LEFTCTRL, down: true, repeat: false },
            Event::Key { code: KEY_LEFTCTRL, down: true, repeat: true },
            Event::Key { code: 30, down: true, repeat: false },
            Event::Key { code: KEY_RIGHTSHIFT, down: true, repeat: false },
            Event::Key { code: KEY_RIGHTSHIFT, down: false, repeat: false },
            Event::Button { button: Button::Right, down: true },
            Event::Button { button: Button::Right, down: false },
        ] {
            h.track(&e);
        }
        let left_up = Event::Button { button: Button::Left, down: false };
        assert_eq!(h.release(&[]), vec![left_up, Event::Key { code: KEY_LEFTCTRL, down: false, repeat: false }]);
        assert!(Held::default().release(&[]).is_empty());
        // a second keyboard still holds Ctrl: only the button is let go (review of #214)
        let mut other = Held::default();
        other.track(&Event::Key { code: KEY_LEFTCTRL, down: true, repeat: false });
        assert_eq!(h.release(&[&other]), vec![left_up]);
    }
}
