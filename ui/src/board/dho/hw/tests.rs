// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The tests of the hardware source (`hw`): settings for the model, decodes, plans, switches and the protocol
//! trigger, run on the host. A test of a new part of `hw` goes here.

use super::decode::decode;
use super::plan::{
    batch_plan, deep_raw_settings, edge_check, final_replay, frame0_check_wanted, frame0_left_out, frame1_off_trigger,
    prearm_keeps_replay, stop_plan, zone_batch, zone_batch_from, Edge, FRAME0_CHECK_MAX_FRAMES, FRAME0_KEEP,
    FRAME1_CHECK_MAX, PREARM,
};
use super::serial::{i2c_of, serial_debug, serial_lines_acquired, spi_of, uart_debug, uart_of, SERIAL_HW};
use super::settings::{
    live_compress, raw_for_ui, raw_newest, screen_margin, seg_max, seg_read_plan, settings, single_wants_more,
    slow_mode, LIVE_DEPTH, ROLL_TDIV, SCAN_TDIV,
};
use super::trim::{overlay_in_use, TrimPending, CORE_OFFSET, TRIM_HOLD};
use super::units::{
    dvm_step, hw_merge, mask_map_from, unit_key, DvmKey, DvmStep, UnitGuard, DVM_HW, HW_STATUS_OFF, MASK_HW, ZONE_HW,
};
use crate::board::{Frame, Slow};
use crate::model::*;
use oos_hal::dho::seq::{Chan, Segments, Settings, Trig};
use oos_hal::dho::{auxout, hz, uarttrig};
use std::sync::Arc;

// Break it catches: an opt-in switch read from the settings file alone, so the image defaults of a unit
// (/etc/oos/system.conf) do not reach it while every other switch takes them.
#[test]
fn the_switches_take_the_image_defaults_under_the_settings_file() {
    use super::{plan::zone_batch_from, units::mask_map_from};
    use crate::sysconf::fork_text_of as both;
    for f in [&CORE_OFFSET, &HW_STATUS_OFF, &DVM_HW, &MASK_HW, &ZONE_HW, &SERIAL_HW, &PREARM, &FRAME0_KEEP] {
        f.check_both_files();
    }
    assert!(!DVM_HW.of(None, "dvm.hw = yes\n") && !CORE_OFFSET.of(None, "cal.core_offset = on\n"));
    let image = "zone.batch = 8\nmask.map.mid = 100\n";
    assert_eq!((zone_batch_from(&both("", image)), mask_map_from(&both("", image)).mid), (8, 100.0));
    let settings = "zone.batch = 2\nmask.map.mid = 120\n";
    assert_eq!((zone_batch_from(&both(settings, image)), mask_map_from(&both(settings, image)).mid), (2, 120.0));
    assert_eq!(zone_batch_from(&both("", "")), 1);
}

// Break it catches: SINGLE armed for a whole batch (248 frames at 1 us/div on the unit, all but the first unseen)
#[test]
fn single_records_one_frame_per_batch() {
    for tdiv in [1e-6f32, 1e-3] {
        let mut m = Model {
            tscale: tscales().iter().position(|&t| (t - tdiv).abs() < tdiv * 1e-3).unwrap(),
            ..Default::default()
        };
        let frames = |m: &Model| {
            let s = settings(m, false);
            let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, 0);
            batch_plan(&s, &p).frames
        };
        assert!(tdiv > 1e-4 || frames(&m) > 1, "{} s/div: RUN reads batches", tdiv);
        m.trig_mode = TrigMode::Single;
        assert_eq!(frames(&m), 1, "{} s/div", tdiv);
    }
}

// Break it catches: batches after the SINGLE frame until the UI's STOP reached the thread (`acquired` +10 at
// 1 us/div, +3 at 1 ms/div per press on the unit); or a SINGLE that waits for good where the UI refuses frames
#[test]
fn single_stops_after_the_batch_with_its_frame() {
    let mut m = Model { trig_mode: TrigMode::Single, ..Default::default() };
    let b = settings(&m, false);
    assert!(b.single);
    assert!(single_wants_more(&b, None, 4), "the first batch after the press");
    assert!(!single_wants_more(&b, Some(4), 4), "its frame came: no more for this press");
    assert!(single_wants_more(&b, Some(4), 5), "the next press");
    m.trig_mode = TrigMode::Normal;
    assert!(single_wants_more(&settings(&m, false), Some(4), 4), "RUN is not limited");
    // the UI may refuse the frame: protocol trigger, zone trigger
    m.trig_mode = TrigMode::Single;
    m.trig_type = TrigType::Bus;
    assert!(!settings(&m, false).single);
    m.trig_type = TrigType::Edge;
    m.zones[0] = crate::zone::Zone { on: true, src: 0, inside: true, pts: vec![(0.0, 0.0), (1e-6, 1.0)] };
    assert!(m.zones[0].usable() && !settings(&m, false).single);
}

// Break it catches: Auto depth that ignores the trigger delay, so the record (centred on the delay, the trigger
// inside it) ends before the screen does: 10 us/div with 200 us delay drew 20 us of the screen (unit 2026-11-13)
#[test]
fn auto_depth_covers_a_delayed_screen() {
    for (tdiv, delay, chans) in
        [(10e-6f32, 200e-6f32, 1usize), (10e-6, -200e-6, 1), (1e-6, 50e-6, 2), (100e-6, 3e-3, 4), (10e-6, 0.0, 1)]
    {
        let mut m = Model {
            tscale: tscales().iter().position(|&t| (t - tdiv).abs() < tdiv * 1e-3).unwrap(),
            delay,
            ..Default::default()
        };
        for c in 0..NCH {
            m.ch[c].on = c < chans;
        }
        let s = settings(&m, false);
        let mut st = oos_hal::dho::seq::State::handover(s.mask, None);
        let (_, p) = st.configure(&s, None, None).unwrap();
        // the record's span relative to the trigger: [first, last] in s
        let dot = p.dot_fs as f64 * 1e-15;
        let (first, last) = (-(p.pre as f64) * dot, p.post as f64 * dot);
        let (lo, hi) = (delay as f64 - 5.0 * tdiv as f64, delay as f64 + 5.0 * tdiv as f64);
        assert!(
            first <= lo + dot && last >= hi - dot,
            "{} s/div delay {} {} ch: record {}..{} s, screen {}..{}",
            tdiv,
            delay,
            chans,
            first,
            last,
            lo,
            hi
        );
        if delay == 0.0 {
            assert!(s.depth <= LIVE_DEPTH, "no delay: the plain Auto depth");
        }
    }
}
use super::*;
use crate::board::to_record;

#[test]
fn aux_out_reaches_the_settings() {
    let mut m = Model::default();
    assert_eq!(settings(&m, false).aux, auxout::AuxOut::TriggerOut);
    m.aux_out = AuxOut::Off;
    let s = settings(&m, false);
    assert_eq!(s.aux, auxout::AuxOut::Off);
    let w = oos_hal::dho::seq::State::handover(s.mask, None).configure(&s, None, None).unwrap().0;
    assert!(w.iter().any(|x| matches!(x, oos_hal::dho::seq::Step::Wr((0x1430, 2)))));
    m.aux_out = AuxOut::PassFail;
    assert_eq!(settings(&m, false).aux, auxout::AuxOut::PassFail(auxout::PassFail::DEFAULT));
}

#[test]
fn mask_columns_of_the_board() {
    assert_eq!(crate::board::MASK_COLUMNS, oos_hal::dho::mask::COLUMNS);
}

/// Live compression and the mask test never run together: a mask (in software or in the FPGA, which needs
/// `mask_on` too) keeps every frame raw, so the fast path the hardware mask keeps on draws uncompressed frames.
#[test]
fn a_mask_keeps_live_compression_off() {
    let mut m = Model {
        tscale: tscales().iter().position(|&t| (t as f64 - 10e-6).abs() < 1e-11).unwrap(),
        ..Default::default()
    };
    let mut s = settings(&m, false);
    live_compress(&m, &mut s);
    assert!(s.compress > 1, "compression applies at 10 us/div without a mask");
    m.mask_on = true;
    let mut s = settings(&m, false);
    live_compress(&m, &mut s);
    assert_eq!(s.compress, 1);
}

