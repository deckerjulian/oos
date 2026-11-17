// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The tests of the SCPI interpreter through whole command lines: here the session and its protocol (`*OPC`, the
//! error queue, compound queries, raw TCP and HiSLIP) and the helpers the files in tests/ share, there one file per
//! group of subsystems. A test of a new command goes into the file of its subsystem; its query also goes into
//! ui/tests/scpi.txt, its line into docs/scpi.md (AGENTS.md).

use super::measure::meas_token;
use super::*;
use crate::math::{MathOp, MATH_REF_BASE};
use crate::wave::Source;

use crate::ui::testkit::{q as line, scpi_session as session};

mod decode;
mod measure;
mod reference;
mod settings;

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
