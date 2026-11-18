// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! oos-fpgad: OOS acquisition control and bring-up tool over `oos_hal` (replaces the C tool oos-hw step by step).
//!
//!   oos-fpgad model [--model NAME] [--limits FILE]   model, serial, capabilities and limits of the unit
//!                                          (/rigol/data/vendor.bin, /oos/limits.conf; docs/models.md), read only
//!   oos-fpgad plan [options]               print the complete acquisition sequence, no hardware access (runs anywhere)
//!   oos-fpgad run  [options] --execute     run it on the unit and write the frame to --out (Linux, FPGA up)
//!   oos-fpgad status                       read only: FPGA version, FSM, DDR init, ADC clock (PLL lock), play window
//!   oos-fpgad rd <offset> [count]          read BAR1 registers
//!   oos-fpgad wr <offset> <value> --execute   write one BAR1 register (0x1008 refused: BRAM fallback bit)
//!   oos-fpgad fpga-boot [--slot 0x400000] [--progress FILE]   only the BOOT command (start of rcS: the bitstream loads
//!                                          while Linux starts the rest); its uptime goes to /tmp/oos-fpga-booted
//!   oos-fpgad cold-boot [--execute] [--power-on [--slot 0x400000] [--booted]] [--ignore-clock] [--progress FILE]
//!                                          (refused while XDMA is loaded: power cycle); --booted: the BOOT command
//!                                          was sent by `fpga-boot`, only the rest of the wait; --progress: start-up
//!                                          screen lines (oos_hal::bootstatus), then the temperature check
//!                                          without --execute: dry run against the unit model (runs anywhere);
//!                                          --execute: cold boot with the FPGA already on PCIe; --power-on: first the
//!                                          BOOT command (slots 0x0 / 0x400000 only) and the PCIe/xdma bring-up;
//!                                          --ignore-clock: 0x1210 out of range only warns, counter logged per step
//!   oos-fpgad pll [--reset | --r0 <val>]                LMX2582 table over GPIO (LE idles high first like spi2pll_lxm2582_gpio.ko;
//!                                          --reset: R0 RESET=1/0 first, as the TI datasheet asks), then reads 0x1210
//!   oos-fpgad pll --freq <Hz> --dry        table for another ADC clock (oos_hal::dho::lmx2582, pll.md) and its decode,
//!                                          no hardware access; without --dry only 1.25 GHz (others are untested)
//!   oos-fpgad gpio <line> <0|1> [--hold 60] [--force]  drive a GPIO line (by DT name) and keep it for --hold seconds;
//!                                          generator lines (afg_in1..5, FG_M2C_GPIO) only with a generator or --force
//!   oos-fpgad roll [run options] [--secs 3] [--scan] --execute
//!                                          roll mode (roll-mode.md): normal set-up, roll display chain, stream for
//!                                          --secs seconds, chunks (216 bytes: header + 100 words) to --out;
//!                                          --scan: scan bit instead of roll, logs 0x1204 / 0x40d0, restarts after done
//!   oos-fpgad selfcal entry|zero|vertical|all [--adc] --ch N [--var 0..3] [--tuple ig,vga,att,sga,sgar,mux,muxr] [--limit n]
//!                                          [--execute] [--save]
//!                                          self-calibration (docs/protocols/self-cal.md), inputs open: `entry` measures one
//!                                          gain configuration and compares it with the table, `zero` the AFE zero trims,
//!                                          `vertical` one bandwidth block; --save writes only to /oos/cal (never /rigol)
//!   oos-fpgad afg sine|square|ramp|pulse|noise|dc --freq 1000 --ampl 1 [--offset 0] [--duty 50] [--symmetry 50]
//!                 [--edge s] [--mod am|fm|pm --mod-rate Hz --depth % --dev Hz|deg --mod-shape sine|square|triangle|up|down]
//!                 [--execute] | afg off [--execute]
//!                                          built-in generator (afg.md): wave at Hz / Vpp / V with the factory
//!                                          cal_afg.hex (dc: level = --offset); dry run prints the writes
//!   oos-fpgad la status | la thr <pod1 V> <pod2 V> [--execute]
//!                                          logic analyser (la.md): probe, line states 0x12dc, thresholds 0x12d4
//!   oos-fpgad adc slots [--runs 4]         ADC measure unit (0x1040..0x1048): mean, min, max per slot and each slot's
//!                                          mean against the slot average in ADC steps (docs/fork/core-offset.md)
//!   oos-fpgad adc offset <core 0..7> <steps> [--execute]
//!                                          one RT8847 core offset register (0x14 + 8n) over 0x1034; dry run prints
//!                                          the word. Both refused while oos-ui runs (it owns the ADC and the measure unit)
//!   oos-fpgad temp                         read only: SoC thermal zones, FPGA die temperature and VCCINT (XADC 0x1438)
//!   oos-fpgad beep [once|long]             beeper (pwm-gpio, 2 kHz): 20 ms click or 4 s alarm
//!   oos-fpgad guard [--period 1] [--power-off]
//!                                          over-temperature guard (SoC > 90 C or FPGA > 75 C): warning + beep after
//!                                          3 periods, power off via the panel MCU after 11 (only with --power-off)
//!
//! Options (defaults = the warm hand-over of 2026-11-02: CH1, 200 us/div, Auto depth, 1 V/div):
//!   --mask 1           channel mask (bit0 = CH1)          --app-mask 1   channel mask the stock system left behind
//!   --tdiv 200e-6      time/div in s                       --depth 0      memory depth (0 = Auto)
//!   --scale 1          V/div of all enabled channels       --offset 0     offset in V
//!   --level 1.5        trigger level in V                  --src 1        trigger source channel
//!   --slope rise|fall|either                               --normal       NORMAL sweep instead of AUTO
//!   --n 4000           points per channel to read          --at -2000     window start relative to the trigger point
//!   --all              read the whole record instead of --n/--at
//!   --play-last        read out with the play-last pulse of the warm hand-over instead of the pass-through + type-4 play
//!   --no-read          do all writes but not the c2h read (diagnostic: read with dd afterwards)
//!   --header           read with the 16-byte frame header (0x1010 bit31 = 0) like the stock live loop
//!   --keep-config      skip trigger, acquisition configuration and auto trigger (arm with what the stock system left behind)
//!   --compress 1       FPGA compression factor k of the whole record (0x1170, stock live display path; use with --all)
//!   --frames 1         frames per batch (one arm, one play, one read of all frames; capped at the frame ring)
//!   --average N        hardware average of N triggers (0x1004 bit3; reads the newest frame of the batch; not verified)
//!   --blocks 7         configuration blocks before the arm: bit0 trigger, bit1 acquisition, bit2 auto trigger
//!   --afe              program the AFEs of the enabled channels (scale/offset, bandwidth --bw OFF|S20M|AUTO|<MHz>)
//!   --la               logic analyser on (la.md, UNTESTED): LA set-up and a second, raw LA read after the analog one
//!                      (run writes the bytes of that last read, the LA words); --la-thr 1.4 threshold of both pods in V
//!                      (--la-alone: no analog channel shown, the analog rate is not halved)
//!   --raw-too          with --compress k > 1: read the newest frame once more, raw (seq::read_raw_again), to --out.raw
//!   --out /tmp/frame.bin
//!   oos-fpgad compress-check <raw.bin> <compressed.bin> --k K [--mask 1]
//!                                          offline (runs anywhere): which min/max pair layout explains the compressed
//!                                          read of the same record (oos_hal::dho::pairs, docs/fork/compress.md)
//!   oos-fpgad made-up-cal <dir>            offline (runs anywhere): writes a made-up calibration set into <dir>, every
//!                                          file `plan` and the dry runs read, with invented values that mean nothing
//!                                          for a scope (docs/protocols/self-cal.md 16). For comparing two builds
//!                                          without the files of a unit: OOS_CAL_DIR=<dir>. Refused under /rigol, /oos
//! A channel set different from --app-mask switches the ADC mode; --afe and a mode switch read the calibration from
//! OOS_CAL_DIR (default /rigol/data).

