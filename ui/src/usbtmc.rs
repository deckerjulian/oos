// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! USBTMC host side over usbfs, for an instrument in the USB-A port (Bode with an external generator, ui/bode.rs).
//! The kernel has no USBTMC driver (`CONFIG_USB_TMC` is off), so oos-ui talks to the device node itself: it finds the
//! interface of class 0xFE subclass 0x03 under `/sys/bus/usb/devices`, opens `/dev/bus/usb/BBB/DDD` (devtmpfs makes
//! these nodes, oos-ui runs as root), claims the interface and moves USBTMC messages over its two bulk endpoints with
//! `USBDEVFS_BULK`.
//!
//! Framing (USBTMC 1.0, chapter 3): every bulk transfer starts with a 12-byte header: MsgID, bTag, its inverse, 0, the
//! transfer size (u32 LE), attributes, then 3 bytes (TermChar for a request, else reserved). The data follow, padded
//! with zeros to a multiple of 4. bTag runs 1..255 and wraps to 1 (never 0). A command is DEV_DEP_MSG_OUT with EOM on
//! its last transfer; an answer is fetched with REQUEST_DEV_DEP_MSG_IN and read back as DEV_DEP_MSG_IN, until EOM.

use std::io;
use std::path::{Path, PathBuf};

/// Interface class and subclass of a USBTMC function.
pub const CLASS: u8 = 0xfe;
pub const SUBCLASS: u8 = 0x03;
/// Where the kernel lists the USB devices and their interfaces.
pub const SYSFS: &str = "/sys/bus/usb/devices";

const DEV_DEP_MSG_OUT: u8 = 1;
const REQUEST_DEV_DEP_MSG_IN: u8 = 2;
const DEV_DEP_MSG_IN: u8 = 2;
/// Size of the bulk header.
pub const HEADER: usize = 12;
/// Data bytes per bulk transfer, both ways.
const CHUNK: usize = 4096;
/// An answer longer than this is cut off with an error.
const MAX_ANSWER: usize = 1 << 20;

/// Header of a DEV_DEP_MSG_OUT transfer with `size` data bytes; `eom` on the last transfer of a message.
pub fn out_header(tag: u8, size: u32, eom: bool) -> [u8; HEADER] {
    let s = size.to_le_bytes();
    [DEV_DEP_MSG_OUT, tag, !tag, 0, s[0], s[1], s[2], s[3], eom as u8, 0, 0, 0]
}

/// Header of a REQUEST_DEV_DEP_MSG_IN asking for at most `max` bytes, ending early at `term` if given.
pub fn request_header(tag: u8, max: u32, term: Option<u8>) -> [u8; HEADER] {
    let s = max.to_le_bytes();
    [REQUEST_DEV_DEP_MSG_IN, tag, !tag, 0, s[0], s[1], s[2], s[3], if term.is_some() { 2 } else { 0 }, term.unwrap_or(0), 0, 0]
}

/// One DEV_DEP_MSG_OUT transfer: header, data, zeros up to a multiple of 4.
pub fn out_transfer(tag: u8, data: &[u8], eom: bool) -> Vec<u8> {
    let mut v = Vec::with_capacity(HEADER + data.len() + 3);
    v.extend_from_slice(&out_header(tag, data.len() as u32, eom));
    v.extend_from_slice(data);
    v.resize(v.len().next_multiple_of(4), 0);
    v
}

/// The header of a DEV_DEP_MSG_IN transfer: data size and end of message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InHeader {
    pub size: u32,
    pub eom: bool,
}

/// Checks the header of a DEV_DEP_MSG_IN against the tag of its request.
pub fn parse_in_header(b: &[u8], tag: u8) -> Result<InHeader, String> {
    if b.len() < HEADER {
        return Err(format!("short answer ({} bytes)", b.len()));
    }
    if b[0] != DEV_DEP_MSG_IN {
        return Err(format!("answer with MsgID {}", b[0]));
    }
    if b[1] != tag || b[2] != !tag {
        return Err(format!("answer with bTag {} for {}", b[1], tag));
    }
    Ok(InHeader { size: u32::from_le_bytes([b[4], b[5], b[6], b[7]]), eom: b[8] & 1 != 0 })
}

