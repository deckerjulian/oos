// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! System > Graphics = GPU: the picture put together on the Mali by `oos-gpu` (gpu/oos-gpu.c).
//!
//! oos-ui draws the screen in two layers (`ui::Layers`: under the traces, over them) and leaves the traces as
//! phosphor maps (`ui::TraceJob`). This module hands the three to `oos-gpu`, a separate process (oos-ui is one static
//! musl binary, Mesa needs glibc), through one shared memory arena and a Unix socket on the helper's stdin. The
//! helper draws straight into the display's scan-out buffers (dma-bufs, `drm::Display::export`), or into its own
//! memory and reads the picture back (host tests, and the picture for the web page and screenshots).
//!
//! The protocol is a few fixed C structs (`#[repr(C)]` here, the same in oos-gpu.c, sizes checked by a test), each
//! message answered by a `Done`. Every failure ends the helper: oos-ui then draws on the CPU again.

use crate::gfx::{Canvas, Rect};
use crate::ui::TraceJob;
use crate::wave::Phosphor;

pub const MAGIC: u32 = 0x4753_4f4f;
pub const VERSION: u32 = 2;
const K_HELLO: u32 = 1;
const K_TARGET: u32 = 2;
const K_FRAME: u32 = 4;
const K_DONE: u32 = 5;
pub const MAX_QUADS: usize = 4;
pub const MAX_JOBS: usize = 16;
const NCH: usize = 4;
const NONE: u32 = u32::MAX;
pub const FOURCC_XRGB8888: u32 = 0x3432_5258;
/// Shared memory for one frame: two layers and the read-back picture at 1920 x 1080, maps, tables.
const ARENA: usize = 96 << 20;
/// The helper as the image has it.
pub const HELPER: &str = "/bin/oos-gpu";

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Hello {
    magic: u32,
    version: u32,
    kind: u32,
    pad: u32,
    arena_size: u64,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct TargetMsg {
    magic: u32,
    kind: u32,
    index: u32,
    w: i32,
    h: i32,
    pitch: i32,
    fourcc: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct QuadMsg {
    under_off: u32,
    over_off: u32,
    lw: i32,
    lh: i32,
    sx: i32,
    sy: i32,
    sw: i32,
    sh: i32,
    dx: i32,
    dy: i32,
    dw: i32,
    dh: i32,
    filter: u32,
    first_job: u32,
    n_jobs: u32,
    reuse: u32,
    under_y0: i32,
    under_y1: i32,
    over_y0: i32,
    over_y1: i32,
    /// NONE: the layer is pixels; else that many commands at its offset
    under_n: u32,
    over_n: u32,
    /// the pixels of the image commands of both layers, rows of `ATLAS_W` words
    img_off: u32,
    img_rows: u32,
}

/// Width of the glyph atlas (bytes) and of the rows of image pixels (words) in the helper.
const ATLAS_W: usize = 4096;
const ATLAS_H: usize = 512;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct JobMsg {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    mw: i32,
    mh: i32,
    map_off: [u32; NCH],
    graded: u32,
    lut_off: u32,
    shift: [u32; NCH],
    col: [u32; NCH],
    map_y0: [i32; NCH],
    map_y1: [i32; NCH],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FrameMsg {
    magic: u32,
    kind: u32,
    target: u32,
    clear: u32,
    n_quads: u32,
    n_jobs: u32,
    readback: u32,
    readback_off: u32,
    /// glyph alphas new to the atlas: rows of `ATLAS_W` bytes to its row `atlas_row`
    atlas_off: u32,
    atlas_rows: u32,
    atlas_row: u32,
    /// the helper's set of textures: 0 the panel's, 1 the monitor's (each keeps what its last frame uploaded)
    set: u32,
    q: [QuadMsg; MAX_QUADS],
    j: [JobMsg; MAX_JOBS],
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct DoneMsg {
    magic: u32,
    kind: u32,
    status: u32,
    pad: u32,
    ms_upload: f32,
    ms_draw: f32,
    ms_read: f32,
    pad2: f32,
}

/// Two rows differ: eight bytes at a time (musl's memcmp, which `==` on byte slices calls, goes byte by byte and
/// took 7.5 ms for the 5 MB of a frame on the unit).
fn differ(a: &[u8], b: &[u8]) -> bool {
    let ((ca, ra), (cb, rb)) = (a.as_chunks::<8>(), b.as_chunks::<8>());
    let words = ca.iter().zip(cb).fold(0u64, |d, (p, q)| d | (u64::from_ne_bytes(*p) ^ u64::from_ne_bytes(*q)));
    words != 0 || ra != rb
}

fn bytes<T>(v: &T) -> &[u8] {
    // SAFETY: plain #[repr(C)] structs of integers, read as their bytes
    unsafe { std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>()) }
}

/// A rectangle of a layer pair put into a rectangle of the target, with the traces of its plot windows.
pub struct Quad<'a> {
    pub under: Option<&'a Canvas>,
    pub over: Option<&'a Canvas>,
    /// the layers' size, also when they are not sent again (`reuse`)
    pub size: (i32, i32),
    pub src: Rect,
    pub dst: Rect,
    /// filtered scaling (a monitor), else nearest
    pub filter: bool,
    /// layers, maps and tables as the panel's last frame left them for this quad (nothing is copied or uploaded)
    pub reuse: bool,
    pub jobs: &'a [TraceJob],
    pub ph: &'a Phosphor,
    pub phz: &'a Phosphor,
}

/// Times of the last frame (ms): copying into the arena here; upload, draw and read back in the helper; and the whole
/// wait for its answer.
#[derive(Clone, Copy, Default, Debug)]
pub struct Times {
    pub copy: f32,
    pub upload: f32,
    pub draw: f32,
    pub read: f32,
    /// how long `collect` waited
    pub wait: f32,
    /// from sending to the answer
    pub latency: f32,
}

/// A frame sent to the helper and not collected yet (`Gpu::submit`, `Gpu::collect`, in the order sent).
pub struct Ticket {
    /// where the picture comes back, and its size, when it was asked for
    readback: Option<(usize, i32, i32)>,
    sent: std::time::Instant,
}

impl Ticket {
    /// The picture of this frame comes back (`collect` puts it into its canvas).
    pub fn reads(&self) -> bool {
        self.readback.is_some()
    }
}

#[cfg(target_os = "linux")]
pub use imp::Gpu;

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use crate::gfx::{Cmd, CMD_GLYPH, CMD_IMAGE, CMD_IMAGE_KEY};
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    pub struct Gpu {
        child: std::process::Child,
        sock: UnixStream,
        arena: *mut u8,
        /// targets known to the helper: (w, h, in its own memory)
        targets: [Option<(i32, i32, bool)>; 8],
        /// the blocks of the last frame in each half of the arena (the panel's, the monitor's): while a frame has
        /// the same blocks, only the rows that changed are copied and uploaded
        blocks: [Vec<(usize, usize)>; 2],
        /// the first of the monitor's targets: the panel's are below it (`monitor_targets_from`)
        monitor_from: usize,
        /// the fonts in the helper's glyph atlas (their alpha bytes, the byte of the atlas they start at) and its
        /// first free row
        atlas: Vec<(usize, u32)>,
        atlas_rows: usize,
        pub times: Times,
    }

    // SAFETY: the arena pointer is only used by the thread that owns the Gpu
    unsafe impl Send for Gpu {}

    fn send_fd(sock: &UnixStream, data: &[u8], fd: Option<i32>) -> std::io::Result<()> {
        let mut iov = libc::iovec { iov_base: data.as_ptr() as *mut libc::c_void, iov_len: data.len() };
        let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32) } as usize;
        let mut ctl = vec![0u8; space];
        let mut m: libc::msghdr = unsafe { std::mem::zeroed() };
        m.msg_iov = &mut iov;
        m.msg_iovlen = 1;
        if let Some(fd) = fd {
            m.msg_control = ctl.as_mut_ptr() as *mut libc::c_void;
            m.msg_controllen = space as _;
            // SAFETY: the control buffer is CMSG_SPACE for one int
            unsafe {
                let c = libc::CMSG_FIRSTHDR(&m);
                (*c).cmsg_level = libc::SOL_SOCKET;
                (*c).cmsg_type = libc::SCM_RIGHTS;
                (*c).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<i32>() as u32) as _;
                std::ptr::write_unaligned(libc::CMSG_DATA(c) as *mut i32, fd);
            }
        }
        let n = unsafe { libc::sendmsg(sock.as_raw_fd(), &m, libc::MSG_NOSIGNAL) };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if n as usize != data.len() {
            return Err(std::io::Error::other("short write"));
        }
        Ok(())
    }

    impl Gpu {
        /// Starts the helper; it answers once it has a GPU context (a few hundred ms).
        pub fn start(helper: &str) -> Result<Gpu, String> {
            let fd = unsafe { libc::memfd_create(c"oos-gpu".as_ptr(), libc::MFD_CLOEXEC) };
            if fd < 0 {
                return Err(format!("memfd: {}", std::io::Error::last_os_error()));
            }
            // SAFETY: a fresh descriptor we own
            let mem = unsafe { OwnedFd::from_raw_fd(fd) };
            if unsafe { libc::ftruncate(fd, ARENA as libc::off_t) } != 0 {
                return Err(format!("arena: {}", std::io::Error::last_os_error()));
            }
            let arena = unsafe { libc::mmap(std::ptr::null_mut(), ARENA, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) };
            if arena == libc::MAP_FAILED {
                return Err(format!("arena map: {}", std::io::Error::last_os_error()));
            }
            let (ours, theirs) = UnixStream::pair().map_err(|e| format!("socket: {}", e))?;
            let child = std::process::Command::new(helper)
                .stdin(std::process::Stdio::from(OwnedFd::from(theirs)))
                .stdout(std::process::Stdio::null())
                .spawn()
                .map_err(|e| format!("{}: {}", helper, e))?;
            let mut g = Gpu { child, sock: ours, arena: arena as *mut u8, targets: [None; 8], blocks: [Vec::new(), Vec::new()], monitor_from: 2, atlas: Vec::new(), atlas_rows: 0, times: Times::default() };
            clock_floor(true);
            let h = Hello { magic: MAGIC, version: VERSION, kind: K_HELLO, pad: 0, arena_size: ARENA as u64 };
            send_fd(&g.sock, bytes(&h), Some(mem.as_raw_fd())).map_err(|e| format!("hello: {}", e))?;
            drop(mem);
            // the helper opens Mesa and the GPU first
            g.wait(Duration::from_secs(10)).map_err(|e| format!("start: {}", e))?;
            Ok(g)
        }

        /// Targets from `n` on are the monitor's (2 unless set: the script's memory targets). The panel has three
        /// scan-out buffers: with the old fixed 2, a panel frame into its third buffer took the monitor's half of the
        /// arena, and the monitor's frame sent right after it overwrote its layers while the GPU still drew them (black
        /// flashes on the panel with HDMI and Graphics = GPU, mostly with a menu open).
        pub fn monitor_targets_from(&mut self, n: usize) {
            self.monitor_from = n;
        }

        fn wait(&mut self, t: Duration) -> Result<DoneMsg, String> {
            let _ = self.sock.set_read_timeout(Some(t));
            let mut d = DoneMsg::default();
            // SAFETY: DoneMsg is plain integers and floats
            let buf = unsafe { std::slice::from_raw_parts_mut(&mut d as *mut DoneMsg as *mut u8, std::mem::size_of::<DoneMsg>()) };
            self.sock.read_exact(buf).map_err(|e| format!("no answer: {}", e))?;
            if d.magic != MAGIC || d.kind != K_DONE {
                return Err("bad answer".into());
            }
            if d.status != 0 {
                return Err(format!("helper error {}", d.status));
            }
            self.times = Times { upload: d.ms_upload, draw: d.ms_draw, read: d.ms_read, ..self.times };
            Ok(d)
        }

        /// A scan-out buffer of the display as target `index` (the helper keeps the descriptor).
        pub fn target_dmabuf(&mut self, index: usize, w: i32, h: i32, pitch: i32, fd: OwnedFd) -> Result<(), String> {
            let m = TargetMsg { magic: MAGIC, kind: K_TARGET, index: index as u32, w, h, pitch, fourcc: FOURCC_XRGB8888, memory: 0 };
            send_fd(&self.sock, bytes(&m), Some(fd.as_raw_fd())).map_err(|e| format!("target: {}", e))?;
            drop(fd);
            self.wait(Duration::from_secs(2))?;
            self.targets[index] = Some((w, h, false));
            Ok(())
        }

        /// A target in the helper's own memory, read back after every frame (tests).
        pub fn target_memory(&mut self, index: usize, w: i32, h: i32) -> Result<(), String> {
            let m = TargetMsg { magic: MAGIC, kind: K_TARGET, index: index as u32, w, h, pitch: w * 4, fourcc: FOURCC_XRGB8888, memory: 1 };
            send_fd(&self.sock, bytes(&m), None).map_err(|e| format!("target: {}", e))?;
            self.wait(Duration::from_secs(2))?;
            self.targets[index] = Some((w, h, true));
            Ok(())
        }

        /// Puts the quads together in target `index`, on `clear` first when given (xrgb); with `back` the picture is
        /// read back into it (its size must be the target's). `submit` and `collect` in one.
        pub fn compose(&mut self, index: usize, clear: Option<u32>, quads: &[Quad], back: Option<&mut Canvas>) -> Result<(), String> {
            let t = self.submit(index, clear, quads, back.is_some())?;
            self.collect(t, back)
        }

        /// Sends a frame and returns at once: the helper works while the caller draws the next one. The layers and
        /// maps are copied now; the arena half of this target must not hold a frame still in work (collect first).
        pub fn submit(&mut self, index: usize, clear: Option<u32>, quads: &[Quad], read: bool) -> Result<Ticket, String> {
            let t0 = std::time::Instant::now();
            let (tw, th, memory) = self.targets.get(index).copied().flatten().ok_or("no such target")?;
            let mut f = FrameMsg {
                magic: MAGIC,
                kind: K_FRAME,
                target: index as u32,
                clear: clear.map_or(0, |c| 0x0100_0000 | (c & 0xff_ffff)),
                n_quads: 0,
                n_jobs: 0,
                readback: 0,
                readback_off: 0,
                atlas_off: 0,
                atlas_rows: 0,
                atlas_row: 0,
                set: 0,
                q: [QuadMsg::default(); MAX_QUADS],
                j: [JobMsg::default(); MAX_JOBS],
            };
            // the panel's frames use the first half of the arena, the monitor's the second: each keeps its blocks,
            // and in the helper its own textures
            let half = (index >= self.monitor_from) as usize;
            f.set = half as u32;
            let base = self.arena;
            let mut at = half * ARENA / 2;
            if quads.len() > MAX_QUADS {
                return Err("too many quads".into());
            }
            // recorded layers (`Canvas::record`): their commands, images and new fonts from the top of the half down,
            // apart from the blocks below that keep their places from frame to frame
            let mut tail = at + ARENA / 2;
            let mut alloc = |len: usize| -> Result<usize, String> {
                tail = tail.checked_sub(len).ok_or("frame too large for the arena")? & !63;
                Ok(tail)
            };
            let mut new_fonts: Vec<&[u8]> = Vec::new();
            for q in quads.iter().filter(|q| !q.reuse) {
                for c in [q.under, q.over].into_iter().flatten() {
                    for f in c.rec.iter().flat_map(|r| r.fonts.iter()) {
                        if !self.atlas.iter().any(|a| a.0 == f.as_ptr() as usize) {
                            let rows = f.len().div_ceil(ATLAS_W);
                            if self.atlas_rows + rows > ATLAS_H {
                                return Err("glyph atlas full".into());
                            }
                            self.atlas.push((f.as_ptr() as usize, (self.atlas_rows * ATLAS_W) as u32));
                            self.atlas_rows += rows;
                            new_fonts.push(f);
                        }
                    }
                }
            }
            if !new_fonts.is_empty() {
                let rows: usize = new_fonts.iter().map(|f| f.len().div_ceil(ATLAS_W)).sum();
                let o = alloc(rows * ATLAS_W)?;
                let mut p = o;
                for f in &new_fonts {
                    // SAFETY: inside the mapping (alloc), rows of ATLAS_W bytes per font
                    unsafe { std::ptr::copy_nonoverlapping(f.as_ptr(), base.add(p), f.len()) };
                    p += f.len().div_ceil(ATLAS_W) * ATLAS_W;
                }
                (f.atlas_off, f.atlas_rows, f.atlas_row) = (o as u32, rows as u32, (self.atlas_rows - rows) as u32);
            }
            // per quad: (offset, count) of the commands of under and over (count NONE: pixels), images (offset, rows)
            let mut recs = [[(NONE, NONE); 3]; MAX_QUADS];
            for (qi, q) in quads.iter().enumerate().filter(|(_, q)| !q.reuse) {
                let (u, o) = (q.under.and_then(|c| c.rec.as_deref()), q.over.and_then(|c| c.rec.as_deref()));
                let n_img = u.map_or(0, |r| r.img.len()) + o.map_or(0, |r| r.img.len());
                if n_img > 0 {
                    let rows = n_img.div_ceil(ATLAS_W);
                    let at = alloc(rows * ATLAS_W * 4)?;
                    let mut p = at;
                    for r in [u, o].into_iter().flatten() {
                        // SAFETY: inside the mapping (alloc)
                        unsafe { std::ptr::copy_nonoverlapping(r.img.as_ptr(), base.add(p) as *mut u32, r.img.len()) };
                        p += r.img.len() * 4;
                    }
                    // the rest of the last row zero: the helper knows images by a hash of the rows
                    unsafe { std::ptr::write_bytes(base.add(p), 0, at + rows * ATLAS_W * 4 - p) };
                    recs[qi][2] = (at as u32, rows as u32);
                }
                let mut img0 = 0u32;
                for (k, r) in [u, o].into_iter().enumerate() {
                    let Some(r) = r else { continue };
                    let bases: Vec<u32> = r.fonts.iter().map(|f| self.atlas.iter().find(|a| a.0 == f.as_ptr() as usize).map_or(0, |a| a.1)).collect();
                    let at = alloc(r.cmds.len() * std::mem::size_of::<Cmd>())?;
                    // SAFETY: inside the mapping (alloc), room for all commands
                    let dst = unsafe { std::slice::from_raw_parts_mut(base.add(at) as *mut Cmd, r.cmds.len()) };
                    for (d, c) in dst.iter_mut().zip(&r.cmds) {
                        *d = *c;
                        match c.kind & 255 {
                            CMD_GLYPH => d.off += bases.get((c.kind >> 8) as usize).copied().unwrap_or(0),
                            CMD_IMAGE | CMD_IMAGE_KEY => d.off += img0,
                            _ => {}
                        }
                    }
                    recs[qi][k] = (at as u32, r.cmds.len() as u32);
                    img0 += r.img.len() as u32;
                }
            }
            let end = tail;
            let mut blocks: Vec<(usize, usize)> = Vec::new();
            // the blocks so far lie where they lay in the last frame of this half
            let same = |blocks: &Vec<(usize, usize)>, prev: &Vec<(usize, usize)>| prev.len() >= blocks.len() && prev[..blocks.len()] == blocks[..];
            let prev = std::mem::take(&mut self.blocks[half]);
            // a block of `rows` rows of `row` bytes: the rows that differ from the arena's copy are copied; the band
            // of changed rows comes back (all of them for a new block)
            let mut put = |src: *const u8, row: usize, rows: usize, blocks: &mut Vec<(usize, usize)>| -> Result<(u32, i32, i32), String> {
                at = (at + 63) & !63;
                let len = row * rows;
                if at + len > end {
                    return Err("frame too large for the arena".into());
                }
                blocks.push((at, len));
                let known = same(blocks, &prev);
                let (mut y0, mut y1) = (rows as i32, 0);
                for r in 0..rows {
                    // SAFETY: inside the mapping; the source has `rows` rows of `row` readable bytes
                    let (a, b) = unsafe { (std::slice::from_raw_parts(base.add(at + r * row), row), std::slice::from_raw_parts(src.add(r * row), row)) };
                    if !known || differ(a, b) {
                        unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), base.add(at + r * row), row) };
                        y0 = y0.min(r as i32);
                        y1 = r as i32 + 1;
                    }
                }
                let o = at as u32;
                at += len;
                Ok((o, y0, y1.max(y0)))
            };
            for (qi, q) in quads.iter().enumerate() {
                let mut layer = |c: Option<&Canvas>, blocks: &mut Vec<(usize, usize)>| -> Result<(u32, i32, i32), String> {
                    match c {
                        Some(c) if c.rec.is_some() => Ok((0, 0, 0)),
                        Some(c) if !q.reuse => put(c.px.as_ptr() as *const u8, c.w as usize * 4, c.h as usize, blocks),
                        Some(_) => Ok((0, 0, 0)),
                        None => Ok((NONE, 0, 0)),
                    }
                };
                let (uo, uy0, uy1) = layer(q.under, &mut blocks)?;
                let (oo, oy0, oy1) = layer(q.over, &mut blocks)?;
                let first = f.n_jobs;
                for job in q.jobs {
                    if f.n_jobs as usize >= MAX_JOBS {
                        return Err("too many plot windows".into());
                    }
                    let ph = if job.zoom { q.phz } else { q.ph };
                    let mut j = JobMsg {
                        x: job.area.x,
                        y: job.area.y,
                        w: job.area.w,
                        h: job.area.h,
                        mw: ph.w as i32,
                        mh: ph.h as i32,
                        map_off: [NONE; NCH],
                        graded: job.graded as u32,
                        lut_off: 0,
                        shift: job.shift,
                        col: job.col,
                        map_y0: [0; NCH],
                        map_y1: [0; NCH],
                    };
                    for c in 0..NCH {
                        if job.show[c] && !q.reuse {
                            let (o, y0, y1) = put(ph.map[c].as_ptr() as *const u8, ph.w * 2, ph.h, &mut blocks)?;
                            (j.map_off[c], j.map_y0[c], j.map_y1[c]) = (o, y0, y1);
                        } else if job.show[c] {
                            j.map_off[c] = 0;
                        }
                    }
                    if !q.reuse {
                        j.lut_off = if job.graded { put(job.colours.as_ptr() as *const u8, job.colours.len() * 4, 1, &mut blocks)?.0 } else { put(job.alpha.as_ptr(), job.alpha.len(), 1, &mut blocks)?.0 };
                    }
                    f.j[f.n_jobs as usize] = j;
                    f.n_jobs += 1;
                }
                let (lw, lh) = q.size;
                f.q[qi] = QuadMsg {
                    under_off: uo,
                    over_off: oo,
                    lw,
                    lh,
                    sx: q.src.x,
                    sy: q.src.y,
                    sw: q.src.w,
                    sh: q.src.h,
                    dx: q.dst.x,
                    dy: q.dst.y,
                    dw: q.dst.w,
                    dh: q.dst.h,
                    filter: q.filter as u32,
                    first_job: first,
                    n_jobs: f.n_jobs - first,
                    reuse: q.reuse as u32,
                    under_y0: uy0,
                    under_y1: uy1,
                    over_y0: oy0,
                    over_y1: oy1,
                    under_n: NONE,
                    over_n: NONE,
                    img_off: recs[qi][2].0,
                    img_rows: if recs[qi][2].1 == NONE { 0 } else { recs[qi][2].1 },
                };
                if q.under.is_some_and(|c| c.rec.is_some()) && !q.reuse {
                    (f.q[qi].under_off, f.q[qi].under_n) = recs[qi][0];
                }
                if q.over.is_some_and(|c| c.rec.is_some()) && !q.reuse {
                    (f.q[qi].over_off, f.q[qi].over_n) = recs[qi][1];
                }
                f.n_quads += 1;
            }
            self.blocks[half] = blocks;
            // a memory target is always read back: its place in the arena is kept free
            if read || memory {
                at = (at + 63) & !63;
                if at + (tw * th * 4) as usize > end {
                    return Err("no room to read back".into());
                }
                f.readback = read as u32;
                f.readback_off = at as u32;
            }
            let t1 = std::time::Instant::now();
            self.sock.write_all(bytes(&f)).map_err(|e| format!("frame: {}", e))?;
            self.times.copy = (t1 - t0).as_secs_f32() * 1e3;
            Ok(Ticket { readback: read.then_some((f.readback_off as usize, tw, th)), sent: t1 })
        }

        /// Waits for a frame `submit` sent; with `back` its picture (asked for when it was sent) comes into it.
        pub fn collect(&mut self, t: Ticket, back: Option<&mut Canvas>) -> Result<(), String> {
            let w0 = std::time::Instant::now();
            self.wait(Duration::from_millis(1500))?;
            self.times.wait = w0.elapsed().as_secs_f32() * 1e3;
            self.times.latency = t.sent.elapsed().as_secs_f32() * 1e3;
            if let (Some((off, tw, th)), Some(cv)) = (t.readback, back) {
                if (cv.w, cv.h) != (tw, th) {
                    return Err("read-back canvas of another size".into());
                }
                // SAFETY: the helper wrote tw * th pixels there
                unsafe { std::ptr::copy_nonoverlapping(self.arena.add(off) as *const u32, cv.px.as_mut_ptr(), (tw * th) as usize) };
            }
            Ok(())
        }
    }

    /// The Mali's devfreq: its load governor sees only the short work of each frame and stays at the lowest step
    /// (200 MHz, an 8 ms frame); 400 MHz runs at the same voltage (rk3399-opp.dtsi) and takes 4 ms. While the
    /// helper runs the lowest step is 400 MHz; after it, the table's lowest again.
    fn clock_floor(on: bool) {
        let Ok(d) = std::fs::read_dir("/sys/class/devfreq") else { return };
        for e in d.flatten() {
            let p = e.path();
            if !e.file_name().to_string_lossy().ends_with(".gpu") {
                continue;
            }
            let steps: Vec<u64> = std::fs::read_to_string(p.join("available_frequencies")).unwrap_or_default().split_whitespace().filter_map(|v| v.parse().ok()).collect();
            let low = steps.iter().copied().min().unwrap_or(0);
            let want = if on { steps.iter().copied().filter(|&f| f >= 400_000_000).min().unwrap_or(low) } else { low };
            if want > 0 {
                let _ = std::fs::write(p.join("min_freq"), want.to_string());
            }
        }
    }

    impl Drop for Gpu {
        fn drop(&mut self) {
            clock_floor(false);
            let _ = self.child.kill();
            let _ = self.child.wait();
            unsafe { libc::munmap(self.arena as *mut libc::c_void, ARENA) };
        }
    }
}

