// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! PCIe host and xdma up and down, with the xdma.ko parameters: the FPGA of the DHO800/DHO900 and that of the MHO900
//! are both XDMA endpoints behind the RK3399's PCIe host (docs/protocols/fpga-boot.md, docs/mho.md).

/// Parameters of xdma.ko without `xdma.params`: DMA completion by polling the write-back word, what OOS was verified
/// with. The stock system loads it with interrupts.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const XDMA_DEFAULT: &str = "poll_mode=1";

/// The xdma.ko parameters of `xdma.params` in the system settings (`oos-conf`; None or empty = `XDMA_DEFAULT`). Only
/// `poll_mode=0|1` and `interrupt_mode=0..3` (0 auto, 1 MSI, 2 legacy INTx, 3 MSI-X), each at most once: the line goes
/// to `insmod` through a shell.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn xdma_params(v: Option<&str>) -> Result<String, String> {
    let v = v.map(str::trim).unwrap_or("");
    if v.is_empty() {
        return Ok(XDMA_DEFAULT.into());
    }
    let mut seen = Vec::new();
    for w in v.split_whitespace() {
        let ok = matches!(
            w,
            "poll_mode=0"
                | "poll_mode=1"
                | "interrupt_mode=0"
                | "interrupt_mode=1"
                | "interrupt_mode=2"
                | "interrupt_mode=3"
        );
        let key = w.split('=').next().unwrap_or(w);
        if !ok || seen.contains(&key) {
            return Err(format!("xdma.params: {:?} is not allowed (poll_mode=0|1, interrupt_mode=0..3, each once)", w));
        }
        seen.push(key);
    }
    Ok(v.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// PCIe host + xdma bring-up with retries (the loop of oos-fpga-up, without the FPGA boot), then BAR1. The device and
/// the xdma nodes are polled every 20 ms instead of the fixed 2 s / 1 s sleeps of oos-fpga-up (`insmod` returns once
/// the driver probed, so a link that is up shows at once).
#[cfg(target_os = "linux")]
pub(crate) fn pcie_up() -> std::io::Result<oos_hal::dho::bus::Bar1> {
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
        let conf = std::process::Command::new("/bin/oos-conf")
            .arg("xdma.params")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
        let params = xdma_params(conf.as_deref()).unwrap_or_else(|e| {
            eprintln!("oos-fpgad: {}; loading xdma with {}", e, XDMA_DEFAULT);
            XDMA_DEFAULT.into()
        });
        eprintln!("oos-fpgad: insmod xdma.ko {}", params);
        if !sh(&format!("insmod {}/updates/xdma.ko {}", m, params)) {
            return Err(std::io::Error::other("insmod xdma failed"));
        }
        let nodes = || ["/dev/xdma0_bypass", "/dev/xdma0_c2h_0"].iter().all(|n| std::path::Path::new(n).exists());
        if !poll(&nodes, 3000) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "xdma loaded, but /dev/xdma0_* did not appear",
            ));
        }
    }
    oos_hal::dho::bus::Bar1::open()
}

/// Unloads xdma and the PCIe host driver, the step before the BOOT command is sent again (`io::power_on`): the FPGA
/// must not reconfigure behind a bound endpoint. An error, with what `rmmod` said, if either module is still loaded
/// afterwards.
#[cfg(target_os = "linux")]
pub(crate) fn pcie_down() -> std::io::Result<()> {
    let run = |c: &str| std::process::Command::new("sh").arg("-c").arg(c).output().ok();
    let said: Vec<String> = ["rmmod xdma 2>&1", "rmmod pcierockchiphost 2>&1"]
        .iter()
        .filter_map(|c| run(c))
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if run("lsmod | grep -qE '^(xdma|pcierockchiphost) '").map(|o| o.status.success()).unwrap_or(false) {
        return Err(std::io::Error::other(format!(
            "xdma or the PCIe host driver is still loaded after rmmod ({})",
            said.join("; ")
        )));
    }
    Ok(())
}
