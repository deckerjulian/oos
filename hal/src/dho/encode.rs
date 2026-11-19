// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The way back of the read-out, for the simulated unit (ADR 0053, docs/board-on-a-host.md 3.4): volts to read-out
//! words, the channels interleaved, the frame header, the stream padding and a chunk of the roll stream, each the
//! inverse of a decode of `seq` or `roll`, held to it by a round-trip test. The formats are those of
//! docs/protocols/: the words in afe-settings.md 4.1 and horizontal-config.md 2.2, the frame header in trigger.md
//! ("Frame header"), the padding in acquisition-start.md ("C2H read rules found on the unit"), the chunk in
//! roll-mode.md 8 and la.md 4.4. What the unit sends where nothing reads it (the padding, the empty slot of three
//! channels, the unread header words) is not known: the encoder sends 0 or the middle code there.
//!
//! Built for the tests only: nothing of the scope's programs encodes, and they stay the same programs (ADR 0041).

use crate::dho::roll::{CHUNK_BYTES, CHUNK_WORDS};
use crate::dho::seq::frame_stride;
use crate::dho::{hz, readout};

/// The read-out word of `v` volts on a channel with (`scale` V/div, `offset` V): the inverse of `seq::volts`, the
/// nearest code, held to 0..=65535 where the volts are beyond the 16 bits.
pub fn code(v: f64, scale: f64, offset: f64) -> u16 {
    ((v + offset) / scale * readout::CODES_PER_DIV + readout::MID).round().clamp(0.0, 65535.0) as u16
}

/// The inverse of `seq::deinterleave`: the words of the channels of `mask` as a read-out (ratio 1 the one channel,
/// ratio 2 the lower on even words, ratio 4 word 4i+k is CH k+1). `ch[c]` holds the points of CH c+1; the read-out
/// has as many points per channel as the shortest enabled channel. The slot of the channel that is off in a group of
/// four (three channels on) carries `readout::MID`. No channel on: no words.
pub fn interleave(ch: [&[u16]; 4], mask: u32) -> Vec<u8> {
    let on: Vec<usize> = (0..4).filter(|c| mask & (1 << c) != 0).collect();
    let Some(n) = on.iter().map(|&c| ch[c].len()).min() else { return Vec::new() };
    // the channel of each word of a group, as the decode reads it
    let slots: Vec<Option<usize>> = match hz::ratio(mask) {
        1 => vec![Some(on[0])],
        2 => vec![Some(on[0]), Some(on[1])],
        _ => (0..4).map(|c| on.contains(&c).then_some(c)).collect(),
    };
    let mut out = Vec::with_capacity(2 * n * slots.len());
    for i in 0..n {
        for c in &slots {
            out.extend(c.map_or(readout::MID as u16, |c| ch[c][i]).to_le_bytes());
        }
    }
    out
}

/// The read-out the unit sends for the volts `v[c]` of the channels of `mask`, with `cal[c]` = (scale, offset) of CH
/// c+1: `code` of each point, then `interleave`. `seq::decode_volts` of it gives the volts back within half a code.
pub fn volts(v: [&[f32]; 4], mask: u32, cal: [(f64, f64); 4]) -> Vec<u8> {
    let words: Vec<Vec<u16>> =
        (0..4).map(|c| v[c].iter().map(|&x| code(x as f64, cal[c].0, cal[c].1)).collect()).collect();
    interleave([&words[0], &words[1], &words[2], &words[3]], mask)
}

/// The 16-byte header the unit sends before a frame while 0x1010 bit 31 is clear: word 0 0xfa05, word 2 0x7b, word 4
/// the frame's number in the batch, words 5..7 the 48-bit stamp, the most significant word first (`seg::header_stamp`
/// reads it). Words 1 and 3 carry the source, the plot type and a time count on the unit; nothing reads them.
pub fn header(frame: u16, stamp: u64) -> [u8; 16] {
    words_le([0xfa05, 0, 0x7b, 0, frame, (stamp >> 32) as u16, (stamp >> 16) as u16, stamp as u16])
}

