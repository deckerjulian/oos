// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The Quad-SPI decoders against generated signals with known content.
//!
//! The tests of a new decoder of the family go here, as the decoder goes into the family's file in `dsp/src/decode/`;
//! its protocol module, registered in `ui/src/protocols/mod.rs` (ADR 0002), has its own tests in the ui.

use super::*;

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
    assert_eq!(
        got[..6],
        [
            (F("CMD"), 0xeb),
            (F("ADDR"), 0xab_cdef),
            (F("DUMMY"), 6),
            (Kind::Data, 0x11),
            (Kind::Data, 0x22),
            (F("CMD"), 0xeb)
        ]
    );
    assert_eq!(got.len(), 7, "{:?}", got);
    assert_eq!(got[6], (F("ADDR"), 0x00_0100)); // 0x12 on IO0 only: 0,0,0,1,0,0 as 4-line nibbles
                                                // a wrong layout gives wrong data
    let p1 = qspi(&w[0], &w[1], &io, &QspiCfg::new(1, 1, 1, 3, 0, false), false, 1.65, 0.3);
    assert!(!p1.iter().any(|x| x.kind == Kind::Data && x.value == 0x11));
}