/// The two bulk endpoints of the interface (usbfs on the unit, a mock in the tests).
pub trait Bulk {
    /// Sends one transfer, returns the bytes sent.
    fn bulk_out(&mut self, data: &[u8], timeout_ms: u32) -> io::Result<usize>;
    /// Receives one transfer (up to `buf.len()`, shorter at a short packet), returns its length.
    fn bulk_in(&mut self, buf: &mut [u8], timeout_ms: u32) -> io::Result<usize>;
    /// Clears a stall of the IN (`inp`) or OUT endpoint.
    fn clear_halt(&mut self, inp: bool) -> io::Result<()>;
}

/// A USBTMC instrument on `B`.
pub struct Usbtmc<B: Bulk> {
    pub bus: B,
    tag: u8,
    pub timeout_ms: u32,
}

/// errno for "device shut down" (Linux 108); the Windows C runtime has no such code, so the same number stands in.
#[cfg(unix)]
const ESHUTDOWN: i32 = libc::ESHUTDOWN;
#[cfg(not(unix))]
const ESHUTDOWN: i32 = 108;

/// A readable text for a failed transfer.
fn io_text(e: &io::Error) -> String {
    match e.raw_os_error() {
        Some(libc::ETIMEDOUT) => "no answer (timeout)".into(),
        Some(libc::EPIPE) => "endpoint stalled".into(),
        Some(libc::ENODEV) | Some(ESHUTDOWN) => "device unplugged".into(),
        _ => e.to_string(),
    }
}

impl<B: Bulk> Usbtmc<B> {
    pub fn new(bus: B) -> Self {
        Usbtmc { bus, tag: 1, timeout_ms: 2000 }
    }

    /// The next bTag: 1..255, then 1 again.
    fn next_tag(&mut self) -> u8 {
        let t = self.tag;
        self.tag = if t == 255 { 1 } else { t + 1 };
        t
    }

    fn send(&mut self, t: &[u8]) -> Result<(), String> {
        match self.bus.bulk_out(t, self.timeout_ms) {
            Ok(n) if n == t.len() => Ok(()),
            Ok(n) => Err(format!("sent {} of {} bytes", n, t.len())),
            Err(e) => {
                if e.raw_os_error() == Some(libc::EPIPE) {
                    let _ = self.bus.clear_halt(false);
                }
                Err(io_text(&e))
            }
        }
    }

    fn recv(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        self.bus.bulk_in(buf, self.timeout_ms).map_err(|e| {
            if e.raw_os_error() == Some(libc::EPIPE) {
                let _ = self.bus.clear_halt(true);
            }
            io_text(&e)
        })
    }

    /// Sends one message (DEV_DEP_MSG_OUT), in transfers of at most `CHUNK` bytes, EOM on the last.
    pub fn write(&mut self, msg: &[u8]) -> Result<(), String> {
        let mut parts = msg.chunks(CHUNK).peekable();
        if parts.peek().is_none() {
            let tag = self.next_tag();
            return self.send(&out_transfer(tag, &[], true));
        }
        while let Some(p) = parts.next() {
            let tag = self.next_tag();
            let eom = parts.peek().is_none();
            self.send(&out_transfer(tag, p, eom))?;
        }
        Ok(())
    }

    /// Reads one message: REQUEST_DEV_DEP_MSG_IN, then DEV_DEP_MSG_IN until its EOM.
    pub fn read(&mut self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        // a multiple of every bulk packet size (64 at full speed, 512 at high speed), so no transfer overflows
        let mut buf = vec![0u8; (HEADER + CHUNK).next_multiple_of(512)];
        loop {
            let tag = self.next_tag();
            self.send(&request_header(tag, CHUNK as u32, None))?;
            let mut got = self.recv(&mut buf)?;
            let h = parse_in_header(&buf[..got], tag)?;
            if h.size as usize > CHUNK {
                return Err(format!("answer of {} bytes for {} asked", h.size, CHUNK));
            }
            let want = HEADER + h.size as usize;
            let mut data = buf[HEADER..got.min(want)].to_vec();
            // the rest of a transfer can come in more bulk reads
            while got < want {
                let n = self.recv(&mut buf)?;
                if n == 0 {
                    return Err("answer cut short".into());
                }
                let take = n.min(want - got);
                data.extend_from_slice(&buf[..take]);
                got += n;
            }
            out.extend_from_slice(&data);
            if h.eom {
                return Ok(out);
            }
            if out.len() > MAX_ANSWER {
                return Err("answer too long".into());
            }
        }
    }

    /// Sends a command line (newline added).
    pub fn command(&mut self, line: &str) -> Result<(), String> {
        self.write(format!("{}\n", line).as_bytes())
    }

