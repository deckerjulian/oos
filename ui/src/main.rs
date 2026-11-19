// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! oos-ui: the OOS oscilloscope screen.
//!
//!   oos-ui                         on the scope: DRM/KMS display, touch, front panel, control FIFO
//!   oos-ui --offline               start in offline mode: analog off, no demo on CH1-CH4
//!   oos-ui --script FILE           anywhere: run a command script off-screen and write screenshots (tests)
//!   oos-ui --replay FRAME.bin --pre "<:WAV:PRE? string>"   show a captured raw frame (CH1) instead of the demo source
//!   --store-dir DIR                storage directory for setups, CSV and PNG (default /tmp/oos; scripts: the script's dir)
//!   --model NAME                   act as this model (DHO802 .. DHO924S), instead of /rigol/data/vendor.bin
//!   --limits FILE                  experimental limits (default /oos/limits.conf, docs/models.md)

// SCPI, web remote, display and the live source only run on the scope (Linux); the host build is the test runner and
// leaves them unused. The guard against dead code is the scope build (tools/check.sh target): what only the tests use
// is #[cfg(test)] or carries an allow with its reason.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

mod board;
mod cmd;
mod compress;
mod decode;
mod deepmeas;
mod drawpipe;
#[cfg(target_os = "linux")]
mod drm;
mod fastdraw;
mod font;
mod framestats;
mod gfx;
mod gpu;
mod hid;
#[cfg(target_os = "linux")]
mod input;
mod layout;
mod logic;
mod math;
mod measures;
mod model;
#[cfg(test)]
mod perfguard;
mod platform;
mod protocols;
mod scpi;
mod settings;
mod shots;
mod site;
#[cfg(test)]
mod soak;
mod sparse;
mod store;
mod trig;
mod trignear;
mod ui;
mod vpu;
mod wave;
mod words;
mod zone;

