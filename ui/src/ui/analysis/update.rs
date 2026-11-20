// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The per-record update of the analysers: the spectrum with its lines, Identify and the protocol watch, and power
//! analysis, run from `Ui::analyzer_update` on every captured sample of the window. A new analyser's computation is
//! called from `analyzer_update` through its page in `analysers.rs`.

use super::*;

/// A bus a finding names ("UART 115200 on CH3"), None for plain signals.
fn bus_name(f: &identify::Finding) -> Option<String> {
    use identify::SignalKind as K;
    let chs = f.channels.iter().map(|c| format!("CH{}", c + 1)).collect::<Vec<_>>().join("+");
    let what = match &f.kind {
        K::Uart { baud, .. } => format!("UART {:.0} Bd", baud),
        K::I2c { .. } => "I2C".to_string(),
        K::Spi { mode, .. } => format!("SPI mode {}", mode),
        K::Can { bitrate, .. } => format!("CAN {}", si_short(*bitrate, "b/s")),
        K::Lin { baud } => format!("LIN {:.0} Bd", baud),
        K::OneWire => "1-Wire".to_string(),
        K::Manchester { bitrate } => format!("Manchester {}", si_short(*bitrate, "b/s")),
        K::Video { .. } => "video".to_string(),
        _ => return None,
    };
    Some(format!("{} on {}", what, chs))
}

impl Ui {
    /// The strongest spectral line above DC.
    pub(in crate::ui) fn strongest_line(&self) -> Option<spectrum::Marker> {
        self.azr.peaks.iter().copied().max_by(|a, b| a.dbv.total_cmp(&b.dbv))
    }

    /// The spectrum is on the screen and its span is left to the signal.
    pub(super) fn spec_follows(&self) -> bool {
        let shown = self.m.view == AView::Spectrum
            || self.m.view == AView::Scope && matches!(self.m.pane, tables::PANE_SPECTRUM | tables::PANE_RESULTS);
        shown && self.m.spec_span == 0.0 && PANE_HARMS[self.m.spec_harm.min(PANE_HARMS.len() - 1)] > 0
    }

    /// Puts the lines into the order of the table: by frequency or by level.
    pub(in crate::ui) fn peaks_sort(&mut self) {
        if self.m.spec_sort == 0 {
            self.azr.peaks.sort_by(|a, b| a.f.total_cmp(&b.f));
        } else {
            self.azr.peaks.sort_by(|a, b| b.dbv.total_cmp(&a.dbv));
        }
    }

    /// The source exists: a channel that is on, or a ref that holds samples.
    pub(in crate::ui) fn wave_ready(&self, id: WaveId) -> bool {
        match id {
            WaveId::Ch(c) => c < NCH && self.m.ch[c].on,
            WaveId::Ref(i) => self.refs.get(i).and_then(|r| r.as_ref()).is_some_and(|r| r.v.len() >= 2 && r.dt > 0.0),
        }
    }

    fn wave_vpd(&self, id: WaveId) -> f64 {
        match id {
            WaveId::Ch(c) => self.m.ch[c.min(NCH - 1)].volts_per_div() as f64,
            WaveId::Ref(i) => self.refs.get(i).and_then(|r| r.as_ref()).map(|r| r.vpd as f64).unwrap_or(1.0),
        }
    }

    /// Frequency of the wave, for the spectrum's auto span. A ref is measured here, not from `ana`.
    fn wave_freq(&self, rec: &Record, id: WaveId) -> f64 {
        if let WaveId::Ch(c) = id {
            if c < NCH && self.m.ch[c].on {
                return self.meas_at(WaveId::Ch(c), Meas::Freq);
            }
        }
        let Some(s) = self.wave_samples(rec, id) else { return f64::NAN };
        oos_dsp::measure::analyze_min(&s.v, s.dt, 0.4 * self.wave_vpd(id)).get(Meas::Freq)
    }

