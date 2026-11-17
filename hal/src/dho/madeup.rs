// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! A made-up calibration set (docs/protocols/self-cal.md 16): every calibration file the register sequences read,
//! with invented values, for computers that have no scope. The files of a real unit never enter the tree, so the
//! comparison of two builds (`tools/compare-base.sh`) would otherwise leave out every value that calibration changes.
//!
//! The values mean nothing for a real unit. They are small round numbers that follow the indices in a plain way
//! (channel, block group, row, mode, core, slot), so code that mixes up an index writes other registers. They stay
//! inside what each field holds: nothing is cut off, no gain is 0. The header stamp is fixed, so the same bytes come
//! out on every run and on every computer.

use crate::dho::adc::CalAdc;
use crate::dho::{afe, calfile, selfcal};

/// Header stamp of every file of the set: 2000-01-01 00:00:00.
pub const STAMP: calfile::Stamp = (2000, 1, 1, 0, 0, 0);

/// The files of the set.
pub const FILES: [&str; 8] = [
    "cal_adc.hex",
    "cal_ddr.hex",
    "cal_lsb.hex",
    "cal_afe_zero.hex",
    "cal_afe_bandwidth.hex",
    "cal_vertical.hex",
    "cal_la.hex",
    "cal_afg.hex",
];

/// `cal_adc.hex` (self-cal.md 14.10). Core offsets of mode m: 10 (m + 1) + core. Core gains of channel c:
/// 10 c + core - 20, and the same rule in record 4, which one pair of channels reads for two cores. Sample-clock
/// delays of single mode m: 20 (m + 1) + i. Clock delay of mode k (records 4..7): 100 (k + 1) and k + 1. References 30
/// and 31. Fine code of slot k for channel c: (c + k) mod 4.
fn adc(of: [usize; 4]) -> Vec<u8> {
    let mut c = CalAdc::from_payload(vec![0; 0x738]).expect("the payload size of cal_adc.hex");
    for m in 0..11 {
        c.set_offsets(m, &std::array::from_fn(|n| 10 * (m as i32 + 1) + n as i32));
    }
    for (rec, ch) in of.into_iter().chain([4]).enumerate() {
        for core in 0..8 {
            c.set16(CalAdc::gain_at(rec, core), 10 * ch as i32 + core as i32 - 20);
        }
    }
    for m in 0..4 {
        let rec = CalAdc::off_at(m, 0);
        for i in 0..4 {
            c.set16(rec + 0x40 + 2 * i, 20 * (m as i32 + 1) + i as i32);
        }
        let rec = CalAdc::off_at(4 + m, 0);
        c.set16(rec + 0x40, 100 * (m as i32 + 1));
        c.set16(rec + 0x42, m as i32 + 1);
    }
    c.set16(0x48, 30);
    c.set16(0x4a, 31);
    for (ch, v) in of.into_iter().enumerate() {
        for slot in 0..16 {
            c.set32(CalAdc::fine_at(ch, slot), ((v + slot) % 4) as i32);
        }
    }
    c.payload().to_vec()
}

/// `cal_ddr.hex` (cold-boot.md 3.2): six taps, each in the low word of 8 bytes.
fn ddr() -> Vec<u8> {
    let mut p = vec![0; 48];
    for (k, v) in [10u32, 1000, 2000, 3000, 500, 0].into_iter().enumerate() {
        p[8 * k..8 * k + 4].copy_from_slice(&v.to_le_bytes());
    }
    p
}

/// `cal_lsb.hex` (afe-settings.md 1.3): 10 + c microvolts per code on input-gain path 0, ten times that on each next
/// path.
fn lsb(of: [usize; 4]) -> Vec<u8> {
    let mut p = Vec::with_capacity(128);
    for v in of {
        for path in [1e-6, 1e-5, 1e-4, 1e-3] {
            p.extend_from_slice(&((10 + v) as f64 * path).to_le_bytes());
        }
    }
    p
}

fn ints(v: impl IntoIterator<Item = i32>) -> Vec<u8> {
    v.into_iter().flat_map(i32::to_le_bytes).collect()
}

/// `cal_afe_zero.hex` (self-cal.md 3.1): slot k of channel c is 5 k + c + 1; the slots 0, 7 and 14 hold one bit,
/// (c + k) mod 2.
fn zero(of: [usize; 4]) -> Vec<u8> {
    ints(
        of.into_iter()
            .flat_map(|v| (0..20).map(move |k| if k % 7 == 0 { (v as i32 + k) % 2 } else { 5 * k + v as i32 + 1 })),
    )
}

