// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The CRCs of the protocol decoders: one bit-serial engine, MSB first or reflected, and the parameter sets of the
//! protocols.

/// A CRC of `width` bits (at most 32). MSB first: the register shifts left and `poly` is in normal form (an x^width
/// term is ignored). Reflected: the register shifts right, bits enter LSB first, and `poly` and `init` are in
/// reflected form. The result is the register XOR `xorout`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crc {
    width: u32,
    poly: u32,
    init: u32,
    xorout: u32,
    reflected: bool,
}

impl Crc {
    pub const fn msb(width: u32, poly: u32, init: u32, xorout: u32) -> Crc {
        Crc { width, poly, init, xorout, reflected: false }
    }

    pub const fn reflected(width: u32, poly: u32, init: u32, xorout: u32) -> Crc {
        Crc { width, poly, init, xorout, reflected: true }
    }

    fn mask(self) -> u32 {
        u32::MAX >> (32 - self.width)
    }

    /// The CRC of a bit stream in transmission order.
    pub fn bits(self, bits: impl IntoIterator<Item = bool>) -> u32 {
        let (mask, poly) = (self.mask(), self.poly & self.mask());
        let step = |r: u32, b: bool| {
            if self.reflected {
                if (r & 1 == 1) != b {
                    r >> 1 ^ poly
                } else {
                    r >> 1
                }
            } else if (r >> (self.width - 1) & 1 == 1) != b {
                (r << 1 & mask) ^ poly
            } else {
                r << 1 & mask
            }
        };
        bits.into_iter().fold(self.init & mask, step) ^ self.xorout
    }

    /// The CRC of bytes, each sent MSB first (or LSB first when reflected).
    pub fn bytes(self, bytes: &[u8]) -> u32 {
        let shift = move |i: u8| if self.reflected { i } else { 7 - i };
        self.bits(bytes.iter().flat_map(move |&b| (0..8).map(move |i| b >> shift(i) & 1 == 1)))
    }
}

/// The SENT seed.
const SENT_SEED: u8 = 0b0101;
/// SENT CRC-4 (x^4 + x^3 + x^2 + 1) with the seed shifted through one zero nibble: the register then gives the
/// remainder of the message followed by a zero nibble (the recommended method of J2716 APR2010).
const SENT: Crc = Crc::msb(4, 0x1d, 0b0011, 0);

/// CRC-15/CAN (polynomial 0x4599) over a bit stream (SOF .. last data bit, unstuffed).
pub fn can_crc15(bits: &[bool]) -> u16 {
    Crc::msb(15, 0x4599, 0, 0).bits(bits.iter().copied()) as u16
}

/// CAN FD CRC (ISO 11898-1:2015): CRC-17 (0x3685B) for up to 16 data bytes, CRC-21 (0x302899) above, register
/// initialised with a 1 in the MSB; over SOF .. data field including the dynamic stuff bits, then the stuff count.
pub fn can_fd_crc(bits: &[bool], len: u32) -> u32 {
    let poly = if len == 21 { 0x30_2899 } else { 0x3_685b };
    Crc::msb(len, poly, 1 << (len - 1), 0).bits(bits.iter().copied())
}

/// CRC-16/MODBUS (Modbus over serial line V1.02, 6.2.2): init 0xFFFF, reflected polynomial 0xA001; sent low byte first.
pub fn modbus_crc(bytes: &[u8]) -> u16 {
    Crc::reflected(16, 0xa001, 0xffff, 0).bytes(bytes) as u16
}

/// Dallas/Maxim 1-Wire CRC-8 (x^8 + x^5 + x^4 + 1, reflected 0x8C, init 0), as in the ROM code and scratchpads.
pub fn ow_crc8(bytes: &[u8]) -> u8 {
    Crc::reflected(8, 0x8c, 0, 0).bytes(bytes) as u8
}

/// FlexRay header CRC-11 (FlexRay Protocol Specification V3.0.1, 4.5.2): polynomial 0x385, init 0x1A, over the
/// 20 bits sync frame indicator, startup frame indicator, frame ID, payload length (`bits20`, MSB first).
pub fn flexray_header_crc(bits20: u32) -> u32 {
    Crc::msb(11, 0x385, 0x1a, 0).bits((0..20).rev().map(|i| (bits20 >> i) & 1 == 1))
}

/// FlexRay frame CRC-24 (V3.0.1, 4.5.3): polynomial 0x5D6DCB, init 0xFEDCBA (channel A) / 0xABCDEF (channel B),
/// over header and payload bytes, MSB first.
pub fn flexray_frame_crc(bytes: &[u8], channel_b: bool) -> u32 {
    let init = if channel_b { 0xab_cdef } else { 0xfe_dcba };
    Crc::msb(24, 0x5d_6dcb, init, 0).bytes(bytes)
}

