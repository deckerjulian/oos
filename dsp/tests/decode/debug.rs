// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The SWD and JTAG decoders against generated signals with known content.
//!
//! The tests of a new decoder of the family go here, as the decoder goes into the family's file in `dsp/src/decode/`;
//! its protocol module, registered in `ui/src/protocols/mod.rs` (ADR 0002), has its own tests in the ui.

use super::*;

#[test]
fn swd_reset_select_and_transfers() {
    let ops = [
        SwdOp::LineReset,
        SwdOp::Idle(2),
        SwdOp::JtagToSwd,
        SwdOp::LineReset,
        SwdOp::Idle(2),
        SwdOp::Xfer { ap: false, read: true, a: 0x0, ack: 1, data: 0x2ba0_1477, bad_parity: false }, // DPIDR
        SwdOp::Xfer { ap: true, read: false, a: 0x4, ack: 1, data: 0x2300_0052, bad_parity: false },
        SwdOp::Xfer { ap: true, read: true, a: 0xc, ack: 2, data: 0, bad_parity: false }, // WAIT: no data phase
        SwdOp::Idle(4),
        SwdOp::Xfer { ap: false, read: false, a: 0x8, ack: 1, data: 0x0000_00f0, bad_parity: false },
        SwdOp::Xfer { ap: true, read: true, a: 0xc, ack: 4, data: 0, bad_parity: false }, // FAULT
        SwdOp::Idle(8),
    ];
    let (clk, io) = gen::swd(&ops, 1e6, 0.0, 3.3);
    let dt = 1.0 / 20e6;
    let w = waves(&[&clk, &io], dt);
    let p = swd(&w[0], &w[1], 1.65, 0.3);
    let got: Vec<(Kind, u32, Option<u32>, bool)> = p.iter().map(|x| (x.kind, x.value, x.value2, x.ok)).collect();
    assert_eq!(
        got,
        vec![
            (F("!RESET"), 0, None, true),
            (F("!JTAG2SWD"), 0, None, true),
            (F("!RESET"), 0, None, true),
            (F("REQ"), 0x0, Some(0b10), true),
            (Kind::Ack, 1, None, true),
            (Kind::Data, 0x2ba0_1477, None, true),
            (F("REQ"), 0x4, Some(0b01), true),
            (Kind::Ack, 1, None, true),
            (Kind::Data, 0x2300_0052, None, true),
            (F("REQ"), 0xc, Some(0b11), true),
            (Kind::Ack, 2, None, false),
            (F("REQ"), 0x8, Some(0b00), true),
            (Kind::Ack, 1, None, true),
            (Kind::Data, 0xf0, None, true),
            (F("REQ"), 0xc, Some(0b11), true),
            (Kind::Ack, 4, None, false),
        ]
    );
}

#[test]
fn swd_data_parity_error_and_no_response() {
    let ops = [
        SwdOp::Idle(4),
        SwdOp::Xfer { ap: true, read: true, a: 0xc, ack: 1, data: 0x1234_5678, bad_parity: true },
        SwdOp::Idle(2),
        SwdOp::Xfer { ap: false, read: false, a: 0x4, ack: 1, data: 0x5000_0000, bad_parity: true },
        SwdOp::Idle(2),
        SwdOp::Xfer { ap: false, read: true, a: 0x0, ack: 7, data: 0, bad_parity: false }, // nobody drives: 0b111
        SwdOp::Idle(4),
    ];
    let (clk, io) = gen::swd(&ops, 4e6, 0.0, 1.8);
    let dt = 1.0 / 50e6;
    let mut w = waves(&[&clk, &io], dt);
    add_noise(&mut w[1], 0.1, 5);
    let p = swd(&w[0], &w[1], 0.9, 0.3);
    let data: Vec<(u32, bool)> = p.iter().filter(|x| x.kind == Kind::Data).map(|x| (x.value, x.ok)).collect();
    assert_eq!(data, vec![(0x1234_5678, false), (0x5000_0000, false)]);
    let acks: Vec<(u32, bool)> = p.iter().filter(|x| x.kind == Kind::Ack).map(|x| (x.value, x.ok)).collect();
    assert_eq!(acks, vec![(1, true), (1, true), (7, false)]);
}

