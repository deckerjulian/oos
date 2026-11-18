// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `oos-fpgad selfcal`: the self-calibration on the unit (AFE over the GPIO SPI bus, gear ioctl, FPGA ADC
//! measure unit) through `oos_hal::dho::selfcal`. A new self-calibration option is read here.

#[cfg(target_os = "linux")]
use super::{bar_checked, c2h};
use super::{cal_dir, flag, opt};
#[cfg(target_os = "linux")]
use oos_hal::dho::adc;
use oos_hal::dho::afe;
#[cfg(target_os = "linux")]
use oos_hal::dho::seq::{self, Settings};

/// The unit behind the self-calibration: AFE over the GPIO SPI bus, gear ioctl, FPGA ADC measure unit.
#[cfg(target_os = "linux")]
struct UnitHw {
    bar: oos_hal::dho::bus::Bar1,
    afe: afe::GpioAfe,
    steps: usize,
}

#[cfg(target_os = "linux")]
impl oos_hal::dho::selfcal::CalHw for UnitHw {
    fn apply(&mut self, ops: &[afe::Op]) {
        use afe::AfeBus;
        use oos_hal::dho::bus::Bus;
        for op in ops {
            match *op {
                afe::Op::Afe { ch, reg, val } => self.afe.write(ch, reg, val),
                afe::Op::Gear { ch, gear } => {
                    if let Err(e) = afe::set_gear(ch, gear) {
                        eprintln!("gear CH{} {}: {}", ch + 1, gear, e);
                    }
                }
                afe::Op::Adc(w) => {
                    std::thread::sleep(std::time::Duration::from_micros(100));
                    self.bar.wr(0x1034, w);
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }
            }
        }
        self.steps += 1;
    }
    fn afe_read(&mut self, ch: usize, reg: u8) -> u16 {
        use afe::AfeBus;
        self.afe.read(ch, reg)
    }
    fn adc_mean(&mut self, _ch: usize) -> Option<f64> {
        // channel-only ADC mode: all 8 cores sample the channel [A: stock averages cores k and k+4]
        let m = oos_hal::dho::adc::measure(&mut self.bar, &mut |ms| {
            std::thread::sleep(std::time::Duration::from_millis(ms as u64))
        })?;
        Some(m.iter().sum::<f64>() / 8.0)
    }
    fn sleep_ms(&mut self, ms: u32) {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }
}

pub(super) fn selfcal(a: &[String]) -> Result<(), String> {
    #[cfg_attr(not(target_os = "linux"), allow(unused_imports))]
    use oos_hal::dho::selfcal as sc;
    let what = a.first().map(|s| s.as_str()).ok_or("selfcal entry|zero|vertical --ch N ...")?;
    let get = |k: &str| opt(a, k).cloned();
    let has = |k: &str| flag(a, k);
    let ch = get("--ch")
        .ok_or("--ch 1..4")?
        .parse::<usize>()
        .map_err(|e| e.to_string())?
        .checked_sub(1)
        .filter(|c| *c < 4)
        .ok_or("--ch 1..4")?;
    let var = get("--var").map_or(Ok(2), |v| v.parse::<usize>()).map_err(|e| e.to_string())?.min(3);
    let limit = get("--limit").map_or(Ok(usize::MAX), |v| v.parse::<usize>()).map_err(|e| e.to_string())?;
    let rigol = cal_dir();
    let cal = afe::Cal::load(&rigol).map_err(|e| format!("{}: {}", rigol, e))?;
    let block = cal.block(var, ch);
    let entries = sc::entries_of(&block);
    let tuples: Vec<[i32; 7]> = match get("--tuple") {
        Some(t) => {
            let v: Vec<i32> =
                t.split(',').map(|x| x.trim().parse::<i32>()).collect::<Result<_, _>>().map_err(|e| e.to_string())?;
            vec![v.try_into().map_err(|_| "--tuple needs 7 values")?]
        }
        None if what == "entry" => {
            vec![entries.iter().find(|e| e.g[0] == 0).ok_or("no IG 0 configuration in the table")?.g]
        }
        None => entries.iter().map(|e| e.g).take(limit).collect(),
    };
    println!(
        "selfcal {} CH{} block group {} ({} configurations in the table; inputs must be open)",
        what,
        ch + 1,
        var,
        entries.len()
    );
    if !has("--execute") {
        selfcal_dry(what, ch, &cal, &entries, &tuples);
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        let mut bar = bar_checked()?;
        let mut afe_bus = afe::GpioAfe::open().map_err(|e| format!("AFE GPIO: {}", e))?;
        // a full configuration of the channel first, so the AFE shadows match the unit (ADC mode = this channel only)
        let cal_adc = adc::CalAdc::load(&rigol).ok();
        let mut state = seq::State::handover(1, cal_adc.as_ref());
        let st = Settings { mask: 1 << ch, trig_src: ch as u32 + 1, afe: true, ..Settings::default() };
        let (steps, _) = state.configure(&st, Some(&cal), cal_adc.as_ref())?;
        let mut c2h = c2h()?;
        seq::execute(&steps, &mut bar, Some(&mut afe_bus), &mut c2h, |_| {})?;
        let x = &Job {
            args: a,
            what,
            ch,
            var,
            rigol: &rigol,
            cal: &cal,
            block: &block,
            entries: &entries,
            tuples: &tuples,
        };
        if what == "adc" {
            return selfcal_adc(x, bar, afe_bus, &mut c2h, &mut state, cal_adc.as_ref());
        }
        if what == "all" {
            return selfcal_all(x, bar, afe_bus, &mut c2h, &mut state, cal_adc.as_ref());
        }
        selfcal_afe(x, bar, afe_bus, &state)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (tuples, block);
        Err("selfcal --execute needs the unit (Linux)".into())
    }
}

