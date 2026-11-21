// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Tests of the screen that are not about the acquisition or the menu dialog: measurements in the menus, decoders and
//! the bus trigger, math, mask, histogram, analysers and views, Bode, storage, the trigger level line, SCPI of the
//! acquisition and the axis labels. A test of acquisition goes into `acquisition_tests.rs` (of STOP and SINGLE into
//! `stop_tests.rs`), one of the menus and keys into `menu_tests.rs`.

use super::axis_label;

/// Measure > Add item: a second tap on a measurement removes it again, the others stay.
#[test]
fn add_item_toggles_a_measurement() {
    use super::*;

    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.meas.clear();
    ui.meas_src = WaveId::Ch(0);
    let (a, b) = (ALL_MEAS[0], ALL_MEAS[1]);
    ui.activate(Item::MeasAdd(a));
    ui.activate(Item::MeasAdd(b));
    assert_eq!(ui.m.meas, vec![(a, WaveId::Ch(0)), (b, WaveId::Ch(0))]);
    ui.activate(Item::MeasAdd(a));
    assert_eq!(ui.m.meas, vec![(b, WaveId::Ch(0))]);
    // the same measurement of another channel is its own entry
    ui.meas_src = WaveId::Ch(1);
    ui.activate(Item::MeasAdd(b));
    assert_eq!(ui.m.meas, vec![(b, WaveId::Ch(0)), (b, WaveId::Ch(1))]);
    ui.meas_src = WaveId::Ch(0);
    ui.activate(Item::MeasAdd(b));
    assert_eq!(ui.m.meas, vec![(b, WaveId::Ch(1))]);
}

/// CAN and LIN decode the demo bus traffic: IDs, data and CRC / checksum.
#[test]
fn can_and_lin_decode_the_demo_bus() {
    use super::*;
    use oos_dsp::decode::Kind;
    for (proto, tdiv, want_id) in [(Proto::CAN, 100e-6f32, 0x123u32), (Proto::LIN, 1e-3, 0x21)] {
        // (I2S and 1553 have their own test below)
        let mut ui = crate::ui::testkit::demo_ui();
        ui.m.dec_on = true;
        ui.set_dec_proto(proto);
        ui.m.tscale = tscales().iter().position(|&t| (t - tdiv).abs() < tdiv * 1e-3).unwrap();
        for _ in 0..20 {
            ui.step(1.0 / 30.0);
        }
        let p = ui.decode_packets().unwrap();
        let ids: Vec<u32> = p.iter().filter(|x| matches!(x.kind, Kind::Id { .. })).map(|x| x.value).collect();
        assert!(ids.contains(&want_id), "{:?}: ids {:x?}", proto, ids);
        assert!(p.iter().filter(|x| x.kind == Kind::Crc).all(|x| x.ok), "{:?}: {:?}", proto, p);
        assert!(p.iter().any(|x| x.kind == Kind::Crc), "{:?}: no CRC/checksum", proto);
        // picture for the eye (target/test-shots)
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        ui.render(&mut cv);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
        let _ = std::fs::create_dir_all(&dir);
        let _ = crate::store::write(&dir.join(format!("i-{:?}.png", proto).to_lowercase()), &crate::store::png_of(&cv));
    }
}

// Break it catches: the Bus trigger's own switches staying after going back to Edge (decoder on: the live picture
// stayed raw; its demo source channels stayed on), or switching off what the user had on before Bus
#[test]
fn leaving_the_bus_trigger_undoes_only_what_it_switched_on() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    for c in 0..NCH {
        ui.m.ch[c].on = c == 0;
    }
    ui.m.dec_on = false;
    ui.m.dec_proto = Proto::I2C;
    ui.set_trig_type(TrigType::Bus);
    assert!(ui.m.dec_on && ui.m.ch[2].on && ui.m.ch[3].on, "Bus: the decoder and its sources (I2C on CH3/CH4)");
    // Bus again: nothing new recorded, still undone once
    ui.set_trig_type(TrigType::Bus);
    ui.set_trig_type(TrigType::Edge);
    assert!(!ui.m.dec_on, "the decoder Bus switched on is off again");
    let on: Vec<bool> = (0..NCH).map(|c| ui.m.ch[c].on).collect();
    assert_eq!(on, vec![true, false, false, false], "only the user's CH1 stays");
    // what the user had on before stays on: the decoder and CH3
    ui.m.dec_on = true;
    ui.m.ch[2].on = true;
    ui.set_trig_type(TrigType::Bus);
    ui.set_trig_type(TrigType::Edge);
    assert!(ui.m.dec_on && ui.m.ch[2].on && !ui.m.ch[3].on, "user's decoder and CH3 kept, CH4 off again");
    // another type in between that is not Bus: nothing to undo twice
    ui.set_trig_type(TrigType::Slope);
    assert!(ui.m.dec_on && ui.m.ch[2].on);
}

/// Protocol trigger: the end of the matching CAN identifier sits at the trigger point; "Error" never fires on clean
/// traffic in Normal mode.
#[test]
fn bus_trigger_aligns_on_the_matching_frame() {
    use super::*;
    use oos_dsp::decode::Kind;
    for id in [0x123u32, 0x18da_f110] {
        let mut ui = crate::ui::testkit::demo_ui();
        ui.m.dec_proto = Proto::CAN;
        ui.set_trig_type(TrigType::Bus);
        ui.m.trig_bus_val = id as u64;
        ui.m.tscale = tscales().iter().position(|&t| (t - 50e-6).abs() < 1e-9).unwrap();
        for _ in 0..10 {
            ui.step(1.0 / 30.0);
        }
        assert!(ui.last[0].as_ref().unwrap().triggered, "id {:x} not triggered", id);
        let p = ui.decode_packets().unwrap();
        let hit =
            p.iter().find(|x| matches!(x.kind, Kind::Id { .. }) && x.value == id).expect("matching ID in the record");
        // trigger point = end of the matching ID field = t 0 (within one screen point of 500 us / 3600)
        assert!(hit.t1.abs() < 0.5e-6, "id {:x}: field ends at {} s", id, hit.t1);
        if id == 0x123 {
            ui.open_menu(MenuId::Trigger);
            let mut cv = Canvas::new(PANEL_W, PANEL_H);
            ui.render(&mut cv);
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
            let _ = std::fs::create_dir_all(&dir);
            let _ = crate::store::write(&dir.join("j-bus-trigger.png"), &crate::store::png_of(&cv));
        }
    }
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.dec_proto = Proto::CAN;
    ui.set_trig_type(TrigType::Bus);
    ui.m.trig_bus = BusWhat::Error;
    ui.m.trig_mode = TrigMode::Normal;
    ui.m.tscale = tscales().iter().position(|&t| (t - 50e-6).abs() < 1e-9).unwrap();
    for _ in 0..10 {
        ui.step(1.0 / 30.0);
    }
    assert!(ui.last[0].is_none(), "Error trigger fired on clean traffic");
}

