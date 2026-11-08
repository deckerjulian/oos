// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Linux GPIO character device (v1 ABI), lines found by their device-tree names (e.g. `spi_afe_sclk`). Only libc.

use std::ffi::CStr;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};

#[repr(C)]
struct ChipInfo {
    name: [u8; 32],
    label: [u8; 32],
    lines: u32,
}

#[repr(C)]
struct LineInfo {
    line_offset: u32,
    flags: u32,
    name: [u8; 32],
    consumer: [u8; 32],
}

#[repr(C)]
struct HandleRequest {
    lineoffsets: [u32; 64],
    flags: u32,
    default_values: [u8; 64],
    consumer_label: [u8; 32],
    lines: u32,
    fd: i32,
}

#[repr(C)]
struct HandleData {
    values: [u8; 64],
}

const fn iowr(nr: u32, size: usize) -> libc::c_ulong {
    ((3u32 << 30) | ((size as u32) << 16) | (0xb4 << 8) | nr) as libc::c_ulong
}
const fn ior(nr: u32, size: usize) -> libc::c_ulong {
    ((2u32 << 30) | ((size as u32) << 16) | (0xb4 << 8) | nr) as libc::c_ulong
}
const GET_CHIPINFO: libc::c_ulong = ior(0x01, std::mem::size_of::<ChipInfo>());
const GET_LINEINFO: libc::c_ulong = iowr(0x02, std::mem::size_of::<LineInfo>());
const GET_LINEHANDLE: libc::c_ulong = iowr(0x03, std::mem::size_of::<HandleRequest>());
const GET_VALUES: libc::c_ulong = iowr(0x08, std::mem::size_of::<HandleData>());
const SET_VALUES: libc::c_ulong = iowr(0x09, std::mem::size_of::<HandleData>());
const REQUEST_INPUT: u32 = 1 << 0;
const REQUEST_OUTPUT: u32 = 1 << 1;

/// One requested GPIO line.
pub struct Line {
    fd: OwnedFd,
}

fn cstr(b: &[u8]) -> &str {
    CStr::from_bytes_until_nul(b).ok().and_then(|c| c.to_str().ok()).unwrap_or("")
}

fn err(what: &str) -> std::io::Error {
    let e = std::io::Error::last_os_error();
    std::io::Error::new(e.kind(), format!("{}: {}", what, e))
}

/// Finds line `name` on /dev/gpiochip0..9: (chip fd, offset).
fn find(name: &str) -> std::io::Result<(OwnedFd, u32)> {
    for c in 0..10 {
        let path = std::ffi::CString::new(format!("/dev/gpiochip{}", c)).unwrap();
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR) };
        if fd < 0 {
            continue;
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let mut ci: ChipInfo = unsafe { std::mem::zeroed() };
        if unsafe { libc::ioctl(fd.as_raw_fd(), GET_CHIPINFO as _, &mut ci) } != 0 {
            continue;
        }
        for l in 0..ci.lines {
            let mut li: LineInfo = unsafe { std::mem::zeroed() };
            li.line_offset = l;
            if unsafe { libc::ioctl(fd.as_raw_fd(), GET_LINEINFO as _, &mut li) } == 0 && cstr(&li.name) == name {
                return Ok((fd, l));
            }
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("GPIO line '{}' not found", name)))
}

impl Line {
    fn request(name: &str, flags: u32, value: u8) -> std::io::Result<Line> {
        let (chip, off) = find(name)?;
        let mut rq: HandleRequest = unsafe { std::mem::zeroed() };
        rq.lineoffsets[0] = off;
        rq.flags = flags;
        rq.default_values[0] = value;
        rq.lines = 1;
        rq.consumer_label[..6].copy_from_slice(b"oos-hw");
        if unsafe { libc::ioctl(chip.as_raw_fd(), GET_LINEHANDLE as _, &mut rq) } < 0 {
            return Err(err(name));
        }
        Ok(Line { fd: unsafe { OwnedFd::from_raw_fd(rq.fd) } })
    }
    /// Output line with initial value `value`.
    pub fn output(name: &str, value: bool) -> std::io::Result<Line> {
        Line::request(name, REQUEST_OUTPUT, value as u8)
    }
    pub fn input(name: &str) -> std::io::Result<Line> {
        Line::request(name, REQUEST_INPUT, 0)
    }
    pub fn set(&self, v: bool) {
        let mut d = HandleData { values: [0; 64] };
        d.values[0] = v as u8;
        unsafe { libc::ioctl(self.fd.as_raw_fd(), SET_VALUES as _, &mut d) };
    }
    pub fn get(&self) -> bool {
        let mut d = HandleData { values: [0; 64] };
        unsafe { libc::ioctl(self.fd.as_raw_fd(), GET_VALUES as _, &mut d) };
        d.values[0] != 0
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ioctl_numbers_match_linux_headers() {
        // linux/gpio.h v1: GPIO_GET_CHIPINFO_IOCTL 0x8044b401, GPIO_GET_LINEINFO_IOCTL 0xc048b402,
        // GPIO_GET_LINEHANDLE_IOCTL 0xc16cb403, GPIOHANDLE_GET/SET_LINE_VALUES_IOCTL 0xc040b408 / 0xc040b409
        assert_eq!(super::GET_CHIPINFO as u32, 0x8044_b401);
        assert_eq!(super::GET_LINEINFO as u32, 0xc048_b402);
        assert_eq!(super::GET_LINEHANDLE as u32, 0xc16c_b403);
        assert_eq!(super::GET_VALUES as u32, 0xc040_b408);
        assert_eq!(super::SET_VALUES as u32, 0xc040_b409);
    }
}
