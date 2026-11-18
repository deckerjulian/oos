// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The tests of the acquisition sequences: the steps `State` builds for given settings (which registers, in which
//! order, when a shadow writes again), the frame geometry and the sample codec. A test of a new step or setting goes
//! here, next to the tests of its neighbours; the register facts it checks are described in docs/protocols/.

/// Holdoff writes only when one is set, and once more when it goes back.

// Break it catches: a code map that puts a channel at another word than the decode, or volts that differ from it
#[test]
fn code_map_reads_what_the_decode_reads() {
    let cal = [(1.0, 0.1), (0.5, -0.2), (2.0, 0.0), (0.2, 0.05)];
    for mask in 1u32..16 {
        let bytes: Vec<u8> = (0..4 * 37 + 3u32).flat_map(|i| ((30_000 + i * 97 % 5000) as u16).to_le_bytes()).collect();
        let d = decode_volts(&bytes, mask, cal);
        let m = code_map(mask, cal);
        for c in 0..4 {
            let Some(k) = m.word[c] else {
                assert!(d[c].is_empty(), "mask {} CH{}", mask, c + 1);
                continue;
            };
            let n = (bytes.len() / 2 + m.ratio - 1 - k) / m.ratio;
            let v: Vec<f32> = (0..n)
                .map(|i| {
                    volt_code(
                        u16::from_le_bytes([bytes[2 * (i * m.ratio + k)], bytes[2 * (i * m.ratio + k) + 1]]),
                        m.affine[c],
                    )
                })
                .collect();
            assert_eq!(v, d[c], "mask {} CH{}", mask, c + 1);
        }
    }
}

// Break it catches: an enable bit written with k = 0 (the read stalls), k beyond the 14-bit field (it wraps), a
// TX length that asks for samples beyond the record (the c2h read of k = 256 at 1 M timed out).
#[test]
fn compression_word_and_length_stay_inside_the_hardware() {
    assert_eq!(compress_word(0), 0x4000_0001);
    assert_eq!(compress_word(1), 0x4000_0001);
    assert_eq!(compress_word(4), 0xc000_0004);
    assert_eq!(compress_word(16383), 0xc000_3fff);
    assert_eq!(compress_word(16384), 0xc000_3fff);
    assert_eq!(compress_word(u32::MAX), 0xc000_3fff);
    for memsel in [1_000u32, 14_064, 62_504, 70_312, 100_000, 999_992, 1_000_000, 10_000_000, 125_000_000] {
        for k in [0u32, 1, 2, 3, 4, 15, 16, 25, 64, 128, 200, 256, 1000, 16383, 20000, u32::MAX] {
            let p = hz::Params {
                ratio: 1,
                rate: 1_250_000_000,
                memsel,
                dot_fs: 800_000,
                seg: 0,
                max_frames: 1,
                pre: memsel / 2,
                post: memsel / 2,
            };
            let s = Settings { compress: k, ..Settings::default() };
            let (kk, n) = (compress_k(&s, &p), compressed_len(&s, &p));
            assert!((1..=COMPRESS_K_MAX).contains(&kk) && n % 8 == 0 && n >= 8, "{} {}: k {} n {}", memsel, k, kk, n);
            assert!(n as u64 * kk as u64 <= memsel as u64, "{} at k {}: {} points x {}", memsel, k, n, kk);
            // at most 8 points (4 pairs) short of the record
            assert!(memsel as u64 - (n as u64 * kk as u64) < 8 * kk as u64, "{} at k {}", memsel, k);
        }
    }
    // the unit's cases: k 256 at 1 M asked 3912 points, now 3904; k 128 7816 -> 7808, k 4 250 000 unchanged
    let at = |k: u32| {
        compressed_len(
            &Settings { compress: k, ..Settings::default() },
            &hz::Params {
                ratio: 1,
                rate: 1_250_000_000,
                memsel: 1_000_000,
                dot_fs: 800_000,
                seg: 0,
                max_frames: 1,
                pre: 500_000,
                post: 500_000,
            },
        )
    };
    assert_eq!((at(256), at(128), at(4)), (3904, 7808, 250_000));
}
#[test]
fn the_afe_sees_the_bnc_behind_a_probe() {
    let dir = crate::dho::calfile::test_dir(false);
    let Ok(cal) = afe::Cal::load(&dir) else {
        eprintln!("calibration files not found, skipped");
        return;
    };
    let afe_ops = |k: Chan| {
        let s = Settings { afe: true, ch: [k; 4], ..Settings::default() };
        let (st, _) = plan(&mut State::handover(1, None), &s, Some(&cal), None).unwrap();
        st.iter().filter(|x| matches!(x, Step::Afe(_))).map(|x| format!("{:?}", x)).collect::<Vec<_>>()
    };
    let k = Settings::default().ch[0];
    let k = Chan { scale: 0.5, offset: 0.25, ..k };
    let tip = Chan { scale: 5.0, offset: 2.5, probe: 10.0, ..k };
    assert!(!afe_ops(k).is_empty());
    assert_eq!(afe_ops(k), afe_ops(tip));
    // the zero correction moves the AFE offset and nothing else
    assert_eq!(afe_ops(Chan { offset: 0.35, bias: 0.1, ..k }), afe_ops(k));
}

fn cal() -> Option<afe::Cal> {
    afe::Cal::load(&crate::dho::calfile::test_dir(false)).ok()
}

