// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Bode plot with the built-in generator (S models, `Board::gen`) or an external generator on the LAN:
//! OOS sends SCPI to the generator (presets for Rigol DG, Siglent SDG and plain SCPI), steps a logarithmic sweep, sets the time base to about 5 periods per screen,
//! measures gain and phase of the output channel against the input channel by a sine fit on every captured sample
//! (`oos_dsp::bode`) and draws both curves. The generator connection runs in its own thread. With the built-in
//! generator and input = output channel the gain is against the set amplitude and there is no phase.
//!
//! Free sweep (`FREE`): an external generator sweeps on its own and marks the start of each sweep on a sync channel.
//! OOS sets the time base and memory for one sweep, triggers once on the sync edge and reads gain (and phase, with
//! an input channel and plain samples) over the whole sweep from that one record (`oos_dsp::bode::sweep_response`).

use super::*;
use oos_dsp::bode;
use std::sync::mpsc;

/// Generator presets: name, command template (`{f}` Hz, `{a}` Vpp), output on. Entry `BUILTIN` is the scope's own.
/// Entry `FREE` is a generator that sweeps on its own (no commands).
pub const GENERATORS: [(&str, &str, &str); 5] = [
    ("Built-in", "", ""),
    ("Rigol DG", ":SOUR1:APPL:SIN {f},{a},0", ":OUTP1 ON"),
    ("Siglent SDG", "C1:BSWV WVTP,SINE,FRQ,{f},AMP,{a},OFST,0", "C1:OUTP ON"),
    ("SCPI", "FREQ {f};VOLT {a}", "OUTP ON"),
    ("Free-running sweep", "", ""),
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
    /// free sweep: waiting for the sync edge, or (`capturing`) a scan record of the sweep is being written
    pub free: bool,
    pub capturing: bool,
    /// the user's settings the free sweep changes, restored at the end
    pub free_saved: Option<FreeSaved>,
    /// why the last free sweep has no phase
    pub no_phase: Option<&'static str>,
}

/// What a free sweep changes: time base, delay, trigger, depth, Auto Roll, acquisition mode, run state.
#[derive(Clone, Copy)]
pub struct FreeSaved {
    tscale: usize,
    delay: f32,
    trig_type: TrigType,
    trig_src: usize,
    trig_la: Option<u8>,
    trig_level: f32,
    slope: Slope,
    trig_mode: TrigMode,
    mdepth: usize,
    auto_roll: bool,
    acq: AcqMode,
    run: RunState,
}

/// How a free sweep is recorded: time base (index), delay (s), depth (index), and whether the time base is a scan
/// (min/max data, the trigger at the centre).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreePlan {
    pub tscale: usize,
    pub delay: f32,
    pub mdepth: usize,
    pub scan: bool,
}

pub const BUILTIN: usize = 0;
pub const FREE: usize = 4;

/// Time base, delay and memory depth for one free sweep of `sw` on the channels shown in `m`. Normal records put the
/// trigger one division from the left edge, the sweep and 2 % in the 9 divisions after it, and the smallest depth
/// with 20 samples per period at the stop frequency (at least 4). From the scan time bases on the trigger sits at the
/// centre (where the scan puts it) and the sweep in the right half; the record holds min/max pairs.
pub fn free_plan(m: &Model, sw: &bode::Sweep) -> Result<FreePlan, String> {
    let ts = tscales();
    let pick = |want: f64| ts.iter().position(|&t| t as f64 >= want * 0.999);
    let mut c = m.clone();
    (c.auto_roll, c.acq, c.trig_mode) = (false, AcqMode::Normal, TrigMode::Single);
    if let Some(i) = pick(sw.time * 1.02 / 9.0) {
        (c.tscale, c.delay) = (i, 4.0 * ts[i]);
        if board().slow_mode(&c) == crate::board::Slow::Off {
            let mut best = (0, 0.0);
            for d in 1..mdepths().len() {
                c.mdepth = d;
                let (sr, _) = c.srate_depth();
                if sr > best.1 {
                    best = (d, sr);
                }
                if sr >= 20.0 * sw.stop {
                    break;
                }
            }
            if best.1 < 4.0 * sw.stop {
                return Err(format!("Sweep time does not fit the memory: {} at {} needs {}, the memory gives {}", si_short(sw.time, "s"), si_short(sw.stop, "Hz"), si_short(4.0 * sw.stop, "Sa/s"), si_short(best.1, "Sa/s")));
            }
            return Ok(FreePlan { tscale: i, delay: c.delay, mdepth: best.0, scan: false });
        }
    }
    let Some(i) = pick(sw.time * 1.02 / 5.0) else {
        return Err(format!("Sweep time too long: at most {}", si_short(*ts.last().unwrap() as f64 * 5.0 / 1.02, "s")));
    };
    (c.tscale, c.delay, c.mdepth) = (i, 0.0, 0);
    Ok(FreePlan { tscale: i, delay: 0.0, mdepth: 0, scan: board().slow_mode(&c) != crate::board::Slow::Off })
}

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