/// The inverse of `seq::unpad`: frames of `frame` bytes, back to back in `frames`, each followed by zero bytes up to
/// `seq::frame_stride`.
pub fn pad(frames: &[u8], frame: usize) -> Vec<u8> {
    if frame == 0 {
        return frames.to_vec();
    }
    let stride = frame_stride(frame);
    let mut out = Vec::with_capacity(frames.len() / frame * stride);
    for f in frames.chunks_exact(frame) {
        out.extend_from_slice(f);
        out.resize(out.len() + stride - frame, 0);
    }
    out
}

/// One chunk of the roll stream: the 8-word header (word 0 0xfa05, word 1 bit 1 set for a chunk of the logic analyser,
/// word 2 0x7b, word 4 the `counter`), then the chunk's 100 words, all channels interleaved as `interleave` lays them
/// out. The unit has more bits in word 1 and a time stamp in words 3 and 5..7; nothing reads them.
pub fn chunk(counter: u16, la: bool, words: &[u16; CHUNK_WORDS]) -> [u8; CHUNK_BYTES] {
    let mut out = [0u8; CHUNK_BYTES];
    out[..16].copy_from_slice(&words_le([0xfa05, if la { 2 } else { 0 }, 0x7b, 0, counter, 0, 0, 0]));
    for (o, w) in out[16..].as_chunks_mut::<2>().0.iter_mut().zip(words) {
        *o = w.to_le_bytes();
    }
    out
}