/// Protocol trigger on ARINC 429 (label), SENT (data nibbles) and FlexRay (frame ID) with the demo traffic: the end
/// of the matching field sits at the trigger point, Frame triggers, Error never fires on the clean traffic.
#[test]
fn bus_trigger_on_arinc_sent_and_flexray() {
    use super::*;
    use oos_dsp::decode::Kind;
    let k = |s: &str| Proto::from_key(s).unwrap();
    let (arinc, sent) = (k("Arinc429"), k("Sent"));
    for (proto, tdiv) in [(arinc, 100e-6f32), (sent, 200e-6), (k("FlexRay"), 5e-6)] {
        let tsc = tscales().iter().position(|&t| (t - tdiv).abs() < tdiv * 1e-3).unwrap();
        let tol = 3.0 * tdiv as f64 * HDIV as f64 / 3600.0; // three record samples
        for what in [BusWhat::Id, BusWhat::Data, BusWhat::Frame, BusWhat::Error] {
            let mut ui = crate::ui::testkit::demo_ui();
            ui.set_dec_proto(proto);
            ui.set_trig_type(TrigType::Bus);
            let id_default = ui.m.trig_bus_val;
            ui.m.trig_bus = what;
            // a payload value of the demo traffic
            if what == BusWhat::Data {
                ui.m.trig_bus_val = if proto == arinc {
                    0x2_5a5a
                } else if proto == sent {
                    0x7f_480b
                } else {
                    0x55
                };
            } else if what == BusWhat::Id && proto == sent {
                ui.m.trig_bus_val = 0x4; // status nibble of the second frame
            }
            ui.m.trig_mode = TrigMode::Normal;
            ui.m.tscale = tsc;
            for _ in 0..6 {
                ui.step(1.0 / 30.0);
            }
            if what == BusWhat::Error {
                assert!(ui.last[0].is_none(), "{:?}: Error trigger fired on clean traffic", proto);
                continue;
            }
            assert!(ui.last[0].as_ref().is_some_and(|r| r.triggered), "{:?} {:?}: not triggered", proto, what);
            let p = ui.decode_packets().unwrap();
            assert!(
                crate::trig::points(&ui.m, &p).iter().any(|t| t.abs() < tol),
                "{:?} {:?}: no matching field ends at 0: {:?}",
                proto,
                what,
                p
            );
            if what == BusWhat::Id && proto != sent {
                let want = if proto == arinc { 0o310 } else { 0x2a };
                assert_eq!(id_default, want);
                assert!(p
                    .iter()
                    .any(|x| matches!(x.kind, Kind::Id { .. }) && x.value as u64 == want && x.t1.abs() < tol));
            }
            if what == BusWhat::Id {
                let mut cv = Canvas::new(PANEL_W, PANEL_H);
                ui.render(&mut cv);
                let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
                let _ = std::fs::create_dir_all(&dir);
                let _ = crate::store::write(
                    &dir.join(format!("j-bus-{}.png", proto.key()).to_lowercase()),
                    &crate::store::png_of(&cv),
                );
            }
        }
    }
}

/// Protocol trigger on USB with the demo traffic (full speed, D+ / D- on CH3 / CH4): the token address, a PID,
/// a payload byte and Frame (end of SYNC) at the trigger point, Error never fires; D+ alone decodes the same.
#[test]
fn bus_trigger_on_usb() {
    use super::*;
    use oos_dsp::decode::Kind;
    let tdiv = 5e-6f32;
    let tsc = tscales().iter().position(|&t| (t - tdiv).abs() < tdiv * 1e-3).unwrap();
    let tol = 3.0 * tdiv as f64 * HDIV as f64 / 3600.0; // three record samples
    for (what, val) in [
        (BusWhat::Id, None),
        (BusWhat::Id, Some(0x4B)),
        (BusWhat::Data, Some(0x40)),
        (BusWhat::Frame, None),
        (BusWhat::Error, None),
    ] {
        let mut ui = crate::ui::testkit::demo_ui();
        ui.m.dec_proto = Proto::from_key("Usb").unwrap();
        ui.set_trig_type(TrigType::Bus);
        assert_eq!((ui.m.dec_src, ui.m.dec_cs, ui.m.trig_bus, ui.m.trig_bus_val), (2, Some(3), BusWhat::Id, 5));
        assert!(ui.decode_items().contains(&Item::DecUsbSpeed) && ui.decode_items().contains(&Item::DecCs));
        assert_eq!(ui.item_label(Item::DecCs), "D-");
        ui.m.trig_bus = what;
        if let Some(v) = val {
            ui.m.trig_bus_val = v;
        }
        ui.m.trig_mode = TrigMode::Normal;
        ui.m.tscale = tsc;
        for _ in 0..6 {
            ui.step(1.0 / 30.0);
        }
        if what == BusWhat::Error {
            assert!(ui.last[0].is_none(), "USB: Error trigger fired on clean traffic");
            continue;
        }
        assert!(ui.last[0].as_ref().is_some_and(|r| r.triggered), "USB {:?} {:?}: not triggered", what, val);
        let p = ui.decode_packets().unwrap();
        assert!(
            crate::trig::points(&ui.m, &p).iter().any(|t| t.abs() < tol),
            "USB {:?}: no matching field ends at 0: {:?}",
            what,
            p
        );
        match (what, val) {
            (BusWhat::Id, None) => {
                assert!(p.iter().any(|x| x.kind == Kind::Field("ADDR") && x.value == 5 && x.t1.abs() < tol))
            }
            (BusWhat::Id, Some(v)) => {
                assert!(p.iter().any(|x| x.kind == Kind::Field("PID") && x.value as u64 == v && x.t1.abs() < tol))
            }
            (BusWhat::Frame, _) => assert!(p.iter().any(|x| x.kind == Kind::Field("SYNC") && x.t1.abs() < tol)),
            _ => {}
        }
        assert!(p.iter().all(|x| x.ok), "{:?}", p);
        // D+ alone: the same packets (without the EOP of a packet cut by the record start), the speed settings
        let both: Vec<_> = p.iter().map(|x| (x.kind, x.value)).collect();
        ui.m.dec_cs = None;
        let one: Vec<_> = ui.decode_packets().unwrap().iter().map(|x| (x.kind, x.value)).collect();
        assert!(both.ends_with(&one) && one.len() + 1 >= both.len(), "{:?}\n{:?}", one, both);
        ui.m.dec_uspeed = 2;
        assert_eq!(ui.decode_packets().unwrap().len(), one.len());
        ui.m.dec_uspeed = 1;
        assert!(ui.decode_packets().unwrap().iter().all(|x| x.kind != Kind::Field("PID")));
        if what == BusWhat::Id && val.is_none() {
            ui.m.dec_cs = Some(3);
            ui.m.dec_uspeed = 0;
            let mut cv = Canvas::new(PANEL_W, PANEL_H);
            ui.render(&mut cv);
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
            let _ = std::fs::create_dir_all(&dir);
            let _ = crate::store::write(&dir.join("j-bus-usb.png"), &crate::store::png_of(&cv));
        }
    }
}

