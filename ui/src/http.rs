// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Web remote (HTTP, port 8080): live screen in the browser, clicks act as touch taps, buttons for keys and knobs.
//!   GET /             page            GET /screen.png   current screen (PNG, 60 to 175 KB; the page's live view and saving;
//!                                     made off the frame loop, with an ETag: If-None-Match gets 304; see `Shots`)
//!   GET /screen.bmp   current screen (24-bit BMP, 1.8 MB; the page can show this instead)
//!   GET /screen.jpg   the live picture as JPEG from the VPU (`vpu.rs`; PNG where there is none), as /screen.png
//!   GET /wave.csv, /la.csv   the last record, as Storage > CSV writes it
//!   POST /waveform?name=...&slot=1   load a waveform into a chosen reference (no file written; parsed on a thread of
//!                     its own and put into the slot by the frame loop, see `WaveJob`)
//!   GET /ui-state     source, acquisition state and capture count for the remote controls
//!   GET /cmd?c=...    one key, knob or touch command (cmd.rs syntax: "key runstop", "knob vscale +1", "tap 100 200");
//!                     other commands are refused (`cmd::remote_ok`)
//!   GET /lxi/identification   LXI identification XML (LXI Device Specification 2011, 10.2)
//!   POST /mcp         Model Context Protocol (JSON-RPC 2.0, streamable HTTP transport, plain JSON responses): tools
//!                     `scpi` (any SCPI command of scpi.rs, returns the response) and `screenshot` (PNG)
//!   GET /update       update state and upload form; POST /update with a signed package (.oosupd) installs it
//!                     (ui/update.rs: the signature is checked before anything is written)
//! Unauthenticated: bench LAN only (an update must carry a trusted signature, but the community key is public).
//! `net.allow` (`netacl`, docs/network.md 6) limits `POST /update` and `/mcp` to the listed address prefixes.

use crate::gfx::Canvas;
use crate::ui::Ui;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Longest request: `POST /update` (a package is ~31 MB, the image at most 64 MiB), every other request.
const MAX_UPDATE: usize = 70 << 20;
const MAX_WAVEFORM: usize = crate::store::MAX_WAVEFORM_BYTES;
const MAX_OTHER: usize = 1 << 20;
/// Open connections at once (more are closed at accept).
const MAX_PENDING: usize = 8;
/// A body above this (an update package, a waveform) is taken one at a time: a second one is answered 503 when its
/// head is in, so the requests waiting hold at most one large body and `MAX_PENDING` small ones.
const LARGE_BODY: usize = MAX_OTHER;
/// Idle time and whole time a request may take (the upload of a package over 10 Mbit/s takes ~30 s).
const IDLE_S: u64 = 10;
const DEADLINE_S: u64 = 120;
/// Answers above this size are sent from a thread of their own: over WLAN a picture takes long enough to stall the
/// frame loop, and with it the scope's own screen and keys. A screen as PNG is 60 to 175 KB (a BMP 1.8 MB), so it
/// goes the same way; the page and the small answers fit into the socket's buffer and go at once.
const SEND_INLINE: usize = 48 << 10;
/// At most this many at once; more are answered 503, and the page tries again.
const MAX_SENDERS: usize = 2;
const SEND_TIMEOUT_S: u64 = 30;
static SENDERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A large answer in the background. False when too many run already, or no thread could be started.
fn send_in_background(s: &TcpStream, head: String, body: impl AsRef<[u8]> + Send + 'static) -> bool {
    use std::sync::atomic::Ordering::SeqCst;
    let Ok(mut c) = s.try_clone() else { return false };
    if SENDERS.fetch_add(1, SeqCst) >= MAX_SENDERS {
        SENDERS.fetch_sub(1, SeqCst);
        return false;
    }
    let t = std::thread::Builder::new().name("web-send".into()).spawn(move || {
        lower_priority();
        let _ = c.set_write_timeout(Some(Duration::from_secs(SEND_TIMEOUT_S)));
        let _ = c.write_all(head.as_bytes());
        let _ = c.write_all(body.as_ref());
        SENDERS.fetch_sub(1, SeqCst);
    });
    if t.is_err() {
        SENDERS.fetch_sub(1, SeqCst);
        return false;
    }
    true
}

/// Which instrument addresses the identification page lists. The default is the scope: VXI-11 and the SCPI socket.
#[derive(Clone, Debug)]
pub struct Offer {
    pub scpi_port: Option<u16>,
    pub vxi11: bool,
    pub hislip_port: Option<u16>,
}

impl Default for Offer {
    fn default() -> Offer {
        Offer { scpi_port: Some(5555), vxi11: true, hislip_port: None }
    }
}

fn busy_answer(s: &TcpStream) {
    let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
    let msg = "busy, try again";
    let _ = (&*s).write_all(format!("HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", msg.len(), msg).as_bytes());
}

/// Nice value of the threads of the web remote (Linux; a thread's own value), where `SCHED_IDLE` is not to be had.
const WEB_NICE: i32 = 10;

/// The threads of the web remote (encoder, senders) run only on what the scope leaves over: scheduling class
/// `SCHED_IDLE` (they get a core only while nothing else wants it), and only on the slower cores of a big.LITTLE SoC
/// (RK3399: the four A53; the frame loop keeps the two A72 for itself). Nice 10 where that fails.
fn lower_priority() {
    // not in the tests: with every core busy with tests in parallel (CI), a thread at SCHED_IDLE got no time for
    // seconds, and a viewer waiting for a picture got 304 or 503 instead
    #[cfg(target_os = "linux")]
    if !cfg!(test) {
        let tid = crate::system::idle_current_thread(WEB_NICE);
        let _ = crate::system::pin_thread(tid, &little_cores());
    }
}

/// The cores with less than the largest `cpu_capacity` (empty on a SoC whose cores are all alike, or without the
/// file): where the background threads go.
#[cfg(target_os = "linux")]
fn little_cores() -> Vec<usize> {
    static L: std::sync::OnceLock<Vec<usize>> = std::sync::OnceLock::new();
    L.get_or_init(|| {
        let cap: Vec<(usize, u32)> = (0..64)
            .filter_map(|c| {
                std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu{}/cpu_capacity", c))
                    .ok()
                    .and_then(|v| v.trim().parse().ok())
                    .map(|v| (c, v))
            })
            .collect();
        let max = cap.iter().map(|x| x.1).max().unwrap_or(0);
        cap.iter().filter(|x| x.1 < max).map(|x| x.0).collect()
    })
    .clone()
}

/// The slower cores of a big.LITTLE SoC (RK3399: 0-3), empty where the cores are all alike.
pub fn little_core_list() -> Vec<usize> {
    #[cfg(target_os = "linux")]
    return little_cores();
    #[cfg(not(target_os = "linux"))]
    Vec::new()
}

/// The calling thread onto `cores` (nothing when it is empty).
pub fn pin_to(cores: &[usize]) {
    #[cfg(target_os = "linux")]
    let _ = crate::system::pin_current_thread(cores);
    #[cfg(not(target_os = "linux"))]
    let _ = cores;
}

/// The calling thread onto the slower cores of a big.LITTLE SoC (RK3399: the four A53), at normal priority: the
/// measurements in the background (`ui::MeasWorker`); the threads it starts after this stay there too.
pub fn to_little_cores() {
    #[cfg(target_os = "linux")]
    let _ = crate::system::pin_current_thread(&little_cores());
}

/// The calling thread onto the fastest cores (RK3399: the two A72), and the processes it starts after this with it.
/// System > Graphics = GPU: the frame loop and the GPU helper compare and send the layers there (on an A53 a
/// frame's compare took three times as long).
#[cfg(target_os = "linux")]
pub fn pin_to_big_cores() {
    let little = little_cores();
    let all: Vec<usize> = (0..64)
        .filter(|c| std::path::Path::new(&format!("/sys/devices/system/cpu/cpu{}/cpu_capacity", c)).exists())
        .collect();
    let big: Vec<usize> = all.into_iter().filter(|c| !little.contains(c)).collect();
    if big.is_empty() || little.is_empty() {
        return;
    }
    let _ = crate::system::pin_current_thread(&big);
}

