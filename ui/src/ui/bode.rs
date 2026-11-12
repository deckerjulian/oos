// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Bode plot with the built-in generator (S models, `Board::gen`) or an external generator on the LAN:
//! OOS sends SCPI to the generator (presets for Rigol DG, Siglent SDG and plain SCPI), steps a logarithmic sweep, sets the time base to about 5 periods per screen,
//! measures gain and phase of the output channel against the input channel by a sine fit on every captured sample
//! (`oos_dsp::bode`) and draws both curves. The generator connection runs in its own thread. With the built-in
//! generator and input = output channel the gain is against the set amplitude and there is no phase.

use super::*;
use oos_dsp::bode;
use std::sync::mpsc;

/// Generator presets: name, command template (`{f}` Hz, `{a}` Vpp), output on. Entry `BUILTIN` is the scope's own.
pub const GENERATORS: [(&str, &str, &str); 4] = [
    ("Built-in", "", ""),
    ("Rigol DG", ":SOUR1:APPL:SIN {f},{a},0", ":OUTP1 ON"),
    ("Siglent SDG", "C1:BSWV WVTP,SINE,FRQ,{f},AMP,{a},OFST,0", "C1:OUTP ON"),
    ("SCPI", "FREQ {f};VOLT {a}", "OUTP ON"),
];

#[derive(Default)]
pub struct State {
    pub running: bool,
    pub freqs: Vec<f64>,
    /// (frequency, gain dB, phase degrees)
    pub points: Vec<(f64, f64, f64)>,
    /// index of the point under the cursor
    pub cursor: usize,
    pub idx: usize,
    /// records to skip after a frequency step (generator and time base settle), counted from the first record of
    /// the step (`stamp`) on
    pub wait: u32,
    /// `Model::hw_stamp` of the current step: hardware records with an older stamp are from before it
    pub stamp: u64,
    pub tx: Option<mpsc::Sender<String>>,
    pub err: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// the user's time base, trigger source and level, restored at the end
    pub saved: Option<(usize, usize, f32)>,
    /// the built-in generator's settings before the sweep (on, Hz, Vpp, V), restored at the end
    pub afg_saved: Option<(bool, f64, f64, f64, usize, usize)>,
}

pub const BUILTIN: usize = 0;

/// The generator thread: connects, sends every line it receives (newline-terminated).
fn gen_thread(addr: String, err: std::sync::Arc<std::sync::Mutex<Option<String>>>) -> mpsc::Sender<String> {
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::Write;
        let target = if addr.contains(':') { addr.clone() } else { format!("{}:5555", addr) };
        let sock = target.parse::<std::net::SocketAddr>().map_err(|e| e.to_string()).and_then(|a| std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_secs(2)).map_err(|e| e.to_string()));
        let mut s = match sock {
            Ok(s) => s,
            Err(e) => {
                *err.lock().unwrap() = Some(format!("generator {}: {}", target, e));
                return;
            }
        };
        for line in rx {
            if let Err(e) = s.write_all(format!("{}\n", line).as_bytes()) {
                *err.lock().unwrap() = Some(format!("generator {}: {}", target, e));
                return;
            }
        }
    });
    tx
}

const BODE_MODES: [oos_dsp::bode::Mode; 3] = [oos_dsp::bode::Mode::Loop, oos_dsp::bode::Mode::Psrr, oos_dsp::bode::Mode::Z];

fn bode_mode_name(m: oos_dsp::bode::Mode) -> &'static str {
    match m {
        oos_dsp::bode::Mode::Loop => "Loop",
        oos_dsp::bode::Mode::Psrr => "PSRR",
        oos_dsp::bode::Mode::Z => "Z",
    }
}

impl Ui {
    pub(super) fn bode_items(&self) -> Vec<Item> {
        let mut v = vec![Item::BodeOn, Item::BodeMode, Item::BodeRun, Item::BodeStart, Item::BodeStop, Item::BodePoints, Item::BodeAmp, Item::BodeIn, Item::BodeOut, Item::BodeGen];
        // a cursor once there is something to read
        if !self.bode.points.is_empty() {
            v.push(Item::BodeCur);
        }
        // the address only for a generator on the network
        if self.bode_gen() != 0 {
            v.push(Item::BodeAddr);
        }
        v
    }

