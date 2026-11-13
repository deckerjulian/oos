// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Timing guards of the fork's batch read and decode (docs/fork/perf-plan.md, P3). Each compares the fork's path with
//! a reference copy of the path it replaced, on the same host in the same run: ratios are stable across hosts,
//! absolute times are not. `#[ignore]`: run them in release with `tools/perf-guard.sh`.

use oos_hal::dho::bus::DmaBuf;
use oos_hal::dho::seq::{decode_volts, unpad_in};

/// Upstream's batch path at 0.1.11 (`read_dma`, `unpad`, `deinterleave`, `volts`), and the fork's f64 fused decode
/// before the f32 one: copies, so a later change to the library cannot move the baseline.
mod reference {
    pub fn read_dma(c2h: &mut impl std::io::Read, bytes: usize) -> std::io::Result<Vec<u8>> {
        let mut buf = vec![0u8; bytes];
        let mut got = 0;
        while got < bytes {
            match c2h.read(&mut buf[got..])? {
                0 => return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "end of stream")),
                k => got += k,
            }
        }
        Ok(buf)
    }

    pub fn unpad(buf: &mut Vec<u8>, frame: usize) {
        let stride = oos_hal::dho::seq::frame_stride(frame);
        if frame == 0 || stride == frame {
            return;
        }
        let n = buf.len() / stride;
        for k in 1..n {
            buf.copy_within(k * stride..k * stride + frame, k * frame);
        }
        buf.truncate(n * frame);
    }

    pub fn deinterleave(bytes: &[u8], mask: u32) -> [Vec<u16>; 4] {
        let w: Vec<u16> = bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        let on: Vec<usize> = (0..4).filter(|c| mask & (1 << c) != 0).collect();
        let mut out: [Vec<u16>; 4] = Default::default();
        match oos_hal::dho::hz::ratio(mask) {
            1 => out[on[0]] = w,
            2 => {
                out[on[0]] = w.iter().step_by(2).copied().collect();
                out[on[1]] = w.iter().skip(1).step_by(2).copied().collect();
            }
            _ => {
                for c in on {
                    out[c] = w.iter().skip(c).step_by(4).copied().collect();
                }
            }
        }
        out
    }

    fn volt(x: u16, scale: f64, offset: f64) -> f32 {
        ((x as f64 - 32768.0) / 7500.0 * scale - offset) as f32
    }

    pub fn volts(w: &[u16], scale: f64, offset: f64) -> Vec<f32> {
        w.iter().map(|&x| volt(x, scale, offset)).collect()
    }

    /// Upstream's decode of one frame: deinterleave to u16, then volts per channel.
    pub fn two_pass(bytes: &[u8], mask: u32, cal: [(f64, f64); 4]) -> [Vec<f32>; 4] {
        let w = deinterleave(bytes, mask);
        std::array::from_fn(|c| if w[c].is_empty() { Vec::new() } else { volts(&w[c], cal[c].0, cal[c].1) })
    }

    fn lane<const G: usize>(bytes: &[u8], k: usize, (scale, offset): (f64, f64)) -> Vec<f32> {
        bytes.chunks_exact(G).map(|g| volt(u16::from_le_bytes([g[2 * k], g[2 * k + 1]]), scale, offset)).collect()
    }

    /// The fork's fused decode in f64 (before the f32 one), whole groups only: enough for the timing.
    pub fn fused_f64(bytes: &[u8], mask: u32, cal: [(f64, f64); 4]) -> [Vec<f32>; 4] {
        let ratio = oos_hal::dho::hz::ratio(mask) as usize;
        let on: Vec<usize> = (0..4).filter(|c| mask & (1 << c) != 0).collect();
        let mut out: [Vec<f32>; 4] = Default::default();
        for (k, &c) in on.iter().enumerate() {
            let k = if ratio == 4 { c } else { k };
            if k >= ratio {
                break;
            }
            out[c] = match ratio {
                1 => lane::<2>(bytes, 0, cal[c]),
                2 => lane::<4>(bytes, k, cal[c]),
                _ => lane::<8>(bytes, k, cal[c]),
            };
        }
        out
    }
}

struct Mem<'a>(&'a [u8], usize);

impl std::io::Read for Mem<'_> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        let k = b.len().min(self.0.len() - self.1);
        b[..k].copy_from_slice(&self.0[self.1..self.1 + k]);
        self.1 += k;
        Ok(k)
    }
}

/// Pseudo-random read-out bytes (a fixed LCG).
fn noise(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (x >> 24) as u8
        })
        .collect()
}

