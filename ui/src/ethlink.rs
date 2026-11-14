// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Ethernet that comes up at 10 Mbit/s: one more autonegotiation (System > Network > 10 Mbit retry, docs/network.md).
//! A user saw 10 Mbit/s after every cold start and 100 Mbit/s after a replug. The restart goes through the ethtool
//! request "restart autonegotiation" (the PHY's BMCR bit 9), not through `ip link set down`: that would take the default
//! route with it, and the DHCP client of rcS is gone by then. The link drops for about two seconds; the addresses and
//! routes stay.
//!
//! Limits, so that a partner that really is 10 Mbit/s never sees a loop: at most one restart per link-up that OOS did not
//! cause itself (a link that comes up within `OWN` seconds after the restart is its result), at most `MAX_TRIES` in a
//! start of oos-ui.

/// The Ethernet interface.
pub const IFACE: &str = "eth0";
/// Restarts per start of oos-ui.
pub const MAX_TRIES: u32 = 3;
/// A link at 10 Mbit/s for this long counts (the speed may be read before the negotiation settled).
pub const SETTLE: f64 = 3.0;
/// A link that comes up this soon after a restart is the result of that restart.
pub const OWN: f64 = 20.0;
/// After a restart, the speed is reported when the link has been up again for `SETTLE`, at the earliest after this.
const REPORT_AFTER: f64 = 6.0;

/// What `Retry::step` asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// restart the autonegotiation now
    Restart,
    /// the link after a restart, for the log
    Result(Option<u32>),
}

#[derive(Clone, Debug, Default)]
pub struct Retry {
    /// when the link came up (none: no link)
    up_since: Option<f64>,
    /// this link came from a restart of ours
    ours: bool,
    /// this link got its restart already
    done: bool,
    last_restart: Option<f64>,
    /// a restart whose result is not reported yet
    pending: bool,
    pub tries: u32,
    /// restarts that ended at a higher speed
    pub fixed: u32,
    /// the speed of the link now, Mbit/s
    pub speed: Option<u32>,
}

impl Retry {
    /// Once a second with the carrier and the speed of the link (`/sys/class/net/eth0`) and the setting.
    pub fn step(&mut self, now: f64, carrier: bool, speed: Option<u32>, on: bool) -> Option<Act> {
        if !carrier {
            self.up_since = None;
            self.speed = None;
            return None;
        }
        self.speed = speed;
        let up = *self.up_since.get_or_insert_with(|| {
            self.ours = self.last_restart.is_some_and(|t| now - t < OWN);
            self.done = false;
            now
        });
        let settled = now - up >= SETTLE;
        if self.pending && settled && self.last_restart.is_some_and(|t| now - t >= REPORT_AFTER) {
            self.pending = false;
            if speed.is_some_and(|s| s > 10) {
                self.fixed += 1;
            }
            return Some(Act::Result(speed));
        }
        if on && settled && speed == Some(10) && !self.ours && !self.done && self.tries < MAX_TRIES {
            self.done = true;
            self.tries += 1;
            self.last_restart = Some(now);
            self.pending = true;
            return Some(Act::Restart);
        }
        None
    }
}

/// Link speed in Mbit/s from sysfs (none without a link).
pub fn speed() -> Option<u32> {
    std::fs::read_to_string(format!("/sys/class/net/{}/speed", IFACE)).ok().and_then(|s| s.trim().parse::<i64>().ok()).filter(|&s| s > 0).map(|s| s as u32)
}

pub fn carrier() -> bool {
    std::fs::read_to_string(format!("/sys/class/net/{}/carrier", IFACE)).is_ok_and(|c| c.trim() == "1")
}

pub fn duplex() -> String {
    std::fs::read_to_string(format!("/sys/class/net/{}/duplex", IFACE)).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// `struct ifreq`: the name, then a 24-byte union (64-bit Linux).
#[cfg(target_os = "linux")]
#[repr(C)]
struct IfReq {
    name: [u8; 16],
    data: [u64; 3],
}

#[cfg(target_os = "linux")]
fn ifreq() -> IfReq {
    let mut r = IfReq { name: [0; 16], data: [0; 3] };
    r.name[..IFACE.len()].copy_from_slice(IFACE.as_bytes());
    r
}

#[cfg(target_os = "linux")]
fn ioctl(req: libc::c_ulong, r: &mut IfReq) -> Result<(), String> {
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let rc = libc::ioctl(fd, req as _, r as *mut IfReq);
        let e = std::io::Error::last_os_error();
        libc::close(fd);
        if rc < 0 {
            return Err(e.to_string());
        }
    }
    Ok(())
}

/// Restarts the autonegotiation of the PHY (ethtool `ETHTOOL_NWAY_RST` through `SIOCETHTOOL`).
#[cfg(target_os = "linux")]
pub fn restart() -> Result<(), String> {
    const SIOCETHTOOL: libc::c_ulong = 0x8946;
    const ETHTOOL_NWAY_RST: u32 = 0x09;
    // struct ethtool_value { u32 cmd; u32 data; }
    let mut v: [u32; 2] = [ETHTOOL_NWAY_RST, 0];
    let mut r = ifreq();
    r.data[0] = v.as_mut_ptr() as u64;
    ioctl(SIOCETHTOOL, &mut r)
}