/// The zones in the hardware tables, and the "touched" logic modelled on the read-out words (the assumed sample
/// scale): agrees with the software zone test on boxes on, off and across a square wave's edge.
#[test]
fn hardware_zone_tables_agree_with_the_software_zones() {
    use oos_hal::dho::{mask, zone};
    let (n, rate, pre, scale, offset) = (20_000u32, 100_000_000u64, 10_000u32, 1.0, -1.5);
    let dt = 1.0 / rate as f64;
    // 0 / 3 V square, period 4000 samples (40 us), rising edge at the trigger (sample 10,000)
    let v: Vec<f32> = (0..n as usize).map(|k| if (k / 2000) % 2 == 0 { 0.0 } else { 3.0 }).collect();
    let t0 = -(pre as f64) * dt;
    let touched = |poly: &[(f64, f64)]| -> bool {
        let t = zone::table(
            &zone::Zone { poly: poly.to_vec(), inside: true },
            n,
            rate,
            pre,
            1,
            scale,
            offset,
            1.0,
            &mask::Map::default(),
        );
        let per = mask::Map::default().layout(n, 1).1 as usize;
        let code = |x: f32| ((32768.0 + (x as f64 + offset) / scale * 7500.0).round() as u32 >> mask::SHIFT) as u8;
        let mut prev: Option<u8> = None;
        for (k, &x) in v.iter().enumerate() {
            let c = t[(k / per).min(t.len() - 1)];
            let s = code(x);
            if c.on
                && ((c.lo..=c.hi).contains(&s)
                    || prev.is_some_and(|p| (p < c.lo && s > c.hi) || (p > c.hi && s < c.lo)))
            {
                return true;
            }
            prev = Some(s);
        }
        false
    };
    // (box, touched): on the high level, on the low level, above the trace, across the rising edge at t = 0
    let cases = [
        (vec![(2e-6, 2.5), (8e-6, 3.5)], true),
        (vec![(-8e-6, -0.5), (-2e-6, 0.5)], true),
        (vec![(2e-6, 3.6), (8e-6, 4.0)], false),
        (vec![(-1e-6, 1.0), (1e-6, 2.0)], true),
        (vec![(5e-6, 1.0), (15e-6, 2.0)], false),
    ];
    for (pts, want) in cases {
        let sw = crate::zone::hits(&crate::zone::Zone { on: true, src: 0, inside: true, pts: pts.clone() }, &v, t0, dt);
        assert_eq!((sw, touched(&pts)), (want, want), "{:?}", pts);
    }
}

#[test]
#[cfg_attr(not(feature = "decoders"), ignore = "needs every decoder")]
fn uart_from_the_decoder_settings() {
    use oos_hal::dho::uarttrig::{Cond, ErrKind, Parity, Relation};
    let mut m = Model::default();
    (m.trig_type, m.dec_proto, m.dec_src, m.dec_bits, m.dec_baud) = (TrigType::Bus, Proto::UART, 1, 8, 7);
    // (condition, value) -> receiver condition (None = the software trigger)
    let cases = [
        (BusWhat::Frame, 0, Some(Cond::Start)),
        (BusWhat::Data, 0x41, Some(Cond::Data(Relation::Eq, 0x41, 0x41))),
        (BusWhat::Data, 0x141, None),
        (BusWhat::Error, 0, Some(Cond::Error(ErrKind::Either))),
        (BusWhat::Id, 0x41, None),
    ];
    for (what, v, want) in cases {
        (m.trig_bus, m.trig_bus_val) = (what, v);
        assert_eq!(uart_of(&m).map(|u| u.cond), want, "{:?} {:#x}", what, v);
    }
    m.trig_bus = BusWhat::Frame;
    m.dec_parity = oos_dsp::decode::Parity::Even;
    let u = uart_of(&m).unwrap();
    assert_eq!((u.parity, u.baud, u.bits, u.invert), (Parity::Even, crate::decode::BAUDS[7], 8, false));
    // 7 data bits: a value with bit 7 set cannot match
    (m.dec_bits, m.trig_bus, m.trig_bus_val) = (7, BusWhat::Data, 0x80);
    assert_eq!(uart_of(&m), None);
    // other protocols, the LA as input, RS-485: the software trigger
    (m.dec_bits, m.trig_bus) = (8, BusWhat::Frame);
    for f in [
        |m: &mut Model| m.dec_proto = Proto::SPI,
        |m: &mut Model| m.dec_proto = Proto::from_key("Uart9").unwrap(),
        |m: &mut Model| m.dec_diff = Some(2),
        |m: &mut Model| m.trig_type = TrigType::Edge,
    ] {
        let mut x = m.clone();
        f(&mut x);
        assert_eq!(uart_of(&x), None);
    }
    assert!(uart_debug(&m).starts_with("off,src=CH2,"), "{}", uart_debug(&m));
}

#[test]
fn spi_and_i2c_from_the_decoder_settings() {
    use oos_hal::dho::serialtrig::I2cCond;
    let mut m = Model::default();
    (m.trig_type, m.dec_proto, m.dec_src, m.dec_src2, m.dec_cs, m.dec_bits, m.dec_mode) =
        (TrigType::Bus, Proto::SPI, 0, 1, Some(2), 8, 0);
    // (condition, value) -> (value, mask) of the SPI compare; None = software
    let cases = [
        (BusWhat::Frame, 0, Some((0, 0))),
        (BusWhat::Data, 0x5a, Some((0x5a, 0xff))),
        (BusWhat::Data, 0x15a, None),
        (BusWhat::Error, 0, None),
        (BusWhat::Id, 1, None),
    ];
    for (what, v, want) in cases {
        (m.trig_bus, m.trig_bus_val) = (what, v);
        assert_eq!(spi_of(&m).map(|s| (s.min, s.mask)), want, "{:?}", what);
    }
    (m.trig_bus, m.trig_bus_val) = (BusWhat::Data, 0x5a);
    let s = spi_of(&m).unwrap();
    assert_eq!((s.clk, s.data, s.cs, s.clk_falling, s.width), (2, 1, 3, false, 8));
    m.dec_mode = 1;
    assert!(spi_of(&m).unwrap().clk_falling);
    m.dec_cs = None;
    assert_eq!(spi_of(&m), None, "no chip select: software");
    // I2C: SDA on dec_src, SCL on dec_src2
    let mut m = Model::default();
    (m.trig_type, m.dec_proto, m.dec_src, m.dec_src2) = (TrigType::Bus, Proto::I2C, 0, 1);
    let cases = [
        (BusWhat::Frame, 0, Some(I2cCond::Start)),
        (BusWhat::Id, 0x50, Some(I2cCond::Address)),
        (BusWhat::Id, 0x150, None),
        (BusWhat::Error, 0, Some(I2cCond::MissingAck)),
        (BusWhat::Data, 0x12, None),
    ];
    for (what, v, want) in cases {
        (m.trig_bus, m.trig_bus_val) = (what, v);
        assert_eq!(i2c_of(&m).map(|i| i.cond), want, "{:?}", what);
    }
    // data in the first two bytes after the address: the receiver, big-endian, mask of two bytes
    (m.trig_bus, m.trig_bus_val, m.ser.bytes) = (BusWhat::Data, 0x1234, 2);
    let d = i2c_of(&m).unwrap();
    assert_eq!((d.cond, d.bytes, d.data), (I2cCond::Data, 2, (0x1234, 0x1234, 0xffff)));
    m.ser.bytes = 0;
    (m.trig_bus, m.trig_bus_val) = (BusWhat::Id, 0x50);
    let i = i2c_of(&m).unwrap();
    assert_eq!((i.scl, i.sda, i.addr), (2, 1, (0x50, 0x50, 0x7f)));
    assert!(serial_debug(&m).starts_with("off,proto=I2C,scl=CH2,sda=CH1,cond=Address,r2190="), "{}", serial_debug(&m));
    // lines that are not acquired fall back to the edge trigger
    assert!(serial_lines_acquired(&Trig::I2c(i), 0b11) && !serial_lines_acquired(&Trig::I2c(i), 0b01));
    assert!(serial_lines_acquired(&Trig::Edge, 0));
}

