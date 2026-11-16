// SPDX-License-Identifier: GPL-2.0-or-later
//! Tests of the SCPI coverage extensions (`ui/scpiext*.rs`): every new header is set and queried once (a round trip), and
//! each one changes the model field or runs the Ui function that the menu item of the same name does, so the item shows
//! the value. A last test checks that every header of the tables was exercised and is documented in `docs/scpi.md`.
//! The commands that power off, restart, update or roll back are only tested without their CONFIRM (they must refuse).

use super::scpiext::{all_headers, resolve};
use super::*;
use crate::scpi::Scpi;
use crate::wave::Source;
use std::collections::BTreeSet;

struct T {
    s: Scpi,
    ui: Ui,
    cv: Canvas,
    used: BTreeSet<&'static str>,
}

fn header_of(c: &str) -> Option<&'static str> {
    let head = c.split_whitespace().next()?.trim_end_matches('?');
    let nodes: Vec<&str> = head.trim_start_matches(':').split(':').collect();
    resolve(&nodes).map(|r| r.0)
}

impl T {
    fn new() -> T {
        let mut t = T { s: Scpi::open(0), ui: Ui::new(Source::Demo { t: 0.0 }), cv: Canvas::new(W, H), used: BTreeSet::new() };
        t.ui.store_dir = std::env::temp_dir().join(format!("oos-scpiext-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&t.ui.store_dir);
        for _ in 0..40 { t.ui.step(0.05); }
        assert!(t.ui.last[0].is_some(), "the demo source gave a record");
        t.ui.m.run = RunState::Stop;
        t
    }
    fn raw(&mut self, c: &str) -> String {
        if let Some(h) = header_of(c) { self.used.insert(h); }
        String::from_utf8_lossy(&self.s.exec_line(&mut self.ui, &self.cv, c)).into_owned()
    }
    fn err(&mut self) -> String { String::from_utf8_lossy(&self.s.exec_line(&mut self.ui, &self.cv, ":SYST:ERR?")).into_owned() }
    /// runs a command that must be accepted
    fn go(&mut self, c: &str) -> String {
        let _ = self.err();
        let r = self.raw(c);
        let e = self.err();
        assert!(e.starts_with("0,"), "{c} -> {e}");
        r
    }
    /// runs a command that must be refused with the error code
    fn no(&mut self, c: &str, code: &str) {
        let _ = self.err();
        let r = self.raw(c);
        let e = self.err();
        assert!(e.starts_with(code), "{c} -> reply {r:?}, error {e}");
    }
    /// set, query, compare text
    fn rt(&mut self, set: &str, query: &str, want: &str) {
        self.go(set);
        let got = self.go(query);
        assert_eq!(got, want, "{set} then {query}");
    }
    /// set, query, compare number
    fn rn(&mut self, set: &str, query: &str, want: f64) {
        self.go(set);
        let got: f64 = self.go(query).parse().unwrap_or_else(|_| panic!("{query}"));
        assert!((got - want).abs() <= want.abs() * 1e-5 + 1e-30, "{set} then {query}: {got} want {want}");
    }
    fn shows(&self, it: Item, want: impl AsRef<str>) { assert_eq!(self.ui.value(it), want.as_ref(), "{it:?}"); }
}

fn trigger(t: &mut T) {
    // pulse: the pulse-width fields of the trigger menu
    t.rt(":TRIG:PULS:POL NEG", ":TRIG:PULS:POL?", "NEG");
    t.shows(Item::TrigPol, "Negative");
    t.rt(":TRIG:PULS:WHEN GREater", ":TRIG:PULS:WHEN?", "GRE");
    t.shows(Item::TrigWhen, ">");
    t.rt(":TRIG:PULS:WHEN PLESs", ":TRIG:PULS:WHEN?", "LESS");
    assert_eq!(t.ui.m.trig_pol, Pol::Pos, "PLESs sets the polarity too");
    t.rn(":TRIG:PULS:WIDT 2e-6", ":TRIG:PULS:WIDT?", 2e-6);
    t.shows(Item::TrigT1, si_short(2e-6f32 as f64, "s"));
    t.rn(":TRIG:PULS:LWID 3e-6", ":TRIG:PULS:LWID?", 3e-6);
    assert_eq!(t.go(":TRIG:PULS:WIDT?"), t.go(":TRIG:PULS:LWID?"), "width and lower limit are one field");
    t.rn(":TRIG:PULS:UWID 5e-6", ":TRIG:PULS:UWID?", 5e-6);
    t.shows(Item::TrigT2, si_short(5e-6f32 as f64, "s"));
    t.rn(":TRIG:PULS:LEV 0.25", ":TRIG:PULS:LEV?", 0.25);
    assert_eq!(t.ui.m.trig_level, 0.25);
    t.no(":TRIG:PULS:WIDT 99", "-224");
    t.no(":TRIG:PULS:POL SIDEWAYS", "-224");
    // slope
    t.rt(":TRIG:SLOP:SLOP NEG", ":TRIG:SLOP:SLOP?", "NEG");
    t.shows(Item::TrigSlope, "Falling");
    t.rt(":TRIG:SLOP:WHEN RANG", ":TRIG:SLOP:WHEN?", "RANG");
    t.shows(Item::TrigWhen, "Inside");
    t.rn(":TRIG:SLOP:TIME 1e-6", ":TRIG:SLOP:TIME?", 1e-6);
    t.rn(":TRIG:SLOP:TLOW 2e-6", ":TRIG:SLOP:TLOW?", 2e-6);
    t.rn(":TRIG:SLOP:TUPP 4e-6", ":TRIG:SLOP:TUPP?", 4e-6);
    t.rn(":TRIG:SLOP:ALEV 1.5", ":TRIG:SLOP:ALEV?", 1.5);
    t.rn(":TRIG:SLOP:BLEV -0.5", ":TRIG:SLOP:BLEV?", -0.5);
    t.shows(Item::TrigLevel2, si(-0.5, t.ui.m.unit(t.ui.m.trig_src)));
    // runt
    t.rt(":TRIG:RUNT:POL NEG", ":TRIG:RUNT:POL?", "NEG");
    assert_eq!(t.ui.m.trig_pol, Pol::Neg);
    t.rt(":TRIG:RUNT:WHEN NONE", ":TRIG:RUNT:WHEN?", "NONE");
    t.shows(Item::TrigWhen, "Off");
    t.rt(":TRIG:RUNT:WHEN OUT", ":TRIG:RUNT:WHEN?", "OUT");
    t.rn(":TRIG:RUNT:WLOW 1e-6", ":TRIG:RUNT:WLOW?", 1e-6);
    t.rn(":TRIG:RUNT:WUPP 8e-6", ":TRIG:RUNT:WUPP?", 8e-6);
    // video
    t.rt(":TRIG:VID:STAN NTSC", ":TRIG:VID:STAN?", "NTSC");
    t.shows(Item::VidStd, "NTSC");
    t.rt(":TRIG:VID:MODE ODDField", ":TRIG:VID:MODE?", "ODDF");
    t.shows(Item::VidSync, "Odd");
    t.rt(":TRIG:VID:MODE LINE", ":TRIG:VID:MODE?", "LINE");
    t.rt(":TRIG:VID:LINE 12", ":TRIG:VID:LINE?", "12");
    t.shows(Item::VidLine, "12");
    t.no(":TRIG:VID:LINE 9999", "-224");
    t.rn(":TRIG:VID:LEV 0.1", ":TRIG:VID:LEV?", 0.1);
    // pattern, duration
    let nc = n_ch();
    let pat: String = "HLXRF".chars().cycle().take(nc).collect();
    t.rt(&format!(":TRIG:PATT:PATT \"{pat}\""), ":TRIG:PATT:PATT?", &format!("\"{pat}\""));
    t.shows(Item::Pat(0), "H");
    t.shows(Item::Pat(3.min(nc - 1)), if nc >= 4 { "Rise" } else { "H" });
    t.go(&format!(":TRIG:DURation:PATT \"{}\"", "X".repeat(nc)));
    assert!(t.ui.m.pat[..nc].iter().all(|&b| b == Bit::X));
    t.no(":TRIG:PATT:PATT \"HL\"", "-224");
    t.rn(":TRIG:PATT:LEV 1,0.75", ":TRIG:PATT:LEV? 1", 0.75);
    assert_eq!(t.ui.m.pat_levels[0], 0.75);
    t.rn(":TRIG:DURation:LEV 2,0.5", ":TRIG:DURation:LEV? 2", 0.5);
    t.rt(":TRIG:DUR:WHEN GRE", ":TRIG:DUR:WHEN?", "GRE");
    t.rn(":TRIG:DUR:TLOW 1e-6", ":TRIG:DUR:TLOW?", 1e-6);
    t.rn(":TRIG:DUR:TUPP 2e-6", ":TRIG:DUR:TUPP?", 2e-6);
    // window
    t.rt(":TRIG:WIND:SLOP RFAL", ":TRIG:WIND:SLOP?", "RFAL");
    t.rt(":TRIG:WIND:POS EXIT", ":TRIG:WIND:POS?", "EXIT");
    t.shows(Item::WinModeI, "Exit");
    t.go(":TRIG:WIND:POS INSide");
    t.shows(Item::WinModeI, "Inside");
    t.rn(":TRIG:WIND:ALEV 0.5", ":TRIG:WIND:ALEV?", 0.5);
    t.rn(":TRIG:WIND:BLEV -0.5", ":TRIG:WIND:BLEV?", -0.5);
    t.rt(":TRIG:WIND:WHEN LESS", ":TRIG:WIND:WHEN?", "LESS");
    t.rn(":TRIG:WIND:TIME 3e-6", ":TRIG:WIND:TIME?", 3e-6);
    // nth edge
    t.rt(":TRIG:NEDG:SLOP POS", ":TRIG:NEDG:SLOP?", "POS");
    t.rn(":TRIG:NEDG:IDLE 2e-5", ":TRIG:NEDG:IDLE?", 2e-5);
    t.shows(Item::NthIdle, si_short(2e-5f32 as f64, "s"));
    t.rt(":TRIG:NEDG:EDGE 7", ":TRIG:NEDG:EDGE?", "7");
    t.shows(Item::NthN, "7");
    t.rn(":TRIG:NEDG:LEV 0.2", ":TRIG:NEDG:LEV?", 0.2);
    // setup / hold
    t.rn(":TRIG:SHOL:STIM 1e-8", ":TRIG:SHOL:STIM?", 1e-8);
    t.shows(Item::Setup, si_short(1e-8f32 as f64, "s"));
    t.rn(":TRIG:SHOL:HTIM 2e-8", ":TRIG:SHOL:HTIM?", 2e-8);
    t.shows(Item::Hold, si_short(2e-8f32 as f64, "s"));
    t.rt(":TRIG:SHOL:DSO CHAN1", ":TRIG:SHOL:DSO?", "CHAN1");
    t.rt(":TRIG:SHOL:CSO CHAN2", ":TRIG:SHOL:CSO?", "CHAN2");
    assert_eq!(t.ui.m.trig_src2, 1);
    t.rt(":TRIG:SHOL:SLOP NEG", ":TRIG:SHOL:SLOP?", "NEG");
    t.shows(Item::TrigSlope2, "Falling");
    // timeout
    t.rt(":TRIG:TIM:SLOP RFAL", ":TRIG:TIM:SLOP?", "RFAL");
    t.rn(":TRIG:TIM:TIME 1e-3", ":TRIG:TIM:TIME?", 1e-3);
    t.rn(":TRIG:TIM:LEV 0.3", ":TRIG:TIM:LEV?", 0.3);
    // delay
    t.rt(":TRIG:DEL:SLOPA POS", ":TRIG:DEL:SLOPA?", "POS");
    t.rt(":TRIG:DEL:SLOPB NEG", ":TRIG:DEL:SLOPB?", "NEG");
    t.rt(":TRIG:DEL:WHEN OUT", ":TRIG:DEL:WHEN?", "OUT");
    t.rn(":TRIG:DEL:TLOW 1e-6", ":TRIG:DEL:TLOW?", 1e-6);
    t.rn(":TRIG:DEL:TUPP 2e-6", ":TRIG:DEL:TUPP?", 2e-6);
    t.rn(":TRIG:DEL:ALEV 0.4", ":TRIG:DEL:ALEV?", 0.4);
    t.rn(":TRIG:DEL:BLEV 0.6", ":TRIG:DEL:BLEV?", 0.6);
    // bus match
    t.rt(":TRIG:BUS:WHEN DATA", ":TRIG:BUS:WHEN?", "DATA");
    t.shows(Item::TrigBusWhat, "Data");
    t.rt(":TRIG:BUS:VAL 0xA5", ":TRIG:BUS:VAL?", "0xA5");
    t.shows(Item::TrigBusVal, "0xA5");
    t.go(":TRIG:BUS:VAL 255");
    assert_eq!(t.ui.m.trig_bus_val, 255);
    // general level, second level, level 50 %
    t.rn(":TRIG:LEV 0.7", ":TRIG:LEV?", 0.7);
    t.rn(":TRIG:LEV2 -0.7", ":TRIG:LEV2?", -0.7);
    t.go(":TRIG:EDGE:SOUR CHAN1;:TRIG:EDGE:LEV 5");
    t.ui.stats[0].valid = true;
    (t.ui.stats[0].vmax, t.ui.stats[0].vmin) = (2.0, 0.0);
    t.go(":TRIG:LEV:AUTO");
    assert_eq!(t.ui.m.trig_level, 1.0, "level 50 % is the middle of the signal, as the Trig50 action");
    // the typed commands do not change the trigger type
    assert_eq!(t.ui.m.trig_type, TrigType::Edge);
}

fn history_search(t: &mut T) {
    // history: stored records of the acquisition
    let rec = t.ui.last[0].clone().expect("a record");
    t.ui.history.clear();
    for _ in 0..4 { t.ui.history.push_back(rec.clone()); }
    t.rt(":ACQ:HIST:COUN?", ":ACQ:HIST:COUN?", "4");
    t.rt(":ACQ:HIST:ENAB ON", ":ACQ:HIST:ENAB?", "1");
    assert_eq!(t.ui.hist_pos, Some(3), "the newest record");
    t.rt(":ACQ:HIST:FRAM 2", ":ACQ:HIST:FRAM?", "2");
    assert_eq!(t.ui.hist_pos, Some(1));
    t.go(":ACQ:HIST:NEXT");
    assert_eq!(t.go(":ACQ:HIST:FRAM?"), "3");
    t.go(":ACQ:HIST:PREV");
    t.go(":ACQ:HIST:PREV");
    assert_eq!(t.go(":ACQ:HIST:FRAM?"), "1");
    t.no(":ACQ:HIST:FRAM 9", "-224");
    t.rt(":ACQ:HIST:ENAB OFF", ":ACQ:HIST:ENAB?", "0");
    assert_eq!(t.ui.hist_pos, None);
    assert_eq!(t.go(":ACQ:HIST:FRAM?"), "0");
    t.ui.history.clear();
    t.no(":ACQ:HIST:ENAB ON", "-221");
    // search: mark events
    t.rt(":SEAR:STAT ON", ":SEAR:STAT?", "1");
    assert!(t.ui.search_on, "the Mark events item");
    t.shows(Item::SearchOn, "On");
    t.rt(":SEAR:MODE EDGE", ":SEAR:MODE?", "EDGE");
    t.go(":TRIG:EDGE:SOUR CHAN1;:TRIG:EDGE:SLOP POS;:TRIG:EDGE:LEV 0");
    assert_eq!(t.go(":SEAR:EDGE:SOUR?"), "CHAN1", "without a search of its own: the trigger's");
    t.rt(":SEAR:EDGE:SOUR CHAN2", ":SEAR:EDGE:SOUR?", "CHAN2");
    assert!(t.ui.studio.search.is_some_and(|s| s.src == WaveId::Ch(1)));
    t.rt(":SEAR:EDGE:SLOP NEG", ":SEAR:EDGE:SLOP?", "NEG");
    t.shows(Item::SrchSlope, "Falling");
    t.rn(":SEAR:EDGE:LEV 0.25", ":SEAR:EDGE:LEV?", 0.25);
    t.go(":SEAR:EDGE:LEV:AUTO");
    let mid = t.ui.studio.search.unwrap().level;
    assert!(mid.is_finite());
    t.go(":SEAR:COPY:FROM TRIG");
    assert_eq!(t.ui.studio.search.unwrap().src, WaveId::Ch(0));
    t.go(":TRIG:EDGE:SOUR CHAN1;:SEAR:EDGE:SOUR CHAN2;:SEAR:EDGE:LEV 0.125;:SEAR:EDGE:SLOP NEG;:SEAR:COPY:TO TRIG");
    assert_eq!((t.ui.m.trig_src, t.ui.m.trig_level, t.ui.m.slope), (1, 0.125, Slope::Fall), "Srch to trigger");
    t.no(":SEAR:COPY:TO SOMEWHERE", "-224");
    let n: usize = t.go(":SEAR:COUN?").parse().unwrap();
    assert_eq!(n, t.ui.search_events().len());
    if n > 0 {
        t.rt(":SEAR:EVEN 1", ":SEAR:EVEN?", "1");
        assert_eq!(t.go(":SEAR:DATA?").lines().count(), n);
    } else {
        t.no(":SEAR:EVEN 1", "-224");
        t.raw(":SEAR:EVEN 1");
    }
    let _ = t.go(":SEAR:DATA?");
    t.go(":SEAR:SAVE");
    t.go(":SEAR:REM");
    assert!(t.ui.studio.search.is_none());
}

fn measure_channel_math(t: &mut T) {
    // measurement copy and remove
    t.go(":MEAS:CLEAR;:MEAS:ITEM VPP,CHAN1;:MEAS:ITEM FREQ,CHAN1");
    t.go(":MEAS:SOUR CHAN1");
    t.go(":MEAS:COPY CHAN2");
    assert!(t.ui.m.meas.iter().any(|&(_, id)| id == WaveId::Ch(1)), "Copy to: the menu's copy_measurements");
    t.no(":MEAS:COPY CHAN1", "-224");
    let n = t.ui.measurement_count();
    t.go(":MEAS:ITEM:REM VPP,CHAN2");
    assert_eq!(t.ui.measurement_count(), n - 1);
    t.go(":MEAS:ITEM:REM 1");
    assert_eq!(t.ui.measurement_count(), n - 2);
    t.no(":MEAS:ITEM:REM 99", "-224");
    // zero bias
    t.go(":CHAN1:BIAS 0");
    t.ui.last[0].as_mut().unwrap().ch[0].iter_mut().for_each(|v| *v = 0.5);
    t.go(":CHAN1:BIAS:ZERO");
    assert!((t.ui.m.ch[0].bias - 0.5).abs() < 1e-5, "the Zero this input action: bias from the mean");
    // analysis deskew
    t.rn(":CAL:DESK CHAN2,1e-9", ":CAL:DESK? CHAN2", 1e-9);
    assert_eq!(t.ui.m.deskew_ch[1], 1e-9);
    t.rn(":CAL:DESK R3,2e-9", ":CAL:DESK? R3", 2e-9);
    assert_eq!(t.ui.m.deskew_ref[2], 2e-9);
    t.no(":CAL:DESK CHAN1,5000", "-224");
    t.go(":CAL:DESK:CLE");
    assert!(t.ui.m.deskew_ch.iter().chain(t.ui.m.deskew_ref.iter()).all(|&d| d == 0.0), "Clear deskew");
    t.go(":CAL:DESK:AUTO");
    // math auto scale and linear
    t.go(":MATH1:OPER ADD;:MATH1:SOUR1 CHAN1;:MATH1:SOUR2 CHAN2;:MATH1:DISP ON");
    t.go(":MATH1:SCAL 1e-3");
    t.go(":MATH1:AUTO");
    t.rn(":MATH1:LIN:A 2", ":MATH1:LIN:A?", 2.0);
    t.rn(":MATH1:LIN:B 0.5", ":MATH1:LIN:B?", 0.5);
    assert_eq!(t.ui.m.math_ch(0).lin, (2.0, 0.5));
    t.rn(":MATH2:LIN:A 3", ":MATH2:LIN:A?", 3.0);
    assert_eq!(t.ui.m.math_ch(1).lin.0, 3.0, "another channel than the selected one");
    // eye and spectrum actions
    t.go(":EYE:CLE");
    assert!(t.ui.azr.eye.is_none());
    t.go(":EYE:AUTO");
    t.go(":SPEC:PEAK:CENT");
    t.go(":SPEC:LIM:SET");
    t.go(":SPEC:LIM:CLE");
    assert!(t.ui.azr.limit.is_none());
    t.rn(":SPEC:LIM:MARG 12", ":SPEC:LIM:MARG?", 12.0);
    t.shows(Item::SpecMargin, "12 dB");
    t.no(":SPEC:LIM:MARG 90", "-224");
}

fn reference_mask_sim(t: &mut T) {
    t.ui.refs[0] = Some(RefWave { src: 0, color: 0, label: "r".into(), t0: 0.0, dt: 1e-9, v: (0..200).map(|i| if i % 20 < 10 { -1.0 } else { 1.0 }).collect::<Vec<f32>>().into(), vpd: 1.0, offset: 0.0, on: true });
    t.ui.ref_sel = 0;
    // export: to a named file and to the slot's own file
    let dir = t.ui.store_dir.clone();
    let p = dir.join("export.csv");
    t.go(&format!(":REF:EXP \"{}\"", p.display()));
    assert!(std::fs::read_to_string(&p).unwrap().starts_with("Time (s),Volts"));
    t.go(":REF:EXP");
    assert!(t.ui.setup_path().with_file_name("ref1.csv").is_file());
    // look: take over, offline waveforms, live
    t.rt(":REF:LOOK TAKE", ":REF:LOOK?", "TAKE");
    assert_eq!(t.ui.look, reflook::RefLook::TakeOver);
    t.rt(":REF:LOOK OFFL", ":REF:LOOK?", "OFFL");
    t.rt(":REF:LOOK LIVE", ":REF:LOOK?", "LIVE");
    // per-reference horizontal (Studio state)
    t.rt(":REF:HOR:MODE OWN", ":REF:HOR:MODE?", "OWN");
    assert!(t.ui.studio.sz.ref_h.contains_key(&0));
    t.rn(":REF:HOR:SCAL 2e-7", ":REF:HOR:SCAL?", 2e-7);
    assert_eq!(t.ui.studio.sz.ref_h[&0].0, 2e-7);
    t.rn(":REF:HOR:OFFS 1e-6", ":REF:HOR:OFFS?", 1e-6);
    assert_eq!(t.ui.studio.sz.ref_h[&0].1, 1e-6);
    t.no(":REF:HOR:FIT", "-221"); // only with stacked Studio lanes
    t.rt(":REF:HOR:MODE FOLL", ":REF:HOR:MODE?", "FOLL");
    assert!(t.ui.studio.sz.ref_h.is_empty());
    t.no(":REF:HOR:SCAL -1", "-224");
    // mask
    t.go(":MASK:CRE:REF R1");
    assert!(t.ui.mask_ref.is_some() && t.ui.m.mask_on, "Create from ref");
    t.no(":MASK:CRE:REF R9", "-221");
    t.rt(":MASK:INV ON", ":MASK:INV?", "1");
    t.shows(Item::MaskInvert, "On");
    t.rt(":MASK:OUTP STOP", ":MASK:OUTP?", "STOP");
    assert!(t.ui.m.mask_stop);
    t.shows(Item::MaskAction, "Stop");
    t.rt(":MASK:OUTP CONT", ":MASK:OUTP?", "CONT");
    let mp = dir.join("mask-test.txt");
    t.go(&format!(":MASK:SAVE \"{}\"", mp.display()));
    assert!(mp.is_file());
    t.go(":MASK:CLE");
    assert!(t.ui.mask_ref.is_none() && !t.ui.m.mask_on);
    t.no(":MASK:SAVE", "-221");
    t.go(&format!(":MASK:LOAD \"{}\"", mp.display()));
    assert!(t.ui.mask_ref.is_some() && t.ui.m.mask_on);
    // SIM actions
    t.go(":SIM:PRES?");
    t.go(":SIM:DEF");
    t.go(":SIM:ADDR");
    t.go(":SIM:LIVE");
    let load = dir.join("sim-load.csv");
    std::fs::write(&load, (0..64).fold(String::from("Time (s),Volts\n"), |s, i| s + &format!("{},{}\n", i as f64 * 1e-9, (i % 8) as f64 * 0.1))).unwrap();
    t.go(":SIM:SLOT R4");
    t.go(&format!(":SIM:LOAD \"{}\"", load.display()));
    assert!(t.ui.refs[3].is_some(), "the file went into the SIM slot");
    t.no(":SIM:LOAD \"/no/such/file.csv\"", "-224");
    // SIM levels, edges, jitter
    t.rn(":SIM:RATE 2.5e9", ":SIM:RATE?", 2.5e9);
    t.rn(":SIM:LEV:HIGH 0.8", ":SIM:LEV:HIGH?", 0.8);
    t.rn(":SIM:LEV:LOW -0.8", ":SIM:LEV:LOW?", -0.8);
    t.rn(":SIM:RISE 4e-11", ":SIM:RISE?", 4e-11);
    t.rn(":SIM:BWL 8e9", ":SIM:BWL?", 8e9);
    t.rn(":SIM:DEEM 3", ":SIM:DEEM?", 3.0);
    t.rn(":SIM:NOIS 0.002", ":SIM:NOIS?", 0.002);
    t.rn(":SIM:JITT:RAND 2e-12", ":SIM:JITT:RAND?", 2e-12);
    t.rn(":SIM:JITT:PER:AMPL 3e-12", ":SIM:JITT:PER:AMPL?", 3e-12);
    t.rn(":SIM:JITT:PER:FREQ 2e6", ":SIM:JITT:PER:FREQ?", 2e6);
    assert_eq!((t.ui.m.sim_rate, t.ui.m.sim_vhigh, t.ui.m.sim_pjhz), (2.5e9, 0.8, 2e6));
    t.shows(Item::SimDeemph, "3.0 dB");
    t.no(":SIM:RATE 1", "-224");
    // clear one slot, then all
    assert!(t.ui.refs[0].is_some() && t.ui.refs[3].is_some());
    t.go(":REF:CLE R1");
    assert!(t.ui.refs[0].is_none() && t.ui.refs[3].is_some(), "Clear slot");
    t.go(":REF:CLE 4");
    assert!(t.ui.refs[3].is_none());
    t.no(":REF:CLE R11", "-224");
    t.ui.refs[5] = t.ui.refs[2].clone().or_else(|| Some(RefWave { src: 0, color: 0, label: String::new(), t0: 0.0, dt: 1e-9, v: vec![0.0f32; 16].into(), vpd: 1.0, offset: 0.0, on: true }));
    t.go(":REF:CLE");
    assert!(t.ui.refs.iter().all(|r| r.is_none()), "Clear all");
}

fn storage_zoom(t: &mut T) {
    let dir = t.ui.store_dir.clone();
    // slot and destination
    t.rt(":SAVE:SLOT 3", ":SAVE:SLOT?", "3");
    t.shows(Item::StoreSlot, "3  (setup3.txt)");
    t.no(":SAVE:SLOT 10", "-224");
    t.rt(":SAVE:DEST DESK", ":SAVE:DEST?", "DESK");
    assert_eq!(t.ui.store_dest, crate::store::StoreDest::Desk);
    t.rt(":SAVE:DEST AUTO", ":SAVE:DEST?", "AUTO");
    t.shows(Item::StoreDest, "Auto");
    // *SAV / *RCL are Save setup / Recall setup of the chosen slot
    t.go(":TIM:SCAL 1e-4;:CHAN2:SCAL 0.5");
    t.go("*SAV 2");
    assert!(t.ui.setup_path().is_file(), "setup2.txt");
    assert_eq!(t.ui.store_slot, 2);
    t.go(":TIM:SCAL 1e-3;:CHAN2:SCAL 2");
    t.go("*RCL 2");
    assert_eq!(t.go(":TIM:SCAL?").parse::<f64>().unwrap(), 1e-4);
    assert_eq!(t.go(":CHAN2:SCAL?").parse::<f64>().unwrap(), 0.5);
    t.no("*SAV 0", "-224");
    // save and load to a named file
    let sp = dir.join("named-setup.txt");
    t.go(&format!(":SAVE:SET \"{}\"", sp.display()));
    t.go(":TIM:SCAL 1e-2");
    t.go(&format!(":LOAD:SET \"{}\"", sp.display()));
    assert_eq!(t.go(":TIM:SCAL?").parse::<f64>().unwrap(), 1e-4);
    t.no(":LOAD:SET \"/no/such/setup.txt\"", "-224");
    // the whole setup as a block, and back
    let block = t.raw(":SYST:SETup?");
    assert!(block.starts_with("#9"), "{}", &block[..block.len().min(20)]);
    let len: usize = block[2..11].parse().unwrap();
    assert_eq!(block.len(), 11 + len);
    t.go(":TIM:SCAL 5e-3;:CHAN3:COUP AC");
    let text = &block[11..];
    // one message holds the block, newlines and all: it is not split
    let msg = format!(":SYSTem:SETup {};:TRIG:STAT?", block);
    let reply = t.raw(&msg);
    let e = t.err();
    assert!(e.starts_with("0,"), "{e}");
    assert!(reply.ends_with("STOP") || reply.ends_with("WAIT") || reply.ends_with("AUTO") || reply.ends_with("TD"), "{reply:?}");
    assert_eq!(t.go(":TIM:SCAL?").parse::<f64>().unwrap(), 1e-4, "the setup block was applied");
    let again = t.go(":SYST:SETup?");
    t.go(&format!(":SYST:SETup {again}"));
    assert_eq!(t.go(":SYST:SETup?"), again, "the setup block is stable through a round trip");
    assert!(text.contains("tdiv = "));
    t.no(":SYSTem:SETup #9000000005hello", "-224");
    // the older :SYSTem:SET is not taken over by the long name
    t.go(":SYST:SET trig_nrej = true");
    assert!(t.ui.m.trig_nrej);
    // waveform, image, fast frame (a new record first: a changed time base dropped the old one)
    t.ui.m.run = RunState::Run;
    for _ in 0..40 { t.ui.step(0.05); }
    t.ui.m.run = RunState::Stop;
    let wp = dir.join("wave-out.csv");
    t.go(&format!(":SAVE:WAV \"{}\"", wp.display()));
    assert!(std::fs::read_to_string(&wp).unwrap().lines().count() > 10);
    let wb = dir.join("wave-out.bin");
    t.go(&format!(":SAVE:WAV \"{}\",BIN", wb.display()));
    assert!(wb.is_file());
    t.no(":SAVE:WAV \"x\",TXT", "-224");
    t.go(":SAVE:WAV");
    let ip = dir.join("image-out.png");
    t.go(&format!(":SAVE:IMAG \"{}\"", ip.display()));
    assert_eq!(t.ui.shot_request.as_deref(), Some(ip.to_string_lossy().as_ref()));
    t.go(":SAVE:IMAG");
    t.go(":SAVE:FFR");
    t.go(":LOAD:FFR");
    // Studio zoom
    t.go(":TIM:DEL:ENAB OFF");
    t.rt(":TIM:DEL:SOUR CHAN2", ":TIM:DEL:SOUR?", "CHAN2");
    assert_eq!(t.ui.studio.sz.lane, Some(workspace::Trace::Channel(1)));
    assert!(t.ui.m.zoom, "choosing the zoom source turns the zoom on");
    t.rt(":TIM:DEL:SOUR R3", ":TIM:DEL:SOUR?", "R3");
    t.rt(":TIM:DEL:SOUR MATH2", ":TIM:DEL:SOUR?", "MATH2");
    t.rt(":TIM:DEL:SOUR ALL", ":TIM:DEL:SOUR?", "ALL");
    assert!(t.ui.studio.sz.lane.is_none());
    t.no(":TIM:DEL:SOUR NOWHERE", "-224");
    t.rn(":TIM:DEL:VZO 4", ":TIM:DEL:VZO?", 4.0);
    assert_eq!(t.ui.zoom_vfactor, 4.0);
    t.go(":TIM:DEL:VZO 1");
    assert_eq!(t.ui.zoom_vfactor, 1.0);
    t.go(":TIM:DEL:VZO 8");
    t.go(":TIM:DEL:VZO:RES");
    assert_eq!(t.ui.zoom_vfactor, 1.0);
    t.go(":TIM:DEL:SCAL 1e-4;:TIM:DEL:OFFS 3e-4");
    t.go(":TIM:DEL:CENT");
    assert_eq!(t.ui.m.zdelay, 0.0, "Center trigger");
    t.go(":TIM:DEL:FIT");
    assert_eq!(t.ui.m.zscale, t.ui.m.tscale, "Fit record");
}

fn bandwidth_logger_analysers(t: &mut T) {
    t.go(":CHAN1:DISP ON;:CHAN2:DISP ON;:CHAN3:DISP ON;:CHAN4:DISP ON");
    // bandwidth test: a view of its own
    t.rt(":BAND:STAT ON", ":BAND:STAT?", "1");
    assert_eq!(t.ui.m.view, AView::Step);
    assert_eq!(t.go(":VIEW?"), "BWTEST");
    t.go(":VIEW SCOP");
    t.go(":VIEW BWT");
    assert_eq!(t.ui.m.view, AView::Step);
    t.rt(":BAND:STAT OFF", ":BAND:STAT?", "0");
    t.rt(":BAND:SOUR CHAN2", ":BAND:SOUR?", "CHAN2");
    assert_eq!(t.ui.m.bw_src, 1);
    t.rn(":BAND:FREQ 98e6", ":BAND:FREQ?", 100e6);
    t.shows(Item::BwHint, si_short(100e6, "Hz"));
    t.go(":BAND:FREQ 0");
    t.shows(Item::BwHint, "Auto");
    t.rt(":BAND:BINS 900", ":BAND:BINS?", "1000");
    t.shows(Item::BwBins, "1000");
    // data logger
    assert_eq!(t.go(":SYST:LOG?"), "0");
    assert_eq!(t.go(":SYST:LOG:FILE?"), "");
    t.go(":MEAS:CLEAR;:MEAS:ITEM VPP,CHAN1");
    t.rt(":SYST:LOG ON", ":SYST:LOG?", "1");
    assert!(t.go(":SYST:LOG:FILE?").contains("log"), "the path a remote user fetches");
    assert!(t.go(":SYST:LOG:INFO?").contains("rows"));
    t.shows(Item::LogOn, "On");
    t.rt(":SYST:LOG OFF", ":SYST:LOG?", "0");
    t.rn(":SYST:LOG:INT 60", ":SYST:LOG:INT?", 60.0);
    t.shows(Item::LogIvl, si_short(60.0, "s"));
    t.rn(":SYST:LOG:INT 4", ":SYST:LOG:INT?", 5.0);
    // spectrum
    t.rt(":SPEC:SOUR CHAN2", ":SPEC:SOUR?", "CH2");
    assert_eq!(t.ui.m.spec_src, WaveId::Ch(1));
    t.rn(":SPEC:CENT 1e6", ":SPEC:CENT?", 1e6);
    t.rn(":SPEC:SPAN 2e6", ":SPEC:SPAN?", 2e6);
    t.rn(":SPEC:RBW 1e3", ":SPEC:RBW?", 1e3);
    t.rt(":SPEC:WIND HANN", ":SPEC:WIND?", "HANN");
    let trace = t.ui.options(Item::SpecTrace)[1].clone();
    t.go(&format!(":SPEC:TRAC \"{trace}\""));
    assert_eq!(t.go(":SPEC:TRAC?"), trace.to_ascii_uppercase().replace(' ', "_"));
    assert_eq!(t.ui.m.spec_trace, 1);
    t.rn(":SPEC:RLEV -20", ":SPEC:RLEV?", -20.0);
    t.rt(":SPEC:WAT ON", ":SPEC:WAT?", "1");
    assert!(t.ui.m.spec_wf);
    t.rt(":SPEC:DIST ON", ":SPEC:DIST?", "1");
    t.go(":SPEC:PEAK:HARM?");
    let harm = t.ui.options(Item::SpecHarm)[1].clone();
    t.go(&format!(":SPEC:PEAK:HARM \"{harm}\""));
    assert_eq!(t.ui.m.spec_harm, 1);
    t.rn(":SPEC:ACPR:WIDT 1e5", ":SPEC:ACPR:WIDT?", 1e5);
    t.rn(":SPEC:ACPR:SPAC 3e5", ":SPEC:ACPR:SPAC?", 3e5);
    t.shows(Item::SpecAcprS, si_short(3e5, "Hz"));
    // eye
    t.rt(":EYE:SOUR CHAN2", ":EYE:SOUR?", "CH2");
    t.rn(":EYE:RATE 1e9", ":EYE:RATE?", 1e9);
    assert!(!t.ui.m.eye_from_record);
    t.rt(":EYE:CLOC OFF", ":EYE:CLOC?", "OFF");
    t.rt(":EYE:CLOC CHAN3", ":EYE:CLOC?", "CH3");
    t.rn(":EYE:BER 1e-9", ":EYE:BER?", 1e-9);
    t.rn(":EYE:PN:LOW 1e4", ":EYE:PN:LOW?", 1e4);
    t.rn(":EYE:PN:HIGH 1e7", ":EYE:PN:HIGH?", 1e7);
    t.rt(":EYE:NPJ ON", ":EYE:NPJ?", "1");
    t.rt(":EYE:RAIL CHAN4", ":EYE:RAIL?", "CH4");
    t.rn(":EYE:NOTC 5e6", ":EYE:NOTC?", 5e6);
    t.rt(":EYE:HPAS ON", ":EYE:HPAS?", "1");
    t.rn(":EYE:PLL:BWID 4e6", ":EYE:PLL:BWID?", 4e6);
    t.rt(":EYE:PLAC SIDE", ":EYE:PLAC?", "SIDE");
    t.shows(Item::EyePlace, "Side");
    // power
    t.rt(":POW:VOLT:SOUR CHAN1", ":POW:VOLT:SOUR?", "CH1");
    t.rt(":POW:CURR:SOUR CHAN2", ":POW:CURR:SOUR?", "CH2");
    let page = t.ui.options(Item::PowMode)[1].clone();
    t.go(&format!(":POW:PAGE \"{page}\""));
    assert_eq!(t.go(":POW:PAGE?"), page.to_ascii_uppercase().replace(' ', "_"));
    t.rn(":POW:FLIN 60", ":POW:FLIN?", 60.0);
    t.rn(":POW:FSW 1e5", ":POW:FSW?", 1e5);
    t.rt(":POW:VIN:SOUR CHAN3", ":POW:VIN:SOUR?", "CH3");
    t.rn(":POW:REF 230", ":POW:REF?", 230.0);
    t.go(":POW:REF 0");
    assert_eq!(t.ui.m.pw_ref, None);
    t.rt(":POW:CLOC:SOUR CHAN4", ":POW:CLOC:SOUR?", "CH4");
    // magnetic
    t.rt(":MAG:VOLT:SOUR CHAN1", ":MAG:VOLT:SOUR?", "CH1");
    t.rt(":MAG:CURR:SOUR CHAN2", ":MAG:CURR:SOUR?", "CH2");
    t.rn(":MAG:TURN 12", ":MAG:TURN?", 12.0);
    t.rn(":MAG:AREA 1e-5", ":MAG:AREA?", 1e-5);
    t.rn(":MAG:LENG 0.05", ":MAG:LENG?", 0.05);
    t.rn(":MAG:RCU 0.1", ":MAG:RCU?", 0.1);
    t.rt(":MAG:PLOT B-H", ":MAG:PLOT?", "B-H");
    t.rt(":MAG:CURS 0", ":MAG:CURS?", "0");
    // inverter
    t.rt(":INV:PAGE Mechanical", ":INV:PAGE?", "MECHANICAL");
    t.rt(":INV:PAGE DQ0", ":INV:PAGE?", "DQ0");
    t.rt(":INV:PHAS1:SOUR CHAN1", ":INV:PHAS1:SOUR?", "CH1");
    t.rt(":INV:PHAS2:SOUR CHAN2", ":INV:PHAS2:SOUR?", "CH2");
    t.rt(":INV:PHAS3:SOUR CHAN3", ":INV:PHAS3:SOUR?", "CH3");
    t.rt(":INV:ANGL:SOUR Phase", ":INV:ANGL:SOUR?", "PHASE");
    t.rt(":INV:POLE 6", ":INV:POLE?", "6");
    t.rt(":INV:ANAL:SOUR CHAN4", ":INV:ANAL:SOUR?", "CH4");
    t.rn(":INV:ANAL:SCAL 2", ":INV:ANAL:SCAL?", 2.0);
    t.rn(":INV:ANAL:DEGR 90", ":INV:ANAL:DEGR?", 90.0);
    t.rn(":INV:ANGL:OFFS 12", ":INV:ANGL:OFFS?", 12.0);
    t.rt(":INV:QUAN Current", ":INV:QUAN?", "CURRENT");
    t.rt(":INV:TORQ:SOUR CHAN2", ":INV:TORQ:SOUR?", "CH2");
    t.rt(":INV:BUS:VOLT:SOUR CHAN1", ":INV:BUS:VOLT:SOUR?", "CH1");
    t.rt(":INV:BUS:CURR:SOUR CHAN2", ":INV:BUS:CURR:SOUR?", "CH2");
    t.rt(":INV:ELEC Elec", ":INV:ELEC?", "ELEC");
    t.rn(":INV:ENC:CPR 1024", ":INV:ENC:CPR?", 1024.0);
    t.rt(":INV:ENC:SWAP ON", ":INV:ENC:SWAP?", "1");
    t.rt(":INV:ENC:A 3", ":INV:ENC:A?", "3");
    t.rt(":INV:ENC:B 4", ":INV:ENC:B?", "4");
    t.rt(":INV:HALL1:LINE 5", ":INV:HALL1:LINE?", "5");
    t.rt(":INV:HALL2:LINE 6", ":INV:HALL2:LINE?", "6");
    t.rt(":INV:HALL3:LINE 7", ":INV:HALL3:LINE?", "7");
    assert_eq!((t.ui.m.inv_poles, t.ui.m.inv_qa, t.ui.m.inv_hc), (6, 3, 7));
    // double pulse
    t.rt(":DPT:VDS:SOUR CHAN1", ":DPT:VDS:SOUR?", "CH1");
    t.rt(":DPT:ID:SOUR CHAN2", ":DPT:ID:SOUR?", "CH2");
    t.rt(":DPT:VGS:SOUR CHAN3", ":DPT:VGS:SOUR?", "CH3");
    t.rt(":DPT:FOUR Gate", ":DPT:FOUR?", "GATE");
    t.rt(":DPT:FOUR:SOUR CHAN4", ":DPT:FOUR:SOUR?", "CH4");
    t.rn(":DPT:GATE 20", ":DPT:GATE?", 20.0);
    t.shows(Item::DptPct, "20 %");
    t.rt(":DPT:ZOOM Turn-on", ":DPT:ZOOM?", "TURN-ON");
    // bode
    t.rn(":BODE:STAR 100", ":BODE:STAR?", 100.0);
    t.rn(":BODE:STOP 1e6", ":BODE:STOP?", 1e6);
    t.rt(":BODE:POIN 50", ":BODE:POIN?", "50");
    t.rn(":BODE:AMPL 0.5", ":BODE:AMPL?", 0.5);
    t.rt(":BODE:INP CHAN1", ":BODE:INP?", "CH1");
    t.rt(":BODE:OUTP CHAN2", ":BODE:OUTP?", "CH2");
    t.rn(":BODE:SWE:TIME 2", ":BODE:SWE:TIME?", 2.0);
    t.rt(":BODE:SWE:LOG Log", ":BODE:SWE:LOG?", "LOG");
    t.rt(":BODE:SYNC:SOUR CH3", ":BODE:SYNC:SOUR?", "CH3");
    t.rt(":BODE:SYNC:EDGE Falling", ":BODE:SYNC:EDGE?", "FALLING");
    t.rn(":BODE:SYNC:LEV 0.3", ":BODE:SYNC:LEV?", 0.3);
    t.rn(":BODE:REFL -10", ":BODE:REFL?", -10.0);
    assert_eq!(t.go(":BODE:RUN?"), "0");
    assert_eq!(t.ui.m.bode_points, 50);
    t.no(":BODE:POIN 1000", "-224");
    t.go(":BODE:RUN OFF");
}

fn system(t: &mut T) {
    // date and time
    t.go(":SYST:DATE 2026,11,16");
    assert_eq!(t.go(":SYST:DATE?").len(), 10);
    t.go(":SYST:TIME 12,30,15");
    assert_eq!(t.go(":SYST:TIME?").len(), 8);
    t.no(":SYST:DATE 2026,13,40", "-224");
    t.no(":SYST:TIME 25,0,0", "-224");
    t.no(":SYST:DATE 2026,11", "-224");
    t.rt(":SYST:TZON UTC0", ":SYST:TZON?", "UTC0");
    assert_eq!(t.ui.sys.tz, "UTC0");
    t.rt(":SYST:NTP:ENAB ON", ":SYST:NTP:ENAB?", "1");
    t.shows(Item::NtpOn, "On");
    t.rt(":SYST:NTP:SERV pool.ntp.org", ":SYST:NTP:SERV?", "pool.ntp.org");
    t.go(":SYST:NTP:SERV DHCP");
    assert_eq!(t.go(":SYST:NTP:SERV?"), crate::sysconf::DHCP);
    t.no(":SYST:NTP:SERV \"not a server!\"", "-224");
    assert!(!t.go(":SYST:NTP:STATus?").is_empty());
    t.go(":SYST:NTP:ENAB OFF");
    t.go(":SYST:NTP:SYNC");
    // network info
    let _ = t.go(":SYST:COMM:LAN:IPAD?");
    let _ = t.go(":SYST:COMM:LAN:MAC?");
    let _ = t.go(":SYST:COMM:LAN:STAT?");
    t.rt(":SYST:COMM:LAN:RETR10 ON", ":SYST:COMM:LAN:RETR10?", "1");
    t.shows(Item::NetRetry10, "On");
    let _ = t.go(":SYST:WLAN:STAT?");
    let _ = t.go(":SYST:WLAN:NETW?");
    let _ = t.go(":SYST:WLAN:SAV?");
    t.go(":SYST:WLAN:SCAN");
    t.no(":SYST:WLAN:FORG 1", "-224");
    // remote services: the same switches as the Remote services page
    t.rt(":SYST:REM:SERV:WEB OFF", ":SYST:REM:SERV:WEB?", "0");
    assert!(!t.ui.sys.remote_web);
    t.shows(Item::RemWeb, "Off");
    t.rt(":SYST:REM:SERV:VXI11 OFF", ":SYST:REM:SERV:VXI11?", "0");
    t.rt(":SYST:REM:SERV:TELN OFF", ":SYST:REM:SERV:TELN?", "0");
    t.rt(":SYST:REM:SERV:MDNS OFF", ":SYST:REM:SERV:MDNS?", "0");
    t.rt(":SYST:REM:SERV:WEB ON", ":SYST:REM:SERV:WEB?", "1");
    t.no(":SYST:REM:SERV:SCPI OFF", "-221");
    assert!(t.ui.sys.remote_scpi, "SCPI stays on without CONFIRM");
    t.rt(":SYST:REM:SERV:SCPI ON", ":SYST:REM:SERV:SCPI?", "1");
    t.go(":SYST:REM:SERV:SCPI OFF,CONFIRM");
    assert!(!t.ui.sys.remote_scpi);
    t.go(":SYST:REM:SERV:SCPI ON");
    t.rt(":SYST:NAME \"bench-scope\"", ":SYST:NAME?", "bench-scope");
    t.no(":SYST:NAME \"not valid!\"", "-224");
    // backlight, display
    t.rt(":DISP:BACK 55", ":DISP:BACK?", "55");
    t.shows(Item::Backlight, "55 %");
    t.no(":DISP:BACK 1", "-224");
    t.rt(":DISP:HDMI Waveform", ":DISP:HDMI?", "WAVEFORM");
    t.shows(Item::Hdmi, "Waveform");
    let size = t.ui.options(Item::TextSize).last().unwrap().clone();
    t.go(&format!(":DISP:TEXT \"{size}\""));
    assert_eq!(t.ui.value(Item::TextSize), size);
    assert_eq!(t.go(":DISP:TEXT?"), size.to_ascii_uppercase().replace(' ', "_"));
    let layout = t.ui.value(Item::Layout);
    t.go(&format!(":DISP:LAY \"{layout}\""));
    assert_eq!(t.go(":DISP:LAY?"), layout.to_ascii_uppercase().replace(' ', "_"));
    t.rt(":SYST:LOCK ON", ":SYST:LOCK?", "1");
    assert!(t.ui.m.touch_lock);
    t.go(":SYST:LOCK OFF");
    assert_eq!(t.go(":SESS?"), "LIVE");
    t.go(":SESS LIVE");
    t.no(":SESS SIM", "-224");
    assert!(t.go(":SYST:INFO?").contains("Model,"));
    // the commands that need CONFIRM
    t.no(":SYST:POW:OFF", "-221");
    t.no(":SYST:POW:REST", "-221");
    assert_eq!(t.ui.power_action, None, "refused: nothing was asked of the main loop");
    t.go(":SYST:POW:OFF CONFIRM");
    assert_eq!(t.ui.power_action, Some(PowerAction::Off), "the main loop would carry it out");
    t.ui.power_action = None;
    t.go(":SYST:POW:REST CONFIRM");
    assert_eq!(t.ui.power_action, Some(PowerAction::Restart));
    t.ui.power_action = None;
    assert!(!t.go(":SYST:UPD:STAT?").is_empty());
    t.no(":SYST:UPD:ROLL", "-221");
    t.no(":SYST:UPD:INST \"/tmp/none.oosupd\"", "-221");
    t.no(":SYST:UPD:INST \"/no/such/package\",CONFIRM", "-224");
    let boot = t.go(":SYST:BOOT?");
    assert!(boot == "ONES" || boot == "PERS");
    t.go(&format!(":SYST:BOOT {boot}"));
    let other = if boot == "ONES" { "PERSist" } else { "ONEShot" };
    t.no(&format!(":SYST:BOOT {other}"), "-221");
    let hz: f64 = t.go(":SYST:OVER?").parse().unwrap();
    t.go(&format!(":SYST:OVER {hz}"));
    let other_hz = board().clock_steps().iter().copied().find(|&s| s != hz).unwrap();
    t.no(&format!(":SYST:OVER {other_hz}"), "-221");
    assert_eq!(t.go(":SYST:OVER?").parse::<f64>().unwrap(), hz, "refused, unchanged");
    t.no(":SYST:STOC", "-221");
}


fn extras(t: &mut T) {
    // decoder parameters: another decoder than the selected one, which stays selected
    let norm = |s: &str| s.to_ascii_uppercase().replace(' ', "_");
    assert_eq!(t.ui.m.dec_sel, 0);
    t.rt(":BUS1:SPI:MODE 2", ":BUS1:SPI:MODE?", "2");
    assert_eq!(t.ui.m.dec_mode, 2, "the selected decoder's field the Mode item edits");
    t.shows(Item::DecMode, "2");
    let opt = |t: &T, it: Item, i: usize| t.ui.options(it)[i].clone();
    let rate = opt(t, Item::DecCanRate, 1);
    t.rt(&format!(":BUS2:CAN:BAUD \"{rate}\""), ":BUS2:CAN:BAUD?", &norm(&rate));
    assert_eq!(t.ui.m.dec_view(1).dec_canrate, 1);
    assert_eq!(t.ui.m.dec_sel, 0, "the selected decoder is the same afterwards");
    t.rt(":BUS2:CAN:SIGN CAN_H", ":BUS2:CAN:SIGN?", "CAN_H");
    assert!(t.ui.m.dec_view(1).dec_canh);
    let fd = opt(t, Item::DecFdRate, 2);
    t.rt(&format!(":BUS3:CAN:FD:BAUD \"{fd}\""), ":BUS3:CAN:FD:BAUD?", &norm(&fd));
    let ar = opt(t, Item::DecArate, 1);
    t.rt(&format!(":BUS3:CAN:FD:ABAU \"{ar}\""), ":BUS3:CAN:FD:ABAU?", &norm(&ar));
    let tick = opt(t, Item::DecTick, 1);
    t.rt(&format!(":BUS4:SENT:TICK \"{tick}\""), ":BUS4:SENT:TICK?", &norm(&tick));
    t.rt(":BUS4:SENT:NIBB 3", ":BUS4:SENT:NIBB?", "3");
    assert_eq!(t.ui.m.dec_view(3).dec_nibbles, 3);
    let sp = opt(t, Item::DecUsbSpeed, 1);
    t.rt(&format!(":BUS1:USB:SPE \"{sp}\""), ":BUS1:USB:SPE?", &norm(&sp));
    t.no(":BUS9:SPI:MODE 1", "-113");
    let with_options = PROTOS.iter().copied().find(|p| p.module().is_some_and(|m| m.options().iter().any(|o| o.choices.len() > 1)));
    if let Some(p) = with_options {
        t.ui.set_dec_proto(p);
        let n = p.module().unwrap().options().iter().take_while(|o| o.choices.len() > 1).count().min(4);
        for k in 0..n {
            let o = opt(t, Item::DecOpt(k), 1);
            let h = format!(":BUS1:OPT{}", k + 1);
            t.rt(&format!("{h} \"{o}\""), &format!("{h}?"), &norm(&o));
        }
    }
    for k in 1..=4 { let _ = t.raw(&format!(":BUS1:OPT{k}?")); }
    // plot bus lane
    t.rt(":BUS:LANE:STAT ON", ":BUS:LANE:STAT?", "1");
    assert!(t.ui.m.bus.on);
    t.rt(":BUS:LANE:LAB \"SENSOR\"", ":BUS:LANE:LAB?", "SENSOR");
    assert_eq!(t.ui.m.bus.label, "SENSOR");
    t.rn(":BUS:LANE:POS 1.5", ":BUS:LANE:POS?", 1.5);
    t.no(":BUS:LANE:POS 9", "-224");
    let clk = opt(t, Item::BusClk, 0);
    t.rt(&format!(":BUS:LANE:CLOC:SOUR \"{clk}\""), ":BUS:LANE:CLOC:SOUR?", &norm(&clk));
    let data = opt(t, Item::BusData, 1);
    t.rt(&format!(":BUS:LANE:DATA:SOUR \"{data}\""), ":BUS:LANE:DATA:SOUR?", &norm(&data));
    t.rn(":BUS:LANE:CLOC:THR 1.2", ":BUS:LANE:CLOC:THR?", 1.2);
    t.rn(":BUS:LANE:DATA:THR 0.9", ":BUS:LANE:DATA:THR?", 0.9);
    t.rt(":BUS:LANE:RW Included", ":BUS:LANE:RW?", "INCLUDED");
    assert!(t.ui.m.bus.addr_rw);
    t.go(":DEC:SAVE");
    // logic analyser details
    t.rt(":LA:DIG3:PATT H", ":LA:DIG3:PATT?", "H");
    assert_eq!(t.ui.m.pat_la[3], Bit::H);
    t.shows(Item::LaPat(3), "H");
    let preset = opt(t, Item::LaThr(0), 1);
    t.rt(&format!(":LA:POD1:PRES \"{preset}\""), ":LA:POD1:PRES?", &norm(&preset));
    assert_eq!(t.ui.m.la.preset[0], 1);
    let size = opt(t, Item::LaHeight, 1);
    t.rt(&format!(":LA:SIZE \"{size}\""), ":LA:SIZE?", &norm(&size));
    t.no(":LA:POD3:PRES TTL", "-113");
    // zones
    t.rt(":ZONE1:STAT ON", ":ZONE1:STAT?", "1");
    assert!(t.ui.m.zones[0].on);
    t.rt(":ZONE1:SOUR CHAN2", ":ZONE1:SOUR?", "CHAN2");
    assert_eq!(t.ui.m.zones[0].src, 1);
    t.rt(":ZONE1:COND NOT", ":ZONE1:COND?", "NOT");
    assert!(!t.ui.m.zones[0].inside);
    t.go(":ZONE1:BOX 1e-6,0.5,2e-6,-0.5");
    assert_eq!(t.ui.m.zones[0].pts, vec![(1e-6, 0.5), (2e-6, -0.5)]);
    assert_eq!(t.go(":ZONE1:BOX?").split(',').count(), 4);
    t.go(":ZONE2:POIN 0,0,1e-6,0,1e-6,1,0,1");
    assert_eq!(t.ui.m.zones[1].pts.len(), 4, "a polygon");
    assert_eq!(t.go(":ZONE2:POIN?").split(',').count(), 8);
    t.no(":ZONE2:BOX?", "-221");
    t.no(":ZONE2:POIN 1,2", "-224");
    t.go(":ZONE2:CLE");
    assert!(t.ui.m.zones[1].pts.is_empty());
    t.no(":ZONE3:STAT ON", "-113");
    // generator wave from a file
    let arb = t.ui.store_dir.join("arb-test.csv");
    std::fs::write(&arb, (0..64).map(|i| format!("{}\n", (i as f64 * 0.2).sin())).collect::<String>()).unwrap();
    t.go(&format!(":SOUR:ARB:LOAD \"{}\"", arb.display()));
    t.go(&format!(":SOUR1:ARB:LOAD \"{}\"", arb.display()));
    t.no(":SOUR:ARB:LOAD \"/no/such/wave.csv\"", "-221");
    // small rows
    t.rt(":CHAN:ACT 2", ":CHAN:ACT?", "2");
    assert_eq!(t.ui.m.active, 1);
    t.no(":CHAN:ACT 9", "-224");
    let quick = opt(t, Item::QuickAct, 1);
    t.rt(&format!(":SYST:QUIC \"{quick}\""), ":SYST:QUIC?", &norm(&quick));
    let pane = opt(t, Item::PaneSel, 1);
    t.rt(&format!(":DISP:LOW \"{pane}\""), ":DISP:LOW?", &norm(&pane));
    assert_eq!(t.ui.m.pane, 1);
    let trend = t.ui.options(Item::TrendSel(0));
    if trend.len() > 1 {
        t.go(&format!(":DISP:TREN1 \"{}\"", trend[1]));
        assert_eq!(t.go(":DISP:TREN1?"), norm(&trend[1]));
        t.go(&format!(":DISP:TREN2 \"{}\"", trend[1]));
        assert_eq!(t.go(":DISP:TREN2?"), norm(&trend[1]));
    } else {
        let _ = (t.raw(":DISP:TREN1?"), t.raw(":DISP:TREN2?"));
    }
}
#[test]
fn every_new_header_is_set_and_queried_and_changes_what_the_menu_changes() {
    let mut t = T::new();
    trigger(&mut t);
    history_search(&mut t);
    measure_channel_math(&mut t);
    reference_mask_sim(&mut t);
    storage_zoom(&mut t);
    bandwidth_logger_analysers(&mut t);
    system(&mut t);
    extras(&mut t);
    catalogue_and_inverter_index(&mut t);
    let _ = std::fs::remove_dir_all(&t.ui.store_dir);
    let missing: Vec<&str> = all_headers().filter(|h| !t.used.contains(h)).collect();
    assert!(missing.is_empty(), "headers no test exercised: {missing:?}");
}

#[test]
fn every_new_header_is_documented() {
    let doc = include_str!("../../docs/scpi.md");
    let missing: Vec<&str> = all_headers().filter(|h| !doc.contains(&format!("`{}{}", if h.starts_with('*') { "" } else { ":" }, h))).collect();
    assert!(missing.is_empty(), "headers missing from docs/scpi.md: {missing:?}");
}

#[test]
fn the_setup_keys_are_documented() {
    let doc = include_str!("../../docs/scpi.md");
    let text = crate::store::setup_text(&Model::default());
    let mut missing = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let key = line.split('=').next().unwrap().trim();
        // the keys of a channel, a math channel, a decoder, a zone, a window slot carry their number: `ch1.vdiv` is `chN.vdiv`
        let generic: String = key.chars().map(|c| if c.is_ascii_digit() { 'N' } else { c }).collect();
        if !(doc.contains(&format!("`{key}`")) || doc.contains(&format!("`{generic}`")) || doc.contains(&format!("`{}`", generic.replace('N', "<n>")))) { missing.push(key.to_string()); }
    }
    assert!(missing.is_empty(), "setup keys missing from docs/scpi.md: {missing:?}");
}

#[test]
fn a_block_argument_is_not_split_at_its_newlines_or_semicolons() {
    let p = crate::scpi::Program::new(":SYST:SETup #9000000010a=1;b\nc=2\n;:RUN;:STOP\n:CLEAR");
    // the block holds "a=1;b\nc=2\n" (11 bytes); the commands around it are separate
    let mut cmds = 0;
    let mut p = p;
    let mut out = Vec::new();
    p.immediate(&mut |c| { out.push(c.to_string()); Vec::new() });
    cmds += out.len();
    assert_eq!(cmds, 4, "{out:?}");
    assert!(out[0].ends_with("a=1;b\nc=2\n") || out[0].contains("a=1;b\nc=2"), "{:?}", out[0]);
    // the TCP framing waits for the whole block
    let mut buf = b":SYST:SETup #9000000010a=1;b\nc".to_vec();
    assert_eq!(crate::scpi::message_end(&buf), None);
    buf.extend_from_slice(b"=2\n\n:RUN\n");
    let end = crate::scpi::message_end(&buf).unwrap();
    assert_eq!(&buf[end - 3..=end], b"=2\n\n", "the newline after the block ends the message");
    assert_eq!(crate::scpi::message_end(b":RUN\n:STOP\n"), Some(4));
    assert_eq!(crate::scpi::message_end(b":RUN"), None);
}


#[test]
fn no_header_is_shadowed_by_another_one() {
    // a header written with the short form of every keyword must find itself, not a neighbour with the same short form
    for h in all_headers() {
        let nodes: Vec<String> = h.split(':').map(|p| {
            let base = p.strip_suffix("<n>");
            let short: String = base.unwrap_or(p).chars().filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '*').collect();
            if base.is_some() { format!("{short}1") } else { short }
        }).collect();
        let refs: Vec<&str> = nodes.iter().map(String::as_str).collect();
        let found = resolve(&refs).map(|r| r.0);
        // the one long-form-only header keeps the older :SYSTem:SET for its short form
        if h == "SYSTem:SETup" { assert_eq!(found, None); continue; }
        assert_eq!(found, Some(h), "{nodes:?}");
    }
}

