// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! oos-ui: the OOS oscilloscope screen.
//!
//!   oos-ui                         on the scope: DRM/KMS display, touch, front panel, control FIFO
//!   oos-ui --script FILE           anywhere: run a command script off-screen and write screenshots (tests)
//!   oos-ui --replay FRAME.bin --pre "<:WAV:PRE? string>"   show a captured raw frame (CH1) instead of the demo source
//!   --store-dir DIR                storage directory for setups, CSV and PNG (default /tmp/oos; scripts: the script's dir)
//!   --model NAME                   act as this model (DHO802 .. DHO924S), instead of /rigol/data/vendor.bin
//!   --limits FILE                  experimental limits (default /oos/limits.conf, docs/models.md)

// SCPI, web remote, display and the live source only run on the scope (Linux); the host build is the test runner.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

mod clock;
mod cmd;
mod decode;
mod ethlink;
mod compress;
mod deepmeas;
mod fastdraw;
mod font;
mod gfx;
mod hostname;
mod http;
mod board;
mod math;
mod mdns;
mod model;
mod net;
mod net6;
mod netacl;
mod protocols;
mod scpi;
mod sntp;
mod store;
mod sysconf;
mod trig;
mod trignear;
mod placement;
#[cfg(test)]
mod perfguard;
mod sparse;
mod drawpipe;
mod framestats;
mod ui;
mod usbtmc;
mod vxi11;
mod json;
mod gpu;
mod vpu;
mod layout;
mod ssh;
mod wave;
mod zone;
#[cfg(target_os = "linux")]
mod drm;
#[cfg(target_os = "linux")]
mod input;

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
    for u in &p.unit {
        eprintln!("oos-ui: unit default {} ({})", u, oos_hal::model::UNIT_CONF);
    }
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
    // on the unit the frame loop and the acquisition thread (started with the source) keep the two A72 for themselves
    #[cfg(target_os = "linux")]
    if args.iter().any(|a| a == "--hw") {
        http::pin_to_big_cores();
        placement::place(placement::Role::Ui);
    }
    let mut ui = Ui::new(source(&args));
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
        // another screen than the panel (host only: the device shows the panel it has)
        if let Some(s) = arg(&args, "--size") {
            match ui::Screen::parse(&s) {
                Some(scr) => ui.scr = scr,
                None => {
                    eprintln!("oos-ui: --size {}: give WIDTHxHEIGHT, at least {}x{}", s, ui::PANEL_W, ui::PANEL_H);
                    std::process::exit(2);
                }
            }
        }
        // `--gpu HELPER`: the picture through the GPU helper, read back every frame (host tests in a container)
        let gpu = arg(&args, "--gpu").map(|h| {
            let mut g = gpu::Gpu::start(&h).unwrap_or_else(|e| {
                eprintln!("oos-ui: --gpu: {}", e);
                std::process::exit(2)
            });
            g.target_memory(0, ui.scr.w, ui.scr.h).unwrap_or_else(|e| {
                eprintln!("oos-ui: --gpu: {}", e);
                std::process::exit(2)
            });
            g
        });
        run_script(&mut ui, &script, gpu);
        return;
    }
    if let Some(d) = arg(&args, "--store-dir") {
        ui.store_dir = d.into();
    }
    if args.iter().any(|a| a == "--serve") {
        return serve(ui, &args);
    }
    #[cfg(target_os = "linux")]
    {
        device_main(ui, &args);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = ui;
        eprintln!("oos-ui: no display backend on this OS; use --script FILE");
    }
}