#[test]
fn serial_settings_reach_the_receivers() {
    use oos_hal::dho::serialtrig::{Dir, I2cCond};
    use oos_hal::dho::uarttrig::{Cond, ErrKind, Relation, Stop};
    // UART: relation and limit, polarity, stop bits, error kind; a mask keeps the software
    let mut m = Model::default();
    (m.trig_type, m.dec_proto, m.dec_bits, m.trig_bus, m.trig_bus_val) =
        (TrigType::Bus, Proto::UART, 8, BusWhat::Data, 0x30);
    (m.ser.rel, m.ser.max, m.ser.uart_inv, m.ser.uart_stop) = (Rel::In, 0x39, true, StopBits::Two);
    let u = uart_of(&m).unwrap();
    assert_eq!((u.cond, u.invert, u.stop), (Cond::Data(Relation::In, 0x30, 0x39), true, Stop::Two));
    m.ser.mask = 0x0f;
    assert_eq!(uart_of(&m), None);
    (m.ser.mask, m.ser.max) = (u64::MAX, 0x139);
    assert_eq!(uart_of(&m), None, "a limit wider than the data bits");
    (m.trig_bus, m.ser.uart_err) = (BusWhat::Error, UartErr::Parity);
    assert_eq!(uart_of(&m).unwrap().cond, Cond::Error(ErrKind::Parity));
    // SPI: without CS the timeout frames; relation, limit and mask to the width; CS polarity
    let mut m = Model::default();
    (m.trig_type, m.dec_proto, m.dec_src, m.dec_src2, m.dec_cs, m.dec_bits) =
        (TrigType::Bus, Proto::SPI, 0, 1, None, 16);
    (m.trig_bus, m.trig_bus_val, m.ser.rel, m.ser.max, m.ser.mask) = (BusWhat::Data, 0x1000, Rel::Out, 0x2000, 0xff00);
    assert_eq!(spi_of(&m), None, "no CS and no timeout");
    m.ser.spi_timeout = 2e-6;
    let s = spi_of(&m).unwrap();
    assert_eq!(
        (s.timeout, s.cs, s.rel, s.min, s.max, s.mask),
        (Some(2e-6f32 as f64), 2, Relation::Out, 0x1000, 0x2000, 0xff00)
    );
    (m.dec_cs, m.ser.spi_cs_high) = (Some(2), true);
    let s = spi_of(&m).unwrap();
    assert_eq!((s.timeout, s.cs, s.cs_high), (None, 3, true));
    // I2C: 10-bit address range, read only
    let mut m = Model::default();
    (m.trig_type, m.dec_proto, m.dec_src, m.dec_src2, m.trig_bus, m.trig_bus_val) =
        (TrigType::Bus, Proto::I2C, 0, 1, BusWhat::Id, 0x200);
    (m.ser.ten_bit, m.ser.addr_rel, m.ser.addr_max, m.ser.dir) = (true, Rel::In, 0x2ff, I2cDir::Read);
    let i = i2c_of(&m).unwrap();
    assert_eq!(
        (i.cond, i.ten_bit, i.addr_rel, i.addr, i.dir),
        (I2cCond::Address, true, Relation::In, (0x200, 0x2ff, 0x3ff), Dir::Read)
    );
    m.ser.ten_bit = false;
    assert_eq!(i2c_of(&m), None, "a 10-bit value with 7-bit addresses");
}

#[test]
fn uart_shifts_the_record_to_the_software_point() {
    let mut m = Model::default();
    (m.trig_type, m.dec_proto, m.dec_src, m.trig_bus) = (TrigType::Bus, Proto::UART, 0, BusWhat::Frame);
    let u = uart_of(&m).unwrap();
    let mut s = settings(&m, false);
    let d0 = s.delay;
    s.trigger = Trig::Uart(u);
    // `settings` applies the shift only with the switch on; the same arithmetic here
    if let (Trig::Uart(u), Slow::Off) = (&s.trigger, slow_mode(&m)) {
        s.delay += uarttrig::sw_offset_s(u);
    }
    let bit = 1.0 / crate::decode::BAUDS[m.dec_baud];
    assert!((s.delay - d0 - (9.4 * bit - 4.0 / 156.25e6)).abs() < 1e-12);
}

#[test]
fn mask_map_from_conf_lines() {
    use oos_hal::dho::mask::{Domain, Layout, Map};
    assert_eq!(mask_map_from(""), Map::default());
    let m =
        mask_map_from("mask.hw = on\nmask.map.domain = pregain\nmask.map.cols = stock # sweep\nmask.map.shift = x\n");
    assert_eq!((m.domain, m.layout, m.shift), (Domain::PreGain, Layout::Stock, 8));
}

#[test]
fn zone_batches_hold_zone_batch_frames() {
    assert_eq!(
        (zone_batch_from(""), zone_batch_from("zone.batch = 8\n"), zone_batch_from("zone.batch = 0\n")),
        (1, 8, 1)
    );
    let m = Model {
        tscale: tscales().iter().position(|&t| (t as f64 - 1e-6).abs() < 1e-12).unwrap(),
        ..Default::default()
    };
    let s = settings(&m, false);
    let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, 0);
    assert!(batch_plan(&s, &p).frames > 1);
    let z = Settings {
        hwzone: Some(oos_hal::dho::seq::HwZone {
            ch: 0,
            zones: vec![],
            clear: 0,
            map: oos_hal::dho::mask::Map::default(),
        }),
        ..s
    };
    assert_eq!(batch_plan(&z, &p).frames, zone_batch());
}

/// Unit round 5: at 100 us/div with 10k points the record is 1.6 screens long, and a table spread over the record
/// failed every frame of a clean 1 kHz square. Mapped by time, the clean signal passes and a 10 us shift fails;
/// spread over the record (the old mapping) the clean signal fails.
#[test]
fn mask_table_follows_the_screen_when_the_record_is_longer() {
    use oos_hal::dho::mask;
    let (n, pre, rate) = (10_000usize, 5_000u32, 6_250_000u64);
    let dt = 1.0 / rate as f64;
    // 0 / 3 V, 1 kHz, rising edge at the trigger; `shift` seconds later
    let wave = |shift: f64| -> Vec<f32> {
        (0..n)
            .map(|k| (k as f64 - pre as f64) * dt - shift)
            .map(|t| if (t * 1e3).rem_euclid(1.0) < 0.5 { 3.0 } else { 0.0 })
            .collect()
    };
    // the screen: -0.5..0.5 ms = samples 1875..8125; the mask from it
    let (s0, s1) = (1875, 8125);
    let m = oos_dsp::mask::build(&wave(0.0)[s0..s1], 20, 0.3);
    let (lo, hi) = m.columns(mask::COLUMNS);
    let fails = |t: &mask::Timing, v: &[f32]| -> bool {
        let map = mask::Map::default();
        let cols = mask::table_map(&lo, &hi, t, 1.0, -1.5, 1.0, &map);
        let per = map.layout(t.points, 1).2;
        v.iter().enumerate().any(|(k, &x)| {
            let c = (k as f64 / per) as usize;
            let code = map.code(x as f64, 1.0, -1.5, 1.0).floor() as i32;
            c < cols.len()
                && ((cols[c].lo_en && code < cols[c].lo as i32) || (cols[c].hi_en && code > cols[c].hi as i32))
        })
    };
    let by_time = mask::Timing { points: n as u32, pre, rate, mask: 1, screen: (-0.5e-3, 1e-3) };
    assert!(!fails(&by_time, &wave(0.0)), "clean signal");
    assert!(fails(&by_time, &wave(10e-6)), "10 us shift");
    assert!(fails(&by_time, &wave(-10e-6)), "-10 us shift");
    let over_record = mask::Timing { screen: (-(pre as f64) * dt, n as f64 * dt), ..by_time };
    assert!(fails(&over_record, &wave(0.0)), "the old mapping fails the clean signal");
}