/// The protocol trigger value is entered on the hex keypad.
#[test]
fn math_on_math() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.ch[1].on = true;
    ui.step(0.02);
    // M1 = CH1 + CH2, M2 = Abs(M1); M1 offers only the channels, M2 also M1
    ui.m.math = MathOp::Add;
    assert_eq!(ui.options(Item::MathA).len(), NCH);
    ui.m.math_select(1);
    assert_eq!(ui.options(Item::MathA).last().map(|s| s.as_str()), Some("M1"));
    ui.m.math = MathOp::Abs;
    ui.m.math_a = NCH; // M1
    let rec = ui.last[0].clone().unwrap();
    let m1 = ui.math_wave_of(0);
    let m2 = ui.math_wave_of(1);
    assert_eq!(m1.len(), rec.ch[0].len());
    for i in (0..m1.len()).step_by(97) {
        assert_eq!(m1[i], rec.ch[0][i] + rec.ch[1][i]);
        assert_eq!(m2[i], m1[i].abs());
    }
    // switching back keeps both channels
    ui.m.math_select(0);
    assert_eq!((ui.m.math, ui.m.math_ch(1).op, ui.m.math_ch(1).a), (MathOp::Add, MathOp::Abs, NCH));
}

#[test]
fn zone_drawn_by_touch_filters_records() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    ui.relayout();
    ui.open_menu(MenuId::Zone);
    ui.activate(Item::ZoneBox);
    assert_eq!(ui.zone_trig.arm, Some((0, false)));
    // a box in the top left corner of the plot (far above the demo signal)
    let a = ui.plot.area;
    let (x0, y0, x1, y1) = (a.x + 60, a.y + 20, a.x + 160, a.y + 60);
    ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x: x0, y: y0 });
    ui.handle(Ev::Touch { id: 0, phase: Phase::Move, x: x1, y: y1 });
    ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x: x1, y: y1 });
    let z = ui.m.zones[0].clone();
    assert!(z.on && z.inside && z.pts.len() == 2, "{:?}", z);
    assert!(z.pts[0].1 > z.pts[1].1, "top edge has the higher voltage");
    // "must intersect" an empty region: every record is dropped
    ui.last[0] = None;
    for _ in 0..5 {
        ui.step(0.02);
    }
    assert!(ui.zone_trig.drops > 0 && ui.last[0].is_none());
    // "must not intersect": records pass again
    ui.m.zones[0].inside = false;
    ui.step(0.02);
    assert!(ui.last[0].is_some());
}

#[test]
fn extended_decoders_decode_their_demo() {
    use super::*;
    // (protocol, time/div that shows a whole frame, a label or kind that must appear)
    // (the modules DMX512, S/PDIF and USB PD run through the same path as every protocol)
    let m = |k: &str| Proto::from_key(k).unwrap();
    // every optional module with demo traffic has its case here (`protocols::tests::every_decoder_is_complete`)
    let cases: [(Proto, f32, &str); 16] = [
        (m("Dcc"), 10e-3, "PRE"),
        (m("CanFd"), 50e-6, "BRS"),
        (m("Dmx512"), 100e-6, "SC"),
        (m("ModbusRtu"), 1e-3, "FUNC"),
        (m("Uart9"), 1e-3, "Addr"),
        (m("OneWire"), 200e-6, "ROM"),
        (m("UsbPd"), 100e-6, "HDR"),
        (m("Swd"), 20e-6, "REQ"),
        (m("Spdif"), 20e-6, "Sample"),
        (m("Example"), 1e-3, "Data"),
        (m("FlexRay"), 5e-6, "HCRC"),
        (m("Arinc429"), 100e-6, "SSM"),
        (m("Sent"), 200e-6, "STAT"),
        (m("Usb"), 5e-6, "ENDP"),
        (m("Mil1553"), 20e-6, "cmd: true"),
        (m("I2s"), 20e-6, "right: true"),
    ];
    for (p, tdiv, want) in cases {
        let mut ui = crate::ui::testkit::demo_ui();
        ui.set_dec_proto(p);
        ui.m.dec_on = true;
        ui.m.tscale = tscales().iter().position(|&x| (x - tdiv).abs() < tdiv * 1e-3).unwrap();
        ui.m.trig_type = TrigType::Edge;
        ui.m.trig_mode = TrigMode::Auto;
        let mut found = String::new();
        for _ in 0..6 {
            ui.step(0.05);
            let pk = ui.decode_packets().unwrap_or_default();
            found = format!("{:?}", pk);
            if found.contains(want) {
                break;
            }
        }
        assert!(found.contains(want), "{:?}: no {} in {}", p, want, &found[..found.len().min(400)]);
    }
}

#[test]
fn segmented_recording_stops_after_n_records() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.segments = 10;
    for _ in 0..20 {
        ui.step(0.02);
    }
    assert_eq!((ui.m.run, ui.seg.n, ui.history.recs.len()), (RunState::Stop, 10, 10));
    // RUN again: a new set
    ui.key(key::RUNSTOP);
    ui.step(0.02);
    assert!(ui.seg.n <= 6 && ui.history.recs.len() == ui.seg.n);
}

#[test]
fn analysers_on_the_demo_signal() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.tscale = tscales().iter().position(|&x| (x - 1e-3).abs() < 1e-6).unwrap();
    ui.step(0.05);
    // spectrum: the strongest line of the 1 kHz square is its fundamental
    ui.open_menu(MenuId::Spectrum);
    ui.adjust(Item::SpecOn, 1);
    assert_eq!(ui.m.view, AView::Spectrum);
    ui.step(0.05);
    let p = spectrum_peak(&ui).expect("a peak");
    // within a display column (the spectrum has one point per plot column)
    assert!((p - 1e3).abs() < 40.0, "peak at {} Hz", p);
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    ui.render(&mut cv);
    // eye: the recovered unit interval is half a period (0.5 ms)
    ui.adjust(Item::EyeOn, 1);
    for _ in 0..3 {
        ui.step(0.05);
    }
    let (_, _, period) = ui.azr.eye_res.clone().expect("eye result");
    assert!((period - 0.5e-3).abs() < 0.02e-3, "UI {}", period);
    ui.render(&mut cv);
    // power and identify do not panic; identify names CH1 and applies an edge trigger on it
    ui.adjust(Item::PowOn, 1);
    ui.step(0.05);
    assert!(!ui.azr.power.is_empty());
    ui.render(&mut cv);
    ui.activate(Item::IdentifyAct);
    assert_eq!(ui.menu, MenuId::Identify);
    let k = ui.azr.findings.iter().position(|f| f.channels.contains(&0)).expect("a finding on CH1");
    ui.m.trig_src = 3;
    ui.activate(Item::IdFind(k));
    assert_eq!(ui.m.trig_src, 0);
}