/// Eight words, little-endian.
fn words_le(w: [u16; 8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (o, w) in out.as_chunks_mut::<2>().0.iter_mut().zip(w) {
        *o = w.to_le_bytes();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dho::{roll, seg, seq};

    /// Words that cover the code range, from a fixed seed (no wall clock, no random source of the host).
    fn words(n: usize, seed: u32) -> Vec<u16> {
        let mut x = seed.wrapping_mul(2_654_435_761).max(1);
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u16
            })
            .collect()
    }

    fn bytes(w: &[u16]) -> Vec<u8> {
        w.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    // Break it catches: an encoder that puts a channel at another word than the decode reads, or loses a code
    #[test]
    fn read_out_bytes_decoded_and_encoded_again_are_the_same_bytes() {
        for mask in 1u32..16 {
            let ratio = hz::ratio(mask) as usize;
            let mut w = words(ratio * 517, mask);
            if ratio == 4 && mask.count_ones() == 3 {
                // the slot of the channel that is off carries the middle code
                let off = (0..4).find(|c| mask & (1 << c) == 0).unwrap();
                for g in w.as_chunks_mut::<4>().0 {
                    g[off] = 32768;
                }
            }
            let b = bytes(&w);
            let ch = seq::deinterleave(&b, mask);
            assert_eq!(interleave([&ch[0], &ch[1], &ch[2], &ch[3]], mask), b, "mask {:#x}", mask);
        }
        assert!(interleave([&[1], &[2], &[3], &[4]], 0).is_empty(), "no channel on: no words");
    }

    // Break it catches: a code scale or offset that is not the inverse of the decode's
    #[test]
    fn volts_encoded_and_decoded_come_back_within_half_a_code() {
        let cal = [(1.0, 0.1), (0.5, -0.2), (2.0, 0.0), (0.02, 0.005)];
        for mask in 1u32..16 {
            let v: Vec<Vec<f32>> = (0..4)
                .map(|c| {
                    let (scale, offset) = cal[c];
                    // four divisions each side of the offset, where the 16 bits hold every value
                    let at = |w: u16| ((w as f64 / 65535.0 - 0.5) * 8.0 * scale - offset) as f32;
                    words(301, 7 * mask + c as u32).into_iter().map(at).collect()
                })
                .collect();
            let d = seq::decode_volts(&volts([&v[0], &v[1], &v[2], &v[3]], mask, cal), mask, cal);
            for c in (0..4).filter(|c| mask & (1 << c) != 0) {
                let half = cal[c].0 / readout::CODES_PER_DIV / 2.0;
                assert_eq!(d[c].len(), v[c].len(), "mask {:#x} CH{}", mask, c + 1);
                for (a, b) in v[c].iter().zip(&d[c]) {
                    assert!(
                        (a - b).abs() as f64 <= half * 1.001 + 1e-6,
                        "mask {:#x} CH{}: {} -> {}",
                        mask,
                        c + 1,
                        a,
                        b
                    );
                }
            }
        }
        // beyond the 16 bits the code holds at its ends
        assert_eq!((code(1e3, 1.0, 0.0), code(-1e3, 1.0, 0.0), code(0.0, 1.0, 0.0)), (65535, 0, 32768));
    }

    // Break it catches: padding that `unpad` does not drop, or a frame shifted by the stride
    #[test]
    fn frames_padded_and_unpadded_are_the_same_frames() {
        for frame in [8usize, 16 + 2 * 1250, 2 * 625, 2 * 4 * 627 + 16, 2 * 1251, 6] {
            let frames: Vec<u8> = bytes(&words(2 * frame, frame as u32))[..4 * frame].to_vec();
            let padded = pad(&frames, frame);
            assert_eq!(padded.len(), seq::read_len(4, frame), "{} bytes a frame", frame);
            let mut back = padded.clone();
            seq::unpad(&mut back, frame);
            assert_eq!(back, frames, "{} bytes a frame", frame);
            assert_eq!(pad(&back, frame), padded, "the padding is all that unpad dropped");
        }
    }

    // Break it catches: a header whose stamp or frame number is not where the decode and the log read them
    #[test]
    fn a_frame_header_holds_its_number_and_stamp_where_they_are_read() {
        for (frame, stamp) in [(0u16, 0u64), (5, 0x1234_5678_9abc), (0xffff, (1 << 48) - 1)] {
            let h = header(frame, stamp);
            assert_eq!(seg::header_stamp(&h), Some(stamp));
            assert_eq!(u16::from_le_bytes([h[8], h[9]]), frame, "word 4");
            assert_eq!(u16::from_le_bytes([h[0], h[1]]), 0xfa05, "word 0");
        }
    }

    // Break it catches: a chunk whose counter, LA mark or words are not where `counter`, `is_la_chunk` and the splits
    // read them
    #[test]
    fn chunks_made_and_split_again_give_their_words() {
        let words = |k: u16| -> [u16; CHUNK_WORDS] { std::array::from_fn(|i| 30_000 + k * 7 + i as u16) };
        for mask in [1u32, 3, 5, 0xf, 0b0111] {
            let ratio = hz::ratio(mask) as usize;
            let (mut stream, mut want) = (Vec::new(), <[Vec<u16>; 4]>::default());
            for k in 1..=5u16 {
                let c = chunk(k, false, &words(k));
                assert_eq!((roll::counter(&c), roll::is_la_chunk(&c)), (k, false));
                assert_eq!(&c[16..], &bytes(&words(k))[..]);
                for (o, d) in want.iter_mut().zip(seq::deinterleave(&c[16..], mask)) {
                    o.extend(d);
                }
                stream.extend(c);
            }
            assert_eq!(roll::split(&stream, mask), want, "mask {:#x}", mask);
            assert!(want.iter().filter(|v| !v.is_empty()).all(|v| v.len() == 5 * CHUNK_WORDS / ratio));
        }
        // an LA chunk between two analog ones: its words go to the LA, not into the channels
        let (a, l, b) = (chunk(1, false, &words(1)), chunk(2, true, &words(2)), chunk(3, false, &words(3)));
        assert!(roll::is_la_chunk(&l));
        let (analog, la) = roll::split_la(&[a, l, b].concat(), 1);
        assert_eq!(la, words(2).to_vec());
        assert_eq!(analog[0], [words(1), words(3)].concat());
    }
}