#[test]
fn a_stalling_unit_is_dropped_for_its_settings() {
    let map = oos_hal::dho::mask::Map::default();
    let s = Settings::default();
    let k = unit_key(&s, map);
    let other = unit_key(&Settings { depth: 100_000, ..s.clone() }, map);
    let mut g = UnitGuard::default();
    // a stall without the unit, or a good batch in between, does not count
    assert!(!g.batch(true, false, k) && !g.batch(true, true, k) && !g.batch(false, true, k) && !g.batch(true, true, k));
    assert!(!g.blocks(&k));
    // two in a row with the unit running: blocked for these settings only
    assert!(g.batch(true, true, k));
    assert!(g.blocks(&k) && !g.blocks(&other));
    // another mapping tries again
    let swept = unit_key(&s, oos_hal::dho::mask::Map { layout: oos_hal::dho::mask::Layout::Clocks, ..map });
    assert!(!g.blocks(&swept));
}

#[test]
fn mask_and_zone_merge() {
    use crate::board::{MaskWant, ZoneWant};
    let mw = MaskWant { ch: 0, lo: vec![-1.0; 1000], hi: vec![1.0; 1000], stop: false, clear: 0 };
    let zw = ZoneWant {
        ch: 1,
        zones: vec![(vec![(0.0, 0.0), (1e-6, 1.0)], true), (vec![(0.0, 2.0), (1e-6, 3.0)], false), (vec![], true)],
        clear: 4,
    };
    let pf = auxout::AuxOut::PassFail(auxout::PassFail::DEFAULT);
    let mut s = Settings { aux: pf, ..Settings::default() };
    hw_merge(&mut s, None, Some(&zw), oos_hal::dho::mask::Map::default());
    let z = s.hwzone.as_ref().unwrap();
    assert_eq!((z.ch, z.zones.len(), z.zones[1].inside, z.clear), (1, 2, false, 4));
    assert_eq!(
        s.aux,
        auxout::AuxOut::PassFail(auxout::PassFail { source: auxout::Source::Zone, ..auxout::PassFail::DEFAULT })
    );
    // the mask test wins and keeps the mask source
    let mut s = Settings { aux: pf, ..Settings::default() };
    hw_merge(&mut s, Some(&mw), Some(&zw), oos_hal::dho::mask::Map::default());
    assert!(s.hwmask.is_some() && s.hwzone.is_none() && s.aux == pf);
    let mut s = Settings::default();
    hw_merge(&mut s, None, None, oos_hal::dho::mask::Map::default());
    assert!(s.hwmask.is_none() && s.hwzone.is_none() && s.aux == auxout::AuxOut::TriggerOut);
}

/// The software mask in the hardware table, and the comparator modelled on the read-out words (the assumed sample
/// scale, `mask::code`): what the software passes the table passes, a runt fails both.
#[test]
fn hardware_table_agrees_with_the_software_mask() {
    use oos_hal::dho::mask;
    let (scale, offset) = (1.0, -1.5);
    let n = 20_000;
    // 5 periods of a 0..3 V square with a little noise, and a runt in period 3
    let wave = |runt: bool, seed: u32| -> Vec<f32> {
        (0..n)
            .map(|k| {
                let ph = k % 4000;
                let hi = if runt && k / 4000 == 3 { 1.2 } else { 3.0 };
                let noise =
                    (((k as u32).wrapping_mul(2_654_435_761).wrapping_add(seed) >> 16) as f32 / 65536.0 - 0.5) * 0.04;
                (if ph < 2000 { hi } else { 0.0 }) + noise
            })
            .collect()
    };
    let m = oos_dsp::mask::build(&wave(false, 1), 20, 0.2);
    let (lo, hi) = m.columns(mask::COLUMNS);
    // the record is the screen here: 20,000 samples at 10 MSa/s around the trigger
    let t = mask::Timing { points: n as u32, pre: n as u32 / 2, rate: 10_000_000, mask: 1, screen: (-1e-3, 2e-3) };
    let cols = mask::table_map(&lo, &hi, &t, scale, offset, 1.0, &mask::Map::default());
    // the read-out word of a sample (inverse of `seq::decode_volts`), and the 8-bit comparator on it
    let fails = |v: &[f32]| -> bool {
        v.iter().enumerate().any(|(k, &x)| {
            let w = (32768.0 + (x as f64 + offset) / scale * 7500.0).round().clamp(0.0, 65535.0) as u32;
            let c = (w >> mask::SHIFT) as u8;
            let col = cols[k * mask::COLUMNS / n];
            (col.lo_en && c < col.lo) || (col.hi_en && c > col.hi)
        })
    };
    for seed in 2..8 {
        let v = wave(false, seed);
        assert!(m.test(&v).pass && !fails(&v), "seed {}", seed);
    }
    let v = wave(true, 9);
    assert!(!m.test(&v).pass && fails(&v));
}

#[test]
fn dvm_steps() {
    let t0 = Some((1, 2, 7));
    let ch = |c: usize| Some(crate::board::DvmWant { ch: c, lane: None });
    // (want, mask, config, running, elapsed) -> (step, running after)
    type Case = (Option<crate::board::DvmWant>, u32, u64, Option<DvmKey>, f64, DvmStep, Option<DvmKey>);
    let cases: [Case; 9] = [
        (None, 0b11, 7, None, 0.0, DvmStep::Idle, None),
        // CH2 of CH1+CH2: lane 2
        (ch(1), 0b11, 7, None, 0.0, DvmStep::Start(2), t0),
        (ch(1), 0b11, 7, t0, 0.1, DvmStep::Idle, t0),
        (ch(1), 0b11, 7, t0, 0.6, DvmStep::Read, t0),
        // a new configuration restarts it
        (ch(1), 0b11, 8, t0, 0.1, DvmStep::Start(2), Some((1, 2, 8))),
        // CH2 alone: lane 1; with lane 3 forced: lane 3
        (ch(1), 0b10, 7, t0, 0.6, DvmStep::Start(1), Some((1, 1, 7))),
        (
            Some(crate::board::DvmWant { ch: 1, lane: Some(3) }),
            0b10,
            7,
            Some((1, 1, 7)),
            0.6,
            DvmStep::Start(3),
            Some((1, 3, 7)),
        ),
        // the channel left the acquired set, or the DVM was switched off
        (ch(1), 0b01, 7, t0, 0.6, DvmStep::Stop, None),
        (None, 0b11, 7, t0, 0.6, DvmStep::Stop, None),
    ];
    for (want, mask, config, running, el, step, after) in cases {
        assert_eq!(
            dvm_step(want, mask, config, running, el),
            (step, after),
            "{:?} {:#b} {} {:?} {}",
            want,
            mask,
            config,
            running,
            el
        );
    }
}

// Break it catches: the kept offsets applied with the key off (no full undo), or read at all then.
#[test]
fn overlay_only_with_the_key_on() {
    let mut file = oos_hal::dho::coretrim::Overlay::default();
    file.set(2, &[1; 8]);
    assert_eq!(overlay_in_use(false, || panic!("read with the key off")), oos_hal::dho::coretrim::Overlay::default());
    assert_eq!(overlay_in_use(true, || Some(file.clone())), file);
    assert_eq!(overlay_in_use(true, || None).modes, 0);
}

// Issue #5 on the unit (2026-11-16): frame 1 now and then has its rising edge about 90 samples before the trigger
// point, or none. Break it catches: such a frame kept, a good frame or one of another trigger type left out.
#[test]
fn frame1_without_its_edge_is_found() {
    let s = Settings { level: 0.0, ..Settings::default() };
    let frame = |edge: usize| {
        let mut f = Frame { pre: 500, ..Frame::default() };
        f.ch[0] = (0..1000).map(|i| if i >= edge { 1.0 } else { -1.0 }).collect();
        f
    };
    // the hardware's edge, a few samples after pre
    assert!(!frame1_off_trigger(&frame(506), &s));
    // the unit's frame 1: the edge 90 samples early, or none at all (a level the whole record)
    assert!(frame1_off_trigger(&frame(410), &s));
    assert!(frame1_off_trigger(&frame(2000), &s));
    // falling slope on a rising edge: off; either: on
    assert!(frame1_off_trigger(&frame(506), &Settings { slope: oos_hal::dho::tpu::Slope::Falling, ..s.clone() }));
    assert!(!frame1_off_trigger(&frame(506), &Settings { slope: oos_hal::dho::tpu::Slope::Either, ..s.clone() }));
    // noise around the level, under a fifth of a division: no edge, the frame is off
    let mut flat = frame(2000);
    flat.ch[0] = (0..1000).map(|i| if i % 2 == 0 { 0.05 } else { -0.05 }).collect();
    assert!(frame1_off_trigger(&flat, &s));
    // what cannot be checked stays: another trigger type, AC coupling, min/max pairs, an LA line as source
    let timeout = Settings { trigger: oos_hal::dho::seq::Trig::Timeout { time: 1e-6 }, ..s.clone() };
    assert!(!frame1_off_trigger(&frame(410), &timeout));
    assert!(!frame1_off_trigger(&frame(410), &Settings { trig_couple: oos_hal::dho::tpu::Coupling::Ac, ..s.clone() }));
    assert!(!frame1_off_trigger(&Frame { envelope: true, ..frame(410) }, &s));
    assert!(!frame1_off_trigger(&frame(410), &Settings { trig_src: 9, ..s.clone() }));
}

