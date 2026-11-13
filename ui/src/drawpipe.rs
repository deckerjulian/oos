// SPDX-License-Identifier: GPL-2.0-or-later

//! Deposit pipeline (fork, docs/fork/perf-plan.md item 1): every acquired frame is used. In `batch` mode the
//! acquisition thread decodes only the newest frame of a batch for the UI queue (the record for measurements, layers
//! and the record path) and hands the read-out buffer itself (no copy) with the other frames to deposit workers on
//! the little cores. Each worker decodes its share of the frames into kept vectors and draws them with the fast path
//! (`draw_frame`, the same function as `Ui::fast_draw`) into its own maps, with the screen mapping the UI published
//! (`View`), and counts the trigger statistics. Once per screen refresh the UI adds the workers' maps to its phosphor
//! (`merge`); it never iterates the frames.
//!
//! The picture is the one of the `frames` mode (every frame through the queue) for the same frames: the workers run
//! `draw_frame` on the frames the UI would have fast-drawn, and the newest frame of each batch takes the UI's path.
//! A batch whose workers are still busy, frames of older settings and maps of an older mapping are dropped and
//! counted (`Counters`). Where the fast path does not apply (no view), the acquisition thread queues every frame as in
//! `frames` mode. Mode: `acq.pipeline = batch` in system.conf or OOS_PIPELINE=batch; `frames` (default) is the old
//! path, kept for comparison until the unit has verified the new one.

use crate::board::Frame;
use crate::model::*;
use crate::wave::Phosphor;
use std::any::Any;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};

/// Trigger statistics of the fast path (`Ui::fast_edges`, `Ui::fast_off`).
pub struct Stats<'a> {
    pub edges: &'a mut [u64; 4],
    pub off: &'a mut (f64, f64),
}

/// One frame of the fast path into `ph`; false if its trigger edge was not found near the hardware trigger point.
/// Sparse frames (fewer than 2 samples per column) take the record path (`sparse::draw`, else `board::to_record`).
pub fn draw_frame(ph: &mut Phosphor, m: &Model, f: &Frame, stats: Option<Stats>) -> bool {
    let (w, h) = (ph.w, ph.h);
    let tdiv = m.time_per_div() as f64;
    let x_per_sample = f.dt / tdiv * w as f64 / HDIV as f64;
    if x_per_sample > 0.5 || f.envelope || f.roll || f.scan {
        if let Some(t) = crate::sparse::draw(ph, m, f, 24) {
            return t;
        }
        let fr = std::sync::Arc::new(f.clone());
        let window = tdiv * HDIV as f64;
        let rec = crate::board::to_record(&fr, m, 3600, window, m.delay as f64 - window / 2.0);
        for c in 0..NCH {
            if m.ch[c].on && !rec.ch[c].is_empty() {
                ph.draw_ex(c, m, (m.time_per_div(), m.delay), rec.t0, rec.dt, &rec.ch[c], 24, None);
            }
        }
        return rec.triggered;
    }
    let src = &f.ch[m.trig_src];
    // an LA line as the source: the hardware trigger point stands, the analog samples cannot refine it
    let on_la = m.trig_on_la();
    // the edge of the set direction nearest the hardware trigger point, up to half a screen away, on the signal smoothed
    // to one screen column: at decimated rates the hardware point lands near the level on either edge (trigger.md 8)
    let hyst = m.trig_hyst_div() * m.ch[m.trig_src].volts_per_div();
    let (rising, either) = (m.slope != Slope::Fall, m.slope == Slope::Either);
    let half = (tdiv * HDIV as f64 / 2.0 / f.dt) as usize;
    let col = (1.0 / x_per_sample).floor().clamp(1.0, 256.0) as usize;
    let edge = if on_la {
        None
    } else {
        let (e, any) = crate::trignear::edge_smooth_near(src, f.pre, crate::board::sw_trig_level(m, f), hyst, rising, either, half, col, |_| true);
        if let Some(s) = stats {
            let k = match (e, any) {
                (None, _) => 3,
                (Some(t), _) if (t - f.pre as f64).abs() <= (col + 16) as f64 => 0,
                (Some(_), Some(n)) if !either && n.rising != rising => 1,
                (Some(t), _) => {
                    let d = (t - f.pre as f64).abs();
                    *s.off = (s.off.0 + d, s.off.1.max(d));
                    2
                }
            };
            s.edges[k] += 1;
        }
        e
    };
    let ti = edge.unwrap_or(f.pre as f64);
    let x0 = w as f64 / 2.0 - m.delay as f64 / tdiv * w as f64 / HDIV as f64;
    let ppd_y = h as f32 / VDIV;
    for c in 0..NCH {
        let ch = &m.ch[c];
        if !ch.on || f.ch[c].is_empty() {
            continue;
        }
        let vpd = ch.volts_per_div();
        let sign = -1.0;
        let xm = crate::fastdraw::Map { ti, x_per_sample, x0, y_per_volt: sign * ppd_y / vpd, y0: h as f32 / 2.0 - ch.offset / vpd * ppd_y };
        if m.dots {
            crate::fastdraw::deposit_dots(&mut ph.map[c], w, h, &f.ch[c], &xm, 24);
        } else {
            crate::fastdraw::deposit(&mut ph.map[c], w, h, &f.ch[c], &xm, 24);
        }
    }
    // a filtered trigger path (coupling): the hardware point stands where the samples show no crossing
    edge.is_some() || on_la || m.trig_coupling() != TrigCoup::Dc
}