    /// Sends a query and returns its answer without the line end.
    pub fn query(&mut self, line: &str) -> Result<String, String> {
        self.command(line)?;
        let a = self.read()?;
        Ok(String::from_utf8_lossy(&a).trim_end_matches(['\n', '\r']).to_string())
    }
}

// ---------------------------------------------------------------------------------------------- finding the device

/// A USBTMC interface as sysfs lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub bus: u32,
    pub dev: u32,
    pub ifno: u32,
    pub ep_out: u8,
    pub ep_in: u8,
    /// "vid:pid", hex
    pub id: String,
    /// manufacturer and product strings, if the device has them
    pub name: String,
}

impl Device {
    pub fn node(&self) -> PathBuf {
        PathBuf::from(format!("/dev/bus/usb/{:03}/{:03}", self.bus, self.dev))
    }
}

fn attr(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name)).ok().map(|s| s.trim().to_string())
}

fn hex(dir: &Path, name: &str) -> Option<u32> {
    attr(dir, name).and_then(|s| u32::from_str_radix(&s, 16).ok())
}

/// The bulk endpoints of an interface directory: (out, in).
fn bulk_endpoints(ifdir: &Path) -> (Option<u8>, Option<u8>) {
    let (mut o, mut i) = (None, None);
    let mut eps: Vec<PathBuf> = std::fs::read_dir(ifdir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("ep_"))).collect();
    eps.sort();
    for ep in eps {
        if attr(&ep, "type").as_deref() != Some("Bulk") {
            continue;
        }
        let Some(a) = hex(&ep, "bEndpointAddress").map(|a| a as u8) else { continue };
        if a & 0x80 != 0 {
            i = i.or(Some(a));
        } else {
            o = o.or(Some(a));
        }
    }
    (o, i)
}

/// The first USBTMC interface under `root` (`SYSFS`). The error names the other devices found, if any.
pub fn find(root: &Path) -> Result<Device, String> {
    let mut names: Vec<String> = std::fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    names.sort();
    let mut others = Vec::new();
    for n in &names {
        let dir = root.join(n);
        let Some((devname, _)) = n.split_once(':') else {
            // a device: note it unless it is a hub
            if hex(&dir, "bDeviceClass") != Some(9) {
                if let (Some(v), Some(p)) = (hex(&dir, "idVendor"), hex(&dir, "idProduct")) {
                    let prod = attr(&dir, "product").map_or(String::new(), |s| format!(" {}", s));
                    others.push(format!("{:04x}:{:04x}{}", v, p, prod));
                }
            }
            continue;
        };
        if hex(&dir, "bInterfaceClass") != Some(CLASS as u32) || hex(&dir, "bInterfaceSubClass") != Some(SUBCLASS as u32) {
            continue;
        }
        let ddir = root.join(devname);
        let num = |k: &str| attr(&ddir, k).and_then(|s| s.parse::<u32>().ok());
        let (Some(bus), Some(dev)) = (num("busnum"), num("devnum")) else { continue };
        let (Some(ep_out), Some(ep_in)) = bulk_endpoints(&dir) else {
            return Err(format!("USBTMC interface {} without two bulk endpoints", n));
        };
        let name = [attr(&ddir, "manufacturer"), attr(&ddir, "product")].into_iter().flatten().collect::<Vec<_>>().join(" ");
        return Ok(Device {
            bus,
            dev,
            ifno: hex(&dir, "bInterfaceNumber").unwrap_or(0),
            ep_out,
            ep_in,
            id: format!("{:04x}:{:04x}", hex(&ddir, "idVendor").unwrap_or(0), hex(&ddir, "idProduct").unwrap_or(0)),
            name,
        });
    }
    if others.is_empty() {
        Err("no USB device found (plug the generator into the USB-A port)".into())
    } else {
        Err(format!("no USBTMC device found; on USB, not USBTMC: {}", others.join(", ")))
    }
}

// ---------------------------------------------------------------------------------------------- usbfs

#[cfg(unix)]
const fn ioc(dir: u32, nr: u32, size: usize) -> libc::c_ulong {
    ((dir << 30) | ((size as u32) << 16) | ((b'U' as u32) << 8) | nr) as libc::c_ulong
}
#[cfg(unix)]
const IOC_W: u32 = 1;
#[cfg(unix)]
const IOC_R: u32 = 2;

