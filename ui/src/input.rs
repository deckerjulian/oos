// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Linux inputs: touch screen (evdev, multi-touch protocol B), front-panel MCU (UART0, frames FA cmd data crc7 AF),
//! control FIFO /tmp/oos-ui.ctl (text commands, see cmd.rs), mice and keyboards (evdev, USB or Bluetooth: hid.rs).

use crate::ui::{Ev, Phase};
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

pub fn install_signal_handlers() {
    unsafe {
        let h = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGINT, h);
        libc::signal(libc::SIGTERM, h);
    }
}

pub fn stop_requested() -> bool {
    STOP.load(Ordering::SeqCst)
}

pub enum In {
    Ev(Ev),
    Cmd(String),
    /// a mouse or keyboard (hid.rs)
    Hid(crate::hid::Event),
}

/// How often the list of mice and keyboards is read again: a Bluetooth device comes and goes at any time.
const HID_SCAN_S: u64 = 2;

/// EVIOCGRAB = _IOW('E', 0x90, int), in the generic ioctl numbering that aarch64 (the scope) and x86_64 use.
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
const EVIOCGRAB: libc::c_ulong =
    (1 << 30) | ((std::mem::size_of::<libc::c_int>() as libc::c_ulong) << 16) | ((b'E' as libc::c_ulong) << 8) | 0x90;

/// Takes a mouse or keyboard for oos-ui alone. Without the grab the kernel's console keyboard gets its keys as well:
/// they would type into the root shell on tty1 behind the UI (a WLAN password into its history, Ctrl+Alt+Del a
/// reboot). The grab goes with the file descriptor.
fn grab(fd: i32) -> std::io::Result<()> {
    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    {
        if unsafe { libc::ioctl(fd, EVIOCGRAB as _, 1 as libc::c_int) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = fd;
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "EVIOCGRAB unknown on this architecture"))
    }
}

/// An open mouse or keyboard.
struct HidDev {
    fd: i32,
    event: String,
    /// its buttons and modifiers that are down, released when it goes
    held: crate::hid::Held,
}

const CTL: &str = "/tmp/oos-ui.ctl";

#[derive(Default, Clone, Copy)]
struct Slot {
    id: i32,
    x: i32,
    y: i32,
    down: bool,
    moved: bool,
    began: bool,
    ended: bool,
}

struct Touch {
    fd: i32,
    slot: i32,
    s: [Slot; 2],
    swap: bool,
    inv_x: bool,
    inv_y: bool,
    max_x: i32,
    max_y: i32,
}

pub struct Inputs {
    touch: Option<Touch>,
    panel: i32,
    panel_buf: Vec<u8>,
    ctl: i32,
    ctl_buf: Vec<u8>,
    hid: Vec<HidDev>,
    hid_scan: Option<std::time::Instant>,
    /// mice and keyboards that could not be grabbed, left out until they come again under another node
    refused: Vec<String>,
}

/// A read of a non-blocking descriptor into `buf`: the bytes read, or -1 with errno (EAGAIN when nothing waits).
fn read_fd(fd: i32, buf: &mut [u8]) -> isize {
    unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) }
}

fn close_fd(fd: i32) {
    unsafe { libc::close(fd) };
}

fn open_nb(path: &str, flags: i32) -> i32 {
    let c = CString::new(path).unwrap();
    unsafe { libc::open(c.as_ptr(), flags | libc::O_NONBLOCK | libc::O_CLOEXEC) }
}

/// Finds the touch screen's event device in /proc/bus/input/devices (FocalTech / edt-ft5x06).
fn find_touch() -> Option<String> {
    let s = std::fs::read_to_string("/proc/bus/input/devices").ok()?;
    for block in s.split("\n\n") {
        let lower = block.to_lowercase();
        if lower.contains("focaltech") || lower.contains("ft5") || lower.contains("touch") {
            for line in block.lines() {
                if let Some(h) = line.strip_prefix("H: Handlers=") {
                    if let Some(ev) = h.split_whitespace().find(|w| w.starts_with("event")) {
                        return Some(format!("/dev/input/{}", ev));
                    }
                }
            }
        }
    }
    None
}

