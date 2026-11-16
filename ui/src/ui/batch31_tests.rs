//! Batch 31: the findings of the function check that were left. Eye PLL bandwidth (an allocation that never ended), the
//! last steppers without a limit, a limit that does not give the value back, fixed limits, defaults off the ladder,
//! the direction of the amplitude cursors, a spectrum from Auto, labels that did not follow, a cursor source that moved
//! the active channel. Each test starts from the state a fresh scope has (no warm-up presses).
use super::ranges::Step;
use super::*;

fn frame(u: &mut Ui) {
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    u.step(0.02);
}

fn touch(u: &mut Ui, phase: Phase, x: i32, y: i32) {
    u.handle(Ev::Touch { id: 0, phase, x, y });
    u.step(0.01);
}

fn tap_rect(u: &mut Ui, r: Rect) {
    let (x, y) = (r.x + r.w / 2, r.y + r.h / 2);
    touch(u, Phase::Down, x, y);
    touch(u, Phase::Up, x, y);
    frame(u);
}

fn fresh(layout: Layout) -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = layout;
    u.m.ch[1].on = true;
    u.refs[0] = Some(RefWave { src: 0, color: 0, label: "R1".into(), t0: 0.0, dt: 1e-9, v: vec![0.0f32; 128].into(), vpd: 0.5, offset: 0.0, on: true });
    u.ref_sel = 0;
    for _ in 0..4 { u.step(0.05); }
    u
}

fn row_of(u: &Ui, it: Item) -> Row {
    u.dlg_layout().expect("a dialog").rows.into_iter().find(|r| r.it == it).unwrap_or_else(|| panic!("{it:?} is a row"))
}

const LAYOUTS: [Layout; 3] = [Layout::Minimal, Layout::Default, Layout::Studio];

fn scpi_line(u: &mut Ui, c: &str) -> String {
    let mut scpi = crate::scpi::Scpi::open(0);
    let cv = Canvas::new(W, H);
    String::from_utf8(scpi.exec_line(u, &cv, c)).unwrap()
}

fn number(u: &Ui, it: Item) -> f64 {
    u.range_get(it).or_else(|| u.numpad_value(it).map(|v| v.0)).unwrap_or_else(|| panic!("{it:?} has a number"))
}

// ------------------------------------------------------------------------------------------------ 1 Eye PLL

/// A pseudo-random NRZ record: `bits` bits of `spb` samples.
fn nrz(bits: usize, spb: usize) -> Vec<f32> {
    let mut s = 0x1234_5678u32;
    let mut v = Vec::new();
    for _ in 0..bits {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        let level = if s & 0x100 != 0 { 0.5 } else { -0.5 };
        v.extend(std::iter::repeat(level).take(spb));
    }
    v
}

#[test]
fn a_pll_faster_than_its_edges_gives_a_bounded_clock_instead_of_running_out_of_memory() {
    let (spb, dt) = (8usize, 1.0 / (8.0 * 1e9));
    let v = nrz(2000, spb);
    let n = v.len();
    // up to far beyond the edge rate (1 GHz): the second order loop has a gain that grows with the square of the
    // bandwidth, the period went to zero or below, and the tick loop allocated until the process was killed
    for bw in [0.0, 1e3, 1e5, 1.5e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e12] {
        for second in [true, false] {
            let c = oos_dsp::eye::recover_clock_pll(&v, dt, 0.0, 0.05, Some(1e9), bw, second).unwrap_or_else(|| panic!("{bw} Hz: a clock"));
            assert!(c.ticks.len() <= 2 * (n / spb) + 8, "{bw} Hz second order {second}: {} ticks for {} bits", c.ticks.len(), n / spb);
            assert!(c.ticks.windows(2).all(|w| w[1] > w[0]), "{bw} Hz: ticks go forward");
            assert!(c.ticks.windows(2).all(|w| (w[1] - w[0]) < 2.0 * c.period), "{bw} Hz: no tick is skipped");
        }
    }
}