mod afg;
mod board;
mod bringup;
mod compcheck;
mod roll;
mod selfcal;
mod tool;

use afg::afg_la;
use oos_hal::dho::seq::{self, Settings, Step};
use oos_hal::dho::{adc, afe, tpu};
use roll::roll;
use selfcal::selfcal;
use tool::{tool, TOOL_CMDS};

struct Opt {
    s: Settings,
    app_mask: u32,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    out: String,
    execute: bool,
    no_read: bool,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    raw_too: bool,
}

fn parse(args: &[String]) -> Result<Opt, String> {
    let mut s = Settings::default();
    let (mut app_mask, mut out, mut execute, mut no_read, mut raw_too) =
        (1u32, "/tmp/frame.bin".to_string(), false, false, false);
    let (mut scale, mut offset, mut bw) = (1.0, 0.0, afe::bw::OFF);
    let (mut n, mut at, mut all) = (4000u32, -2000i64, false);
    let mut i = 0;
    let num = |v: Option<&String>| -> Result<f64, String> {
        v.ok_or("missing value")?.parse::<f64>().map_err(|e| e.to_string())
    };
    let int = |v: Option<&String>| -> Result<u32, String> { crate::num(v.ok_or("missing value")?) };
    while i < args.len() {
        let v = args.get(i + 1);
        let mut take = true;
        match args[i].as_str() {
            "--mask" => s.mask = int(v)?,
            "--app-mask" => app_mask = int(v)?,
            "--tdiv" => s.tdiv = num(v)?,
            "--depth" => s.depth = num(v)? as i128,
            "--scale" => scale = num(v)?,
            "--offset" => offset = num(v)?,
            "--level" => s.level = num(v)?,
            "--src" => s.trig_src = int(v)?,
            "--blocks" => s.blocks = int(v)? as u8,
            "--frames" => s.frames = int(v)?,
            "--compress" => s.compress = int(v)?,
            // hardware averaging, not verified on the unit
            "--average" => s.mode = oos_hal::dho::acq::Mode::Average { count: int(v)? },
            "--la-thr" => {
                let t = num(v)?;
                s.la = Some(oos_hal::dho::la::Set { thr: [t, t], ..s.la.unwrap_or_default() });
            }
            "--n" => n = int(v)?,
            "--at" => at = num(v)? as i64,
            "--out" => out = v.ok_or("missing value")?.clone(),
            "--bw" => {
                bw = match v.map(|x| x.as_str()) {
                    Some("OFF") => afe::bw::OFF,
                    Some("S20M") | Some("20M") => afe::bw::S20M,
                    Some("AUTO") | Some("Auto") => afe::bw::AUTO,
                    Some(m) => afe::bw::mhz(m.trim_end_matches('M').parse().map_err(|_| format!("bad --bw {}", m))?),
                    None => return Err("missing value".into()),
                }
            }
            "--slope" => {
                s.slope = match v.map(|x| x.as_str()) {
                    Some("rise") => tpu::Slope::Rising,
                    Some("fall") => tpu::Slope::Falling,
                    Some("either") => tpu::Slope::Either,
                    _ => return Err("--slope rise|fall|either".into()),
                }
            }
            a => {
                take = false;
                match a {
                    "--normal" => s.auto = false,
                    "--play4" => s.play4 = true,
                    "--play-last" => s.play4 = false,
                    "--keep-config" => s.blocks = 0,
                    "--header" => s.header = true,
                    "--no-read" => no_read = true,
                    "--afe" => s.afe = true,
                    "--la" => s.la = Some(s.la.unwrap_or_default()),
                    "--la-alone" => {
                        s.la = Some(oos_hal::dho::la::Set { analog_shown: false, ..s.la.unwrap_or_default() })
                    }
                    "--all" => all = true,
                    "--execute" => execute = true,
                    "--raw-too" => raw_too = true,
                    _ => return Err(format!("unknown option {}", a)),
                }
            }
        }
        i += if take { 2 } else { 1 };
    }
    s.ch = [seq::Chan { scale, offset, bw, ..seq::Settings::default().ch[0] }; 4];
    s.window = if all { None } else { Some((n, at)) };
    if !(1..=15).contains(&app_mask) {
        return Err("--app-mask 1..0xf".into());
    }
    if raw_too && s.compress < 2 {
        return Err("--raw-too needs --compress k > 1".into());
    }
    Ok(Opt { s, app_mask, out, execute, no_read, raw_too })
}

