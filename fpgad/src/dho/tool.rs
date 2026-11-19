// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `oos-fpgad status | rd | wr | cold-boot | fpga-boot | pll | gpio | adc`: register, board and ADC access
//! (`tool`), with the status interpretation, the `adc slots` lines and the dry run of `pll`. A new command of
//! this group gets its arm in `tool` and its name in `TOOL_CMDS`.

#[cfg(target_os = "linux")]
use super::bringup::{bar_for_temps, check_cal, check_temps, pcie_down, pcie_up, BOOTED_FILE};
#[cfg(target_os = "linux")]
use super::{bar, opt};
use super::{cal_dir, flag, num};
#[cfg(target_os = "linux")]
use oos_hal::bootstatus;

/// The commands `tool` runs (register, board and ADC access). A command missing here falls through to the usage text
/// without a word (unit round 9: `adc` did).
pub(super) const TOOL_CMDS: [&str; 8] = ["status", "rd", "wr", "cold-boot", "fpga-boot", "pll", "gpio", "adc"];

/// An oos-ui process is running (/proc/<pid>/comm); also true when /proc cannot be read.
#[cfg(target_os = "linux")]
fn ui_running() -> bool {
    oos_hal::procs::pids_where(|p| oos_hal::procs::comm_is(p, "oos-ui")).is_none_or(|p| !p.is_empty())
}

/// The lines of `adc slots`: per slot the mean over all runs (16-bit measure scale), the lowest min and highest max
/// (12-bit), and the mean's distance from the average of all slots in ADC steps (16 codes).
#[cfg(any(target_os = "linux", test))]
pub(super) fn slot_lines(runs: &[[oos_hal::dho::levels::Slot; 8]]) -> Vec<String> {
    let n = runs.len().max(1) as f64;
    let mean: [f64; 8] = std::array::from_fn(|k| runs.iter().map(|r| r[k].mean).sum::<f64>() / n);
    let all = mean.iter().sum::<f64>() / 8.0;
    (0..8)
        .map(|k| {
            let lo = runs.iter().map(|r| r[k].min).min().unwrap_or(0);
            let hi = runs.iter().map(|r| r[k].max).max().unwrap_or(0);
            format!("slot {}: mean {:.1}  min {}  max {}  {:+.2} steps", k, mean[k], lo, hi, (mean[k] - all) / 16.0)
        })
        .collect()
}

/// Interpretation of the read-only status registers.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(super) fn status_lines(rd: &mut dyn FnMut(u32) -> u32) -> Vec<String> {
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
        format!(
            "FPGA version 0x4000 = {:#010x} {}",
            v,
            if v == oos_hal::dho::bus::FPGA_VERSION { "(expected)" } else { "(UNEXPECTED)" }
        ),
        format!(
            "status 0x4034 = {:#010x}: FSM {}, {}",
            st,
            if st & 0x2000 != 0 { "stopped" } else { "running" },
            if st & 0x10 != 0 { "waiting for trigger" } else { "not waiting" }
        ),
        format!("DDR 0x1008 = {:#010x}: init {}", ddr, if ddr & DDR_INIT_PASS != 0 { "pass" } else { "NOT passed" }),
        format!(
            "ADC clock count 0x1210 = {} -> PLL {}",
            clk,
            if (cmin..=cmax).contains(&clk) { "locked" } else { "NOT locked / not running" }
        ),
        format!("XADC 0x1438 = {:#010x}: FPGA die {:.1} C, VCCINT {:.3} V", x, t, vcc),
        format!(
            "play window 0x4014..0x4010 = {}..{} ({} frames), 0x4080 = {:#010x} (done bit30 {})",
            first,
            last,
            last.wrapping_sub(first).wrapping_add(1),
            play,
            (play >> 30) & 1
        ),
    ]
}

