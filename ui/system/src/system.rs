// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Safe wrappers for the OS calls of oos-ui that need `unsafe`: thread placement (`sched_setaffinity`, and
//! `SCHED_IDLE` with a nice fallback for the threads of the web remote), file-system statistics (`statvfs`) and setting
//! the clock (`clock_settime`). Each wrapper holds its `unsafe` once, so its callers stay safe; each makes the same call
//! with the same arguments the callers made before and leaves the choice to ignore, log or pass on an error to them. A
//! new OS call goes here as a function of its own, behind the `cfg` its callers need, with a test where the host can
//! run it.

use std::io;

/// The calling thread, and the threads and processes it starts after this, onto `cores`; nothing when it is empty.
#[cfg(target_os = "linux")]
pub fn pin_current_thread(cores: &[usize]) -> io::Result<()> {
    pin_thread(0, cores)
}

/// Thread `tid` (0: the calling thread) onto `cores`; nothing when it is empty.
#[cfg(target_os = "linux")]
pub fn pin_thread(tid: libc::pid_t, cores: &[usize]) -> io::Result<()> {
    if cores.is_empty() {
        return Ok(());
    }
    // SAFETY: zero is a valid empty CPU set, CPU_SET only writes into it (an index beyond it panics on the slice), and
    // the kernel reads the local set during the call with its exact size.
    let rc = unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        for &c in cores {
            libc::CPU_SET(c, &mut set);
        }
        libc::sched_setaffinity(tid, std::mem::size_of::<libc::cpu_set_t>(), &set)
    };
    status(rc)
}

/// The calling thread to scheduling class `SCHED_IDLE`, or to nice value `nice` where that fails; returns the thread's
/// id (for `pin_thread`).
#[cfg(target_os = "linux")]
pub fn idle_current_thread(nice: i32) -> libc::pid_t {
    // SAFETY: gettid takes no pointer; zero fills every field of sched_param (musl's struct has more fields), and
    // priority 0 is the only one SCHED_IDLE takes; the kernel reads the local parameter during the call.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::pid_t;
        let p: libc::sched_param = std::mem::zeroed();
        // the system call itself: musl's sched_setscheduler() is a stub that always fails
        if libc::syscall(libc::SYS_sched_setscheduler, tid, libc::SCHED_IDLE, &p as *const libc::sched_param) != 0 {
            libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, nice);
        }
        tid
    }
}

/// (available, total) bytes of the file system holding `path`.
pub fn fs_stats(path: &str) -> io::Result<(u64, u64)> {
    let c = std::ffi::CString::new(path)?;
    // SAFETY: statvfs is a C output record for which zero is valid; the path is NUL terminated and both live through
    // the call.
    let s = unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        status(libc::statvfs(c.as_ptr(), &mut s))?;
        s
    };
    let fr = s.f_frsize as u64;
    Ok((s.f_bavail as u64 * fr, s.f_blocks as u64 * fr))
}

/// Sets `CLOCK_REALTIME` to `unix` seconds, with their fraction as nanoseconds.
#[cfg(target_os = "linux")]
pub fn set_realtime_clock(unix: f64) -> io::Result<()> {
    let ts = realtime_spec(unix);
    // SAFETY: ts is an initialised timespec that lives through the call; the kernel checks its values.
    status(unsafe { libc::clock_settime(libc::CLOCK_REALTIME, &ts) })
}

#[cfg(any(target_os = "linux", test))]
fn realtime_spec(unix: f64) -> libc::timespec {
    libc::timespec { tv_sec: unix.floor() as _, tv_nsec: ((unix - unix.floor()) * 1e9) as _ }
}

/// 0: Ok; otherwise the error the call left in errno.
fn status(rc: libc::c_int) -> io::Result<()> {
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realtime_seconds_and_fraction_keep_the_conversion() {
        for (unix, seconds, nanos) in [
            (0.0, 0, 0),
            (1_794_398_400.25, 1_794_398_400, 250_000_000),
            (42.0, 42, 0),
            (-0.25, -1, 750_000_000),
            (1.999_999_999_9, 1, 999_999_999),
        ] {
            let ts = realtime_spec(unix);
            assert_eq!((ts.tv_sec, ts.tv_nsec), (seconds, nanos));
        }
    }

    #[test]
    fn filesystem_stats_report_available_bytes_and_errors() {
        let root = env!("CARGO_MANIFEST_DIR");
        let (available, total) = fs_stats(root).unwrap();
        assert!(total > 0);
        assert!(available <= total);
        assert_eq!(fs_stats(".\0").unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert!(fs_stats(&format!("{}/Cargo.toml/child", root)).is_err());
    }

    /// The CPUs the calling thread may run on, as the kernel lists them (`0-3,5`).
    #[cfg(target_os = "linux")]
    fn affinity() -> String {
        let s = std::fs::read_to_string("/proc/thread-self/status").unwrap();
        s.lines().find_map(|l| l.strip_prefix("Cpus_allowed_list:")).unwrap().trim().to_string()
    }

    // Break it catches: an empty set that pins to nothing (or to CPU 0), the set of another thread changed, a child
    // that does not inherit the set.
    #[test]
    #[cfg(target_os = "linux")]
    fn affinity_pins_only_the_calling_thread_and_is_inherited() {
        let outside = affinity();
        std::thread::spawn(|| {
            let before = affinity();
            pin_current_thread(&[]).unwrap();
            assert_eq!(affinity(), before);
            let cpu: usize = before.split(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap();
            pin_current_thread(&[cpu]).unwrap();
            assert_eq!(affinity(), cpu.to_string());
            assert_eq!(std::thread::spawn(affinity).join().unwrap(), cpu.to_string());
        })
        .join()
        .unwrap();
        assert_eq!(affinity(), outside);
    }
}