/// `cal_afe_bandwidth.hex` (afe-settings.md 5.4): for the input-gain paths 1..3 the three trims OOS takes from the
/// file, 100 + 10 path + c, (path + c) mod 8 and (2 path + c) mod 16; for path 0 the two trims 500 + 10 c and
/// 600 + 10 c. The entries OOS replaces by fixed values are 0.
fn bandwidth(of: [usize; 4]) -> Vec<u8> {
    ints(of.into_iter().flat_map(|v| {
        let (mut b, v) = ([0i32; 20], v as i32);
        for path in 1..=3 {
            let o = 6 * path as usize - 5;
            b[o] = 100 + 10 * path + v;
            b[o + 1] = (path + v) % 8;
            b[o + 4] = (2 * path + v) % 16;
        }
        b[18] = 500 + 10 * v;
        b[19] = 600 + 10 * v;
        b
    }))
}

/// Row `i` of the block of group `var` for channel values `v` (afe-settings.md 1.3). The gain codes follow the row in
/// steps and keep the pairs the zero trims know (input gain 0 with VGA 0 or 1 and SGA 0 or 1, input gain 1..3 with
/// VGA 2 and SGA 2 or 3); SGAR is the group, MUXR the channel. Digital gain 1 + (c + 1) / 100 + group / 1000 +
/// (row mod 100) / 10000, one more in the first hundred rows, so the x2 of the read path is used too. Zero code
/// 32000 + 100 (c + 1) + 10 group + row / 100.
fn row(var: i32, v: i32, i: i32, key: i64) -> afe::Rec {
    let ig = [0, 0, 0, 0, 1, 1, 2, 2, 3][(i / 100) as usize];
    let (vga, sga) = if ig == 0 { (i / 200, i / 50 % 2) } else { (2, 2 + i / 50 % 2) };
    let part = 100 * (v + 1) + 10 * var;
    afe::Rec {
        key,
        g: [ig, vga, i / 25 % 4, sga, var, i / 10 % 8, v, 0],
        gain: ((if i < 100 { 20000 } else { 10000 }) + part + i % 100) as f64 / 10000.0,
        zero: 32000 + part + i / 100,
        last: 0,
    }
}

/// `cal_vertical.hex`: 16 blocks (group x channel) of 801 rows.
fn vertical(of: [usize; 4]) -> Vec<u8> {
    let keys = afe::scale_ladder();
    let mut p = Vec::with_capacity(16 * 801 * 56);
    for var in 0..4 {
        for v in of {
            for (i, &key) in keys.iter().enumerate() {
                p.extend_from_slice(&selfcal::rec_bytes(&row(var, v as i32, i as i32, key)));
            }
        }
    }
    p
}

/// `cal_la.hex` (la.md 5.2): the zero codes of the two pods.
fn la() -> Vec<u8> {
    [8000u32, 9000].into_iter().flat_map(u32::to_le_bytes).collect()
}

/// `cal_afg.hex` (afg.md 6): 40 records of two ranges each where a record is used. Offset records 0..14: -10 V..0 and
/// 0..10 V, k = 1 + (2 (r + 1) + i) / 1000, b = r + 1 millivolts. Amplitude records 16..28 (gear g): 0..1 V and
/// 1..20 V, k = 1 + (10 (g + 1) + i) / 1000, b = (g + 1) / 10 millivolts. Flatness records 30..32 (below 100 kHz, the
/// upper end first) and 34..36: k = 0, b = 1 + (10 (g + 1) + i) / 1000 below and 1 - (10 (g + 1) + i) / 1000 above.
fn afg() -> Vec<u8> {
    const REC: usize = 0x968;
    let mut p = vec![0u8; 0x18 + 40 * REC];
    p[0x10..0x14].copy_from_slice(&1f32.to_le_bytes());
    let mut seg = |r: usize, i: usize, ends: (i64, i64), k: f32, b: f32| {
        let at = 0x18 + r * REC;
        p[at..at + 4].copy_from_slice(&(i as i32 + 1).to_le_bytes());
        p[at + 8 + 8 * i..at + 16 + 8 * i].copy_from_slice(&ends.0.to_le_bytes());
        p[at + 0x328 + 8 * i..at + 0x330 + 8 * i].copy_from_slice(&ends.1.to_le_bytes());
        p[at + 0x648 + 4 * i..at + 0x64c + 4 * i].copy_from_slice(&k.to_le_bytes());
        p[at + 0x7d8 + 4 * i..at + 0x7dc + 4 * i].copy_from_slice(&b.to_le_bytes());
    };
    let milli = |n: usize| n as f32 / 1000.0;
    const V: i64 = 1_000_000_000;
    const KHZ: i64 = 1_000_000_000;
    for i in 0..2 {
        for r in 0..15 {
            let ends = [(-10 * V, 0), (0, 10 * V)][i];
            seg(r, i, ends, milli(1000 + 2 * (r + 1) + i), ((r + 1) * 1_000_000) as f32);
        }
        for g in 0..13 {
            let ends = [(0, V), (V, 20 * V)][i];
            seg(16 + g, i, ends, milli(1000 + 10 * (g + 1) + i), ((g + 1) * 100_000) as f32);
        }
        for g in 0..3 {
            seg(30 + g, i, [(100 * KHZ, KHZ), (KHZ, 0)][i], 0.0, milli(1000 + 10 * (g + 1) + i));
            seg(34 + g, i, [(100 * KHZ, 10_000 * KHZ), (10_000 * KHZ, 100_000 * KHZ)][i], 0.0, milli(990 - 10 * g - i));
        }
    }
    p
}