/// Identify finding with a baud rate of NaN: the nearest rate of the decoder was chosen with
/// `partial_cmp(..).unwrap()`, which panicked; every distance is NaN now and the first rate is taken.
#[test]
fn identify_finding_of_nan_baud_applies() {
    use super::*;
    use oos_dsp::identify::{DecoderHint, Finding, SignalKind, TriggerHint};

    let mut ui = crate::ui::testkit::demo_ui();
    let uart = |baud| DecoderHint::Uart {
        ch: 0,
        baud,
        bits: 8,
        parity: oos_dsp::decode::Parity::None,
        thr: 1.0,
        invert: false,
    };
    let finding = |baud| Finding {
        channels: vec![0],
        kind: SignalKind::Noise { rms: 0.0 },
        confidence: 1.0,
        summary: "UART".into(),
        trigger: TriggerHint::None,
        decoder: Some(uart(baud)),
    };
    ui.azr.findings = vec![finding(f64::NAN), finding(9700.0)];
    ui.activate(Item::IdFind(0));
    assert_eq!((ui.m.dec_proto, ui.m.dec_baud), (Proto::UART, 0));
    ui.activate(Item::IdFind(1));
    assert_eq!(crate::decode::BAUDS[ui.m.dec_baud], 9600.0);
}

/// Screenshots of the new views for a visual check: `OOS_SHOTS=<dir> cargo test shots -- --ignored`.
#[test]
#[ignore]
fn shots_of_new_views() {
    use super::*;
    let dir = std::env::var("OOS_SHOTS").unwrap_or_else(|_| ".".into());
    let shot = |ui: &mut Ui, name: &str| {
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        ui.render(&mut cv);
        std::fs::write(format!("{}/{}.png", dir, name), crate::store::png_of(&cv)).unwrap();
    };
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.tscale = tscales().iter().position(|&x| (x - 1e-3).abs() < 1e-6).unwrap();
    ui.m.ch[1].on = true;
    ui.step(0.05);
    ui.m.spec_wf = true;
    ui.m.spec_dist = true;
    ui.adjust(Item::SpecOn, 1);
    for _ in 0..30 {
        ui.step(0.05);
    }
    shot(&mut ui, "spectrum");
    ui.adjust(Item::EyeOn, 1);
    for _ in 0..10 {
        ui.step(0.05);
    }
    shot(&mut ui, "eye");
    ui.adjust(Item::PowOn, 1);
    ui.step(0.05);
    shot(&mut ui, "power");
    ui.m.view = AView::Scope;
    ui.m.math = MathOp::Add;
    ui.m.math_select(1);
    ui.m.math = MathOp::Bpf;
    ui.m.math_a = NCH;
    ui.m.hist = Hist::Vertical;
    ui.m.zones[0] = crate::zone::Zone { on: true, src: 0, inside: true, pts: vec![(-2e-3, 2.0), (-1.5e-3, 3.0)] };
    for _ in 0..10 {
        ui.step(0.05);
    }
    shot(&mut ui, "scope_math_zone_hist");
    ui.activate(Item::IdentifyAct);
    shot(&mut ui, "identify");
    ui.close_menu();
    ui.set_dec_proto(Proto::from_key("CanFd").unwrap());
    ui.m.dec_on = true;
    ui.m.math = MathOp::Off;
    ui.m.math_select(0);
    ui.m.math = MathOp::Off;
    ui.m.zones[0].on = false;
    ui.m.hist = Hist::Off;
    ui.m.tscale = tscales().iter().position(|&x| (x - 20e-6).abs() < 1e-9).unwrap();
    for _ in 0..5 {
        ui.step(0.05);
    }
    shot(&mut ui, "canfd");
    ui.open_menu(MenuId::Main);
    shot(&mut ui, "main_menu");
}

/// System > Graphics = GPU draws the layers from commands: replayed, they give exactly the pixels of drawing
/// directly, on every kind of screen.
#[test]
fn recorded_layers_replay_to_the_same_pixels() {
    use super::*;
    let same = |ui: &mut Ui, what: &str| {
        let (w, h) = (ui.scr.w, ui.scr.h);
        let (mut u, mut o) = (Canvas::new(w, h), Canvas::new_layer(w, h));
        ui.render_layers(&mut Layers { under: &mut u, over: Some(&mut o), jobs: Vec::new() });
        let (mut ru, mut ro) = (Canvas::new(w, h), Canvas::new_layer(w, h));
        ru.record(true);
        ro.record(true);
        ui.render_layers(&mut Layers { under: &mut ru, over: Some(&mut ro), jobs: Vec::new() });
        let (cu, co) = (ru.rec.take().unwrap(), ro.rec.take().unwrap());
        assert!(cu.cmds.len() > 10, "{}: nothing recorded", what);
        let (mut pu, mut po) = (Canvas::new(w, h), Canvas::new_layer(w, h));
        cu.replay(&mut pu);
        co.replay(&mut po);
        for (name, a, b) in [("under", &u, &pu), ("over", &o, &po)] {
            let bad = a.px.iter().zip(&b.px).position(|(p, q)| p != q);
            assert!(
                bad.is_none(),
                "{} {}: pixel {:?} differs: {:08x} drawn, {:08x} replayed",
                what,
                name,
                bad.map(|i| (i as i32 % w, i as i32 / w)),
                bad.map_or(0, |i| a.px[i]),
                bad.map_or(0, |i| b.px[i])
            );
        }
    };
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.ch[1].on = true;
    for _ in 0..5 {
        ui.step(0.05);
    }
    same(&mut ui, "scope");
    ui.open_menu(MenuId::Main);
    same(&mut ui, "main menu");
    ui.close_menu();
    ui.open_menu(MenuId::Measure);
    same(&mut ui, "measure menu");
    ui.close_menu();
    ui.key(key::MEASURE);
    same(&mut ui, "measure page");
    ui.close_menu();
    ui.m.zoom = true;
    ui.m.color_grade = true;
    ui.step(0.05);
    same(&mut ui, "zoom, colour grade");
    ui.m.zoom = false;
    ui.m.color_grade = false;
    ui.adjust(Item::SpecOn, 1);
    for _ in 0..10 {
        ui.step(0.05);
    }
    same(&mut ui, "spectrum");
    ui.adjust(Item::EyeOn, 1);
    for _ in 0..10 {
        ui.step(0.05);
    }
    same(&mut ui, "eye");
    ui.m.view = AView::Scope;
    ui.set_dec_proto(Proto::from_key("CanFd").unwrap());
    ui.m.dec_on = true;
    for _ in 0..5 {
        ui.step(0.05);
    }
    same(&mut ui, "decode");
}