    pub(super) fn bode_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::BodeStart | Item::BodeStop | Item::BodePoints | Item::BodeAmp | Item::BodeAddr | Item::BodeCur => Kind::Value,
            Item::BodeRun => Kind::Action,
            Item::BodeOn | Item::BodeIn | Item::BodeOut | Item::BodeGen | Item::BodeMode => Kind::Choice,
            _ => return None,
        })
    }

    pub(super) fn bode_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Bode) => "Bode",
            Item::BodeOn => "View",
            Item::BodeMode => "Mode",
            Item::BodeRun => "Start / stop sweep",
            Item::BodeStart => "Start freq",
            Item::BodeStop => "Stop freq",
            Item::BodePoints => "Points",
            Item::BodeAmp => "Amplitude",
            Item::BodeIn => "Input (ref)",
            Item::BodeOut => "Output",
            Item::BodeGen => "Generator",
            Item::BodeCur => "Cursor",
            Item::BodeAddr => "Gen address",
            _ => return None,
        })
    }

    pub(super) fn bode_value(&self, it: Item) -> Option<String> {
        let m = &self.m;
        Some(match it {
            Item::BodeOn => if m.view == AView::Bode { "On".into() } else { "Off".into() },
            Item::BodeMode => bode_mode_name(m.bode_mode).into(),
            Item::BodeRun => String::new(),
            Item::BodeStart => si_short(m.bode_start, "Hz"),
            Item::BodeStop => si_short(m.bode_stop, "Hz"),
            Item::BodePoints => m.bode_points.to_string(),
            Item::BodeAmp => si_short(m.bode_amp, "Vpp"),
            Item::BodeIn => self.wave_name(m.bode_in),
            Item::BodeOut => self.wave_name(m.bode_out),
            Item::BodeGen => GENERATORS[m.bode_gen.min(GENERATORS.len() - 1)].0.into(),
            Item::BodeAddr => m.bode_addr.clone(),
            Item::BodeCur => self.bode_cursor_text(),
            _ => return None,
        })
    }

    pub(super) fn bode_options(&self, it: Item) -> Option<Vec<String>> {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect();
        Some(match it {
            Item::BodeOn => v(&["Off", "On"]),
            Item::BodeMode => BODE_MODES.iter().map(|m| bode_mode_name(*m).to_string()).collect(),
            Item::BodeIn | Item::BodeOut => self.wave_names(),
            Item::BodeGen => GENERATORS.iter().map(|g| g.0.to_string()).collect(),
            _ => return None,
        })
    }

    pub(super) fn bode_adjust(&mut self, it: Item, d: i32) -> bool {
        if matches!(it, Item::BodeIn | Item::BodeOut) {
            let cur = if it == Item::BodeIn { self.m.bode_in } else { self.m.bode_out };
            let next = self.step_wave(cur, d);
            if it == Item::BodeIn { self.m.bode_in = next; } else { self.m.bode_out = next; }
            return true;
        }
        let m = &mut self.m;
        let ladder = |x: f64, d: i32| {
            let l: Vec<f64> = (0..10).flat_map(|e| [1.0, 2.0, 5.0].map(|k| k * 10f64.powi(e))).collect();
            let i = l.iter().position(|&v| v >= x * 0.999).unwrap_or(l.len() - 1);
            l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize]
        };
        match it {
            Item::BodeOn => m.view = if m.view == AView::Bode { AView::Scope } else { AView::Bode },
            Item::BodeMode => {
                let i = BODE_MODES.iter().position(|&x| x == m.bode_mode).unwrap_or(0);
                let next = BODE_MODES[(i as i32 + d).rem_euclid(BODE_MODES.len() as i32) as usize];
                if next != m.bode_mode {
                    m.bode_mode = next;
                    self.bode.points.clear();
                    self.bode.cursor = 0;
                }
            }
            Item::BodeStart => m.bode_start = ladder(m.bode_start, d).min(m.bode_stop / 2.0),
            Item::BodeStop => m.bode_stop = ladder(m.bode_stop, d).max(m.bode_start * 2.0),
            Item::BodePoints => m.bode_points = (m.bode_points as i32 + 5 * d).clamp(5, 200) as usize,
            Item::BodeAmp => m.bode_amp = ladder(m.bode_amp, d).clamp(0.01, 20.0),
            Item::BodeGen => m.bode_gen = (m.bode_gen as i32 + d).rem_euclid(GENERATORS.len() as i32) as usize,
            Item::BodeCur => {
                let n = self.bode.points.len() as i32;
                self.bode.cursor = (self.bode.cursor as i32 + d).clamp(0, (n - 1).max(0)) as usize;
            }
            Item::BodeAddr => {
                // the knob moves the last octet of an IPv4 address
                let (head, tail) = m.bode_addr.rsplit_once('.').unwrap_or(("192.168.1", "100"));
                let (oct, port) = tail.split_once(':').map_or((tail, None), |(o, p)| (o, Some(p)));
                let o = (oct.parse::<i32>().unwrap_or(100) + d).clamp(1, 254);
                m.bode_addr = match port {
                    Some(p) => format!("{}.{}:{}", head, o, p),
                    None => format!("{}.{}", head, o),
                };
            }
            _ => return false,
        }
        true
    }

    pub(super) fn bode_activate(&mut self, it: Item) -> bool {
        if it != Item::BodeRun {
            return false;
        }
        if self.bode.running {
            self.bode_finish("Bode sweep stopped");
        } else {
            self.bode_begin();
        }
        true
    }

    fn bode_gen(&self) -> usize {
        self.m.bode_gen.min(GENERATORS.len() - 1)
    }

    fn bode_send_freq(&mut self) {
        let f = self.bode.freqs[self.bode.idx];
        if self.bode_gen() == BUILTIN {
            self.afg_set(Some(true), Some(f), Some(self.m.bode_amp), Some(0.0));
        } else {
            let g = GENERATORS[self.bode_gen()];
            let cmd = g.1.replace("{f}", &bode::fmt_hz(f)).replace("{a}", &bode::fmt_hz(self.m.bode_amp));
            if let Some(tx) = &self.bode.tx {
                let _ = tx.send(cmd);
            }
        }
        // about 5 periods on the screen
        let want = bode::tdiv_for(f, 5.0) as f32;
        let ts = tscales();
        self.m.tscale = ts.iter().position(|&t| t >= want * 0.999).unwrap_or(ts.len() - 1);
        self.bode.wait = 2;
        self.m.hw_stamp += 1;
        self.bode.stamp = self.m.hw_stamp;
        self.settings_changed();
    }

    pub(super) fn bode_begin(&mut self) {
        if !self.bode_sources_are_refs() && self.bode_gen() == BUILTIN && !oos_hal::model::platform().caps.has_gen() {
            let name = &oos_hal::model::platform().caps.name;
            return self.toast(&format!("No built-in generator on the {}: choose an external one", name));
        }
        self.m.view = AView::Bode;
        self.bode.freqs = bode::log_sweep(self.m.bode_start, self.m.bode_stop, self.m.bode_points);
        self.bode.points.clear();
        self.bode.idx = 0;
        *self.bode.err.lock().unwrap() = None;
        // two stored records: the sweep is the sine fit of those samples, with no generator and no front end
        if self.bode_sources_are_refs() {
            let rec = Self::blank_record();
            let freqs = self.bode.freqs.clone();
            for &f in &freqs {
                let _ = self.bode_point(&rec, f);
            }
            self.bode.idx = self.bode.freqs.len();
            self.toast(&format!("Bode sweep done: {} points", self.bode.points.len()));
            return;
        }
        self.bode.saved = Some((self.m.tscale, self.m.trig_src, self.m.trig_level));
        // trigger on a live input at its centre, free running if nothing triggers
        let (inp, out) = (self.m.bode_in, self.m.bode_out);
        if let WaveId::Ch(c) = inp {
            self.m.ch[c].on = true;
        }
        if let WaveId::Ch(c) = out {
            self.m.ch[c].on = true;
        }
        let trig = match inp {
            WaveId::Ch(c) => c,
            WaveId::Ref(_) => match out {
                WaveId::Ch(c) => c,
                WaveId::Ref(_) => self.m.trig_src,
            },
        };
        (self.m.trig_type, self.m.trig_src, self.m.trig_level, self.m.trig_mode, self.m.run) = (TrigType::Edge, trig, 0.0, TrigMode::Auto, RunState::Run);
        self.bode.running = true;
        if self.bode_gen() == BUILTIN {
            self.bode.afg_saved = Some((self.m.afg_on, self.m.afg_freq, self.m.afg_ampl, self.m.afg_offset, self.m.afg_wave, self.m.afg_mod));
            // the sweep measures unmodulated sines
            (self.m.afg_wave, self.m.afg_mod) = (0, 0);
            self.bode_send_freq();
        } else {
            let tx = gen_thread(self.m.bode_addr.clone(), self.bode.err.clone());
            let on = GENERATORS[self.bode_gen()].2.to_string();
            self.bode.tx = Some(tx);
            self.bode_send_freq();
            if let Some(tx) = &self.bode.tx {
                let _ = tx.send(on);
            }
        }
        self.toast("Bode sweep started");
    }

    pub(super) fn bode_finish(&mut self, msg: &str) {
        self.bode.running = false;
        self.bode.tx = None;
        if let Some((on, f, a, o, w, md)) = self.bode.afg_saved.take() {
            (self.m.afg_wave, self.m.afg_mod) = (w, md);
            self.afg_set(Some(on), Some(f), Some(a), Some(o));
        }
        if let Some((ts, src, lvl)) = self.bode.saved.take() {
            (self.m.tscale, self.m.trig_src, self.m.trig_level) = (ts, src, lvl);
            self.settings_changed();
        }
        self.toast(msg);
    }

    /// Every wave this sweep reads is a stored ref.
    fn bode_sources_are_refs(&self) -> bool {
        let (inp, out) = (self.m.bode_in, self.m.bode_out);
        let one = inp == out && self.bode_gen() == BUILTIN;
        let ids = if one { [out, out] } else { [inp, out] };
        ids.iter().all(|id| matches!(id, WaveId::Ref(_)))
    }

    /// One frequency. False when a source has no samples yet, so the sweep waits. A ref is its own `dt`,
    /// with no coupling and no bandwidth limit.
    fn bode_point(&mut self, rec: &Record, f: f64) -> bool {
        let (inp, out) = (self.m.bode_in, self.m.bode_out);
        let mode = self.m.bode_mode;
        if inp == out && self.bode_gen() == BUILTIN {
            let Some(s) = self.wave_samples(rec, out) else { return false };
            if let Some((a, _, _)) = bode::sine_fit(&s.v, s.dt, f).filter(|_| self.m.afg_ampl > 0.0) {
                let ratio = 2.0 * a / self.m.afg_ampl;
                let mid = match mode {
                    oos_dsp::bode::Mode::Psrr => -20.0 * ratio.log10(),
                    oos_dsp::bode::Mode::Z => ratio,
                    oos_dsp::bode::Mode::Loop => 20.0 * ratio.log10(),
                };
                self.bode.points.push((f, mid, f64::NAN));
            }
            return true;
        }
        let Ok(t) = self.take_waves(rec, &[inp, out], &[]) else { return false };
        if let Some((g, ph)) = bode::response(mode, &t.v[0], &t.v[1], t.dt, f) {
            self.bode.points.push((f, g, ph));
        }
        true
    }

    /// One sweep step per new record: measure, then the next frequency.
    pub(super) fn bode_update(&mut self) {
        if !self.bode.running {
            return;
        }
        let e = self.bode.err.lock().unwrap().take();
        if let Some(e) = e {
            self.bode_finish(&e);
            return;
        }
        let Some(rec) = self.last[0].clone() else { return };
        if rec.raw.as_ref().is_some_and(|r| r.frame.stamp < self.bode.stamp) {
            return;
        }
        if self.bode.wait > 0 {
            self.bode.wait -= 1;
            return;
        }
        let f = self.bode.freqs[self.bode.idx];
        if !self.bode_point(&rec, f) {
            return;
        }
        self.bode.idx += 1;
        if self.bode.idx >= self.bode.freqs.len() {
            self.bode_finish(&format!("Bode sweep done: {} points", self.bode.points.len()));
        } else {
            self.bode_send_freq();
        }
    }

    /// SCPI: start/stop the sweep and the result as "f,middle,phase_deg;...".
    /// The middle field is dB for the control loop and for PSRR. In impedance mode it is the linear Vout/Vin ratio.
    pub fn bode_run(&mut self, on: bool) {
        if on != self.bode.running {
            self.bode_activate(Item::BodeRun);
        }
    }
    pub fn bode_text(&self) -> String {
        self.bode.points.iter().map(|(f, g, p)| format!("{},{:.3},{:.2}", bode::fmt_hz(*f), g, p)).collect::<Vec<_>>().join(";")
    }

    /// Gain (top) and phase (bottom) over log frequency.
    /// The point under the cursor: frequency, gain, phase.
    fn bode_cursor_text(&self) -> String {
        let z = self.m.bode_mode == oos_dsp::bode::Mode::Z;
        match self.bode.points.get(self.bode.cursor) {
            Some(&(hz, g, p)) if z && p.is_nan() => format!("{} {:.3}", si_short(hz, "Hz"), g),
            Some(&(hz, g, p)) if z => format!("{} {:.3} {:.0}\u{b0}", si_short(hz, "Hz"), g, p),
            Some(&(hz, g, p)) if p.is_nan() => format!("{} {:.1} dB", si_short(hz, "Hz"), g),
            Some(&(hz, g, p)) => format!("{} {:.1} dB {:.0}\u{b0}", si_short(hz, "Hz"), g, p),
            None => "-".into(),
        }
    }

    pub(super) fn draw_bode(&self, cv: &mut Canvas) {
        let a = self.area;
        let f = &self.fonts;
        let (f0, f1) = (self.m.bode_start.ln(), self.m.bode_stop.ln());
        let x_of = |hz: f64| a.x + ((hz.ln() - f0) / (f1 - f0) * a.w as f64).round() as i32;
        let (gt, gb) = (a.y + 8, a.y + a.h / 2 - 8);
        let (pt, pb) = (a.y + a.h / 2 + 8, a.y + a.h - 24);
        // loop and PSRR: the measured range rounded to 10 dB. Impedance is a linear ratio, padded by about 10 %.
        let z = self.m.bode_mode == oos_dsp::bode::Mode::Z;
        let (gmin, gmax) = self.bode.points.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
        let (gmin, gmax) = if z {
            if gmin > gmax { (0.0, 2.0) } else {
                let pad = (gmax - gmin).abs().max(gmax.abs()).max(1e-3) * 0.1;
                (gmin - pad, gmax + pad)
            }
        } else if gmin > gmax {
            (-40.0, 10.0)
        } else {
            ((gmin / 10.0).floor() * 10.0 - 10.0, (gmax / 10.0).ceil() * 10.0)
        };
        let gy = |g: f64| gb - ((g - gmin) / (gmax - gmin) * (gb - gt) as f64).round() as i32;
        let py = |p: f64| pb - ((p + 180.0) / 360.0 * (pb - pt) as f64).round() as i32;
        // decade lines
        let mut d = 10f64.powf(self.m.bode_start.log10().floor());
        while d <= self.m.bode_stop * 1.001 {
            if d >= self.m.bode_start * 0.999 {
                let x = x_of(d);
                cv.vline(x, gt, gb, LINE);
                cv.vline(x, pt, pb, LINE);
                cv.text(&f.s, x + 2, a.y + a.h - 6, &si_short(d, "Hz"), DIM);
            }
            d *= 10.0;
        }
        cv.hline(a.x, a.x + a.w, py(0.0), LINE);
        cv.hline(a.x, a.x + a.w, gy(0.0).clamp(gt, gb), LINE);
        let col_g = self.wave_color(self.m.bode_out);
        let mut prev: Option<((i32, i32), (i32, i32))> = None;
        for &(hz, g, p) in &self.bode.points {
            let (x, yg, yp) = (x_of(hz), gy(g), py(p));
            if let Some(((x0, g0), (_, p0))) = prev {
                cv.line(x0, g0, x, yg, col_g);
                if !p.is_nan() && p0 != i32::MIN && (yp - p0).abs() < (pb - pt) / 2 {
                    cv.line(x0, p0, x, yp, ACCENT);
                }
            }
            cv.fill(Rect::new(x - 1, yg - 1, 3, 3), col_g);
            let yp = if p.is_nan() { i32::MIN } else { yp };
            if !p.is_nan() {
                cv.fill(Rect::new(x - 1, yp - 1, 3, 3), ACCENT);
            }
            prev = Some(((x, yg), (x, yp)));
        }
        if let Some(&(hz, ..)) = self.bode.points.get(self.bode.cursor) {
            let x = x_of(hz);
            cv.dashed_vline(x, gt, pb, 4, 3, CURSOR);
            cv.text(&f.s, a.x + 6, a.y + a.h / 2 + 2, &self.bode_cursor_text(), CURSOR);
        }
        let refn = if self.m.bode_in == self.m.bode_out && self.bode_gen() == BUILTIN { "generator".to_string() } else { self.wave_name(self.m.bode_in) };
        let (what, span) = match self.m.bode_mode {
            oos_dsp::bode::Mode::Psrr => ("PSRR", format!("{:.0} .. {:.0} dB", gmin, gmax)),
            oos_dsp::bode::Mode::Z => ("Vout/Vin", format!("{:.3} .. {:.3}", gmin, gmax)),
            oos_dsp::bode::Mode::Loop => ("Gain", format!("{:.0} .. {:.0} dB", gmin, gmax)),
        };
        cv.text(&f.s, a.x + 6, gt + 14, &format!("{} {} / {}  {}", what, self.wave_name(self.m.bode_out), refn, span), col_g);
        cv.text(&f.s, a.x + 6, pt + 14, "Phase -180 .. 180\u{b0}", ACCENT);
        let status = if self.bode.running {
            format!("sweeping {} of {}: {}", self.bode.idx + 1, self.bode.freqs.len(), si_short(self.bode.freqs[self.bode.idx], "Hz"))
        } else if self.m.bode_mode == oos_dsp::bode::Mode::Loop {
            if let Some(&(hz, g, p)) = self.bode.points.iter().min_by(|a, b| ((a.1 + 3.0).abs()).partial_cmp(&(b.1 + 3.0).abs()).unwrap()) {
                let ph = if p.is_nan() { String::new() } else { format!(", {:.0}\u{b0}", p) };
                format!("{} points; nearest -3 dB: {} ({:.1} dB{})", self.bode.points.len(), si_short(hz, "Hz"), g, ph)
            } else if self.bode_gen() == BUILTIN {
                "built-in generator (Bode > Start)".to_string()
            } else {
                format!("generator {} at {} (Bode > Start)", GENERATORS[self.bode_gen()].0, self.m.bode_addr)
            }
        } else if self.bode.points.is_empty() && self.bode_gen() == BUILTIN {
            "built-in generator (Bode > Start)".to_string()
        } else if self.bode.points.is_empty() {
            format!("generator {} at {} (Bode > Start)", GENERATORS[self.bode_gen()].0, self.m.bode_addr)
        } else {
            format!("{} points", self.bode.points.len())
        };
        // an open loop passes one of these two; a filter sweep neither
        let mg = oos_dsp::bode::margins(&self.bode.points);
        if self.m.bode_mode == oos_dsp::bode::Mode::Loop && !self.bode.running && (mg.gain.is_some() || mg.phase.is_some()) {
            let gm = mg.gain.map_or("Gain margin -".to_string(), |(v, hz)| format!("Gain margin {:.1} dB at {}", v, si_short(hz, "Hz")));
            let pm = mg.phase.map_or("phase margin -".to_string(), |(v, hz)| format!("phase margin {:.0}\u{b0} at {}", v, si_short(hz, "Hz")));
            cv.text(&f.s, a.x + 6, pt + 30, &format!("{}, {}", gm, pm), TEXT);
        }
        let w = f.s.width(&status) + 16;
        let r = Rect::new(a.x + a.w - w - 8, a.y + 22, w, 20);
        cv.round(r, 4, PANEL_BG, 230);
        cv.text_in(&f.s, r, 1, &status, TEXT);
    }
}
