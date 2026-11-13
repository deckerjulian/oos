// SPDX-License-Identifier: GPL-2.0-or-later

//! Deposit worker (fork, docs/fork/perf-plan.md item 1). The UI queue keeps the newest `QUEUE_MAX` frames of a batch;
//! at short time bases a batch holds up to 1000, and the rest was never drawn. With OOS_DRAWPIPE set, the acquisition
//! thread hands the bytes of those older frames to a worker on a little core. The worker decodes them and draws them
//! with the fast path (`draw_frame`, the same function as `Ui::fast_draw`) into its own maps, using the screen mapping
//! the UI published (`View`). Once per screen frame the UI adds the worker's maps to its phosphor (`merge`).
//!
//! Batches are dropped, not queued, while the worker is busy, while the fast path does not apply (no view), and when
//! their settings stamp is older than the view's. Maps drawn under an older mapping are dropped at the merge.
//! Off by default until the rate gain and the CPU load are measured on the unit.

use crate::board::Frame;
use crate::model::*;
use crate::wave::Phosphor;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
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

/// The screen mapping the worker draws with: the UI's model and map size, the settings stamp in force, and a
/// generation that changes with anything the drawing depends on.
struct View {
    gen: u64,
    m: Model,
    w: usize,
    h: usize,
}

/// The fields of the model `draw_frame` depends on (a change of any other field keeps the worker's maps).
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

/// Turns the bytes of one frame into a frame (the board's decoder with the batch's settings).
pub type Decode = Box<dyn Fn(&[u8]) -> Frame + Send>;

/// A batch of raw frames for the worker.
struct Batch {
    bytes: Vec<u8>,
    fb: usize,
    decode: Decode,
    stamp: u64,
}

/// What the worker drew and the UI has not merged yet.
#[derive(Default)]
struct Acc {
    gen: u64,
    maps: Option<Phosphor>,
    frames: u64,
    triggered: u64,
    /// batches handled (drawn or dropped), for tests
    done: u64,
}

pub struct Pipe {
    /// the view and the stamp of the settings it belongs to; None while the fast path does not apply
    view: Mutex<(Option<Arc<View>>, u64, Option<Key>)>,
    acc: Mutex<Acc>,
    tx: SyncSender<Batch>,
}

/// Merged frames: how many, and how many of them found their trigger.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Merged {
    pub frames: u64,
    pub triggered: u64,
}

impl Pipe {
    /// A pipe with its worker thread; `little`: the worker goes to the little cores.
    pub fn start(little: bool) -> Arc<Pipe> {
        // one batch waiting at most: the worker is either drawing the last batch or idle
        let (tx, rx) = sync_channel(1);
        let pipe = Arc::new(Pipe { view: Mutex::new((None, 0, None)), acc: Mutex::new(Acc::default()), tx });
        let me = pipe.clone();
        let spawned = std::thread::Builder::new().name("drawpipe".into()).spawn(move || {
            if little {
                crate::http::to_little_cores();
            }
            me.work(rx)
        });
        if let Err(e) = spawned {
            eprintln!("oos-ui: drawpipe: {}", e);
        }
        pipe
    }

    /// The UI's mapping for the frames to come (None: the fast path does not apply; batches are dropped).
    pub fn set_view(&self, m: Option<&Model>, w: usize, h: usize) {
        let mut v = self.view.lock().unwrap();
        let Some(m) = m else {
            v.0 = None;
            return;
        };
        let key = Key::of(m, w, h);
        if v.0.is_none() || v.2.as_ref() != Some(&key) {
            let gen = v.0.as_ref().map_or(0, |x| x.gen) + 1;
            *v = (Some(Arc::new(View { gen, m: m.clone(), w, h })), m.hw_stamp, Some(key));
        } else if v.1 != m.hw_stamp {
            v.1 = m.hw_stamp;
        }
    }

