// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! oos-fpgad, the neutral part (ADR 0067): it reads the unit (model, limits), answers `model`, refuses another board
//! than the one it was built for, and hands every other command to the module of its board family, chosen when OOS
//! is built. The commands of the DHO800/DHO900 and their options are listed at the top of `dho/mod.rs`; `oos-fpgad
//! help` prints them.

// The board family of this build, the one of `oos_hal::target::ACTIVE`: `OOS_BOARD` when OOS is built, `dho` by
// default (ADR 0067). A port adds a block like this one for its folder.
oos_hal::if_board! { dho:
    mod dho;
    use dho as board;
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("help");
    // the unit (docs/models.md): model from vendor.bin, standard limits plus /oos/limits.conf; read only
    let opt_arg = |k: &str| opt(&args, k).cloned();
    let platform = oos_hal::model::Platform::detect(
        &cal_dir(),
        &opt_arg("--limits").unwrap_or_else(|| oos_hal::model::LIMITS_CONF.into()),
        opt_arg("--model").as_deref(),
    );
    oos_hal::model::init(platform);
    if cmd == "model" {
        print_model();
        return;
    }
    // another board (docs/porting.md): its registers are not those of the DHO800/DHO900
    if let Some(m) = &oos_hal::model::platform().foreign {
        if !matches!(cmd, "help" | "--help" | "-h") {
            eprintln!(
                "oos-fpgad: this unit is a {}, another board: OOS does not drive its hardware (docs/porting.md)",
                m
            );
            std::process::exit(3);
        }
    }
    board::run(cmd, &args);
}

/// `model`: model, serial, capabilities and limits of the unit, read only.
fn print_model() {
    let p = oos_hal::model::platform();
    println!("model     {}", p.ident());
    println!("caps      {}", p.caps_line());
    println!("series    {}, bandwidth code {}, EDU {}", p.caps.series, p.caps.bw_code, p.caps.edu);
    println!("mac       {}", p.mac.as_deref().unwrap_or("-"));
    let l = &p.limits;
    println!("srate     {:?} Sa/s (1 / 2 / 3-4 channels)", l.max_srate);
    println!("mem       {:?} pts, LA on {:?}", l.max_mem, l.max_mem_la);
    println!(
        "afg max   sine {} square {} ramp {} pulse {} noise {} Hz",
        l.afg_max_hz.sine, l.afg_max_hz.square, l.afg_max_hz.ramp, l.afg_max_hz.pulse, l.afg_max_hz.noise
    );
    println!("clocks    ADC {} Hz, generator {} Hz", l.adc_clock_hz, l.afg_clock_hz);
    for u in &p.unit {
        println!("unit      {} ({})", u, oos_hal::model::UNIT_CONF);
    }
    for o in &p.overrides {
        println!("EXPERIMENTAL  {}", o);
    }
    for w in &p.warnings {
        println!("warning   {}", w);
    }
}

fn num(s: &str) -> Result<u32, String> {
    match s.strip_prefix("0x") {
        Some(h) => u32::from_str_radix(h, 16),
        None => s.parse(),
    }
    .map_err(|e| format!("{}: {}", s, e))
}

/// The directory of the calibration files: OOS_CAL_DIR, default /rigol/data.
fn cal_dir() -> String {
    std::env::var("OOS_CAL_DIR").unwrap_or_else(|_| "/rigol/data".into())
}

/// The value after option `k`.
fn opt<'a>(a: &'a [String], k: &str) -> Option<&'a String> {
    a.iter().position(|x| x == k).and_then(|i| a.get(i + 1))
}

/// Option `k` is given.
fn flag(a: &[String], k: &str) -> bool {
    a.iter().any(|x| x == k)
}
