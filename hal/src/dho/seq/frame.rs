// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The read-out geometry: how many frames a batch or a segmented recording has, how many bytes a frame and a read
//! take (window, header, stream padding, FPGA compression), and dropping the padding again. Pure arithmetic on
//! `Settings` and `hz::Params`, no steps; a new size or limit of the read-out goes here, the steps that use it into
//! `State` in seq.rs. The read-out window, the padding and the compression are described in docs/protocols/.

use super::Settings;
use crate::dho::hz;

/// Words (all channels) of the stamp play: 128, the smallest window of the stock raw export (record-play.md 7.1: a
/// TX length of 8 points becomes 128 / ratio).
pub const STAMP_WORDS: u32 = 128;
/// Bytes of the stamp play's read: the 16-byte frame header and `STAMP_WORDS` words.
pub const STAMP_BYTES: usize = 16 + 2 * STAMP_WORDS as usize;

/// Frames of a hardware segmented recording: `Segments::n`, at least 1, at most `hz::max_record_frames`; 0 without
/// segments.
pub fn record_frames(s: &Settings, p: &hz::Params) -> u32 {
    s.segments.map_or(0, |g| g.n.clamp(1, hz::max_record_frames(p.seg)))
}

/// Frames of one batch: `Settings::frames`, at least 1, at most the frame ring (`max_frames`); 1 for play-last.
pub fn frames(s: &Settings, p: &hz::Params) -> u32 {
    if !s.play4 || s.la.is_some() {
        return 1;
    }
    s.frames.clamp(1, p.max_frames.max(1))
}

/// Bytes one frame of `frame` bytes takes in the C2H stream: the FPGA pads every frame of a play-out to 8 bytes (4
/// points at ratio 1). Unit 2026-11-04, 10 MHz sine, batches of 4..6 frames: 1250 points (100 ns/div) come at a
/// stride of 1252, 625 points at 628 (the phase is the same in every frame at that stride and walks by 2 / 3 samples a
/// frame at the unpadded one); 2500 points need no padding. Reading the unpadded length shifted every later frame
/// and left the padding in the stream for the next read.
pub fn frame_stride(frame: usize) -> usize {
    frame.div_ceil(8) * 8
}

/// Total bytes of a read of `frames` frames (`frame_stride` each).
pub fn read_len(frames: usize, frame: usize) -> usize {
    frames * frame_stride(frame)
}

/// Bytes of one frame in the read: the window of all interleaved channels (+ the 16-byte header in header mode).
pub fn frame_bytes(s: &Settings, p: &hz::Params) -> usize {
    let n = if s.compress > 1 && s.play4 { compressed_len(s, p) } else { window(s, p).0 };
    2 * (p.ratio * n) as usize + if s.header { 16 } else { 0 }
}

/// Largest compression factor: 0x1170 holds k in bits 13:0, a larger value wraps (assumed).
pub const COMPRESS_K_MAX: u32 = 16383;

/// The compression factor `s.compress` as it is written: 1..=`COMPRESS_K_MAX`, and at most memsel / 8, so a read has at
/// least 8 points.
pub fn compress_k(s: &Settings, p: &hz::Params) -> u32 {
    s.compress.clamp(1, COMPRESS_K_MAX).min((p.memsel / 8).max(1))
}

/// The 0x1170 word for factor `k`: k > 1 enables the min/max path with k in bits 13:0; k <= 1 is the pass-through
/// word. Enable with k = 0 stalls the read (no C2H word at all, assumed), so it is never written.
pub fn compress_word(k: u32) -> u32 {
    match k.min(COMPRESS_K_MAX) {
        0 | 1 => 0x4000_0001,
        k => 0xc000_0000 | k,
    }
}

/// Points per channel after the FPGA compression (`compress_k`): the most whole multiples of 8 points (so whole min/max
/// pairs) whose samples lie inside the record, points x k <= memsel. The FPGA delivers exactly 0x11f0 points and waits
/// for samples until it has them: asking for more than the window holds stalls the read. The stock rounding up to 8
/// asked for 3912 points at k = 256 at 1 M (1 001 472 samples) and the c2h read timed out on the unit, while k = 128
/// (7816 points, 1 000 448 samples) still fitted the 0x248 samples the window reads beyond the record (unitlab round 2).
pub fn compressed_len(s: &Settings, p: &hz::Params) -> u32 {
    let k = compress_k(s, p);
    (p.memsel / k / 8 * 8).max(8)
}

/// Drops the stream padding behind every frame (`frame_stride`) in place: the frames end up back to back.
pub fn unpad(buf: &mut Vec<u8>, frame: usize) {
    let n = unpad_in(buf, frame);
    buf.truncate(n);
}

/// `unpad` on a slice: returns the bytes left at the front. Frames without padding (all live set-ups of oos-ui) cost
/// nothing.
pub fn unpad_in(buf: &mut [u8], frame: usize) -> usize {
    let stride = frame_stride(frame);
    if frame == 0 || stride == frame {
        return buf.len();
    }
    let n = buf.len() / stride;
    for k in 1..n {
        buf.copy_within(k * stride..k * stride + frame, k * frame);
    }
    n * frame
}

/// (points per channel, first memory point) of the read-out window.
pub fn window(s: &Settings, p: &hz::Params) -> (u32, u32) {
    match s.window {
        Some((n, at)) => {
            let n = n.min(p.memsel);
            (n, (p.pre as i64 + at).clamp(0, (p.memsel - n) as i64) as u32)
        }
        None => (p.memsel, 0),
    }
}
