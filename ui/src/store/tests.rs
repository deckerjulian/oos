// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The tests of the storage module: setup files, CSV, OOS's waveform and fast-frame files, PNG and PPM pictures, file
//! names and safe writing. A test of a new part of `store` goes here; one of an importer of another instrument's files
//! into `header_import_tests.rs`.

use super::*;

// Break it catches: an importer that indexes past the end of a damaged file and panics (the UI thread with it)
// instead of refusing it. Random bodies behind each recognised header, some with small numbers where counts and
// offsets are read.
#[test]
fn damaged_waveform_files_are_refused_not_a_panic() {
    let mut seed = 0x1234_5678_9abc_def0u64;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let heads: [&[u8]; 6] =
        [b"OOSW\x01\x00\x00\x00", b"\x0F\x0F:WFM#001", b":CURVE #", b"WAVEDESC", b"AG10", b"OOSF\x01\x00\x00\x00"];
    for it in 0..20_000 {
        let mut b = heads[it % heads.len()].to_vec();
        for _ in 0..rnd() % 600 {
            let r = rnd();
            b.push(if r % 3 == 0 { 0 } else { (r >> 8) as u8 });
        }
        if it % 5 == 0 {
            for k in (8..b.len().saturating_sub(4)).step_by(4) {
                if rnd() % 2 == 0 {
                    b[k..k + 4].copy_from_slice(&((rnd() % 300) as u32).to_le_bytes());
                }
            }
        }
        let _ = parse_wave(&b, ["a.bin", "a.wfm", "a.isf", "a.trc", "a.bin", "a.bin"][it % 6]);
        let _ = parse_ff(&b);
    }
    let f = Fields::new(b"\x01\x02\x03", true);
    assert_eq!((f.u16(1), Fields::new(b"\x01\x02\x03", false).u16(1)), (Ok(0x0302), Ok(0x0203)));
    assert_eq!(f.u32(0), Err("truncated waveform".to_string()));
    assert_eq!(f.u16(usize::MAX), Err("truncated waveform".to_string()));
}
use crate::gfx::{rgb, Canvas};
use crate::math::{MathOp, Window};

#[test]
fn a_save_does_not_follow_links_or_write_to_devices() {
    let d = crate::ui::testkit::Scratch::new("link");
    std::fs::create_dir_all(&d).unwrap();
    let (victim, link) = (d.join("victim"), d.join("shot-001.png"));
    std::fs::write(&victim, b"keep").unwrap();
    std::os::unix::fs::symlink(&victim, &link).unwrap();
    assert!(write(&link, b"x").is_err());
    assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
    // taken, also when it dangles
    std::fs::remove_file(&victim).unwrap();
    assert_eq!(next_name(&d, "shot", "png"), d.join("shot-002.png"));
    assert!(write(&link, b"x").is_err());
    assert!(!victim.exists());
    assert!(write(Path::new("/dev/null"), b"x").is_err());
    write(&d.join("ok.txt"), b"1").unwrap();
    write(&d.join("ok.txt"), b"2").unwrap();
    assert_eq!(std::fs::read(d.join("ok.txt")).unwrap(), b"2");
    let _ = std::fs::remove_dir_all(&d);
}

// Break it catches: the hardware average switch lost in a setup or on by default, the count not stored with it.
#[test]
fn average_settings_round_trip() {
    let mut m = Model::default();
    assert!(!m.avg_hw);
    (m.acq, m.avg_count, m.avg_hw) = (AcqMode::Average, 64, true);
    let back = parse_setup(&setup_text(&m)).unwrap();
    assert_eq!((back.acq, back.avg_count, back.avg_hw), (AcqMode::Average, 64, true));
    // older setups without the key: software average
    assert!(!parse_setup("acq = Average\navg_count = 8\n").unwrap().avg_hw);
    assert!(parse_setup("avg_hw = maybe\n").is_err());
}

// Break it catches: hardware segment settings lost in a setup, a count clamped to the software limit, a negative
// or absurd interval reaching 0x4068.
#[test]
fn segment_settings_round_trip() {
    let mut m = Model::default();
    (m.segments, m.seg_hw, m.seg_ivl) = (20_000, true, 1e-3);
    let back = parse_setup(&setup_text(&m)).unwrap();
    assert_eq!((back.segments, back.seg_hw, back.seg_ivl), (20_000, true, 1e-3));
    let d = parse_setup("segments = 900000\nseg_interval = -1\n").unwrap();
    assert_eq!((d.segments, d.seg_hw, d.seg_ivl), (crate::model::SEG_HW_MAX, false, 0.0));
    assert_eq!(parse_setup("seg_interval = 1e9\n").unwrap().seg_ivl, crate::model::SEG_IVL_MAX);
}

#[test]
fn tdiv_by_value_and_legacy_index() {
    let m = parse_setup("tdiv = 1e-9\nzoom_tdiv = 1e-9\n").unwrap();
    assert_eq!((tscales()[m.tscale], tscales()[m.zscale]), (1e-9, 1e-9));
    assert!(setup_text(&m).contains("tdiv = 0.000000001"));
    // old ladder started at 5 ns: index 0 was 5 ns/div, 3 was 50 ns
    let m = parse_setup("tscale = 3\nzscale = 0\n").unwrap();
    assert_eq!((tscales()[m.tscale], tscales()[m.zscale]), (50e-9, 5e-9));
    assert!(parse_setup("tdiv = 3e-9").is_err());
}

#[test]
fn vdiv_by_value_and_legacy_index() {
    let m = parse_setup("ch1.vdiv = 0.0001\nch2.vscale = 10\n").unwrap();
    assert_eq!(VSCALES[m.ch[0].vscale], 100e-6);
    // old ladder started at 500 uV: index 10 was 1 V/div
    assert_eq!(VSCALES[m.ch[1].vscale], 1.0);
    assert_eq!(VSCALES[Model::default().ch[0].vscale], 1.0);
    assert!(setup_text(&m).contains("ch1.vdiv = 0.0001"));
}

