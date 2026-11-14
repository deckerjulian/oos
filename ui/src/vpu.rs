// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The VPU's JPEG encoder for the live picture of the web remote (`http.rs`, `/screen.jpg`).
//!
//! The RK3399 has a Hantro video unit whose encoder makes JPEG; Linux drives it with the upstream `hantro-vpu`
//! driver as a V4L2 memory-to-memory device (kernel/config, the `vpu` node of the device tree). The screen goes in as
//! it is, xrgb, and the encoder converts it to YCbCr itself (`kernel/patches/0012-…`; the driver of upstream Linux
//! takes only YUV); with a kernel without that, the screen is converted to YUYV here first, which took most of the
//! encoder thread's time. A complete JPEG file comes out. The encoder works in lines of 16: a
//! screen of 600 lines is encoded as 608 (the last line repeated) and the JPEG header then says 600, which decoders
//! take, since both need the same 38 rows of blocks.
//!
//! The V4L2 structures are written out here (`#[repr(C)]`, 64-bit layout, sizes checked by a test), as for DRM.

/// JPEG quality (1 to 100): text and lines of the screen stay sharp, a screen is about 60 to 100 KB.
pub const QUALITY: i32 = 75;

/// xrgb pixels into the encoder's buffer as they are, rows of `stride` bytes; rows below `h` up to `rows` repeat the
/// last one.
pub fn xrgb(px: &[u32], w: usize, h: usize, out: &mut [u8], stride: usize, rows: usize) {
    for y in 0..rows {
        let src = &px[y.min(h - 1) * w..][..w];
        // SAFETY: a u32 slice seen as its bytes
        let src = unsafe { std::slice::from_raw_parts(src.as_ptr() as *const u8, w * 4) };
        out[y * stride..][..w * 4].copy_from_slice(src);
    }
}

/// xrgb pixels to YUYV (JFIF: BT.601, full range) into `out`, rows of `stride` bytes; rows below `h` up to `rows`
/// repeat the last one.
pub fn yuyv(px: &[u32], w: usize, h: usize, out: &mut [u8], stride: usize, rows: usize) {
    for y in 0..rows {
        let src = &px[y.min(h - 1) * w..][..w];
        let dst = &mut out[y * stride..][..w * 2];
        for (p, d) in src.as_chunks::<2>().0.iter().zip(dst.as_chunks_mut::<4>().0) {
            let c = |v: u32| (((v >> 16) & 255) as i32, ((v >> 8) & 255) as i32, (v & 255) as i32);
            let ((r0, g0, b0), (r1, g1, b1)) = (c(p[0]), c(p[1]));
            let luma = |r: i32, g: i32, b: i32| ((19595 * r + 38470 * g + 7471 * b + 32768) >> 16) as u8;
            // the chroma of the pair: of the sum, halved in the scaling
            let (r, g, b) = (r0 + r1, g0 + g1, b0 + b1);
            let cb = (((-11059 * r - 21709 * g + 32768 * b + 65536) >> 17) + 128).clamp(0, 255) as u8;
            let cr = (((32768 * r - 27439 * g - 5329 * b + 65536) >> 17) + 128).clamp(0, 255) as u8;
            *d = [luma(r0, g0, b0), cb, luma(r1, g1, b1), cr];
        }
    }
}

/// The height in the frame header (SOF0) of a JPEG file set to `h`.
pub fn set_jpeg_height(jpeg: &mut [u8], h: u16) -> bool {
    let mut i = 2;
    while i + 9 <= jpeg.len() {
        if jpeg[i] != 0xff {
            return false;
        }
        let m = jpeg[i + 1];
        let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        if (0xc0..=0xc2).contains(&m) {
            jpeg[i + 5..i + 7].copy_from_slice(&h.to_be_bytes());
            return true;
        }
        if m == 0xda {
            return false;
        }
        i += 2 + len;
    }
    false
}

