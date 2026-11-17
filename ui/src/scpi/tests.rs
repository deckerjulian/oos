// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The tests of the SCPI interpreter through whole command lines: the session and its protocol (`*OPC`, the error
//! queue, compound queries, raw TCP and HiSLIP) and the subsystems. A test of a new command goes here; its query
//! also goes into ui/tests/scpi.txt, its line into docs/scpi.md (AGENTS.md).

use super::measure::meas_token;
use super::*;
use crate::math::{MathOp, MATH_REF_BASE};
use crate::wave::Source;

use crate::ui::testkit::{q as line, scpi_session as session};

// Break it catches: a root moved out of exec_cmd that answers, changes or reports an error otherwise than before
// (scale and position set on the model, a bad number, an unknown node, the debug nodes)
#[test]
fn timebase_measure_and_system_roots_answer_as_before() {
    let (mut s, mut ui, cv) = session();
    let err = |s: &mut Scpi, ui: &mut Ui| line(s, ui, &cv, ":SYST:ERR?");
    line(&mut s, &mut ui, &cv, ":TIM:SCAL 1e-3");
    assert_eq!(line(&mut s, &mut ui, &cv, ":TIMebase:MAIN:SCALe?"), "1.000000E-03");
    line(&mut s, &mut ui, &cv, ":TIM:OFFS 2e-4");
    assert_eq!(line(&mut s, &mut ui, &cv, ":TIM:POS?"), "2.000000E-04");
    assert!(err(&mut s, &mut ui).starts_with("0,"));
    for bad in [":TIM:SCAL abc", ":TIM:FOO 1", ":TIM", ":MEAS:FOO 1", ":MEAS:SET:FOO?", ":SYST:DEB:FOO?", ":SYST:FOO 1"]
    {
        line(&mut s, &mut ui, &cv, bad);
        let got = err(&mut s, &mut ui);
        assert!(got.starts_with("-113,"), "{}: {}", bad, got);
    }
    assert_eq!(line(&mut s, &mut ui, &cv, ":TIM:SCAL?"), "1.000000E-03");
    line(&mut s, &mut ui, &cv, ":MEAS:BURS:IDLE 2e-3");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:BURS:IDLE?"), "2.000000E-03");
    line(&mut s, &mut ui, &cv, ":MEAS:TSET 1e-9");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:TSET?"), "1.000000E-09");
    line(&mut s, &mut ui, &cv, ":SYST:DEB:DVM:LANE 2");
    assert!(err(&mut s, &mut ui).starts_with("0,"));
    line(&mut s, &mut ui, &cv, ":SYST:DEB:DVM:LANE 9");
    assert!(err(&mut s, &mut ui).starts_with("-224,"));
    assert!(err(&mut s, &mut ui).starts_with("0,"));
}

#[test]
fn compatibility_trigger_modes_roundtrip_actual_types() {
    let (mut s, mut ui, cv) = reference_session();
    for (token, reply, kind) in [
        ("EDGE", "EDGE", TrigType::Edge),
        ("PULSe", "PULS", TrigType::Pulse),
        ("SLOPe", "SLOP", TrigType::Slope),
        ("VIDeo", "VID", TrigType::Video),
        ("PATTern", "PATT", TrigType::Pattern),
        ("DURation", "DUR", TrigType::Duration),
        ("TIMeout", "TIM", TrigType::Timeout),
        ("RUNT", "RUNT", TrigType::Runt),
        ("WINDow", "WIND", TrigType::Window),
        ("DELay", "DEL", TrigType::Delay),
        ("SETup", "SET", TrigType::SetupHold),
        ("NEDGe", "NEDG", TrigType::NthEdge),
        ("BUS", "BUS", TrigType::Bus),
    ] {
        line(&mut s, &mut ui, &cv, &format!(":TRIG:MODE {token}"));
        assert_eq!(ui.m.trig_type, kind);
        assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:MODE?"), reply);
        line(&mut s, &mut ui, &cv, &format!(":TRIG:MODE {reply}"));
        assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:TYPE?"), kind.name());
        assert_eq!(ui.m.trig_ref, Some(0));
    }
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("0,"));
    line(&mut s, &mut ui, &cv, ":TRIG:MODE MADEUP");
    assert_eq!(ui.m.trig_type, TrigType::Bus);
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"));
}

#[test]
fn compatibility_waveform_metadata_and_invalid_enums() {
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":WAV:SOUR R1;:WAV:FORM WORD");
    for (mode, kind) in [("RAW", 2), ("NORM", 0)] {
        line(&mut s, &mut ui, &cv, &format!(":WAV:MODE {mode}"));
        let pre = line(&mut s, &mut ui, &cv, ":WAV:PRE?");
        let f: Vec<_> = pre.split(',').collect();
        assert_eq!(f.len(), 10);
        assert_eq!(f[0], "1");
        assert_eq!(f[1], kind.to_string());
        assert_eq!(f[2], "4000");
        assert!((f[4].parse::<f64>().unwrap() - 1e-9).abs() < 1e-15);
        assert_eq!(s.exec_line(&mut ui, &cv, ":WAV:DATA?").len(), 8011);
    }
    line(&mut s, &mut ui, &cv, ":WAV:MODE RAW");
    for command in [":WAV:MODE WRONG", ":WAV:MODE RAWsuffix", ":WAV:FORM WRONG", ":WAV:FORM WORDsuffix"] {
        line(&mut s, &mut ui, &cv, command);
        assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:MODE?;:WAV:FORM?"), "RAW;WORD");
        assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"), "{command}");
    }
}

#[test]
fn compatibility_opc_single_waits_across_ticks_and_resumes_without_replaying_writes() {
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 10");
    let mut p = Program::new(":SINGLE;:TRIG:STAT?;*OPC?;:TRIG:STAT?");
    assert!(s.poll_program(&mut ui, &cv, &mut p).is_none());
    for _ in 0..3 {
        ui.step(0.2);
        assert!(s.poll_program(&mut ui, &cv, &mut p).is_none());
    }
    assert_eq!(captures(&ui), 0.0);
    line(&mut s, &mut ui, &cv, ":TRIG:FORC");
    ui.step(0.2);
    assert_eq!(s.poll_program(&mut ui, &cv, &mut p), Some(b"WAIT;1;STOP".to_vec()));
    assert_eq!(captures(&ui), 1.0);
    ui.step(0.2);
    assert_eq!(captures(&ui), 1.0);
    line(&mut s, &mut ui, &cv, ":RUN");
    assert_eq!(line(&mut s, &mut ui, &cv, "*OPC?"), "1");
}

#[test]
fn compatibility_opc_events_clear_reset_and_immediate_adapter() {
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 10;:SINGLE;*OPC");
    assert_eq!(line(&mut s, &mut ui, &cv, "*ESR?"), "0");
    line(&mut s, &mut ui, &cv, "*CLS");
    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 0");
    ui.step(0.2);
    assert_eq!(line(&mut s, &mut ui, &cv, "*ESR?"), "0");
    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 10;:SINGLE;*OPC");
    assert_eq!(line(&mut s, &mut ui, &cv, "*OPC?"), "");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-221,"));
    line(&mut s, &mut ui, &cv, "*RST");
    assert_eq!(line(&mut s, &mut ui, &cv, "*ESR?"), "0");
    line(&mut s, &mut ui, &cv, ":STOP;*OPC");
    assert_eq!(line(&mut s, &mut ui, &cv, "*ESR?;*ESR?"), "1;0");
}

