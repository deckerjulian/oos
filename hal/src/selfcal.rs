// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! User self-calibration (docs/protocols/self-cal.md): AFE zero trims with the AFE's own comparators (section 3) and
//! the vertical calibration with the offset DAC as the reference against the ADC average (section 2), rebuilding the
//! 801-row scale table of a bandwidth block (2.5). The algorithms run against `CalHw`, so the
//! host tests drive them with a model of the AFE and ADC; on the unit `CalHw` applies the AFE ops over the GPIO SPI
//! bus and reads the FPGA ADC measure unit. Results are kept in memory; the caller decides whether they are written,
//! and only to OOS's own directory (`calfile::OOS_CAL_DIR`).
//!
//! Deviations from the stock system [A]: the DAC search measures both window ends first and bisects on the sign of the error
//! (stock uses a fixed direction per input-gain path); the input mode is taken from the r07 shadow.

use crate::afe::{Afe, Cal, Op, Rec};

/// What the calibration needs from the unit.
pub trait CalHw {
    /// AFE register writes and gear ioctls, in order.
    fn apply(&mut self, ops: &[Op]);
    /// AFE register read (reg 0 holds the comparator bits).
    fn afe_read(&mut self, ch: usize, reg: u8) -> u16;
    /// Mean ADC code of channel `ch` (65536 samples per core, the channel's two cores averaged).
    fn adc_mean(&mut self, ch: usize) -> Option<f64>;
    fn sleep_ms(&mut self, ms: u32);
    /// The user pressed Cancel.
    fn cancelled(&mut self) -> bool {
        false
    }
    /// The normal set-up of channel `ch` alone with bandwidth code `bw` (ADC mode "this channel only", AFE, filter and
    /// trims); returns the channel's AFE shadows.
    fn configure(&mut self, _ch: usize, _bw: u32) -> Result<Afe, String> {
        Err("configure not supported".into())
    }
    /// ADC SPI / FPGA writes built on the caller's ADC shadows (`f` returns the writes, `CalHw` applies them).
    fn adc_op(&mut self, _f: &mut dyn FnMut(&mut crate::adc::Adc) -> Vec<crate::adc::Wr>) {}
    /// Mean raw code of every measure slot (0x1044 per slot).
    fn adc_slots(&mut self) -> Option<[f64; 8]> {
        None
    }
    /// Use these ADC core trims for every later `configure` (after the ADC core calibration).
    fn use_cal_adc(&mut self, _c: crate::adc::CalAdc) {}
}

/// Bandwidth codes of the four vertical passes (filters S20M, 155 M, 315 M, (3, 3, 7)) -> block groups 0..3.
pub const PASS_BW: [u32; 4] = [crate::afe::bw::S20M, crate::afe::bw::M155, crate::afe::bw::M315, crate::afe::bw::M350];

/// Results of a self-calibration run (kept in memory until saved).
#[derive(Default)]
pub struct Results {
    /// new cal_adc payload (ADC core gains and offsets)
    pub adc: Option<crate::adc::CalAdc>,
    pub zero: Vec<(usize, [i32; 20])>,
    pub blocks: Vec<(usize, usize, Vec<Rec>)>,
    /// largest relative change of a measured scale against the active table
    pub worst: f64,
    pub measured: usize,
}

/// The whole user self-cal for `chans`: AFE zero (optional), then the four vertical passes per channel.
/// `progress(done, total, text)` after every step.
pub fn run_all(hw: &mut dyn CalHw, cal: &Cal, chans: &[usize], zero: bool, progress: &mut dyn FnMut(usize, usize, &str)) -> Result<Results, String> {
    run_all_with(hw, cal, None, chans, zero, progress)
}

/// `run_all` with the ADC core calibration first when `adc` (the active cal_adc) is given (the vertical
/// levels depend on the core gains).
pub fn run_all_with(hw: &mut dyn CalHw, cal: &Cal, adc: Option<&crate::adc::CalAdc>, chans: &[usize], zero: bool, progress: &mut dyn FnMut(usize, usize, &str)) -> Result<Results, String> {
    let mut adc_new = None;
    if let Some(base) = adc {
        let r = adc_cores(hw, base, chans, progress)?;
        // the vertical passes measure with the new trims
        hw.use_cal_adc(r.clone_payload());
        adc_new = Some(r);
    }
    let keys: Vec<i64> = (0..801).map(|i| cal.key(i)).collect();
    // the same configuration list for every pass (a table of 77); block group 3 is empty in factory files
    let list = |ch: usize| -> Vec<[i32; 7]> {
        let mut v: Vec<Entry> = Vec::new();
        for var in 0..4 {
            for e in entries_of(&cal.block(var, ch)) {
                if !v.iter().any(|x| x.g == e.g) {
                    v.push(e);
                }
            }
        }
        v.sort_by(|a, b| a.scale.partial_cmp(&b.scale).unwrap());
        v.into_iter().map(|e| e.g).collect()
    };
    let total: usize = chans.iter().map(|&ch| 4 * list(ch).len() + if zero { 3 } else { 0 }).sum();
    let mut done = 0;
    let mut r = Results::default();
    for &ch in chans {
        let mut zt = cal.zero_trims(ch);
        if zero {
            progress(done, total, &format!("CH{} AFE zero", ch + 1));
            let mut a = hw.configure(ch, PASS_BW[0])?;
            zt = afe_zero(hw, &mut a, zt)?;
            r.zero.push((ch, zt));
            done += 3;
        }
        for (var, &bw) in PASS_BW.iter().enumerate() {
            let mut a = hw.configure(ch, bw)?;
            let block = cal.block(var, ch);
            let table = entries_of(&block);
            let tuples = list(ch);
            let mut got = Vec::new();
            for (k, g) in tuples.iter().enumerate() {
                if hw.cancelled() {
                    return Err("cancelled".into());
                }
                progress(done, total, &format!("CH{} pass {} of 4, configuration {} of {}", ch + 1, var + 1, k + 1, tuples.len()));
                let m = vertical_entry(hw, &mut a, cal, *g, &zt).ok_or_else(|| format!("CH{} {:?}: no measurement", ch + 1, g))?;
                if let Some(e) = table.iter().find(|e| e.g == *g) {
                    r.worst = r.worst.max((m.scale / e.scale - 1.0).abs());
                }
                got.push(m);
                done += 1;
            }
            let mut nb = block.clone();
            rebuild_scales(&mut nb, &keys, &got);
            r.blocks.push((var, ch, nb));
            r.measured += got.len();
        }
    }
    progress(total, total, "done");
    r.adc = adc_new;
    Ok(r)
}