#[cfg(unix)]
/// struct usbdevfs_bulktransfer
#[repr(C)]
struct BulkTransfer {
    ep: u32,
    len: u32,
    timeout: u32,
    data: *mut libc::c_void,
}

#[cfg(unix)]
/// struct usbdevfs_ioctl
#[repr(C)]
struct UsbIoctl {
    ifno: libc::c_int,
    code: libc::c_int,
    data: *mut libc::c_void,
}

#[cfg(unix)]
const USBDEVFS_BULK: libc::c_ulong = ioc(IOC_R | IOC_W, 2, std::mem::size_of::<BulkTransfer>());
#[cfg(unix)]
const USBDEVFS_CLAIMINTERFACE: libc::c_ulong = ioc(IOC_R, 15, 4);
#[cfg(unix)]
const USBDEVFS_RELEASEINTERFACE: libc::c_ulong = ioc(IOC_R, 16, 4);
#[cfg(unix)]
const USBDEVFS_IOCTL: libc::c_ulong = ioc(IOC_R | IOC_W, 18, std::mem::size_of::<UsbIoctl>());
#[cfg(unix)]
const USBDEVFS_CLEAR_HALT: libc::c_ulong = ioc(IOC_R, 21, 4);
#[cfg(unix)]
const USBDEVFS_DISCONNECT: libc::c_ulong = ioc(0, 22, 0);

#[cfg(unix)]
fn ioctl<T>(fd: i32, req: libc::c_ulong, arg: *mut T) -> io::Result<i32> {
    loop {
        let r = unsafe { libc::ioctl(fd, req as _, arg) };
        if r >= 0 {
            return Ok(r);
        }
        let e = io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::EINTR) {
            return Err(e);
        }
    }
}

#[cfg(unix)]
/// A claimed USBTMC interface on its usbfs node.
pub struct UsbFs {
    file: std::fs::File,
    ifno: u32,
    ep_out: u8,
    ep_in: u8,
}

#[cfg(unix)]
impl UsbFs {
    /// Opens the device node and claims the interface (a kernel driver on it is detached first).
    pub fn open(d: &Device) -> Result<UsbFs, String> {
        use std::os::fd::AsRawFd;
        let node = d.node();
        let file = std::fs::OpenOptions::new().read(true).write(true).open(&node).map_err(|e| match e.raw_os_error() {
            Some(libc::EACCES) | Some(libc::EPERM) => format!("no permission for {}", node.display()),
            Some(libc::ENOENT) => format!("{} missing", node.display()),
            _ => format!("{}: {}", node.display(), e),
        })?;
        let fd = file.as_raw_fd();
        let mut ifno = d.ifno;
        if let Err(e) = ioctl(fd, USBDEVFS_CLAIMINTERFACE, &mut ifno) {
            if e.raw_os_error() != Some(libc::EBUSY) {
                return Err(format!("claiming the interface: {}", e));
            }
            // a kernel driver holds it: let go of it, then claim again
            let mut req = UsbIoctl { ifno: d.ifno as libc::c_int, code: USBDEVFS_DISCONNECT as libc::c_int, data: std::ptr::null_mut() };
            let _ = ioctl(fd, USBDEVFS_IOCTL, &mut req);
            ioctl(fd, USBDEVFS_CLAIMINTERFACE, &mut ifno).map_err(|e| format!("interface in use: {}", e))?;
        }
        Ok(UsbFs { file, ifno: d.ifno, ep_out: d.ep_out, ep_in: d.ep_in })
    }

    fn fd(&self) -> i32 {
        use std::os::fd::AsRawFd;
        self.file.as_raw_fd()
    }

    fn bulk(&mut self, ep: u8, ptr: *mut u8, len: usize, timeout_ms: u32) -> io::Result<usize> {
        let mut t = BulkTransfer { ep: ep as u32, len: len as u32, timeout: timeout_ms, data: ptr as *mut libc::c_void };
        ioctl(self.fd(), USBDEVFS_BULK, &mut t).map(|n| n as usize)
    }
}