/// Frames acquired, used (drawn into the picture) and dropped, since the start; in both modes.
#[derive(Default)]
pub struct Counters {
    pub acquired: AtomicU64,
    pub used: AtomicU64,
    pub dropped: AtomicU64,
    /// frames whose cheap measurements went into the statistics (framestats.rs)
    pub measured: AtomicU64,
}

/// The process's counters (the acquisition thread, the workers and the UI add to them).
pub fn counters() -> &'static Counters {
    static C: OnceLock<Counters> = OnceLock::new();
    C.get_or_init(Counters::default)
}

impl Counters {
    pub fn add(c: &AtomicU64, n: u64) {
        c.fetch_add(n, Ordering::Relaxed);
    }
    /// (acquired, used, dropped)
    pub fn read(&self) -> (u64, u64, u64) {
        (self.acquired.load(Ordering::Relaxed), self.used.load(Ordering::Relaxed), self.dropped.load(Ordering::Relaxed))
    }
}

/// The pipeline mode: `batch` (this pipeline) or `frames` (every frame through the UI queue).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Frames,
    Batch,
}

/// OOS_PIPELINE, else the `acq.pipeline` line of system.conf, else `frames`.
pub fn mode_from(env: Option<&str>, conf: &str) -> Mode {
    let conf_val = conf.lines().filter_map(|l| l.split('#').next()?.split_once('=')).find(|(k, _)| k.trim() == "acq.pipeline").map(|(_, v)| v.trim().to_string());
    match env.map(str::to_string).or(conf_val).as_deref() {
        Some("batch") => Mode::Batch,
        _ => Mode::Frames,
    }
}

/// A read-out buffer of the board (the acquisition thread's DMA target), lent to the workers.
pub trait RawBatch: Send + Sync {
    fn bytes(&self) -> &[u8];
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send>;
}

/// Decodes the frame bytes into a kept frame (the board's decoder with the batch's settings).
pub type DecodeInto = Arc<dyn Fn(&[u8], &mut Frame) + Send + Sync>;

/// The buffer while the workers have it: back to the acquisition thread's pool when the last share is done.
struct Lease {
    raw: Option<Box<dyn RawBatch>>,
    home: Sender<Box<dyn RawBatch>>,
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(r) = self.raw.take() {
            // the pool is gone only when the pipe is: nothing to return to
            let _ = self.home.send(r);
        }
    }
}

/// One worker's share of a batch: frames `range` of `fb` bytes.
struct Job {
    lease: Arc<Lease>,
    fb: usize,
    range: Range<usize>,
    decode: DecodeInto,
    stamp: u64,
}

/// The screen mapping the workers draw with, and a generation that changes with anything the drawing depends on.
struct View {
    gen: u64,
    m: Model,
    w: usize,
    h: usize,
}

