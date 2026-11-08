// Copyright (C) 2026 Julian Decker
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
//!   oos-fpgad gpio <line> <0|1> [--hold 60]  drive a GPIO line (by DT name) and keep it for --hold seconds
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
//!   --blocks 7         configuration blocks before the arm: bit0 trigger, bit1 acquisition, bit2 auto trigger
//!   --afe              program the AFEs of the enabled channels (scale/offset, bandwidth --bw OFF|S20M|AUTO|<MHz>)
//!   --la               logic analyser on (la.md, UNTESTED): LA set-up and a second, raw LA read after the analog one
//!                      (run writes the bytes of that last read, the LA words); --la-thr 1.4 threshold of both pods in V
//!                      (--la-alone: no analog channel shown, the analog rate is not halved)
//!   --out /tmp/frame.bin
//! A channel set different from --app-mask switches the ADC mode; --afe and a mode switch read the calibration from
//! OOS_CAL_DIR (default /rigol/data).

mod board;

use oos_hal::dho::seq::{self, Settings, Step};
use oos_hal::dho::{adc, afe, tpu};
#[cfg(target_os = "linux")]
use oos_hal::bootstatus;

struct Opt {
    s: Settings,
    app_mask: u32,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    out: String,
    execute: bool,
    no_read: bool,
}

fn parse(args: &[String]) -> Result<Opt, String> {
    let mut s = Settings::default();
    let (mut app_mask, mut out, mut execute, mut no_read) = (1u32, "/tmp/frame.bin".to_string(), false, false);
    let (mut scale, mut offset, mut bw) = (1.0, 0.0, afe::bw::OFF);
    let (mut n, mut at, mut all) = (4000u32, -2000i64, false);
    let mut i = 0;
    let num = |v: Option<&String>| -> Result<f64, String> { v.ok_or("missing value")?.parse::<f64>().map_err(|e| e.to_string()) };
    let int = |v: Option<&String>| -> Result<u32, String> {
        let t = v.ok_or("missing value")?;
        match t.strip_prefix("0x") {
            Some(h) => u32::from_str_radix(h, 16).map_err(|e| e.to_string()),
            None => t.parse().map_err(|e: std::num::ParseIntError| e.to_string()),
        }
    };
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
                    "--la-alone" => s.la = Some(oos_hal::dho::la::Set { analog_shown: false, ..s.la.unwrap_or_default() }),
                    "--all" => all = true,
                    "--execute" => execute = true,
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
    Ok(Opt { s, app_mask, out, execute, no_read })
}

