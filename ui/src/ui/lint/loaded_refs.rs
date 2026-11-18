// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Checks of loaded references: files in a ref slot feed the measurements and analysers at their own interval, the
//! eye of DQ/DQS refs, and SCPI queries of a ref. The helpers that build reference waveforms are here too.

use super::*;

#[test]
fn a_spectrum_limit_counts_what_goes_over() {
    let mut u = with_two_channels();
    u.m.view = AView::Spectrum;
    u.activate(Item::SpecLimitSet);
    assert!(u.azr.limit.is_none());
    for _ in 0..24 {
        u.step(0.04);
    }
    u.adjust(Item::SpecMargin, 4);
    assert_eq!(u.value(Item::SpecMargin), "14 dB");
    u.activate(Item::SpecLimitSet);
    assert!(u.azr.limit.is_some());
    for _ in 0..24 {
        u.step(0.04);
    }
    assert!(u.azr.limit_n > 0);
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    // a limit far below the trace: every one goes over
    for p in u.azr.limit.as_mut().unwrap() {
        p.1 -= 200.0;
    }
    (u.azr.limit_n, u.azr.limit_over) = (0, 0);
    for _ in 0..24 {
        u.step(0.04);
    }
    assert!(u.azr.limit_n > 0 && u.azr.limit_over == u.azr.limit_n);
    u.render(&mut cv);
    u.activate(Item::SpecLimitClear);
    assert!(u.azr.limit.is_none());
}

#[test]
fn a_reference_has_its_colour_and_name() {
    let mut u = with_two_channels();
    for _ in 0..24 {
        u.step(0.04);
    }
    u.activate(Item::RefLabel);
    assert!(u.wlan.osk.is_none(), "an empty slot has no name");
    u.activate(Item::RefSave);
    u.adjust(Item::RefColor, 2);
    assert_eq!(u.value(Item::RefColor), "Green");
    u.activate(Item::RefLabel);
    u.wlan.osk.as_mut().unwrap().text = "golden".into();
    u.osk_done();
    assert_eq!(u.refs[0].as_ref().unwrap().label, "golden");
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
}