fn spectrum_peak(ui: &super::Ui) -> Option<f64> {
    ui.strongest_line().map(|m| m.f)
}

#[test]
fn protocol_watch_names_a_new_bus() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    ui.set_dec_proto(Proto::UART);
    // demo UART on CH3 (115200 Bd): watch on at 100 us/div (a screen record resolves the bits), a few seconds
    ui.m.ch[2].on = true;
    ui.m.tscale = tscales().iter().position(|&x| (x - 100e-6).abs() < 1e-9).unwrap();
    ui.adjust(Item::ProtoWatch, 1);
    let mut named = String::new();
    for _ in 0..80 {
        ui.step(0.05);
        if let Some((t, _)) = &ui.toast {
            if t.starts_with("Protocol watch") {
                named = t.clone();
                break;
            }
        }
    }
    let kinds: Vec<String> =
        ui.azr.findings.iter().map(|f| format!("{:?} {:?} {:.2}", f.channels, f.kind, f.confidence)).collect();
    assert!(named.contains("CH3"), "toast: {:?}, seen {:?}, findings {:?}", named, ui.azr.watch_seen, kinds);
    assert!(ui.identify_items().len() > 1);
}

#[test]
fn bode_sweep_drives_the_generator() {
    use super::*;
    use std::io::Read;
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let got = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let g2 = got.clone();
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = l.accept() {
            let mut b = [0u8; 4096];
            while let Ok(n) = s.read(&mut b) {
                if n == 0 {
                    break;
                }
                g2.lock().unwrap().push_str(&String::from_utf8_lossy(&b[..n]));
            }
        }
    });
    let mut ui = crate::ui::testkit::demo_ui();
    (ui.m.bode_addr, ui.m.bode_start, ui.m.bode_stop, ui.m.bode_points, ui.m.bode_gen) = (addr, 1e3, 1e5, 5, 1);
    let ts0 = ui.m.tscale;
    ui.activate(Item::BodeRun);
    assert!(ui.bode.running);
    for _ in 0..200 {
        ui.step(0.05);
        if !ui.bode.running {
            break;
        }
    }
    assert!(!ui.bode.running, "sweep did not finish ({} of {})", ui.bode.idx, ui.bode.freqs.len());
    assert_eq!(ui.bode.points.len(), 5);
    assert_eq!(ui.m.tscale, ts0, "time base restored");
    std::thread::sleep(std::time::Duration::from_millis(200));
    let cmds = got.lock().unwrap().clone();
    assert!(
        cmds.contains(":SOUR1:APPL:SIN 1000,1,0")
            && cmds.contains(":OUTP1 ON")
            && cmds.contains(":SOUR1:APPL:SIN 100000"),
        "{}",
        cmds
    );
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    ui.render(&mut cv);
}

#[test]
fn bode_sweep_with_the_builtin_generator() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    (ui.m.bode_gen, ui.m.bode_start, ui.m.bode_stop, ui.m.bode_points, ui.m.bode_in, ui.m.bode_out) =
        (crate::model::bode::BUILTIN, 1e3, 1e5, 3, WaveId::Ch(0), WaveId::Ch(0));
    (ui.m.afg_on, ui.m.afg_freq) = (false, 1234.0);
    ui.activate(Item::BodeRun);
    assert!(ui.m.afg_on && ui.m.afg_freq == 1e3 && ui.m.afg_offset == 0.0);
    let mut seen = vec![];
    for _ in 0..200 {
        if !ui.bode.running {
            break;
        }
        seen.push(ui.m.afg_freq);
        ui.step(0.05);
    }
    assert!(!ui.bode.running);
    assert!(seen.iter().any(|f| (f / 1e4 - 1.0).abs() < 1e-9) && seen.contains(&1e5), "{:?}", seen);
    assert_eq!((ui.m.afg_on, ui.m.afg_freq), (false, 1234.0), "generator restored");
    // one channel on the generator: gain only
    assert!(ui.bode.points.iter().all(|p| p.2.is_nan() && p.1.is_finite()), "{:?}", ui.bode.points);
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    ui.render(&mut cv);
}

#[test]
fn web_csv_refuses_a_record_it_would_have_to_cut() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    assert!(ui.web_wave_csv().is_err());
    for _ in 0..30 {
        ui.step(0.02);
    }
    assert!(ui.web_wave_csv().unwrap().starts_with("Time (s)"));
    ui.last[0].as_mut().unwrap().ch[0] = vec![0.0; WEB_CSV_MAX + 1];
    assert!(ui.web_wave_csv().unwrap_err().contains("too many"));
}

#[test]
fn keypad_enters_a_long_can_id() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.dec_proto = Proto::CAN;
    ui.set_trig_type(TrigType::Bus);
    ui.open_menu(MenuId::Trigger);
    ui.open_picker(Item::TrigBusVal);
    assert_eq!(ui.keypad, "123");
    let key = |ui: &mut Ui, k: &str| {
        let i = KEYPAD.iter().position(|x| *x == k).unwrap();
        ui.activate(Item::PickOpt(i));
    };
    key(&mut ui, "Clear");
    for d in ["1", "8", "D", "A", "F", "1", "1", "0"] {
        key(&mut ui, d);
    }
    // up to ten digits (40 bits): the eleventh is ignored
    for d in ["9", "9", "9"] {
        key(&mut ui, d);
    }
    key(&mut ui, "Del");
    key(&mut ui, "0");
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    ui.render(&mut cv);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
    let _ = std::fs::create_dir_all(&dir);
    let _ = crate::store::write(&dir.join("j-keypad.png"), &crate::store::png_of(&cv));
    key(&mut ui, "OK");
    assert_eq!(ui.m.trig_bus_val, 0x18_daf1_1090);
    assert_eq!(ui.menu, MenuId::Trigger);
}

