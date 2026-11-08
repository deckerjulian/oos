// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Trial start by kexec (docs/update.md, "Trial start by kexec").
//!
//! OOS has no loader of its own, so it cannot count starts before rcS: a new kernel or initramfs that hangs before
//! `oos-oneshot` would start again at every power-on, and only taking the card out would help. So an update first
//! starts the new image's kernel and initramfs from the running OOS, by kexec, with `recovery` left as it is (it
//! still holds the running build, so the boot selection can stay). The package rides along in the new initramfs
//! (`/oos-kexec.oosupd`), with the key that verified it (`/oos-kexec.key`: a new build may trust other keys) and the
//! device tree the scope started with (`/oos-kexec.dt`). Only that start, once it reached rcS and found the card and
//! /oos, installs it (`oos-update boot`). A hang there: power-cycle; `recovery` still holds the running build.
//!
//! What it does not try: the device tree. `kexec_file_load` gives the new kernel the tree of the running one, so an
//! image whose `second` (the RSCE container with the tree) differs from the tree the scope started with
//! (`BOOTED_DT`) is refused for this way. And the start runs on warm hardware: a kernel that only fails from a cold
//! start passes.
//!
//! Only an image that knows how to finish such a start can take it: its package says so with the signed line
//! `trial kexec` (`LINE`), which `oos-update pack` writes. Older packages install the direct way.

use super::card::check_image;

/// The signed package line of an image that finishes its own install after a kexec start.
pub const LINE: (&str, &str) = ("trial", "kexec");
/// Where the package is in the initramfs of the kexec start.
pub const PACKAGE: &str = "/oos-kexec.oosupd";
/// The key that verified the package, one hex line, in the same initramfs.
pub const KEY_FILE: &str = "/oos-kexec.key";
/// The device tree section the scope started with, in the same initramfs.
pub const DT_FILE: &str = "/oos-kexec.dt";
/// The device tree section of the image the loader started (`oos-update boot` saves it; a kexec start carries it on).
pub const BOOTED_DT: &str = "/tmp/oos-booted.dt";
/// Kernel command line word of a kexec start: `oos.kexec=install[,downgrade][,force]`.
pub const KEY: &str = "oos.kexec=";

/// Does this package's image finish a kexec start?
pub fn announced(m: &super::pkg::Manifest) -> bool {
    m.extra.iter().any(|(k, v)| k == LINE.0 && v == LINE.1)
}

/// The sections of a checked image: kernel, ramdisk, second.
pub fn parts(img: &[u8]) -> Result<(&[u8], &[u8], &[u8]), String> {
    let info = check_image(img)?;
    let le = |at: usize| u32::from_le_bytes([img[at], img[at + 1], img[at + 2], img[at + 3]]) as usize;
    let (ks, rs, ss) = (le(8), le(16), le(24));
    let page = 2048;
    let k0 = page;
    let r0 = k0 + ks.div_ceil(page) * page;
    let s0 = r0 + rs.div_ceil(page) * page;
    debug_assert!(s0 + ss <= info.len);
    Ok((&img[k0..k0 + ks], &img[r0..r0 + rs], &img[s0..s0 + ss]))
}

/// Where the device tree section of an image is, from its header page alone: (offset, length). For the image the loader
/// just started, which it has checked.
pub fn tree_at(header: &[u8]) -> Option<(usize, usize)> {
    super::card::image_header(header).ok()?;
    let le = |at: usize| u32::from_le_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]]) as usize;
    let page = 2048;
    let s0 = page + le(8).div_ceil(page) * page + le(16).div_ceil(page) * page;
    Some((s0, le(24)))
}