/// The fastest the live picture is made: 20 a second.
const SHOT_MIN: Duration = Duration::from_millis(50);
/// The encoder's share of its core at most (half of a slow core, which is idle otherwise): the time between two copies
/// is at least this many times the last encode.
const SHOT_DUTY: u32 = 2;
/// While the screen stands still, it is looked at again only this often.
const SHOT_STILL: Duration = Duration::from_millis(200);
/// After a request, pictures are made ahead for this long (a viewer that left costs no more than this).
const SHOT_AHEAD: Duration = Duration::from_secs(1);
/// A viewer that has the newest picture waits this long for the next one (long poll), then gets 304.
const LONG_POLL: Duration = if cfg!(test) { Duration::from_millis(1000) } else { Duration::from_secs(2) };
/// A picture from a copy at most this old goes out at once to a viewer that does not have it; else a fresh copy first.
const SHOT_FRESH: Duration = Duration::from_millis(300);
/// Connections waiting for a picture at most (more are answered 503).
const SHOT_WAITING: usize = 8;
/// After the VPU failed, JPEG is tried again this much later (PNG meanwhile).
const VPU_RETRY: Duration = Duration::from_secs(60);

/// The live picture of the web remote.
///
/// A viewer asks with the ETag of the picture it has (If-None-Match). Has it an older one, or none, it gets the newest
/// at once. Has it the newest, the request waits (long poll) until the screen has changed and a new picture is made,
/// at most `LONG_POLL`, then 304. So every new picture reaches the page without delay, and nothing is asked for in
/// vain.
///
/// The frame loop only copies the screen (2.4 MB), and only while somebody waits, and not more often than the encoder
/// can keep up with on a third of a slow core (`SHOT_DUTY`), at most 20 times a second. The encoder thread compares the
/// copy with the last picture, makes a PNG only of a changed screen and hands it to the waiting viewers; it and the
/// senders get only the time the scope leaves over (`lower_priority`). The PNG used to be made in the frame loop:
/// one viewer at 15 pictures a second took the scope's screen from 27 to 16 frames a second.
pub struct Shots {
    st: Mutex<ShotState>,
    wake: Condvar,
}

#[derive(Default)]
struct ShotState {
    /// a copy of the screen for the encoder: width, height, pixels
    next: Option<(u32, u32, Vec<u32>)>,
    /// the encoder works on a copy
    busy: bool,
    /// connections waiting for a picture newer than the ETag they have, since when, and in which format (JPEG; PNG
    /// once the VPU failed)
    waiting: Vec<(TcpStream, Option<String>, Instant, bool)>,
    /// the last picture: its pixels (taken out by the encoder while it compares), size, PNG, ETag
    px: Vec<u32>,
    size: (u32, u32),
    png: Option<Arc<[u8]>>,
    tag: String,
    /// when the last copy was taken, and the time to wait after it before the next
    at: Option<Instant>,
    /// the last request for the live picture: while it is recent, the next picture is made ahead of the next request
    asked: Option<Instant>,
    gap: Duration,
    /// pixel buffer for the next copy
    spare: Vec<u32>,
    /// PNGs made so far
    encodes: u64,
    /// the live picture as JPEG from the VPU (`/screen.jpg`, `vpu.rs`) rather than PNG: as the last request asked;
    /// the format of `png`; the VPU failed (PNG until `no_jpeg_at` + `VPU_RETRY`)
    jpeg: bool,
    pic_jpeg: bool,
    no_jpeg: bool,
    /// when the VPU failed: it is tried again after `VPU_RETRY` (one timeout must not end JPEG for the session)
    no_jpeg_at: Option<Instant>,
    /// tests: a long poll other than `LONG_POLL`
    #[cfg(test)]
    long_poll: Option<Duration>,
    /// the frame loop hands its screens over (`hand_over`) instead of `tick` copying them
    frames: bool,
    thread: bool,
    quit: bool,
}

impl Shots {
    fn new() -> Arc<Shots> {
        Arc::new(Shots { st: Default::default(), wake: Default::default() })
    }

    fn lock(&self) -> MutexGuard<'_, ShotState> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The answer to one `GET /screen.png` (`have`: the ETag of If-None-Match); the frame loop never encodes here.
    fn request(self: &Arc<Self>, s: &TcpStream, have: Option<String>, cv: &Canvas, jpeg: bool) {
        let mut st = self.lock();
        st.asked = Some(Instant::now());
        if st.no_jpeg && st.no_jpeg_at.is_some_and(|t| t.elapsed() >= VPU_RETRY) {
            st.no_jpeg = false;
        }
        st.jpeg = jpeg && !st.no_jpeg;
        // an older picture or none: the newest at once, when it is fresh (a screenshot must not be an old one) and in
        // the format asked for
        let fresh =
            st.at.is_some_and(|t| t.elapsed() < SHOT_FRESH) && !st.busy && st.next.is_none() && st.pic_jpeg == st.jpeg;
        if let Some(png) = st
            .png
            .clone()
            .filter(|_| fresh && have.as_deref() != Some(st.tag.as_str()) && st.size == (cv.w as u32, cv.h as u32))
        {
            let tag = st.tag.clone();
            drop(st);
            return answer_shot(s, have.as_deref(), png, &tag);
        }
        if st.waiting.len() >= SHOT_WAITING {
            drop(st);
            return busy_answer(s);
        }
        if !st.thread {
            let me = self.clone();
            if std::thread::Builder::new().name("web-png".into()).spawn(move || me.encoder()).is_err() {
                // no thread: the old way, in the frame loop
                drop(st);
                return answer_shot(s, None, crate::store::png_of(cv).into(), "");
            }
            st.thread = true;
        }
        let Ok(c) = s.try_clone() else { return };
        let want = st.jpeg;
        st.waiting.push((c, have, Instant::now(), want));
        drop(st);
        self.tick(cv);
    }

    /// Every frame: a copy for the encoder when the time has come and somebody waits, or asked within `SHOT_AHEAD` (the
    /// picture is then ready when the viewer's next request comes); 304 to whoever waited too long.
    fn tick(&self, cv: &Canvas) {
        let mut st = self.lock();
        let watched = st.asked.is_some_and(|t| t.elapsed() < SHOT_AHEAD);
        if st.waiting.is_empty() && !watched {
            return;
        }
        let now = Instant::now();
        #[cfg(test)]
        let long_poll = st.long_poll.unwrap_or(LONG_POLL);
        #[cfg(not(test))]
        let long_poll = LONG_POLL;
        let (old, keep): (Vec<_>, Vec<_>) =
            std::mem::take(&mut st.waiting).into_iter().partition(|w| now - w.2 >= long_poll);
        st.waiting = keep;
        if !old.is_empty() {
            let (png, tag) = (st.png.clone().unwrap_or_else(|| Arc::from(&[][..])), st.tag.clone());
            let fit = |w: bool| Self::fits(w, st.pic_jpeg, st.no_jpeg);
            let fits: Vec<bool> = old.iter().map(|w| fit(w.3)).collect();
            drop(st);
            for ((c, have, _, _), ok) in old.into_iter().zip(fits) {
                // the last picture in the format asked for, else "busy" (the page asks again)
                if ok {
                    answer_shot(&c, have.as_deref(), png.clone(), &tag);
                } else {
                    busy_answer(&c);
                }
            }
            st = self.lock();
        }
        if !st.frames && Self::wanted(&st, now, watched) {
            Self::give_copy(&mut st, cv, now);
            self.wake.notify_one();
        }
    }