/// The fields of the model `draw_frame` depends on (a change of any other field keeps the workers' maps).
#[derive(Clone, PartialEq)]
struct Key {
    ch: [Channel; NCH],
    tscale: usize,
    delay: f32,
    trig: (usize, Option<u8>, TrigType, Slope, f32, TrigCoup, bool, bool),
    acq: AcqMode,
    dots: bool,
    size: (usize, usize),
}

impl Key {
    fn of(m: &Model, w: usize, h: usize) -> Key {
        Key { ch: m.ch.clone(), tscale: m.tscale, delay: m.delay, trig: (m.trig_src, m.trig_line(), m.trig_type, m.slope, m.trig_level, m.trig_coup, m.trig_nrej, m.trig_on_la()), acq: m.acq, dots: m.dots, size: (w, h) }
    }
}

/// What a worker drew and the UI has not merged yet.
struct Acc {
    gen: u64,
    ph: Phosphor,
    /// statistics of the cheap measurements over the frames (framestats.rs)
    folds: crate::framestats::Folds,
    frames: u64,
    triggered: u64,
    edges: [u64; 4],
    off: (f64, f64),
}

impl Acc {
    fn new(gen: u64, w: usize, h: usize) -> Acc {
        Acc { gen, ph: Phosphor::new(w, h), folds: Vec::new(), frames: 0, triggered: 0, edges: [0; 4], off: (0.0, 0.0) }
    }
    /// Empty again, keeping the maps' memory.
    fn reset(&mut self, gen: u64) {
        self.ph.clear();
        self.folds.clear();
        (self.gen, self.frames, self.triggered, self.edges, self.off) = (gen, 0, 0, [0; 4], (0.0, 0.0));
    }
    fn add(&mut self, o: &Acc) {
        add(&mut self.ph, &o.ph);
        crate::framestats::add_folds(&mut self.folds, &o.folds);
        self.frames += o.frames;
        self.triggered += o.triggered;
        for (a, b) in self.edges.iter_mut().zip(o.edges) {
            *a += b;
        }
        self.off = (self.off.0 + o.off.0, self.off.1.max(o.off.1));
    }
}

/// A worker's mailbox: the drawn maps the UI takes, and the maps it gives back to be cleared and reused.
#[derive(Default)]
struct Mail {
    full: Option<Acc>,
    spare: Option<Acc>,
    /// jobs handled (drawn or dropped), for tests
    done: u64,
}

struct Worker {
    tx: SyncSender<Job>,
    mail: Arc<Mutex<Mail>>,
}

/// The view and the settings stamp the UI is at; None while the fast path does not apply.
#[derive(Default)]
struct ViewState {
    view: Option<Arc<View>>,
    stamp: u64,
    key: Option<Key>,
    gen: u64,
}

pub struct Pipe {
    view: Arc<Mutex<ViewState>>,
    counters: &'static Counters,
    workers: Vec<Worker>,
    home: Sender<Box<dyn RawBatch>>,
    pool: Mutex<Receiver<Box<dyn RawBatch>>>,
}

/// Merged frames: how many, how many found their trigger, the statistics of the cheap measurements over them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Merged {
    pub frames: u64,
    pub triggered: u64,
    pub folds: crate::framestats::Folds,
}

impl Pipe {
    /// A pipe with `n` workers counting into `counters`; `little`: the workers go to the little cores.
    pub fn start(n: usize, little: bool, counters: &'static Counters) -> Pipe {
        let view: Arc<Mutex<ViewState>> = Default::default();
        let workers = (0..n.max(1))
            .filter_map(|k| {
                // one share waiting at most: the worker is drawing the last one or idle
                let (tx, rx) = sync_channel::<Job>(1);
                let mail: Arc<Mutex<Mail>> = Default::default();
                let (v, m) = (view.clone(), mail.clone());
                match std::thread::Builder::new().name(format!("deposit{}", k)).spawn(move || {
                    if little {
                        crate::http::to_little_cores();
                    }
                    work(rx, &v, &m, counters)
                }) {
                    Ok(_) => Some(Worker { tx, mail }),
                    Err(e) => {
                        eprintln!("oos-ui: deposit worker: {}", e);
                        None
                    }
                }
            })
            .collect();
        let (home, pool) = channel();
        Pipe { view, counters, workers, home, pool: Mutex::new(pool) }
    }

