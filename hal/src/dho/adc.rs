// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! ADC channel mode (which channels share the 8 interleaved ADC cores): input mode, reference, per-core gain/offset,
//! sample-clock delays and SPU fine gain from the unit's cal_adc.hex. Port of `tools/adcmodel.py`
//! (docs/protocols/adc-mode.md). The ADC words go to FPGA 0x1034 (bit 24 start, 22:16 ADC register, 15:0 data);
//! several ADC registers are write-only and kept as shadows, so modes must be applied in order from the state after boot.

/// One FPGA write with the sleeps before and after (microseconds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wr {
    pub pre_us: u32,
    pub reg: u32,
    pub val: u32,
    pub post_us: u32,
}

/// ADC mode of a channel mask: 0..3 one channel, 4..9 pairs, 10 = 3 or 4 channels.
pub fn mode_of_mask(mask: u32) -> Option<usize> {
    const M: [i8; 16] = [-1, 0, 1, 4, 2, 5, 7, 10, 3, 6, 8, 10, 9, 10, 10, 10];
    let m = M[(mask & 0xf) as usize];
    (m >= 0).then_some(m as usize)
}

/// Channel pair (i, j) of modes 0..9 for the SPU fine gain.
const FINE_PAIR: [(usize, usize); 10] = [(0, 0), (1, 1), (2, 2), (3, 3), (0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
/// Input-mode shadows (SWONP1, SWONP2) per mode; reg 0x54 data = SWONP1 ^ 0x1041, reg 0x55 data = SWONP2 ^ 4.
const SWONP: [(u16, u16); 11] = [
    (0x8421, 0), (0x0842, 1), (0x2108, 4), (0x1084, 2), (0x0821, 1), (0x2021, 4), (0x1021, 2), (0x2042, 4), (0x1042, 2), (0x1108, 2), (0x1041, 4),
];
const REC: usize = 0x68;
const FINE: usize = 0x478;

/// Payload of a /rigol/data cal_*.hex container (20-byte header, u32 length, u32 crc32, payload), length checked.
pub fn container(d: &[u8]) -> Option<&[u8]> {
    let ln = u32::from_le_bytes(d.get(20..24)?.try_into().ok()?) as usize;
    d.get(28..28 + ln)
}

pub struct CalAdc {
    p: Vec<u8>,
}

impl CalAdc {
    pub fn from_file_bytes(d: &[u8]) -> Option<CalAdc> {
        let p = container(d)?;
        (p.len() == 0x738).then(|| CalAdc { p: p.to_vec() })
    }
    /// The first valid cal_adc.hex: OOS's own (`calfile::OOS_CAL_DIR`), then `dir`, then `dir/default`.
    pub fn load(dir: &str) -> std::io::Result<CalAdc> {
        let def = format!("{}/default", dir.trim_end_matches('/'));
        let (d, _) = crate::dho::calfile::load_first("cal_adc.hex", &[crate::dho::calfile::OOS_CAL_DIR, dir, def.as_str()])
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "cal_adc.hex: no valid copy"))?;
        CalAdc::from_file_bytes(&d).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "cal_adc.hex: bad container"))
    }
    /// The 0x738-byte payload (for the self-cal, which edits a copy and saves it with `calfile`).
    pub fn payload(&self) -> &[u8] {
        &self.p
    }
    /// A copy (the ADC writes of a mode change read the payload while the calibration edits its own).
    pub fn clone_payload(&self) -> CalAdc {
        CalAdc { p: self.p.clone() }
    }
    pub fn from_payload(p: Vec<u8>) -> Option<CalAdc> {
        (p.len() == 0x738).then_some(CalAdc { p })
    }
    /// Payload offsets (self-cal.md 14.9): offsets of core n in mode m, gain of core n for channel c, fine code of
    /// slot k for channel c.
    pub fn off_at(mode: usize, core: usize) -> usize {
        mode * REC + 2 * core
    }
    pub fn gain_at(ch: usize, core: usize) -> usize {
        ch * REC + 0x20 + 2 * core
    }
    pub fn fine_at(ch: usize, slot: usize) -> usize {
        FINE + 64 * ch + 4 * slot
    }
    pub fn get16(&self, o: usize) -> i32 {
        self.s16(o)
    }
    pub fn set16(&mut self, o: usize, v: i32) {
        self.p[o..o + 2].copy_from_slice(&(v as i16).to_le_bytes());
    }
    pub fn set32(&mut self, o: usize, v: i32) {
        self.p[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn s16(&self, o: usize) -> i32 {
        i16::from_le_bytes([self.p[o], self.p[o + 1]]) as i32
    }
    fn i32(&self, o: usize) -> i32 {
        i32::from_le_bytes(self.p[o..o + 4].try_into().unwrap())
    }
    fn gain_src(&self, mode: usize, core: usize) -> usize {
        let ch = if mode <= 3 {
            mode
        } else if mode == 10 {
            core % 4
        } else {
            let (lo, hi) = FINE_PAIR[mode];
            if core % 2 == 0 {
                lo
            } else {
                hi
            }
        };
        if mode == 9 && (core == 3 || core == 7) {
            return 4 * REC + 0x20 + 2 * core; // vendor quirk: record 4 instead of record 3
        }
        ch * REC + 0x20 + 2 * core
    }
    fn fine(&self, mode: usize) -> u32 {
        let buf: Vec<i32> = if mode < 10 {
            let (i, j) = FINE_PAIR[mode];
            (0..16).map(|k| self.i32(FINE + 64 * if k < 8 { i } else { j } + 4 * k)).collect()
        } else {
            (0..16).map(|k| self.i32(FINE + 64 * (k / 4) + 4 * k)).collect()
        };
        buf.iter().enumerate().fold(0, |v, (k, &f)| v | ((f as u32 & 3) << (2 * k)))
    }
}

/// Shadows of the write-only ADC registers.
pub struct Adc {
    r01: u16,
    r04: u16,
    r05: u16,
    r09: u16,
    gain: [u16; 8],
    off: [u16; 8],
    swonp: (u16, u16),
}

impl Default for Adc {
    /// Start values.
    fn default() -> Adc {
        Adc { r01: 0, r04: 0x8000, r05: 0, r09: 0x5020, gain: [2; 8], off: [0; 8], swonp: SWONP[0] }
    }
}

fn spi(w: &mut Vec<Wr>, pre: u32, addr: u32, data: u16, post: u32) {
    w.push(Wr { pre_us: pre, reg: 0x1034, val: 0x0100_0000 | (addr & 0x7f) << 16 | data as u32, post_us: post });
}

impl Adc {
    /// Input mode words of a mode.
    fn input_mode(&mut self, w: &mut Vec<Wr>, mode: usize) {
        self.swonp = SWONP[mode];
        spi(w, 100, 0x54, self.swonp.0 ^ 0x1041, 0);
        spi(w, 100, 0x55, self.swonp.1 ^ 0x0004, 100);
    }

    /// ADC parameters of `mode` (the words for ADC 1 only update shadows).
    pub fn parameter(&mut self, cal: &CalAdc, mode: usize) -> Vec<Wr> {
        let mut w = Vec::new();
        let (v0, v1) = (cal.s16(0x48), cal.s16(0x4a));
        self.r09 = (self.r09 & 0xffc0) | (v0 as u16 & 0x3f);
        spi(&mut w, 100, 0x09, self.r09 ^ 0x2720, 100);
        self.r09 = (self.r09 & 0xffc0) | (v1 as u16 & 0x3f);
        w.push(Wr { pre_us: 0, reg: 0x1058, val: cal.fine(mode), post_us: 0 });
        let order: [[usize; 2]; 4] = if mode <= 3 || mode == 10 { [[0, 4], [1, 5], [2, 6], [3, 7]] } else { [[0, 4], [2, 6], [1, 5], [3, 7]] };
        for pair in order {
            for core in pair {
                let g = cal.s16(cal.gain_src(mode, core));
                if (-64..=63).contains(&g) {
                    self.gain[core] = (self.gain[core] & 0xff80) | (g as u16 & 0x7f);
                    spi(&mut w, 0, 0x15 + 8 * core as u32, self.gain[core], 80);
                }
            }
            for core in pair {
                let g1 = cal.s16(cal.gain_src(mode, core) + 0x10);
                if (-64..=63).contains(&g1) {
                    self.gain[core] = (self.gain[core] & 0xff80) | (g1 as u16 & 0x7f);
                }
            }
        }
        for core in 0..8 {
            let o = cal.s16(mode * REC + 2 * core);
            if (-1024..=1023).contains(&o) {
                self.off[core] = (self.off[core] & 0xf800) | (o as u16 & 0x7ff);
                spi(&mut w, 100, 0x14 + 8 * core as u32, self.off[core], 100);
            }
        }
        for core in 0..8 {
            let o1 = cal.s16(mode * REC + 2 * core + 0x10);
            if (-1024..=1023).contains(&o1) {
                self.off[core] = (self.off[core] & 0xf800) | (o1 as u16 & 0x7ff);
            }
        }
        for i in 0..4 {
            let d = cal.s16((if mode <= 3 { mode } else { 0 }) * REC + 0x40 + 2 * i) as u16 & 0xff;
            let s = if i < 2 { &mut self.r04 } else { &mut self.r05 };
            *s = if i % 2 == 0 { (*s & 0xff00) | d } else { (*s & 0x00ff) | d << 8 };
            let v = *s ^ 0x8080;
            spi(&mut w, 0, if i < 2 { 0x04 } else { 0x05 }, v, 100);
        }
        let r = if mode <= 3 { 4 + mode } else { 4 };
        let (a, b) = (cal.s16(r * REC + 0x42), cal.s16(r * REC + 0x40));
        self.r01 = (self.r01 & 0xe000) | (a as u16 & 7) << 10 | (b as u16 & 0x3ff);
        spi(&mut w, 0, 0x01, self.r01, 100);
        w
    }

    /// Input mode + parameters, for a change of the channel set.
    pub fn channel_mode(&mut self, cal: &CalAdc, mode: usize) -> Vec<Wr> {
        let mut w = Vec::new();
        self.input_mode(&mut w, mode);
        w.extend(self.parameter(cal, mode));
        w
    }

    /// Core gain register 0x15 + 8n (7-bit signed, -64..63).
    pub fn core_gain(&mut self, core: usize, g: i32) -> Vec<Wr> {
        let mut w = Vec::new();
        if (-64..=63).contains(&g) {
            self.gain[core] = (self.gain[core] & 0xff80) | (g as u16 & 0x7f);
            spi(&mut w, 0, 0x15 + 8 * core as u32, self.gain[core], 80);
        }
        w
    }

    /// Core offset register 0x14 + 8n (11-bit signed, -1024..1023; others not written).
    pub fn core_offset(&mut self, core: usize, o: i32) -> Vec<Wr> {
        let mut w = Vec::new();
        if (-1024..=1023).contains(&o) {
            self.off[core] = (self.off[core] & 0xf800) | (o as u16 & 0x7ff);
            spi(&mut w, 100, 0x14 + 8 * core as u32, self.off[core], 100);
        }
        w
    }

    /// Rewrite the input mode from the shadows (after a band-limit change).
    pub fn onoff(&self) -> Vec<Wr> {
        let mut w = Vec::new();
        spi(&mut w, 100, 0x54, self.swonp.0 ^ 0x1041, 0);
        spi(&mut w, 100, 0x55, self.swonp.1 ^ 0x0004, 100);
        w
    }
}

/// Read-path gain per enabled channel (x1 or x2): the 0x105c values written, in
/// channel order. `sh` = the current 0x105c shadow (0x5555 = all x1).
pub fn wpu_gain(mask: u32, gains: [u32; 4], mut sh: u32) -> Vec<(usize, u32)> {
    let r = crate::dho::hz::ratio(mask);
    let mut out = Vec::new();
    for ch in 0..4 {
        if mask >> ch & 1 == 0 {
            continue;
        }
        let g = gains[ch] & 3;
        if r == 1 {
            sh = (0..8).fold(0, |s, k| s | g << (2 * k));
        } else if r == 2 {
            let lower = (0..ch).all(|c| mask >> c & 1 == 0);
            let byte = g * 0x55;
            sh = if lower { (sh & 0xff00) | byte } else { (sh & 0x00ff) | byte << 8 };
        } else {
            let f: [u32; 2] = [[0, 2], [1, 3], [4, 6], [5, 7]][ch];
            for k in f {
                sh = (sh & !(3 << (2 * k))) | g << (2 * k);
            }
        }
        out.push((ch, sh & 0xffff));
    }
    out
}

/// ADC core averages (self-cal.md 2.4): starts the FPGA ADC measure unit (0x1040, averaging exponent 15,
/// start = bit 31), polls done (bit 30) every 10 ms up to 5 times, then reads the mean code of every core (0x1040[11:8]
/// = core, 0x1044; 0xeeeeeeee = not ready, read again once). The values are raw ADC codes before the read-path gain
/// (unit 2026-11-03: ~6970 codes per volt at 1 V/div, x1.077 digital gain = the 7500 codes per division shown).
pub fn measure(bus: &mut dyn crate::dho::bus::Bus, sleep_ms: &mut dyn FnMut(u32)) -> Option<[f64; 8]> {
    let cfg = (bus.rd(0x1040) & 0x77ff_f0e0) | 15;
    bus.wr(0x1040, cfg);
    bus.wr(0x1040, cfg | 0x8000_0000);
    let mut done = false;
    for _ in 0..5 {
        sleep_ms(10);
        if bus.rd(0x1040) & 0x4000_0000 != 0 {
            done = true;
            break;
        }
    }
    if !done {
        return None;
    }
    let sh = bus.rd(0x1040);
    let mut out = [0.0; 8];
    for (k, v) in out.iter_mut().enumerate() {
        bus.wr(0x1040, (sh & !0xf00) | (k as u32) << 8);
        let mut a = bus.rd(0x1044);
        if a == 0xeeee_eeee {
            sleep_ms(1);
            a = bus.rd(0x1044);
        }
        if a == 0xeeee_eeee {
            return None;
        }
        *v = (a & 0xffff) as f64;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAL_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal");

    #[test]
    fn modes_and_input_words() {
        assert_eq!(mode_of_mask(1), Some(0));
        assert_eq!(mode_of_mask(0xc), Some(9));
        assert_eq!(mode_of_mask(7), Some(10));
        assert_eq!(mode_of_mask(0), None);
        // the input-mode words agree with the table of horizontal-config.md (hal::dho::afe::adc_mode_words)
        for mask in 1..16 {
            let mut a = Adc::default();
            let mut w = Vec::new();
            a.input_mode(&mut w, mode_of_mask(mask).unwrap());
            assert_eq!([w[0].val, w[1].val], crate::dho::afe::adc_mode_words(mask), "mask {:#x}", mask);
        }
    }

    /// Identical to tools/adcmodel.py (testdata/adc_expected.txt) for boot, 1 -> 3 -> f -> c -> 1, input mode restore.
    /// Needs the unit's cal_adc.hex (device data, not in the tree); skipped when missing.
    #[test]
    fn matches_adcmodel() {
        let Ok(cal) = CalAdc::load(&std::env::var("OOS_CAL_DIR").unwrap_or(CAL_DIR.into())) else {
            eprintln!("cal_adc.hex not found, skipped");
            return;
        };
        let fmt = |w: &[Wr]| w.iter().map(|x| format!("{} {:#x} {:#x} {}", x.pre_us, x.reg, x.val, x.post_us)).collect::<Vec<_>>();
        let mut got = vec!["== boot".to_string()];
        let mut adc = Adc::default();
        got.extend(fmt(&adc.parameter(&cal, 0)));
        for m in [0x3, 0xf, 0xc, 0x1] {
            got.push(format!("== {:#x}", m));
            got.extend(fmt(&adc.channel_mode(&cal, mode_of_mask(m).unwrap())));
            got.push(format!("-- wpu {}", wpu_gain(m, [1; 4], 0x5555).iter().map(|(c, v)| format!("{}:{:#x}", c, v)).collect::<Vec<_>>().join(" ")));
        }
        got.push("== onoff".into());
        got.extend(fmt(&adc.onoff()));
        let want: Vec<&str> = include_str!("../../testdata/adc_expected.txt").lines().collect();
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want) {
            assert_eq!(g, w);
        }
    }
}
