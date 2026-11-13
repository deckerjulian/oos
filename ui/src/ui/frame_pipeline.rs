// SPDX-License-Identifier: GPL-2.0-or-later
//! Every dense hardware frame is drawn and summarized; only composition is per refresh.
use super::*;
use std::sync::Arc;

struct Outcome { triggered: bool, diagnostic: usize, offset: f64, basic: [crate::board::BasicStats; NCH] }
#[derive(Default)]
struct Buffer { maps: [Vec<u16>; NCH], outcomes: Vec<Outcome> }
#[derive(Default)]
pub(super) struct State { buffers: Vec<Buffer> }

fn process(buffer: &mut Buffer, frames: &[Arc<crate::board::Frame>], model: &Model, w: usize, h: usize) {
    for c in 0..NCH {
        if model.ch[c].on && model.persist != 0 { buffer.maps[c].resize(w * h, 0); buffer.maps[c].fill(0); }
        else { buffer.maps[c].clear(); }
    }
    buffer.outcomes.clear();
    let tdiv = model.hw_time_per_div() as f64;
    for frame in frames {
        let x_per_sample = frame.dt / tdiv * w as f64 / HDIV as f64;
        let half = (tdiv * HDIV as f64 * 0.5 / frame.dt) as usize;
        let col = (1.0 / x_per_sample).floor().clamp(1.0, 256.0) as usize;
        let rising = model.slope != Slope::Fall;
        let either = model.slope == Slope::Either;
        let on_la = model.trig_on_la();
        let (edge, any) = if on_la { (None, None) } else {
            crate::fastdraw::edge_smooth(&frame.ch[model.trig_src], frame.pre, model.trig_level,
                model.trig_hyst_div() * model.ch[model.trig_src].volts_per_div(), rising, either, half, col, |_| true)
        };
        let diagnostic = match (edge, any) {
            (None, _) => 3,
            (Some(t), _) if (t - frame.pre as f64).abs() <= (col + 16) as f64 => 0,
            (Some(_), Some(e)) if !either && e.rising != rising => 1,
            _ => 2,
        };
        let ti = edge.unwrap_or(frame.pre as f64);
        let mut basic = [crate::board::BasicStats::default(); NCH];
        for c in 0..NCH {
            let ch = &model.ch[c];
            let values = &frame.ch[c];
            if !ch.on || values.is_empty() { continue; }
            let (a, b) = if model.meas_full_record { (0, values.len()) } else {
                let origin = -ti * frame.dt;
                let half = model.time_per_div() as f64 * HDIV as f64 * 0.5;
                let at = |t: f64| (((t - origin) / frame.dt).ceil().max(0.0) as usize).min(values.len());
                (at(model.delay as f64 - half), at(model.delay as f64 + half))
            };
            if b > a {
                basic[c] = if a == 0 && b == values.len() && frame.basic[c].valid { frame.basic[c] }
                    else { crate::board::BasicStats::of(&values[a..b]) };
            }
            if model.persist == 0 { continue; }
            let map = crate::fastdraw::Map { ti, x_per_sample,
                x0: w as f64 * 0.5 - model.delay as f64 / tdiv * w as f64 / HDIV as f64,
                y_per_volt: -(h as f32 / VDIV) / ch.volts_per_div(),
                y0: h as f32 * 0.5 - ch.offset / ch.volts_per_div() * h as f32 / VDIV };
            if model.dots { crate::fastdraw::deposit_dots(&mut buffer.maps[c], w, h, values, &map, 24); }
            else { crate::fastdraw::deposit(&mut buffer.maps[c], w, h, values, &map, 24); }
        }
        buffer.outcomes.push(Outcome { triggered: edge.is_some() || on_la, diagnostic,
            offset: (ti - frame.pre as f64).abs(), basic });
    }
}