/// A non-default set-up in every group of fields.
pub(super) fn custom() -> Model {
    let mut m = Model::default();
    (m.bode_sw_time, m.bode_sw_log, m.bode_sync, m.bode_sync_slope, m.bode_sync_level, m.bode_sw_in, m.bode_ref) =
        (12.5, false, 2, Slope::Fall, -0.75, 0, -6.02);
    m.ch[1].on = true;
    m.ch[1].vscale = 1; // 200 uV/div
    m.ch[1].offset = -0.123_456_79;
    m.ch[2].coupling = Coupling::Ac;
    m.ch[2].bw = BW_20M;
    m.ch[1].bw = 17;
    m.ch[3].probe = 10.0;
    m.ch[3].invert = true;
    m.active = 2;
    m.tscale = 5;
    m.delay = 1.5e-7;
    m.trig_src = 3;
    m.trig_level = 0.1;
    m.slope = Slope::Either;
    m.trig_mode = TrigMode::Normal;
    m.trig_type = TrigType::SetupHold;
    m.trig_level2 = -2.25;
    m.trig_src2 = 1;
    m.trig_slope2 = Slope::Fall;
    m.trig_pol = Pol::Neg;
    m.trig_when = WhenKind::Out;
    m.trig_t1 = 3e-9;
    m.trig_t2 = 0.7;
    m.video_std = VideoStd::Ntsc;
    m.video_sync = VSync::Line;
    m.video_line = 123;
    m.pat = [Bit::R, Bit::F, Bit::L, Bit::X];
    m.pat_levels = [0.1, -0.2, 3.3, 1e-3];
    m.win_mode = WinMode::Inside;
    m.nth_idle = 2e-3;
    m.nth_n = 7;
    m.setup = 5e-9;
    m.hold = 6e-9;
    m.acq = AcqMode::Average;
    m.avg_count = 256;
    m.mdepth = 4;
    m.persist = 6;
    m.intensity = 85;
    m.grid = Grid::Light;
    m.meas = vec![(Meas::Rise, WaveId::Ch(3)), (Meas::Vrms, WaveId::Ch(0)), (Meas::PDuty, WaveId::Ch(1))];
    m.strip_dock = StripDock::Right;
    m.cursor = CursorMode::Volt;
    m.cur_a = 0.125;
    m.cur_b = 0.875;
    m.touch_lock = true;
    m.aux_out = AuxOut::Off;
    m.ser = SerialTrig {
        rel: Rel::Out,
        max: 0x1_2345_6789,
        mask: 0xf0f0,
        uart_inv: true,
        uart_stop: StopBits::Two,
        uart_err: UartErr::Parity,
        spi_cs_high: true,
        spi_timeout: 2e-6,
        addr_rel: Rel::In,
        addr_max: 0x3a0,
        dir: I2cDir::Read,
        ten_bit: true,
        bytes: 4,
    };
    m.trig_bus_val = 0xab_cdef_0123;
    m.maths[3] = crate::math::MathCh {
        op: MathOp::Bpf,
        a: 4,
        b: 0,
        scale: 7,
        fc: 1234.0,
        bw: 56.0,
        offset: -1.5,
        win: Window::Blackman,
        lin: (1.0, 0.0),
    };
    m.math_bw = 777.0;
    m.zones[1] =
        crate::zone::Zone { on: true, src: 2, inside: false, pts: vec![(-1e-4, 0.5), (2.5e-5, 1.25), (3e-5, -0.75)] };
    m.dots = true;
    m.z_src = Some(3);
    m.hist = Hist::Horizontal;
    (m.view, m.spec_src, m.spec_center, m.spec_span, m.spec_rbw, m.spec_trace, m.spec_ref, m.spec_wf, m.spec_dist) =
        (AView::Spectrum, WaveId::Ch(2), 1.5e6, 2e5, 1e3, 2, -12.5, true, true);
    (m.spec_sort, m.spec_harm, m.trend_sel) = (1, 4, [3, 1]);
    (m.eye_src, m.eye_rate, m.pw_v, m.pw_i, m.pw_mode) =
        (WaveId::Ch(3), 1.25e9, WaveId::Ch(2), WaveId::Ch(0), PowerMode::Switching);
    m.math = MathOp::Fft;
    m.math_a = 2;
    m.math_b = 3;
    m.math_scale = 2;
    m.math_fc = 12345.0;
    m.math_offset = -1.3;
    m.fft_win = Window::Flattop;
    m.dec_on = true;
    m.dec_src = 1;
    m.dec_baud = 3;
    m.dec_thr = 0.8;
    m.dec_ascii = false;
    m.dec_proto = Proto::SPI;
    (m.dec_arate, m.dec_tick, m.dec_nibbles, m.dec_uspeed) = (2, 3, 4, 2);
    m.dec_src2 = 0;
    m.dec_cs = None;
    m.dec_mode = 3;
    m.dec_bits = 16;
    m.dec_parity = Parity::Odd;
    m.zoom = true;
    m.zscale = 2;
    m.zdelay = -3e-8;
    m.color_grade = true;
    m.mask_on = true;
    m.mask_src = 3;
    m.mask_x = 0.5;
    m.mask_y = 1.25;
    m.mask_stop = true;
    m.quick = Quick::SaveCsv;
    m.la = crate::model::la::LaSet {
        on: true,
        show: 0x0f0f,
        preset: [2, crate::ui::la::USER],
        user: [0.5, -0.25],
        height: 2,
    };
    m
}

#[test]
fn a_ref_in_a_setup_stays_a_ref_and_a_channel_number_stays_a_channel() {
    let m = Model {
        mag_v: WaveId::Ref(1),
        mag_i: WaveId::Ch(0),
        dpt_v: WaveId::Ref(0),
        inv_a: WaveId::Ref(4),
        inv_tq: Some(WaveId::Ref(9)),
        spec_src: WaveId::Ref(0),
        eye_src: WaveId::Ref(2),
        pw_v: WaveId::Ref(1),
        pw_i: WaveId::Ch(0),
        bode_in: WaveId::Ref(3),
        bode_out: WaveId::Ch(1),
        meas: vec![(Meas::Vpp, WaveId::Ref(0))],
        meas2: vec![(0, WaveId::Ref(0), WaveId::Ch(1))],
        ..Default::default()
    };
    let text = setup_text(&m);
    assert!(text.contains("magnetic = R2 1 "));
    assert!(text.contains("dpt = R1 "));
    assert!(text.contains("spec = R1 "));
    assert!(text.contains("eye = R3 "));
    assert!(text.contains("power = R2 1 "));
    assert!(text.contains("bode = "));
    assert!(text.contains(" R4 2 "));
    assert!(text.contains("meas = Vpp:R1"));
    assert!(text.contains("meas2 = 0:R1:2"));
    let back = parse_setup(&text).unwrap();
    assert_eq!(
        (back.mag_v, back.mag_i, back.dpt_v, back.inv_a, back.inv_tq),
        (m.mag_v, m.mag_i, m.dpt_v, m.inv_a, m.inv_tq)
    );
    assert_eq!(
        (back.spec_src, back.eye_src, back.pw_v, back.pw_i, back.bode_in, back.bode_out),
        (m.spec_src, m.eye_src, m.pw_v, m.pw_i, m.bode_in, m.bode_out)
    );
    assert_eq!((back.meas, back.meas2), (m.meas, m.meas2));
    // a setup written before refs were sources
    let old = parse_setup("magnetic = 1 2 1 0 0 0 IPsi 0\ninverter = Dq0 1 2 3 Off 1 4 true off off off 1 1 0 false 0 1 off 1024 false 0 1 2\ndpt = 1 2 3 Off 4 10 Both\n").unwrap();
    assert_eq!(old.mag_v, WaveId::Ch(0));
    assert_eq!(old.mag_i, WaveId::Ch(1));
    assert_eq!(old.inv_a, WaveId::Ch(0));
    assert_eq!(old.dpt_g, WaveId::Ch(2));
    assert_eq!(old.inv_tq, None);
}