fn afe_writes(st: &[Step], reg: u8) -> Vec<u16> {
    st.iter()
        .filter_map(|x| {
            if let Step::Afe(afe::Op::Afe { reg: r, val, .. }) = x {
                (*r == reg).then_some(*val)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn coupling_goes_to_the_afe_and_gnd_to_the_fpga() {
    let Some(cal) = cal() else { return };
    let mut st = State::handover(1, None);
    let k = Settings::default().ch[0];
    let run = |st: &mut State, couple: Couple| {
        let s = Settings { afe: true, ch: [Chan { couple, offset: 0.5, ..k }; 4], ..Settings::default() };
        st.configure(&s, Some(&cal), None).unwrap().0
    };
    let dc = run(&mut st, Couple::Dc);
    assert!(afe_writes(&dc, 0x07).iter().all(|v| v & 0x8000 != 0));
    let ac = run(&mut st, Couple::Ac);
    assert_eq!(afe_writes(&ac, 0x07).last().unwrap() & 0x8000, 0);
    // GND leaves the AFE alone and flattens the channel in the FPGA: 0.5 V at 1 V/div
    let g = run(&mut st, Couple::Gnd);
    assert_eq!(afe_writes(&g, 0x07).last().unwrap() & 0x8000, 0);
    let w = writes(&g);
    assert!(w.contains(&(0x14c8, 0)) && w.contains(&(0x14d0, 32768 + 3750)), "{:?}", &w[..4]);
    assert!(afe_writes(&run(&mut st, Couple::Dc), 0x07).last().unwrap() & 0x8000 != 0);
}

#[test]
fn invert_is_done_in_the_fpga_with_a_mirrored_offset() {
    let Some(cal) = cal() else { return };
    let k = Settings::default().ch[0];
    let run = |invert: bool, offset: f64| {
        let s = Settings { afe: true, ch: [Chan { invert, offset, ..k }; 4], ..Settings::default() };
        State::handover(1, None).configure(&s, Some(&cal), None).unwrap().0
    };
    let (a, b) = (run(false, 0.5), run(true, -0.5));
    assert_eq!(afe_writes(&a, 0x28), afe_writes(&b, 0x28));
    let inv = |st: &[Step]| writes(st).iter().rev().find(|w| w.0 == 0x1004).unwrap().1 >> 16 & 0xf;
    assert_eq!((inv(&a), inv(&b)), (0, 0xf));
}

#[test]
fn peak_mode_delay_and_holdoff_reach_the_hardware() {
    let peak = |mode: acq::Mode, tdiv: f64| {
        let s = Settings { mode, tdiv, depth: 100_000, ..Settings::default() };
        let st = State::handover(1, None).configure(&s, None, None).unwrap().0;
        writes(&st).iter().rev().find(|w| w.0 == 0x1004).unwrap().1 & 2
    };
    assert_eq!((peak(acq::Mode::Normal, 1e-3), peak(acq::Mode::Peak, 1e-3)), (0, 2));
    // the record follows the delay, and stops at half a record
    let pre = |delay: f64| {
        let s = Settings { delay, depth: 100_000, tdiv: 1e-6, ..Settings::default() };
        State::handover(1, None).configure(&s, None, None).unwrap().1
    };
    let (a, b) = (pre(0.0), pre(3e-6));
    assert_eq!(a.pre as i64 - b.pre as i64, (3e-6 * a.rate as f64) as i64);
    assert_eq!((pre(1.0).pre, pre(-1.0).pre), (0, a.memsel));
    // the auto time waits out a long holdoff (trigger.md 2.6): 1 s are 250 M ticks of the 4 ns timer
    let t2 = |holdoff: f64| {
        let s = Settings { holdoff, ..Settings::default() };
        writes(&State::handover(1, None).configure(&s, None, None).unwrap().0)
            .iter()
            .rev()
            .find(|w| w.0 == 0x4064)
            .unwrap()
            .1
    };
    assert_eq!((t2(HOLDOFF_MIN), t2(1.0)), (25_000_000, 250_000_000));
}

// Break it catches: the auto set-up's fast auto trigger not reaching the SCU, so at 20 ms/div every record of a
// signal the level does not cross waits 400 ms (docs/fork/trigger-lockin.md)
#[test]
fn fast_auto_trigger_takes_the_shortest_times() {
    let t = |auto_fast: bool| {
        let s = Settings { tdiv: 20e-3, auto_fast, ..Settings::default() };
        let w = writes(&State::handover(1, None).configure(&s, None, None).unwrap().0);
        let last = |r: u32| w.iter().rev().find(|x| x.0 == r).unwrap().1;
        (last(0x4060), last(0x4064))
    };
    // 4 ns ticks: 400 ms / 200 ms at 20 ms/div, 10 ms / 100 ms fast
    assert_eq!(t(false), (100_000_000, 50_000_000));
    assert_eq!(t(true), (2_500_000, 25_000_000));
}

#[test]
fn hysteresis_follows_the_table_below_2_mv() {
    let h = |scale: f64, probe: f64| tpu::noise_hysteresis(scale / probe * 1e9, false);
    assert_eq!(
        (h(1.0, 1.0), h(0.002, 1.0), h(0.00204, 1.0), h(0.001, 1.0), h(0.0005, 1.0), h(0.0002, 1.0), h(0.0001, 1.0)),
        (0.3, 0.5, 0.3, 1.0, 1.25, 1.15, 2.3)
    );
    assert_eq!(h(0.01, 10.0), 1.0, "the BNC scale counts");
    assert_eq!(h(0.002f32 as f64, 1.0), 0.5, "through f32");
    assert_eq!(tpu::noise_hysteresis(1e9, true), 0.7);
}

#[test]
fn times_survive_f32() {
    assert_eq!(time_fs(1e-6f32 as f64), 1_000_000_000);
    assert_eq!(time_fs(5e-3f32 as f64), 5_000_000_000_000);
    assert_eq!(time_fs(200e-3f32 as f64), 200_000_000_000_000);
    assert_eq!(time_fs(-2e-6f32 as f64), -2_000_000_000);
    assert_eq!(time_fs(8e-9), 8_000_000);
    assert_eq!(time_fs(1.23456e-6), 1_234_560_000);
    assert_eq!(time_fs(0.0), 0);
}

#[test]
fn holdoff_is_written_when_it_matters() {
    let mut st = State::handover(1, None);
    let has = |w: &[crate::dho::Write]| w.iter().any(|x| x.0 == 0x2084);
    let base = Settings {
        trigger: Trig::Pulse { pol: tpu::Polarity::Pos, when: tpu::When::Gt, lower: 1e-6, upper: 2e-6 },
        ..Settings::default()
    };
    assert!(!has(&st.trigger(&base)));
    let on = Settings { holdoff: 1e-3, ..base.clone() };
    assert!(has(&st.trigger(&on)));
    assert!(has(&st.trigger(&base)), "back to none must reach the hardware");
    assert!(!has(&st.trigger(&base)));
}

use super::*;

fn writes(st: &[Step]) -> Vec<Write> {
    st.iter().filter_map(|s| if let Step::Wr(w) = s { Some(*w) } else { None }).collect()
}

#[test]
fn aux_out_is_written_once_and_on_change() {
    use auxout::{AuxOut, PassFail};
    let aux = |st: &mut State, s: &Settings| -> Vec<Write> {
        writes(&st.configure(s, None, None).unwrap().0)
            .into_iter()
            .filter(|w| w.0 == auxout::SELECT || w.0 == auxout::PASSFAIL)
            .collect()
    };
    let mut st = State::handover(1, None);
    let base = Settings::default();
    assert_eq!(aux(&mut st, &base), vec![(0x1430, 0)], "the first set-up states the power-up source");
    assert_eq!(aux(&mut st, &base), vec![]);
    let off = Settings { aux: AuxOut::Off, ..base.clone() };
    assert_eq!(aux(&mut st, &off), vec![(0x1430, 2)]);
    assert_eq!(aux(&mut st, &off), vec![]);
    let pf = Settings { aux: AuxOut::PassFail(PassFail::DEFAULT), ..base.clone() };
    assert_eq!(aux(&mut st, &pf), vec![(0x119c, 156), (0x1430, 3)]);
    assert_eq!(aux(&mut st, &base), vec![(0x1430, 0)]);
    // the diagnostic arm without the acquisition block leaves AUX OUT alone
    let mut st = State::handover(1, None);
    assert_eq!(aux(&mut st, &Settings { blocks: 5, ..off }), vec![]);
}

#[test]
fn hardware_mask_starts_reloads_clears_and_stops() {
    let only = |st: &mut State, s: &Settings| -> Vec<Write> {
        writes(&st.configure(s, None, None).unwrap().0)
            .into_iter()
            .filter(|w| matches!(w.0, 0x1184 | 0x1190 | 0x1194) || w.0 == 0x4020)
            .collect()
    };
    let band = |h: f32| HwMask {
        ch: 0,
        lo: vec![-h; mask::COLUMNS],
        hi: vec![h; mask::COLUMNS],
        stop_on_fail: false,
        clear: 0,
        map: mask::Map::default(),
    };
    let mut st = State::handover(1, None);
    let base = Settings { window: None, ..Settings::default() };
    let on = Settings { hwmask: Some(band(1.0)), ..base.clone() };
    let w = only(&mut st, &on);
    // off, 1000 columns + write enable dropped, compression for 2.5 Mpts, slot 0 with the counter clear, on; then
    // the auto-trigger block writes 0x4020 from the same shadow
    assert_eq!(w.len(), 1 + 1001 + 1 + 2 + 1 + 1, "{:x?}", &w[..4]);
    assert_eq!(w[0], (0x4020, 0x2001));
    assert_eq!(w[1], (0x1194, 0x1c00_0000 | 158 << 8 | 98));
    assert_eq!(w[1001], (0x1194, 0));
    assert_eq!(w[1002..1006], [(0x1184, 0xc000_03ff), (0x1190, 0x6010_0000), (0x1190, 0x0010_0000), (0x4020, 0xe001)]);
    assert_eq!(*w.last().unwrap(), (0x4020, 0xe001), "auto trigger keeps the mask bits");
    assert_eq!(st.mask_ctrl(), Some(0x0010_0000));
    // unchanged: nothing but the auto-trigger write
    assert_eq!(only(&mut st, &on), vec![(0x4020, 0xe001)]);
    // a tighter band: the table again, the counters keep counting
    let w = only(&mut st, &Settings { hwmask: Some(band(0.5)), ..base.clone() });
    assert_eq!(w.len(), 1 + 1001 + 1 + 1 + 1 + 1);
    assert_eq!(w[1003], (0x1190, 0x0010_0000));
    // a clear request: no table, the counters cleared
    let w = only(&mut st, &Settings { hwmask: Some(HwMask { clear: 1, ..band(0.5) }), ..base.clone() });
    assert_eq!(
        w,
        vec![
            (0x4020, 0x6001),
            (0x1184, 0xc000_03ff),
            (0x1190, 0x6010_0000),
            (0x1190, 0x0010_0000),
            (0x4020, 0xe001),
            (0x4020, 0xe001)
        ]
    );
    // stop on fail sets bit 12
    let w =
        only(&mut st, &Settings { hwmask: Some(HwMask { clear: 1, stop_on_fail: true, ..band(0.5) }), ..base.clone() });
    assert_eq!(*w.last().unwrap(), (0x4020, 0xf001));
    // off: enable and stop cleared, compression back, slot dropped
    let w = only(&mut st, &base);
    assert_eq!(w, vec![(0x4020, 0x6001), (0x1184, 0x4000_0001), (0x1190, 0), (0x4020, 0x6001)]);
    assert_eq!(st.mask_ctrl(), None);
    assert_eq!(only(&mut st, &base), vec![(0x4020, 0x6001)]);
    // a source that is not acquired leaves the test off
    let w = only(&mut st, &Settings { hwmask: Some(HwMask { ch: 1, ..band(1.0) }), ..base.clone() });
    assert_eq!(w, vec![(0x4020, 0x6001)]);
    // CH2 of CH1+CH2: slot 1 (stream slot)
    let two = Settings { mask: 0b11, hwmask: Some(HwMask { ch: 1, ..band(1.0) }), ..base.clone() };
    let mut st = State::handover(0b11, None);
    assert!(only(&mut st, &two).contains(&(0x1190, 0x0020_0000)));
}

#[test]
fn hardware_zone_starts_hands_over_to_the_mask_and_stops() {
    let only = |st: &mut State, s: &Settings| -> Vec<Write> {
        writes(&st.configure(s, None, None).unwrap().0)
            .into_iter()
            .filter(|w| matches!(w.0, 0x1184 | 0x1190 | 0x1194 | 0x11d0 | 0x11d8 | 0x11dc | 0x4020))
            .collect()
    };
    let bx = |inside| zone::Zone { poly: vec![(-1e-4, 0.5), (1e-4, 1.5)], inside };
    let z1 = HwZone { ch: 0, zones: vec![bx(true)], clear: 0, map: mask::Map::default() };
    let base = Settings { window: None, ..Settings::default() };
    let on = Settings { hwzone: Some(z1.clone()), ..base.clone() };
    let mut st = State::handover(1, None);
    let w = only(&mut st, &on);
    // off, table 1 (1000 + 1), control, compression, slot with clear, armed; then the auto-trigger write
    assert_eq!(w.len(), 1 + 1001 + 1 + 1 + 2 + 1 + 1);
    assert_eq!(w[0], (0x4020, 0x2001));
    assert!(w[1..1001].iter().all(|x| x.0 == 0x11d8));
    assert_eq!(
        w[1002..1008],
        [
            (0x11d0, 0x0500_0000),
            (0x1184, 0xc000_03ff),
            (0x1190, 0x6010_0000),
            (0x1190, 0x0010_0000),
            (0x4020, 0x2003),
            (0x4020, 0x2003)
        ]
    );
    assert_eq!(st.zone_ctrl(), Some(0x0010_0000));
    assert_eq!(only(&mut st, &on), vec![(0x4020, 0x2003)]);
    // a second zone that must not be touched: its table, the new control word, no clear
    let two = Settings { hwzone: Some(HwZone { zones: vec![bx(true), bx(false)], ..z1.clone() }), ..base.clone() };
    let w = only(&mut st, &two);
    assert!(
        w.iter().filter(|x| x.0 == 0x11dc).count() == 1001
            && w.contains(&(0x11d0, 0x0700_0000))
            && w.contains(&(0x1190, 0x0010_0000))
            && !w.contains(&(0x1190, 0x6010_0000))
    );
    // the mask test wins: the zone logic off, the mask keeps the slot it sets
    let mut m = Settings {
        hwmask: Some(HwMask {
            ch: 0,
            lo: vec![-1.0; mask::COLUMNS],
            hi: vec![1.0; mask::COLUMNS],
            stop_on_fail: false,
            clear: 0,
            map: mask::Map::default(),
        }),
        ..two.clone()
    };
    let w = only(&mut st, &m);
    assert_eq!(&w[..2], [(0x4020, 0x2001), (0x11d0, 0)]);
    assert_eq!(st.zone_ctrl(), None);
    assert!(st.mask_ctrl().is_some() && *w.last().unwrap() == (0x4020, 0xe001));
    // back to the zone: the mask goes off without taking the zone's slot and strobe
    m.hwmask = None;
    let w = only(&mut st, &m);
    assert_eq!(st.zone_ctrl(), Some(0x0010_0000));
    assert!(!w.contains(&(0x1184, 0x4000_0001)), "{:x?}", w);
    assert_eq!(*w.last().unwrap(), (0x4020, 0x6003));
    // off
    let w = only(&mut st, &base);
    assert_eq!(w, vec![(0x4020, 0x6001), (0x11d0, 0), (0x1184, 0x4000_0001), (0x1190, 0), (0x4020, 0x6001)]);
}

#[test]
fn uart_trigger_is_written_with_the_trigger_block() {
    use crate::dho::uarttrig::{Cond, Parity, Stop, Uart};
    let u = Uart { baud: 115_200.0, bits: 8, parity: Parity::None, stop: Stop::One, cond: Cond::Start, invert: false };
    let s =
        Settings { trig_src: 2, mask: 0b11, slope: tpu::Slope::Falling, trigger: Trig::Uart(u), ..Settings::default() };
    let mut st = State::handover(0b11, None);
    let w = writes(&st.configure(&s, None, None).unwrap().0);
    let k = w.iter().position(|x| *x == (0x205c, 4)).expect("mux on the UART event");
    assert!(w[..k].contains(&(0x2100, 1356)) && w[..k].contains(&(0x2104, 7)));
    // the trigger block comes before the FSM stop poll and the acquisition block, like every trigger type
    assert!(k < w.iter().position(|x| x.0 == 0x4020).unwrap());
}

#[test]
fn batch_of_frames() {
    // 1 us/div, CH1: 12 500 points per frame, ring of max_frames frames; 100 frames in one arm and one read
    let s = Settings { tdiv: 1e-6, frames: 100, window: None, ..Settings::default() };
    let mut state = State::handover(1, None);
    let (st, p) = plan(&mut state, &s, None, None).unwrap();
    assert!(p.max_frames >= 100, "max_frames {}", p.max_frames);
    let w = writes(&st);
    // arm: 0x4018 = first + n - 1 twice (record and play)
    assert_eq!(w.iter().filter(|x| **x == (0x4018, 99)).count(), 2);
    assert!(matches!(st.last(), Some(&Step::Read { frames: 100, frame }) if frame == 2 * p.memsel as usize));
    // more than the ring is clamped, play-last stays at one frame
    let (_, p2) =
        plan(&mut State::handover(1, None), &Settings { frames: 1_000_000, ..s.clone() }, None, None).unwrap();
    assert_eq!(frames(&Settings { frames: 1_000_000, ..s.clone() }, &p2), p2.max_frames);
    assert_eq!(frames(&Settings { play4: false, ..s }, &p2), 1);
}

#[test]
fn f32_time_base_still_hits_the_rate_table() {
    assert_eq!(tdiv_fs(200e-6_f32 as f64), 200_000_000_000);
    assert_eq!(tdiv_fs(2e-9), 2_000_000);
    assert_eq!(tdiv_fs(500e-3_f32 as f64), 500_000_000_000_000);
    // 100 kpts at 200 us/div: 50 MSa/s is remapped to 31.25 MSa/s like the stock system does
    let p = hz::prep(1, tdiv_fs(200e-6_f32 as f64), 100_000, 0);
    assert_eq!(p.rate, 31_250_000);
}

#[test]
fn default_plan_ends_with_export_init_and_type4_play() {
    let mut state = State::handover(1, None);
    let (st, p) = plan(&mut state, &Settings::default(), None, None).unwrap();
    assert_eq!(p.memsel, 2_500_000);
    let w = writes(&st);
    assert!(w.contains(&(0x1108, 1_248_000)) && w.contains(&(0x1100, 0x11e8)));
    assert!(matches!(st.last(), Some(Step::Read { frames: 1, frame: 8000 })));
    // trigger 17, acquisition 22 + AUX OUT 1 (first set-up only), auto trigger 3, arm 29, pass-through 10, read-out
    // window 7 + stop sequence 17 + type-4 play 12 (the sequence that delivered own frames on the unit, 2026-11-03)
    assert_eq!(w.len(), 17 + 22 + 1 + 3 + 29 + 10 + 7 + 17 + 12);
    let k = w.iter().position(|x| *x == (0x1170, 0x4000_0001)).unwrap();
    assert!(k > 17 + 22 + 1 + 3 + 29 - 1, "pass-through comes after the arm");
    assert_eq!(*w.last().unwrap(), (0x4004, 0x1));
    // the play-last read-out of the warm hand-over is still available: 28 writes
    let mut state = State::handover(1, None);
    let (st, _) = plan(&mut state, &Settings { play4: false, ..Settings::default() }, None, None).unwrap();
    assert_eq!(writes(&st).len(), 17 + 22 + 1 + 3 + 29 + 28);
}

#[test]
fn la_off_writes_nothing_new_and_la_on_adds_the_set_up_and_a_second_read() {
    // LA off: no LA register in the plan
    let mut state = State::handover(1, None);
    let s = Settings { tdiv: 1e-3, depth: 100_000, window: None, ..Settings::default() };
    let (st, p) = plan(&mut state, &s, None, None).unwrap();
    let la_regs = [0x128c, 0x12a4, 0x12a8, 0x12ac, 0x12c0, 0x12d4, 0x1298, 0x1280];
    assert!(!writes(&st).iter().any(|w| la_regs.contains(&w.0)));
    assert!(writes(&st).iter().all(|w| w.0 != 0x4008 || w.1 & (1 << 26) == 0));
    // LA on with CH1 shown: halved rate, doubled record registers, enable, rate block, thresholds, second read
    let mut state = State::handover(1, None);
    let sl = Settings { la: Some(la::Set::default()), frames: 50, ..s.clone() };
    let (st, pl) = plan(&mut state, &sl, None, None).unwrap();
    assert_eq!((pl.rate * 2, pl.memsel, pl.seg), (p.rate, p.memsel, 2 * p.seg));
    let w = writes(&st);
    assert!(
        w.contains(&(0x10e4, 2 * pl.memsel)) && w.contains(&(0x4050, 2 * pl.pre)) && w.contains(&(0x12d4, 0x2336_2353))
    );
    assert!(w.iter().any(|x| x.0 == 0x4008 && x.1 & (1 << 26) != 0));
    let t = la::timing(&pl, 1);
    assert!(w.contains(&la::rate_block(t.rate)[2]) && w.contains(&(0x1100, 10 * t.n)));
    // one frame per batch, the analog read, then the LA read of 2 x roundup4(n) bytes, then the stream restore
    assert_eq!(frames(&sl, &pl), 1);
    let reads: Vec<&Step> = st.iter().filter(|x| matches!(x, Step::Read { .. })).collect();
    assert_eq!(reads.len(), 2);
    assert!(matches!(reads[1], Step::Read { frames: 1, frame } if *frame == la::read_bytes(t.n)));
    assert_eq!(state.sh.s4008 & (1 << 23 | 1 << 24 | 1 << 26), 1 << 23 | 1 << 26);
    // LA alone (no analog channel shown): the rate is not halved
    let mut state = State::handover(1, None);
    let (_, pa) = plan(
        &mut state,
        &Settings { la: Some(la::Set { analog_shown: false, ..la::Set::default() }), ..s },
        None,
        None,
    )
    .unwrap();
    assert_eq!(pa.rate, p.rate);
}

#[test]
fn frames_padded_to_8_bytes() {
    use super::{frame_stride, read_len, unpad};
    assert_eq!(
        (frame_stride(2500), frame_stride(1250), frame_stride(5000), frame_stride(500)),
        (2504, 1256, 5000, 504)
    );
    assert_eq!(read_len(4, 2500), 10016);
    // three frames of 5 bytes at a stride of 8
    let mut b: Vec<u8> = vec![1, 2, 3, 4, 5, 0, 0, 0, 6, 7, 8, 9, 10, 0, 0, 0, 11, 12, 13, 14, 15, 0, 0, 0];
    unpad(&mut b, 5);
    assert_eq!(b, (1..=15).collect::<Vec<u8>>());
}

#[test]
fn four_channel_window_like_phase05_step_d() {
    // step D (2026-10-09, word-identical): 4 channels, 4000 points from point 0 -> 0x1100 = 0x17e8, 0x1108 = 0,
    // 0x11f0 = 0xfa0, one read of 32000 bytes
    let s = Settings { mask: 0xf, tdiv: 2e-6, window: Some((4000, -1_000_000)), ..Settings::default() };
    let mut state = State::handover(0xf, None);
    let (st, p) = plan(&mut state, &s, None, None).unwrap();
    assert_eq!(p.ratio, 4);
    let w = writes(&st);
    assert!(w.contains(&(0x1100, 0x17e8)) && w.contains(&(0x1108, 0)) && w.contains(&(0x11f0, 0xfa0)));
    assert!(matches!(st.last(), Some(Step::Read { frames: 1, frame: 32000 })));
}

#[test]
fn channel_set_change_switches_the_adc_once() {
    let s = Settings { mask: 3, ..Settings::default() };
    let mut state = State::handover(1, None);
    assert!(state.configure(&s, None, None).is_err());
    if let Ok(cal) = adc::CalAdc::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal")) {
        let mut state = State::handover(1, Some(&cal));
        let (st, _) = state.configure(&s, None, Some(&cal)).unwrap();
        let w = writes(&st);
        assert_eq!(&w[..2], &[(0x1034, 0x0154_1860), (0x1034, 0x0155_0005)]);
        // second configure with the same set: no ADC writes
        let (st2, _) = state.configure(&s, None, Some(&cal)).unwrap();
        assert!(!writes(&st2).iter().any(|w| w.0 == 0x1034));
    }
}

#[test]
fn every_hardware_trigger_type_configures() {
    use tpu::*;
    let pats = [Pat::H, Pat::X, Pat::X, Pat::Rise];
    for t in [
        Trig::Edge,
        Trig::Pulse { pol: Polarity::Pos, when: When::Lt, lower: 0.0, upper: 20e-6 },
        Trig::Slope { when: When::Gt, lower: 1e-6, upper: 0.0, lo: 0.5 },
        Trig::Runt { pol: Polarity::Pos, when: When::None, lower: 0.0, upper: 0.0, lo: 0.5 },
        Trig::Window { event: WindowEvent::Enter, time: 0.0, lo: -0.5 },
        Trig::Timeout { time: 1.2e-3 },
        Trig::Delay { srcb: 4, eb: Edge::Rising, when: When::Gt, lower: 40e-6, upper: 0.0, levelb: 1.65 },
        Trig::Pattern { pats, la: [Pat::X; 16], levels: [1.25, 0.0, 1.65, 1.65] },
        Trig::Duration {
            pats: [Pat::H, Pat::X, Pat::X, Pat::X],
            la: [Pat::X; 16],
            when: When::Gt,
            lower: 200e-6,
            upper: 0.0,
            levels: [1.25; 4],
        },
        Trig::SetupHold { clk: 4, clk_edge: Edge::Rising, setup: 2e-6, hold: 2e-6, levelc: 1.65 },
        Trig::Nth { count: 3, idle: 50e-6 },
        Trig::Video { std: VideoStd::Pal, sync: VideoSync::OddField, line: 1 },
    ] {
        let s = Settings { mask: 0xf, trigger: t.clone(), ..Settings::default() };
        let mut state = State::handover(0xf, None);
        let (st, _) = state.configure(&s, None, None).unwrap();
        // every type ends with the type select in 0x205c[4:0] written
        assert!(writes(&st).iter().any(|w| w.0 == 0x205c), "{:?}", t);
    }
    // Nth edge: ABR flags (1,1,1,0,1) + type 3 (trigger.md 10.11)
    let s = Settings { trigger: Trig::Nth { count: 3, idle: 50e-6 }, ..Settings::default() };
    let mut state = State::handover(1, None);
    let (st, _) = state.configure(&s, None, None).unwrap();
    assert_eq!(writes(&st).iter().rev().find(|w| w.0 == 0x205c).unwrap().1 & 0x7df, 0x5c3);
}

#[test]
fn deinterleave_and_volts() {
    let words: Vec<u16> = (0..16).collect();
    let b: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let d = deinterleave(&b, 0b1010);
    assert_eq!(d[1], [0, 2, 4, 6, 8, 10, 12, 14]);
    assert_eq!(d[3], [1, 3, 5, 7, 9, 11, 13, 15]);
    let d = deinterleave(&b, 0b0111); // 3 channels run in 4-channel mode
    assert_eq!(d[2], [2, 6, 10, 14]);
    assert!(d[3].is_empty());
    // the replayed frame: 21518 = yref + yorg at 1 V/div, offset -1.5 V -> 0 V
    assert!((volts(&[21518], 1.0, -1.5)[0]).abs() < 1e-6);
    assert!((volts(&[32768 + 7500], 2.0, 0.0)[0] - 2.0).abs() < 1e-6);
}

/// Pseudo-random read-out bytes (a fixed LCG, so failures repeat).
fn noise(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (x >> 24) as u8
        })
        .collect()
}

/// The bound of `volt32` against the f64 `volt` of code `x`: 4 f32 ulps of the larger of the two terms.
fn f32_bound(x: u16, (scale, offset): (f64, f64)) -> f64 {
    4.0 * f32::EPSILON as f64 * ((x as f64 - 32768.0).abs() / 7500.0 * scale.abs() + offset.abs())
        + f32::MIN_POSITIVE as f64
}

// Break it catches: a fused decode that maps a word to the wrong channel, drops the words of an incomplete last
// group, or moves a value by more than the f32 rounding (the display and the measurements would move).
#[test]
fn fused_decode_equals_deinterleave_and_volts() {
    let cal = [(1.0, -1.5), (0.02, 0.013), (5.0, 2.5), (0.5, 0.0)];
    let mut worst = 0f64;
    for mask in 1..16u32 {
        for len in [0usize, 2, 6, 8, 14, 16, 2816, 8000, 12_502] {
            let b = noise(len, mask * 7919 + len as u32);
            let w = deinterleave(&b, mask);
            let got = decode_volts(&b, mask, cal);
            for c in 0..4 {
                assert_eq!(got[c].len(), w[c].len(), "mask {:#x}, {} bytes, CH{}", mask, len, c + 1);
                let want = volts(&w[c], cal[c].0, cal[c].1);
                for (i, (&g, &x)) in got[c].iter().zip(&w[c]).enumerate() {
                    let d = (g as f64 - want[i] as f64).abs();
                    worst = worst.max(d / (cal[c].0 / 7500.0));
                    assert!(
                        d <= f32_bound(x, cal[c]),
                        "mask {:#x}, {} bytes, CH{} word {}: {} vs {}",
                        mask,
                        len,
                        c + 1,
                        i,
                        g,
                        want[i]
                    );
                }
            }
        }
    }
    // in codes: far below one
    assert!(worst < 0.01, "{} codes", worst);
}

// Break it catches: kept vectors that keep stale samples or a channel that is no longer enabled, or that differ from
// the allocating decode.
#[test]
fn decode_into_kept_vectors_equals_decode() {
    let cal = [(1.0, -1.5), (0.02, 0.013), (5.0, 2.5), (0.5, 0.0)];
    let mut out: [Vec<f32>; 4] = Default::default();
    for (mask, len) in [(0xfu32, 8000usize), (1, 2816), (0b1010, 4002), (0b0111, 802), (8, 6)] {
        let b = noise(len, mask);
        decode_volts_into(&b, mask, cal, &mut out);
        let want = decode_volts(&b, mask, cal);
        for c in 0..4 {
            let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<u32>>();
            assert_eq!(bits(&out[c]), bits(&want[c]), "mask {:#x}, CH{}", mask, c + 1);
        }
    }
}

// Break it catches: an unpad on the reused DMA buffer that differs from the vector one.
#[test]
fn unpad_in_place_equals_unpad() {
    for frame in [5usize, 2500, 2504, 1250, 16] {
        let b = noise(7 * frame_stride(frame) + 3, frame as u32);
        let mut v = b.clone();
        unpad(&mut v, frame);
        let mut s = b.clone();
        let n = unpad_in(&mut s, frame);
        assert_eq!(&s[..n], &v[..], "frame {}", frame);
    }
}

// Break it catches: an arm ahead of the decode that writes anything else, or in another order, than the batch loop
// without it (stop sequence, record, play and their shadows must stay exactly as verified on the unit). Two batches
// of 100 frames; the pre-armed loop issues arm 2 right after read 1 and only waits before read 2.
#[test]
fn prearm_writes_the_same_sequence_as_the_plain_batch_loop() {
    let s = Settings { tdiv: 1e-6, frames: 100, window: None, ..Settings::default() };
    let p = State::handover(1, None).configure(&s, None, None).unwrap().1;
    let batch = |st: &mut State, pre: bool, armed: bool| -> Vec<Step> {
        let mut v = if armed { wait_steps(&p) } else { st.arm_steps(&s, &p) };
        v.extend(st.read_steps(&s, &p, 100));
        if pre {
            v.extend(st.arm_only_steps(&s, &p));
        }
        v
    };
    let mut a = State::handover(1, None);
    a.configure(&s, None, None).unwrap();
    let plain: Vec<Step> = (0..3).flat_map(|_| batch(&mut a, false, false)).collect();
    let mut b = State::handover(1, None);
    b.configure(&s, None, None).unwrap();
    let mut pre = batch(&mut b, true, false);
    pre.extend(batch(&mut b, true, true));
    pre.extend(batch(&mut b, false, true));
    assert_eq!(writes(&pre), writes(&plain));
    assert_eq!(writes(&pre).len(), 3 * (29 + 10 + 7 + 17 + 12));
    // the waits and reads are the same steps too, only the arm moved before the decode
    let polls = |v: &[Step]| v.iter().filter(|x| matches!(x, Step::Poll { .. } | Step::Read { .. })).count();
    assert_eq!(polls(&pre), polls(&plain));
    assert_eq!((a.sh.s4004, a.sh.s4008, a.sh.s400c, a.sh.s4080), (b.sh.s4004, b.sh.s4008, b.sh.s400c, b.sh.s4080));
    // arm_steps is still arm + wait
    let mut c = State::handover(1, None);
    let mut d = State::handover(1, None);
    let mut split = c.arm_only_steps(&s, &p);
    split.extend(wait_steps(&p));
    assert_eq!(format!("{:?}", d.arm_steps(&s, &p)), format!("{:?}", split));
}

// Break it catches: a segmented record that is not stamp clear + the batch arm with N and the interval, a count
// beyond the whole-memory limit, segments that leak into the live batch path.
#[test]
fn record_steps_clear_the_stamp_and_arm_n_frames() {
    let seg = |n: u32, interval: f64| Settings {
        tdiv: 1e-6,
        depth: 1000,
        window: None,
        segments: Some(Segments { n, interval }),
        ..Settings::default()
    };
    let mut st = State::handover(1, None);
    let p = st.configure(&seg(100, 1e-3), None, None).unwrap().1;
    let w = writes(&st.record_steps(&seg(100, 1e-3), &p));
    assert_eq!(&w[..2], &[(0x40b8, 4), (0x40b8, 0)]);
    assert_eq!(w.len(), 2 + 29);
    assert!(w.contains(&(0x4068, 156_250)) && w.contains(&(0x4018, 99)));
    // the stock limit for 1,000 points: 61,440 frames
    assert_eq!(record_frames(&seg(1_000_000, 0.0), &p), 61_440);
    assert_eq!(record_frames(&seg(0, 0.0), &p), 1);
    assert_eq!(record_frames(&Settings::default(), &p), 0);
    // the live plan does not change with segments set
    let live = |s: &Settings| writes(&plan(&mut State::handover(1, None), s, None, None).unwrap().0);
    assert_eq!(live(&seg(100, 1e-3)), live(&Settings { segments: None, ..seg(100, 1e-3) }));
}

// Break it catches: a segment read that plays from frame 0, a stamp play with the 8-point TX length the stock
// export never uses, a stamp play without the header.
#[test]
fn segment_reads_play_from_their_frame() {
    let s = Settings {
        mask: 3,
        tdiv: 1e-6,
        depth: 1000,
        window: None,
        segments: Some(Segments { n: 500, interval: 0.0 }),
        header: true,
        ..Settings::default()
    };
    let mut a = State::handover(3, None);
    let p = a.configure(&s, None, None).unwrap().1;
    let mut b = State::handover(3, None);
    b.configure(&s, None, None).unwrap();
    let at = writes(&a.read_steps_at(&s, &p, 200, 40));
    let zero = writes(&b.read_steps(&s, &p, 40));
    let diff: Vec<(Write, Write)> = at.iter().zip(&zero).filter(|(x, y)| x != y).map(|(x, y)| (*x, *y)).collect();
    assert_eq!(diff, [((0x4010, 200), (0x4010, 0)), ((0x4014, 200), (0x4014, 0)), ((0x4018, 239), (0x4018, 39))]);
    assert!(at.contains(&(0x1010, a.sh.s1010)) && a.sh.s1010 & 1 << 31 == 0, "header on");
    let st = a.segment_stamp_steps(&s, &p, 7);
    let w = writes(&st);
    // 128 words of two channels: TX length 64 per channel, one 16 + 256 byte read, then the latch pulse
    assert!(w.contains(&(0x11f0, 64)) && w.contains(&(0x4010, 7)) && w.contains(&(0x4018, 7)));
    assert!(w.iter().any(|x| x.0 == 0x1010 && x.1 & 1 << 31 == 0));
    assert_eq!(&w[w.len() - 3..], &[(0x40b8, 0), (0x40b8, 1), (0x40b8, 0)]);
    assert!(st.iter().any(|x| matches!(x, Step::Read { frames: 1, frame: 272 })));
    assert!(w.iter().position(|x| x.0 == 0x40b8).unwrap() > w.iter().position(|x| *x == (0x4004, a.sh.s4004)).unwrap());
}

// Break it catches: a disarm that is not the plain stop sequence every arm starts with.
#[test]
fn disarm_is_the_stop_sequence_of_an_arm() {
    let s = Settings::default();
    let p = hz::prep(1, tdiv_fs(s.tdiv), 0, 0);
    let mut a = State::handover(1, None);
    let d = writes(&a.disarm_steps());
    assert_eq!(d.len(), 17);
    let arm = writes(&State::handover(1, None).arm_only_steps(&s, &p));
    assert_eq!(&arm[..17], &d[..]);
    assert!(matches!(a.disarm_steps()[1], Step::Poll { reg: 0x4034, ms: 10, .. }));
}