    /// A copy of the screen for the encoder (the caller wakes it).
    fn give_copy(st: &mut ShotState, cv: &Canvas, now: Instant) {
        let mut px = std::mem::take(&mut st.spare);
        px.clear();
        px.extend_from_slice(&cv.px);
        st.next = Some((cv.w as u32, cv.h as u32, px));
        st.at = Some(now);
    }

    /// A picture in JPEG (`pic_jpeg`) or PNG answers a request for `want_jpeg`: the same format, or PNG for JPEG once
    /// the VPU failed. A screenshot asked for as PNG must never get JPEG bytes (it is saved as .png).
    fn fits(want_jpeg: bool, pic_jpeg: bool, no_jpeg: bool) -> bool {
        want_jpeg == pic_jpeg || (want_jpeg && no_jpeg && !pic_jpeg)
    }

    /// Whether the encoder should get the screen now.
    fn wanted(st: &ShotState, now: Instant, watched: bool) -> bool {
        (!st.waiting.is_empty() || watched) && !st.busy && st.next.is_none() && st.at.is_none_or(|t| now - t >= st.gap)
    }

    /// Right before the frame loop renders the next screen (which clears and draws all of it): the shown screen goes to
    /// the encoder by swapping buffers, no copy. A copy of 2.4 MB in the frame loop pushed everything else of the frame
    /// out of the fast core's cache and made the whole frame about 10 % slower. `whole`: the next render draws every
    /// pixel (not the start-up screen); else nothing is swapped and `tick` copies.
    fn hand_over(&self, cv: &mut Canvas, whole: bool) {
        let mut st = self.lock();
        st.frames = whole;
        let now = Instant::now();
        let watched = st.asked.is_some_and(|t| t.elapsed() < SHOT_AHEAD);
        if !whole || !Self::wanted(&st, now, watched) {
            return;
        }
        let mut px = std::mem::take(&mut st.spare);
        px.resize(cv.px.len(), 0);
        std::mem::swap(&mut px, &mut cv.px);
        st.next = Some((cv.w as u32, cv.h as u32, px));
        st.at = Some(now);
        self.wake.notify_one();
    }

    fn encoder(&self) {
        lower_priority();
        let mut vpu: Option<crate::vpu::JpegEnc> = None;
        let mut vpu_size = (0, 0);
        let boot = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64 & 0xffff_ffff);
        loop {
            let mut st = self.lock();
            while st.next.is_none() && !st.quit {
                st = self.wake.wait(st).unwrap_or_else(|e| e.into_inner());
            }
            if st.quit {
                return;
            }
            let Some((w, h, px)) = st.next.take() else { continue };
            st.busy = true;
            let last = std::mem::take(&mut st.px);
            // the format of the oldest request waiting (one viewer may ask for JPEG, another for a PNG screenshot)
            let jpeg = st.waiting.first().map_or(st.jpeg, |w| w.3) && !st.no_jpeg;
            let known = st.png.is_some() && st.size == (w, h) && st.pic_jpeg == jpeg;
            drop(st);
            // the comparison and the encoding without the lock: the frame loop may ask meanwhile
            let t0 = Instant::now();
            let mut vpu_failed = false;
            let png: Option<Arc<[u8]>> = (!known || !same_pixels(&last, &px)).then(|| {
                if jpeg {
                    if vpu_size != (w, h) {
                        vpu = None;
                    }
                    if vpu.is_none() {
                        match crate::vpu::JpegEnc::open(w as usize, h as usize) {
                            Ok(e) => {
                                eprintln!("oos-ui: JPEG encoder: VPU, {} in", e.input());
                                (vpu, vpu_size) = (Some(e), (w, h));
                            }
                            Err(e) => eprintln!("oos-ui: JPEG encoder: {}", e),
                        }
                    }
                    match vpu.as_mut().map(|e| e.encode(&px)) {
                        Some(Ok(j)) => return j.into(),
                        Some(Err(e)) => eprintln!("oos-ui: JPEG encoder: {}", e),
                        None => {}
                    }
                    (vpu, vpu_failed) = (None, true);
                }
                crate::store::png(w, h, &px).into()
            });
            let took = t0.elapsed();
            let mut st = self.lock();
            st.busy = false;
            if vpu_failed {
                (st.no_jpeg, st.jpeg, st.no_jpeg_at) = (true, false, Some(Instant::now()));
            }
            let Some(png) = png else {
                // the same screen: who has an older picture gets this one, who has it waits on; the next look later
                st.gap = SHOT_STILL.max(took * SHOT_DUTY);
                st.px = last;
                st.spare = px;
                let tag = st.tag.clone();
                let (pj, nj) = (st.pic_jpeg, st.no_jpeg);
                let (lack, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut st.waiting)
                    .into_iter()
                    .partition(|w| w.1.as_deref() != Some(tag.as_str()) && Self::fits(w.3, pj, nj));
                st.waiting = keep;
                let png = st.png.clone().unwrap_or_else(|| Arc::from(&[][..]));
                drop(st);
                for (c, have, _, _) in lack {
                    answer_shot(&c, have.as_deref(), png.clone(), &tag);
                }
                continue;
            };
            st.gap = SHOT_MIN.max(took * SHOT_DUTY);
            st.encodes += 1;
            st.tag = format!("\"{:x}-{}\"", boot, st.encodes);
            st.pic_jpeg = png.starts_with(&[0xff, 0xd8]);
            st.png = Some(png.clone());
            st.size = (w, h);
            st.px = px;
            st.spare = last;
            // who asked for the other format waits for the next picture, made in that format
            let (pj, nj) = (st.pic_jpeg, st.no_jpeg);
            let (waiting, other): (Vec<_>, Vec<_>) =
                std::mem::take(&mut st.waiting).into_iter().partition(|w| Self::fits(w.3, pj, nj));
            st.waiting = other;
            let tag = st.tag.clone();
            drop(st);
            for (c, have, _, _) in waiting {
                answer_shot(&c, have.as_deref(), png.clone(), &tag);
            }
        }
    }
}

/// Whether two screens are the same, eight bytes at a step. `==` on the slices ends in musl's memcmp, which goes byte
/// by byte: 12 ms for a screen on an A53, against about 1 ms here.
pub fn same_pixels(a: &[u32], b: &[u32]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let ((ca, ra), (cb, rb)) = (a.as_chunks::<8>(), b.as_chunks::<8>());
    // 32 bytes at a time with an OR of the differences: no early exit per word, which lets the compiler use vectors
    ca.iter().zip(cb).all(|(x, y)| x.iter().zip(y).fold(0u32, |d, (p, q)| d | (p ^ q)) == 0) && ra == rb
}

/// 304 when the viewer has this picture, else the PNG through a sender thread (503 when all are busy).
fn answer_shot(s: &TcpStream, have: Option<&str>, png: Arc<[u8]>, tag: &str) {
    let etag = if tag.is_empty() { String::new() } else { format!("ETag: {}\r\n", tag) };
    if !tag.is_empty() && have == Some(tag) {
        let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
        let _ = (&*s).write_all(
            format!("HTTP/1.1 304 Not Modified\r\n{}Cache-Control: no-store\r\nConnection: close\r\n\r\n", etag)
                .as_bytes(),
        );
        return;
    }
    let ctype = if png.starts_with(&[0xff, 0xd8]) { "image/jpeg" } else { "image/png" };
    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n{}Cache-Control: no-store\r\nConnection: close\r\n\r\n", ctype, png.len(), etag);
    if !send_in_background(s, head, png) {
        busy_answer(s);
    }
}

/// A request being read.
struct Pending {
    s: TcpStream,
    /// the request so far
    req: Vec<u8>,
    /// last data, accepted
    t: Instant,
    t_acc: Instant,
    /// the length of the whole request, once its head is in and the body is taken
    need: Option<usize>,
    /// its body is over `LARGE_BODY`: the one large body being read
    large: bool,
}

