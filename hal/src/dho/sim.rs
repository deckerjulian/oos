// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The simulated unit, for host runs of the board code (ADR 0053, docs/board-on-a-host.md 3): what the unit answers
//! on the register window and sends in the C2H stream, as docs/protocols/ describes it (acquisition-start.md,
//! record-play.md, the head of `seg`). Behind the feature `sim`, which only tests switch on: the programs for the scope
//! are built without it (ADR 0041).
//!
//! `Engine` is the model of the record engine and the play path for host runs of the whole sequence (the steps of
//! `State::record_steps`, `read_steps_at`, `segment_stamp_steps` and `disarm_steps`, and the average mode). It is
//! still written in `seg` and only named here: moving its code out of `seg` changed the programs for the scope,
//! although they never call it (the compiler lays out the rest of hal anew). It moves with the stage that runs on a
//! unit anyway (docs/board-on-a-host.md 6).

pub use crate::dho::seg::sim::{engine, Engine, SimBus, SimC2h};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dho::bus::Bus;
    use crate::dho::seg::HEADER_TICK_FS;
    use std::io::Read;

    // Break it catches: a play whose frames lose their header, their number or the stamp of the trigger before them
    #[test]
    fn a_played_frame_carries_its_number_and_the_trigger_before_it() {
        let (t0, period) = (1_000_000i128, 50_000_000i128);
        let (_, mut bar, mut c2h) = engine(Engine::new(t0, period, 1, 1));
        // play frames 2..=3 of 8 words each, with the header (0x1010 bit 31 clear)
        bar.wr(0x11f0, 8);
        bar.wr(0x4010, 2);
        bar.wr(0x4018, 3);
        bar.wr(0x400c, 0x4000_0000);
        bar.wr(0x4004, 1);
        let mut b = vec![0u8; 2 * (16 + 2 * 8)];
        c2h.read_exact(&mut b).unwrap();
        for (f, k) in b.as_chunks::<32>().0.iter().zip([2u32, 3]) {
            assert_eq!(u16::from_le_bytes([f[8], f[9]]), k as u16, "header word 4");
            let before = ((t0 + (k as i128 - 1) * period) / HEADER_TICK_FS) as u64;
            assert_eq!(crate::dho::seg::header_stamp(&f[..16]), Some(before), "frame {}", k);
            assert_eq!(u16::from_le_bytes([f[16], f[17]]), Engine::word(k, 0), "frame {}: its words", k);
        }
        assert!(c2h.read(&mut [0u8; 1]).is_err(), "nothing more was played");
    }
}
