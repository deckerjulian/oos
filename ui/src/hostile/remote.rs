// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hostile input for remote commands: a program message cut into commands (`scpi::Program`), and the interpreter
//! on one line against the demo screen. The transports only frame (ADR 0022) and are tested by their frames.

use super::run;
use crate::gfx::Canvas;
use crate::scpi::{Program, Scpi};
use crate::ui::testkit::{scpi_session, Scratch};
use crate::ui::Ui;

/// The queries the comparison asks (every query of docs/scpi.md), one a line.
fn queries() -> Vec<&'static str> {
    include_str!("../../tests/scpi.txt")
        .lines()
        .filter_map(|l| l.strip_prefix("scpi ")?.split_once(' ').map(|(_, q)| q))
        .filter(|q| !sandboxed(q))
        .collect()
}

/// Commands that set something, with the kinds of argument there are.
const SETTERS: [&str; 16] = [
    ":CHANnel1:SCALe 0.5;:CHANnel2:DISPlay ON;:CHANnel2:OFFSet -1.5e-3",
    ":TIMebase:SCALe 1e-3;:TIMebase:OFFSet 2e-4\n:TIMebase:SCALe?",
    ":TRIGger:MODE EDGE;:TRIGger:EDGE:SOURce CHAN2;:TRIGger:EDGE:LEVel 0.25;:TRIG:SWE NORM",
    ":MEASure:ITEM VPP,CHAN1;:MEASure:ITEM? FREQ,CHAN1;:MEAS:STAT:DISP ON",
    ":CURSor:MODE MAN;:CURSor:MANual:TYPE TIME;:CURSor:CAX 100;:CURSor:CBX 300",
    ":MATH1:DISPlay ON;:MATH1:OPERator ADD;:MATH1:SOURce1 CHAN1;:MATH1:EXPRession \"ch1*2+sin(t)\"",
    ":DECode:PROTocol UART;:DECode:SOURce CHAN1;:DECode:THReshold 1.5;:BUS1:RS232:BAUD 115200",
    ":WAVeform:SOURce CHAN1;:WAVeform:FORMat ASC;:WAVeform:STARt 1;:WAVeform:STOP 100;:WAVeform:DATA?",
    ":ACQuire:MDEPth 10k;:ACQuire:TYPE AVER;:ACQuire:AVERages 16",
    ":REFerence:SLOT R2;:REFerence:SOURce CHAN1;:REFerence:SAVE;:REFerence:VSCale 0.5;:REFerence:LABel:CONTent \"a b\"",
    ":SYSTem:SET ch1.vdiv = 0.5;:SYSTem:GET? ch1.vdiv;:SYST:ERR?",
    ":MASK:SOURce CHAN1;:MASK:X 0.2;:MASK:Y 0.4;:MASK:CREate;:MASK:ENABle ON",
    ":SOURce1:FUNCtion SQUare;:SOURce1:FREQuency 1e3;:SOURce1:VOLTage 2;:SOURce1:OUTPut ON",
    ":LA:STATe ON;:LA:DIGital3:DISPlay ON;:LA:POD1:THReshold 1.4;:LA:DATA?",
    ":DISPlay:GRADing:TIME 0.5;:DISPlay:VIEW 2;:VIEW SPECtrum;:HISTogram:ENABle ON;:DVM:ENABle ON",
    "*CLS;*ESR?;*OPC;*OPC?;:RUN;:STOP;:SINGle;:TFORce;*RST",
];

/// What the test leaves out so it stays inside the test process: a command (not a query) that reads a file named
/// by the client, that reaches for the network or starts a thread (`:BODE`, `:SIM`, `:CALibrate`), or that changes
/// the board's state shared by every test of this process (`MAP`, `LANE`). Their queries only read, and are asked.
fn sandboxed(text: &str) -> bool {
    text.split(['\n', '\r', ';']).any(|command| {
        let head = command.trim().split(char::is_whitespace).next().unwrap_or("").to_ascii_uppercase();
        let nodes: Vec<&str> = head.trim_end_matches('?').split(':').filter(|n| !n.is_empty()).collect();
        let root = nodes.first().is_some_and(|n| ["BODE", "SIM", "CAL"].iter().any(|r| n.starts_with(r)));
        let node = nodes.iter().any(|n| ["LOAD", "IMP", "MAP", "LANE"].iter().any(|r| n.starts_with(r)));
        (root || node) && !head.ends_with('?')
    })
}

