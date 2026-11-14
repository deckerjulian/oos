// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Minimal DRM/KMS output without libdrm: two dumb buffers, mode set on the DSI panel (else the first connected
//! connector), page flips (vsync) with a fallback to SETCRTC. The previous CRTC state is restored on exit. An HDMI
//! monitor gets a second output on the same DRM file (one DRM master) and its own CRTC (`Display::hdmi`), showing
//! the panel picture scaled to its mode. In the board's DTS the panel (DSI) is wired to VOP-little only and HDMI
//! to VOP-big only, so the two never compete for a CRTC.

use crate::gfx::Canvas;
use std::ffi::CString;
use std::io;

const fn iowr(nr: u32, size: usize) -> libc::c_ulong {
    ((3u32 << 30) | ((size as u32) << 16) | ((b'd' as u32) << 8) | nr) as libc::c_ulong
}

#[repr(C)]
#[derive(Default)]
struct CardRes {
    fb_id_ptr: u64,
    crtc_id_ptr: u64,
    connector_id_ptr: u64,
    encoder_id_ptr: u64,
    count_fbs: u32,
    count_crtcs: u32,
    count_connectors: u32,
    count_encoders: u32,
    min_width: u32,
    max_width: u32,
    min_height: u32,
    max_height: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ModeInfo {
    clock: u32,
    hdisplay: u16,
    hsync_start: u16,
    hsync_end: u16,
    htotal: u16,
    hskew: u16,
    vdisplay: u16,
    vsync_start: u16,
    vsync_end: u16,
    vtotal: u16,
    vscan: u16,
    vrefresh: u32,
    flags: u32,
    typ: u32,
    name: [u8; 32],
}

#[repr(C)]
#[derive(Default)]
struct GetConnector {
    encoders_ptr: u64,
    modes_ptr: u64,
    props_ptr: u64,
    prop_values_ptr: u64,
    count_modes: u32,
    count_props: u32,
    count_encoders: u32,
    encoder_id: u32,
    connector_id: u32,
    connector_type: u32,
    connector_type_id: u32,
    connection: u32,
    mm_width: u32,
    mm_height: u32,
    subpixel: u32,
    pad: u32,
}

#[repr(C)]
#[derive(Default)]
struct GetEncoder {
    encoder_id: u32,
    encoder_type: u32,
    crtc_id: u32,
    possible_crtcs: u32,
    possible_clones: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Crtc {
    set_connectors_ptr: u64,
    count_connectors: u32,
    crtc_id: u32,
    fb_id: u32,
    x: u32,
    y: u32,
    gamma_size: u32,
    mode_valid: u32,
    mode: ModeInfo,
}

#[repr(C)]
#[derive(Default)]
struct CreateDumb {
    height: u32,
    width: u32,
    bpp: u32,
    flags: u32,
    handle: u32,
    pitch: u32,
    size: u64,
}

#[repr(C)]
#[derive(Default)]
struct MapDumb {
    handle: u32,
    pad: u32,
    offset: u64,
}

#[repr(C)]
#[derive(Default)]
struct FbCmd {
    fb_id: u32,
    width: u32,
    height: u32,
    pitch: u32,
    bpp: u32,
    depth: u32,
    handle: u32,
}

#[repr(C)]
#[derive(Default)]
struct PageFlip {
    crtc_id: u32,
    fb_id: u32,
    flags: u32,
    reserved: u32,
    user_data: u64,
}

const GETRESOURCES: libc::c_ulong = iowr(0xA0, std::mem::size_of::<CardRes>());
const GETCRTC: libc::c_ulong = iowr(0xA1, std::mem::size_of::<Crtc>());
const SETCRTC: libc::c_ulong = iowr(0xA2, std::mem::size_of::<Crtc>());
const GETENCODER: libc::c_ulong = iowr(0xA6, std::mem::size_of::<GetEncoder>());
const GETCONNECTOR: libc::c_ulong = iowr(0xA7, std::mem::size_of::<GetConnector>());
const ADDFB: libc::c_ulong = iowr(0xAE, std::mem::size_of::<FbCmd>());
const RMFB: libc::c_ulong = iowr(0xAF, 4);
const PAGE_FLIP: libc::c_ulong = iowr(0xB0, std::mem::size_of::<PageFlip>());
const CREATE_DUMB: libc::c_ulong = iowr(0xB2, std::mem::size_of::<CreateDumb>());
const MAP_DUMB: libc::c_ulong = iowr(0xB3, std::mem::size_of::<MapDumb>());
const DESTROY_DUMB: libc::c_ulong = iowr(0xB4, 4);
const PAGE_FLIP_EVENT: u32 = 0x01;

#[repr(C)]
#[derive(Default)]
struct PrimeHandle {
    handle: u32,
    flags: u32,
    fd: i32,
}
const PRIME_HANDLE_TO_FD: libc::c_ulong = iowr(0x2D, std::mem::size_of::<PrimeHandle>());

fn ioc<T>(fd: i32, req: libc::c_ulong, arg: &mut T) -> io::Result<()> {
    loop {
        let r = unsafe { libc::ioctl(fd, req as _, arg as *mut T) };
        if r == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::EINTR) || e.raw_os_error() == Some(libc::EAGAIN) {
            continue;
        }
        return Err(e);
    }
}

struct Buf {
    handle: u32,
    fb: u32,
    pitch: u32,
    size: usize,
    map: *mut u32,
}

pub struct Display {
    fd: i32,
    pub width: i32,
    pub height: i32,
    crtc_id: u32,
    conn_id: u32,
    mode: ModeInfo,
    saved: Option<Crtc>,
    bufs: Vec<Buf>,
    front: usize,
    /// the buffer shown before `front`: still on the screen until the flip to `front` is done
    prev: usize,
    /// page flips asked for with an event whose event has not been read yet (normally 0 or 1; more only after a late
    /// event, which must not count for the flip after it)
    flips_out: u32,
    /// how often a flip event came later than `wait_flip` waited (said on stderr at 1, 2, 4, ...)
    flips_late: u32,
    use_flip: bool,
    /// scaled output (HDMI): the map from the canvas, built for the first canvas size
    fit: Option<crate::gfx::Fit>,
}

/// DRM connector types (drm_mode.h).
const CONNECTOR_HDMIA: u32 = 11;
const CONNECTOR_DSI: u32 = 16;

/// The number of flip-complete events (`DRM_EVENT_FLIP_COMPLETE`) in what a read of the DRM file gave: events are
/// a type and a length (u32 each) and their data.
fn flip_events(buf: &[u8]) -> u32 {
    let (mut at, mut n) = (0, 0);
    while at + 8 <= buf.len() {
        let typ = u32::from_ne_bytes(buf[at..at + 4].try_into().unwrap());
        let len = u32::from_ne_bytes(buf[at + 4..at + 8].try_into().unwrap()) as usize;
        if len < 8 {
            break;
        }
        n += (typ == 2) as u32;
        at += len;
    }
    n
}

impl Display {
    pub fn open(path: &str) -> io::Result<Display> {
        let c = CString::new(path).unwrap();
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // the panel first: a monitor on HDMI at start must not take the scope's own screen
        match Display::on_fd(fd, Some(CONNECTOR_DSI), 0) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Display::on_fd(fd, None, 0),
            r => r,
        }
    }