/// Writes the results to OOS's own files (the current OOS file, else the factory file, as the base for the rows and
/// channels not calibrated). Returns the files written.
pub fn save(r: &Results, rigol_dir: &str) -> Result<Vec<String>, String> {
    use crate::calfile::{self, OOS_CAL_DIR};
    let base = |n: &str| -> Result<Vec<u8>, String> {
        let (b, _) = calfile::load_first(n, &[OOS_CAL_DIR, rigol_dir]).ok_or(format!("no valid {}", n))?;
        Ok(calfile::parse(&b).map_err(|e| format!("{}: {:?}", n, e))?.to_vec())
    };
    let mut out = Vec::new();
    let stamp = calfile::now();
    if !r.blocks.is_empty() {
        let p = vertical_payload(&base("cal_vertical.hex")?, &r.blocks);
        calfile::save(OOS_CAL_DIR, "cal_vertical.hex", &p, stamp).map_err(|e| e.to_string())?;
        out.push(format!("{}/cal_vertical.hex", OOS_CAL_DIR));
    }
    if let Some(adc) = &r.adc {
        calfile::save(OOS_CAL_DIR, "cal_adc.hex", adc.payload(), stamp).map_err(|e| e.to_string())?;
        out.push(format!("{}/cal_adc.hex", OOS_CAL_DIR));
    }
    if !r.zero.is_empty() {
        let mut p = base("cal_afe_zero.hex")?;
        for (ch, z) in &r.zero {
            p = zero_payload(&p, *ch, z);
        }
        calfile::save(OOS_CAL_DIR, "cal_afe_zero.hex", &p, stamp).map_err(|e| e.to_string())?;
        out.push(format!("{}/cal_afe_zero.hex", OOS_CAL_DIR));
    }
    Ok(out)
}

// ---- ADC core gain & offset (self-cal.md 14) -------------------------------------------------------------------

/// Gain records calibrated and offset modes for a channel set (self-cal.md 14.7).
pub fn adc_plan(chans: &[usize]) -> (Vec<usize>, Vec<usize>) {
    let mut c: Vec<usize> = chans.iter().copied().filter(|&c| c < 4).collect();
    c.sort();
    c.dedup();
    let pair = |a: usize, b: usize| match (a, b) {
        (0, 1) => 4,
        (0, 2) => 5,
        (0, 3) => 6,
        (1, 2) => 7,
        (1, 3) => 8,
        _ => 9,
    };
    let mut modes = c.clone();
    for i in 0..c.len() {
        for j in i + 1..c.len() {
            modes.push(pair(c[i], c[j]));
        }
    }
    if c.len() == 4 {
        modes.push(10);
    }
    (c, modes)
}

/// Which measure slot shows core n (each core pushed to offset +1023 in turn; the slot that moves
/// by more than 7936 codes is its slot). The channel's DAC must centre the trace first.
pub fn core_order(hw: &mut dyn CalHw) -> Result<[usize; 8], String> {
    for n in 0..8 {
        hw.adc_op(&mut |a| a.core_offset(n, 0));
    }
    hw.sleep_ms(2);
    let base = hw.adc_slots().ok_or("ADC measure unit not answering")?;
    let mut order = [usize::MAX; 8];
    for (n, slot) in order.iter_mut().enumerate() {
        hw.adc_op(&mut |a| a.core_offset(n, 1023));
        hw.sleep_ms(2);
        let m = hw.adc_slots().ok_or("ADC measure unit not answering")?;
        for k in 0..8 {
            if (m[k] - base[k]).abs() > 7936.0 {
                *slot = k;
            }
        }
        hw.adc_op(&mut |a| a.core_offset(n, 0));
    }
    // every core must have exactly one slot of its own
    let mut seen = [false; 8];
    for &k in &order {
        if k > 7 || seen[k] {
            return Err(format!("ADC core order not found: {:?}", order));
        }
        seen[k] = true;
    }
    Ok(order)
}

fn slots_avg(hw: &mut dyn CalHw, n: usize) -> Option<[f64; 8]> {
    let mut acc = [0.0; 8];
    for _ in 0..n {
        let m = hw.adc_slots()?;
        for k in 0..8 {
            acc[k] += m[k] / n as f64;
        }
    }
    Some(acc)
}

/// Span per slot between the two DAC codes (10 averages each, 100 ms settle) and the mid-range.
fn spans(hw: &mut dyn CalHw, a: &mut Afe, hi: i32, lo: i32) -> Option<([f64; 8], f64)> {
    run(hw, a, |a, o| a.set_dac(o, hi));
    hw.sleep_ms(100);
    let h = slots_avg(hw, 10)?;
    run(hw, a, |a, o| a.set_dac(o, lo));
    hw.sleep_ms(100);
    let l = slots_avg(hw, 10)?;
    let s: [f64; 8] = std::array::from_fn(|k| h[k] - l[k]);
    let (mn, mx) = s.iter().fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    Some((s, (mn + mx) / 2.0))
}

