// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! The TPU's UART trigger (docs/fork/hwuart.md): a receiver on the comparator line of event R that fires on a start
//! bit, a data value or a frame error. Field layout from a simulation of the FPGA netlist; the register writes of the
//! trigger itself are `tpu::uart`.

use crate::dho::Write;

/// Configuration word: 20:0 bit time (low part), 22:21 condition, 25:23 relation, 27:26 error type, 29:28 stop bits,
/// 31:30 parity.
pub const CFG: u32 = 0x2100;
/// Data word: 2:0 data bits - 1, 10:3 min, 18:11 max.
pub const DATA: u32 = 0x2104;
/// Bit time bits 31:21 in [10:0]. The counter reads its results at the same offset; the write side is the UART's alone.
pub const BIT_HI: u32 = 0x221c;
/// The trigger mux value of the UART event (0x205c[4:0]).
pub const MUX: u32 = 4;

/// FPGA clock of the receiver (adc156), Hz.
pub const CLOCK_HZ: f64 = 156.25e6;
const BIT_LO_BITS: u32 = 21;
const BIT_LO_MASK: u32 = (1 << BIT_LO_BITS) - 1;
const BIT_HI_MASK: u32 = 0x7ff;

/// Clocks the event takes through the delay line and the trigger mux.
pub const PIPE_CLOCKS: f64 = 4.0;

/// Data relation of the data condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Eq,
    Ne,
    Gt,
    Lt,
    /// min <= x <= max
    In,
    /// x < min or x > max
    Out,
}

/// Error type of the error condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrKind {
    Parity,
    Framing,
    Either,
}

/// What fires the trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cond {
    /// every start bit
    Start,
    /// a data value: relation, min, max (Eq/Ne/Gt compare with min, Lt with max)
    Data(Relation, u8, u8),
    Error(ErrKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parity {
    None,
    Odd,
    Even,
    /// a parity bit is sent but not checked
    Unchecked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    One,
    Two,
    OneHalf,
}

/// A UART trigger set-up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Uart {
    pub baud: f64,
    /// data bits 1..8
    pub bits: u32,
    pub parity: Parity,
    pub stop: Stop,
    pub cond: Cond,
    /// idle low line (the comparator output is inverted before the receiver)
    pub invert: bool,
}

/// Receiver clocks per bit.
pub fn bit_clocks(baud: f64) -> u32 {
    (CLOCK_HZ / baud.max(1.0)).round().clamp(1.0, u32::MAX as f64) as u32
}

fn cond_fields(c: Cond) -> (u32, u32, u32) {
    match c {
        Cond::Start => (0, 0, 0),
        Cond::Data(r, _, _) => (
            1,
            match r {
                Relation::Eq => 0,
                Relation::Ne => 1,
                Relation::Gt => 2,
                Relation::Lt => 3,
                Relation::In => 4,
                Relation::Out => 5,
            },
            0,
        ),
        Cond::Error(e) => (
            2,
            0,
            match e {
                ErrKind::Parity => 0,
                ErrKind::Framing => 1,
                ErrKind::Either => 2,
            },
        ),
    }
}

/// The three words of `u`: (0x221c, 0x2100, 0x2104).
pub fn words(u: &Uart) -> [Write; 3] {
    let bt = bit_clocks(u.baud);
    let (cond, rel, err) = cond_fields(u.cond);
    let stop = match u.stop {
        Stop::One => 0,
        Stop::Two => 1,
        Stop::OneHalf => 2,
    };
    let par = match u.parity {
        Parity::None => 0,
        Parity::Odd => 1,
        Parity::Even => 2,
        Parity::Unchecked => 3,
    };
    let cfg = par << 30 | stop << 28 | err << 26 | rel << 23 | cond << 21 | (bt & BIT_LO_MASK);
    let (lo, hi) = match u.cond {
        Cond::Data(_, lo, hi) => (lo as u32, hi as u32),
        _ => (0, 0),
    };
    let data = (u.bits.clamp(1, 8) - 1) | lo << 3 | hi << 11;
    [(BIT_HI, (bt >> BIT_LO_BITS) & BIT_HI_MASK), (CFG, cfg), (DATA, data)]
}

