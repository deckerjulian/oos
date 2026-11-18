// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! USB Power Delivery (BMC on CC).
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::{edge_times, usbpd_crc32, Kind, Packet};
use crate::protocols::ModuleInfo;

/// 4b5b symbols of USB PD (r3.1 table 5-1) as 5-bit numbers, first transmitted bit = LSB.
pub const PD_4B5B: [u8; 16] =
    [0x1e, 0x09, 0x14, 0x15, 0x0a, 0x0b, 0x0e, 0x0f, 0x12, 0x13, 0x16, 0x17, 0x1a, 0x1b, 0x1c, 0x1d];

pub const PD_SYNC1: u8 = 0x18;

pub const PD_SYNC2: u8 = 0x11;

pub const PD_SYNC3: u8 = 0x06;

pub const PD_RST1: u8 = 0x07;

pub const PD_RST2: u8 = 0x19;

pub const PD_EOP: u8 = 0x0d;

/// Ordered sets (start of packet / resets): label, K-codes.
pub(super) const PD_SETS: [(&str, [u8; 4]); 7] = [
    ("SOP", [PD_SYNC1, PD_SYNC1, PD_SYNC1, PD_SYNC2]),
    ("SOP'", [PD_SYNC1, PD_SYNC1, PD_SYNC3, PD_SYNC3]),
    ("SOP''", [PD_SYNC1, PD_SYNC3, PD_SYNC1, PD_SYNC3]),
    ("SOP'D", [PD_SYNC1, PD_RST2, PD_RST2, PD_SYNC3]),
    ("SOP''D", [PD_SYNC1, PD_RST2, PD_SYNC3, PD_SYNC2]),
    ("!HRST", [PD_RST1, PD_RST1, PD_RST1, PD_RST2]),
    ("!CRST", [PD_RST1, PD_SYNC1, PD_RST1, PD_SYNC3]),
];

/// Message type name from a USB PD message header (r3.1 tables 6-5 control, 6-6 data, 6-53 extended).
pub fn usbpd_msg_name(hdr: u16) -> &'static str {
    let t = hdr & 0x1f;
    let ndo = hdr >> 12 & 7;
    if hdr & 0x8000 != 0 {
        return match t {
            1 => "Source_Capabilities_Extended",
            2 => "Status",
            3 => "Get_Battery_Cap",
            4 => "Get_Battery_Status",
            5 => "Battery_Capabilities",
            6 => "Get_Manufacturer_Info",
            7 => "Manufacturer_Info",
            8 => "Security_Request",
            9 => "Security_Response",
            10 => "Firmware_Update_Request",
            11 => "Firmware_Update_Response",
            12 => "PPS_Status",
            13 => "Country_Info",
            14 => "Country_Codes",
            15 => "Sink_Capabilities_Extended",
            16 => "Extended_Control",
            17 => "EPR_Source_Capabilities",
            18 => "EPR_Sink_Capabilities",
            30 => "Vendor_Defined_Extended",
            _ => "Reserved",
        };
    }
    if ndo == 0 {
        match t {
            1 => "GoodCRC",
            2 => "GotoMin",
            3 => "Accept",
            4 => "Reject",
            5 => "Ping",
            6 => "PS_RDY",
            7 => "Get_Source_Cap",
            8 => "Get_Sink_Cap",
            9 => "DR_Swap",
            10 => "PR_Swap",
            11 => "VCONN_Swap",
            12 => "Wait",
            13 => "Soft_Reset",
            14 => "Data_Reset",
            15 => "Data_Reset_Complete",
            16 => "Not_Supported",
            17 => "Get_Source_Cap_Extended",
            18 => "Get_Status",
            19 => "FR_Swap",
            20 => "Get_PPS_Status",
            21 => "Get_Country_Codes",
            22 => "Get_Sink_Cap_Extended",
            23 => "Get_Source_Info",
            24 => "Get_Revision",
            _ => "Reserved",
        }
    } else {
        match t {
            1 => "Source_Capabilities",
            2 => "Request",
            3 => "BIST",
            4 => "Sink_Capabilities",
            5 => "Battery_Status",
            6 => "Alert",
            7 => "Get_Country_Info",
            8 => "Enter_USB",
            9 => "EPR_Request",
            10 => "EPR_Mode",
            11 => "Source_Info",
            12 => "Revision",
            15 => "Vendor_Defined",
            _ => "Reserved",
        }
    }
}