/// The ADC core calibration (self-cal.md 14.5/14.6) on `base` (the active cal_adc payload): per channel of `chans`
/// the 8 core gains in single-channel mode, then the 8 core offsets of every ADC mode of the set. Needs every input
/// of the set disconnected. The fine gains (0x1058) are set to 0 and their payload fields cleared [A: OOS corrects
/// with the core gains only]. Returns the new payload.
pub fn adc_cores(hw: &mut dyn CalHw, base: &crate::adc::CalAdc, chans: &[usize], progress: &mut dyn FnMut(usize, usize, &str)) -> Result<crate::adc::CalAdc, String> {
    use crate::adc::CalAdc;
    const TUPLE: [i32; 7] = [0, 1, 1, 1, 0, 3, 0];
    let (gchans, modes) = adc_plan(chans);
    let mut cal = CalAdc::from_payload(base.payload().to_vec()).ok_or("bad cal_adc payload")?;
    let total = gchans.len() * 13 + modes.len();
    let mut done = 0;
    // every channel of the set at the cal tuple (the AFE keeps it while the ADC mode changes); offset DAC centred
    let mut afes: Vec<(usize, Afe)> = Vec::new();
    let mut centre = [32768i32; 4];
    for &c in &gchans {
        let mut a = hw.configure(c, crate::afe::bw::S20M)?;
        run(hw, &mut a, |a, o| {
            a.set_tuple(o, TUPLE);
            o.push(Op::Gear { ch: c, gear: 1 });
            a.set_dac(o, 32768);
        });
        afes.push((c, a));
    }
    hw.sleep_ms(100);
    let mut order = None;
    for (idx, &c) in gchans.iter().enumerate() {
        if hw.cancelled() {
            return Err("cancelled".into());
        }
        progress(done, total, &format!("ADC cores: CH{} gain", c + 1));
        // single-channel mode c with the stored parameters, then all trims 0
        let mode_words = cal.clone_payload();
        hw.adc_op(&mut |a| a.channel_mode(&mode_words, c));
        for n in 0..8 {
            hw.adc_op(&mut |a| {
                let mut w = a.core_offset(n, 0);
                w.extend(a.core_gain(n, 0));
                w
            });
            cal.set16(CalAdc::gain_at(c, n), 0);
        }
        hw.adc_op(&mut |_| vec![crate::adc::Wr { pre_us: 0, reg: 0x1058, val: 0, post_us: 0 }]);
        for k in 0..16 {
            cal.set32(CalAdc::fine_at(c, k.min(15)), 0);
        }
        let a = &mut afes[idx].1;
        if order.is_none() {
            let (cz, _) = dac_for(hw, a, 32768.0).ok_or("DAC search failed")?;
            run(hw, a, |a, o| a.set_dac(o, cz));
            hw.sleep_ms(20);
            order = Some(core_order(hw)?);
        }
        let ord = order.unwrap();
        let (h, _) = dac_for(hw, a, ADC_HI).ok_or("DAC search failed")?;
        let (l, _) = dac_for(hw, a, ADC_LO).ok_or("DAC search failed")?;
        let (_, rf) = spans(hw, a, h, l).ok_or("ADC measure unit not answering")?;
        let mut g = [0i32; 8];
        let mut converged = false;
        for _round in 0..12 {
            if hw.cancelled() {
                return Err("cancelled".into());
            }
            for n in 0..8 {
                let gn = g[n];
                hw.adc_op(&mut |a| a.core_gain(n, gn));
            }
            let (s, _) = spans(hw, a, h, l).ok_or("ADC measure unit not answering")?;
            let mut corrected = 0;
            for n in 0..8 {
                let k = ord[n];
                if ((s[k] - rf) / 16.0).abs() > 2.0 {
                    g[n] = ((g[n] as f64 - 2048.0 * (s[k] - rf) / rf).trunc() as i32).clamp(-64, 63);
                    corrected += 1;
                }
            }
            done += 1;
            progress(done, total, &format!("ADC cores: CH{} gain, {} cores off", c + 1, corrected));
            if corrected == 0 {
                converged = true;
                break;
            }
        }
        if !converged {
            return Err(format!("ADC gain of CH{} did not converge in 12 rounds", c + 1));
        }
        for n in 0..8 {
            cal.set16(CalAdc::gain_at(c, n), g[n]);
        }
        let (cz, _) = dac_for(hw, a, 32768.0).ok_or("DAC search failed")?;
        centre[c] = cz;
        done = (idx + 1) * 13;
    }
    let ord = order.ok_or("no channel")?;
    for (_, a) in afes.iter_mut() {
        let cz = centre[a.ch];
        run(hw, a, |a, o| a.set_dac(o, cz));
    }
    hw.sleep_ms(50);
    for &m in &modes {
        if hw.cancelled() {
            return Err("cancelled".into());
        }
        progress(done, total, &format!("ADC cores: offsets of mode {}", m));
        let words = cal.clone_payload();
        hw.adc_op(&mut |a| a.channel_mode(&words, m));
        let mut o = [0i32; 8];
        let mut converged = false;
        for _round in 0..20 {
            for n in 0..8 {
                let on = o[n];
                hw.adc_op(&mut |a| a.core_offset(n, on));
            }
            let mean = hw.adc_slots().ok_or("ADC measure unit not answering")?;
            let mut corrected = 0;
            for n in 0..8 {
                let e = (mean[ord[n]] - 32768.0) / 16.0;
                if e.abs() > 1.5 {
                    o[n] = (o[n] as f64 - e).trunc() as i32;
                    corrected += 1;
                }
            }
            if corrected == 0 {
                converged = true;
                break;
            }
            if o.iter().any(|x| !(-1024..=1023).contains(x)) {
                break;
            }
            hw.sleep_ms(50);
        }
        if !converged {
            return Err(format!("ADC offsets of mode {} did not converge: {:?}", m, o));
        }
        for n in 0..8 {
            cal.set16(CalAdc::off_at(m, n), o[n]);
        }
        done += 1;
    }
    progress(total, total, "ADC cores done");
    Ok(cal)
}

/// `CalHw` on the unit: AFE over the GPIO SPI bus, gear ioctl, FPGA ADC measure unit, the normal set-up through
/// `seq` for `configure`.
#[cfg(target_os = "linux")]
pub struct Unit<'a> {
    pub bar: &'a mut crate::bus::Bar1,
    pub afe: &'a mut crate::afe::GpioAfe,
    pub c2h: &'a mut std::fs::File,
    pub state: &'a mut crate::seq::State,
    pub cal: &'a Cal,
    pub cal_adc: Option<&'a crate::adc::CalAdc>,
    pub cancel: &'a dyn Fn() -> bool,
    /// ADC trims of this run (after the ADC core calibration), used instead of `cal_adc`
    pub adc_new: Option<crate::adc::CalAdc>,
}