    /// The UI's mapping for the frames to come (None: the fast path does not apply).
    pub fn set_view(&self, m: Option<&Model>, w: usize, h: usize) {
        let mut v = self.view.lock().unwrap();
        let Some(m) = m else {
            (v.view, v.key) = (None, None);
            return;
        };
        let key = Key::of(m, w, h);
        if v.key.as_ref() != Some(&key) {
            v.gen += 1;
            v.view = Some(Arc::new(View { gen: v.gen, m: m.clone(), w, h }));
            v.key = Some(key);
        }
        v.stamp = m.hw_stamp;
    }

    /// The fast path applies: the workers take frames.
    pub fn accepting(&self) -> bool {
        self.view.lock().unwrap().view.is_some()
    }

    /// A buffer back from the workers, to read the next batch into (None: all are still out).
    pub fn take_buffer(&self) -> Option<Box<dyn Any + Send>> {
        self.pool.lock().unwrap().try_recv().ok().map(|r| r.into_any())
    }

    /// From the acquisition thread: the first `frames` frames of `fb` bytes in `raw`, recorded under settings stamp
    /// `stamp`, split over the workers. The shares of busy workers are dropped and counted. The buffer comes back
    /// through `take_buffer` when every share is done. Returns the frames taken.
    pub fn feed(&self, raw: Box<dyn RawBatch>, fb: usize, frames: usize, stamp: u64, decode: DecodeInto) -> usize {
        let frames = if fb == 0 { 0 } else { frames.min(raw.bytes().len() / fb) };
        let lease = Arc::new(Lease { raw: Some(raw), home: self.home.clone() });
        let n = self.workers.len();
        let mut taken = 0;
        for (k, w) in self.workers.iter().enumerate() {
            let range = frames * k / n..frames * (k + 1) / n;
            let len = range.len();
            if len == 0 {
                continue;
            }
            match w.tx.try_send(Job { lease: lease.clone(), fb, range, decode: decode.clone(), stamp }) {
                Ok(()) => taken += len,
                Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => Counters::add(&self.counters.dropped, len as u64),
            }
        }
        taken
    }

    /// Adds what the workers drew under the current mapping to `ph` and the trigger statistics to `edges`, `off`.
    /// `clear`: minimum persistence, the picture is replaced: cleared first if anything is merged.
    pub fn merge(&self, ph: &mut Phosphor, clear: &mut bool, edges: &mut [u64; 4], off: &mut (f64, f64)) -> Merged {
        let gen = self.view.lock().unwrap().view.as_ref().map(|v| v.gen);
        let mut got = Merged::default();
        for w in &self.workers {
            let full = w.mail.lock().unwrap().full.take();
            let Some(a) = full else { continue };
            if Some(a.gen) == gen && (a.ph.w, a.ph.h) == (ph.w, ph.h) && a.frames > 0 {
                if *clear {
                    ph.clear();
                    *clear = false;
                }
                add(ph, &a.ph);
                for (x, y) in edges.iter_mut().zip(a.edges) {
                    *x += y;
                }
                *off = (off.0 + a.off.0, off.1.max(a.off.1));
                got.frames += a.frames;
                got.triggered += a.triggered;
                crate::framestats::add_folds(&mut got.folds, &a.folds);
            } else {
                Counters::add(&self.counters.dropped, a.frames);
            }
            // back to the worker, which clears it
            w.mail.lock().unwrap().spare = Some(a);
        }
        got
    }

    #[cfg(test)]
    fn done(&self) -> u64 {
        self.workers.iter().map(|w| w.mail.lock().unwrap().done).sum()
    }
}