    /// A connected HDMI monitor as a second output on this display's DRM file, on another CRTC; NotFound if none.
    pub fn hdmi(&self) -> io::Result<Display> {
        let fd = unsafe { libc::fcntl(self.fd, libc::F_DUPFD_CLOEXEC, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let r = Display::on_fd(fd, Some(CONNECTOR_HDMIA), self.crtc_id);
        if r.is_err() {
            unsafe { libc::close(fd) };
        }
        r
    }

    /// The first connected connector (of type `want`, if given) with modes, on a CRTC other than `avoid`.
    fn on_fd(fd: i32, want: Option<u32>, avoid: u32) -> io::Result<Display> {
        let mut res = CardRes::default();
        ioc(fd, GETRESOURCES, &mut res)?;
        let mut crtcs = vec![0u32; res.count_crtcs as usize];
        let mut conns = vec![0u32; res.count_connectors as usize];
        let mut encs = vec![0u32; res.count_encoders as usize];
        let mut fbs = vec![0u32; res.count_fbs as usize];
        res.crtc_id_ptr = crtcs.as_mut_ptr() as u64;
        res.connector_id_ptr = conns.as_mut_ptr() as u64;
        res.encoder_id_ptr = encs.as_mut_ptr() as u64;
        res.fb_id_ptr = fbs.as_mut_ptr() as u64;
        ioc(fd, GETRESOURCES, &mut res)?;

        let mut chosen = None;
        // a connected monitor without a usable mode: not "no monitor" (the caller backs off longer)
        let mut unusable = false;
        for &cid in &conns {
            let mut gc = GetConnector { connector_id: cid, ..Default::default() };
            ioc(fd, GETCONNECTOR, &mut gc)?;
            if gc.connection != 1 || gc.count_modes == 0 || want.is_some_and(|t| t != gc.connector_type) {
                continue;
            }
            let mut modes = vec![unsafe { std::mem::zeroed::<ModeInfo>() }; gc.count_modes as usize];
            let mut encoders = vec![0u32; gc.count_encoders as usize];
            let mut props = vec![0u32; gc.count_props as usize];
            let mut pvals = vec![0u64; gc.count_props as usize];
            gc.modes_ptr = modes.as_mut_ptr() as u64;
            gc.encoders_ptr = encoders.as_mut_ptr() as u64;
            gc.props_ptr = props.as_mut_ptr() as u64;
            gc.prop_values_ptr = pvals.as_mut_ptr() as u64;
            ioc(fd, GETCONNECTOR, &mut gc)?;
            if gc.count_modes == 0 {
                continue;
            }
            let modes = &modes[..(gc.count_modes as usize).min(modes.len())];
            // the panel: its preferred mode (type bit 3), else the first; a monitor: at most 1920 x 1080
            let mode = if want == Some(CONNECTOR_HDMIA) {
                // DRM_MODE_FLAG_INTERLACE = 0x10
                let list: Vec<_> = modes.iter().map(|m| (m.hdisplay, m.vdisplay, m.vrefresh, m.typ & 8 != 0, m.flags & 0x10 != 0)).collect();
                match crate::gfx::pick_mode(&list, (1920, 1080)) {
                    Some(i) => modes[i],
                    None => {
                        unusable = true;
                        continue;
                    }
                }
            } else {
                *modes.iter().find(|m| m.typ & 8 != 0).unwrap_or(&modes[0])
            };
            // crtc: the encoder's current one, else the first possible (never the other output's)
            let mut crtc = 0;
            let enc_ids: Vec<u32> = if gc.encoder_id != 0 { vec![gc.encoder_id] } else { encoders.clone() };
            for eid in enc_ids.iter().chain(encoders.iter()) {
                let mut ge = GetEncoder { encoder_id: *eid, ..Default::default() };
                if ioc(fd, GETENCODER, &mut ge).is_err() {
                    continue;
                }
                if ge.crtc_id != 0 && ge.crtc_id != avoid {
                    crtc = ge.crtc_id;
                    break;
                }
                if let Some(i) = (0..crtcs.len()).find(|&i| ge.possible_crtcs & (1 << i) != 0 && crtcs[i] != avoid) {
                    crtc = crtcs[i];
                    break;
                }
            }
            if crtc != 0 {
                chosen = Some((cid, mode, crtc));
                break;
            }
        }
        let (conn_id, mode, crtc_id) = chosen.ok_or_else(|| {
            if unusable {
                io::Error::new(io::ErrorKind::Unsupported, "monitor has no progressive mode up to 1920 x 1080")
            } else {
                io::Error::new(io::ErrorKind::NotFound, "no connected display")
            }
        })?;
        let mut saved: Crtc = unsafe { std::mem::zeroed() };
        saved.crtc_id = crtc_id;
        let saved = if ioc(fd, GETCRTC, &mut saved).is_ok() { Some(saved) } else { None };

        let (w, h) = (mode.hdisplay as u32, mode.vdisplay as u32);
        let mut d = Display { fd, width: w as i32, height: h as i32, crtc_id, conn_id, mode, saved, bufs: Vec::new(), front: 0, prev: 0, flips_out: 0, flips_late: 0, use_flip: true, fit: None };
        // on any error the buffers made so far are freed again (the HDMI search retries)
        match d.alloc().and_then(|_| d.set_crtc(0)) {
            Ok(()) => Ok(d),
            Err(e) => {
                d.saved = None;
                d.restore();
                Err(e)
            }
        }
    }

    fn alloc(&mut self) -> io::Result<()> {
        let (fd, w, h) = (self.fd, self.width as u32, self.height as u32);
        // three: the GPU fills the third while one is shown and the next waits for its flip (System > Graphics)
        for _ in 0..3 {
            let mut cd = CreateDumb { width: w, height: h, bpp: 32, ..Default::default() };
            ioc(fd, CREATE_DUMB, &mut cd)?;
            let undo = |fb: Option<u32>, e: io::Error| {
                if let Some(mut f) = fb {
                    let _ = ioc(fd, RMFB, &mut f);
                }
                let mut h = cd.handle;
                let _ = ioc(fd, DESTROY_DUMB, &mut h);
                e
            };
            let mut fb = FbCmd { width: w, height: h, pitch: cd.pitch, bpp: 32, depth: 24, handle: cd.handle, ..Default::default() };
            ioc(fd, ADDFB, &mut fb).map_err(|e| undo(None, e))?;
            let mut md = MapDumb { handle: cd.handle, ..Default::default() };
            ioc(fd, MAP_DUMB, &mut md).map_err(|e| undo(Some(fb.fb_id), e))?;
            let map = unsafe { libc::mmap(std::ptr::null_mut(), cd.size as usize, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, md.offset as libc::off_t) };
            if map == libc::MAP_FAILED {
                return Err(undo(Some(fb.fb_id), io::Error::last_os_error()));
            }
            unsafe { std::ptr::write_bytes(map as *mut u8, 0, cd.size as usize) };
            self.bufs.push(Buf { handle: cd.handle, fb: fb.fb_id, pitch: cd.pitch, size: cd.size as usize, map: map as *mut u32 });
        }
        Ok(())
    }

    fn set_crtc(&mut self, i: usize) -> io::Result<()> {
        let mut conn = self.conn_id;
        let mut c = Crtc { set_connectors_ptr: &mut conn as *mut u32 as u64, count_connectors: 1, crtc_id: self.crtc_id, fb_id: self.bufs[i].fb, x: 0, y: 0, gamma_size: 0, mode_valid: 1, mode: self.mode };
        ioc(self.fd, SETCRTC, &mut c)
    }

    /// Waits up to 100 ms for the events of the flips asked for. A late one stays counted (`flips_out`) and is read
    /// the next time, so it never passes for the flip after it.
    fn wait_flip(&mut self) {
        while self.flips_out > 0 {
            let mut p = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
            if unsafe { libc::poll(&mut p, 1, 100) } <= 0 {
                self.flips_late += 1;
                if self.flips_late.is_power_of_two() {
                    eprintln!("oos-ui: page flip event late ({} times)", self.flips_late);
                }
                return;
            }
            let mut buf = [0u8; 256];
            let n = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n <= 0 {
                return;
            }
            self.flips_out = self.flips_out.saturating_sub(flip_events(&buf[..n as usize]));
        }
    }

    /// Copies the canvas into the back buffer and shows it at the next vblank.
    pub fn present(&mut self, cv: &Canvas) {
        let back = self.back();
        let b = &self.bufs[back];
        let stride = (b.pitch / 4) as usize;
        let w = (cv.w.min(self.width)) as usize;
        let h = (cv.h.min(self.height)) as usize;
        for y in 0..h {
            unsafe {
                std::ptr::copy_nonoverlapping(cv.px.as_ptr().add(y * cv.w as usize), b.map.add(y * stride), w);
            }
        }
        self.flip(back);
    }

    /// Scan-out buffer `i` as a dma-buf for the GPU (System > Graphics = GPU), with its row pitch in bytes.
    pub fn export(&self, i: usize) -> io::Result<(std::os::fd::OwnedFd, i32)> {
        use std::os::fd::FromRawFd;
        let b = self.bufs.get(i).ok_or_else(|| io::Error::other("no such buffer"))?;
        let mut p = PrimeHandle { handle: b.handle, flags: (libc::O_CLOEXEC | libc::O_RDWR) as u32, fd: -1 };
        ioc(self.fd, PRIME_HANDLE_TO_FD, &mut p)?;
        // SAFETY: the kernel just made this descriptor for us
        Ok((unsafe { std::os::fd::OwnedFd::from_raw_fd(p.fd) }, b.pitch as i32))
    }

    pub fn buffers(&self) -> usize {
        self.bufs.len()
    }

    /// The buffer the next picture goes into, once the last flip is done.
    pub fn back(&mut self) -> usize {
        self.wait_flip();
        (self.front + 1) % self.bufs.len()
    }

    /// A buffer neither shown nor waiting to be: the GPU fills it without waiting for the flip.
    pub fn free(&self) -> usize {
        (0..self.bufs.len()).find(|&i| i != self.front && i != self.prev).unwrap_or((self.front + 1) % self.bufs.len())
    }

    /// Shows buffer `back` (filled by `present` or by the GPU) at the next vblank, once the last flip is done.
    pub fn flip(&mut self, back: usize) {
        self.wait_flip();
        let b = &self.bufs[back];
        if self.use_flip {
            let mut pf = PageFlip { crtc_id: self.crtc_id, fb_id: b.fb, flags: PAGE_FLIP_EVENT, reserved: 0, user_data: 0 };
            match ioc(self.fd, PAGE_FLIP, &mut pf) {
                Ok(()) => {
                    self.flips_out += 1;
                    (self.prev, self.front) = (self.front, back);
                    return;
                }
                // the last flip still in the kernel (its event late): this frame by SETCRTC, flips go on after it
                Err(e) if e.raw_os_error() == Some(libc::EBUSY) => {}
                Err(e) => {
                    eprintln!("oos-ui: page flip failed ({}), using SETCRTC", e);
                    self.use_flip = false;
                }
            }
        }
        let _ = self.set_crtc(back);
        (self.prev, self.front) = (self.front, back);
    }

    /// HDMI: the canvas scaled into the mode with its aspect ratio (black bars), shown at the next vblank. The flip
    /// asks for no event (the panel's flip events come on the same file); a flip still pending skips the frame.
    pub fn present_scaled(&mut self, cv: &Canvas) {
        if self.fit.as_ref().is_none_or(|f| f.src != (cv.w, cv.h)) {
            self.fit = Some(crate::gfx::Fit::new(cv.w, cv.h, self.width, self.height));
            // the bars: both buffers black once
            for b in &self.bufs {
                unsafe { std::ptr::write_bytes(b.map as *mut u8, 0, b.size) };
            }
        }
        let back = self.free();
        let b = &self.bufs[back];
        let fit = self.fit.as_ref().unwrap();
        let stride = (b.pitch / 4) as usize;
        let dst = unsafe { std::slice::from_raw_parts_mut(b.map, b.size / 4) };
        fit.scale(&cv.px, dst, stride);
        self.flip_quiet(back);
    }

    /// HDMI: shows buffer `back` (filled by the GPU, its bars included) without a flip event; a flip still pending
    /// skips the frame. The CPU's scaled picture blackens its bars again afterwards.
    pub fn flip_scaled(&mut self, back: usize) {
        self.fit = None;
        self.flip_quiet(back);
    }

    fn flip_quiet(&mut self, back: usize) {
        let b = &self.bufs[back];
        let mut pf = PageFlip { crtc_id: self.crtc_id, fb_id: b.fb, flags: 0, reserved: 0, user_data: 0 };
        match ioc(self.fd, PAGE_FLIP, &mut pf) {
            Ok(()) => (self.prev, self.front) = (self.front, back),
            Err(e) if e.raw_os_error() == Some(libc::EBUSY) => {}
            Err(_) => {
                if self.set_crtc(back).is_ok() {
                    (self.prev, self.front) = (self.front, back);
                }
            }
        }
    }

    /// The connector is still connected (a cheap state read: no new probe of the monitor).
    pub fn connected(&self) -> bool {
        let mut gc = GetConnector { connector_id: self.conn_id, ..Default::default() };
        // count_modes 0 asks the kernel to probe; GETCONNECTOR with modes_ptr set and count_modes 1 does not
        let mut mode = unsafe { std::mem::zeroed::<ModeInfo>() };
        gc.modes_ptr = &mut mode as *mut ModeInfo as u64;
        gc.count_modes = 1;
        ioc(self.fd, GETCONNECTOR, &mut gc).is_ok() && gc.connection == 1
    }

    /// Turns the output off and frees it (HDMI unplugged or oos-ui ending).
    pub fn close(mut self) {
        let mut c = Crtc { set_connectors_ptr: 0, count_connectors: 0, crtc_id: self.crtc_id, fb_id: 0, x: 0, y: 0, gamma_size: 0, mode_valid: 0, mode: self.mode };
        let _ = ioc(self.fd, SETCRTC, &mut c);
        self.saved = None;
        self.restore();
        unsafe { libc::close(self.fd) };
    }

    pub fn restore(&mut self) {
        self.wait_flip();
        if let Some(mut s) = self.saved {
            if s.fb_id != 0 {
                let mut conn = self.conn_id;
                s.set_connectors_ptr = &mut conn as *mut u32 as u64;
                s.count_connectors = 1;
                let _ = ioc(self.fd, SETCRTC, &mut s);
            }
        }
        for b in self.bufs.drain(..) {
            unsafe { libc::munmap(b.map as *mut libc::c_void, b.size) };
            let mut fb = b.fb;
            let _ = ioc(self.fd, RMFB, &mut fb);
            let mut h = b.handle;
            let _ = ioc(self.fd, DESTROY_DUMB, &mut h);
        }
    }
}