#[test]
fn the_pll_bandwidth_is_limited_by_the_range_table_in_the_menu_the_pad_scpi_and_the_setup_file() {
    for layout in LAYOUTS {
        let mut u = fresh(layout);
        let r = u.item_range(Item::EyePll).expect("a range");
        assert_eq!((r.lo, r.hi), (0.0, crate::model::EYE_PLL_MAX));
        for _ in 0..80 { u.adjust(Item::EyePll, 1); }
        assert_eq!(u.m.eye_pll_bw, r.hi, "{layout:?} stepped to the top");
        for _ in 0..80 { u.adjust(Item::EyePll, 4); }
        assert_eq!(u.m.eye_pll_bw, r.hi, "{layout:?} knob");
        assert_eq!(u.numpad_apply(Item::EyePll, 1e12), r.hi, "{layout:?} pad");
        scpi_line(&mut u, ":EYE:PLL:BWIDth 1e12");
        assert!(u.m.eye_pll_bw <= r.hi, "{layout:?} SCPI set {}", u.m.eye_pll_bw);
        scpi_line(&mut u, ":EYE:PLL:BWIDth 1e8");
        assert_eq!(u.m.eye_pll_bw, 1e8);
        // down from the lowest rung is Auto again
        for _ in 0..80 { u.adjust(Item::EyePll, -1); }
        assert_eq!(u.m.eye_pll_bw, 0.0, "{layout:?} back to Auto");
    }
    let mut m = Model::default();
    crate::store::set(&mut m, "eye_pll", "1e15").unwrap();
    assert_eq!(m.eye_pll_bw, crate::model::EYE_PLL_MAX);
}

#[test]
fn the_eye_runs_with_every_pll_bandwidth_without_hanging() {
    let mut u = fresh(Layout::Default);
    u.m.view = AView::Eye;
    for bw in [1e5, 2e5, 1e6, 1e8, 1e12] {
        u.m.eye_pll_bw = bw;
        for _ in 0..6 { frame(&mut u); }
    }
}

// ------------------------------------------------------------------------------------------------ 2 limits

#[test]
fn the_magnetic_turns_the_math_offset_and_the_reference_level_stop_at_a_limit() {
    for layout in LAYOUTS {
        let mut u = fresh(layout);
        let r = u.item_range(Item::MagN).expect("turns are in the table");
        assert_eq!((r.lo, r.hi, r.step), (1.0, 10_000.0, Step::Index));
        u.m.mag_n = 9990.0;
        for _ in 0..30 { u.adjust(Item::MagN, 1); }
        assert_eq!(u.m.mag_n, 10_000.0, "{layout:?}");
        u.adjust(Item::MagN, 512);
        assert_eq!(u.m.mag_n, 10_000.0);
        assert_eq!(u.numpad_apply(Item::MagN, 1e9), 10_000.0);
        for _ in 0..4 { u.adjust(Item::MagN, -4096); }
        assert_eq!(u.m.mag_n, 1.0);
        for _ in 0..3000 {
            u.adjust(Item::MathOffset, 1);
            u.adjust(Item::SpecRef, 1);
        }
        assert_eq!(u.m.math_offset, 5.0);
        assert_eq!(u.m.spec_ref, ranges::SPEC_REF_DBV.1);
        for _ in 0..3000 {
            u.adjust(Item::MathOffset, -1);
            u.adjust(Item::SpecRef, -1);
        }
        assert_eq!(u.m.math_offset, -5.0);
        assert_eq!(u.m.spec_ref, ranges::SPEC_REF_DBV.0);
    }
}

// ------------------------------------------------------------------------------------------------ 3 clamp and back

/// Presses `d` until a press does not move the value any more. The number of presses that moved it.
fn to_the_limit(u: &mut Ui, it: Item, d: i32) -> u32 {
    let mut n = 0;
    loop {
        let before = number(u, it);
        u.adjust(it, d);
        if number(u, it) == before || n > 100_000 { return n; }
        n += 1;
    }
}

const CLAMPED: [Item; 12] = [
    Item::TrigLevel, Item::HDelay, Item::ChOffset(0), Item::ChOffset(1), Item::ChBias(0), Item::ChSkew(0), Item::DecThr, Item::BusClkThr,
    Item::BusDataThr, Item::MathOffset, Item::SpecRef, Item::PatLevel,
];