// the system services (ADR 0064): their paths in this crate stay `crate::sysconf` and the like
#[cfg(target_os = "linux")]
use oos_system::hostname;
// the transports (ADR 0064), generic over the program they carry: here always `scpi::Program`
use oos_remote::{announce, hislip, http, vxi11};
use oos_system::{
    base64, clock, ethlink, json, mdns, net, net6, netacl, placement, remote, sntp, ssh, sysconf, system, usbtmc,
};

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
        let mask =
            arg(args, "--app-mask").and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()).unwrap_or(1);
        let cal_dir = arg(args, "--cal").unwrap_or_else(|| "/rigol/data".into());
        eprintln!("oos-ui: board {}", board::board().name());
        let shared = board::board().spawn(board::Spawn {
            mask,
            program_afe: args.iter().any(|a| a == "--afe"),
            cal_dir,
            start_up: arg(args, "--start-up"),
        });
        return Source::Hw { shared, seen: 0 };
    }
    if let Some(path) = arg(args, "--replay") {
        let raw = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {}", path, e));
        let words: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
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
        placement::pin_to_big_cores();
        placement::place(placement::Role::Ui);
    }
    let mut ui = Ui::new(source(&args));
    if args.iter().any(|a| a == "--offline") {
        ui.set_offline(true);
    }
    if let Some(m) = &foreign {
        ui.toast(&format!(
            "This is a {}: OOS drives DHO800/DHO900 hardware only. Demo signal shown (docs/porting.md)",
            m
        ));
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

/// No display and no hardware: the demo source behind SCPI and the web server on a computer, which also accepts
/// HiSLIP. VXI-11 stays on the scope, where port 111 is free. `OOS_DEMO_LOOP=<mask>` puts the generator on those
/// channels, as a cable from the output would.
fn serve(mut ui: Ui, args: &[String]) {
    let (mut web, mut scpi, mut vxi, mut hislip, mut ann) = lan(false, args, &ui.acl());
    let mut cv = Canvas::new(ui.scr.w, ui.scr.h);
    let live = shots::Live::default();
    while !ui.quit {
        ui.step(0.04);
        ui.render(&mut cv);
        scpi.poll(&mut ui, &cv);
        vxi.poll_async(&mut |program| scpi.poll_program(&mut ui, &cv, program));
        hislip.poll_async(&mut |program| scpi.poll_program(&mut ui, &cv, program));
        for c in web.poll(&mut site::Ctx::new(&mut ui, &cv, &mut scpi).with_live(&live)) {
            cmd::run(&mut ui, &c);
        }
        ann.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// The scope, where port 111 is free for VXI-11: the acquisition device is present, or the boot identity was read.
/// A computer has neither, and accepts HiSLIP instead.
fn on_scope() -> bool {
    std::path::Path::new("/dev/xdma0_bypass").exists()
        || matches!(oos_hal::model::platform().source, oos_hal::model::Source::Vendor)
}

/// SCPI, the web server, and whichever of VXI-11 and HiSLIP this machine accepts, plus the announcement. `acl`: the
/// address list they check, the one the screen sets (`Ui::acl`).
fn lan(
    scope: bool,
    args: &[String],
    acl: &netacl::Shared,
) -> (site::Web, scpi::Scpi, vxi11::Server<scpi::Program>, hislip::Server<scpi::Program>, announce::Announcer) {
    let (web, scpi, vxi, hislip) = lan_servers(scope, args, acl);
    let services = announce::listening(web.port(), scpi.port(), vxi.service_port(), hislip.port());
    let ann = announce::Announcer::open(&services);
    (web, scpi, vxi, hislip, ann)
}

/// The listeners of `lan` without the announcement: the scope announces itself by name (mdns.rs, System > Network).
fn lan_servers(
    scope: bool,
    args: &[String],
    acl: &netacl::Shared,
) -> (site::Web, scpi::Scpi, vxi11::Server<scpi::Program>, hislip::Server<scpi::Program>) {
    let mut web = site::Web::open(lan_port(args, "--web-port", 8080));
    let mut scpi = scpi::Scpi::open(lan_port(args, "--scpi-port", 5555));
    let mut vxi = if scope { vxi11::Server::open() } else { vxi11::Server::closed() };
    let hislip_port = lan_port(args, "--hislip-port", hislip::PORT);
    let mut hislip = if scope { hislip::Server::closed() } else { hislip::Server::open(hislip_port) };
    (web.acl, scpi.acl, vxi.acl, hislip.acl) = (acl.clone(), acl.clone(), acl.clone(), acl.clone());
    web.offer(http::Offer { scpi_port: scpi.port(), vxi11: vxi.service_port().is_some(), hislip_port: hislip.port() });
    (web, scpi, vxi, hislip)
}

fn lan_port(args: &[String], name: &str, d: u16) -> u16 {
    arg(args, name).and_then(|p| p.parse().ok()).unwrap_or(d)
}

/// The name responder's services as System > Network sets them, made to match the listeners that are really up:
/// VXI-11 only where it listens, HiSLIP where it does, and LXI discovery (`_lxi._tcp`) beside the web server, both
/// pointing at the identification page.
fn mdns_services(mut c: mdns::Config, web: Option<u16>, vxi: Option<u16>, hislip: Option<u16>) -> mdns::Config {
    if vxi.is_none() {
        c.services.retain(|s| s.kind != "_vxi-11._tcp");
    }
    if let Some(p) = web.filter(|_| c.services.iter().any(|s| s.kind == "_http._tcp")) {
        let path = format!("path={}", announce::IDENTIFICATION_PATH);
        c.services.push(mdns::Service { kind: "_lxi._tcp".into(), port: p, txt: vec![path] });
    }
    if let Some(p) = hislip {
        c.services.push(mdns::Service { kind: "_hislip._tcp".into(), port: p, txt: Vec::new() });
    }
    c
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
        gpu::Quad {
            under: Some(&self.under),
            over: Some(&self.over),
            size: (self.under.w, self.under.h),
            src,
            dst,
            filter,
            reuse,
            jobs: &self.jobs,
            ph: &ui.ph,
            phz: &ui.phz,
        }
    }

    /// The panel's picture into `target` (the size of `cv`); with `read` it also comes back into `cv` (the web page,
    /// screenshots, SCPI).
    fn compose(
        &mut self,
        ui: &mut Ui,
        g: &mut gpu::Gpu,
        cv: &mut Canvas,
        target: usize,
        read: bool,
    ) -> Result<(), String> {
        self.draw(ui, cv.w, cv.h, false);
        let full = gfx::Rect::new(0, 0, cv.w, cv.h);
        g.compose(target, None, &[self.quad(ui, full, full, false, false)], read.then_some(cv))
    }
}

/// The monitor's picture on the GPU (Display > HDMI with Graphics = GPU), right after the panel's: Mirror and
/// Waveform take the panel's layers again without a new upload (quad 0); Layout draws the monitor's own layers. The
/// monitor is `mw` x `mh`; the picture goes to `target`, read back with `read` (as `Gpu::submit`).
fn hdmi_gpu_frame(
    ui: &mut Ui,
    g: &mut gpu::Gpu,
    panel: &GpuLayers,
    view: &mut GpuLayers,
    head: &mut Option<Canvas>,
    (mw, mh): (i32, i32),
    (target, read): (usize, bool),
) -> Result<gpu::Ticket, String> {
    let whole = gfx::Rect::new(0, 0, ui.scr.w, ui.scr.h);
    let fit_of = |sw: i32, sh: i32, dw: i32, dh: i32| {
        let f = gfx::Fit::new(sw, sh, dw, dh);
        let (w, h) = f.size();
        gfx::Rect::new(f.x0 as i32, f.y0 as i32, w as i32, h as i32)
    };
    let mirror = |g: &mut gpu::Gpu| {
        g.submit(target, Some(0), &[panel.quad(ui, whole, fit_of(whole.w, whole.h, mw, mh), true, true)], read)
    };
    match ui.m.hdmi {
        model::HdmiMode::Layout => match ui::Screen::for_monitor(mw, mh) {
            Some(s) => {
                view.draw(ui, s.w, s.h, true);
                // quad 0 stays the panel's: an empty one, the monitor's layers go to quad 1
                let none = gpu::Quad {
                    under: None,
                    over: None,
                    size: (1, 1),
                    src: gfx::Rect::new(0, 0, 1, 1),
                    dst: gfx::Rect::new(0, 0, 0, 0),
                    filter: false,
                    reuse: false,
                    jobs: &[],
                    ph: &ui.ph,
                    phz: &ui.phz,
                };
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
            let over =
                ui.hdmi_over.as_ref().filter(|o| (o.w, o.h) == (panel.under.w, panel.under.h)).unwrap_or(&panel.over);
            let plot = gpu::Quad {
                over: Some(over),
                reuse: false,
                ..panel.quad(ui, body, gfx::Rect::new(r.x, band, r.w, r.h), true, false)
            };
            let line = gpu::Quad {
                under: Some(hc),
                over: None,
                size: (mw, band),
                src: gfx::Rect::new(0, 0, mw, band),
                dst: gfx::Rect::new(0, 0, mw, band),
                filter: false,
                reuse: false,
                jobs: &[],
                ph: &ui.ph,
                phz: &ui.phz,
            };
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
    placement::pin_to_big_cores();
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
    if let Some(m) = std::env::var("OOS_GPU_HDMI_MODE")
        .ok()
        .and_then(|v| model::HDMI_MODES.iter().copied().find(|x| format!("{:?}", x) == v))
    {
        ui.m.hdmi = m;
    }
    let (mut gl_view, mut gl_head, mut hdmi_cv) = (GpuLayers::new(1, 1), None, hdmi.map(|s| Canvas::new(s.w, s.h)));
    if let (Some(g), Some(s)) = (gpu.as_mut(), hdmi) {
        g.target_memory(2, s.w, s.h).expect("HDMI target");
    }
    ui.info = "host test run".into();
    let mut shot_n = 0;
    // for the script command `scpi`: the interpreter without a port
    let mut scpi = scpi::Scpi::open(0);
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
                    if let Err(e) = hdmi_gpu_frame(ui, g, &gl, &mut gl_view, &mut gl_head, (w, h), (2, true))
                        .and_then(|t| g.collect(t, Some(hc)))
                    {
                        eprintln!("oos-ui: GPU HDMI: {}", e);
                        std::process::exit(3);
                    }
                    cmd::write_ppm(hc, &p.replace(".ppm", "-hdmi.ppm")).expect("write shot");
                    // OOS_GPU_TIMES=<file>: the helper's time of the last panel frame and of this monitor frame, a
                    // line per shot (gpu/test-host.sh: the monitor's cost per pixel against the panel's)
                    if let Some(f) = std::env::var_os("OOS_GPU_TIMES") {
                        use std::io::Write;
                        let line = format!("{:.3} {:.3}\n", g.times.panel, g.times.monitor);
                        let _ = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(f)
                            .and_then(|mut f| f.write_all(line.as_bytes()));
                    }
                }
            }
            cmd::Out::Scpi(path, message) => {
                // an answer that waits for an operation (`*OPC?` after `:SINGle`) gets frames until it is there; the
                // bound is a number of frames, not a time
                let mut program = scpi::Program::new(&message);
                let mut answer = scpi.poll_program(ui, &cv, &mut program);
                for _ in 0..600 {
                    if answer.is_some() {
                        break;
                    }
                    frame(ui, &mut cv, &mut gpu, &mut gl, &mut shot_n);
                    answer = scpi.poll_program(ui, &cv, &mut program);
                }
                use std::io::Write;
                // the message, then its answer on lines of its own; an empty answer is an empty line
                let answer = answer.unwrap_or_else(|| b"(no answer after 600 frames)".to_vec());
                let mut out = format!("> {}\n", message).into_bytes();
                out.extend_from_slice(&answer);
                if answer.last() != Some(&b'\n') {
                    out.push(b'\n');
                }
                let file = std::fs::OpenOptions::new().create(true).append(true).open(&path);
                file.and_then(|mut f| f.write_all(&out)).expect("write scpi answers");
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
    ui.measure_in_background();
    let mut disp = open_display(&mut ui);
    let mut inp = input::Inputs::open(args);
    // "app UI loaded", sent once at the end of the start-up (board.md section 4)
    inp.send(&board::board().panel_cmd(board::PanelCmd::UiLoaded));
    // start-up screen (ui/startup.rs) while the acquisition hardware starts; the own picture is its background
    let start_up = arg(args, "--start-up");
    power_on_picture(&mut ui, &mut disp, start_up.as_deref());
    restore_settings(&mut ui);
    let mut d = Device::new(ui, disp, inp, start_up, args);
    while !d.ui.quit && !input::stop_requested() {
        d.pass();
    }
    d.close();
}

/// The panel, /dev/dri/card0; the layout follows its size. Without it oos-ui ends.
#[cfg(target_os = "linux")]
fn open_display(ui: &mut Ui) -> drm::Display {
    let disp = match drm::Display::open("/dev/dri/card0") {
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
    disp
}

/// The own picture: the background of the start-up screen (`--start-up`), else for 2 s at the first start of a boot.
#[cfg(target_os = "linux")]
fn power_on_picture(ui: &mut Ui, disp: &mut drm::Display, start_up: Option<&str>) {
    if start_up.is_some() {
        let mut cv = Canvas::new(ui::PANEL_W, ui::PANEL_H);
        let splash =
            SPLASH.iter().find_map(|p| std::fs::read(p).ok()).and_then(|b| store::ppm_into(&b, &mut cv)).map(|_| cv.px);
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
}

/// The settings of the last run in this boot, those of the scope itself and the SSH folder.
#[cfg(target_os = "linux")]
fn restore_settings(ui: &mut Ui) {
    // settings survive a restart of oos-ui within one boot (rcS restarts it on the hardware after the cold boot)
    let setup_path = std::path::Path::new(SETUP_FILE);
    if let Ok(t) = std::fs::read_to_string(setup_path) {
        // what this build cannot take is left out, the rest stands (store::parse_setup_lenient)
        let (m, bad) = store::parse_setup_lenient(&t);
        for e in &bad {
            eprintln!("oos-ui: {}: left out: {}", SETUP_FILE, e);
        }
        ui.m = m;
        ui.sync_full();
        eprintln!("oos-ui: settings restored from {}", SETUP_FILE);
    }
    // settings of the scope itself: NTP, time zone, Ethernet (/oos/system.conf)
    ui.load_system(sysconf::default_path());
    ui.set_ssh_dir(ssh::dir());
}

/// The frame in the GPU's work: its ticket, the panel buffer it fills, and the monitor's frame with its buffer.
#[cfg(target_os = "linux")]
type GpuPending = (gpu::Ticket, usize, Option<(gpu::Ticket, usize)>);

/// The frame loop of oos-ui on the scope: the screen, the panel and the monitor, the inputs, the network services and
/// the GPU, with the clocks of what it does now and then. The fields are in the reverse order of their making, so
/// they are dropped in the order the loop's variables were: the GPU helper before the display, `ui` last.
#[cfg(target_os = "linux")]
struct Device {
    start_up_t: std::time::Instant,
    gpu_pending: Option<GpuPending>,
    hdmi_gpu: Option<bool>,
    hdmi_base: usize,
    gl_head: Option<Canvas>,
    gl_view: GpuLayers,
    gl: GpuLayers,
    gpu_changed: bool,
    gpu_failed: bool,
    gpu_want: bool,
    gpu: Option<gpu::Gpu>,
    frame_n: u64,
    hdmi_t: Option<std::time::Instant>,
    hdmi_cv: Option<Canvas>,
    hdmi: Option<drm::Display>,
    hdmi_on: bool,
    mdns_t: Option<std::time::Instant>,
    /// when the ports of web and SCPI were last tried again (`remote_services`)
    listen_t: Option<std::time::Instant>,
    mdns_now: Option<mdns::Config>,
    mdns: mdns::Mdns,
    remote_now: Option<[bool; 5]>,
    hislip: hislip::Server<scpi::Program>,
    vxi: vxi11::Server<scpi::Program>,
    scpi: scpi::Scpi,
    web: site::Web,
    /// the live picture of the web remote (`/screen.png`), handed the screens of the frame loop
    live: shots::Live,
    scpi_port: u16,
    web_port: u16,
    scope: bool,
    eth_t: Option<std::time::Instant>,
    eth_t0: std::time::Instant,
    net_t: Option<std::time::Instant>,
    led_t: std::time::Instant,
    led_mask: u32,
    shot_n: u32,
    fps_n: i32,
    fps_t: std::time::Instant,
    last: std::time::Instant,
    cv: Canvas,
    setup_t: std::time::Instant,
    setup_last: String,
    start_up: Option<String>,
    inp: input::Inputs,
    disp: drm::Display,
    ui: Ui,
}

#[cfg(target_os = "linux")]
impl Device {
    /// The loop's state after the start-up phases: clocks, the network services as their switches and net.allow say,
    /// no monitor and no GPU yet.
    fn new(mut ui: Ui, disp: drm::Display, inp: input::Inputs, start_up: Option<String>, args: &[String]) -> Device {
        use std::time::Instant;
        let setup_last = store::setup_text(&ui.m);
        let setup_t = Instant::now();
        ui.info = std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|s| format!("Linux {}", s.trim()))
            .unwrap_or_default();
        let cv = Canvas::new(ui.scr.w, ui.scr.h);
        let last = Instant::now();
        let fps_t = Instant::now();
        let fps_n = 0;
        let shot_n = 0;
        let led_mask = u32::MAX;
        let led_t = Instant::now();
        let net_t: Option<Instant> = None;
        let eth_t0 = Instant::now();
        let eth_t: Option<Instant> = None;
        input::install_signal_handlers();
        // the scope accepts VXI-11, a computer HiSLIP instead; the scope announces itself by name (mdns.rs below)
        let scope = on_scope();
        let (web_port, scpi_port) = (lan_port(args, "--web-port", 8080), lan_port(args, "--scpi-port", 5555));
        // who may update or control the scope over the network: net.allow (docs/network.md 6), read here at the start;
        // the servers share the list with Remote services > Allowed, which sets it after a change and runs the firewall
        // again
        let acl = ui.acl();
        acl.set(netacl::Acl::load(&sysconf::default_path()));
        if std::path::Path::new(remote::FIREWALL).exists() {
            ui.set_allow_runner(remote::script_runner());
        }
        let (web, scpi, vxi, hislip) = lan_servers(scope, args, &acl);
        // System > Network > Remote services, as applied: web, SCPI, VXI-11, SSH and its password login (None: not yet,
        // at the start)
        let remote_now: Option<[bool; 5]> = None;
        // the name on the network and the mDNS responder (mdns.rs), as applied; looked at once a second
        let mdns = mdns::Mdns::new(None);
        let mdns_now: Option<mdns::Config> = None;
        let mdns_t: Option<Instant> = None;
        let listen_t: Option<Instant> = None;
        // HDMI mirror: a monitor is looked for every 3 s and gets the panel picture at 15 frames/s (`--no-hdmi`: off)
        let hdmi_on = !args.iter().any(|a| a == "--no-hdmi");
        let hdmi: Option<drm::Display> = None;
        // the monitor's own picture (Display > HDMI = Waveform or Layout)
        let hdmi_cv: Option<Canvas> = None;
        let hdmi_t: Option<Instant> = None;
        let frame_n = 0u64;
        // System > Graphics = GPU: the helper, the layers of the panel and of the monitor's own layout, the monitor's
        // settings line; `gpu_failed`: the GPU did not start or failed, the CPU draws until the setting changes
        let gpu: Option<gpu::Gpu> = None;
        let (gpu_want, gpu_failed) = (false, false);
        // the GPU's start is told on screen only when the setting was changed, not at every power-on (GPU is the
        // default)
        let gpu_changed = false;
        let gl = GpuLayers::new(ui.scr.w, ui.scr.h);
        let gl_view = GpuLayers::new(1, 1);
        let gl_head: Option<Canvas> = None;
        // the monitor's buffers are the targets after the panel's (`start_gpu`: one per panel buffer, three). They were
        // 2 to 4 while the panel had two: the monitor's first buffer took the place of the panel's third, the GPU drew
        // the panel's picture into it, and the panel showed an old picture in its turn (both screens flickered with
        // HDMI and Graphics = GPU). None not yet, Some(false) they could not be taken
        let hdmi_base = disp.buffers();
        let hdmi_gpu: Option<bool> = None;
        let gpu_pending: Option<GpuPending> = None;
        // the first read at once (a complete start-up shows no screen at all)
        let start_up_t = Instant::now().checked_sub(std::time::Duration::from_millis(300)).unwrap_or_else(Instant::now);
        Device {
            start_up_t,
            gpu_pending,
            hdmi_gpu,
            hdmi_base,
            gl_head,
            gl_view,
            gl,
            gpu_changed,
            gpu_failed,
            gpu_want,
            gpu,
            frame_n,
            hdmi_t,
            hdmi_cv,
            hdmi,
            hdmi_on,
            mdns_t,
            listen_t,
            mdns_now,
            mdns,
            remote_now,
            hislip,
            vxi,
            scpi,
            web,
            live: Default::default(),
            scpi_port,
            web_port,
            scope,
            eth_t,
            eth_t0,
            net_t,
            led_t,
            led_mask,
            shot_n,
            fps_n,
            fps_t,
            last,
            cv,
            setup_t,
            setup_last,
            start_up,
            inp,
            disp,
            ui,
        }
    }

    /// One pass of the loop: the start-up file, the inputs and the network services; a frame when one is due.
    fn pass(&mut self) {
        self.start_up_file();
        self.inputs();
        self.remote_services();
        self.mdns_names();
        self.serve_lan();
        if self.last.elapsed().as_secs_f64() < 1.0 / 30.0 {
            return;
        }
        self.frame();
    }

    /// The start-up file, every 250 ms while the screen is up.
    fn start_up_file(&mut self) {
        use std::time::Instant;
        if let Some(p) = self
            .start_up
            .as_deref()
            .filter(|_| self.ui.start_up_active() && self.start_up_t.elapsed().as_millis() >= 250)
        {
            self.start_up_t = Instant::now();
            let up = std::fs::read_to_string("/proc/uptime")
                .ok()
                .and_then(|s| s.split_whitespace().next().and_then(|t| t.parse().ok()))
                .unwrap_or(0.0);
            self.ui.start_up_update(&std::fs::read_to_string(p).unwrap_or_default(), up);
        }
    }

    /// The inputs (waits up to the next frame).
    fn inputs(&mut self) {
        let elapsed = self.last.elapsed().as_secs_f64();
        let wait_ms = ((1.0 / 30.0 - elapsed) * 1000.0).max(0.0) as i32;
        for line_or_ev in self.inp.poll(wait_ms) {
            match line_or_ev {
                input::In::Ev(ev) => self.ui.handle(ev),
                input::In::Hid(e) => {
                    // a key that stands for a front-panel key: its command, as from the control FIFO
                    if let Some(c) = self.ui.hid(e) {
                        cmd::run(&mut self.ui, c);
                    }
                }
                input::In::Cmd(line) => {
                    if let cmd::Out::Shot(p) = cmd::run(&mut self.ui, &line) {
                        self.ui.shot_request = Some(p);
                    }
                }
            }
        }
    }

    /// The ports the web page offers to the browser (SCPI, VXI-11, HiSLIP), as they are open now.
    fn offer_ports(&mut self) {
        self.web.offer(http::Offer {
            scpi_port: self.scpi.port(),
            vxi11: self.vxi.service_port().is_some(),
            hislip_port: self.hislip.port(),
        });
    }

    /// The services follow their switches: the ports were opened at the start, SSH (oos-ssh) is started by rcS.
    fn remote_services(&mut self) {
        let ui = &self.ui;
        let want = [ui.sys.remote_web, ui.sys.remote_scpi, ui.sys.remote_vxi11, ui.sys.remote_ssh, ui.sys.ssh_password];
        if self.remote_now != Some(want) {
            let was = self.remote_now.unwrap_or([true, true, true, want[3], want[4]]);
            if want[0] != was[0] {
                self.web.set_open(want[0].then_some(self.web_port));
            }
            if want[1] != was[1] {
                self.scpi.set_open(want[1].then_some(self.scpi_port));
            }
            // VXI-11 needs port 111, which only the scope has free
            if want[2] != was[2] && self.scope {
                self.vxi.set_open(want[2]);
            }
            if want[3] != was[3] || want[4] != was[4] {
                ssh::apply(want[3]);
            }
            self.offer_ports();
            self.remote_now = Some(want);
        }
        // a port the oos-ui before this one still held at the start is tried again once a second, while its switch
        // is on: before, SCPI and the web page stayed off until the next start (#212)
        if self.listen_t.is_none_or(|t| t.elapsed().as_secs() >= 1) {
            self.listen_t = Some(std::time::Instant::now());
            let web = want[0] && self.web.listen_again(self.web_port);
            let scpi = want[1] && self.scpi.listen_again(self.scpi_port);
            if web || scpi {
                self.offer_ports();
            }
        }
    }

    /// The name on the network and the mDNS services, once a second.
    fn mdns_names(&mut self) {
        use std::time::Instant;
        if self.mdns_t.is_none_or(|t| t.elapsed().as_secs() >= 1) {
            self.mdns_t = Some(Instant::now());
            let c = mdns_services(
                self.ui.mdns_config(self.web_port, self.scpi_port),
                self.web.port(),
                self.vxi.service_port(),
                self.hislip.port(),
            );
            if self.mdns_now.as_ref() != Some(&c) {
                if self.mdns_now.as_ref().is_none_or(|o| o.name != c.name) {
                    hostname::set(&c.name);
                }
                self.mdns.set(c.clone());
                self.mdns_now = Some(c);
            }
        }
    }

    /// SCPI, VXI-11, HiSLIP and the web server.
    fn serve_lan(&mut self) {
        self.scpi.poll(&mut self.ui, &self.cv);
        self.vxi.poll_async(&mut |program| self.scpi.poll_program(&mut self.ui, &self.cv, program));
        self.hislip.poll_async(&mut |program| self.scpi.poll_program(&mut self.ui, &self.cv, program));
        for c in self.web.poll(&mut site::Ctx::new(&mut self.ui, &self.cv, &mut self.scpi).with_live(&self.live)) {
            if let cmd::Out::Shot(p) = cmd::run(&mut self.ui, &c) {
                self.ui.shot_request = Some(p);
            }
        }
    }

    /// A frame: the step of the screen, its picture on the GPU or the CPU, the monitor's, then the chores.
    fn frame(&mut self) {
        use std::time::Instant;
        let dt = self.last.elapsed().as_secs_f64().min(0.2);
        self.last = Instant::now();
        let t0 = Instant::now();
        self.ui.step(dt);
        let t1 = Instant::now();
        self.ui.set_hdmi_present(self.hdmi.is_some());
        let hdmi_frame = self.hdmi.is_some() && (self.frame_n + 1).is_multiple_of(2);
        self.ui.hdmi_snap_want = hdmi_frame && self.ui.m.hdmi == model::HdmiMode::Waveform;
        self.gpu_follow();
        // the web page's picture: the screen the CPU draws next swaps out the shown one; with the GPU, `cv` changes
        // only when a picture comes back, and each one goes to the web page from there
        if self.gpu.is_some() {
            self.live.before_gpu_frame();
        } else {
            self.live.before_render(&mut self.cv, !self.ui.start_up_active());
        }
        let (drawn, hdmi_done) = self.gpu_frame(hdmi_frame);
        if !drawn {
            (self.ui.times.gpu, self.ui.times.gpu_screens) = (None, None);
            self.ui.render(&mut self.cv);
        }
        let t2 = Instant::now();
        if !drawn {
            self.disp.present(&self.cv);
        }
        self.frame_n += 1;
        self.hdmi_picture(hdmi_frame, hdmi_done);
        let t3 = Instant::now();
        self.hdmi_watch();
        self.ui.hdmi_out = self.hdmi.as_ref().map(|h| (h.width as u32, h.height as u32));
        let a = 0.05;
        self.ui.times.step += a * ((t1 - t0).as_secs_f32() * 1000.0 - self.ui.times.step);
        self.ui.times.render += a * ((t2 - t1).as_secs_f32() * 1000.0 - self.ui.times.render);
        self.ui.times.present += a * ((t3 - t2).as_secs_f32() * 1000.0 - self.ui.times.present);
        self.chores();
    }

    /// System > Graphics: the helper follows the setting.
    fn gpu_follow(&mut self) {
        if self.ui.sys.gpu != self.gpu_want {
            self.gpu_changed = self.frame_n > 0;
            (self.gpu_want, self.gpu_failed) = (self.ui.sys.gpu, false);
            if self.gpu.take().is_some() {
                eprintln!("oos-ui: GPU off");
            }
            (self.hdmi_gpu, self.gpu_pending) = (None, None);
        }
        if self.gpu_want && self.gpu.is_none() && !self.gpu_failed {
            match start_gpu(&self.disp) {
                Ok(g) => {
                    eprintln!("oos-ui: GPU on");
                    if self.gpu_changed {
                        self.ui.toast("Graphics: GPU");
                    }
                    self.gpu = Some(g);
                }
                Err(e) => {
                    self.gpu_failed = true;
                    eprintln!("oos-ui: GPU: {}", e);
                    if self.gpu_changed {
                        self.ui.toast(&format!("No GPU ({}): drawing on the CPU", e));
                    }
                }
            }
            self.hdmi_gpu = None;
        }
    }

    /// The frame on the GPU when it runs: (drawn, the monitor's picture done); a failure hands the drawing back to
    /// the CPU.
    fn gpu_frame(&mut self, hdmi_frame: bool) -> (bool, bool) {
        use std::time::Instant;
        let mut drawn = false;
        let mut hdmi_done = false;
        if let Some(g) = self.gpu.as_mut() {
            let r: Result<(), String> = (|| {
                // the picture comes back only for someone who wants it: the web page, a screenshot, and twice a
                // second for SCPI and the rest
                let shot = self.ui.shot_request.is_some();
                // and every frame for a monitor that gets the panel's picture from the CPU (its buffers could not be
                // given to the GPU), else it would show a picture up to half a second old
                let hdmi_cpu = self.hdmi.is_some() && self.hdmi_gpu == Some(false);
                let read = shot || self.live.picture_wanted() || self.frame_n.is_multiple_of(15) || hdmi_cpu;
                // 1. this frame's layers, while the GPU still puts the last one together
                let tl = Instant::now();
                self.gl.draw(&mut self.ui, self.cv.w, self.cv.h, false);
                let layers = tl.elapsed().as_secs_f32() * 1e3;
                // 2. the last frame: shown once it is done (its picture into `cv` when it was asked for)
                let mut wait = 0.0;
                if let Some((tp, bp, th)) = self.gpu_pending.take() {
                    let back = tp.reads();
                    g.collect(tp, Some(&mut self.cv))?;
                    if back {
                        self.live.picture_read_back(&self.cv);
                    }
                    wait += g.times.wait;
                    self.disp.flip(bp);
                    if let Some((t2, b2)) = th {
                        g.collect(t2, None)?;
                        if let Some(h) = self.hdmi.as_mut() {
                            h.flip_scaled(b2);
                        }
                    }
                }
                // the monitor's buffers for the GPU, once per monitor (a message that is answered at once: only
                // with no frame in work)
                if let (Some(h), None) = (self.hdmi.as_ref(), self.hdmi_gpu) {
                    self.hdmi_gpu = Some(hdmi_targets(g, h, self.hdmi_base)?);
                }
                // 3. this frame to the GPU (into the buffer neither shown nor waiting to be); a screenshot waits for
                // it, so it is of this frame
                let b = self.disp.free();
                let full = gfx::Rect::new(0, 0, self.cv.w, self.cv.h);
                let tp = g.submit(b, None, &[self.gl.quad(&self.ui, full, full, false, false)], read)?;
                let copy = g.times.copy;
                let th = match (self.hdmi.as_ref(), self.hdmi_gpu, hdmi_frame) {
                    (Some(h), Some(true), true) => {
                        let b2 = h.free();
                        Some((
                            hdmi_gpu_frame(
                                &mut self.ui,
                                g,
                                &self.gl,
                                &mut self.gl_view,
                                &mut self.gl_head,
                                (h.width, h.height),
                                (self.hdmi_base + b2, false),
                            )?,
                            b2,
                        ))
                    }
                    _ => None,
                };
                hdmi_done = th.is_some() || (self.hdmi_gpu == Some(true) && hdmi_frame);
                if shot {
                    g.collect(tp, Some(&mut self.cv))?;
                    self.live.picture_read_back(&self.cv);
                    self.disp.flip(b);
                    if let Some((t2, b2)) = th {
                        g.collect(t2, None)?;
                        if let Some(h) = self.hdmi.as_mut() {
                            h.flip_scaled(b2);
                        }
                    }
                } else {
                    self.gpu_pending = Some((tp, b, th));
                }
                let t = g.times;
                self.ui.times.gpu = Some([layers, copy, wait, t.upload, t.draw, t.read]);
                self.ui.times.gpu_screens =
                    Some((t.panel, (self.hdmi.is_some() && self.hdmi_gpu == Some(true)).then_some(t.monitor)));
                Ok(())
            })();
            match r {
                Ok(()) => drawn = true,
                Err(e) => {
                    (self.gpu, self.gpu_failed, self.hdmi_gpu, self.gpu_pending, hdmi_done) =
                        (None, true, None, None, false);
                    eprintln!("oos-ui: GPU frame: {}", e);
                    self.ui.toast("GPU failed: drawing on the CPU");
                }
            }
        }
        (drawn, hdmi_done)
    }

    /// The monitor's picture from the CPU, when the GPU did not make it: its own layout, the workspace, or the panel's.
    fn hdmi_picture(&mut self, hdmi_frame: bool, hdmi_done: bool) {
        if let Some(h) = self.hdmi.as_mut().filter(|_| hdmi_frame && !hdmi_done) {
            // the monitor's picture: its own layout, the workspace, or the panel's
            let size = match self.ui.m.hdmi {
                model::HdmiMode::Layout => ui::Screen::for_monitor(h.width, h.height),
                model::HdmiMode::Waveform if !self.ui.hdmi_snap.is_empty() => {
                    Some(ui::Screen { w: h.width, h: h.height })
                }
                _ => None,
            };
            match size {
                Some(s) => {
                    if self.hdmi_cv.as_ref().is_none_or(|c| (c.w, c.h) != (s.w, s.h)) {
                        self.hdmi_cv = Some(Canvas::new(s.w, s.h));
                    }
                    let hc = self.hdmi_cv.as_mut().unwrap();
                    if self.ui.m.hdmi == model::HdmiMode::Layout {
                        self.ui.render_view(hc);
                    } else {
                        hdmi_workspace(&self.ui, hc);
                    }
                    h.present_scaled(hc);
                }
                None => h.present_scaled(&self.cv),
            }
        }
    }

    /// A monitor is looked for every 3 s (`--no-hdmi`: never); one that was unplugged is closed.
    fn hdmi_watch(&mut self) {
        use std::time::Instant;
        if self.hdmi_on && self.hdmi_t.is_none_or(|t| t.elapsed().as_secs() >= 3) {
            self.hdmi_t = Some(Instant::now());
            match self.hdmi.take() {
                Some(h) if !h.connected() => {
                    eprintln!("oos-ui: HDMI disconnected");
                    h.close();
                    self.hdmi_cv = None;
                    // a frame of it still in work is collected, not shown
                    self.hdmi_gpu = None;
                }
                Some(h) => self.hdmi = Some(h),
                None => match self.disp.hdmi() {
                    Ok(h) => {
                        eprintln!("oos-ui: HDMI {}x{}", h.width, h.height);
                        self.hdmi = Some(h);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        // e.g. a mode the second VOP refuses: try again in 30 s, not every 3 s
                        eprintln!("oos-ui: HDMI: {}", e);
                        self.hdmi_t = Some(Instant::now() + std::time::Duration::from_secs(27));
                    }
                },
            }
        }
    }

    /// After each frame: LEDs, a screenshot, panel commands, power, the settings file, the network, the 10 Mbit/s
    /// watch and the frame rate.
    fn chores(&mut self) {
        use board::Board;
        use std::time::Instant;
        // LEDs: on change, and refreshed every 2 s (the MCU may have been reset)
        let lm = self.ui.leds();
        if lm != self.led_mask || self.led_t.elapsed().as_secs() >= 2 {
            self.inp.set_leds(lm);
            self.led_mask = lm;
            self.led_t = Instant::now();
        }
        if let Some(p) = self.ui.shot_request.take() {
            save_shot(&mut self.ui, &self.cv, p, &mut self.shot_n);
        }
        for c in self.ui.panel_tx.drain(..) {
            self.inp.send(&board::board().panel_cmd(c));
        }
        if let Some(a) = self.ui.power_action.take() {
            power(&mut self.inp, a);
        }
        self.save_setup();
        if self.net_t.is_none_or(|t| t.elapsed().as_secs() >= 5) {
            let ui = &mut self.ui;
            let (first, all) = ipv4_addresses();
            (ui.net_if, ui.net) = first;
            ui.net_ifs = all;
            ui.net_fixed = std::path::Path::new("/tmp/oos-net.fallback").exists();
            ui.eth_link = std::fs::read_to_string("/sys/class/net/eth0/carrier").is_ok_and(|c| c.trim() == "1");
            self.net_t = Some(Instant::now());
        }
        if self.eth_t.is_none_or(|t| t.elapsed().as_secs() >= 1) {
            self.eth_t = Some(Instant::now());
            eth_watch(&mut self.ui, self.eth_t0.elapsed().as_secs_f64());
        }
        self.fps_n += 1;
        if self.fps_t.elapsed().as_secs_f64() >= 1.0 {
            self.ui.fps = self.fps_n as f32 / self.fps_t.elapsed().as_secs_f32();
            self.fps_n = 0;
            self.fps_t = Instant::now();
        }
    }

    /// The settings to SETUP_FILE every 2 s when they changed, and to OOS's own data partition.
    fn save_setup(&mut self) {
        use std::time::Instant;
        let setup_path = std::path::Path::new(SETUP_FILE);
        if self.setup_t.elapsed().as_secs() >= 2 {
            self.setup_t = Instant::now();
            let t = store::setup_text(self.ui.setup_model());
            if t != self.setup_last {
                let tmp = format!("{}.new", SETUP_FILE);
                if std::fs::write(&tmp, &t).and_then(|_| std::fs::rename(&tmp, setup_path)).is_ok() {
                    // and across power cycles on OOS's own data partition (rcS copies it back to /tmp at boot)
                    if std::path::Path::new("/oos/setups").is_dir() {
                        let _ = std::fs::write("/oos/oos-ui.setup.new", &t)
                            .and_then(|_| std::fs::rename("/oos/oos-ui.setup.new", "/oos/oos-ui.setup"));
                    }
                    self.setup_last = t;
                }
            }
        }
    }

    /// Goodbyes on the network (SIGTERM, quit), LEDs off, the monitor closed and the panel restored.
    fn close(mut self) {
        // goodbyes on the network (SIGTERM, quit)
        self.mdns.stop();
        self.inp.set_leds(0);
        if let Some(h) = self.hdmi.take() {
            h.close();
        }
        self.disp.restore();
    }
}

/// The monitor's buffers as GPU targets from `base` on; false when one could not be taken.
#[cfg(target_os = "linux")]
fn hdmi_targets(g: &mut gpu::Gpu, h: &drm::Display, base: usize) -> Result<bool, String> {
    let mut ok = true;
    for i in 0..h.buffers() {
        let (fd, pitch) = h.export(i).map_err(|e| format!("HDMI export: {}", e))?;
        ok &= g
            .target_dmabuf(base + i, h.width, h.height, pitch, fd)
            .map_err(|e| eprintln!("oos-ui: GPU HDMI: {}", e))
            .is_ok();
    }
    Ok(ok)
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
            eprintln!(
                "oos-ui: {} after the restart: {} {} ({})",
                ethlink::IFACE,
                s,
                ethlink::duplex(),
                ethlink::mii_regs()
            );
        }
        None => {}
    }
}

/// The HDMI workspace: a settings line on top, below it the waveform area of the panel (plot and measurement rows,
/// without the bars and menus) scaled to the monitor with its aspect ratio.
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

/// The address to show in the top bar, and all of them, as (interface, address).
#[cfg(target_os = "linux")]
fn ipv4_addresses() -> ((String, String), Vec<(String, String)>) {
    let mut found = Vec::new();
    unsafe {
        let mut ifa: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifa) != 0 {
            return ((String::new(), String::new()), Vec::new());
        }
        let mut p = ifa;
        while !p.is_null() {
            let a = &*p;
            let up = a.ifa_flags & (libc::IFF_UP as u32) != 0 && a.ifa_flags & (libc::IFF_LOOPBACK as u32) == 0;
            if up && !a.ifa_addr.is_null() && (*a.ifa_addr).sa_family as i32 == libc::AF_INET {
                let link = a.ifa_flags & (libc::IFF_RUNNING as u32) != 0;
                let sin = &*(a.ifa_addr as *const libc::sockaddr_in);
                let name = std::ffi::CStr::from_ptr(a.ifa_name).to_string_lossy().into_owned();
                found.push((name, std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)), link));
            }
            p = a.ifa_next;
        }
        libc::freeifaddrs(ifa);
    }
    pick_addresses(found)
}