fn print(st: &[Step]) {
    for s in st {
        match s {
            Step::Wr((o, v)) => println!("  wr  {:#06x} {:#010x}", o, v),
            Step::Afe(afe::Op::Afe { ch, reg, val }) => println!("  afe ch{} {:#04x} {:#06x}", ch + 1, reg, val),
            Step::Afe(afe::Op::Gear { ch, gear }) => println!("  gear ch{} {}", ch + 1, gear),
            Step::Afe(afe::Op::Adc(w)) => println!("  wr  0x1034 {:#010x}  (ADC SPI)", w),
            Step::Poll { reg, bit, ms, what } => println!("  poll {:#06x} & {:#x} (max {} ms): {}", reg, bit, ms, what),
            Step::Read { frames, frame } => println!(
                "  read {} frames of {} bytes ({} with the stream padding) from /dev/xdma0_c2h_0",
                frames,
                frame,
                seq::read_len(*frames, *frame)
            ),
            Step::Sleep { us } => println!("  sleep {} us", us),
            Step::Peek(r) => println!("  peek {:x?}", r),
            Step::Note(n) => println!("# {}", n),
        }
    }
    println!("# {} hardware writes", st.iter().filter(|s| matches!(s, Step::Wr(_) | Step::Afe(_))).count());
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
    if cmd == "afg" && !oos_hal::model::platform().caps.has_gen() && !flag(&args, "--force") {
        eprintln!(
            "oos-fpgad: the {} has no generator (--force to drive the lines anyway)",
            oos_hal::model::platform().caps.name
        );
        std::process::exit(1);
    }
    let a = args.get(2..).unwrap_or(&[]);
    let done = match cmd {
        "temp" | "beep" | "guard" => Some(board::run(cmd, a)),
        "afg" | "la" => Some(afg_la(cmd, a)),
        "selfcal" => Some(selfcal(a)),
        "roll" => Some(roll(a)),
        "compress-check" => Some(compcheck::run(a).map(|text| print!("{}", text))),
        "made-up-cal" => Some(made_up_cal(a)),
        c if TOOL_CMDS.contains(&c) => Some(tool(cmd, a)),
        _ => None,
    };
    if let Some(r) = done {
        if let Err(e) = r {
            eprintln!("oos-fpgad: {}", e);
            std::process::exit(1);
        }
        return;
    }
    if !matches!(cmd, "plan" | "run") {
        // the module comment, after the licence lines
        let doc = include_str!("main.rs").lines().skip_while(|l| !l.starts_with("//!"));
        for l in doc.take_while(|l| l.starts_with("//!")) {
            println!("{}", l.trim_start_matches("//!").trim_start_matches(' '));
        }
        return;
    }
    let o = match parse(&args[2..]) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("oos-fpgad: {}", e);
            std::process::exit(2);
        }
    };
    let dir = cal_dir();
    let cal = if o.s.afe { afe::Cal::load(&dir).ok() } else { None };
    let cal_adc = adc::CalAdc::load(&dir).ok();
    let mut state = seq::State::handover(o.app_mask, cal_adc.as_ref());
    let (st, plan_p) = match seq::plan(&mut state, &o.s, cal.as_ref(), cal_adc.as_ref()) {
        Ok((mut st, p)) => {
            if o.no_read {
                st.retain(|x| !matches!(x, Step::Read { .. }));
            }
            (st, p)
        }
        Err(e) => {
            eprintln!("oos-fpgad: {}", e);
            std::process::exit(2);
        }
    };
    #[cfg(not(target_os = "linux"))]
    let _ = plan_p;
    if cmd == "plan" || !o.execute {
        print(&st);
        if cmd == "run" {
            eprintln!("oos-fpgad: dry run; add --execute to write the FPGA");
        }
        return;
    }
    #[cfg(target_os = "linux")]
    {
        let mut run = || -> Result<(), String> {
            let mut bar = bar_checked()?;
            let mut afe_bus =
                if o.s.afe { Some(afe::GpioAfe::open().map_err(|e| format!("AFE GPIO: {}", e))?) } else { None };
            let mut c2h = c2h()?;
            let t0 = std::time::Instant::now();
            let log = |m: String| eprintln!("{:8.1} ms  {}", t0.elapsed().as_secs_f64() * 1e3, m);
            let mut raw = oos_hal::dho::bus::DmaBuf::new();
            let bytes = if o.s.frames > 1 {
                // batch: configuration, then arm / wait for the ring / read the recorded frames (max 30 ms extra)
                let mut state = seq::State::handover(o.app_mask, cal_adc.as_ref());
                let (cfg, p) = state.configure(&o.s, cal.as_ref(), cal_adc.as_ref())?;
                seq::execute(&cfg, &mut bar, afe_bus.as_mut(), &mut c2h, log)?;
                let mut dma = oos_hal::dho::bus::DmaBuf::new();
                let n = seq::acquire_batch(&mut state, &o.s, &p, &mut bar, &mut c2h, &mut dma, false, 30, log)?;
                eprintln!(
                    "{:8.1} ms  {} frames of {} bytes",
                    t0.elapsed().as_secs_f64() * 1e3,
                    n,
                    seq::frame_bytes(&o.s, &p)
                );
                if o.raw_too {
                    seq::read_raw_again(&mut state, &o.s, &p, &mut bar, &mut c2h, &mut raw, n.max(1) - 1, log)?;
                }
                dma.into_vec()
            } else {
                let b = seq::execute(&st, &mut bar, afe_bus.as_mut(), &mut c2h, log)?;
                if o.raw_too {
                    seq::read_raw_again(&mut state, &o.s, &plan_p, &mut bar, &mut c2h, &mut raw, 0, log)?;
                }
                b
            };
            if o.raw_too {
                let path = format!("{}.raw", o.out);
                std::fs::write(&path, raw.data()).map_err(|e| format!("{}: {}", path, e))?;
                eprintln!(
                    "{:8.1} ms  {} raw bytes of the same record -> {}",
                    t0.elapsed().as_secs_f64() * 1e3,
                    raw.data().len(),
                    path
                );
            }
            std::fs::write(&o.out, &bytes).map_err(|e| format!("{}: {}", o.out, e))?;
            eprintln!("{:8.1} ms  {} bytes -> {}", t0.elapsed().as_secs_f64() * 1e3, bytes.len(), o.out);
            Ok(())
        };
        if let Err(e) = run() {
            eprintln!("oos-fpgad: {}", e);
            std::process::exit(1);
        }
    }
    #[cfg(not(target_os = "linux"))]
    eprintln!("oos-fpgad: run needs the unit (Linux)");
}