// Issue #220 on the unit (2026-11-18): at 1 ms/div a batch holds the held frame 0 and one more, so leaving frame 0
// out unseen put 18 of 37 records a second in the picture. The rule: frame 0 of a small batch is kept only when its
// edge check finds the edge at the trigger point; it is left out when the check cannot judge it, finds it off, or was
// not run, and always in a long batch (the rule of #5 as verified at 10 us/div). Breaks it catches: either side of
// that rule turned around, and the extra decode for a batch of one or a long one.
#[test]
fn frame0_of_a_small_batch_is_kept_at_the_trigger_point() {
    let s = Settings { level: 0.0, ..Settings::default() };
    let frame = |edge: usize| {
        let mut f = Frame { pre: 500, ..Frame::default() };
        f.ch[0] = (0..1000).map(|i| if i >= edge { 1.0 } else { -1.0 }).collect();
        f
    };
    assert_eq!(edge_check(&frame(506), &s), Edge::At);
    assert_eq!(edge_check(&frame(410), &s), Edge::Off);
    let timeout = Settings { trigger: oos_hal::dho::seq::Trig::Timeout { time: 1e-6 }, ..s.clone() };
    assert_eq!(edge_check(&frame(410), &timeout), Edge::Unchecked);
    // 1 ms/div, 100 kpts: a batch of two, frame 0 at the trigger point is kept
    assert!(frame0_check_wanted(2, true, 100_000));
    assert!(!frame0_left_out(2, true, Some(Edge::At)));
    for small in 2..=FRAME0_CHECK_MAX_FRAMES {
        assert!(frame0_left_out(small, true, Some(Edge::Off)), "{} frames, off the trigger", small);
        assert!(frame0_left_out(small, true, Some(Edge::Unchecked)), "{} frames, a trigger not checked", small);
        assert!(frame0_left_out(small, true, None), "{} frames, no check", small);
    }
    // a long batch (10 us/div: 26 to 32 frames) leaves it out whatever the check, and does not decode it for one
    let long = FRAME0_CHECK_MAX_FRAMES + 1;
    assert!(frame0_left_out(long, true, Some(Edge::At)));
    assert!(!frame0_check_wanted(long, true, 1000));
    // a batch of one keeps its only frame; `acq.frame0 = keep` keeps it everywhere, unchecked
    assert!(!frame0_left_out(1, true, None) && !frame0_check_wanted(1, true, 1000));
    assert!(!frame0_left_out(2, false, None) && !frame0_left_out(long, false, None));
    assert!(!frame0_check_wanted(2, false, 1000));
    // a record too long for the extra decode: left out unseen, as before
    assert!(!frame0_check_wanted(2, true, FRAME1_CHECK_MAX + 1));
}

// Round 15: a trim right after the start-up mode switches read a settling front end. Break it catches: a trim
// that is due before the configuration has stood for TRIM_HOLD.
#[test]
fn core_trim_waits_for_a_standing_configuration() {
    let t0 = std::time::Instant::now();
    let p = TrimPending { mode: 2, mask: 0b100, since: t0 };
    assert!(!p.due(t0) && !p.due(t0 + TRIM_HOLD - std::time::Duration::from_millis(1)));
    assert!(p.due(t0 + TRIM_HOLD) && TRIM_HOLD >= std::time::Duration::from_secs(1));
}

#[test]
fn slow_modes_follow_auto_roll() {
    let mut m = Model::default();
    let at = |m: &mut Model, t: f64| {
        m.tscale = tscales().iter().position(|&x| (x as f64 - t).abs() < t * 1e-3).unwrap();
        slow_mode(m)
    };
    assert_eq!(at(&mut m, 0.1), Slow::Off);
    assert_eq!(at(&mut m, 0.2), Slow::Roll);
    m.auto_roll = false;
    assert_eq!(at(&mut m, 0.1), Slow::Off);
    assert_eq!(at(&mut m, 0.2), Slow::Scan { delay: 0.0, single: false });
    m.trig_mode = TrigMode::Single;
    assert_eq!(at(&mut m, 0.5), Slow::Scan { delay: 0.0, single: true });
    // averaging: no scan
    m.acq = AcqMode::Average;
    assert_eq!(at(&mut m, 0.5), Slow::Off);
    // roll and scan stream with the stock Auto depth
    m.acq = AcqMode::Normal;
    assert_eq!(settings(&m, false).depth, 0);
}

/// STOP reads the stored record again for each view: zoomed in every sample of the part on screen (also far from
/// the trigger, which then lies outside the frame), zoomed out the whole record compressed; and the frame read
/// that way puts the trigger where the record had it.
#[test]
fn stop_reads_the_part_of_the_record_the_view_shows() {
    let m = Model {
        tscale: tscales().iter().position(|&t| (t as f64 - 1e-7).abs() < 1e-12).unwrap(),
        mdepth: mdepths().iter().position(|d| d.1 == 10_000_000).unwrap(),
        ..Default::default()
    };
    let s = settings(&m, false);
    let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, oos_hal::dho::seq::tdiv_fs(s.delay));
    let b = batch_plan(&s, &p);
    let rate = p.rate as f64;
    // as shown at the stop: a window around the trigger, every sample, the trigger inside
    let (r, ti) = stop_plan(&b, &p, 1e-7, 0.0);
    let (n, start) = oos_hal::dho::seq::window(&r, &p);
    assert_eq!((r.compress, r.frames), (1, 1));
    assert_eq!(ti, p.pre as f64 - start as f64);
    assert!(ti > 0.0 && ti < n as f64 && (n as f64) < 20_000.0, "{} {}", ti, n);
    // 1 us/div 3 ms after the trigger: the window there, the trigger far before it
    let (r, ti) = stop_plan(&b, &p, 1e-6, 3e-3);
    let (n, start) = oos_hal::dho::seq::window(&r, &p);
    assert_eq!(r.compress, 1);
    let centre = start as f64 + n as f64 / 2.0;
    assert!((centre - (p.pre as f64 + 3e-3 * rate)).abs() < 16.0, "{} {}", centre, p.pre);
    assert!((ti + 3e-3 * rate - n as f64 / 2.0).abs() < 16.0, "{}", ti);
    // 1 ms/div: the whole 8 ms record through the compression, the trigger at its point / k
    let (r, ti) = stop_plan(&b, &p, 1e-3, 0.0);
    assert!(r.compress > 1 && r.window.is_none());
    assert_eq!(ti, p.pre as f64 / r.compress as f64);
    // a frame of that read (a square with its edge at the trigger) becomes a record with the edge at 0 s
    let k = r.compress as usize;
    let len = (p.memsel as usize).div_ceil(k);
    let v: Vec<f32> = (0..len).map(|i| if (i as f64) < ti { -1.0 } else { 1.0 }).collect();
    let f = Frame {
        seq: 1,
        dt: k as f64 / rate,
        pre: 0,
        ch: [v, Vec::new(), Vec::new(), Vec::new()],
        envelope: true,
        acq: (rate, p.memsel as usize),
        fixed_ti: Some(ti),
        ..Default::default()
    };
    let rec = to_record(&Arc::new(f), &m, 3600, 10e-3, -5e-3);
    assert!(rec.triggered);
    let at = |t: f64| rec.ch[0][((t + 5e-3) / rec.dt) as usize];
    assert!(at(-1e-4) < 0.0 && at(1e-4) > 0.0, "{} {}", at(-1e-4), at(1e-4));
}