#[cfg(target_os = "linux")]
impl CalHw for Unit<'_> {
    fn apply(&mut self, ops: &[Op]) {
        use crate::afe::AfeBus;
        use crate::bus::Bus;
        for op in ops {
            match *op {
                Op::Afe { ch, reg, val } => self.afe.write(ch, reg, val),
                Op::Gear { ch, gear } => {
                    let _ = crate::afe::set_gear(ch, gear);
                }
                Op::Adc(w) => {
                    std::thread::sleep(std::time::Duration::from_micros(100));
                    self.bar.wr(0x1034, w);
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }
            }
        }
    }
    fn afe_read(&mut self, ch: usize, reg: u8) -> u16 {
        use crate::afe::AfeBus;
        self.afe.read(ch, reg)
    }
    fn adc_mean(&mut self, _ch: usize) -> Option<f64> {
        // channel-only ADC mode: all 8 cores sample the channel [A: stock averages cores k and k+4]
        let m = crate::adc::measure(self.bar, &mut |ms| std::thread::sleep(std::time::Duration::from_millis(ms as u64)))?;
        Some(m.iter().sum::<f64>() / 8.0)
    }
    fn sleep_ms(&mut self, ms: u32) {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }
    fn cancelled(&mut self) -> bool {
        (self.cancel)()
    }
    fn adc_op(&mut self, f: &mut dyn FnMut(&mut crate::adc::Adc) -> Vec<crate::adc::Wr>) {
        use crate::bus::Bus;
        for w in f(&mut self.state.adc) {
            std::thread::sleep(std::time::Duration::from_micros(w.pre_us as u64));
            self.bar.wr(w.reg, w.val);
            std::thread::sleep(std::time::Duration::from_micros(w.post_us as u64));
        }
    }
    fn adc_slots(&mut self) -> Option<[f64; 8]> {
        crate::adc::measure(self.bar, &mut |ms| std::thread::sleep(std::time::Duration::from_millis(ms as u64)))
    }
    fn use_cal_adc(&mut self, c: crate::adc::CalAdc) {
        self.adc_new = Some(c);
    }
    fn configure(&mut self, ch: usize, bw: u32) -> Result<Afe, String> {
        let mut st = crate::seq::Settings { mask: 1 << ch, trig_src: ch as u32 + 1, afe: true, ..Default::default() };
        st.ch[ch].bw = bw;
        let adc = self.adc_new.as_ref().or(self.cal_adc);
        let (steps, _) = self.state.configure(&st, Some(self.cal), adc)?;
        crate::seq::execute(&steps, self.bar, Some(&mut *self.afe), self.c2h, |_| {})?;
        Ok(self.state.afe[ch].clone())
    }
}

fn run(hw: &mut dyn CalHw, a: &mut Afe, f: impl FnOnce(&mut Afe, &mut Vec<Op>)) {
    let mut o = Vec::new();
    f(a, &mut o);
    hw.apply(&o);
}

/// The binary search of every zero trim (self-cal.md 3.2): index x in 0..=hi from `start`, `steps` halvings; the
/// comparator reads 1 -> the upper bound moves to x, 0 -> the lower bound moves to x. The last x is written once more.
pub fn search(hw: &mut dyn CalHw, a: &mut Afe, hi: i32, start: i32, steps: u32, write: &mut dyn FnMut(&mut Afe, &mut Vec<Op>, i32), bit: u32) -> i32 {
    let ch = a.ch;
    let (mut lo, mut up, mut x) = (0, hi, start);
    for _ in 0..steps {
        run(hw, a, |a, o| write(a, o, x));
        hw.sleep_ms(10);
        // majority of 5 comparator reads (one read is too few: on the unit the high-gain paths scattered by +-20 trim
        // codes between runs with single reads)
        let ones = (0..5).filter(|_| hw.afe_read(ch, 0) >> bit & 1 == 1).count();
        if ones >= 3 {
            up = x;
        } else {
            lo = x;
        }
        x = (lo + up) / 2;
    }
    run(hw, a, |a, o| write(a, o, x));
    x
}

/// First stage: sign-magnitude trim from the search index 0..2047.
pub fn first_code(x: i32) -> (i32, i32) {
    if x < 1024 {
        (1, 1023 - x)
    } else {
        (0, x - 1024)
    }
}

/// VGA: 9-bit trim from the search index 0..224 (gaps of 41 codes every 23 steps above 63).
pub fn vga_code(x: i32) -> i32 {
    if x <= 63 {
        x
    } else {
        x + 41 * (1 + (x - 64) / 23)
    }
}

/// SGA: index < 128 trims the first value (r27[14:8]) = 127 - x, else the second (r27[6:0]) =
/// x - 128; the other one stays.
pub fn pn_code(x: i32, p: i32, n: i32) -> (i32, i32) {
    if x < 128 {
        (127 - x, n)
    } else {
        (p, x - 128)
    }
}

/// MUX: the other way round (unit 2026-11-03: the SGA mapping drove every MUX search to
/// the rail): index < 128 trims the second value (r06[14:8]) = 127 - x, else the first (r06[6:0]) = x - 128.
pub fn mux_code(x: i32, a: i32, b: i32) -> (i32, i32) {
    if x < 128 {
        (a, 127 - x)
    } else {
        (x - 128, b)
    }
}

/// The three AFE configurations of the zero calibration (IG, VGA, ATT, SGA, SGAR, MUX, MUXR).
pub const ZERO_CONFIGS: [[i32; 7]; 3] = [[0, 0, 2, 0, 0, 3, 1], [0, 1, 2, 1, 0, 2, 1], [1, 2, 2, 3, 0, 3, 0]];

fn zero_first(hw: &mut dyn CalHw, a: &mut Afe, mode: u32) -> (i32, i32) {
    run(hw, a, |a, o| a.cz_self_check(o, mode, true));
    let bit = if mode == 0x4000 { 2 } else { 1 };
    let x = search(hw, a, 2047, 1023, 14, &mut |a, o, x| {
        let (s, m) = first_code(x);
        a.cz_first(o, mode, s, m)
    }, bit);
    run(hw, a, |a, o| a.cz_self_check(o, mode, false));
    first_code(x)
}