/// A made-up calibration set in the folder named (`oos_hal::dho::madeup`); no hardware access.
fn made_up_cal(a: &[String]) -> Result<(), String> {
    let dir = a.first().ok_or("made-up-cal <dir>")?;
    let files = oos_hal::dho::madeup::write(dir).map_err(|e| e.to_string())?;
    for f in &files {
        println!("{}", f);
    }
    println!("{} files with invented values: for comparing builds on a computer, never for a scope", files.len());
    Ok(())
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

/// BAR1 of the FPGA.
#[cfg(target_os = "linux")]
fn bar() -> Result<oos_hal::dho::bus::Bar1, String> {
    oos_hal::dho::bus::Bar1::open().map_err(|e| format!("/dev/xdma0_bypass: {}", e))
}

/// BAR1, refused unless the FPGA answers with the image OOS knows.
#[cfg(target_os = "linux")]
fn bar_checked() -> Result<oos_hal::dho::bus::Bar1, String> {
    use oos_hal::dho::bus::Bus;
    let mut bar = bar()?;
    let v = bar.rd(0x4000);
    if v != oos_hal::dho::bus::FPGA_VERSION {
        return Err(format!("unexpected FPGA version {:#x}, not touching it", v));
    }
    Ok(bar)
}

/// The card-to-host DMA channel of the records.
#[cfg(target_os = "linux")]
fn c2h() -> Result<std::fs::File, String> {
    std::fs::File::open("/dev/xdma0_c2h_0").map_err(|e| format!("/dev/xdma0_c2h_0: {}", e))
}

#[cfg(test)]
mod tests {
    use super::bringup::xdma_params;
    use super::tool::{pll_dry, slot_lines, status_lines};
    use super::*;

    #[test]
    fn every_tool_command_is_routed() {
        let src = include_str!("tool.rs");
        let body = &src[src.find("fn tool(cmd: &str").unwrap()..];
        let body = &body[..body.find("\n}\n").unwrap()];
        let arms: Vec<&str> = body
            .lines()
            .filter_map(|l| l.strip_prefix("            \"")?.split_once("\" => {").map(|(c, _)| c))
            .collect();
        assert!(arms.len() >= 6, "{:?}", arms);
        for c in arms {
            assert!(TOOL_CMDS.contains(&c), "tool arm {} missing from TOOL_CMDS", c);
        }
    }

    #[test]
    fn status_interpretation() {
        let mut regs = std::collections::HashMap::from([
            (0x4000u32, 0xe207_2115u32),
            (0x4034, 0x2000),
            (0x1008, 1 << 29),
            (0x1210, 62500),
        ]);
        let l = status_lines(&mut |o| *regs.entry(o).or_insert(0));
        assert!(
            l[0].contains("expected") && l[1].contains("stopped") && l[2].contains("pass") && l[3].contains("locked")
        );
    }

    #[test]
    fn slot_lines_show_each_slot_against_the_average() {
        use oos_hal::dho::levels::Slot;
        let mut r = [Slot { min: 2040, max: 2056, mean: 32768.0 }; 8];
        r[5].mean = 32768.0 + 7.0 * 16.0;
        let l = slot_lines(&[r, r]);
        assert!(l[5].ends_with("+6.12 steps"), "{}", l[5]);
        assert!(l[0].ends_with("-0.88 steps") && l[0].contains("min 2040  max 2056"), "{}", l[0]);
        let mut bad = std::collections::HashMap::from([(0x1210u32, 0u32)]);
        assert!(status_lines(&mut |o| *bad.entry(o).or_insert(0))[3].contains("NOT locked"));
    }

    #[test]
    fn pll_dry_run() {
        let args = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let s = pll_dry(&args(&["--freq", "1.5e9", "--dry"])).unwrap().unwrap();
        assert!(s.contains("R38=002c") && s.contains("R45=0002") && s.contains("4500.000000 MHz"), "{}", s);
        assert!(pll_dry(&args(&["--freq", "1500000000"])).unwrap_err().contains("only 1.25 GHz"));
        assert_eq!(pll_dry(&args(&["--freq", "1.25e9"])).unwrap(), None);
        assert_eq!(pll_dry(&args(&["--reset"])).unwrap(), None);
        assert!(pll_dry(&args(&["--freq", "5e9", "--dry"])).is_err());
    }

    // Break it catches: a default that changes without a hardware check, or a value that puts more than the two known
    // parameters on the insmod shell line.
    #[test]
    fn xdma_parameters_from_the_system_settings() {
        assert_eq!(xdma_params(None).unwrap(), "poll_mode=1");
        assert_eq!(xdma_params(Some(" \n")).unwrap(), "poll_mode=1");
        assert_eq!(xdma_params(Some("poll_mode=0\n")).unwrap(), "poll_mode=0");
        assert_eq!(xdma_params(Some("poll_mode=0   interrupt_mode=2")).unwrap(), "poll_mode=0 interrupt_mode=2");
        for bad in [
            "poll_mode=2",
            "poll_mode=0; reboot",
            "poll_mode=0 poll_mode=1",
            "interrupt_mode=4",
            "c2h_timeout=1",
            "poll_mode=0 $(id)",
        ] {
            assert!(xdma_params(Some(bad)).is_err(), "{}", bad);
        }
    }

    #[test]
    fn options() {
        let o = parse(&["--mask", "3", "--scale", "0.5", "--all"].map(String::from)).unwrap();
        assert_eq!(o.s.mask, 3);
        assert_eq!(o.s.ch[1].scale, 0.5);
        assert!(o.s.window.is_none());
        assert!(parse(&["--bogus".to_string()]).is_err());
    }
}