#[test]
fn scan_picture_wraps_and_fills_the_screen() {
    let mut pic = Vec::new();
    scan_write(&mut pic, 10, 0, &[1.0; 4]);
    scan_write(&mut pic, 10, 8, &[2.0; 4]); // wraps: 8, 9, 0, 1
    assert_eq!(&pic[..4], &[2.0, 2.0, 1.0, 1.0]);
    assert!(pic[4].is_nan() && pic[9] == 2.0);
    let mut ch: [Vec<f32>; NCH] = [pic, Vec::new(), Vec::new(), Vec::new()];
    scan_gap(&mut ch, 9, 2);
    assert!(ch[0][9].is_nan() && ch[0][0].is_nan() && ch[0][1] == 2.0);
    // a scan frame of 1000 points spans the screen: point 0 left, point 999 right
    let m = Model::default();
    let v: Vec<f32> = (0..1000).map(|i| i as f32).collect();
    let f = Frame {
        seq: 1,
        dt: 1e-3,
        pre: 0,
        ch: [v, Vec::new(), Vec::new(), Vec::new()],
        roll: false,
        envelope: true,
        scan: true,
        partial: true,
        acq: (0.0, 0),
        stamp: 0,
        la: None,
        fixed_ti: None,
        hw_avg: false,
        basic: Default::default(),
    };
    let r = to_record(&Arc::new(f), &m, 100, 1.0, -0.5);
    assert!(r.partial && !r.triggered);
    assert!(r.ch[0][0] < 10.0 && r.ch[0][99] > 980.0, "{} {}", r.ch[0][0], r.ch[0][99]);
}

#[test]
fn scan_writes_the_la_where_the_analog_point_is() {
    // 25 000 analog points and 100 000 LA words over the screen
    assert_eq!(scan_la_pos(0, 25_000, 100_000), 0);
    assert_eq!(scan_la_pos(12_500, 25_000, 100_000), 50_000);
    assert_eq!(scan_la_pos(24_999, 25_000, 100_000), 99_996);
}

#[test]
fn short_time_bases_read_beyond_the_screen() {
    let mut m = Model {
        tscale: tscales().iter().position(|&x| (x as f64 - 1e-7).abs() < 1e-10).unwrap(),
        ..Default::default()
    };
    // 100 ns/div, one channel: 1250 points per screen at 1.25 GSa/s -> 1408 at the same rate
    let s = settings(&m, false);
    assert_eq!(s.depth, 1408);
    let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(1e-7), s.depth, 0);
    assert_eq!((p.rate, p.memsel, p.pre + p.post), (1_250_000_000, 1408, 1408));
    assert_eq!(p.pre, 704);
    // the smallest records get at least 64 more points
    assert_eq!(screen_margin(62), 128);
    // 1 ns/div: 12 points per screen, read 80 at the full rate (more than stock's smallest record, 62 at 5 ns)
    m.tscale = tscales().iter().position(|&t| (t - 1e-9).abs() < 1e-12).unwrap();
    assert!((tscales()[m.tscale] - 1e-9).abs() < 1e-12);
    let s = settings(&m, false);
    let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(1e-9), s.depth, 0);
    assert_eq!((p.rate, p.memsel, p.pre), (1_250_000_000, 80, 40), "depth {}", s.depth);
}

// Break it catches: live compression where the model needs every sample, a factor that leaves fewer than two
// pairs per column, compression above 1 M points (the deep-memory read), a batch that does not grow with it.
#[test]
fn live_compression_plan_by_time_base() {
    let plan = |tdiv: f64, two: bool| {
        let mut m = Model {
            tscale: tscales().iter().position(|&t| (t as f64 - tdiv).abs() < tdiv * 1e-6).unwrap(),
            ..Default::default()
        };
        m.ch[1].on = two;
        let mut s = settings(&m, false);
        let raw = s.clone();
        live_compress(&m, &mut s);
        let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, 0);
        let (b, r) = (batch_plan(&s, &p), batch_plan(&raw, &p));
        (b.compress, oos_hal::dho::seq::frame_bytes(&b, &p), b.frames, r.frames, p)
    };
    // every sample at 100 ns/div
    assert_eq!(plan(100e-9, false).0, 1);
    for (tdiv, two) in [(1e-6, false), (2e-6, false), (10e-6, false), (100e-6, false), (10e-6, true), (1e-3, false)] {
        let (k, fb, frames, raw_frames, p) = plan(tdiv, two);
        let per_column = p.rate as f64 * tdiv * 10.0 / crate::compress::SCREEN_COLUMNS;
        assert!(k >= crate::compress::K_MIN, "{} s/div: k {}", tdiv, k);
        // at least POINTS_PER_COLUMN points per column, less than twice that
        let points = per_column / k as f64;
        assert!(
            (crate::compress::POINTS_PER_COLUMN..2.0 * crate::compress::POINTS_PER_COLUMN).contains(&points),
            "{} s/div: {} points per column",
            tdiv,
            points
        );
        assert_eq!(
            fb,
            2 * p.ratio as usize
                * oos_hal::dho::seq::compressed_len(&Settings { compress: k, ..Settings::default() }, &p) as usize
        );
        assert!(frames >= raw_frames, "{} s/div: {} vs {} frames", tdiv, frames, raw_frames);
        assert!(raw_newest(&Settings { compress: k, ..Settings::default() }, &p));
    }
    // 10 us/div, one channel: 100 k points at 625 MSa/s (the Auto depth cap), 200 kB raw per frame become 13 kB,
    // 20 frames per batch become 300 (then the frame ring, 448, is the limit)
    let (k, fb, frames, raw_frames, p) = plan(10e-6, false);
    assert_eq!((p.rate, p.memsel, k, raw_frames), (625_000_000, 100_000, 15, 20), "{:?}", p);
    assert_eq!((fb, frames), (13_328, 300));
    // the model needs every sample
    let mut m = Model {
        tscale: tscales().iter().position(|&t| (t as f64 - 10e-6).abs() < 1e-11).unwrap(),
        trig_mode: TrigMode::Single,
        ..Default::default()
    };
    let mut s = settings(&m, false);
    live_compress(&m, &mut s);
    assert_eq!(s.compress, 1);
    // deep records keep the deep-memory read
    m.trig_mode = TrigMode::Auto;
    m.mdepth = mdepths().iter().position(|d| d.1 == 10_000_000).unwrap();
    let mut s = settings(&m, false);
    live_compress(&m, &mut s);
    assert_eq!(s.compress, 1);
}

