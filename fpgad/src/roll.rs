// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `oos-fpgad roll`: the roll mode test (configuration as for `run`, the roll display chain, streaming, stop).
//! A new option of the roll test is read here.

#[cfg(target_os = "linux")]
use super::{bar_checked, c2h, cal_dir};
use super::{parse, Opt};
use oos_hal::dho::scu::Shadows;
use oos_hal::dho::seq;
use oos_hal::dho::Write;
#[cfg(target_os = "linux")]
use oos_hal::dho::{adc, afe};

/// Roll mode test: configuration as for `run`, then the roll display chain, streaming for `--secs`, stop.
pub(super) fn roll(a: &[String]) -> Result<(), String> {
    let mut a = a.to_vec();
    let secs = match a.iter().position(|x| x == "--secs") {
        Some(i) => {
            let v: f64 = a.get(i + 1).ok_or("--secs needs a value")?.parse().map_err(|e| format!("--secs: {}", e))?;
            a.drain(i..=i + 1);
            v
        }
        None => 3.0,
    };
    let scan = match a.iter().position(|x| x == "--scan") {
        Some(i) => {
            a.remove(i);
            true
        }
        None => false,
    };
    let o = parse(&a)?;
    let tdiv_fs = seq::tdiv_fs(o.s.tdiv);
    let p = oos_hal::dho::hz::prep(o.s.mask, tdiv_fs, o.s.depth, 0);
    let rp = oos_hal::dho::roll::params(o.s.mask, tdiv_fs, o.s.depth, &p);
    let name = if scan { "scan" } else { "roll" };
    // scan: trigger at the screen centre -> 0x4058 = 0 (roll-mode.md 5, test E6)
    let slow_cfg = |sh: &mut oos_hal::dho::scu::Shadows| {
        if scan {
            oos_hal::dho::roll::config_scan(sh, &rp, p.memsel, 0)
        } else {
            oos_hal::dho::roll::config(sh, &rp, p.memsel)
        }
    };
    println!(
        "{}: {} Sa/s, memsel {}, k {}, f {}/{}, len {}, {:.3} us per point, chunk = {:.3} ms",
        name,
        p.rate,
        p.memsel,
        rp.k,
        rp.f_num,
        rp.f_den,
        rp.len,
        rp.spacing_fs as f64 / 1e9,
        rp.spacing_fs as f64 * 100.0 / rp.ratio as f64 / 1e12
    );
    if !o.execute {
        roll_dry(&o, scan, &slow_cfg);
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        roll_execute(&o, secs, scan, name, &slow_cfg)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (secs, name);
        Err("roll --execute needs the unit (Linux)".into())
    }
}

/// The dry run of the roll test: its writes against the shadows of the hand-over, nothing written.
fn roll_dry(o: &Opt, scan: bool, slow_cfg: &impl Fn(&mut Shadows) -> Vec<Write>) {
    let mut sh = seq::State::handover(o.app_mask, None).sh;
    let wr = |w: &[oos_hal::dho::Write]| w.iter().for_each(|(r, v)| println!("  wr  {:#06x} {:#010x}", r, v));
    wr(&slow_cfg(&mut sh));
    wr(&oos_hal::dho::roll::start(&mut sh, o.s.mask));
    // the writes of the read loop and of the end, against the shadows of the dry run
    println!("# before each fill level read");
    wr(&oos_hal::dho::roll::fill_pulse(&sh));
    if scan {
        println!("# after a complete sweep, then the configuration and the start again");
        wr(&oos_hal::dho::roll::scan_done(&mut sh));
    }
    println!("# stop");
    wr(&oos_hal::dho::roll::stop(&mut sh));
    eprintln!("oos-fpgad: dry run; add --execute");
}