/// ms per call of `old` and of `new`: `rounds` rounds of `runs` calls each, the two sides alternating (and which one
/// starts), the fastest round of each. Other load on the host hits both sides alike, and the fastest round is the
/// least disturbed (timing each side in one block gave ratios from 1.1 to 1.9 on an unchanged tree under load).
fn best_pair_ms(rounds: usize, runs: usize, mut old: impl FnMut(), mut new: impl FnMut()) -> (f64, f64) {
    let time = |f: &mut dyn FnMut()| {
        let t = std::time::Instant::now();
        for _ in 0..runs {
            f();
        }
        t.elapsed().as_secs_f64() * 1e3 / runs as f64
    };
    let (mut a, mut b) = (f64::INFINITY, f64::INFINITY);
    for r in 0..rounds {
        if r % 2 == 0 {
            a = a.min(time(&mut old));
            b = b.min(time(&mut new));
        } else {
            b = b.min(time(&mut new));
            a = a.min(time(&mut old));
        }
    }
    (a, b)
}

/// Rounds and calls per round of every comparison (as many calls as the earlier 5 x 10, in more, shorter rounds).
const ROUNDS: usize = 10;
const RUNS: usize = 5;

const CAL: [(f64, f64); 4] = [(1.0, 0.0), (0.5, 0.1), (2.0, -0.3), (1.0, 0.0)];

/// (mask, frame bytes, frames) of one ~4 MB batch per live set-up.
const CASES: [(u32, usize, usize); 5] = [(1, 2816, 1420), (1, 2500, 1600), (1, 200_000, 20), (3, 4000, 1000), (0xf, 8000, 500)];

/// One 4 MB batch, read + decode: upstream's path against the fork's. Fails when the fork's decode is less than 2x
/// faster (a merge that brought back deinterleave + volts in the batch loop, or slowed the fused decode; measured
/// 3.4-5x on an M3 Max, 1.6x before the f32 decode). `cargo test --release --test perf_ratio -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_batch_decode() {
    for (mask, frame, frames) in CASES {
        let data = noise(frames * frame, 1);
        let mut dma = DmaBuf::new();
        let (read_old, read_new) = best_pair_ms(
            ROUNDS,
            RUNS,
            || {
                let mut v = reference::read_dma(&mut Mem(&data, 0), data.len()).unwrap();
                reference::unpad(&mut v, frame);
                std::hint::black_box(v);
            },
            || {
                let b = dma.read(&mut Mem(&data, 0), data.len(), |_, _| {}).unwrap();
                let n = unpad_in(b, frame);
                dma.truncate(n);
                std::hint::black_box(dma.data());
            },
        );
        let (dec_old, dec_new) = best_pair_ms(
            ROUNDS,
            RUNS,
            || {
                for f in data.chunks_exact(frame) {
                    std::hint::black_box(reference::two_pass(f, mask, CAL));
                }
            },
            || {
                for f in data.chunks_exact(frame) {
                    std::hint::black_box(decode_volts(f, mask, CAL));
                }
            },
        );
        let ratio = dec_old / dec_new;
        eprintln!("mask {:#x}, {} frames of {} bytes: read {:.2} -> {:.2} ms, decode {:.2} -> {:.2} ms ({:.2}x)", mask, frames, frame, read_old, read_new, dec_old, dec_new, ratio);
        assert!(ratio >= 2.0, "mask {:#x}: fused decode only {:.2}x the two-pass one", mask, ratio);
        assert!(read_old / read_new >= 1.0, "mask {:#x}: the kept DMA buffer reads slower than a new vector", mask);
    }
}

/// The f32 decode against the fork's earlier f64 one. Fails above 0.8x the f64 time: the conversion stopped
/// vectorising or went back to f64 (measured 0.5-0.65x on an M3 Max; the loop is near memory bandwidth, so not the
/// 0.2x of the arithmetic alone that perf-plan item 8 estimated).
#[test]
#[ignore]
fn bench_decode_f32_vs_f64() {
    for (mask, frame, frames) in CASES {
        let data = noise(frames * frame, 2);
        let (f64_ms, f32_ms) = best_pair_ms(
            ROUNDS,
            RUNS,
            || {
                for f in data.chunks_exact(frame) {
                    std::hint::black_box(reference::fused_f64(f, mask, CAL));
                }
            },
            || {
                for f in data.chunks_exact(frame) {
                    std::hint::black_box(decode_volts(f, mask, CAL));
                }
            },
        );
        let mut out: [Vec<f32>; 4] = Default::default();
        // reported only: the same comparison partner keeps its timing honest under load
        let (_, into_ms) = best_pair_ms(
            ROUNDS,
            RUNS,
            || {
                for f in data.chunks_exact(frame) {
                    std::hint::black_box(decode_volts(f, mask, CAL));
                }
            },
            || {
                for f in data.chunks_exact(frame) {
                    oos_hal::dho::seq::decode_volts_into(f, mask, CAL, &mut out);
                    std::hint::black_box(&out);
                }
            },
        );
        eprintln!("mask {:#x}, {} frames of {} bytes: decode f64 {:.2} ms, f32 {:.2} ms ({:.2}x), f32 into kept vectors {:.2} ms", mask, frames, frame, f64_ms, f32_ms, f64_ms / f32_ms, into_ms);
        assert!(f32_ms <= 0.8 * f64_ms, "mask {:#x}: f32 decode {:.2} ms vs f64 {:.2} ms", mask, f32_ms, f64_ms);
    }
}