#[test]
fn a_press_up_into_the_limit_and_the_same_number_down_return_to_the_start_for_every_level_offset_and_threshold() {
    for layout in LAYOUTS {
        for it in CLAMPED {
            for dir in [1, -1] {
                let mut u = fresh(layout);
                // an awkward start: not a multiple of the step
                if let Some(Step::Add(s)) = u.item_range(it).map(|r| r.step) {
                    let v = u.range_get(it).unwrap();
                    u.range_set(it, v + 0.37 * s);
                }
                let start = number(&u, it);
                let n = to_the_limit(&mut u, it, dir);
                assert!(n > 0, "{layout:?} {it:?} moves");
                for _ in 0..n { u.adjust(it, -dir); }
                let back = number(&u, it);
                let tol = match u.item_range(it).map(|r| r.step) { Some(Step::Add(s)) => s * 1e-3, _ => 1e-9 };
                assert!((back - start).abs() <= tol, "{layout:?} {it:?} {dir:+}: {start} -> {n} presses to the limit -> {back}");
            }
        }
    }
}

#[test]
fn the_trigger_level_of_the_function_check_comes_back_from_the_limit() {
    // Level 1.65 V at 1 V/div: to the limit in 209 presses and back 1.69 V before
    let mut u = fresh(Layout::Default);
    u.m.trig_level = 1.65;
    let n = to_the_limit(&mut u, Item::TrigLevel, 1);
    assert!((u.m.trig_level as f64 - 5.0 * u.trigger_vpd(false) as f64).abs() < 1e-5);
    for _ in 0..n { u.adjust(Item::TrigLevel, -1); }
    assert!((u.m.trig_level - 1.65).abs() < 1e-4, "{}", u.m.trig_level);
}

// ------------------------------------------------------------------------------------------------ 4 limits follow the range

#[test]
fn the_trigger_level_and_the_channel_offset_follow_the_volts_per_division() {
    let mut seen = Vec::new();
    for vs in [0usize, 4, 8, 11] {
        let mut u = fresh(Layout::Default);
        u.m.ch[0].vscale = vs;
        u.m.ch[0].vfine = 1.0;
        u.m.trig_src = 0;
        let vpd = u.m.ch[0].volts_per_div() as f64;
        let level = u.item_range(Item::TrigLevel).unwrap();
        assert!((level.hi - 5.0 * vpd).abs() < 1e-9, "{vpd} V/div: level limit {}", level.hi);
        let off = u.item_range(Item::ChOffset(0)).unwrap();
        assert!((off.hi - u.m.ch[0].offset_max() as f64).abs() < 1e-9);
        // to the limit (by presses where it is near, by the setter where the range is many thousand steps), and no further
        u.range_set(Item::TrigLevel, 1e9);
        u.range_set(Item::ChOffset(0), 1e9);
        for _ in 0..3 {
            u.adjust(Item::TrigLevel, 1);
            u.adjust(Item::ChOffset(0), 1);
        }
        assert!((u.m.trig_level as f64 - level.hi).abs() < 1e-4 * level.hi.max(1e-3), "{vpd}: {}", u.m.trig_level);
        assert!((u.m.ch[0].offset as f64 - off.hi).abs() < 1e-4 * off.hi, "{vpd}: {}", u.m.ch[0].offset);
        seen.push((vpd, level.hi, off.hi));
    }
    assert!(seen.windows(2).all(|w| w[1].1 != w[0].1), "level limits differ per V/div: {seen:?}");
    assert!(seen.iter().any(|s| (s.1 - 10.0).abs() > 1e-6) && seen.iter().any(|s| (s.2 - 10.0).abs() > 1e-6), "{seen:?}");
}

// ------------------------------------------------------------------------------------------------ 5 defaults off the ladder

type Setup = Box<dyn Fn(&mut Ui)>;