/// A newc cpio archive of regular files (`name`, mode, data), with its trailer.
pub fn newc(files: &[(&str, u32, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let put = |out: &mut Vec<u8>, ino: u32, name: &str, mode: u32, data: &[u8]| {
        let name0 = name.len() + 1;
        let fields = [ino, mode, 0, 0, 1, 0, data.len() as u32, 0, 0, 0, 0, name0 as u32, 0];
        out.extend_from_slice(b"070701");
        for f in fields {
            out.extend_from_slice(format!("{:08x}", f).as_bytes());
        }
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    };
    for (i, (name, mode, data)) in files.iter().enumerate() {
        put(&mut out, 1000 + i as u32, name, *mode, data);
    }
    put(&mut out, 0, "TRAILER!!!", 0, b"");
    out
}

/// The initramfs of the kexec start: the image's own ramdisk, then an uncompressed archive with the package, its key and
/// the running device tree. The kernel unpacks archives one after the other; the second must start at a multiple of 4
/// (the ramdisk length already is one).
pub fn initrd(ramdisk: &[u8], package: &[u8], key: &[u8; 32], dt: &[u8]) -> Vec<u8> {
    let mut b = ramdisk.to_vec();
    b.resize(ramdisk.len().div_ceil(4) * 4, 0);
    let k = format!("{}\n", super::hash::hex(key));
    let n = |p: &'static str| p.trim_start_matches('/');
    b.extend_from_slice(&newc(&[(n(PACKAGE), 0o100600, package), (n(KEY_FILE), 0o100600, k.as_bytes()), (n(DT_FILE), 0o100600, dt)]));
    b
}

/// What a kexec start is to do with the package.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Flags {
    pub downgrade: bool,
    pub force: bool,
}

/// The command line of the kexec start: the running one (the loader built it from the same device tree) with the word
/// for the install.
pub fn cmdline(running: &str, f: Flags) -> String {
    // initramfs_async=0: the package is unpacked before any driver probes, not alongside them
    let mut w: Vec<&str> =
        running.split_whitespace().filter(|w| !w.starts_with(KEY) && !w.starts_with("initramfs_async=")).collect();
    w.push("initramfs_async=0");
    let mut v = String::from(KEY);
    v.push_str("install");
    if f.downgrade {
        v.push_str(",downgrade");
    }
    if f.force {
        v.push_str(",force");
    }
    w.push(&v);
    w.join(" ")
}

/// The install word of the running kernel's command line, if this is a kexec start.
pub fn flags(cmdline: &str) -> Option<Flags> {
    let v = cmdline.split_whitespace().find_map(|w| w.strip_prefix(KEY))?;
    let mut f = Flags::default();
    let mut install = false;
    for p in v.split(',') {
        match p {
            "install" => install = true,
            "downgrade" => f.downgrade = true,
            "force" => f.force = true,
            _ => {}
        }
    }
    install.then_some(f)
}

/// The running kernel's command line.
pub fn running_cmdline() -> String {
    std::fs::read_to_string("/proc/cmdline").unwrap_or_default().trim().to_string()
}

#[cfg(target_os = "linux")]
mod sys {
    // the generic syscall table (arm64): the musl target of the libc crate does not name it
    const SYS_KEXEC_FILE_LOAD: libc::c_long = 294;
    const KEXEC_FILE_UNLOAD: libc::c_ulong = 1;

    fn memfd(name: &str, data: &[u8]) -> Result<std::fs::File, String> {
        use std::io::Write;
        use std::os::fd::FromRawFd;
        let n = std::ffi::CString::new(name).unwrap();
        let fd = unsafe { libc::memfd_create(n.as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return Err(format!("memfd_create: {}", std::io::Error::last_os_error()));
        }
        let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
        f.write_all(data).map_err(|e| format!("memfd: {}", e))?;
        Ok(f)
    }

    /// Load kernel and initramfs for the next `start` (kexec_file_load; the kernel keeps its own device tree).
    pub fn load(kernel: &[u8], initrd: &[u8], cmdline: &str) -> Result<(), String> {
        use std::os::fd::AsRawFd;
        let k = memfd("oos-kexec-kernel", kernel)?;
        let i = memfd("oos-kexec-initrd", initrd)?;
        let c = std::ffi::CString::new(cmdline).map_err(|_| "command line with a NUL")?;
        let len = c.as_bytes_with_nul().len();
        let r = unsafe { libc::syscall(SYS_KEXEC_FILE_LOAD, k.as_raw_fd(), i.as_raw_fd(), len as libc::c_ulong, c.as_ptr(), 0 as libc::c_ulong) };
        if r != 0 {
            return Err(format!("kexec_file_load: {}", std::io::Error::last_os_error()));
        }
        Ok(())
    }

    /// Forget a loaded kernel.
    pub fn unload() -> Result<(), String> {
        let r = unsafe { libc::syscall(SYS_KEXEC_FILE_LOAD, -1, -1, 0 as libc::c_ulong, std::ptr::null::<u8>(), KEXEC_FILE_UNLOAD) };
        if r != 0 {
            return Err(format!("kexec unload: {}", std::io::Error::last_os_error()));
        }
        Ok(())
    }

    pub fn loaded() -> bool {
        std::fs::read_to_string("/sys/kernel/kexec_loaded").is_ok_and(|s| s.trim() == "1")
    }

    /// Start the loaded kernel. Returns only on failure. The caller has synced and unmounted (the kernel does not).
    pub fn start() -> String {
        unsafe { libc::sync() };
        let r = unsafe { libc::reboot(libc::LINUX_REBOOT_CMD_KEXEC) };
        format!("reboot(KEXEC) returned {}: {}", r, std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
pub use sys::{load, loaded, start, unload};

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads a newc archive back: (name, mode, data).
    fn read_newc(mut b: &[u8]) -> Vec<(String, u32, Vec<u8>)> {
        let mut out = Vec::new();
        loop {
            assert_eq!(&b[..6], b"070701");
            let f = |i: usize| u32::from_str_radix(std::str::from_utf8(&b[6 + i * 8..14 + i * 8]).unwrap(), 16).unwrap();
            let (mode, size, nlen) = (f(1), f(6) as usize, f(11) as usize);
            let name = String::from_utf8(b[110..110 + nlen - 1].to_vec()).unwrap();
            // every header starts at a multiple of 4, so aligning within `b` is aligning in the archive
            let at = (110 + nlen).div_ceil(4) * 4;
            let data = b[at..at + size].to_vec();
            b = &b[(at + size).div_ceil(4) * 4..];
            if name == "TRAILER!!!" {
                assert!(b.is_empty());
                return out;
            }
            out.push((name, mode, data));
        }
    }

    #[test]
    fn archive_reads_back() {
        let a = newc(&[("oos-kexec.oosupd", 0o100600, b"package bytes"), ("x", 0o100644, b"")]);
        assert_eq!(a.len() % 4, 0);
        let r = read_newc(&a);
        assert_eq!(r, vec![("oos-kexec.oosupd".into(), 0o100600, b"package bytes".to_vec()), ("x".into(), 0o100644, vec![])]);
    }

    #[test]
    fn initrd_appends_at_a_multiple_of_four() {
        let rd = vec![0x1f, 0x8b, 8, 0, 1, 2, 3, 4];
        let i = initrd(&rd, b"pkg", &[0xab; 32], b"tree");
        assert_eq!(&i[..8], &rd[..]);
        assert_eq!(&i[8..14], b"070701");
        let a = read_newc(&i[8..]);
        assert_eq!(a[0].2, b"pkg");
        assert_eq!(a[1].2, format!("{}\n", "ab".repeat(32)).as_bytes());
        assert_eq!((a[2].0.as_str(), a[2].2.as_slice()), ("oos-kexec.dt", &b"tree"[..]));
    }

    #[test]
    fn tree_from_the_header() {
        let mut h = vec![0u8; 2048];
        h[..8].copy_from_slice(b"ANDROID!");
        for (at, v) in [(8, 5000u32), (16, 2049), (24, 300), (36, 2048)] {
            h[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        assert_eq!(tree_at(&h), Some((2048 + 3 * 2048 + 2 * 2048, 300)));
        assert_eq!(tree_at(&[0u8; 2048]), None);
    }

    #[test]
    fn command_line_round_trip() {
        let run = "storagemedia=sd console=ttyFIQ0 oos.kexec=install,force initramfs_async=1 root=/dev/ram";
        let c = cmdline(run, Flags { downgrade: true, force: false });
        assert_eq!(c, "storagemedia=sd console=ttyFIQ0 root=/dev/ram initramfs_async=0 oos.kexec=install,downgrade");
        assert_eq!(flags(&c), Some(Flags { downgrade: true, force: false }));
        assert_eq!(flags("console=ttyFIQ0"), None);
        assert_eq!(flags("oos.kexec=other"), None);
    }
}
