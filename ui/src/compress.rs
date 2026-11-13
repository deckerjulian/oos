// SPDX-License-Identifier: GPL-2.0-or-later

//! Live display compression (fork, docs/fork/compress.md): when a record holds many more samples than the screen has
//! columns, the FPGA delivers min/max pairs (0x1170, `seq::Settings::compress` = k) instead of every sample, so a batch
//! holds k times more frames for the same read. Opt-in: `acq.compress = on` in system.conf or OOS_COMPRESS.
//!
//! Every record the UI keeps stays raw: the acquisition thread reads the newest frame of each compressed batch a
//! second time without compression (`seq::read_raw_again`), and that frame is the one `acquire` makes the record of
//! (measurements, cursors, FFT, decode, history, the record shown after STOP). The compressed frames are only drawn
//! (`deposit_pairs`, from `drawpipe::draw_frame`). Where even that is not the same picture, compression stays off
//! (`allowed`).

use crate::board::Frame;
use crate::fastdraw::Map;
use crate::model::*;

/// Points per min/max pair (`Frame::envelope`): pair i is points 2i and 2i + 1 of the frame. The order inside a pair
/// and the samples it stands for are the board's (`oos_hal::dho::pairs` for the DHO); the drawing needs neither.
pub const PAIR: usize = 2;

/// Compressed points per screen column the factor aims at: two min/max pairs, so a pair never stands for more than
/// half a column and the column boundaries cut at most a quarter column of samples into the wrong column.
pub const POINTS_PER_COLUMN: f64 = 4.0;
/// Screen columns the factor is computed for (the stock 1000; the trace map is at most this wide).
pub const SCREEN_COLUMNS: f64 = 1000.0;
/// Smallest factor worth a compressed read; below it every sample is read.
pub const K_MIN: u32 = 2;
/// Largest factor: 0x1170 holds k in bits 13:0 (`seq::COMPRESS_K_MAX`; the netlist simulation shows larger values wrap).
pub const K_MAX: u32 = 16383;
/// Least time between two raw reads of a compressed batch's newest frame (`seq::read_raw_again`). A read after every
/// batch cost 7-9 % of the frames at 1 channel, 1 and 100 us/div (unitlab round 2); between the raw records the UI
/// keeps the measurement of the last one (`hold`), so measurements follow at up to 5 a second.
pub const RAW_PERIOD: std::time::Duration = std::time::Duration::from_millis(200);
/// Deepest record compressed this way. Deeper records keep the deep-memory read of `hw::batch_plan` (a screen window
/// raw, or the whole record compressed to ~625 k points).
pub const MAX_DEPTH: u32 = 1_000_000;

/// `acq.compress = on` in system.conf, or OOS_COMPRESS set (any value but empty or 0). Off by default until the unit
/// has passed the procedure in docs/fork/compress.md.
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let env = std::env::var("OOS_COMPRESS").ok().map(|v| if v.is_empty() || v == "0" { "off" } else { "on" });
        let conf = crate::sysconf::fork_text();
        enabled_from(env, &conf)
    })
}

/// `env` if set, else the `acq.compress` line of `conf`; off unless `on`.
pub fn enabled_from(env: Option<&str>, conf: &str) -> bool {
    crate::drawpipe::conf_value(env, conf, "acq.compress").as_deref() == Some("on")
}

/// Whether the UI leaves the newest record unmeasured: running (not Single), and the record is of live-compression
/// pairs. Its batch's newest frame is read raw only every `RAW_PERIOD`; the measurement shown stays that of the last
/// raw record, so every value belongs to one record and the statistics count it once.
pub fn hold(m: &Model, f: Option<&Frame>) -> bool {
    m.run == RunState::Run && m.trig_mode != TrigMode::Single && f.is_some_and(live)
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
        && (0..NCH).all(|c| !m.ch[c].on || (matches!(m.ch[c].coupling, Coupling::Dc | Coupling::Dc50) && m.ch[c].bw != BW_20M))
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
    if k < K_MIN || memsel > MAX_DEPTH { 1 } else { k }
}

/// A frame of the live compression (min/max pairs at most `MAX_DEPTH` deep), as opposed to the deep-memory read,
/// roll and scan.
pub fn live(f: &Frame) -> bool {
    f.envelope && !f.roll && !f.scan && f.acq.1 <= MAX_DEPTH as usize
}