// Break it catches: with compression the unit default, a mode that must see every sample (or the records as raw
// samples) getting min/max pairs. Every row must leave the frames raw; the base set-up must compress, so the table
// tests something. STOP: the record left on screen is a raw final frame (`ui::tests::
// live_pair_records_keep_the_raw_measurement_and_stop_takes_a_raw_frame`); measurements and cursors get raw
// records (`compress::hold`).
#[test]
fn modes_that_need_raw_frames_never_compress() {
    use crate::math::MathOp;
    let base = || {
        let m = Model {
            tscale: tscales().iter().position(|&t| (t as f64 - 10e-6).abs() < 1e-11).unwrap(),
            ..Default::default()
        };
        m
    };
    let k = |m: &Model, needs_raw: bool| {
        let mut s = settings(m, false);
        live_compress(m, &mut s);
        raw_for_ui(&mut s, needs_raw);
        s.compress
    };
    assert!(k(&base(), false) > 1, "the base set-up compresses");
    let zone = crate::zone::Zone { on: true, src: 0, inside: true, pts: vec![(0.0, 0.0), (1e-6, 1.0), (2e-6, 0.0)] };
    type Row = (&'static str, Box<dyn Fn(&mut Model)>);
    let rows: Vec<Row> = vec![
        ("Single", Box::new(|m| m.trig_mode = TrigMode::Single)),
        ("zoom", Box::new(|m| m.zoom = true)),
        ("FFT math channel", Box::new(|m| m.maths[0].op = MathOp::Fft)),
        ("math channel", Box::new(|m| m.maths[1].op = MathOp::Add)),
        ("math (first page)", Box::new(|m| m.math = MathOp::Sub)),
        ("protocol decode", Box::new(|m| m.dec_on = true)),
        ("spectrum view", Box::new(|m| m.view = AView::Spectrum)),
        ("eye view", Box::new(|m| m.view = AView::Eye)),
        ("power view", Box::new(|m| m.view = AView::Power)),
        ("Bode view", Box::new(|m| m.view = AView::Bode)),
        ("step view", Box::new(|m| m.view = AView::Step)),
        ("mask", Box::new(|m| m.mask_on = true)),
        ("zone", Box::new(move |m| m.zones[0] = zone.clone())),
        ("Peak", Box::new(|m| m.acq = AcqMode::Peak)),
        ("Average", Box::new(|m| m.acq = AcqMode::Average)),
        ("High res", Box::new(|m| m.acq = AcqMode::HiRes)),
        ("channel AC", Box::new(|m| m.ch[0].coupling = Coupling::Ac)),
        ("20 MHz limit", Box::new(|m| m.ch[0].bw = BW_20M)),
        ("trigger AC", Box::new(|m| m.trig_coup = TrigCoup::Ac)),
        ("trigger LF reject", Box::new(|m| m.trig_coup = TrigCoup::LfReject)),
        ("trigger HF reject", Box::new(|m| m.trig_coup = TrigCoup::HfReject)),
        ("pulse trigger", Box::new(|m| m.trig_type = TrigType::Pulse)),
        ("segments", Box::new(|m| m.segments = 8)),
        (
            "roll",
            Box::new(|m| {
                m.tscale = tscales().iter().position(|&t| t as f64 >= ROLL_TDIV).unwrap();
                m.auto_roll = true;
            }),
        ),
        (
            "scan",
            Box::new(|m| {
                m.tscale = tscales().iter().position(|&t| t as f64 >= SCAN_TDIV).unwrap();
                m.auto_roll = false;
            }),
        ),
        ("Z input", Box::new(|m| m.z_src = Some(1))),
    ];
    for (name, f) in &rows {
        let mut m = base();
        f(&mut m);
        assert_eq!(k(&m, false), 1, "{}", name);
    }
    // the readers only the UI knows of (protocol watch, XY): `Shared::needs_raw`
    assert_eq!(k(&base(), true), 1, "protocol watch / XY");
    // the LA reads its own raw window of the record (only where the platform has one)
    let mut m = base();
    m.la.on = true;
    if m.la_on() {
        assert_eq!(k(&m, false), 1, "LA");
    }
}

// Break it catches (docs/fork/stop-exact.md): a replay at STOP of a frame already raw, of a ring armed again, without
// the deep measurements, or of a batch that was not read through the compression
#[test]
fn final_replay_at_stop() {
    let mut m = Model {
        tscale: tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap(),
        mdepth: mdepths().iter().position(|d| d.1 == 10_000_000).unwrap(),
        ..Default::default()
    };
    let s = settings(&m, false);
    let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, oos_hal::dho::seq::tdiv_fs(s.delay));
    let b = batch_plan(&s, &p);
    let w = Some(crate::deepmeas::Want { every: false });
    assert_eq!(final_replay(&b, &p, 3, false, false, w), deep_raw_settings(&b, &p));
    assert!(final_replay(&b, &p, 3, false, false, w).is_some());
    assert!(final_replay(&b, &p, 3, true, false, w).is_none(), "newest frame already raw");
    assert!(final_replay(&b, &p, 3, false, true, w).is_none(), "armed ahead");
    assert!(final_replay(&b, &p, 3, false, false, None).is_none(), "no deep measurements");
    assert!(final_replay(&b, &p, 0, false, false, w).is_none());
    assert!(final_replay(&Settings { compress: 1, ..b.clone() }, &p, 3, false, false, w).is_none(), "read raw");
    // prearm on (the unit's set-up): no arming ahead for this batch, so the replay at STOP stays possible
    assert!(!prearm_keeps_replay(&b, &p, w));
    let ahead = prearm_keeps_replay(&b, &p, w);
    assert!(final_replay(&b, &p, 3, false, ahead, w).is_some());
    // prearm stays where no deep replay can come: without the deep measurements, a shallow record
    assert!(prearm_keeps_replay(&b, &p, None));
    m.mdepth = 0;
    m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-6).abs() < 1e-12).unwrap();
    let s = settings(&m, false);
    let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, 0);
    assert!(prearm_keeps_replay(&batch_plan(&s, &p), &p, w));
    // the live compression at 10 us/div: the whole newest frame raw at STOP, with or without deep measurements;
    // no arming ahead for its batches
    m.tscale = tscales().iter().position(|&t| (t as f64 - 10e-6).abs() < 1e-11).unwrap();
    let mut s = settings(&m, false);
    live_compress(&m, &mut s);
    let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, 0);
    let b = batch_plan(&s, &p);
    assert!(b.compress > 1 && raw_newest(&b, &p));
    let r = final_replay(&b, &p, 3, false, false, None).expect("raw newest at STOP");
    assert_eq!((r.compress, r.window), (1, None));
    assert!(final_replay(&b, &p, 3, true, false, None).is_none(), "newest frame already raw");
    assert!(!prearm_keeps_replay(&b, &p, None));
}

// Break it catches: a raw deep read of the wrong part of the record (screen or trigger point outside it), one
// beyond the budget, one where the batch was already raw or of the live compression, or in Peak.
#[test]
fn deep_raw_window_for_the_measurements() {
    let plan = |tdiv: f64, depth: u64, two: bool| {
        let mut m = Model {
            tscale: tscales().iter().position(|&t| (t as f64 - tdiv).abs() < tdiv * 1e-6).unwrap(),
            mdepth: mdepths().iter().position(|d| d.1 == depth).unwrap(),
            ..Default::default()
        };
        m.ch[1].on = two;
        let s = settings(&m, false);
        let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, oos_hal::dho::seq::tdiv_fs(s.delay));
        let b = batch_plan(&s, &p);
        (b.clone(), p, deep_raw_settings(&b, &p))
    };
    // 10 M at 1 ms/div, CH1: the batch reads pairs, the measurement window raw: the whole record, 20 MB
    let (b, p, d) = plan(1e-3, 10_000_000, false);
    assert!(b.compress > 1 && b.window.is_none());
    let d = d.expect("raw window");
    assert_eq!(d.compress, 1);
    let (n, start) = oos_hal::dho::seq::window(&d, &p);
    let screen = (b.tdiv * 10.0 * p.rate as f64) as u32;
    assert!(
        start <= (p.memsel / 2).saturating_sub(screen / 2) && start + n >= (p.memsel / 2 + screen / 2).min(p.memsel),
        "{} + {} of {}",
        start,
        n,
        p.memsel
    );
    assert!(start < p.pre && start + n > p.pre);
    let bytes = oos_hal::dho::seq::frame_bytes(&d, &p);
    assert!(bytes <= crate::deepmeas::MAX_BYTES, "{} bytes", bytes);
    // the decoded frame is raw and knows its trigger point
    let f = decode(&vec![0u8; bytes], &d, &p, 1);
    assert!(!f.envelope && f.ch[0].len() == n as usize && f.pre == (p.pre - start) as usize);
    // 2 channels at 10 M: 40 MB, beyond the budget
    assert!(plan(1e-3, 10_000_000, true).2.is_none());
    // 25 M at 2 ms/div: 50 MB
    assert!(plan(2e-3, 25_000_000, false).2.is_none());
    // a deep record at a short time base is read raw already (screen window), 1 M is the live path
    assert!(plan(1e-7, 10_000_000, false).2.is_none());
    assert!(plan(1e-3, 1_000_000, false).2.is_none());
    // Peak: the memory holds peak pairs
    let (b, p, _) = plan(1e-3, 10_000_000, false);
    assert!(deep_raw_settings(&Settings { mode: oos_hal::dho::acq::Mode::Peak, ..b }, &p).is_none());
}