#[test]
fn catalog_setup_validates_ids_sources_duplicates_and_shared_selection_limit() {
    let mut model = Model::default();
    set(&mut model, "cat", "90:R1:R2:3:4 170:R3:R4:R5:4").unwrap();
    let text = setup_text(&model);
    let restored = parse_setup(&text).unwrap();
    assert_eq!(restored.cat, model.cat);
    for invalid in ["999:R1:R2:3:4", "1:R1:R2:3:4", "90:R99:R2:3:4", "90:R1:R2:3", "90:R1:R2:3:4 90:R1:R2:3:4"] {
        let before = model.cat.clone();
        assert!(set(&mut model, "cat", invalid).is_err(), "{invalid}");
        assert_eq!(model.cat, before);
    }
    model.meas = vec![(Meas::Vpp, WaveId::Ch(0)); MEAS_SELECTION_MAX - 1];
    model.meas2.clear();
    model.cat.clear();
    assert!(set(&mut model, "cat", "90:R1:R2:3:4 170:R3:R4:R5:4").is_err());
    assert!(model.cat.is_empty());
    set(&mut model, "cat", "90:R1:R2:3:4").unwrap();
    assert!(set(&mut model, "meas2", "0:1:2").is_err());
    assert!(model.meas2.is_empty());
    set(&mut model, "cat", "").unwrap();
    assert!(model.cat.is_empty());
}

#[test]
fn setup_round_trip() {
    let m = custom();
    assert_ne!(m, Model::default());
    let text = setup_text(&m);
    let back = parse_setup(&text).unwrap();
    assert_eq!(back, m);
    // and the default set-up
    assert_eq!(parse_setup(&setup_text(&Model::default())).unwrap(), Model::default());
}

#[test]
fn setup_sim_advanced_round_trip_and_legacy_defaults() {
    let model = Model {
        sim_prbs: 13,
        sim_ssc_center: true,
        sim_ssc_deviation: 0.01,
        sim_ssc_hz: 40e3,
        sim_dcd_pp: 8e-12,
        sim_corr_rj: 2e-12,
        sim_corr_hz: 2e6,
        ..Default::default()
    };
    let setup = setup_text(&model);
    assert_eq!(parse_setup(&setup).unwrap(), model);
    let legacy = setup.lines().filter(|line| !line.starts_with("sim_extra =")).collect::<Vec<_>>().join("\n");
    let old = parse_setup(&legacy).unwrap();
    assert_eq!(old.sim_prbs, 13);
    assert!(!old.sim_ssc_center);
    assert_eq!(old.sim_ssc_deviation, 0.005);
    assert_eq!(old.sim_ssc_hz, 33e3);
    assert_eq!(old.sim_dcd_pp, 0.0);
    assert_eq!(old.sim_corr_rj, 0.0);
    assert_eq!(old.sim_corr_hz, 1e6);
    for invalid in
        ["true NaN 33000 0 0 1e6", "false .005 0 0 0 1e6", "true .005 33000 -1 0 1e6", "false .005 33000 0 1 1e6"]
    {
        assert!(parse_setup(&format!("sim_extra = {invalid}\n")).is_err());
    }
}

#[test]
fn setup_errors_and_unknown_keys() {
    let m = parse_setup("# comment\n\nnew_key_of_a_later_version = 3\nch2.on = true\n").unwrap();
    assert!(m.ch[1].on);
    assert!(parse_setup("tscale = 9999").unwrap_err().contains("line 1: tscale"));
    assert!(parse_setup("ch1.coupling = XY").is_err());
    assert!(parse_setup("ch9.on = true").is_err());
    assert!(parse_setup("pat = H L").is_err());
    assert!(parse_setup("delay").is_err());
}

// Break it catches: a setup of a version with a choice of draw paths (`draw_path`, before Batch was the only one,
// ADR 0059) that no longer loads, or whose key is written again
#[test]
fn an_old_draw_path_key_is_passed_over() {
    for v in ["standard", "batch", "bogus"] {
        let m = parse_setup(&format!("ch2.on = true\ndraw_path = {v}\n")).unwrap();
        assert!(m.ch[1].on);
        assert_eq!(parse_setup_lenient(&format!("draw_path = {v}\n")).1, Vec::<String>::new());
        assert!(!setup_text(&m).contains("draw_path"));
    }
}

/// AUX OUT: written by name, missing key = trigger out (the FPGA's power-up state).
#[test]
fn setup_aux_out() {
    assert_eq!(parse_setup("ch2.on = true").unwrap().aux_out, AuxOut::TrigOut);
    assert!(setup_text(&Model::default()).contains("aux_out = TrigOut\n"));
    assert_eq!(parse_setup("aux_out = Off").unwrap().aux_out, AuxOut::Off);
    assert!(parse_setup("aux_out = Blink").is_err());
    assert_eq!(parse_setup("aux_out = PassFail").unwrap().aux_out, AuxOut::PassFail);
}