fn open_panel(path: &str, baud: u32) -> i32 {
    let fd = open_nb(path, libc::O_RDWR | libc::O_NOCTTY);
    let speed = match baud {
        1_500_000 => libc::B1500000,
        921_600 => libc::B921600,
        460_800 => libc::B460800,
        230_400 => libc::B230400,
        _ => libc::B115200,
    };
    if fd < 0 {
        return fd;
    }
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(fd, &mut t);
        libc::cfmakeraw(&mut t);
        t.c_cflag &= !(libc::PARENB | libc::CSTOPB | libc::CSIZE | libc::CRTSCTS);
        t.c_cflag |= libc::CS8 | libc::CREAD | libc::CLOCAL;
        libc::cfsetispeed(&mut t, speed);
        libc::cfsetospeed(&mut t, speed);
        libc::tcsetattr(fd, libc::TCSANOW, &t);
        libc::tcflush(fd, libc::TCIFLUSH);
    }
    fd
}

impl Inputs {
    pub fn open(args: &[String]) -> Inputs {
        let has = |f: &str| args.iter().any(|a| a == f);
        let arg = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1).cloned());
        let touch = arg("--touch").or_else(find_touch).and_then(|p| {
            let fd = open_nb(&p, libc::O_RDONLY);
            if fd < 0 {
                eprintln!("oos-ui: touch {}: {}", p, std::io::Error::last_os_error());
                return None;
            }
            eprintln!("oos-ui: touch {}", p);
            let absmax = |code: u32| -> i32 {
                // EVIOCGABS(code) = _IOR('E', 0x40 + code, struct input_absinfo[6 x s32])
                let mut info = [0i32; 6];
                let req = ((2u32 << 30) | (24u32 << 16) | ((b'E' as u32) << 8) | (0x40 + code)) as libc::c_ulong;
                let r = unsafe { libc::ioctl(fd, req as _, info.as_mut_ptr()) };
                if r == 0 {
                    info[2]
                } else {
                    0
                }
            };
            let max_x = absmax(0x35);
            let max_y = absmax(0x36);
            eprintln!("oos-ui: touch range {} x {}", max_x, max_y);
            let idle = Slot { id: -1, ..Default::default() };
            Some(Touch {
                fd,
                slot: 0,
                s: [idle, idle],
                swap: has("--touch-swap"),
                inv_x: has("--touch-invx"),
                inv_y: has("--touch-invy"),
                max_x,
                max_y,
            })
        });
        // the board's panel port (frontpanel.md on the DHO800/DHO900); --panel overrides the device
        let (port, baud) = {
            use crate::board::Board;
            crate::board::board().panel_port()
        };
        let panel =
            if has("--no-panel") { -1 } else { open_panel(&arg("--panel").unwrap_or_else(|| port.into()), baud) };
        if panel < 0 {
            eprintln!("oos-ui: no front panel");
        }
        let c = CString::new(CTL).unwrap();
        unsafe {
            libc::unlink(c.as_ptr());
            libc::mkfifo(c.as_ptr(), 0o666);
        }
        let ctl = open_nb(CTL, libc::O_RDWR);
        Inputs {
            touch,
            panel,
            panel_buf: Vec::new(),
            ctl,
            ctl_buf: Vec::new(),
            hid: Vec::new(),
            hid_scan: None,
            refused: Vec::new(),
        }
    }

    /// Sets the front-panel LEDs. `mask` is in panel LED order (see `Ui::leds`); it is translated to the MCU's
    /// hardware bits and sent as three frames cmd 1/2/3 with the bitmasks of hardware LEDs 0-7, 8-15, 16-18.
    pub fn set_leds(&mut self, mask: u32) {
        if self.panel < 0 {
            return;
        }
        use crate::board::Board;
        for f in crate::board::board().panel_leds(mask) {
            unsafe { libc::write(self.panel, f.as_ptr() as *const libc::c_void, f.len()) };
        }
    }

    /// Sends one MCU frame (`FA cmd data crc AF`).
    pub fn send(&mut self, f: &[u8]) {
        if self.panel >= 0 {
            unsafe { libc::write(self.panel, f.as_ptr() as *const libc::c_void, f.len()) };
        }
    }

    /// Waits up to `timeout_ms` for input and returns everything that arrived.
    pub fn poll(&mut self, timeout_ms: i32) -> Vec<In> {
        let mut fds = Vec::new();
        if let Some(t) = &self.touch {
            fds.push(libc::pollfd { fd: t.fd, events: libc::POLLIN, revents: 0 });
        }
        if self.panel >= 0 {
            fds.push(libc::pollfd { fd: self.panel, events: libc::POLLIN, revents: 0 });
        }
        if self.ctl >= 0 {
            fds.push(libc::pollfd { fd: self.ctl, events: libc::POLLIN, revents: 0 });
        }
        self.hid_rescan();
        for d in &self.hid {
            fds.push(libc::pollfd { fd: d.fd, events: libc::POLLIN, revents: 0 });
        }
        unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
        let mut out = Vec::new();
        self.read_touch(&mut out);
        self.read_panel(&mut out);
        self.read_ctl(&mut out);
        self.read_hid(&mut out);
        out
    }

    /// Opens the mice and keyboards that came, every `HID_SCAN_S`; one that went is closed by `read_hid`. What a node
    /// is comes from sysfs, which the kernel writes (`hid::classify`). A device that cannot be grabbed is left out:
    /// its keys would reach the console shell as well.
    fn hid_rescan(&mut self) {
        if self.hid_scan.is_some_and(|t| t.elapsed().as_secs() < HID_SCAN_S) {
            return;
        }
        self.hid_scan = Some(std::time::Instant::now());
        let Ok(dir) = std::fs::read_dir("/sys/class/input") else { return };
        for e in dir.flatten() {
            let event = e.file_name().to_string_lossy().into_owned();
            if !crate::hid::is_event(&event)
                || self.hid.iter().any(|h| h.event == event)
                || self.refused.contains(&event)
            {
                continue;
            }
            let read = |f: &str| std::fs::read_to_string(e.path().join("device").join(f)).unwrap_or_default();
            let caps = |c: &str| read(&format!("capabilities/{c}"));
            let Some(d) = crate::hid::classify(&event, &read("name"), &caps("ev"), &caps("rel"), &caps("key")) else {
                continue;
            };
            let fd = open_nb(&format!("/dev/input/{}", d.event), libc::O_RDONLY);
            if fd < 0 {
                continue;
            }
            let what = match (d.mouse, d.keyboard) {
                (true, true) => "mouse and keyboard",
                (true, false) => "mouse",
                _ => "keyboard",
            };
            if let Err(err) = grab(fd) {
                eprintln!("oos-ui: {} {} ({}) not taken: not grabbed ({})", what, d.event, d.name, err);
                close_fd(fd);
                self.refused.push(d.event);
                continue;
            }
            eprintln!("oos-ui: {} {} ({})", what, d.event, d.name);
            self.hid.push(HidDev { fd, event: d.event, held: crate::hid::Held::default() });
        }
    }

    fn read_hid(&mut self, out: &mut Vec<In>) {
        let mut buf = [0u8; 24 * 64];
        let mut gone = Vec::new();
        self.hid.retain_mut(|d| loop {
            let n = read_fd(d.fd, &mut buf);
            if n < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENODEV) || n == 0 {
                // unplugged, or the Bluetooth link went: its node is gone, and what it held is let go
                eprintln!("oos-ui: input {} gone", d.event);
                close_fd(d.fd);
                gone.push(std::mem::take(&mut d.held));
                break false;
            }
            if n < 0 {
                break true;
            }
            for ev in buf[..n as usize].as_chunks::<24>().0 {
                let ty = u16::from_ne_bytes([ev[16], ev[17]]);
                let code = u16::from_ne_bytes([ev[18], ev[19]]);
                let value = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                if let Some(e) = crate::hid::decode(ty, code, value) {
                    d.held.track(&e);
                    out.push(In::Hid(e));
                }
            }
        });
        // what a device that went held is let go, unless a device still there holds it too
        let others: Vec<&crate::hid::Held> = self.hid.iter().map(|d| &d.held).collect();
        for h in gone {
            out.extend(h.release(&others).into_iter().map(In::Hid));
        }
    }

    fn read_touch(&mut self, out: &mut Vec<In>) {
        let Some(t) = &mut self.touch else { return };
        let mut buf = [0u8; 24 * 64];
        loop {
            let n = read_fd(t.fd, &mut buf);
            if n <= 0 {
                break;
            }
            for ev in buf[..n as usize].as_chunks::<24>().0 {
                let typ = u16::from_ne_bytes([ev[16], ev[17]]);
                let code = u16::from_ne_bytes([ev[18], ev[19]]);
                let val = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                let si = t.slot;
                match (typ, code) {
                    (3, 0x2f) => t.slot = val,
                    (3, 0x39) if (0..2).contains(&si) => {
                        let sl = &mut t.s[si as usize];
                        if val >= 0 && sl.id < 0 {
                            sl.began = true;
                        } else if val < 0 && sl.id >= 0 {
                            sl.ended = true;
                        }
                        sl.id = val;
                    }
                    (3, 0x35) if (0..2).contains(&si) => {
                        t.s[si as usize].x = val;
                        t.s[si as usize].moved = true;
                    }
                    (3, 0x36) if (0..2).contains(&si) => {
                        t.s[si as usize].y = val;
                        t.s[si as usize].moved = true;
                    }
                    (0, 0) => {
                        // SYN_REPORT: one event per changed slot
                        for k in 0..2 {
                            let sl = t.s[k];
                            let (mut x, mut y) = (sl.x, sl.y);
                            if t.swap {
                                std::mem::swap(&mut x, &mut y);
                            }
                            let (mx, my) = if t.swap { (t.max_y, t.max_x) } else { (t.max_x, t.max_y) };
                            if mx > 0 && mx != 1023 {
                                x = x * 1023 / mx;
                            }
                            if my > 0 && my != 599 {
                                y = y * 599 / my;
                            }
                            if t.inv_x {
                                x = 1023 - x;
                            }
                            if t.inv_y {
                                y = 599 - y;
                            }
                            let id = k as i32;
                            if sl.began {
                                out.push(In::Ev(Ev::Touch { id, phase: Phase::Down, x, y }));
                                t.s[k].down = true;
                            } else if sl.down && sl.moved && !sl.ended {
                                out.push(In::Ev(Ev::Touch { id, phase: Phase::Move, x, y }));
                            }
                            if sl.ended {
                                out.push(In::Ev(Ev::Touch { id, phase: Phase::Up, x, y }));
                                t.s[k].down = false;
                            }
                            t.s[k].began = false;
                            t.s[k].ended = false;
                            t.s[k].moved = false;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn read_panel(&mut self, out: &mut Vec<In>) {
        if self.panel < 0 {
            return;
        }
        let mut buf = [0u8; 256];
        loop {
            let n = read_fd(self.panel, &mut buf);
            if n <= 0 {
                break;
            }
            self.panel_buf.extend_from_slice(&buf[..n as usize]);
        }
        use crate::board::Board;
        for k in crate::board::board().panel_keys(&mut self.panel_buf) {
            out.push(In::Ev(Ev::Key(k)));
        }
    }

    fn read_ctl(&mut self, out: &mut Vec<In>) {
        if self.ctl < 0 {
            return;
        }
        let mut buf = [0u8; 1024];
        loop {
            let n = read_fd(self.ctl, &mut buf);
            if n <= 0 {
                break;
            }
            self.ctl_buf.extend_from_slice(&buf[..n as usize]);
        }
        while let Some(p) = self.ctl_buf.iter().position(|&c| c == b'\n') {
            let line: Vec<u8> = self.ctl_buf.drain(..=p).collect();
            out.push(In::Cmd(String::from_utf8_lossy(&line).trim().to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    fn the_grab_has_the_number_of_linux_input_h() {
        assert_eq!(super::EVIOCGRAB, 0x4004_4590);
    }
}