/// Points of the moving average the trigger refinement runs on (`trignear::edge_smooth_near`): about one column, an
/// even number so that the average always holds whole pairs (an odd one moves the edge by half a point).
pub fn smooth_len(x_per_point: f64) -> usize {
    let col = (1.0 / x_per_point).floor().clamp(1.0, 256.0) as usize;
    (col / PAIR * PAIR).max(PAIR)
}

/// Per screen column (0..`w`) the minimum and maximum of the pairs whose block centre lies in it, mapped like
/// `fastdraw::deposit` maps samples (`m` in points; pair i covers points 2i and 2i+1, its centre is point 2i + 1).
pub fn ranges(v: &[f32], m: &Map, w: usize) -> Vec<Option<(f32, f32)>> {
    let mut out = vec![None; w];
    if m.x_per_sample <= 0.0 {
        return out;
    }
    for (i, p) in v.chunks(PAIR).enumerate() {
        let centre = (i * PAIR + PAIR / 2) as f64;
        let x = ((centre - m.ti) * m.x_per_sample + m.x0).floor();
        if x < 0.0 || x >= w as f64 {
            continue;
        }
        let (mn, mx) = p.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &y| (a.min(y), b.max(y)));
        if mn <= mx {
            let r = &mut out[x as usize];
            *r = Some(r.map_or((mn, mx), |(a, b)| (a.min(mn), b.max(mx))));
        }
    }
    out
}

/// `fastdraw::deposit` for a frame of min/max pairs: per column the span of its pairs (`ranges`), joined to the last
/// pair before the column where the two do not overlap (to its nearest end: the raw trace's last sample lies inside
/// that pair). With `dots` (`Model::dots`) no join: each column is the span its samples cover, as the record path
/// draws pairs in dots mode (`Phosphor::draw_ex`); the dot density of a raw frame is not in the pairs. Needs at least
/// one pair per column (`x_per_sample` <= 0.5).
pub fn deposit_pairs(map: &mut [u16], w: usize, h: usize, v: &[f32], m: &Map, wgt: u16, dots: bool) {
    let cols = ranges(v, m, w);
    let hh = h as i32;
    for (c, r) in cols.iter().enumerate() {
        let Some((mut mn, mut mx)) = *r else { continue };
        if let Some((pa, pb)) = last_before(v, m, c).filter(|_| !dots) {
            if pb < mn {
                mn = pb;
            } else if pa > mx {
                mx = pa;
            }
        }
        let (ya, yb) = (mn * m.y_per_volt + m.y0, mx * m.y_per_volt + m.y0);
        let r0 = (ya.min(yb).round() as i32).max(0);
        let r1 = (ya.max(yb).round() as i32).min(hh - 1);
        let mut i = r0.max(0) as usize * w + c;
        for _ in r0..=r1 {
            map[i] = map[i].saturating_add(wgt);
            i += w;
        }
    }
}