/// I2S and MIL-STD-1553 decode their demo signals.
#[test]
fn i2s_and_1553_decode_the_demo() {
    use super::*;
    use oos_dsp::decode::Kind;
    for (key, tdiv) in [("I2s", 10e-6f32), ("Mil1553", 20e-6)] {
        let proto = Proto::from_key(key).unwrap();
        let mut ui = crate::ui::testkit::demo_ui();
        ui.m.dec_on = true;
        ui.set_dec_proto(proto);
        // the trigger that holds the demo still: I2S the falling WS edge on CH1, 1553 the first rising edge after
        // 3 us of quiet bus on CH3
        let m = &ui.m;
        let want = if key == "I2s" {
            (TrigType::Edge, 0, Slope::Fall, 1.65)
        } else {
            (TrigType::NthEdge, 2, Slope::Rise, 2.0)
        };
        assert_eq!((m.trig_type, m.trig_src, m.slope, m.trig_level), want, "{}", key);
        assert!(key == "I2s" || (m.nth_idle, m.nth_n) == (3e-6, 1));
        ui.m.tscale = tscales().iter().position(|&t| (t - tdiv).abs() < tdiv * 1e-3).unwrap();
        for _ in 0..20 {
            ui.step(1.0 / 30.0);
        }
        let p = ui.decode_packets().unwrap();
        if key == "I2s" {
            let s: Vec<(bool, u32)> = p
                .iter()
                .filter_map(|x| if let Kind::Sample { right } = x.kind { Some((right, x.value)) } else { None })
                .collect();
            assert!(s.contains(&(false, 0x4000)) && s.contains(&(true, 0xc000)), "{:x?}", s);
        } else {
            let w: Vec<(bool, u32, bool)> = p
                .iter()
                .filter_map(|x| if let Kind::Word { cmd } = x.kind { Some((cmd, x.value, x.ok)) } else { None })
                .collect();
            assert!(w.contains(&(true, 0x0c23, true)) && w.contains(&(false, 0xbeef, true)), "{:x?}", w);
        }
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        ui.render(&mut cv);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
        let _ = std::fs::create_dir_all(&dir);
        let _ = crate::store::write(&dir.join(format!("i-{}.png", key).to_lowercase()), &crate::store::png_of(&cv));
    }
}

/// Power key: "cancel power-down" to the MCU on every press, dialog toggles, buttons set the action.
#[test]
fn power_key_dialog() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    ui.key(key::POWER);
    assert!(ui.power.open);
    assert_eq!(ui.panel_tx, vec![crate::board::PanelCmd::CancelPowerDown]);
    // second press cancels, again with the cancel frame
    ui.key(key::POWER);
    assert!(!ui.power.open && ui.power.action.is_none());
    assert_eq!(ui.panel_tx.len(), 2);
    // tap on "Power off"
    ui.key(key::POWER);
    let b = ui.pwr_layout().1[0];
    ui.tap(b.x + b.w / 2, b.y + b.h / 2);
    assert_eq!(ui.power.action, Some(PowerAction::Off));
    assert!(!ui.power.open);
    // tap outside: nothing happens
    ui.power.action = None;
    ui.key(key::POWER);
    ui.tap(5, 5);
    assert!(!ui.power.open && ui.power.action.is_none());
    // any other key closes the dialog without acting
    ui.key(key::POWER);
    ui.key(key::RUNSTOP);
    assert!(!ui.power.open && ui.power.action.is_none());
    let b = ui.pwr_layout().1[1];
    ui.key(key::POWER);
    ui.tap(b.x + 5, b.y + 5);
    assert_eq!(ui.power.action, Some(PowerAction::Restart));
}

/// Every trigger type with its starting values triggers on the demo signals.
#[test]
fn every_trigger_type_triggers_on_the_demo() {
    use super::*;
    for &t in TRIG_TYPES.iter() {
        let mut ui = crate::ui::testkit::demo_ui();
        ui.set_trig_type(t);
        ui.m.trig_mode = TrigMode::Normal;
        let mut hits = 0;
        for _ in 0..20 {
            ui.step(0.033);
            if ui.m.status == TrigStatus::Triggered {
                hits += 1;
            }
        }
        assert!(hits >= 5, "{:?}: triggered in {} of 20 frames", t, hits);
    }
}

#[test]
fn heat_palette_runs_blue_to_red() {
    let p = super::heat_palette();
    let c = |x: u32| ((x >> 16) & 255, (x >> 8) & 255, x & 255);
    let (r0, g0, b0) = c(p[0]);
    let (r1, g1, b1) = c(p[128]);
    let (r2, g2, b2) = c(p[255]);
    assert!(b0 > r0 && b0 > g0, "rare = blue");
    assert!(g1 > r1 && g1 > b1, "middle = green");
    assert!(r2 > g2 && r2 > b2, "frequent = red");
}

/// Colour grade in the normal and the zoomed view: trace pixels take palette colours.
#[test]
fn colour_grade_renders() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.ch[1].on = true;
    ui.m.color_grade = true;
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    for zoom in [false, true] {
        ui.set_zoom(zoom);
        for _ in 0..5 {
            ui.step(0.033);
        }
        ui.render(&mut cv);
        let pal = heat_palette();
        let a = ui.plot.area;
        let ph = if zoom { &ui.phz } else { &ui.ph };
        let mut direct = Canvas::new(W, H);
        ui.draw_graded(&mut direct, a, ph, if zoom { ui.grade.vmaxz_cg } else { ui.grade.vmax_cg }, false);
        let mut active = 0;
        for y in 0..ph.h {
            for x in 0..ph.w {
                let expected = (0..NCH).any(|c| ui.m.ch[c].on && ui.shows(c) && ph.map[c][y * ph.w + x] != 0);
                let graded = pal.contains(&direct.px[((a.y + y as i32) * W + a.x + x as i32) as usize]);
                assert_eq!(graded, expected, "zoom {zoom}: map pixel {x},{y}");
                active += usize::from(expected);
            }
        }
        assert!(active > 0, "the fixture must deposit trace samples");
        let hits = (a.y..a.y + a.h)
            .flat_map(|y| (a.x..a.x + a.w).map(move |x| (x, y)))
            .filter(|&(x, y)| pal.contains(&cv.px[(y * W + x) as usize]))
            .count();
        // Axes and measurement labels cover a few trace pixels. The zoom
        // map can contain fewer than 1000 active pixels at this timebase.
        assert!(hits * 2 > active, "zoom {zoom}: {hits} of {active} graded pixels remain visible");
    }
}