/// The Quick key action: written by name, read back, unknown names rejected, missing key = Screenshot.
#[test]
fn setup_quick_key() {
    for &q in QUICKS.iter() {
        let m = Model { quick: q, ..Default::default() };
        let text = setup_text(&m);
        assert!(text.contains(&format!("quick = {:?}\n", q)));
        assert_eq!(parse_setup(&text).unwrap().quick, q);
    }
    assert_eq!(parse_setup("quick = Autoset\nquick_v2 = Whatever\n").unwrap().quick, Quick::Autoset);
    assert_eq!(parse_setup("ch1.on = true\n").unwrap().quick, Quick::Screenshot);
    assert!(parse_setup("quick = Coffee").unwrap_err().contains("line 1: quick"));
}

/// Every protocol is stored by name; the USB decoder with its D- source and speed.
#[test]
fn setup_protocols_by_name() {
    for &p in crate::protocols::PROTOS.iter() {
        let m = Model { dec_proto: p, ..Default::default() };
        let text = setup_text(&m);
        assert!(text.contains(&format!("dec_proto = {}\n", p.key())));
        assert_eq!(parse_setup(&text).unwrap().dec_proto, p);
    }
    // the protocols that became modules keep the names of setups saved before
    for (old, name) in [("Dmx512", "DMX512"), ("Spdif", "S/PDIF"), ("UsbPd", "USB PD")] {
        assert_eq!(parse_setup(&format!("dec_proto = {}\n", old)).unwrap().dec_proto.name(), name);
    }
    // a module's options go with it
    let m = Model { dec_proto: Proto::from_key("Example").unwrap(), dec_opt: [2, 1, 0, 0], ..Default::default() };
    let text = setup_text(&m);
    assert!(text.contains("dec_proto = Example\n") && text.contains("dec_opt = 2 1 0 0\n"));
    assert_eq!(parse_setup(&text).unwrap(), m);
    assert!(parse_setup("dec_proto = NoSuchBus").is_err());
    let mut m = Model::default();
    (m.dec_proto, m.dec_src, m.dec_cs, m.dec_uspeed) = (Proto::from_key("Usb").unwrap(), 2, Some(3), 1);
    let text = setup_text(&m);
    assert!(text.contains("dec_proto = Usb\n") && text.contains("dec_uspeed = 1\n"));
    assert_eq!(parse_setup(&text).unwrap(), m);
    assert!(parse_setup("dec_uspeed = 3").is_err());
}

#[test]
fn csv_of_a_record() {
    let mut m = Model::default();
    m.ch[2].on = true;
    let mut rec = Record {
        t0: -1e-3,
        dt: 1e-6,
        ch: Default::default(),
        triggered: true,
        partial: false,
        raw: None,
        la: None,
        pairs: false,
    };
    rec.ch[0] = vec![0.5, 1.25, f32::NAN];
    rec.ch[2] = vec![-1.0, 0.0, 3.3];
    let csv = wave_csv(&rec, &m);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "Time (s),CH1 (V),CH3 (V)");
    assert_eq!(lines[1], "-1.000000000e-3,0.5,-1");
    assert_eq!(lines[3], "-9.980000000e-4,,3.3");
    assert_eq!(lines.len(), 4);
    let back = parse_wave(csv.as_bytes(), "wave.csv").unwrap();
    assert_eq!(back.len(), 2);
    assert!((back[0].dt - 1e-6).abs() < 1e-18);
    assert_eq!(back[0].v[0], 0.5);
    assert_eq!(back[1].v[2], 3.3);
}

/// A line wider than the reference slots ends the parse: before, one line of a million columns made a million
/// traces (40 MB of text took 1.9 GB) before the slots refused them.
#[test]
fn csv_wider_than_the_reference_slots_is_refused_while_parsing() {
    let wide = format!("0{}\n1,0\n", ",0".repeat(NREF + 1));
    assert!(parse_wave(wide.as_bytes(), "wide.csv").unwrap_err().contains("more traces"));
    let full = format!("0{}\n1{}\n", ",1".repeat(NREF), ",2".repeat(NREF));
    assert_eq!(parse_wave(full.as_bytes(), "full.csv").unwrap().len(), NREF);
    // ragged lines: a column missing on a line, or starting late, is NaN there
    let back = parse_wave(b"Time,A\n0,1\n1,2,7\n2,,8\n3\n", "r.csv").unwrap();
    assert_eq!(back.len(), 2);
    assert_eq!((back[0].t0, back[0].dt, back[0].label.as_str()), (0.0, 1.0, "CH1"));
    assert_eq!(&back[0].v[..2], [1.0, 2.0]);
    assert!(back[0].v[2].is_nan() && back[0].v.len() == 3);
    assert!(back[1].v[0].is_nan());
    assert_eq!(&back[1].v[1..], [7.0, 8.0]);
}

#[test]
fn oos_bin_and_wfm_round_trip() {
    let mut m = Model::default();
    m.ch[0].on = true;
    let rec = Record {
        t0: -2e-6,
        dt: 5e-9,
        ch: {
            let mut c: [Vec<f32>; NCH] = Default::default();
            c[0] = vec![0.1, 0.2, -0.3, 0.4];
            c
        },
        triggered: true,
        partial: false,
        raw: None,
        la: None,
        pairs: false,
    };
    let bin = wave_bin(&rec, &m);
    let t = &parse_wave(&bin, "wave.bin").unwrap()[0];
    assert_eq!(t.v, rec.ch[0]);
    assert!((t.dt - rec.dt).abs() < 1e-18);
    assert!((t.t0 - rec.t0).abs() < 1e-18);

    let v = [0.0_f32, 0.5, 1.0, -0.25];
    let wfm = wfm_f32(b":WFM#003", -1e-6, 1e-9, &v, "lane");
    let g = parse_wave(&wfm, "x.wfm").unwrap();
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].label, "lane");
    assert!((g[0].dt - 1e-9).abs() < 1e-18);
    assert!((g[0].t0 + 1e-6).abs() < 1e-18);
    for (a, b) in g[0].v.iter().zip(v) {
        assert!((a - b).abs() < 1e-6, "{a} {b}");
    }
    let old = wfm_f32(b":WFM#001", -1e-6, 1e-9, &v, "lane");
    assert_eq!(parse_wave(&old, "old.wfm").unwrap()[0].v, v);
    let two = wfm_f32(b":WFM#002", 0.0, 2e-9, &v, "two");
    assert_eq!(parse_wave(&two, "two.wfm").unwrap()[0].label, "two");
}

