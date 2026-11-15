//! Batch 6: the zoom workflow (bar under the zoomed view, overview box, wheel) and measurements of sources that are off.
//! OOS_SHOTS=<dir> writes a picture of each scene.
use super::*;

const LAYOUTS: [Layout; 3] = [Layout::Default, Layout::Minimal, Layout::Studio];

fn layout(l: Layout) -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = l;
    u.m.ch[1].on = true;
    if l == Layout::Studio { u.studio.enter = true; }
    for _ in 0..8 { u.step(0.05); }
    u.render(&mut Canvas::new(W, H));
    u
}

fn zoomed(l: Layout) -> Ui {
    let mut u = layout(l);
    u.m.run = RunState::Stop;
    u.set_zoom(true);
    u.m.zscale = u.m.tscale.saturating_sub(3);
    u.m.zdelay = 0.0;
    u.relayout();
    for _ in 0..4 { u.step(0.05); }
    u.render(&mut Canvas::new(W, H));
    u
}

fn touch(u: &mut Ui, phase: Phase, x: i32, y: i32) {
    u.handle(Ev::Touch { id: 0, phase, x, y });
    u.step(0.01);
}

fn drag(u: &mut Ui, (x0, y0): (i32, i32), (x1, y1): (i32, i32)) {
    touch(u, Phase::Down, x0, y0);
    for k in 1..=6 { touch(u, Phase::Move, x0 + (x1 - x0) * k / 6, y0 + (y1 - y0) * k / 6); }
    touch(u, Phase::Up, x1, y1);
}

fn centre(r: Rect) -> (i32, i32) { (r.x + r.w / 2, r.y + r.h / 2) }

fn shot(u: &mut Ui, name: &str) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

#[test]
fn the_studio_zoom_bar_has_scale_plus_minus_readout_and_close() {
    let mut u = zoomed(Layout::Studio);
    shot(&mut u, "b6-zoom-bar-studio");
    let bar = u.studio_zoom_bar().expect("bar");
    let row = u.zoom_readout();
    for (i, r) in [bar.label, bar.tdiv, bar.plus, bar.minus, bar.close].iter().enumerate() {
        assert!(r.w > 0 && r.x >= row.x && r.right() <= row.right() && r.y >= row.y && r.bottom() <= row.bottom(), "part {i} {r:?} in {row:?}");
    }
    assert!(bar.label.right() <= bar.tdiv.x && bar.tdiv.right() <= bar.plus.x && bar.plus.right() <= bar.minus.x);
    assert!(bar.minus.right() <= bar.center.x && bar.fit.right() <= bar.more.x && bar.more.right() <= bar.close.x);
    let (tscale, delay, tfine) = (u.m.tscale, u.m.delay, u.m.tfine);
    let z0 = u.m.zscale;
    let factor0 = u.studio_time_zoom();
    // + zooms in one 1-2-5 step, - back out; a touch 19 px off the middle of the drawn button still counts (40 px target)
    let (x, y) = centre(bar.plus);
    u.tap(x, y - 19);
    assert!(u.m.zscale < z0, "plus zooms in");
    assert!(u.studio_time_zoom() > factor0);
    let (x, y) = centre(bar.minus);
    u.tap(x, y + 19);
    assert_eq!(u.m.zscale, z0, "minus zooms out");
    // the main time base is never touched, and the readout is the one source of truth
    assert_eq!((u.m.tscale, u.m.delay, u.m.tfine), (tscale, delay, tfine));
    assert!((u.studio_time_zoom() - u.m.time_per_div() / tscales()[u.m.zscale]).abs() < 1e-3);
    // the scale field opens the time/div entry
    let (x, y) = centre(bar.tdiv);
    u.tap(x, y);
    assert_eq!(u.menu, MenuId::Horizontal);
    u.close_menu();
    // the readout "(Nx zoom)" has room
    assert!(bar.readout.w >= 60, "room for the readout: {:?}", bar.readout);
    // the close button turns zoom off
    let (x, y) = centre(bar.close);
    u.tap(x, y);
    assert!(!u.m.zoom, "x closes the zoom");
    assert_eq!((u.m.tscale, u.m.delay), (tscale, delay));
}

#[test]
fn the_overview_box_pans_resizes_and_is_drawn_anew() {
    let mut u = zoomed(Layout::Studio);
    let o = u.split_zoom(u.grid).0;
    let main = (u.m.tscale, u.m.tfine, u.m.delay);
    let (xa, xb) = u.zoom_window_x(o);
    let y = o.y + 30;
    // inside: pan
    let (z0, d0) = (u.m.zscale, u.m.zdelay);
    drag(&mut u, ((xa + xb) / 2, y), ((xa + xb) / 2 + 60, y));
    assert!(u.m.zdelay > d0 && u.m.zscale == z0, "dragging inside pans");
    // the right edge: resize
    let (_, xb) = u.zoom_window_x(o);
    let z1 = u.m.zscale;
    drag(&mut u, (xb, y), (xb + 90, y));
    assert!(u.m.zscale > z1, "dragging the right edge widens the box: {} -> {}", z1, u.m.zscale);
    // the left edge: resize the other way
    let (xa, xb) = u.zoom_window_x(o);
    let z2 = u.m.zscale;
    drag(&mut u, (xa, y), (xa + (xb - xa) / 3, y));
    assert!(u.m.zscale < z2, "dragging the left edge in narrows it");
    // empty overview: a new box from where the finger went down to where it is
    let (xa, xb) = u.zoom_window_x(o);
    let (from, to) = if xa - o.x > o.right() - xb { (o.x + 8, o.x + 8 + (xa - o.x) / 2) } else { (xb + 20, xb + 20 + (o.right() - xb - 20) / 2) };
    drag(&mut u, (from, y), (to, y));
    let (na, nb) = u.zoom_window_x(o);
    // the width snaps to the 1-2-5 steps: the finest window that holds the drawn box
    assert!(na <= from.min(to) + 2 && nb >= from.max(to) - 2, "box {na}..{nb} for the drag {from}..{to}");
    assert_eq!((u.m.tscale, u.m.tfine, u.m.delay), main, "the main time base stays");
}