    /// The spectrum of the source from the record: the view, or `pane` beside the waveform.
    pub(super) fn spectrum_update(&mut self, rec: &Record, pane: bool) {
        let id = self.m.spec_src;
        let Some(s) = self.wave_samples(rec, id) else {
            self.azr.spec = None;
            self.azr.peaks.clear();
            return;
        };
        let (v, dt) = (s.v, s.dt);
        let f1 = self.wave_freq(rec, id);
        let (mut center, mut span) = (self.m.spec_center, self.m.spec_span);
        if self.spec_follows() {
            // no frequency this record: the span stays
            self.azr.spec_stop =
                spectrum::auto_stop(f1, PANE_HARMS[self.m.spec_harm.min(PANE_HARMS.len() - 1)], self.azr.spec_stop);
            (center, span) = (self.azr.spec_stop / 2.0, self.azr.spec_stop);
        }
        let area = if pane { self.pane_rect() } else { self.area };
        // Auto RBW: the finest the record allows (window ENBW / record time)
        let finest = spectrum::enbw(self.m.spec_win) / (v.len().max(1) as f64 * dt);
        let s = spectrum::Settings {
            center,
            span,
            rbw: if self.m.spec_rbw > 0.0 { self.m.spec_rbw } else { finest },
            window: self.m.spec_win,
            averaging: 16,
            trace: [
                spectrum::Trace::Normal,
                spectrum::Trace::Average,
                spectrum::Trace::MaxHold,
                spectrum::Trace::MinHold,
            ][self.m.spec_trace.min(3)],
            detector: spectrum::Detector::Peak,
            points: self.spec_box(area, pane).plot.w.max(100) as usize,
            ref_level_dbv: self.m.spec_ref as f64,
        };
        let sp = spectrum::analyze(&v, dt, &s, &mut self.azr.spec_avg);
        self.azr.peaks = spectrum::rank_lines(&sp, Some(f1), panes::PEAKS_MAX);
        self.peaks_sort();
        if self.m.spec_wf && !pane {
            self.azr.wf.get_or_insert_with(|| spectrum::Waterfall::new(120)).push(&sp);
        }
        self.azr.dist = if self.m.spec_dist && !pane { spectrum::distortion(&v, dt, None, 10) } else { None };
        if let Some(l) = &self.azr.limit {
            self.azr.limit_n += 1;
            if let Some(w) = spectrum::limit_check(&sp, l) {
                self.azr.limit_over += 1;
                if self.azr.limit_worst.is_none_or(|x| w.1 > x.1) {
                    self.azr.limit_worst = Some(w);
                }
            }
        }
        self.azr.spec = Some(sp);
    }

    pub(in crate::ui) fn identify_items(&self) -> Vec<Item> {
        let mut v = vec![Item::ProtoWatch];
        v.extend((0..self.azr.findings.len().min(8)).map(Item::IdFind));
        v
    }

    /// Protocol watch: every 2 s Identify on the screen record of the channels that are on; a bus that was
    /// not there before is announced, the findings are ready in the Identify menu.
    pub(in crate::ui) fn protocol_watch(&mut self) {
        if !self.azr.watch || self.now - self.azr.watch_t < 2.0 {
            return;
        }
        self.azr.watch_t = self.now;
        let Some(rec) = self.last[0].clone() else { return };
        // every captured sample of the window (the screen record undersamples fast buses), as the measurements do
        let mut dt = rec.dt;
        let owned: Vec<Vec<f32>> = (0..NCH)
            .map(|c| {
                if !self.m.ch[c].on || rec.ch[c].is_empty() {
                    return Vec::new();
                }
                let (v, d) = self.meas_wave(&rec, c);
                dt = d;
                v
            })
            .collect();
        let ws: Vec<&[f32]> = owned.iter().map(|v| v.as_slice()).collect();
        let found = identify::identify(&ws, dt);
        let buses: Vec<String> = found.iter().filter(|f| f.confidence >= 0.6).filter_map(bus_name).collect();
        for b in &buses {
            if !self.azr.watch_seen.contains(b) {
                self.toast(&format!("Protocol watch: {} found (Identify to apply)", b));
            }
        }
        self.azr.watch_seen = buses;
        self.azr.findings = found;
    }

    /// SCPI / MCP: Identify now, one finding per line ("CH1 Clock 1.000 kHz ... (90 %)").
    pub fn identify_text(&mut self) -> String {
        self.identify_now();
        self.azr
            .findings
            .iter()
            .map(|f| {
                format!(
                    "{} {} ({:.0} %)",
                    f.channels.iter().map(|c| format!("CH{}", c + 1)).collect::<Vec<_>>().join("+"),
                    f.summary,
                    f.confidence * 100.0
                )
            })
            .collect::<Vec<_>>()
            .join(";")
    }

