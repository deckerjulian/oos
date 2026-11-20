// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The live compression (compress.rs): when a record holds many more samples than the screen has columns, the FPGA
//! delivers min/max pairs instead of every sample. Its switch, its limits and its factor.

use crate::model::*;

/// Compressed points per screen column the factor aims at: two min/max pairs, so a pair never stands for more than
/// half a column and the column boundaries cut at most a quarter column of samples into the wrong column.
pub const POINTS_PER_COLUMN: f64 = 4.0;
/// Screen columns the factor is computed for (the stock 1000; the trace map is at most this wide).
pub const SCREEN_COLUMNS: f64 = 1000.0;
/// Smallest factor worth a compressed read; below it every sample is read.
pub const K_MIN: u32 = 2;
/// Largest factor: 0x1170 holds k in bits 13:0 (`seq::COMPRESS_K_MAX`; larger values wrap, assumed).
pub const K_MAX: u32 = 16383;
/// Least time between two raw reads of a compressed batch's newest frame (`seq::read_raw_again`). A read after every
/// batch cost 7-9 % of the frames at 1 channel, 1 and 100 us/div (verified on the unit, unitlab round 2); between the
/// raw records the UI keeps the measurement of the last one (`compress::hold`), so measurements follow at up to 5 a
/// second.
pub const RAW_PERIOD: std::time::Duration = std::time::Duration::from_millis(200);
/// Deepest record compressed this way. Deeper records keep the deep-memory read of `hw::batch_plan` (a screen window
/// raw, or the whole record compressed to ~625 k points).
pub const MAX_DEPTH: u32 = 1_000_000;

thread_local! {
    /// tests: `enabled` for this thread
    #[cfg(test)]
    static FORCE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Sets `enabled` for the calling thread (tests).
#[cfg(test)]
pub fn force(on: Option<bool>) {
    FORCE.with(|f| f.set(on));
}

/// `acq.compress = on` in system.conf, or OOS_COMPRESS set (any value but empty or 0). Off by default until the unit
/// has passed the procedure in docs/fork/compress.md.
pub fn enabled() -> bool {
    #[cfg(test)]
    if let Some(on) = FORCE.with(|f| f.get()) {
        return on;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| FLAG.get())
}

const FLAG: crate::sysconf::Flag = crate::sysconf::Flag { key: "acq.compress", env: Some("OOS_COMPRESS"), on: "on" };

/// The live-compression factor of the last batch read: 1 for raw frames (`:SYSTem:STATe?` `compress k`).
static LAST_K: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

pub fn note_batch_k(k: u32) {
    LAST_K.store(k, std::sync::atomic::Ordering::Relaxed);
}

pub fn last_k() -> u32 {
    LAST_K.load(std::sync::atomic::Ordering::Relaxed)
}

/// The model allows compressed frames: the frames of a batch are only drawn, and drawn by the dense fast path
/// (`Ui::fast_ok`: plain edge trigger, Normal acquisition, no mask, zones, zoom or Z input, DC coupling without the
/// 20 MHz limit), not Single (the one record is the answer), no segments, no LA (its own second read of the record), no
/// Roll and scan are the caller's (`hw::slow_mode`).
pub fn allowed(m: &Model) -> bool {
    m.trig_type == TrigType::Edge
        && m.acq == AcqMode::Normal
        && m.trig_mode != TrigMode::Single
        && m.segments == 0
        && !m.mask_on
        && !m.zoom
        && m.z_src.is_none()
        && !m.zones.iter().any(|z| z.usable())
        && !m.la_on()
        && (0..NCH)
            .all(|c| !m.ch[c].on || (matches!(m.ch[c].coupling, Coupling::Dc | Coupling::Dc50) && m.ch[c].bw != BW_20M))
        && raw_consumers_off(m)
}

/// Nothing reads the record on screen beyond the picture and the measurements (which get raw records, `hold`): no
/// protocol decoder, no math channel or FFT, the scope view (not spectrum, eye, power, Bode or step), and a DC trigger
/// path (the software trigger level of AC, LF and HF reject comes from the frame's samples). The UI-only readers
/// (protocol watch, XY) ask for raw frames through `Shared::needs_raw` instead.
pub fn raw_consumers_off(m: &Model) -> bool {
    (0..DECODERS).all(|k| !m.dec_is_on(k))
        && m.math == crate::math::MathOp::Off
        && m.maths.iter().all(|x| x.op == crate::math::MathOp::Off)
        && m.view == AView::Scope
        && m.trig_coupling() == TrigCoup::Dc
}

/// The compression factor for a record of `memsel` points at `rate` samples/s per channel and `tdiv` s/div:
/// `POINTS_PER_COLUMN` points per screen column, 1 (off) below `K_MIN` or above `MAX_DEPTH`.
pub fn k_for(rate: u64, tdiv: f64, memsel: u32) -> u32 {
    let per_column = rate as f64 * tdiv * HDIV as f64 / SCREEN_COLUMNS;
    let k = (per_column / POINTS_PER_COLUMN).floor().min(K_MAX as f64) as u32;
    if k < K_MIN || memsel > MAX_DEPTH {
        1
    } else {
        k
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factor_by_time_base() {
        // 1 GSa/s at 10 us/div: 100 samples per column, 4 points per column: k = 25
        assert_eq!(k_for(1_000_000_000, 10e-6, 100_000), 25);
        // 1.25 GSa/s at 1 us/div: 12.5 samples per column: k = 3
        assert_eq!(k_for(1_250_000_000, 1e-6, 12_512), 3);
        // fewer than 8 samples per column: every sample
        assert_eq!(k_for(1_250_000_000, 500e-9, 6_256), 1);
        // deep records keep the deep-memory read
        assert_eq!(k_for(1_250_000_000, 1e-3, 10_000_000), 1);
        assert_eq!(k_for(u64::MAX, 1.0, 1000), K_MAX);
    }

    #[test]
    fn opt_in_and_the_model_conditions() {
        FLAG.check_both_files();
        let mut m = Model::default();
        m.ch[0].on = true;
        assert!(allowed(&m));
        m.dots = false;
        assert!(allowed(&m));
        for f in [
            |m: &mut Model| m.trig_mode = TrigMode::Single,
            |m: &mut Model| m.acq = AcqMode::Average,
            |m: &mut Model| m.acq = AcqMode::HiRes,
            |m: &mut Model| m.acq = AcqMode::Peak,
            |m: &mut Model| m.zoom = true,
            |m: &mut Model| m.mask_on = true,
            |m: &mut Model| m.segments = 4,
            |m: &mut Model| m.trig_type = TrigType::Pulse,
            |m: &mut Model| m.ch[0].bw = BW_20M,
            |m: &mut Model| m.ch[0].coupling = Coupling::Ac,
        ] {
            let mut x = m.clone();
            f(&mut x);
            assert!(!allowed(&x));
        }
    }
}