/// When the receiver fires, in bit times after the falling edge of the start bit: the start condition at 0.6, the data
/// condition in the last data bit (bits + 0.7), the error condition at the stop sample (bits + parity + 1.7).
pub fn event_bits(u: &Uart) -> f64 {
    let n = u.bits.clamp(1, 8) as f64;
    let p = if u.parity == Parity::None { 0.0 } else { 1.0 };
    match u.cond {
        Cond::Start => 0.6,
        Cond::Data(..) => n + 0.7,
        Cond::Error(_) => n + p + 1.7,
    }
}

/// Seconds from the start edge to the trigger: the event plus the pipeline.
pub fn event_s(u: &Uart) -> f64 {
    event_bits(u) / u.baud + PIPE_CLOCKS / CLOCK_HZ
}

/// Seconds from the hardware trigger to the software protocol trigger's point, the end of the frame's first stop bit
/// (`dsp::decode::uart`, start + (2 + bits + parity) bit times). Shifting the record by this keeps the software trigger
/// point where the screen expects it.
pub fn sw_offset_s(u: &Uart) -> f64 {
    let n = u.bits.clamp(1, 8) as f64 + if u.parity == Parity::None { 0.0 } else { 1.0 };
    (2.0 + n) / u.baud - event_s(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(cond: Cond) -> Uart {
        Uart { baud: 115_200.0, bits: 8, parity: Parity::None, stop: Stop::One, cond, invert: false }
    }

    #[test]
    fn bit_time() {
        // (baud, clocks)
        for (b, c) in [(9600.0, 16276), (115_200.0, 1356), (1_000_000.0, 156), (50.0, 3_125_000)] {
            assert_eq!(bit_clocks(b), c, "{}", b);
        }
        // below ~75 baud the bit time needs bit 21 and up: 0x221c
        assert_eq!(words(&Uart { baud: 50.0, ..u(Cond::Start) })[0], (0x221c, 3_125_000 >> 21));
        assert_eq!(words(&u(Cond::Start))[0], (0x221c, 0));
    }

    #[test]
    fn words_by_condition() {
        // (set-up, 0x2100 [31:21], 0x2104)
        let cases: [(Uart, u32, u32); 9] = [
            (u(Cond::Start), 0, 7),
            (u(Cond::Data(Relation::Eq, 0x41, 0x41)), 1, 7 | 0x41 << 3 | 0x41 << 11),
            (u(Cond::Data(Relation::Ne, 0x41, 0x41)), 1 | 1 << 2, 7 | 0x41 << 3 | 0x41 << 11),
            (u(Cond::Data(Relation::In, 0x30, 0x39)), 1 | 4 << 2, 7 | 0x30 << 3 | 0x39 << 11),
            (u(Cond::Data(Relation::Out, 0x30, 0x39)), 1 | 5 << 2, 7 | 0x30 << 3 | 0x39 << 11),
            (u(Cond::Error(ErrKind::Parity)), 2, 7),
            (u(Cond::Error(ErrKind::Either)), 2 | 2 << 5, 7),
            (Uart { bits: 7, parity: Parity::Even, stop: Stop::Two, ..u(Cond::Error(ErrKind::Framing)) }, 2 | 1 << 5 | 1 << 7 | 2 << 9, 6),
            (Uart { bits: 5, parity: Parity::Odd, stop: Stop::OneHalf, ..u(Cond::Start) }, 2 << 7 | 1 << 9, 4),
        ];
        for (x, hi, data) in cases {
            let w = words(&x);
            assert_eq!((w[1].1 >> 21, w[1].1 & BIT_LO_MASK, w[2].1), (hi, 1356, data), "{:?}", x);
        }
    }

    #[test]
    fn event_timing_and_the_software_point() {
        let bit = 1.0 / 115_200.0;
        let pipe = 4.0 / 156.25e6;
        assert!((event_s(&u(Cond::Start)) - (0.6 * bit + pipe)).abs() < 1e-12);
        assert!((event_s(&u(Cond::Data(Relation::Eq, 0, 0))) - (8.7 * bit + pipe)).abs() < 1e-12);
        assert!((event_s(&Uart { parity: Parity::Even, ..u(Cond::Error(ErrKind::Either)) }) - (10.7 * bit + pipe)).abs() < 1e-12);
        // the software point is the end of the stop bit: 10 bit times for 8N1
        assert!((sw_offset_s(&u(Cond::Start)) - (9.4 * bit - pipe)).abs() < 1e-12);
        assert!((sw_offset_s(&u(Cond::Data(Relation::Eq, 0, 0))) - (1.3 * bit - pipe)).abs() < 1e-12);
    }
}
