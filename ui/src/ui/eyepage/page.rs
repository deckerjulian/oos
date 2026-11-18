// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The sums and the text of the eye page: `Page` takes each record's TIE and eye, keeps the running figures, fills
//! the four charts and writes every figure with its reason; the facts of the page (`populate_eye_facts`) come from
//! the same numbers. A new figure or chart of the page goes here.

use super::*;

impl Page {
    fn put_fig(&mut self, name: &'static str, v: f64) {
        debug_assert!(EYE_ITEMS.contains(&name), "{name}");
        self.readings.push((name, v));
    }

    /// Takes the TIE of one record into the sums.
    pub(super) fn add(&mut self, tie: &[f64], clk: &eye::Clock) {
        self.records += 1;
        self.ui.add(clk.period);
        if tie.len() < eye::MIN_EDGES {
            self.short = tie.len();
            return;
        }
        self.short = 0;
        self.edges += tie.len() as u64;
        self.means.add(tie, clk);
        let mean = tie.iter().sum::<f64>() / tie.len() as f64;
        self.rms.add((tie.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / tie.len() as f64).sqrt());
        let mut prev = None;
        for w in tie.windows(2) {
            let d = w[1] - w[0];
            self.step.0 += d * d;
            self.step.1 += 1;
            if let Some(p) = prev {
                let e: f64 = d - p;
                self.step2.0 += e * e;
                self.step2.1 += 1;
            }
            prev = Some(d);
        }
        for &x in tie {
            if self.pool.len() < POOL {
                self.pool.push(x);
            } else {
                self.pool[self.head] = x;
                self.head = (self.head + 1) % POOL;
            }
        }
    }

    /// Colours the eye: log of the hits on the heat scale, so rare paths stay visible beside the dense levels.
    pub(super) fn paint(&mut self, e: &eye::Eye) {
        let pal = heat_palette();
        let peak = e.hits.iter().copied().max().unwrap_or(0);
        let k = 255.0 / (1.0 + peak as f32).ln().max(1e-6);
        self.img.clear();
        self.img.extend(e.hits.iter().map(|&h| {
            if h == 0 {
                0
            } else {
                pal[(((1.0 + h as f32).ln() * k) as usize).min(255)]
            }
        }));
        self.img_w = e.w;
    }

