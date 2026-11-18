// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hostile input for the protocol decoders: what the probes carry is outside input too, and a record can be empty,
//! one sample, or hold samples that are no numbers (a channel without data).

use super::run;
use crate::protocols::{demo, opt_default, opt_fit, shared, Lines, MODULES};

/// Every decoder of the registry (`ModuleInfo::decode`) on records made from its own demo traffic: returns for any
/// samples, also none, one, and ones that are no numbers; it gives packets or says what is missing; the times of a
/// packet are numbers, and there are not more packets than a record of that length can hold.
#[test]
fn hostile_decoder_records() {
    for (i, m) in MODULES.iter().enumerate() {
        let i = i as u8;
        let opt = opt_fit(i, opt_default(i));
        let sh = shared(&Default::default());
        let dt = m.info().bit_time(&opt, &sh) / 8.0;
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
                sh,
                chans: std::array::from_fn(|k| used(k).then(|| waves[k].as_slice())),
                src: std::array::from_fn(|k| used(k).then_some(k)),
            };
            match m.info().decode(&l) {
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

// Break it catches: a CS# frame that ends where it starts, with a clock edge at that instant, panics (#181)
#[cfg(feature = "dec-qspi")]
#[test]
fn hostile_qspi_zero_frame() {
    let Some(m) = MODULES.iter().find(|m| m.key() == "Qspi") else { panic!("no QSPI decoder in this build") };
    // CS# falls and rises between samples so far apart that both crossings are at sample 1; so is the clock's edge
    let (cs, sck) = ([1e30f32, 0.0, 1e17], [0.0f32, 0.0, 1e30]);
    let lines = [Some(&cs[..]), Some(&sck[..]), Some(&cs[..]), None];
    let l = Lines {
        lines,
        dt: 1e-7,
        thr: 1.65,
        hy: 0.1,
        opt: [0; 4],
        sh: shared(&Default::default()),
        chans: lines,
        src: [None; 4],
    };
    let p = m.info().decode(&l);
    assert!(p.as_ref().is_ok_and(|p| p.is_empty()), "a frame of no time holds no clock edge: {p:?}");
}
