// SPDX-License-Identifier: GPL-2.0-or-later
//! Source time calibration. Original file/acquisition metadata and samples stay intact.
//! Auto uses at most 65536 native points around the visible center per shown trace.
use super::*;

const EDGE_WINDOW: usize = 65_536;

/// Nearest mid-level transitions in a bounded native window. Working on native
/// neighbors preserves interpolation accuracy; subsampling fast edges would not.
fn edge_times(v: &[f32], dt: f64, t0: f64, center: f64) -> (Option<f64>, Option<f64>) {
    if v.len() < 4 || !dt.is_finite() || dt <= 0.0 || !t0.is_finite() { return (None, None); }
    let at = ((center - t0) / dt).max(0.0).min(v.len() as f64 - 1.0) as usize;
    let start = at.saturating_sub(EDGE_WINDOW / 2).min(v.len().saturating_sub(EDGE_WINDOW));
    let window = &v[start..(start + EDGE_WINDOW).min(v.len())];
    let (lo, hi) = window.iter().copied().filter(|v| v.is_finite())
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
    if !lo.is_finite() || hi - lo <= 1e-6 { return (None, None); }
    let middle = (lo as f64 + hi as f64) * 0.5;
    let hysteresis = (hi - lo) as f64 * 0.08;
    let mut state = None;
    let mut crossing = None;
    let (mut rise, mut fall) = (None, None);
    let nearest = |old: Option<f64>, time: f64| old.is_none_or(|old| (time - center).abs() < (old - center).abs());
    for (i, pair) in window.windows(2).enumerate() {
        let (a, b) = (pair[0] as f64, pair[1] as f64);
        if !a.is_finite() || !b.is_finite() { state = None; crossing = None; continue; }
        if state.is_none() {
            if a < middle - hysteresis { state = Some(false); }
            else if a > middle + hysteresis { state = Some(true); }
        }
        if (state == Some(false) && a < middle && b >= middle) || (state == Some(true) && a > middle && b <= middle) {
            crossing = Some(t0 + (start as f64 + i as f64 + (middle - a) / (b - a)) * dt);
        }
        if (state == Some(false) && b >= middle + hysteresis) || (state == Some(true) && b <= middle - hysteresis) {
            let rising = state == Some(false);
            if let Some(time) = crossing.take() {
                let best = if rising { &mut rise } else { &mut fall };
                if nearest(*best, time) { *best = Some(time); }
            }
            state = Some(rising);
        }
    }
    (rise, fall)
}

impl Ui {
    pub(super) fn source_time_shift(&self, source: WaveId) -> f64 {
        match source { WaveId::Ch(c) => self.m.deskew_ch.get(c), WaveId::Ref(i) => self.m.deskew_ref.get(i) }.copied().unwrap_or(0.0)
    }
    pub(super) fn source_time_origin(&self, source: WaveId, native_t0: f64) -> f64 { native_t0 - self.source_time_shift(source) }
    pub(super) fn deskew_channel_view(&self, record: &Record, channel: usize, view: (f32, f32)) -> Option<std::sync::Arc<Record>> {
        let shift = self.m.deskew_ch[channel];
        if shift == 0.0 { return None; }
        self.captured_record_view(record, (view.0, (view.1 as f64 + shift) as f32))
    }