/// The payloads of the set, by file name. `of[c]`: the channel whose invented values channel c gets; `[0, 1, 2, 3]`
/// is the set, anything else exchanges channels (the tests show with it that the values of each channel reach the
/// registers of that channel).
pub fn payloads(of: [usize; 4]) -> Vec<(&'static str, Vec<u8>)> {
    let p = [adc(of), ddr(), lsb(of), zero(of), bandwidth(of), vertical(of), la(), afg()];
    FILES.into_iter().zip(p).collect()
}

/// Writes the set into `dir` (made if it is missing) through `calfile::save` and returns the paths. Refused: the
/// places a scope keeps its calibration in (`/rigol`, `/oos`), and a folder that holds a file of the same name
/// which is not of this set, so the files of a real unit are never replaced by invented ones.
pub fn write(dir: &str) -> std::io::Result<Vec<String>> {
    let refuse = |m: String| Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, m));
    let path = std::path::absolute(dir)?;
    if path.components().any(|c| c == std::path::Component::ParentDir) {
        return refuse(format!("{}: name the folder without '..'", dir));
    }
    // the folder as it will be reached: the part of the path that exists, links followed
    let mut real = path.clone();
    while !real.exists() && real.pop() {}
    let real = real.canonicalize()?;
    if [&path, &real].iter().any(|p| p.starts_with("/rigol") || p.starts_with("/oos")) {
        return refuse(format!("{}: a made-up calibration set is never written where a scope keeps its own", dir));
    }
    // nothing is written before every file that is there is known to be of a set
    for name in FILES {
        let file = path.join(name);
        if std::fs::read(&file).is_ok_and(|old| calfile::stamp(&old) != Some(STAMP)) {
            return refuse(format!("{} is not a file of a made-up set: nothing written", file.display()));
        }
    }
    let mut out = Vec::new();
    for (name, payload) in payloads([0, 1, 2, 3]) {
        let file = path.join(name);
        // a file that is already as it should be stays: a second run leaves the folder alone
        if std::fs::read(&file).ok() != Some(calfile::build(&payload, STAMP)) {
            calfile::save(dir, name, &payload, STAMP)?;
        }
        out.push(file.display().to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dho::seq::{self, Settings, State, Step};
    use crate::dho::{adc, afg, boot, la};

    const OWN: [usize; 4] = [0, 1, 2, 3];
    /// CH1 and CH2 exchanged.
    const EXCHANGED: [usize; 4] = [1, 0, 2, 3];

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("oos-madeup-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// The container of file `name` of the set with the channels as in `of`.
    fn file(of: [usize; 4], name: &str) -> Vec<u8> {
        let p = payloads(of).into_iter().find(|(n, _)| *n == name).unwrap().1;
        calfile::build(&p, STAMP)
    }

    fn afe_cal(of: [usize; 4]) -> afe::Cal {
        let f = |n: &str| file(of, n);
        afe::Cal::from_bytes(
            &f("cal_lsb.hex"),
            &f("cal_afe_zero.hex"),
            &f("cal_afe_bandwidth.hex"),
            f("cal_vertical.hex"),
        )
    }

    fn adc_cal(of: [usize; 4]) -> CalAdc {
        CalAdc::from_file_bytes(&file(of, "cal_adc.hex")).unwrap()
    }

    #[test]
    fn every_loader_accepts_its_file() {
        let d = temp("load");
        let dir = d.to_str().unwrap();
        let written = write(dir).unwrap();
        assert_eq!(written.len(), FILES.len());
        for name in FILES {
            let b = std::fs::read(d.join(name)).unwrap();
            assert!(calfile::parse(&b).is_ok(), "{}", name);
            assert_eq!(calfile::stamp(&b), Some(STAMP), "{}", name);
        }
        // the four files of the front ends, all from this folder, and what they hold
        let (cal, from) = afe::Cal::load_from(dir).unwrap();
        assert!(from.iter().all(|(_, at)| at == dir), "{:?}", from);
        assert_eq!(from.len(), 4);
        assert_eq!(cal.key(640), 1_000_000_000);
        let r = cal.rec(2, 1, 640);
        assert_eq!((r.key, r.g, r.zero), (1_000_000_000, [2, 2, 1, 2, 2, 0, 1, 0], 32226));
        assert_eq!(r.gain, 1.026);
        assert_eq!(cal.rec(0, 3, 50).gain, 2.045);
        assert_eq!(cal.lsb(1, 2), 11.0 * 1e-4);
        assert_eq!(cal.zero_trims(2)[..4], [0, 8, 13, 18]);
        assert!((0..4).all(|var| (0..4).all(|ch| cal.block_filled(var, ch))));
        // the two files of the ADC: by the acquisition and by the cold boot
        let own = adc::CalAdc::load(dir).unwrap();
        assert_eq!(own.payload(), adc(OWN));
        assert_eq!((own.offsets(5), own.get16(CalAdc::gain_at(3, 7))), ([60, 61, 62, 63, 64, 65, 66, 67], 17));
        let boot = boot::CalFiles::load(dir);
        assert_eq!(boot.adc.unwrap().payload(), adc(OWN));
        assert_eq!(boot.ddr.unwrap().taps, [10, 1000, 2000, 3000, 500, 0]);
        // logic analyser and generator
        assert_eq!(la::Cal::load(dir), Some(la::Cal { zero: [8000, 9000] }));
        let g = afg::Cal::load(dir).unwrap();
        let (plain, with) =
            (afg::codes(None, 1e9, 0.0, 1e9, false, false), afg::codes(Some(&g), 1e9, 0.0, 1e9, false, false));
        assert!(plain.0 != with.0 && plain.1 != with.1, "{:?} {:?}", plain, with);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn two_runs_give_the_same_bytes() {
        let (a, b) = (temp("a"), temp("b"));
        let read = |d: &std::path::Path| FILES.map(|n| std::fs::read(d.join(n)).unwrap());
        write(a.to_str().unwrap()).unwrap();
        let first = read(&a);
        // again into the same folder: nothing changes, and no file is kept as a copy of an older one
        write(a.to_str().unwrap()).unwrap();
        assert_eq!(read(&a), first);
        assert_eq!(std::fs::read_dir(&a).unwrap().count(), FILES.len());
        write(b.to_str().unwrap()).unwrap();
        assert_eq!(read(&b), first);
        // the same bytes on every computer: the check sums of the files as they are now. A change of the invented
        // values changes them, and with them every comparison that uses the set.
        let sums = first.iter().map(|f| crate::crc32(f)).collect::<Vec<_>>();
        assert_eq!(sums, PINNED, "{:#010x?}", sums);
        for d in [a, b] {
            std::fs::remove_dir_all(d).unwrap();
        }
    }

    /// CRC32 of each file of the set, in the order of `FILES`.
    const PINNED: [u32; 8] =
        [0x42a8_0938, 0x4894_844d, 0x0be6_fd9c, 0xd853_7bbe, 0x5879_fbca, 0x1741_ffbc, 0x142c_b59c, 0x34b3_7554];

    #[test]
    fn the_places_of_a_scope_and_foreign_files_are_refused() {
        for dir in ["/rigol/data", "/rigol/data/default", "/rigol//data/", "/oos/cal", "/oos/cal/", "/oos"] {
            let was_there = std::path::Path::new(dir).exists();
            let e = write(dir).unwrap_err();
            assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{}", dir);
            assert!(was_there || !std::path::Path::new(dir).exists(), "{} was made", dir);
        }
        // a way round through `..` is not followed
        let up = std::env::temp_dir().join("../rigol/data");
        assert_eq!(write(up.to_str().unwrap()).unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
        // a folder with a calibration file that is not of the set (another stamp) stays as it is
        let d = temp("foreign");
        let dir = d.to_str().unwrap();
        calfile::save(dir, "cal_la.hex", &[1; 8], (2026, 11, 3, 21, 5, 9)).unwrap();
        let before = std::fs::read(d.join("cal_la.hex")).unwrap();
        assert_eq!(write(dir).unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read(d.join("cal_la.hex")).unwrap(), before);
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1, "nothing was written next to it");
        std::fs::remove_dir_all(&d).unwrap();
    }

    fn afe_writes(st: &[Step], ch: usize) -> Vec<(u8, u16)> {
        st.iter()
            .filter_map(|x| match x {
                Step::Afe(afe::Op::Afe { ch: c, reg, val }) if *c == ch => Some((*reg, *val)),
                _ => None,
            })
            .collect()
    }

    fn text(st: &[Step]) -> Vec<String> {
        st.iter().map(|x| format!("{:?}", x)).collect()
    }

    /// The front ends: an acquisition of one channel with the AFE set-up.
    #[test]
    fn exchanged_channels_change_the_planned_afe_writes() {
        let plan = |of: [usize; 4], ch: usize| {
            let s = Settings { afe: true, mask: 1 << ch, trig_src: ch as u32 + 1, ..Settings::default() };
            seq::plan(&mut State::handover(1 << ch, None), &s, Some(&afe_cal(of)), None).unwrap().0
        };
        let (own, exchanged) = (plan(OWN, 0), plan(EXCHANGED, 0));
        assert_ne!(text(&own), text(&exchanged));
        // exactly exchanged: CH1 now gets the register values CH2 had, in the AFE and in the read path
        assert!(afe_writes(&own, 0).len() > 20);
        assert_ne!(afe_writes(&own, 0), afe_writes(&exchanged, 0));
        assert_eq!(afe_writes(&exchanged, 0), afe_writes(&plan(OWN, 1), 1));
        let read_path =
            |st: &[Step]| st.iter().find_map(|x| if let Step::Wr((0x14c8, v)) = x { Some(*v) } else { None }).unwrap();
        assert_ne!(read_path(&own), read_path(&exchanged));
        assert_eq!(read_path(&exchanged), read_path(&plan(OWN, 1)));
        // the other channels do not notice
        assert_eq!(text(&plan(OWN, 2)), text(&plan(EXCHANGED, 2)));
    }

    /// The ADC: an acquisition of CH1 and CH2 after one of CH1 alone switches the ADC mode.
    #[test]
    fn exchanged_channels_change_the_planned_adc_writes() {
        let plan = |of: [usize; 4], mask: u32| {
            let cal = adc_cal(of);
            let s = Settings { mask, trig_src: mask.trailing_zeros() + 1, ..Settings::default() };
            seq::plan(&mut State::handover(1, Some(&cal)), &s, None, Some(&cal)).unwrap().0
        };
        let adc_words = |st: &[Step]| {
            st.iter().filter_map(|x| if let Step::Wr((0x1034, v)) = x { Some(*v) } else { None }).collect::<Vec<_>>()
        };
        let (own, exchanged) = (plan(OWN, 3), plan(EXCHANGED, 3));
        // input mode, reference, 8 gains, 8 offsets, 4 sample-clock delays, clock delay: every value is in range
        assert_eq!(adc_words(&own).len(), 2 + 1 + 8 + 8 + 4 + 1);
        assert_ne!(adc_words(&own), adc_words(&exchanged));
        assert_ne!(text(&plan(OWN, 2)), text(&plan(EXCHANGED, 2)));
        // CH3 and CH4 keep their values
        assert_eq!(text(&plan(OWN, 0xc)), text(&plan(EXCHANGED, 0xc)));
    }

    /// Nothing is cut off: at every tenth row of every block the offset DAC stays inside its range with two
    /// divisions of offset, and the read-path gain is between 1 and 2.
    #[test]
    fn the_values_stay_in_range() {
        let cal = afe_cal(OWN);
        for ch in 0..4 {
            for bwv in [afe::bw::S20M, afe::bw::mhz(100), afe::bw::mhz(300), afe::bw::OFF] {
                for i in (0..801).step_by(10) {
                    let scale = cal.key(i);
                    let (ops, r, code) =
                        afe::configure(&mut afe::Afe::new(ch), &cal, scale, 2 * scale, bwv, 1_250_000_000, 1 << ch);
                    assert!(code > 0 && code < 0xffff, "CH{} row {}: DAC {}", ch + 1, i, code);
                    assert!(r.key == scale && ops.len() > 20);
                    let g = crate::dho::wpu::split_gain(r.gain).1;
                    assert!((1.0..2.0).contains(&g), "CH{} row {}: gain {}", ch + 1, i, r.gain);
                    assert!(r.g[..7].iter().zip([3, 3, 3, 3, 3, 7, 3]).all(|(&v, max)| (0..=max).contains(&v)));
                }
            }
        }
    }
}