    /// Identify on every captured sample of the channels that are on.
    pub(super) fn identify_now(&mut self) {
        let Some(rec) = self.last[0].clone() else {
            self.azr.findings.clear();
            return;
        };
        let mut dt = rec.dt;
        let ws: Vec<Vec<f32>> = (0..NCH)
            .map(|c| {
                if self.m.ch[c].on && !rec.ch[c].is_empty() {
                    let (v, d) = self.meas_wave(&rec, c);
                    dt = d;
                    v
                } else {
                    Vec::new()
                }
            })
            .collect();
        let refs: Vec<&[f32]> = ws.iter().map(|v| v.as_slice()).collect();
        self.azr.findings = identify::identify(&refs, dt);
        if self.azr.findings.is_empty() {
            self.toast("Identify: nothing found");
        }
    }

    /// Trigger and decoder of an Identify finding.
    pub(super) fn apply_finding(&mut self, f: &identify::Finding) {
        use identify::{DecoderHint as D, TriggerHint as T};
        let m = &mut self.m;
        match f.trigger {
            T::Edge { source, level, rising, .. } => {
                (m.trig_type, m.trig_src, m.trig_level, m.slope) =
                    (TrigType::Edge, source, level, if rising { Slope::Rise } else { Slope::Fall });
            }
            T::PulseWidth { source, level, positive, less_than, greater_than } => {
                (m.trig_type, m.trig_src, m.trig_level) = (TrigType::Pulse, source, level);
                m.trig_pol = if positive { Pol::Pos } else { Pol::Neg };
                match (less_than, greater_than) {
                    (Some(t), _) => (m.trig_when, m.trig_t1) = (WhenKind::Lt, t as f32),
                    (None, Some(t)) => (m.trig_when, m.trig_t1) = (WhenKind::Gt, t as f32),
                    _ => {}
                }
            }
            T::Video { source, ntsc } => {
                (m.trig_type, m.trig_src) = (TrigType::Video, source);
                m.video_std = if ntsc { VideoStd::Ntsc } else { VideoStd::Pal };
            }
            T::None => {}
        }
        let nearest = |l: &[f64], x: f64| {
            l.iter().enumerate().min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).map_or(0, |e| e.0)
        };
        let Some(d) = &f.decoder else { return };
        m.dec_on = true;
        match d {
            D::Uart { ch, baud, bits, parity, thr, .. } => {
                (m.dec_proto, m.dec_src, m.dec_bits, m.dec_parity, m.dec_thr) =
                    (Proto::UART, *ch, *bits, *parity, *thr);
                m.dec_baud = nearest(&crate::decode::BAUDS, *baud);
            }
            D::I2c { sda, scl, thr } => {
                (m.dec_proto, m.dec_src, m.dec_src2, m.dec_thr) = (Proto::I2C, *sda, *scl, *thr)
            }
            D::Spi { clk, data, cs, mode, bits, thr } => {
                (m.dec_proto, m.dec_src2, m.dec_cs, m.dec_mode, m.dec_bits, m.dec_thr) =
                    (Proto::SPI, *clk, *cs, *mode, *bits, *thr);
                m.dec_src = data.first().copied().unwrap_or(0);
            }
            D::Can { ch, bitrate, thr, invert } => {
                (m.dec_proto, m.dec_src, m.dec_thr, m.dec_canh) = (Proto::CAN, *ch, *thr, *invert);
                m.dec_canrate = nearest(&crate::decode::CAN_RATES, *bitrate);
            }
            D::Lin { ch, baud, thr } => {
                (m.dec_proto, m.dec_src, m.dec_thr) = (Proto::LIN, *ch, *thr);
                m.dec_baud = nearest(&crate::decode::BAUDS, *baud);
            }
            // modules since 0.1.13; Manchester's baud row is its option 0
            D::OneWire { ch, thr } => {
                if let Some(p) = Proto::from_key("OneWire") {
                    (m.dec_proto, m.dec_src, m.dec_thr) = (p, *ch, *thr);
                }
            }
            D::Manchester { ch, bitrate, thr } => {
                if let Some(p) = Proto::from_key("Manchester") {
                    (m.dec_proto, m.dec_src, m.dec_thr) = (p, *ch, *thr);
                    m.dec_opt[0] = nearest(&crate::decode::BAUDS, *bitrate) as u8;
                }
            }
        }
    }

    /// Recomputes the active analyser from the last record (every captured sample of the window).
    pub(in crate::ui) fn analyzer_update(&mut self) {
        // a source that is off shows nothing: the first channel that is on takes over
        if let Some(on) = (0..NCH).find(|&c| self.m.ch[c].on) {
            for src in [&mut self.m.spec_src, &mut self.m.eye_src] {
                if let WaveId::Ch(c) = *src {
                    if !self.m.ch[c].on {
                        *src = WaveId::Ch(on);
                    }
                }
            }
        }
        // the second pane runs the spectrum beside the scope; the peak list of the Results pane needs it less often
        let beside = self.m.view == AView::Scope
            && self.pane_h() > 0
            && matches!(self.m.pane, tables::PANE_SPECTRUM | tables::PANE_RESULTS);
        if beside && self.m.pane == tables::PANE_RESULTS {
            if self.azr.spec.is_some() && (self.now - self.azr.spec_t).abs() < 0.5 {
                return;
            }
            self.azr.spec_t = self.now;
        }
        let view = if beside { AView::Spectrum } else { self.m.view };
        if view == AView::Scope {
            return;
        }
        // A loaded ref is measured with the scope stopped or still waiting. Live channels still need a record.
        let rec = match self.last[0].clone() {
            Some(r) => r,
            None if super::analysers::of(view).is_some_and(|a| a.without_record(self)) => Self::blank_record(),
            None => return,
        };
        if let Some(a) = super::analysers::of(view) {
            a.update(self, &rec, beside);
        }
    }

    /// Why a power-quality page would show only zeros: the lines that say so (no signal, no current probe, no whole
    /// cycle), or `None` when the numbers mean something.
    fn power_quality_reason(&self, v: &[f32], i: &[f32], q: &power::Quality) -> Option<Vec<String>> {
        let flat = |x: &[f32]| {
            let (lo, hi) = x
                .iter()
                .filter(|a| a.is_finite())
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), &a| (l.min(a), h.max(a)));
            !(hi - lo > 1e-9)
        };
        let (v_name, i_name) = (self.wave_name(self.m.pw_v), self.wave_name(self.m.pw_i));
        let mut why = Vec::new();
        if flat(v) {
            why.push(format!("No voltage: {v_name} shows no signal"));
        }
        if flat(i) {
            let probe = matches!(self.m.pw_i, WaveId::Ch(c) if self.m.ch[c].unit == AmpUnit::A);
            if probe {
                why.push(format!("No current: {i_name} reads 0 A, check the current probe"));
            } else {
                why.push(format!("No current probe set: {i_name} is not in amps"));
            }
        }
        if why.is_empty() && q.cycles == 0 {
            why.push("No whole cycle of the voltage in the record".into());
            why.push("Set the line frequency or a slower time base".into());
        }
        (!why.is_empty()).then_some(why)
    }

    /// Power analysis of the record: the rows of the page for its mode.
    pub(super) fn power_update(&mut self, rec: &Record) {
        let mode = self.m.pw_mode;
        let (v_id, i_id) = (self.m.pw_v, self.m.pw_i);
        let (fline, fsw, reference) = (self.m.pw_fline, self.m.pw_fsw, self.m.pw_ref);
        let taken = match mode {
            PowerMode::Ripple => self.take_waves(rec, &[v_id], &[Some(i_id), self.m.pw_vin]),
            PowerMode::Rail => self.take_waves(rec, &[v_id], &[self.m.pw_vin, self.m.pw_clk]),
            _ => self.take_waves(rec, &[v_id, i_id], &[]),
        };
        let taken = match taken {
            Ok(t) => t,
            Err(id) => {
                self.azr.power = vec![format!("{}: Switch on source in the menu", missing_wave(id))];
                return;
            }
        };
        let v = taken.v[0].clone();
        let dt = taken.dt;
        let opt = |k: usize| taken.opt.get(k).and_then(|s| s.clone()).unwrap_or_default();
        let i = if matches!(mode, PowerMode::Ripple) {
            opt(0)
        } else if matches!(mode, PowerMode::Rail) {
            Vec::new()
        } else {
            taken.v[1].clone()
        };
        self.azr.power = match mode {
            PowerMode::Quality => {
                let q = power::quality(&v, &i, dt, if fline > 0.0 { Some(fline) } else { None });
                match self.power_quality_reason(&v, &i, &q) {
                    Some(why) => why,
                    None => vec![
                        format!("Vrms {}   Irms {}", si(q.vrms, "V"), si(q.irms, "A")),
                        format!(
                            "P {}   S {}   Q {}",
                            si(q.p_real, "W"),
                            si(q.s_apparent, "VA"),
                            si(q.q_reactive, "var")
                        ),
                        format!("PF {:.3}   phase {:.1}\u{b0}   f {}", q.pf, q.phase_deg, si(q.freq, "Hz")),
                        format!("crest V {:.2}   crest I {:.2}   {} cycles", q.crest_v, q.crest_i, q.cycles),
                    ],
                }
            }
            PowerMode::Harmonics => {
                let h = power::harmonics(&i, dt, None, 15);
                let mut out = vec!["Current harmonics vs. IEC 61000-3-2 class A".to_string()];
                out.extend(h.iter().map(|(n, a, lim, pass)| {
                    format!(
                        "H{:<2} {:>10}  limit {:>10}  {}",
                        n,
                        si(*a, "A"),
                        if lim.is_finite() { si(*lim, "A") } else { "-".into() },
                        if *pass { "pass" } else { "FAIL" }
                    )
                }));
                out
            }
            PowerMode::Switching => {
                let s = power::switching(&v, &i, dt, 10.0);
                vec![
                    format!("E on {}   E off {}", si(s.e_on, "J"), si(s.e_off, "J")),
                    format!("E cond {}   P avg {}", si(s.e_cond, "J"), si(s.p_avg, "W")),
                    format!("dv/dt {} / {}", si(s.dvdt_rise, "V/s"), si(s.dvdt_fall, "V/s")),
                    format!("di/dt {} / {}", si(s.didt_rise, "A/s"), si(s.didt_fall, "A/s")),
                    format!("RDS(on) {}", si(s.rds_on, "\u{3a9}")),
                    format!("{} transitions", s.transitions.len()),
                ]
            }
            PowerMode::Ripple => {
                let (pp, ac) = power::ripple(&v);
                let (pk, tpk, i2t) = if i.is_empty() { (f64::NAN, f64::NAN, f64::NAN) } else { power::inrush(&i, dt) };
                let fl = if fline > 0.0 { fline } else { power::line_hz(&v, dt) };
                let fs = if fsw > 0.0 { fsw } else { power::switch_hz(&v, dt) };
                let line = power::tone_ripple(&v, dt, fl);
                let sw = power::switching_ripple(&v, dt, fs);
                let mut rows = vec![
                    format!("Ripple {} pp   {} rms AC", si(pp, "V"), si(ac, "V")),
                    format!("Line {}  {} pp  {} rms", si(fl, "Hz"), si(line.pp, "V"), si(line.rms, "V")),
                    format!("Switch {}  {} pp  {} rms", si(fs, "Hz"), si(sw.pp, "V"), si(sw.rms, "V")),
                    format!("Inrush peak {} at {}   I\u{b2}t {}", si(pk, "A"), si(tpk, "s"), si(i2t, "A\u{b2}s")),
                ];
                let vin = opt(1);
                if !vin.is_empty() {
                    rows.push(format!(
                        "Turn on {}   turn off {}",
                        si(power::turn_on_time(&vin, &v, dt), "s"),
                        si(power::turn_off_time(&vin, &v, dt), "s")
                    ));
                }
                rows
            }
            PowerMode::Capacitance => {
                vec![
                    format!("C {}", si(power::input_capacitance(&v, &i, dt), "F")),
                    "The record is the turn-on event".into(),
                ]
            }
            PowerMode::Rail => {
                let vin = opt(0);
                let clk = opt(1);
                let r = power::rail(
                    &v,
                    dt,
                    reference,
                    (!vin.is_empty()).then_some(vin.as_slice()),
                    (!clk.is_empty()).then_some(clk.as_slice()),
                );
                let refn = reference.map(|v| si(v, "V")).unwrap_or_else(|| "top".into());
                vec![
                    format!("Overshoot {}   undershoot {}", si(r.overshoot, "V"), si(r.undershoot, "V")),
                    format!("DC {}   turn-on max {}", si(r.dc, "V"), si(r.turn_on, "V")),
                    format!("Reference {}", refn),
                ]
            }
        };
    }
}
