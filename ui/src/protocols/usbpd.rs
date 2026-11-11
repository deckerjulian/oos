// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! USB Power Delivery on the CC line: BMC at 300 kbit/s, 4b5b symbols, SOP header, data objects, CRC-32.

use super::*;
use oos_dsp::decode_ext2 as x2;

pub struct UsbPd;

impl Protocol for UsbPd {
    fn key(&self) -> &'static str {
        "UsbPd"
    }
    fn name(&self) -> &'static str {
        "USB PD"
    }
    fn roles(&self) -> [Option<&'static str>; 4] {
        [Some("CC"), None, None, None]
    }
    fn bit_time(&self, _: &[usize; OPTS]) -> f64 {
        3.33e-6
    }
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        Ok(x2::usbpd(l.main(), l.dt, l.thr, l.hy))
    }
    /// The CC line idles near 1.1 V: threshold half of it.
    fn setup(&self) -> Setup {
        Setup { src: [Some(2), None, None, None], thr: 0.55, ..Setup::default() }
    }
    /// A source capabilities message with one fixed 5 V / 3 A object.
    fn demo(&self) -> Option<Demo> {
        let pd = x2::gen::PdFrame::Msg { sop: 0, header: 0x1161, data: vec![0x0801_912C], bad_crc: false };
        let a = x2::gen::usbpd(&[pd], 300e3, 0.0, 1.1);
        let period = a.seg.last().map_or(1e-3, |s| s.0) + 100e-6;
        Some(Demo { period, lines: [Some(Box::new(move |t| a.v(t))), None, None, None] })
    }
}