#[test]
fn a_value_between_two_rungs_comes_back_after_presses_down_and_up_without_warm_up() {
    let cases: Vec<(&str, Item, Setup)> = vec![
        ("Sim Rate", Item::SimRate, Box::new(|u| u.m.sim_rate = 16e9)),
        ("Sim Bandwidth", Item::SimBw, Box::new(|u| u.m.sim_bw = 12e9)),
        ("Sim PJ", Item::SimPj, Box::new(|u| u.m.sim_pj = 4e-12)),
        ("Sim RJ", Item::SimRj, Box::new(|u| u.m.sim_rj = 1.2e-12)),
        ("Sim Rise", Item::SimRise, Box::new(|u| u.m.sim_rise = 20e-12)),
        ("SSC frequency", Item::SimSscHz, Box::new(|u| u.m.sim_ssc_hz = 33e3)),
        ("Trigger Time", Item::TrigT1, Box::new(|u| u.m.trig_t1 = 30e-6)),
        ("Eye Bit rate", Item::EyeRate, Box::new(|u| u.m.eye_rate = 16e9)),
        ("Spectrum Span", Item::SpecSpan, Box::new(|u| u.m.spec_span = 3e6)),
    ];
    for layout in LAYOUTS {
        for (name, it, set) in &cases {
            for d in [-1, 1] {
                let mut u = fresh(layout);
                set(&mut u);
                let start = u.value(*it);
                // N presses one way and N the other, from a value between two rungs
                for _ in 0..2 { u.adjust(*it, d); }
                assert_ne!(u.value(*it), start, "{name} moved");
                for _ in 0..2 { u.adjust(*it, -d); }
                assert_eq!(u.value(*it), start, "{layout:?} {name}: {d:+} x2 then back");
            }
            let mut u = fresh(layout);
            set(&mut u);
            let a = u.value(*it);
            u.adjust(*it, -1);
            let mid = u.value(*it);
            u.adjust(*it, 1);
            assert_ne!(a, mid, "{name}");
            assert_eq!(a, u.value(*it), "{layout:?} {name}: {a} -> {mid} -> {}", u.value(*it));
        }
    }
}

#[test]
fn the_ladder_goes_to_the_nearest_rung_in_the_direction_of_the_press_and_never_skips_one() {
    let mut u = fresh(Layout::Default);
    let l = [1e8, 2e8, 5e8, 1e9, 2e9, 5e9, 10e9, 20e9, 50e9];
    assert_eq!(u.ladder_move(Item::SimRate, 16e9, 1, &l), 20e9);
    assert_eq!(u.ladder_move(Item::SimRate, 20e9, -1, &l), 16e9);
    assert_eq!(u.ladder_move(Item::SimRate, 16e9, -1, &l), 10e9);
    assert_eq!(u.ladder_move(Item::SimRate, 10e9, 1, &l), 16e9);
    // a value set by something else starts over
    assert_eq!(u.ladder_move(Item::SimRate, 14e9, 1, &l), 20e9);
    assert_eq!(u.ladder_move(Item::SimRate, 20e9, -1, &l), 14e9);
    // the ends stop
    assert_eq!(u.ladder_move(Item::SimRate, 50e9, 5, &l), 50e9);
    assert_eq!(u.ladder_move(Item::SimRate, 1e8, -5, &l), 1e8);
}

// ------------------------------------------------------------------------------------------------ 6 cursors

#[test]
fn a_plus_on_an_amplitude_cursor_raises_its_reading_and_a_minus_lowers_it() {
    for layout in LAYOUTS {
        let mut u = fresh(layout);
        u.m.cursor = CursorMode::Volt;
        u.m.cur_a = 0.4;
        u.m.cur_b = 0.6;
        frame(&mut u);
        for it in [Item::CurA, Item::CurB] {
            let v0 = number(&u, it);
            u.adjust(it, 1);
            let up = number(&u, it);
            assert!(up > v0, "{layout:?} {it:?}: + {v0} -> {up}");
            u.adjust(it, -2);
            let down = number(&u, it);
            assert!(down < v0, "{layout:?} {it:?}: - {v0} -> {down}");
            u.adjust(it, 1);
            assert!((number(&u, it) - v0).abs() < 1e-9);
        }
        // the time cursors still move to the right with +
        u.m.cursor = CursorMode::Time;
        let a = u.m.cur_a;
        u.adjust(Item::CurA, 1);
        assert!(u.m.cur_a > a);
    }
}

// ------------------------------------------------------------------------------------------------ 7 spectrum from Auto

fn spectrum_scope(layout: Layout) -> Ui {
    let mut u = fresh(layout);
    u.m.view = AView::Spectrum;
    u.m.spec_rbw = 0.0;
    u.m.spec_span = 0.0;
    u.relayout();
    for _ in 0..3 { frame(&mut u); }
    u.analyzer_update();
    u
}