#[cfg(unix)]
impl Bulk for UsbFs {
    fn bulk_out(&mut self, data: &[u8], timeout_ms: u32) -> io::Result<usize> {
        // usbfs only reads from this buffer for an OUT endpoint
        self.bulk(self.ep_out, data.as_ptr().cast_mut(), data.len(), timeout_ms)
    }
    fn bulk_in(&mut self, buf: &mut [u8], timeout_ms: u32) -> io::Result<usize> {
        self.bulk(self.ep_in, buf.as_mut_ptr(), buf.len(), timeout_ms)
    }
    fn clear_halt(&mut self, inp: bool) -> io::Result<()> {
        let mut ep = if inp { self.ep_in } else { self.ep_out } as u32;
        ioctl(self.fd(), USBDEVFS_CLEAR_HALT, &mut ep).map(|_| ())
    }
}

#[cfg(unix)]
impl Drop for UsbFs {
    fn drop(&mut self) {
        let mut ifno = self.ifno;
        let _ = ioctl(self.fd(), USBDEVFS_RELEASEINTERFACE, &mut ifno);
    }
}

/// Windows desktop build: there is no usbfs, so no USB instrument can be opened (the Bode generator falls back to its
/// other links).
#[cfg(not(unix))]
pub struct UsbFs;

#[cfg(not(unix))]
impl UsbFs {
    pub fn open(d: &Device) -> Result<UsbFs, String> {
        Err(format!("{}: USBTMC over usbfs needs Linux", d.node().display()))
    }
}

#[cfg(not(unix))]
impl Bulk for UsbFs {
    fn bulk_out(&mut self, _data: &[u8], _timeout_ms: u32) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "USBTMC over usbfs needs Linux"))
    }
    fn bulk_in(&mut self, _buf: &mut [u8], _timeout_ms: u32) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "USBTMC over usbfs needs Linux"))
    }
    fn clear_halt(&mut self, _inp: bool) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "USBTMC over usbfs needs Linux"))
    }
}

