// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! oos-ui: the OOS oscilloscope screen.
//!
//!   oos-ui                         on the scope: DRM/KMS display, touch, front panel, control FIFO
//!   oos-ui                         on Windows: the screen in a desktop window, demo signal (src/desktop.rs)
//!   oos-ui --offline               start in offline mode: analog off, no demo on CH1-CH4
//!   oos-ui --script FILE           anywhere: run a command script off-screen and write screenshots (tests)
//!   oos-ui --replay FRAME.bin --pre "<:WAV:PRE? string>"   show a captured raw frame (CH1) instead of the demo source
//!   --store-dir DIR                storage directory for setups, CSV and PNG (default /tmp/oos; scripts: the script's dir)
//!   --model NAME                   act as this model (DHO802 .. DHO924S), instead of /rigol/data/vendor.bin
//!   --limits FILE                  experimental limits (default /oos/limits.conf, docs/models.md)

// SCPI, web remote, display and the live source only run on the scope (Linux); the host build is the test runner.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

mod cmd;
mod decode;
mod fastdraw;
mod font;
mod gfx;
mod http;
mod board;
mod math;
mod model;
mod scpi;
mod store;
mod trig;
mod ui;
mod vxi11;
mod hislip;
mod announce;
mod json;
mod wave;
mod zone;
#[cfg(target_os = "linux")]
mod drm;
#[cfg(target_os = "linux")]
mod input;
#[cfg(windows)]
mod desktop;

use gfx::Canvas;
use ui::Ui;
use wave::Source;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn source(args: &[String]) -> Source {
    #[cfg(target_os = "linux")]
    if args.iter().any(|a| a == "--fpga-live") {
        use board::Board;
        let pre = arg(args, "--pre").unwrap_or_else(|| "1,2,4000,1,8e-10,0,0,1.3333e-4,-11250,32768".into());
        let p: Vec<f64> = pre.trim().split(',').map(|s| s.trim().parse().unwrap_or(0.0)).collect();
        if let Some(shared) = board::board().live(args) {
            return Source::Live { shared, seen: 0, words: Vec::new(), xinc: p[4], yinc: p[7], yorg: p[8], yref: p[9] };
        }
    }
    #[cfg(target_os = "linux")]
    if args.iter().any(|a| a == "--hw") {
        use board::Board;
        let mask = arg(args, "--app-mask").and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()).unwrap_or(1);
        let cal_dir = arg(args, "--cal").unwrap_or_else(|| "/rigol/data".into());
        eprintln!("oos-ui: board {}", board::board().name());
        let shared = board::board().spawn(board::Spawn { mask, program_afe: args.iter().any(|a| a == "--afe"), cal_dir, start_up: arg(args, "--start-up") });
        return Source::Hw { shared, seen: 0 };
    }
    if let Some(path) = arg(args, "--replay") {
        let raw = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {}", path, e));
        let words: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        // :WAV:PRE? = format,type,points,count,xinc,xorigin,xref,yinc,yorigin,yref
        let pre = arg(args, "--pre").unwrap_or_else(|| "1,2,4000,1,8e-10,0,0,1.3333e-4,-11250,32768".into());
        let p: Vec<f64> = pre.trim().split(',').map(|s| s.trim().parse().unwrap_or(0.0)).collect();
        return Source::Replay { words, xinc: p[4], yinc: p[7], yorg: p[8], yref: p[9] };
    }
    Source::Demo { t: 0.0 }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--fpga-dry") {
        // print the register writes of the live source (no hardware access)
        use board::Board;
        board::board().dry();
        return;
    }
    // the unit: model from /rigol/data/vendor.bin, standard limits plus /oos/limits.conf (docs/models.md);
    // before the source starts, the acquisition thread reads it
    let limits = arg(&args, "--limits").unwrap_or_else(|| oos_hal::model::LIMITS_CONF.into());
    let p = oos_hal::model::Platform::detect(
        &arg(&args, "--cal").unwrap_or_else(|| "/rigol/data".into()),
        &limits,
        arg(&args, "--model").as_deref(),
    );
    eprintln!("oos-ui: {} ({})", p.ident(), p.caps_line());
    for o in &p.overrides {
        eprintln!("oos-ui: EXPERIMENTAL limit {}", o);
    }
    for w in &p.warnings {
        eprintln!("oos-ui: {}", w);
    }
    let experimental = p.experimental();
    let foreign = p.foreign.clone();
    oos_hal::model::init(p);
    // another board (docs/porting.md): no hardware source, the demo signal instead
    let args: Vec<String> = match &foreign {
        Some(_) => args.iter().filter(|a| !matches!(a.as_str(), "--hw" | "--fpga-live" | "--afe")).cloned().collect(),
        None => args,
    };
    let mut ui = Ui::new(source(&args));
    if args.iter().any(|a| a == "--offline") {
        ui.set_offline(true);
    }
    if let Some(m) = &foreign {
        ui.toast(&format!("This is a {}: OOS drives DHO800/DHO900 hardware only. Demo signal shown (docs/porting.md)", m));
    }
    ui.limits_path = limits;
    if experimental {
        ui.toast("Experimental limits active (limits.conf), see System > System info");
    }
    if let Some(script) = arg(&args, "--script") {
        // host test runs store next to the script (build.sh: target/test-shots)
        if let Some(d) = std::path::Path::new(&script).parent().filter(|d| !d.as_os_str().is_empty()) {
            ui.store_dir = d.to_path_buf();
        }
        if let Some(d) = arg(&args, "--store-dir") {
            ui.store_dir = d.into();
        }
        run_script(&mut ui, &script);
        return;
    }
    if let Some(d) = arg(&args, "--store-dir") {
        ui.store_dir = d.into();
    } else if cfg!(windows) {
        // no /tmp on Windows: setups, CSV and screenshots go to Documents\OOS
        if let Some(home) = std::env::var_os("USERPROFILE") {
            ui.store_dir = std::path::Path::new(&home).join("Documents").join("OOS");
        }
    }
    if args.iter().any(|a| a == "--serve") {
        return serve(ui, &args);
    }
    #[cfg(target_os = "linux")]
    {
        device_main(ui, &args);
    }
    #[cfg(windows)]
    {
        desktop::run(ui, &args);
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = ui;
        eprintln!("oos-ui: no display backend on this OS; use --script FILE");
    }
}

