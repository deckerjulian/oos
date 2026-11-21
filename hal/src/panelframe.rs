// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The frames of the front-panel MCU on its UART, `FA cmd data crc7 AF` (docs/protocols/frontpanel.md): the DHO800/DHO900
//! speaks them, and the MHO900 build listens for them on the same port (docs/mho.md). Board independent; what the
//! commands and LEDs mean is the board's (`dho::panel`).

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

/// What `take` found at the front of a buffer: a whole frame (cmd, data), or a byte that starts none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    Frame(u8, u8),
    Skipped(u8),
}

/// Takes the complete frames from the front of `b`; a byte that starts none is skipped. Fewer than five bytes stay for
/// the next read.
pub fn take(b: &mut Vec<u8>) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(f) = b.get(i..i + 5) {
        if f[0] == 0xfa && f[4] == 0xaf && f[3] == crc7(f[1], f[2]) {
            out.push(Piece::Frame(f[1], f[2]));
            i += 5;
        } else {
            out.push(Piece::Skipped(f[0]));
            i += 1;
        }
    }
    b.drain(..i);
    out
}

/// The key codes of `take`: the data byte of every frame (frontpanel.md).
pub fn keys(pieces: &[Piece]) -> Vec<u8> {
    pieces.iter().filter_map(|p| if let Piece::Frame(_, d) = p { Some(*d) } else { None }).collect()
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
    fn frames_are_taken_and_other_bytes_skipped() {
        let mut b = vec![0x00];
        b.extend(frame(0x01, 0x0c));
        b.extend([0xfa, 0x01, 0x0c, 0x00, 0xaf]); // wrong CRC
        b.extend(frame(0x02, 0x00));
        b.extend([0xfa, 0x01]);
        let p = take(&mut b);
        assert_eq!(p.first(), Some(&Piece::Skipped(0x00)));
        assert_eq!(p[1], Piece::Frame(0x01, 0x0c));
        assert_eq!(p.iter().filter(|x| matches!(x, Piece::Skipped(_))).count(), 6);
        assert_eq!(p.last(), Some(&Piece::Frame(0x02, 0x00)));
        assert_eq!(keys(&p), vec![0x0c, 0x00]);
        assert_eq!(b, vec![0xfa, 0x01]);
    }
}