    /// From the acquisition thread: frames of `fb` bytes in `bytes`, recorded under settings stamp `stamp`. Dropped
    /// when the worker is still busy. Returns whether the worker took it.
    pub fn feed(&self, bytes: &[u8], fb: usize, stamp: u64, decode: Decode) -> bool {
        if fb == 0 || bytes.len() < fb {
            return false;
        }
        let b = Batch { bytes: bytes[..bytes.len() / fb * fb].to_vec(), fb, decode, stamp };
        match self.tx.try_send(b) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
        }
    }

    fn work(&self, rx: Receiver<Batch>) {
        let mut ph: Option<Phosphor> = None;
        while let Ok(b) = rx.recv() {
            let (view, stamp) = {
                let v = self.view.lock().unwrap();
                (v.0.clone(), v.1)
            };
            let Some(view) = view.filter(|_| b.stamp >= stamp) else {
                self.acc.lock().unwrap().done += 1;
                continue;
            };
            let ph = ph.get_or_insert_with(|| Phosphor::new(view.w, view.h));
            if (ph.w, ph.h) != (view.w, view.h) {
                *ph = Phosphor::new(view.w, view.h);
            }
            let (mut frames, mut trig) = (0u64, 0u64);
            for chunk in b.bytes.chunks_exact(b.fb) {
                let f = (b.decode)(chunk);
                trig += draw_frame(ph, &view.m, &f, None) as u64;
                frames += 1;
            }
            let mut a = self.acc.lock().unwrap();
            if a.gen != view.gen || a.maps.as_ref().is_some_and(|x| (x.w, x.h) != (view.w, view.h)) {
                (a.gen, a.maps, a.frames, a.triggered) = (view.gen, None, 0, 0);
            }
            match a.maps.as_mut() {
                None => a.maps = Some(std::mem::replace(ph, Phosphor::new(view.w, view.h))),
                Some(acc) => {
                    add(acc, ph);
                    ph.clear();
                }
            }
            a.frames += frames;
            a.triggered += trig;
            a.done += 1;
        }
    }

    /// Adds what the worker drew under the current mapping to `ph` (a different size or an older mapping: dropped).
    /// `clear`: the picture is replaced by new frames (minimum persistence): cleared first if anything is merged.
    pub fn merge(&self, ph: &mut Phosphor, clear: &mut bool) -> Merged {
        let gen = self.view.lock().unwrap().0.as_ref().map(|v| v.gen);
        let mut a = self.acc.lock().unwrap();
        let maps = a.maps.take();
        let got = Merged { frames: a.frames, triggered: a.triggered };
        (a.frames, a.triggered) = (0, 0);
        let fresh = Some(a.gen) == gen;
        drop(a);
        match maps {
            Some(m) if fresh && (m.w, m.h) == (ph.w, ph.h) && got.frames > 0 => {
                if *clear {
                    ph.clear();
                    *clear = false;
                }
                add(ph, &m);
                got
            }
            _ => Merged::default(),
        }
    }

    #[cfg(test)]
    fn done(&self) -> u64 {
        self.acc.lock().unwrap().done
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

/// The pipe when OOS_DRAWPIPE is set (started on first use), else None.
pub fn pipe() -> Option<&'static Arc<Pipe>> {
    static P: OnceLock<Option<Arc<Pipe>>> = OnceLock::new();
    P.get_or_init(|| std::env::var("OOS_DRAWPIPE").is_ok_and(|v| !v.is_empty() && v != "0").then(|| Pipe::start(true))).as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;
    use oos_hal::dho::hz;
    use oos_hal::dho::seq::Settings;

    fn dec(s: &Settings, p: &hz::Params) -> Decode {
        let (s, p) = (s.clone(), *p);
        Box::new(move |c| crate::board::dho::hw::decode(c, &s, &p, 0))
    }

    /// A raw batch of `frames` frames, CH1 (and CH2 with `two`): a sine with a rising zero crossing at the hardware
    /// trigger point plus a per-frame jitter, as `hw::decode` reads it.
    fn batch(frames: usize, memsel: u32, two: bool) -> (Vec<u8>, usize, Settings, hz::Params) {
        let p = hz::Params { ratio: if two { 2 } else { 1 }, rate: 1_250_000_000 / if two { 2 } else { 1 }, memsel, dot_fs: 800_000, seg: 0, max_frames: 1000, pre: memsel / 2, post: memsel / 2 };
        let s = Settings { mask: if two { 3 } else { 1 }, window: None, compress: 1, header: false, ..Settings::default() };
        let fb = oos_hal::dho::seq::frame_bytes(&s, &p);
        let mut bytes = Vec::new();
        for k in 0..frames {
            let jit = (k % 7) as f64 * 0.13 - 0.4;
            for i in 0..memsel as usize {
                let ph = (i as f64 - memsel as f64 / 2.0 + jit) / 400.0 * std::f64::consts::TAU;
                for c in 0..if two { 2 } else { 1 } {
                    let v = 0.9 * (ph * (1.0 + c as f64 * 0.5)).sin() + if k % 5 == 0 { 0.05 } else { 0.0 };
                    let w = (32768.0 + v * 7500.0) as u16;
                    bytes.extend(w.to_le_bytes());
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
            assert!(t0.elapsed() < std::time::Duration::from_secs(10), "worker stuck at {} of {}", p.done(), done);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    // Break it catches: worker maps that differ from drawing the same frames on the UI path (decode, mapping, dense and
    // sparse frames, dots, two channels), frames lost or counted twice, a merge that does not clear for minimum
    // persistence.
    #[test]
    fn worker_maps_equal_the_ui_path() {
        let (w, h) = (970, 448);
        for (tdiv, memsel, two, dots) in [(1e-6, 12_512u32, false, false), (1e-6, 12_512, true, true), (1e-7, 1408, false, false), (2e-8, 400, true, false)] {
            let m = model(tdiv, two, dots);
            let (bytes, fb, s, p) = batch(40, memsel, two);
            let pipe = Pipe::start(false);
            pipe.set_view(Some(&m), w, h);
            // one batch in two parts: the second waits for the first, nothing is dropped
            let half = 17 * fb;
            assert!(pipe.feed(&bytes[..half], fb, 0, dec(&s, &p)));
            wait(&pipe, 1);
            assert!(pipe.feed(&bytes[half..], fb, 0, dec(&s, &p)));
            wait(&pipe, 2);
            let mut got = Phosphor::new(w, h);
            got.map[0][0] = 7;
            let mut clear = true;
            let n = pipe.merge(&mut got, &mut clear);
            let mut want = Phosphor::new(w, h);
            let mut trig = 0;
            for chunk in bytes.chunks_exact(fb) {
                trig += draw_frame(&mut want, &m, &crate::board::dho::hw::decode(chunk, &s, &p, 0), None) as u64;
            }
            assert_eq!(n, Merged { frames: 40, triggered: trig }, "{} s/div", tdiv);
            assert!(!clear && trig > 30, "{} s/div: {} triggered", tdiv, trig);
            for c in 0..NCH {
                assert!(got.map[c] == want.map[c], "{} s/div {} pt: CH{} differs", tdiv, memsel, c + 1);
            }
            // merged once: nothing the second time
            assert_eq!(pipe.merge(&mut got, &mut clear), Merged::default());
        }
    }

    // Break it catches: frames of older settings drawn, maps of an older mapping merged, batches drawn while the fast
    // path does not apply.
    #[test]
    fn stale_batches_and_maps_are_dropped() {
        let (w, h) = (970, 448);
        let mut m = model(1e-6, false, false);
        let (bytes, fb, s, p) = batch(10, 12_512, false);
        let pipe = Pipe::start(false);
        // no view: dropped
        assert!(pipe.feed(&bytes, fb, 0, dec(&s, &p)));
        wait(&pipe, 1);
        let mut ph = Phosphor::new(w, h);
        assert_eq!(pipe.merge(&mut ph, &mut false), Merged::default());
        // settings stamp older than the view's: dropped
        m.hw_stamp = 3;
        pipe.set_view(Some(&m), w, h);
        assert!(pipe.feed(&bytes, fb, 2, dec(&s, &p)));
        wait(&pipe, 2);
        assert_eq!(pipe.merge(&mut ph, &mut false), Merged::default());
        // drawn, then the mapping changes before the merge: dropped
        assert!(pipe.feed(&bytes, fb, 3, dec(&s, &p)));
        wait(&pipe, 3);
        m.ch[0].offset = 0.5;
        pipe.set_view(Some(&m), w, h);
        assert_eq!(pipe.merge(&mut ph, &mut false), Merged::default());
        assert!(ph.map.iter().all(|x| x.iter().all(|&y| y == 0)));
        // a change the drawing does not depend on keeps the maps
        assert!(pipe.feed(&bytes, fb, 3, dec(&s, &p)));
        wait(&pipe, 4);
        m.status = TrigStatus::Wait;
        pipe.set_view(Some(&m), w, h);
        assert_eq!(pipe.merge(&mut ph, &mut false).frames, 10);
    }

    /// Host throughput of the worker (run with --ignored --nocapture): frames per second it decodes and draws.
    #[test]
    #[ignore]
    fn worker_throughput() {
        let (w, h) = (970, 448);
        for (tdiv, memsel) in [(1e-7, 1408u32), (1e-6, 12_512)] {
            let m = model(tdiv, false, false);
            let (bytes, fb, s, p) = batch(200, memsel, false);
            let pipe = Pipe::start(false);
            pipe.set_view(Some(&m), w, h);
            let t0 = std::time::Instant::now();
            for k in 0..20 {
                assert!(pipe.feed(&bytes, fb, 0, dec(&s, &p)));
                wait(&pipe, k + 1);
            }
            let el = t0.elapsed();
            println!("{} s/div, {} pt: {:?} per frame, {:.0} frames/s", tdiv, memsel, el / 4000, 4000.0 / el.as_secs_f64());
        }
    }
}