/// A waveform upload (`POST /waveform`) parsed on a thread of its own: a file
/// near the 128 MiB limit takes seconds to parse, which on the frame loop stopped the scope's screen and keys. The
/// frame loop puts the traces into the slot (`Ui::commit_waveform_traces_at`) and answers the client, which waits.
struct WaveJob {
    s: TcpStream,
    name: String,
    /// R1 is 0
    slot: usize,
    /// `Ui::waveform_epoch` at the upload: a session changed meanwhile does not get the late file
    epoch: u64,
    parsed: std::sync::mpsc::Receiver<Result<Vec<crate::store::WaveTrace>, String>>,
}

impl WaveJob {
    /// Starts parsing `req[body..]`; None when no thread could be started.
    fn start(s: TcpStream, req: Vec<u8>, body: usize, name: String, slot: usize, epoch: u64) -> Option<WaveJob> {
        let (tx, parsed) = std::sync::mpsc::channel();
        let file = name.clone();
        std::thread::Builder::new()
            .name("web-wave".into())
            .spawn(move || {
                lower_priority();
                let _ = tx.send(crate::store::parse_wave(&req[body..], &file));
            })
            .ok()?;
        Some(WaveJob { s, name, slot, epoch, parsed })
    }

    /// The answer once the parse is done: the traces in the slot, or why not. None while it runs.
    fn finish(&self, ui: &mut Ui) -> Option<(bool, String)> {
        let parsed = match self.parsed.try_recv() {
            Ok(parsed) => parsed,
            Err(std::sync::mpsc::TryRecvError::Empty) => return None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("The waveform reader stopped; try again.".into()),
        };
        let previous = ui.ref_slot();
        let result = if ui.waveform_epoch() != self.epoch {
            Err("The session changed while the file was read; load it again.".to_string())
        } else {
            parsed.and_then(|traces| ui.commit_waveform_traces_at(traces, &self.name, self.slot))
        };
        Some(match result {
            Ok(()) => {
                let hint = if ui.record_mode() {
                    "Use Run/Stop or Single to acquire from this record; Return to live restores the live source."
                } else {
                    "It is on with the live channels. Fit shows its full record; Take over enters stored acquisition."
                };
                (true, format!("Loaded {} into R{}. {}", self.name, ui.ref_slot() + 1, hint))
            }
            Err(error) => {
                ui.ref_slot_set(previous);
                (false, error)
            }
        })
    }
}

pub struct Http {
    l: Option<TcpListener>,
    pending: Vec<Pending>,
    /// the waveform upload being parsed
    wave: Option<WaveJob>,
    shots: Arc<Shots>,
    /// who may install an update or use MCP (empty: everyone); checked per request, so a change counts at once
    pub acl: crate::netacl::Shared,
    /// the addresses the identification page names
    offer: Offer,
}

impl Drop for Http {
    fn drop(&mut self) {
        self.shots.lock().quit = true;
        self.shots.wake.notify_all();
    }
}

