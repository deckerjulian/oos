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
