// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Board subcommands: temperatures, beeper, over-temperature guard (oos_hal::board, docs/protocols/board.md).

use oos_hal::board::{self, Alarm, Beep, Beeper, Guard, Zone};
use oos_hal::panel;
use std::path::Path;

/// FPGA die temperature and VCCINT, if BAR1 is there and the FPGA answers.
fn fpga_xadc() -> Option<(f32, f32)> {
    #[cfg(target_os = "linux")]
    {
        let mut bar = oos_hal::bus::Bar1::open().ok()?;
        board::read_xadc(&mut bar)
    }
    #[cfg(not(target_os = "linux"))]
    None
}

/// One line of readings.
pub fn line(zones: &[Zone], fpga: Option<(f32, f32)>) -> String {
    let mut s: Vec<String> = zones.iter().map(|z| format!("{} {:.1} C", z.name, z.temp)).collect();
    match fpga {
        Some((t, v)) => s.push(format!("fpga {:.1} C, vccint {:.3} V", t, v)),
        None => s.push("fpga -".into()),
    }
    s.join(", ")
}

/// Power off through the front-panel MCU: sync, then `FA 05 01` twice like the stock system.
fn power_off() -> Result<(), String> {
    unsafe { libc::sync() };
    let mut f = std::fs::OpenOptions::new().write(true).open("/dev/ttyS0").map_err(|e| format!("/dev/ttyS0: {}", e))?;
    use std::io::Write;
    for _ in 0..2 {
        f.write_all(&panel::POWER_OFF).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn run(cmd: &str, a: &[String]) -> Result<(), String> {
    let thermal = Path::new("/sys/class/thermal");
    match cmd {
        "temp" => println!("{}", line(&board::thermal_zones(thermal), fpga_xadc())),
        "beep" => {
            let b = match a.first().map(|s| s.as_str()) {
                Some("once") | None => Beep::Once,
                Some("long") => Beep::Long,
                Some(x) => return Err(format!("beep once|long, not {}", x)),
            };
            let bp = Beeper::open(Path::new("/sys/class/pwm")).map_err(|e| format!("beeper: {}", e))?;
            bp.play(b).map_err(|e| format!("beeper: {}", e))?;
        }
        "guard" => {
            let period = a.iter().position(|x| x == "--period").and_then(|i| a.get(i + 1)).map(|x| x.parse::<f32>().map_err(|e| e.to_string())).transpose()?.unwrap_or(1.0);
            let arm = a.iter().any(|x| x == "--power-off");
            let beeper = Beeper::open(Path::new("/sys/class/pwm")).ok();
            let mut g = Guard::default();
            eprintln!("oos-fpgad guard: period {} s, limits SoC {} C / FPGA {} C, power off {}", period, board::SOC_LIMIT, board::FPGA_LIMIT, if arm { "ARMED" } else { "not armed (log only)" });
            loop {
                let (z, x) = (board::thermal_zones(thermal), fpga_xadc());
                let ev = g.feed(board::is_hot(&z, x.map(|v| v.0)));
                if let Some(ev) = ev {
                    eprintln!("guard: {:?} after {} hot periods: {}", ev, g.count(), line(&z, x));
                    match ev {
                        Alarm::Warn => {
                            if let Some(b) = &beeper {
                                let _ = b.play(Beep::Long);
                            }
                        }
                        Alarm::PowerOff if arm => return power_off(),
                        _ => {}
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs_f32(period));
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_line() {
        let z = vec![Zone { name: "cpu-thermal".into(), temp: 45.25 }];
        assert_eq!(line(&z, Some((51.0, 0.998))), "cpu-thermal 45.2 C, fpga 51.0 C, vccint 0.998 V");
        assert_eq!(line(&[], None), "fpga -");
    }
}