/// `pll --freq <Hz> --dry`: the table `oos_hal::dho::lmx2582::table_for` makes and its decode, nothing written. Without
/// --dry a --freq other than 1.25 GHz is refused (no other table has run on a unit); None = go on with the stock table.
pub(super) fn pll_dry(a: &[String]) -> Result<Option<String>, String> {
    use oos_hal::dho::lmx2582;
    let freq = a
        .iter()
        .position(|x| x == "--freq")
        .map(|i| a.get(i + 1).ok_or("--freq <Hz>")?.parse::<f64>().map_err(|e| format!("--freq: {}", e)))
        .transpose()?;
    if !flag(a, "--dry") {
        return match freq {
            Some(f) if f != 1.25e9 => Err(format!(
                "pll --freq {}: only 1.25 GHz is tested on the unit; other clocks only with --dry for now",
                f
            )),
            _ => Ok(None),
        };
    }
    let f = freq.unwrap_or(1.25e9);
    let t = lmx2582::table_for(f)?;
    let d = lmx2582::Fields::decode(&t)?;
    let mut s = format!(
        "LMX2582 table for {} Hz (dry run, nothing written; R0 last starts the VCO calibration)\n{}\n",
        f,
        lmx2582::format_table(&t)
    );
    for l in d.lines() {
        s += &format!("  {}\n", l);
    }
    let changed: Vec<String> = t
        .iter()
        .zip(oos_hal::dho::boot::regs::PLL_TABLE.iter())
        .filter(|(x, y)| x != y)
        .map(|(x, _)| format!("R{}", x[0]))
        .collect();
    s += &format!("  changed vs. 1.25 GHz: {}\n", if changed.is_empty() { "none".into() } else { changed.join(" ") });
    for n in d.notes() {
        s += &format!("  note: {}\n", n);
    }
    Ok(Some(s))
}