impl Ui {
    pub(super) fn process_fast_frames(&mut self, frames: &[Arc<crate::board::Frame>]) {
        let dense = frames.iter().all(|f| !f.envelope && !f.roll && !f.scan && f.dt > 0.0
            && f.dt / self.m.hw_time_per_div() as f64 * self.ph.w as f64 / HDIV as f64 <= 0.5);
        if frames.len() < 2 || !dense {
            for frame in frames { if self.fast_draw(frame) { self.m.status = TrigStatus::Triggered; } }
            return;
        }
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(4).min(frames.len());
        let chunk = frames.len().div_ceil(workers);
        self.frame_pipeline.buffers.resize_with(frames.len().div_ceil(chunk), Buffer::default);
        let (w, h) = (self.ph.w, self.ph.h);
        let model = &self.m;
        std::thread::scope(|scope| {
            for (buffer, batch) in self.frame_pipeline.buffers.iter_mut().zip(frames.chunks(chunk)) {
                scope.spawn(move || process(buffer, batch, model, w, h));
            }
        });
        // Merge in acquisition order; saturating addition is deterministic.
        let mut outcomes = Vec::new();
        for buffer in &mut self.frame_pipeline.buffers {
            if self.m.persist != 0 {
                for c in 0..NCH {
                    for (out, value) in self.ph.map[c].iter_mut().zip(&buffer.maps[c]) { *out = out.saturating_add(*value); }
                }
            }
            outcomes.append(&mut buffer.outcomes);
        }
        for outcome in outcomes {
            if outcome.triggered { self.m.status = TrigStatus::Triggered; }
            self.fast_edges[outcome.diagnostic] += 1;
            if outcome.diagnostic == 2 {
                self.fast_off.0 += outcome.offset; self.fast_off.1 = self.fast_off.1.max(outcome.offset);
            }
            for c in 0..NCH { self.accumulate_basic_summary(c, outcome.basic[c]); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parallel_frames_match_serial_pixels_and_every_basic_statistic() {
        let setup = || {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.persist = 2;
            ui.m.meas_full_record = true;
            ui.m.tscale = tscales().iter().position(|&t| (t - 1e-6).abs() < 1e-12).unwrap();
            ui.m.trig_level = 0.0;
            ui.m.meas = [Meas::Vmax, Meas::Vmin, Meas::Vpp, Meas::Vavg, Meas::Vrms, Meas::StdDev]
                .into_iter().map(|k| (k, WaveId::Ch(0))).collect();
            ui
        };
        let frames: Vec<_> = (0..193).map(|seq| {
            let v: Vec<_> = (0..10_000).map(|i| if (i + seq as usize) % 100 < 50 { -0.5 } else { 0.5 }).collect();
            let mut frame = crate::board::Frame { seq, dt: 1e-9, pre: 5000, ch: [v, Vec::new(), Vec::new(), Vec::new()], ..Default::default() };
            frame.basic[0] = crate::board::BasicStats::of(&frame.ch[0]);
            Arc::new(frame)
        }).collect();
        let mut serial = setup();
        for frame in &frames { serial.fast_draw(frame); }
        let mut parallel = setup();
        parallel.process_fast_frames(&frames);
        assert_eq!(parallel.ph.map, serial.ph.map);
        assert_eq!(parallel.measurement_statistics_snapshot_for_test(), serial.measurement_statistics_snapshot_for_test());
        assert!(parallel.mstat.iter().all(|s| s.n == 193));
        parallel.m.persist = 0;
        parallel.mstat.clear();
        parallel.ph.clear();
        parallel.process_fast_frames(&frames);
        assert!(parallel.ph.map.iter().all(|m| m.iter().all(|&v| v == 0)));
        assert!(parallel.mstat.iter().all(|s| s.n == 193));
    }

    #[test]
    #[ignore = "Explicit optimized host CPU benchmark; no hardware or window"]
    fn benchmark_parallel_frame_deposit_host() {
        let mut frames = Vec::new();
        for seq in 0..48 {
            let ch = std::array::from_fn(|c| (0..100_000).map(|i| {
                if (i + seq as usize + c * 7) % 200 < 100 { -0.5 } else { 0.5 }
            }).collect::<Vec<_>>());
            let basic = std::array::from_fn(|c| crate::board::BasicStats::of(&ch[c]));
            frames.push(Arc::new(crate::board::Frame { seq, dt: 1e-9, pre: 50_000, ch, basic, ..Default::default() }));
        }
        let make = || {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.persist = 2;
            ui.m.meas_full_record = true;
            for ch in &mut ui.m.ch { ch.on = true; }
            ui.m.tscale = tscales().iter().position(|&t| (t - 10e-6).abs() < 1e-12).unwrap();
            ui.m.trig_level = 0.0;
            ui.m.meas = [Meas::Vmax, Meas::Vmin, Meas::Vpp, Meas::Vavg, Meas::Vrms, Meas::StdDev]
                .into_iter().flat_map(|k| (0..NCH).map(move |c| (k, WaveId::Ch(c)))).collect();
            ui
        };
        let (mut serial, mut parallel) = (make(), make());
        // Warm worker buffers and code paths before measuring steady-state cost.
        parallel.process_fast_frames(&frames);
        let (mut st, mut pt) = (Vec::new(), Vec::new());
        for _ in 0..5 {
            serial.ph.clear(); serial.mstat.clear();
            let now = std::time::Instant::now();
            for frame in &frames { serial.fast_draw(frame); }
            st.push(now.elapsed().as_secs_f64() * 1000.0);
            parallel.ph.clear(); parallel.mstat.clear();
            let now = std::time::Instant::now();
            parallel.process_fast_frames(&frames);
            pt.push(now.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(serial.ph.map, parallel.ph.map);
            assert_eq!(serial.measurement_statistics_snapshot_for_test(), parallel.measurement_statistics_snapshot_for_test());
        }
        st.sort_by(f64::total_cmp); pt.sort_by(f64::total_cmp);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../local/ref-integration/frame-deposit-host.json");
        std::fs::write(path, format!("{{\"frames\":48,\"channels\":4,\"points_per_channel\":100000,\"serial_median_ms\":{},\"parallel_median_ms\":{},\"speedup\":{},\"scope\":\"Windows host CPU; exact pixels and six-stat counts\"}}", st[2], pt[2], st[2] / pt[2])).unwrap();
        eprintln!("48x4x100k frame deposit: serial {:.3}ms parallel {:.3}ms speedup {:.3}x", st[2], pt[2], st[2] / pt[2]);
    }
}