#[test]
fn a_reference_travels_through_a_file() {
    let dir = crate::ui::testkit::Scratch::new("ref");
    let mut u = with_two_channels();
    u.store_dir = dir.clone();
    u.activate(Item::RefSave);
    let a = u.refs[0].as_ref().unwrap().v.clone();
    u.activate(Item::RefExport);
    u.ref_sel = 4;
    u.activate(Item::RefExport);
    assert!(u.toast.as_ref().is_some_and(|t| t.0.contains("empty")));
    std::fs::rename(dir.join("ref1.csv"), dir.join("ref5.csv")).unwrap();
    u.activate(Item::RefImport);
    let r = u.refs[4].as_ref().unwrap();
    assert_eq!(r.v.len(), a.len());
    assert!((r.v[3] - a[3]).abs() < 1e-5);
    assert!(r.dt > 0.0);
    // scale and offset of the slot
    u.adjust(Item::RefVpd, 1);
    u.adjust(Item::RefOffset, 3);
    assert_ne!(u.value(Item::RefOffset), "-");
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    // a file that is not one
    std::fs::write(dir.join("ref5.csv"), "nothing here\n").unwrap();
    u.activate(Item::RefImport);
    assert!(u.toast.as_ref().is_some_and(|t| t.0.contains("no waveform")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bin_or_wfm_in_storage_opens_on_the_slot() {
    let dir = crate::ui::testkit::Scratch::new("refbin");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut u = with_two_channels();
    u.store_dir = dir.clone();
    let rec = u.last[0].as_ref().unwrap().clone();
    let want = rec.ch[0].clone();
    let dt = rec.dt;
    std::fs::write(dir.join("wave-001.bin"), crate::store::wave_bin(&rec, &u.m)).unwrap();
    u.activate(Item::RefImport);
    let r = u.refs[0].as_ref().expect("R1");
    assert_eq!(r.v.as_ref(), &want);
    assert!((r.dt - dt).abs() < 1e-18);
    std::fs::remove_file(dir.join("wave-001.bin")).unwrap();
    // A real f32 WFM fixture is independent of the live channel-enable bits: importing the BIN
    // entered reference mode and deliberately switched those channels off.
    for magic in [b":WFM#002", b":WFM#003"] {
        const CURVE: usize = 838;
        let mut wfm = vec![0u8; CURVE + want.len() * 4 + 8];
        wfm[..2].copy_from_slice(&0x0F0Fu16.to_le_bytes());
        wfm[2..10].copy_from_slice(magic);
        wfm[15] = 4;
        wfm[16..20].copy_from_slice(&(CURVE as i32).to_le_bytes());
        wfm[40..48].copy_from_slice(b"captured");
        wfm[168..176].copy_from_slice(&1f64.to_le_bytes());
        wfm[240..244].copy_from_slice(&4u32.to_le_bytes());
        wfm[488..496].copy_from_slice(&dt.to_le_bytes());
        wfm[496..504].copy_from_slice(&rec.t0.to_le_bytes());
        wfm[826..830].copy_from_slice(&((want.len() * 4) as u32).to_le_bytes());
        for (index, value) in want.iter().enumerate() {
            wfm[CURVE + index * 4..CURVE + index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        u.refs[0] = None;
        std::fs::write(dir.join("cap.wfm"), wfm).unwrap();
        u.activate(Item::RefImport);
        let loaded = u.refs[0].as_ref().expect("WFM loaded into R1");
        assert_eq!(loaded.v.as_ref(), &want);
        assert_eq!(loaded.label, "captured");
        assert!((loaded.dt - dt).abs() < 1e-18);
        assert!((loaded.t0 - rec.t0).abs() < 1e-18);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A CSV in a ref slot is measured at the file's sample interval. The 20 MHz front-end limit
/// does not touch it. CH1 still reads the hardware frame, and the live source is left alone.
#[test]
fn a_loaded_ref_feeds_the_analysers_at_the_files_interval() {
    let dir = crate::ui::testkit::Scratch::new("refmeas");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut u = crate::ui::testkit::demo_ui();
    u.store_dir = dir.clone();
    assert!(matches!(u.src, Source::Demo { .. }));

    let dt = 5e-9;
    let n = 400;
    let sine: Vec<f32> = (0..n).map(|k| (2.0 * std::f64::consts::PI * 40e6 * k as f64 * dt).sin() as f32).collect();
    write_wave(&dir, 1, 0.0, dt, &sine);
    u.ref_sel = 0;
    u.activate(Item::RefImport);
    let loaded = u.refs[0].as_ref().unwrap();
    assert!((loaded.dt - dt).abs() < 1e-18, "{}", loaded.dt);
    assert_eq!(loaded.v.as_ref(), &sine);

    let mut screen: [Vec<f32>; NCH] = Default::default();
    screen[0] = vec![0.25, 0.5, 0.75, 1.0];
    let mut frame = crate::board::Frame { dt: 2e-9, ..Default::default() };
    frame.ch[0] = vec![0.0; 200];
    frame.ch[0][10] = 9.0;
    u.last[0] = Some(Record {
        t0: 0.0,
        dt: 1e-6,
        ch: screen.clone(),
        triggered: true,
        partial: false,
        raw: Some(crate::wave::Raw {
            frame: std::sync::Arc::new(frame),
            i0: 0,
            i1: 200,
            t0: -2e-7,
            conditioned: false,
        }),
        la: None,
        pairs: false,
    });
    let rec = u.last[0].as_ref().unwrap();
    let samp = u.wave_samples(rec, WaveId::Ref(0)).unwrap();
    assert_eq!(samp.v, sine);
    assert!((samp.dt - dt).abs() < 1e-18);
    let pp = |w: &[f32], step: f64| oos_dsp::measure::analyze_min(w, step, 0.1).get(oos_dsp::measure::Meas::Vpp);
    assert!((pp(&samp.v, samp.dt) - pp(&sine, dt)).abs() < 1e-5);

    u.m.ch[0].bw = BW_20M;
    u.m.ch[0].coupling = Coupling::Ac;
    let again = u.wave_samples(u.last[0].as_ref().unwrap(), WaveId::Ref(0)).unwrap();
    assert_eq!(again.v, sine);
    assert!((again.dt - dt).abs() < 1e-18);
    let mut filtered = sine.clone();
    crate::wave::condition(&u.m, 0, &mut filtered, dt);
    assert!((pp(&filtered, dt) - pp(&sine, dt)).abs() > 0.2, "filtered {} file {}", pp(&filtered, dt), pp(&sine, dt));

    let hw = u.wave_samples(u.last[0].as_ref().unwrap(), WaveId::Ch(0)).unwrap();
    assert_eq!(hw.v.len(), 200);
    assert!((hw.dt - 2e-9).abs() < 1e-18);
    assert_eq!(hw.v[10], 9.0);
    assert_eq!(u.last[0].as_ref().unwrap().ch[0], screen[0]);
    assert!(matches!(u.src, Source::Demo { .. }));
    assert!(u.wave_samples(u.last[0].as_ref().unwrap(), WaveId::Ref(9)).is_none());
    u.refs[0].as_mut().unwrap().on = false;
    assert_eq!(u.wave_samples(u.last[0].as_ref().unwrap(), WaveId::Ref(0)).unwrap().v, sine);
    assert!(u.magnetic_options(Item::MagV).unwrap().iter().any(|s| s == "R1"));
    u.m.mag_v = WaveId::Ch(n_ch() - 1);
    assert!(u.magnetic_adjust(Item::MagV, 1));
    assert_eq!(u.m.mag_v, WaveId::Ref(0));
    assert_eq!(u.value(Item::MagV), "R1");

    // two windings from files, including one the screen cannot show at this rate
    let (mdt, mn) = (2e-6, 4000usize);
    let (mv, mi) = inductor_pair(1e-3, mdt, mn);
    write_wave(&dir, 1, 0.0, mdt, &mv);
    write_wave(&dir, 2, 0.0, mdt, &mi);
    u.ref_sel = 0;
    u.activate(Item::RefImport);
    u.ref_sel = 1;
    u.activate(Item::RefImport);
    u.m.view = AView::Magnetic;
    u.m.mag_v = WaveId::Ref(0);
    u.m.mag_i = WaveId::Ref(1);
    u.last[0] = None;
    u.analyzer_update();
    let expect = oos_dsp::magnetic::measure(&mv, &mi, mdt, oos_dsp::magnetic::Geom::default());
    let got = u.azr.mag.as_ref().unwrap();
    assert!(got.l.is_some() && expect.l.is_some(), "{} / {}", got.note, expect.note);
    assert!((got.l.unwrap() - expect.l.unwrap()).abs() < 1e-9, "{} vs {}", got.l.unwrap(), expect.l.unwrap());
    assert!((got.loss.unwrap() - expect.loss.unwrap()).abs() < 1e-9);
    u.m.ch[0].bw = BW_20M;
    u.analyzer_update();
    assert!((u.azr.mag.as_ref().unwrap().l.unwrap() - expect.l.unwrap()).abs() < 1e-9);
    assert!(u.last[0].is_none());
    assert!(matches!(u.src, Source::Demo { .. }));

    // The following fixtures replace refs directly, rather than importing a new
    // workflow record. Leave the import's stopped screen gate before testing
    // full-record analyser adapters at unrelated sample intervals.
    u.return_to_live();

    // three phase refs, no live record
    let (f, a, pn, pdt) = (50.0, 10.0, 2000usize, 1e-4);
    let w = 2.0 * std::f64::consts::PI * f;
    let phase = |shift: f64| -> Vec<f32> { (0..pn).map(|k| (a * (w * k as f64 * pdt - shift).cos()) as f32).collect() };
    let (pa, pb, pc) = (phase(0.0), phase(2.0 * std::f64::consts::PI / 3.0), phase(-2.0 * std::f64::consts::PI / 3.0));
    put_ref(&mut u, 0, 0.0, pdt, pa.clone());
    put_ref(&mut u, 1, 0.0, pdt, pb.clone());
    put_ref(&mut u, 2, 0.0, pdt, pc.clone());
    u.m.view = AView::Inverter;
    u.m.inv_page = InvPage::Dq0;
    u.m.inv_ang = InvAngle::Off;
    u.m.inv_a = WaveId::Ref(0);
    u.m.inv_b = WaveId::Ref(1);
    u.m.inv_c = WaveId::Ref(2);
    u.analyzer_update();
    let dq = u.azr.dq.as_ref().expect(&u.azr.inv_note);
    let direct = oos_dsp::motor::dq0(&pa, &pb, &pc, pdt, oos_dsp::motor::Theta::Vector).unwrap();
    assert!(
        (dq.d - direct.d).abs() < 1e-6 && (dq.q - direct.q).abs() < 1e-6,
        "{:?} {:?}",
        (dq.d, dq.q),
        (direct.d, direct.q)
    );
    assert!((dq.d - a).abs() < 0.15, "D {}", dq.d);

    // double pulse at 1 ns, far above a 250 MHz front end
    let (vds, id, gate) = double_pulse(1e-9);
    put_ref(&mut u, 0, 0.0, 1e-9, vds.clone());
    put_ref(&mut u, 1, 0.0, 1e-9, id.clone());
    put_ref(&mut u, 2, 0.0, 1e-9, gate.clone());
    u.m.view = AView::Dpt;
    u.m.dpt_v = WaveId::Ref(0);
    u.m.dpt_i = WaveId::Ref(1);
    u.m.dpt_g = WaveId::Ref(2);
    u.m.dpt_4th = DptFourth::Off;
    u.analyzer_update();
    let pulse = u.azr.dpt.as_ref().expect(&u.azr.dpt_note);
    let direct = oos_dsp::dpt::measure(&vds, &id, Some(&gate), oos_dsp::dpt::Fourth::Off, 1e-9, 10.0);
    assert!((pulse.vpeak.unwrap() - 480.0).abs() < 1e-3, "{}", pulse.vpeak.unwrap());
    assert!((pulse.vpeak.unwrap() - direct.vpeak.unwrap()).abs() < 1e-6);
    assert!((pulse.td_on.unwrap() - direct.td_on.unwrap()).abs() < 1e-12);
    u.m.ch[0].bw = BW_20M;
    u.analyzer_update();
    assert!((u.azr.dpt.as_ref().unwrap().vpeak.unwrap() - 480.0).abs() < 1e-3);

    // the finer ref keeps its samples; the coarser one is placed on that interval
    let mut coarse = vec![0.0f32; 20];
    coarse[0] = 1.0;
    let mut fine = vec![0.0f32; 40];
    fine[7] = 4.0;
    put_ref(&mut u, 0, 0.0, 2e-6, coarse);
    put_ref(&mut u, 1, 0.0, 1e-6, fine.clone());
    let taken = u
        .take_waves(
            u.last[0].as_ref().unwrap_or(&Record {
                t0: 0.0,
                dt: 1.0,
                ch: Default::default(),
                triggered: false,
                partial: false,
                raw: None,
                la: None,
                pairs: false,
            }),
            &[WaveId::Ref(0), WaveId::Ref(1)],
            &[],
        )
        .unwrap();
    assert!((taken.dt - 1e-6).abs() < 1e-18);
    assert_eq!(taken.v[1], fine);
    assert_eq!(taken.v[0].len(), fine.len());
    assert!(taken.moved);

    let _ = std::fs::remove_dir_all(&dir);
}

fn write_wave(dir: &std::path::Path, slot: usize, t0: f64, dt: f64, v: &[f32]) {
    let mut t = String::from("Time (s),Volts\n");
    for (k, x) in v.iter().enumerate() {
        t.push_str(&format!("{:.12e},{}\n", t0 + k as f64 * dt, x));
    }
    std::fs::write(dir.join(format!("ref{}.csv", slot)), t).unwrap();
}

fn put_ref(u: &mut Ui, slot: usize, t0: f64, dt: f64, v: Vec<f32>) {
    u.refs[slot] = Some(RefWave { vpd: 0.2, ..RefWave::test(v, dt, t0) });
}

/// Voltage of a 1 kHz triangle current in an ideal inductor.
fn inductor_pair(l: f64, dt: f64, n: usize) -> (Vec<f32>, Vec<f32>) {
    let f = 1000.0;
    let i = |t: f64| {
        let x = (t * f).fract();
        if x < 0.5 {
            -1.0 + 4.0 * x
        } else {
            3.0 - 4.0 * x
        }
    };
    let v: Vec<f32> = (0..n)
        .map(|k| {
            let t = k as f64 * dt;
            let x = (t * f).fract();
            let di = if x < 0.5 { 4.0 * f } else { -4.0 * f };
            (l * di) as f32
        })
        .collect();
    let cur: Vec<f32> = (0..n).map(|k| i(k as f64 * dt) as f32).collect();
    (v, cur)
}

fn pwl(p: &[(f64, f64)], t: f64) -> f64 {
    if t <= p[0].0 {
        return p[0].1;
    }
    for w in p.windows(2) {
        if t <= w[1].0 {
            return w[0].1 + (w[1].1 - w[0].1) * (t - w[0].0) / (w[1].0 - w[0].0);
        }
    }
    p[p.len() - 1].1
}

fn double_pulse(dt: f64) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let n = (13e-6 / dt) as usize;
    let gate = [
        (0.0, 0.0),
        (1e-6, 0.0),
        (1.02e-6, 10.0),
        (6e-6, 10.0),
        (6.02e-6, 0.0),
        (10e-6, 0.0),
        (10.02e-6, 10.0),
        (12e-6, 10.0),
        (12.02e-6, 0.0),
    ];
    let id = [
        (0.0, 0.0),
        (1.1e-6, 0.0),
        (1.2e-6, 10.0),
        (6.05e-6, 10.0),
        (6.15e-6, 0.0),
        (10.05e-6, 0.0),
        (10.15e-6, 10.0),
        (10.25e-6, 14.0),
        (10.35e-6, 10.0),
        (12e-6, 10.0),
    ];
    let vds = [
        (0.0, 400.0),
        (1.05e-6, 400.0),
        (1.15e-6, 1.0),
        (6.1e-6, 1.0),
        (6.2e-6, 400.0),
        (6.25e-6, 480.0),
        (6.4e-6, 400.0),
        (10.05e-6, 400.0),
        (10.15e-6, 1.0),
        (12e-6, 1.0),
    ];
    let sample = |p: &[(f64, f64)]| (0..n).map(|k| pwl(p, k as f64 * dt) as f32).collect();
    (sample(&vds), sample(&id), sample(&gate))
}

/// Measure, power, Bode, spectrum and the eye read a loaded ref at its own interval.
#[test]
fn a_loaded_ref_feeds_measure_power_bode_spectrum_and_eye() {
    let mut u = crate::ui::testkit::demo_ui();
    let (dt, n, f) = (5e-9, 800usize, 40e6);
    let sine: Vec<f32> = (0..n).map(|k| (2.0 * std::f64::consts::PI * f * k as f64 * dt).sin() as f32).collect();
    put_ref(&mut u, 0, 0.0, dt, sine.clone());
    u.measure_refs();
    u.m.ch[0].bw = BW_20M;
    u.m.ch[0].coupling = Coupling::Ac;
    u.m.meas = vec![(Meas::Vpp, WaveId::Ref(0))];
    let row = u.meas_rows().into_iter().find(|r| r.name == Meas::Vpp.name()).expect("pk-pk");
    let direct = oos_dsp::measure::analyze_min(&sine, dt, 0.4 * 0.2);
    assert!((row.v - direct.get(Meas::Vpp)).abs() < 1e-6, "{} vs {}", row.v, direct.get(Meas::Vpp));
    let mut filtered = sine.clone();
    crate::wave::condition(&u.m, 0, &mut filtered, dt);
    let filtered_pp = oos_dsp::measure::analyze_min(&filtered, dt, 0.4).get(Meas::Vpp);
    assert!((filtered_pp - row.v).abs() > 0.2, "filtered {} ref {}", filtered_pp, row.v);
    assert!(u.options(Item::MeasSrc).iter().any(|s| s == "R1"));
    u.meas_src = WaveId::Ch(n_ch() - 1);
    u.adjust(Item::MeasSrc, 1);
    assert_eq!(u.meas_src, WaveId::Ref(0));

    // a later copy of the same sine: delay is the difference of the two t0 values
    put_ref(&mut u, 1, 25e-9, dt, sine.clone());
    u.measure_refs();
    u.m.meas2 = vec![(0, WaveId::Ref(0), WaveId::Ref(1))];
    let delay = u.meas_rows().into_iter().find(|r| r.name.starts_with("Delay")).unwrap().v;
    assert!((delay - 25e-9).abs() < 2e-9, "delay {}", delay);

    u.last[0] = None;
    u.m.view = AView::Spectrum;
    u.m.spec_src = WaveId::Ref(0);
    u.m.spec_span = 0.0;
    u.m.spec_center = 0.0;
    u.analyzer_update();
    let peak = u.azr.peaks.iter().copied().max_by(|a, b| a.dbv.total_cmp(&b.dbv)).expect("a line");
    assert!((peak.f - f).abs() < 1e6, "peak {}", peak.f);
    let f_kept = peak.f;
    u.analyzer_update();
    let again = u.azr.peaks.iter().copied().max_by(|a, b| a.dbv.total_cmp(&b.dbv)).unwrap();
    assert!((again.f - f_kept).abs() < 1.0);

    // one record, however many times the stopped scope asks
    let sq: Vec<f32> = (0..8000).map(|k| if (k / 20) % 2 == 0 { 0.5 } else { -0.5 }).collect();
    put_ref(&mut u, 0, 0.0, 1e-9, sq.clone());
    u.m.view = AView::Eye;
    u.m.eye_src = WaveId::Ref(0);
    u.m.eye_rate = 0.0;
    u.azr.eye = None;
    u.now = 1.0;
    u.analyzer_update();
    u.analyzer_update();
    let clk = oos_dsp::eye::recover_clock(&sq, 1e-9, 0.0, 0.02, None).unwrap();
    let short = oos_dsp::eye::recover_clock(&sq[..3600], 1e-9, 0.0, 0.02, None).unwrap();
    assert!(clk.edges.len() > short.edges.len(), "{} vs {}", clk.edges.len(), short.edges.len());
    let n = oos_dsp::eye::tie(&clk.edges, &clk).len();
    assert_eq!(u.azr.page.status[0], format!("Jitter of {} edges, 1 records", n));

    // DQS on R2 folds DQ on R1: the UI is the strobe's edge spacing, not a CDR from the data.
    let ui = 1e-9;
    let dt = ui / 16.0;
    let n = 1600usize;
    let dq: Vec<f32> =
        (0..n).map(|k| if ((k as f64 * dt + 0.5 * ui) / ui).floor() as i32 % 2 == 0 { 1.0 } else { 0.0 }).collect();
    let dqs: Vec<f32> = (0..n).map(|k| if (k as f64 * dt / ui).floor() as i32 % 2 == 0 { 1.0 } else { 0.0 }).collect();
    put_ref(&mut u, 0, 0.0, dt, dq);
    put_ref(&mut u, 1, 0.0, dt, dqs);
    u.m.view = AView::Eye;
    u.m.eye_src = WaveId::Ref(0);
    u.m.eye_b = Some(WaveId::Ref(1));
    u.m.eye_rate = 0.0;
    u.azr.eye = None;
    u.now = 1.0;
    u.analyzer_update();
    let (_, _, period) = u.azr.eye_res.clone().expect("strobe eye");
    assert!((period - ui).abs() < ui * 1e-3, "period {period}");
    assert!(u.azr.page.title.contains("clock from R2"), "{}", u.azr.page.title);

    let (pdt, pn) = (1e-5, 2000usize);
    let tone = |amp: f64, shift: f64| -> Vec<f32> {
        (0..pn).map(|k| (amp * (2.0 * std::f64::consts::PI * 1000.0 * k as f64 * pdt + shift).sin()) as f32).collect()
    };
    let (vv, ii) = (tone(2.0, 0.0), tone(1.0, 0.0));
    put_ref(&mut u, 0, 0.0, pdt, vv.clone());
    put_ref(&mut u, 1, 0.0, pdt, ii.clone());
    u.m.view = AView::Power;
    u.m.pw_mode = PowerMode::Quality;
    u.m.pw_v = WaveId::Ref(0);
    u.m.pw_i = WaveId::Ref(1);
    u.analyzer_update();
    let q = oos_dsp::power::quality(&vv, &ii, pdt, None);
    assert_eq!(u.azr.power[0], format!("Vrms {}   Irms {}", si(q.vrms, "V"), si(q.irms, "A")));
    u.analyzer_update();
    assert_eq!(u.azr.power[0], format!("Vrms {}   Irms {}", si(q.vrms, "V"), si(q.irms, "A")));

    let bf = 1e6;
    let bdt = 1e-8;
    let bn = 4000usize;
    let bin: Vec<f32> = (0..bn).map(|k| (2.0 * std::f64::consts::PI * bf * k as f64 * bdt).sin() as f32).collect();
    let bout: Vec<f32> = bin.iter().map(|x| x * 0.5).collect();
    put_ref(&mut u, 0, 0.0, bdt, bin);
    put_ref(&mut u, 1, 0.0, bdt, bout);
    u.m.bode_in = WaveId::Ref(0);
    u.m.bode_out = WaveId::Ref(1);
    u.m.bode_start = bf;
    u.m.bode_stop = bf * 4.0;
    u.m.bode_points = 3;
    u.m.bode_gen = bode::BUILTIN;
    let run = u.m.run;
    u.bode_activate(Item::BodeRun);
    assert!(!u.bode.running);
    assert_eq!(u.m.run, run);
    let (hz, g, _) = u.bode.points[0];
    assert!((hz - bf).abs() / bf < 1e-6, "{}", hz);
    assert!((g - 20.0 * 0.5f64.log10()).abs() < 0.05, "gain {}", g);
    assert!(matches!(u.src, Source::Demo { .. }));
}

fn load_dq_dqs(u: &mut Ui) -> bool {
    let dq = std::path::Path::new("/home/ubuntu/.cursor/projects/agent-repos-oos/uploads/ch3-dq_67a5.wfm");
    let dqs = std::path::Path::new("/home/ubuntu/.cursor/projects/agent-repos-oos/uploads/ch1-dqs_a0b1.wfm");
    if !dq.is_file() || !dqs.is_file() {
        return false;
    }
    for c in 0..NCH {
        u.m.ch[c].on = false;
    }
    u.m.la.on = false;
    u.m.run = RunState::Stop;
    u.last[0] = None;
    u.ref_sel = 0;
    u.ref_load_path(dq);
    u.ref_sel = 1;
    u.ref_load_path(dqs);
    true
}

#[test]
fn dq_and_dqs_refs_make_a_strobe_eye() {
    let mut u = crate::ui::testkit::demo_ui();
    if !load_dq_dqs(&mut u) {
        return;
    }
    let r1 = u.refs[0].as_ref().unwrap();
    let r2 = u.refs[1].as_ref().unwrap();
    assert_eq!(r1.v.len(), 1_000_000);
    assert_eq!(r2.v.len(), 1_000_000);
    assert!((r1.dt - 2e-11).abs() < 1e-18 && (r2.dt - 2e-11).abs() < 1e-18);
    assert!((r1.t0 - r2.t0).abs() < 1e-18);
    u.m.meas = [
        (Meas::Vpp, WaveId::Ref(0)),
        (Meas::Vmax, WaveId::Ref(0)),
        (Meas::Vmin, WaveId::Ref(0)),
        (Meas::Freq, WaveId::Ref(0)),
        (Meas::Vpp, WaveId::Ref(1)),
        (Meas::Vmax, WaveId::Ref(1)),
        (Meas::Period, WaveId::Ref(1)),
        (Meas::Freq, WaveId::Ref(1)),
    ]
    .into();
    u.m.meas_labels = true;
    u.meas_src = WaveId::Ref(0);
    u.measure_refs();
    if let Some(r) = u.refs[1].as_mut() {
        r.color = 1;
    }
    u.m.tscale = tscales().iter().position(|&t| (t - 2e-6).abs() < 1e-12).unwrap();
    u.m.tfine = 1.0;
    u.m.delay = -1.06e-6;
    u.set_zoom(true);
    u.m.zscale = tscales().iter().position(|&t| (t - 1e-9).abs() < 1e-12).unwrap();
    u.m.zdelay = -1.06e-6;
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    if let Ok(dir) = std::env::var("OOS_SHOT_DIR") {
        let _ = std::fs::write(format!("{}/ddr-dq-dqs-scope.png", dir), crate::store::png_of(&cv));
    }
    u.m.view = AView::Eye;
    u.m.eye_src = WaveId::Ref(0);
    u.m.eye_b = Some(WaveId::Ref(1));
    u.m.eye_rate = 0.0;
    u.azr.eye = None;
    u.now = 1.0;
    u.analyzer_update();
    let (meas, _, period) = u.azr.eye_res.clone().expect("DQ vs DQS eye");
    eprintln!(
        "DQS UI {} s ({:.3} GT/s) height {} V width {} s title {}",
        period,
        1e-9 / period,
        meas.height,
        meas.width,
        u.azr.page.title
    );
    assert!(period > 1e-10 && period < 2e-9, "UI {period}");
    assert!(u.azr.page.title.contains("clock from R2"), "{}", u.azr.page.title);
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    if let Ok(dir) = std::env::var("OOS_SHOT_DIR") {
        let _ = std::fs::write(format!("{}/ddr-dq-dqs-eye.png", dir), crate::store::png_of(&cv));
    }
}

#[test]
fn cphy_eye_full_screen_shot() {
    let dir = std::path::Path::new("/home/ubuntu/.cursor/projects/agent-repos-oos/uploads");
    let va = dir.join("VA_SE_f764.wfm");
    let clk = dir.join("ClockRecovery_b5e0.wfm");
    if !va.is_file() || !clk.is_file() {
        return;
    }
    let mut u = crate::ui::testkit::demo_ui();
    for c in 0..NCH {
        u.m.ch[c].on = false;
    }
    u.m.la.on = false;
    u.m.run = RunState::Stop;
    u.last[0] = None;
    u.m.meas.clear();
    u.ref_sel = 0;
    u.ref_load_path(&va);
    if let Some(r) = u.refs[0].as_mut() {
        r.label = "VA".into();
    }
    let b = std::fs::read(&clk).unwrap();
    let n = (b.len() - 8 - 820) / 4;
    let mut v = Vec::with_capacity(n);
    for k in 0..n {
        let o = 820 + k * 4;
        v.push(f32::from_le_bytes(b[o..o + 4].try_into().unwrap()));
    }
    let dt = f64::from_le_bytes(b[478..486].try_into().unwrap());
    let t0 = f64::from_le_bytes(b[486..494].try_into().unwrap());
    u.refs[3] = Some(RefWave { color: 3, label: "CLK".into(), vpd: 0.5, ..RefWave::test(v, dt, t0) });
    u.full = true;
    u.relayout();
    u.m.view = AView::Eye;
    u.m.eye_src = WaveId::Ref(0);
    u.m.eye_b = Some(WaveId::Ref(3));
    u.m.eye_rate = 2.27e9;
    u.azr.eye = None;
    u.now = 1.0;
    u.analyzer_update();
    assert!(u.azr.eye.is_some());
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    if let Ok(dir) = std::env::var("OOS_SHOT_DIR") {
        let _ = std::fs::write(format!("{}/cphy-va-eye-full.png", dir), crate::store::png_of(&cv));
    }
}

/// R1–R10 answer `:MEASure:ITEM?`. The strip, the eye figures, and one readout of everything on screen.
#[test]
fn scpi_reads_a_ref_the_strip_and_the_eye() {
    let (mut scpi, mut u, cv) = crate::ui::testkit::scpi_session();
    for _ in 0..4 {
        u.step(0.05);
    }
    let q = |scpi: &mut crate::scpi::Scpi, u: &mut Ui, c: &str| crate::ui::testkit::q(scpi, u, &cv, c);

    let ch = q(&mut scpi, &mut u, ":MEAS:ITEM? VPP,CHAN1");
    assert!(ch.contains('E') && ch != "9.9E37", "{ch}");
    for item in ["FREQUENCY", "PERIOD", "RTIME", "VBASE", "CYCAMP", "CYCPP", "HTIME", "BWIDTH"] {
        let r = q(&mut scpi, &mut u, &format!(":MEAS:ITEM? {item}"));
        assert!(r.contains('E'), "{item} {r}");
    }
    assert!(q(&mut scpi, &mut u, ":SYST:ERR?").starts_with("0,"), "a known item must not be an error");

    let (dt, n) = (5e-9, 800usize);
    let sine: Vec<f32> = (0..n).map(|k| (2.0 * std::f64::consts::PI * 1e6 * k as f64 * dt).sin() as f32).collect();
    put_ref(&mut u, 0, 0.0, dt, sine.clone());
    u.measure_refs();
    let want = oos_dsp::measure::analyze_min(&sine, dt, 0.4 * 0.2).get(Meas::Vpp);
    let got: f64 = q(&mut scpi, &mut u, ":MEAS:ITEM? VPP,R1").parse().unwrap();
    assert!((got - want).abs() < 1e-4, "{got} vs {want}");
    assert_eq!(q(&mut scpi, &mut u, ":MEAS:ITEM? VPP,R2"), "9.9E37");
    assert!(q(&mut scpi, &mut u, ":SYST:ERR?").starts_with("0,"), "an empty slot is not an error");
    assert_eq!(q(&mut scpi, &mut u, ":MEAS:ITEM? VPP,R11"), "9.9E37");
    assert!(q(&mut scpi, &mut u, ":SYST:ERR?").starts_with("-224"));
    assert_eq!(q(&mut scpi, &mut u, ":MEAS:ITEM? NOPE,R1"), "9.9E37");
    assert!(q(&mut scpi, &mut u, ":SYST:ERR?").starts_with("-224"));

    put_ref(&mut u, 1, 25e-9, dt, sine);
    u.measure_refs();
    let delay: f64 = q(&mut scpi, &mut u, ":MEAS:ITEM? RRDELAY,R1,R2").parse().unwrap();
    assert!((delay - 25e-9).abs() < 2e-9, "delay {delay}");
    assert_eq!(q(&mut scpi, &mut u, ":MEAS:ITEM? SETUP,R1"), "9.9E37");
    assert!(q(&mut scpi, &mut u, ":SYST:ERR?").starts_with("-224"));

    u.m.meas = vec![(Meas::Vpp, WaveId::Ref(0)), (Meas::Freq, WaveId::Ch(0))];
    u.m.meas2 = vec![(0, WaveId::Ref(0), WaveId::Ref(1))];
    u.m.dvm_on = true;
    let strip = q(&mut scpi, &mut u, ":MEAS:STR?");
    assert!(strip.contains("VPP,R1,"), "{strip}");
    assert!(strip.contains("FREQUENCY,CHAN1,"), "{strip}");
    assert!(strip.contains("RRDELAY,R1,R2,"), "{strip}");
    assert!(strip.contains("DVM,DC,CHAN1,"), "{strip}");

    u.m.view = AView::Scope;
    let screen = q(&mut scpi, &mut u, ":MEAS:READ?");
    assert!(screen.contains("VPP,R1,"), "{screen}");
    assert!(!screen.contains("HEIGHT"), "{screen}");

    let sq: Vec<f32> = (0..8000).map(|k| if (k / 20) % 2 == 0 { 0.5 } else { -0.5 }).collect();
    put_ref(&mut u, 2, 0.0, 1e-9, sq);
    u.m.view = AView::Eye;
    u.m.eye_src = WaveId::Ref(2);
    u.m.eye_b = None;
    u.m.eye_rate = 0.0;
    u.azr.eye = None;
    u.now = 1.0;
    u.analyzer_update();
    let height: f64 = q(&mut scpi, &mut u, ":EYE:ITEM? HEIGHT").parse().unwrap();
    let width: f64 = q(&mut scpi, &mut u, ":EYE:ITEM? WIDT").parse().unwrap();
    let ui_s: f64 = q(&mut scpi, &mut u, ":EYE:ITEM? UI").parse().unwrap();
    let tj = q(&mut scpi, &mut u, ":EYE:ITEM? TJ");
    assert!(height > 0.5 && height < 1.2, "height {height}");
    assert!(width > 0.0 && width < 40e-9, "width {width}");
    assert!((ui_s - 20e-9).abs() < 2e-9, "ui {ui_s}");
    assert!(tj.contains('E'), "{tj}");
    assert_eq!(q(&mut scpi, &mut u, ":EYE:ITEM? NOPE"), "9.9E37");
    let screen = q(&mut scpi, &mut u, ":MEAS:READ?");
    for token in ["VPP,R1,", "HEIGHT,", "WIDTH,", "UI,", "TJ,", "AMPLITUDE,", "DVM,DC,CHAN1,"] {
        assert!(screen.contains(token), "{token} missing in {screen}");
    }
    assert_eq!(screen.matches(';').count() + 1, strip.matches(';').count() + 1 + EYE_ITEMS.len(), "{screen}");
}

#[test]
fn measurement_queries_read_a_channel_or_a_ref() {
    let mut u = crate::ui::testkit::demo_ui();
    for _ in 0..8 {
        u.step(0.05);
    }
    let cv = Canvas::new(W, H);
    let mut s = crate::scpi::Scpi::open(0);
    let ask = |s: &mut crate::scpi::Scpi, u: &mut Ui, cmd: &str| {
        String::from_utf8_lossy(&s.exec_line(u, &cv, cmd)).into_owned()
    };
    let ch = ask(&mut s, &mut u, ":MEAS:ITEM? VPP");
    assert!(ch.parse::<f64>().unwrap().is_finite() && ch != "9.9E37", "{}", ch);

    let mut ramp = vec![0.0f32; 40];
    ramp.extend((0..100).map(|i| i as f32 / 99.0));
    ramp.extend(vec![1.0; 80]);
    put_ref(&mut u, 0, 0.0, 1e-9, ramp);
    assert_eq!(ask(&mut s, &mut u, ":MEAS:REFS?"), "10/90");
    let r10 = ask(&mut s, &mut u, ":MEAS:ITEM? RTIM,R1").parse::<f64>().unwrap();
    ask(&mut s, &mut u, ":MEAS:REFS 20/80");
    assert_eq!(ask(&mut s, &mut u, ":MEAS:REFS?"), "20/80");
    let r20 = ask(&mut s, &mut u, ":MEAS:ITEM? RTIME,REF1").parse::<f64>().unwrap();
    assert!(r20 < r10 && r10.is_finite(), "10/90 {} 20/80 {}", r10, r20);
    let med = ask(&mut s, &mut u, ":MEAS:ITEM? MEDIAN,R1").parse::<f64>().unwrap();
    assert!(med.is_finite(), "{}", med);
    let mut bits = Vec::new();
    for k in 0..1600 {
        bits.push(if (k / 100) % 2 == 0 { 0.0 } else { 1.0 });
    }
    put_ref(&mut u, 3, 0.0, 1e-9, bits);
    let ui_est = ask(&mut s, &mut u, ":MEAS:ITEM? ESTUI,R4").parse::<f64>().unwrap();
    assert!((ui_est - 200e-9).abs() < 20e-9 || (ui_est - 100e-9).abs() < 20e-9, "{}", ui_est);
    assert!(ask(&mut s, &mut u, ":MEAS:ITEM? ONES,R4").parse::<f64>().unwrap() > 0.0);
    assert_eq!(ask(&mut s, &mut u, ":MEAS:ITEM? VPP,R11"), "9.9E37");
    assert!(ask(&mut s, &mut u, ":SYST:ERR?").contains("-224"));

    // clock rises at 100 ns and 200 ns; data leads by 10 ns and holds for 40 ns
    let edge = |at: &[usize]| {
        let mut v = vec![0.0f32; 300];
        for &t in at {
            for x in v.iter_mut().skip(t).take(40) {
                *x = 1.0;
            }
        }
        v
    };
    put_ref(&mut u, 0, 0.0, 1e-9, edge(&[100, 200]));
    put_ref(&mut u, 1, 0.0, 1e-9, edge(&[90, 190]));
    // A is the data, B the clock, as on the measurement page
    let setup = ask(&mut s, &mut u, ":MEAS:ITEM? SETUP,R2,R1").parse::<f64>().unwrap();
    assert!((setup - 10e-9).abs() < 1e-12, "{}", setup);
    let hold = ask(&mut s, &mut u, ":MEAS:ITEM? HOLD,R2,R1").parse::<f64>().unwrap();
    assert!((hold - 30e-9).abs() < 1e-12, "{}", hold);
    ask(&mut s, &mut u, ":MEAS:TSET 20e-9");
    ask(&mut s, &mut u, ":MEAS:THOL 5e-9");
    assert_eq!(ask(&mut s, &mut u, ":MEAS:ITEM? SETHOLD,R2,R1").parse::<f64>().unwrap(), 2.0);
    ask(&mut s, &mut u, ":MEAS:TSET 5e-9");
    assert_eq!(ask(&mut s, &mut u, ":MEAS:ITEM? SETHOLD,R2,R1").parse::<f64>().unwrap(), 0.0);

    let pdt = 1e-4;
    let tone = |amp: f64, shift: f64| -> Vec<f32> {
        (0..2000).map(|k| (amp * (2.0 * std::f64::consts::PI * 50.0 * k as f64 * pdt + shift).sin()) as f32).collect()
    };
    put_ref(&mut u, 0, 0.0, pdt, tone(2.0, 0.0));
    put_ref(&mut u, 1, 0.0, pdt, tone(1.0, 0.0));
    let vrms = ask(&mut s, &mut u, ":POW:ITEM? VRMS,R1,R2").parse::<f64>().unwrap();
    assert!((vrms - 2.0 / 2.0f64.sqrt()).abs() < 0.05, "{}", vrms);
    let harm = ask(&mut s, &mut u, ":POW:HARM? R2");
    assert!(harm.starts_with("1,"), "{}", harm);
    put_ref(&mut u, 2, 0.0, pdt, tone(1.0, 2.0 * std::f64::consts::PI / 3.0));
    let freq = ask(&mut s, &mut u, ":INV:ITEM? FREQ,R1,R2,R3").parse::<f64>().unwrap();
    assert!((freq - 50.0).abs() < 1.0, "{}", freq);
    let f1 = ask(&mut s, &mut u, ":SPEC:ITEM? F1,R1").parse::<f64>().unwrap();
    assert!((f1 - 50.0).abs() < 1.0, "{}", f1);

    let (mv, mi) = inductor_pair(1e-3, 2e-6, 4000);
    put_ref(&mut u, 0, 0.0, 2e-6, mv);
    put_ref(&mut u, 1, 0.0, 2e-6, mi);
    let l = ask(&mut s, &mut u, ":MAG:ITEM? L,R1,R2").parse::<f64>().unwrap();
    assert!((l - 1e-3).abs() < 1e-4, "{}", l);

    let (vds, id, gate) = double_pulse(1e-9);
    put_ref(&mut u, 0, 0.0, 1e-9, vds);
    put_ref(&mut u, 1, 0.0, 1e-9, id);
    put_ref(&mut u, 2, 0.0, 1e-9, gate);
    let vp = ask(&mut s, &mut u, ":DPT:ITEM? VPEAK,R1,R2,R3").parse::<f64>().unwrap();
    assert!((vp - 480.0).abs() < 5.0, "{}", vp);

    ask(&mut s, &mut u, ":BODE:MODE PSRR");
    assert_eq!(ask(&mut s, &mut u, ":BODE:MODE?"), "PSRR");
    ask(&mut s, &mut u, ":BODE:MODE IMP");
    assert_eq!(ask(&mut s, &mut u, ":BODE:MODE?"), "IMP");

    let mut sq = Vec::new();
    for k in 0..8000 {
        let p = k % 400;
        let x = if p < 40 {
            p as f32 / 40.0
        } else if p < 200 {
            1.0
        } else if p < 240 {
            1.0 - (p - 200) as f32 / 40.0
        } else {
            0.0
        };
        sq.push(x);
    }
    put_ref(&mut u, 0, 0.0, 1e-8, sq);
    ask(&mut s, &mut u, ":MEAS:REFS 10/90");
    let rise = ask(&mut s, &mut u, ":BAND:ITEM? RISE,R1").parse::<f64>().unwrap();
    assert!(rise.is_finite() && rise > 0.0, "{}", rise);
}