    pub(super) fn chart_trend(&mut self, tr: &[f32], clk: &eye::Clock, cols: usize) {
        self.trend.clear();
        let c = &mut self.charts[0];
        *c = Chart { title: "TIE trend".into(), ..Chart::default() };
        if tr.len() < 2 {
            c.empty = "Needs 2 edges in a record".into();
            return;
        }
        let pk = tr.iter().fold(0.0f32, |a, x| a.max(x.abs())).max(1e-18);
        c.title = format!("TIE trend  peak {}", si(pk as f64, "s"));
        c.x = ("0".into(), si_short(tr.len() as f64 * clk.period, "s"));
        c.y = (format!("+{}", si_short(pk as f64, "s")), format!("-{}", si_short(pk as f64, "s")));
        for k in 0..cols.min(tr.len()) {
            let n = cols.min(tr.len());
            let part = &tr[k * tr.len() / n..((k + 1) * tr.len() / n).max(k * tr.len() / n + 1)];
            self.trend.push(part.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x / pk), b.max(x / pk))));
        }
    }

    pub(super) fn chart_hist(&mut self, j: &Jitter) {
        self.hist.clear();
        let c = &mut self.charts[1];
        *c = Chart { title: "TIE histogram".into(), ..Chart::default() };
        if j.fit == JitterFit::TooFew {
            c.empty = format!("Needs {} edges", eye::MIN_EDGES);
            return;
        }
        c.title = format!("TIE histogram  \u{3c3} {}", si(j.rms, "s"));
        let top = j.histogram.iter().map(|h| h.1).max().unwrap_or(1).max(1) as f32;
        self.hist.extend(j.histogram.iter().map(|h| h.1 as f32 / top));
        let (lo, hi) = (j.histogram.first().map_or(0.0, |h| h.0), j.histogram.last().map_or(0.0, |h| h.0));
        c.x = (si_short(lo, "s"), si_short(hi, "s"));
        c.y = (format!("{}", top as u32), "0".into());
    }

    /// The spectrum chart; returns PJ (RMS of the lines) or NaN and why not.
    pub(super) fn chart_spec(&mut self, tr: &[f32], clk: &eye::Clock, cols: usize) -> (f64, String) {
        self.spec.clear();
        let c = &mut self.charts[2];
        *c = Chart { title: "TIE spectrum".into(), ..Chart::default() };
        if tr.len() < SPECTRUM_MIN {
            let why = format!("Needs {} UI in a record", SPECTRUM_MIN);
            c.empty = why.clone();
            return (f64::NAN, why);
        }
        let (df, s) = eye::tie_spectrum(&tr[..tr.len().min(SPECTRUM_UI)], clk.period);
        let Some((k, &pk)) = s.iter().enumerate().skip(2).max_by(|a, b| a.1.total_cmp(b.1)).filter(|p| *p.1 > 0.0)
        else {
            c.empty = "No jitter in the record".into();
            return (0.0, String::new());
        };
        c.title = format!("TIE spectrum  {} at {}", si(pk, "s"), si(k as f64 * df, "Hz"));
        c.x = ("0".into(), si_short(0.5 / clk.period, "Hz"));
        c.y = (si_short(pk, "s"), format!("-{:.0} dB", SPECTRUM_DB));
        let n = cols.min(s.len() - 2).max(1);
        for col in 0..n {
            let (a, b) = (2 + col * (s.len() - 2) / n, 2 + (col + 1) * (s.len() - 2) / n);
            let v = s[a..b.max(a + 1)].iter().fold(0.0f64, |m, &x| m.max(x));
            self.spec.push(((20.0 * (v.max(1e-30) / pk).log10()) as f32).max(-SPECTRUM_DB));
        }
        let lines = eye::pj_lines(df, &s, PJ_LINES);
        (lines.iter().map(|l| l.1 * l.1).sum::<f64>().sqrt(), String::new())
    }

    pub(super) fn chart_tub(&mut self, sorted: &[f64], j: &Jitter, period: f64, cols: usize) {
        let c = &mut self.charts[3];
        *c = Chart { title: "Bathtub (model)".into(), ..Chart::default() };
        self.tub = eye::bathtub(sorted, j, period, cols.max(2));
        if self.tub.model.is_empty() {
            c.empty = format!("Needs {} edges", eye::MIN_EDGES);
            return;
        }
        c.title = "Bathtub (model)".into();
        c.x = ("0".into(), "1 UI".into());
        c.y = ("1".into(), format!("1e{:.0}", TUB_FOOT));
    }

    pub(super) fn text_jitter(&mut self, j: &Jitter, pj: f64, pj_why: &str) {
        let few = format!("needs {} edges", eye::MIN_EDGES);
        let means = format!("needs {} per edge", eye::RUN_MIN);
        let ddj = self.means.ddj();
        let dcd = self.means.dcd();
        self.readings.clear();
        for (name, v) in [
            ("TPP", j.pp),
            ("TRMS", j.rms),
            ("DCD", dcd),
            ("DDJ", ddj),
            ("PJRms", pj),
            ("PRMS", j.period_jitter_rms),
            ("C2C", j.c2c_rms),
            ("TJ", j.tj_pp),
            ("RJ", j.rj_rms),
            ("DJ", j.dj_pp),
            ("OPENing", self.tub.opening),
            ("SEEN", self.tub.seen),
        ] {
            self.put_fig(name, v);
        }
        self.measured = vec![
            figure("TIE pp", j.pp, "s", Kind::Measured, &few),
            figure("TIE rms", j.rms, "s", Kind::Measured, &few),
            figure("DCD", dcd, "s", Kind::Measured, &means),
            figure("DDJ", ddj, "s", Kind::Measured, if dcd.is_finite() { "one run length" } else { &means }),
            // no line over the floor is a finding, not a jitter of 0 s
            if pj == 0.0 {
                cell("PJ rms", "no lines".into(), Kind::Missing)
            } else {
                figure("PJ rms", pj, "s", Kind::Measured, if pj_why.is_empty() { &few } else { "needs 64 UI" })
            },
            figure("Period rms", j.period_jitter_rms, "s", Kind::Measured, &few),
            figure("Cycle-cycle", j.c2c_rms, "s", Kind::Measured, &few),
        ];
        let at_most = |v: f64| format!("under {}", si(v, "s"));
        self.model = match j.fit {
            JitterFit::TooFew => {
                ["TJ 1e-12", "RJ rms", "DJ", "Opening"].iter().map(|l| cell(l, few.clone(), Kind::Missing)).collect()
            }
            JitterFit::Limit => vec![
                cell("TJ 1e-12", at_most(j.tj_pp), Kind::Model),
                cell("RJ rms", at_most(j.rj_rms), Kind::Model),
                cell("DJ", format!("needs {} edges", eye::TAIL_EDGES), Kind::Missing),
                cell("Opening", format!("over {:.3} UI", self.tub.opening), Kind::Model),
            ],
            _ => vec![
                figure("TJ 1e-12", j.tj_pp, "s", Kind::Model, &few),
                figure("RJ rms", j.rj_rms, "s", Kind::Model, &few),
                figure("DJ", j.dj_pp, "s", Kind::Model, &few),
                cell("Opening", format!("{:.3} UI", self.tub.opening), Kind::Model),
            ],
        };
        if self.tub.seen.is_finite() {
            self.model.push(cell("Seen open", format!("{:.3} UI", self.tub.seen), Kind::Measured));
        }
    }

    pub(super) fn text_eye(&mut self, e: &eye::Eye, m: &eye::EyeMeas, period: f64, dt: f64) {
        let empty = "no hits yet";
        // a level noise under one eye row cannot be told from none
        let row = (e.v_max - e.v_min) as f64 / e.h as f64;
        let level = |v: f64, s: f64| match (v.is_finite(), s < row) {
            (false, _) => empty.to_string(),
            (_, true) => format!("{}  \u{3c3} < {}", si(v, "V"), si(row, "V")),
            _ => format!("{}  \u{3c3} {}", si(v, "V"), si(s, "V")),
        };
        let tr = if m.rise.is_finite() && m.fall.is_finite() {
            format!("{} / {}", si(m.rise, "s"), si(m.fall, "s"))
        } else {
            "not resolved".to_string()
        };
        for (name, v) in [
            ("ONE", m.one),
            ("ZERO", m.zero),
            ("AMPLitude", m.amplitude),
            ("CROSsing", m.crossing_pct),
            ("RISe", m.rise),
            ("FALL", m.fall),
            ("HEIGht", m.height),
            ("WIDTh", m.width),
            ("Q", m.q_factor),
            ("RECords", self.records as f64),
            ("EDGes", self.pool.len() as f64),
            ("UI", self.ui.mean()),
            ("SAMPling", dt),
        ] {
            self.put_fig(name, v);
        }
        self.eye_measured = vec![
            cell("One", level(m.one, m.sigma_one), if m.one.is_finite() { Kind::Measured } else { Kind::Missing }),
            cell("Zero", level(m.zero, m.sigma_zero), if m.zero.is_finite() { Kind::Measured } else { Kind::Missing }),
            figure("Amplitude", m.amplitude, "V", Kind::Measured, empty),
            if m.crossing_pct.is_finite() {
                cell("Crossing", format!("{:.1} %", m.crossing_pct), Kind::Measured)
            } else {
                cell("Crossing", empty.into(), Kind::Missing)
            },
            cell(
                "Rise / fall",
                tr,
                if m.rise.is_finite() && m.fall.is_finite() { Kind::Measured } else { Kind::Missing },
            ),
        ];
        let q = if m.sigma_one + m.sigma_zero < row {
            cell("Q", format!("over {:.0}", m.amplitude / row), Kind::Model)
        } else {
            cell("Q", format!("{:.1}", m.q_factor), Kind::Model)
        };
        self.eye_model = vec![
            figure("Height", m.height, "V", Kind::Model, empty),
            figure("Width", m.width, "s", Kind::Model, empty),
            if m.amplitude.is_finite() { q } else { cell("Q", empty.into(), Kind::Missing) },
            cell("Height", "levels less 3\u{3c3}".into(), Kind::Missing),
            cell("Width", "1 UI less 6\u{3c3}".into(), Kind::Missing),
        ];
        self.stats = vec![
            cell("Records", format!("{}", self.records), Kind::Measured),
            cell("Edges", format!("{} of {}", self.pool.len(), self.edges), Kind::Measured),
            figure("UI mean", self.ui.mean(), "s", Kind::Measured, empty),
            if self.rms.n > 0 {
                cell("TIE rms", format!("{} .. {}", si(self.rms.min, "s"), si(self.rms.max, "s")), Kind::Measured)
            } else {
                cell("TIE rms", format!("needs {} edges", eye::MIN_EDGES), Kind::Missing)
            },
            cell("Sampling", format!("{} ({:.0}/UI)", si(dt, "s"), period / dt), Kind::Measured),
        ];
    }

    pub(super) fn text_status(&mut self, j: &Jitter) {
        let n = self.pool.len();
        self.status[0] = format!("Jitter of {} edges, {} records", n, self.records);
        self.status[1] = if self.short > 0 {
            format!("{} edges in a record: needs {} for jitter", self.short, eye::MIN_EDGES)
        } else {
            match j.fit {
                JitterFit::TooFew => format!("Needs {} edges for a model", eye::MIN_EDGES),
                JitterFit::Quiet => "No jitter above the time resolution".into(),
                JitterFit::Limit if n < eye::TAIL_EDGES => {
                    format!("Upper limits: tail fit from {} edges", eye::TAIL_EDGES)
                }
                JitterFit::Limit => "Upper limits: the TIE has no Gaussian tails".into(),
                JitterFit::Tails => "Model: dual-Dirac fit of the TIE tails".into(),
            }
        };
    }
}