/// No GPU path off the scope's Linux (the macOS host build).
#[cfg(not(target_os = "linux"))]
pub struct Gpu {
    pub times: Times,
}

#[cfg(not(target_os = "linux"))]
impl Gpu {
    pub fn start(_: &str) -> Result<Gpu, String> {
        Err("the GPU path runs on Linux only".into())
    }
    pub fn target_memory(&mut self, _: usize, _: i32, _: i32) -> Result<(), String> {
        Err("no GPU".into())
    }
    pub fn compose(&mut self, _: usize, _: Option<u32>, _: &[Quad], _: Option<&mut Canvas>) -> Result<(), String> {
        Err("no GPU".into())
    }
    pub fn submit(&mut self, _: usize, _: Option<u32>, _: &[Quad], _: bool) -> Result<Ticket, String> {
        Err("no GPU".into())
    }
    pub fn collect(&mut self, _: Ticket, _: Option<&mut Canvas>) -> Result<(), String> {
        Err("no GPU".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_that_differ_anywhere_are_found() {
        let a: Vec<u8> = (0..1027).map(|i| i as u8).collect();
        assert!(!differ(&a, &a.clone()));
        for at in [0, 7, 8, 1023, 1024, 1026] {
            let mut b = a.clone();
            b[at] ^= 1;
            assert!(differ(&a, &b), "{}", at);
        }
    }

    /// The same sizes as the structs of gpu/oos-gpu.c.
    #[test]
    fn messages_have_the_helpers_layout() {
        use crate::gfx::Cmd;
        use std::mem::size_of;
        assert_eq!(
            (size_of::<Hello>(), size_of::<TargetMsg>(), size_of::<QuadMsg>(), size_of::<JobMsg>(), size_of::<FrameMsg>(), size_of::<DoneMsg>(), size_of::<Cmd>()),
            (24, 32, 96, 112, 2224, 32, 24)
        );
        let _ = (K_HELLO, K_TARGET, K_FRAME, K_DONE, NONE);
    }
}