/// No display and no hardware: the demo source behind SCPI and the web server on a computer, which also accepts
/// HiSLIP. VXI-11 stays on the scope, where port 111 is free. `OOS_DEMO_LOOP=<mask>` puts the generator on those
/// channels, as a cable from the output would.
fn serve(mut ui: Ui, args: &[String]) {
    let (mut web, mut scpi, mut vxi, mut hislip, mut ann) = lan(false, args);
    let mut cv = Canvas::new(ui::W, ui::H);
    while !ui.quit {
        ui.step(0.04);
        ui.render(&mut cv);
        scpi.poll(&mut ui, &cv);
        vxi.poll(&mut |c| scpi.exec_line(&mut ui, &cv, c));
        hislip.poll(&mut |c| scpi.exec_line(&mut ui, &cv, c));
        for c in web.poll(&mut ui, &cv, &mut scpi) {
            cmd::run(&mut ui, &c);
        }
        ann.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// The scope, where port 111 is free for VXI-11: the acquisition device is present, or the boot identity was read.
/// A computer has neither, and accepts HiSLIP instead.
fn on_scope() -> bool {
    std::path::Path::new("/dev/xdma0_bypass").exists() || matches!(oos_hal::model::platform().source, oos_hal::model::Source::Vendor)
}

/// SCPI, the web server, and whichever of VXI-11 and HiSLIP this machine accepts, plus the announcement.
fn lan(scope: bool, args: &[String]) -> (http::Http, scpi::Scpi, vxi11::Server, hislip::Server, announce::Announcer) {
    let port = |name: &str, d: u16| arg(args, name).and_then(|p| p.parse().ok()).unwrap_or(d);
    let mut web = http::Http::open(port("--web-port", 8080));
    let scpi = scpi::Scpi::open(port("--scpi-port", 5555));
    let vxi = if scope { vxi11::Server::open() } else { vxi11::Server::closed() };
    let hislip = if scope { hislip::Server::closed() } else { hislip::Server::open(port("--hislip-port", hislip::PORT)) };
    let services = announce::listening(web.port(), scpi.port(), vxi.service_port(), hislip.port());
    web.offer(http::Offer { scpi_port: scpi.port(), vxi11: vxi.service_port().is_some(), hislip_port: hislip.port() });
    let ann = announce::Announcer::open(&services);
    (web, scpi, vxi, hislip, ann)
}

fn run_script(ui: &mut Ui, path: &str) {
    let text = std::fs::read_to_string(path).expect("script");
    let mut cv = Canvas::new(ui::W, ui::H);
    ui.info = "host test run".into();
    let mut shot_n = 0;
    let mut frame = |ui: &mut Ui, cv: &mut Canvas| {
        ui.step(1.0 / 30.0);
        ui.render(cv);
        if let Some(p) = ui.shot_request.take() {
            save_shot(ui, cv, p, &mut shot_n);
        }
    };
    frame(ui, &mut cv);
    for line in text.lines() {
        match cmd::run(ui, line) {
            cmd::Out::Frames(n) => {
                for _ in 0..n {
                    frame(ui, &mut cv);
                }
            }
            cmd::Out::Shot(p) => {
                frame(ui, &mut cv);
                cmd::write_ppm(&cv, &p).expect("write shot");
                println!("shot {}", p);
            }
            cmd::Out::None => {}
        }
        if ui.quit {
            break;
        }
    }
}

/// Writes a requested screenshot of the frame just rendered: "" = /tmp/oos-shot-N.ppm, *.png = PNG (storage, the
/// toast shows the file name), else PPM.
fn save_shot(ui: &mut Ui, cv: &Canvas, p: String, n: &mut u32) {
    let path = if p.is_empty() {
        *n += 1;
        format!("/tmp/oos-shot-{}.ppm", n)
    } else {
        p
    };
    let png = path.ends_with(".png");
    let r = if png { store::write(std::path::Path::new(&path), &store::png_of(cv)) } else { cmd::write_ppm(cv, &path) };
    match r {
        Ok(()) if png => {
            ui.flush_stick();
            ui.toast(&format!("Saved {}{}", store::file_name(std::path::Path::new(&path)), ui.saved_where()));
        }
        Ok(()) => ui.toast(&format!("Saved {}", path)),
        Err(e) => ui.toast(&format!("Screenshot failed: {}", e)),
    }
}

/// Settings of the running oos-ui (key = value text, store.rs), kept in RAM for restarts within one boot.
#[cfg(target_os = "linux")]
const SETUP_FILE: &str = "/tmp/oos-ui.setup";
/// Power-on pictures, first found wins: a picture copied by rcS from removable media, then the image's own.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const SPLASH: [&str; 2] = ["/tmp/oos-splash.ppm", "/etc/oos/splash.ppm"];

/// Power off / restart through the front-panel MCU, as the stock system does (board.md 4/8.3): file systems synced
/// first, LEDs off, then `FA 05 01` twice (power off) or `FA 05 03` (restart). Whether the MCU waits for Linux to halt
/// is not known; OOS keeps nothing on the card open for writing except during an update job (`Ui::update_busy`, which
/// refuses the power-off), so a hard cut is harmless.
#[cfg(target_os = "linux")]
fn power(inp: &mut input::Inputs, a: ui::PowerAction) {
    use board::Board;
    eprintln!("oos-ui: power {:?}", a);
    inp.set_leds(0);
    unsafe { libc::sync() };
    match a {
        ui::PowerAction::Off => {
            inp.send(&board::board().panel_cmd(board::PanelCmd::PowerOff));
            inp.send(&board::board().panel_cmd(board::PanelCmd::PowerOff));
        }
        ui::PowerAction::Restart => inp.send(&board::board().panel_cmd(board::PanelCmd::Restart)),
    }
}

#[cfg(target_os = "linux")]
fn device_main(mut ui: Ui, args: &[String]) {
    use board::Board;
    use std::time::Instant;
    let mut disp = match drm::Display::open("/dev/dri/card0") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("oos-ui: display: {}", e);
            std::process::exit(1);
        }
    };
    eprintln!("oos-ui: display {}x{}", disp.width, disp.height);
    let mut inp = input::Inputs::open(args);
    // "app UI loaded", sent once at the end of the start-up (board.md section 4)
    inp.send(&board::board().panel_cmd(board::PanelCmd::UiLoaded));
    // start-up screen (ui/startup.rs) while the acquisition hardware starts; the own picture is its background
    let start_up = arg(args, "--start-up");
    if start_up.is_some() {
        let mut cv = Canvas::new(ui::W, ui::H);
        let splash = SPLASH.iter().find_map(|p| std::fs::read(p).ok()).and_then(|b| store::ppm_into(&b, &mut cv)).map(|_| cv.px);
        ui.start_up_begin(splash);
    }
    // own picture at power-on: without the start-up screen, the first start of a boot shows a binary
    // PPM for 2 s
    if start_up.is_none() && !std::path::Path::new("/tmp/oos-splash.done").exists() {
        let _ = std::fs::write("/tmp/oos-splash.done", b"");
        let mut cv = Canvas::new(ui::W, ui::H);
        if let Some(p) = SPLASH.iter().find_map(|p| std::fs::read(p).ok()).and_then(|b| store::ppm_into(&b, &mut cv)) {
            disp.present(&cv);
            eprintln!("oos-ui: power-on picture {}x{}", p.0, p.1);
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }
    // settings survive a restart of oos-ui within one boot (rcS restarts it on the hardware after the cold boot)
    let setup_path = std::path::Path::new(SETUP_FILE);
    if let Ok(t) = std::fs::read_to_string(setup_path) {
        match store::parse_setup(&t) {
            Ok(m) => {
                ui.m = m;
                ui.sync_full();
                eprintln!("oos-ui: settings restored from {}", SETUP_FILE);
            }
            Err(e) => eprintln!("oos-ui: {}: {}", SETUP_FILE, e),
        }
    }
    let mut setup_last = store::setup_text(&ui.m);
    let mut setup_t = Instant::now();
    ui.info = std::fs::read_to_string("/proc/sys/kernel/osrelease").map(|s| format!("Linux {}", s.trim())).unwrap_or_default();
    let mut cv = Canvas::new(ui::W, ui::H);
    let mut last = Instant::now();
    let mut fps_t = Instant::now();
    let mut fps_n = 0;
    let mut shot_n = 0;
    let mut led_mask = u32::MAX;
    let mut led_t = Instant::now();
    let mut net_t: Option<Instant> = None;
    input::install_signal_handlers();
    let (mut web, mut scpi, mut vxi, mut hislip, mut ann) = lan(on_scope(), args);
    // HDMI mirror: a monitor is looked for every 3 s and gets the panel picture at 15 frames/s (`--no-hdmi`: off)
    let hdmi_on = !args.iter().any(|a| a == "--no-hdmi");
    let mut hdmi: Option<drm::Display> = None;
    // the monitor picture of the HDMI workspace (Display > HDMI = Waveform)
    let mut hdmi_cv: Option<Canvas> = None;
    let mut hdmi_t: Option<Instant> = None;
    let mut frame_n = 0u64;
    // the first read at once (a complete start-up shows no screen at all)
    let mut start_up_t = Instant::now().checked_sub(std::time::Duration::from_millis(300)).unwrap_or_else(Instant::now);
    while !ui.quit && !input::stop_requested() {
        // the start-up file, every 250 ms while the screen is up
        if let Some(p) = start_up.as_deref().filter(|_| ui.start_up_active() && start_up_t.elapsed().as_millis() >= 250) {
            start_up_t = Instant::now();
            let up = std::fs::read_to_string("/proc/uptime").ok().and_then(|s| s.split_whitespace().next().and_then(|t| t.parse().ok())).unwrap_or(0.0);
            ui.start_up_update(&std::fs::read_to_string(p).unwrap_or_default(), up);
        }
        // inputs (waits up to the next frame)
        let elapsed = last.elapsed().as_secs_f64();
        let wait_ms = ((1.0 / 30.0 - elapsed) * 1000.0).max(0.0) as i32;
        for line_or_ev in inp.poll(wait_ms) {
            match line_or_ev {
                input::In::Ev(ev) => ui.handle(ev),
                input::In::Cmd(line) => match cmd::run(&mut ui, &line) {
                    cmd::Out::Shot(p) => ui.shot_request = Some(p),
                    _ => {}
                },
            }
        }
        scpi.poll(&mut ui, &cv);
        vxi.poll(&mut |c| scpi.exec_line(&mut ui, &cv, c));
        hislip.poll(&mut |c| scpi.exec_line(&mut ui, &cv, c));
        ann.poll();
        for c in web.poll(&mut ui, &cv, &mut scpi) {
            if let cmd::Out::Shot(p) = cmd::run(&mut ui, &c) {
                ui.shot_request = Some(p);
            }
        }
        if last.elapsed().as_secs_f64() < 1.0 / 30.0 {
            continue;
        }
        let dt = last.elapsed().as_secs_f64().min(0.2);
        last = Instant::now();
        let t0 = Instant::now();
        ui.step(dt);
        let t1 = Instant::now();
        let hdmi_frame = hdmi.is_some() && (frame_n + 1) % 2 == 0;
        ui.hdmi_snap_want = hdmi_frame && ui.m.hdmi_wave;
        ui.render(&mut cv);
        let t2 = Instant::now();
        disp.present(&cv);
        frame_n += 1;
        if let Some(h) = hdmi.as_mut().filter(|_| hdmi_frame) {
            if ui.m.hdmi_wave && !ui.hdmi_snap.is_empty() {
                let hc = hdmi_cv.get_or_insert_with(|| Canvas::new(h.width, h.height));
                hdmi_workspace(&ui, hc);
                h.present_scaled(hc);
            } else {
                h.present_scaled(&cv);
            }
        }
        let t3 = Instant::now();
        if hdmi_on && hdmi_t.is_none_or(|t| t.elapsed().as_secs() >= 3) {
            hdmi_t = Some(Instant::now());
            match hdmi.take() {
                Some(h) if !h.connected() => {
                    eprintln!("oos-ui: HDMI disconnected");
                    h.close();
                    hdmi_cv = None;
                }
                Some(h) => hdmi = Some(h),
                None => match disp.hdmi() {
                    Ok(h) => {
                        eprintln!("oos-ui: HDMI {}x{}", h.width, h.height);
                        hdmi = Some(h);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        // e.g. a mode the second VOP refuses: try again in 30 s, not every 3 s
                        eprintln!("oos-ui: HDMI: {}", e);
                        hdmi_t = Some(Instant::now() + std::time::Duration::from_secs(27));
                    }
                },
            }
        }
        let a = 0.05;
        ui.t_step += a * ((t1 - t0).as_secs_f32() * 1000.0 - ui.t_step);
        ui.t_render += a * ((t2 - t1).as_secs_f32() * 1000.0 - ui.t_render);
        ui.t_present += a * ((t3 - t2).as_secs_f32() * 1000.0 - ui.t_present);
        // LEDs: on change, and refreshed every 2 s (the MCU may have been reset)
        let lm = ui.leds();
        if lm != led_mask || led_t.elapsed().as_secs() >= 2 {
            inp.set_leds(lm);
            led_mask = lm;
            led_t = Instant::now();
        }
        if let Some(p) = ui.shot_request.take() {
            save_shot(&mut ui, &cv, p, &mut shot_n);
        }
        for c in ui.panel_tx.drain(..) {
            inp.send(&board::board().panel_cmd(c));
        }
        if let Some(a) = ui.power_action.take() {
            power(&mut inp, a);
        }
        if setup_t.elapsed().as_secs() >= 2 {
            setup_t = Instant::now();
            let t = store::setup_text(&ui.m);
            if t != setup_last {
                let tmp = format!("{}.new", SETUP_FILE);
                if std::fs::write(&tmp, &t).and_then(|_| std::fs::rename(&tmp, setup_path)).is_ok() {
                    // and across power cycles on OOS's own data partition (rcS copies it back to /tmp at boot)
                    if std::path::Path::new("/oos/setups").is_dir() {
                        let _ = std::fs::write("/oos/oos-ui.setup.new", &t).and_then(|_| std::fs::rename("/oos/oos-ui.setup.new", "/oos/oos-ui.setup"));
                    }
                    setup_last = t;
                }
            }
        }
        if net_t.map_or(true, |t| t.elapsed().as_secs() >= 5) {
            let (first, all) = ipv4_addresses();
            (ui.net_if, ui.net) = first;
            ui.net_ifs = all;
            ui.net_fixed = std::path::Path::new("/tmp/oos-net.fallback").exists();
            ui.eth_link = std::fs::read_to_string("/sys/class/net/eth0/carrier").is_ok_and(|c| c.trim() == "1");
            net_t = Some(Instant::now());
        }
        fps_n += 1;
        if fps_t.elapsed().as_secs_f64() >= 1.0 {
            ui.fps = fps_n as f32 / fps_t.elapsed().as_secs_f32();
            fps_n = 0;
            fps_t = Instant::now();
        }
    }
    inp.set_leds(0);
    if let Some(h) = hdmi {
        h.close();
    }
    disp.restore();
}

/// The HDMI workspace: a settings line on top, below it the waveform area of the panel (plot and measurement rows,
/// without the bars and menus) scaled to the monitor with its aspect ratio.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn hdmi_workspace(ui: &Ui, hc: &mut Canvas) {
    hc.clear(gfx::rgb(0x0d, 0x11, 0x17));
    let (sw, sh) = (ui::W, ui::BOTTOM.y - ui::TOP.h);
    if ui.hdmi_snap.len() < (sw * sh) as usize {
        return;
    }
    // the waveform area as large as it fits; the settings line in the band above it (at least 48 px)
    let head = 48.max((hc.h - hc.w * sh / sw) / 2);
    let fit = gfx::Fit::new(sw, sh, hc.w, hc.h - head);
    let stride = hc.w as usize;
    let f = &ui.fonts;
    hc.text_in(&f.xl, gfx::Rect::new(0, 0, hc.w, head), 1, &ui.hdmi_header(), gfx::rgb(0xe6, 0xed, 0xf3));
    // `fit` centres in the rest: start at the band so the plot sits right under the settings line
    let top = (head as usize).saturating_sub(fit.y0) * stride;
    fit.scale(&ui.hdmi_snap, &mut hc.px[top..], stride);
}