fn catalogue_and_inverter_index(t: &mut T) {
    t.go(":CHAN1:DISP ON;:CHAN2:DISP ON");
    t.rt(":INV:ENC:IND 5", ":INV:ENC:IND?", "5");
    assert_eq!(t.ui.m.inv_qi, Some(5));
    t.rt(":INV:ENC:IND OFF", ":INV:ENC:IND?", "OFF");
    t.no(":INV:ENC:IND 99", "-224");
    t.rt(":MEAS:CAT:SOURC CHAN3", ":MEAS:CAT:SOURC?", "CH3");
    t.rt(":MEAS:CAT:SOURD CHAN4", ":MEAS:CAT:SOURD?", "CH4");
    let id = (1u16..200).find(|&i| super::meascatalog::cat_from(i).is_some()).unwrap();
    let n = t.ui.measurement_count();
    t.go(&format!(":MEAS:CAT:ITEM {id},CHAN1,CHAN2"));
    let added = t.ui.measurement_count();
    assert!(added >= n, "row added or already there");
    t.go(&format!(":MEAS:CAT:ITEM {id},CHAN1,CHAN2"));
    assert_eq!(t.ui.measurement_count(), added, "adding twice is idempotent");
    let _ = t.go(&format!(":MEAS:CAT:ITEM? {id},CHAN1,CHAN2"));
    t.no(":MEAS:CAT:ITEM 9999", "-224");
}