/// No display and no hardware: the demo source behind SCPI, the web server and VXI-11, for tools/device-test.py on a
/// host. `OOS_DEMO_LOOP=<mask>` puts the generator on those channels, as a cable from the output would.
fn serve(mut ui: Ui, args: &[String]) {
    let port = |name: &str, d: u16| arg(args, name).and_then(|p| p.parse().ok()).unwrap_or(d);
    let mut web = http::Http::open(port("--web-port", 8080));
    let mut scpi = scpi::Scpi::open(port("--scpi-port", 5555));
    let mut vxi = vxi11::Server::open();
    let mut cv = Canvas::new(ui.scr.w, ui.scr.h);
    while !ui.quit {
        ui.step(0.04);
        ui.render(&mut cv);
        scpi.poll(&mut ui, &cv);
        vxi.poll(&mut |c| scpi.exec_line(&mut ui, &cv, c));
        for c in web.poll(&mut ui, &cv, &mut scpi) {
            cmd::run(&mut ui, &c);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// The layers of a GPU picture (System > Graphics = GPU): under and over the traces, and the traces as jobs. The
/// picture itself is in the display's buffer; it comes back into the caller's canvas only when asked.
struct GpuLayers {
    under: Canvas,
    over: Canvas,
    jobs: Vec<ui::TraceJob>,
}

impl GpuLayers {
    /// The layers record commands that the GPU draws (`gfx::Canvas::record`); `OOS_GPU_PIXELS=1`: they are drawn
    /// on the CPU and uploaded as pixels, as before (for comparison).
    fn new(w: i32, h: i32) -> GpuLayers {
        let (mut under, mut over) = (Canvas::new(w, h), Canvas::new_layer(w, h));
        let rec = std::env::var_os("OOS_GPU_PIXELS").is_none();
        under.record(rec);
        over.record(rec);
        GpuLayers { under, over, jobs: Vec::new() }
    }

    /// The screen in layers at `w` x `h`: the panel's, or (`view`) the monitor's own layout.
    fn draw(&mut self, ui: &mut Ui, w: i32, h: i32, view: bool) {
        if (self.under.w, self.under.h) != (w, h) {
            *self = GpuLayers::new(w, h);
        }
        let mut l = ui::Layers { under: &mut self.under, over: Some(&mut self.over), jobs: Vec::new() };
        if view {
            ui.render_view_layers(&mut l);
        } else {
            ui.render_layers(&mut l);
        }
        self.jobs = std::mem::take(&mut l.jobs);
    }

    fn quad<'a>(&'a self, ui: &'a Ui, src: gfx::Rect, dst: gfx::Rect, filter: bool, reuse: bool) -> gpu::Quad<'a> {
        gpu::Quad { under: Some(&self.under), over: Some(&self.over), size: (self.under.w, self.under.h), src, dst, filter, reuse, jobs: &self.jobs, ph: &ui.ph, phz: &ui.phz }
    }

    /// The panel's picture into `target` (the size of `cv`); with `read` it also comes back into `cv` (the web page,
    /// screenshots, SCPI).
    fn compose(&mut self, ui: &mut Ui, g: &mut gpu::Gpu, cv: &mut Canvas, target: usize, read: bool) -> Result<(), String> {
        self.draw(ui, cv.w, cv.h, false);
        let full = gfx::Rect::new(0, 0, cv.w, cv.h);
        g.compose(target, None, &[self.quad(ui, full, full, false, false)], read.then_some(cv))
    }

}

/// The monitor's picture on the GPU (Display > HDMI with Graphics = GPU), right after the panel's: Mirror and
/// Waveform take the panel's layers again without a new upload (quad 0); Layout draws the monitor's own layers.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[allow(clippy::too_many_arguments)]
fn hdmi_gpu_frame(ui: &mut Ui, g: &mut gpu::Gpu, panel: &GpuLayers, view: &mut GpuLayers, head: &mut Option<Canvas>, mw: i32, mh: i32, target: usize, read: bool) -> Result<gpu::Ticket, String> {
    let whole = gfx::Rect::new(0, 0, ui.scr.w, ui.scr.h);
    let fit_of = |sw: i32, sh: i32, dw: i32, dh: i32| {
        let f = gfx::Fit::new(sw, sh, dw, dh);
        let (w, h) = f.size();
        gfx::Rect::new(f.x0 as i32, f.y0 as i32, w as i32, h as i32)
    };
    let mirror = |g: &mut gpu::Gpu| g.submit(target, Some(0), &[panel.quad(ui, whole, fit_of(whole.w, whole.h, mw, mh), true, true)], read);
    match ui.m.hdmi {
        model::HdmiMode::Layout => match ui::Screen::for_monitor(mw, mh) {
            Some(s) => {
                view.draw(ui, s.w, s.h, true);
                // quad 0 stays the panel's: an empty one, the monitor's layers go to quad 1
                let none = gpu::Quad { under: None, over: None, size: (1, 1), src: gfx::Rect::new(0, 0, 1, 1), dst: gfx::Rect::new(0, 0, 0, 0), filter: false, reuse: false, jobs: &[], ph: &ui.ph, phz: &ui.phz };
                let q = view.quad(ui, gfx::Rect::new(0, 0, s.w, s.h), gfx::Rect::new(0, 0, mw, mh), true, false);
                g.submit(target, None, &[none, q], read)
            }
            None => mirror(g),
        },
        model::HdmiMode::Waveform => {
            // as `hdmi_workspace`: the settings line in a band on top, the waveform area as large as it fits under it
            let body = ui.frame().body;
            let (sw, sh) = (ui.scr.w, body.h);
            let band = 48.max((mh - mw * sh / sw) / 2);
            let r = fit_of(sw, sh, mw, mh - band);
            let hc = head.get_or_insert_with(|| Canvas::new(mw, band));
            if (hc.w, hc.h) != (mw, band) {
                *hc = Canvas::new(mw, band);
            }
            hc.clear(HDMI_BG);
            let f = font::Fonts::get(0);
            hc.text_in(&f.xl, gfx::Rect::new(0, 0, mw, band), 1, &ui.hdmi_header(), gfx::rgb(0xe6, 0xed, 0xf3));
            // the waveform area without the bars and menus: the layer over the traces as it was before them
            let over = ui.hdmi_over.as_ref().filter(|o| (o.w, o.h) == (panel.under.w, panel.under.h)).unwrap_or(&panel.over);
            let plot = gpu::Quad { over: Some(over), reuse: false, ..panel.quad(ui, body, gfx::Rect::new(r.x, band, r.w, r.h), true, false) };
            let line = gpu::Quad { under: Some(hc), over: None, size: (mw, band), src: gfx::Rect::new(0, 0, mw, band), dst: gfx::Rect::new(0, 0, mw, band), filter: false, reuse: false, jobs: &[], ph: &ui.ph, phz: &ui.phz };
            g.submit(target, Some(HDMI_BG), &[plot, line], read)
        }
        model::HdmiMode::Mirror => mirror(g),
    }
}

/// Background of the HDMI workspace.
const HDMI_BG: u32 = 0x0d1117;

/// The GPU helper with the panel's scan-out buffers as targets 0 to 2 (the monitor's follow them, up to the helper's 8).
#[cfg(target_os = "linux")]
fn start_gpu(disp: &drm::Display) -> Result<gpu::Gpu, String> {
    http::pin_to_big_cores();
    placement::place(placement::Role::Ui);
    let mut g = gpu::Gpu::start(gpu::HELPER)?;
    for i in 0..disp.buffers() {
        let (fd, pitch) = disp.export(i).map_err(|e| format!("export: {}", e))?;
        g.target_dmabuf(i, disp.width, disp.height, pitch, fd)?;
    }
    g.monitor_targets_from(disp.buffers());
    Ok(g)
}

fn run_script(ui: &mut Ui, path: &str, mut gpu: Option<gpu::Gpu>) {
    let text = std::fs::read_to_string(path).expect("script");
    let mut cv = Canvas::new(ui.scr.w, ui.scr.h);
    let mut gl = GpuLayers::new(ui.scr.w, ui.scr.h);
    // with --gpu, `OOS_GPU_HDMI=1920x1080 OOS_GPU_HDMI_MODE=Mirror|Waveform|Layout`: each shot also writes the
    // monitor's picture as the GPU puts it together (`<shot>-hdmi.ppm`)
    let hdmi = std::env::var("OOS_GPU_HDMI").ok().and_then(|s| ui::Screen::parse(&s)).filter(|_| gpu.is_some());
    if let Some(m) = std::env::var("OOS_GPU_HDMI_MODE").ok().and_then(|v| model::HDMI_MODES.iter().copied().find(|x| format!("{:?}", x) == v)) {
        ui.m.hdmi = m;
    }
    let (mut gl_view, mut gl_head, mut hdmi_cv) = (GpuLayers::new(1, 1), None, hdmi.map(|s| Canvas::new(s.w, s.h)));
    if let (Some(g), Some(s)) = (gpu.as_mut(), hdmi) {
        g.target_memory(2, s.w, s.h).expect("HDMI target");
    }
    ui.info = "host test run".into();
    let mut shot_n = 0;
    // one frame: on the CPU, or in layers through the GPU and read back
    fn frame(ui: &mut Ui, cv: &mut Canvas, gpu: &mut Option<gpu::Gpu>, gl: &mut GpuLayers, shot_n: &mut u32) {
        ui.step(1.0 / 30.0);
        ui.hdmi_snap_want = std::env::var_os("OOS_GPU_HDMI").is_some();
        match gpu.as_mut() {
            Some(g) => {
                if let Err(e) = gl.compose(ui, g, cv, 0, true) {
                    eprintln!("oos-ui: GPU frame: {}", e);
                    std::process::exit(3);
                }
            }
            None => ui.render(cv),
        }
        if let Some(p) = ui.shot_request.take() {
            save_shot(ui, cv, p, shot_n);
        }
    }
    frame(ui, &mut cv, &mut gpu, &mut gl, &mut shot_n);
    for line in text.lines() {
        match cmd::run(ui, line) {
            cmd::Out::Frames(n) => {
                for _ in 0..n {
                    frame(ui, &mut cv, &mut gpu, &mut gl, &mut shot_n);
                }
            }
            cmd::Out::Shot(p) => {
                frame(ui, &mut cv, &mut gpu, &mut gl, &mut shot_n);
                cmd::write_ppm(&cv, &p).expect("write shot");
                println!("shot {}", p);
                if let (Some(g), Some(hc)) = (gpu.as_mut(), hdmi_cv.as_mut()) {
                    let (w, h) = (hc.w, hc.h);
                    if let Err(e) = hdmi_gpu_frame(ui, g, &gl, &mut gl_view, &mut gl_head, w, h, 2, true).and_then(|t| g.collect(t, Some(hc))) {
                        eprintln!("oos-ui: GPU HDMI: {}", e);
                        std::process::exit(3);
                    }
                    cmd::write_ppm(hc, &p.replace(".ppm", "-hdmi.ppm")).expect("write shot");
                    // OOS_GPU_TIMES=<file>: the helper's time of the last panel frame and of this monitor frame, a
                    // line per shot (gpu/test-host.sh: the monitor's cost per pixel against the panel's)
                    if let Some(f) = std::env::var_os("OOS_GPU_TIMES") {
                        use std::io::Write;
                        let line = format!("{:.3} {:.3}\n", g.times.panel, g.times.monitor);
                        let _ = std::fs::OpenOptions::new().create(true).append(true).open(f).and_then(|mut f| f.write_all(line.as_bytes()));
                    }
                }
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
    ui.measure_in_background();
    let mut disp = match drm::Display::open("/dev/dri/card0") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("oos-ui: display: {}", e);
            std::process::exit(1);
        }
    };
    eprintln!("oos-ui: display {}x{}", disp.width, disp.height);
    // the layout follows the panel of the unit (the DHO800/DHO900: 1024 x 600); a smaller one keeps the panel layout
    if let Some(s) = ui::Screen::of_size(disp.width, disp.height) {
        ui.scr = s;
    }
    let mut inp = input::Inputs::open(args);
    // "app UI loaded", sent once at the end of the start-up (board.md section 4)
    inp.send(&board::board().panel_cmd(board::PanelCmd::UiLoaded));
    // start-up screen (ui/startup.rs) while the acquisition hardware starts; the own picture is its background
    let start_up = arg(args, "--start-up");
    if start_up.is_some() {
        let mut cv = Canvas::new(ui::PANEL_W, ui::PANEL_H);
        let splash = SPLASH.iter().find_map(|p| std::fs::read(p).ok()).and_then(|b| store::ppm_into(&b, &mut cv)).map(|_| cv.px);
        ui.start_up_begin(splash);
    }
    // own picture at power-on: without the start-up screen, the first start of a boot shows a binary
    // PPM for 2 s
    if start_up.is_none() && !std::path::Path::new("/tmp/oos-splash.done").exists() {
        let _ = std::fs::write("/tmp/oos-splash.done", b"");
        let mut cv = Canvas::new(ui::PANEL_W, ui::PANEL_H);
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
                eprintln!("oos-ui: settings restored from {}", SETUP_FILE);
            }
            Err(e) => eprintln!("oos-ui: {}: {}", SETUP_FILE, e),
        }
    }
    // settings of the scope itself: NTP, time zone, Ethernet (/oos/system.conf)
    ui.load_system(sysconf::default_path());
    ui.set_ssh_dir(ssh::dir());
    let mut setup_last = store::setup_text(&ui.m);
    let mut setup_t = Instant::now();
    ui.info = std::fs::read_to_string("/proc/sys/kernel/osrelease").map(|s| format!("Linux {}", s.trim())).unwrap_or_default();
    let mut cv = Canvas::new(ui.scr.w, ui.scr.h);
    let mut last = Instant::now();
    let mut fps_t = Instant::now();
    let mut fps_n = 0;
    let mut shot_n = 0;
    let mut led_mask = u32::MAX;
    let mut led_t = Instant::now();
    let mut net_t: Option<Instant> = None;
    let eth_t0 = Instant::now();
    let mut eth_t: Option<Instant> = None;
    input::install_signal_handlers();
    let web_port = arg(args, "--web-port").and_then(|p| p.parse().ok()).unwrap_or(8080);
    let scpi_port = arg(args, "--scpi-port").and_then(|p| p.parse().ok()).unwrap_or(5555);
    let mut web = http::Http::open(web_port);
    let mut scpi = scpi::Scpi::open(scpi_port);
    let mut vxi = vxi11::Server::open();
    // who may update or control the scope over the network: net.allow (docs/network.md 6); read once per start
    let acl = netacl::Acl::load(&sysconf::default_path());
    web.acl = acl.clone();
    scpi.acl = acl.clone();
    vxi.acl = acl;
    // System > Network > Remote services, as applied: web, SCPI, VXI-11, SSH and its password login (None: not yet,
    // at the start)
    let mut remote_now: Option<[bool; 5]> = None;
    // the name on the network and the mDNS responder (mdns.rs), as applied; looked at once a second
    let mut mdns = mdns::Mdns::new(None);
    let mut mdns_now: Option<mdns::Config> = None;
    let mut mdns_t: Option<Instant> = None;
    // HDMI mirror: a monitor is looked for every 3 s and gets the panel picture at 15 frames/s (`--no-hdmi`: off)
    let hdmi_on = !args.iter().any(|a| a == "--no-hdmi");
    let mut hdmi: Option<drm::Display> = None;
    // the monitor's own picture (Display > HDMI = Waveform or Layout)
    let mut hdmi_cv: Option<Canvas> = None;
    let mut hdmi_t: Option<Instant> = None;
    let mut frame_n = 0u64;
    // System > Graphics = GPU: the helper, the layers of the panel and of the monitor's own layout, the monitor's
    // settings line; `gpu_failed`: the GPU did not start or failed, the CPU draws until the setting changes
    let mut gpu: Option<gpu::Gpu> = None;
    let (mut gpu_want, mut gpu_failed) = (false, false);
    // the GPU's start is told on screen only when the setting was changed, not at every power-on (GPU is the default)
    let mut gpu_changed = false;
    let mut gl = GpuLayers::new(ui.scr.w, ui.scr.h);
    let mut gl_view = GpuLayers::new(1, 1);
    let mut gl_head: Option<Canvas> = None;
    // the monitor's buffers are the targets after the panel's (`start_gpu`: one per panel buffer, three). They were
    // 2 to 4 while the panel had two: the monitor's first buffer took the place of the panel's third, the GPU drew the
    // panel's picture into it, and the panel showed an old picture in its turn (both screens flickered with HDMI and
    // Graphics = GPU). None not yet, Some(false) they could not be taken
    let hdmi_base = disp.buffers();
    let mut hdmi_gpu: Option<bool> = None;
    // the frame in the GPU's work: its ticket, the panel buffer it fills, and the monitor's frame with its buffer
    let mut gpu_pending: Option<(gpu::Ticket, usize, Option<(gpu::Ticket, usize)>)> = None;
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
        // the services follow their switches: the ports were opened above, SSH is started by rcS (oos-ssh)
        let want = [ui.sys.remote_web, ui.sys.remote_scpi, ui.sys.remote_vxi11, ui.sys.remote_ssh, ui.sys.ssh_password];
        if remote_now != Some(want) {
            let was = remote_now.unwrap_or([true, true, true, want[3], want[4]]);
            if want[0] != was[0] {
                web.set_open(want[0].then_some(web_port));
            }
            if want[1] != was[1] {
                scpi.set_open(want[1].then_some(scpi_port));
            }
            if want[2] != was[2] {
                vxi.set_open(want[2]);
            }
            if want[3] != was[3] || want[4] != was[4] {
                ssh::apply(want[3]);
            }
            remote_now = Some(want);
        }
        if mdns_t.map_or(true, |t| t.elapsed().as_secs() >= 1) {
            mdns_t = Some(Instant::now());
            let c = ui.mdns_config(web_port, scpi_port);
            if mdns_now.as_ref() != Some(&c) {
                if mdns_now.as_ref().map_or(true, |o| o.name != c.name) {
                    hostname::set(&c.name);
                }
                mdns.set(c.clone());
                mdns_now = Some(c);
            }
        }
        scpi.poll(&mut ui, &cv);
        vxi.poll(&mut |c| scpi.exec_line(&mut ui, &cv, c));
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
        ui.hdmi_snap_want = hdmi_frame && ui.m.hdmi == model::HdmiMode::Waveform;
        // System > Graphics: the helper follows the setting
        if ui.sys.gpu != gpu_want {
            gpu_changed = frame_n > 0;
            (gpu_want, gpu_failed) = (ui.sys.gpu, false);
            if gpu.take().is_some() {
                eprintln!("oos-ui: GPU off");
            }
            (hdmi_gpu, gpu_pending) = (None, None);
        }
        if gpu_want && gpu.is_none() && !gpu_failed {
            match start_gpu(&disp) {
                Ok(g) => {
                    eprintln!("oos-ui: GPU on");
                    if gpu_changed {
                        ui.toast("Graphics: GPU");
                    }
                    gpu = Some(g);
                }
                Err(e) => {
                    gpu_failed = true;
                    eprintln!("oos-ui: GPU: {}", e);
                    if gpu_changed {
                        ui.toast(&format!("No GPU ({}): drawing on the CPU", e));
                    }
                }
            }
            hdmi_gpu = None;
        }
        // the web page's picture: the screen the CPU draws next swaps out the shown one; with the GPU, `cv` changes only
        // when a picture comes back, and each one goes to the web page from there
        if gpu.is_some() {
            web.before_gpu_frame();
        } else {
            web.before_render(&mut cv, !ui.start_up_active());
        }
        let mut drawn = false;
        let mut hdmi_done = false;
        if let Some(g) = gpu.as_mut() {
            let r: Result<(), String> = (|| {
                // the picture comes back only for someone who wants it: the web page, a screenshot, and twice a
                // second for SCPI and the rest
                let shot = ui.shot_request.is_some();
                // and every frame for a monitor that gets the panel's picture from the CPU (its buffers could not be
                // given to the GPU), else it would show a picture up to half a second old
                let hdmi_cpu = hdmi.is_some() && hdmi_gpu == Some(false);
                let read = shot || web.picture_wanted() || frame_n % 15 == 0 || hdmi_cpu;
                // 1. this frame's layers, while the GPU still puts the last one together
                let tl = Instant::now();
                gl.draw(&mut ui, cv.w, cv.h, false);
                let layers = tl.elapsed().as_secs_f32() * 1e3;
                // 2. the last frame: shown once it is done (its picture into `cv` when it was asked for)
                let mut wait = 0.0;
                if let Some((tp, bp, th)) = gpu_pending.take() {
                    let back = tp.reads();
                    g.collect(tp, Some(&mut cv))?;
                    if back {
                        web.picture_read_back(&cv);
                    }
                    wait += g.times.wait;
                    disp.flip(bp);
                    if let Some((t2, b2)) = th {
                        g.collect(t2, None)?;
                        if let Some(h) = hdmi.as_mut() {
                            h.flip_scaled(b2);
                        }
                    }
                }
                // the monitor's buffers for the GPU, once per monitor (a message that is answered at once: only
                // with no frame in work)
                if let (Some(h), None) = (hdmi.as_ref(), hdmi_gpu) {
                    let mut ok = true;
                    for i in 0..h.buffers() {
                        let (fd, pitch) = h.export(i).map_err(|e| format!("HDMI export: {}", e))?;
                        ok &= g.target_dmabuf(hdmi_base + i, h.width, h.height, pitch, fd).map_err(|e| eprintln!("oos-ui: GPU HDMI: {}", e)).is_ok();
                    }
                    hdmi_gpu = Some(ok);
                }
                // 3. this frame to the GPU (into the buffer neither shown nor waiting to be); a screenshot waits for
                // it, so it is of this frame
                let b = disp.free();
                let full = gfx::Rect::new(0, 0, cv.w, cv.h);
                let tp = g.submit(b, None, &[gl.quad(&ui, full, full, false, false)], read)?;
                let copy = g.times.copy;
                let th = match (hdmi.as_ref(), hdmi_gpu, hdmi_frame) {
                    (Some(h), Some(true), true) => {
                        let b2 = h.free();
                        Some((hdmi_gpu_frame(&mut ui, g, &gl, &mut gl_view, &mut gl_head, h.width, h.height, hdmi_base + b2, false)?, b2))
                    }
                    _ => None,
                };
                hdmi_done = th.is_some() || (hdmi_gpu == Some(true) && hdmi_frame);
                if shot {
                    g.collect(tp, Some(&mut cv))?;
                    web.picture_read_back(&cv);
                    disp.flip(b);
                    if let Some((t2, b2)) = th {
                        g.collect(t2, None)?;
                        if let Some(h) = hdmi.as_mut() {
                            h.flip_scaled(b2);
                        }
                    }
                } else {
                    gpu_pending = Some((tp, b, th));
                }
                let t = g.times;
                ui.t_gpu = Some([layers, copy, wait, t.upload, t.draw, t.read]);
                ui.t_gpu_screens = Some((t.panel, (hdmi.is_some() && hdmi_gpu == Some(true)).then_some(t.monitor)));
                Ok(())
            })();
            match r {
                Ok(()) => drawn = true,
                Err(e) => {
                    (gpu, gpu_failed, hdmi_gpu, gpu_pending, hdmi_done) = (None, true, None, None, false);
                    eprintln!("oos-ui: GPU frame: {}", e);
                    ui.toast("GPU failed: drawing on the CPU");
                }
            }
        }
        if !drawn {
            (ui.t_gpu, ui.t_gpu_screens) = (None, None);
            ui.render(&mut cv);
        }
        let t2 = Instant::now();
        if !drawn {
            disp.present(&cv);
        }
        frame_n += 1;
        if let Some(h) = hdmi.as_mut().filter(|_| hdmi_frame && !hdmi_done) {
            // the monitor's picture: its own layout, the workspace, or the panel's
            let size = match ui.m.hdmi {
                model::HdmiMode::Layout => ui::Screen::for_monitor(h.width, h.height),
                model::HdmiMode::Waveform if !ui.hdmi_snap.is_empty() => Some(ui::Screen { w: h.width, h: h.height }),
                _ => None,
            };
            match size {
                Some(s) => {
                    if hdmi_cv.as_ref().is_none_or(|c| (c.w, c.h) != (s.w, s.h)) {
                        hdmi_cv = Some(Canvas::new(s.w, s.h));
                    }
                    let hc = hdmi_cv.as_mut().unwrap();
                    if ui.m.hdmi == model::HdmiMode::Layout {
                        ui.render_view(hc);
                    } else {
                        hdmi_workspace(&ui, hc);
                    }
                    h.present_scaled(hc);
                }
                None => h.present_scaled(&cv),
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
                    // a frame of it still in work is collected, not shown
                    hdmi_gpu = None;
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
        ui.hdmi_out = hdmi.as_ref().map(|h| (h.width as u32, h.height as u32));
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
            let t = store::setup_text(ui.setup_model());
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
        if eth_t.map_or(true, |t| t.elapsed().as_secs() >= 1) {
            eth_t = Some(Instant::now());
            eth_watch(&mut ui, eth_t0.elapsed().as_secs_f64());
        }
        fps_n += 1;
        if fps_t.elapsed().as_secs_f64() >= 1.0 {
            ui.fps = fps_n as f32 / fps_t.elapsed().as_secs_f32();
            fps_n = 0;
            fps_t = Instant::now();
        }
    }
    // goodbyes on the network (SIGTERM, quit)
    mdns.stop();
    inp.set_leds(0);
    if let Some(h) = hdmi {
        h.close();
    }
    disp.restore();
}

/// Once a second: a link at 10 Mbit/s gets one more autonegotiation (System > Network > 10 Mbit retry, ethlink.rs).
#[cfg(target_os = "linux")]
fn eth_watch(ui: &mut Ui, now: f64) {
    let speed = ethlink::speed();
    match ui.eth.step(now, ethlink::carrier(), speed, ui.sys.eth_retry_10m) {
        Some(ethlink::Act::Restart) => {
            eprintln!(
                "oos-ui: {} up at 10 Mbit/s {}, restarting the autonegotiation ({} of {}; {})",
                ethlink::IFACE,
                ethlink::duplex(),
                ui.eth.tries,
                ethlink::MAX_TRIES,
                ethlink::mii_regs()
            );
            if let Err(e) = ethlink::restart() {
                eprintln!("oos-ui: {}: autonegotiation restart failed: {}", ethlink::IFACE, e);
            }
        }
        Some(ethlink::Act::Result(s)) => {
            let s = s.map_or("no speed".to_string(), |s| format!("{} Mbit/s", s));
            eprintln!("oos-ui: {} after the restart: {} {} ({})", ethlink::IFACE, s, ethlink::duplex(), ethlink::mii_regs());
        }
        None => {}
    }
}

/// The HDMI workspace: a settings line on top, below it the waveform area of the panel (plot and measurement rows,
/// without the bars and menus) scaled to the monitor with its aspect ratio.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn hdmi_workspace(ui: &Ui, hc: &mut Canvas) {
    hc.clear(HDMI_BG);
    let body = ui.frame().body;
    let (sw, sh) = (ui.scr.w, body.h);
    if ui.hdmi_snap.len() < (sw * sh) as usize {
        return;
    }
    // the waveform area as large as it fits; the settings line in the band above it (at least 48 px)
    let head = 48.max((hc.h - hc.w * sh / sw) / 2);
    let fit = gfx::Fit::new(sw, sh, hc.w, hc.h - head);
    let stride = hc.w as usize;
    // the monitor is large: its header keeps the normal text size
    let f = font::Fonts::get(0);
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
        let mut cv = Canvas::new(ui::PANEL_W, ui::PANEL_H);
        ui.hdmi_snap_want = true;
        for _ in 0..10 {
            ui.step(1.0 / 30.0);
            ui.render(&mut cv);
        }
        let mut hc = Canvas::new(1920, 1080);
        hdmi_workspace(&ui, &mut hc);
        // the plot fills the width under the settings band (1024 x 486 -> 1920 x 911 under an 84-px band)
        assert_eq!(ui.hdmi_snap.len(), (ui.scr.w * ui.frame().body.h) as usize);
        assert!(ui.hdmi_header().contains("CH1 1 V/div"));
        if let Ok(d) = std::env::var("OOS_SHOT_DIR") {
            let _ = std::fs::write(format!("{}/hdmi-workspace.png", d), store::png_of(&hc));
        }
    }
}