/// `Program::new` (a program message of the raw socket, VXI-11, HiSLIP or MCP): returns for any text; it gives at
/// most 1024 commands, each without a separator or space around it and none empty, in the order of the text.
#[test]
fn hostile_program_messages() {
    let all = queries();
    let mut seeds: Vec<&[u8]> = SETTERS.iter().map(|s| s.as_bytes()).collect();
    seeds.extend(all.iter().step_by(40).map(|q| q.as_bytes()));
    run("scpi::Program::new", &seeds, 1, |b| {
        let text = String::from_utf8_lossy(b);
        let mut commands = Vec::new();
        Program::new(&text).immediate(&mut |c| {
            commands.push(c.to_string());
            Vec::new()
        });
        let words: String = text.chars().filter(|c| !c.is_whitespace() && *c != ';').collect();
        let taken: String = commands.concat().chars().filter(|c| !c.is_whitespace()).collect();
        if commands.len() > 1024 || (!commands.is_empty() && taken != words) {
            return Err(format!("{} commands that are not the text in its order", commands.len()));
        }
        match commands.iter().find(|c| c.is_empty() || c.trim() != c.as_str() || c.contains([';', '\n', '\r'])) {
            Some(c) => Err(format!("the command {c:?}")),
            None => Ok(()),
        }
    });
}

/// A session of the interpreter on the demo screen, with a record and a save folder of its own.
fn session() -> (Scpi, Ui, Canvas, Scratch) {
    let (scpi, mut ui, cv) = scpi_session();
    let dir = Scratch::new("hostile-scpi");
    ui.store_dir = dir.to_path_buf();
    for _ in 0..3 {
        ui.step(0.05);
    }
    (scpi, ui, cv, dir)
}

/// One line through the interpreter, held to its contract.
fn line_ok(scpi: &mut Scpi, ui: &mut Ui, cv: &Canvas, text: &str) -> Result<(), String> {
    let commands: Vec<&str> =
        text.split(['\n', '\r']).flat_map(|l| l.split(';')).map(str::trim).filter(|c| !c.is_empty()).collect();
    let asks = commands.iter().any(|c| c.split_whitespace().next().is_some_and(|head| head.ends_with('?')));
    match scpi.exec_message_checked(ui, cv, text) {
        Ok((response, has_reply)) => {
            if response.len() > crate::scpi::RESP_MAX {
                return Err(format!("a response of {} bytes", response.len()));
            }
            if asks && !has_reply && commands.len() <= 1024 {
                return Err("a query without an answer: the client would wait for ever".into());
            }
        }
        // `*OPC?` behind an operation that has not ended: refused by the transports that cannot wait
        Err(e) if e.starts_with("Operation pending") => {}
        Err(e) => return Err(format!("refused with {e:?}")),
    }
    // the error queue ends: at most 20 entries, each short
    for k in 0.. {
        let e = scpi.exec_message_checked(ui, cv, ":SYSTem:ERRor?").map_err(|e| e.to_string())?.0;
        if e.starts_with(b"0,") {
            break;
        }
        if k > 20 || e.len() > 400 {
            return Err(format!("error {} of the queue has {} bytes", k, e.len()));
        }
    }
    // what the line left in the settings is a setup the reader takes
    let setup = crate::store::setup_text(&ui.m);
    match crate::store::parse_setup_lenient(&setup).1.first() {
        Some(bad) => Err(format!("left a setting the setup reader refuses: {bad}")),
        None => Ok(()),
    }
}

/// The interpreter on one line (`Scpi::exec_message_checked`, what MCP and the tests call; the raw socket, VXI-11
/// and HiSLIP run the same `poll_program`): returns for any text; a line with a query gets an answer; a response
/// is at most `RESP_MAX`; the error queue is at most 20 short entries; the settings it leaves are a setup the setup
/// reader takes without a refused line. The lines run one after the other on one screen, as a client sends them; a
/// line that fails is tried again alone on a new screen, and the message says whether it fails there too.
#[test]
fn hostile_command_lines() {
    let all = queries();
    let mut seeds: Vec<&[u8]> = SETTERS.iter().map(|s| s.as_bytes()).collect();
    seeds.extend(all.iter().map(|q| q.as_bytes()));
    let (mut scpi, mut ui, cv, _dir) = session();
    run("scpi::exec_line", &seeds, 1, |b| {
        let text = String::from_utf8_lossy(b);
        if sandboxed(&text) {
            return Ok(());
        }
        line_ok(&mut scpi, &mut ui, &cv, &text).map_err(|why| {
            let (mut scpi, mut ui, cv, _dir) = session();
            match line_ok(&mut scpi, &mut ui, &cv, &text) {
                Err(_) => format!("{why} (also alone on a new screen)"),
                Ok(()) => format!("{why} (only after the lines before it: the whole run is the input)"),
            }
        })
    });
}
