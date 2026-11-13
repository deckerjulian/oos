// SPDX-License-Identifier: GPL-2.0-or-later

//! Deposit pipeline (fork, docs/fork/perf-plan.md item 1): every acquired frame is used. In `batch` mode the
//! acquisition thread decodes only the newest frame of a batch for the UI queue (the record for measurements, layers
//! and the record path) and hands the read-out buffer itself (no copy) with the other frames to the deposit workers,
//! one per little core. The workers claim the frames a few at a time; while the FPGA's ring fills with the next batch
//! the acquisition thread claims single frames too (`help`, from `seq::acquire_batch_idle`). Each frame is
//! decoded into kept vectors and drawn with the fast path (`draw_frame`, the same function as `Ui::fast_draw`) into
//! the claimer's own maps, with the screen mapping the UI published (`View`); the trigger statistics and the cheap
//! measurements (framestats.rs) are counted with it. Once per screen refresh the UI adds the maps to its phosphor
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
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
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
    // min/max pairs of the live compression (compress.rs) take the dense path with a pair deposit
    let pairs = crate::compress::live(f) && x_per_sample <= 0.5;
    if !pairs && (x_per_sample > 0.5 || f.envelope || f.roll || f.scan) {
        if let Some(t) = crate::sparse::draw(ph, m, f, 24) {
            return t;
        }
        let fr = std::sync::Arc::new(f.clone());
        let window = tdiv * HDIV as f64;
        let rec = crate::board::to_record(&fr, m, 3600, window, m.delay as f64 - window / 2.0);
        for c in 0..NCH {
            if m.ch[c].on && !rec.ch[c].is_empty() {
                ph.draw_ex(c, m, (m.time_per_div(), m.delay), rec.t0, rec.dt, &rec.ch[c], 24, None, rec.pairs);
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
    let col = if pairs { crate::compress::smooth_len(x_per_sample) } else { (1.0 / x_per_sample).floor().clamp(1.0, 256.0) as usize };
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
        if pairs {
            crate::compress::deposit_pairs(&mut ph.map[c], w, h, &f.ch[c], &xm, 24, m.dots);
        } else if m.dots {
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

/// `env` if set, else the value of `key` in system.conf text `conf` (a fork key: kept by `SysConf` as an unknown line).
pub fn conf_value(env: Option<&str>, conf: &str, key: &str) -> Option<String> {
    let conf_val = conf.lines().filter_map(|l| l.split('#').next()?.split_once('=')).find(|(k, _)| k.trim() == key).map(|(_, v)| v.trim().to_string());
    env.map(str::to_string).or(conf_val)
}

/// OOS_PIPELINE, else the `acq.pipeline` line of system.conf, else `frames`.
pub fn mode_from(env: Option<&str>, conf: &str) -> Mode {
    match conf_value(env, conf, "acq.pipeline").as_deref() {
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

/// A batch with the workers: its frames are claimed a few at a time by whoever is free (the workers, and the
/// acquisition thread while it waits for the next batch: `help`). Frames never claimed count as dropped.
struct Batch {
    lease: Lease,
    fb: usize,
    frames: usize,
    decode: DecodeInto,
    stamp: u64,
    next: AtomicUsize,
    counters: &'static Counters,
    /// batches not done yet (`Pipe::live`)
    live: Arc<AtomicUsize>,
}

impl Batch {
    /// The next `n` frames nobody has taken yet.
    fn claim(&self, n: usize) -> Option<Range<usize>> {
        let i = self.next.fetch_add(n, Ordering::Relaxed);
        (i < self.frames).then(|| i..(i + n).min(self.frames))
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        let left = self.frames.saturating_sub(self.next.load(Ordering::Relaxed));
        Counters::add(&self.counters.dropped, left as u64);
        self.live.fetch_sub(1, Ordering::Release);
    }
}

/// Frames a worker claims at a time.
const CLAIM: usize = 4;

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
}

/// The acquisition thread's state as a helper (`Pipe::help`): its maps and its kept frame.
#[derive(Default)]
pub struct Helper {
    acc: Option<Acc>,
    spare: Option<Acc>,
    f: Frame,
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
    workers: Vec<SyncSender<Arc<Batch>>>,
    /// the workers' mailboxes and, last, the helper's
    mails: Vec<Arc<Mutex<Mail>>>,
    /// batches the helper may take frames of, oldest first
    open: Mutex<std::collections::VecDeque<std::sync::Weak<Batch>>>,
    /// batches fed and not done (drawn and handed on, or dropped)
    live: Arc<AtomicUsize>,
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
        let mut mails = Vec::new();
        let mut workers = Vec::new();
        for k in 0..n {
            // batches are bounded by the read-out buffers (`hw::POOL`), a few at most
            let (tx, rx) = sync_channel::<Arc<Batch>>(16);
            let mail: Arc<Mutex<Mail>> = Default::default();
            let (v, m) = (view.clone(), mail.clone());
            match std::thread::Builder::new().name(format!("deposit{}", k)).spawn(move || {
                if little {
                    crate::http::to_little_cores();
                }
                work(rx, &v, &m)
            }) {
                Ok(_) => {
                    workers.push(tx);
                    mails.push(mail);
                }
                Err(e) => eprintln!("oos-ui: deposit worker: {}", e),
            }
        }
        mails.push(Default::default());
        let (home, pool) = channel();
        Pipe { view, counters, workers, mails, open: Default::default(), live: Default::default(), home, pool: Mutex::new(pool) }
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
    /// `stamp`, for the workers (and the helper) to claim. The buffer comes back through `take_buffer` when the batch
    /// is done. Frames nobody claims are counted as dropped.
    pub fn feed(&self, raw: Box<dyn RawBatch>, fb: usize, frames: usize, stamp: u64, decode: DecodeInto) {
        self.feed_batch(raw, fb, frames, stamp, decode);
    }

    /// `feed`, returning the batch (tests hold it to let the helper alone draw it).
    fn feed_batch(&self, raw: Box<dyn RawBatch>, fb: usize, frames: usize, stamp: u64, decode: DecodeInto) -> Arc<Batch> {
        let frames = if fb == 0 { 0 } else { frames.min(raw.bytes().len() / fb) };
        let b = Arc::new(Batch { lease: Lease { raw: Some(raw), home: self.home.clone() }, fb, frames, decode, stamp, next: AtomicUsize::new(0), counters: self.counters, live: self.live.clone() });
        self.live.fetch_add(1, Ordering::Acquire);
        {
            let mut open = self.open.lock().unwrap();
            open.retain(|w| w.strong_count() > 0);
            open.push_back(Arc::downgrade(&b));
        }
        for w in &self.workers {
            // full only if the workers are far behind: the others take its frames
            if let Err(TrySendError::Disconnected(_)) = w.try_send(b.clone()) {
                eprintln!("oos-ui: deposit worker gone");
            }
        }
        b
    }

    /// From the acquisition thread while the ring fills (`seq::acquire_batch_idle`): draws one frame of the oldest open batch into the
    /// helper's maps. False when there is none (the helper's maps then go to the UI).
    pub fn help(&self, h: &mut Helper) -> bool {
        let b = {
            let mut open = self.open.lock().unwrap();
            loop {
                match open.front().map(|w| w.upgrade()) {
                    None => break None,
                    Some(Some(b)) if b.next.load(Ordering::Relaxed) < b.frames => break Some(b),
                    Some(_) => {
                        open.pop_front();
                    }
                }
            }
        };
        let Some(b) = b else {
            self.help_done(h);
            return false;
        };
        let Some(r) = b.claim(1) else { return true };
        let (v, stamp) = self.view_now();
        let mails = self.mails.last().expect("helper mailbox");
        match v.filter(|_| b.stamp >= stamp) {
            None => Counters::add(&self.counters.dropped, r.len() as u64),
            Some(v) => {
                if h.acc.as_ref().is_some_and(|a| a.gen != v.gen) {
                    self.help_done(h);
                }
                if h.acc.is_none() {
                    let spare = h.spare.take().or_else(|| mails.lock().unwrap().spare.take());
                    h.acc = Some(fresh_acc(spare, &v));
                }
                let a = h.acc.as_mut().expect("helper maps");
                draw_range(&b, r, &v, a, &mut h.f, self.counters);
            }
        }
        true
    }

    /// The helper's maps to the UI.
    pub fn help_done(&self, h: &mut Helper) {
        match h.acc.take() {
            Some(a) if a.frames > 0 => h.spare = publish(self.mails.last().expect("helper mailbox"), a, self.counters),
            Some(a) => h.spare = Some(a),
            None => {}
        }
    }

    fn view_now(&self) -> (Option<Arc<View>>, u64) {
        let s = self.view.lock().unwrap();
        (s.view.clone(), s.stamp)
    }

    /// Adds what the workers drew under the current mapping to `ph` and the trigger statistics to `edges`, `off`.
    /// `clear`: minimum persistence, the picture is replaced: cleared first if anything is merged.
    pub fn merge(&self, ph: &mut Phosphor, clear: &mut bool, edges: &mut [u64; 4], off: &mut (f64, f64)) -> Merged {
        let gen = self.view.lock().unwrap().view.as_ref().map(|v| v.gen);
        let mut got = Merged::default();
        for mail in &self.mails {
            let full = mail.lock().unwrap().full.take();
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
            mail.lock().unwrap().spare = Some(a);
        }
        got
    }

}

/// Empty maps for view `v`, reusing `spare` when it has the size.
fn fresh_acc(spare: Option<Acc>, v: &View) -> Acc {
    let mut a = match spare {
        Some(mut a) if (a.ph.w, a.ph.h) == (v.w, v.h) => {
            a.reset(v.gen);
            a
        }
        _ => Acc::new(v.gen, v.w, v.h),
    };
    a.folds = crate::framestats::folds_for(&v.m);
    a
}

/// Frames `r` of batch `b` decoded into `f` and drawn into `a`.
fn draw_range(b: &Batch, r: Range<usize>, v: &View, a: &mut Acc, f: &mut Frame, counters: &Counters) {
    let Some(bytes) = b.lease.raw.as_ref().map(|x| x.bytes()) else { return };
    for i in r {
        (b.decode)(&bytes[i * b.fb..(i + 1) * b.fb], f);
        let st = Stats { edges: &mut a.edges, off: &mut a.off };
        a.triggered += draw_frame(&mut a.ph, &v.m, f, Some(st)) as u64;
        if crate::framestats::fold_frame(f, &v.m, &mut a.folds) {
            Counters::add(&counters.measured, 1);
        }
        a.frames += 1;
    }
}

/// Hands drawn maps to `mail` (added to what the UI has not taken yet); returns maps to draw the next ones into.
fn publish(mail: &Mutex<Mail>, a: Acc, counters: &Counters) -> Option<Acc> {
    let mut mb = mail.lock().unwrap();
    match mb.full.as_mut() {
        Some(full) if full.gen == a.gen && (full.ph.w, full.ph.h) == (a.ph.w, a.ph.h) => {
            full.add(&a);
            Some(a)
        }
        _ => {
            if let Some(old) = mb.full.take() {
                Counters::add(&counters.dropped, old.frames);
            }
            mb.full = Some(a);
            mb.spare.take()
        }
    }
}

/// A worker: claims frames of each batch until none are left, draws them into its own maps, then hands them on.
fn work(rx: Receiver<Arc<Batch>>, view: &Mutex<ViewState>, mail: &Mutex<Mail>) {
    let mut f = Frame::default();
    let mut spare: Option<Acc> = None;
    while let Ok(b) = rx.recv() {
        let (v, stamp) = {
            let s = view.lock().unwrap();
            (s.view.clone(), s.stamp)
        };
        let Some(v) = v.filter(|_| b.stamp >= stamp) else {
            // older settings or no fast path: the frames are dropped (counted when the batch goes)
            while let Some(r) = b.claim(b.frames.max(1)) {
                Counters::add(&b.counters.dropped, r.len() as u64);
            }
            continue;
        };
        let mut a = fresh_acc(spare.take().or_else(|| mail.lock().unwrap().spare.take()), &v);
        while let Some(r) = b.claim(CLAIM) {
            draw_range(&b, r, &v, &mut a, &mut f, b.counters);
        }
        // handed on before the batch goes: once it is gone, its frames are in a mailbox
        if a.frames == 0 {
            spare = Some(a);
        } else {
            spare = publish(mail, a, b.counters);
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

/// Workers where the cores are all alike (the host): two.
const WORKERS: usize = 2;

/// The pipe in `batch` mode (started on first use), else None.
pub fn pipe() -> Option<&'static Pipe> {
    static P: OnceLock<Option<Pipe>> = OnceLock::new();
    P.get_or_init(|| {
        let env = std::env::var("OOS_PIPELINE").ok();
        let conf = std::fs::read_to_string(crate::sysconf::default_path()).unwrap_or_default();
        (mode_from(env.as_deref(), &conf) == Mode::Batch).then(|| Pipe::start(crate::placement::little_count().unwrap_or(WORKERS), true, counters()))
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

    /// Until every batch fed is done (drawn and handed on, or dropped).
    fn settle(p: &Pipe) {
        let t0 = std::time::Instant::now();
        while p.live.load(Ordering::Acquire) > 0 {
            assert!(t0.elapsed() < std::time::Duration::from_secs(20), "batches still open");
            std::thread::yield_now();
        }
    }

    /// The frames mode for the same frames: `Ui::fast_draw` and the per-frame statistics on every frame.
    fn reference(bytes: &[u8], fb: usize, s: &Settings, p: &hz::Params, m: &Model, w: usize, h: usize) -> (Phosphor, u64, [u64; 4], (f64, f64), crate::framestats::Folds) {
        let mut want = Phosphor::new(w, h);
        let (mut trig, mut edges, mut off) = (0, [0u64; 4], (0.0, 0.0));
        let mut folds = crate::framestats::folds_for(m);
        for chunk in bytes.chunks_exact(fb) {
            let f = crate::board::dho::hw::decode(chunk, s, p, 0);
            trig += draw_frame(&mut want, m, &f, Some(Stats { edges: &mut edges, off: &mut off })) as u64;
            crate::framestats::fold_frame(&f, m, &mut folds);
        }
        (want, trig, edges, off, folds)
    }

    // Break it catches: a batch picture that differs from the frames mode for the same frames (decode into kept
    // vectors, frames claimed by several workers and the helper, dense and sparse frames, dots, two channels),
    // trigger or measurement statistics that leave frames out, frames lost or counted twice, a merge that does not
    // clear for minimum persistence, a buffer that does not come back.
    #[test]
    fn batch_picture_equals_the_frames_mode() {
        let (w, h) = (970, 448);
        for (tdiv, memsel, two, dots, workers, helper) in [(1e-6, 12_512u32, false, false, 2, false), (1e-6, 12_512, true, true, 4, true), (1e-7, 1408, false, false, 1, true), (2e-8, 400, true, false, 1, true)] {
            let mut m = model(tdiv, two, dots);
            m.meas = vec![(Meas::Vpp, 0), (Meas::Vavg, 0), (Meas::Vrms, 1)];
            let (bytes, fb, s, p) = batch(41, memsel, two);
            let c = fresh();
            let pipe = Pipe::start(workers, false, c);
            pipe.set_view(Some(&m), w, h);
            assert!(pipe.accepting());
            pipe.feed(Box::new(Bytes(bytes[..17 * fb].to_vec())), fb, 17, 0, dec(&s, &p));
            pipe.feed(Box::new(Bytes(bytes[17 * fb..].to_vec())), fb, 24, 0, dec(&s, &p));
            // the acquisition thread's share, as while it waits for the next batch
            let mut hp = Helper::default();
            if helper {
                while pipe.help(&mut hp) {}
            }
            settle(&pipe);
            pipe.help_done(&mut hp);
            let mut got = Phosphor::new(w, h);
            got.map[0][0] = 7;
            let (mut clear, mut edges, mut off) = (true, [0u64; 4], (0.0, 0.0));
            let n = pipe.merge(&mut got, &mut clear, &mut edges, &mut off);
            let (want, trig, wedges, woff, folds) = reference(&bytes, fb, &s, &p, &m, w, h);
            let what = format!("{} s/div, {} workers, helper {}", tdiv, workers, helper);
            assert_eq!((n.frames, n.triggered), (41, trig), "{}", what);
            assert!(!clear && trig > 30 && trig < 41, "{}: {} triggered", what, trig);
            assert_eq!(edges, wedges, "{}", what);
            assert!((off.0 - woff.0).abs() <= 1e-9 * woff.0.abs().max(1.0) && off.1 == woff.1);
            for c in 0..NCH {
                assert!(got.map[c] == want.map[c], "{}: CH{} differs", what, c + 1);
            }
            // per-frame statistics: the same frames counted, sums in another order
            assert_eq!(n.folds.len(), folds.len());
            for ((k, a), (k2, b)) in n.folds.iter().zip(&folds) {
                assert_eq!((k, a.n, a.min, a.max), (k2, b.n, b.min, b.max), "{}", what);
                assert!((a.sum - b.sum).abs() <= 1e-12 * b.sum.abs().max(1.0) && (a.sumsq - b.sumsq).abs() <= 1e-12 * b.sumsq.abs().max(1.0));
            }
            assert!(tdiv < 1e-6 || folds.iter().all(|x| x.1.n > 30), "{}: {:?}", what, folds);
            assert_eq!(c.read(), (0, 0, 0), "{}: nothing dropped", what);
            assert_eq!(c.measured.load(Ordering::Relaxed), folds.first().map_or(0, |x| x.1.n));
            // merged once; both buffers back in the pool
            assert_eq!(pipe.merge(&mut got, &mut clear, &mut edges, &mut off), Merged::default());
            let back: Vec<_> = std::iter::from_fn(|| pipe.take_buffer()).collect();
            assert_eq!(back.len(), 2);
            assert!(back.into_iter().all(|b| b.downcast::<Bytes>().is_ok()));
        }
    }

    // Break it catches: the acquisition thread's share drawn differently from the workers', or not handed to the UI.
    #[test]
    fn the_helper_alone_draws_the_same_picture() {
        let (w, h) = (970, 448);
        for (tdiv, memsel) in [(1e-6, 12_512u32), (1e-7, 1408)] {
            let mut m = model(tdiv, false, false);
            m.meas = vec![(Meas::Vpp, 0)];
            let (bytes, fb, s, p) = batch(20, memsel, false);
            let pipe = Pipe::start(0, false, fresh());
            pipe.set_view(Some(&m), w, h);
            let b = pipe.feed_batch(Box::new(Bytes(bytes.clone())), fb, 20, 0, dec(&s, &p));
            let mut hp = Helper::default();
            let mut k = 0;
            while pipe.help(&mut hp) {
                k += 1;
                assert!(k < 100);
            }
            drop(b);
            let mut got = Phosphor::new(w, h);
            let n = pipe.merge(&mut got, &mut false, &mut [0; 4], &mut (0.0, 0.0));
            let (want, trig, ..) = reference(&bytes, fb, &s, &p, &m, w, h);
            assert_eq!((k, n.frames, n.triggered), (20, 20, trig), "{} s/div", tdiv);
            assert!((0..NCH).all(|c| got.map[c] == want.map[c]), "{} s/div", tdiv);
        }
    }

    // Break it catches: frames of older settings drawn, maps of an older mapping merged, frames nobody drew not
    // counted, batches taken while the fast path does not apply.
    #[test]
    fn stale_work_is_dropped_and_counted() {
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
        settle(&pipe);
        assert_eq!(merge(&mut ph), Merged::default());
        assert_eq!(c.read(), (0, 0, 10));
        // drawn, then the mapping changes before the merge: dropped at the merge
        pipe.feed(Box::new(Bytes(bytes.clone())), fb, 10, 3, dec(&s, &p));
        settle(&pipe);
        m.ch[0].offset = 0.5;
        pipe.set_view(Some(&m), w, h);
        assert_eq!(merge(&mut ph), Merged::default());
        assert!(ph.map.iter().all(|x| x.iter().all(|&y| y == 0)));
        assert_eq!(c.read(), (0, 0, 20));
        // a change the drawing does not depend on keeps the maps
        pipe.feed(Box::new(Bytes(bytes.clone())), fb, 10, 3, dec(&s, &p));
        settle(&pipe);
        m.status = TrigStatus::Wait;
        pipe.set_view(Some(&m), w, h);
        assert_eq!(merge(&mut ph).frames, 10);
        // many batches at once: all drawn, nothing more dropped
        let (big, fb2, s2, p2) = batch(200, 12_512, false);
        for _ in 0..6 {
            pipe.feed(Box::new(Bytes(big.clone())), fb2, 200, 3, dec(&s2, &p2));
        }
        settle(&pipe);
        assert_eq!(merge(&mut ph).frames, 1200);
        assert_eq!(c.read().2, 20);
        // a batch dropped before anybody claimed a frame counts all of them
        let pipe0 = Pipe::start(0, false, c);
        pipe0.set_view(Some(&m), w, h);
        pipe0.feed(Box::new(Bytes(bytes.clone())), fb, 10, 3, dec(&s, &p));
        assert_eq!(c.read().2, 30);
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
    /// mode's per-frame work on the UI thread (`decode` + `Ui::fast_draw`), and the rate of 1, 2 and 4 workers (the
    /// helper runs the same code per frame). Run by tools/perf-guard.sh.
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
            // the frames mode: decode + fast_draw on one thread (its measurements: a few records a second)
            let mut ph = Phosphor::new(w, h);
            let t0 = std::time::Instant::now();
            for chunk in bytes.chunks_exact(fb) {
                std::hint::black_box(draw_frame(&mut ph, &m, &crate::board::dho::hw::decode(chunk, &s, &p, 0), None));
            }
            let old = t0.elapsed() / frames as u32;
            let mut per = Vec::new();
            for n in [1usize, 2, 4] {
                let pipe = Pipe::start(n, false, fresh());
                pipe.set_view(Some(&m), w, h);
                let t0 = std::time::Instant::now();
                for _ in 0..3 {
                    pipe.feed(Box::new(Bytes(bytes.clone())), fb, frames, 0, dec(&s, &p));
                    settle(&pipe);
                }
                per.push(t0.elapsed() / (3 * frames) as u32);
            }
            let r = per[0].as_secs_f64() / old.as_secs_f64();
            println!("{} s/div {} pt, frame statistics {}: frames mode {:?}/frame; one core in the pipeline {:?}/frame (ratio {:.2}); 1/2/4 workers {:.0}/{:.0}/{:.0} frames/s", tdiv, memsel, meas, old, per[0], r, 1.0 / per[0].as_secs_f64(), 1.0 / per[1].as_secs_f64(), 1.0 / per[2].as_secs_f64());
            assert!(meas || r <= 1.5, "a frame in the pipeline must not cost more than in the frames mode: {:.2}", r);
        }
    }
}