/// The range of the last pair whose centre lies left of column `c`.
fn last_before(v: &[f32], m: &Map, c: usize) -> Option<(f32, f32)> {
    // centre of pair i: PAIR * i + PAIR / 2 points, left of the column while below `edge`
    let edge = m.ti + (c as f64 - m.x0) / m.x_per_sample;
    let i = ((edge - (PAIR / 2) as f64) / PAIR as f64).ceil() as i64 - 1;
    let p = v.chunks(PAIR).nth(usize::try_from(i).ok()?)?;
    let (a, b) = p.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &y| (a.min(y), b.max(y)));
    (a <= b).then_some((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oos_hal::dho::pairs;

    /// The FPGA's compressed read of `raw` volts (codes 7500 per volt around 32768) with factor `k`, in volts, in the
    /// assumed layout (`pairs::LAYOUT`).
    fn fpga(raw: &[f32], k: usize) -> Vec<f32> {
        let codes: Vec<u16> = raw.iter().map(|&v| code(v)).collect();
        pairs::model(&codes, k, pairs::LAYOUT).into_iter().map(volt).collect()
    }

    fn code(v: f32) -> u16 {
        (32768.0 + v * 7500.0).round() as u16
    }

    fn volt(c: u16) -> f32 {
        (c as f32 - 32768.0) / 7500.0
    }

    /// Software min/max decimation of the raw record per screen column (the columns of `fastdraw::deposit`), without
    /// the join.
    fn soft(raw: &[f32], m: &Map, w: usize) -> Vec<Option<(f32, f32)>> {
        let first = |c: usize| ((m.ti + (c as f64 - m.x0) / m.x_per_sample).ceil().max(0.0) as usize).min(raw.len());
        (0..w)
            .map(|c| {
                let s = &raw[first(c)..first(c + 1).max(first(c))];
                let (a, b) = s.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &y| (a.min(y), b.max(y)));
                (a <= b).then_some((a, b))
            })
            .collect()
    }

    /// The same, over the samples from `pad` before to `pad` after the column (`pad` < 0 narrows it).
    fn soft_pad(raw: &[f32], m: &Map, w: usize, pad: f64) -> Vec<Option<(f32, f32)>> {
        let at = |c: usize, d: f64| ((m.ti + (c as f64 - m.x0) / m.x_per_sample + d).ceil().max(0.0) as usize).min(raw.len());
        (0..w)
            .map(|c| {
                let (a, b) = (at(c, -pad), at(c + 1, pad));
                let s = &raw[a.min(b)..b];
                let (a, b) = s.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &y| (a.min(y), b.max(y)));
                (a <= b).then_some((a, b))
            })
            .collect()
    }

    fn square(n: usize, period: f64, phase: f64, noise: f32) -> Vec<f32> {
        let mut x = 99u32;
        (0..n)
            .map(|i| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let hi = ((i as f64 + phase) / period).fract() < 0.5;
                let r = ((x >> 16) % 1000) as f32 / 1000.0 - 0.5;
                (if hi { 1.0 } else { -1.0 }) + noise * r
            })
            .map(|v| volt(code(v)))
            .collect()
    }

    fn sine(n: usize, period: f64) -> Vec<f32> {
        (0..n).map(|i| volt(code((i as f64 / period * std::f64::consts::TAU).sin() as f32 * 1.5))).collect()
    }

    const ONE_CODE: f32 = 1.0 / 7500.0 + 1e-6;

    fn close(a: Option<(f32, f32)>, b: Option<(f32, f32)>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => (a.0 - b.0).abs() <= ONE_CODE && (a.1 - b.1).abs() <= ONE_CODE,
            (None, None) => true,
            _ => false,
        }
    }

    // Break it catches: compressed pairs drawn into other columns than the samples they stand for, a pair split
    // between columns, extremes of a column lost (a min/max layout that drops one half of a block).
    #[test]
    fn aligned_columns_hold_the_software_min_max_of_the_raw_record() {
        let w = 970;
        for k in [4usize, 16, 64, 256] {
            let per_column = (POINTS_PER_COLUMN as usize * k) as f64;
            let n = (w as f64 * per_column) as usize;
            for raw in [square(n, 37.3 * per_column, 11.0, 0.0), square(n, 5.7 * per_column, 3.0, 0.2), sine(n, 13.1 * per_column)] {
                let comp = fpga(&raw, k);
                let mr = Map { ti: 0.0, x_per_sample: 1.0 / per_column, x0: 0.0, y_per_volt: -56.0, y0: 224.0 };
                let mc = Map { x_per_sample: 1.0 / POINTS_PER_COLUMN, ..mr };
                let (got, want) = (ranges(&comp, &mc, w), soft(&raw, &mr, w));
                for c in 0..w {
                    assert!(close(got[c], want[c]), "k {} column {}: {:?} vs {:?}", k, c, got[c], want[c]);
                }
            }
        }
    }

    // Break it catches: at the real geometry (fractional samples per column, any trigger phase) a pair that lands
    // more than its own half block away from its samples, or a column missing samples well inside it.
    #[test]
    fn unaligned_columns_stay_within_one_block_of_the_raw_columns() {
        let w = 970;
        for k in [4usize, 16, 64, 256] {
            let per_column = POINTS_PER_COLUMN * k as f64 * 1000.0 / 970.0 * 1.013;
            let n = (w as f64 * per_column) as usize + 4 * k;
            let raw = square(n, 7.7 * per_column, 5.0, 0.3);
            let comp = fpga(&raw, k);
            for ti in [0.0, 0.37 * k as f64, 1.61 * k as f64] {
                let mr = Map { ti, x_per_sample: 1.0 / per_column, x0: 0.0, y_per_volt: -56.0, y0: 224.0 };
                let mc = Map { ti: ti / k as f64, x_per_sample: k as f64 / per_column, ..mr };
                let got = ranges(&comp, &mc, w);
                // a pair stands for 2k samples around its centre: at most k beyond the column on either side
                let half = (pairs::BLOCK_K * k) as f64 / 2.0;
                let (wide, narrow) = (soft_pad(&raw, &mr, w, half + 1.0), soft_pad(&raw, &mr, w, -(half + 1.0)));
                for c in 1..w - 1 {
                    let Some((a, b)) = got[c] else { panic!("k {} column {} empty", k, c) };
                    let (wa, wb) = wide[c].unwrap();
                    assert!(a >= wa - ONE_CODE && b <= wb + ONE_CODE, "k {} ti {} column {}: {:?} outside {:?}", k, ti, c, got[c], wide[c]);
                    if let Some((na, nb)) = narrow[c] {
                        assert!(a <= na + ONE_CODE && b >= nb - ONE_CODE, "k {} ti {} column {}: {:?} inside {:?}", k, ti, c, got[c], narrow[c]);
                    }
                }
            }
        }
    }

    // Break it catches: a single-sample glitch lost by the compression or the drawing (the reason for min/max pairs
    // rather than decimation).
    #[test]
    fn a_single_sample_glitch_survives_in_its_column() {
        let w = 970;
        for k in [4usize, 16, 64, 256] {
            let per_column = POINTS_PER_COLUMN * k as f64;
            let n = (w as f64 * per_column) as usize;
            for (j, at) in [n / 3, n / 2 + 1, 2 * n / 3 + 3, n / 5 + k - 1].into_iter().enumerate() {
                let mut raw = vec![0.0f32; n];
                let up = j % 2 == 0;
                raw[at] = if up { 2.5 } else { -2.5 };
                let comp = fpga(&raw, k);
                let m = Map { ti: 0.0, x_per_sample: 1.0 / POINTS_PER_COLUMN, x0: 0.0, y_per_volt: -56.0, y0: 224.0 };
                let c = (at as f64 / per_column) as usize;
                let got = ranges(&comp, &m, w)[c].unwrap();
                assert!(if up { got.1 >= 2.5 - ONE_CODE } else { got.0 <= -2.5 + ONE_CODE }, "k {} glitch at {}: column {} {:?}", k, at, c, got);
                // in the picture: the glitch row lit in its column
                let (h, mut map) = (448usize, vec![0u16; w * 448]);
                deposit_pairs(&mut map, w, h, &comp, &m, 24, false);
                let row = (raw[at] * m.y_per_volt + m.y0).round() as usize;
                assert!(map[row * w + c] > 0, "k {}: glitch row {} not lit in column {}", k, row, c);
                // and nowhere else but the columns next to it
                let lit: Vec<usize> = (0..w).filter(|&x| map[row * w + x] > 0).collect();
                assert!(lit.iter().all(|&x| x.abs_diff(c) <= 1), "k {}: {:?}", k, lit);
            }
        }
    }

    // Break it catches: a picture of the pairs that differs from the raw fast path (`fastdraw::deposit`) on clean
    // signals: the join drawing an extra column at edges, or spans one row off.
    #[test]
    fn picture_matches_the_raw_fast_path_on_clean_signals() {
        let (w, h) = (970usize, 448usize);
        for k in [4usize, 16, 64, 256] {
            let per_column = POINTS_PER_COLUMN * k as f64;
            let n = (w as f64 * per_column) as usize;
            for raw in [square(n, 37.3 * per_column, 11.0, 0.0), sine(n, 13.1 * per_column)] {
                let comp = fpga(&raw, k);
                let mr = Map { ti: 0.0, x_per_sample: 1.0 / per_column, x0: 0.0, y_per_volt: -56.0, y0: 224.0 };
                let mc = Map { x_per_sample: 1.0 / POINTS_PER_COLUMN, ..mr };
                let (mut a, mut b) = (vec![0u16; w * h], vec![0u16; w * h]);
                crate::fastdraw::deposit(&mut a, w, h, &raw, &mr, 1);
                deposit_pairs(&mut b, w, h, &comp, &mc, 1, false);
                let span = |m: &[u16], c: usize| {
                    let rows: Vec<usize> = (0..h).filter(|&r| m[r * w + c] > 0).collect();
                    rows.first().map(|&f| (f, *rows.last().unwrap()))
                };
                for c in 0..w {
                    let (x, y) = (span(&a, c), span(&b, c));
                    let ok = match (x, y) {
                        (Some(x), Some(y)) => x.0.abs_diff(y.0) <= 1 && x.1.abs_diff(y.1) <= 1,
                        (x, y) => x == y,
                    };
                    assert!(ok, "k {} column {}: raw {:?} pairs {:?}", k, c, x, y);
                }
            }
        }
    }

    /// The per-column lit row span of channel 0 of a picture.
    fn spans(ph: &crate::wave::Phosphor) -> Vec<Option<(usize, usize)>> {
        let (w, h) = (ph.w, ph.h);
        (0..w)
            .map(|c| {
                let rows: Vec<usize> = (0..h).filter(|&r| ph.map[0][r * w + c] > 0).collect();
                rows.first().map(|&f| (f, *rows.last().unwrap()))
            })
            .collect()
    }

    /// 10 us/div, CH1: 100 k points at 625 MSa/s (k = 15), a 40 kHz square of 3 div peak to peak with its rising
    /// edge `jitter` samples after the trigger point and a little noise: the model, the raw read-out bytes and settings,
    /// the compressed ones.
    #[allow(clippy::type_complexity)]
    fn square_10us(jitter: f64) -> (Model, oos_hal::dho::hz::Params, (Vec<u8>, oos_hal::dho::seq::Settings), (Vec<u8>, oos_hal::dho::seq::Settings)) {
        use oos_hal::dho::{hz, seq::Settings};
        let p = hz::Params { ratio: 1, rate: 625_000_000, memsel: 100_000, dot_fs: 1_600_000, seg: 16384, max_frames: 448, pre: 50_000, post: 50_000 };
        let k = k_for(p.rate, 10e-6, p.memsel) as usize;
        assert_eq!(k, 15);
        let mut m = Model::default();
        m.tscale = tscales().iter().position(|&t| (t as f64 - 10e-6).abs() < 1e-11).unwrap();
        (m.trig_level, m.ch[0].on, m.ch[0].offset) = (0.0, true, 0.0);
        let vpd = m.ch[0].volts_per_div() as f64;
        let mut x = 7u32;
        let codes: Vec<u16> = (0..p.memsel as usize)
            .map(|i| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let t = i as f64 - p.pre as f64 - jitter;
                let hi = (t / 15_625.0).rem_euclid(1.0) < 0.5;
                let v = if hi { 1.5 } else { -1.5 } * vpd + ((x >> 16) % 100) as f64 / 100.0 * 0.02 * vpd;
                (32768.0 + v / vpd * 7500.0).round() as u16
            })
            .collect();
        let raw_s = Settings { mask: 1, window: None, compress: 1, header: false, ch: [oos_hal::dho::seq::Chan { scale: vpd, offset: 0.0, ..Settings::default().ch[0] }; 4], ..Settings::default() };
        let comp_s = Settings { compress: k as u32, ..raw_s.clone() };
        let raw_bytes: Vec<u8> = codes.iter().flat_map(|c| c.to_le_bytes()).collect();
        let mut comp = pairs::model(&codes, k, pairs::LAYOUT);
        comp.resize(oos_hal::dho::seq::compressed_len(&comp_s, &p) as usize, *comp.last().unwrap());
        let comp_bytes: Vec<u8> = comp.iter().flat_map(|c| c.to_le_bytes()).collect();
        (m, p, (raw_bytes, raw_s), (comp_bytes, comp_s))
    }

    // Break it catches, end to end through `drawpipe::draw_frame` (decode, trigger refinement on the pairs, deposit):
    // a compressed frame drawn at another place than its raw frame, a trigger not found on pairs, the pair path not
    // taken (the record path would differ in many columns).
    #[test]
    fn draw_frame_of_pairs_matches_the_raw_frame() {
        let (w, h) = (970usize, 448usize);
        for (dots, jitter) in [(false, 0.0), (false, 7.3), (true, 3.1)] {
            let (mut m, p, (raw_bytes, raw_s), (comp_bytes, comp_s)) = square_10us(jitter);
            m.dots = dots;
            let fr = crate::board::dho::hw::decode(&raw_bytes, &raw_s, &p, 1);
            let fc = crate::board::dho::hw::decode(&comp_bytes, &comp_s, &p, 1);
            assert!(live(&fc) && !live(&fr));
            let (mut a, mut b) = (crate::wave::Phosphor::new(w, h), crate::wave::Phosphor::new(w, h));
            assert!(crate::drawpipe::draw_frame(&mut a, &m, &fr, None));
            assert!(crate::drawpipe::draw_frame(&mut b, &m, &fc, None));
            let (sa, sb) = (spans(&a), spans(&b));
            let off: Vec<usize> = (0..w)
                .filter(|&c| match (sa[c], sb[c]) {
                    (Some(x), Some(y)) => x.0.abs_diff(y.0) > 1 || x.1.abs_diff(y.1) > 1,
                    (x, y) => x != y,
                })
                .collect();
            // 6.4 periods on screen, 13 edges: an edge may move by one column where the trigger on pairs (good to
            // about a pair, a quarter column) lands on the other side of a column boundary. Host: 0, 2 and 4 columns;
            // the record path drew 8 to 13 columns differently.
            assert!(off.len() <= 4, "dots {} jitter {}: {} columns differ: {:?}", dots, jitter, off.len(), &off[..off.len().min(40)]);
        }
    }

    // Timing guard (tools/perf-guard.sh): decode + draw of one compressed frame against its raw frame at 10 us/div
    // (k = 15). The batch gain is the read size (15x fewer bytes); the host work per frame must shrink with it.
    #[test]
    #[ignore]
    fn perf_guard_compressed_frame() {
        let (m, p, (raw_bytes, raw_s), (comp_bytes, comp_s)) = square_10us(2.0);
        let mut ph = crate::wave::Phosphor::new(970, 448);
        let time = |b: &[u8], s: &oos_hal::dho::seq::Settings, ph: &mut crate::wave::Phosphor| {
            let n = 200;
            let t0 = std::time::Instant::now();
            for _ in 0..n {
                std::hint::black_box(crate::drawpipe::draw_frame(ph, &m, &crate::board::dho::hw::decode(b, s, &p, 0), None));
            }
            t0.elapsed() / n
        };
        let (raw, comp) = (time(&raw_bytes, &raw_s, &mut ph), time(&comp_bytes, &comp_s, &mut ph));
        let r = comp.as_secs_f64() / raw.as_secs_f64();
        println!("10 us/div 100 k pt, k = 15: raw {:?}/frame, compressed {:?}/frame (ratio {:.3}); read {} -> {} bytes", raw, comp, r, raw_bytes.len(), comp_bytes.len());
        assert!(r <= 0.25, "a compressed frame must cost far less than its raw frame: {:.3}", r);
    }

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
    fn pairs_of_the_board() {
        assert_eq!((PAIR, pairs::LAYOUT.shift, K_MAX), (pairs::PAIR, 0, oos_hal::dho::seq::COMPRESS_K_MAX));
    }

    #[test]
    fn smoothing_holds_whole_pairs() {
        assert_eq!(smooth_len(0.25), 4);
        assert_eq!(smooth_len(0.2), 4);
        assert_eq!(smooth_len(0.9), 2);
        assert_eq!(smooth_len(1e-6), 256);
    }

    #[test]
    fn hold_only_running_on_live_pairs() {
        let mut m = Model::default();
        let f = |envelope: bool, depth: usize| Frame { envelope, acq: (625e6, depth), ..Default::default() };
        assert!(hold(&m, Some(&f(true, 100_000))));
        assert!(!hold(&m, Some(&f(false, 100_000))), "a raw record is measured");
        assert!(!hold(&m, Some(&f(true, 10_000_000))), "deep pairs: deepmeas.rs");
        assert!(!hold(&m, None));
        m.run = RunState::Stop;
        assert!(!hold(&m, Some(&f(true, 100_000))));
        (m.run, m.trig_mode) = (RunState::Run, TrigMode::Single);
        assert!(!hold(&m, Some(&f(true, 100_000))));
    }

    #[test]
    fn opt_in_and_the_model_conditions() {
        assert!(!enabled_from(None, ""));
        assert!(enabled_from(None, "acq.compress = on\n"));
        assert!(!enabled_from(Some("off"), "acq.compress = on\n"));
        let mut m = Model::default();
        m.ch[0].on = true;
        assert!(allowed(&m));
        m.dots = false;
        assert!(allowed(&m));
        for f in [|m: &mut Model| m.trig_mode = TrigMode::Single, |m: &mut Model| m.acq = AcqMode::Average, |m: &mut Model| m.acq = AcqMode::HiRes, |m: &mut Model| m.acq = AcqMode::Peak, |m: &mut Model| m.zoom = true, |m: &mut Model| m.mask_on = true, |m: &mut Model| m.segments = 4, |m: &mut Model| m.trig_type = TrigType::Pulse, |m: &mut Model| m.ch[0].bw = BW_20M, |m: &mut Model| m.ch[0].coupling = Coupling::Ac] {
            let mut x = m.clone();
            f(&mut x);
            assert!(!allowed(&x));
        }
    }
}
