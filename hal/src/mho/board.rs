// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What the UI takes from the MHO900 without touching its acquisition board: the volts-per-division steps and the
//! SoC's temperatures.

use std::path::Path;

/// The fine volts-per-division steps in nV: 1-2-5 with fine steps in between, 500 uV/div to 10 V/div (data sheet;
/// **assumed**, the front ends are not driven).
pub fn scale_ladder() -> Vec<i64> {
    let mut v = Vec::with_capacity(701);
    for d in 0..5 {
        let b = 100_000 * 10i64.pow(d);
        v.extend((0..50).map(|k| b + k * b / 50));
        v.extend((0..60).map(|k| 2 * b + k * b / 20));
        v.extend((0..50).map(|k| 5 * b + k * b / 10));
    }
    v.push(10_000_000_000);
    v.retain(|&nv| nv >= 500_000);
    v
}

/// The SoC's thermal zones under `dir` (`/sys/class/thermal`): (type, degC), in the order of the zone numbers.
pub fn thermal_zones(dir: &Path) -> Vec<(String, f32)> {
    let mut v: Vec<(u32, String, f32)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let n: u32 = e.file_name().to_str()?.strip_prefix("thermal_zone")?.parse().ok()?;
            let p = e.path();
            let name = std::fs::read_to_string(p.join("type")).ok()?.trim().to_string();
            let temp = std::fs::read_to_string(p.join("temp")).ok()?.trim().parse::<f32>().ok()? / 1000.0;
            Some((n, name, temp))
        })
        .collect();
    v.sort_by_key(|z| z.0);
    v.into_iter().map(|(_, n, t)| (n, t)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scales_run_from_half_a_millivolt_to_ten_volts() {
        let v = scale_ladder();
        assert_eq!(v.first(), Some(&500_000));
        assert_eq!(v.last(), Some(&10_000_000_000));
        assert!(v.windows(2).all(|w| w[0] < w[1]));
        assert!(v.contains(&1_000_000) && v.contains(&2_000_000) && v.contains(&5_000_000));
    }

    #[test]
    fn the_thermal_zones_are_read_in_order() {
        let d = std::env::temp_dir().join(format!("oos-mho-tz-{}", std::process::id()));
        let zones = [
            ("thermal_zone10", "x", "1000"),
            ("thermal_zone1", "gpu-thermal", "46875"),
            ("thermal_zone0", "soc-thermal", "52222"),
        ];
        for (n, t, c) in zones {
            std::fs::create_dir_all(d.join(n)).unwrap();
            std::fs::write(d.join(n).join("type"), t).unwrap();
            std::fs::write(d.join(n).join("temp"), c).unwrap();
        }
        std::fs::create_dir_all(d.join("cooling_device0")).unwrap();
        let z = thermal_zones(&d);
        std::fs::remove_dir_all(&d).unwrap();
        assert_eq!(z.iter().map(|z| z.0.as_str()).collect::<Vec<_>>(), ["soc-thermal", "gpu-thermal", "x"]);
        assert!((z[0].1 - 52.222).abs() < 1e-3);
    }
}