/// A short Tek WFM curve of f32 samples, little endian.
pub(super) fn wfm_f32(magic: &[u8; 8], t0: f64, dt: f64, v: &[f32], label: &str) -> Vec<u8> {
    let (explicit, implicit, specification, curve) =
        if magic == b":WFM#001" { (166, 478, 790, 820) } else { (168, 488, 808, 838) };
    let mut b = vec![0u8; curve + v.len() * 4 + 8];
    b[0..2].copy_from_slice(&0x0F0Fu16.to_le_bytes());
    b[2..10].copy_from_slice(magic);
    b[15] = 4;
    b[16..20].copy_from_slice(&(curve as i32).to_le_bytes());
    let lab = label.as_bytes();
    b[40..40 + lab.len()].copy_from_slice(lab);
    let put_f64 = |b: &mut [u8], o: usize, x: f64| b[o..o + 8].copy_from_slice(&x.to_le_bytes());
    let put_u32 = |b: &mut [u8], o: usize, x: u32| b[o..o + 4].copy_from_slice(&x.to_le_bytes());
    put_f64(&mut b, explicit, 1.0);
    put_f64(&mut b, explicit + 8, 0.0);
    put_u32(&mut b, explicit + 72, 4);
    put_f64(&mut b, implicit, dt);
    put_f64(&mut b, implicit + 8, t0);
    put_u32(&mut b, specification + 14, 0);
    put_u32(&mut b, specification + 18, (v.len() * 4) as u32);
    for (k, &x) in v.iter().enumerate() {
        b[curve + k * 4..curve + k * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    b
}

#[test]
fn all_tek_wfm_versions_keep_voltage_calibration_time_origin_and_byte_order() {
    for magic in [b":WFM#001", b":WFM#002", b":WFM#003"] {
        let (explicit, implicit, specification, curve) =
            if magic == b":WFM#001" { (166, 478, 790, 820) } else { (168, 488, 808, 838) };
        for big_endian in [false, true] {
            let codes = [-2.0, 0.0, 2.0, 4.0];
            let mut bytes = wfm_f32(magic, -9.5e-7, 20e-12, &codes, "captured");
            bytes[explicit..explicit + 8].copy_from_slice(&0.125f64.to_le_bytes());
            bytes[explicit + 8..explicit + 16].copy_from_slice(&0.75f64.to_le_bytes());
            if big_endian {
                bytes[0..2].copy_from_slice(&[0xf0, 0xf0]);
                for offset in [16, explicit + 72, specification + 14, specification + 18] {
                    bytes[offset..offset + 4].reverse();
                }
                for offset in [explicit, explicit + 8, implicit, implicit + 8] {
                    bytes[offset..offset + 8].reverse();
                }
                for offset in (curve..curve + codes.len() * 4).step_by(4) {
                    bytes[offset..offset + 4].reverse();
                }
            }
            let parsed = parse_wave(&bytes, "capture.wfm").unwrap();
            assert_eq!(parsed[0].v, [0.5, 0.75, 1.0, 1.25]);
            assert_eq!(parsed[0].label, "captured");
            assert_eq!(parsed[0].dt, 20e-12);
            assert_eq!(parsed[0].t0, -9.5e-7);
        }
    }
}

#[test]
fn tek_wfm_rejects_truncated_misaligned_or_invalid_scaling_before_decoding() {
    let original = wfm_f32(b":WFM#001", 0.0, 20e-12, &[-1.0, 1.0], "capture");
    let mut truncated = original.clone();
    truncated.remove(truncated.len() - 9);
    assert!(parse_wave(&truncated, "capture.wfm").unwrap_err().contains("truncated"));
    let mut wrong_size = original.clone();
    wrong_size[15] = 2;
    assert!(parse_wave(&wrong_size, "capture.wfm").unwrap_err().contains("bytes per point"));
    let mut negative = original.clone();
    negative[16..20].copy_from_slice(&(-1i32).to_le_bytes());
    assert!(parse_wave(&negative, "capture.wfm").unwrap_err().contains("negative"));
    let mut bad_time = original.clone();
    bad_time[478..486].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(parse_wave(&bad_time, "capture.wfm").unwrap_err().contains("finite"));
    let mut bad_voltage = original;
    bad_voltage[166..174].copy_from_slice(&f64::INFINITY.to_le_bytes());
    assert!(parse_wave(&bad_voltage, "capture.wfm").unwrap_err().contains("finite"));
}

#[test]
fn wave_and_fast_frame_round_trip() {
    let mut m = Model::default();
    m.ch[0].on = true;
    m.ch[2].on = true;
    let mut rec = Record {
        t0: -1e-3,
        dt: 1e-6,
        ch: Default::default(),
        triggered: true,
        partial: false,
        raw: None,
        la: None,
        pairs: false,
    };
    rec.ch[0] = vec![0.5, 1.25];
    rec.ch[2] = vec![-1.0, 0.0];
    let b = wave_bin(&rec, &m);
    let (back, n) = parse_wave_bin(&b).unwrap();
    assert_eq!(n, b.len());
    assert_eq!(back.dt, rec.dt);
    assert_eq!(back.ch[0], rec.ch[0]);
    assert_eq!(back.ch[2], rec.ch[2]);
    let ff = ff_bin(&[rec.clone(), rec.clone()], &m);
    let recs = parse_ff(&ff).unwrap();
    assert_eq!(recs.len(), 2);
    assert_eq!(recs[1].ch[0], rec.ch[0]);
    assert_eq!(parse_ff(&b).unwrap().len(), 1);
    let t = mask_text(2, 0.2, 0.4, true, &[1.0, 2.5]);
    let mf = parse_mask(&t).unwrap();
    assert_eq!((mf.src, mf.inv, mf.v), (Some(2), Some(true), vec![1.0, 2.5]));
    let old = parse_mask("1.0\n2.0\n").unwrap();
    assert_eq!(old.v, vec![1.0, 2.0]);
    assert!(old.src.is_none());
}

/// Counts in a frame file come from the file. One that does not fit the bytes is an error: before, a sample count
/// near 2^62 overflowed the offset (a panic, and with panic=abort the end of oos-ui) and a frame count of 2^32 - 1
/// asked for a list of that many records.
#[test]
fn frame_file_counts_beyond_the_bytes_are_refused() {
    let mut huge = b"OOSW".to_vec();
    for x in [1u32, 1] {
        huge.extend_from_slice(&x.to_le_bytes());
    }
    huge.extend_from_slice(&((1u64 << 62) + 1).to_le_bytes());
    huge.extend_from_slice(&1e-9f64.to_le_bytes());
    huge.extend_from_slice(&0f64.to_le_bytes());
    huge.extend_from_slice(&1u32.to_le_bytes());
    huge.extend_from_slice(&[0u8; 16]);
    assert!(parse_wave_bin(&huge).err().unwrap().contains("limit"));
    let over = (MAX_WAVEFORM_SAMPLES as u64 + 1).to_le_bytes();
    huge[12..20].copy_from_slice(&over);
    assert!(parse_ff(&huge).err().unwrap().contains("limit"));
    huge[12..20].copy_from_slice(&5u64.to_le_bytes());
    assert!(parse_wave_bin(&huge).err().unwrap().contains("truncated"));
    huge[12..20].copy_from_slice(&4u64.to_le_bytes());
    assert_eq!(parse_wave_bin(&huge).unwrap().1, huge.len());

    let mut ff = b"OOSF".to_vec();
    ff.extend_from_slice(&1u32.to_le_bytes());
    ff.extend_from_slice(&u32::MAX.to_le_bytes());
    ff.extend_from_slice(&huge);
    assert!(parse_ff(&ff).err().unwrap().contains("truncated"));
    ff[8..12].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(parse_ff(&ff).unwrap().len(), 1);
}

/// A record read from a file has the time base the import of other files asks for: before, a step of NaN or infinity
/// and any start became a `Record` whose times indexing and measurements then used.
#[test]
fn waveform_file_time_base_is_finite() {
    let mut m = Model::default();
    m.ch[1].on = true;
    let mut rec = Record {
        t0: -2e-3,
        dt: 4e-9,
        ch: Default::default(),
        triggered: true,
        partial: false,
        raw: None,
        la: None,
        pairs: false,
    };
    rec.ch[1] = vec![0.25, -0.5, 1.0];
    let good = wave_bin(&rec, &m);
    let (back, used) = parse_wave_bin(&good).unwrap();
    assert_eq!((back.t0, back.dt, &back.ch[1], used), (rec.t0, rec.dt, &rec.ch[1], good.len()));
    assert_eq!(parse_wave(&good, "wave.bin").unwrap()[0].t0, rec.t0);
    let bad = |at: usize, x: f64| {
        let mut b = good.clone();
        b[at..at + 8].copy_from_slice(&x.to_le_bytes());
        b
    };
    for (at, x) in
        [(20, f64::NAN), (20, f64::INFINITY), (20, 0.0), (20, -1e-9), (28, f64::NAN), (28, f64::NEG_INFINITY)]
    {
        let b = bad(at, x);
        assert_eq!(parse_wave_bin(&b).err().as_deref(), Some("waveform header"), "{at} {x}");
        assert!(parse_ff(&ff_from(&b)).is_err(), "{at} {x}");
        assert_eq!(parse_wave(&b, "wave.bin").unwrap_err(), "that file holds no waveform", "{at} {x}");
    }
    let mut one = good.clone();
    one[12..20].copy_from_slice(&1u64.to_le_bytes());
    assert_eq!(parse_wave_bin(&one).err().as_deref(), Some("waveform header"));
}

/// A fast-frame file of one OOSW blob.
fn ff_from(blob: &[u8]) -> Vec<u8> {
    let mut b = b"OOSF".to_vec();
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(blob);
    b
}

#[test]
fn checksums() {
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    // the eight-byte steps against the plain bitwise definitions, all lengths around the step sizes
    let data: Vec<u8> = (0..20000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    for n in (0..70).chain([5551, 5552, 5553, 11104, 20000]) {
        let d = &data[..n];
        let crc = !d.iter().fold(!0u32, |mut c, &x| {
            c ^= x as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            c
        });
        assert_eq!(crc32(d), crc, "CRC of {} bytes", n);
        let (a, b) =
            d.iter().fold((1u32, 0u32), |(a, b), &x| ((a + x as u32) % 65521, (b + (a + x as u32) % 65521) % 65521));
        assert_eq!(adler32(d), b << 16 | a, "Adler of {} bytes", n);
    }
    assert_eq!(adler32(&[0xff; 100_000]), {
        let (a, b) = (0..100_000).fold((1u64, 0u64), |(a, b), _| ((a + 255) % 65521, (b + (a + 255) % 65521) % 65521));
        (b << 16 | a) as u32
    });
    assert_eq!(crc32(b"IEND"), 0xae42_6082);
    assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    assert_eq!(adler32(&[]), 1);
}

/// Reads a deflate stream bit by bit (RFC 1951 3.1.1): values LSB first, Huffman codes MSB first.
struct Bits<'a> {
    d: &'a [u8],
    p: usize,
}
impl Bits<'_> {
    fn bit(&mut self) -> u32 {
        let b = (self.d[self.p >> 3] >> (self.p & 7)) & 1;
        self.p += 1;
        b as u32
    }
    fn bits(&mut self, n: u32) -> u32 {
        (0..n).fold(0, |v, i| v | self.bit() << i)
    }
}

/// One symbol of the fixed Huffman code (RFC 1951 3.2.6): 7 bits 256..279, 8 bits 0..143 and 280..287, 9 bits 144..255.
fn fixed_symbol(b: &mut Bits) -> u32 {
    let mut c = (0..7).fold(0, |c, _| c << 1 | b.bit());
    if c <= 0b001_0111 {
        return 256 + c;
    }
    c = c << 1 | b.bit();
    if (0b0011_0000..=0b1011_1111).contains(&c) {
        return c - 0b0011_0000;
    }
    if (0b1100_0000..=0b1100_0111).contains(&c) {
        return 280 + c - 0b1100_0000;
    }
    c = c << 1 | b.bit();
    144 + c - 0b1_1001_0000
}

/// Independent inflate for what the encoder may write (stored and fixed Huffman blocks, not dynamic ones); returns the
/// data and the number of bytes the stream took.
fn inflate(z: &[u8]) -> (Vec<u8>, usize) {
    const LBASE: [u32; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227,
        258,
    ];
    const LEXTRA: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
    const DBASE: [u32; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097,
        6145, 8193, 12289, 16385, 24577,
    ];
    const DEXTRA: [u32; 30] =
        [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
    let mut b = Bits { d: z, p: 0 };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = b.bit();
        match b.bits(2) {
            0 => {
                b.p = (b.p + 7) & !7;
                let (n, nn) = (b.bits(16), b.bits(16));
                assert_eq!(n, !nn & 0xffff, "stored block length check");
                for _ in 0..n {
                    out.push(b.bits(8) as u8);
                }
            }
            1 => loop {
                let s = fixed_symbol(&mut b);
                if s < 256 {
                    out.push(s as u8);
                } else if s == 256 {
                    break;
                } else {
                    assert!(s <= 285, "length symbol {}", s);
                    let i = (s - 257) as usize;
                    let len = LBASE[i] + b.bits(LEXTRA[i]);
                    let dc = (0..5).fold(0, |c, _| c << 1 | b.bit()) as usize;
                    assert!(dc < 30, "distance code {}", dc);
                    let dist = (DBASE[dc] + b.bits(DEXTRA[dc])) as usize;
                    assert!(
                        dist <= out.len(),
                        "distance {} reaches before the start ({} bytes so far)",
                        dist,
                        out.len()
                    );
                    for _ in 0..len {
                        out.push(out[out.len() - dist]);
                    }
                }
            },
            t => panic!("block type {} is not written by the encoder", t),
        }
        if last == 1 {
            break;
        }
    }
    (out, b.p.div_ceil(8))
}

#[test]
fn inflate_reference_streams() {
    // Python zlib, Z_FIXED, raw deflate of b"hello hello hello hello": "hello " as literals, then one match (distance 6)
    let z = [0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x27, 0x01];
    let (out, used) = inflate(&z);
    assert_eq!(out, b"hello hello hello hello");
    assert_eq!(used, z.len());
    // a stored block
    let (out, used) = inflate(&[1, 3, 0, 0xfc, 0xff, b'a', b'b', b'c']);
    assert_eq!((out.as_slice(), used), (&b"abc"[..], 8));
}

/// Parses the PNG structure: signature, chunk lengths and CRCs, IHDR, the zlib stream (inflated, Adler32) and the
/// pixels.
fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
    assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    let mut i = 8;
    let (mut w, mut h, mut z) = (0, 0, Vec::new());
    let mut kinds = Vec::new();
    while i < png.len() {
        let n = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
        let kind = &png[i + 4..i + 8];
        let data = &png[i + 8..i + 8 + n];
        let crc = u32::from_be_bytes(png[i + 8 + n..i + 12 + n].try_into().unwrap());
        assert_eq!(crc, crc32(&png[i + 4..i + 8 + n]), "CRC of {:?}", std::str::from_utf8(kind));
        match kind {
            b"IHDR" => {
                w = u32::from_be_bytes(data[0..4].try_into().unwrap());
                h = u32::from_be_bytes(data[4..8].try_into().unwrap());
                assert_eq!(&data[8..], &[8, 2, 0, 0, 0]);
            }
            b"IDAT" => z.extend_from_slice(data),
            _ => {}
        }
        kinds.push(String::from_utf8_lossy(kind).into_owned());
        i += 12 + n;
    }
    assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);
    assert_eq!(i, png.len());
    // zlib header, deflate data, Adler32
    assert_eq!(((z[0] as u32) << 8 | z[1] as u32) % 31, 0);
    assert_eq!(z[0] & 0x0f, 8);
    assert_eq!(z[1] & 0x20, 0, "no preset dictionary");
    let (raw, used) = inflate(&z[2..]);
    let p = 2 + used;
    assert_eq!(u32::from_be_bytes(z[p..p + 4].try_into().unwrap()), adler32(&raw));
    assert_eq!(p + 4, z.len());
    // rows: filter 0 + RGB
    let mut pix = Vec::new();
    for row in raw.chunks(w as usize * 3 + 1) {
        assert_eq!(row[0], 0);
        pix.extend_from_slice(&row[1..]);
    }
    assert_eq!(pix.len(), (w * h * 3) as usize);
    (w, h, pix)
}