/// The dry run: what `what` would measure on channel `ch` and the table's values, nothing written.
fn selfcal_dry(what: &str, ch: usize, cal: &afe::Cal, entries: &[oos_hal::dho::selfcal::Entry], tuples: &[[i32; 7]]) {
    use oos_hal::dho::selfcal as sc;
    match what {
        "zero" => println!("  3 AFE configurations {:?}, binary searches against the AFE comparators (reg 0 bits 1..3); the offset DAC is switched off meanwhile; table now {:?}", sc::ZERO_CONFIGS, cal.zero_trims(ch)),
        _ => {
            for g in tuples {
                let e = entries.iter().find(|e| &e.g == g);
                println!("  {:?}: table scale {} nV/div, zero {}", g, e.map_or(f64::NAN, |e| e.scale), e.map_or(0, |e| e.zero));
            }
        }
    }
    eprintln!("oos-fpgad: dry run; add --execute (writes AFE registers and the offset DAC of CH{} only)", ch + 1);
}

/// What `selfcal` read from its options and the calibration files, for the runs on the unit.
#[cfg(target_os = "linux")]
struct Job<'a> {
    args: &'a [String],
    what: &'a str,
    ch: usize,
    var: usize,
    rigol: &'a str,
    cal: &'a afe::Cal,
    block: &'a Vec<afe::Rec>,
    entries: &'a [oos_hal::dho::selfcal::Entry],
    tuples: &'a [[i32; 7]],
}

/// `selfcal adc`: only the ADC core gains/offsets of the channel; --save writes /oos/cal.
#[cfg(target_os = "linux")]
fn selfcal_adc(
    x: &Job,
    bar: oos_hal::dho::bus::Bar1,
    afe_bus: afe::GpioAfe,
    c2h: &mut std::fs::File,
    state: &mut seq::State,
    cal_adc: Option<&adc::CalAdc>,
) -> Result<(), String> {
    use oos_hal::dho::{calfile, selfcal as sc};
    let has = |k: &str| flag(x.args, k);
    let (ch, cal) = (x.ch, x.cal);
    // only the ADC core gains/offsets of this channel (mode = the channel alone); --save writes /oos/cal
    let cancel = || false;
    let (mut bar, mut afe_bus) = (bar, afe_bus);
    let base = cal_adc.ok_or("no cal_adc.hex")?;
    let mut unit = sc::Unit {
        bar: &mut bar,
        afe: &mut afe_bus,
        c2h,
        state,
        cal,
        cal_adc: Some(base),
        cancel: &cancel,
        adc_new: None,
    };
    let t0 = std::time::Instant::now();
    let new = sc::adc_cores(&mut unit, base, &[ch], &mut |d, t, s| {
        println!("  {:6.1} s  {}/{}  {}", t0.elapsed().as_secs_f64(), d, t, s)
    })?;
    use oos_hal::dho::adc::CalAdc;
    let row = |c: &CalAdc, f: &dyn Fn(usize) -> usize| (0..8).map(|n| c.get16(f(n))).collect::<Vec<_>>();
    println!(
        "  gains   new {:?}  old {:?}",
        row(&new, &|n| CalAdc::gain_at(ch, n)),
        row(base, &|n| CalAdc::gain_at(ch, n))
    );
    println!(
        "  offsets new {:?}  old {:?}",
        row(&new, &|n| CalAdc::off_at(ch, n)),
        row(base, &|n| CalAdc::off_at(ch, n))
    );
    if has("--save") {
        calfile::save(calfile::OOS_CAL_DIR, "cal_adc.hex", new.payload(), calfile::now()).map_err(|e| e.to_string())?;
        println!("  saved {}/cal_adc.hex", calfile::OOS_CAL_DIR);
    }
    Ok(())
}

