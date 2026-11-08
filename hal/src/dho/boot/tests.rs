// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Whole-sequence tests against `sim::Unit` (scripted reads, virtual time).

use super::sim::{AfeStub, PllRec, SimClock, Unit, UNIT_IDELAY};
use super::*;

const CAL_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal");

/// Every register the cold boot may write.
const ALLOWED: [u32; 21] = [
    0x1000, 0x1010, 0x1014, 0x1018, 0x1020, 0x1034, 0x1058, 0x105c, 0x1418, 0x141c, 0x1420, 0x1424, 0x1428, 0x1470, 0x1474, 0x147c,
    0x1484, 0x148c, 0x1494, 0x4004, 0x4008,
];

struct Run {
    r: Report,
    clk: SimClock,
    pll: PllRec,
    afe: AfeStub,
}

fn boot(unit: &mut Unit, cal: &CalFiles) -> Run {
    let (mut clk, mut pll, mut afe) = (SimClock::default(), PllRec::default(), AfeStub::default());
    let r = cold_boot(Hw { bus: unit, clock: &mut clk, pll: Some(&mut pll), afe: Some(&mut afe), early: None }, cal, &Options::default());
    Run { r, clk, pll, afe }
}

fn with(f: impl FnOnce(&mut Unit)) -> Unit {
    let mut u = Unit::nominal();
    f(&mut u);
    u
}

fn last(unit: &Unit, off: u32) -> Option<u32> {
    unit.writes.iter().rev().find(|w| w.0 == off).map(|w| w.1)
}

fn ddr_container(taps: [u32; 6]) -> Vec<u8> {
    let mut p = vec![0u8; 48];
    for (k, v) in taps.iter().enumerate() {
        p[8 * k..8 * k + 4].copy_from_slice(&v.to_le_bytes());
    }
    let mut d = Vec::new();
    for w in [0xad2b_9fefu32, 0x14, 0, 0, 0, 48, cal::crc32(&p)] {
        d.extend(w.to_le_bytes());
    }
    d.extend(p);
    d
}

fn check_safe(unit: &Unit) {
    for &(o, v) in &unit.writes {
        assert!(ALLOWED.contains(&o), "unexpected write {:#x} <- {:#x}", o, v);
    }
    assert!(!unit.writes.iter().any(|w| w.0 == 0x1008), "0x1008 must never be written");
}

#[test]
fn nominal_boot_ends_in_the_live_state() {
    let mut unit = Unit::nominal();
    let run = boot(&mut unit, &CalFiles::default());
    let o = match run.r.result {
        Ok(o) => o,
        Err(e) => panic!("{}", e),
    };
    check_safe(&unit);
    assert_eq!(run.pll.words, regs::PLL_TABLE.to_vec());
    assert!(run.afe.writes > 4 * 30 && run.afe.reads == 4 * (5 * 16 + 5));
    assert_eq!((o.ddr_status, o.adc_clk, o.sync_delay), (0x2400_0201, 62500, 0));
    assert_eq!(o.sync_do, sim::UNIT_SYNC_DO);
    assert!(o.idelay_scanned);
    assert_eq!(o.idelay, UNIT_IDELAY);
    assert_eq!(o.preamble, [(0, 0), (0, 0)]);
    assert!(o.trim_done);
    // live values of the stock system [O]: 0x1014 = 0x14f01301, 0x1020 = 0x1010, s00 = 0x24, s06 = 0x00c3, 0x1428 = 0x646e
    assert_eq!(o.sh.s1014, 0x14f0_1301);
    assert_eq!(o.sh.s1020, 0x1010);
    assert_eq!((o.sh.adc00, o.sh.adc06, o.sh.s1428), (0x24, 0x00c3, 0x646e));
    assert_eq!(last(&unit, 0x1014), Some(0x14f0_1301));
    assert_eq!(last(&unit, 0x1020), Some(0x1010));
    assert_eq!(unit.writes.iter().find(|w| w.0 == 0x1014), Some(&(0x1014, 0x00f0_0001)));
    // the final load is the oos-hw adc-idelay list, preceded by select 1
    let mut sh = Shadows { s1014: 0x00f0_1001, ..Default::default() };
    let load = regs::idelay_load(&mut sh, &UNIT_IDELAY);
    assert!(unit.writes.windows(load.len()).any(|w| w == load.as_slice()));
    // sync: 8 reg-7 reads, the final delay word is delay 0 followed by the sync sequence
    assert_eq!(unit.writes.iter().filter(|w| **w == (0x1034, 0x0187_0000)).count(), 8);
    let i = unit.writes.iter().rposition(|w| *w == (0x1034, 0x0106_00c3)).unwrap();
    assert_eq!(
        &unit.writes[i + 1..i + 8],
        &[(0x1034, 0x0100_0045), (0x1000, 0), (0x1000, 2), (0x1000, 0), (0x1034, 0x0100_00c5), (0x1000, 1), (0x1000, 0)]
    );
    // the once-only writes and the SCU enables
    for w in [(0x1428, 0x646e), (0x1418, 0x8000), (0x1424, 0x8000), (0x4008, 0x80_0000)] {
        assert!(unit.writes.contains(&w));
    }
    // no cal files: neither 0x1058 nor DDR taps
    assert!(!unit.writes.iter().any(|w| w.0 == 0x1058 || w.0 == 0x1470));
    assert!(o.warnings.iter().any(|w| w.contains("cal_adc.hex")));
    // hand-over state
    assert_eq!(o.state.mode, usize::MAX);
    assert_eq!((o.state.sh.s4004, o.state.sh.s4008, o.state.sh.s1000, o.state.sh.s1010), (0, 0x80_0000, 0, 0x2000_0000));
    // time: PLL 46 x 50 ms, ADC init 209 x 1.2 ms, sync sweep 8 x 20 ms, ...
    assert!(run.clk.t_us > 2_300_000 + 250_000 + 160_000 && run.clk.t_us < 3_500_000, "{}", run.clk.t_us);
}

