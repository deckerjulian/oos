// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `oos-fpgad afg | la`: the generator and the logic analyser port (`afg_la`). A new command of either goes
//! here.

#[cfg(target_os = "linux")]
use super::{bar, bar_checked};
use super::{cal_dir, flag, opt};

pub(super) fn afg_la(cmd: &str, a: &[String]) -> Result<(), String> {
    use oos_hal::dho::{afg, la};
    let get = |k: &str| opt(a, k).map(|v| v.parse::<f64>().map_err(|e| format!("{}: {}", k, e))).transpose();
    let exec = flag(a, "--execute");
    let dir = cal_dir();
    match (cmd, a.first().map(|s| s.as_str())) {
        ("afg", Some("sine" | "square" | "ramp" | "pulse" | "noise" | "dc" | "off")) => {
            let ops = if a[0] != "off" {
                let (f, amp, off) =
                    (get("--freq")?.unwrap_or(1000.0), get("--ampl")?.unwrap_or(1.0), get("--offset")?.unwrap_or(0.0));
                let duty = get("--duty")?.unwrap_or(50.0) / 100.0;
                let wave = match a[0].as_str() {
                    "square" => afg::Wave::Square { duty },
                    "ramp" => afg::Wave::Ramp { symmetry: get("--symmetry")?.unwrap_or(50.0) / 100.0 },
                    "pulse" => afg::Wave::Pulse {
                        duty,
                        rise: get("--edge")?.unwrap_or(0.0),
                        fall: get("--edge")?.unwrap_or(0.0),
                    },
                    "noise" => afg::Wave::Noise,
                    "dc" => afg::Wave::Dc,
                    _ => afg::Wave::Sine,
                };
                let cal = afg::Cal::load(&dir);
                let (ac, oc, g) = afg::codes(
                    cal.as_ref(),
                    amp * 1e9,
                    off * 1e9,
                    f * 1e6,
                    wave == afg::Wave::Noise,
                    wave == afg::Wave::Dc,
                );
                println!(
                    "afg {:?} {} Hz {} Vpp {} V: relays {:#04x}, 0x3070 {:#06x}, 0x3078 {:#06x}, increment {:#x} ({})",
                    wave,
                    f,
                    amp,
                    off,
                    g.code,
                    0xffff - ac,
                    0xffff - oc,
                    afg::increment(f * 1e6),
                    if cal.is_some() { "cal_afg.hex" } else { "no cal file" }
                );
                // --mod am|fm|pm --mod-rate Hz --depth % (AM) --dev Hz (FM) / degrees (PM) --mod-shape sine|square|triangle|up|down
                let shape = match opt(a, "--mod-shape").map(|s| s.as_str()) {
                    Some("square") => afg::ModShape::Square,
                    Some("triangle") => afg::ModShape::Triangle,
                    Some("up") => afg::ModShape::UpRamp,
                    Some("down") => afg::ModShape::DownRamp,
                    Some("noise") => afg::ModShape::Noise,
                    _ => afg::ModShape::Sine,
                };
                let rate = get("--mod-rate")?.unwrap_or(100.0);
                let md = match opt(a, "--mod").map(|s| s.as_str()) {
                    Some("am") => afg::Mod::Am { rate, depth: get("--depth")?.unwrap_or(100.0) / 100.0, shape },
                    Some("fm") => afg::Mod::Fm { rate, dev: get("--dev")?.unwrap_or(f / 10.0), shape },
                    Some("pm") => afg::Mod::Pm { rate, dev: get("--dev")?.unwrap_or(90.0), shape },
                    Some(x) => return Err(format!("--mod {}: am, fm or pm", x)),
                    None => afg::Mod::None,
                };
                if md != afg::Mod::None {
                    println!("modulation {:?}", md);
                }
                afg::start_mod(cal.as_ref(), wave, f * 1e6, amp * 1e9, off * 1e9, 0x00, md)
            } else {
                afg::stop()
            };
            if !exec {
                let n = ops.iter().filter(|o| matches!(o, afg::Op::Bar1 { off: 0x3038, .. })).count();
                for o in ops.iter().filter(|o| !matches!(o, afg::Op::Bar1 { off: 0x3038, .. })) {
                    println!("  {:?}", o);
                }
                println!("  (+ {} table samples to 0x3038)", n);
                eprintln!("oos-fpgad: dry run; add --execute (GI/GO up to 10 Vpp: nothing sensitive connected)");
                return Ok(());
            }
            #[cfg(target_os = "linux")]
            {
                let mut bar = bar_checked()?;
                let open = if flag(a, "--force") { afg::Io::open_forced } else { afg::Io::open };
                let mut io = open().map_err(|e| format!("AFG GPIO: {}", e))?;
                io.apply(&mut bar, &ops);
                println!("done; relays now {:#04x}. The lines stay as set while this process runs: Ctrl-C ends it (output keeps its state).", io.relays());
                if a[0] != "off" {
                    // keep the GPIO handles (relays/supply) for a while
                    std::thread::sleep(std::time::Duration::from_secs(get("--hold")?.unwrap_or(60.0) as u64));
                }
            }
            Ok(())
        }
        ("la", Some("status")) | ("la", Some("thr")) => {
            let cal = la::Cal::load(&dir).unwrap_or_default();
            let w = if a[0] == "thr" {
                let v =
                    |i: usize| a.get(i).ok_or("la thr <pod1 V> <pod2 V>")?.parse::<f64>().map_err(|e| e.to_string());
                Some(la::thresholds(&cal, v(1)?, v(2)?))
            } else {
                None
            };
            println!(
                "la: zero codes {:?}{}",
                cal.zero,
                w.map_or(String::new(), |w| format!(", write {:#06x} <- {:#010x}", w.0, w.1))
            );
            #[cfg(target_os = "linux")]
            {
                use oos_hal::dho::bus::Bus;
                let mut bar = bar()?;
                if let (Some(w), true) = (w, exec) {
                    bar.wr(w.0, w.1);
                }
                let l = la::lines(bar.rd(la::LINES));
                println!(
                    "probe {}, thresholds {:#010x}, lines D15..D0 {}",
                    match la::probe_present() {
                        Ok(true) => "present",
                        Ok(false) => "absent",
                        Err(_) => "unknown",
                    },
                    bar.rd(la::THRESHOLD),
                    l.iter().rev().map(|&b| if b { '1' } else { '0' }).collect::<String>()
                );
            }
            if w.is_some() && !exec {
                eprintln!("oos-fpgad: dry run; add --execute");
            }
            Ok(())
        }
        _ => Err("afg sine|off, la status|thr".into()),
    }
}