/// A worker: per job its share of the frames decoded into a kept frame and drawn into its own maps, then handed to the
/// mailbox (added to what the UI has not taken yet).
fn work(rx: Receiver<Job>, view: &Mutex<ViewState>, mail: &Mutex<Mail>, counters: &Counters) {
    let mut f = Frame::default();
    let mut acc: Option<Acc> = None;
    while let Ok(job) = rx.recv() {
        let (v, stamp) = {
            let s = view.lock().unwrap();
            (s.view.clone(), s.stamp)
        };
        let Some(v) = v.filter(|_| job.stamp >= stamp) else {
            Counters::add(&counters.dropped, job.range.len() as u64);
            mail.lock().unwrap().done += 1;
            continue;
        };
        let mut a = match acc.take() {
            Some(mut a) if (a.ph.w, a.ph.h) == (v.w, v.h) => {
                a.reset(v.gen);
                a
            }
            _ => Acc::new(v.gen, v.w, v.h),
        };
        a.folds = crate::framestats::folds_for(&v.m);
        if let Some(bytes) = job.lease.raw.as_ref().map(|r| r.bytes()) {
            for i in job.range.clone() {
                (job.decode)(&bytes[i * job.fb..(i + 1) * job.fb], &mut f);
                let st = Stats { edges: &mut a.edges, off: &mut a.off };
                a.triggered += draw_frame(&mut a.ph, &v.m, &f, Some(st)) as u64;
                if crate::framestats::fold_frame(&f, &v.m, &mut a.folds) {
                    Counters::add(&counters.measured, 1);
                }
                a.frames += 1;
            }
        }
        drop(job);
        let mut mb = mail.lock().unwrap();
        mb.done += 1;
        match mb.full.as_mut() {
            Some(full) if full.gen == a.gen && (full.ph.w, full.ph.h) == (a.ph.w, a.ph.h) => {
                full.add(&a);
                acc = Some(a);
            }
            _ => {
                if let Some(old) = mb.full.take() {
                    Counters::add(&counters.dropped, old.frames);
                }
                mb.full = Some(a);
                acc = mb.spare.take();
            }
        }
    }
}

/// `dst` += `src`, saturating, per channel map.
fn add(dst: &mut Phosphor, src: &Phosphor) {
    for (d, s) in dst.map.iter_mut().zip(src.map.iter()) {
        for (x, y) in d.iter_mut().zip(s.iter()) {
            *x = x.saturating_add(*y);
        }
    }
}

/// Workers of the pipeline on the unit (two of the four A53).
const WORKERS: usize = 2;

/// The pipe in `batch` mode (started on first use), else None.
pub fn pipe() -> Option<&'static Pipe> {
    static P: OnceLock<Option<Pipe>> = OnceLock::new();
    P.get_or_init(|| {
        let env = std::env::var("OOS_PIPELINE").ok();
        let conf = std::fs::read_to_string(crate::sysconf::default_path()).unwrap_or_default();
        (mode_from(env.as_deref(), &conf) == Mode::Batch).then(|| Pipe::start(WORKERS, true, counters()))
    })
    .as_ref()
}