#[test]
fn rbw_and_span_step_from_the_value_auto_shows_not_from_one_hertz() {
    for layout in LAYOUTS {
        let mut u = spectrum_scope(layout);
        let rbw = u.azr.spec.as_ref().map(|s| s.rbw).expect("a spectrum");
        assert!(rbw > 10.0, "{rbw}");
        assert!(u.value(Item::SpecRbw).starts_with("Auto"));
        u.adjust(Item::SpecRbw, 1);
        assert!(u.m.spec_rbw > rbw && u.m.spec_rbw < 3.0 * rbw, "{layout:?}: + from Auto {rbw} gave {}", u.m.spec_rbw);
        let mut d = spectrum_scope(layout);
        d.adjust(Item::SpecRbw, -1);
        assert!(d.m.spec_rbw > 0.0 && d.m.spec_rbw < rbw, "{layout:?}: - from Auto gave {}", d.m.spec_rbw);
        // the span
        let mut s = spectrum_scope(layout);
        if s.azr.spec_stop > 0.0 && s.spec_follows() {
            let shown = s.azr.spec_stop;
            s.adjust(Item::SpecSpan, 1);
            assert!(s.m.spec_span >= shown && s.m.spec_span < 3.0 * shown, "{layout:?}: span + from Auto {shown} gave {}", s.m.spec_span);
        }
        // with nothing to show: the first rung up
        let mut n = fresh(layout);
        n.m.spec_rbw = 0.0;
        n.azr.spec = None;
        n.adjust(Item::SpecRbw, 1);
        assert!(n.m.spec_rbw >= 1.0);
    }
}

// ------------------------------------------------------------------------------------------------ 8 Bode

#[test]
fn bode_start_and_amplitude_show_what_they_hold_after_every_tap() {
    for layout in [Layout::Minimal, Layout::Default] {
        let mut u = fresh(layout);
        u.open_menu(MenuId::Bode);
        frame(&mut u);
        let tscale = u.m.tscale;
        for it in [Item::BodeStart, Item::BodeAmp] {
            for press in 0..6 {
                let (minus, _, plus) = Ui::value_parts(row_of(&u, it).ctrl);
                let before = (u.m.bode_start, u.m.bode_stop, u.m.bode_amp);
                let shown_before = u.value(it);
                tap_rect(&mut u, if press < 3 { minus } else { plus });
                let after = (u.m.bode_start, u.m.bode_stop, u.m.bode_amp);
                // the other numbers do not move
                if it == Item::BodeStart {
                    assert_eq!((before.1, before.2), (after.1, after.2), "{layout:?} {it:?}");
                } else {
                    assert_eq!((before.0, before.1), (after.0, after.1), "{layout:?} {it:?}");
                }
                let (b, a) = if it == Item::BodeStart { (before.0, after.0) } else { (before.2, after.2) };
                if a != b { assert_ne!(u.value(it), shown_before, "{layout:?} {it:?} press {press}: the value moved to {a} but shows {shown_before}"); }
                assert_eq!(u.m.tscale, tscale, "{layout:?} {it:?}: time/div");
                let want = if it == Item::BodeStart { si_short(after.0, "Hz") } else { si_short(after.2, "Vpp") };
                assert_eq!(u.value(it), want, "{layout:?} {it:?}");
            }
        }
    }
}

#[test]
fn the_bode_amplitude_steps_below_one_volt_down_to_ten_millivolts() {
    let mut u = fresh(Layout::Default);
    u.m.bode_amp = 1.0;
    let mut seen = vec![u.m.bode_amp];
    for _ in 0..8 {
        u.adjust(Item::BodeAmp, -1);
        seen.push(u.m.bode_amp);
    }
    assert_eq!(seen[..4], [1.0, 0.5, 0.2, 0.1]);
    assert_eq!(*seen.last().unwrap(), 0.01);
    for _ in 0..40 { u.adjust(Item::BodeAmp, 1); }
    assert_eq!(u.m.bode_amp, 20.0);
}

// ------------------------------------------------------------------------------------------------ 9 labels

