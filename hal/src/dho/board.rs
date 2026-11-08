// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Board management: FPGA XADC (die temperature, VCCINT), SoC thermal zones, over-temperature supervision and the
//! beeper. See docs/protocols/board.md (sections 2, 3, 5, 8). The decoding and the alarm logic are pure; the sysfs
//! readers only use std, so everything builds and tests on the host.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

/// FPGA register with the XADC readings.
pub const XADC_REG: u32 = 0x1438;

/// FPGA die temperature (degC) and VCCINT (V) from register 0x1438: bits [11:0] temperature code, [27:16] VCCINT code,
/// with the standard Xilinx XADC transfer functions.
pub fn xadc(raw: u32) -> (f32, f32) {
    let t = (raw & 0xfff) as f32 * 503.975 / 4096.0 - 273.15;
    let v = ((raw >> 16) & 0xfff) as f32 / 4096.0 * 3.0;
    (t, v)
}

/// Reads and converts the XADC register. An unloaded FPGA reads 0xffffffff or 0, both out of range: `None`.
pub fn read_xadc(bus: &mut dyn crate::dho::bus::Bus) -> Option<(f32, f32)> {
    let raw = bus.rd(XADC_REG);
    let (t, v) = xadc(raw);
    (raw != u32::MAX && (-40.0..=150.0).contains(&t) && (0.5..=1.5).contains(&v)).then_some((t, v))
}

/// One SoC thermal zone (`/sys/class/thermal/thermal_zoneN`, `type` e.g. "cpu-thermal" / "gpu-thermal").
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub name: String,
    pub temp: f32,
}

/// All readable thermal zones under `root` (normally /sys/class/thermal), temperatures in degC.
pub fn thermal_zones(root: &Path) -> Vec<Zone> {
    let mut v = Vec::new();
    let Ok(rd) = fs::read_dir(root) else { return v };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("thermal_zone")))
        .collect();
    dirs.sort();
    for d in dirs {
        let t = fs::read_to_string(d.join("temp")).ok().and_then(|s| s.trim().parse::<i64>().ok());
        let name = fs::read_to_string(d.join("type")).map(|s| s.trim().to_string()).unwrap_or_default();
        if let Some(t) = t {
            v.push(Zone { name, temp: t as f32 / 1000.0 });
        }
    }
    v
}

/// Alarm events of the over-temperature supervision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alarm {
    /// Hot for `warn_after` periods in a row: show the warning and sound the tone.
    Warn,
    /// Hot for `off_after` periods in a row: orderly shutdown, then `panel::POWER_OFF`.
    PowerOff,
    /// Back below the limits after a warning: stop the tone, clear the warning.
    Cleared,
}

/// Over-temperature supervision (board.md section 3): every period, "hot" = any reading
/// above its limit; a counter of hot periods in a row, reset when the condition is gone. It warns at
/// 3 and powers off at 11 (1 s period, 75 degC for CPU/GPU/FPGA die; stock: 3 s, 95 degC for CPU/GPU).
#[derive(Clone, Debug)]
pub struct Guard {
    pub warn_after: u32,
    pub off_after: u32,
    count: u32,
    warned: bool,
}

impl Default for Guard {
    fn default() -> Self {
        Guard { warn_after: 3, off_after: 11, count: 0, warned: false }
    }
}

/// Limits proposed in board.md 8.2: 75 degC for the FPGA die, 90 degC for the SoC zones (the kernel already
/// throttles at 70/85 degC).
pub const FPGA_LIMIT: f32 = 75.0;
pub const SOC_LIMIT: f32 = 90.0;

impl Guard {
    /// Feeds one period. Returns an event on the transitions only.
    pub fn feed(&mut self, hot: bool) -> Option<Alarm> {
        if !hot {
            self.count = 0;
            return std::mem::take(&mut self.warned).then_some(Alarm::Cleared);
        }
        self.count = self.count.saturating_add(1);
        if self.count == self.off_after {
            Some(Alarm::PowerOff)
        } else if self.count == self.warn_after {
            self.warned = true;
            Some(Alarm::Warn)
        } else {
            None
        }
    }

    /// Hot periods in a row so far.
    pub fn count(&self) -> u32 {
        self.count
    }
}

/// Whether one set of readings is hot: any SoC zone above `SOC_LIMIT` or the FPGA die above `FPGA_LIMIT`.
pub fn is_hot(zones: &[Zone], fpga: Option<f32>) -> bool {
    zones.iter().any(|z| z.temp > SOC_LIMIT) || fpga.is_some_and(|t| t > FPGA_LIMIT)
}

/// Beeper patterns of the stock system: 2 kHz, 20 % duty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beep {
    /// Key click: 20 ms on.
    Once,
    /// Alarm: 10 x (200 ms on, 200 ms off) = 4 s.
    Long,
}

pub const BEEP_PERIOD_NS: u32 = 500_000;
pub const BEEP_DUTY_NS: u32 = 100_000;

impl Beep {
    /// (on ms, off ms) steps.
    pub fn pattern(self) -> &'static [(u32, u32)] {
        match self {
            Beep::Once => &[(20, 0)],
            Beep::Long => &[(200, 200); 10],
        }
    }
}