/// Closing the display must not undo explicit overview/timebase changes.
#[test]
fn zoom_close_preserves_explicit_main_timebase_changes() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    ui.step(0.02);
    let (ts, tf, d) = (ui.m.tscale, ui.m.tfine, ui.m.delay);
    ui.set_zoom(true);
    ui.m.tscale = if ts > 3 { ts - 3 } else { ts + 3 };
    ui.m.tfine = 0.97;
    ui.m.delay = d + 2.5e-4;
    let changed = (ui.m.tscale, ui.m.tfine, ui.m.delay);
    let mut cv = Canvas::new(W, H);
    ui.render(&mut cv);
    let row = ui.zoom_readout();
    let c = ui.zoom_close();
    assert!(
        c.x >= row.x && c.x + c.w <= row.x + row.w && c.y >= row.y && c.y + c.h <= row.y + row.h,
        "close {:?} readout {:?}",
        c,
        row
    );
    let (o, z) = Ui::split_zoom(ui.plot.grid);
    assert!(c.y >= o.y + o.h && c.y + c.h <= z.y, "close {:?} between {:?} and {:?}", c, o, z);
    assert_ne!(cv.px[((c.y + c.h / 2) * W + c.x + c.w / 2) as usize], BG);
    ui.tap(c.x + c.w / 2, c.y + c.h / 2);
    assert!(!ui.m.zoom);
    assert_eq!((ui.m.tscale, ui.m.tfine, ui.m.delay), changed);
    let _ = tf;
}

/// Mask from CH1 (square): passes; from CH4 (PWM with sweeping duty) with a tight X tolerance: fails, and stops.
#[test]
fn mask_test_counts_and_stops() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    ui.step(0.033);
    ui.create_mask();
    assert!(ui.m.mask_on && ui.mask_sw.mask.is_some());
    for _ in 0..10 {
        ui.step(0.033);
    }
    assert!(ui.mask_sw.pass >= 30 && ui.mask_sw.fail == 0, "pass {} fail {}", ui.mask_sw.pass, ui.mask_sw.fail);
    ui.m.ch[3].on = true;
    ui.adjust(Item::MaskSrc, 3);
    assert!(ui.mask_sw.mask.is_none() && !ui.m.mask_on);
    ui.step(0.033);
    ui.m.mask_x = 0.04;
    ui.create_mask();
    ui.m.mask_stop = true;
    for _ in 0..100 {
        ui.step(0.033);
        if ui.m.run == RunState::Stop {
            break;
        }
    }
    assert_eq!(ui.m.run, RunState::Stop);
    assert_eq!(ui.mask_sw.fail, 1);
}

#[test]
fn math_category_filters_the_operation_list() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    ui.select_option(Item::MathFam, "Logic");
    let ops = ui.options(Item::MathOpI);
    assert!(ops.contains(&"AND".into()) && ops.contains(&"XOR".into()));
    assert!(!ops.iter().any(|s| s == "FFT" || s == "A+B"));
    ui.select_option(Item::MathOpI, "AND");
    assert_eq!(ui.m.math, MathOp::And);
    ui.select_option(Item::MathFam, "Convert");
    assert_eq!(ui.m.math, MathOp::A2d);
    ui.select_option(Item::MathFam, "Arithmetic");
    assert!(ui.options(Item::MathOpI).contains(&"Conv".into()));
}