/// `selfcal all`: the whole self-cal of the channel through hal (as System > Self-cal); --save writes /oos/cal.
#[cfg(target_os = "linux")]
fn selfcal_all(
    x: &Job,
    bar: oos_hal::dho::bus::Bar1,
    afe_bus: afe::GpioAfe,
    c2h: &mut std::fs::File,
    state: &mut seq::State,
    cal_adc: Option<&adc::CalAdc>,
) -> Result<(), String> {
    use oos_hal::dho::selfcal as sc;
    let has = |k: &str| flag(x.args, k);
    let (ch, cal, rigol) = (x.ch, x.cal, x.rigol);
    // the whole self-cal of this channel through hal (as System > Self-cal): [--adc] ADC cores first,
    // [--zero] AFE zero, the four vertical passes; --save writes /oos/cal
    let cancel = || false;
    let mut bar = bar;
    let mut afe_bus = afe_bus;
    let mut unit =
        sc::Unit { bar: &mut bar, afe: &mut afe_bus, c2h, state, cal, cal_adc, cancel: &cancel, adc_new: None };
    let adc_base = if has("--adc") { cal_adc } else { None };
    let t0 = std::time::Instant::now();
    let mut last = String::new();
    let r = sc::run_all_with(&mut unit, cal, adc_base, &[ch], has("--zero"), &mut |d, t, s| {
        if s != last {
            println!("  {:6.1} s  {}/{}  {}", t0.elapsed().as_secs_f64(), d, t, s);
            last = s.to_string();
        }
    })?;
    println!("  {} configurations, largest change {:.2} %", r.measured, r.worst * 100.0);
    if has("--save") {
        for f in sc::save(&r, rigol)? {
            println!("  saved {}", f);
        }
    }
    Ok(())
}

/// `selfcal zero|probe|entry|vertical`: the AFE passes of the channel through `UnitHw`.
#[cfg(target_os = "linux")]
fn selfcal_afe(x: &Job, bar: oos_hal::dho::bus::Bar1, afe_bus: afe::GpioAfe, state: &seq::State) -> Result<(), String> {
    let (what, ch) = (x.what, x.ch);
    let mut a = state.afe[ch].clone();
    let mut hw = UnitHw { bar, afe: afe_bus, steps: 0 };
    let t0 = std::time::Instant::now();
    match what {
        "zero" => selfcal_zero(x, &mut hw, &mut a, t0)?,
        "probe" => selfcal_probe(x, &mut hw, &mut a)?,
        "entry" | "vertical" => selfcal_vertical(x, &mut hw, &mut a, t0)?,
        _ => return Err("selfcal entry|zero|vertical".into()),
    }
    println!("  {} AFE write groups; re-run the normal set-up (oos-ui does) to leave the cal configuration", hw.steps);
    Ok(())
}

/// `selfcal zero`: the AFE zero trims against the table; --save writes /oos/cal.
#[cfg(target_os = "linux")]
fn selfcal_zero(x: &Job, hw: &mut UnitHw, a: &mut afe::Afe, t0: std::time::Instant) -> Result<(), String> {
    use oos_hal::dho::{calfile, selfcal as sc};
    let has = |k: &str| flag(x.args, k);
    let (ch, cal, rigol) = (x.ch, x.cal, x.rigol);
    let z = sc::afe_zero(hw, a, cal.zero_trims(ch))?;
    println!("  new  {:?}\n  table {:?}  ({:.1} s)", z, cal.zero_trims(ch), t0.elapsed().as_secs_f64());
    if has("--save") {
        let (b, from) =
            calfile::load_first("cal_afe_zero.hex", &[calfile::OOS_CAL_DIR, rigol]).ok_or("no cal_afe_zero.hex")?;
        let p = sc::zero_payload(calfile::parse(&b).map_err(|e| format!("{:?}", e))?, ch, &z);
        calfile::save(calfile::OOS_CAL_DIR, "cal_afe_zero.hex", &p, calfile::now()).map_err(|e| e.to_string())?;
        println!("  saved {}/cal_afe_zero.hex (base: {})", calfile::OOS_CAL_DIR, from);
    }
    Ok(())
}

