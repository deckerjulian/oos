// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! System information page (System > System info): the unit (model, serial, capabilities, `oos_hal::model`),
//! software, temperatures with their limits (docs/protocols/board.md), load, the acquisition limits in effect
//! (standard or experimental, docs/models.md) and the current acquisition. Three columns, refreshed once a second
//! while open; a tap or a key closes it like a dialog.

use super::*;
use crate::board::{board, Board};

pub(super) enum Row {
    /// section heading
    Head(String),
    /// label, value, value colour
    Kv(String, String, Rgb),
    /// full-width text, wrapped (overrides, warnings)
    Note(String, Rgb),
}

fn kv(k: &str, v: impl Into<String>) -> Row {
    Row::Kv(k.into(), v.into(), TEXT)
}

fn kvc(k: &str, v: impl Into<String>, c: Rgb) -> Row {
    Row::Kv(k.into(), v.into(), c)
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Uptime "3 h 12 min" from /proc/uptime.
fn uptime() -> Option<String> {
    let s: f64 = read("/proc/uptime")?.split_whitespace().next()?.parse().ok()?;
    let (h, m) = ((s / 3600.0) as u64, ((s % 3600.0) / 60.0) as u64);
    Some(if h > 0 { format!("{} h {} min", h, m) } else { format!("{} min {} s", m, s as u64 % 60) })
}

/// (available, total) bytes of RAM from /proc/meminfo.
fn memory() -> Option<(u64, u64)> {
    let t = read("/proc/meminfo")?;
    let get = |k: &str| t.lines().find(|l| l.starts_with(k)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<u64>().ok()).map(|kb| kb * 1024);
    Some((get("MemAvailable:")?, get("MemTotal:")?))
}

/// (free, total) bytes of the file system holding `path`.
fn disk(path: &str) -> Option<(u64, u64)> {
    let c = std::ffi::CString::new(path).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let fr = s.f_frsize as u64;
    Some((s.f_bavail as u64 * fr, s.f_blocks as u64 * fr))
}

/// Current clock of CPU `n` in Hz (cpufreq).
fn cpu_hz(n: usize) -> Option<f64> {
    read(&format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", n))?.parse::<f64>().ok().map(|khz| khz * 1e3)
}

/// SI value with up to 5 digits and no trailing zeros ("312.5 MSa/s", "31.25 Mpts", "1.25 GSa/s").
fn exact(v: f64, unit: &str) -> String {
    let s = si_digits(v, unit, 5);
    match s.split_once(' ') {
        Some((num, rest)) if num.contains('.') => format!("{} {}", num.trim_end_matches('0').trim_end_matches('.'), rest),
        _ => s,
    }
}

fn bytes(b: u64) -> String {
    si_short(b as f64, "B")
}

/// Colour of a temperature against a warning and a limit.
fn temp_col(t: f32, warn: f32, limit: f32) -> Rgb {
    if t >= limit {
        RED
    } else if t >= warn {
        AMBER
    } else {
        GREEN
    }
}

impl Ui {
    /// Opens the page (System > System info).
    pub(super) fn sysinfo_open(&mut self) {
        self.sysinfo = Some(self.sysinfo_rows());
        self.sysinfo_t = self.now;
        // the dialog marker: every path that closes a dialog (tap, key) closes the page too
        self.dialog = Some(Vec::new());
    }

    /// Per frame: refresh once a second while open; forget the page once its dialog was closed.
    pub(super) fn sysinfo_poll(&mut self) {
        if self.dialog.is_none() {
            self.sysinfo = None;
        } else if self.sysinfo.is_some() && self.now - self.sysinfo_t >= 1.0 {
            self.sysinfo = Some(self.sysinfo_rows());
            self.sysinfo_t = self.now;
        }
    }

    pub(super) fn sysinfo_rows(&self) -> [Vec<Row>; 3] {
        let p = oos_hal::model::platform();
        let (c, l, base) = (&p.caps, &p.limits, oos_hal::model::Limits::standard());
        let readings = self.src.hw_board();

        // ---- column 1: unit and software
        let mut a = vec![Row::Head("Unit".into())];
        a.push(kv("Model", format!("Rigol {}", c.name)));
        a.push(kv("Serial", p.serial.clone().unwrap_or("-".into())));
        a.push(kv("MAC", p.mac.clone().unwrap_or("-".into())));
        a.push(kv("Series", c.series));
        a.push(kv("Rated bandwidth", format!("{} MHz", c.rated_bw_mhz)));
        a.push(kv("Analog channels", c.channels.to_string()));
        a.push(kv("Logic analyser", yes_no(c.la)));
        a.push(kv("Generator", yes_no(c.afg)));
        a.push(kv("EXT trigger", yes_no(c.ext_trig)));
        a.push(match p.source {
            oos_hal::model::Source::Vendor => kvc("Identified", "vendor.bin", GREEN),
            oos_hal::model::Source::Forced => kvc("Identified", "forced (--model / limits.conf)", AMBER),
            oos_hal::model::Source::Assumed => kvc("Identified", "assumed (no vendor.bin)", AMBER),
        });
        a.push(Row::Head("Software".into()));
        a.push(kv("OOS", format!("oos-ui {}", env!("CARGO_PKG_VERSION"))));
        a.push(kv("Kernel", if self.info.is_empty() { "-".to_string() } else { self.info.clone() }));
        a.push(match readings {
            Some((0, _)) => kvc("FPGA", "not reached", AMBER),
            Some((v, _)) if v == board().fpga_version() => kv("FPGA", format!("{:#010x}", v)),
            Some((v, _)) => kvc("FPGA", format!("{:#010x} (unexpected)", v), RED),
            None => kv("FPGA", "-"),
        });
        a.push(kv("Uptime", uptime().unwrap_or("-".into())));
        a.push(if self.net.is_empty() { kvc("Network", "not connected", DIM) } else { kv("Network", self.net.clone()) });
        a.push(kv("Source", format!("{}  {:.0} fps", self.src.name(), self.fps)));

        // ---- column 2: temperatures, load
        let mut b = vec![Row::Head("Temperatures".into())];
        let zones = board().thermal_zones();
        let (soc, fpga) = board().temp_limits();
        for z in &zones {
            let name = match z.name.as_str() {
                "cpu-thermal" | "soc-thermal" => "SoC (CPU)".to_string(),
                "gpu-thermal" => "SoC (GPU)".to_string(),
                n => n.trim_end_matches("-thermal").to_string(),
            };
            b.push(kvc(&name, format!("{:.1} \u{b0}C", z.temp), temp_col(z.temp, 70.0, soc)));
        }
        if zones.is_empty() {
            b.push(kvc("SoC", "-", DIM));
        }
        match readings.and_then(|b| b.1) {
            Some((t, v)) => {
                b.push(kvc("FPGA die", format!("{:.1} \u{b0}C", t), temp_col(t, fpga - 10.0, fpga)));
                b.push(kvc("FPGA VCCINT", format!("{:.3} V", v), if (0.95..=1.05).contains(&v) { TEXT } else { AMBER }));
            }
            None => b.push(kvc("FPGA die", "-", DIM)),
        }
        b.push(kv("Fan", "always on (GPIO hog)"));
        b.push(Row::Head("Temperature limits".into()));
        b.push(kv("SoC throttling", "70 / 85 \u{b0}C (kernel)"));
        b.push(kv("SoC alarm", format!("{:.0} \u{b0}C", soc)));
        b.push(kv("FPGA alarm", format!("{:.0} \u{b0}C", fpga)));
        b.push(Row::Head("Load".into()));
        if let Some(la) = read("/proc/loadavg") {
            b.push(kv("CPU load", la.split_whitespace().take(3).collect::<Vec<_>>().join("  ")));
        }
        if let (Some(lo), Some(hi)) = (cpu_hz(0), cpu_hz(4)) {
            b.push(kv("CPU clock", format!("A53 {}  A72 {}", si_short(lo, "Hz"), si_short(hi, "Hz"))));
        }
        match memory() {
            Some((avail, total)) => b.push(kv("RAM", format!("{} free of {}", bytes(avail), bytes(total)))),
            None => b.push(kvc("RAM", "-", DIM)),
        }
        match disk("/oos") {
            Some((free, total)) => b.push(kvc("/oos", format!("{} free of {}", bytes(free), bytes(total)), if free < total / 20 { AMBER } else { TEXT })),
            None => b.push(kvc("/oos", "-", DIM)),
        }

        // ---- column 3: limits, acquisition, overrides, warnings
        let src = if p.experimental() { "standard + limits.conf" } else { "standard" };
        let mut d = vec![Row::Head(format!("Limits ({})", src))];
        // values that differ from the standard limits in amber (experimental)
        let mark = |same: bool| if same { TEXT } else { AMBER };
        // one row per channel count: max. sample rate and memory depth per channel (LA on: depth only)
        for (i, name) in ["1 channel", "2 channels", "3-4 channels"].iter().enumerate() {
            let same = l.max_srate[i] == base.max_srate[i] && l.max_mem[i] == base.max_mem[i];
            d.push(kvc(name, format!("{}  {}", exact(l.max_srate[i], "Sa/s"), exact(l.max_mem[i] as f64, "pts")), mark(same)));
        }
        if c.la {
            let la = l.max_mem_la.iter().map(|&x| exact(x as f64, "")).collect::<Vec<_>>().join("/");
            d.push(kvc("With LA", format!("{} pts", la.replace(' ', "")), mark(l.max_mem_la == base.max_mem_la)));
        }
        if c.afg {
            let (g, n) = (l.afg_max_hz, base.afg_max_hz);
            d.push(kvc("Generator sine", exact(g.sine, "Hz"), mark(g.sine == n.sine)));
            d.push(kvc("square / pulse", format!("{} / {}", si_short(g.square, "Hz"), si_short(g.pulse, "Hz")), mark(g.square == n.square && g.pulse == n.pulse)));
            d.push(kvc("ramp", si_short(g.ramp, "Hz"), mark(g.ramp == n.ramp)));
        }
        d.push(kv("ADC clock", exact(l.adc_clock_hz, "Hz")));
        if self.clock_next != l.adc_clock_hz {
            d.push(kvc("after power off and on", exact(self.clock_next, "Hz"), AMBER));
        }
        if c.afg {
            d.push(kv("Generator clock", exact(l.afg_clock_hz, "Hz")));
        }
        d.push(Row::Head("Acquisition now".into()));
        d.push(kv("Channels on", format!("{} of {}", self.m.n_on(), n_ch())));
        let (sr, depth) = self.src.hw_rate_depth().map(|(r, n)| (r, n as u64)).unwrap_or_else(|| self.m.srate_depth());
        d.push(kv("Sample rate", si_short(sr, "Sa/s")));
        d.push(kv("Record", si_short(depth as f64, "pts")));
        d.push(kv("Waveforms/s", format!("{:.0}", self.wfm_rate)));
        d.push(kv("Waveforms", fmt_count(self.wfm_total as f64)));
        d.push(kv("Screen", format!("{:.0} fps", self.fps)));
        if p.experimental() {
            d.push(Row::Head("Experimental limits".into()));
            d.extend(p.overrides.iter().map(|o| Row::Note(o.clone(), AMBER)));
        }
        if !p.warnings.is_empty() {
            d.push(Row::Head("Warnings".into()));
            d.extend(p.warnings.iter().map(|w| Row::Note(w.clone(), AMBER)));
        }
        [a, b, d]
    }

    pub(super) fn draw_sysinfo(&self, cv: &mut Canvas) {
        let Some(cols) = &self.sysinfo else { return };
        let f = &self.fonts;
        cv.blend(Rect::new(0, 0, W, H), rgb(0, 0, 0), 140);
        let r = Rect::new(10, 10, W - 20, H - 20);
        cv.round(r, 8, ACCENT, 255);
        cv.round(r.inset(2), 7, DLG_BG, 255);
        let top = Rect::new(r.x + 16, r.y + 8, r.w - 32, 30);
        cv.text_in(&f.lb, top, 0, "System information", ACCENT);
        cv.text_in(&f.s, top, 2, "updated every second \u{b7} tap to close", DIM);
        let gap = 18;
        let cw = (r.w - 32 - 2 * gap) / 3;
        for (i, col) in cols.iter().enumerate() {
            let x = r.x + 16 + i as i32 * (cw + gap);
            let mut y = r.y + 44;
            let kw = cw * 2 / 5;
            cv.set_clip(Rect::new(x, r.y, cw, r.h - 4));
            for row in col {
                match row {
                    Row::Head(t) => {
                        y += 6;
                        cv.text_in(&f.mb, Rect::new(x, y, cw, 20), 0, t, ACCENT);
                        cv.hline(x, x + cw, y + 21, LINE);
                        y += 26;
                    }
                    Row::Kv(k, v, c) => {
                        cv.text_in(&f.s, Rect::new(x, y, kw, 18), 0, k, DIM);
                        cv.text_in(&f.m, Rect::new(x + kw, y, cw - kw, 18), 0, v, *c);
                        y += 19;
                    }
                    Row::Note(t, c) => {
                        // word wrap to the column width
                        let mut line = String::new();
                        for w in t.split(' ') {
                            let cand = if line.is_empty() { w.to_string() } else { format!("{} {}", line, w) };
                            if f.s.width(&cand) > cw && !line.is_empty() {
                                cv.text_in(&f.s, Rect::new(x, y, cw, 17), 0, &line, *c);
                                y += 17;
                                line = w.to_string();
                            } else {
                                line = cand;
                            }
                        }
                        if !line.is_empty() {
                            cv.text_in(&f.s, Rect::new(x, y, cw, 17), 0, &line, *c);
                            y += 17;
                        }
                    }
                }
            }
            cv.no_clip();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_refreshes_and_closes_like_a_dialog() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.activate(Item::SysInfo);
        assert!(ui.sysinfo.is_some() && ui.dialog.is_some());
        let rows = ui.sysinfo.as_ref().unwrap();
        // unit, temperatures, limits; the default platform is the DHO924S with the standard limits
        assert!(rows.iter().all(|c| !c.is_empty()));
        assert!(rows[0].iter().any(|r| matches!(r, Row::Kv(k, v, _) if k == "Model" && v.contains("DHO924S"))));
        assert!(rows[2].iter().any(|r| matches!(r, Row::Kv(k, v, _) if k == "3-4 channels" && v == "312.5 MSa/s  31.25 Mpts")));
        let mut cv = Canvas::new(W, H);
        ui.render(&mut cv);
        // any key closes it; the next frame forgets the rows
        ui.key(key::RUNSTOP);
        ui.step(0.02);
        assert!(ui.dialog.is_none() && ui.sysinfo.is_none());
    }

    #[test]
    fn exact_values() {
        assert_eq!(exact(312.5e6, "Sa/s"), "312.5 MSa/s");
        assert_eq!(exact(1.25e9, "Hz"), "1.25 GHz");
        assert_eq!(exact(156.25e6, "Hz"), "156.25 MHz");
        assert_eq!(exact(125e6, "pts"), "125 Mpts");
    }
}
