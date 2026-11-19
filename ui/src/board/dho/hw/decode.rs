// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Read-outs into frames: the header log, the decode of a whole record into volts (with and without matched
//! cores), the frame fields without the decode, the read-out buffers lent to the deposit workers and the LA
//! record. A new read-out format is decoded here.

use super::units::header_probe;
use crate::board::Frame;
use crate::model::*;
use oos_hal::dho::seq::Settings;
use oos_hal::dho::{afe, hz};
use std::sync::Arc;

/// One line of the header log: the 8 header words, `pre`, and the offsets (samples) of the nearest rising and falling
/// crossing of the trigger level from `pre` on the trigger channel. With OOS_FRAME_HEADER=off only the frames whose
/// edge (of the trigger slope) is not within 20 samples of `pre`, and a count of all frames every 10,000: a long run
/// for rare frames such as issue #5's.
fn log_header(h: &[u8], v: &[f32], s: &Settings, pre: usize, k: usize) {
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
    static LINES: AtomicU32 = AtomicU32::new(0);
    static FRAMES: AtomicU64 = AtomicU64::new(0);
    let words: Vec<String> = h.as_chunks::<2>().0.iter().map(|&b| format!("{:04x}", u16::from_le_bytes(b))).collect();
    let tc = (s.trig_src.clamp(1, 4) - 1) as usize;
    let hyst = (0.3 * s.ch[tc].scale) as f32;
    let span = v.len();
    let near = |rising: bool| {
        crate::fastdraw::edge_near(v, pre, s.level as f32, hyst, rising, false, span).map(|t| t - pre as f64)
    };
    let off = |rising: bool| near(rising).map_or("-".to_string(), |t| format!("{:.1}", t));
    let only_off = std::env::var_os("OOS_FRAME_HEADER").is_some_and(|x| x == "off");
    let mut line = String::new();
    if only_off {
        let n = FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
        if n.is_multiple_of(10_000) {
            line = format!("frames {}\n", n);
        }
        let rising = !matches!(s.slope, oos_hal::dho::tpu::Slope::Falling);
        if near(rising).is_some_and(|t| t.abs() <= 20.0) {
            if !line.is_empty() {
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/oos-hdr.log") {
                    let _ = f.write_all(line.as_bytes());
                }
            }
            return;
        }
    }
    if LINES.fetch_add(1, Ordering::Relaxed) >= 20_000 {
        return;
    }
    line += &format!(
        "{} pre {} k {} n {} rise {} fall {} scale {} tdiv {:e}\n",
        words.join(" "),
        pre,
        k,
        v.len(),
        off(true),
        off(false),
        s.ch[tc].scale,
        s.tdiv
    );
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/oos-hdr.log") {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Decodes a read-out of the whole record into a frame.
pub fn decode(bytes: &[u8], s: &Settings, p: &hz::Params, seq: u64) -> Frame {
    decode_with(bytes, s, p, seq, None)
}

/// `decode`, with the offsets of the ADC cores taken off the words of every sample (not of min/max pairs: Peak, FPGA
/// compression) when `cores` is given.
pub fn decode_with(
    bytes: &[u8],
    s: &Settings,
    p: &hz::Params,
    seq: u64,
    cores: Option<&mut super::cores::Cores>,
) -> Frame {
    let mut f = Frame::default();
    decode_into_with(bytes, s, p, seq, &mut f, cores);
    f
}

/// `decode` into a kept frame: its channel vectors are refilled (no allocation once they have room), every other
/// field is set as `decode` sets it.
pub fn decode_into(bytes: &[u8], s: &Settings, p: &hz::Params, seq: u64, f: &mut Frame) {
    decode_into_with(bytes, s, p, seq, f, None)
}

