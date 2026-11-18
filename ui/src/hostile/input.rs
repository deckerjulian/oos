// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hostile input for the reader of the kernel's list of input devices, in which a USB or Bluetooth device names
//! itself.

use super::run;
use crate::hid;

/// `hid::devices` (/proc/bus/input/devices, with the names the devices give): returns for any text; a device it
/// finds is a mouse or a keyboard, its node is `eventN` and nothing else (oos-ui opens /dev/input/<node>), and it
/// finds no more devices than the text has blocks. `hid::decode` returns for any event.
#[test]
fn hostile_input_devices() {
    let list = "I: Bus=0005 Vendor=04e8 Product=7021 Version=0001\n\
                N: Name=\"BLE Keyboard\"\n\
                P: Phys=00:11:22:33:44:01\n\
                S: Sysfs=/devices/virtual/misc/uhid/0005:04E8:7021.0002/input/input4\n\
                U: Uniq=00:11:22:33:44:02\n\
                H: Handlers=kbd leds event2 \n\
                B: PROP=0\n\
                B: EV=120013\n\
                B: KEY=1000302000007 ff80187a480057ff febeffdfffefffff fffffffffffffffe\n\
                B: MSC=10\n\
                \n\
                I: Bus=0003 Vendor=046d Product=c077 Version=0111\n\
                N: Name=\"USB Optical Mouse\"\n\
                H: Handlers=mouse0 event3 \n\
                B: EV=17\n\
                B: KEY=70000 0 0 0 0\n\
                B: REL=903\n";
    let seeds: [&[u8]; 1] = [list.as_bytes()];
    run("hid::devices", &seeds, 1, |b| {
        let text = String::from_utf8_lossy(b);
        let found = hid::devices(&text);
        if found.len() > text.split("\n\n").count() {
            return Err(format!("{} devices in {} blocks", found.len(), text.split("\n\n").count()));
        }
        for d in &found {
            if !hid::is_event(&d.event) || !(d.mouse || d.keyboard) {
                return Err(format!("took {:?} as {}", d.event, d.name));
            }
        }
        for c in b.chunks(8) {
            let n = |k: usize| c.get(k).copied().unwrap_or(0);
            let _ = hid::decode(
                u16::from_le_bytes([n(0), n(1)]),
                u16::from_le_bytes([n(2), n(3)]),
                i32::from_le_bytes([n(4), n(5), n(6), n(7)]),
            );
        }
        Ok(())
    });
    // a name with a line break cannot bring a node of its own
    let forged =
        "N: Name=\"Keys\nH: Handlers=event/../../mem\"\nH: Handlers=kbd\nB: EV=120013\nB: KEY=ffffffffffffffff\n";
    assert!(hid::devices(forged).is_empty());
}