#[cfg(target_os = "linux")]
pub use imp::JpegEnc;

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::os::fd::{AsRawFd, OwnedFd};

    const VIDIOC_QUERYCAP: u64 = 0x8068_5600;
    const VIDIOC_S_FMT: u64 = 0xc0d0_5605;
    const VIDIOC_REQBUFS: u64 = 0xc014_5608;
    const VIDIOC_QUERYBUF: u64 = 0xc058_5609;
    const VIDIOC_QBUF: u64 = 0xc058_560f;
    const VIDIOC_DQBUF: u64 = 0xc058_5611;
    const VIDIOC_STREAMON: u64 = 0x4004_5612;
    const VIDIOC_S_CTRL: u64 = 0xc008_561c;
    const CAPTURE: u32 = 9; // V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE
    const OUTPUT: u32 = 10; // V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE
    const MMAP: u32 = 1;
    const CAP_M2M_MPLANE: u32 = 0x4000;
    const CID_JPEG_QUALITY: u32 = 0x009d_0903;
    const fn fourcc(s: &[u8; 4]) -> u32 {
        s[0] as u32 | (s[1] as u32) << 8 | (s[2] as u32) << 16 | (s[3] as u32) << 24
    }
    const JPEG: u32 = fourcc(b"JPEG");
    const YUYV: u32 = fourcc(b"YUYV");
    /// V4L2_PIX_FMT_XBGR32: bytes B, G, R, X, the canvas' xrgb words
    const XRGB: u32 = fourcc(b"XR24");

    #[repr(C)]
    pub(super) struct Cap {
        pub driver: [u8; 16],
        card: [u8; 32],
        bus: [u8; 32],
        version: u32,
        caps: u32,
        pub device_caps: u32,
        res: [u32; 3],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub(super) struct PlaneFmt {
        pub sizeimage: u32,
        pub bytesperline: u32,
        res: [u16; 6],
    }

    #[repr(C)]
    pub(super) struct Format {
        pub typ: u32,
        pad: u32,
        pub width: u32,
        pub height: u32,
        pub pixfmt: u32,
        field: u32,
        colorspace: u32,
        pub planes: [PlaneFmt; 8],
        pub num_planes: u8,
        flags: u8,
        ycbcr: u8,
        quant: u8,
        xfer: u8,
        res: [u8; 7],
        rest: [u8; 8],
    }

    #[repr(C)]
    pub(super) struct ReqBufs {
        count: u32,
        typ: u32,
        memory: u32,
        caps: u32,
        flags: u8,
        res: [u8; 3],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub(super) struct Plane {
        bytesused: u32,
        length: u32,
        m: u64,
        data_offset: u32,
        res: [u32; 11],
    }

    #[repr(C)]
    pub(super) struct Buffer {
        index: u32,
        typ: u32,
        bytesused: u32,
        flags: u32,
        field: u32,
        pad: u32,
        ts: [i64; 2],
        timecode: [u32; 4],
        sequence: u32,
        memory: u32,
        m: u64,
        length: u32,
        reserved2: u32,
        request_fd: i32,
        pad2: u32,
    }

    #[repr(C)]
    struct Control {
        id: u32,
        value: i32,
    }

    fn zeroed<T>() -> T {
        // SAFETY: plain C structs of integers, all zero is valid
        unsafe { std::mem::zeroed() }
    }

    fn ioc<T>(fd: i32, req: u64, arg: &mut T) -> std::io::Result<()> {
        loop {
            // SAFETY: `arg` is the struct this request takes
            let r = unsafe { libc::ioctl(fd, req as _, arg as *mut T) };
            if r == 0 {
                return Ok(());
            }
            let e = std::io::Error::last_os_error();
            if e.kind() != std::io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }

    /// A mapped buffer plane.
    struct Map {
        ptr: *mut u8,
        len: usize,
    }

    impl Drop for Map {
        fn drop(&mut self) {
            unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len) };
        }
    }

    /// The encoder, set up for one picture size: one raw buffer in, one JPEG buffer out.
    pub struct JpegEnc {
        fd: OwnedFd,
        w: usize,
        h: usize,
        rows: usize,
        stride: usize,
        raw: Map,
        raw_size: u32,
        jpeg: Map,
        /// the encoder takes xrgb (else YUYV)
        rgb: bool,
    }

    // SAFETY: the mappings belong to the encoder and are used only through &mut self
    unsafe impl Send for JpegEnc {}

    impl JpegEnc {
        /// The VPU's JPEG encoder for pictures of `w` x `h`: the video device of `hantro-vpu` that makes JPEG.
        pub fn open(w: usize, h: usize) -> Result<JpegEnc, String> {
            let mut last = String::from("no hantro-vpu video device");
            for n in 0..16 {
                let path = format!("/dev/video{}\0", n);
                // SAFETY: a NUL-terminated path
                let fd = unsafe { libc::open(path.as_ptr() as *const libc::c_char, libc::O_RDWR | libc::O_CLOEXEC) };
                if fd < 0 {
                    continue;
                }
                // SAFETY: a fresh descriptor we own
                let fd = unsafe { <OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(fd) };
                let mut cap: Cap = zeroed();
                if ioc(fd.as_raw_fd(), VIDIOC_QUERYCAP, &mut cap).is_err() || !cap.driver.starts_with(b"hantro-vpu") || cap.device_caps & CAP_M2M_MPLANE == 0 {
                    continue;
                }
                match Self::setup(fd, w, h) {
                    Ok(e) => return Ok(e),
                    Err(e) => last = format!("/dev/video{}: {}", n, e),
                }
            }
            Err(last)
        }

        fn setup(fd: OwnedFd, w: usize, h: usize) -> Result<JpegEnc, String> {
            let f = fd.as_raw_fd();
            // the coded format first (it chooses the encoder), then the raw one
            let mut cf: Format = zeroed();
            (cf.typ, cf.width, cf.height, cf.pixfmt, cf.num_planes) = (CAPTURE, w as u32, h as u32, JPEG, 1);
            ioc(f, VIDIOC_S_FMT, &mut cf).map_err(|e| format!("JPEG format: {}", e))?;
            if cf.pixfmt != JPEG {
                return Err("no JPEG encoder".into());
            }
            // xrgb when the driver takes it (it answers with another format when not), else YUYV
            let raw_fmt = |pixfmt: u32, bytes: usize| {
                let mut rf: Format = zeroed();
                (rf.typ, rf.width, rf.height, rf.pixfmt, rf.num_planes) = (OUTPUT, w as u32, h as u32, pixfmt, 1);
                let ok = ioc(f, VIDIOC_S_FMT, &mut rf).is_ok() && rf.pixfmt == pixfmt && rf.width as usize >= w && rf.height as usize >= h && rf.planes[0].bytesperline as usize >= w * bytes;
                ok.then_some(rf)
            };
            let (rf, rgb) = match raw_fmt(XRGB, 4) {
                Some(rf) if std::env::var_os("OOS_VPU_YUYV").is_none() => (rf, true),
                _ => (raw_fmt(YUYV, 2).ok_or_else(|| format!("raw format {}x{} not taken", w, h))?, false),
            };
            let mut q = Control { id: CID_JPEG_QUALITY, value: QUALITY };
            let _ = ioc(f, VIDIOC_S_CTRL, &mut q);
            let raw = Self::buffer(f, OUTPUT)?;
            let jpeg = Self::buffer(f, CAPTURE)?;
            for t in [OUTPUT, CAPTURE] {
                let mut t = t as i32;
                ioc(f, VIDIOC_STREAMON, &mut t).map_err(|e| format!("stream on: {}", e))?;
            }
            Ok(JpegEnc { fd, w, h, rows: rf.height as usize, stride: rf.planes[0].bytesperline as usize, raw_size: rf.planes[0].sizeimage, raw, jpeg, rgb })
        }

        /// What the encoder is given: the screen as it is, or converted to YUYV here.
        pub fn input(&self) -> &'static str {
            if self.rgb { "xrgb" } else { "YUYV" }
        }

        /// One buffer of the queue, mapped.
        fn buffer(f: i32, typ: u32) -> Result<Map, String> {
            let mut r = ReqBufs { count: 1, typ, memory: MMAP, caps: 0, flags: 0, res: [0; 3] };
            ioc(f, VIDIOC_REQBUFS, &mut r).map_err(|e| format!("buffers: {}", e))?;
            if r.count < 1 {
                return Err("no buffer".into());
            }
            let mut planes = [zeroed::<Plane>(); 1];
            let mut b: Buffer = zeroed();
            (b.typ, b.memory, b.m, b.length) = (typ, MMAP, planes.as_mut_ptr() as u64, 1);
            ioc(f, VIDIOC_QUERYBUF, &mut b).map_err(|e| format!("buffer: {}", e))?;
            let (len, off) = (planes[0].length as usize, planes[0].m as u32 as libc::off_t);
            // SAFETY: the plane's offset and length as the driver gave them
            let p = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, f, off) };
            if p == libc::MAP_FAILED {
                return Err(format!("map: {}", std::io::Error::last_os_error()));
            }
            Ok(Map { ptr: p as *mut u8, len })
        }

        fn queue(&self, typ: u32, used: u32) -> Result<(), String> {
            let mut planes = [zeroed::<Plane>(); 1];
            planes[0].bytesused = used;
            let mut b: Buffer = zeroed();
            (b.typ, b.memory, b.m, b.length) = (typ, MMAP, planes.as_mut_ptr() as u64, 1);
            ioc(self.fd.as_raw_fd(), VIDIOC_QBUF, &mut b).map_err(|e| format!("queue: {}", e))
        }

        fn dequeue(&self, typ: u32) -> Result<(u32, u32), String> {
            let mut planes = [zeroed::<Plane>(); 1];
            let mut b: Buffer = zeroed();
            (b.typ, b.memory, b.m, b.length) = (typ, MMAP, planes.as_mut_ptr() as u64, 1);
            ioc(self.fd.as_raw_fd(), VIDIOC_DQBUF, &mut b).map_err(|e| format!("dequeue: {}", e))?;
            Ok((planes[0].bytesused, b.flags))
        }

        /// The xrgb screen `px` (the size the encoder was opened for) as a JPEG file.
        pub fn encode(&mut self, px: &[u32]) -> Result<Vec<u8>, String> {
            if px.len() != self.w * self.h {
                return Err("picture of another size".into());
            }
            // SAFETY: the raw buffer's mapping, `stride` x `rows` bytes as the format said
            let raw = unsafe { std::slice::from_raw_parts_mut(self.raw.ptr, self.raw.len) };
            if self.stride * self.rows > raw.len() {
                return Err("raw buffer too small".into());
            }
            if self.rgb {
                xrgb(px, self.w, self.h, raw, self.stride, self.rows);
            } else {
                yuyv(px, self.w, self.h, raw, self.stride, self.rows);
            }
            self.queue(OUTPUT, self.raw_size)?;
            self.queue(CAPTURE, 0)?;
            let mut p = libc::pollfd { fd: self.fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            // SAFETY: one pollfd
            if unsafe { libc::poll(&mut p, 1, 1000) } <= 0 {
                return Err("encoder did not answer".into());
            }
            let (n, flags) = self.dequeue(CAPTURE)?;
            self.dequeue(OUTPUT)?;
            if flags & 0x40 != 0 || n == 0 || n as usize > self.jpeg.len {
                return Err("encoder error".into());
            }
            // SAFETY: the JPEG buffer's mapping, n bytes written by the encoder
            let mut out = unsafe { std::slice::from_raw_parts(self.jpeg.ptr, n as usize) }.to_vec();
            if self.rows != self.h && !set_jpeg_height(&mut out, self.h as u16) {
                return Err("no frame header in the JPEG".into());
            }
            Ok(out)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn v4l2_structs_have_the_kernels_layout() {
            use std::mem::size_of;
            assert_eq!((size_of::<Cap>(), size_of::<Format>(), size_of::<ReqBufs>(), size_of::<Plane>(), size_of::<Buffer>()), (104, 208, 20, 64, 88));
        }
    }
}