#[test]
fn compatibility_raw_tcp_pending_opc_does_not_block_other_clients() {
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 10");
    let mut waiting = TcpStream::connect(("127.0.0.1", s.port().unwrap())).unwrap();
    waiting.set_nonblocking(true).unwrap();
    waiting.write_all(b":SINGLE;*OPC?;:TRIG:STAT?\n").unwrap();
    for _ in 0..25 {
        s.poll(&mut ui, &cv);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    ui.step(0.2);
    for _ in 0..25 {
        s.poll(&mut ui, &cv);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let mut bytes = [0; 128];
    assert_eq!(waiting.read(&mut bytes).unwrap_err().kind(), ErrorKind::WouldBlock);
    let mut control = TcpStream::connect(("127.0.0.1", s.port().unwrap())).unwrap();
    control.write_all(b":TRIG:FORC\n").unwrap();
    for _ in 0..25 {
        s.poll(&mut ui, &cv);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    ui.step(0.2);
    for _ in 0..25 {
        s.poll(&mut ui, &cv);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let n = waiting.read(&mut bytes).unwrap();
    assert_eq!(&bytes[..n], b"1;STOP\n");
    assert_eq!(captures(&ui), 1.0);
}

#[test]
fn compatibility_cancelled_single_interrupts_query_and_does_not_follow_new_arm() {
    for stored in [false, true] {
        let (mut s, mut ui, cv) = if stored { reference_session() } else { session() };
        line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 10;:SINGLE;*OPC");
        let mut p = Program::new("*OPC?;:RUN");
        assert!(s.poll_program(&mut ui, &cv, &mut p).is_none());
        ui.step(0.02);
        assert!(
            s.poll_program(&mut ui, &cv, &mut p).is_none(),
            "initial hardware stamp must not complete {stored:?} Single"
        );
        line(&mut s, &mut ui, &cv, ":STOP;:SINGLE");
        assert_eq!(s.poll_program(&mut ui, &cv, &mut p), Some(Vec::new()));
        assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-410,"));
        assert_eq!(line(&mut s, &mut ui, &cv, "*ESR?"), "4", "cancelled query latches query-error bit, not OPC");
        assert_eq!(ui.m.trig_mode, TrigMode::Single);
        let mut fresh = Program::new("*OPC?");
        assert!(s.poll_program(&mut ui, &cv, &mut fresh).is_none());
    }
}

#[test]
fn compatibility_hislip_defers_entire_multiline_payload_with_original_message_id() {
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 10");
    let mut server = crate::hislip::Server::open(0);
    let mut client = TcpStream::connect(("127.0.0.1", server.port().unwrap())).unwrap();
    client.set_nonblocking(true).unwrap();
    client.write_all(&crate::hislip::encode(0, 0, 0x0100_0000, b"hislip0")).unwrap();
    for _ in 0..25 {
        server.poll_async(&mut |p| s.poll_program(&mut ui, &cv, p));
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let mut bytes = [0; 1024];
    let n = client.read(&mut bytes).unwrap();
    let init = crate::hislip::take_message(&mut bytes[..n].to_vec()).unwrap().unwrap();
    assert_eq!(init.typ, 1);
    client.write_all(&crate::hislip::encode(7, 0, 77, b":SINGLE;:TRIG:STAT?;*OPC?\n:TRIG:STAT?\n")).unwrap();
    for _ in 0..25 {
        server.poll_async(&mut |p| s.poll_program(&mut ui, &cv, p));
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    ui.step(0.2);
    for _ in 0..25 {
        server.poll_async(&mut |p| s.poll_program(&mut ui, &cv, p));
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(client.read(&mut bytes).unwrap_err().kind(), ErrorKind::WouldBlock);
    line(&mut s, &mut ui, &cv, ":TRIG:FORC");
    ui.step(0.2);
    for _ in 0..25 {
        server.poll_async(&mut |p| s.poll_program(&mut ui, &cv, p));
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let n = client.read(&mut bytes).unwrap();
    let response = crate::hislip::take_message(&mut bytes[..n].to_vec()).unwrap().unwrap();
    assert_eq!((response.typ, response.param), (7, 77));
    assert_eq!(response.data, b"WAIT;1\nSTOP\n");
}

// Break it catches: SCPI keeping the net.allow of its start (a list changed on the screen counted only after a
// restart of oos-ui), or a client the new list leaves out kept on its open connection
#[test]
fn net_allow_follows_a_change_without_a_restart() {
    let (mut s, mut ui, cv) = session();
    let acl = crate::netacl::Shared::default();
    s.acl = acl.clone();
    // the test's connections come from 198.51.100.7
    acl.peers_as(Some("198.51.100.7".parse().unwrap()));
    let port = s.port().unwrap();
    // what the client reads until it has a line (Some) or the server closed it (None)
    let mut read = |s: &mut Scpi, c: &mut TcpStream| -> Option<String> {
        c.set_nonblocking(true).unwrap();
        let (mut got, until) = (Vec::new(), std::time::Instant::now() + Duration::from_secs(20));
        loop {
            assert!(std::time::Instant::now() < until, "no answer and not closed");
            s.poll(&mut ui, &cv);
            let mut b = [0u8; 256];
            match c.read(&mut b) {
                Ok(0) => return None,
                Ok(n) => got.extend_from_slice(&b[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(1)),
                Err(_) => return None,
            }
            if got.ends_with(b"\n") {
                return Some(String::from_utf8_lossy(&got).into_owned());
            }
        }
    };
    let mut held = TcpStream::connect(("127.0.0.1", port)).unwrap();
    held.write_all(b"*IDN?\n").unwrap();
    assert!(read(&mut s, &mut held).is_some_and(|a| a.starts_with("OOS,")), "no list: everyone");
    // a list without the client: the connection held is closed, a new one refused
    acl.set(crate::netacl::Acl::parse("192.0.2.0/24").0);
    assert_eq!(read(&mut s, &mut held), None, "the connection held is closed");
    let mut new = TcpStream::connect(("127.0.0.1", port)).unwrap();
    assert_eq!(read(&mut s, &mut new), None, "a new connection is refused");
    assert!(s.clients.is_empty());
    // a list with it: in again, without a restart
    acl.set(crate::netacl::Acl::parse("192.0.2.0/24 198.51.100.0/24").0);
    let mut again = TcpStream::connect(("127.0.0.1", port)).unwrap();
    again.write_all(b"*IDN?\n").unwrap();
    assert!(read(&mut s, &mut again).is_some_and(|a| a.starts_with("OOS,")), "inside the new list");
}

// Break it catches: `echo "*IDN?" | nc host 5555` (the client closes its side after the line) got no answer
#[test]
fn a_query_is_answered_after_the_client_closed_its_side() {
    let (mut s, mut ui, cv) = session();
    let mut client = TcpStream::connect(("127.0.0.1", s.port().unwrap())).unwrap();
    client.write_all(b"*IDN?\n").unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    for _ in 0..50 {
        s.poll(&mut ui, &cv);
        if s.clients.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut answer = String::new();
    std::io::Read::read_to_string(&mut client, &mut answer).unwrap();
    assert!(answer.starts_with("OOS,"), "{answer:?}");
}

#[test]
fn compatibility_disconnected_raw_client_cannot_resume_trailing_writes() {
    // a hang guard, not a time the test depends on
    let guard = Duration::from_secs(60);
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 10");
    let mut client = TcpStream::connect(("127.0.0.1", s.port().unwrap())).unwrap();
    client.write_all(b":SINGLE;*OPC?;:RUN\n").unwrap();
    // until the program is in, however long the host takes to deliver the line (it was 50 polls, 100 ms)
    let deadline = std::time::Instant::now() + guard;
    while !s.clients.first().is_some_and(|c| c.program.is_some()) {
        s.poll(&mut ui, &cv);
        assert!(std::time::Instant::now() < deadline, "the program never arrived");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    client.shutdown(std::net::Shutdown::Both).unwrap();
    drop(client);
    // The close goes through the host's network stack and can reach the server's socket after the next poll on a
    // busy computer; that poll would then resume the program. Wait until the server's side reads the end.
    s.clients[0].s.set_read_timeout(Some(guard)).unwrap();
    assert_eq!(s.clients[0].s.peek(&mut [0u8; 1]).expect("the close is readable at the server"), 0);
    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 0");
    ui.step(0.2);
    s.poll(&mut ui, &cv);
    assert!(s.clients.is_empty());
    assert_eq!(ui.m.run, RunState::Stop);
    assert_eq!(captures(&ui), 1.0);
}

#[test]
fn compatibility_program_command_count_is_bounded_before_execution() {
    let (mut s, mut ui, cv) = session();
    let old = ui.m.trig_level;
    let message = format!("{}:TRIG:EDGE:LEV 2", ";".to_string() + &":TRIG:STAT?;".repeat(1025));
    assert!(s.exec_line(&mut ui, &cv, &message).is_empty());
    assert_eq!(ui.m.trig_level, old);
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-363,"));
}

#[test]
fn compatibility_opc_waits_for_actual_large_sim_generation_but_not_future_captures() {
    let (mut s, mut ui, cv) = session();
    let mut p = Program::new(":SIM:PLAY ON;*OPC?;:SIM:BUSY?");
    assert!(s.poll_program(&mut ui, &cv, &mut p).is_none());
    assert!(ui.sim_generating());
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let response = loop {
        ui.step(0.01);
        if let Some(response) = s.poll_program(&mut ui, &cv, &mut p) {
            break response;
        }
        assert!(std::time::Instant::now() < deadline, "SIM operation did not finish");
        std::thread::sleep(Duration::from_millis(2));
    };
    assert!(response.starts_with(b"1;"));
    assert!(ui.ref_info(0).unwrap().0 > 262_144, "exercise real background default SSC");
    line(&mut s, &mut ui, &cv, ":STOP");
    let (mut s, mut ui, cv) = session();
    let mut p = Program::new(":SIM:PLAY ON;*OPC?;:RUN");
    assert!(s.poll_program(&mut ui, &cv, &mut p).is_none());
    line(&mut s, &mut ui, &cv, ":SIM:PLAY OFF");
    assert_eq!(s.poll_program(&mut ui, &cv, &mut p), Some(Vec::new()));
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-410,"));
}

#[test]
fn compatibility_newly_accepted_arm_cannot_complete_a_cancelled_prior_fence() {
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 10;:SINGLE");
    let mut old = Program::new("*OPC?;:RUN");
    assert!(s.poll_program(&mut ui, &cv, &mut old).is_none());
    line(&mut s, &mut ui, &cv, ":STOP;:TRIG:EDGE:LEV 0;:SINGLE");
    ui.step(0.2);
    assert_eq!(captures(&ui), 1.0);
    assert_eq!(s.poll_program(&mut ui, &cv, &mut old), Some(Vec::new()));
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-410,"));
    assert_eq!(ui.m.run, RunState::Stop);
}

#[test]
fn compatibility_hislip_sync_and_async_clear_discard_pending_tail_without_stale_reply() {
    fn read_message(socket: &mut TcpStream) -> crate::hislip::Message {
        let mut b = [0; 4096];
        let n = socket.read(&mut b).unwrap();
        crate::hislip::take_message(&mut b[..n].to_vec()).unwrap().unwrap()
    }
    for asynchronous in [false, true] {
        let (mut scpi, mut ui, cv) = reference_session();
        line(&mut scpi, &mut ui, &cv, ":TRIG:EDGE:LEV 10");
        let mut server = crate::hislip::Server::open(0);
        let port = server.port().unwrap();
        let mut sync = TcpStream::connect(("127.0.0.1", port)).unwrap();
        sync.set_nonblocking(true).unwrap();
        sync.write_all(&crate::hislip::encode(0, 0, 0x0100_0000, b"hislip0")).unwrap();
        for _ in 0..25 {
            server.poll_async(&mut |p| scpi.poll_program(&mut ui, &cv, p));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let session = read_message(&mut sync).param as u16;
        sync.write_all(&crate::hislip::encode(7, 0, 42, b":SINGLE;*OPC?;:RUN\n")).unwrap();
        for _ in 0..25 {
            server.poll_async(&mut |p| scpi.poll_program(&mut ui, &cv, p));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        if asynchronous {
            let mut control = TcpStream::connect(("127.0.0.1", port)).unwrap();
            control.set_nonblocking(true).unwrap();
            control.write_all(&crate::hislip::encode(17, 0, session as u32, b"")).unwrap();
            for _ in 0..25 {
                server.poll_async(&mut |p| scpi.poll_program(&mut ui, &cv, p));
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert_eq!(read_message(&mut control).typ, 18);
            control.write_all(&crate::hislip::encode(19, 0, 0, b"")).unwrap();
            for _ in 0..25 {
                server.poll_async(&mut |p| scpi.poll_program(&mut ui, &cv, p));
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert_eq!(read_message(&mut control).typ, 23);
        } else {
            sync.write_all(&crate::hislip::encode(8, 0, 0, b"")).unwrap();
            for _ in 0..25 {
                server.poll_async(&mut |p| scpi.poll_program(&mut ui, &cv, p));
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert_eq!(read_message(&mut sync).typ, 9);
        }
        line(&mut scpi, &mut ui, &cv, ":TRIG:EDGE:LEV 0");
        ui.step(0.2);
        for _ in 0..25 {
            server.poll_async(&mut |p| scpi.poll_program(&mut ui, &cv, p));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(ui.m.run, RunState::Stop);
        let mut b = [0; 128];
        assert_eq!(sync.read(&mut b).unwrap_err().kind(), ErrorKind::WouldBlock);
        sync.write_all(&crate::hislip::encode(7, 0, 44, b"*IDN?\n")).unwrap();
        for _ in 0..25 {
            server.poll_async(&mut |p| scpi.poll_program(&mut ui, &cv, p));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(read_message(&mut sync).data.starts_with(b"OOS,"));
    }
}

#[test]
fn compatibility_sweep_single_allocates_new_arm_after_a_completed_single() {
    let (mut scpi, mut ui, cv) = reference_session();
    line(&mut scpi, &mut ui, &cv, ":SINGLE");
    ui.step(0.2);
    assert_eq!(captures(&ui), 1.0);
    let mut program = Program::new(":TRIG:SWE NORM;:RUN;:TRIG:EDGE:LEV 10;:TRIG:SWE SING;*OPC?");
    assert!(
        scpi.poll_program(&mut ui, &cv, &mut program).is_none(),
        "a previous Single receipt must not complete the SWEEP arm"
    );
    ui.step(0.2);
    assert!(scpi.poll_program(&mut ui, &cv, &mut program).is_none());
    assert_eq!(captures(&ui), 1.0);
    assert_eq!((ui.m.run, ui.m.status), (RunState::Run, TrigStatus::Wait));
}

#[test]
fn compatibility_pending_and_invalid_program_errors_stay_bounded() {
    let (mut scpi, mut ui, cv) = reference_session();
    line(&mut scpi, &mut ui, &cv, ":TRIG:EDGE:LEV 10;:SINGLE");
    for _ in 0..100 {
        assert!(scpi.exec_line_checked(&mut ui, &cv, "*OPC?").is_err());
    }
    assert!(scpi.err.len() <= ERR_MAX);
    let invalid = ":TRIG:STAT?;".repeat(1025);
    for _ in 0..100 {
        scpi.exec_line(&mut ui, &cv, &invalid);
    }
    assert!(scpi.err.len() <= ERR_MAX);
    assert!(scpi.err.iter().all(|error| error.len() <= ERR_LEN));
}

#[test]
fn compatibility_setup_single_rearms_with_a_fresh_operation_identity() {
    let (mut scpi, mut ui, cv) = reference_session();
    line(&mut scpi, &mut ui, &cv, ":SINGLE");
    ui.step(0.2);
    let mut program = Program::new(":TRIG:SWE NORM;:RUN;:TRIG:EDGE:LEV 10;:SYST:SET trig_mode = Single;*OPC?");
    assert!(scpi.poll_program(&mut ui, &cv, &mut program).is_none());
    ui.step(0.2);
    assert!(scpi.poll_program(&mut ui, &cv, &mut program).is_none());
    assert_eq!(captures(&ui), 1.0);
}

#[test]
fn integration_reference_source_switch_interrupts_old_fence_and_new_single_accepts_once() {
    let (mut scpi, mut ui, cv) = session();
    ui.set_offline(true);
    for slot in 0..2 {
        let values = (0..4000).map(|i| if slot == 0 || i % 40 < 20 { -0.5 } else { 0.5 }).collect();
        ui.commit_waveform_traces_at(
            vec![crate::store::WaveTrace { v: values, dt: 1e-9, t0: 2e-6, label: format!("R{}", slot + 1) }],
            "source-change",
            slot,
        )
        .unwrap();
    }
    let original = [ui.ref_volts(0), ui.ref_volts(1)];
    line(&mut scpi, &mut ui, &cv, ":TRIG:SOUR R1;:TRIG:MODE EDGE;:TRIG:EDGE:LEV 0;:TIM:SCAL 1e-8;:TIM:OFFS 3e-6");
    let mut old = Program::new(":SINGLE;*OPC?;:RUN");
    assert!(scpi.poll_program(&mut ui, &cv, &mut old).is_none());
    ui.step(0.2);
    assert_eq!(captures(&ui), 0.0);
    assert!(scpi.poll_program(&mut ui, &cv, &mut old).is_none());
    // Use the UI acquisition router directly: a SCPI mark_changed call here
    // would mask a missing operation-token hook on the actual source change.
    assert!(ui.set_trigger_source(WaveId::Ref(1), false));
    assert_eq!((ui.m.trig_mode, ui.m.run, ui.m.status), (TrigMode::Single, RunState::Run, TrigStatus::Wait));
    assert_eq!(scpi.poll_program(&mut ui, &cv, &mut old), Some(Vec::new()));
    assert!(line(&mut scpi, &mut ui, &cv, ":SYST:ERR?").starts_with("-410,"));
    let mut new = Program::new("*OPC?;:TRIG:STAT?");
    assert!(scpi.poll_program(&mut ui, &cv, &mut new).is_none());
    ui.step(0.2);
    assert_eq!(scpi.poll_program(&mut ui, &cv, &mut new), Some(b"1;STOP".to_vec()));
    assert_eq!(captures(&ui), 1.0);
    assert_eq!(ui.m.trig_ref, Some(1));
    ui.step(0.2);
    assert_eq!(captures(&ui), 1.0, "the cancelled query's trailing RUN must never execute");
    assert_eq!([ui.ref_volts(0), ui.ref_volts(1)], original);
}

fn reference_session() -> (Scpi, Ui, Canvas) {
    let (mut s, mut ui, cv) = session();
    // Import is additive in Live. These tests explicitly exercise stored
    // acquisition, rather than changing the live-import policy implicitly.
    ui.set_offline(true);
    let mut csv = String::from("Time (s),Volts\n");
    for index in 0..4000 {
        let value = if index % 40 < 20 { -0.5 } else { 0.5 };
        csv.push_str(&format!("{},{}\n", index as f64 * 1e-9, value));
    }
    ui.load_waveform_bytes(csv.as_bytes(), "reference.csv").unwrap();
    ui.set_offline(true);
    line(&mut s, &mut ui, &cv, ":TIM:SCAL 1e-8;:TIM:OFFS 1e-6;:TRIG:EDGE:LEV 0;:TRIG:SWE NORM");
    (s, ui, cv)
}

fn captures(ui: &Ui) -> f64 {
    crate::json::parse(&ui.workflow_state()).unwrap().get("captures").unwrap().num().unwrap()
}

#[test]
fn generic_model_queries_preserve_statistics_configuration_and_pending_requests() {
    let (mut s, mut ui, cv) = session();
    for _ in 0..4 {
        ui.step(0.05);
    }
    let statistics = ui.measurement_statistics_snapshot_for_test();
    assert!(statistics.iter().any(|&(count, _)| count >= 3), "seed statistics from genuine acquired demo frames");
    let configuration = crate::store::setup_text(&ui.m);
    let workflow = ui.workflow_state();
    let acquisition_stamp = ui.m.hw_stamp;
    line(&mut s, &mut ui, &cv, ":UNDEFINED_ONE?;:UNDEFINED_TWO?");
    assert_eq!(s.err.len(), 2);
    let errors = s.err.clone();
    s.wav_src = WaveId::Ref(2);
    s.wav_fmt = 1;
    s.wav_mode = 2;
    // An independently pending client request must not be consumed or
    // cancelled while another client polls configuration/status.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let _peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (stream, _) = listener.accept().unwrap();
    let pending = b":MEAS:ITEM? VPP,CHAN1".to_vec();
    s.clients.push(Client { s: stream, buf: pending.clone(), program: None });
    for query in [":TRIG:STAT?", ":TIM:SCAL?", ":CHAN1:DISP?", ":CHAN1:OFFS?", ":ACQ:SRAT?", ":ACQ:MDEP?"] {
        assert!(!line(&mut s, &mut ui, &cv, query).is_empty(), "{query} must return its existing reply");
        assert_eq!(
            ui.measurement_statistics_snapshot_for_test(),
            statistics,
            "{query} must preserve every statistic bit-for-bit"
        );
        assert_eq!(crate::store::setup_text(&ui.m), configuration, "{query} must not change configuration");
        assert_eq!(ui.workflow_state(), workflow, "{query} must not change acquisition/run state");
        assert_eq!(ui.m.hw_stamp, acquisition_stamp);
        assert_eq!(s.err, errors, "status reads must not consume queued errors");
        assert_eq!(s.clients.len(), 1);
        assert_eq!(s.clients[0].buf, pending);
        assert_eq!((s.wav_src, s.wav_fmt, s.wav_mode), (WaveId::Ref(2), 1, 2));
    }
    line(&mut s, &mut ui, &cv, ":TIM:SCAL 0.000005");
    assert!(
        ui.measurement_statistics_snapshot_for_test().is_empty(),
        "a successful setting still invalidates statistics"
    );
    assert!((ui.m.time_per_div() as f64 - 5e-6).abs() < 1e-12);
    assert_eq!(s.err, errors);
    assert_eq!(s.clients[0].buf, pending);
}

#[test]
fn unsupported_write_only_queries_preserve_records_statistics_and_error_queue() {
    for run in [RunState::Run, RunState::Stop] {
        let (mut s, mut ui, cv) = session();
        for _ in 0..4 {
            ui.step(0.05);
        }
        ui.m.run = run;
        ui.m.trig_mode = TrigMode::Normal;
        ui.m.trig_level = 0.125;
        let statistics = ui.measurement_statistics_snapshot_for_test();
        assert!(statistics.iter().any(|&(count, _)| count >= 3), "use genuinely acquired demo statistics");
        let configuration = crate::store::setup_text(&ui.m);
        let workflow = ui.workflow_state();
        let record: Vec<u32> = ui.record_volts(0).iter().map(|value| value.to_bits()).collect();
        assert!(!record.is_empty());
        line(&mut s, &mut ui, &cv, ":EXISTING_ERROR_ONE?;:EXISTING_ERROR_TWO?");
        assert_eq!(s.err.len(), 2);
        for query in ["RUN?", ":STOP?", ":SING?", ":SINGLE?", "*CLS?", "*RST?", ":CLE?", ":CLEAR?"] {
            let errors = s.err.clone();
            assert!(
                line(&mut s, &mut ui, &cv, query).starts_with("-113,"),
                "{query} must answer Undefined header without acting"
            );
            assert_eq!(crate::store::setup_text(&ui.m), configuration, "{query} must not execute its action");
            assert_eq!(ui.workflow_state(), workflow, "{query} must preserve Run/Stop and acquisition state");
            assert_eq!(
                ui.measurement_statistics_snapshot_for_test(),
                statistics,
                "{query} must preserve all acquired statistics"
            );
            assert_eq!(
                ui.record_volts(0).iter().map(|value| value.to_bits()).collect::<Vec<_>>(),
                record,
                "{query} must not clear data"
            );
            assert_eq!(s.err.len(), errors.len() + 1);
            assert_eq!(&s.err[..errors.len()], errors.as_slice(), "{query} must not clear existing errors");
            assert!(s.err.last().unwrap().starts_with("-113,"), "unsupported query must report Undefined header");
        }
        let errors = s.err.clone();
        assert_eq!(line(&mut s, &mut ui, &cv, ":AUTOSCALE?"), "0", "the existing supported status query stays valid");
        assert_eq!(line(&mut s, &mut ui, &cv, "*OPC?"), "1");
        assert_eq!(s.err, errors);
        assert_eq!(ui.measurement_statistics_snapshot_for_test(), statistics);
    }
}

fn decoded_reference_fixture(ui: &mut Ui, points: usize) {
    ui.set_offline(true);
    let mut values = vec![3.3; points];
    let bits = std::iter::once(false).chain((0..8).map(|bit| 0xa5u8 >> bit & 1 != 0)).chain(std::iter::once(true));
    for (bit, high) in bits.enumerate() {
        values[100 + bit * 20..100 + (bit + 1) * 20].fill(if high { 3.3 } else { 0.0 });
    }
    ui.commit_waveform_traces(
        vec![crate::store::WaveTrace {
            v: values,
            dt: 1.0 / (115_200.0 * 20.0),
            t0: -0.008,
            label: "UART reference".into(),
        }],
        "reference",
        true,
    )
    .unwrap();
}

#[test]
fn decoder_scpi_configures_refs_reports_original_packet_times_and_rejects_empty_sources() {
    let (mut s, mut ui, cv) = session();
    decoded_reference_fixture(&mut ui, 1000);
    assert_eq!(ui.m.n_on(), 0);
    assert_eq!(line(&mut s, &mut ui, &cv, ":DEC:PROTO UART;:DEC:SOUR R1;:DEC:THR 1.65;:DEC:STAT ON;:SYST:SET dec_baud = 7;:DEC:SOUR?;:DEC:PROTO?;:DEC:STAT?"), "R1;UART;1");
    let events = line(&mut s, &mut ui, &cv, ":DEC:EVEN?");
    assert!(events.contains("D1 UART R1") && events.contains("A5"), "{events}");
    let first_time: f64 = events.split(',').next().unwrap().parse().unwrap();
    assert!(first_time > -0.008 && first_time < -0.007);
    assert_eq!(line(&mut s, &mut ui, &cv, ":DEC:BUSY?"), "0");
    let state = crate::store::setup_text(&ui.m);
    assert_eq!(line(&mut s, &mut ui, &cv, ":DEC:SOUR R10;:DEC:SOUR?"), "R1");
    assert_eq!(crate::store::setup_text(&ui.m), state);
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"));
    assert_eq!(
        line(&mut s, &mut ui, &cv, ":DEC:CS NONE;:DEC:SOUR4 NONE;:DEC:DIFF NONE;:DEC:CS?;:DEC:SOUR4?;:DEC:DIFF?"),
        "NONE;NONE;NONE"
    );
    assert_eq!(line(&mut s, &mut ui, &cv, ":DEC:SEL 2;:DEC:SEL?;:DEC:SEL 1;:DEC:SOUR?"), "2;R1");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
}

#[test]
fn decoder_scpi_large_reference_reports_busy_then_fresh_events_without_acquisition() {
    let (mut s, mut ui, cv) = session();
    decoded_reference_fixture(&mut ui, 300_000);
    line(&mut s, &mut ui, &cv, ":DEC:PROTO UART;:DEC:SOUR R1;:DEC:THR 1.65;:DEC:STAT ON;:SYST:SET dec_baud = 7");
    assert_eq!(line(&mut s, &mut ui, &cv, ":DEC:EVEN?"), "BUSY");
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while line(&mut s, &mut ui, &cv, ":DEC:BUSY?") == "1" {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let events = line(&mut s, &mut ui, &cv, ":DEC:EVEN?");
    assert!(events.contains("D1 UART R1") && events.contains("A5"), "{events}");
    assert_eq!(ui.m.run, RunState::Stop);
    assert!(ui.workflow_state().contains("\"captures\":0"));
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
}

#[test]
fn cphy_scpi_configures_three_refs_and_exports_the_published_16_bit_word() {
    let (mut s, mut ui, cv) = session();
    let mut symbols = vec![3; 12];
    symbols.extend([3, 4, 4, 4, 4, 4, 3]);
    symbols.extend([0, 2, 2, 4, 1, 0, 0]);
    symbols.extend([4; 7]);
    let states = oos_dsp::cphy::states_for_symbols(0, &symbols);
    let traces = (0..3)
        .map(|wire| crate::store::WaveTrace {
            v: states
                .iter()
                .flat_map(|&state| std::iter::repeat_n(oos_dsp::cphy::wire_values(state)[wire], 20))
                .collect(),
            dt: 5e-12,
            t0: -12e-9,
            label: format!("C-PHY {}", ['A', 'B', 'C'][wire]),
        })
        .collect();
    ui.commit_waveform_traces(traces, "C-PHY fixture", true).unwrap();
    assert_eq!(line(&mut s, &mut ui, &cv, ":DEC:PROTO CPHY;:DEC:SOUR R1;:DEC:SOUR2 R2;:DEC:CS R3;:DEC:STAT ON;:DEC:PROTO?;:DEC:SOUR?;:DEC:SOUR2?;:DEC:CS?"), "C-PHY;R1;R2;R3");
    let events = line(&mut s, &mut ui, &cv, ":DEC:EVEN?");
    assert!(events.contains("D1 C-PHY R1") && events.contains("SYNC") && events.contains("7290"), "{events}");
    let lines: Vec<_> = events.lines().filter(|line| line.contains("7290")).collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].split(',').next().unwrap().parse::<f64>().unwrap() < 0.0);
    assert_eq!(line(&mut s, &mut ui, &cv, ":DEC:CS NONE;:DEC:CS?"), "R3");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"));
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
}

#[test]
fn cphy_known_rate_roundtrips_per_decoder_setup_and_rejects_invalid_values() {
    let (mut s, mut ui, cv) = session();
    assert_eq!(line(&mut s, &mut ui, &cv, ":DEC:CPHY:RATE?"), "AUTO");
    assert_eq!(
        line(&mut s, &mut ui, &cv, ":DEC:PROTO CPHY;:DEC:CPHY:RATE 2.25e9;:DEC:CPHY:RATE?;:DEC:SRAT?"),
        "2.250000E+09;2.250000E+09"
    );
    let configured = crate::store::setup_text(&ui.m);
    assert_eq!(crate::store::parse_setup(&configured).unwrap().dec_cphy_rate, 2.25e9);
    let old_setup = configured
        .lines()
        .filter(|line| !line.trim_start().starts_with("dec_cphy_rate") && !line.contains(".dec_cphy_rate"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(crate::store::parse_setup(&old_setup).unwrap().dec_cphy_rate, 0.0, "old setups keep Auto");
    for bad in ["-1", "NaN", "inf", "1e13", "garbage"] {
        line(&mut s, &mut ui, &cv, &format!(":DEC:CPHY:RATE {bad}"));
        assert_eq!(crate::store::setup_text(&ui.m), configured);
        assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"));
        let mut restored = ui.m.clone();
        assert!(crate::store::set(&mut restored, "dec_cphy_rate", bad).is_err());
        assert_eq!(restored.dec_cphy_rate, 2.25e9);
    }
    assert_eq!(
        line(&mut s, &mut ui, &cv, ":DEC:SEL 2;:DEC:CPHY:RATE?;:DEC:SEL 1;:DEC:CPHY:RATE?"),
        "AUTO;2.250000E+09"
    );
    assert_eq!(line(&mut s, &mut ui, &cv, ":DEC:CPHY:RATE AUTO;:DEC:CPHY:RATE?"), "AUTO");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
}

#[test]
fn write_only_actions_without_query_suffix_still_execute() {
    let (mut s, mut ui, cv) = session();
    for _ in 0..4 {
        ui.step(0.05);
    }
    assert!(!ui.record_volts(0).is_empty());
    line(&mut s, &mut ui, &cv, ":STOP");
    assert_eq!(ui.m.run, RunState::Stop);
    line(&mut s, &mut ui, &cv, ":RUN");
    assert_eq!(ui.m.run, RunState::Run);
    line(&mut s, &mut ui, &cv, ":SINGLE");
    assert_eq!((ui.m.run, ui.m.trig_mode), (RunState::Run, TrigMode::Single));
    line(&mut s, &mut ui, &cv, ":EXISTING_ERROR?");
    assert!(!s.err.is_empty());
    line(&mut s, &mut ui, &cv, "*CLS");
    assert!(s.err.is_empty());
    line(&mut s, &mut ui, &cv, ":STOP;:CLEAR");
    assert!(ui.record_volts(0).is_empty());
    ui.m.trig_level = 0.125;
    line(&mut s, &mut ui, &cv, "*RST");
    assert_eq!(ui.m.trig_level, Model::default().trig_level);
    assert_eq!(ui.m.trig_mode, Model::default().trig_mode);
    assert!(s.err.is_empty());
}

#[test]
fn digital_trigger_source_queries_preserve_identity_and_secondary_channel() {
    let (mut s, mut ui, cv) = session();
    line(&mut s, &mut ui, &cv, ":LA:STAT ON;:TRIG:EDGE:SOUR D7");
    assert_eq!(ui.m.trig_line(), Some(7), "D7 must be the active digital source");
    assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:EDGE:SOUR?;:TRIG:SOUR?;:TRIG:SOUR2?"), "D7;D7;CHAN4");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");

    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:SOUR CHAN2");
    assert_eq!(ui.m.trig_line(), None);
    assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:EDGE:SOUR?;:TRIG:SOUR2?"), "CHAN2;CHAN4");
}

#[test]
fn reference_trigger_source_query_takes_precedence_over_a_stale_la_selection() {
    let (mut s, mut ui, cv) = reference_session();
    ui.m.la.on = true;
    ui.m.trig_la = Some(7);
    assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:EDGE:SOUR?;:TRIG:SOUR?"), "R1;R1");
}

#[test]
fn selected_catalog_rows_are_exported_and_setup_clear_accepts_an_empty_catalog() {
    let (mut s, mut ui, cv) = session();
    for amplitude in [2.0, 0.5] {
        let mut csv = String::from("Time (s),Volts\n");
        for i in 0..1000 {
            csv.push_str(&format!(
                "{:.12e},{}\n",
                i as f64 * 1e-6,
                amplitude * (i as f64 * std::f64::consts::TAU / 20.0).sin()
            ));
        }
        ui.load_waveform_auto(csv.as_bytes(), "power.csv").unwrap();
    }
    ui.set_offline(true);
    ui.m.meas_full_record = true;
    ui.m.meas.clear();
    ui.m.meas2.clear();
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:SET cat = 90:R1:R2:3:4;:SYST:ERR?"), "0,\"No error\"");
    let end = std::time::Instant::now() + Duration::from_secs(10);
    let answer = loop {
        let answer = line(&mut s, &mut ui, &cv, ":MEAS:STRIP?");
        let value = answer.rsplit(',').next().unwrap().parse::<f64>().unwrap();
        if value.abs() < 1e30 {
            assert!((value - 0.5).abs() < 0.01, "{answer}");
            break answer;
        }
        assert!(std::time::Instant::now() < end, "{answer}");
        ui.step(0.0);
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(answer.starts_with("CAT90,R1,R2,CHAN3,CHAN4,"), "{answer}");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:READOUT?"), answer);
    assert_eq!(captures(&ui), 0.0, "SCPI row exports are not acquisitions");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:SET cat = ;:SYST:ERR?"), "0,\"No error\"");
    assert!(ui.m.cat.is_empty());
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:STRIP?"), "");
}

#[test]
fn strip_and_readout_export_the_configured_row_levels_and_physical_time() {
    let (mut s, mut ui, cv) = session();
    let mut csv = String::from("Time (s),Volts\n");
    for i in 0..800 {
        let k = i % 400;
        let v = match k {
            0..=99 => -0.5,
            100..=199 => -0.5 + (k - 100) as f64 / 99.0,
            200..=299 => 0.5,
            _ => 0.5 - (k - 300) as f64 / 99.0,
        };
        csv.push_str(&format!("{:.12e},{v}\n", 0.001 + i as f64 * 1e-9));
    }
    ui.load_waveform_bytes(csv.as_bytes(), "row-levels.csv").unwrap();
    ui.m.run = RunState::Stop;
    ui.m.meas_full_record = true;
    let id = WaveId::Ref(0);
    ui.m.meas = vec![(Meas::Rise, id), (Meas::TVmax, id)];
    ui.m.meas2.clear();
    ui.set_meas_refs(0, 0.2, 0.8);
    ui.set_meas_refs(1, 0.2, 0.8);
    let shared = ui.meas_at(id, Meas::Rise);
    let own = ui.fact_meas(Meas::Rise, id);
    assert!(own < shared * 0.85, "row {own}, shared {shared}");
    let time = ui.fact_meas(Meas::TVmax, id);
    assert!((time - (0.001 + 199e-9)).abs() < 1e-13, "{time}");
    let expected = format!("{},R1,{};{},R1,{}", meas_token(Meas::Rise), e(own), meas_token(Meas::TVmax), e(time));
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:STRIP?"), expected);
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:READOUT?"), expected);
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
    assert_eq!(captures(&ui), 0.0);
}

#[test]
fn measurement_gating_screen_tracks_pan_while_none_measures_the_full_record() {
    let (mut s, mut ui, cv) = session();
    ui.set_offline(true);
    let mut csv = String::from("Time (s),Volts\n");
    for index in 0..4000 {
        let amplitude = if index < 2000 { 0.25 } else { 0.75 };
        let value = if index % 40 < 20 { -amplitude } else { amplitude };
        csv.push_str(&format!("{},{}\n", index as f64 * 1e-9, value));
    }
    ui.load_waveform_bytes(csv.as_bytes(), "gate-regions.csv").unwrap();
    line(&mut s, &mut ui, &cv, ":TIM:SCAL 1e-8;:TIM:OFFS 1e-6;:MEAS:GAT SCREEN");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:GATING?"), "SCREEN");
    let vpp = |s: &mut Scpi, ui: &mut Ui| line(s, ui, &cv, ":MEAS:ITEM? VPP,R1").parse::<f64>().unwrap();
    let first = vpp(&mut s, &mut ui);
    assert!(
        (first - 0.5).abs() < 1e-6,
        "first screen Vpp {first}, timebase {:?}, delay {}, zoom {}, state {}",
        ui.m.time_per_div(),
        ui.m.delay,
        ui.m.zoom,
        ui.workflow_state()
    );
    line(&mut s, &mut ui, &cv, ":TIM:OFFS 3e-6");
    assert!((vpp(&mut s, &mut ui) - 1.5).abs() < 1e-6, "SCREEN follows the stopped viewport");

    line(&mut s, &mut ui, &cv, ":MEAS:GATING NONE");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:GAT?"), "NONE");
    assert!((vpp(&mut s, &mut ui) - 1.5).abs() < 1e-6);
    line(&mut s, &mut ui, &cv, ":TIM:OFFS 1e-6");
    assert!((vpp(&mut s, &mut ui) - 1.5).abs() < 1e-6, "NONE keeps the full record while panning");

    line(&mut s, &mut ui, &cv, ":MEAS:GAT SCREEN");
    assert!((vpp(&mut s, &mut ui) - 0.5).abs() < 1e-6, "restoring SCREEN recomputes the stopped gate");
    assert_eq!((ui.m.run, captures(&ui)), (RunState::Stop, 0.0), "gate changes and pan must not acquire");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");

    line(&mut s, &mut ui, &cv, ":MEAS:GAT unsupported");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:GAT?"), "SCREEN");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"));
}

#[test]
fn reference_run_stop_are_idempotent_and_hold_acquisitions() {
    let (mut s, mut ui, cv) = reference_session();
    assert_eq!(line(&mut s, &mut ui, &cv, ":RUN;:SIM:PLAY?"), "1", "RUN updates replay immediately");
    assert_eq!(line(&mut s, &mut ui, &cv, ":RUN;:SIM:PLAY?"), "1", "repeated RUN must keep running");
    ui.step(0.2);
    assert!(captures(&ui) > 0.0, "RUN must capture a reference window");
    let acquired = captures(&ui);
    assert_eq!(line(&mut s, &mut ui, &cv, ":STOP;:SIM:PLAY?;:TRIG:STAT?"), "0;STOP");
    assert_eq!(line(&mut s, &mut ui, &cv, ":STOP;:SIM:PLAY?"), "0", "repeated STOP must keep stopped");
    ui.step(0.5);
    assert_eq!(captures(&ui), acquired, "STOP must hold the acquired record");
    assert_eq!(ui.m.run, RunState::Stop);
}

#[test]
fn reference_single_waits_for_a_trigger_and_rearms_with_a_menu_open() {
    let (mut s, mut ui, cv) = reference_session();
    ui.workflow_command("settings");
    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 10;:SING");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:PLAY?;:TRIG:STAT?"), "1;WAIT");
    ui.step(0.2);
    assert_eq!(captures(&ui), 0.0, "an impossible trigger must wait");
    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 0");
    ui.step(0.2);
    assert_eq!(captures(&ui), 1.0);
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:PLAY?;:TRIG:STAT?"), "0;STOP");
    line(&mut s, &mut ui, &cv, ":SING");
    ui.step(0.2);
    assert_eq!(captures(&ui), 2.0, "a second SINGLE must re-arm and capture once");
    assert_eq!(ui.m.run, RunState::Stop);
    ui.step(0.5);
    assert_eq!(captures(&ui), 2.0);
}

#[test]
fn raw_tcp_compound_queries_share_one_response_message() {
    let (mut s, mut ui, cv) = session();
    let mut client = TcpStream::connect(("127.0.0.1", s.port().unwrap())).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    client.write_all(b"*IDN?;*OPC?\n").unwrap();
    for _ in 0..50 {
        s.poll(&mut ui, &cv);
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut response = std::io::BufReader::new(client.try_clone().unwrap());
    let mut first = String::new();
    std::io::BufRead::read_line(&mut response, &mut first).unwrap();
    assert!(first.starts_with("OOS,") && first.ends_with(";1\n"), "{first:?}");
    client.write_all(b"*OPC?\n").unwrap();
    for _ in 0..50 {
        s.poll(&mut ui, &cv);
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut second = String::new();
    std::io::BufRead::read_line(&mut response, &mut second).unwrap();
    assert_eq!(second, "1\n", "compound replies must not leave a stray response for the next query");
}

#[test]
fn a_remote_eye_view_analyzes_a_stopped_reference_without_another_capture() {
    let (mut s, mut ui, cv) = session();
    line(&mut s, &mut ui, &cv, ":SIM:PRES \"USB 2.0 HS\";:SIM:PLAY ON;:STOP");
    let acquired = captures(&ui);
    line(&mut s, &mut ui, &cv, ":VIEW EYE");
    let unit_interval: f64 = line(&mut s, &mut ui, &cv, ":EYE:ITEM? UI").parse().unwrap();
    assert!((unit_interval * 480e6 - 1.0).abs() < 0.02, "UI {unit_interval}");
    let edges: f64 = line(&mut s, &mut ui, &cv, ":EYE:ITEM? EDGES").parse().unwrap();
    assert!(edges > 1000.0, "Eye needs the full reference, got {edges} edges");
    assert_eq!(captures(&ui), acquired, "opening Eye must not acquire another window");
    assert_eq!(ui.m.run, RunState::Stop);
}

#[test]
fn sim_advanced_parameters_validate_without_regenerating_the_record() {
    let (mut s, mut ui, cv) = session();
    line(&mut s, &mut ui, &cv, ":SIM:SSC:MODE CENTER;:SIM:SSC:DEV 0.007;:SIM:SSC:FREQ 40000;:SIM:JITT:DCD 4e-12;:SIM:JITT:CORR 2e-12;:SIM:JITT:CORN 2e6;:SIM:PATT PRBS-13");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:SSC:MODE?;:SIM:PATT?;:SIM:BUSY?"), "CENTER;PRBS13;0");
    for (command, expected) in [
        (":SIM:SSC:DEV?", 0.007),
        (":SIM:SSC:FREQ?", 40000.0),
        (":SIM:JITT:DCD?", 4e-12),
        (":SIM:JITT:CORR?", 2e-12),
        (":SIM:JITT:CORN?", 2e6),
    ] {
        let actual: f64 = line(&mut s, &mut ui, &cv, command).parse().unwrap();
        assert!((actual - expected).abs() <= expected.abs() * 1e-6, "{command}: {actual}");
    }
    assert!((0..NREF).all(|slot| ui.ref_volts(slot).is_empty()), "configuration must not replace sources until Play");
    for command in [
        ":SIM:SSC:MODE BAD",
        ":SIM:SSC:DEV NaN",
        ":SIM:SSC:FREQ 0",
        ":SIM:JITT:DCD -1",
        ":SIM:JITT:CORR inf",
        ":SIM:JITT:CORN 1e30",
        ":SIM:PATT PRBS8",
    ] {
        line(&mut s, &mut ui, &cv, command);
        assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"), "{command}");
    }
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:SSC:MODE?;:SIM:PATT?"), "CENTER;PRBS13");
    assert_eq!(ui.m.sim_ssc_deviation, 0.007);
    assert_eq!(ui.m.sim_dcd_pp, 4e-12);
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
}

#[test]
fn sim_rejects_an_oversized_full_ssc_record_without_changing_the_live_session() {
    let (mut s, mut ui, cv) = session();
    let running = ui.m.run;
    line(&mut s, &mut ui, &cv, ":SIM:PRES \"PCIe 4.0\";:SIM:SSC:FREQ 1000;:SIM:PLAY ON");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"));
    assert_eq!(ui.m.run, running);
    assert!(!ui.record_mode());
    assert!((0..NREF).all(|slot| ui.ref_volts(slot).is_empty()));
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:BUSY?"), "0");
}

#[test]
fn sim_play_fills_the_slot_and_waveform_reads_it() {
    let (mut s, mut ui, cv) = session();
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:PRES?"), "PCIe 4.0");
    line(&mut s, &mut ui, &cv, ":SIM:PRES \"USB 2.0 HS\"");
    line(&mut s, &mut ui, &cv, ":SIM:SLOT R3");
    line(&mut s, &mut ui, &cv, ":SIM:PLAY");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:PRES?;:SIM:SLOT?;:SIM:PLAY?"), "USB 2.0 HS;R3;1");
    let vpp = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? VPP,R3");
    assert_ne!(vpp, "9.9E37");
    line(&mut s, &mut ui, &cv, ":WAV:SOUR R3");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:SOUR?"), "R3");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:POIN?"), "65536");
    line(&mut s, &mut ui, &cv, ":SIM:SHOW OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:SHOW?"), "0");
    line(&mut s, &mut ui, &cv, ":SIM:PLAY OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:PLAY?"), "0");
    line(&mut s, &mut ui, &cv, ":WAV:SOUR CHAN1");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:SOUR?"), "CHAN1");
}

#[test]
fn load_and_import_csv_bin_and_wfm() {
    let (mut s, mut ui, cv) = session();
    let dir = crate::ui::testkit::Scratch::new("ref");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    ui.store_dir = dir.clone();

    let csv = dir.join("lane.csv");
    std::fs::write(&csv, "Time (s),Volts\n0,0.1\n1e-9,0.4\n2e-9,-0.2\n").unwrap();
    line(&mut s, &mut ui, &cv, &format!(":REF:SLOT R2;:REF:LOAD \"{}\"", csv.display()));
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
    line(&mut s, &mut ui, &cv, ":WAV:FORM ASC;:WAV:SOUR R2");
    let data = line(&mut s, &mut ui, &cv, ":WAV:DATA?");
    assert!(data.starts_with("#9"), "{data}");
    assert!(data.contains("1.000000E-01"), "{data}");

    let bin_src = {
        let mut m = crate::model::Model::default();
        m.ch[0].on = true;
        let rec = crate::wave::Record {
            t0: 0.0,
            dt: 1e-9,
            ch: {
                let mut c: [Vec<f32>; crate::model::NCH] = Default::default();
                c[0] = vec![0.2, 0.3, 0.4];
                c
            },
            triggered: true,
            partial: false,
            raw: None,
            la: None,
            pairs: false,
        };
        crate::store::wave_bin(&rec, &m)
    };
    let bin = dir.join("ref4.bin");
    std::fs::write(&bin, &bin_src).unwrap();
    line(&mut s, &mut ui, &cv, ":REF:SLOT 4");
    line(&mut s, &mut ui, &cv, ":REF:IMP");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("0,"), "slot file should be the bin");
    line(&mut s, &mut ui, &cv, ":WAV:SOUR REF4");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:POIN?"), "3");

    let wfm = wfm_bytes();
    std::fs::write(dir.join("ref4.wfm"), &wfm).unwrap();
    line(&mut s, &mut ui, &cv, ":REF:IMP");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
    line(&mut s, &mut ui, &cv, ":WAV:SOUR R4");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:POIN?"), "4");
    assert_eq!(line(&mut s, &mut ui, &cv, ":REF:SHOW?"), "1");
    line(&mut s, &mut ui, &cv, ":REF:SHOW OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":REF:SHOW?"), "0");

    let bad = dir.join("nope.wfm");
    let mut raw = wfm.clone();
    raw[2..10].copy_from_slice(b":WFM#999");
    std::fs::write(&bad, &raw).unwrap();
    line(&mut s, &mut ui, &cv, &format!(":REF:LOAD \"{}\"", bad.display()));
    let err = line(&mut s, &mut ui, &cv, ":SYST:ERR?");
    assert!(err.contains("version 001, 002 or 003"), "{err}");

    line(&mut s, &mut ui, &cv, ":REF:CLE");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:POIN?"), "0");
    let _ = std::fs::remove_dir_all(&dir);
}

/// :REF:LOAD names any path on the scope. A file of the kernel's file systems is refused by where it resolves to
/// (before, /proc/kmsg passed the is_file check and its read never ended: the frame loop stood still), and a file
/// over the import limit by its size, before it is read.
#[test]
fn load_refuses_kernel_files_and_oversize_files_before_reading() {
    let (mut s, mut ui, cv) = session();
    let dir = crate::ui::testkit::Scratch::new("ref-limits");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("lane.csv");
    std::fs::write(&csv, "Time (s),Volts\n0,0.1\n1e-9,0.4\n").unwrap();
    // a regular file under /proc or /dev: Linux has /proc; macOS shows an open regular file as /dev/fd/N
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let open = std::fs::File::open(&csv).unwrap();
        let kernel = if cfg!(target_os = "linux") {
            std::path::PathBuf::from("/proc/self/status")
        } else {
            use std::os::fd::AsRawFd;
            std::path::PathBuf::from(format!("/dev/fd/{}", open.as_raw_fd()))
        };
        assert!(kernel.is_file());
        line(&mut s, &mut ui, &cv, &format!(":REF:SLOT R1;:REF:LOAD \"{}\"", kernel.display()));
        let err = line(&mut s, &mut ui, &cv, ":SYST:ERR?");
        assert!(err.starts_with("-224,") && err.contains("/proc, /sys and /dev"), "{err}");
        assert!(ui.ref_shown_slot(0).is_none());
    }
    let big = dir.join("big.csv");
    std::fs::File::create(&big).unwrap().set_len(crate::store::MAX_WAVEFORM_BYTES as u64 + 1).unwrap();
    line(&mut s, &mut ui, &cv, &format!(":REF:LOAD \"{}\"", big.display()));
    let err = line(&mut s, &mut ui, &cv, ":SYST:ERR?");
    assert!(err.starts_with("-224,") && err.contains("big.csv exceeds the 128 MiB"), "{err}");
    line(&mut s, &mut ui, &cv, &format!(":REF:LOAD \"{}\"", dir.display()));
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-256,"), "a directory is no file");
    line(&mut s, &mut ui, &cv, &format!(":REF:LOAD \"{}\"", csv.display()));
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
    assert!(ui.ref_shown_slot(0).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn probewright_queries_answer_and_the_rigol_pages_round_trip() {
    let (mut s, mut ui, cv) = session();
    assert_eq!(line(&mut s, &mut ui, &cv, "*ESR?"), "0", "OOS starts its implemented status subset clear");
    assert_eq!(line(&mut s, &mut ui, &cv, "*ESR?"), "0");
    let unknown = line(&mut s, &mut ui, &cv, ":REFerence:NOPE?");
    assert!(unknown.starts_with("-113,"), "a missing query must answer, got {unknown:?}");
    assert_eq!(line(&mut s, &mut ui, &cv, "*ESR?"), "32");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-113,"));
    line(&mut s, &mut ui, &cv, "*CLS");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");

    line(&mut s, &mut ui, &cv, ":CHAN1:DISP ON;:CHAN1:SCAL 0.5;:CHAN1:OFFS -0.25;:CHAN1:COUP AC;:CHAN1:BWL 20M");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CHAN1:DISP?;:CHAN1:COUP?;:CHAN1:BWL?"), "1;AC;20M");
    let scale: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:SCAL?").parse().unwrap();
    assert!((scale - 0.5).abs() < 1e-6, "{scale}");
    line(&mut s, &mut ui, &cv, ":CHAN1:PROB 10");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CHAN1:PROB?"), "1.000000E+01");
    line(&mut s, &mut ui, &cv, ":TIM:MAIN:SCAL 1e-3;:TIM:MAIN:OFFS 2e-3;:TRIG:SWE NORM;:TRIG:EDGE:SOUR CHAN1;:TRIG:EDGE:LEV 0.1;:TRIG:EDGE:SLOP NEG");
    assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:SWE?;:TRIG:EDGE:SOUR?;:TRIG:EDGE:SLOP?"), "NORM;CHAN1;NEG");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:ITEM? VPP,CHAN1"), "9.9E37");

    let png = s.exec_line(&mut ui, &cv, ":DISP:DATA? PNG");
    assert!(png.starts_with(b"#9"), "IEEE block");
    assert_eq!(&png[11..15], b"\x89PNG");
    let bmp = s.exec_line(&mut ui, &cv, ":DISP:DATA?");
    assert_eq!(&bmp[11..13], b"BM");

    line(&mut s, &mut ui, &cv, ":WAV:MODE MAX;:WAV:FORM WORD;:WAV:STAR 1;:WAV:STOP 1000");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:MODE?;:WAV:FORM?;:WAV:STAR?;:WAV:STOP?"), "MAX;WORD;1;1000");
}

#[test]
fn reference_commands_and_a_raw_word_page() {
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":REF:CURR 1;:REF:SOUR 1,CHAN2;:REF:VSC 1,0.2;:REF:VOFF 1,-0.05;:REF:LAB:CONT 1,lane");
    assert_eq!(line(&mut s, &mut ui, &cv, ":REF:CURR?;:REF:SOUR? 1;:REF:LAB:CONT? 1"), "1;CHAN2;lane");
    let vsc: f64 = line(&mut s, &mut ui, &cv, ":REF:VSC? 1").parse().unwrap();
    let voff: f64 = line(&mut s, &mut ui, &cv, ":REF:VOFF? 1").parse().unwrap();
    assert!((vsc - 0.2).abs() < 1e-6 && (voff + 0.05).abs() < 1e-6, "{vsc} {voff}");
    line(&mut s, &mut ui, &cv, ":REF:SHOW 1,OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":REF:SHOW? 1"), "0");
    line(&mut s, &mut ui, &cv, ":REF:SHOW 1,ON");

    line(&mut s, &mut ui, &cv, ":WAV:SOUR R1;:WAV:MODE RAW;:WAV:FORM WORD;:WAV:STAR 10;:WAV:STOP 12");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:POIN?"), "3");
    let data = s.exec_line(&mut ui, &cv, ":WAV:DATA?");
    assert!(data.starts_with(b"#9"), "{:?}", &data[..data.len().min(16)]);
    let n: usize = std::str::from_utf8(&data[2..11]).unwrap().parse().unwrap();
    assert_eq!(n, 6, "three little-endian WORD points");
    assert_eq!(data.len(), 11 + 6);

    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 100;:SING");
    ui.step(0.2);
    assert_eq!(captures(&ui), 0.0);
    line(&mut s, &mut ui, &cv, ":TFOR");
    ui.step(0.2);
    assert_eq!(captures(&ui), 1.0, "TFORce must release a waiting Single");
    assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:STAT?"), "STOP");
}

#[test]
fn cursors_and_a_readout_round_trip() {
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":TIM:MAIN:SCAL 1e-6;:TIM:MAIN:OFFS 0;:REF:VSC 1,1;:REF:VOFF 1,0");
    line(
        &mut s,
        &mut ui,
        &cv,
        ":CURS:MODE MAN;:CURS:MAN:TYPE TIME;:CURS:MAN:SOUR R1;:CURS:MAN:CAX 0;:CURS:MAN:CBX 1e-6",
    );
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MODE?;:CURS:MAN:TYPE?;:CURS:MAN:SOUR?"), "MAN;TIME;R1");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MAN:CAX?"), "0.000000E+00");
    let dx: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:XDEL?").parse().unwrap();
    let inv: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:IXD?").parse().unwrap();
    assert!((dx - 1e-6).abs() < 1e-9, "{dx}");
    assert!((inv - 1e6).abs() < 1.0, "{inv}");

    line(&mut s, &mut ui, &cv, ":CURS:MAN:TYPE AMPL;:CURS:MAN:CAY 0.5;:CURS:MAN:CBY -0.5");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MODE?;:CURS:MAN:TYPE?"), "MAN;AMPL");
    let ay: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:AYV?").parse().unwrap();
    let dy: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:YDEL?").parse().unwrap();
    assert!((ay - 0.5).abs() < 1e-4, "{ay}");
    assert!((dy + 1.0).abs() < 1e-3, "{dy}");

    line(&mut s, &mut ui, &cv, ":CURS:MODE XY;:CURS:XY:AX 1e-6;:CURS:XY:AY 0.25");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MODE?"), "XY");
    let ax: f64 = line(&mut s, &mut ui, &cv, ":CURS:XY:AX?").parse().unwrap();
    let y: f64 = line(&mut s, &mut ui, &cv, ":CURS:XY:AY?").parse().unwrap();
    assert!((ax - 1e-6).abs() < 1e-9 && (y - 0.25).abs() < 1e-4, "{ax} {y}");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MODE OFF;:CURS:MAN:XDEL?"), "9.9E37");

    line(&mut s, &mut ui, &cv, ":MEAS:CLE;:MEAS:SOUR R1;:MEAS:ITEM VPP;:MEAS:ITEM RRDELAY,CHAN1,CHAN2");
    let strip = line(&mut s, &mut ui, &cv, ":MEAS:STR?");
    assert!(strip.contains("VPP,R1,"), "{strip}");
    assert!(strip.contains("RRDELAY,CHAN1,CHAN2,"), "{strip}");
    let vpp: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? VPP").parse().unwrap();
    assert!((vpp - 1.0).abs() < 1e-3, "the saved square is 1 Vpp, got {vpp}");
    line(&mut s, &mut ui, &cv, ":MEAS:CLE");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:STR?"), "");

    line(
        &mut s,
        &mut ui,
        &cv,
        ":DVM:ENAB ON;:DVM:SOUR CHAN2;:DVM:MODE ACRMS;:COUN:ENAB ON;:COUN:MODE PER;:COUN:NDIG 6",
    );
    assert_eq!(
        line(&mut s, &mut ui, &cv, ":DVM:ENAB?;:DVM:SOUR?;:DVM:MODE?;:COUN:ENAB?;:COUN:MODE?;:COUN:NDIG?"),
        "1;CHAN2;ACRMS;1;PER;6"
    );
    assert!(!line(&mut s, &mut ui, &cv, ":DVM:CURR?;:COUN:CURR?").is_empty());
    line(&mut s, &mut ui, &cv, ":COUN:TOT:CLE");
}

#[test]
fn raw_tcp_empty_query_is_a_line_and_compound_query_keeps_its_position() {
    let (mut scpi, mut ui, canvas) = session();
    line(&mut scpi, &mut ui, &canvas, ":MEAS:CLEAR");
    let mut client = TcpStream::connect(("127.0.0.1", scpi.port().unwrap())).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    client.write_all(b":MEAS:STRIP?\n").unwrap();
    for _ in 0..50 {
        scpi.poll(&mut ui, &canvas);
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut byte = [0];
    client.read_exact(&mut byte).unwrap();
    assert_eq!(&byte, b"\n");
    client.write_all(b":MEAS:STRIP?;*OPC?\n*ESR?\n").unwrap();
    for _ in 0..50 {
        scpi.poll(&mut ui, &canvas);
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut reply = [0; 5];
    client.read_exact(&mut reply).unwrap();
    assert_eq!(&reply, b";1\n0\n");
    assert_eq!(scpi.exec_message_checked(&mut ui, &canvas, ":MEAS:STRIP?"), Ok((Vec::new(), true)));
    assert_eq!(scpi.exec_message_checked(&mut ui, &canvas, ":STOP"), Ok((Vec::new(), false)));
}

#[test]
fn cursor_setters_preserve_genuine_statistics_and_pending_single() {
    let (mut scpi, mut ui, canvas) = session();
    for _ in 0..4 {
        ui.step(0.05);
    }
    let statistics = ui.measurement_statistics_snapshot_for_test();
    assert!(statistics.iter().any(|&(count, _)| count >= 3));
    let record = ui.record_volts(0).to_vec();
    ui.m.trig_level = 100.0;
    ui.key(crate::ui::key::SINGLE);
    let stamp = ui.m.hw_stamp;
    let source = ui.m.trigger_wave();
    let mut waiting = Program::new("*OPC?;:RUN");
    assert!(scpi.poll_program(&mut ui, &canvas, &mut waiting).is_none());
    for command in [
        ":CURS:MODE MAN",
        ":CURS:MAN:TYPE TIME",
        ":CURS:MAN:SOUR CHAN1",
        ":CURS:MAN:CAX 0",
        ":CURS:MAN:CBX 1e-8",
        ":CURS:MAN:TYPE AMPL",
        ":CURS:MAN:CAY 0.2",
        ":CURS:MAN:CBY -0.2",
        ":CURS:MODE XY",
        ":CURS:XY:AX 0",
        ":CURS:XY:AY 0.1",
        ":CURS:XY:BX 1e-8",
        ":CURS:XY:BY -0.1",
        ":CURS:MODE TRAC",
        ":CURS:TRAC:SOUR2 CHAN1",
        ":CURS:MODE OFF",
    ] {
        line(&mut scpi, &mut ui, &canvas, command);
        assert_eq!(ui.measurement_statistics_snapshot_for_test(), statistics, "{command}");
        assert_eq!(ui.record_volts(0), record, "{command}");
        assert_eq!(ui.m.hw_stamp, stamp, "{command}");
        assert_eq!(ui.m.trigger_wave(), source);
        assert_eq!(ui.m.trig_mode, TrigMode::Single);
        assert!(scpi.poll_program(&mut ui, &canvas, &mut waiting).is_none(), "{command}");
    }
    assert_eq!(line(&mut scpi, &mut ui, &canvas, ":SYST:ERR?"), "0,\"No error\"");
}

#[test]
fn waveform_word_is_little_endian_and_page_metadata_has_the_page_origin() {
    let (mut scpi, mut ui, canvas) = session();
    let volts = |code: u16| (code as f64 - 32768.0) * 8.0 / 60000.0;
    ui.commit_waveform_traces(
        vec![crate::store::WaveTrace {
            v: vec![0.0, volts(0x8123) as f32, volts(0x2345) as f32, 0.0],
            dt: 5e-12,
            t0: -2e-9,
            label: "Known WORD codes".into(),
        }],
        "wire-policy.csv",
        true,
    )
    .unwrap();
    line(
        &mut scpi,
        &mut ui,
        &canvas,
        ":REF:VSC 1,1;:REF:VOFF 1,0;:WAV:SOUR R1;:WAV:MODE RAW;:WAV:FORM WORD;:WAV:STAR 2;:WAV:STOP 3",
    );
    assert_eq!(scpi.exec_line(&mut ui, &canvas, ":WAV:DATA?"), b"#9000000004\x23\x81\x45\x23");
    let preamble = line(&mut scpi, &mut ui, &canvas, ":WAV:PRE?");
    let values: Vec<_> = preamble.split(',').collect();
    assert_eq!(&values[..4], &["1", "2", "2", "1"]);
    assert!((values[4].parse::<f64>().unwrap() - 5e-12).abs() < 1e-18);
    assert!((values[5].parse::<f64>().unwrap() - (-2e-9 + 5e-12)).abs() < 1e-15);
    line(&mut scpi, &mut ui, &canvas, ":WAV:MODE MAX;:STOP");
    assert_eq!(line(&mut scpi, &mut ui, &canvas, ":WAV:PRE?").split(',').nth(1), Some("1"));
    line(&mut scpi, &mut ui, &canvas, ":RUN");
    assert_eq!(line(&mut scpi, &mut ui, &canvas, ":WAV:PRE?").split(',').nth(1), Some("1"));
}

#[test]
fn query_error_stays_specific_when_the_bounded_error_queue_is_full() {
    let (mut scpi, mut ui, canvas) = session();
    for _ in 0..ERR_MAX {
        scpi.push_err("-113,\"Old command error\"".into());
    }
    let error = line(&mut scpi, &mut ui, &canvas, ":REF:VSC? 10");
    assert!(error.starts_with("-230,"), "{error}");
    assert_eq!(scpi.err.len(), ERR_MAX);
    assert!(scpi.err.last().unwrap().starts_with("-350,"));
    assert_eq!(line(&mut scpi, &mut ui, &canvas, "*ESR?;*ESR?"), "56;0", "command, execution and queue-overflow bits");
    line(&mut scpi, &mut ui, &canvas, ":SYST:ERR?");
    assert_eq!(line(&mut scpi, &mut ui, &canvas, "*ESR?"), "0", "reading old errors does not relatch ESR");
    let huge = format!(":{}?", "é".repeat(2000));
    let reply = scpi.exec_line(&mut ui, &canvas, &huge);
    assert!(reply.len() <= ERR_LEN);
    assert!(std::str::from_utf8(&reply).unwrap().starts_with("-113,"));
}

#[test]
fn additional_action_queries_never_start_calibration_bode_or_force() {
    let (mut scpi, mut ui, canvas) = reference_session();
    let model = crate::store::setup_text(&ui.m);
    let workflow = ui.workflow_state();
    for query in [":TFOR?", ":TRIG:FORC?", ":CAL:STAR?", ":CAL:CANC?", ":BODE:RUN?", ":REF:CLEAR?", ":REF:SAVE?"] {
        assert!(line(&mut scpi, &mut ui, &canvas, query).starts_with("-113,"), "{query}");
        assert_eq!(crate::store::setup_text(&ui.m), model);
        assert_eq!(ui.workflow_state(), workflow);
    }
    let old = ui.meas_pair_sources();
    line(&mut scpi, &mut ui, &canvas, ":MEAS:SOUR R1,WRONG");
    assert_eq!(ui.meas_pair_sources(), old, "invalid secondary source cannot partially apply");
}

#[test]
fn manual_cursors_keep_time_and_voltage_together() {
    let (mut s, mut ui, cv) = session();
    line(&mut s, &mut ui, &cv, ":TIM:MAIN:SCAL 1e-6;:TIM:MAIN:OFFS 0;:CHAN1:SCAL 1;:CHAN1:OFFS 0");
    line(
        &mut s,
        &mut ui,
        &cv,
        ":CURS:MODE MAN;:CURS:MAN:CAX 0;:CURS:MAN:CBX 1e-6;:CURS:MAN:CAY 0.5;:CURS:MAN:CBY -0.5",
    );
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MODE?"), "XY", "both axes are the screen's XY cursors");
    let ax: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:CAX?").parse().unwrap();
    let bx: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:CBX?").parse().unwrap();
    let ay: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:CAY?").parse().unwrap();
    let by: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:CBY?").parse().unwrap();
    assert!((ax).abs() < 1e-9 && (bx - 1e-6).abs() < 1e-9, "times {ax} {bx}");
    assert!((ay - 0.5).abs() < 1e-4 && (by + 0.5).abs() < 1e-4, "volts {ay} {by}");
    line(&mut s, &mut ui, &cv, ":CURS:MODE OFF;:CURS:MAN:TYPE AMPL;:CURS:MAN:CAY 0.25;:CURS:MAN:CAX 0");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MODE?"), "XY");
    let kept: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:CAY?").parse().unwrap();
    let moved: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:CAX?").parse().unwrap();
    assert!((kept - 0.25).abs() < 1e-4 && moved.abs() < 1e-9, "voltage survived the time cursor: {kept} {moved}");
}

#[test]
fn track_cursor_b_follows_its_own_channel() {
    let (mut s, mut ui, cv) = session();
    line(
        &mut s,
        &mut ui,
        &cv,
        "*CLS;:CURS:MODE TRAC;:CURS:TRAC:SOUR1 CHAN1;:CURS:TRAC:SOUR2 CHAN2;:CURS:TRAC:CAX 0;:CURS:TRAC:CBX 1e-6",
    );
    for _ in 0..4 {
        ui.step(0.05);
    }
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:TRAC:SOUR1?;:CURS:TRAC:SOUR2?"), "CHAN1;CHAN2");
    let ay = line(&mut s, &mut ui, &cv, ":CURS:TRAC:AYV?");
    let by = line(&mut s, &mut ui, &cv, ":CURS:TRAC:BYV?");
    assert!(ay.parse::<f64>().unwrap().is_finite() && by.parse::<f64>().unwrap().is_finite(), "{ay} {by}");
    line(&mut s, &mut ui, &cv, "*CLS;:CURS:MODE MAN;:CURS:MAN:SOUR2 CHAN2");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224"), "manual mode keeps one source");
}

#[test]
fn statistics_channel_labels_and_the_zoom_window_round_trip() {
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":CHAN1:LAB:CONT Vbus;:CHAN1:UNIT AMP;:CHAN1:VERN ON;:CHAN1:POS 0.2");
    assert_eq!(
        line(&mut s, &mut ui, &cv, ":CHAN1:LAB:CONT?;:CHAN1:LAB:SHOW?;:CHAN1:UNIT?;:CHAN1:VERN?"),
        "Vbus;1;AMP;1"
    );
    let pos: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:OFFS?").parse().unwrap();
    assert!((pos - 0.2).abs() < 1e-4, "{pos}");
    line(&mut s, &mut ui, &cv, ":CHAN1:LAB:SHOW OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CHAN1:LAB:SHOW?;:CHAN1:LAB:CONT?"), "0;");

    line(&mut s, &mut ui, &cv, ":TIM:MAIN:SCAL 1e-3;:TIM:HREF:MODE TRIG;:TIM:VERN ON;:TIM:DEL:ENAB ON;:TIM:DEL:SCAL 1e-4;:TIM:DEL:OFFS 0;:TIM:XY:ENAB ON");
    assert_eq!(line(&mut s, &mut ui, &cv, ":TIM:HREF:MODE?;:TIM:VERN?;:TIM:DEL:ENAB?;:TIM:XY:ENAB?"), "TRIG;1;1;1");
    let z: f64 = line(&mut s, &mut ui, &cv, ":TIM:DEL:SCAL?").parse().unwrap();
    assert!((z - 1e-4).abs() < 1e-8, "{z}");

    line(&mut s, &mut ui, &cv, ":MEAS:CLE;:MEAS:STAT:DISP ON;:MEAS:STAT:ITEM VPP,R1;:RUN");
    ui.step(0.25);
    ui.step(0.25);
    let current: f64 = line(&mut s, &mut ui, &cv, ":MEAS:STAT:ITEM? CURR,VPP,R1").parse().unwrap();
    let mean: f64 = line(&mut s, &mut ui, &cv, ":MEAS:STAT:ITEM? AVER,VPP,R1").parse().unwrap();
    let n: f64 = line(&mut s, &mut ui, &cv, ":MEAS:STAT:ITEM? CNT,VPP,R1").parse().unwrap();
    assert!((current - 1.0).abs() < 1e-3, "{current}");
    assert!((mean - 1.0).abs() < 1e-3 && n >= 2.0, "mean {mean} count {n}");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:STAT:DISP?;:MEAS:STR?"), "1;VPP,R1,1.000000E+00");
    line(&mut s, &mut ui, &cv, ":MEAS:STAT:RES");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:STAT:ITEM? CNT,VPP,R1"), "0.000000E+00");
}

#[test]
fn measurement_gate_changes_burst_outside_and_duration() {
    let (mut s, mut ui, cv) = session();
    line(&mut s, &mut ui, &cv, ":TIM:SCAL 1e-3");
    for _ in 0..8 {
        ui.step(0.05);
    }
    line(&mut s, &mut ui, &cv, "*CLS;:MEAS:BURS:IDLE 0;:MEAS:OUTS BOTH;:MEAS:NPER 1;:MEAS:EDG CLOC");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:OUTS?;:MEAS:NPER?;:MEAS:EDG?"), "BOTH;1;CLOC");
    let idle: f64 = line(&mut s, &mut ui, &cv, ":MEAS:BURS:IDLE?").parse().unwrap();
    assert!(idle.abs() < 1e-15, "{idle}");
    let one: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? NPER,CHAN1").parse().unwrap();
    line(&mut s, &mut ui, &cv, ":MEAS:NPER 2");
    let two: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? NPER,CHAN1").parse().unwrap();
    assert!(one.is_finite() && one < 1e37 && (two / one - 2.0).abs() < 0.15, "one {one} two {two}");
    line(&mut s, &mut ui, &cv, ":MEAS:NPER 1;:MEAS:EDG DATA");
    let data: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? NPER,CHAN1").parse().unwrap();
    assert!((data / one - 0.5).abs() < 0.15, "clock {one} data {data}");
    let burst: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? BWID,CHAN1").parse().unwrap();
    assert!(burst.is_finite() && burst < 1e37, "{burst}");
    line(&mut s, &mut ui, &cv, ":MEAS:BURS:IDLE 1e-6");
    let split: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? BWID,CHAN1").parse().unwrap();
    assert!(split > 1e37, "a gap shorter than the half period splits the burst, {split}");
    let mut csv = String::from("Time (s),Volts\n");
    for index in 0..2000 {
        let value = if index % 100 < 10 { 1.0 } else { 0.0 };
        csv.push_str(&format!("{},{}\n", index as f64 * 1e-6, value));
    }
    ui.load_waveform_bytes(csv.as_bytes(), "pulse.csv").unwrap();
    line(&mut s, &mut ui, &cv, ":MEAS:OUTS ABOV");
    let above: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? TOUT,R1").parse().unwrap();
    line(&mut s, &mut ui, &cv, ":MEAS:OUTS BEL");
    let below: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? TOUT,R1").parse().unwrap();
    assert!(above > 0.0 && below > above * 5.0, "a 10 % pulse is high briefly, above {above} below {below}");
    line(&mut s, &mut ui, &cv, ":MEAS:NPER 0;:MEAS:BURS:IDLE -1;:MEAS:EDG BOTH");
    let err = line(&mut s, &mut ui, &cv, ":SYST:ERR?");
    assert!(err.starts_with("-224,"), "{err}");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:NPER?;:MEAS:EDG?"), "1;DATA");
}

#[test]
fn channel_zero_bias_shifts_the_reading() {
    let (mut s, mut ui, cv) = session();
    for _ in 0..8 {
        ui.step(0.05);
    }
    let before: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? VAVG,CHAN1").parse().unwrap();
    assert!(before.is_finite() && before < 1e37, "{before}");
    line(&mut s, &mut ui, &cv, ":CHAN1:BIAS 0.25");
    for _ in 0..4 {
        ui.step(0.05);
    }
    let after: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? VAVG,CHAN1").parse().unwrap();
    assert!((before - after - 0.25).abs() < 0.05, "before {before} after {after}");
}

#[test]
fn acquisition_deskew_reference_color_and_thresholds_round_trip() {
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":ACQ:TYPE AVER;:ACQ:AVER 100;:ACQ:ULTR:MAXF 100");
    assert_eq!(line(&mut s, &mut ui, &cv, ":ACQ:TYPE?;:ACQ:AVER?;:ACQ:ULTR:MAXF?"), "AVER;64;100");
    line(&mut s, &mut ui, &cv, ":ACQ:TYPE HRES;:ACQ:ULTR:MAXF 0");
    assert_eq!(line(&mut s, &mut ui, &cv, ":ACQ:TYPE?;:ACQ:ULTR:MAXF?"), "HRES;0");

    line(&mut s, &mut ui, &cv, ":CHAN1:BIAS 0.25");
    let bias: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:BIAS?").parse().unwrap();
    assert!((bias - 0.25).abs() < 1e-6, "{bias}");
    line(&mut s, &mut ui, &cv, "*CLS;:CHAN1:BIAS nan");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"), "a non-finite zero bias is refused");
    let held_bias: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:BIAS?").parse().unwrap();
    assert!((held_bias - 0.25).abs() < 1e-6, "{held_bias}");
    line(&mut s, &mut ui, &cv, ":CHAN1:BIAS 0");

    line(&mut s, &mut ui, &cv, "*CLS;:CHAN1:OFFS 0.5;:CHAN1:SCAL 1;:CHAN1:EXP GND");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CHAN1:EXP?;:CHAN2:EXP?"), "GND;GND");
    line(&mut s, &mut ui, &cv, ":CHAN1:SCAL 2");
    let grown: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:OFFS?").parse().unwrap();
    assert!((grown - 1.0).abs() < 1e-4, "ground expand keeps the zero in place, offset {grown}");
    line(&mut s, &mut ui, &cv, ":CHAN1:EXP CENT;:CHAN1:SCAL 1");
    let held: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:OFFS?").parse().unwrap();
    assert!((held - 1.0).abs() < 1e-4, "center expand leaves the offset, {held}");
    line(&mut s, &mut ui, &cv, ":CHAN1:EXP FOO");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"));
    assert_eq!(line(&mut s, &mut ui, &cv, ":CHAN1:EXP?"), "CENT");

    line(&mut s, &mut ui, &cv, ":CHAN1:TCAL 2e-8");
    let skew: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:TCAL?").parse().unwrap();
    assert!((skew - 2e-8).abs() < 1e-12, "{skew}");
    line(&mut s, &mut ui, &cv, ":TIM:SCAL 2e-5;:CHAN1:TCAL 1e-9");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"), "a slow time base refuses a deskew change");
    let held: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:TCAL?").parse().unwrap();
    assert!((held - 2e-8).abs() < 1e-12, "{held}");

    line(&mut s, &mut ui, &cv, ":REF:VSC 1,0.5;:REF:VOFF 1,0.1;:REF:RES 1;:REF:COL 1,RED");
    let vsc: f64 = line(&mut s, &mut ui, &cv, ":REF:VSC? 1").parse().unwrap();
    let voff: f64 = line(&mut s, &mut ui, &cv, ":REF:VOFF? 1").parse().unwrap();
    assert!((vsc - 1.0).abs() < 1e-6 && voff.abs() < 1e-9, "reset scale {vsc} offset {voff}");
    assert_eq!(line(&mut s, &mut ui, &cv, ":REF:COL? 1"), "RED");

    line(&mut s, &mut ui, &cv, "*CLS;:MEAS:SET:PSA CHAN2;:MEAS:SET:PSB R1");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:SET:DSA?;:MEAS:SET:DSB?;:MEAS:SOUR?"), "CHAN2;R1;CHAN2");
    line(&mut s, &mut ui, &cv, ":MEAS:SET:PSA MATH1");
    assert!(
        line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"),
        "a measurement source is a channel or a reference"
    );
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:SOUR?"), "CHAN2");

    line(&mut s, &mut ui, &cv, ":MEAS:SET:MIN 20;:MEAS:SET:MAX 80");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:SET:MIN?;:MEAS:SET:MAX?"), "20;80");
    line(&mut s, &mut ui, &cv, ":MEAS:SET:MAX 10");
    assert!(
        line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"),
        "the upper threshold must stay above the lower"
    );
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:SET:MAX?"), "80");

    // the runt thresholds the same way (review U6)
    line(&mut s, &mut ui, &cv, "*CLS;:TRIG:TYPE RUNT;:TRIG:RUNT:UPP 1.5;:TRIG:RUNT:LOW 0.25");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("0,"), "thresholds in order are taken");
    line(&mut s, &mut ui, &cv, ":TRIG:RUNT:LOW 2");
    let error = line(&mut s, &mut ui, &cv, ":SYST:ERR?");
    assert!(error.starts_with("-224,"), "the lower threshold must stay below the upper: {error}");
    line(&mut s, &mut ui, &cv, ":TRIG:RUNT:UPP 0.1");
    let error = line(&mut s, &mut ui, &cv, ":SYST:ERR?");
    assert!(error.starts_with("-224,"), "the upper threshold must stay above the lower: {error}");
    let upper: f64 = line(&mut s, &mut ui, &cv, ":TRIG:RUNT:UPP?").parse().unwrap();
    let lower: f64 = line(&mut s, &mut ui, &cv, ":TRIG:RUNT:LOW?").parse().unwrap();
    assert!((upper - 1.5).abs() < 1e-6 && (lower - 0.25).abs() < 1e-6, "{upper} {lower}");
}

#[test]
fn a_mask_counts_passes_and_failures() {
    let (mut s, mut ui, cv) = session();
    for _ in 0..20 {
        ui.step(0.05);
    }
    line(&mut s, &mut ui, &cv, "*CLS;:MASK:SOUR CHAN1;:MASK:X 0.5;:MASK:Y 0.5;:MASK:ENAB ON;:MASK:CRE;:MASK:OPER RUN");
    assert_eq!(
        line(&mut s, &mut ui, &cv, ":SYST:ERR?"),
        "0,\"No error\"",
        "the demo record must be there to build a mask"
    );
    assert_eq!(line(&mut s, &mut ui, &cv, ":MASK:ENAB?;:MASK:SOUR?;:MASK:OPER?"), "1;CHAN1;RUN");
    let x: f64 = line(&mut s, &mut ui, &cv, ":MASK:X?").parse().unwrap();
    let y: f64 = line(&mut s, &mut ui, &cv, ":MASK:Y?").parse().unwrap();
    assert!((x - 0.5).abs() < 1e-6 && (y - 0.5).abs() < 1e-6, "{x} {y}");
    for _ in 0..10 {
        ui.step(0.05);
    }
    let passed: u64 = line(&mut s, &mut ui, &cv, ":MASK:PASS?").parse().unwrap();
    let failed: u64 = line(&mut s, &mut ui, &cv, ":MASK:FAIL?").parse().unwrap();
    let total: u64 = line(&mut s, &mut ui, &cv, ":MASK:TOT?").parse().unwrap();
    assert_eq!(total, passed + failed);
    assert!(total > 0, "a running mask must count frames, pass {passed} fail {failed}");
    line(&mut s, &mut ui, &cv, ":MASK:RES");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MASK:PASS?;:MASK:FAIL?;:MASK:TOT?"), "0;0;0");
    line(&mut s, &mut ui, &cv, ":MASK:OPER STOP");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MASK:OPER?"), "STOP");
}

#[test]
fn histogram_type_source_and_result() {
    let (mut s, mut ui, cv) = session();
    for _ in 0..8 {
        ui.step(0.05);
    }
    line(&mut s, &mut ui, &cv, "*CLS;:HIST:ENAB ON;:HIST:TYPE VERT;:HIST:SOUR CHAN1");
    assert_eq!(line(&mut s, &mut ui, &cv, ":HIST:ENAB?;:HIST:TYPE?;:HIST:SOUR?"), "1;VERT;CHAN1");
    line(&mut s, &mut ui, &cv, ":HIST:TYPE HOR");
    assert_eq!(line(&mut s, &mut ui, &cv, ":HIST:TYPE?"), "HOR");
    let result = line(&mut s, &mut ui, &cv, ":HIST:STAT:RES?");
    let sum: u64 = result.split([',', ':']).nth(1).unwrap_or("0").parse().unwrap_or(0);
    assert!(sum > 0 && result.contains("Mean:") && result.contains("Sigma:"), "{result}");
    line(&mut s, &mut ui, &cv, ":HIST:ENAB OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":HIST:ENAB?"), "0");
    let off = line(&mut s, &mut ui, &cv, ":HIST:STAT:RES?");
    assert!(off.starts_with("Sum:0,"), "{off}");
}

#[test]
fn a_math_trace_sets_operator_scale_fft_and_filter() {
    let (mut s, mut ui, cv) = session();
    line(
        &mut s,
        &mut ui,
        &cv,
        "*CLS;:MATH1:DISP ON;:MATH1:OPER ADD;:MATH1:SOUR1 CHAN1;:MATH1:SOUR2 CHAN2;:MATH1:SCAL 0.2;:MATH1:OFFS 1",
    );
    assert_eq!(
        line(&mut s, &mut ui, &cv, ":MATH1:DISP?;:MATH1:OPER?;:MATH1:SOUR1?;:MATH1:SOUR2?"),
        "1;ADD;CHAN1;CHAN2"
    );
    let scale: f64 = line(&mut s, &mut ui, &cv, ":MATH1:SCAL?").parse().unwrap();
    let offset: f64 = line(&mut s, &mut ui, &cv, ":MATH1:OFFS?").parse().unwrap();
    assert!((scale - 0.2).abs() < 1e-9 && (offset - 1.0).abs() < 1e-6, "{scale} {offset}");
    line(&mut s, &mut ui, &cv, ":MATH2:OPER INTG;:MATH2:SOUR1 MATH1");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MATH2:OPER?;:MATH2:SOUR1?"), "INTG;MATH1");
    line(&mut s, &mut ui, &cv, ":MATH1:SOUR1 MATH1");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224"), "a trace cannot use itself");
    line(&mut s, &mut ui, &cv, ":MATH1:FFT:SOUR CHAN1;:MATH1:FFT:WIND BLAC;:MATH1:FFT:SCAL 10");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MATH1:OPER?;:MATH1:FFT:SOUR?;:MATH1:FFT:WIND?"), "FFT;CHAN1;BLAC");
    let db: f64 = line(&mut s, &mut ui, &cv, ":MATH1:FFT:SCAL?").parse().unwrap();
    assert!((db - 10.0).abs() < 1e-9, "{db}");
    line(&mut s, &mut ui, &cv, ":MATH1:FILT:TYPE BPAS;:MATH1:FILT:W1 1000;:MATH1:FILT:W2 3000");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MATH1:FILT:TYPE?;:MATH1:OPER?"), "BPAS;BPAS");
    let w1: f64 = line(&mut s, &mut ui, &cv, ":MATH1:FILT:W1?").parse().unwrap();
    let w2: f64 = line(&mut s, &mut ui, &cv, ":MATH1:FILT:W2?").parse().unwrap();
    assert!((w1 - 1000.0).abs() < 1e-3 && (w2 - 3000.0).abs() < 1e-3, "{w1} {w2}");
    line(&mut s, &mut ui, &cv, ":MATH1:EXPR \"2*CH1-CH2\"");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MATH1:EXPR?;:MATH1:OPER?"), "2*CH1-CH2;EXPR");
    line(&mut s, &mut ui, &cv, ":MATH1:DISP OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MATH1:DISP?;:MATH1:OPER?"), "0;OFF");
}

#[test]
fn a_bus_sets_uart_spi_and_reads_the_event_block() {
    let (mut s, mut ui, cv) = session();
    line(&mut s, &mut ui, &cv, "*CLS;:BUS1:MODE RS232;:BUS1:DISP ON;:BUS1:FORM ASC;:BUS1:EVEN ON");
    line(&mut s, &mut ui, &cv, ":BUS1:RS232:TX CHAN1;:BUS1:RS232:RX CHAN2;:BUS1:RS232:BAUD 9600;:BUS1:RS232:PAR EVEN;:BUS1:RS232:DBIT 8;:BUS1:THR 1.25");
    assert_eq!(line(&mut s, &mut ui, &cv, ":BUS1:MODE?;:BUS1:DISP?;:BUS1:FORM?;:BUS1:EVEN?"), "RS232;1;ASC;1");
    assert_eq!(
        line(
            &mut s,
            &mut ui,
            &cv,
            ":BUS1:RS232:TX?;:BUS1:RS232:RX?;:BUS1:RS232:BAUD?;:BUS1:RS232:PAR?;:BUS1:RS232:DBIT?"
        ),
        "CHAN1;CHAN2;9600;EVEN;8"
    );
    let thr: f64 = line(&mut s, &mut ui, &cv, ":BUS1:THR?").parse().unwrap();
    assert!((thr - 1.25).abs() < 1e-6, "{thr}");
    line(&mut s, &mut ui, &cv, ":BUS2:MODE SPI;:BUS2:DISP ON;:BUS2:SPI:SCLK:SOUR CHAN2;:BUS2:SPI:MOSI:SOUR CHAN1;:BUS2:SPI:SS:SOUR CHAN3;:BUS2:SPI:DBIT 12");
    assert_eq!(
        line(
            &mut s,
            &mut ui,
            &cv,
            ":BUS2:MODE?;:BUS2:SPI:SCLK:SOUR?;:BUS2:SPI:MOSI:SOUR?;:BUS2:SPI:SS:SOUR?;:BUS2:SPI:DBIT?"
        ),
        "SPI;CHAN2;CHAN1;CHAN3;12"
    );
    assert_eq!(
        line(&mut s, &mut ui, &cv, ":BUS1:MODE?;:BUS1:RS232:BAUD?"),
        "RS232;9600",
        "the second bus must not replace the first"
    );
    line(&mut s, &mut ui, &cv, ":BUS2:SPI:SS:SOUR OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":BUS2:SPI:SS:SOUR?"), "OFF");
    let data = line(&mut s, &mut ui, &cv, ":BUS1:DATA?");
    assert!(data.starts_with("#9") && data.contains("RS232"), "{data}");
    line(&mut s, &mut ui, &cv, "*CLS;:BUS1:FORM DEC");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224"), "only hex and ASCII are on this screen");
}

#[test]
fn a_live_force_trigger_asks_the_hardware_thread_to_pulse() {
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.run = RunState::Stop;
    let mut s = Scpi::open(0);
    let cv = Canvas::new(crate::ui::W, crate::ui::H);
    line(&mut s, &mut ui, &cv, ":TFOR");
    assert!(shared.lock().unwrap().force, "the acquisition thread pulses the FPGA force bit");
    assert_eq!(ui.m.run, RunState::Run);
}

#[test]
fn display_type_persistence_brightness_and_grid() {
    let (mut s, mut ui, cv) = session();
    line(&mut s, &mut ui, &cv, "*CLS;:DISP:TYPE VECT;:DISP:GRAD:TIME 0.1;:DISP:WBR 50;:DISP:GRID HALF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":DISP:TYPE?;:DISP:GRAD:TIME?;:DISP:WBR?;:DISP:GRID?"), "VECT;0.1;50;HALF");
    line(&mut s, &mut ui, &cv, ":DISP:GRAD:TIME 0.2");
    assert_eq!(line(&mut s, &mut ui, &cv, ":DISP:GRAD:TIME?"), "0.1", "0.2 s snaps to the screen's 100 ms step");
    line(&mut s, &mut ui, &cv, ":DISP:GRAD:TIME INF;:DISP:TYPE DOTS;:DISP:GRID NONE;:DISP:WBR 7");
    assert_eq!(line(&mut s, &mut ui, &cv, ":DISP:GRAD:TIME?;:DISP:TYPE?;:DISP:GRID?;:DISP:WBR?"), "INF;DOTS;NONE;10");

    line(&mut s, &mut ui, &cv, "*CLS;:DISP:GBR 40;:DISP:RUL OFF;:DISP:COL ON");
    assert_eq!(line(&mut s, &mut ui, &cv, ":DISP:GBR?;:DISP:RUL?;:DISP:COL?"), "40;0;1");
    line(&mut s, &mut ui, &cv, ":DISP:GBR 3");
    assert_eq!(line(&mut s, &mut ui, &cv, ":DISP:GBR?"), "10", "grid brightness stays at the screen floor");
    line(&mut s, &mut ui, &cv, ":DISP:GBR 101");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"));
    assert_eq!(line(&mut s, &mut ui, &cv, ":DISP:GBR?"), "10");

    line(&mut s, &mut ui, &cv, "*CLS;:TIM:ROLL 0;:TIM:MODE XY;:TIM:XY:Z CHAN3");
    assert_eq!(
        line(&mut s, &mut ui, &cv, ":TIM:ROLL?;:TIM:MODE?;:TIM:XY:ENAB?;:TIM:XY:X?;:TIM:XY:Y?;:TIM:XY:Z?"),
        "0;XY;1;CHAN1;CHAN2;CHAN3"
    );
    line(&mut s, &mut ui, &cv, ":TIM:MODE ROLL");
    assert!(
        line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"),
        "roll is the auto-roll switch, not a timebase mode"
    );
    line(&mut s, &mut ui, &cv, ":TIM:XY:X CHAN2");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"), "the XY plot is CH1 against CH2");
    assert_eq!(line(&mut s, &mut ui, &cv, ":TIM:MODE?;:TIM:XY:X?;:TIM:XY:Z?"), "XY;CHAN1;CHAN3");
    line(&mut s, &mut ui, &cv, ":TIM:MODE MAIN;:TIM:XY:Z NONE");
    assert_eq!(line(&mut s, &mut ui, &cv, ":TIM:MODE?;:TIM:XY:ENAB?;:TIM:XY:Z?"), "MAIN;0;NONE");

    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:IND?"), "0");
    line(&mut s, &mut ui, &cv, ":MEAS:IND ON");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MEAS:IND?"), "1");
    line(&mut s, &mut ui, &cv, ":CURS:MEAS:IND OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:IND?"), "0");
}

#[test]
fn frozen_math_scpi_preserves_reference_ids_expression_parser_and_cycles() {
    let (mut scpi, mut ui, canvas) = reference_session();
    ui.commit_waveform_traces_at(
        vec![crate::store::WaveTrace { v: vec![0.0, 1.0, 0.0, 1.0], dt: 1e-9, t0: 2e-6, label: "math R2".into() }],
        "R2",
        1,
    )
    .unwrap();
    line(
        &mut scpi,
        &mut ui,
        &canvas,
        ":MATH1:OPER SUBT;:MATH1:SOUR1 R1;:MATH1:SOUR2 R2;:MATH2:OPER ADD;:MATH2:SOUR1 MATH1;:MATH2:SOUR2 R1",
    );
    assert_eq!(
        line(&mut scpi, &mut ui, &canvas, ":MATH1:SOUR1?;:MATH1:SOUR2?;:MATH2:SOUR1?;:MATH2:SOUR2?"),
        "R1;R2;MATH1;R1"
    );
    assert_eq!((ui.m.math_ch(0).a, ui.m.math_ch(0).b), (MATH_REF_BASE, MATH_REF_BASE + 1));
    assert_eq!(ui.m.math_ch(1).a, NCH);
    line(&mut scpi, &mut ui, &canvas, ":MATH1:EXPR R1 - R2");
    assert_eq!(ui.m.math_ch(0).op, MathOp::Expr);
    assert_eq!(line(&mut scpi, &mut ui, &canvas, ":MATH1:EXPR?"), "R1 - R2");
    let old = ui.m.math_ch(0);
    line(&mut scpi, &mut ui, &canvas, ":MATH1:SOUR1 MATH1");
    assert_eq!(ui.m.math_ch(0), old);
    assert!(line(&mut scpi, &mut ui, &canvas, ":SYST:ERR?").starts_with("-224,"));
    line(&mut scpi, &mut ui, &canvas, ":MATH1:FFT:SOUR R2");
    assert_eq!(ui.m.math_ch(0).a, MATH_REF_BASE + 1);
    assert_eq!(line(&mut scpi, &mut ui, &canvas, ":SYST:ERR?"), "0,\"No error\"");
}

#[test]
fn frozen_bus_scpi_preserves_reference_roles_and_inactive_slot_acquisition() {
    let (mut scpi, mut ui, canvas) = reference_session();
    ui.commit_waveform_traces_at(
        vec![crate::store::WaveTrace { v: vec![0., 1., 0., 1.], dt: 1e-9, t0: 0., label: "clock".into() }],
        "R2",
        1,
    )
    .unwrap();
    line(&mut scpi, &mut ui, &canvas, ":BUS1:IIC:SDA:SOUR R1;:BUS1:IIC:SCLK:SOUR R2");
    assert_eq!(line(&mut scpi, &mut ui, &canvas, ":BUS1:IIC:SDA:SOUR?;:BUS1:IIC:SCLK:SOUR?"), "R1;R2");
    assert_eq!((ui.m.dec_src, ui.m.dec_src2), (DEC_REF_BASE, DEC_REF_BASE + 1));
    line(&mut scpi, &mut ui, &canvas, ":TRIG:EDGE:LEV 100;:SINGLE");
    let before = ui.workflow_state();
    let source = ui.m.trigger_wave();
    let mut pending = Program::new("*OPC?");
    assert!(scpi.poll_program(&mut ui, &canvas, &mut pending).is_none());
    line(&mut scpi, &mut ui, &canvas, ":BUS2:SPI:MOSI:SOUR R2;:BUS2:SPI:SCLK:SOUR R1;:BUS2:SPI:SS:SOUR OFF");
    assert_eq!(
        line(&mut scpi, &mut ui, &canvas, ":BUS2:SPI:MOSI:SOUR?;:BUS2:SPI:SCLK:SOUR?;:BUS2:SPI:SS:SOUR?"),
        "R2;R1;OFF"
    );
    assert_eq!(ui.m.dec_sel, 0);
    assert_eq!(ui.m.trigger_wave(), source);
    assert_eq!(ui.workflow_state(), before);
    assert!(
        scpi.poll_program(&mut ui, &canvas, &mut pending).is_none(),
        "inactive decoder configuration must not rearm the accepted acquisition context"
    );
    let old = ui.m.dec_slot(1);
    line(&mut scpi, &mut ui, &canvas, ":BUS2:SPI:MOSI:SOUR R10");
    assert_eq!(ui.m.dec_slot(1).src, old.src);
    assert!(line(&mut scpi, &mut ui, &canvas, ":SYST:ERR?").starts_with("-224,"));
}

fn wfm_bytes() -> Vec<u8> {
    const CURVE: usize = 838;
    let v = [0.0_f32, 0.5, 1.0, -0.25];
    let mut b = vec![0u8; CURVE + v.len() * 4 + 8];
    b[0..2].copy_from_slice(&0x0F0Fu16.to_le_bytes());
    b[2..10].copy_from_slice(b":WFM#002");
    b[15] = 4;
    b[16..20].copy_from_slice(&(CURVE as i32).to_le_bytes());
    b[40..44].copy_from_slice(b"lane");
    let put_f64 = |b: &mut [u8], o: usize, x: f64| b[o..o + 8].copy_from_slice(&x.to_le_bytes());
    let put_u32 = |b: &mut [u8], o: usize, x: u32| b[o..o + 4].copy_from_slice(&x.to_le_bytes());
    put_f64(&mut b, 168, 1.0);
    put_f64(&mut b, 176, 0.0);
    put_u32(&mut b, 240, 4);
    put_f64(&mut b, 488, 1e-9);
    put_f64(&mut b, 496, 0.0);
    put_u32(&mut b, 822, 0);
    put_u32(&mut b, 826, (v.len() * 4) as u32);
    for (k, &x) in v.iter().enumerate() {
        b[CURVE + k * 4..CURVE + k * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    b
}