pub(super) fn tool(cmd: &str, a: &[String]) -> Result<(), String> {
    let has = |f: &str| flag(a, f);
    let dir = cal_dir();
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
    if (cmd == "cold-boot" || cmd == "fpga-boot")
        && std::path::Path::new("/sys/module/xdma").exists()
        && !has("--force")
    {
        return Err("XDMA is already loaded (an earlier cold boot or hand-over in this boot): power cycle the scope instead of retrying".into());
    }
    #[cfg(target_os = "linux")]
    {
        match cmd {
            "status" => {
                tool_status()?;
            }
            "gpio" => {
                tool_gpio(a)?;
            }
            "pll" => {
                tool_pll(a)?;
            }
            "rd" => {
                tool_rd(a)?;
            }
            "adc" => {
                tool_adc(a)?;
            }
            "wr" => {
                tool_wr(a)?;
            }
            "fpga-boot" => {
                tool_fpga_boot(a)?;
            }
            _ => {
                tool_cold_boot(a, &dir)?;
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

/// `status`: the read-only status registers, interpreted.
#[cfg(target_os = "linux")]
fn tool_status() -> Result<(), String> {
    use oos_hal::dho::bus::Bus;
    let mut bar = bar()?;
    for l in status_lines(&mut |o| bar.rd(o)) {
        println!("{}", l);
    }
    Ok(())
}

/// `gpio <line> <0|1>`: drives a GPIO line and keeps it for --hold seconds.
#[cfg(target_os = "linux")]
fn tool_gpio(a: &[String]) -> Result<(), String> {
    let name = a.first().ok_or("gpio <line> <0|1> [--hold s]")?;
    let v = a.get(1).ok_or("gpio <line> <0|1> [--hold s]")? == "1";
    let hold = opt(a, "--hold").map(|x| x.parse::<u64>().map_err(|e| e.to_string())).transpose()?.unwrap_or(0);
    // generator relays and supply only on a unit with a generator (docs/models.md 4.2)
    oos_hal::dho::afg::line_allowed(&oos_hal::model::platform().caps, name, flag(a, "--force"))?;
    let _l = oos_hal::dho::gpio::Line::output(name, v).map_err(|e| format!("{}: {}", name, e))?;
    println!("{} = {} (held {} s)", name, v as u8, hold);
    std::thread::sleep(std::time::Duration::from_secs(hold));
    Ok(())
}

/// `pll`: the LMX2582 table (or one R0 word) over GPIO, then the ADC clock counter.
#[cfg(target_os = "linux")]
fn tool_pll(a: &[String]) -> Result<(), String> {
    use oos_hal::dho::bus::Bus;
    let has = |f: &str| flag(a, f);
    use oos_hal::dho::boot::{regs, PllPins};
    let mut pll = oos_hal::dho::boot::io::GpioPll::open().map_err(|e| format!("PLL lines: {}", e))?;
    let send = |pll: &mut oos_hal::dho::boot::io::GpioPll, w: [u8; 3]| {
        regs::send_pll_word(pll, w);
        std::thread::sleep(std::time::Duration::from_micros(regs::PLL_WORD_GAP_US));
    };
    // idle state of the stock module after insmod: LE high (the first word then starts with a falling edge)
    pll.set(regs::PllPin::Le, true);
    std::thread::sleep(std::time::Duration::from_millis(10));
    if let Some(v) = opt(a, "--r0") {
        // diagnostic: one R0 word only (e.g. 0x221d = POWERDOWN set: the counter must drop to 0)
        let v = num(v)?;
        send(&mut pll, [0, (v >> 8) as u8, v as u8]);
        println!("LMX2582: R0 <- {:#06x}", v);
        let mut bar = bar()?;
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
    let mut bar = bar()?;
    for _ in 0..3 {
        println!("0x1210 = {}", bar.rd(0x1210));
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    Ok(())
}

/// `rd <offset> [count]`: reads BAR1 registers.
#[cfg(target_os = "linux")]
fn tool_rd(a: &[String]) -> Result<(), String> {
    use oos_hal::dho::bus::Bus;
    let off = num(a.first().ok_or("rd <offset> [count]")?)?;
    let n = a.get(1).map(|x| num(x)).transpose()?.unwrap_or(1);
    let mut bar = bar()?;
    for k in 0..n {
        let o = off + 4 * k;
        println!("{:#06x} = {:#010x}", o, bar.rd(o));
    }
    Ok(())
}

/// `adc slots | adc offset`: the measure unit per slot, or one core offset over 0x1034.
#[cfg(target_os = "linux")]
fn tool_adc(a: &[String]) -> Result<(), String> {
    use oos_hal::dho::bus::Bus;
    let has = |f: &str| flag(a, f);
    if ui_running() {
        return Err("oos-ui is running: stop it first (it owns the ADC and the measure unit)".into());
    }
    match a.first().map(String::as_str) {
        Some("slots") => {
            let runs =
                opt(a, "--runs").map(|x| x.parse::<u32>().map_err(|e| e.to_string())).transpose()?.unwrap_or(4).max(1);
            let mut bar = bar()?;
            let mut got = Vec::new();
            for _ in 0..runs {
                got.push(
                    oos_hal::dho::levels::run(&mut bar, &mut |us| {
                        std::thread::sleep(std::time::Duration::from_micros(us as u64))
                    })
                    .ok_or("measure unit did not finish")?,
                );
            }
            for l in slot_lines(&got) {
                println!("{}", l);
            }
        }
        Some("offset") => {
            let usage = "adc offset <core 0..7> <steps -1024..1023> [--execute]";
            let core = a.get(1).ok_or(usage)?.parse::<usize>().map_err(|e| e.to_string())?;
            let o = a.get(2).ok_or(usage)?.parse::<i32>().map_err(|e| e.to_string())?;
            let w = oos_hal::dho::adc::Adc::core_offset_word(core, o)?;
            if !has("--execute") {
                println!(
                    "dry run: would write 0x1034 <- {:#010x} (core {} offset {} steps); add --execute",
                    w, core, o
                );
                return Ok(());
            }
            let mut bar = bar()?;
            bar.wr(0x1034, w);
            std::thread::sleep(std::time::Duration::from_micros(100));
            println!("0x1034 <- {:#010x} (core {} offset {} steps)", w, core, o);
        }
        _ => return Err("adc slots [--runs n] | adc offset <core> <steps> [--execute]".into()),
    }
    Ok(())
}

/// `wr <offset> <value> --execute`: writes one BAR1 register (0x1008 refused).
#[cfg(target_os = "linux")]
fn tool_wr(a: &[String]) -> Result<(), String> {
    use oos_hal::dho::bus::Bus;
    let has = |f: &str| flag(a, f);
    let (off, v) = (
        num(a.first().ok_or("wr <offset> <value> --execute")?)?,
        num(a.get(1).ok_or("wr <offset> <value> --execute")?)?,
    );
    if off == 0x1008 {
        return Err("0x1008 refused (DDR status / BRAM fallback bit, cold-boot.md never-do list)".into());
    }
    if !has("--execute") {
        println!("dry run: would write {:#06x} <- {:#010x}; add --execute", off, v);
        return Ok(());
    }
    let mut bar = bar()?;
    bar.wr(off, v);
    println!("{:#06x} <- {:#010x}", off, v);
    Ok(())
}

/// `fpga-boot`: only the BOOT command; its uptime goes to the booted file.
#[cfg(target_os = "linux")]
fn tool_fpga_boot(a: &[String]) -> Result<(), String> {
    let slot = opt(a, "--slot").map(|x| num(x)).transpose()?.unwrap_or(0x40_0000);
    let progress = opt(a, "--progress").cloned();
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
    Ok(())
}

/// `cold-boot --execute`: the cold boot (with --power-on first the BOOT command and PCIe), its log and checks.
#[cfg(target_os = "linux")]
fn tool_cold_boot(a: &[String], dir: &str) -> Result<(), String> {
    let has = |f: &str| flag(a, f);
    // cold-boot --execute
    let progress = opt(a, "--progress").cloned();
    let say = |state: &str, stage: &str, detail: &str| {
        if let Some(p) = &progress {
            bootstatus::append(p, state, stage, detail);
        }
    };
    let cal = oos_hal::dho::boot::CalFiles::load(dir);
    check_cal(&cal, dir, &say);
    let boot_opt = oos_hal::dho::boot::Options { ignore_clock: has("--ignore-clock"), ..Default::default() };
    // the start-up screen's stage of the running step: ok when the next one starts
    let mut cur: Option<&'static str> = None;
    let report = if has("--power-on") {
        tool_power_on(a, &cal, &boot_opt, &say, &mut cur)?
    } else {
        let mut bar = bar()?;
        let mut pll = oos_hal::dho::boot::io::GpioPll::open().map_err(|e| format!("PLL lines: {}", e))?;
        let mut afe = oos_hal::dho::afe::GpioAfe::open().map_err(|e| format!("AFE lines: {}", e))?;
        let mut clk = oos_hal::dho::boot::SysClock::default();
        oos_hal::dho::boot::cold_boot(
            oos_hal::dho::boot::Hw {
                bus: &mut bar,
                clock: &mut clk,
                pll: Some(&mut pll),
                afe: Some(&mut afe),
                early: None,
            },
            &cal,
            &boot_opt,
        )
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
    Ok(())
}

/// `cold-boot --power-on`: the BOOT command (unless --booted), PCIe and the cold boot; the stages go to `say`.
#[cfg(target_os = "linux")]
fn tool_power_on(
    a: &[String],
    cal: &oos_hal::dho::boot::CalFiles,
    boot_opt: &oos_hal::dho::boot::Options,
    say: &dyn Fn(&str, &str, &str),
    cur: &mut Option<&'static str>,
) -> Result<oos_hal::dho::boot::Report, String> {
    let has = |f: &str| flag(a, f);
    let slot = opt(a, "--slot").map(|x| num(x)).transpose()?.unwrap_or(0x40_0000);
    // --booted without the file (the early BOOT command failed): send it here
    let booted = has("--booted")
        .then(|| std::fs::read_to_string(BOOTED_FILE).ok().and_then(|t| t.trim().parse::<f64>().ok()))
        .flatten();
    if booted.is_none() {
        say("run", "FPGA image", "loading the image from flash");
    }
    // after a repeated BOOT command the stage keeps the warning's text, not "PCIe link"
    let repeats = std::cell::Cell::new(0u32);
    let up = || {
        if repeats.get() == 0 {
            say("run", "FPGA image", "PCIe link");
        }
        pcie_up()
    };
    let noted = |n: u32| {
        repeats.set(n);
        say(
            "warn",
            "FPGA image",
            &format!(
                "the BOOT command got no answer, sent again ({} of {})",
                n,
                oos_hal::dho::boot::io::FPGA_BOOT_RETRIES
            ),
        );
    };
    let mut step = |n: u32| {
        let g = oos_hal::dho::boot::stage(n);
        if *cur != Some(g) {
            if let Some(c) = *cur {
                say("ok", c, "");
            }
            say("run", g, "");
            *cur = Some(g);
        }
    };
    let r = oos_hal::dho::boot::io::power_on(slot, booted, up, pcie_down, noted, cal, boot_opt, &mut step);
    if let Err(e) = &r {
        // before the first step: the BOOT command / PCIe, or the PLL / AFE lines (e.g. EBUSY)
        let stage = if e.starts_with("PLL lines") || e.starts_with("AFE lines") { "GPIO lines" } else { "FPGA image" };
        say("fail", cur.unwrap_or(stage), e);
    }
    r
}
