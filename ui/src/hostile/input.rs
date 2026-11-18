// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hostile input for what oos-ui reads about a mouse or keyboard: the name a USB or Bluetooth device gives itself and
//! the capability bitmaps of its node in sysfs, and the events it sends.

use super::run;
use crate::hid;

/// `hid::classify` (a node's name, which the device chooses, and its capability bitmaps) and `hid::decode`: return
/// for any bytes; a device found is a mouse or a keyboard, its node is `eventN` and nothing else (oos-ui opens
/// /dev/input/<node>), and its name has no control character and at most 64 characters.
#[test]
fn hostile_input_devices() {
    // fields divided by a line of `|`: node, name, ev, rel, key
    let keyboard =
        "event2\n|\nBLE Keyboard\n|\n120013\n|\n0\n|\n1000302000007 ff80187a480057ff febeffdfffefffff fffffffffffffffe";
    let mouse = "event3\n|\nUSB Optical Mouse\n|\n17\n|\n903\n|\n70000 0 0 0 0";
    let seeds: [&[u8]; 2] = [keyboard.as_bytes(), mouse.as_bytes()];
    run("hid::classify", &seeds, 1, |b| {
        let text = String::from_utf8_lossy(b);
        let f: Vec<&str> = text.split("\n|\n").collect();
        let at = |k: usize| f.get(k).copied().unwrap_or("");
        if let Some(d) = hid::classify(at(0), at(1), at(2), at(3), at(4)) {
            if !hid::is_event(&d.event) || !(d.mouse || d.keyboard) {
                return Err(format!("took {:?} as {:?}", d.event, d.name));
            }
            if d.name.chars().count() > 64 || d.name.chars().any(char::is_control) {
                return Err(format!("a name for the log of {:?}", d.name));
            }
        }
        for c in b.chunks(8) {
            let n = |k: usize| c.get(k).copied().unwrap_or(0);
            let (ty, code) = (u16::from_le_bytes([n(0), n(1)]), u16::from_le_bytes([n(2), n(3)]));
            let _ = hid::decode(ty, code, i32::from_le_bytes([n(4), n(5), n(6), n(7)]));
        }
        Ok(())
    });
}