/// The roll test on the unit: configuration, roll display chain, streaming to `--out` for `secs`, stop.
#[cfg(target_os = "linux")]
fn roll_execute(
    o: &Opt,
    secs: f64,
    scan: bool,
    name: &str,
    slow_cfg: &impl Fn(&mut Shadows) -> Vec<Write>,
) -> Result<(), String> {
    use oos_hal::dho::bus::Bus;
    use std::io::Write as _;
    let dir = cal_dir();
    let cal = if o.s.afe { afe::Cal::load(&dir).ok() } else { None };
    let cal_adc = adc::CalAdc::load(&dir).ok();
    let mut bar = bar_checked()?;
    let mut afe_bus = if o.s.afe { Some(afe::GpioAfe::open().map_err(|e| format!("AFE GPIO: {}", e))?) } else { None };
    let mut c2h = c2h()?;
    let mut state = seq::State::handover(o.app_mask, cal_adc.as_ref());
    let (cfg, _) = state.configure(&o.s, cal.as_ref(), cal_adc.as_ref())?;
    seq::execute(&cfg, &mut bar, afe_bus.as_mut(), &mut c2h, |_| {})?;
    bar.apply(&slow_cfg(&mut state.sh));
    bar.apply(&oos_hal::dho::roll::start(&mut state.sh, o.s.mask));
    let (mut last, mut sweeps, mut sweep_chunks) = (oos_hal::dho::roll::ScanInfo::default(), 0usize, 0usize);
    let mut out = std::fs::File::create(&o.out).map_err(|e| format!("{}: {}", o.out, e))?;
    let t0 = std::time::Instant::now();
    let (mut total, mut reads, mut empty, mut tsec, mut csec) = (0usize, 0usize, 0usize, 0u64, 0usize);
    while t0.elapsed().as_secs_f64() < secs {
        bar.apply(&oos_hal::dho::roll::fill_pulse(&state.sh));
        let n = bar.rd(0x11e8);
        let c = oos_hal::dho::roll::chunks(n);
        if c > 0 {
            let buf = oos_hal::dho::bus::read_dma(&mut c2h, c * oos_hal::dho::roll::CHUNK_BYTES, |_, _| {})
                .map_err(|e| format!("c2h read of {} chunks (0x11e8 = {}): {}", c, n, e))?;
            out.write_all(&buf).map_err(|e| e.to_string())?;
            total += c;
            csec += c;
            sweep_chunks += c;
            reads += 1;
        }
        if scan {
            // after every read, also an empty one
            let r = oos_hal::dho::roll::SCAN_INFO_REGS.map(|x| bar.rd(x));
            let info = oos_hal::dho::roll::scan_info(r[0], r[1]);
            if info != last {
                println!(
                    "{:5.2} s  0x1204 = {:#010x}, 0x40d0 = {:#x} after {} chunks of this sweep",
                    t0.elapsed().as_secs_f64(),
                    r[0],
                    r[1],
                    sweep_chunks
                );
                last = info;
            }
            if info.done {
                // stock state 8 (RUN): stop, clear the average, 500 ms, configure again, next sweep
                sweeps += 1;
                bar.apply(&oos_hal::dho::roll::scan_done(&mut state.sh));
                std::thread::sleep(std::time::Duration::from_millis(500));
                bar.apply(&slow_cfg(&mut state.sh));
                bar.apply(&oos_hal::dho::roll::start(&mut state.sh, o.s.mask));
                sweep_chunks = 0;
                continue;
            }
        }
        if c == 0 {
            empty += 1;
            std::thread::sleep(std::time::Duration::from_millis(9));
            continue;
        }
        if t0.elapsed().as_secs() > tsec {
            println!(
                "{:5.1} s  {} chunks in the last second (last 0x11e8 = {}, {} chunks)",
                t0.elapsed().as_secs_f64(),
                csec,
                n,
                c
            );
            tsec = t0.elapsed().as_secs();
            csec = 0;
        }
    }
    bar.apply(&oos_hal::dho::roll::stop(&mut state.sh));
    println!(
        "{}: {} chunks in {} reads ({} empty polls) in {:.1} s; 0x1208 = {:#010x}",
        name,
        total,
        reads,
        empty,
        t0.elapsed().as_secs_f64(),
        bar.rd(0x1208)
    );
    if scan {
        println!("scan: {} complete sweeps", sweeps);
    }
    Ok(())
}