#[test]
fn ppm_power_on_picture() {
    let mut b = b"P6\n# OOS\n2 1\n255\n".to_vec();
    b.extend_from_slice(&[255, 0, 0, 0, 0, 255]);
    let mut cv = Canvas::new(10, 4);
    assert_eq!(ppm_into(&b, &mut cv), Some((2, 1)));
    assert_eq!(
        (cv.px[(10 + 4) as usize], cv.px[(10 + 5) as usize]),
        (crate::gfx::rgb(255, 0, 0), crate::gfx::rgb(0, 0, 255))
    );
    assert!(ppm_into(b"P5\n1 1\n255\n\0", &mut cv).is_none());
}

#[test]
fn png_structure_and_pixels() {
    // larger than one stored block (65535 bytes)
    let (w, h) = (301u32, 97u32);
    let px: Vec<u32> = (0..w * h).map(|i| i.wrapping_mul(2654435761u32) & 0xff_ffff).collect();
    let data = png(w, h, &px);
    let (dw, dh, pix) = decode(&data);
    assert_eq!((dw, dh), (w, h));
    for (k, &p) in px.iter().enumerate() {
        assert_eq!(&pix[k * 3..k * 3 + 3], &[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
}

/// A picture like the scope screen: black, a grid, filled boxes, a trace, some speckle.
fn screen_like(w: u32, h: u32) -> Vec<u32> {
    let mut px = vec![0u32; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut c = 0;
            if x % 100 == 0 || y % 75 == 0 {
                c = rgb(60, 60, 60);
            }
            if (40..200).contains(&x) && (30..70).contains(&y) {
                c = rgb(20, 90, 160);
            }
            let trace = (h as f32 / 2.0 + (x as f32 / 20.0).sin() * h as f32 / 4.0) as u32;
            if y == trace || y == trace + 1 {
                c = rgb(255, 220, 0);
            }
            if (x * 7 + y * 13) % 997 == 0 {
                c = rgb(200, 200, 200);
            }
            px[(y * w + x) as usize] = c;
        }
    }
    px
}

fn assert_decodes_to(data: &[u8], w: u32, h: u32, px: &[u32]) {
    let (dw, dh, pix) = decode(data);
    assert_eq!((dw, dh), (w, h));
    for (k, &p) in px.iter().enumerate() {
        assert_eq!(&pix[k * 3..k * 3 + 3], &[(p >> 16) as u8, (p >> 8) as u8, p as u8], "pixel {}", k);
    }
}

#[test]
fn png_of_a_screen_round_trips() {
    // runs longer than the 258 a match can take, rows equal to the row above, matches across the end of a row
    for (w, h) in [(1024u32, 600u32), (301, 97), (1, 1), (1, 300), (2, 3), (700, 1)] {
        let px = screen_like(w, h);
        assert_decodes_to(&png(w, h, &px), w, h, &px);
    }
    let flat = vec![rgb(1, 2, 3); 50 * 40];
    assert_decodes_to(&png(50, 40, &flat), 50, 40, &flat);
}

#[test]
fn png_is_much_smaller_than_the_bitmap() {
    let (w, h) = (1024u32, 600u32);
    let raw = (w * 3 + 1) * h;
    let black = png(w, h, &vec![0u32; (w * h) as usize]);
    assert!(black.len() < raw as usize / 100, "black screen: {} bytes", black.len());
    let screen = png(w, h, &screen_like(w, h));
    assert!(screen.len() < raw as usize / 20, "screen-like picture: {} bytes of {}", screen.len(), raw);
}

/// Cross-check with an independent decoder when the host has Python with Pillow (skipped otherwise).
#[test]
fn png_opens_in_pillow() {
    let has_pillow =
        std::process::Command::new("python3").args(["-c", "import PIL"]).output().is_ok_and(|o| o.status.success());
    if !has_pillow {
        return eprintln!("python3 with Pillow not found: check skipped");
    }
    let (w, h) = (64u32, 40u32);
    let px: Vec<u32> = (0..w * h).map(|i| rgb((i % w) * 4, (i / w) * 6, 0x80)).collect();
    let dir = crate::ui::testkit::Scratch::new("ui-png");
    let path = dir.join("t.png");
    write(&path, &png(w, h, &px)).unwrap();
    // a full screen with long runs and repeated rows, compared byte for byte with what Pillow decodes
    let (sw, sh) = (1024u32, 600u32);
    let screen = screen_like(sw, sh);
    let big = dir.join("big.png");
    write(&big, &png(sw, sh, &screen)).unwrap();
    let rgb_bytes: Vec<u8> = screen.iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
    write(&dir.join("big.rgb"), &rgb_bytes).unwrap();
    let script = "import sys\nfrom PIL import Image\nim = Image.open(sys.argv[1]); im.load()\nprint(im.size[0], im.size[1], im.mode, *im.getpixel((5, 7)))\nbig = Image.open(sys.argv[2]); big.load()\nprint(big.tobytes() == open(sys.argv[3], 'rb').read())";
    let out = std::process::Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(&path)
        .arg(&big)
        .arg(dir.join("big.rgb"))
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "Pillow: {}", String::from_utf8_lossy(&out.stderr));
    let s = String::from_utf8_lossy(&out.stdout);
    assert_eq!(s.trim(), format!("64 40 RGB {} {} 128\nTrue", 5 * 4, 7 * 6));
}

/// A real screen (the demo, four channels and the menus of the UI) decodes back to its pixels.
#[test]
fn png_of_the_demo_screen_round_trips() {
    let mut ui = crate::ui::testkit::demo_ui();
    for c in ["key ch2", "key ch3", "key ch4", "key measure"] {
        crate::cmd::run(&mut ui, c);
    }
    let mut cv = Canvas::new(crate::ui::PANEL_W, crate::ui::PANEL_H);
    for _ in 0..3 {
        ui.step(0.04);
        ui.render(&mut cv);
    }
    assert_decodes_to(&png_of(&cv), cv.w as u32, cv.h as u32, &cv.px);
}

/// Time of one PNG of a real screen (the demo source, 4 channels on, as the web remote sends it) and of the
/// screen-like test picture: `cargo test --release png_encode_time -- --ignored --nocapture`.
#[test]
#[ignore]
fn png_encode_time() {
    let mut ui = crate::ui::testkit::demo_ui();
    for c in ["key ch2", "key ch3", "key ch4"] {
        crate::cmd::run(&mut ui, c);
    }
    let mut cv = Canvas::new(crate::ui::PANEL_W, crate::ui::PANEL_H);
    for _ in 0..10 {
        ui.step(0.04);
        ui.render(&mut cv);
    }
    let synthetic = screen_like(cv.w as u32, cv.h as u32);
    for (name, px) in [("demo screen", &cv.px), ("screen-like", &synthetic)] {
        let n = 40;
        let mut size = 0;
        let t = std::time::Instant::now();
        for _ in 0..n {
            size = std::hint::black_box(png(cv.w as u32, cv.h as u32, px)).len();
        }
        let per = t.elapsed().as_secs_f64() / n as f64;
        println!("{}: {} x {}, {} bytes, {:.2} ms per PNG", name, cv.w, cv.h, size, per * 1e3);
        assert_decodes_to(&png(cv.w as u32, cv.h as u32, px), cv.w as u32, cv.h as u32, px);
    }
    // what the frame loop does per picture of the web remote now (http.rs `Shots`): one copy of the screen; and
    // what the encoder thread does before it encodes: compare it with the last copy
    let n = 200;
    let mut copy: Vec<u32> = Vec::with_capacity(cv.px.len());
    let t = std::time::Instant::now();
    for _ in 0..n {
        copy.clear();
        copy.extend_from_slice(std::hint::black_box(&cv.px));
    }
    let per_copy = t.elapsed().as_secs_f64() / n as f64;
    let t = std::time::Instant::now();
    let mut same = false;
    for _ in 0..n {
        same = crate::shots::same_pixels(std::hint::black_box(&copy), std::hint::black_box(&cv.px));
    }
    let per_cmp = t.elapsed().as_secs_f64() / n as f64;
    assert!(same);
    println!(
        "frame loop: copy of the screen {:.3} ms; encoder thread: comparison {:.3} ms",
        per_copy * 1e3,
        per_cmp * 1e3
    );
}

#[test]
fn next_free_name() {
    let dir = crate::ui::testkit::Scratch::new("ui-names");
    assert_eq!(file_name(&next_name(&dir, "shot", "png")), "shot-001.png");
    write(&dir.join("shot-001.png"), b"x").unwrap();
    write(&dir.join("shot-002.png"), b"x").unwrap();
    assert_eq!(file_name(&next_name(&dir, "shot", "png")), "shot-003.png");
    let _ = std::fs::remove_dir_all(&dir);
}