fn print(st: &[Step]) {
    for s in st {
        match s {
            Step::Wr((o, v)) => println!("  wr  {:#06x} {:#010x}", o, v),
            Step::Afe(afe::Op::Afe { ch, reg, val }) => println!("  afe ch{} {:#04x} {:#06x}", ch + 1, reg, val),
            Step::Afe(afe::Op::Gear { ch, gear }) => println!("  gear ch{} {}", ch + 1, gear),
            Step::Afe(afe::Op::Adc(w)) => println!("  wr  0x1034 {:#010x}  (ADC SPI)", w),
            Step::Poll { reg, bit, ms, what } => println!("  poll {:#06x} & {:#x} (max {} ms): {}", reg, bit, ms, what),
            Step::Read { frames, frame } => println!("  read {} frames of {} bytes ({} with the stream padding) from /dev/xdma0_c2h_0", frames, frame, seq::read_len(*frames, *frame)),
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
    let opt_arg = |k: &str| args.iter().position(|x| x == k).and_then(|i| args.get(i + 1)).cloned();
    let cal_dir = std::env::var("OOS_CAL_DIR").unwrap_or_else(|_| "/rigol/data".into());
    let platform = oos_hal::model::Platform::detect(&cal_dir, &opt_arg("--limits").unwrap_or_else(|| oos_hal::model::LIMITS_CONF.into()), opt_arg("--model").as_deref());
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
        println!("afg max   sine {} square {} ramp {} pulse {} noise {} Hz", l.afg_max_hz.sine, l.afg_max_hz.square, l.afg_max_hz.ramp, l.afg_max_hz.pulse, l.afg_max_hz.noise);
        println!("clocks    ADC {} Hz, generator {} Hz", l.adc_clock_hz, l.afg_clock_hz);
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
            eprintln!("oos-fpgad: this unit is a {}, another board: OOS does not drive its hardware (docs/porting.md)", m);
            std::process::exit(3);
        }
    }
    if cmd == "afg" && !oos_hal::model::platform().caps.afg && !args.iter().any(|x| x == "--force") {
        eprintln!("oos-fpgad: the {} has no generator (--force to drive the lines anyway)", oos_hal::model::platform().caps.name);
        std::process::exit(1);
    }
    if matches!(cmd, "temp" | "beep" | "guard") {
        if let Err(e) = board::run(cmd, &args[2..]) {
            eprintln!("oos-fpgad: {}", e);
            std::process::exit(1);
        }
        return;
    }
    if cmd == "afg" || cmd == "la" {
        if let Err(e) = afg_la(cmd, &args[2..]) {
            eprintln!("oos-fpgad: {}", e);
            std::process::exit(1);
        }
        return;
    }
    if cmd == "selfcal" {
        if let Err(e) = selfcal(&args[2..]) {
            eprintln!("oos-fpgad: {}", e);
            std::process::exit(1);
        }
        return;
    }
    if cmd == "roll" {
        if let Err(e) = roll(&args[2..]) {
            eprintln!("oos-fpgad: {}", e);
            std::process::exit(1);
        }
        return;
    }
    if matches!(cmd, "status" | "rd" | "wr" | "cold-boot" | "fpga-boot" | "pll" | "gpio") {
        if let Err(e) = tool(cmd, &args[2..]) {
            eprintln!("oos-fpgad: {}", e);
            std::process::exit(1);
        }
        return;
    }
    if !matches!(cmd, "plan" | "run") {
        for l in include_str!("main.rs").lines().take_while(|l| l.starts_with("//!")) {
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
    let dir = std::env::var("OOS_CAL_DIR").unwrap_or_else(|_| "/rigol/data".into());
    let cal = if o.s.afe { afe::Cal::load(&dir).ok() } else { None };
    let cal_adc = adc::CalAdc::load(&dir).ok();
    let mut state = seq::State::handover(o.app_mask, cal_adc.as_ref());
    let st = match seq::plan(&mut state, &o.s, cal.as_ref(), cal_adc.as_ref()) {
        Ok((mut st, _)) => {
            if o.no_read {
                st.retain(|x| !matches!(x, Step::Read { .. }));
            }
            st
        }
        Err(e) => {
            eprintln!("oos-fpgad: {}", e);
            std::process::exit(2);
        }
    };
    if cmd == "plan" || !o.execute {
        print(&st);
        if cmd == "run" {
            eprintln!("oos-fpgad: dry run; add --execute to write the FPGA");
        }
        return;
    }
    #[cfg(target_os = "linux")]
    {
        use oos_hal::dho::bus::{Bar1, Bus, FPGA_VERSION};
        let run = || -> Result<(), String> {
            let mut bar = Bar1::open().map_err(|e| format!("/dev/xdma0_bypass: {}", e))?;
            let v = bar.rd(0x4000);
            if v != FPGA_VERSION {
                return Err(format!("unexpected FPGA version {:#x}, not touching it", v));
            }
            let mut afe_bus = if o.s.afe { Some(afe::GpioAfe::open().map_err(|e| format!("AFE GPIO: {}", e))?) } else { None };
            let mut c2h = std::fs::File::open("/dev/xdma0_c2h_0").map_err(|e| format!("/dev/xdma0_c2h_0: {}", e))?;
            let t0 = std::time::Instant::now();
            let log = |m: String| eprintln!("{:8.1} ms  {}", t0.elapsed().as_secs_f64() * 1e3, m);
            let bytes = if o.s.frames > 1 {
                // batch: configuration, then arm / wait for the ring / read the recorded frames (max 30 ms extra)
                let mut state = seq::State::handover(o.app_mask, cal_adc.as_ref());
                let (cfg, p) = state.configure(&o.s, cal.as_ref(), cal_adc.as_ref())?;
                seq::execute(&cfg, &mut bar, afe_bus.as_mut(), &mut c2h, log)?;
                let (b, n) = seq::acquire_batch(&mut state, &o.s, &p, &mut bar, &mut c2h, 30, log)?;
                eprintln!("{:8.1} ms  {} frames of {} bytes", t0.elapsed().as_secs_f64() * 1e3, n, seq::frame_bytes(&o.s, &p));
                b
            } else {
                seq::execute(&st, &mut bar, afe_bus.as_mut(), &mut c2h, log)?
            };
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

fn num(s: &str) -> Result<u32, String> {
    match s.strip_prefix("0x") {
        Some(h) => u32::from_str_radix(h, 16),
        None => s.parse(),
    }
    .map_err(|e| format!("{}: {}", s, e))
}

/// Roll mode test: configuration as for `run`, then the roll display chain, streaming for `--secs`, stop.
fn roll(a: &[String]) -> Result<(), String> {
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
    let slow_cfg = |sh: &mut oos_hal::dho::scu::Shadows| if scan { oos_hal::dho::roll::config_scan(sh, &rp, p.memsel, 0) } else { oos_hal::dho::roll::config(sh, &rp, p.memsel) };
    println!("{}: {} Sa/s, memsel {}, k {}, f {}/{}, len {}, {:.3} us per point, chunk = {:.3} ms", name, p.rate, p.memsel, rp.k, rp.f_num, rp.f_den, rp.len, rp.spacing_fs as f64 / 1e9, rp.spacing_fs as f64 * 100.0 / rp.ratio as f64 / 1e12);
    if !o.execute {
        let mut sh = seq::State::handover(o.app_mask, None).sh;
        for (r, v) in slow_cfg(&mut sh).iter().chain(oos_hal::dho::roll::start(&mut sh, o.s.mask).iter()) {
            println!("  wr  {:#06x} {:#010x}", r, v);
        }
        eprintln!("oos-fpgad: dry run; add --execute");
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        use oos_hal::dho::bus::{Bar1, Bus, FPGA_VERSION};
        use std::io::Write as _;
        let dir = std::env::var("OOS_CAL_DIR").unwrap_or_else(|_| "/rigol/data".into());
        let cal = if o.s.afe { afe::Cal::load(&dir).ok() } else { None };
        let cal_adc = adc::CalAdc::load(&dir).ok();
        let mut bar = Bar1::open().map_err(|e| format!("/dev/xdma0_bypass: {}", e))?;
        if bar.rd(0x4000) != FPGA_VERSION {
            return Err("unexpected FPGA version, not touching it".into());
        }
        let mut afe_bus = if o.s.afe { Some(afe::GpioAfe::open().map_err(|e| format!("AFE GPIO: {}", e))?) } else { None };
        let mut c2h = std::fs::File::open("/dev/xdma0_c2h_0").map_err(|e| format!("/dev/xdma0_c2h_0: {}", e))?;
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
                let buf = oos_hal::dho::bus::read_dma(&mut c2h, c * oos_hal::dho::roll::CHUNK_BYTES, |_, _| {}).map_err(|e| format!("c2h read of {} chunks (0x11e8 = {}): {}", c, n, e))?;
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
                    println!("{:5.2} s  0x1204 = {:#010x}, 0x40d0 = {:#x} after {} chunks of this sweep", t0.elapsed().as_secs_f64(), r[0], r[1], sweep_chunks);
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
                println!("{:5.1} s  {} chunks in the last second (last 0x11e8 = {}, {} chunks)", t0.elapsed().as_secs_f64(), csec, n, c);
                tsec = t0.elapsed().as_secs();
                csec = 0;
            }
        }
        bar.apply(&oos_hal::dho::roll::stop(&mut state.sh));
        println!("{}: {} chunks in {} reads ({} empty polls) in {:.1} s; 0x1208 = {:#010x}", name, total, reads, empty, t0.elapsed().as_secs_f64(), bar.rd(0x1208));
        if scan {
            println!("scan: {} complete sweeps", sweeps);
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (secs, name);
        Err("roll --execute needs the unit (Linux)".into())
    }
}

/// Interpretation of the read-only status registers.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn status_lines(rd: &mut dyn FnMut(u32) -> u32) -> Vec<String> {
    use oos_hal::dho::boot::DDR_INIT_PASS;
    let (cmin, cmax) = oos_hal::dho::boot::adc_clk_window();
    let v = rd(0x4000);
    let st = rd(0x4034);
    let ddr = rd(0x1008);
    let clk = rd(0x1210);
    let (last, first, play) = (rd(0x4010), rd(0x4014), rd(0x4080));
    let x = rd(oos_hal::dho::board::XADC_REG);
    let (t, vcc) = oos_hal::dho::board::xadc(x);
    vec![
        format!("FPGA version 0x4000 = {:#010x} {}", v, if v == oos_hal::dho::bus::FPGA_VERSION { "(expected)" } else { "(UNEXPECTED)" }),
        format!("status 0x4034 = {:#010x}: FSM {}, {}", st, if st & 0x2000 != 0 { "stopped" } else { "running" }, if st & 0x10 != 0 { "waiting for trigger" } else { "not waiting" }),
        format!("DDR 0x1008 = {:#010x}: init {}", ddr, if ddr & DDR_INIT_PASS != 0 { "pass" } else { "NOT passed" }),
        format!("ADC clock count 0x1210 = {} -> PLL {}", clk, if (cmin..=cmax).contains(&clk) { "locked" } else { "NOT locked / not running" }),
        format!("XADC 0x1438 = {:#010x}: FPGA die {:.1} C, VCCINT {:.3} V", x, t, vcc),
        format!("play window 0x4014..0x4010 = {}..{} ({} frames), 0x4080 = {:#010x} (done bit30 {})", first, last, last.wrapping_sub(first).wrapping_add(1), play, (play >> 30) & 1),
    ]
}

/// PCIe host + xdma bring-up with retries (the loop of oos-fpga-up, without the FPGA boot), then BAR1. The device and
/// the xdma nodes are polled every 20 ms instead of the fixed 2 s / 1 s sleeps of oos-fpga-up (`insmod` returns once
/// the driver probed, so a link that is up shows at once).
#[cfg(target_os = "linux")]
fn pcie_up() -> std::io::Result<oos_hal::dho::bus::Bar1> {
    let sh = |c: &str| std::process::Command::new("sh").arg("-c").arg(c).status().map(|s| s.success()).unwrap_or(false);
    let poll = |cond: &dyn Fn() -> bool, ms: u64| {
        let t = std::time::Instant::now();
        while !cond() {
            if t.elapsed().as_millis() as u64 >= ms {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        true
    };
    let dev = || std::path::Path::new("/sys/bus/pci/devices/0000:01:00.0").exists();
    let m = "/lib/modules/$(uname -r)";
    if !sh("lsmod | grep -q '^xdma '") {
        for _ in 0..6 {
            sh(&format!("insmod {}/kernel/drivers/pci/controller/pcierockchiphost.ko 2>/dev/null", m));
            if poll(&dev, 2000) {
                break;
            }
            sh("rmmod pcierockchiphost 2>/dev/null");
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        if !dev() {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "FPGA did not appear on PCIe"));
        }
        if !sh(&format!("insmod {}/updates/xdma.ko poll_mode=1", m)) {
            return Err(std::io::Error::other("insmod xdma failed"));
        }
        let nodes = || ["/dev/xdma0_bypass", "/dev/xdma0_c2h_0"].iter().all(|n| std::path::Path::new(n).exists());
        if !poll(&nodes, 3000) {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "xdma loaded, but /dev/xdma0_* did not appear"));
        }
    }
    oos_hal::dho::bus::Bar1::open()
}

/// Uptime of the BOOT command sent by `oos-fpgad fpga-boot`.
#[cfg(target_os = "linux")]
const BOOTED_FILE: &str = "/tmp/oos-fpga-booted";

/// Start-up self-test: every calibration file OOS reads is there with a good CRC (a missing one only warns: the
/// scope runs on defaults, less accurate).
#[cfg(target_os = "linux")]
fn check_cal(cal: &oos_hal::dho::boot::CalFiles, dir: &str, say: &dyn Fn(&str, &str, &str)) {
    const STAGE: &str = "Calibration data";
    say("run", STAGE, "");
    let mut missing = Vec::new();
    if cal.adc.is_none() {
        missing.push("cal_adc.hex");
    }
    if cal.ddr.is_none() {
        missing.push("cal_ddr.hex");
    }
    if afe::Cal::load_from(dir).is_err() {
        missing.push("vertical");
    }
    if oos_hal::dho::afg::Cal::load(dir).is_none() {
        missing.push("cal_afg.hex");
    }
    if missing.is_empty() {
        say("ok", STAGE, "all files read, CRC ok");
    } else {
        say("warn", STAGE, &format!("missing or damaged: {}", missing.join(", ")));
    }
}

#[cfg(target_os = "linux")]
fn bar_for_temps() -> Option<oos_hal::dho::bus::Bar1> {
    oos_hal::dho::bus::Bar1::open().ok()
}

/// Start-up self-test: SoC and FPGA temperatures below the guard's limits (`board::is_hot`).
#[cfg(target_os = "linux")]
fn check_temps(bar: &mut Option<oos_hal::dho::bus::Bar1>, say: &dyn Fn(&str, &str, &str)) {
    use oos_hal::dho::board;
    const STAGE: &str = "Temperatures";
    say("run", STAGE, "");
    let zones = board::thermal_zones(std::path::Path::new("/sys/class/thermal"));
    let fpga = bar.as_mut().and_then(|b| board::read_xadc(b)).map(|x| x.0);
    let soc = zones.iter().map(|z| z.temp).fold(f32::NAN, f32::max);
    // 0x1438 reads 0 on the DHO924S, under stock as well: no reading is no fault
    let txt = format!("SoC {:.0} C{}", soc, fpga.map_or(String::new(), |t| format!(", FPGA {:.0} C", t)));
    if board::is_hot(&zones, fpga) {
        say("warn", STAGE, &txt);
    } else {
        say("ok", STAGE, &txt);
    }
}

/// `pll --freq <Hz> --dry`: the table `oos_hal::dho::lmx2582::table_for` makes and its decode, nothing written. Without
/// --dry a --freq other than 1.25 GHz is refused (no other table has run on a unit); None = go on with the stock table.
fn pll_dry(a: &[String]) -> Result<Option<String>, String> {
    use oos_hal::dho::lmx2582;
    let freq = a.iter().position(|x| x == "--freq").map(|i| a.get(i + 1).ok_or("--freq <Hz>")?.parse::<f64>().map_err(|e| format!("--freq: {}", e))).transpose()?;
    if !a.iter().any(|x| x == "--dry") {
        return match freq {
            Some(f) if f != 1.25e9 => Err(format!("pll --freq {}: only 1.25 GHz is tested on the unit; other clocks only with --dry for now", f)),
            _ => Ok(None),
        };
    }
    let f = freq.unwrap_or(1.25e9);
    let t = lmx2582::table_for(f)?;
    let d = lmx2582::Fields::decode(&t)?;
    let mut s = format!("LMX2582 table for {} Hz (dry run, nothing written; R0 last starts the VCO calibration)\n{}\n", f, lmx2582::format_table(&t));
    for l in d.lines() {
        s += &format!("  {}\n", l);
    }
    let changed: Vec<String> = t.iter().zip(oos_hal::dho::boot::regs::PLL_TABLE.iter()).filter(|(x, y)| x != y).map(|(x, _)| format!("R{}", x[0])).collect();
    s += &format!("  changed vs. 1.25 GHz: {}\n", if changed.is_empty() { "none".into() } else { changed.join(" ") });
    for n in d.notes() {
        s += &format!("  note: {}\n", n);
    }
    Ok(Some(s))
}

fn tool(cmd: &str, a: &[String]) -> Result<(), String> {
    let has = |f: &str| a.iter().any(|x| x == f);
    let dir = std::env::var("OOS_CAL_DIR").unwrap_or_else(|_| "/rigol/data".into());
    if cmd == "cold-boot" && !has("--execute") {
        let cal = oos_hal::dho::boot::CalFiles::load(&dir);
        print!("{}", oos_hal::dho::boot::dry_run(&cal, &oos_hal::dho::boot::Options::default()));
        eprintln!("oos-fpgad: dry run against the unit model; add --execute on the unit");
        return Ok(());
    }
    if cmd == "pll" {
        if let Some(t) = pll_dry(a)? {
            print!("{}", t);
            return Ok(());
        }
    }
    // unit 2026-11-03: a second cold boot while PCIe/XDMA from the first one are loaded hung the whole SoC (no ping);
    // a failed cold boot needs a power cycle, never a retry in the same boot
    if (cmd == "cold-boot" || cmd == "fpga-boot") && std::path::Path::new("/sys/module/xdma").exists() && !has("--force") {
        return Err("XDMA is already loaded (an earlier cold boot or hand-over in this boot): power cycle the scope instead of retrying".into());
    }
    #[cfg(target_os = "linux")]
    {
        use oos_hal::dho::bus::{Bar1, Bus};
        match cmd {
            "status" => {
                let mut bar = Bar1::open().map_err(|e| format!("/dev/xdma0_bypass: {}", e))?;
                for l in status_lines(&mut |o| bar.rd(o)) {
                    println!("{}", l);
                }
            }
            "gpio" => {
                let name = a.first().ok_or("gpio <line> <0|1> [--hold s]")?;
                let v = a.get(1).ok_or("gpio <line> <0|1> [--hold s]")? == "1";
                let hold = a.iter().position(|x| x == "--hold").and_then(|i| a.get(i + 1)).map(|x| x.parse::<u64>().map_err(|e| e.to_string())).transpose()?.unwrap_or(0);
                let _l = oos_hal::dho::gpio::Line::output(name, v).map_err(|e| format!("{}: {}", name, e))?;
                println!("{} = {} (held {} s)", name, v as u8, hold);
                std::thread::sleep(std::time::Duration::from_secs(hold));
            }
            "pll" => {
                use oos_hal::dho::boot::{regs, PllPins};
                let mut pll = oos_hal::dho::boot::io::GpioPll::open().map_err(|e| format!("PLL lines: {}", e))?;
                let send = |pll: &mut oos_hal::dho::boot::io::GpioPll, w: [u8; 3]| {
                    for (p, v) in regs::pll_edges(w) {
                        pll.set(p, v);
                    }
                    std::thread::sleep(std::time::Duration::from_micros(regs::PLL_WORD_GAP_US));
                };
                // idle state of the stock module after insmod: LE high (the first word then starts with a falling edge)
                pll.set(regs::PllPin::Le, true);
                std::thread::sleep(std::time::Duration::from_millis(10));
                if let Some(v) = a.iter().position(|x| x == "--r0").and_then(|i| a.get(i + 1)) {
                    // diagnostic: one R0 word only (e.g. 0x221d = POWERDOWN set: the counter must drop to 0)
                    let v = num(v)?;
                    send(&mut pll, [0, (v >> 8) as u8, v as u8]);
                    println!("LMX2582: R0 <- {:#06x}", v);
                    let mut bar = Bar1::open().map_err(|e| format!("/dev/xdma0_bypass: {}", e))?;
                    for _ in 0..3 {
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        println!("0x1210 = {}", bar.rd(0x1210));
                    }
                    return Ok(());
                }
                if has("--reset") {
                    send(&mut pll, [0, 0x00, 0x02]);
                    send(&mut pll, [0, 0x00, 0x00]);
                    println!("LMX2582 reset (R0 = 0x0002, 0x0000)");
                }
                // Rigol's table, or the experimental one of limits.conf `adc.clock_hz` (as the cold boot)
                let table = oos_hal::dho::boot::pll_table()?;
                if oos_hal::dho::clock::overclocked() {
                    println!("LMX2582: EXPERIMENTAL ADC clock {} Hz from limits.conf", oos_hal::dho::clock::adc_hz());
                }
                for w in table {
                    send(&mut pll, w);
                }
                println!("LMX2582: {} words written", table.len());
                std::thread::sleep(std::time::Duration::from_millis(100));
                let mut bar = Bar1::open().map_err(|e| format!("/dev/xdma0_bypass: {}", e))?;
                for _ in 0..3 {
                    println!("0x1210 = {}", bar.rd(0x1210));
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
            "rd" => {
                let off = num(a.first().ok_or("rd <offset> [count]")?)?;
                let n = a.get(1).map(|x| num(x)).transpose()?.unwrap_or(1);
                let mut bar = Bar1::open().map_err(|e| format!("/dev/xdma0_bypass: {}", e))?;
                for k in 0..n {
                    let o = off + 4 * k;
                    println!("{:#06x} = {:#010x}", o, bar.rd(o));
                }
            }
            "wr" => {
                let (off, v) = (num(a.first().ok_or("wr <offset> <value> --execute")?)?, num(a.get(1).ok_or("wr <offset> <value> --execute")?)?);
                if off == 0x1008 {
                    return Err("0x1008 refused (DDR status / BRAM fallback bit, cold-boot.md never-do list)".into());
                }
                if !has("--execute") {
                    println!("dry run: would write {:#06x} <- {:#010x}; add --execute", off, v);
                    return Ok(());
                }
                let mut bar = Bar1::open().map_err(|e| format!("/dev/xdma0_bypass: {}", e))?;
                bar.wr(off, v);
                println!("{:#06x} <- {:#010x}", off, v);
            }
            "fpga-boot" => {
                let slot = a.iter().position(|x| x == "--slot").and_then(|i| a.get(i + 1)).map(|x| num(x)).transpose()?.unwrap_or(0x40_0000);
                let progress = a.iter().position(|x| x == "--progress").and_then(|i| a.get(i + 1)).cloned();
                oos_hal::dho::boot::io::fpga_boot(slot).map_err(|e| {
                    if let Some(p) = &progress {
                        // the cold boot sends it again (rcS runs it with --booted, which falls back)
                        bootstatus::append(p, "warn", "FPGA image", &format!("early BOOT command failed ({}), retried later", e));
                    }
                    format!("FPGA boot: {}", e)
                })?;
                let t = oos_hal::dho::boot::io::uptime();
                std::fs::write(BOOTED_FILE, format!("{:.2}\n", t)).map_err(|e| format!("{}: {}", BOOTED_FILE, e))?;
                if let Some(p) = &progress {
                    bootstatus::append(p, "run", "FPGA image", "loading the image from flash");
                }
                println!("BOOT {:#x} sent at uptime {:.2} s", slot, t);
            }
            _ => {
                // cold-boot --execute
                let progress = a.iter().position(|x| x == "--progress").and_then(|i| a.get(i + 1)).cloned();
                let say = |state: &str, stage: &str, detail: &str| {
                    if let Some(p) = &progress {
                        bootstatus::append(p, state, stage, detail);
                    }
                };
                let cal = oos_hal::dho::boot::CalFiles::load(&dir);
                check_cal(&cal, &dir, &say);
                let opt = oos_hal::dho::boot::Options { ignore_clock: has("--ignore-clock"), ..Default::default() };
                // the start-up screen's stage of the running step: ok when the next one starts
                let mut cur: Option<&'static str> = None;
                let report = if has("--power-on") {
                    let slot = a.iter().position(|x| x == "--slot").and_then(|i| a.get(i + 1)).map(|x| num(x)).transpose()?.unwrap_or(0x40_0000);
                    // --booted without the file (the early BOOT command failed): send it here
                    let booted = has("--booted").then(|| std::fs::read_to_string(BOOTED_FILE).ok().and_then(|t| t.trim().parse::<f64>().ok())).flatten();
                    if booted.is_none() {
                        say("run", "FPGA image", "loading the image from flash");
                    }
                    let up = || {
                        say("run", "FPGA image", "PCIe link");
                        pcie_up()
                    };
                    let mut step = |n: u32| {
                        let g = oos_hal::dho::boot::stage(n);
                        if cur != Some(g) {
                            if let Some(c) = cur {
                                say("ok", c, "");
                            }
                            say("run", g, "");
                            cur = Some(g);
                        }
                    };
                    let r = oos_hal::dho::boot::io::power_on(slot, booted, up, &cal, &opt, &mut step);
                    if let Err(e) = &r {
                        // before the first step: the BOOT command / PCIe, or the PLL / AFE lines (e.g. EBUSY)
                        let stage = if e.starts_with("PLL lines") || e.starts_with("AFE lines") { "GPIO lines" } else { "FPGA image" };
                        say("fail", cur.unwrap_or(stage), e);
                    }
                    r?
                } else {
                    let mut bar = Bar1::open().map_err(|e| format!("/dev/xdma0_bypass: {}", e))?;
                    let mut pll = oos_hal::dho::boot::io::GpioPll::open().map_err(|e| format!("PLL lines: {}", e))?;
                    let mut afe = oos_hal::dho::afe::GpioAfe::open().map_err(|e| format!("AFE lines: {}", e))?;
                    let mut clk = oos_hal::dho::boot::SysClock::default();
                    oos_hal::dho::boot::cold_boot(oos_hal::dho::boot::Hw { bus: &mut bar, clock: &mut clk, pll: Some(&mut pll), afe: Some(&mut afe), early: None }, &cal, &opt)
                };
                print!("{}", oos_hal::dho::boot::format_log(&report.log));
                match report.result {
                    Ok(o) => {
                        println!("cold boot ok: {}", o.summary());
                        if let Some(c) = cur {
                            say("ok", c, "");
                        }
                        if !o.warnings.is_empty() {
                            say("warn", "Cold-boot checks", &o.warnings.join("; "));
                        }
                        if !o.idelay_scanned {
                            say("warn", oos_hal::dho::boot::stage(12), "eye scan failed, stored lane delays loaded");
                        }
                        if !o.trim_done {
                            say("warn", oos_hal::dho::boot::stage(14), "ADC trim timeout");
                        }
                        check_temps(&mut bar_for_temps(), &say);
                    }
                    Err(e) => {
                        say("fail", cur.unwrap_or("FPGA image"), &e);
                        return Err(format!("cold boot stopped: {}", e));
                    }
                }
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (has, num("0").ok());
        Err(format!("{} needs the unit (Linux)", cmd))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_interpretation() {
        let mut regs = std::collections::HashMap::from([(0x4000u32, 0xe207_2115u32), (0x4034, 0x2000), (0x1008, 1 << 29), (0x1210, 62500)]);
        let l = status_lines(&mut |o| *regs.entry(o).or_insert(0));
        assert!(l[0].contains("expected") && l[1].contains("stopped") && l[2].contains("pass") && l[3].contains("locked"));
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

    #[test]
    fn options() {
        let o = parse(&["--mask", "3", "--scale", "0.5", "--all"].map(String::from)).unwrap();
        assert_eq!(o.s.mask, 3);
        assert_eq!(o.s.ch[1].scale, 0.5);
        assert!(o.s.window.is_none());
        assert!(parse(&["--bogus".to_string()]).is_err());
    }
}

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
        let m = oos_hal::dho::adc::measure(&mut self.bar, &mut |ms| std::thread::sleep(std::time::Duration::from_millis(ms as u64)))?;
        Some(m.iter().sum::<f64>() / 8.0)
    }
    fn sleep_ms(&mut self, ms: u32) {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }
}

fn selfcal(a: &[String]) -> Result<(), String> {
    #[cfg_attr(not(target_os = "linux"), allow(unused_imports))]
    use oos_hal::dho::{calfile, selfcal as sc};
    let what = a.first().map(|s| s.as_str()).ok_or("selfcal entry|zero|vertical --ch N ...")?;
    let get = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    let has = |k: &str| a.iter().any(|x| x == k);
    let ch = get("--ch").ok_or("--ch 1..4")?.parse::<usize>().map_err(|e| e.to_string())?.checked_sub(1).filter(|c| *c < 4).ok_or("--ch 1..4")?;
    let var = get("--var").map_or(Ok(2), |v| v.parse::<usize>()).map_err(|e| e.to_string())?.min(3);
    let limit = get("--limit").map_or(Ok(usize::MAX), |v| v.parse::<usize>()).map_err(|e| e.to_string())?;
    let rigol = std::env::var("OOS_CAL_DIR").unwrap_or_else(|_| "/rigol/data".into());
    let cal = afe::Cal::load(&rigol).map_err(|e| format!("{}: {}", rigol, e))?;
    let block = cal.block(var, ch);
    let entries = sc::entries_of(&block);
    let tuples: Vec<[i32; 7]> = match get("--tuple") {
        Some(t) => {
            let v: Vec<i32> = t.split(',').map(|x| x.trim().parse::<i32>()).collect::<Result<_, _>>().map_err(|e| e.to_string())?;
            vec![v.try_into().map_err(|_| "--tuple needs 7 values")?]
        }
        None if what == "entry" => vec![entries.iter().find(|e| e.g[0] == 0).ok_or("no IG 0 configuration in the table")?.g],
        None => entries.iter().map(|e| e.g).take(limit).collect(),
    };
    println!("selfcal {} CH{} block group {} ({} configurations in the table; inputs must be open)", what, ch + 1, var, entries.len());
    if !has("--execute") {
        match what {
            "zero" => println!("  3 AFE configurations {:?}, binary searches against the AFE comparators (reg 0 bits 1..3); the offset DAC is switched off meanwhile; table now {:?}", sc::ZERO_CONFIGS, cal.zero_trims(ch)),
            _ => {
                for g in &tuples {
                    let e = entries.iter().find(|e| &e.g == g);
                    println!("  {:?}: table scale {} nV/div, zero {}", g, e.map_or(f64::NAN, |e| e.scale), e.map_or(0, |e| e.zero));
                }
            }
        }
        eprintln!("oos-fpgad: dry run; add --execute (writes AFE registers and the offset DAC of CH{} only)", ch + 1);
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        use oos_hal::dho::bus::{Bar1, FPGA_VERSION};
        use oos_hal::dho::bus::Bus;
        let mut bar = Bar1::open().map_err(|e| format!("/dev/xdma0_bypass: {}", e))?;
        if bar.rd(0x4000) != FPGA_VERSION {
            return Err("unexpected FPGA version, not touching it".into());
        }
        let mut afe_bus = afe::GpioAfe::open().map_err(|e| format!("AFE GPIO: {}", e))?;
        // a full configuration of the channel first, so the AFE shadows match the unit (ADC mode = this channel only)
        let cal_adc = adc::CalAdc::load(&rigol).ok();
        let mut state = seq::State::handover(1, cal_adc.as_ref());
        let st = Settings { mask: 1 << ch, trig_src: ch as u32 + 1, afe: true, ..Settings::default() };
        let (steps, _) = state.configure(&st, Some(&cal), cal_adc.as_ref())?;
        let mut c2h = std::fs::File::open("/dev/xdma0_c2h_0").map_err(|e| format!("/dev/xdma0_c2h_0: {}", e))?;
        seq::execute(&steps, &mut bar, Some(&mut afe_bus), &mut c2h, |_| {})?;
        if what == "adc" {
            // only the ADC core gains/offsets of this channel (mode = the channel alone); --save writes /oos/cal
            let cancel = || false;
            let (mut bar, mut afe_bus) = (bar, afe_bus);
            let base = cal_adc.as_ref().ok_or("no cal_adc.hex")?;
            let mut unit = sc::Unit { bar: &mut bar, afe: &mut afe_bus, c2h: &mut c2h, state: &mut state, cal: &cal, cal_adc: Some(base), cancel: &cancel, adc_new: None };
            let t0 = std::time::Instant::now();
            let new = sc::adc_cores(&mut unit, base, &[ch], &mut |d, t, s| println!("  {:6.1} s  {}/{}  {}", t0.elapsed().as_secs_f64(), d, t, s))?;
            use oos_hal::dho::adc::CalAdc;
            let row = |c: &CalAdc, f: &dyn Fn(usize) -> usize| (0..8).map(|n| c.get16(f(n))).collect::<Vec<_>>();
            println!("  gains   new {:?}  old {:?}", row(&new, &|n| CalAdc::gain_at(ch, n)), row(base, &|n| CalAdc::gain_at(ch, n)));
            println!("  offsets new {:?}  old {:?}", row(&new, &|n| CalAdc::off_at(ch, n)), row(base, &|n| CalAdc::off_at(ch, n)));
            if has("--save") {
                calfile::save(calfile::OOS_CAL_DIR, "cal_adc.hex", new.payload(), calfile::now()).map_err(|e| e.to_string())?;
                println!("  saved {}/cal_adc.hex", calfile::OOS_CAL_DIR);
            }
            return Ok(());
        }
        if what == "all" {
            // the whole self-cal of this channel through hal (as System > Self-cal): [--adc] ADC cores first,
            // [--zero] AFE zero, the four vertical passes; --save writes /oos/cal
            let cancel = || false;
            let mut bar = bar;
            let mut afe_bus = afe_bus;
            let mut unit = sc::Unit { bar: &mut bar, afe: &mut afe_bus, c2h: &mut c2h, state: &mut state, cal: &cal, cal_adc: cal_adc.as_ref(), cancel: &cancel, adc_new: None };
            let adc_base = if has("--adc") { cal_adc.as_ref() } else { None };
            let t0 = std::time::Instant::now();
            let mut last = String::new();
            let r = sc::run_all_with(&mut unit, &cal, adc_base, &[ch], has("--zero"), &mut |d, t, s| {
                if s != last {
                    println!("  {:6.1} s  {}/{}  {}", t0.elapsed().as_secs_f64(), d, t, s);
                    last = s.to_string();
                }
            })?;
            println!("  {} configurations, largest change {:.2} %", r.measured, r.worst * 100.0);
            if has("--save") {
                for f in sc::save(&r, &rigol)? {
                    println!("  saved {}", f);
                }
            }
            return Ok(());
        }
        let mut a = state.afe[ch].clone();
        let mut hw = UnitHw { bar, afe: afe_bus, steps: 0 };
        let t0 = std::time::Instant::now();
        match what {
            "zero" => {
                let z = sc::afe_zero(&mut hw, &mut a, cal.zero_trims(ch))?;
                println!("  new  {:?}\n  table {:?}  ({:.1} s)", z, cal.zero_trims(ch), t0.elapsed().as_secs_f64());
                if has("--save") {
                    let (b, from) = calfile::load_first("cal_afe_zero.hex", &[calfile::OOS_CAL_DIR, &rigol]).ok_or("no cal_afe_zero.hex")?;
                    let p = sc::zero_payload(calfile::parse(&b).map_err(|e| format!("{:?}", e))?, ch, &z);
                    calfile::save(calfile::OOS_CAL_DIR, "cal_afe_zero.hex", &p, calfile::now()).map_err(|e| e.to_string())?;
                    println!("  saved {}/cal_afe_zero.hex (base: {})", calfile::OOS_CAL_DIR, from);
                }
            }
            "probe" => {
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
                    let m = oos_hal::dho::adc::measure(&mut hw.bar, &mut |ms| std::thread::sleep(std::time::Duration::from_millis(ms as u64)));
                    println!("  DAC {:5}: {:?}", code, m.map(|c| c.map(|x| x as u32)));
                }
            }
            "entry" | "vertical" => {
                let zt = cal.zero_trims(ch);
                let mut got = Vec::new();
                for g in &tuples {
                    let Some(e) = sc::vertical_entry(&mut hw, &mut a, &cal, *g, &zt) else {
                        println!("  {:?}: no result", g);
                        continue;
                    };
                    let t = entries.iter().find(|x| x.g == *g);
                    println!("  {:?}: scale {:.0} nV/div (table {:.0}, {:+.2} %), zero {} (table {})  t {:.0} s", g, e.scale, t.map_or(f64::NAN, |t| t.scale), t.map_or(f64::NAN, |t| (e.scale / t.scale - 1.0) * 100.0), e.zero, t.map_or(0, |t| t.zero), t0.elapsed().as_secs_f64());
                    got.push(e);
                }
                if what == "vertical" && has("--save") {
                    if got.len() < entries.len() {
                        return Err(format!("only {} of {} configurations measured: not saved", got.len(), entries.len()));
                    }
                    let keys: Vec<i64> = (0..801).map(|i| cal.key(i)).collect();
                    let mut nb = block.clone();
                    sc::rebuild_scales(&mut nb, &keys, &got);
                    let (b, from) = calfile::load_first("cal_vertical.hex", &[calfile::OOS_CAL_DIR, &rigol]).ok_or("no cal_vertical.hex")?;
                    let p = sc::vertical_payload(calfile::parse(&b).map_err(|e| format!("{:?}", e))?, &[(var, ch, nb)]);
                    calfile::save(calfile::OOS_CAL_DIR, "cal_vertical.hex", &p, calfile::now()).map_err(|e| e.to_string())?;
                    println!("  saved {}/cal_vertical.hex block group {} CH{} (base: {})", calfile::OOS_CAL_DIR, var, ch + 1, from);
                }
            }
            _ => return Err("selfcal entry|zero|vertical".into()),
        }
        println!("  {} AFE write groups; re-run the normal set-up (oos-ui does) to leave the cal configuration", hw.steps);
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (tuples, block);
        Err("selfcal --execute needs the unit (Linux)".into())
    }
}

fn afg_la(cmd: &str, a: &[String]) -> Result<(), String> {
    use oos_hal::dho::{afg, la};
    let get = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|v| v.parse::<f64>().map_err(|e| format!("{}: {}", k, e))).transpose();
    let exec = a.iter().any(|x| x == "--execute");
    let dir = std::env::var("OOS_CAL_DIR").unwrap_or_else(|_| "/rigol/data".into());
    match (cmd, a.first().map(|s| s.as_str())) {
        ("afg", Some("sine" | "square" | "ramp" | "pulse" | "noise" | "dc" | "off")) => {
            let ops = if a[0] != "off" {
                let (f, amp, off) = (get("--freq")?.unwrap_or(1000.0), get("--ampl")?.unwrap_or(1.0), get("--offset")?.unwrap_or(0.0));
                let duty = get("--duty")?.unwrap_or(50.0) / 100.0;
                let wave = match a[0].as_str() {
                    "square" => afg::Wave::Square { duty },
                    "ramp" => afg::Wave::Ramp { symmetry: get("--symmetry")?.unwrap_or(50.0) / 100.0 },
                    "pulse" => afg::Wave::Pulse { duty, rise: get("--edge")?.unwrap_or(0.0), fall: get("--edge")?.unwrap_or(0.0) },
                    "noise" => afg::Wave::Noise,
                    "dc" => afg::Wave::Dc,
                    _ => afg::Wave::Sine,
                };
                let cal = afg::Cal::load(&dir);
                let (ac, oc, g) = afg::codes(cal.as_ref(), amp * 1e9, off * 1e9, f * 1e6, wave == afg::Wave::Noise, wave == afg::Wave::Dc);
                println!("afg {:?} {} Hz {} Vpp {} V: relays {:#04x}, 0x3070 {:#06x}, 0x3078 {:#06x}, increment {:#x} ({})", wave, f, amp, off, g.code, 0xffff - ac, 0xffff - oc, afg::increment(f * 1e6), if cal.is_some() { "cal_afg.hex" } else { "no cal file" });
                // --mod am|fm|pm --mod-rate Hz --depth % (AM) --dev Hz (FM) / degrees (PM) --mod-shape sine|square|triangle|up|down
                let shape = match a.iter().position(|x| x == "--mod-shape").and_then(|i| a.get(i + 1)).map(|s| s.as_str()) {
                    Some("square") => afg::ModShape::Square,
                    Some("triangle") => afg::ModShape::Triangle,
                    Some("up") => afg::ModShape::UpRamp,
                    Some("down") => afg::ModShape::DownRamp,
                    Some("noise") => afg::ModShape::Noise,
                    _ => afg::ModShape::Sine,
                };
                let rate = get("--mod-rate")?.unwrap_or(100.0);
                let md = match a.iter().position(|x| x == "--mod").and_then(|i| a.get(i + 1)).map(|s| s.as_str()) {
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
                use oos_hal::dho::bus::{Bar1, Bus, FPGA_VERSION};
                let mut bar = Bar1::open().map_err(|e| format!("/dev/xdma0_bypass: {}", e))?;
                if bar.rd(0x4000) != FPGA_VERSION {
                    return Err("unexpected FPGA version, not touching it".into());
                }
                let mut io = afg::Io::open().map_err(|e| format!("AFG GPIO: {}", e))?;
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
                let v = |i: usize| a.get(i).ok_or("la thr <pod1 V> <pod2 V>")?.parse::<f64>().map_err(|e| e.to_string());
                Some(la::thresholds(&cal, v(1)?, v(2)?))
            } else {
                None
            };
            println!("la: zero codes {:?}{}", cal.zero, w.map_or(String::new(), |w| format!(", write {:#06x} <- {:#010x}", w.0, w.1)));
            #[cfg(target_os = "linux")]
            {
                use oos_hal::dho::bus::{Bar1, Bus};
                let mut bar = Bar1::open().map_err(|e| format!("/dev/xdma0_bypass: {}", e))?;
                if let (Some(w), true) = (w, exec) {
                    bar.wr(w.0, w.1);
                }
                let l = la::lines(bar.rd(la::LINES));
                println!("probe {}, thresholds {:#010x}, lines D15..D0 {}", match la::probe_present() { Ok(true) => "present", Ok(false) => "absent", Err(_) => "unknown" }, bar.rd(la::THRESHOLD), l.iter().rev().map(|&b| if b { '1' } else { '0' }).collect::<String>());
            }
            if w.is_some() && !exec {
                eprintln!("oos-fpgad: dry run; add --execute");
            }
            Ok(())
        }
        _ => Err("afg sine|off, la status|thr".into()),
    }
}