#[test]
fn a_deep_record_at_a_short_time_base_reads_the_screen_window() {
    // 10 M at 100 ns/div, CH1: 1.25 GSa/s, the screen is 1250 of the 10 M points
    let mut m = Model {
        tscale: tscales().iter().position(|&t| (t as f64 - 1e-7).abs() < 1e-12).unwrap(),
        mdepth: mdepths().iter().position(|d| d.1 == 10_000_000).unwrap(),
        ..Default::default()
    };
    let s = settings(&m, false);
    let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, oos_hal::dho::seq::tdiv_fs(s.delay));
    let b = batch_plan(&s, &p);
    assert_eq!(b.compress, 1, "every sample, not ~80 min/max points for the screen");
    let (n, start) = oos_hal::dho::seq::window(&b, &p);
    assert!(n < 20_000 && n % 8 == 0, "{}", n);
    // the screen and the trigger point are inside, and the frame's trigger point counts from the window
    let screen = (s.tdiv * 10.0 * p.rate as f64) as u32;
    assert!(start <= p.memsel / 2 - screen / 2 && start + n >= p.memsel / 2 + screen / 2);
    assert!(start < p.pre && start + n > p.pre);
    let bytes = vec![0u8; seq_bytes(&b, &p)];
    let f = decode(&bytes, &b, &p, 1);
    assert_eq!(f.ch[0].len(), n as usize);
    assert_eq!(f.pre, (p.pre - start) as usize);
    // a long time base keeps the compressed whole record
    m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap();
    let s = settings(&m, false);
    let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, 0);
    let b = batch_plan(&s, &p);
    assert!(b.compress > 1 && b.window.is_none());
}

fn seq_bytes(b: &Settings, p: &hz::Params) -> usize {
    oos_hal::dho::seq::frame_bytes(b, p)
}

#[test]
fn depth_stays_within_the_limit_of_the_channels_on() {
    let mut m = Model {
        mdepth: mdepths().iter().position(|d| d.1 == 125_000_000).unwrap(),
        tscale: tscales().iter().position(|&t| t >= 1e-3).unwrap(),
        ..Default::default()
    };
    for c in 0..NCH {
        m.ch[c].on = c == 0;
    }
    assert_eq!(settings(&m, false).depth, 125_000_000);
    for c in 0..NCH {
        m.ch[c].on = true;
    }
    assert_eq!(settings(&m, false).depth, m.max_mem() as i128);
    assert_eq!(m.max_mem(), 31_250_000);
}

// Break it catches: hardware segments requested in roll or with the LA, a read-back batch larger than the UI queue
// or without the header that carries the stamp.
#[test]
fn hardware_segments_only_where_they_record() {
    let mut m = Model {
        tscale: tscales().iter().position(|&x| (x as f64 - 1e-7).abs() < 1e-10).unwrap(),
        ..Default::default()
    };
    (m.segments, m.seg_hw, m.seg_ivl) = (500, true, 0.0);
    let s = settings(&m, false);
    assert_eq!(s.segments, Some(Segments { n: 500, interval: 0.0 }));
    assert_eq!(seg_max(&m), Some(61_440));
    m.seg_hw = false;
    assert_eq!(settings(&m, false).segments, None);
    m.seg_hw = true;
    m.la.on = true;
    assert_eq!((settings(&m, false).segments, seg_max(&m)), (None, None));
    m.la.on = false;
    m.tscale = tscales().iter().position(|&x| x >= 0.2).unwrap();
    assert_eq!(settings(&m, false).segments, None);
    // read-back: header on, at most half the queue, ~4 MB
    for t in [1e-7, 1e-5, 1e-3] {
        m.tscale = tscales().iter().position(|&x| (x as f64 - t).abs() < t * 1e-3).unwrap();
        let s = settings(&m, false);
        let p = oos_hal::dho::seq::State::handover(s.mask, None).configure(&s, None, None).unwrap().1;
        let r = seg_read_plan(&s, &p);
        let fb = oos_hal::dho::seq::frame_bytes(&r, &p);
        assert!(r.header && r.frames >= 1 && r.frames as usize <= crate::board::QUEUE_MAX / 2);
        assert!(r.frames == 1 || r.frames as usize * fb <= 4_000_000, "{} x {}", r.frames, fb);
    }
}

#[test]
fn settings_from_the_model() {
    let mut m = Model::default();
    m.ch[1].on = true;
    let s = settings(&m, false);
    assert_eq!(s.mask, 3);
    assert_eq!(s.trig_src, 1);
    // 200 us/div with 2 channels: Auto would be 1.25 M points -> live depth 100 k
    assert_eq!(s.depth, LIVE_DEPTH);
    assert!(s.window.is_none());
    // edge: the level itself, even below level 2; runt: upper level and lower level
    m.trig_level = 0.2;
    assert_eq!(settings(&m, false).level, 0.2f32 as f64);
    m.trig_type = TrigType::Runt;
    m.trig_level2 = 1.8;
    let s = settings(&m, false);
    assert_eq!(s.level, 1.8f32 as f64);
    assert!(matches!(s.trigger, Trig::Runt { lo, .. } if (lo - 0.2).abs() < 1e-6));
}

// Break it catches: a live batch whose frames the FPGA pads in the stream (frame bytes not a multiple of 8), so
// every read moves the whole batch once more in `seq::unpad`. All time bases, depths and channel sets of the UI
// that read through the batch path (not roll or scan); with and without the LA.
#[test]
fn live_batches_need_no_stream_padding() {
    let mut m = Model::default();
    let (mut bad, mut checked, mut la_seen) = (Vec::new(), 0, false);
    for la in [false, true] {
        m.la.on = la;
        for chans in 1u32..16 {
            for c in 0..NCH {
                m.ch[c].on = chans & (1 << c) != 0;
            }
            for t in 0..tscales().len() {
                m.tscale = t;
                if slow_mode(&m) != Slow::Off {
                    continue;
                }
                for d in 0..mdepths().len() {
                    m.mdepth = d;
                    let s = settings(&m, false);
                    let Ok((_, p)) = oos_hal::dho::seq::State::handover(s.mask, None).configure(&s, None, None) else {
                        continue;
                    };
                    let b = batch_plan(&s, &p);
                    let fb = oos_hal::dho::seq::frame_bytes(&b, &p);
                    checked += 1;
                    la_seen |= s.la.is_some();
                    if !fb.is_multiple_of(8) {
                        bad.push((la, chans, tscales()[t], mdepths()[d].0, fb));
                    }
                }
            }
        }
    }
    assert!(checked > 1000 && la_seen, "{} set-ups checked, LA among them: {}", checked, la_seen);
    assert!(bad.is_empty(), "{} padded set-ups, e.g. {:?}", bad.len(), &bad[..bad.len().min(8)]);
}

#[test]
fn decode_and_align_a_square() {
    // a 2-channel record: CH1 square 0/2.5 V (1 kHz), CH2 constant; 100 k points at 625 MSa/s -> fake 1 MSa/s here
    let m = Model::default();
    let p = hz::Params {
        ratio: 2,
        rate: 1_000_000,
        memsel: 4000,
        dot_fs: 1_000_000_000,
        seg: 0,
        max_frames: 0,
        pre: 2000,
        post: 2000,
    };
    let s =
        Settings { mask: 3, ch: [Chan { scale: 1.0, bw: 0, ..Settings::default().ch[0] }; 4], ..Settings::default() };
    let mut bytes = Vec::new();
    for i in 0..4000usize {
        // rising edge of CH1 at sample 2003 (3 samples after the hardware trigger point)
        let v1 = if (i + 1000 - 3) % 1000 < 500 { 2.5 } else { 0.0 };
        for v in [v1, 0.5] {
            let w = (32768.0 + v * 7500.0) as u16;
            bytes.extend(w.to_le_bytes());
        }
    }
    let f = decode(&bytes, &s, &p, 1);
    assert_eq!(f.ch[0].len(), 4000);
    assert!((f.ch[1][10] - 0.5).abs() < 1e-3);
    let window = 1e-3;
    let r = to_record(&Arc::new(f), &m, 1000, window, -window / 2.0);
    assert!(r.triggered);
    // the software edge (level 1.5 V) sits at the centre of the screen window
    assert!(r.ch[0][495] < 0.5 && r.ch[0][505] > 2.0, "{} {}", r.ch[0][495], r.ch[0][505]);
}