#[derive(Clone, Copy)]
pub(super) struct FactSettings {
    ber: f64,
    npj: bool,
    pn_lo: f64,
    pn_hi: f64,
    notch: f64,
    hp: bool,
}

impl From<&Model> for FactSettings {
    fn from(m: &Model) -> Self {
        Self {
            ber: m.eye_ber,
            npj: m.eye_npj,
            pn_lo: m.eye_pn_lo,
            pn_hi: m.eye_pn_hi,
            notch: m.eye_notch,
            hp: m.eye_hp,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn populate_eye_facts(
    page: &mut Page,
    settings: FactSettings,
    clk: &eye::Clock,
    tie: &[f64],
    trend: &[f32],
    j: &Jitter,
    meas: &eye::EyeMeas,
    wave: &[f32],
    dt: f64,
    means: &EdgeMeans,
    rail: Option<(&[f32], f64)>,
    companion: Option<(&[f32], &[f32], f64)>,
) -> (Vec<Cell>, Vec<Cell>, Vec<Cell>) {
    let few = format!("needs {} edges", eye::MIN_EDGES);
    let miss = |l| cell(l, few.clone(), Kind::Missing);
    if j.fit == JitterFit::TooFew {
        for name in EYE_FACTS {
            page.put_fig(name, f64::NAN);
        }
        let model = ["TJ@BER", "EW@BER", "J2", "J9", "NPJ"].map(miss).to_vec();
        let measured = [
            "F/2", "F/4", "F/8", "SRJ", "Pn rms", "Pn note", "TIE pre", "TIE post", "DC CM", "AC CM", "Cross", "T/nT",
            "SSC ppm", "SSC rate",
        ]
        .map(miss)
        .to_vec();
        let eye_model = vec![cell("Ht@BER", "no eye".into(), Kind::Missing)];
        return (measured, model, eye_model);
    }
    let ber = settings.ber;
    let (tjber, ewber, j2, j9) = (
        eye::tj_at(j, ber),
        eye::eye_width_at(j, clk.period, ber),
        eye::tj_at(j, eye::BER_J2),
        eye::tj_at(j, eye::BER_J9),
    );
    let npj = if settings.npj { eye::npj(tie, clk, trend, means) } else { f64::NAN };
    let (f2, f4, f8) = (
        eye::subrate_pp(trend, clk.period, 2),
        eye::subrate_pp(trend, clk.period, 4),
        eye::subrate_pp(trend, clk.period, 8),
    );
    let srj = eye::srj_pp(trend, clk.period);
    let pn = eye::phase_noise(trend, clk.period, settings.pn_lo, settings.pn_hi);
    let ps = eye::psij(trend, clk, tie, rail, settings.notch, meas.height, meas.width, Some((wave, dt)));
    let (dc, ac, cross) = companion.map_or_else(
        || (f64::NAN, eye::ac_common_pp(wave, None, dt, settings.hp), f64::NAN),
        |(a, b, dt)| (eye::dc_common(a, b), eye::ac_common_pp(a, Some(b), dt, settings.hp), eye::diff_crossover(a, b)),
    );
    let tnt = eye::tnt_ratio(wave, dt, clk);
    let (ppm, _) = eye::ssc_deviation(clk);
    let ssc_hz = eye::ssc_rate(clk);
    let hber = meas.height_at(ber);
    for (name, v) in [
        ("HBER", hber),
        ("F2", f2),
        ("F4", f4),
        ("F8", f8),
        ("SRJ", srj),
        ("PN", pn),
        ("TPRE", ps.before.tie_pp),
        ("TPOST", ps.after.tie_pp),
        ("PPRE", ps.before.pj),
        ("PPOST", ps.after.pj),
        ("DCCM", dc),
        ("ACCM", ac),
        ("XOVer", cross),
        ("TNT", tnt),
        ("SSCPpm", ppm),
        ("SSCRate", ssc_hz),
        ("TJBer", tjber),
        ("EWBer", ewber),
        ("J2", j2),
        ("J9", j9),
        ("NPJ", npj),
    ] {
        page.put_fig(name, v);
    }
    let mut model = vec![
        figure("TJ@BER", tjber, "s", Kind::Model, &few),
        figure("EW@BER", ewber, "UI", Kind::Model, &few),
        figure("J2", j2, "s", Kind::Model, &few),
        figure("J9", j9, "s", Kind::Model, &few),
    ];
    if settings.npj {
        model.push(figure("NPJ", npj, "s", Kind::Model, &few));
    } else {
        model.push(cell("NPJ", "off".into(), Kind::Missing));
    }
    let mut measured = vec![
        figure("F/2", f2, "s", Kind::Measured, &few),
        figure("F/4", f4, "s", Kind::Measured, &few),
        figure("F/8", f8, "s", Kind::Measured, &few),
        figure("SRJ", srj, "s", Kind::Measured, &few),
        figure("Pn rms", pn, "s", Kind::Measured, &few),
        cell(
            "Pn note",
            if eye::is_clock(clk) { "clock" } else { "data" }.into(),
            if eye::is_clock(clk) { Kind::Measured } else { Kind::Missing },
        ),
    ];
    measured.push(figure("TIE pre", ps.before.tie_pp, "s", Kind::Measured, &few));
    measured.push(figure("TIE post", ps.after.tie_pp, "s", Kind::Measured, &few));
    measured.push(figure("PJ pre", ps.before.pj, "s", Kind::Measured, &few));
    measured.push(figure("PJ post", ps.after.pj, "s", Kind::Measured, &few));
    measured.push(figure("DC CM", dc, "V", Kind::Measured, "no B"));
    measured.push(figure("AC CM", ac, "V", Kind::Measured, &few));
    measured.push(figure("Cross", cross, "V", Kind::Measured, "no B"));
    measured.push(figure("T/nT", tnt, "dB", Kind::Measured, &few));
    measured.push(figure("SSC ppm", ppm, "ppm", Kind::Measured, &few));
    measured.push(figure("SSC rate", ssc_hz, "Hz", Kind::Measured, &few));
    let eye_model = vec![figure("Ht@BER", hber, "V", Kind::Model, "no eye")];
    (measured, model, eye_model)
}
