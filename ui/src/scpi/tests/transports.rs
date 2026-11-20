// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The transports of oos-remote with this interpreter on the demo screen: an empty query still gives its end marker,
//! a write gives nothing, and a query behind a fence that is cancelled gives no reply (ADR 0022).

use super::*;
use oos_remote::{hislip, vxi11};

#[test]
fn vxi11_empty_query_has_a_readable_end_marker_and_cancelled_fence_has_no_reply() {
    let mut ui = crate::ui::testkit::demo_ui();
    let cv = Canvas::new(crate::ui::W, crate::ui::H);
    let mut scpi = Scpi::open(0);
    let mut links = vec![Some(vxi11::Link::<Program>::default())];
    let mut run = |p: &mut Program| scpi.poll_program(&mut ui, &cv, p);
    links[0].as_mut().unwrap().write(":MEAS:CLEAR;:MEAS:STRIP?", &mut run);
    assert_eq!(links[0].as_ref().unwrap().out, b"\n");
    let read = vxi11::device_read(&mut links, &mut run);
    assert_eq!(read, (0, 4, b"\n".to_vec()), "empty query succeeds instead of an immediate read timeout");
    let link = links[0].as_mut().unwrap();
    link.write(":MEAS:CLEAR", &mut run);
    assert!(link.out.is_empty());
    link.write(":MEAS:STRIP?;*OPC?", &mut run);
    assert_eq!(std::mem::take(&mut link.out), b";1\n");
    link.write(":MEAS:STRIP?;:TRIG:EDGE:LEV 10;:SINGLE;*OPC?;:RUN", &mut run);
    assert!(link.out.is_empty());
    assert_eq!(link.waiting(), 1);
    scpi.exec_line(&mut ui, &cv, ":STOP");
    link.pump(&mut |p| scpi.poll_program(&mut ui, &cv, p));
    assert!(link.out.is_empty());
    assert_eq!(link.waiting(), 0);
    assert_eq!(ui.m.run, crate::model::RunState::Stop);
}

#[test]
fn hislip_empty_query_emits_data_end_but_writes_and_cancelled_fences_do_not() {
    let mut ui = crate::ui::testkit::demo_ui();
    let cv = Canvas::new(crate::ui::W, crate::ui::H);
    let mut scpi = Scpi::open(0);
    let mut session = hislip::SyncSession::<Program>::default();
    let mut run = |p: &mut Program| scpi.poll_program(&mut ui, &cv, p);
    let empty = session.data_end(7, ":MEAS:CLEAR;:MEAS:STRIP?", &mut run);
    assert_eq!(empty.len(), 1);
    assert_eq!((empty[0].typ, empty[0].param, empty[0].data.clone()), (hislip::DATA_END_TYPE, 7, vec![b'\n']));
    assert!(session.data_end(8, ":MEAS:CLEAR", &mut run).is_empty());
    assert_eq!(session.data_end(9, ":MEAS:STRIP?;*OPC?", &mut run)[0].data, b";1\n");
    let pending = session.data_end(10, ":MEAS:STRIP?;:TRIG:EDGE:LEV 10;:SINGLE;*OPC?;:RUN", &mut run);
    assert!(pending.is_empty(), "an earlier empty query must not emit a premature response at a pending fence");
    let mut program = session.take_waiting().unwrap();
    assert!(program.has_reply);
    scpi.exec_line(&mut ui, &cv, ":STOP");
    assert_eq!(scpi.poll_program(&mut ui, &cv, &mut program), Some(Vec::new()));
    assert!(!program.has_reply, "cancelled queries must not emit an empty DATA_END response");
    assert_eq!(ui.m.run, crate::model::RunState::Stop);
}
