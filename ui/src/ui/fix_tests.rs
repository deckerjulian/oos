//! Regression tests for the compact (Minimal) chrome audit.
use super::*;
use super::workspace::Action;

#[cfg(audit_shots)]
mod audit_shots;

fn fresh() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.relayout();
    for _ in 0..4 { u.step(0.05); }
    u
}

#[test]
fn the_last_visible_source_cannot_be_switched_off() {
    let mut u = fresh();
    assert_eq!(u.trace_sources().len(), 1);
    u.workspace_action(Action::Source(WaveId::Ch(0)));
    assert!(u.m.ch[0].on && u.trace_sources().len() == 1, "CH1 stays on");
}

fn two_channels() -> Ui {
    let mut u = fresh();
    u.activate_live_channel(1);
    u.relayout();
    u
}

#[test]
fn overlay_keeps_a_split_and_stacked_leaves_a_consistent_density() {
    for density in [Density::Split, Density::Four] {
        let mut u = two_channels();
        u.m.density = density;
        u.apply_density();
        u.relayout();
        let layout = u.m.win_layout;
        assert_ne!(layout, WinLayout::One);
        u.trace_mode(false);
        assert_eq!((u.m.density, u.m.win_layout, u.workspace.stacked), (density, layout, false), "Overlay keeps the split");
        u.trace_mode(true);
        assert!(u.workspace.stacked && u.m.win_layout == WinLayout::One && u.m.density == Density::Scope);
        // choosing a split again drops Stacked, so the highlighted pill is the one in force
        u.m.density = density;
        u.apply_density();
        assert!(!u.workspace.stacked && u.m.win_layout != WinLayout::One);
    }
}

fn with_refs(u: &mut Ui, n: usize) {
    for i in 0..n {
        u.refs[i] = Some(RefWave { src: 0, color: i, label: format!("source-{i}"), t0: -2e-9, dt: 1e-10,
            v: ((0..128).map(|k| ((k as f32 + i as f32) * 0.2).sin() * 0.2).collect::<Vec<_>>()).into(), vpd: 0.1, offset: 0.0, on: true });
    }
}

fn tap(u: &mut Ui, x: i32, y: i32) {
    u.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
    u.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
}

#[test]
fn sources_that_do_not_fit_sit_behind_a_plus_pill_that_opens_the_popup() {
    let mut u = fresh();
    assert!(u.source_overflow().is_none());
    for c in 1..4 { u.activate_live_channel(c); }
    with_refs(&mut u, NREF);
    u.relayout();
    let (hidden, pill) = u.source_overflow().expect("14 sources do not fit");
    let shown = u.source_pills();
    assert_eq!(shown.len() + hidden, 4 + NREF);
    assert!(shown.iter().all(|(_, r)| r.x + r.w <= pill.x), "pills do not overlap the +N pill");
    let (overlay, ..) = u.footer_rects();
    assert!(pill.x + pill.w <= overlay.x, "the +N pill stays left of Overlay");
    tap(&mut u, pill.x + 5, pill.y + 5);
    let popup = u.workspace.popup.as_ref().expect("Sources popup");
    assert!(popup.rows.len() >= 4 + NREF, "every source is in the popup");
}