#[cfg(not(target_os = "linux"))]
pub fn restart() -> Result<(), String> {
    Err("only on the unit".into())
}

/// The PHY's registers that tell how a link was negotiated, for the log: BMCR, BMSR, ANAR (what OOS offers), ANLPAR
/// (what the partner offers; 0 when it did not negotiate) and ANER (bit 0: the partner can negotiate, bit 4:
/// parallel detection fault). Read with `SIOCGMIIPHY` / `SIOCGMIIREG`.
#[cfg(target_os = "linux")]
pub fn mii_regs() -> String {
    const SIOCGMIIPHY: libc::c_ulong = 0x8947;
    const SIOCGMIIREG: libc::c_ulong = 0x8948;
    // struct mii_ioctl_data { u16 phy_id, reg_num, val_in, val_out } in the union of the ifreq itself
    let mut r = ifreq();
    if let Err(e) = ioctl(SIOCGMIIPHY, &mut r) {
        return format!("MII not readable: {}", e);
    }
    let phy = r.data[0] as u16;
    let mut out = format!("phy {}", phy);
    for (n, reg) in [("bmcr", 0u16), ("bmsr", 1), ("anar", 4), ("anlpar", 5), ("aner", 6)] {
        let mut r = ifreq();
        r.data[0] = phy as u64 | (reg as u64) << 16;
        match ioctl(SIOCGMIIREG, &mut r) {
            Ok(()) => out.push_str(&format!(" {}={:04x}", n, (r.data[0] >> 48) as u16)),
            Err(_) => out.push_str(&format!(" {}=?", n)),
        }
    }
    out
}

#[cfg(not(target_os = "linux"))]
pub fn mii_regs() -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `step` once a second over `secs` seconds from `t`, returning what it asked for.
    fn run(r: &mut Retry, t: &mut f64, secs: u32, carrier: bool, speed: Option<u32>, on: bool) -> Vec<(f64, Act)> {
        let mut v = Vec::new();
        for _ in 0..secs {
            if let Some(a) = r.step(*t, carrier, speed, on) {
                v.push((*t, a));
            }
            *t += 1.0;
        }
        v
    }

    #[test]
    fn a_slow_link_gets_one_restart_and_the_result_is_reported() {
        let (mut r, mut t) = (Retry::default(), 0.0);
        // the speed is read after SETTLE
        let a = run(&mut r, &mut t, 5, true, Some(10), true);
        assert_eq!(a, vec![(SETTLE, Act::Restart)]);
        // the link drops and comes back at 100
        assert!(run(&mut r, &mut t, 2, false, None, true).is_empty());
        let a = run(&mut r, &mut t, 10, true, Some(100), true);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].1, Act::Result(Some(100)));
        assert_eq!((r.tries, r.fixed, r.speed), (1, 1, Some(100)));
    }

    #[test]
    fn a_partner_at_10_mbits_sees_no_loop() {
        let (mut r, mut t) = (Retry::default(), 0.0);
        assert_eq!(run(&mut r, &mut t, 5, true, Some(10), true).len(), 1);
        run(&mut r, &mut t, 2, false, None, true);
        // back at 10: reported, no second restart
        let a = run(&mut r, &mut t, 600, true, Some(10), true);
        assert_eq!(a.iter().map(|x| &x.1).collect::<Vec<_>>(), vec![&Act::Result(Some(10))]);
        assert_eq!((r.tries, r.fixed), (1, 0));
        // a PHY whose link does not even drop: no loop either
        let (mut r, mut t) = (Retry::default(), 0.0);
        let a = run(&mut r, &mut t, 600, true, Some(10), true);
        assert_eq!(a.iter().filter(|x| x.1 == Act::Restart).count(), 1);
    }

    #[test]
    fn a_replug_counts_again_up_to_the_limit() {
        let (mut r, mut t) = (Retry::default(), 0.0);
        let mut restarts = 0;
        for _ in 0..6 {
            // unplugged for a minute, plugged in at 10 Mbit/s
            run(&mut r, &mut t, 60, false, None, true);
            restarts += run(&mut r, &mut t, 60, true, Some(10), true).iter().filter(|x| x.1 == Act::Restart).count();
        }
        assert_eq!(restarts, MAX_TRIES as usize);
    }

    #[test]
    fn nothing_when_off_or_fast() {
        let (mut r, mut t) = (Retry::default(), 0.0);
        assert!(run(&mut r, &mut t, 60, true, Some(10), false).is_empty());
        let (mut r, mut t) = (Retry::default(), 0.0);
        assert!(run(&mut r, &mut t, 60, true, Some(100), true).is_empty());
        // a short moment at 10 during the negotiation does not count
        let (mut r, mut t) = (Retry::default(), 0.0);
        assert!(run(&mut r, &mut t, 2, true, Some(10), true).is_empty());
        assert!(run(&mut r, &mut t, 60, true, Some(100), true).is_empty());
        assert_eq!(r.tries, 0);
    }
}
