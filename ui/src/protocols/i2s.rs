// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! I2S: serial audio, data (SD) sampled on the rising bit clock (SCK), the word select (WS) marks left and right; the
//! word length is the shared Data bits row. Built in until OOS 0.1.12, a module since, key `I2s` as before.

use super::*;

pub struct I2s;

/// The demo stream: 4 stereo frames of 16 bits at 48 kHz, starting after four idle clocks of 1 / (48 kHz x 32).
const PERIOD: f64 = 4.0 / 48e3;
const T0: f64 = 4.0 / (48e3 * 32.0);

impl Protocol for I2s {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::I2S
    }
    fn shared(&self) -> &'static [Row] {
        &[Row::Bits]
    }
    fn bit_time(&self, _: &[usize; OPTS]) -> f64 {
        0.6e-6
    }
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        let sck = l.need(1, "source channel off")?;
        Ok(oos_dsp::decode::i2s(sck, l.lines[2], l.main(), l.sh.bits, l.thr, l.hy))
    }
    /// A frame starts at a change of WS, either way.
    fn start_edge(&self) -> Slope {
        Slope::Either
    }
    fn start_role(&self) -> usize {
        2
    }
    /// The demo: WS on CH1, SD on CH2, SCK on CH3, all three shown; 16-bit words; the trigger on WS falling.
    fn setup(&self) -> Setup {
        Setup {
            src: [Some(1), Some(2), Some(0), None],
            show: true,
            rows: &[(Row::Bits, 16)],
            trig: Some(DemoTrig { role: 2, slope: Slope::Fall, idle: 0.0, level: 1.65 }),
            ..Setup::default()
        }
    }
    fn demo(&self) -> Option<Demo> {
        // `i2s_logic` gives (SCK, WS, SD)
        let (sck, ws, sd) = oos_dsp::sim::i2s_logic(
            &[(0x1234, -0x1234), (0x4000, -0x4000), (0x7fff, 0x0001), (0, -1)],
            16,
            48e3,
            0.0,
            3.3,
        );
        Some(Demo {
            period: PERIOD,
            lines: [
                Some(Box::new(move |t| sd.v(T0 + t))),
                Some(Box::new(move |t| sck.v(T0 + t))),
                Some(Box::new(move |t| ws.v(T0 + t))),
                None,
            ],
        })
    }
}