fn zero_comparator(hw: &mut dyn CalHw, a: &mut Afe) {
    run(hw, a, |a, o| a.comp_self_check(o));
    search(hw, a, 127, 64, 7, &mut |a, o, x| a.comp_trim(o, x), 3);
}

fn zero_pn(hw: &mut dyn CalHw, a: &mut Afe, mux: bool) -> (i32, i32) {
    run(hw, a, |a, o| a.comp_input(o, mux));
    // P and N both start at 0 [A]: the first comparator decision picks the side, the other one stays 0, so the
    // result is (P, 0) or (0, N)
    let (mut p, mut n) = (0, 0);
    let code = |x: i32, p: i32, n: i32| if mux { mux_code(x, p, n) } else { pn_code(x, p, n) };
    let x = search(hw, a, 255, 127, 8, &mut |a, o, x| {
        (p, n) = code(x, p, n);
        if mux {
            a.cz_mux(o, p, n)
        } else {
            a.cz_sga(o, p, n)
        }
    }, 3);
    code(x, p, n)
}

/// AFE zero calibration of one channel (self-cal.md 3.1): the 20 ints of its `cal_afe_zero` record, starting from
/// `old` (slots a configuration does not reach keep their value).
pub fn afe_zero(hw: &mut dyn CalHw, a: &mut Afe, old: [i32; 20]) -> Result<[i32; 20], String> {
    let ch = a.ch;
    let mut z = old;
    run(hw, a, |a, o| {
        a.dac_enable(o, false);
        o.push(Op::Gear { ch, gear: 0 });
        a.cz_buf(o, 8, 8);
    });
    for cfg in ZERO_CONFIGS {
        if hw.cancelled() {
            return Err("cancelled".into());
        }
        run(hw, a, |a, o| a.set_tuple(o, cfg));
        hw.sleep_ms(1000);
        match a.input_mode() {
            0x4000 => {
                // VGA 25 (r29[9:8] = 0) fills z0..z6, VGA 2.5 fills z7..z13
                let base = if a.reg(0x29) >> 8 & 3 == 0 { 0 } else { 7 };
                let sel = base == 0;
                let (s, m) = zero_first(hw, a, 0x4000);
                (z[base], z[base + 1]) = (s, m);
                // VGA: the inverting first stage first, then the VGA trim
                let (s1, m1) = zero_first(hw, a, 0);
                (z[14], z[15]) = (s1, m1);
                run(hw, a, |a, o| {
                    a.cz_mode(o);
                    a.vga_output(o, sel);
                });
                let x = search(hw, a, 224, 112, 8, &mut |a, o, x| a.cz_vga(o, vga_code(x), sel), 1);
                z[base + 2] = vga_code(x);
                zero_comparator(hw, a);
                (z[base + 3], z[base + 4]) = zero_pn(hw, a, false);
                (z[base + 5], z[base + 6]) = zero_pn(hw, a, true);
            }
            0 => {
                (z[14], z[15]) = zero_first(hw, a, 0);
                zero_comparator(hw, a);
                (z[16], z[17]) = zero_pn(hw, a, false);
                (z[18], z[19]) = zero_pn(hw, a, true);
            }
            m => return Err(format!("CH{}: unexpected input mode {:#x}", ch + 1, m)),
        }
        hw.sleep_ms(1000);
    }
    run(hw, a, |a, o| {
        o.push(Op::Gear { ch, gear: 1 });
        a.dac_enable(o, true);
    });
    Ok(z)
}

/// ADC targets of the vertical calibration: +-3 divisions of 7500 codes around 32768, centre 32752.
pub const ADC_HI: f64 = 55268.0;
pub const ADC_LO: f64 = 10268.0;
pub const ADC_ZERO: f64 = 32752.0;

fn mean_stable(hw: &mut dyn CalHw, ch: usize) -> Option<f64> {
    // two consecutive means within 100 codes, 5 ms apart
    let mut prev = hw.adc_mean(ch)?;
    for _ in 0..10 {
        hw.sleep_ms(5);
        let m = hw.adc_mean(ch)?;
        if (m - prev).abs() <= 100.0 {
            return Some(m);
        }
        prev = m;
    }
    Some(prev)
}

/// The DAC code in 20000..40000 where the channel's ADC mean is within 16 codes of `target`
/// (bisection, 18 steps). Returns (code, mean).
pub fn dac_for(hw: &mut dyn CalHw, a: &mut Afe, target: f64) -> Option<(i32, f64)> {
    let ch = a.ch;
    let at = |hw: &mut dyn CalHw, a: &mut Afe, c: i32| -> Option<f64> {
        run(hw, a, |a, o| a.set_dac(o, c));
        // 5 ms is too short: on the unit the offset DAC still moved after 5 ms (2026-11-03): 30 ms
        hw.sleep_ms(30);
        mean_stable(hw, ch)
    };
    let (mut lo, mut hi) = (20000, 40000);
    let (flo, fhi) = (at(hw, a, lo)? - target, at(hw, a, hi)? - target);
    if flo.signum() == fhi.signum() {
        // the target is outside the window: the closer end
        return Some(if flo.abs() < fhi.abs() { (lo, flo + target) } else { (hi, fhi + target) });
    }
    let mut best = (lo, flo + target);
    for _ in 0..18 {
        let mid = (lo + hi) / 2;
        let f = at(hw, a, mid)? - target;
        best = (mid, f + target);
        if f.abs() <= 16.0 || hi - lo <= 1 {
            break;
        }
        if f.signum() == flo.signum() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(best)
}

/// One gain configuration of the vertical calibration (self-cal.md 2.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    /// IG, VGA, ATT, SGA, SGAR, MUX, MUXR
    pub g: [i32; 7],
    /// measured scale, nV/div
    pub scale: f64,
    /// offset-DAC code that puts the trace at ADC 32752
    pub zero: i32,
}