/// The mode in force, for the status line.
pub fn mode() -> Mode {
    if pipe().is_some() { Mode::Batch } else { Mode::Frames }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oos_hal::dho::hz;
    use oos_hal::dho::seq::Settings;

    /// A read-out buffer for the tests.
    struct Bytes(Vec<u8>);

    impl RawBatch for Bytes {
        fn bytes(&self) -> &[u8] {
            &self.0
        }
        fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
            self
        }
    }

    fn fresh() -> &'static Counters {
        Box::leak(Box::default())
    }

    fn dec(s: &Settings, p: &hz::Params) -> DecodeInto {
        let (s, p) = (s.clone(), *p);
        Arc::new(move |c: &[u8], f: &mut Frame| crate::board::dho::hw::decode_into(c, &s, &p, 0, f))
    }

    /// A raw batch of `frames` frames, CH1 (and CH2 with `two`): a sine with a rising zero crossing near the hardware
    /// trigger point, a per-frame jitter, every fifth frame shifted up, every seventh without the crossing.
    fn batch(frames: usize, memsel: u32, two: bool) -> (Vec<u8>, usize, Settings, hz::Params) {
        let r = if two { 2 } else { 1 };
        let p = hz::Params { ratio: r, rate: 1_250_000_000 / r as u64, memsel, dot_fs: 800_000, seg: 0, max_frames: 1000, pre: memsel / 2, post: memsel / 2 };
        let s = Settings { mask: if two { 3 } else { 1 }, window: None, compress: 1, header: false, ..Settings::default() };
        let fb = oos_hal::dho::seq::frame_bytes(&s, &p);
        let mut bytes = Vec::new();
        for k in 0..frames {
            let jit = (k % 7) as f64 * 0.13 - 0.4;
            for i in 0..memsel as usize {
                let ph = (i as f64 - memsel as f64 / 2.0 + jit) / 400.0 * std::f64::consts::TAU;
                for c in 0..r as usize {
                    let v = if k % 7 == 6 { 0.7 } else { 0.9 * (ph * (1.0 + c as f64 * 0.5)).sin() + if k % 5 == 0 { 0.05 } else { 0.0 } };
                    bytes.extend(((32768.0 + v * 7500.0) as u16).to_le_bytes());
                }
            }
        }
        assert_eq!(bytes.len(), frames * fb, "frame layout");
        (bytes, fb, s, p)
    }

    fn model(tdiv: f64, two: bool, dots: bool) -> Model {
        let mut m = Model::default();
        m.tscale = tscales().iter().position(|&t| (t as f64 - tdiv).abs() < tdiv * 1e-3).unwrap();
        (m.trig_level, m.dots) = (0.0, dots);
        for c in 0..NCH {
            (m.ch[c].on, m.ch[c].offset) = (c == 0 || c == 1 && two, 0.0);
        }
        m
    }

    fn wait(p: &Pipe, done: u64) {
        let t0 = std::time::Instant::now();
        while p.done() < done {
            assert!(t0.elapsed() < std::time::Duration::from_secs(20), "workers stuck at {} of {}", p.done(), done);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    // Break it catches: a batch picture that differs from the frames mode for the same frames (decode into kept
    // vectors, the split over workers, dense and sparse frames, dots, two channels), trigger statistics that leave
    // frames out, frames lost or counted twice, a merge that does not clear for minimum persistence, a buffer that
    // does not come back.
    #[test]
    fn batch_picture_equals_the_frames_mode() {
        let (w, h) = (970, 448);
        for (tdiv, memsel, two, dots) in [(1e-6, 12_512u32, false, false), (1e-6, 12_512, true, true), (1e-7, 1408, false, false), (2e-8, 400, true, false)] {
            let mut m = model(tdiv, two, dots);
            m.meas = vec![(Meas::Vpp, 0), (Meas::Vavg, 0), (Meas::Vrms, 1)];
            let (bytes, fb, s, p) = batch(41, memsel, two);
            let c = fresh();
            let pipe = Pipe::start(2, false, c);
            pipe.set_view(Some(&m), w, h);
            assert!(pipe.accepting());
            // two batches, the second after the first is drawn: nothing is dropped
            assert_eq!(pipe.feed(Box::new(Bytes(bytes[..17 * fb].to_vec())), fb, 17, 0, dec(&s, &p)), 17);
            wait(&pipe, 2);
            assert_eq!(pipe.feed(Box::new(Bytes(bytes[17 * fb..].to_vec())), fb, 24, 0, dec(&s, &p)), 24);
            wait(&pipe, 4);
            let mut got = Phosphor::new(w, h);
            got.map[0][0] = 7;
            let (mut clear, mut edges, mut off) = (true, [0u64; 4], (0.0, 0.0));
            let n = pipe.merge(&mut got, &mut clear, &mut edges, &mut off);
            // the frames mode: `Ui::fast_draw` on every frame
            let mut want = Phosphor::new(w, h);
            let (mut trig, mut wedges, mut woff) = (0, [0u64; 4], (0.0, 0.0));
            let mut folds = crate::framestats::folds_for(&m);
            for chunk in bytes.chunks_exact(fb) {
                let st = Stats { edges: &mut wedges, off: &mut woff };
                let f = crate::board::dho::hw::decode(chunk, &s, &p, 0);
                trig += draw_frame(&mut want, &m, &f, Some(st)) as u64;
                crate::framestats::fold_frame(&f, &m, &mut folds);
            }
            // per-frame statistics: the same frames counted, sums in another order (shares of two workers)
            assert_eq!(n.folds.len(), folds.len());
            for ((k, a), (k2, b)) in n.folds.iter().zip(&folds) {
                assert_eq!((k, a.n, a.min, a.max), (k2, b.n, b.min, b.max), "{} s/div", tdiv);
                assert!((a.sum - b.sum).abs() <= 1e-12 * b.sum.abs().max(1.0) && (a.sumsq - b.sumsq).abs() <= 1e-12 * b.sumsq.abs().max(1.0));
            }
            assert!(tdiv < 1e-6 || folds.iter().all(|x| x.1.n > 30), "{} s/div: {:?}", tdiv, folds);
            assert_eq!((n.frames, n.triggered), (41, trig), "{} s/div", tdiv);
            assert!(!clear && trig > 30 && trig < 41, "{} s/div: {} triggered", tdiv, trig);
            assert_eq!(edges, wedges, "{} s/div", tdiv);
            assert!((off.0 - woff.0).abs() <= 1e-9 * woff.0.abs().max(1.0) && off.1 == woff.1);
            for c in 0..NCH {
                assert!(got.map[c] == want.map[c], "{} s/div {} pt: CH{} differs", tdiv, memsel, c + 1);
            }
            assert_eq!(c.read(), (0, 0, 0));
            assert_eq!(c.measured.load(Ordering::Relaxed), folds.first().map_or(0, |x| x.1.n));
            // merged once; both buffers back in the pool
            assert_eq!(pipe.merge(&mut got, &mut clear, &mut edges, &mut off), Merged::default());
            let back: Vec<_> = std::iter::from_fn(|| pipe.take_buffer()).collect();
            assert_eq!(back.len(), 2);
            assert!(back.into_iter().all(|b| b.downcast::<Bytes>().is_ok()));
        }
    }

    // Break it catches: frames of older settings drawn, maps of an older mapping merged, shares of a busy worker lost
    // without being counted, batches taken while the fast path does not apply.
    #[test]
    fn stale_and_busy_work_is_dropped_and_counted() {
        let (w, h) = (970, 448);
        let mut m = model(1e-6, false, false);
        let (bytes, fb, s, p) = batch(10, 12_512, false);
        let c = fresh();
        let pipe = Pipe::start(2, false, c);
        let mut ph = Phosphor::new(w, h);
        let merge = |ph: &mut Phosphor| pipe.merge(ph, &mut false, &mut [0; 4], &mut (0.0, 0.0));
        assert!(!pipe.accepting());
        // settings stamp older than the view's: dropped
        m.hw_stamp = 3;
        pipe.set_view(Some(&m), w, h);
        pipe.feed(Box::new(Bytes(bytes.clone())), fb, 10, 2, dec(&s, &p));
        wait(&pipe, 2);
        assert_eq!(merge(&mut ph), Merged::default());
        assert_eq!(c.read(), (0, 0, 10));
        // drawn, then the mapping changes before the merge: dropped at the merge
        pipe.feed(Box::new(Bytes(bytes.clone())), fb, 10, 3, dec(&s, &p));
        wait(&pipe, 4);
        m.ch[0].offset = 0.5;
        pipe.set_view(Some(&m), w, h);
        assert_eq!(merge(&mut ph), Merged::default());
        assert!(ph.map.iter().all(|x| x.iter().all(|&y| y == 0)));
        assert_eq!(c.read(), (0, 0, 20));
        // a change the drawing does not depend on keeps the maps
        pipe.feed(Box::new(Bytes(bytes.clone())), fb, 10, 3, dec(&s, &p));
        wait(&pipe, 6);
        m.status = TrigStatus::Wait;
        pipe.set_view(Some(&m), w, h);
        assert_eq!(merge(&mut ph).frames, 10);
        // many batches at once: what the busy workers cannot take is counted, the rest is merged
        let (big, fb2, s2, p2) = batch(200, 12_512, false);
        let mut taken = 0;
        for _ in 0..6 {
            taken += pipe.feed(Box::new(Bytes(big.clone())), fb2, 200, 3, dec(&s2, &p2));
        }
        let t0 = std::time::Instant::now();
        let mut merged = 0;
        while merged < taken as u64 {
            assert!(t0.elapsed() < std::time::Duration::from_secs(20), "merged {} of {}", merged, taken);
            merged += merge(&mut ph).frames;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(merged, taken as u64);
        assert_eq!(c.read().2, 20 + 1200 - taken as u64, "dropped = fed - taken");
        // no view: the acquisition thread queues the frames itself
        pipe.set_view(None, 0, 0);
        assert!(!pipe.accepting());
    }

    #[test]
    fn pipeline_mode_from_env_and_system_conf() {
        assert_eq!(mode_from(None, ""), Mode::Frames);
        assert_eq!(mode_from(None, "ntp = off\nacq.pipeline = batch   # new path\n"), Mode::Batch);
        assert_eq!(mode_from(Some("frames"), "acq.pipeline = batch\n"), Mode::Frames);
        assert_eq!(mode_from(Some("batch"), ""), Mode::Batch);
        assert_eq!(mode_from(None, "acq.pipeline = bogus\n"), Mode::Frames);
    }

    /// Host cost per frame of a worker (decode into kept vectors, trigger refinement, deposit) against the frames
    /// mode's per-frame work on the UI thread (`decode` + `Ui::fast_draw`). Run by tools/perf-guard.sh.
    #[test]
    #[ignore]
    fn perf_guard_deposit_worker_per_frame() {
        let (w, h) = (970, 448);
        for (tdiv, memsel, meas) in [(1e-7, 1408u32, false), (1e-6, 12_512, false), (1e-6, 12_512, true), (1e-5, 125_000, false), (1e-5, 125_000, true)] {
            let mut m = model(tdiv, false, false);
            // the measurements made on every frame cost on top (reported, not guarded): the worst case, all of them on
            m.meas = if meas { crate::framestats::CHEAP.iter().map(|&k| (k, 0)).collect() } else { Vec::new() };
            let frames = (4_000_000 / (memsel as usize * 2)).min(1000);
            let (bytes, fb, s, p) = batch(frames, memsel, false);
            // the frames mode: decode + fast_draw on one thread (its measurements: a few records a second, not counted)
            let mut ph = Phosphor::new(w, h);
            let t0 = std::time::Instant::now();
            for chunk in bytes.chunks_exact(fb) {
                std::hint::black_box(draw_frame(&mut ph, &m, &crate::board::dho::hw::decode(chunk, &s, &p, 0), None));
            }
            let old = t0.elapsed() / frames as u32;
            // one worker, then two
            let mut per = Vec::new();
            for n in [1usize, 2] {
                let pipe = Pipe::start(n, false, fresh());
                pipe.set_view(Some(&m), w, h);
                let mut bufs: Vec<Box<dyn RawBatch>> = (0..3).map(|_| Box::new(Bytes(bytes.clone())) as Box<dyn RawBatch>).collect();
                let t0 = std::time::Instant::now();
                let mut done = 0;
                let mut taken = 0;
                while let Some(b) = bufs.pop() {
                    taken += pipe.feed(b, fb, frames, 0, dec(&s, &p));
                    done += n as u64;
                    while pipe.done() < done {
                        std::thread::yield_now();
                    }
                }
                let el = t0.elapsed();
                assert_eq!(taken, 3 * frames);
                per.push(el / (3 * frames) as u32);
            }
            let r = per[0].as_secs_f64() / old.as_secs_f64();
            println!("{} s/div {} pt, frame statistics {}: frames mode {:?}/frame, 1 worker {:?}/frame (ratio {:.2}), 2 workers {:?}/frame -> {:.0} frames/s", tdiv, memsel, meas, old, per[0], r, per[1], 1.0 / per[1].as_secs_f64());
            assert!(meas || r <= 1.3, "a worker must not cost more per frame than the frames mode: {:.2}", r);
        }
    }
}