/// `decode_into` with the ADC cores matched (`decode_with`). The cores' offsets are taken off the words before they
/// become volts, so that path de-interleaves first; without them the fused decode (`seq::decode_volts_into`) runs.
pub fn decode_into_with(
    bytes: &[u8],
    s: &Settings,
    p: &hz::Params,
    seq: u64,
    f: &mut Frame,
    cores: Option<&mut super::cores::Cores>,
) {
    let (hdr, bytes) = if s.header && bytes.len() >= 16 { bytes.split_at(16) } else { (&bytes[..0], bytes) };
    match cores.filter(|_| s.mode == oos_hal::dho::acq::Mode::Normal && s.compress <= 1) {
        Some(cores) => {
            let mut w = oos_hal::dho::seq::deinterleave(bytes, s.mask);
            for (c, w) in w.iter_mut().enumerate() {
                if !w.is_empty() {
                    cores.apply((s.mask, c, p.rate), w);
                }
            }
            f.ch = std::array::from_fn(|c| {
                if w[c].is_empty() {
                    Vec::new()
                } else {
                    oos_hal::dho::seq::volts(&w[c], s.ch[c].scale, s.ch[c].offset)
                }
            });
        }
        None => oos_hal::dho::seq::decode_volts_into(bytes, s.mask, s.ch.map(|k| (k.scale, k.offset)), &mut f.ch),
    }
    // with FPGA compression every point stands for k samples (min/max pairs, acquisition-start.md)
    let k = oos_hal::dho::seq::compress_k(s, p) as usize;
    if !hdr.is_empty() && header_probe() {
        let tc = (s.trig_src.clamp(1, 4) - 1) as usize;
        log_header(hdr, &f.ch[tc], s, p.pre as usize / k, k);
    }
    set_meta(s, p, seq, f);
}

/// Every field of a decoded frame but the channel vectors (kept as they are).
fn set_meta(s: &Settings, p: &hz::Params, seq: u64, f: &mut Frame) {
    let k = oos_hal::dho::seq::compress_k(s, p) as usize;
    // a window of the record (`batch_plan`): the trigger point counts from its first point
    let (_, start) = oos_hal::dho::seq::window(s, p);
    let pre = p.pre.saturating_sub(start) as usize / k;
    let ch = std::mem::take(&mut f.ch);
    *f = Frame {
        seq,
        dt: k as f64 / p.rate as f64,
        pre,
        ch,
        roll: false,
        envelope: k > 1,
        scan: false,
        partial: false,
        acq: (p.rate as f64, p.memsel as usize),
        stamp: 0,
        la: None,
        hw_avg: false,
        fixed_ti: None,
        basic: Default::default(),
    };
}

/// `decode_into` without the decode (drawpipe::draw_frame_codes): the frame's fields as the decode sets them, the
/// channel vectors emptied, and where the read-out holds each channel. None with compression or a frame header (the
/// decode only).
pub fn codes_into(
    bytes: &[u8],
    s: &Settings,
    p: &hz::Params,
    seq: u64,
    f: &mut Frame,
) -> Option<crate::drawpipe::CodeLayout> {
    if s.header || oos_hal::dho::seq::compress_k(s, p) > 1 || bytes.is_empty() {
        return None;
    }
    for v in f.ch.iter_mut() {
        v.clear();
    }
    set_meta(s, p, seq, f);
    let m = oos_hal::dho::seq::code_map(s.mask, s.ch.map(|k| (k.scale, k.offset)));
    Some(crate::drawpipe::CodeLayout {
        stride: m.ratio,
        word: m.word,
        affine: m.affine,
        volt: oos_hal::dho::seq::volt_code,
        volts: oos_hal::dho::seq::lane_volts_into,
    })
}

/// Read-out buffers of the batch mode: one being read, two with the deposit workers (each holds one share and has room
/// for one more).
#[cfg(any(target_os = "linux", test))]
pub(super) const POOL: usize = 3;

/// The read-out buffer lent to the deposit workers as it is (drawpipe.rs).
impl crate::drawpipe::RawBatch for oos_hal::dho::bus::DmaBuf {
    fn bytes(&self) -> &[u8] {
        self.data()
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any + Send> {
        self
    }
}

/// The LA words of the acquisition `p` (read with `seq::acquire_la`) as a record: word j of the window at
/// (start + j - trigger index) / LA rate from the hardware trigger point (la.md 3.3; the trigger index is [A] until
/// test T1).
pub fn la_record(words: Vec<u16>, s: &Settings, p: &hz::Params) -> crate::logic::LaRec {
    let t = oos_hal::dho::la::timing(p, s.mask);
    let (_, start) = oos_hal::dho::la::window(&t);
    let dt = 1.0 / t.rate.max(1) as f64;
    crate::logic::LaRec { words: Arc::new(words), t0: (start as f64 - t.pre as f64) * dt, dt }
}

/// hal bandwidth code of a BWS entry.
pub fn bw_code(i: usize) -> u32 {
    match i {
        0 => afe::bw::AUTO,
        BW_FULL => afe::bw::OFF,
        BW_20M => afe::bw::S20M,
        3 => afe::bw::GND,
        _ => afe::bw::mhz(BWS[i.min(BWS.len() - 1)].1),
    }
}

/// Points of a live frame read from a record above 1 M points (the stock live length at 2.5 M / k = 4).
pub(super) const LIVE_READ: u32 = 625_000;