/// Measures one configuration: (scale, zero) from the two DAC codes for +-3 divisions; IG 0 paths verify the zero
/// within 32448..33088 (up to 20 rounds, the last result is kept).
pub fn vertical_entry(hw: &mut dyn CalHw, a: &mut Afe, cal: &Cal, g: [i32; 7], zero_trims: &[i32; 20]) -> Option<Entry> {
    let ch = a.ch;
    // the configuration exactly as normal operation sets it (incl. the bandwidth trims of the path) [A: with
    // the trims of the channel's current scale the unit showed up to 0.03 div of zero error]
    run(hw, a, |a, o| a.set_config(o, cal, g, zero_trims));
    hw.sleep_ms(1000);
    let lsb = cal.lsb(ch, g[0] as usize);
    let mut last = None;
    let debug = std::env::var("OOS_CAL_DEBUG").is_ok();
    for _ in 0..20 {
        let hi = dac_for(hw, a, ADC_HI);
        let lo = dac_for(hw, a, ADC_LO);
        if debug {
            eprintln!("selfcal {:?}: hi {:?} lo {:?}", g, hi, lo);
        }
        let ((c_hi, a_hi), (c_lo, a_lo)) = (hi?, lo?);
        if (a_hi - a_lo).abs() < 1.0 {
            return None;
        }
        let cpd = (c_hi - c_lo) as f64 / ((a_hi - a_lo) / 7500.0);
        let scale = (lsb * cpd).abs() * 1e9;
        let mut zero = c_hi - ((a_hi - ADC_ZERO) * (c_hi - c_lo) as f64 / (a_hi - a_lo)).trunc() as i32;
        // refine the zero on every path: the line through the
        // +-3 division points missed the centre by up to 0.06 div on the inverting paths of the unit (2026-11-03)
        let slope = (a_hi - a_lo) / (c_hi - c_lo) as f64;
        let mut ok = false;
        for _ in 0..4 {
            run(hw, a, |a, o| a.set_dac(o, zero));
            hw.sleep_ms(300);
            let m = mean_stable(hw, ch)?;
            if debug {
                eprintln!("selfcal {:?}: zero {} -> ADC {:.1}", g, zero, m);
            }
            // within 8 codes, or within 0.6 DAC steps where one step moves the ADC by more than that
            if (m - ADC_ZERO).abs() <= (0.6 * slope.abs()).max(8.0) || slope.abs() < 1e-6 {
                ok = true;
                break;
            }
            zero -= ((m - ADC_ZERO) / slope).round() as i32;
        }
        last = Some(Entry { g, scale, zero });
        if ok {
            break;
        }
    }
    last
}

/// Every row takes the configuration with the smallest measured scale above the row's key; digital
/// gain = scale / key, zero = its DAC code. Rows without a candidate keep their record.
pub fn rebuild_scales(block: &mut [Rec], keys: &[i64], entries: &[Entry]) {
    for (i, r) in block.iter_mut().enumerate() {
        let k = keys[i] as f64;
        let Some(e) = entries.iter().filter(|e| e.scale > k).min_by(|a, b| a.scale.partial_cmp(&b.scale).unwrap()) else { continue };
        r.key = keys[i];
        r.g[..7].copy_from_slice(&e.g);
        r.gain = e.scale / k;
        r.zero = e.zero;
    }
}

/// The configurations of a block as the factory table left them (tuple, scale = gain x key, zero), sorted by scale:
/// OOS takes the gain tuples from the unit's own file instead of shipping the vendor table.
pub fn entries_of(block: &[Rec]) -> Vec<Entry> {
    let mut v: Vec<Entry> = Vec::new();
    for r in block {
        let g: [i32; 7] = r.g[..7].try_into().unwrap();
        if r.key == 0 || v.iter().any(|e| e.g == g) {
            continue;
        }
        v.push(Entry { g, scale: r.gain * r.key as f64, zero: r.zero });
    }
    v.sort_by(|a, b| a.scale.partial_cmp(&b.scale).unwrap());
    v
}

/// The 56-byte record layout of cal_vertical.hex.
pub fn rec_bytes(r: &Rec) -> [u8; 56] {
    let mut b = [0u8; 56];
    b[0..8].copy_from_slice(&r.key.to_le_bytes());
    for (k, g) in r.g.iter().enumerate() {
        b[8 + 4 * k..12 + 4 * k].copy_from_slice(&g.to_le_bytes());
    }
    b[40..48].copy_from_slice(&r.gain.to_le_bytes());
    b[48..52].copy_from_slice(&r.zero.to_le_bytes());
    b[52..56].copy_from_slice(&r.last.to_le_bytes());
    b
}

/// cal_vertical payload with some blocks replaced: `base` = the current payload (16 blocks x 801 x 56 bytes).
pub fn vertical_payload(base: &[u8], blocks: &[(usize, usize, Vec<Rec>)]) -> Vec<u8> {
    let mut p = base.to_vec();
    for (var, ch, recs) in blocks {
        for (i, r) in recs.iter().enumerate().take(801) {
            let o = ((var * 4 + ch) * 801 + i) * 56;
            p[o..o + 56].copy_from_slice(&rec_bytes(r));
        }
    }
    p
}