impl Ui {
    pub(super) fn bode_items(&self) -> Vec<Item> {
        if self.bode_gen() == FREE {
            use Item::*;
            let mut v = vec![BodeOn, BodeRun, BodeStart, BodeStop, BodeSwTime, BodeSwLog, BodePoints, BodeIn];
            // no input channel: the gain is against a level
            if self.m.bode_sw_in == 0 {
                v.push(BodeRef);
            }
            v.extend([BodeOut, BodeGen, BodeSync, BodeSyncEdge, BodeSyncLvl]);
            if !self.bode.points.is_empty() {
                v.push(BodeCur);
            }
            return v;
        }
        let mut v = vec![Item::BodeOn, Item::BodeRun, Item::BodeStart, Item::BodeStop, Item::BodePoints, Item::BodeAmp, Item::BodeIn, Item::BodeOut, Item::BodeGen];
        // a cursor once there is something to read
        if !self.bode.points.is_empty() {
            v.push(Item::BodeCur);
        }
        // the address only for a generator on the network
        if self.bode_gen() != BUILTIN {
            v.push(Item::BodeAddr);
        }
        v
    }

    pub(super) fn bode_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::BodeStart | Item::BodeStop | Item::BodePoints | Item::BodeAmp | Item::BodeAddr | Item::BodeCur | Item::BodeSwTime | Item::BodeSyncLvl | Item::BodeRef => Kind::Value,
            Item::BodeRun => Kind::Action,
            Item::BodeOn | Item::BodeIn | Item::BodeOut | Item::BodeGen | Item::BodeSwLog | Item::BodeSync | Item::BodeSyncEdge => Kind::Choice,
            _ => return None,
        })
    }

    pub(super) fn bode_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Bode) => "Bode",
            Item::BodeOn => "View",
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
            Item::BodeSwTime => "Sweep time",
            Item::BodeSwLog => "Sweep",
            Item::BodeSync => "Sync",
            Item::BodeSyncEdge => "Sync edge",
            Item::BodeSyncLvl => "Sync level",
            Item::BodeRef => "Ref level",
            _ => return None,
        })
    }

    pub(super) fn bode_value(&self, it: Item) -> Option<String> {
        let m = &self.m;
        Some(match it {
            Item::BodeOn => if m.view == AView::Bode { "On".into() } else { "Off".into() },
            Item::BodeRun => String::new(),
            Item::BodeStart => si_short(m.bode_start, "Hz"),
            Item::BodeStop => si_short(m.bode_stop, "Hz"),
            Item::BodePoints => m.bode_points.to_string(),
            Item::BodeAmp => si_short(m.bode_amp, "Vpp"),
            Item::BodeIn if self.bode_gen() == FREE => match m.bode_sw_in {
                0 => "Off".into(),
                k => format!("CH{}", k),
            },
            Item::BodeIn => format!("CH{}", m.bode_in + 1),
            Item::BodeOut => format!("CH{}", m.bode_out + 1),
            Item::BodeGen => GENERATORS[m.bode_gen.min(GENERATORS.len() - 1)].0.into(),
            Item::BodeAddr => m.bode_addr.clone(),
            Item::BodeCur => self.bode_cursor_text(),
            Item::BodeSwTime => si_short(m.bode_sw_time, "s"),
            Item::BodeSwLog => if m.bode_sw_log { "Log".into() } else { "Lin".into() },
            Item::BodeSync => format!("CH{}", m.bode_sync + 1),
            Item::BodeSyncEdge => if m.bode_sync_slope == Slope::Fall { "Falling".into() } else { "Rising".into() },
            Item::BodeSyncLvl => si_short(m.bode_sync_level as f64, "V"),
            Item::BodeRef => format!("{:.1} dBV", m.bode_ref),
            _ => return None,
        })
    }

    pub(super) fn bode_options(&self, it: Item) -> Option<Vec<String>> {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect();
        Some(match it {
            Item::BodeOn => v(&["Off", "On"]),
            Item::BodeIn if self.bode_gen() == FREE => opt_ch_names("Off"),
            Item::BodeIn | Item::BodeOut | Item::BodeSync => ch_names(),
            Item::BodeGen => GENERATORS.iter().map(|g| g.0.to_string()).collect(),
            Item::BodeSwLog => v(&["Lin", "Log"]),
            Item::BodeSyncEdge => v(&["Rising", "Falling"]),
            _ => return None,
        })
    }

    pub(super) fn bode_adjust(&mut self, it: Item, d: i32) -> bool {
        let free = self.bode_gen() == FREE;
        let m = &mut self.m;
        let ladder = |x: f64, d: i32| {
            let l: Vec<f64> = (0..10).flat_map(|e| [1.0, 2.0, 5.0].map(|k| k * 10f64.powi(e))).collect();
            let i = l.iter().position(|&v| v >= x * 0.999).unwrap_or(l.len() - 1);
            l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize]
        };
        match it {
            Item::BodeOn => m.view = if m.view == AView::Bode { AView::Scope } else { AView::Bode },
            Item::BodeStart => m.bode_start = ladder(m.bode_start, d).min(m.bode_stop / 2.0),
            Item::BodeStop => m.bode_stop = ladder(m.bode_stop, d).max(m.bode_start * 2.0),
            Item::BodePoints => m.bode_points = (m.bode_points as i32 + 5 * d).clamp(5, 200) as usize,
            Item::BodeAmp => m.bode_amp = ladder(m.bode_amp, d).clamp(0.01, 20.0),
            Item::BodeIn if free => m.bode_sw_in = opt_src_step(m.bode_sw_in, d),
            Item::BodeIn => m.bode_in = src_step(m.bode_in, NCH, d),
            Item::BodeSwTime => {
                // 1-2-5 steps from 1 ms to 500 s
                let l: Vec<f64> = (-3..3).flat_map(|e| [1.0, 2.0, 5.0].map(|k| k * 10f64.powi(e))).collect();
                let i = l.iter().position(|&v| v >= m.bode_sw_time * 0.999).unwrap_or(l.len() - 1);
                m.bode_sw_time = l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize];
            }
            // two choices: an odd step switches
            Item::BodeSwLog => m.bode_sw_log ^= d % 2 != 0,
            Item::BodeSyncEdge if d % 2 != 0 => m.bode_sync_slope = if m.bode_sync_slope == Slope::Fall { Slope::Rise } else { Slope::Fall },
            Item::BodeSyncEdge => {}
            Item::BodeSync => m.bode_sync = src_step(m.bode_sync, NCH, d),
            Item::BodeSyncLvl => m.bode_sync_level = (((m.bode_sync_level * 10.0).round() as i64 + d as i64).clamp(-200, 200) as f32) / 10.0,
            Item::BodeRef => m.bode_ref = (m.bode_ref.round() + d as f64).clamp(-80.0, 40.0),
            Item::BodeOut => m.bode_out = src_step(m.bode_out, NCH, d),
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
        self.bode.no_phase = None;
        if self.bode_gen() == FREE {
            return self.free_begin();
        }
        if self.bode_gen() == BUILTIN && !oos_hal::model::platform().caps.has_gen() {
            let name = &oos_hal::model::platform().caps.name;
            return self.toast(&format!("No built-in generator on the {}: choose an external one", name));
        }
        self.m.view = AView::Bode;
        self.bode.freqs = bode::log_sweep(self.m.bode_start, self.m.bode_stop, self.m.bode_points);
        self.bode.points.clear();
        self.bode.idx = 0;
        *self.bode.err.lock().unwrap() = None;
        self.bode.saved = Some((self.m.tscale, self.m.trig_src, self.m.trig_level));
        // trigger on the input channel at its centre, free running if nothing triggers
        let (inp, out) = (self.m.bode_in, self.m.bode_out);
        self.m.ch[inp].on = true;
        self.m.ch[out].on = true;
        (self.m.trig_type, self.m.trig_src, self.m.trig_level, self.m.trig_mode, self.m.run) = (TrigType::Edge, inp, 0.0, TrigMode::Auto, RunState::Run);
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
        (self.bode.free, self.bode.capturing) = (false, false);
        if let Some(s) = self.bode.free_saved.take() {
            let m = &mut self.m;
            (m.tscale, m.delay, m.trig_type, m.trig_src, m.trig_la, m.trig_level, m.slope) = (s.tscale, s.delay, s.trig_type, s.trig_src, s.trig_la, s.trig_level, s.slope);
            (m.trig_mode, m.mdepth, m.auto_roll, m.acq, m.run) = (s.trig_mode, s.mdepth, s.auto_roll, s.acq, s.run);
            self.dirty_map = true;
            self.settings_changed();
        }
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

    /// One sweep step per new record: measure, then the next frequency.
    pub(super) fn bode_update(&mut self) {
        if !self.bode.running {
            return;
        }
        if self.bode.free {
            return self.free_update();
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
        let (inp, out) = (self.m.bode_in, self.m.bode_out);
        if rec.ch[inp].is_empty() || rec.ch[out].is_empty() {
            return;
        }
        let (vo, dt) = self.meas_wave(&rec, out);
        let f = self.bode.freqs[self.bode.idx];
        if inp == out && self.bode_gen() == BUILTIN {
            // one channel on the generator: gain against the set amplitude (after the frequency limits), no phase
            if let Some((a, _, _)) = bode::sine_fit(&vo, dt, f).filter(|_| self.m.afg_ampl > 0.0) {
                self.bode.points.push((f, 20.0 * (2.0 * a / self.m.afg_ampl).log10(), f64::NAN));
            }
        } else {
            let (vi, _) = self.meas_wave(&rec, inp);
            if let Some((g, ph)) = bode::gain_phase(&vi, &vo, dt, f) {
                self.bode.points.push((f, g, ph));
            }
        }
        self.bode.idx += 1;
        if self.bode.idx >= self.bode.freqs.len() {
            self.bode_finish(&format!("Bode sweep done: {} points", self.bode.points.len()));
        } else {
            self.bode_send_freq();
        }
    }

    /// The free sweep as set in the menu.
    fn free_sweep(&self) -> bode::Sweep {
        let m = &self.m;
        bode::Sweep { start: m.bode_start, stop: m.bode_stop, time: m.bode_sw_time, log: m.bode_sw_log }
    }

    /// Free sweep: one record of the whole sweep, triggered once on the sync edge (`free_plan`).
    fn free_begin(&mut self) {
        let sw = self.free_sweep();
        let (out, sync, inp) = (self.m.bode_out, self.m.bode_sync, self.m.bode_sw_in.checked_sub(1));
        if !sw.valid() {
            return self.toast("Free sweep: the stop frequency must be above the start");
        }
        if sync == out || inp == Some(sync) {
            return self.toast("Free sweep: the sync channel must not be the input or output");
        }
        if inp == Some(out) {
            return self.toast("Free sweep: input and output must be different channels");
        }
        let mut want = self.m.clone();
        for c in [Some(out), Some(sync), inp].into_iter().flatten() {
            want.ch[c].on = true;
        }
        let plan = match free_plan(&want, &sw) {
            Ok(p) => p,
            Err(e) => return self.toast(&e),
        };
        let m = &mut self.m;
        self.bode.free_saved = Some(FreeSaved {
            tscale: m.tscale,
            delay: m.delay,
            trig_type: m.trig_type,
            trig_src: m.trig_src,
            trig_la: m.trig_la,
            trig_level: m.trig_level,
            slope: m.slope,
            trig_mode: m.trig_mode,
            mdepth: m.mdepth,
            auto_roll: m.auto_roll,
            acq: m.acq,
            run: m.run,
        });
        m.ch = want.ch;
        (m.tscale, m.delay, m.mdepth, m.auto_roll, m.acq) = (plan.tscale, plan.delay, plan.mdepth, false, AcqMode::Normal);
        let slope = if m.bode_sync_slope == Slope::Fall { Slope::Fall } else { Slope::Rise };
        (m.trig_type, m.trig_src, m.trig_la, m.trig_level, m.slope) = (TrigType::Edge, sync, None, m.bode_sync_level, slope);
        (m.trig_mode, m.run, m.view) = (TrigMode::Single, RunState::Run, AView::Bode);
        m.hw_stamp += 1;
        self.bode.stamp = m.hw_stamp;
        self.bode.points.clear();
        self.bode.cursor = 0;
        (self.bode.running, self.bode.free, self.bode.capturing) = (true, true, false);
        self.dirty_map = true;
        self.settings_changed();
        let slow = if plan.scan { " (slow time base: gain only)" } else { "" };
        self.toast(&format!("Waiting for the sync edge on CH{}: start the sweep{}", sync + 1, slow));
    }

    /// Free sweep: the SINGLE record of the sweep, once it is complete.
    fn free_update(&mut self) {
        let Some(rec) = self.last[0].clone() else { return };
        if rec.raw.as_ref().is_some_and(|r| r.frame.stamp < self.bode.stamp) {
            return;
        }
        if rec.partial {
            // a scan writes the sweep picture from the left; SINGLE stops on the complete one
            self.bode.capturing = true;
            return;
        }
        if self.m.run != RunState::Stop {
            return;
        }
        let msg = self.free_analyse(&rec);
        self.bode_finish(&msg);
    }

    /// Time from the trigger point of sample 0 of `meas_slice`.
    fn meas_t0(&self, rec: &Record, c: usize) -> f64 {
        match (&rec.raw, self.meas_frame(rec, c)) {
            (Some(r), Some(_)) => r.t0,
            _ => rec.t0,
        }
    }

    /// Gain (and phase) of the free sweep in `rec`: the sync edge is time 0 of the sweep. Returns the message.
    fn free_analyse(&mut self, rec: &Record) -> String {
        let sw = self.free_sweep();
        let m = &self.m;
        let (out, sync, inp) = (m.bode_out, m.bode_sync, m.bode_sw_in.checked_sub(1));
        let (vo, dt) = self.meas_slice(rec, out);
        let (vs, dts) = self.meas_slice(rec, sync);
        let vi = inp.map(|c| self.meas_slice(rec, c));
        if vo.is_empty() || vs.is_empty() || dts != dt || vi.is_some_and(|(v, d)| v.is_empty() || d != dt) {
            return "Free sweep: a channel is missing in the record".into();
        }
        // min/max pairs at the scan time bases
        let envelope = self.meas_frame(rec, out).is_some_and(|(f, _, _)| f.envelope);
        let hyst = m.trig_hyst_div() * m.ch[sync].volts_per_div();
        let rising = m.bode_sync_slope != Slope::Fall;
        let Some(ts) = bode::sync_start(vs, dt, self.meas_t0(rec, sync), m.bode_sync_level, hyst, rising, sw.time) else {
            return format!("No sync edge on CH{} in the record: check its edge and level", sync + 1);
        };
        let r = bode::sweep_response(&sw, &bode::SweepRecord { out: vo, inp: vi.map(|v| v.0), dt, t0: self.meas_t0(rec, out) - ts, envelope }, m.bode_points);
        // without an input: the output in dBV (RMS of the sine) against the reference level
        let ref_dbv = m.bode_ref;
        self.bode.points = r
            .points
            .iter()
            .map(|p| {
                let g = match p.inp {
                    Some(a) => 20.0 * (p.out / a).log10(),
                    None => 20.0 * (p.out / 2f64.sqrt()).log10() - ref_dbv,
                };
                (p.f, g, p.phase.unwrap_or(f64::NAN))
            })
            .filter(|p| p.1.is_finite())
            .collect();
        self.bode.cursor = 0;
        self.bode.no_phase = if inp.is_none() {
            Some("No phase without an input channel")
        } else if envelope {
            Some("No phase at slow time bases (min/max data)")
        } else {
            None
        };
        let why: Vec<&str> = [(r.too_fast, "sweep too fast for the low end"), (r.too_high, "sample rate too low for the top"), (r.outside, "sweep not in the record")]
            .iter()
            .filter(|w| w.0 > 0)
            .map(|w| w.1)
            .collect();
        let n = self.bode.points.len();
        match (n, why.is_empty()) {
            (0..=2, true) => format!("Too few points ({}): no signal on CH{}", n, out + 1),
            (0..=2, false) => format!("Too few points ({}): {}", n, why.join(", ")),
            (_, true) => format!("Sweep captured: {} points", n),
            (_, false) => format!("Sweep captured: {} points; {} left out: {}", n, r.too_fast + r.too_high + r.outside, why.join(", ")),
        }
    }

    /// SCPI: start/stop the sweep and the result as "f,gain_db,phase_deg;..." (empty while none).
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
        match self.bode.points.get(self.bode.cursor) {
            Some(&(hz, g, p)) if p.is_nan() => format!("{} {:.1} dB", si_short(hz, "Hz"), g),
            Some(&(hz, g, p)) => format!("{} {:.1} dB {:.0}\u{b0}", si_short(hz, "Hz"), g, p),
            None => "-".into(),
        }
    }

    pub(super) fn draw_bode(&self, cv: &mut Canvas) {
        let a = self.area;
        let f = self.fonts();
        let (f0, f1) = (self.m.bode_start.ln(), self.m.bode_stop.ln());
        let x_of = |hz: f64| a.x + ((hz.ln() - f0) / (f1 - f0) * a.w as f64).round() as i32;
        let (gt, gb) = (a.y + 8, a.y + a.h / 2 - 8);
        let (pt, pb) = (a.y + a.h / 2 + 8, a.y + a.h - 24);
        // gain axis: the measured range rounded to 10 dB, phase -180..180
        let (gmin, gmax) = self.bode.points.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
        let (gmin, gmax) = if gmin > gmax { (-40.0, 10.0) } else { ((gmin / 10.0).floor() * 10.0 - 10.0, (gmax / 10.0).ceil() * 10.0) };
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
        let col_g = CH_COLORS[self.m.bode_out];
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
        let free = self.bode_gen() == FREE;
        let refn = match self.m.bode_sw_in {
            0 if free => format!("{:.1} dBV", self.m.bode_ref),
            k if free => format!("CH{}", k),
            _ if self.m.bode_in == self.m.bode_out && self.bode_gen() == BUILTIN => "generator".to_string(),
            _ => format!("CH{}", self.m.bode_in + 1),
        };
        cv.text(&f.s, a.x + 6, gt + 14, &format!("Gain CH{} / {}  {:.0} .. {:.0} dB", self.m.bode_out + 1, refn, gmin, gmax), col_g);
        // a free sweep has a phase only from the fit of both channels on plain samples
        let phase_text = match (free, self.m.bode_sw_in, self.bode.no_phase) {
            (true, 0, _) => "No phase without an input channel",
            (true, _, Some(why)) => why,
            (true, _, None) => "Phase -180 .. 180\u{b0} (fit of both channels)",
            _ => "Phase -180 .. 180\u{b0}",
        };
        cv.text(&f.s, a.x + 6, pt + 14, phase_text, ACCENT);
        let sync_text = || {
            let edge = if self.m.bode_sync_slope == Slope::Fall { "falling" } else { "rising" };
            format!("waiting for the sync edge on CH{} ({}, {})", self.m.bode_sync + 1, edge, si_short(self.m.bode_sync_level as f64, "V"))
        };
        let status = if self.bode.running && self.bode.free {
            if self.bode.capturing {
                format!("recording the sweep ({})", si_short(self.m.bode_sw_time, "s"))
            } else {
                sync_text()
            }
        } else if self.bode.running {
            format!("sweeping {} of {}: {}", self.bode.idx + 1, self.bode.freqs.len(), si_short(self.bode.freqs[self.bode.idx], "Hz"))
        } else if let Some(&(hz, g, p)) = self.bode.points.iter().min_by(|a, b| ((a.1 + 3.0).abs()).partial_cmp(&(b.1 + 3.0).abs()).unwrap()) {
            let ph = if p.is_nan() { String::new() } else { format!(", {:.0}\u{b0}", p) };
            format!("{} points; nearest -3 dB: {} ({:.1} dB{})", self.bode.points.len(), si_short(hz, "Hz"), g, ph)
        } else if free {
            let law = if self.m.bode_sw_log { "log" } else { "lin" };
            format!("free sweep {} .. {} in {}, {} (Bode > Start)", si_short(self.m.bode_start, "Hz"), si_short(self.m.bode_stop, "Hz"), si_short(self.m.bode_sw_time, "s"), law)
        } else if self.bode_gen() == BUILTIN {
            "built-in generator (Bode > Start)".to_string()
        } else {
            format!("generator {} at {} (Bode > Start)", GENERATORS[self.bode_gen()].0, self.m.bode_addr)
        };
        // an open loop passes one of these two; a filter sweep neither
        let mg = oos_dsp::bode::margins(&self.bode.points);
        if !self.bode.running && (mg.gain.is_some() || mg.phase.is_some()) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A free log sweep 20 Hz .. 20 kHz in 0.5 s, its record handed in as the SINGLE record: CH1 the generator, CH2
    /// a first-order low-pass at 1 kHz, CH4 the sync pulse at the start.
    #[test]
    fn free_sweep_reads_one_record() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        (ui.m.bode_gen, ui.m.bode_start, ui.m.bode_stop, ui.m.bode_points) = (FREE, 20.0, 20e3, 40);
        (ui.m.bode_sw_time, ui.m.bode_sw_log, ui.m.bode_sw_in, ui.m.bode_out, ui.m.bode_sync, ui.m.bode_sync_level) = (0.5, true, 1, 1, 3, 1.5);
        let (ts0, trig0, mode0) = (ui.m.tscale, ui.m.trig_src, ui.m.trig_mode);
        assert!(ui.items(MenuId::Bode).contains(&Item::BodeSyncLvl) && !ui.items(MenuId::Bode).contains(&Item::BodeAmp));
        ui.activate(Item::BodeRun);
        assert!(ui.bode.running && ui.bode.free, "{:?}", ui.toast);
        assert!(ui.toast.as_ref().unwrap().0.contains("sync edge on CH4"));
        assert_eq!((ui.m.trig_src, ui.m.trig_mode, ui.m.trig_type, ui.m.run), (3, TrigMode::Single, TrigType::Edge, RunState::Run));
        assert!(ui.m.ch[0].on && ui.m.ch[1].on && ui.m.ch[3].on);
        // 100 ms/div: the trigger one division from the left, 9 for the sweep; a depth with 20 samples per period
        let tdiv = tscales()[ui.m.tscale] as f64;
        assert!((tdiv - 0.1).abs() < 1e-6 && (ui.m.delay as f64 - 0.4).abs() < 1e-6, "{} {}", tdiv, ui.m.delay);
        assert!(ui.m.srate_depth().0 >= 400e3, "{:?}", ui.m.srate_depth());
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        ui.render(&mut cv);
        // the record: from one division before the trigger, 200 kSa/s
        let sw = ui.free_sweep();
        let (dt, t0) = (5e-6, ui.m.delay as f64 - 5.0 * tdiv);
        let n = (10.0 * tdiv / dt) as usize;
        let x: Vec<f32> = (0..n).map(|k| (sw.phase_at((t0 + k as f64 * dt).clamp(0.0, sw.time)) + 0.3).sin() as f32).collect();
        let alpha = 1.0 - (-2.0 * std::f64::consts::PI * 1e3 * dt).exp();
        let mut y = 0.0f64;
        let out: Vec<f32> = x
            .iter()
            .map(|&v| {
                y += alpha * (v as f64 - y);
                y as f32
            })
            .collect();
        let sync: Vec<f32> = (0..n).map(|k| if (0.0..1e-3).contains(&(t0 + k as f64 * dt)) { 3.3 } else { 0.0 }).collect();
        ui.last[0] = Some(Record { t0, dt, ch: [x, out, Vec::new(), sync], triggered: true, partial: false, raw: None, la: None });
        ui.m.run = RunState::Stop;
        ui.bode_update();
        assert!(!ui.bode.running && !ui.bode.free);
        // 0.5 s over three decades passes the lowest frequencies in less than a period each
        let t = ui.toast.as_ref().unwrap().0.clone();
        assert!(t.starts_with("Sweep captured") && t.contains("too fast for the low end"), "{}", t);
        assert!(ui.bode.points.len() >= 30 && ui.bode.points[0].0 > 50.0 && ui.bode.points[0].0 < 100.0, "{:?}", ui.bode.points);
        // the exact response of that sampled low-pass: alpha / (1 - (1 - alpha) e^-jw)
        let truth = |f: f64| {
            let w = 2.0 * std::f64::consts::PI * f * dt;
            let (re, im) = (1.0 - (1.0 - alpha) * w.cos(), (1.0 - alpha) * w.sin());
            (20.0 * (alpha / (re * re + im * im).sqrt()).log10(), -im.atan2(re).to_degrees())
        };
        for &(f, g, p) in &ui.bode.points {
            let (tg, tp) = truth(f);
            assert!((g - tg).abs() < 0.5 && (p - tp).abs() < 3.0, "{} Hz: {} dB {} deg", f, g, p);
        }
        assert_eq!((ui.m.tscale, ui.m.trig_src, ui.m.trig_mode, ui.m.run), (ts0, trig0, mode0, RunState::Run), "settings restored");
        ui.render(&mut cv);
        // the same without the input channel: against 1 V peak = -3.01 dBV, no phase
        (ui.m.bode_sw_in, ui.m.bode_ref) = (0, -3.0);
        assert!(ui.items(MenuId::Bode).contains(&Item::BodeRef));
        ui.activate(Item::BodeRun);
        ui.m.run = RunState::Stop;
        ui.bode_update();
        assert!(ui.bode.points.len() >= 30 && ui.bode.points.iter().all(|p| p.2.is_nan()));
        assert!(ui.bode.points.iter().all(|&(f, g, _)| (g - truth(f).0).abs() < 0.5));
        assert_eq!(ui.bode.no_phase, Some("No phase without an input channel"));
        ui.render(&mut cv);
    }

    #[test]
    fn free_sweep_refuses_what_does_not_fit() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        (ui.m.bode_gen, ui.m.bode_start, ui.m.bode_stop, ui.m.bode_sw_time) = (FREE, 1e3, 200e6, 0.5);
        ui.activate(Item::BodeRun);
        assert!(!ui.bode.running && ui.toast.as_ref().unwrap().0.contains("does not fit the memory"), "{:?}", ui.toast);
        // sync on the output
        (ui.m.bode_stop, ui.m.bode_sync) = (20e3, ui.m.bode_out);
        ui.activate(Item::BodeRun);
        assert!(!ui.bode.running && ui.toast.as_ref().unwrap().0.contains("sync channel"));
        // 500 s: a scan at 200 s/div, the trigger at the centre
        (ui.m.bode_sync, ui.m.bode_sw_time) = (3, 500.0);
        let m = ui.m.clone();
        let p = free_plan(&m, &ui.free_sweep()).unwrap();
        assert!(p.scan && p.delay == 0.0 && (tscales()[p.tscale] - 200.0).abs() < 1e-3, "{:?}", p);
        // 2 s: a scan too (normal records end at 100 ms/div)
        ui.m.bode_sw_time = 2.0;
        let p = free_plan(&m, &ui.free_sweep()).unwrap();
        assert!(p.scan && (tscales()[p.tscale] - 0.5).abs() < 1e-6, "{:?}", p);
        // a record that never comes: waiting, then stopped by hand, settings back
        let ts0 = ui.m.tscale;
        ui.activate(Item::BodeRun);
        assert!(ui.bode.running);
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        ui.render(&mut cv);
        ui.activate(Item::BodeRun);
        assert!(!ui.bode.running && ui.m.tscale == ts0 && ui.m.trig_mode == TrigMode::Auto && ui.m.auto_roll == Model::default().auto_roll);
        // the demo signals through the whole loop: whatever CH4 does, the sweep waits or ends with a message
        ui.m.bode_sw_time = 0.05;
        ui.activate(Item::BodeRun);
        assert!(ui.bode.running);
        for _ in 0..40 {
            ui.step(0.05);
            ui.render(&mut cv);
        }
        assert!(ui.bode.running || (ui.m.tscale == ts0 && ui.m.trig_mode == TrigMode::Auto), "{:?}", ui.toast);
    }

    #[test]
    fn free_sweep_menu_settings() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.bode_gen = FREE;
        assert_eq!(ui.value(Item::BodeGen), "Free-running sweep");
        assert_eq!((ui.value(Item::BodeSwTime), ui.value(Item::BodeSwLog), ui.value(Item::BodeSync), ui.value(Item::BodeIn)), ("1 s".into(), "Log".into(), "CH4".into(), "CH1".into()));
        ui.adjust(Item::BodeSwTime, 2);
        ui.adjust(Item::BodeSwLog, 1);
        ui.adjust(Item::BodeSyncEdge, 1);
        ui.adjust(Item::BodeSyncLvl, 5);
        ui.adjust(Item::BodeIn, -1);
        assert_eq!((ui.m.bode_sw_time, ui.m.bode_sw_log, ui.m.bode_sync_slope, ui.m.bode_sw_in == 0), (5.0, false, Slope::Fall, true));
        assert!((ui.m.bode_sync_level - 1.5).abs() < 1e-6);
        ui.adjust(Item::BodeSwTime, -100);
        assert_eq!(ui.m.bode_sw_time, 1e-3);
        ui.adjust(Item::BodeSwTime, 100);
        assert_eq!(ui.m.bode_sw_time, 500.0);
    }
}