#[test]
fn core_alignment_from_spu_init_ends_in_0x14f01301() {
    // cold-boot.md 3.5: from 0x00f00001 (SPU init) the sequence 3.4 + 3.5 ends exactly in the live 0x14f01301
    let mut unit = Unit::nominal();
    let mut clk = SimClock::default();
    let mut c = Ctx { bus: &mut unit, clk: &mut clk, sh: Shadows::default(), log: Vec::new(), warnings: Vec::new(), progress: &mut |_| {} };
    let w = regs::adc_init(&mut c.sh, regs::ADC_INIT_PRE_US_SHORT);
    c.adc(&w);
    let w = regs::spu_init(&mut c.sh);
    c.apply(&w);
    assert_eq!(c.sh.s1014, 0x00f0_0001);
    let eye = c.eye_scan();
    let d = wnd::lane_delays(&eye).unwrap();
    let w = regs::idelay_load(&mut c.sh, &d);
    c.apply(&w);
    assert_eq!(c.sh.s1014, 0x00f0_1001);
    assert_eq!(c.core_align(), (0, 0));
    assert_eq!((c.sh.s1014, c.sh.s1020), (0x14f0_1301, 0x1010));
    // the second run (step 16) and the bit range keep it
    c.core_align();
    let w = regs::bit_range(&mut c.sh);
    assert_eq!(w, (0x1014, 0x14f0_1301));
    // the preamble codes Da/Db go to 0x1020 bits 1:0 / 9:8
    unit.preamble = 0x0002_0001;
    let mut c = Ctx { bus: &mut unit, clk: &mut clk, sh: Shadows { s1014: 0x00f0_1001, ..Default::default() }, log: Vec::new(), warnings: Vec::new(), progress: &mut |_| {} };
    assert_eq!(c.core_align(), (1, 2));
    assert_eq!(c.sh.s1020, 0x1010 | 0x0201);
    assert_eq!(c.warnings.len(), 1);
}

#[test]
fn ddr_never_ready_stops_without_bram_fallback() {
    let mut unit = with(|u| u.ddr_busy_reads = u32::MAX);
    let run = boot(&mut unit, &CalFiles::default());
    let e = run.r.result.err().unwrap();
    assert!(e.contains("DDR init not passed"), "{}", e);
    check_safe(&unit);
    assert_eq!(unit.reads.iter().filter(|r| r.0 == 0x1008).count(), 1 + DDR_POLLS as usize);
    // nothing after the check: no ADC calibration, no lane delays
    assert!(!unit.writes.iter().any(|w| w.0 == 0x1018 || w.0 == 0x1020));
    // a slow DDR is fine
    let mut unit = with(|u| u.ddr_busy_reads = 10);
    assert!(boot(&mut unit, &CalFiles::default()).r.result.is_ok());
}

#[test]
fn bad_adc_clock_or_fpga_version_stops() {
    let mut unit = with(|u| u.adc_clk = 62507);
    let e = boot(&mut unit, &CalFiles::default()).r.result.err().unwrap();
    assert!(e.contains("0x1210 = 62507"), "{}", e);
    let mut unit = with(|u| u.adc_clk = 62494);
    assert!(boot(&mut unit, &CalFiles::default()).r.result.is_ok());
    let mut unit = with(|u| u.version = 0xffff_ffff);
    let run = boot(&mut unit, &CalFiles::default());
    assert!(run.r.result.is_err());
    assert!(unit.writes.is_empty() && run.pll.words.is_empty() && run.afe.writes == 0);
}

