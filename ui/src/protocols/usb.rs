// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! USB 1.x / 2.0 at low and full speed: NRZI with bit stuffing on D+ (D- optional, without it no SE0), SYNC, PID,
//! address and endpoint, data with CRC; the speed is the shared Speed row (Auto: from the idle state and the SYNC).
//! Built in until OOS 0.1.12, a module since, key `Usb` as before.

use super::*;
use oos_dsp::decode::Kind;
use oos_dsp::decode::{self as x3, UsbSpeed, USB_FULL, USB_LOW};

pub struct Usb;

impl Protocol for Usb {
    fn key(&self) -> &'static str {
        "Usb"
    }
    fn name(&self) -> &'static str {
        "USB"
    }
    fn roles(&self) -> [Option<&'static str>; 4] {
        [Some("D+"), None, Some("D-"), None]
    }
    fn shared(&self) -> &'static [Row] {
        &[Row::UsbSpeed]
    }
    /// Auto: full speed.
    fn bit_time(&self, _: &[usize; OPTS]) -> f64 {
        1.0 / USB_FULL
    }
    fn bit_time_sh(&self, _: &[usize; OPTS], sh: &Shared) -> f64 {
        1.0 / if sh.usb_speed == UsbSpeed::Low { USB_LOW } else { USB_FULL }
    }
    /// Auto: room for a low-speed packet before the trigger point.
    fn frame_span_sh(&self, _: &[usize; OPTS], sh: &Shared) -> Option<f64> {
        (sh.usb_speed == UsbSpeed::Auto).then_some(200.0 / USB_LOW)
    }
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        Ok(x3::usb(l.main(), l.lines[2], l.dt, l.sh.usb_speed, l.thr, l.hy))
    }
    /// The speed of the SYNC, PID names, bytes.
    fn text(&self, p: &Packet, _: &[usize; OPTS]) -> Option<String> {
        Some(match p.kind {
            Kind::Field("SYNC") => {
                format!("SYNC {}{}", if p.value >= 12000 { "FS" } else { "LS" }, if p.ok { "" } else { " !" })
            }
            Kind::Field("PID") if p.ok => x3::usb_pid_name(p.value as u8).to_string(),
            Kind::Field("PID") => format!("PID {:02X} !", p.value),
            Kind::Data => format!("{:02X}", p.value),
            _ => return None,
        })
    }
    /// ID: the device address or a PID.
    fn trigger_match(&self, what: BusWhat, value: u32, p: &Packet) -> Option<bool> {
        (what == BusWhat::Id).then_some(matches!(p.kind, Kind::Field("ADDR" | "PID")) && p.value == value)
    }
    /// The demo device's address.
    fn trigger_default(&self) -> (BusWhat, u32) {
        (BusWhat::Id, 5)
    }
    /// D+ on CH3, D- on CH4, 3.3 V.
    fn setup(&self) -> Setup {
        Setup { src: [Some(2), None, Some(3), None], ..Setup::default() }
    }
    /// Full speed: SOF; SETUP GET_DESCRIPTOR(device) to address 5, ACK; IN, the first 8 bytes of the device descriptor
    /// in DATA1, ACK.
    fn demo(&self) -> Option<Demo> {
        use x3::gen as g3;
        let up = |b: Vec<u8>| g3::UsbItem::packet(&b, USB_FULL);
        let ip = || g3::UsbItem::Idle(4.0 / USB_FULL);
        let items = [
            g3::UsbItem::Idle(2e-6),
            up(g3::usb_sof(0x2a5)),
            g3::UsbItem::Idle(3e-6),
            up(g3::usb_token(0x2D, 5, 0)),
            ip(),
            up(g3::usb_data(0xC3, &[0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00])),
            ip(),
            up(vec![0xD2]),
            g3::UsbItem::Idle(3e-6),
            up(g3::usb_token(0x69, 5, 0)),
            ip(),
            up(g3::usb_data(0x4B, &[0x12, 0x01, 0x00, 0x02, 0xff, 0x00, 0x00, 0x40])),
            ip(),
            up(vec![0xD2]),
        ];
        let (dp, dm) = g3::usb(&items, false, 0.0, 0.0, 3.3);
        let end = |l: &oos_dsp::sim::Logic| l.seg.last().map_or(1e-3, |s| s.0);
        let period = end(&dp).max(end(&dm)) + 5e-6;
        Some(Demo { period, lines: [Some(Box::new(move |t| dp.v(t))), None, Some(Box::new(move |t| dm.v(t))), None] })
    }
    fn legacy(&self, _: &Legacy) -> Option<[usize; OPTS]> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Break it catches: a setup written while I2S and USB were built in losing its protocol or its shared rows
    #[test]
    fn old_setups_keep_protocol_and_rows() {
        let load = |text: &str| crate::store::parse_setup(&format!("dec_on = true\n{}", text)).unwrap();
        let m = load("dec_proto = Usb\ndec_uspeed = 1\n");
        assert_eq!((m.dec_proto.name(), m.dec_uspeed), ("USB", 1));
        assert_eq!(m.bus_bit(), 1.0 / USB_LOW);
        let m = load("dec_proto = I2s\ndec_bits = 24\n");
        assert_eq!((m.dec_proto.name(), m.dec_bits), ("I2S", 24));
    }

    // Break it catches: the demo traffic of a module decoding to nothing
    #[test]
    fn the_demos_decode() {
        for p in [
            &Usb as &dyn Protocol,
            #[cfg(feature = "dec-i2s")]
            &super::super::i2s::I2s,
        ] {
            let d = p.demo().unwrap();
            let dt = 1e-8;
            let n = (d.period / dt) as usize;
            let waves: Vec<Option<Vec<f32>>> =
                d.lines.iter().map(|f| f.as_ref().map(|f| (0..n).map(|k| f(k as f64 * dt) as f32).collect())).collect();
            let l = Lines {
                lines: std::array::from_fn(|k| waves[k].as_deref()),
                dt,
                thr: 1.65,
                hy: 0.1,
                opt: [0; OPTS],
                sh: Shared::default(),
                chans: [None; 4],
                src: [None; 4],
            };
            assert!(!p.decode(&l).unwrap().is_empty(), "{}", p.name());
        }
    }
}