/// Orders the IPv4 addresses (interface, address, link) for the screen: an interface's link-local address
/// (169.254/16, what eth0 keeps when no DHCP server answers) after its others, so a page that shows one address per
/// interface shows the one set by DHCP or by hand. The top bar takes an interface with a link first (eth0 keeps its
/// fallback address without a cable), then an address that is not link-local, then the first.
fn pick_addresses(mut v: Vec<(String, std::net::Ipv4Addr, bool)>) -> ((String, String), Vec<(String, String)>) {
    v.sort_by_key(|a| a.1.is_link_local());
    let best = v.iter().enumerate().max_by_key(|(i, a)| (a.2, !a.1.is_link_local(), std::cmp::Reverse(*i)));
    let first = best.map(|(_, a)| (a.0.clone(), a.1.to_string())).unwrap_or_default();
    (first, v.into_iter().map(|a| (a.0, a.1.to_string())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// eth0 with the fallback address it took before the hand-set one (seen on a DHO924S, 2026-11-16): both the top
    /// bar and the Network page show the hand-set one.
    #[test]
    fn a_link_local_address_comes_after_the_others() {
        let a = |n: &str, ip: &str, link: bool| (n.to_string(), ip.parse::<std::net::Ipv4Addr>().unwrap(), link);
        let s = |n: &str, ip: &str| (n.to_string(), ip.to_string());
        let (first, all) = pick_addresses(vec![a("eth0", "169.254.1.1", true), a("eth0", "192.0.2.51", true)]);
        assert_eq!(first, s("eth0", "192.0.2.51"));
        assert_eq!(all, vec![s("eth0", "192.0.2.51"), s("eth0", "169.254.1.1")]);
        // only a link-local address: it is still shown
        assert_eq!(pick_addresses(vec![a("eth0", "169.254.1.2", true)]).0, s("eth0", "169.254.1.2"));
        // eth0 without a cable keeps its address, the WLAN with a link wins
        let v = vec![a("eth0", "192.168.1.20", false), a("wlan0", "192.168.177.64", true)];
        assert_eq!(pick_addresses(v).0, s("wlan0", "192.168.177.64"));
        // with a link on both: the address that is not link-local, else the first
        let v = vec![a("eth0", "169.254.9.9", true), a("wlan0", "192.168.177.64", true)];
        assert_eq!(pick_addresses(v).0, s("wlan0", "192.168.177.64"));
        let v = vec![a("eth0", "10.0.0.2", true), a("wlan0", "192.168.177.64", true)];
        assert_eq!(pick_addresses(v).0, s("eth0", "10.0.0.2"));
        assert_eq!(pick_addresses(Vec::new()).0, (String::new(), String::new()));
    }

    /// The HDMI workspace on a 1920 x 1080 monitor: settings line on top, the waveform area below without menus.
    #[test]
    fn hdmi_workspace_picture() {
        let mut ui = crate::ui::testkit::demo_ui();
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

// at the end, so the lines of the program keep their numbers (tools/scope-binaries.sh)
#[cfg(test)]
mod hostile;