/// USB CRC5 of tokens and SOF: x^5 + x^2 + 1 over the bits in bus order (every field LSB first), preset to ones,
/// the remainder inverted. The result is the CRC field as read from the bus LSB first (bit 0 = the first CRC bit
/// sent, the x^4 coefficient), comparable to the received field.
pub fn usb_crc5(bits: &[bool]) -> u8 {
    Crc::reflected(5, 0x14, 0x1f, 0x1f).bits(bits.iter().copied()) as u8
}

/// USB CRC16 of data packets: x^16 + x^15 + x^2 + 1 over the payload bytes (LSB first), preset to ones, inverted;
/// sent low byte first.
pub fn usb_crc16(data: &[u8]) -> u16 {
    Crc::reflected(16, 0xa001, 0xffff, 0xffff).bytes(data) as u16
}

/// CRC-32 of USB PD (IEEE 802.3: reflected polynomial 0xEDB88320, init and final XOR 0xFFFFFFFF).
pub fn usbpd_crc32(data: &[u8]) -> u32 {
    Crc::reflected(32, 0xedb8_8320, 0xffff_ffff, 0xffff_ffff).bytes(data)
}

/// SENT CRC-4 (SAE J2716): polynomial x^4 + x^3 + x^2 + 1, seed 0101, over the data nibbles (not the status
/// nibble). The recommended method (J2716 APR2010) appends a zero nibble; `legacy` = without it (earlier issues):
/// the remainder of all but the last nibble with the last one added.
pub fn sent_crc(data: &[u8], legacy: bool) -> u8 {
    let nibbles = |d: &[u8]| SENT.bits(d.iter().flat_map(|&n| (0..4).rev().map(move |i| n >> i & 1 == 1))) as u8;
    match (legacy, data.split_last()) {
        (false, _) => nibbles(data),
        (true, Some((last, rest))) => nibbles(rest) ^ (last & 0xf),
        (true, None) => SENT_SEED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHECK: &[u8] = b"123456789";

    /// `bytes` as a bit stream, each byte MSB first or LSB first.
    fn stream(bytes: &[u8], msb_first: bool) -> Vec<bool> {
        let shift = |i: u8| if msb_first { 7 - i } else { i };
        bytes.iter().flat_map(|&b| (0..8).map(move |i| b >> shift(i) & 1 == 1)).collect()
    }

    /// The check values of the CRC catalogue (reveng) for "123456789".
    #[test]
    fn catalogue_check_values() {
        assert_eq!(can_crc15(&stream(CHECK, true)), 0x059e, "CRC-15/CAN");
        assert_eq!(modbus_crc(CHECK), 0x4b37, "CRC-16/MODBUS");
        assert_eq!(ow_crc8(CHECK), 0xa1, "CRC-8/MAXIM-DOW");
        assert_eq!(flexray_frame_crc(CHECK, false), 0x79_79bd, "CRC-24/FLEXRAY-A");
        assert_eq!(flexray_frame_crc(CHECK, true), 0x1f_23b8, "CRC-24/FLEXRAY-B");
        assert_eq!(usb_crc5(&stream(CHECK, false)), 0x19, "CRC-5/USB");
        assert_eq!(usb_crc16(CHECK), 0xb4c8, "CRC-16/USB");
        assert_eq!(usbpd_crc32(CHECK), 0xcbf4_3926, "CRC-32/ISO-HDLC");
        // the catalogue entries start from 0, CAN FD and the FlexRay header from their own init values
        assert_eq!(Crc::msb(17, 0x3_685b, 0, 0).bytes(CHECK), 0x0_4f03, "CRC-17/CAN-FD");
        assert_eq!(Crc::msb(21, 0x30_2899, 0, 0).bytes(CHECK), 0x0e_d841, "CRC-21/CAN-FD");
        assert_eq!(Crc::msb(11, 0x385, 0x1a, 0).bytes(CHECK), 0x5a3, "CRC-11/FLEXRAY");
    }

    #[test]
    fn sent_seed_through_a_zero_nibble() {
        assert_eq!(sent_crc(&[], false), SENT.init as u8);
        assert_eq!(sent_crc(&[], true), SENT_SEED);
        // legacy and recommended differ by the appended zero nibble
        assert_eq!(sent_crc(&[0x3, 0xa, 0x0], true), sent_crc(&[0x3, 0xa], false));
    }
}
