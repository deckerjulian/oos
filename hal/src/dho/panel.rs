// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Front-panel MCU protocol (UART0, frames `FA cmd data crc7 AF`, see docs/protocols/frontpanel.md), platform independent
//! so the host tests cover it.

/// CRC-7/MMC over [cmd, data] (poly 0x09, init 0), 7-bit result.
pub fn crc7(cmd: u8, data: u8) -> u8 {
    let mut c: u8 = 0;
    for b in [cmd, data] {
        c ^= b;
        for _ in 0..8 {
            c = if c & 0x80 != 0 { (c << 1) ^ 0x12 } else { c << 1 };
        }
    }
    c >> 1
}

/// One MCU frame `FA cmd data crc7 AF`.
pub fn frame(cmd: u8, data: u8) -> [u8; 5] {
    [0xfa, cmd, data, crc7(cmd, data), 0xaf]
}

/// Key code of the power key, see docs/protocols/board.md section 4.
pub const KEY_POWER: u8 = 0x1b;

/// Power commands (docs/protocols/board.md section 4).
/// "App UI loaded", sent once when the UI is up.
pub const UI_LOADED: [u8; 5] = [0xfa, 0x05, 0x00, 0x27, 0xaf];
/// Power off (stock sends it twice back to back).
pub const POWER_OFF: [u8; 5] = [0xfa, 0x05, 0x01, 0x2e, 0xaf];
/// Cancel the MCU's power-down; the stock system sends it on every power-key press.
pub const CANCEL_POWER_DOWN: [u8; 5] = [0xfa, 0x05, 0x02, 0x35, 0xaf];
/// Restart.
pub const RESTART: [u8; 5] = [0xfa, 0x05, 0x03, 0x3c, 0xaf];
/// Ask for the MCU firmware version; the reply's data byte >> 4 is the major version.
pub const KEYBOARD_VERSION: [u8; 5] = [0xfa, 0x06, 0xbb, 0x03, 0xaf];

/// Power-on mode: data 5 when `flag` is set, else 4. Which one
/// means "power on when mains is applied" is not tested yet.
pub fn power_on_mode(flag: bool) -> [u8; 5] {
    frame(0xff, if flag { 5 } else { 4 })
}

/// Power-LED flash time: data = seconds * 10, 0..25.5 s. Effect not tested.
pub fn flash_time(seconds: f32) -> [u8; 5] {
    frame(0x04, (seconds * 10.0).round().clamp(0.0, 255.0) as u8)
}

/// Hardware LED bit of each panel LED index (from the LED id: bit = 8 * (id >> 8) + log2(id & 0xff) - 8).
/// G2 (index 10) has no valid id on the DHO900.
pub const LED_HW_BIT: [Option<u8>; 19] = [
    Some(10), Some(11), Some(12), Some(13), // CH1-CH4
    Some(16), Some(17), Some(15), Some(14), // RUN, STOP, SINGLE red, SINGLE orange
    Some(1), Some(2), None, Some(0),        // LA, G1, G2, TOUCH
    Some(7), Some(5), Some(9), Some(8),     // A up/down, B left/right, SLOPE1, SLOPE2
    Some(6), Some(3), Some(4),              // A left/right, B up/down, INTENSITY
];

/// The three LED frames (cmd 1/2/3 = hardware LEDs 0-7, 8-15, 16-18) for a mask in panel LED order (see `Ui::leds`).
pub fn led_frames(mask: u32) -> [[u8; 5]; 3] {
    let mut hw = 0u32;
    for (i, b) in LED_HW_BIT.iter().enumerate() {
        if let (true, Some(b)) = (mask & (1 << i) != 0, b) {
            hw |= 1 << b;
        }
    }
    let mut out = [[0u8; 5]; 3];
    for (k, f) in out.iter_mut().enumerate() {
        let cmd = k as u8 + 1;
        let data = ((hw >> (8 * k)) & 0xff) as u8;
        *f = frame(cmd, data);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc7_matches_frontpanel_py() {
        // tools/frontpanel.py selftest
        assert_eq!(crc7(0x01, 0x00), 0x0b);
        assert_eq!(crc7(0x02, 0xff), 0x6f);
        assert_eq!(crc7(0x06, 0x15), 0x0e);
    }

    #[test]
    fn power_frames_match_board_md() {
        // frames as listed in board.md section 4
        for f in [UI_LOADED, POWER_OFF, CANCEL_POWER_DOWN, RESTART, KEYBOARD_VERSION] {
            assert_eq!(frame(f[1], f[2]), f);
        }
        assert_eq!(power_on_mode(true), [0xfa, 0xff, 0x05, 0x31, 0xaf]);
        assert_eq!(power_on_mode(false), [0xfa, 0xff, 0x04, 0x38, 0xaf]);
        assert_eq!(flash_time(1.5)[2], 15);
        assert_eq!(flash_time(99.0)[2], 255);
    }

    #[test]
    fn leds_map_to_hardware_bits() {
        // CH1-CH4 + RUN (LEDs 0-4) -> hardware bits 10-13 and 16, confirmed by eye on 2026-11-03
        let f = led_frames(0b1_1111);
        assert_eq!(f[0][2], 0x00);
        assert_eq!(f[1][2], 0x3c);
        assert_eq!(f[2][2], 0x01);
        // INTENSITY (LED 18) -> hardware bit 4, TOUCH (11) -> bit 0
        assert_eq!(led_frames(1 << 18)[0][2], 0x10);
        assert_eq!(led_frames(1 << 11)[0][2], 0x01);
        // G2 (10) has no LED on the DHO900
        assert_eq!(led_frames(1 << 10), led_frames(0));
    }
}