/// The first USBTMC instrument under `root` (`SYSFS`), opened and claimed.
pub fn connect(root: &Path) -> Result<(Usbtmc<UsbFs>, Device), String> {
    let d = find(root)?;
    let fs = UsbFs::open(&d).map_err(|e| format!("{} ({}): {}", d.id, d.name, e))?;
    Ok((Usbtmc::new(fs), d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// A USBTMC device: checks every OUT transfer against the spec, keeps the messages, answers queries.
    #[derive(Default)]
    struct Mock {
        /// complete messages received (EOM)
        msgs: Vec<String>,
        part: Vec<u8>,
        tags: Vec<u8>,
        /// the answer to the next query, handed out in `max`-sized parts
        answer: Vec<u8>,
        /// bulk IN transfers waiting to be read
        queue: VecDeque<Vec<u8>>,
        /// split every IN transfer after this many bytes (a device that sends the header alone first)
        split: Option<usize>,
        halts: Vec<bool>,
        fail_out: Option<i32>,
    }

    impl Bulk for Mock {
        fn bulk_out(&mut self, t: &[u8], _: u32) -> io::Result<usize> {
            if let Some(e) = self.fail_out {
                return Err(io::Error::from_raw_os_error(e));
            }
            assert!(t.len() >= HEADER && t.len().is_multiple_of(4), "transfer of {} bytes", t.len());
            let tag = t[1];
            assert!(tag != 0 && t[2] == !tag && t[3] == 0, "{:?}", &t[..4]);
            self.tags.push(tag);
            let size = u32::from_le_bytes([t[4], t[5], t[6], t[7]]) as usize;
            match t[0] {
                DEV_DEP_MSG_OUT => {
                    assert_eq!(&t[9..12], &[0, 0, 0]);
                    assert!(t.len() - HEADER - size < 4 && t[HEADER + size..].iter().all(|&b| b == 0), "padding");
                    self.part.extend_from_slice(&t[HEADER..HEADER + size]);
                    if t[8] & 1 != 0 {
                        self.msgs.push(String::from_utf8(std::mem::take(&mut self.part)).unwrap());
                        if self.msgs.last().unwrap().ends_with("?\n") {
                            self.answer = b"UNI-T Technologies,UTG900,0001,1.0\n".to_vec();
                        }
                    }
                }
                REQUEST_DEV_DEP_MSG_IN => {
                    assert_eq!(t.len(), HEADER);
                    // nothing to say: the IN endpoint stays quiet (a timeout on the host)
                    if self.answer.is_empty() {
                        return Ok(t.len());
                    }
                    let n = size.min(self.answer.len());
                    let data: Vec<u8> = self.answer.drain(..n).collect();
                    let s = (n as u32).to_le_bytes();
                    let mut r = vec![DEV_DEP_MSG_IN, tag, !tag, 0, s[0], s[1], s[2], s[3], self.answer.is_empty() as u8, 0, 0, 0];
                    r.extend_from_slice(&data);
                    r.resize(r.len().next_multiple_of(4), 0);
                    match self.split {
                        Some(k) if r.len() > k => {
                            self.queue.push_back(r[..k].to_vec());
                            self.queue.push_back(r[k..].to_vec());
                        }
                        _ => self.queue.push_back(r),
                    }
                }
                id => panic!("MsgID {}", id),
            }
            Ok(t.len())
        }
        fn bulk_in(&mut self, buf: &mut [u8], _: u32) -> io::Result<usize> {
            let Some(r) = self.queue.pop_front() else { return Err(io::Error::from_raw_os_error(libc::ETIMEDOUT)) };
            assert!(r.len() <= buf.len());
            buf[..r.len()].copy_from_slice(&r);
            Ok(r.len())
        }
        fn clear_halt(&mut self, inp: bool) -> io::Result<()> {
            self.halts.push(inp);
            Ok(())
        }
    }

    #[test]
    fn headers_follow_the_spec() {
        // USBTMC 1.0 table 3: MsgID 1, bTag, ~bTag, 0, TransferSize LE, bmTransferAttributes bit 0 = EOM, 3 reserved
        assert_eq!(out_header(1, 7, true), [1, 1, 0xfe, 0, 7, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(out_header(0x80, 0x01020304, false), [1, 0x80, 0x7f, 0, 4, 3, 2, 1, 0, 0, 0, 0]);
        // table 4: MsgID 2, TransferSize = most bytes wanted, bit 1 = TermCharEnabled, then TermChar
        assert_eq!(request_header(5, 4096, None), [2, 5, 0xfa, 0, 0, 0x10, 0, 0, 0, 0, 0, 0]);
        assert_eq!(request_header(5, 64, Some(b'\n')), [2, 5, 0xfa, 0, 64, 0, 0, 0, 2, b'\n', 0, 0]);
        // data padded to a multiple of 4 with zeros
        let t = out_transfer(3, b"*IDN?\n", true);
        assert_eq!(t.len(), 20);
        assert_eq!(&t[HEADER..], b"*IDN?\n\0\0");
        assert_eq!(out_transfer(3, b"OUTP", true).len(), 16);
        assert_eq!(out_transfer(3, b"", true).len(), 12);
    }

    #[test]
    fn in_header_is_checked() {
        let h = [2, 9, 0xf6, 0, 3, 0, 0, 0, 1, 0, 0, 0];
        assert_eq!(parse_in_header(&h, 9), Ok(InHeader { size: 3, eom: true }));
        assert!(parse_in_header(&h, 8).unwrap_err().contains("bTag"));
        assert!(parse_in_header(&h[..8], 9).is_err());
        let mut bad = h;
        bad[0] = 1;
        assert!(parse_in_header(&bad, 9).unwrap_err().contains("MsgID"));
        bad = h;
        bad[2] = 0;
        assert!(parse_in_header(&bad, 9).is_err());
    }

    #[test]
    fn tags_run_from_1_to_255_and_skip_0() {
        let mut t = Usbtmc::new(Mock::default());
        for _ in 0..300 {
            t.command(":CHANnel1:OUTPut ON").unwrap();
        }
        let tags = &t.bus.tags;
        assert_eq!((tags[0], tags[254], tags[255], tags[299]), (1, 255, 1, 45));
        assert!(!tags.contains(&0));
        assert_eq!(t.bus.msgs.len(), 300);
    }

    #[test]
    fn query_reads_the_answer() {
        let mut t = Usbtmc::new(Mock::default());
        assert_eq!(t.query("*IDN?").unwrap(), "UNI-T Technologies,UTG900,0001,1.0");
        assert_eq!(t.bus.msgs, vec!["*IDN?\n"]);
        // the header and the data in separate bulk reads
        t.bus.split = Some(HEADER);
        assert_eq!(t.query("*IDN?").unwrap(), "UNI-T Technologies,UTG900,0001,1.0");
        t.bus.split = Some(HEADER + 5);
        assert_eq!(t.query("*IDN?").unwrap(), "UNI-T Technologies,UTG900,0001,1.0");
        // no answer: a timeout, said so
        t.bus.split = None;
        t.command("*RST").unwrap();
        assert_eq!(t.read().unwrap_err(), "no answer (timeout)");
    }

    #[test]
    fn long_messages_go_in_parts_and_answers_in_requests() {
        let mut t = Usbtmc::new(Mock::default());
        let long = "X".repeat(CHUNK * 2 + 10);
        t.write(long.as_bytes()).unwrap();
        assert_eq!(t.bus.msgs, vec![long.clone()]);
        assert_eq!(t.bus.tags.len(), 3);
        // an answer longer than one request: EOM only on the last part
        t.bus.answer = vec![b'a'; CHUNK + 100];
        assert_eq!(t.read().unwrap().len(), CHUNK + 100);
    }

    #[test]
    fn a_stall_is_cleared() {
        let mut t = Usbtmc::new(Mock { fail_out: Some(libc::EPIPE), ..Default::default() });
        assert_eq!(t.command("x").unwrap_err(), "endpoint stalled");
        assert_eq!(t.bus.halts, vec![false]);
        t.bus.fail_out = Some(libc::ENODEV);
        assert_eq!(t.command("x").unwrap_err(), "device unplugged");
    }

    #[cfg(unix)]
    fn write(p: &Path, k: &str, v: &str) {
        std::fs::create_dir_all(p).unwrap();
        std::fs::write(p.join(k), format!("{}\n", v)).unwrap();
    }

    // sysfs interface directories are named like `1-1:1.0`, which is not a valid file name on Windows
    #[cfg(unix)]
    #[test]
    fn find_reads_sysfs() {
        let root = std::env::temp_dir().join(format!("oos-usbtmc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert!(find(&root).unwrap_err().contains("no USB device"));
        // a root hub and a memory stick: no USBTMC
        write(&root.join("usb1"), "bDeviceClass", "09");
        write(&root.join("usb1"), "idVendor", "1d6b");
        write(&root.join("usb1"), "idProduct", "0002");
        let stick = root.join("1-1");
        for (k, v) in [("bDeviceClass", "00"), ("idVendor", "0781"), ("idProduct", "5567"), ("product", "Cruzer Blade"), ("busnum", "1"), ("devnum", "2")] {
            write(&stick, k, v);
        }
        for (k, v) in [("bInterfaceClass", "08"), ("bInterfaceSubClass", "06"), ("bInterfaceNumber", "00")] {
            write(&root.join("1-1:1.0"), k, v);
        }
        let e = find(&root).unwrap_err();
        assert!(e.contains("not USBTMC: 0781:5567 Cruzer Blade") && !e.contains("1d6b"), "{}", e);
        // the generator on bus 3, device 4: interface 0 with an interrupt and two bulk endpoints
        let g = root.join("3-1");
        for (k, v) in [("bDeviceClass", "00"), ("idVendor", "6656"), ("idProduct", "0834"), ("manufacturer", "UNI-T"), ("product", "UTG900"), ("busnum", "3"), ("devnum", "4")] {
            write(&g, k, v);
        }
        let i = root.join("3-1:1.0");
        for (k, v) in [("bInterfaceClass", "fe"), ("bInterfaceSubClass", "03"), ("bInterfaceNumber", "00")] {
            write(&i, k, v);
        }
        for (ep, ty, a) in [("ep_83", "Interrupt", "83"), ("ep_02", "Bulk", "02"), ("ep_81", "Bulk", "81")] {
            write(&i.join(ep), "type", ty);
            write(&i.join(ep), "bEndpointAddress", a);
        }
        let d = find(&root).unwrap();
        assert_eq!(d, Device { bus: 3, dev: 4, ifno: 0, ep_out: 0x02, ep_in: 0x81, id: "6656:0834".into(), name: "UNI-T UTG900".into() });
        assert_eq!(d.node(), PathBuf::from("/dev/bus/usb/003/004"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn usbfs_requests() {
        // the numbers of <linux/usbdevice_fs.h> on a 64-bit kernel
        if std::mem::size_of::<usize>() == 8 {
            assert_eq!(USBDEVFS_BULK, 0xc018_5502);
            assert_eq!(USBDEVFS_IOCTL, 0xc010_5512);
        }
        assert_eq!(USBDEVFS_CLAIMINTERFACE, 0x8004_550f);
        assert_eq!(USBDEVFS_RELEASEINTERFACE, 0x8004_5510);
        assert_eq!(USBDEVFS_CLEAR_HALT, 0x8004_5515);
        assert_eq!(USBDEVFS_DISCONNECT, 0x5516);
    }
}