    pub(super) fn deskew_samples(&self, source: WaveId) -> Option<(&[f32], f64, f64)> {
        match source {
            WaveId::Ref(i) => { let r = self.refs.get(i)?.as_ref()?; Some((&r.v, r.dt, self.source_time_origin(source, r.t0))) }
            WaveId::Ch(c) => {
                let r = self.last[0].as_ref()?;
                if let Some(raw) = r.raw.as_ref().filter(|raw| !raw.frame.envelope && raw.frame.ch[c].len() >= 4) {
                    Some((&raw.frame.ch[c], raw.frame.dt, self.source_time_origin(source, raw.t0 - raw.i0 as f64 * raw.frame.dt)))
                } else { Some((&r.ch[c], r.dt, self.source_time_origin(source, r.t0))) }
            }
        }
    }
    pub(super) fn auto_deskew(&mut self) {
        let center = self.view().1 as f64;
        let sources = (0..n_ch()).filter(|&c| self.m.ch[c].on).map(WaveId::Ch)
            .chain((0..NREF).filter(|&i| self.refs[i].as_ref().is_some_and(|r| r.on)).map(WaveId::Ref));
        let edges: Vec<_> = sources.filter_map(|id| {
            let (v, dt, t0) = self.deskew_samples(id)?;
            let (rise, fall) = edge_times(v, dt, t0, center); Some((id, rise, fall))
        }).collect();
        let rises = edges.iter().filter(|e| e.1.is_some()).count();
        let falls = edges.iter().filter(|e| e.2.is_some()).count();
        if rises.max(falls) < 2 { self.toast("Need two shown traces with an edge near the view"); return; }
        let rising = rises >= falls;
        let times: Vec<_> = edges.into_iter().filter_map(|(id, rise, fall)| (if rising { rise } else { fall }).map(|time| (id, time))).collect();
        let anchor = self.selected_workspace_source().and_then(|selected| times.iter().find(|e| e.0 == selected))
            .unwrap_or(&times[0]).1;
        for &(id, time) in &times {
            let delta = self.source_time_shift(id) + time - anchor;
            match id { WaveId::Ch(c) => self.m.deskew_ch[c] = delta, WaveId::Ref(i) => self.m.deskew_ref[i] = delta }
        }
        self.deskew_changed();
        self.toast(&format!("Deskewed {} shown traces near the view", times.len()));
    }
    pub(super) fn clear_deskew(&mut self) {
        self.m.deskew_ch.fill(0.0); self.m.deskew_ref.fill(0.0); self.deskew_changed(); self.toast("Source deskew cleared");
    }
    fn deskew_changed(&mut self) {
        self.ph.clear(); self.phz.clear(); self.dirty_map = true;
        self.replay.viewport = None;
        // Time is part of every downstream gate/search/math/decode key. Do not
        // call settings_changed: this is calibration, not a new acquisition.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(t0: f64, on: bool) -> RefWave {
        RefWave { src: 0, color: 0, label: "timing fixture".into(), t0, dt: 1e-9,
            v: ((0..500).map(|i| if i % 40 < 20 { -1.0 } else { 1.0 }).collect::<Vec<_>>()).into(),
            vpd: 0.5, offset: 0.0, on }
    }
    fn stopped() -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.set_offline(true);
        ui.refs[0] = Some(reference(-250e-9, true));
        ui.refs[1] = Some(reference(-247e-9, true));
        ui.refs[2] = Some(reference(-247e-9, false));
        ui.m.deskew_ref[2] = 17e-9;
        ui.m.meas_full_record = true;
        ui.m.trig_level = 0.0;
        ui.m.delay = 0.0;
        ui.m.zoom = false;
        ui.select_workspace_source(WaveId::Ref(0));
        ui
    }

    #[test]
    fn auto_calibration_preserves_hidden_offsets_samples_native_metadata_and_acquisition() {
        let mut ui = stopped();
        let native = ui.refs[1].as_ref().unwrap().clone();
        let state = (ui.m.run, ui.m.status, ui.replay.captures, ui.wfm_total, ui.m.trigger());
        assert!((ui.pair_at(0, WaveId::Ref(0), WaveId::Ref(1)) - 3e-9).abs() < 1e-15);
        let before = ui.reference_trigger_hits(1).unwrap();
        ui.auto_deskew();
        assert!((ui.m.deskew_ref[1] - 3e-9).abs() < 1e-15);
        assert_eq!(ui.m.deskew_ref[2], 17e-9, "a hidden nonzero calibration is retained");
        assert_eq!((ui.m.run, ui.m.status, ui.replay.captures, ui.wfm_total, ui.m.trigger()), state);
        let after = ui.refs[1].as_ref().unwrap();
        assert_eq!((after.t0, after.dt, &after.v), (native.t0, native.dt, &native.v));
        assert!(ui.pair_at(0, WaveId::Ref(0), WaveId::Ref(1)).abs() < 1e-15, "pair cache uses corrected origins");
        assert!((ui.reference_trigger_hits(1).unwrap()[0] - before[0] + 3e-9).abs() < 1e-15, "Search uses the same calibrated time");
        ui.select_workspace_source(WaveId::Ref(1));
        assert!(ui.wave_at(9.5e-9).unwrap().abs() < 1e-5, "tracking samples use corrected physical time");
        ui.m.math = MathOp::Sub; ui.m.math_a = crate::math::MATH_REF_BASE; ui.m.math_b = crate::math::MATH_REF_BASE + 1;
        let math = ui.math_record(0).unwrap().unwrap();
        assert!(math.v.iter().all(|v| v.abs() < 1e-5), "math alignment follows the same source calibration");
        let text = crate::store::setup_text(&ui.m);
        let mut restored = crate::store::parse_setup(&text).unwrap();
        assert_eq!(restored.deskew_ref, ui.m.deskew_ref);
        assert!(crate::store::set(&mut restored, "deskew_ref1", "NaN").is_err());
    }

    #[test]
    fn native_edge_window_is_bounded_and_interpolates_slow_crossings() {
        let dt = 5e-12;
        let mut long = vec![f32::NAN; 2_000_000];
        let center = 1_000_000;
        for i in center - EDGE_WINDOW / 2..center + EDGE_WINDOW / 2 {
            long[i] = ((i as isize - center as isize) as f32 / 1000.0).clamp(-1.0, 1.0);
        }
        let time = center as f64 * dt;
        let actual = edge_times(&long, dt, 0.0, time).0.unwrap();
        assert!((actual - time).abs() < dt * 1e-6);
        long[0] = -1e30; long[1_999_999] = 1e30;
        assert_eq!(edge_times(&long, dt, 0.0, time).0, Some(actual), "outside the bounded native window cannot affect the level or result");
        assert_eq!(edge_times(&[f32::NAN; 10], dt, 0.0, 0.0), (None, None));
    }

    #[test]
    fn screen_gate_and_native_channel_origin_follow_calibration_without_rewriting_samples() {
        let mut ui = stopped();
        ui.m.meas_full_record = false;
        ui.m.tscale = tscales().iter().position(|&t| (t - 1e-9).abs() < 1e-15).unwrap();
        ui.m.delay = -228e-9;
        let before = ui.meas_at(WaveId::Ref(1), Meas::Vavg);
        ui.m.deskew_ref[1] = 3e-9;
        let after = ui.meas_at(WaveId::Ref(1), Meas::Vavg);
        assert!(before.is_finite() && after.is_finite() && (before - after).abs() > 0.1);
        ui.offline = false; ui.m.ch[0].on = true;
        let record = Record { t0: 10e-9, dt: 1e-9,
            ch: [(0..64).map(|i| i as f32).collect(), Vec::new(), Vec::new(), Vec::new()],
            raw: None, la: None, partial: false, triggered: true };
        ui.last[0] = Some(record);
        ui.m.meas_full_record = true; ui.m.deskew_ch[0] = 2e-9;
        let (_, origin) = ui.meas_analysis(WaveId::Ch(0)).unwrap();
        assert!((origin - 8e-9).abs() < 1e-20);
        assert_eq!(ui.record_info(0).unwrap().2, 10e-9, "public native metadata stays original");
        ui.select_workspace_source(WaveId::Ch(0));
        assert!((ui.wave_at(9e-9).unwrap() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn stacked_auto_aligns_all_enabled_channels_not_only_the_current_draw_lane() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.ch[1].on = true; ui.m.run = RunState::Stop; ui.m.delay = 0.0;
        ui.workspace.stacked = true; ui.relayout(); ui.cur_win = 1;
        let a = reference(-250e-9, true).v;
        let b: Vec<f32> = (0..a.len()).map(|i| a[i.saturating_sub(3)]).collect();
        ui.last[0] = Some(Record { t0: -250e-9, dt: 1e-9, ch: [a.as_ref().clone(), b, Vec::new(), Vec::new()],
            raw: None, la: None, partial: false, triggered: true });
        ui.m.deskew_ch[2] = 9e-9;
        ui.select_workspace_source(WaveId::Ch(0));
        ui.auto_deskew();
        assert!((ui.m.deskew_ch[1] - 3e-9).abs() < 1e-15);
        assert_eq!(ui.m.deskew_ch[2], 9e-9, "disabled ADC calibration is preserved");
        assert_eq!(ui.m.run, RunState::Stop); assert_eq!(ui.wfm_total, 0);
    }
}
