// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! SWD, JTAG, Quad-SPI, I3C, S/PDIF and USB PD decoders against generated signals with known content.

use oos_dsp::decode::{Kind, Packet};
use oos_dsp::decode_ext2::gen::{self, bits_lsb, I3cBus, JtagGen, PdFrame, QspiFrame, SwdOp};
use oos_dsp::decode_ext2::*;
use oos_dsp::sim::{add_noise, sample, Logic};

/// Samples logic waveforms to a common length (until shortly after the last change of any of them).
fn waves(ls: &[&Logic], dt: f64) -> Vec<Vec<f32>> {
    let end = ls.iter().filter_map(|l| l.seg.last().map(|s| s.0)).fold(0.0, f64::max);
    let n = (end / dt) as usize + 200;
    ls.iter().map(|l| sample(|t| l.v(t), 0.0, dt, n)).collect()
}

fn kv(p: &[Packet]) -> Vec<(Kind, u32, bool)> {
    p.iter().map(|x| (x.kind, x.value, x.ok)).collect()
}

const F: fn(&'static str) -> Kind = Kind::Field;

// ------------------------------------------------------------------------------------------------- SWD

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

// ------------------------------------------------------------------------------------------------- JTAG

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
    assert_eq!(sh.iter().map(|s| (s.ir, s.tdi.len(), s.tdo.len())).collect::<Vec<_>>(), vec![(true, 4, 4), (false, 32, 32), (false, 40, 40)]);
    // the paused scan passes Pause-DR and Exit2-DR
    let st = jtag_states(&w[0], &w[1], Some(TapState::TestLogicReset), 1.65, 0.3);
    let names: Vec<TapState> = st.iter().map(|x| x.1).collect();
    use TapState::*;
    assert_eq!(&names[..8], &[TestLogicReset, RunTestIdle, SelectDrScan, SelectIrScan, CaptureIr, ShiftIr, Exit1Ir, UpdateIr]);
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

// ------------------------------------------------------------------------------------------------- Quad-SPI

fn qf(cfg: QspiCfg, op: u8, addr: u32, mode: u8, data: &[u8], miso: bool) -> QspiFrame {
    QspiFrame { cfg, op, addr, mode, data: data.to_vec(), miso }
}

fn qspi_waves(frames: &[QspiFrame]) -> Vec<Vec<f32>> {
    let l = gen::qspi(frames, 20e6, 0.0, 3.3);
    waves(&[&l[0], &l[1], &l[2], &l[3], &l[4], &l[5]], 1.0 / 400e6)
}

#[test]
fn qspi_auto_mode_follows_opcodes() {
    let c = QspiCfg::new;
    let frames = [
        qf(c(1, 1, 1, 0, 0, false), 0x9f, 0, 0, &[0xef, 0x40, 0x18], true),
        qf(c(1, 1, 1, 0, 0, false), 0x06, 0, 0, &[], false),
        qf(c(1, 1, 1, 3, 0, false), 0x02, 0x00_1000, 0, &[1, 2, 3], false),
        qf(c(1, 1, 1, 3, 8, false), 0x0b, 0x00_2000, 0, &[0x5a], true),
        qf(c(1, 1, 2, 3, 8, false), 0x3b, 0x00_3000, 0, &[0x12, 0x34], false),
        qf(c(1, 2, 2, 3, 0, true), 0xbb, 0x00_4000, 0xf0, &[0x56], false),
        qf(c(1, 1, 4, 3, 8, false), 0x6b, 0x00_5000, 0, &[0x78, 0x9a], false),
        qf(c(1, 4, 4, 3, 4, true), 0xeb, 0x12_3456, 0xa0, &[0xde, 0xad, 0xbe, 0xef], false),
        qf(c(1, 1, 1, 0, 0, false), 0x38, 0, 0, &[], false), // enter QPI (no data)
        qf(c(4, 4, 4, 3, 6, false), 0xeb, 0x65_4321, 0, &[0xc0, 0xff, 0xee], false),
        qf(c(4, 4, 4, 0, 0, false), 0xf5, 0, 0, &[], false), // exit QPI
        qf(c(1, 1, 1, 0, 0, false), 0xb7, 0, 0, &[], false), // 4-byte addresses
        qf(c(1, 1, 1, 4, 0, false), 0x03, 0x0123_4567, 0, &[0x42], true),
        qf(c(1, 1, 1, 0, 0, false), 0x05, 0, 0, &[0x03], true),
    ];
    let w = qspi_waves(&frames);
    let io: Vec<&[f32]> = w[2..6].iter().map(|v| v.as_slice()).collect();
    let start = QspiCfg::new(1, 1, 1, 3, 6, false); // SPI, 3-byte addresses, QPI reads with 6 dummy clocks
    let p = qspi(&w[0], &w[1], &io, &start, true, 1.65, 0.3);
    let d = |v: u32, v2: Option<u32>| (Kind::Data, v, v2);
    let got: Vec<(Kind, u32, Option<u32>)> = p.iter().map(|x| (x.kind, x.value, x.value2)).collect();
    #[rustfmt::skip]
    let want = vec![
        (F("CMD"), 0x9f, None), d(0, Some(0xef)), d(0, Some(0x40)), d(0, Some(0x18)),
        (F("CMD"), 0x06, None),
        (F("CMD"), 0x02, None), (F("ADDR"), 0x1000, None), d(1, Some(0)), d(2, Some(0)), d(3, Some(0)),
        (F("CMD"), 0x0b, None), (F("ADDR"), 0x2000, None), (F("DUMMY"), 8, None), d(0, Some(0x5a)),
        (F("CMD"), 0x3b, None), (F("ADDR"), 0x3000, None), (F("DUMMY"), 8, None), d(0x12, None), d(0x34, None),
        (F("CMD"), 0xbb, None), (F("ADDR"), 0x4000, None), (F("MODE"), 0xf0, None), d(0x56, None),
        (F("CMD"), 0x6b, None), (F("ADDR"), 0x5000, None), (F("DUMMY"), 8, None), d(0x78, None), d(0x9a, None),
        (F("CMD"), 0xeb, None), (F("ADDR"), 0x12_3456, None), (F("MODE"), 0xa0, None), (F("DUMMY"), 4, None),
        d(0xde, None), d(0xad, None), d(0xbe, None), d(0xef, None),
        (F("CMD"), 0x38, None),
        (F("CMD"), 0xeb, None), (F("ADDR"), 0x65_4321, None), (F("DUMMY"), 6, None), d(0xc0, None), d(0xff, None), d(0xee, None),
        (F("CMD"), 0xf5, None),
        (F("CMD"), 0xb7, None),
        (F("CMD"), 0x03, None), (F("ADDR"), 0x0123_4567, None), d(0, Some(0x42)),
        (F("CMD"), 0x05, None), d(0, Some(0x03)),
    ];
    assert_eq!(got, want);
    assert_eq!(qspi_opcode_name(0xeb), "4READ");
    assert_eq!(qspi_opcode_name(0x9f), "RDID");
}

#[test]
fn qspi_fixed_layout_and_truncated_frame() {
    let cfg = QspiCfg::new(1, 4, 4, 3, 6, false);
    // second frame: CS rises in the middle of the address
    let mut frames = vec![qf(cfg, 0xeb, 0xab_cdef, 0, &[0x11, 0x22], false)];
    frames.push(qf(QspiCfg::new(1, 1, 1, 0, 0, false), 0xeb, 0, 0, &[0x12], false)); // 8 + 8 single-line cycles
    let w = qspi_waves(&frames);
    let io: Vec<&[f32]> = w[2..6].iter().map(|v| v.as_slice()).collect();
    let p = qspi(&w[0], &w[1], &io, &cfg, false, 1.65, 0.3);
    // the short frame has 8 cycles after the command: enough for the 6 address cycles, not for the 6 dummy cycles
    let got: Vec<(Kind, u32)> = p.iter().map(|x| (x.kind, x.value)).collect();
    assert_eq!(got[..6], [(F("CMD"), 0xeb), (F("ADDR"), 0xab_cdef), (F("DUMMY"), 6), (Kind::Data, 0x11), (Kind::Data, 0x22), (F("CMD"), 0xeb)]);
    assert_eq!(got.len(), 7, "{:?}", got);
    assert_eq!(got[6], (F("ADDR"), 0x00_0100)); // 0x12 on IO0 only: 0,0,0,1,0,0 as 4-line nibbles
    // a wrong layout gives wrong data
    let p1 = qspi(&w[0], &w[1], &io, &QspiCfg::new(1, 1, 1, 3, 0, false), false, 1.65, 0.3);
    assert!(!p1.iter().any(|x| x.kind == Kind::Data && x.value == 0x11));
}

// ------------------------------------------------------------------------------------------------- I3C

#[test]
fn i3c_daa_ccc_private_i2c_ibi_and_hdr() {
    let mut b = I3cBus::new(1e6, 0.0, 1.8);
    // RSTDAA
    b.start();
    b.addr(0x7e, false, true);
    b.byte_t(0x06, false);
    b.stop();
    // ENTDAA: one target, then nobody answers
    b.start();
    b.addr(0x7e, false, true);
    b.byte_t(0x07, false);
    b.start();
    b.addr(0x7e, true, true);
    b.bits(0x0123_4567_89ab, 48);
    b.bits(0x27, 8);
    b.bits(0x44, 8);
    let da = 0x08u8;
    b.bits((da as u64) << 1 | (da.count_ones() % 2 == 0) as u64, 8);
    b.bit(false); // ACK
    b.start();
    b.addr(0x7e, true, false);
    b.stop();
    // private write and read with T-bits
    b.start();
    b.addr(0x7e, false, true);
    b.start();
    b.addr(da, false, true);
    b.byte_t(0x55, false);
    b.byte_t(0xaa, true); // wrong T-bit
    b.start();
    b.addr(da, true, true);
    b.read_t(0x11, true);
    b.read_t(0x22, false);
    b.stop();
    // legacy I2C device
    b.start();
    b.addr(0x50, false, true);
    b.byte_ack(0x00, true);
    b.start();
    b.addr(0x50, true, true);
    b.byte_ack(0xcd, false);
    b.stop();
    // SETDASA: static address 0x51 -> dynamic 0x09
    b.start();
    b.addr(0x7e, false, true);
    b.byte_t(0x87, false);
    b.start();
    b.addr(0x51, false, true);
    b.byte_t(0x09 << 1, false);
    b.stop();
    // IBI from 0x08 with one mandatory byte
    b.start();
    b.addr(da, true, true);
    b.read_t(0xae, false);
    b.stop();
    // HDR
    b.start();
    b.addr(0x7e, false, true);
    b.byte_t(0x20, false);
    b.hdr_junk(60);
    b.hdr_exit();
    // I2C after HDR, to the new address 0x09 (now I3C: T-bit)
    b.start();
    b.addr(0x09, false, true);
    b.byte_t(0x3c, false);
    b.stop();
    let dt = 1.0 / 40e6;
    let w = waves(&[&b.sda, &b.scl], dt);
    let p = i3c(&w[0], &w[1], &[], 0.9, 0.2);
    let a = |v: u32, read: bool, ok: bool| (Kind::Addr { read }, v, ok);
    let (st, sr, sp) = ((Kind::Start, 0, true), (Kind::RepStart, 0, true), (Kind::Stop, 0, true));
    let d = |v: u32, ok: bool| (Kind::Data, v, ok);
    #[rustfmt::skip]
    let want = vec![
        st, a(0x7e, false, true), (F("CCC"), 0x06, true), sp,
        st, a(0x7e, false, true), (F("CCC"), 0x07, true), sr, a(0x7e, true, true),
        (F("PID"), 0x4567_89ab, true), (F("BCR"), 0x27, true), (F("DCR"), 0x44, true), (F("DA"), 0x08, true),
        sr, a(0x7e, true, false), sp,
        st, a(0x7e, false, true), sr, a(0x08, false, true), d(0x55, true), d(0xaa, false),
        sr, a(0x08, true, true), d(0x11, true), d(0x22, true), (F("!EOD"), 0, true), sp,
        st, a(0x50, false, true), d(0x00, true), sr, a(0x50, true, true), d(0xcd, false), sp,
        st, a(0x7e, false, true), (F("CCC"), 0x87, true), sr, a(0x51, false, true), (F("DA"), 0x09, true), sp,
        st, (F("IBI"), 0x08, true), d(0xae, true), (F("!EOD"), 0, true), sp,
        st, a(0x7e, false, true), (F("CCC"), 0x20, true), (F("!HDR"), 0, true), (F("!HDREXIT"), 0, true), sp,
        st, a(0x09, false, true), d(0x3c, true), sp,
    ];
    assert_eq!(kv(&p), want);
    let pid = p.iter().find(|x| x.kind == F("PID")).unwrap();
    assert_eq!(pid.value2, Some(0x0123));
    assert_eq!(i3c_ccc_name(0x07), "ENTDAA");
    assert_eq!(i3c_ccc_name(0x8d), "GETPID");
}

#[test]
fn i3c_bad_parity_and_known_addresses() {
    let mut b = I3cBus::new(4e6, 0.0, 3.3);
    b.start();
    b.addr(0x7e, false, true);
    b.byte_t(0x01, true); // DISEC with a wrong T-bit
    b.byte_t(0x0b, false);
    b.stop();
    // 0x30 is unknown in the capture: decoded as I2C unless listed
    b.start();
    b.addr(0x30, false, true);
    b.byte_t(0x81, false); // two ones -> T = 1
    b.stop();
    let dt = 1.0 / 100e6;
    let mut w = waves(&[&b.sda, &b.scl], dt);
    add_noise(&mut w[0], 0.1, 9);
    let p = i3c(&w[0], &w[1], &[], 1.65, 0.4);
    let ccc: Vec<(u32, bool)> = p.iter().filter(|x| x.kind == F("CCC")).map(|x| (x.value, x.ok)).collect();
    assert_eq!(ccc, vec![(0x01, false)]);
    let data: Vec<(u32, bool)> = p.iter().filter(|x| x.kind == Kind::Data).map(|x| (x.value, x.ok)).collect();
    // as I2C the T-bit (1) is a NACK
    assert_eq!(data, vec![(0x0b, true), (0x81, false)]);
    let p = i3c(&w[0], &w[1], &[0x30], 1.65, 0.4);
    let data: Vec<(u32, bool)> = p.iter().filter(|x| x.kind == Kind::Data).map(|x| (x.value, x.ok)).collect();
    assert_eq!(data, vec![(0x0b, true), (0x81, true)]);
}

// ------------------------------------------------------------------------------------------------- S/PDIF

fn spdif_frames(n: usize) -> Vec<(u32, u32)> {
    (0..n as u32).map(|k| ((k * 0x01_2345) & 0xff_ffff, (0xff_ffff - k * 0x777) & 0xff_ffff)).collect()
}

#[test]
fn spdif_samples_blocks_and_channel_status() {
    let frames = spdif_frames(200);
    let mut cs = [0u8; 24];
    cs[0] = 0x04; // consumer, copying permitted
    cs[1] = 0x01; // category
    cs[3] = 0x02; // 48 kHz
    let fs = 48e3;
    let l = gen::spdif(&frames, &cs, fs, &[], 0.0, 0.5);
    let dt = 1.0 / (128.0 * fs) / 6.0; // 6 samples per UI
    let mut w = waves(&[&l], dt);
    add_noise(&mut w[0], 0.03, 1);
    for inv in [false, true] {
        let v: Vec<f32> = if inv { w[0].iter().map(|x| 0.5 - x).collect() } else { w[0].clone() };
        let p = spdif(&v, 0.25, 0.1);
        let s: Vec<(bool, u32)> = p.iter().filter_map(|x| if let Kind::Sample { right } = x.kind { Some((right, x.value)) } else { None }).collect();
        let want: Vec<(bool, u32)> = frames.iter().flat_map(|&(a, b)| [(false, a), (true, b)]).collect();
        assert_eq!(s, want, "inverted {}", inv);
        assert!(p.iter().filter(|x| matches!(x.kind, Kind::Sample { .. })).all(|x| x.ok));
        let blk: Vec<u32> = p.iter().filter(|x| x.kind == F("BLK")).map(|x| x.value).collect();
        assert_eq!(blk, vec![0, 1]);
        let got = spdif_channel_status(&p).expect("one complete block");
        assert_eq!(got, cs);
        assert_eq!(spdif_sample_rate(&got), Some(48000));
    }
    let mut pro = [0u8; 24];
    pro[0] = 0x01 | 2 << 6;
    assert_eq!(spdif_sample_rate(&pro), Some(44100));
}

#[test]
fn spdif_parity_error_and_short_capture() {
    let frames = spdif_frames(20);
    let cs = [0u8; 24];
    let l = gen::spdif(&frames, &cs, 44.1e3, &[5, 12], 0.0, 1.0);
    let dt = 1.0 / (128.0 * 44.1e3) / 5.0;
    let w = waves(&[&l], dt);
    let p = spdif(&w[0], 0.5, 0.2);
    let ok: Vec<bool> = p.iter().filter(|x| matches!(x.kind, Kind::Sample { .. })).map(|x| x.ok).collect();
    assert_eq!(ok.len(), 40);
    let bad: Vec<usize> = ok.iter().enumerate().filter(|x| !x.1).map(|x| x.0).collect();
    assert_eq!(bad, vec![5, 12]);
    // fewer than 192 frames: no channel status
    assert_eq!(spdif_channel_status(&p), None);
}

// ------------------------------------------------------------------------------------------------- USB PD

/// header: message type, number of data objects, message ID; PD 3.0, source, DFP
fn pd_hdr(t: u16, ndo: u16, id: u16) -> u16 {
    t | 1 << 5 | 2 << 6 | 1 << 8 | (id & 7) << 9 | (ndo & 7) << 12
}

#[test]
fn usbpd_source_caps_goodcrc_and_hard_reset() {
    let pdo5 = 100 << 10 | 300; // 5 V 3 A
    let pdo9 = 180 << 10 | 300; // 9 V 3 A
    let pps = 3 << 30 | 210 << 17 | 33 << 8 | 60; // PPS 3.3-21 V 3 A
    let frames = [
        PdFrame::Msg { sop: 0, header: pd_hdr(1, 3, 2), data: vec![pdo5, pdo9, pps], bad_crc: false },
        PdFrame::Msg { sop: 1, header: 0x0041, data: vec![], bad_crc: false }, // GoodCRC to SOP'
        PdFrame::HardReset,
        PdFrame::Msg { sop: 0, header: pd_hdr(2, 1, 3), data: vec![0x1304_b12c], bad_crc: true },
    ];
    // a transmitter 7 % fast
    let l = gen::usbpd(&frames, 300e3 * 1.07, 0.0, 1.1);
    let dt = 1.0 / 12e6;
    let mut w = waves(&[&l], dt);
    add_noise(&mut w[0], 0.05, 3);
    let p = usbpd(&w[0], dt, 0.55, 0.2);
    let pre = (F("PRE"), 64, true);
    #[rustfmt::skip]
    let want = vec![
        pre, (F("SOP"), 0, true), (F("HDR"), pd_hdr(1, 3, 2) as u32, true),
        (F("PDO"), pdo5, true), (F("PDO"), pdo9, true), (F("PDO"), pps, true), (Kind::Crc, 0, true), (F("!EOP"), 0, true),
        pre, (F("SOP'"), 0, true), (F("HDR"), 0x41, true), (Kind::Crc, 0, true), (F("!EOP"), 0, true),
        pre, (F("!HRST"), 0, true),
        pre, (F("SOP"), 0, true), (F("HDR"), pd_hdr(2, 1, 3) as u32, true), (F("PDO"), 0x1304_b12c, true), (Kind::Crc, 0, false), (F("!EOP"), 0, true),
    ];
    let got: Vec<(Kind, u32, bool)> = p.iter().map(|x| (x.kind, if x.kind == Kind::Crc { 0 } else { x.value }, x.ok)).collect();
    assert_eq!(got, want);
    assert_eq!(usbpd_msg_name(pd_hdr(1, 3, 2)), "Source_Capabilities");
    assert_eq!(usbpd_msg_name(0x0041), "GoodCRC");
    assert_eq!(usbpd_msg_name(pd_hdr(6, 0, 0)), "PS_RDY");
    assert_eq!(usbpd_msg_name(pd_hdr(2, 1, 0)), "Request");
    assert_eq!(usbpd_pdo(pdo9), Pdo::Fixed { volts: 9.0, amps: 3.0 });
    match usbpd_pdo(pps) {
        Pdo::Pps { min_v, max_v, amps } => assert!((min_v - 3.3).abs() < 1e-9 && (max_v - 21.0).abs() < 1e-9 && (amps - 3.0).abs() < 1e-9),
        x => panic!("{:?}", x),
    }
    assert_eq!(usbpd_crc32(b"123456789"), 0xcbf4_3926);
}

#[test]
fn usbpd_corrupted_symbol_and_slow_transmitter() {
    // a 9 % slow transmitter; a corrupted 4b5b symbol in the header -> !ERR
    let f = PdFrame::Msg { sop: 2, header: pd_hdr(3, 0, 1), data: vec![], bad_crc: false };
    let mut bits = gen::usbpd_bits(&f);
    let h = 64 + 20; // first header symbol
    bits[h..h + 5].copy_from_slice(&[false; 5]); // 00000 is not a symbol
    let mut l = Logic::new(0.0, 1.1, 0.1e-6);
    l.push(0.0, false);
    let ui = 0.5 / (300e3 * 0.91);
    let (mut t, mut lv) = (10.0 * ui, false);
    for &b in &bits {
        lv = !lv;
        l.push(t, lv);
        if b {
            lv = !lv;
        }
        l.push(t + ui, lv);
        t += 2.0 * ui;
    }
    l.push(t, false);
    let dt = 1.0 / 10e6;
    let w = waves(&[&l], dt);
    let p = usbpd(&w[0], dt, 0.55, 0.2);
    assert_eq!(kv(&p[..3]), vec![(F("PRE"), 64, true), (F("SOP''"), 0, true), (F("!ERR"), 0, false)]);
}

// ------------------------------------------------------------------------------------------------- robustness

#[test]
fn garbage_never_panics() {
    let mut s = 0x1234_5678_9abc_def1u64;
    let mut rnd = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    for round in 0..12 {
        let n = [0usize, 1, 3, 100, 5000, 20000][round % 6];
        // random logic with random run lengths, or pure noise
        let mut chan = || -> Vec<f32> {
            let mut v = Vec::with_capacity(n);
            let mut lv = false;
            while v.len() < n {
                let run = 1 + (rnd() % if round < 6 { 40 } else { 3 }) as usize;
                lv = rnd() % 2 == 0 || !lv;
                v.extend((0..run).map(|_| if lv { 3.3 } else { 0.0 }));
            }
            v.truncate(n);
            if round % 3 == 2 {
                v.iter_mut().for_each(|x| *x = 1.65);
                add_noise(&mut v, 2.0, round as u64 + 1);
            }
            if round == 11 && n > 0 {
                v[n / 2] = f32::NAN;
            }
            v
        };
        let c: Vec<Vec<f32>> = (0..6).map(|_| chan()).collect();
        let _ = swd(&c[0], &c[1], 1.65, 0.2);
        let _ = jtag(&c[0], &c[1], &c[2], Some(&c[3]), None, 1.65, 0.2);
        let _ = jtag(&c[0], &c[1], &c[2], None, Some(TapState::ShiftDr), 1.65, 0.2);
        let io: Vec<&[f32]> = c[2..6].iter().map(|v| v.as_slice()).collect();
        let _ = qspi(&c[0], &c[1], &io, &QspiCfg::new(1, 4, 4, 4, 6, true), true, 1.65, 0.2);
        let _ = qspi(&c[0], &c[1], &io[..1], &QspiCfg::new(3, 0, 9, 9, 1000, true), false, 1.65, 0.2);
        let _ = qspi(&c[0], &c[1], &[], &QspiCfg::new(4, 4, 4, 3, 0, false), true, 1.65, 0.2);
        let _ = i3c(&c[0], &c[1], &[0x08, 0x7e], 1.65, 0.2);
        let _ = spdif(&c[0], 1.65, 0.2);
        let _ = spdif_channel_status(&spdif(&c[2], 1.65, 0.2));
        for dt in [1e-9, 1e-7, 0.0, -1.0, f64::NAN] {
            let _ = usbpd(&c[0], dt, 1.65, 0.2);
        }
    }
    for x in 0..=255u8 {
        let _ = (qspi_opcode_name(x), qspi_opcode_cfg(x, x % 2 == 0, 3, 6), i3c_ccc_name(x));
    }
    for k in 0..2000u32 {
        let h = (rnd() & 0xffff) as u16;
        let _ = (usbpd_msg_name(h), usbpd_pdo(rnd() as u32 ^ k));
    }
}