/// First IPv4 address of a non-loopback interface that is up ("" when none).
#[cfg(target_os = "linux")]
/// The address to show in the top bar, and all of them, as (interface, address).
fn ipv4_addresses() -> ((String, String), Vec<(String, String)>) {
    // an interface with a link first: eth0 keeps its fallback address without a cable
    let mut out = (String::new(), String::new());
    let mut all = Vec::new();
    let mut linked = false;
    unsafe {
        let mut ifa: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifa) != 0 {
            return (out, all);
        }
        let mut p = ifa;
        while !p.is_null() {
            let a = &*p;
            let up = a.ifa_flags & (libc::IFF_UP as u32) != 0 && a.ifa_flags & (libc::IFF_LOOPBACK as u32) == 0;
            if up && !a.ifa_addr.is_null() && (*a.ifa_addr).sa_family as i32 == libc::AF_INET {
                let link = a.ifa_flags & (libc::IFF_RUNNING as u32) != 0;
                let sin = &*(a.ifa_addr as *const libc::sockaddr_in);
                let name = std::ffi::CStr::from_ptr(a.ifa_name).to_string_lossy().into_owned();
                let addr = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)).to_string();
                if out.1.is_empty() || (link && !linked) {
                    out = (name.clone(), addr.clone());
                    linked = link;
                }
                all.push((name, addr));
            }
            p = a.ifa_next;
        }
        libc::freeifaddrs(ifa);
    }
    (out, all)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The HDMI workspace on a 1920 x 1080 monitor: settings line on top, the waveform area below without menus.
    #[test]
    fn hdmi_workspace_picture() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let mut cv = Canvas::new(ui::W, ui::H);
        ui.hdmi_snap_want = true;
        for _ in 0..10 {
            ui.step(1.0 / 30.0);
            ui.render(&mut cv);
        }
        let mut hc = Canvas::new(1920, 1080);
        hdmi_workspace(&ui, &mut hc);
        // the plot fills the width under the settings band (1024 x 486 -> 1920 x 911 under an 84-px band)
        assert_eq!(ui.hdmi_snap.len(), (ui::W * (ui::BOTTOM.y - ui::TOP.h)) as usize);
        assert!(ui.hdmi_header().contains("CH1 1 V/div"));
        if let Ok(d) = std::env::var("OOS_SHOT_DIR") {
            let _ = std::fs::write(format!("{}/hdmi-workspace.png", d), store::png_of(&hc));
        }
    }
}