/// cal_afe_zero payload (4 x 20 s32) with channel `ch` replaced.
pub fn zero_payload(base: &[u8], ch: usize, z: &[i32; 20]) -> Vec<u8> {
    let mut p = base.to_vec();
    for (k, v) in z.iter().enumerate() {
        let o = (ch * 20 + k) * 4;
        p[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_cal() -> Option<(Cal, Vec<u8>)> {
        let dir = std::env::var("OOS_CAL_DIR").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal-full").into());
        let v = std::fs::read(format!("{}/cal_vertical.hex", dir)).ok()?;
        Some((Cal::load(&dir).ok()?, v))
    }

    /// A model of the AFE comparators and the ADC: every trim has a hidden threshold (the comparator reads 1 above
    /// it); the ADC mean is linear in the offset DAC code with a slope from the gain path.
    struct Model {
        regs: [u16; 0x40],
        last: u8,
        thr: std::collections::HashMap<u8, i32>,
        /// ADC = 32752 + slope * (dac - zero)
        slope: f64,
        zero: f64,
        writes: usize,
    }

    impl Model {
        fn new(slope: f64, zero: f64) -> Model {
            let thr = [(0x23, 1500), (0x26, 600), (0x35, 70), (0x29, 70), (0x07, 40), (0x27, 180), (0x06, 30)].into_iter().collect();
            Model { regs: [0; 0x40], last: 0, thr, slope, zero, writes: 0 }
        }
        /// The search index the last trim write stands for.
        fn index(&self) -> i32 {
            let r = self.regs[self.last as usize] as i32;
            match self.last {
                0x23 | 0x26 => if r >> 10 & 1 == 1 { 1023 - (r & 0x3ff) } else { 1024 + (r & 0x3ff) },
                0x35 => (0..=224).find(|&x| vga_code(x) == (r & 0x1ff)).unwrap_or(0),
                0x07 => r & 0x7f,
                // net offset of the P/N pair on the search index scale: 127 - P + N
                0x27 => 127 - (r >> 8 & 0x7f) + (r & 0x7f),
                0x06 => 127 - (r >> 8 & 0x7f) + (r & 0x7f),
                _ => 0,
            }
        }
    }

    impl CalHw for Model {
        fn apply(&mut self, ops: &[Op]) {
            for op in ops {
                if let Op::Afe { reg, val, .. } = *op {
                    self.regs[reg as usize] = val;
                    if self.thr.contains_key(&reg) {
                        self.last = reg;
                    }
                    self.writes += 1;
                }
            }
        }
        fn afe_read(&mut self, _ch: usize, _reg: u8) -> u16 {
            let above = self.thr.get(&self.last).is_some_and(|&t| self.index() > t);
            if above { 0b1110 } else { 0 }
        }
        fn adc_mean(&mut self, _ch: usize) -> Option<f64> {
            Some((ADC_ZERO + self.slope * (self.regs[0x28] as f64 - self.zero)).clamp(0.0, 65535.0))
        }
        fn sleep_ms(&mut self, _ms: u32) {}
        fn configure(&mut self, ch: usize, _bw: u32) -> Result<Afe, String> {
            Ok(Afe::new(ch))
        }
    }

    #[test]
    fn code_maps() {
        assert_eq!((first_code(0), first_code(1023), first_code(1024), first_code(2047)), ((1, 1023), (1, 0), (0, 0), (0, 1023)));
        assert_eq!((vga_code(63), vga_code(64), vga_code(86), vga_code(87), vga_code(224)), (63, 105, 127, 169, 511));
        assert_eq!(pn_code(0, 5, 6), (127, 6));
        assert_eq!(pn_code(200, 5, 6), (5, 72));
        assert_eq!((mux_code(99, 5, 6), mux_code(200, 5, 6)), ((5, 28), (72, 6)));
    }

    #[test]
    fn searches_find_the_comparator_thresholds() {
        let mut m = Model::new(1.0, 32768.0);
        let mut a = Afe::new(0);
        let x = search(&mut m, &mut a, 127, 64, 7, &mut |a, o, x| a.comp_trim(o, x), 3);
        assert!((x - 40).abs() <= 1, "comparator trim {}", x);
        // the full zero cal: every slot lands at its threshold (in trim units)
        let z = afe_zero(&mut m, &mut Afe::new(0), [0; 20]).unwrap();
        let (s, mg) = first_code(1500);
        assert!(z[0] == s && (z[1] - mg).abs() <= 1, "first {:?}", &z[..2]);
        let (s, mg) = first_code(600);
        assert!(z[14] == s && (z[15] - mg).abs() <= 1, "first inv {:?}", &z[14..16]);
        assert!((z[2] - vga_code(70)).abs() <= 1, "vga {}", z[2]);
        // SGA threshold 180 >= 128: (0, ~52); MUX threshold 30 < 128: (0, ~97) with the swapped MUX mapping
        assert!(z[3] == 0 && (z[4] - 52).abs() <= 1 && z[5] == 0 && (z[6] - 97).abs() <= 1, "sga/mux {:?}", &z[3..7]);
        assert_eq!(m.regs[0x28], 0, "the offset DAC is not touched");
    }

    #[test]
    fn vertical_entry_measures_scale_and_zero() {
        let Some((cal, _)) = unit_cal() else { return };
        // IG 0 path with 7500 ADC codes per division = 300 DAC codes per division
        let mut m = Model::new(25.0, 31000.0);
        let mut a = Afe::new(0);
        let e = vertical_entry(&mut m, &mut a, &cal, [0, 1, 2, 1, 0, 2, 1], &cal.zero_trims(0)).unwrap();
        let want = (cal.lsb(0, 0) * 300.0).abs() * 1e9;
        assert!((e.scale / want - 1.0).abs() < 2e-3, "scale {} want {}", e.scale, want);
        assert!((e.zero - 31000).abs() <= 1, "zero {}", e.zero);
        // inverting path (negative slope) works the same
        let mut m = Model::new(-25.0, 33000.0);
        let e = vertical_entry(&mut m, &mut Afe::new(0), &cal, [1, 2, 2, 3, 0, 3, 0], &cal.zero_trims(0)).unwrap();
        assert!((e.zero - 33000).abs() <= 1, "inverting zero {}", e.zero);
    }

    #[test]
    fn table_rebuild_reproduces_the_unit_file() {
        let Some((cal, _)) = unit_cal() else { return };
        let keys: Vec<i64> = (0..801).map(|i| cal.key(i)).collect();
        for var in 0..3 {
            for ch in 0..4 {
                let block = cal.block(var, ch);
                let entries = entries_of(&block);
                assert!(entries.len() >= 60, "block {}/{}: {} configurations", var, ch, entries.len());
                let mut fresh = vec![Rec { key: 0, g: [0; 8], gain: 0.0, zero: 0, last: 0 }; 801];
                rebuild_scales(&mut fresh, &keys, &entries);
                for i in 0..801 {
                    let (a, b) = (&fresh[i], &block[i]);
                    assert_eq!((a.key, &a.g[..7], a.zero), (b.key, &b.g[..7], b.zero), "block {}/{} row {}", var, ch, i);
                    assert!((a.gain - b.gain).abs() < 1e-9 * b.gain.max(1.0), "block {}/{} row {}: {} vs {}", var, ch, i, a.gain, b.gain);
                }
            }
        }
    }

    #[test]
    fn run_all_covers_every_pass() {
        let Some((cal, _)) = unit_cal() else { return };
        let mut m = Model::new(25.0, 31000.0);
        let mut steps = 0;
        let r = run_all(&mut m, &cal, &[1], true, &mut |d, t, _| {
            assert!(d <= t);
            steps += 1;
        })
        .unwrap();
        assert_eq!((r.zero.len(), r.blocks.len()), (1, 4));
        assert!(r.measured >= 4 * 70, "{} configurations", r.measured);
        assert!(steps > r.measured);
        // the model has one DAC slope for every path: most rows take a measured configuration (zero 31000); the rows
        // above the largest measured scale keep their record
        for (var, ch, b) in &r.blocks {
            let fresh = b.iter().filter(|x| (x.zero - 31000).abs() <= 1).count();
            assert!(*ch == 1 && fresh > 500, "block {}: {} rows measured", var, fresh);
        }
    }

    /// Eight interleaved cores with gain and offset errors behind a permuted slot order; the ADC SPI words are decoded
    /// like the chip does (gain reg 0x15 + 8n 7-bit signed, 1/2048 per LSB; offset reg 0x14 + 8n 11-bit signed, one
    /// 12-bit LSB = 16 codes).
    struct Cores {
        dac: f64,
        order: [usize; 8],
        gain_err: [f64; 8],
        off_err: [f64; 8],
        g: [i32; 8],
        o: [i32; 8],
        adc: crate::adc::Adc,
    }

    impl CalHw for Cores {
        fn apply(&mut self, ops: &[Op]) {
            for op in ops {
                if let Op::Afe { reg: 0x28, val, .. } = *op {
                    self.dac = val as f64;
                }
            }
        }
        fn afe_read(&mut self, _: usize, _: u8) -> u16 {
            0
        }
        fn adc_mean(&mut self, _: usize) -> Option<f64> {
            self.adc_slots().map(|m| m.iter().sum::<f64>() / 8.0)
        }
        fn sleep_ms(&mut self, _: u32) {}
        fn configure(&mut self, ch: usize, _: u32) -> Result<Afe, String> {
            Ok(Afe::new(ch))
        }
        fn adc_op(&mut self, f: &mut dyn FnMut(&mut crate::adc::Adc) -> Vec<crate::adc::Wr>) {
            for w in f(&mut self.adc) {
                if w.reg != 0x1034 {
                    continue;
                }
                let (addr, data) = ((w.val >> 16) & 0x7f, (w.val & 0xffff) as u16);
                if (0x14..0x54).contains(&addr) {
                    let n = ((addr - 0x14) / 8) as usize;
                    match (addr - 0x14) % 8 {
                        0 => self.o[n] = ((data << 5) as i16 >> 5) as i32,
                        1 => self.g[n] = (((data & 0x7f) << 9) as i16 >> 9) as i32,
                        _ => {}
                    }
                }
            }
        }
        fn adc_slots(&mut self) -> Option<[f64; 8]> {
            let mut m = [0.0; 8];
            for n in 0..8 {
                // 25 ADC codes per DAC code around 32760, per-core gain and offset
                let v = 32768.0 + (self.dac - 32760.0) * 25.0 * (1.0 + self.gain_err[n] + self.g[n] as f64 / 2048.0) + 16.0 * (self.off_err[n] + self.o[n] as f64);
                m[self.order[n]] = v.clamp(0.0, 65520.0);
            }
            Some(m)
        }
    }

    #[test]
    fn adc_cores_converge_on_a_permuted_core_model() {
        let Some(cal) = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal-full/cal_adc.hex")).ok().and_then(|d| crate::adc::CalAdc::from_file_bytes(&d)) else { return };
        let mut hw = Cores {
            dac: 32768.0,
            order: [0, 2, 1, 3, 4, 6, 5, 7],
            gain_err: [0.004, -0.006, 0.010, 0.0, -0.012, 0.003, 0.007, -0.002],
            off_err: [9.0, -14.0, 30.0, -3.0, 0.0, 22.0, -40.0, 5.0],
            g: [0; 8],
            o: [0; 8],
            adc: crate::adc::Adc::default(),
        };
        assert_eq!(core_order(&mut hw).unwrap(), [0, 2, 1, 3, 4, 6, 5, 7]);
        let out = adc_cores(&mut hw, &cal, &[1], &mut |_, _, _| {}).unwrap();
        // gains: every core's effective gain within 0.1 % of the others; offsets: within 1.5 LSB of mid-scale
        let eff: Vec<f64> = (0..8).map(|n| hw.gain_err[n] + out.get16(crate::adc::CalAdc::gain_at(1, n)) as f64 / 2048.0).collect();
        let (mn, mx) = eff.iter().fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        assert!(mx - mn < 0.002, "gain spread {:?}", eff);
        // offsets centre every core on absolute mid-scale (incl. the channel's rest error) within 1.5 LSB
        let m = hw.adc_slots().unwrap();
        assert!(m.iter().all(|x| (x - 32768.0).abs() <= 24.0), "slots {:?}", m);
        for n in 0..8 {
            assert_eq!(out.get16(crate::adc::CalAdc::off_at(1, n)), hw.o[n]);
        }
        // the plan for one channel: gain record 1, offset mode 1; for all four: 4 gains, all 11 modes
        assert_eq!(adc_plan(&[1]), (vec![1], vec![1]));
        assert_eq!(adc_plan(&[0, 1, 2, 3]).1.len(), 11);
        assert_eq!(adc_plan(&[0, 2]).1, vec![0, 2, 5]);
    }

    #[test]
    fn payloads_round_trip() {
        let Some((cal, v)) = unit_cal() else { return };
        let base = crate::calfile::parse(&v).unwrap();
        let b = cal.block(1, 2);
        assert_eq!(vertical_payload(base, &[(1, 2, b)]), base);
        let z = cal.zero_trims(3);
        let zb = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal-full/cal_afe_zero.hex")).unwrap();
        let zp = crate::calfile::parse(&zb).unwrap();
        assert_eq!(zero_payload(zp, 3, &z), zp);
    }
}