/// No VPU off the scope's Linux.
#[cfg(not(target_os = "linux"))]
pub struct JpegEnc;

#[cfg(not(target_os = "linux"))]
impl JpegEnc {
    pub fn open(_: usize, _: usize) -> Result<JpegEnc, String> {
        Err("no VPU".into())
    }
    pub fn encode(&mut self, _: &[u32]) -> Result<Vec<u8>, String> {
        Err("no VPU".into())
    }
    pub fn input(&self) -> &'static str {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuyv_of_grey_white_and_colours() {
        let px = [0x000000, 0xffffff, 0xff0000, 0xff0000];
        let mut out = [0u8; 16];
        yuyv(&px, 4, 1, &mut out, 8, 2);
        assert_eq!(&out[..4], &[0, 128, 255, 128], "black and white: no colour");
        // pure red: Y 76, Cb 85, Cr 255 (JFIF)
        assert_eq!(&out[4..8], &[76, 85, 76, 255]);
        assert_eq!(out[..8], out[8..], "the repeated row");
    }

    #[test]
    fn xrgb_rows_with_the_last_repeated() {
        let px = [0x0011_2233, 0x0044_5566];
        let mut out = [0u8; 24];
        xrgb(&px, 2, 1, &mut out, 12, 2);
        assert_eq!(&out[..8], &[0x33, 0x22, 0x11, 0, 0x66, 0x55, 0x44, 0]);
        assert_eq!(&out[8..12], &[0; 4], "the stride's padding is left");
        assert_eq!(out[..8], out[12..20]);
    }

    #[test]
    fn jpeg_height_is_set_in_the_frame_header() {
        // SOI, APP0 of 4 bytes, SOF0 with 608 x 1024, SOS
        let mut j = vec![0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0, 0xff, 0xc0, 0, 11, 8, 0x02, 0x60, 0x04, 0x00, 1, 1, 0x11, 0, 0xff, 0xda];
        assert!(set_jpeg_height(&mut j, 600));
        assert_eq!(&j[13..15], &600u16.to_be_bytes());
        assert_eq!(&j[15..17], &1024u16.to_be_bytes());
    }
}