#[test]
fn mask_from_ref_invert_save_and_load() {
    use super::*;
    let dir = crate::ui::testkit::Scratch::new("mask");
    let mut ui = crate::ui::testkit::demo_ui();
    ui.files.dir = dir.clone();
    ui.files.dest = crate::store::StoreDest::Local;
    ui.step(0.033);
    let rec = ui.last[0].as_ref().unwrap();
    ui.refs[0] = Some(RefWave { label: "R1".into(), ..RefWave::test(rec.ch[0].clone(), rec.dt, rec.t0) });
    ui.activate(Item::MaskFromRef);
    assert!(ui.m.mask_on && ui.mask_sw.mask.is_some());
    ui.adjust(Item::MaskInvert, 1);
    assert!(ui.m.mask_inv && ui.mask_sw.mask.as_ref().unwrap().invert);
    ui.activate(Item::MaskSave);
    assert!(dir.join("mask-001.txt").is_file());
    assert!(dir.join("mask1.txt").is_file());
    let t = std::fs::read_to_string(dir.join("mask-001.txt")).unwrap();
    assert!(t.starts_with("# OOS mask\n") && t.contains("inv=1"));
    ui.activate(Item::MaskClear);
    assert!(ui.mask_sw.mask.is_none() && !ui.m.mask_on);
    ui.m.mask_inv = false;
    ui.activate(Item::MaskLoad);
    assert!(ui.m.mask_on && ui.m.mask_inv && ui.mask_sw.reference.as_ref().unwrap().len() > 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fast_frame_save_and_load_and_local_dest() {
    use super::*;
    let dir = crate::ui::testkit::Scratch::new("ff");
    let mut ui = crate::ui::testkit::demo_ui();
    ui.files.dir = dir.clone();
    ui.files.dest = crate::store::StoreDest::Local;
    for _ in 0..4 {
        ui.step(0.033);
    }
    assert!(ui.history.recs.len() >= 3);
    ui.activate(Item::StoreFfSave);
    let p = dir.join("frame-001.bin");
    assert!(p.is_file());
    let n = ui.history.recs.len();
    ui.history.recs.clear();
    ui.last[0] = None;
    ui.activate(Item::StoreFfLoad);
    assert_eq!(ui.history.recs.len(), n);
    assert!(ui.history.pos.is_some());
    assert_eq!(ui.m.run, RunState::Stop);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Save setup, waveform CSV; recall restores the setup but not the run state.
#[test]
fn storage_save_and_recall() {
    use super::*;
    let dir = crate::ui::testkit::Scratch::new("ui-store");
    let mut ui = crate::ui::testkit::demo_ui();
    ui.files.dir = dir.join("sub");
    ui.m.ch[2].on = true;
    ui.m.tscale -= 2;
    ui.m.color_grade = true;
    ui.step(0.033);
    let saved = ui.m.clone();
    ui.activate(Item::StoreSaveSetup);
    ui.activate(Item::StoreCsv);
    let csv = std::fs::read_to_string(dir.join("sub/wave-001.csv")).unwrap();
    assert!(csv.starts_with("Time (s),CH1 (V),CH3 (V)\n"));
    assert_eq!(csv.lines().count(), 1 + ui.last[0].as_ref().unwrap().ch[0].len());
    ui.key(key::DEFAULT);
    ui.m.run = RunState::Stop;
    ui.activate(Item::StoreRecall);
    assert_eq!(ui.m.run, RunState::Stop);
    let mut want = saved;
    (want.run, want.status) = (ui.m.run, ui.m.status);
    assert_eq!(ui.m, want);
    ui.files.slot = 2;
    ui.activate(Item::StoreRecall);
    assert!(ui.toast.as_ref().unwrap().0.starts_with("Recall failed"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The trigger level line shows for a moment after the level changes by any way (not only while dragging it);
/// with AC or LF reject coupling there is no level mark (the level counts from the signal's mean).
#[test]
fn level_line_after_a_change_and_no_mark_with_ac_coupling() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    ui.step(0.05);
    assert!(!ui.level_line_on(), "no line at start-up");
    ui.adjust(Item::TrigLevel, 3);
    ui.step(0.05);
    assert!(ui.level_line_on());
    for _ in 0..40 {
        ui.step(0.05);
    }
    assert!(!ui.level_line_on(), "gone after {} s", LEVEL_LINE_S);
    // the mark: drawn in the accent colour at the right edge of the plot, at the level's height
    let mark = |ui: &mut Ui| {
        let mut cv = Canvas::new(ui.plot.area.w, ui.plot.area.h + 200);
        ui.render(&mut cv);
        let g = ui.plot.grid;
        let x = g.x + g.w - MARK_W + 16;
        (g.y..g.y + g.h).filter(|&y| cv.px[(y * cv.w + x) as usize] == ACCENT).count()
    };
    ui.m.trig_coup = TrigCoup::Dc;
    assert!(mark(&mut ui) > 3, "DC: the T mark is there");
    ui.m.trig_coup = TrigCoup::Ac;
    assert_eq!(mark(&mut ui), 0, "AC: no T mark");
    ui.m.trig_coup = TrigCoup::LfReject;
    assert_eq!(mark(&mut ui), 0, "LF reject: no T mark");
}

#[test]
fn dvm_lane_scpi() {
    use super::*;
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
    let mut q = |ui: &mut Ui, c: &str| crate::ui::testkit::q(&mut scpi, ui, &cv, c);
    assert_eq!(q(&mut ui, ":SYST:DEB:DVM:LANE?"), "AUTO");
    q(&mut ui, ":SYSTem:DEBug:DVM:LANE 3");
    assert_eq!((q(&mut ui, ":SYST:DEB:DVM:LANE?"), ui.dvm.lane), ("3".into(), Some(3)));
    q(&mut ui, ":SYST:DEB:DVM:LANE 4");
    assert_eq!(ui.dvm.lane, Some(3));
    assert!(q(&mut ui, ":SYST:ERR?").starts_with("-224"));
    q(&mut ui, ":SYST:DEB:DVM:LANE auto");
    assert_eq!(ui.dvm.lane, None);
}

#[test]
fn average_scpi() {
    use super::*;
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
    let mut q = |ui: &mut Ui, c: &str| crate::ui::testkit::q(&mut scpi, ui, &cv, c);
    assert_eq!(q(&mut ui, ":ACQ:TYPE?"), "NORM");
    q(&mut ui, ":ACQuire:TYPE AVERages");
    q(&mut ui, ":ACQ:AVER 256");
    q(&mut ui, ":ACQ:AVER:HARD ON");
    assert_eq!((ui.m.acq, ui.m.avg_count, ui.m.avg_hw), (AcqMode::Average, 256, true));
    assert_eq!(
        (q(&mut ui, ":ACQ:TYPE?"), q(&mut ui, ":ACQ:AVER?"), q(&mut ui, ":ACQ:AVER:HARD?")),
        ("AVER".into(), "256".into(), "1".into())
    );
    // a count the menu does not offer snaps down to the next one it does (PR #8, as the stock firmware); a bogus type
    // leaves the type alone
    q(&mut ui, ":ACQ:AVER 100");
    q(&mut ui, ":ACQ:TYPE BOGUS");
    assert_eq!((ui.m.acq, ui.m.avg_count), (AcqMode::Average, 64));
    q(&mut ui, ":ACQ:AVER:HARD OFF");
    q(&mut ui, ":ACQ:TYPE PEAK");
    assert_eq!((ui.m.acq, ui.m.avg_hw), (AcqMode::Peak, false));
}

#[test]
fn axis_labels() {
    // prefix from the span, decimals from step and base (offset / delay)
    assert_eq!(axis_label(-1e-6, 2e-7, 0.0, 1e-6, "s"), "-1.0 \u{b5}s");
    assert_eq!(axis_label(1.5, 0.5, 0.0, 2.0, "V"), "1.5 V");
    assert_eq!(axis_label(-0.6, 0.2, 0.0, 0.8, "V"), "-600 mV");
    assert_eq!(axis_label(3.25, 2.0, 4.75, 12.75, "V"), "3.25 V");
    assert_eq!(axis_label(-1e-9, 2.0, 0.0, 8.0, "V"), "0 V");
    // 1 us/div at a delay of 3 ms: every label its own (was "3 ms" each)
    assert_eq!(axis_label(3.001e-3, 1e-6, 3e-3, 3.005e-3, "s"), "3.001 ms");
    assert_eq!(axis_label(2.996e-3, 1e-6, 3e-3, 3.005e-3, "s"), "2.996 ms");
    assert_eq!(axis_label(0.2e-6, 0.1e-6, 0.0, 0.5e-6, "s"), "200 ns");
    // an offset of 1.481 V: the labels keep its three decimals
    assert_eq!(axis_label(1.519, 1.0, 1.481, 4.481, "V"), "1.519 V");
    assert_eq!(axis_label(1.545455, 1.0, 1.454545, 4.454545, "V"), "1.545 V");
    // 1 us/div at a delay of 1 s
    assert_eq!(axis_label(1.000002, 1e-6, 1.0, 1.000005, "s"), "1.000002 s");
}

// Break it catches (#279): level 50 % with AC trigger coupling set the middle of the signal in volts, while the trigger
// compares the source without its mean and counts its level from there: on a signal that does not cross 0 V the level
// lay above the whole swing and the trigger did not fire
#[test]
fn level_50_with_ac_trigger_coupling_counts_from_the_mean() {
    use super::*;
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.run = RunState::Stop;
    ui.m.trig_src = 0;
    ui.m.trig_type = TrigType::Edge;
    // 0 to 3.3 V, high a quarter of the time: middle 1.65 V, mean 0.825 V
    let dt = 1e-6;
    let v: Vec<f32> = (0..4000).map(|k| if k % 400 < 100 { 3.3 } else { 0.0 }).collect();
    let rec = crate::wave::Record {
        t0: -2e-3,
        dt,
        ch: [v.clone(), Vec::new(), Vec::new(), Vec::new()],
        triggered: true,
        partial: false,
        raw: None,
        la: None,
        pairs: false,
    };
    ui.last[0] = Some(rec);
    ui.stats[0] = crate::wave::stats(&v, dt);
    ui.m.trig_coup = TrigCoup::Dc;
    ui.activate(Item::Trig50);
    assert!((ui.m.trig_level - 1.65).abs() < 0.02, "DC: the middle, {}", ui.m.trig_level);
    for coup in [TrigCoup::Ac, TrigCoup::LfReject] {
        ui.m.trig_coup = coup;
        ui.activate(Item::Trig50);
        assert!((ui.m.trig_level - 0.825).abs() < 0.02, "{:?}: the middle less the mean, {}", coup, ui.m.trig_level);
    }
}
