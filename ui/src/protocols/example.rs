// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! A complete protocol module that uses every hook of [`Protocol`], as a template for new ones. It is compiled in the
//! tests only, so it never shows on a scope: copy it, rename it, and register the copy in `MODULES`.
//!
//! The "protocol" is UART at a selectable bit rate: the view counts the bytes of all records since the display was
//! cleared (in its memory), shows the last ones, and turns red when a byte had a framing error.

use super::*;
use oos_dsp::decode::{uart, Kind, Parity};

pub struct Example;

const RATES: [f64; 3] = [9600.0, 19200.0, 115200.0];

static OPTIONS: [Opt; 2] = [
    Opt { name: "Bit rate", choices: &["9600", "19200", "115200"], default: 1 },
    Opt { name: "Show", choices: &["Hex", "Decimal"], default: 0 },
];

impl Protocol for Example {
    fn key(&self) -> &'static str {
        "Example"
    }
    fn name(&self) -> &'static str {
        "Example"
    }
    fn roles(&self) -> [Option<&'static str>; 4] {
        [Some("Data"), None, None, None]
    }
    fn options(&self) -> &'static [Opt] {
        &OPTIONS
    }
    fn bit_time(&self, opt: &[usize; OPTS]) -> f64 {
        1.0 / RATES[opt[0]]
    }
    /// A frame is ten bits; 200 bit times would be enough, this shows the hook.
    fn frame_span(&self, opt: &[usize; OPTS]) -> Option<f64> {
        Some(40.0 * self.bit_time(opt))
    }
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        Ok(uart(l.need(0, "data line missing")?, l.dt, RATES[l.opt[0]], 8, Parity::None, l.thr, l.hy, false))
    }
    fn text(&self, p: &Packet, opt: &[usize; OPTS]) -> Option<String> {
        (p.kind == Kind::Data).then(|| if opt[1] == 1 { format!("{}", p.value) } else { format!("{:02X}", p.value) })
    }
    /// Frame = every byte; Data = the byte value.
    fn trigger_match(&self, what: BusWhat, value: u32, p: &Packet) -> Option<bool> {
        match what {
            BusWhat::Frame => Some(p.kind == Kind::Data),
            BusWhat::Data => Some(p.kind == Kind::Data && p.value == value),
            _ => None,
        }
    }
    fn trigger_default(&self) -> (BusWhat, u32) {
        (BusWhat::Data, 0x4f)
    }
    fn start_edge(&self) -> Slope {
        Slope::Fall
    }
    fn setup(&self) -> Setup {
        Setup { src: [Some(2), None, None, None], ..Setup::default() }
    }
    /// "OOS" at 19200 bit/s, 3.3 V.
    fn demo(&self) -> Option<Demo> {
        let baud = RATES[OPTIONS[0].default];
        let mut w = oos_dsp::decode_ext::gen::Wave::new(0.0, 3.3, 20e-9);
        w.level(true, 5.0 / baud);
        for b in *b"OOS" {
            w.uart(b as u32, 8, Parity::None, 1, baud);
        }
        let a = w.done();
        // an idle of two frames after the last stop bit: a UART decoder that started inside a byte finds the next
        // start bit again (with a short idle it would stay off by the same bits in every repeat)
        let period = a.seg.last().map_or(1e-3, |s| s.0) + 20.0 / baud;
        Some(Demo { period, lines: [Some(Box::new(move |t| a.v(t))), None, None, None] })
    }
    fn view(&self, p: &[Packet], opt: &[usize; OPTS], mem: &mut Memory) -> Option<View> {
        let bytes: Vec<&Packet> = p.iter().filter(|x| x.kind == Kind::Data).collect();
        // the total over all records: the module's own type in its memory
        let total: &mut usize = memory(mem);
        *total += bytes.len();
        let bad = p.iter().any(|x| !x.ok);
        let status = if bad { ("Framing error".to_string(), Tone::Bad) } else { (format!("{} bytes", total), Tone::Good) };
        let last = bytes.iter().rev().take(4).map(|x| self.text(x, opt).unwrap_or_default()).collect();
        Some(View { status: Some(status), sections: vec![Section { title: "Last bytes".into(), rows: last }] })
    }
}