fn jtag_example() -> JtagGen {
    let mut g = JtagGen::new();
    g.reset();
    g.idle(2);
    // IR = 0b1110 (4 bits), the TAP returns 0b0001 (IR capture pattern)
    g.shift(true, &bits_lsb(0b1110, 4), &bits_lsb(0b0001, 4), None);
    // IDCODE read: 32 bits out on TDO
    g.shift(false, &bits_lsb(0, 32), &bits_lsb(0x4ba0_0477, 32), None);
    // 40-bit DR with a pause after bit 12
    g.shift(false, &bits_lsb(0xab_1234_5678, 40), &bits_lsb(0x55_0f0f_0f0f, 40), Some(12));
    g.reset();
    g.idle(1);
    g
}

#[test]
fn jtag_ir_dr_scans_and_states() {
    let g = jtag_example();
    let [tck, tms, tdi, tdo] = g.logic(10e6, 0.0, 3.3);
    let dt = 1.0 / 200e6;
    let w = waves(&[&tck, &tms, &tdi, &tdo], dt);
    let p = jtag(&w[0], &w[1], &w[2], Some(&w[3]), Some(TapState::TestLogicReset), 1.65, 0.3);
    let got: Vec<(Kind, u32, Option<u32>)> = p.iter().map(|x| (x.kind, x.value, x.value2)).collect();
    assert_eq!(
        got,
        vec![
            (F("IR"), 0b1110, Some(0b0001)),
            (F("DR"), 0, Some(0x4ba0_0477)),
            (F("DR"), 0x1234_5678, Some(0x0f0f_0f0f)),
            (F("DR+"), 0xab, Some(0x55)),
            (F("!TLR"), 0, None),
        ]
    );
    let sh = jtag_shifts(&w[0], &w[1], &w[2], Some(&w[3]), Some(TapState::TestLogicReset), 1.65, 0.3);
    assert_eq!(
        sh.iter().map(|s| (s.ir, s.tdi.len(), s.tdo.len())).collect::<Vec<_>>(),
        vec![(true, 4, 4), (false, 32, 32), (false, 40, 40)]
    );
    // the paused scan passes Pause-DR and Exit2-DR
    let st = jtag_states(&w[0], &w[1], Some(TapState::TestLogicReset), 1.65, 0.3);
    let names: Vec<TapState> = st.iter().map(|x| x.1).collect();
    use TapState::*;
    assert_eq!(
        &names[..8],
        &[TestLogicReset, RunTestIdle, SelectDrScan, SelectIrScan, CaptureIr, ShiftIr, Exit1Ir, UpdateIr]
    );
    assert!(names.windows(4).any(|w| w == [Exit1Dr, PauseDr, Exit2Dr, ShiftDr]));
    assert_eq!(names.last(), Some(&RunTestIdle));
    assert!(st.windows(2).all(|w| w[0].0 <= w[1].0));
}

#[test]
fn jtag_unknown_start_waits_for_reset() {
    // a capture that begins in Run-Test/Idle: the decoder does not know the state until 5 TMS-high cycles
    let mut g = JtagGen::new();
    g.idle(3);
    g.shift(true, &bits_lsb(0b0101, 4), &[], None);
    g.reset();
    g.shift(true, &bits_lsb(0b1000, 4), &[], None);
    let [tck, tms, tdi, _] = g.logic(1e6, 0.0, 5.0);
    let dt = 1.0 / 20e6;
    let w = waves(&[&tck, &tms, &tdi], dt);
    let p = jtag(&w[0], &w[1], &w[2], None, None, 2.5, 0.3);
    assert_eq!(kv(&p), vec![(F("!TLR"), 0, true), (F("IR"), 0b1000, true)]);
    assert!(p.iter().all(|x| x.value2.is_none()));
    // with the right start state the first scan is found too
    let p = jtag(&w[0], &w[1], &w[2], None, Some(TapState::RunTestIdle), 2.5, 0.3);
    assert_eq!(p.iter().filter(|x| x.kind == F("IR")).map(|x| x.value).collect::<Vec<_>>(), vec![0b0101, 0b1000]);
}
