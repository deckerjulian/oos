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