/// A decoded power data object (USB PD r3.1 section 6.4.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pdo {
    Fixed {
        volts: f64,
        amps: f64,
    },
    Battery {
        min_v: f64,
        max_v: f64,
        watts: f64,
    },
    Variable {
        min_v: f64,
        max_v: f64,
        amps: f64,
    },
    /// SPR programmable power supply (augmented PDO)
    Pps {
        min_v: f64,
        max_v: f64,
        amps: f64,
    },
    Other(u32),
}

/// Decodes a source/sink capability PDO.
pub fn usbpd_pdo(pdo: u32) -> Pdo {
    let f = |sh: u32, bits: u32| (pdo >> sh & ((1 << bits) - 1)) as f64;
    match pdo >> 30 {
        0 => Pdo::Fixed { volts: f(10, 10) * 0.05, amps: f(0, 10) * 0.01 },
        1 => Pdo::Battery { min_v: f(10, 10) * 0.05, max_v: f(20, 10) * 0.05, watts: f(0, 10) * 0.25 },
        2 => Pdo::Variable { min_v: f(10, 10) * 0.05, max_v: f(20, 10) * 0.05, amps: f(0, 10) * 0.01 },
        _ if pdo >> 28 & 3 == 0 => Pdo::Pps { min_v: f(8, 8) * 0.1, max_v: f(17, 8) * 0.1, amps: f(0, 7) * 0.05 },
        _ => Pdo::Other(pdo),
    }
}

/// One BMC burst decoded to bits (t0, t1, bit).
fn bmc_bits(e: &[f64], start: usize, bit: f64) -> (Vec<(f64, f64, bool)>, usize) {
    let mut bits = Vec::new();
    let mut tb = bit;
    let mut j = start;
    loop {
        let Some(&t1) = e.get(j + 1) else {
            // last edge of the capture: start of a final 0 bit
            if j < e.len() && !bits.is_empty() {
                bits.push((e[j], e[j] + tb, false));
            }
            j += 1;
            break;
        };
        let d = t1 - e[j];
        if d > 1.6 * tb {
            // idle after the burst: the last edge starts a final 0 bit (EOP ends with 0, then the line holds)
            if !bits.is_empty() {
                bits.push((e[j], e[j] + tb, false));
            }
            j += 1;
            break;
        } else if d > 0.75 * tb {
            bits.push((e[j], t1, false));
            tb = 0.9 * tb + 0.1 * d;
            j += 1;
        } else if d > 0.25 * tb {
            match e.get(j + 2) {
                Some(&t2) if t2 - t1 > 0.25 * tb && t2 - t1 <= 0.75 * tb => {
                    bits.push((e[j], t2, true));
                    tb = 0.9 * tb + 0.1 * (t2 - e[j]);
                    j += 2;
                }
                Some(&t2) if t2 - t1 > 1.6 * tb => {
                    // a final 1 whose cell ends low without another edge (e.g. RST-2 at the end of Hard Reset)
                    bits.push((e[j], 2.0 * t1 - e[j], true));
                    j += 2;
                    break;
                }
                None => {
                    bits.push((e[j], 2.0 * t1 - e[j], true));
                    j += 2;
                    break;
                }
                _ => {
                    j += 1;
                    break;
                }
            }
        } else {
            j += 1;
            break;
        }
    }
    (bits, j)
}