/// Style of both pages (the update page uses it too).
pub const STYLE: &str = r#"*{box-sizing:border-box}
body{background:#14171c;color:#e4e7ea;font:15px -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;margin:0 auto;max-width:1048px;padding:12px}
header{display:flex;align-items:center;gap:14px;margin-bottom:10px}header b{font-size:17px}header nav{margin-left:auto;display:flex;gap:14px}
a{color:#5cb2f0;text-decoration:none}a:hover{text-decoration:underline}
#st{color:#9aa4af;font-size:13px}#st.bad{color:#f0a93b}
button{background:#242a33;color:#e4e7ea;border:1px solid #3a424d;border-radius:6px;padding:7px 11px;font:inherit;cursor:pointer;touch-action:manipulation}
button:hover{background:#2f3743}button:active{background:#3a4452}button.on{border-color:#f0a93b;color:#f0a93b}
button:focus-visible,a:focus-visible,select:focus-visible{outline:2px solid #f0a93b;outline-offset:3px}button:disabled{opacity:.55;cursor:wait}
select{background:#242a33;color:#e4e7ea;border:1px solid #3a424d;border-radius:6px;padding:7px;font:inherit}
pre{background:#0d0f13;border:1px solid #2a3038;border-radius:6px;padding:12px;overflow-x:auto;white-space:pre-wrap}
code{font:13px ui-monospace,Menlo,Consolas,monospace}"#;

const PAGE: &str = r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>OOS remote</title>
<link rel="stylesheet" href="/style.css">
<style>img{display:block;width:100%;height:auto;border:1px solid #3a424d;border-radius:6px;cursor:crosshair;touch-action:none;user-select:none;-webkit-user-select:none}
.g{display:flex;flex-wrap:wrap;gap:12px 22px;margin-top:12px}.k span{display:block;font-size:12px;color:#9aa4af;margin-bottom:3px}.k div{display:flex;flex-wrap:wrap;gap:4px}
.workflow{display:flex;flex-wrap:wrap;align-items:center;gap:8px;margin:0 0 8px}.workflow label{display:flex;align-items:center;gap:6px;color:#b8c0ca;font-size:13px}.workflow .primary{border-color:#f0a93b;color:#f0a93b}.session{margin-left:auto;color:#b8c0ca;font-size:13px;font-variant-numeric:tabular-nums}.file-hint{min-height:20px;margin:0 0 8px;color:#b8c0ca;font-size:13px}.file-hint.bad{color:#ffab91}header{flex-wrap:wrap}@media(max-width:620px){.session{margin-left:0;flex-basis:100%}.workflow{gap:6px}button{min-height:40px}}</style></head>
<body><header><b>OOS remote</b><span id="st">connecting ...</span><nav><a href="/update">Update</a></nav></header>
<div class="workflow" aria-label="Waveform and source actions">
<button id="load-wave" class="primary" onclick="askFile()">Load waveform</button>
<input id="wave-file" type="file" accept=".csv,.bin,.wfm,.trc,.isf,.tr0,.wfmx" hidden>
<label>Into <select id="wave-slot" aria-label="Reference slot"><option value="" selected>Choose slot</option><option value="1">R1</option><option value="2">R2</option><option value="3">R3</option><option value="4">R4</option><option value="5">R5</option><option value="6">R6</option><option value="7">R7</option><option value="8">R8</option><option value="9">R9</option><option value="10">R10</option></select></label>
<button id="sim-mode" onclick="c('workflow sim')">SIM</button><button id="return-live" onclick="c('workflow live')" hidden>Return to live</button>
<button id="acquire-toggle" onclick="c('key runstop')" aria-label="Run or stop acquisition">Run/Stop</button><button onclick="c('key single')">Single</button>
<button onclick="c('workflow measure')">Add measurement</button><span id="session-state" class="session" aria-live="polite">Reading source...</span></div>
<p id="wave-status" class="file-hint" role="status">Choose a slot, then CSV, BIN, WFM, TRC, ISF, TR0 or WFMX · up to 128 MiB. You can also drop a file on the waveform.</p>
<img id="s" width="1024" height="600" alt="scope screen" src="/screen.png">
<div class="g"><div class="k"><span>Acquisition</span><div>
<button onclick="c('key runstop')">Run/Stop</button><button onclick="c('key single')">Single</button><button onclick="c('key auto')">Autoset</button><button onclick="c('key clear')">Clear</button><button onclick="c('key default')">Default</button></div></div>
<div class="k"><span>Channels</span><div>
<button onclick="c('key ch1')">CH1</button><button onclick="c('key ch2')">CH2</button><button onclick="c('key ch3')">CH3</button><button onclick="c('key ch4')">CH4</button><button onclick="c('key math')">Math</button><button onclick="c('key ref')">Ref</button><button onclick="c('key la')">LA</button><button onclick="c('key gi')">Gen</button></div></div>
<div class="k"><span>Tools</span><div>
<button onclick="c('workflow settings')">Measure settings</button><button onclick="c('key cursor')">Cursor</button><button onclick="c('key analyse')">Decode</button><button onclick="c('key tmenu')">Trigger</button><button onclick="c('key force')">Force trigger</button></div></div></div>
<div class="g">
<div class="k"><span>V scale</span><div><button onclick="c('knob vscale -1')">-</button><button onclick="c('knob vscale +1')">+</button></div></div>
<div class="k"><span>V pos</span><div><button onclick="c('knob vpos -5')">-</button><button onclick="c('knob vpos +5')">+</button></div></div>
<div class="k"><span>H scale</span><div><button onclick="c('knob hscale -1')">-</button><button onclick="c('knob hscale +1')">+</button><button onclick="c('knob hscale push')">zoom</button></div></div>
<div class="k"><span>H pos</span><div><button onclick="c('knob hpos -5')">-</button><button onclick="c('knob hpos +5')">+</button></div></div>
<div class="k"><span>Trig level</span><div><button onclick="c('knob tlevel -5')">-</button><button onclick="c('knob tlevel +5')">+</button><button onclick="c('knob tlevel push')">50%</button></div></div>
<div class="k"><span>Multi 1</span><div><button onclick="c('knob multi1 -1')">-</button><button onclick="c('knob multi1 +1')">+</button><button onclick="c('knob multi1 push')">push</button></div></div>
<div class="k"><span>Multi 2</span><div><button onclick="c('knob multi2 -1')">-</button><button onclick="c('knob multi2 +1')">+</button></div></div></div>
<div class="g"><div class="k"><span>Save to this computer</span><div>
<button onclick="dl('/screen.png','png')">Screenshot (PNG)</button><button onclick="dl('/wave.csv','csv')">Waveform (CSV)</button><button onclick="dl('/wave.bin','bin')">Waveform (binary)</button><button onclick="dl('/la.csv','la.csv')">LA lines (CSV)</button></div></div>
<div class="k"><span>Browser picture only</span><div><button id="p" onclick="pause()">Pause picture</button></div></div>
<div class="k"><span>Live picture format</span><div><button id="fp" onclick="setfmt('png')" title="compressed, about 90 KB per picture">PNG</button><button id="fb" onclick="setfmt('bmp')" title="uncompressed, 1.8 MB per picture">BMP</button><button id="fj" onclick="setfmt('jpg')" title="made by the scope's video encoder, about 70 KB per picture: up to 20 a second, but text a little less sharp">JPEG</button></div></div></div>
<script>
const s=document.getElementById('s'),st=document.getElementById('st'),pb=document.getElementById('p'),fp=document.getElementById('fp'),fb=document.getElementById('fb'),fj=document.getElementById('fj');
let paused=false,running=false,n=0,msg='',until=0,fmt='png';
try{const f=localStorage.getItem('fmt');if(f=='bmp'||f=='jpg')fmt=f;}catch(e){}
function setfmt(f){fmt=f;try{localStorage.setItem('fmt',f);}catch(e){}fp.className=f=='png'?'on':'';fb.className=f=='bmp'?'on':'';fj.className=f=='jpg'?'on':'';}
setfmt(fmt);
function say(t){msg=t;until=Date.now()+5000;show();}
function show(bad){st.textContent=Date.now()<until?msg:(paused?'picture paused':bad?'no connection':'connected, '+n+' pictures/s');st.className=bad?'bad':'';}
function c(x){return fetch('/cmd?c='+encodeURIComponent(x)).then(async r=>{const text=await r.text();if(!r.ok||text==='refused')throw Error(text);refreshState();}).catch(()=>say('Command not sent. Check the connection.'));}
let statePending=false,uploading=false,workflowState=null;
function askFile(){const slot=document.getElementById('wave-slot'),status=document.getElementById('wave-status');if(!slot.value){status.textContent='Choose a slot from R1 to R10 before the file.';status.className='file-hint bad';return;}document.getElementById('wave-file').click();}
function refreshState(){if(statePending||document.hidden)return;statePending=true;fetch('/ui-state').then(r=>{if(!r.ok)throw Error();return r.json();}).then(v=>{
const mode=v.mode==='sim'?'SIM':v.mode==='reference'?'Stored waveform':'Live';
workflowState=v;
document.getElementById('session-state').textContent=[mode,v.source,v.status,(v.captures>0?'Capture '+v.captures:'')].filter(Boolean).join(' · ');
document.getElementById('sim-mode').className=v.mode==='sim'?'on':'';document.getElementById('return-live').hidden=v.mode==='live';
document.getElementById('acquire-toggle').textContent=v.running?'Stop':'Run';
}).catch(()=>{}).finally(()=>{statePending=false;});}
async function uploadWave(file){if(!file||uploading)return;const status=document.getElementById('wave-status'),button=document.getElementById('load-wave'),slot=document.getElementById('wave-slot');
if(!slot.value){status.textContent='Choose a slot from R1 to R10 before the file.';status.className='file-hint bad';document.getElementById('wave-file').value='';return;}
if(file.size>128*1024*1024){status.textContent='That file is larger than 128 MiB. Export a shorter waveform and try again.';status.className='file-hint bad';return;}
uploading=true;button.disabled=true;slot.disabled=true;button.textContent='Loading...';status.className='file-hint';status.textContent='Loading '+file.name+' into R'+slot.value+'...';
try{const response=await fetch('/waveform?name='+encodeURIComponent(file.name)+'&slot='+encodeURIComponent(slot.value),{method:'POST',headers:{'Content-Type':'application/octet-stream'},body:file});const text=await response.text();if(!response.ok)throw Error(text);status.textContent=text;refreshState();}
catch(error){status.textContent='Could not load waveform: '+(error.message||'check the connection and try again.');status.className='file-hint bad';}
finally{uploading=false;button.disabled=false;slot.disabled=false;button.textContent='Load waveform';document.getElementById('wave-file').value='';}}
document.getElementById('wave-file').addEventListener('change',event=>uploadWave(event.target.files[0]));
s.addEventListener('dragover',event=>{if(Array.from(event.dataTransfer?.types||[]).includes('Files'))event.preventDefault();});
s.addEventListener('drop',event=>{event.preventDefault();uploadWave(event.dataTransfer.files[0]);});
document.addEventListener('keydown',event=>{if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='o'){event.preventDefault();askFile();}});
refreshState();
function dl(u,ext){say('saving ...');Promise.all([fetch(u).then(r=>r.ok?r.blob():r.text().then(t=>Promise.reject(t))),fetch('/stamp').then(r=>r.ok?r.text():'').catch(()=>'')]).then(([b,t])=>{
const a=document.createElement('a'),d=t.trim()||new Date(Date.now()-new Date().getTimezoneOffset()*60000).toISOString().slice(0,19).replace(/-|:/g,'').replace('T','-');
a.href=URL.createObjectURL(b);a.download='oos-'+d+'.'+ext;document.body.appendChild(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(a.href),10000);say('saved');
}).catch(e=>say(typeof e=='string'?e:'not saved: no connection'));}
function pause(){paused=!paused;pb.textContent=paused?'Resume picture':'Pause picture';pb.className=paused?'on':'';show();go();}
function pt(e){const r=s.getBoundingClientRect();const p=e.changedTouches?e.changedTouches[0]:e;return [Math.round((p.clientX-r.left)*(s.naturalWidth||1024)/r.width),Math.round((p.clientY-r.top)*(s.naturalHeight||600)/r.height)];}
let d0=null;
function down(e){e.preventDefault();d0=pt(e);}
function up(e){if(!d0)return;e.preventDefault();const p=pt(e),start=d0;d0=null;if(Math.abs(p[0]-start[0])+Math.abs(p[1]-start[1])<8){c('tap '+start[0]+' '+start[1]);}else c('drag '+start[0]+' '+start[1]+' '+p[0]+' '+p[1]);}
s.addEventListener('mousedown',down);s.addEventListener('mouseup',up);s.addEventListener('touchstart',down,{passive:false});s.addEventListener('touchend',up,{passive:false});
s.ondragstart=()=>false;
let count=0;setInterval(()=>{n=count;count=0;if(!document.hidden){refreshState();if(!paused)show(st.className=='bad');}},1000);
// no pictures while paused or in a background tab: each one is 90 KB as PNG, 1.8 MB as BMP, which the scope has to send
// a PNG comes with an ETag: sent back, the scope holds the request until its screen has changed (304 after 2 s), so
// the next request can go at once; a BMP has none, so a pause between them
let tag='',shown='';
function next(ok){running=false;show(!ok);setTimeout(go,!ok?1000:fmt!='bmp'?0:150);}
function go(){if(running||paused||document.hidden)return;running=true;const f=fmt;
fetch('/screen.'+f+'?t='+Date.now(),{cache:'no-store',headers:tag&&f!='bmp'?{'If-None-Match':tag}:{}}).then(r=>{
if(r.status==304)return next(true);
if(!r.ok)return next(false);
const t=f!='bmp'&&r.headers.get('ETag')||'';
return r.blob().then(b=>{const u=URL.createObjectURL(b),i=new Image();
i.onload=()=>{s.src=u;if(shown)URL.revokeObjectURL(shown);shown=u;tag=t;count++;next(true);};
i.onerror=()=>{URL.revokeObjectURL(u);next(false);};i.src=u;});
}).catch(()=>next(false));}
document.addEventListener('visibilitychange',go);go();
</script></body></html>"#;

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                // on the bytes: the two characters after % need not be ASCII
                let hex = |c: u8| (c as char).to_digit(16);
                if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                    out.push((h * 16 + l) as u8);
                }
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A browser supplies only a display name. It never selects a path on the instrument.
fn waveform_name(query: &str) -> String {
    let decoded = query.split('&').find_map(|part| part.strip_prefix("name=")).map(unescape).unwrap_or_default();
    let leaf = decoded.rsplit(['/', '\\']).next().unwrap_or("");
    let name: String = leaf.chars().filter(|c| !c.is_control()).take(128).collect();
    if name.trim().is_empty() {
        "waveform.csv".into()
    } else {
        name
    }
}

impl Http {
    pub fn open(port: u16) -> Http {
        Http {
            l: Self::bind(port),
            pending: Vec::new(),
            wave: None,
            shots: Shots::new(),
            acl: crate::netacl::Shared::default(),
            offer: Offer::default(),
        }
    }

    fn bind(port: u16) -> Option<TcpListener> {
        let l = crate::net6::tcp(port);
        match &l {
            Some(l) => {
                let _ = l.set_nonblocking(true);
                eprintln!(
                    "oos-ui: web remote on port {}{}",
                    port,
                    if l.local_addr().is_ok_and(|a| a.is_ipv6()) { " (IPv4 and IPv6)" } else { "" }
                );
            }
            None => eprintln!("oos-ui: web port {} not available", port),
        }
        l
    }

    pub fn port(&self) -> Option<u16> {
        self.l.as_ref().and_then(|l| l.local_addr().ok()).map(|a| a.port())
    }

    /// The addresses the identification page names. Set from the listeners that actually came up.
    pub fn offer(&mut self, o: Offer) {
        self.offer = o;
    }

    /// System > Network > Remote services: the port open (`Some`), or closed with the requests waiting on it.
    pub fn set_open(&mut self, port: Option<u16>) {
        match port {
            Some(p) if self.l.is_none() => self.l = Self::bind(p),
            Some(_) => {}
            None => {
                if self.l.take().is_some() {
                    eprintln!("oos-ui: web remote off");
                }
                self.pending.clear();
            }
        }
    }

    /// Call right before rendering the next screen: hands the shown one to the live picture (`Shots::hand_over`).
    /// `whole`: the render draws every pixel.
    pub fn before_render(&mut self, cv: &mut Canvas, whole: bool) {
        self.shots.hand_over(cv, whole);
    }

    /// System > Graphics = GPU: the frame loop hands over the pictures that come back from the GPU
    /// (`picture_read_back`), `tick` copies none and nothing is swapped. `cv` holds the last picture read back, not
    /// one drawn every frame: a buffer swapped out of it would stay in it, an older picture, until the next read.
    pub fn before_gpu_frame(&mut self) {
        self.shots.lock().frames = true;
    }

    /// System > Graphics = GPU: `cv` was just read back whole; a copy goes to the encoder when it wants one.
    pub fn picture_read_back(&self, cv: &Canvas) {
        let mut st = self.shots.lock();
        let watched = st.asked.is_some_and(|t| t.elapsed() < SHOT_AHEAD);
        let now = Instant::now();
        if Shots::wanted(&st, now, watched) {
            Shots::give_copy(&mut st, cv, now);
            self.shots.wake.notify_one();
        }
    }

    /// The live picture would take the next screen (System > Graphics = GPU: read it back for it).
    pub fn picture_wanted(&self) -> bool {
        let st = self.shots.lock();
        let watched = st.asked.is_some_and(|t| t.elapsed() < SHOT_AHEAD);
        Shots::wanted(&st, Instant::now(), watched)
    }

    /// Accepts connections, answers complete requests; text commands are returned for the caller to run.
    pub fn poll(&mut self, ui: &mut Ui, cv: &Canvas, scpi: &mut crate::scpi::Scpi) -> Vec<String> {
        let mut cmds = Vec::new();
        self.shots.tick(cv);
        self.finish_wave(ui);
        self.accept_new();
        let mut i = 0;
        while i < self.pending.len() {
            let mut buf = [0u8; 65536];
            // a waveform being parsed holds its body too
            let large_taken = self.pending.iter().any(|p| p.large) || self.wave.is_some();
            let Pending { s, req, t, t_acc, need, large } = &mut self.pending[i];
            let mut dead = read_some(s, req, t, *need, &mut buf);
            // headers complete, and the body when there is a Content-Length
            let hdr_end = req.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4);
            if hdr_end.is_none() && req.len() > 16384 {
                dead = true; // no header end in 16 KiB
            }
            let mut too_big = false;
            // answered when the head is in, before any of the body is kept: 403 refused, 503 busy
            let mut early = None;
            let complete = hdr_end.is_some_and(|e| {
                let h = String::from_utf8_lossy(&req[..e]).to_ascii_lowercase();
                let n = h
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                let first = h.lines().next().unwrap_or("");
                let upload_path = first.split_whitespace().nth(1).unwrap_or("").split('?').next().unwrap_or("");
                let limit = if first.starts_with("post ") && upload_path == "/update" {
                    MAX_UPDATE
                } else if first.starts_with("post ") && upload_path == "/waveform" {
                    MAX_WAVEFORM
                } else {
                    MAX_OTHER
                };
                too_big = n > limit || req.len() > limit + e;
                if !too_big && need.is_none() {
                    // what only `net.allow` may send is refused before its body is read, not after
                    let guarded = upload_path == "/mcp" || (first.starts_with("post ") && upload_path == "/update");
                    if guarded && !self.acl.permits_peer(s.peer_addr()) {
                        early = Some("403 Forbidden");
                        return false;
                    }
                    if n > LARGE_BODY && large_taken {
                        early = Some("503 Service Unavailable");
                        return false;
                    }
                    // the whole request in one allocation, not a doubling past it
                    req.reserve_exact((e + n).saturating_sub(req.len()));
                    *need = Some(e + n);
                    *large = n > LARGE_BODY;
                }
                too_big || e.checked_add(n).is_some_and(|need| req.len() >= need)
            });
            if let Some(status) = early {
                let msg = if status.starts_with("403") { "refused" } else { "busy, try again" };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\
                    Cache-Control: no-store\r\nConnection: close\r\n\r\n{msg}",
                    msg.len()
                );
                let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
                let _ = s.write_all(head.as_bytes());
                dead = true;
            } else if too_big {
                let msg = b"request too large";
                let head = format!("HTTP/1.1 413 Payload Too Large\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", msg.len());
                let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(msg);
                dead = true;
            } else if complete {
                if !self.answer(i, hdr_end, ui, cv, scpi, &mut cmds) {
                    continue;
                }
                dead = true;
            } else if t.elapsed().as_secs() > IDLE_S || t_acc.elapsed().as_secs() > DEADLINE_S {
                dead = true;
            }
            if dead {
                self.pending.remove(i);
            } else {
                i += 1;
            }
        }
        cmds
    }

    /// Answers the waveform upload whose parsing has finished.
    fn finish_wave(&mut self, ui: &mut Ui) {
        if let Some((ok, text)) = self.wave.as_ref().and_then(|job| job.finish(ui)) {
            let mut job = self.wave.take().unwrap();
            let status = if ok { "200 OK" } else { "409 Conflict" };
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\
                Cache-Control: no-store\r\nConnection: close\r\n\r\n",
                text.len()
            );
            let _ = job.s.set_write_timeout(Some(Duration::from_secs(2)));
            let _ = job.s.write_all(head.as_bytes());
            let _ = job.s.write_all(text.as_bytes());
        }
    }

    /// Takes the connections waiting at the listener, up to `MAX_PENDING`.
    fn accept_new(&mut self) {
        if let Some(l) = &self.l {
            while let Ok((s, _)) = l.accept() {
                if self.pending.len() >= MAX_PENDING {
                    continue; // dropped: closes the connection
                }
                let _ = s.set_nonblocking(false);
                let now = Instant::now();
                self.pending.push(Pending { s, req: Vec::new(), t: now, t_acc: now, need: None, large: false });
            }
        }
    }

    /// Answers the complete request of connection `i`; false when the connection was handed on and is gone.
    fn answer(
        &mut self,
        i: usize,
        hdr_end: Option<usize>,
        ui: &mut Ui,
        cv: &Canvas,
        scpi: &mut crate::scpi::Scpi,
        cmds: &mut Vec<String>,
    ) -> bool {
        let Pending { s, req, .. } = &mut self.pending[i];
        // only the header as text: the body may be a 31 MB binary package
        let text = String::from_utf8_lossy(&req[..hdr_end.unwrap_or(req.len())]).into_owned();
        let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
        let (route, query) = path.split_once('?').unwrap_or((&path, ""));
        let req_body = &req[hdr_end.unwrap_or(req.len())..];
        let post = text.starts_with("POST ");
        // what changes the scope beyond keys and knobs: only from the allowed prefixes (also at the head)
        let peer_ok = self.acl.permits_peer(s.peer_addr());
        // an answer that is text for the page to show, not the data asked for
        let mut failed = false;
        let mut csv = |r: Result<String, String>| match r {
            Ok(t) => ("text/csv; charset=utf-8", t.into_bytes()),
            Err(e) => {
                failed = true;
                ("text/plain", e.into_bytes())
            }
        };
        let (ctype, body): (&str, Vec<u8>) = match route {
            _ if foreign(&text, route, post) => ("text/plain", b"refused".to_vec()),
            "/update" | "/mcp" if (post || route == "/mcp") && !peer_ok => ("text/plain", b"refused".to_vec()),
            "/" => ("text/html; charset=utf-8", PAGE.as_bytes().to_vec()),
            "/style.css" => ("text/css", STYLE.as_bytes().to_vec()),
            "/ui-state" if !post => ("application/json", ui.workflow_state().into_bytes()),
            "/waveform" if post => {
                let slot = query
                    .split('&')
                    .find_map(|part| part.strip_prefix("slot="))
                    .and_then(|value| value.parse::<usize>().ok());
                match slot.filter(|s| (1..=10).contains(s)) {
                    None => {
                        failed = true;
                        ("text/plain; charset=utf-8", b"Choose a reference slot from R1 to R10.".to_vec())
                    }
                    Some(slot) => {
                        self.waveform_take(i, slot, query, hdr_end, ui);
                        return false;
                    }
                }
            }
            "/screen.png" | "/screen.jpg" => {
                let have = header(&text, "if-none-match").map(str::to_string);
                self.shots.request(s, have, cv, route == "/screen.jpg");
                self.pending.remove(i);
                return false;
            }
            // the scope's local time for the names of the page's downloads; empty while its clock is not set
            "/stamp" => ("text/plain", ui.file_stamp().unwrap_or_default().into_bytes()),
            "/wave.csv" => csv(ui.web_wave_csv()),
            "/la.csv" => csv(ui.web_la_csv()),
            "/wave.bin" => match ui.web_wave_bin() {
                Ok(b) => ("application/octet-stream", b),
                Err(e) => {
                    failed = true;
                    ("text/plain", e.into_bytes())
                }
            },
            "/update" if post => match ui.update_upload(req_body) {
                Ok(m) => ("text/plain", m.into_bytes()),
                Err(e) => ("text/plain", format!("REFUSED: {}", e).into_bytes()),
            },
            "/update" => ("text/html; charset=utf-8", ui.update_page().into_bytes()),
            "/lxi/identification" => ("text/xml", lxi_identification(&self.offer).into_bytes()),
            "/mcp" => match mcp(&String::from_utf8_lossy(req_body), ui, cv, scpi) {
                Some(r) => ("application/json", r.into_bytes()),
                None => ("", Vec::new()),
            },
            "/screen.bmp" => ("image/bmp", crate::scpi::bmp(cv)),
            "/cmd" => remote_cmds(query, cmds),
            _ => ("text/plain", b"not found".to_vec()),
        };
        let status = if failed {
            "409 Conflict"
        } else if body == b"not found" {
            "404 Not Found"
        } else if body == b"refused" {
            "403 Forbidden"
        } else if ctype.is_empty() {
            "202 Accepted"
        } else {
            "200 OK"
        };
        let head = format!("HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", status, ctype, body.len());
        if body.len() > SEND_INLINE {
            if !send_in_background(s, head, body) {
                busy_answer(s);
            }
        } else {
            let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&body);
        }
        true
    }

    /// POST /waveform to `slot`: the request goes whole to the parser, or is answered busy while one is parsed; the
    /// connection leaves `pending` either way.
    fn waveform_take(&mut self, i: usize, slot: usize, query: &str, hdr_end: Option<usize>, ui: &mut Ui) {
        let Pending { s, req, .. } = &mut self.pending[i];
        if self.wave.is_some() {
            busy_answer(s);
            self.pending.remove(i);
        } else {
            // the request goes to the parser whole, its body is not copied out
            let (body, epoch) = (hdr_end.unwrap_or(req.len()), ui.waveform_epoch());
            let Pending { s, req, .. } = self.pending.remove(i);
            match WaveJob::start(s, req, body, waveform_name(query), slot - 1, epoch) {
                Some(job) => self.wave = Some(job),
                None => eprintln!("oos-ui: no thread for a waveform upload"),
            }
        }
    }
}

/// Reads what connection `s` has now into `req`; true when it closed or failed.
fn read_some(s: &TcpStream, req: &mut Vec<u8>, t: &mut Instant, need: Option<usize>, buf: &mut [u8]) -> bool {
    let mut dead = false;
    // up to 2 MB per connection and frame (an update package is ~31 MB: a few seconds at 100 Mbit/s), and once
    // the length is known not beyond the request: the buffer reserved for it is not outgrown
    let mut got = 0;
    while got < 2 << 20 {
        let room = need.map_or(buf.len(), |n| n.saturating_sub(req.len()).min(buf.len()));
        if room == 0 {
            break;
        }
        match crate::net::read_now(s, &mut buf[..room]) {
            Ok(0) => {
                dead = true;
                break;
            }
            Ok(n) => {
                req.extend_from_slice(&buf[..n]);
                got += n;
                *t = Instant::now();
            }
            Err(e) if crate::net::later(&e) => break,
            Err(_) => {
                dead = true;
                break;
            }
        }
    }
    dead
}

/// GET /cmd: the commands `c=` of the query that a remote may give go to `cmds`; "refused" when one may not.
fn remote_cmds(query: &str, cmds: &mut Vec<String>) -> (&'static str, Vec<u8>) {
    let mut refused = false;
    for kv in query.split('&') {
        if let Some(v) = kv.strip_prefix("c=") {
            let c = unescape(v);
            if crate::cmd::remote_ok(&c) {
                cmds.push(c);
            } else {
                refused = true;
            }
        }
    }
    ("text/plain", if refused { b"refused".to_vec() } else { b"ok".to_vec() })
}

/// The value of a header of the request head (name without case).
fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

fn lxi_identification(o: &Offer) -> String {
    let mut addrs = String::new();
    if o.vxi11 {
        addrs.push_str("<InstrumentAddressString>TCPIP::INSTR</InstrumentAddressString>\n");
    }
    if let Some(p) = o.scpi_port {
        addrs.push_str(&format!("<InstrumentAddressString>TCPIP::{p}::SOCKET</InstrumentAddressString>\n"));
    }
    if let Some(port) = o.hislip_port {
        let device = if port == crate::hislip::PORT { "hislip0".into() } else { format!("hislip0,{port}") };
        addrs.push_str(&format!("<InstrumentAddressString>TCPIP::{device}::INSTR</InstrumentAddressString>\n"));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<LXIDevice xmlns="http://www.lxistandard.org/InstrumentIdentification/1.0">
<Manufacturer>OOS</Manufacturer><Model>DHO924S</Model><SerialNumber>0</SerialNumber>
<FirmwareRevision>oos-ui {}</FirmwareRevision><ManufacturerDescription>OOS oscilloscope</ManufacturerDescription>
<HomepageURL>/</HomepageURL><DriverURLs/>
<UserDescription>OOS</UserDescription>
<IdentificationURL>/lxi/identification</IdentificationURL>
<Interface InterfaceType="LXI" IPType="IPv4" xsi:type="NetworkInformation" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
{addrs}</Interface>
<LXIVersion>1.5</LXIVersion>
</LXIDevice>
"#,
        env!("CARGO_PKG_VERSION")
    )
}

pub fn base64(d: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::with_capacity(d.len().div_ceil(3) * 4);
    for c in d.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            o.push(if k <= c.len() { T[(n >> (18 - 6 * k) & 63) as usize] as char } else { '=' });
        }
    }
    o
}

/// One MCP JSON-RPC message; None for notifications (answered with 202 and no body).
pub fn mcp(body: &str, ui: &mut Ui, cv: &Canvas, scpi: &mut crate::scpi::Scpi) -> Option<String> {
    use crate::json::{parse, J};
    let err = |id: J, code: f64, msg: &str| {
        J::obj(&[
            ("jsonrpc", J::s("2.0")),
            ("id", id),
            ("error", J::obj(&[("code", J::Num(code)), ("message", J::s(msg))])),
        ])
        .to_string()
    };
    let Some(req) = parse(body) else { return Some(err(J::Null, -32700.0, "parse error")) };
    let id = req.get("id").cloned()?;
    let method = req.get("method").and_then(J::str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(J::Null);
    let text = |s: &str| J::obj(&[("content", J::Arr(vec![J::obj(&[("type", J::s("text")), ("text", J::s(s))])]))]);
    let result = match method {
        "initialize" => {
            let ver = params.get("protocolVersion").and_then(J::str).unwrap_or("2025-06-18").to_string();
            J::obj(&[
                ("protocolVersion", J::Str(ver)),
                ("capabilities", J::obj(&[("tools", J::obj(&[]))])),
                ("serverInfo", J::obj(&[("name", J::s("oos-ui")), ("version", J::s(env!("CARGO_PKG_VERSION")))])),
                ("instructions", J::s("Rigol DHO924S running OOS. Use the scpi tool with Rigol-style commands (e.g. :TIM:SCAL 1e-3, :CHAN1:SCAL 0.5, :MEAS:ITEM? VPP,CHAN1, :WAV:DATA?) and screenshot to see the screen.")),
            ])
        }
        "ping" => J::obj(&[]),
        "tools/list" => {
            let schema = |props: J, req: Vec<J>| {
                J::obj(&[("type", J::s("object")), ("properties", props), ("required", J::Arr(req))])
            };
            J::obj(&[(
                "tools",
                J::Arr(vec![
                    J::obj(&[
                        ("name", J::s("scpi")),
                        ("description", J::s("Run one SCPI program message on the oscilloscope (commands separated by ';'); returns the response of the queries.")),
                        ("inputSchema", schema(J::obj(&[("command", J::obj(&[("type", J::s("string"))]))]), vec![J::s("command")])),
                    ]),
                    J::obj(&[
                        ("name", J::s("screenshot")),
                        ("description", J::s("The current oscilloscope screen as a PNG image.")),
                        ("inputSchema", schema(J::obj(&[]), vec![])),
                    ]),
                ]),
            )])
        }
        "tools/call" => match params.get("name").and_then(J::str) {
            Some("scpi") => {
                let c = params.get("arguments").and_then(|a| a.get("command")).and_then(J::str).unwrap_or("");
                match scpi.exec_message_checked(ui, cv, c) {
                    Ok((r, has_reply)) => text(if r.is_empty() && !has_reply {
                        "ok"
                    } else {
                        std::str::from_utf8(&r).unwrap_or("(binary response)")
                    }),
                    Err(error) => J::obj(&[
                        ("isError", J::Bool(true)),
                        ("content", J::Arr(vec![J::obj(&[("type", J::s("text")), ("text", J::s(error))])])),
                    ]),
                }
            }
            Some("screenshot") => J::obj(&[(
                "content",
                J::Arr(vec![J::obj(&[
                    ("type", J::s("image")),
                    ("mimeType", J::s("image/png")),
                    ("data", J::Str(base64(&crate::store::png_of(cv)))),
                ])]),
            )]),
            _ => return Some(err(id, -32602.0, "unknown tool")),
        },
        _ => return Some(err(id, -32601.0, "method not found")),
    };
    Some(J::obj(&[("jsonrpc", J::s("2.0")), ("id", id), ("result", result)]).to_string())
}

/// A request a web page on another site made through the browser of the bench PC, not the page of the scope or a tool:
/// it may press keys or install a package without the owner's doing. The Host names the scope by address or local
/// name (a foreign name that resolves here is DNS rebinding), an Origin matches the Host, and the browser's own
/// Sec-Fetch-Site says the rest, also for the GET of an image tag, which has no Origin. A link to the page itself may
/// come from anywhere. curl and the like send neither header.
fn foreign(head: &str, route: &str, post: bool) -> bool {
    let header = |name: &str| {
        head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_ascii_lowercase())
        })
    };
    let name_of = |h: &str| {
        h.rsplit_once(':')
            .filter(|(n, p)| !n.ends_with(':') && p.bytes().all(|b| b.is_ascii_digit()))
            .map_or(h.to_string(), |(n, _)| n.to_string())
    };
    let host = header("host");
    if let Some(h) = &host {
        let n = name_of(h);
        let n = n.trim_matches(['[', ']']);
        let local =
            n.parse::<std::net::IpAddr>().is_ok() || n == "localhost" || !n.contains('.') || n.ends_with(".local");
        if !local {
            return true;
        }
    }
    if let Some(o) = header("origin") {
        let o = o.split_once("://").map_or(o.as_str(), |x| x.1).to_string();
        if host.as_deref() != Some(o.as_str()) {
            return true;
        }
    }
    let page = !post && matches!(route, "/" | "/style.css" | "/update" | "/lxi/identification");
    !page && matches!(header("sec-fetch-site").as_deref(), Some("cross-site" | "same-site"))
}

#[cfg(test)]
mod tests;