#[test]
fn sync_bit_that_never_moves_stops_at_delay_7() {
    let mut unit = with(|u| u.sync_do = [true; 8]);
    let e = boot(&mut unit, &CalFiles::default()).r.result.err().unwrap();
    assert!(e.contains("ADC0 Sync DO:1,1,1,1,1,1,1,1"), "{}", e);
    let sync_words: Vec<u32> = unit.writes.iter().filter(|w| w.0 == 0x1034 && (w.1 >> 16) == 0x0106).map(|w| w.1).collect();
    assert_eq!(sync_words.last(), Some(&0x0106_07c3));
    assert!(!unit.writes.iter().any(|w| w.0 == 0x1018));
}

/// Seen on the unit at a power-on: stock stays at 7 and goes on, so must we.
#[test]
fn sync_with_a_rising_edge_only_goes_on_at_delay_7() {
    let mut unit = with(|u| u.sync_do = [false, false, false, true, true, true, true, true]);
    let o = boot(&mut unit, &CalFiles::default()).r.result.unwrap();
    assert_eq!(o.sync_delay, 7);
    let sync_words: Vec<u32> = unit.writes.iter().filter(|w| w.0 == 0x1034 && (w.1 >> 16) == 0x0106).map(|w| w.1).collect();
    assert_eq!(sync_words.last(), Some(&0x0106_07c3), "no final sync at another delay");
    assert!(unit.writes.iter().any(|w| w.0 == 0x1018), "the eye scan runs");
}

/// Sample clock and AFEs during the FPGA wait: the boot goes on from step 4 and the AFE stays at hand for step 20.
#[test]
fn early_start_leaves_steps_2_and_3_out() {
    let mut unit = sim::Unit::nominal();
    let (mut clk, mut pll, mut afe) = (SimClock::default(), PllRec::default(), AfeStub::default());
    let mut seen = Vec::new();
    let e = early_start(&mut pll, &mut afe, &mut clk, &mut |n| seen.push(n)).unwrap();
    assert_eq!(seen, [2, 3]);
    let writes = afe.writes;
    let mut steps = Vec::new();
    let r = cold_boot_with(Hw { bus: &mut unit, clock: &mut clk, pll: None, afe: Some(&mut afe), early: Some(e) }, &CalFiles::default(), &Options::default(), &mut |n| steps.push(n));
    assert!(r.result.is_ok(), "{:?}", r.result.err());
    assert!(!steps.contains(&2) && !steps.contains(&3) && steps.contains(&4));
    assert!(afe.writes > writes, "Hz enable at the end");
}

#[test]
fn broken_eye_falls_back_to_the_stored_list() {
    let mut unit = with(|u| u.eye = [0x0fff_ffff; 32]);
    let run = boot(&mut unit, &CalFiles::default());
    let o = run.r.result.unwrap_or_else(|e| panic!("{}", e));
    assert!(!o.idelay_scanned);
    assert_eq!(o.idelay, Options::default().idelay_fallback);
    assert!(o.warnings.iter().any(|w| w.contains("NoReference")));
    // the ADC is back in normal data mode after the scan
    let i = unit.writes.iter().position(|w| *w == (0x1034, regs::DATA_MODE_TEST)).unwrap();
    assert!(unit.writes[i..].contains(&(0x1034, regs::DATA_MODE_NORMAL)));
}

#[test]
fn adc_trim_timeout_warns_and_continues() {
    let mut unit = with(|u| u.trim_polls = u32::MAX);
    let run = boot(&mut unit, &CalFiles::default());
    let o = run.r.result.unwrap_or_else(|e| panic!("{}", e));
    assert!(!o.trim_done);
    assert!(o.warnings.iter().any(|w| w.contains("ADC trim")));
    // about 2 s of polls (100 us each) on core 0, the trim of core 0 stays on (only the init list's 0x01130000 twice)
    let polls = unit.writes.iter().filter(|w| **w == (0x1034, 0x0196_0000)).count();
    assert!((19_900..=20_100).contains(&polls), "{}", polls);
    assert_eq!(unit.writes.iter().filter(|w| **w == (0x1034, 0x0113_0000)).count(), 2);
    assert!(run.clk.t_us > 2_000_000 + 2_300_000);
    // with done flags: trim on for 8 cores, poll, trim off per core
    let mut unit = with(|u| u.trim_polls = 3);
    let o = boot(&mut unit, &CalFiles::default()).r.result.unwrap_or_else(|e| panic!("{}", e));
    assert!(o.trim_done);
    let i = unit.writes.iter().position(|w| *w == (0x1034, 0x0113_4000)).unwrap();
    let on: Vec<u32> = unit.writes[i..i + 8].iter().map(|w| w.1).collect();
    assert_eq!(on, (0..8).map(|n| regs::qi_trim(n, true).val).collect::<Vec<_>>());
    assert_eq!(unit.writes[i + 8..i + 11], [(0x1034, 0x0196_0000); 3]);
    assert_eq!(unit.writes[i + 11], (0x1034, 0x0113_0000));
}