#[test]
fn the_eye_placement_under_is_kept_in_studio_and_shown() {
    for layout in LAYOUTS {
        let mut u = fresh(layout);
        u.m.view = AView::Eye;
        u.select_option(Item::EyePlace, "Under");
        frame(&mut u);
        if layout == Layout::Studio {
            assert_eq!(u.m.eye_place, EyePlace::Under);
            assert_eq!(u.value(Item::EyePlace), "Under");
        } else {
            // beside the trace when there is no room below (and told so); the label says what it is
            assert_eq!(u.value(Item::EyePlace), if u.m.eye_place == EyePlace::Under { "Under" } else { "Side" });
        }
        // chosen before the eye is open: it stays
        let mut u = fresh(layout);
        u.select_option(Item::EyePlace, "Under");
        assert_eq!(u.m.eye_place, EyePlace::Under, "{layout:?}");
    }
}

#[test]
fn the_bus_read_write_bit_included_is_shown_after_a_tap() {
    for layout in [Layout::Minimal, Layout::Default] {
        let mut u = fresh(layout);
        u.open_menu(MenuId::Bus);
        frame(&mut u);
        let row = row_of(&u, Item::BusRw);
        let Ctl::Seg(opts, _) = &row.ctl else { panic!("a segmented row") };
        assert_eq!(opts.as_slice(), ["Left out", "Included"]);
        let r = Rect::new(row.ctrl.x + row.ctrl.w * 3 / 4, row.ctrl.y, 4, row.ctrl.h);
        tap_rect(&mut u, r);
        assert!(u.m.bus.addr_rw, "{layout:?}");
        assert_eq!(u.value(Item::BusRw), "Included");
        let Ctl::Seg(_, sel) = row_of(&u, Item::BusRw).ctl else { panic!() };
        assert_eq!(sel, Some(1));
    }
    let mut u = fresh(Layout::Studio);
    u.select_option(Item::BusRw, "Included");
    assert_eq!(u.value(Item::BusRw), "Included");
}

// ------------------------------------------------------------------------------------------------ 10 cursor source

#[test]
fn the_cursor_source_changes_only_the_cursors() {
    for layout in LAYOUTS {
        let mut u = fresh(layout);
        u.m.cursor = CursorMode::Track;
        u.m.active = 0;
        u.meas_src = WaveId::Ch(0);
        u.select_workspace_source(WaveId::Ch(0));
        assert_eq!(u.cursor_source(), WaveId::Ch(0));
        if layout == Layout::Studio {
            // the Cursor sheet has the Source row
            u.select_option(Item::CurSrc, "CH2");
            assert_eq!(u.cursor_source(), WaveId::Ch(1), "{layout:?}");
            assert_eq!(u.m.active, 0, "{layout:?}: the active channel");
            assert_eq!(u.meas_src, WaveId::Ch(0), "{layout:?}: the measurement source");
            assert_eq!(u.value(Item::CurSrc), "CH2");
            // by the stepper too
            u.adjust(Item::CurSrc, -1);
            assert_eq!(u.cursor_source(), WaveId::Ch(0));
            assert_eq!(u.m.active, 0);
        }
        // by SCPI
        scpi_line(&mut u, ":CURSor:MANual:SOURce CHAN2");
        assert_eq!(u.cursor_source(), WaveId::Ch(1));
        assert_eq!((u.m.active, u.meas_src), (0, WaveId::Ch(0)), "{layout:?}: SCPI");
        // choosing a trace anywhere else lets the cursors follow it
        u.select_workspace_source(WaveId::Ch(0));
        assert_eq!(u.cursor_source(), WaveId::Ch(0));
    }
}

// ------------------------------------------------------------------------------------------------ 12 graphics

#[test]
fn the_graphics_choice_shows_what_was_chosen_and_an_even_step_does_nothing() {
    for layout in LAYOUTS {
        let mut u = fresh(layout);
        u.sys.gpu = true;
        u.select_option(Item::Graphics, "CPU");
        assert!(!u.sys.gpu && u.value(Item::Graphics) == "CPU", "{layout:?}");
        u.select_option(Item::Graphics, "CPU");
        assert!(!u.sys.gpu && u.value(Item::Graphics) == "CPU");
        u.adjust(Item::Graphics, 2);
        assert!(!u.sys.gpu);
        u.adjust(Item::Graphics, 1);
        assert!(u.sys.gpu && u.value(Item::Graphics) == "GPU");
        u.adjust(Item::Graphics, 0);
        assert!(u.sys.gpu);
    }
}