#[test]
fn the_wheel_over_the_zoomed_view_no_longer_zooms() {
    let mut u = zoomed(Layout::Studio);
    let z = u.split_zoom(u.grid).1;
    let (tscale, delay) = (u.m.tscale, u.m.delay);
    let x = z.x + z.w * 3 / 4;
    let y = z.y + 40;
    let (z0, d0) = (u.m.zscale, u.m.zdelay);
    // the zoom is the box's business: the wheel over the zoomed waveform leaves it alone
    assert!(!u.pointer_wheel(x, y, 1, false));
    assert_eq!((u.m.zscale, u.m.zdelay), (z0, d0), "the wheel does not zoom");
    assert_eq!((u.m.tscale, u.m.delay), (tscale, delay));
}

fn shown(u: &Ui) -> Vec<WaveId> { u.meas_rows().iter().filter(|r| !u.meas_row_hidden(r)).map(|r| r.src).collect() }

#[test]
fn a_measurement_of_a_channel_that_is_off_is_not_shown_and_returns_with_it() {
    for l in LAYOUTS {
        let mut u = layout(l);
        u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(1))];
        u.m.meas2.clear();
        u.m.cat.clear();
        assert_eq!(u.meas_rows().len(), 3);
        assert_eq!(u.meas_shown_count(), 3, "{l:?}");
        u.m.ch[0].on = false;
        for _ in 0..3 { u.step(0.05); }
        assert_eq!(u.meas_rows().len(), 3, "{l:?}: it stays configured");
        assert_eq!(shown(&u), vec![WaveId::Ch(1)], "{l:?}");
        assert_eq!(u.meas_shown_count(), 1, "{l:?}");
        assert!(!u.meas_groups().contains(&WaveId::Ch(0)), "{l:?}: no strip row for CH1");
        if l == Layout::Studio {
            let rows = u.meas_rows();
            assert_eq!(u.studio_listed(&rows).len(), 1);
        }
        shot(&mut u, &format!("b6-meas-ch1-off-{l:?}"));
        u.m.ch[0].on = true;
        for _ in 0..3 { u.step(0.05); }
        assert_eq!(u.meas_shown_count(), 3, "{l:?}: back with the channel");
        assert!(u.meas_groups().contains(&WaveId::Ch(0)));
    }
}

#[test]
fn a_hidden_ref_measurement_is_not_shown() {
    let mut u = layout(Layout::Default);
    u.refs[0] = Some(RefWave { src: 0, color: 0, label: String::new(), t0: 0.0, dt: 1e-6, v: vec![0.0; 64].into(), vpd: 0.5, offset: 0.0, on: false });
    u.m.meas = vec![(Meas::Vpp, WaveId::Ref(0)), (Meas::Vpp, WaveId::Ch(0))];
    u.m.meas2.clear();
    u.m.cat.clear();
    assert_eq!(shown(&u), vec![WaveId::Ch(0)]);
    u.refs[0].as_mut().unwrap().on = true;
    assert_eq!(shown(&u).len(), 2);
}

#[test]
fn deep_zoom_time_labels_never_repeat() {
    // 500 ps/div with the window near -1.8 us, and neighbours of it
    for delay in [-1.8e-6f32, -1.8004e-6, -1.8037e-6, 3.0e-6, 1.2345e-3] {
        for tdiv in [500e-12f32, 200e-12, 100e-12, 1e-9, 2e-9] {
            // Studio: whole steps from the trigger
            let ticks = studio_time_ticks(tdiv, delay, 1);
            let span = (HDIV / 2.0 * tdiv).abs() + delay.abs();
            let labels: Vec<String> = ticks.iter().map(|&(k, t)| if k == 0 { "0 s".into() } else { axis_label64(t, tdiv, 0.0, span, "s") }).collect();
            let mut seen = std::collections::HashSet::new();
            for s in &labels { assert!(seen.insert(s.clone()), "studio {tdiv} at {delay}: {s} repeats in {labels:?}"); }
            // the other layouts: the window's own divisions
            let labels: Vec<String> = (0..=HDIV as i32).map(|i| axis_label64(delay as f64 + (i as f64 - HDIV as f64 / 2.0) * tdiv as f64, tdiv, delay, span, "s")).collect();
            let mut seen = std::collections::HashSet::new();
            for s in &labels { assert!(seen.insert(s.clone()), "{tdiv} at {delay}: {s} repeats in {labels:?}"); }
        }
    }
}

#[test]
fn shots_of_the_zoom_bar_in_each_layout() {
    for l in LAYOUTS {
        let mut u = zoomed(l);
        shot(&mut u, &format!("b6-zoom-{l:?}"));
    }
}