/// One burst's bits -> packets.
fn pd_frame(b: &[(f64, f64, bool)], out: &mut Vec<Packet>) {
    let n = b.len();
    let Some(brk) = (0..n.saturating_sub(1)).find(|&k| b[k].2 == b[k + 1].2) else { return };
    let sym = |s: usize| -> Option<u8> { (s + 5 <= n).then(|| (0..5).fold(0u8, |v, k| v | (b[s + k].2 as u8) << k)) };
    let nib = |s: usize| -> Option<u32> { sym(s).and_then(|x| PD_4B5B.iter().position(|&c| c == x)).map(|p| p as u32) };
    // the preamble (alternating, ends with 1) runs into the first K-code: try both possible boundaries
    let mut best: Option<(usize, usize, u32)> = None; // (start, set, matching K-codes)
    for s in [brk, brk + 1] {
        if s < 8 {
            continue;
        }
        let Some(k) = (0..4).map(|q| sym(s + 5 * q)).collect::<Option<Vec<u8>>>() else { continue };
        for (si, set) in PD_SETS.iter().enumerate() {
            let m = (0..4).filter(|&q| k[q] == set.1[q]).count() as u32;
            if m >= 3 && best.is_none_or(|x| m > x.2) {
                best = Some((s, si, m));
            }
        }
    }
    let Some((s, si, m)) = best else { return };
    out.push(Packet::field("PRE", b[0].0, b[s - 1].1, s as u32, true));
    let (label, _) = PD_SETS[si];
    out.push(Packet::tag(label, b[s].0, b[s + 19].1, m == 4));
    if label.starts_with('!') {
        return;
    }
    let mut p = s + 20;
    let word = |p: &mut usize, nibbles: usize| -> Option<(u32, f64, f64)> {
        let t0 = b.get(*p)?.0;
        let mut v = 0u32;
        for q in 0..nibbles {
            v |= nib(*p)? << (4 * q);
            *p += 5;
        }
        Some((v, t0, b[*p - 1].1))
    };
    let mut bytes = Vec::new();
    let Some((hdr, t0, t1)) = word(&mut p, 4) else {
        out.push(Packet::tag("!ERR", b[p.min(n - 1)].0, b[n - 1].1, false));
        return;
    };
    out.push(Packet::field("HDR", t0, t1, hdr, true));
    bytes.extend_from_slice(&(hdr as u16).to_le_bytes());
    for _ in 0..(hdr >> 12 & 7) {
        let Some((d, t0, t1)) = word(&mut p, 8) else {
            out.push(Packet::tag("!ERR", b[p.min(n - 1)].0, b[n - 1].1, false));
            return;
        };
        out.push(Packet::field("PDO", t0, t1, d, true));
        bytes.extend_from_slice(&d.to_le_bytes());
    }
    let Some((crc, t0, t1)) = word(&mut p, 8) else {
        out.push(Packet::tag("!ERR", b[p.min(n - 1)].0, b[n - 1].1, false));
        return;
    };
    out.push(Packet::new(Kind::Crc, t0, t1, crc, crc == usbpd_crc32(&bytes)));
    match sym(p) {
        Some(PD_EOP) => out.push(Packet::tag("!EOP", b[p].0, b[p + 4].1, true)),
        _ => out.push(Packet::tag("!ERR", b[p.min(n - 1)].0, b[n - 1].1, false)),
    }
}

/// USB Power Delivery on the CC line.
pub static USB_PD: ModuleInfo = ModuleInfo::new("UsbPd", "USB PD", "usbpd").roles([Some("CC"), None, None, None]);

/// USB Power Delivery (USB PD r3.1 chapter 5) on the CC line: BMC at 300 kbit/s +-10 % (the bit time is tracked
/// from the nominal value), preamble, 4b5b symbols LSB first. Per message: `Field("PRE")` (value = preamble bits),
/// the ordered set `Field("SOP")` / `"SOP'"` / `"SOP''"` / `"SOP'D"` / `"SOP''D"` (ok = all four K-codes right, three
/// suffice), `Field("HDR")` (value = 16-bit header, see `usbpd_msg_name`), `Field("PDO")` per 32-bit data object (see
/// `usbpd_pdo`), `Kind::Crc` (value = received CRC-32, ok = matches) and `Field("!EOP")`; `Field("!HRST")` /
/// `Field("!CRST")` for hard / cable reset, `Field("!ERR")` for an invalid symbol.
pub fn usbpd(cc: &[f32], dt: f64, thr: f32, hyst: f32) -> Vec<Packet> {
    let mut out = Vec::new();
    let bit = 1.0 / 300e3 / dt;
    if !bit.is_finite() || bit < 4.0 {
        return out;
    }
    let e = edge_times(cc, thr, hyst);
    let mut i = 0;
    while i < e.len() {
        let (b, j) = bmc_bits(&e, i, bit);
        if b.len() >= 40 {
            pd_frame(&b, &mut out);
        }
        i = j.max(i + 1);
    }
    out
}
