// SPDX-License-Identifier: GPL-2.0-or-later

//! Thread placement on a big.LITTLE SoC (fork, docs/fork/perf-plan.md item 6). Without it the frame loop, the GPU
//! helper and the acquisition thread share both A72 of the RK3399 (`pin_to_big_cores`). With `perf.place = on`:
//! the frame loop (and the GPU helper it starts) on the first big core, the acquisition thread on the second. Workers
//! already go to the little cores (`to_little_cores`). Opt-in until measured on the unit; Linux only, a no-op elsewhere and on SoCs whose cores are
//! all alike. The xdma interrupt goes to a little core from the rootfs (`/proc/irq/<n>/smp_affinity`), not from here.

use std::path::Path;

/// What a thread does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// the frame loop and the GPU helper
    Ui,
    /// the acquisition thread (`board::dho::hw`)
    Hw,
}

/// The CPUs of each role.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub ui: Vec<usize>,
    pub hw: Vec<usize>,
}

impl Plan {
    /// From (cpu, `cpu_capacity`) pairs: None without two kinds of core.
    pub fn from_capacities(cap: &[(usize, u32)]) -> Option<Plan> {
        let max = cap.iter().map(|x| x.1).max()?;
        let mut big: Vec<usize> = cap.iter().filter(|x| x.1 == max).map(|x| x.0).collect();
        if big.len() == cap.len() {
            return None;
        }
        big.sort_unstable();
        // one big core: the frame loop and the acquisition share it, as without a plan
        let hw = big.get(1).copied().unwrap_or(big[0]);
        Some(Plan { ui: vec![big[0]], hw: vec![hw] })
    }

    pub fn cpus(&self, r: Role) -> &[usize] {
        match r {
            Role::Ui => &self.ui,
            Role::Hw => &self.hw,
        }
    }
}

/// (cpu, capacity) of every CPU under `root` (`/sys/devices/system/cpu`) that has a `cpu_capacity` file.
pub fn capacities(root: &Path) -> Vec<(usize, u32)> {
    (0..64)
        .filter_map(|c| {
            std::fs::read_to_string(root.join(format!("cpu{}/cpu_capacity", c)))
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .map(|v| (c, v))
        })
        .collect()
}

/// The number of little cores (None on a SoC whose cores are all alike, or without `cpu_capacity`): the deposit
/// workers of the batch pipeline, one per little core.
pub fn little_count() -> Option<usize> {
    little_of(&capacities(Path::new("/sys/devices/system/cpu")))
}

fn little_of(cap: &[(usize, u32)]) -> Option<usize> {
    let max = cap.iter().map(|x| x.1).max()?;
    Some(cap.iter().filter(|x| x.1 < max).count()).filter(|&n| n > 0)
}

/// `perf.place = on` in system.conf (it also moves the xdma interrupts: rootfs `oos-irq-place`), or OOS_PLACE set
/// (any value but empty or 0; off with those).
fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| FLAG.get())
}

const FLAG: crate::sysconf::Flag = crate::sysconf::Flag { key: "perf.place", env: Some("OOS_PLACE"), on: "on" };

/// The calling thread (and the threads and processes it starts after this) onto the CPUs of `r`, when placement is on.
pub fn place(r: Role) {
    if !enabled() {
        return;
    }
    #[cfg(target_os = "linux")]
    {
        static PLAN: std::sync::OnceLock<Option<Plan>> = std::sync::OnceLock::new();
        let Some(plan) = PLAN.get_or_init(|| Plan::from_capacities(&capacities(Path::new("/sys/devices/system/cpu"))))
        else {
            return;
        };
        let cpus = plan.cpus(r);
        if let Err(e) = crate::system::pin_current_thread(cpus) {
            eprintln!("oos-ui: placement {:?} on {:?}: {}", r, cpus, e);
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = r;
}

/// The cores with less than the largest `cpu_capacity` (empty on a SoC whose cores are all alike, or without the
/// file): where the background threads go.
#[cfg(target_os = "linux")]
pub fn little_cores() -> Vec<usize> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_reads_both_files() {
        FLAG.check_both_files();
    }

    fn fake_sysfs(cap: &[(usize, u32)]) -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "oos-place-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        for &(c, v) in cap {
            let d = root.join(format!("cpu{}", c));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("cpu_capacity"), format!("{}\n", v)).unwrap();
        }
        // a directory without the file (cpufreq, cpuidle) is no CPU
        std::fs::create_dir_all(root.join("cpufreq")).unwrap();
        root
    }

    // Break it catches: the acquisition thread on the frame loop's core, a plan on a SoC whose cores are all alike,
    // big and little swapped.
    #[test]
    fn rk3399_frame_loop_and_acquisition_get_one_a72_each() {
        let root = fake_sysfs(&[(0, 485), (1, 485), (2, 485), (3, 485), (4, 1024), (5, 1024)]);
        let p = Plan::from_capacities(&capacities(&root)).unwrap();
        assert_eq!((p.cpus(Role::Ui), p.cpus(Role::Hw)), (&[4][..], &[5][..]));
        assert_eq!(little_of(&capacities(&root)), Some(4));
        std::fs::remove_dir_all(&root).unwrap();
        // all alike, or no capacity files: nothing to place
        let root = fake_sysfs(&[(0, 1024), (1, 1024), (2, 1024), (3, 1024)]);
        assert_eq!(Plan::from_capacities(&capacities(&root)), None);
        assert_eq!(little_of(&capacities(&root)), None);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(Plan::from_capacities(&[]), None);
        // one big core: the frame loop and the acquisition share it; the order of the files does not matter
        let p = Plan::from_capacities(&[(3, 1024), (2, 400), (0, 400), (1, 400)]).unwrap();
        assert_eq!((p.ui.clone(), p.hw.clone()), (vec![3], vec![3]));
    }
}