/// The beeper through the PWM sysfs class: the OOS DT has a `pwm-gpio` node on GPIO4_A3 (module pwm-gpio.ko).
/// The stock `/dev/pwm-1` driver (beeper_gpio.ko) is not used.
pub struct Beeper {
    pwm: PathBuf,
}

impl Beeper {
    /// Finds the pwmchip whose device is the `pwm-gpio` platform device, exports channel 0 and sets 2 kHz / 20 %.
    pub fn open(class: &Path) -> std::io::Result<Beeper> {
        let chip = fs::read_dir(class)?
            .flatten()
            .map(|e| e.path())
            .find(|p| fs::read_link(p.join("device")).is_ok_and(|l| l.to_string_lossy().ends_with("pwm-gpio")))
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no pwm-gpio pwmchip (modprobe pwm-gpio)"))?;
        let pwm = chip.join("pwm0");
        if !pwm.exists() {
            fs::write(chip.join("export"), "0")?;
        }
        // period first: duty_cycle must never exceed it
        fs::write(pwm.join("period"), BEEP_PERIOD_NS.to_string())?;
        fs::write(pwm.join("duty_cycle"), BEEP_DUTY_NS.to_string())?;
        Ok(Beeper { pwm })
    }

    pub fn set(&self, on: bool) -> std::io::Result<()> {
        fs::write(self.pwm.join("enable"), if on { "1" } else { "0" })
    }

    /// Plays a pattern (blocking; up to 4 s for `Long`). The line ends disabled (pwm-gpio drives it inactive = low).
    pub fn play(&self, b: Beep) -> std::io::Result<()> {
        for &(on, off) in b.pattern() {
            self.set(true)?;
            sleep(Duration::from_millis(on as u64));
            self.set(false)?;
            sleep(Duration::from_millis(off as u64));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xadc_transfer_functions() {
        // code 0x9b0 = 2480: 2480 * 503.975 / 4096 - 273.15 = 31.99 degC; VCCINT code 0x555 = 1365 -> 0.9998 V
        let (t, v) = xadc(0x0555_09b0);
        assert!((t - 31.99).abs() < 0.01, "{t}");
        assert!((v - 1.0).abs() < 0.001, "{v}");
        // upper bits outside the 12-bit fields are ignored
        assert_eq!(xadc(0xf555_f9b0), xadc(0x0555_09b0));
    }

    #[test]
    fn xadc_rejects_unloaded_fpga() {
        struct R(u32);
        impl crate::dho::bus::Bus for R {
            fn wr(&mut self, _: u32, _: u32) {}
            fn rd(&mut self, _: u32) -> u32 {
                self.0
            }
        }
        assert!(read_xadc(&mut R(u32::MAX)).is_none());
        assert!(read_xadc(&mut R(0)).is_none());
        assert!(read_xadc(&mut R(0x0555_09b0)).is_some());
    }

    #[test]
    fn guard_warns_at_3_and_powers_off_at_11() {
        let mut g = Guard::default();
        let ev: Vec<_> = (0..12).map(|_| g.feed(true)).collect();
        assert_eq!(ev[2], Some(Alarm::Warn));
        assert_eq!(ev[10], Some(Alarm::PowerOff));
        assert_eq!(ev.iter().flatten().count(), 2);
        assert_eq!(g.feed(false), Some(Alarm::Cleared));
        assert_eq!(g.feed(false), None);
        // a single cool period resets the count; no Cleared without a warning before
        g.feed(true);
        g.feed(true);
        assert_eq!(g.feed(false), None);
        assert_eq!(g.count(), 0);
    }

    #[test]
    fn hot_condition() {
        let z = |t| vec![Zone { name: "cpu-thermal".into(), temp: t }];
        assert!(!is_hot(&z(60.0), Some(70.0)));
        assert!(is_hot(&z(91.0), None));
        assert!(is_hot(&z(60.0), Some(76.0)));
    }

    #[test]
    fn reads_thermal_zones_from_sysfs_layout() {
        let d = std::env::temp_dir().join(format!("oos-thermal-{}", std::process::id()));
        for (i, (ty, t)) in [("cpu-thermal", "45000"), ("gpu-thermal", "41250")].iter().enumerate() {
            let z = d.join(format!("thermal_zone{i}"));
            fs::create_dir_all(&z).unwrap();
            fs::write(z.join("type"), format!("{ty}\n")).unwrap();
            fs::write(z.join("temp"), format!("{t}\n")).unwrap();
        }
        fs::create_dir_all(d.join("cooling_device0")).unwrap();
        let z = thermal_zones(&d);
        fs::remove_dir_all(&d).unwrap();
        assert_eq!(z, vec![Zone { name: "cpu-thermal".into(), temp: 45.0 }, Zone { name: "gpu-thermal".into(), temp: 41.25 }]);
    }

    #[test]
    fn beep_patterns() {
        assert_eq!(Beep::Once.pattern(), &[(20, 0)]);
        let total: u32 = Beep::Long.pattern().iter().map(|(a, b)| a + b).sum();
        assert_eq!(total, 4000);
    }
}