/// `selfcal probe`: ADC means of the first configuration over the DAC window (diagnostic).
#[cfg(target_os = "linux")]
fn selfcal_probe(x: &Job, hw: &mut UnitHw, a: &mut afe::Afe) -> Result<(), String> {
    let (ch, cal, tuples) = (x.ch, x.cal, x.tuples);
    // diagnostic: ADC means of the first configuration over the DAC window
    use oos_hal::dho::selfcal::CalHw;
    let g = tuples[0];
    let mut o = Vec::new();
    a.set_tuple(&mut o, g);
    o.push(afe::Op::Gear { ch, gear: 1 });
    a.apply_zero_trims(&mut o, &cal.zero_trims(ch), g[0], g[1]);
    hw.apply(&o);
    hw.sleep_ms(1000);
    for code in [20000, 25000, 30000, 32000, 32768, 33500, 35000, 40000] {
        let mut o = Vec::new();
        a.set_dac(&mut o, code);
        hw.apply(&o);
        hw.sleep_ms(20);
        let m = oos_hal::dho::adc::measure(&mut hw.bar, &mut |ms| {
            std::thread::sleep(std::time::Duration::from_millis(ms as u64))
        });
        println!("  DAC {:5}: {:?}", code, m.map(|c| c.map(|x| x as u32)));
    }
    Ok(())
}

/// `selfcal entry|vertical`: the gain configurations against the table; vertical --save writes /oos/cal.
#[cfg(target_os = "linux")]
fn selfcal_vertical(x: &Job, hw: &mut UnitHw, a: &mut afe::Afe, t0: std::time::Instant) -> Result<(), String> {
    use oos_hal::dho::{calfile, selfcal as sc};
    let has = |k: &str| flag(x.args, k);
    let (what, ch, var, rigol) = (x.what, x.ch, x.var, x.rigol);
    let (cal, block, entries, tuples) = (x.cal, x.block, x.entries, x.tuples);
    let zt = cal.zero_trims(ch);
    let mut got = Vec::new();
    for g in tuples {
        let Some(e) = sc::vertical_entry(hw, a, cal, *g, &zt) else {
            println!("  {:?}: no result", g);
            continue;
        };
        let t = entries.iter().find(|x| x.g == *g);
        println!(
            "  {:?}: scale {:.0} nV/div (table {:.0}, {:+.2} %), zero {} (table {})  t {:.0} s",
            g,
            e.scale,
            t.map_or(f64::NAN, |t| t.scale),
            t.map_or(f64::NAN, |t| (e.scale / t.scale - 1.0) * 100.0),
            e.zero,
            t.map_or(0, |t| t.zero),
            t0.elapsed().as_secs_f64()
        );
        got.push(e);
    }
    if what == "vertical" && has("--save") {
        if got.len() < entries.len() {
            return Err(format!("only {} of {} configurations measured: not saved", got.len(), entries.len()));
        }
        let keys: Vec<i64> = (0..801).map(|i| cal.key(i)).collect();
        let mut nb = block.clone();
        sc::rebuild_scales(&mut nb, &keys, &got);
        let (b, from) =
            calfile::load_first("cal_vertical.hex", &[calfile::OOS_CAL_DIR, rigol]).ok_or("no cal_vertical.hex")?;
        let p = sc::vertical_payload(calfile::parse(&b).map_err(|e| format!("{:?}", e))?, &[(var, ch, nb)]);
        calfile::save(calfile::OOS_CAL_DIR, "cal_vertical.hex", &p, calfile::now()).map_err(|e| e.to_string())?;
        println!("  saved {}/cal_vertical.hex block group {} CH{} (base: {})", calfile::OOS_CAL_DIR, var, ch + 1, from);
    }
    Ok(())
}
