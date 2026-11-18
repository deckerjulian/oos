// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hostile input for the protocol decoders: what the probes carry is outside input too, and a record can be empty,
//! one sample, or hold samples that are no numbers (a channel without data).

use super::{is_known, run};
use crate::protocols::{demo, opt_default, opt_fit, Lines, Shared, MODULES};

/// Every decoder of the registry (`Protocol::decode`) on records made from its own demo traffic: returns for any
/// samples, also none, one, and ones that are no numbers; it gives packets or says what is missing; the times of a
/// packet are numbers, and there are not more packets than a record of that length can hold.
#[test]
fn hostile_decoder_records() {
    for (i, m) in MODULES.iter().enumerate() {
        let i = i as u8;
        let opt = opt_fit(i, opt_default(i));
        let dt = m.bit_time(&opt) / 8.0;
        // the role slots the module uses, at their places (USB and Parallel leave slots 1 and 3 empty)
        let mut slots: Vec<usize> = (0..4).filter(|&k| m.info().roles[k].is_some()).collect();
        if slots.is_empty() {
            slots.push(0);
        }
        let roles = slots.len();
        // the lines of a record side by side: one little-endian f32 per line and sample
        let mut traffic = Vec::new();
        if let Some(d) = demo(i) {
            for k in 0..((d.period / dt) as usize).clamp(64, 6000) {
                for &line in &slots {
                    let v = d.lines[line].as_ref().map_or(0.0, |f| f(k as f64 * dt) as f32);
                    traffic.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        let fill = |v: f32, n: usize| v.to_le_bytes().repeat(n * roles);
        let fixed = [fill(1.0, 1), fill(f32::NAN, 64), fill(f32::INFINITY, 64), fill(0.0, 64), traffic];
        let seeds: Vec<&[u8]> = fixed.iter().map(|f| f.as_slice()).collect();
        let s = m.setup();
        run(&format!("protocols::{}::decode", m.key()), &seeds, 1, |b| {
            let mut waves: [Vec<f32>; 4] = Default::default();
            for step in b.chunks_exact(4 * roles) {
                for (&line, v) in slots.iter().zip(step.as_chunks::<4>().0) {
                    waves[line].push(f32::from_le_bytes(*v));
                }
            }
            let n = waves[slots[0]].len();
            let used = |k: usize| slots.contains(&k);
            let l = Lines {
                lines: std::array::from_fn(|k| used(k).then(|| waves[k].as_slice())),
                dt,
                thr: s.thr,
                hy: 0.1,
                opt,
                sh: Shared::default(),
                chans: std::array::from_fn(|k| used(k).then(|| waves[k].as_slice())),
                src: std::array::from_fn(|k| used(k).then_some(k)),
            };
            if m.key() == "Qspi" && qspi_trap(&waves[2], &waves[1], s.thr, 0.1) && is_known("qspi-zero-frame") {
                return Ok(());
            }
            match m.decode(&l) {
                Ok(p) => {
                    if let Some(x) = p.iter().find(|x| !(x.t0.is_finite() && x.t1.is_finite())) {
                        return Err(format!("a packet from {} to {}: {:?}", x.t0, x.t1, x.kind));
                    }
                    if p.len() > 8 * n + 8 {
                        return Err(format!("{} packets from {} samples", p.len(), n));
                    }
                    Ok(())
                }
                Err("") => Err("refused without a message".into()),
                Err(_) => Ok(()),
            }
        });
    }
}

/// The finding `qspi-zero-frame` (in oos-dsp, where `hostile_known_qspi_frame_ends_before_it_starts` has its
/// smallest record), if this record runs into it: a CS# frame that ends at or before its start with a clock edge at
/// that instant.
fn qspi_trap(cs: &[f32], sck: &[f32], thr: f32, hy: f32) -> bool {
    use oos_dsp::edges::{edges, states};
    let clk: Vec<f64> = edges(sck, thr, hy).iter().filter(|e| e.rising).map(|e| e.t).collect();
    let mut open = (states(cs, thr, hy).iter().flatten().next() == Some(&false)).then_some(0.0);
    for e in edges(cs, thr, hy) {
        if !e.rising {
            open = Some(e.t);
        } else if let Some(start) = open.take() {
            if clk.iter().any(|&t| t <= start && t >= e.t) {
                return true;
            }
        }
    }
    false
}

#[cfg(feature = "dec-qspi")]
#[test]
#[ignore = "the QSPI decoder panics on a CS# frame that ends where it starts with a clock edge there, #181"]
pub(super) fn hostile_known_qspi_zero_frame() {
    let Some(m) = MODULES.iter().find(|m| m.key() == "Qspi") else { panic!("no QSPI decoder in this build") };
    // CS# falls and rises between samples so far apart that both crossings are at sample 1; so is the clock's edge
    let (cs, sck) = ([1e30f32, 0.0, 1e17], [0.0f32, 0.0, 1e30]);
    let lines = [Some(&cs[..]), Some(&sck[..]), Some(&cs[..]), None];
    let l =
        Lines { lines, dt: 1e-7, thr: 1.65, hy: 0.1, opt: [0; 4], sh: Shared::default(), chans: lines, src: [None; 4] };
    let _ = m.decode(&l);
}