#[test]
fn cal_files_write_adc_parameters_and_ddr_taps() {
    let mut cal = CalFiles { ddr: DdrCal::from_file_bytes(&ddr_container([0x12, 0x4d6, 0x8a2, 0x924, 0x1ef, 0])), ..Default::default() };
    assert!(cal.ddr.is_some());
    let unit_cal = crate::dho::adc::CalAdc::load(&std::env::var("OOS_CAL_DIR").unwrap_or(CAL_DIR.into())).ok();
    let have_adc = unit_cal.is_some();
    cal.adc = unit_cal;
    let mut unit = Unit::nominal();
    let run = boot(&mut unit, &cal);
    let o = run.r.result.unwrap_or_else(|e| panic!("{}", e));
    check_safe(&unit);
    let taps = [(0x1000, 8), (0x1470, 0x12), (0x1474, 0x4d6), (0x147c, 0x8a2), (0x1484, 0x924), (0x148c, 0x1ef), (0x1494, 0), (0x1000, 0)];
    let ti = unit.writes.windows(8).position(|w| w == taps).expect("DDR taps");
    // the taps come after the ADC parameters and before the sync sweep
    let si = unit.writes.iter().position(|w| *w == (0x1034, 0x0187_0000)).unwrap();
    assert!(ti < si);
    assert_eq!(o.ddr_status, 0x2400_0201);
    if have_adc {
        // cold-boot.md 3.7 / adc-mode.md 5.3: ref word, 0x1058 = 0xab2a, ..., the boot's first reg-0x04 word 0x0080
        let pi = unit.writes.iter().position(|w| *w == (0x1058, 0xab2a)).unwrap();
        assert_eq!(unit.writes[pi - 1], (0x1034, 0x0109_7700));
        assert!(pi < ti);
        assert_eq!(unit.writes[pi + 1], (0x1034, 0x0115_0009));
        let tail = [0x0104_0080, 0x0104_8080, 0x0105_8080, 0x0105_8080, 0x0101_0000].map(|v| (0x1034, v));
        assert!(unit.writes[pi..ti].ends_with(&tail));
        assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    } else {
        eprintln!("cal_adc.hex not found, ADC parameter part skipped");
    }
}

#[test]
fn dry_run_prints_the_plan() {
    let cal = CalFiles::load(&std::env::var("OOS_CAL_DIR").unwrap_or(CAL_DIR.into()));
    let s = dry_run(&cal, &Options::default());
    for want in ["DRY RUN", "== 21", "pll R0 <- 0x221c", "wr 0x1014 <- 0x00f00001", "wr 0x1014 <- 0x14f01301", "-- result: ok", "-- todo: step 19", "== 20 AFE Hz enable"] {
        assert!(s.contains(want), "missing {:?}", want);
    }
    assert!(!s.contains("wr 0x1008"));
    println!("{}", s);
}

#[test]
fn progress_reports_every_step_in_order() {
    let mut unit = Unit::nominal();
    let (mut clk, mut pll, mut afe) = (SimClock::default(), PllRec::default(), AfeStub::default());
    let mut steps = Vec::new();
    let r = cold_boot_with(Hw { bus: &mut unit, clock: &mut clk, pll: Some(&mut pll), afe: Some(&mut afe), early: None }, &CalFiles::load(CAL_DIR), &Options::default(), &mut |n| steps.push(n));
    assert!(r.result.is_ok());
    assert_eq!(steps, (1..=21).collect::<Vec<_>>());
    // the start-up screen's groups, in the order they first appear
    let mut groups: Vec<&str> = Vec::new();
    for n in steps {
        if !groups.contains(&stage(n)) {
            groups.push(stage(n));
        }
    }
    assert_eq!(groups, ["FPGA image", "Sample clock (PLL lock)", "Analog front ends", "ADC and memory set-up", "ADC data lanes", "Signal path"]);
    // every group comes as one run of steps (no row of the screen starts twice)
    let runs = steps_runs(&(1..=21).map(stage).collect::<Vec<_>>());
    assert_eq!(runs, groups.len());
    // a wrong FPGA image stops in step 1
    let mut unit = with(|u| u.version = 0);
    let mut steps = Vec::new();
    let r = cold_boot_with(Hw { bus: &mut unit, clock: &mut clk, pll: None, afe: None, early: None }, &CalFiles::load(CAL_DIR), &Options::default(), &mut |n| steps.push(n));
    assert!(r.result.is_err() && steps == [1]);
}

/// Number of runs of equal neighbours.
fn steps_runs(v: &[&str]) -> usize {
    v.windows(2).filter(|w| w[0] != w[1]).count() + 1
}
