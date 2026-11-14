// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! SCPI server (TCP 5555, the port of the stock firmware), a stock-compatible subset:
//! *IDN? *RST *OPC[?] *ESR? *CLS, :RUN :STOP :SINGle :AUToscale :CLEar,
//! :CHANnel<n>:DISPlay|SCALe|OFFSet|COUPling|BWLimit|PROBe|INVert, :TIMebase[:MAIN]:SCALe|OFFSet,
//! :TRIGger:MODE[?]|TYPe[?] :TRIGger:EDGE:SOURce|LEVel|SLOPe, :TRIGger:SWEep, :TRIGger:HOLDoff, :TRIGger:STATus?, :ACQuire:SRATe?|MDEPth[?] (AUTO, 1k..125M),
//! :MEASure:ITEM? <item>[,CHANn|Rn[,src]], :MEASure:STRip?, :MEASure:READout?, :EYE:ITEM? <figure>,
//! :MEASure:ITEM? <item>[,CHANn|Rn[,src]], :MEASure:GATing SCREEN|NONE, :MEASure:REFS 10/90|20/80, :MEASure:TSETup, :MEASure:THOLd,
//! :POWer:ITEM? <item>[,src...], :POWer:HARMonics? [src], :SPECtrum:ITEM? <item>[,src], :BODE:MODE LOOP|PSRR|IMPedance,
//! :MAGnetic:ITEM? <item>[,V,I], :INVerter:ITEM? <item>[,src...], :DPT:ITEM? <item>[,V,I[,G]], :BANDwidth:ITEM? <item>[,src],
//! :WAVeform:SOURce CHANn|Rn|MODE NORM/RAW|FORMat|DATA?|PREamble?|POINts?, :DISPlay:DATA? (BMP),
//! :REFerence:SLOT R1..R10, :LOAD <file>, :IMPort [file], :SAVE, :SHOW ON|OFF, :CLEar,
//! :SIM:PRESet <name>, :SIM:SLOT R1..R10, :SIM:PLAY ON|OFF, :SIM:SHOW ON|OFF,
//! :SIM:SSC:MODE OFF|DOWN|CENTER, :SIM:SSC:DEViation <fraction>, :SIM:SSC:FREQuency <Hz>,
//! :SIM:JITTer:DCD <seconds pp>|:CORRelated <seconds RMS>|:CORNer <Hz>, :SIM:PATTern PRBS7..31.
//! SIM configuration is applied together on the next :SIM:PLAY ON. :SIM:BUSY? reports background generation.
//! :DECode:{SOURce,SOURce2,CS,SOURce4,DIFFerential} names CH1..CH4/R1..R10 (NONE for optional roles).
//! :DECode:{PROTocol,STATe,SELect,THReshold,INPut} configures the selected decoder; EVENts? returns BUSY while
//! a long stored record is computing, and BUSY? polls without changing acquisition or statistics.
//! :DECode:CPHY:RATE {AUTO|symbols_per_second} sets a known C-PHY symbol rate; :DECode:SRATe is an alias.
//! :LA:STATe, :LA:DIGital<n>:DISPlay, :LA:POD<n>:THReshold, :LA:POINts?, :LA:PREamble?, :LA:DATA? (u16 LE words),
//! :SYSTem:ERRor?, OOS: :VIEW SCOPe|SPECtrum|EYE|POWer|BODE|MAGnetic|INVerter|DPT, :IDENtify?, :CALibrate:STARt 0..4[,ZERO] / :STATus? /
//! :CANCel, :BODE:RUN ON|OFF / :DATA? / :GENerator "ip:port",
//! built-in generator: :SOURce[1]:OUTPut[:STATe] ON|OFF, :FUNCtion SINusoid|SQUare|RAMP|PULSe|NOISe|DC|ARB, :ARB:DATA, :ARB:CAPTure, :FUNCtion:SQUare:DUTY, :FUNCtion:RAMP:SYMMetry, :MOD[:STATe], :MOD:TYPe AM|FM|PM, :MOD:AM:DEPTh,
//! :MOD:FM|PM:DEViation, :MOD:<type>:INTernal:FREQuency|FUNCtion, :FREQuency <Hz>,
//! :VOLTage[:AMPLitude] <Vpp>, :VOLTage:OFFSet <V> (all with queries)
//! Settings go through the same model as the screen, so the screen follows remote changes.

use crate::gfx::Canvas;
use crate::board::{board, Board};
use crate::model::*;
use crate::ui::Ui;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

/// One bounded SCPI message, retained by its transport while *OPC? is pending.
#[derive(Default)]
pub struct Program {
    commands: std::collections::VecDeque<(String, bool)>,
    out: Vec<u8>,
    waiting: Option<crate::ui::Operations>,
    separator: u8,
    pub bytes: usize,
    invalid: bool,
}
impl Program {
    pub fn new(text: &str) -> Self {
        if text.len() > MAX_LINE { return Self { invalid: true, ..Self::default() }; }
        let mut commands = std::collections::VecDeque::new();
        for line in text.split(['\n', '\r']).filter(|line| !line.trim().is_empty()) {
            for (index, command) in line.split(';').map(str::trim).filter(|s| !s.is_empty()).enumerate() {
                if commands.len() >= 1024 { return Self { invalid: true, ..Self::default() }; }
                commands.push_back((command.to_string(), index == 0));
            }
        }
        Self { commands, separator: b';', bytes: text.len(), ..Self::default() }
    }
    fn append(&mut self, reply: &[u8]) {
        if !self.out.is_empty() { self.out.push(self.separator); }
        self.out.extend_from_slice(reply); self.separator = b';';
    }
    /// Adapter used by transport unit fixtures with a synchronous test interpreter.
    #[cfg(test)]
    pub fn immediate(&mut self, exec: &mut dyn FnMut(&str) -> Vec<u8>) -> Vec<u8> {
        while let Some((command, new_line)) = self.commands.pop_front() {
            if new_line && !self.out.is_empty() { self.separator = b'\n'; }
            let reply = exec(&command); if !reply.is_empty() { self.append(&reply); }
        }
        std::mem::take(&mut self.out)
    }
}

struct Client {
    s: TcpStream,
    buf: Vec<u8>,
    program: Option<Program>,
}

/// Longest command line (`:SOURce:ARB:DATA` with its samples is the longest there is); a client past it is dropped.
const MAX_LINE: usize = 4 << 20;
/// Connections at once.
const MAX_CLIENTS: usize = 8;
/// A message stops here: one answer may be a long record, ten thousand of them in one line may not.
pub const RESP_MAX: usize = 64 << 20;
const ERR_MAX: usize = 20;
const ERR_LEN: usize = 256;
/// A client that takes no data for this long, or not all of a response in `SEND_LIMIT`, is dropped: the server runs in
/// the main loop, the screen and the keys wait while it writes.
const SEND_STALL: Duration = Duration::from_secs(2);
const SEND_LIMIT: Duration = Duration::from_secs(30);

/// Writes `d` to a client; false when it was given up (the connection is then to be closed).
pub fn send(s: &mut TcpStream, d: &[u8]) -> bool {
    let end = std::time::Instant::now() + SEND_LIMIT;
    let _ = s.set_write_timeout(Some(SEND_STALL));
    let mut p = 0;
    while p < d.len() {
        if std::time::Instant::now() >= end {
            return false;
        }
        match s.write(&d[p..]) {
            Ok(0) => return false,
            Ok(n) => p += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return false,
        }
    }
    true
}

pub struct Scpi {
    l: Option<TcpListener>,
    clients: Vec<Client>,
    /// :WAVeform:SOURce, a channel or a reference slot
    wav_src: WaveId,
    wav_fmt: u8, // 0 BYTE, 1 WORD, 2 ASCII
    /// :WAVeform:MODE: false = the screen CH record, true = the entire retained native CH acquisition (RAW); refs are full records in either mode
    wav_raw: bool,
    err: Vec<String>,
    event_status: u8,
    opc_events: Vec<crate::ui::Operations>,
}

/// SCPI keyword match: the long form or its short form (the upper-case part), case-insensitive.
fn kw(tok: &str, long: &str) -> bool {
    let t = tok.to_ascii_uppercase();
    let short: String = long.chars().filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit()).collect();
    t == long.to_ascii_uppercase() || t == short
}

/// Splits a keyword with a numeric suffix: "CHAN2" -> ("CHAN", 2).
fn suffix(tok: &str) -> (&str, Option<usize>) {
    let i = tok.find(|c: char| c.is_ascii_digit()).unwrap_or(tok.len());
    (&tok[..i], tok[i..].parse().ok())
}

fn parse_ch(s: &str) -> Option<usize> {
    let (k, n) = suffix(s.trim());
    if kw(k, "CHANnel") {
        n.filter(|&n| (1..=NCH).contains(&n)).map(|n| n - 1)
    } else {
        None
    }
}

fn shown(v: f64) -> String {
    if v.is_finite() { e(v) } else { "9.9E37".into() }
}

fn arg_item(arg: &str) -> String {
    arg.split(',').next().unwrap_or("").trim().to_ascii_uppercase()
}

fn arg_rest(arg: &str) -> &str {
    arg.split_once(',').map(|(_, r)| r).unwrap_or("")
}

/// A measurement source: CHANn, or a ref slot R1–R10 (also REF1–REF10).
fn parse_wave(s: &str) -> Option<WaveId> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(c) = parse_ch(s) {
        return Some(WaveId::Ch(c));
    }
    let (k, n) = suffix(s);
    if kw(k, "REF") || k.eq_ignore_ascii_case("R") {
        return n.filter(|&n| (1..=NREF).contains(&n)).map(|n| WaveId::Ref(n - 1));
    }
    None
}

fn wave_tok(id: WaveId) -> String {
    match id {
        WaveId::Ch(c) => format!("CHAN{}", c + 1),
        WaveId::Ref(i) => format!("R{}", i + 1),
    }
}

/// Single-channel items the strip can show. The first spelling of a measurement is the one `STRip` and `READout` print.
/// A second spelling is only another way to ask.
const MEAS_ITEMS: &[(&str, &str, Meas)] = &[
    ("VPP", "VPP", Meas::Vpp),
    ("VMAX", "VMAX", Meas::Vmax),
    ("VMIN", "VMIN", Meas::Vmin),
    ("VAVG", "VAVG", Meas::Vavg),
    ("VRMS", "VRMS", Meas::Vrms),
    ("VAMP", "VAMP", Meas::Vamp),
    ("FREQuency", "FREQUENCY", Meas::Freq),
    ("PERiod", "PERIOD", Meas::Period),
    ("RTIMe", "RTIME", Meas::Rise),
    ("FTIMe", "FTIME", Meas::Fall),
    ("PWIDth", "PWIDTH", Meas::PWidth),
    ("PDUTy", "PDUTY", Meas::PDuty),
    ("NDUTy", "NDUTY", Meas::NDuty),
    ("NWIDth", "NWIDTH", Meas::NWidth),
    ("VTOP", "VTOP", Meas::Vtop),
    ("VBASe", "VBASE", Meas::Vbase),
    ("OVERshoot", "OVERSHOOT", Meas::Overshoot),
    ("PREShoot", "PRESHOOT", Meas::Preshoot),
    ("HTIMe", "HTIME", Meas::HighTime),
    ("LTIMe", "LTIME", Meas::LowTime),
    ("BWIDth", "BWIDTH", Meas::Burst),
    ("TOUTside", "TOUTSIDE", Meas::Outside),
    ("NPERiods", "NPERIODS", Meas::NPeriods),
    ("CAMP", "CAMP", Meas::CycAmp),
    ("CYCAmp", "CAMP", Meas::CycAmp),
    ("CTOP", "CTOP", Meas::CycTop),
    ("CBASe", "CBASE", Meas::CycBase),
    ("CPP", "CPP", Meas::CycPp),
    ("CYCPP", "CPP", Meas::CycPp),
    ("CMAX", "CMAX", Meas::CycMax),
    ("CMIN", "CMIN", Meas::CycMin),
    ("VUPPer", "VUPPER", Meas::Vupper),
    ("VMID", "VMID", Meas::Vmid),
    ("VLOWer", "VLOWER", Meas::Vlower),
    ("CAVG", "CAVG", Meas::VavgCyc),
    ("CRMS", "CRMS", Meas::VrmsCyc),
    ("ACRms", "ACRMS", Meas::VrmsAc),
    ("SDEViation", "SDEVIATION", Meas::StdDev),
    ("AREA", "AREA", Meas::Area),
    ("CARea", "CAREA", Meas::AreaCyc),
    ("PPULses", "PPULSES", Meas::PPulses),
    ("NPULses", "NPULSES", Meas::NPulses),
    ("PEDGes", "PEDGES", Meas::PEdges),
    ("NEDGes", "NEDGES", Meas::NEdges),
    ("TVMAX", "TVMAX", Meas::TVmax),
    ("TVMIN", "TVMIN", Meas::TVmin),
    ("PSLew", "PSLEW", Meas::PSlew),
    ("NSLew", "NSLEW", Meas::NSlew),
    ("ESTUI", "ESTUI", Meas::EstUi),
    ("UI", "ESTUI", Meas::EstUi),
    ("MEDian", "MEDIAN", Meas::Median),
    ("MODE", "MODE", Meas::Mode),
    ("LSLO", "LSLO", Meas::LinSlope),
    ("LINSLOPE", "LSLO", Meas::LinSlope),
    ("LINT", "LINT", Meas::LinInt),
    ("LININT", "LINT", Meas::LinInt),
    ("PAM0", "PAM0", Meas::Pam0),
    ("L0", "PAM0", Meas::Pam0),
    ("PAM1", "PAM1", Meas::Pam1),
    ("L1", "PAM1", Meas::Pam1),
    ("PAM2", "PAM2", Meas::Pam2),
    ("L2", "PAM2", Meas::Pam2),
    ("PAM3", "PAM3", Meas::Pam3),
    ("L3", "PAM3", Meas::Pam3),
    ("PSEP", "PSEP", Meas::PamSep),
    ("BITS", "BITS", Meas::Bits),
    ("ONES", "BITS", Meas::Bits),
    ("PLLUI", "PLLUI", Meas::PllUi),
    ("UIWTD", "UIWTD", Meas::UiWtd),
    ("FOLD", "FOLD", Meas::FoldRms),
];

/// Channel-pair rows, in the same order as the Measure menu (`PAIRS`).
const PAIR_ITEMS: &[(&str, &str, usize)] = &[
    ("RRDelay", "RRDELAY", 0),
    ("FFDelay", "FFDELAY", 1),
    ("RFDelay", "RFDELAY", 2),
    ("FRDelay", "FRDELAY", 3),
    ("RRPHase", "RRPHASE", 4),
    ("FFPHase", "FFPHASE", 5),
    ("RFPHase", "RFPHASE", 6),
    ("FRPHase", "FRPHASE", 7),
    ("RRSkew", "RRSKEW", 8),
    ("FFSkew", "FFSKEW", 9),
    ("RFSkew", "RFSKEW", 10),
    ("FRSkew", "FRSKEW", 11),
    ("SETup", "SETUP", 12),
    ("HOLD", "HOLD", 13),
    ("SETHold", "SETHOLD", 14),
    ("FSETup", "FSETUP", 15),
    ("FHOLd", "FHOLD", 16),
];

fn meas_kind(item: &str) -> Option<Meas> {
    MEAS_ITEMS.iter().find(|it| kw(item, it.0)).map(|it| it.2)
}

fn meas_token(k: Meas) -> &'static str {
    MEAS_ITEMS.iter().find(|it| it.2 == k).map(|it| it.1).unwrap_or("ITEM")
}

fn pair_kind(item: &str) -> Option<usize> {
    PAIR_ITEMS.iter().find(|it| kw(item, it.0)).map(|it| it.2)
}

fn pair_token(k: usize) -> &'static str {
    PAIR_ITEMS.get(k.min(PAIR_ITEMS.len() - 1)).map(|it| it.1).unwrap_or("PAIR")
}

/// R1..R10, REF1..REF10, or 1..10.
fn parse_slot(s: &str) -> Option<usize> {
    let s = s.trim().trim_matches('"');
    if let Some(WaveId::Ref(i)) = parse_wave(s) {
        return Some(i);
    }
    s.parse::<usize>().ok().filter(|&n| (1..=NREF).contains(&n)).map(|n| n - 1)
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_uppercase().as_str() {
        "1" | "ON" => Some(true),
        "0" | "OFF" => Some(false),
        _ => None,
    }
}

/// A finite number ("nan", "inf" and overflows are refused: they would end up in the model and in saved setups).
fn num(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

fn nearest(ladder: &[f32], v: f32) -> usize {
    let mut best = 0;
    for (i, &x) in ladder.iter().enumerate() {
        if ((x / v).ln()).abs() < ((ladder[best] / v).ln()).abs() {
            best = i;
        }
    }
    best
}

/// Stock number format: 7 significant digits, two-digit exponent ("2.000000E-04").
fn e(v: f64) -> String {
    if !v.is_finite() {
        return "9.9E37".into();
    }
    let s = format!("{:.6E}", v);
    match s.split_once('E') {
        Some((m, x)) => {
            let n: i32 = x.parse().unwrap_or(0);
            format!("{}E{}{:02}", m, if n < 0 { '-' } else { '+' }, n.abs())
        }
        None => s,
    }
}

impl Scpi {
    pub fn open(port: u16) -> Scpi {
        let l = TcpListener::bind(("0.0.0.0", port)).ok();
        if let Some(l) = &l {
            let _ = l.set_nonblocking(true);
            eprintln!("oos-ui: SCPI on port {}", port);
        } else {
            eprintln!("oos-ui: SCPI port {} not available", port);
        }
        Scpi { l, clients: Vec::new(), wav_src: WaveId::Ch(0), wav_fmt: 0, wav_raw: false, err: Vec::new(), event_status: 0, opc_events: Vec::new() }
    }

    pub fn port(&self) -> Option<u16> {
        self.l.as_ref().and_then(|l| l.local_addr().ok()).map(|a| a.port())
    }

    pub fn poll(&mut self, ui: &mut Ui, cv: &Canvas) {
        if let Some(l) = &self.l {
            while let Ok((s, _)) = l.accept() {
                let _ = s.set_nonblocking(false);
                let _ = s.set_read_timeout(Some(Duration::from_millis(1)));
                let _ = s.set_nodelay(true);
                // each connection costs a read per frame
                if self.clients.len() < MAX_CLIENTS {
                    self.clients.push(Client { s, buf: Vec::new(), program: None });
                }
            }
        }
        let mut i = 0;
        while i < self.clients.len() {
            let mut tmp = [0u8; 4096];
            let mut closed = false;
            let mut read_bytes = 0;
            loop {
                match self.clients[i].s.read(&mut tmp) {
                    Ok(0) => {
                        closed = true;
                        break;
                    }
                    Ok(n) => {
                        if self.clients[i].buf.len() + n > MAX_LINE { closed = true; break; }
                        self.clients[i].buf.extend_from_slice(&tmp[..n]); read_bytes += n;
                        if read_bytes >= 65536 { break; }
                    },
                    Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => break,
                    Err(_) => {
                        closed = true;
                        break;
                    }
                }
            }
            while !closed {
                if self.clients[i].program.is_none() {
                    let Some(p) = self.clients[i].buf.iter().position(|&c| c == b'\n') else { break; };
                    let line: Vec<u8> = self.clients[i].buf.drain(..=p).collect();
                    self.clients[i].program = Some(Program::new(&String::from_utf8_lossy(&line)));
                }
                let mut program = self.clients[i].program.take().unwrap();
                let Some(mut response) = self.poll_program(ui, cv, &mut program) else {
                    self.clients[i].program = Some(program);
                    break;
                };
                if !response.is_empty() {
                    response.push(b'\n');
                    if !send(&mut self.clients[i].s, &response) { closed = true; break; }
                }
            }
            closed |= self.clients[i].buf.len() > MAX_LINE;
            if closed {
                self.clients.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Immediate adapter for HTTP and in-process callers. Deferred barriers must use
    /// `poll_program`: returning 1 here while an operation is pending would be false.
    /// Commands before a pending barrier execute once; the remaining message is rejected.
    pub fn exec_line(&mut self, ui: &mut Ui, cv: &Canvas, line: &str) -> Vec<u8> {
        self.exec_line_checked(ui, cv, line).unwrap_or_default()
    }

    pub fn exec_line_checked(&mut self, ui: &mut Ui, cv: &Canvas, line: &str) -> Result<Vec<u8>, &'static str> {
        let mut program = Program::new(line);
        match self.poll_program(ui, cv, &mut program) {
            Some(response) => Ok(response),
            None => {
                let error = "Operation pending; use raw TCP or HiSLIP for *OPC?";
                self.err.push(format!("-221,\"{}\"", error));
                self.normalize_errors();
                Err(error)
            }
        }
    }

    fn poll_events(&mut self, ui: &Ui) {
        self.opc_events.retain(|fence| match fence.state(ui) {
            crate::ui::OperationState::Pending => true,
            crate::ui::OperationState::Complete => { self.event_status |= 1; false },
            crate::ui::OperationState::Cancelled => false,
        });
    }

    /// Advance a bounded per-client message without waiting on the UI thread.
    /// The continuation owns its earlier replies and remaining commands: no write is replayed.
    pub fn poll_program(&mut self, ui: &mut Ui, cv: &Canvas, program: &mut Program) -> Option<Vec<u8>> {
        let response = self.advance_program(ui, cv, program);
        self.normalize_errors();
        response
    }

    fn advance_program(&mut self, ui: &mut Ui, cv: &Canvas, program: &mut Program) -> Option<Vec<u8>> {
        self.poll_events(ui);
        if program.invalid { self.err.push("-363,\"Program message too large or too many commands\"".into()); return Some(Vec::new()); }
        if let Some(fence) = &program.waiting {
            match fence.state(ui) {
                crate::ui::OperationState::Pending => return None,
                crate::ui::OperationState::Complete => { program.waiting = None; program.append(b"1"); },
                crate::ui::OperationState::Cancelled => {
                    self.err.push("-410,\"OPC query interrupted by cancelled operation\"".into());
                    program.commands.clear(); program.out.clear(); program.waiting = None;
                    return Some(Vec::new());
                }
            }
        }
        while let Some((command, new_line)) = program.commands.pop_front() {
            if new_line && !program.out.is_empty() { program.separator = b'\n'; }
            if program.out.len() >= RESP_MAX {
                self.err.push("-363,\"Input buffer overrun\" rest of the line dropped".into());
                program.commands.clear(); break;
            }
            if command.eq_ignore_ascii_case("*OPC?") {
                let fence = ui.scpi_operations();
                if fence.state(ui) == crate::ui::OperationState::Pending { program.waiting = Some(fence); return None; }
                program.append(b"1");
            } else if let Some(response) = self.exec(ui, cv, &command) {
                program.append(&response);
            }
        }
        Some(std::mem::take(&mut program.out))
    }

    fn exec(&mut self, ui: &mut Ui, cv: &Canvas, cmd: &str) -> Option<Vec<u8>> {
        if let Some(error) = ui.take_sim_error() {
            self.err.push(format!("-224,\"SIM generation failed\" {error}"));
        }
        let r = self.exec_cmd(ui, cv, cmd);
        self.normalize_errors();
        r
    }

    fn normalize_errors(&mut self) {
        // a client that never asks for its errors, or sends megabytes of nonsense, must not fill the memory
        for e in &mut self.err {
            if e.len() > ERR_LEN {
                *e = e.chars().take(ERR_LEN).collect();
            }
        }
        if self.err.len() > ERR_MAX {
            self.err.truncate(ERR_MAX - 1);
            self.err.push("-350,\"Queue overflow\"".into());
        }
    }

    fn exec_cmd(&mut self, ui: &mut Ui, cv: &Canvas, cmd: &str) -> Option<Vec<u8>> {
        if cmd.is_empty() {
            return None;
        }
        let (head, arg) = match cmd.find(char::is_whitespace) {
            Some(i) => (&cmd[..i], cmd[i..].trim()),
            None => (cmd, ""),
        };
        let query = head.ends_with('?');
        let head = head.trim_end_matches('?');
        let up = head.to_ascii_uppercase();
        let t = |s: &str| Some(s.as_bytes().to_vec());
        // common commands
        match up.as_str() {
            "*IDN" => {
                let p = oos_hal::model::platform();
                return t(&format!("OOS,{},{},oos-ui {}", p.caps.name, p.serial.as_deref().unwrap_or("0"), env!("CARGO_PKG_VERSION")));
            }
            "*OPC" if !query => {
                self.poll_events(ui);
                if self.opc_events.len() < 64 { self.opc_events.push(ui.scpi_operations()); }
                else { self.err.push("-350,\"Too many pending OPC events\"".into()); }
                self.poll_events(ui);
                return None;
            }
            "*ESR" if query => {
                self.poll_events(ui);
                let status = std::mem::take(&mut self.event_status);
                return t(&status.to_string());
            }
            "*CLS" if !query => {
                self.err.clear();
                self.event_status = 0;
                self.opc_events.clear();
                return None;
            }
            "*RST" if !query => {
                self.event_status = 0; self.opc_events.clear();
                ui.key(crate::ui::key::DEFAULT);
                return None;
            }
            _ => {}
        }
        let nodes: Vec<&str> = head.trim_start_matches(':').split(':').collect();
        if !query && nodes.len() == 1 {
            let command = if kw(nodes[0], "STOP") { Some(0) } else if kw(nodes[0], "RUN") { Some(1) }
                else if kw(nodes[0], "SINGle") { Some(2) } else { None };
            if command.is_some_and(|command| ui.mixed_bus_command(command)) { return None; }
        }
        if !query && ((nodes.len() == 1 && kw(nodes[0], "TFORce"))
            || (nodes.len() == 2 && kw(nodes[0], "TRIGger") && kw(nodes[1], "FORCe"))) {
            if !ui.mixed_bus_command(3) {
                if ui.record_mode() { ui.record_key(crate::ui::key::FORCE); }
                else { ui.key(crate::ui::key::FORCE); }
            }
            return None;
        }
        if nodes.len() == 3 && kw(nodes[0], "DECode") && kw(nodes[1], "CPHY")
            && (kw(nodes[2], "RATE") || kw(nodes[2], "SRATe")) {
            return self.decode_cphy_rate_cmd(ui, query, arg);
        }
        if nodes.len() == 2 && kw(nodes[0], "DECode") {
            return self.decode_cmd(ui, nodes[1], query, arg);
        }
        if nodes.len() >= 2 && kw(nodes[0], "LA") {
            return self.la_cmd(ui, &nodes[1..], query, arg);
        }
        if nodes.len() == 1 && kw(nodes[0], "AUToscale") && query {
            return t(if ui.auto_running() { "1" } else { "0" });
        }
        if nodes.len() == 1 && kw(nodes[0], "AUToscale") {
            ui.key(crate::ui::key::AUTO);
            return None;
        }
        if !query && nodes.len() == 1 && kw(nodes[0], "CLEar") {
            ui.key(crate::ui::key::CLEAR);
            return None;
        }
        if !query && nodes.len() == 1 && kw(nodes[0], "STOP") && ui.sim_generating() {
            let _ = ui.sim_play_set(false);
            ui.mark_changed();
            return None;
        }
        // A reference capture uses the same acquisition state as the screen.
        // RUN and STOP are assignments, so repeating either must not toggle it.
        // Call the shared record handler directly: an open UI dialog must not
        // consume a remote acquisition command as a dialog-dismissal key.
        if !query && nodes.len() == 1 && ui.record_mode()
            && ["RUN", "STOP", "SINGle"].iter().any(|command| kw(nodes[0], command)) {
            let key = crate::ui::key::RUNSTOP;
            if kw(nodes[0], "RUN") {
                if ui.m.trig_mode == TrigMode::Single {
                    ui.m.trig_mode = TrigMode::Normal;
                }
                if ui.m.run != RunState::Run {
                    ui.record_key(key);
                }
            } else if kw(nodes[0], "STOP") {
                if ui.m.run == RunState::Run {
                    ui.record_key(key);
                } else {
                    let _ = ui.sim_play_set(false);
                }
            } else {
                ui.record_key(crate::ui::key::SINGLE);
            }
            ui.mark_changed();
            return None;
        }
        // Reference trigger identities belong to the UI acquisition router, not
        // a silent physical-channel alias inside Model::trigger().
        if nodes.first().is_some_and(|node| kw(node, "TRIGger")) {
            let last = nodes.last().copied().unwrap_or("");
            let (leaf, source_number) = suffix(last);
            if (nodes.len() == 2 || nodes.len() == 3) && kw(leaf, "SOURce") {
                let secondary = source_number == Some(2);
                if query {
                    if !secondary && ui.m.trig_ref.is_none() {
                        if let Some(line) = ui.m.trig_line() { return t(&format!("D{line}")); }
                    }
                    let id = if secondary { ui.m.trigger_wave2() } else { ui.m.trigger_wave() };
                    return t(&match id { WaveId::Ref(i) => format!("R{}", i + 1), WaveId::Ch(c) => format!("CHAN{}", c + 1) });
                }
                if let Some(id) = parse_wave(arg) {
                    if !ui.set_trigger_source(id, secondary) { self.err.push("-224,\"Trigger source is unavailable\"".into()); }
                    ui.mark_changed();
                    return None;
                }
                // Leave D0..D15 to the existing LA source handler.
                if !arg.trim().to_ascii_uppercase().starts_with('D') {
                    self.err.push("-224,\"Invalid trigger source\"".into());
                    return None;
                }
            }
            if nodes.len() == 2 && (kw(last, "TYPe") || kw(last, "MODE")) {
                if query { return t(if kw(last, "MODE") { match ui.m.trig_type {
                    TrigType::Edge => "EDGE", TrigType::Pulse => "PULS", TrigType::Slope => "SLOP", TrigType::Video => "VID",
                    TrigType::Pattern => "PATT", TrigType::Duration => "DUR", TrigType::Timeout => "TIM", TrigType::Runt => "RUNT",
                    TrigType::Window => "WIND", TrigType::Delay => "DEL", TrigType::SetupHold => "SET", TrigType::NthEdge => "NEDG", TrigType::Bus => "BUS",
                }} else { ui.m.trig_type.name() }); }
                let name = arg.trim().to_ascii_uppercase();
                let kind = match name.as_str() {
                    "EDGE" => Some(TrigType::Edge), "PULSE" | "PULS" => Some(TrigType::Pulse),
                    "SLOPE" | "SLOP" => Some(TrigType::Slope), "VIDEO" | "VID" => Some(TrigType::Video),
                    "PATTERN" | "PATT" => Some(TrigType::Pattern), "DURATION" | "DUR" => Some(TrigType::Duration),
                    "TIMEOUT" | "TIM" => Some(TrigType::Timeout), "RUNT" => Some(TrigType::Runt),
                    "WINDOW" | "WIND" => Some(TrigType::Window), "DELAY" | "DEL" => Some(TrigType::Delay),
                    "SETUPHOLD" | "SETUP" | "SET" => Some(TrigType::SetupHold), "NTHEDGE" | "NTH" | "NEDGE" | "NEDG" => Some(TrigType::NthEdge),
                    "BUS" => Some(TrigType::Bus), _ => None,
                };
                if let Some(kind) = kind { ui.trigger_type_set(kind); ui.mark_changed(); }
                else { self.err.push("-224,\"Invalid trigger type\"".into()); }
                return None;
            }
            if nodes.len() == 3 && kw(nodes[1], "RUNT") {
                let value = if kw(last, "UPPer") || kw(last, "ALEVel") { Some(&mut ui.m.trig_level) }
                    else if kw(last, "LOWer") || kw(last, "BLEVel") { Some(&mut ui.m.trig_level2) } else { None };
                if let Some(value) = value {
                    if query { return t(&e(*value as f64)); }
                    if let Some(number) = num(arg) { *value = number; ui.mark_changed(); }
                    else { self.err.push("-224,\"Invalid runt threshold\"".into()); }
                    return None;
                }
            }
        }
        if !query && (nodes.len() == 1 && kw(nodes[0], "SINGle")
            || nodes.len() == 2 && kw(nodes[0], "TRIGger") && kw(nodes[1], "SWEep")
                && arg.to_ascii_uppercase().starts_with("SING") && ui.m.run == RunState::Run) {
            ui.scpi_arm_operation();
        }
        let m = &mut ui.m;
        let r: Option<Option<Vec<u8>>> = (|| {
            let n0 = nodes.first().copied().unwrap_or("");
            let (k0, sfx) = suffix(n0);
            if !query && nodes.len() == 1 {
                if kw(n0, "RUN") {
                    m.run = RunState::Run;
                    if m.trig_mode == TrigMode::Single {
                        m.trig_mode = TrigMode::Auto;
                    }
                    return Some(None);
                }
                if kw(n0, "STOP") {
                    m.run = RunState::Stop;
                    return Some(None);
                }
                if kw(n0, "SINGle") {
                    m.trig_mode = TrigMode::Single;
                    m.run = RunState::Run;
                    return Some(None);
                }
            }
            if kw(k0, "CHANnel") && nodes.len() == 2 {
                let c = sfx?.checked_sub(1).filter(|&c| c < NCH)?;
                let n1 = nodes[1];
                let ch = &mut m.ch[c];
                if kw(n1, "DISPlay") {
                    if query {
                        return Some(t(if ch.on { "1" } else { "0" }));
                    }
                    ch.on = parse_bool(arg)?;
                } else if kw(n1, "SCALe") {
                    if query {
                        return Some(t(&e(ch.volts_per_div() as f64)));
                    }
                    ch.vscale = nearest(&VSCALES, num(arg)? / ch.probe);
                    ch.vfine = 1.0;
                } else if kw(n1, "OFFSet") {
                    if query {
                        return Some(t(&e(ch.offset as f64)));
                    }
                    ch.offset = num(arg)?;
                } else if kw(n1, "COUPling") {
                    if query {
                        return Some(t(ch.coupling.name()));
                    }
                    ch.coupling = match arg.to_ascii_uppercase().as_str() {
                        "DC" => Coupling::Dc,
                        "AC" => Coupling::Ac,
                        "GND" => Coupling::Gnd,
                        "DC50" if board().couplings().contains(&Coupling::Dc50) => Coupling::Dc50,
                        _ => return None,
                    };
                } else if kw(n1, "BWLimit") {
                    if query {
                        return Some(t(match ch.bw {
                            BW_20M => "20M".to_string(),
                            BW_FULL => "OFF".to_string(),
                            0 => "AUTO".to_string(),
                            i => BWS[i].0.replace(' ', "").to_ascii_uppercase(),
                        }
                        .as_str()));
                    }
                    // OFF (full), 20M, AUTO, GND or a filter like 100M / 100MHZ / 1GHZ
                    let a = arg.trim().to_ascii_uppercase().replace(' ', "");
                    ch.bw = match a.as_str() {
                        "OFF" | "FULL" => BW_FULL,
                        "AUTO" => 0,
                        "GND" => 3,
                        "20M" | "20MHZ" => BW_20M,
                        _ => BWS.iter().position(|b| b.0.replace(' ', "").to_ascii_uppercase() == a || b.0.replace(' ', "").to_ascii_uppercase() == format!("{}HZ", a))?,
                    };
                } else if kw(n1, "PROBe") {
                    if query {
                        return Some(t(&e(ch.probe as f64)));
                    }
                    ch.probe = num(arg).filter(|&p| p > 0.0)?;
                } else if kw(n1, "INVert") {
                    if query {
                        return Some(t(if ch.invert { "1" } else { "0" }));
                    }
                    ch.invert = parse_bool(arg)?;
                } else {
                    return None;
                }
                return Some(None);
            }
            if kw(n0, "TIMebase") {
                let last = *nodes.last()?;
                if kw(last, "SCALe") {
                    if query {
                        return Some(t(&e(m.time_per_div() as f64)));
                    }
                    m.tscale = nearest(&tscales(), num(arg)?);
                    m.tfine = 1.0;
                    return Some(None);
                }
                if kw(last, "OFFSet") || kw(last, "POSition") {
                    if query {
                        return Some(t(&e(m.delay as f64)));
                    }
                    m.delay = num(arg)?;
                    return Some(None);
                }
                return None;
            }
            if kw(n0, "TRIGger") {
                let last = *nodes.last()?;
                if nodes.len() == 2 && kw(last, "STATus") {
                    let s = match (m.run, m.status) {
                        (RunState::Stop, _) => "STOP",
                        (_, TrigStatus::Triggered) => "TD",
                        (_, TrigStatus::Auto) => "AUTO",
                        (_, TrigStatus::Wait) => "WAIT",
                        _ => "STOP",
                    };
                    return Some(t(s));
                }
                if nodes.len() == 2 && kw(last, "NREJect") {
                    if query {
                        return Some(t(if m.trig_nrej { "1" } else { "0" }));
                    }
                    let a = arg.trim().to_ascii_uppercase();
                    m.trig_nrej = match a.as_str() {
                        "1" | "ON" => true,
                        "0" | "OFF" => false,
                        _ => return None,
                    };
                    return Some(None);
                }
                if nodes.len() == 2 && kw(last, "HOLDoff") {
                    if query {
                        return Some(t(&e(m.holdoff as f64)));
                    }
                    m.holdoff = num(arg).filter(|&h| (0.0..=10.0).contains(&h))? as f32;
                    return Some(None);
                }
                if nodes.len() == 2 && kw(last, "SWEep") {
                    if query {
                        return Some(t(match m.trig_mode {
                            TrigMode::Auto => "AUTO",
                            TrigMode::Normal => "NORM",
                            TrigMode::Single => "SING",
                        }));
                    }
                    let a = arg.to_ascii_uppercase();
                    m.trig_mode = if a.starts_with("AUTO") { TrigMode::Auto } else if a.starts_with("NORM") { TrigMode::Normal } else if a.starts_with("SING") { TrigMode::Single } else { return None };
                    return Some(None);
                }
                if nodes.len() == 3 && kw(nodes[1], "EDGE") {
                    if kw(last, "SOURce") {
                        if query {
                            return Some(t(&m.trig_line().map_or(format!("CHAN{}", m.trig_src + 1), |n| format!("D{}", n))));
                        }
                        // D0..D15 like the stock source names, with the LA on
                        match arg.trim().to_ascii_uppercase().strip_prefix('D').and_then(|n| n.parse::<u8>().ok()).filter(|&n| n < 16 && m.trig_la_ok()) {
                            Some(n) => m.trig_la = Some(n),
                            None => (m.trig_src, m.trig_la) = (parse_ch(arg)?, None),
                        }
                    } else if kw(last, "LEVel") {
                        if query {
                            return Some(t(&e(m.trig_level as f64)));
                        }
                        m.trig_level = num(arg)?;
                    } else if kw(last, "SLOPe") {
                        if query {
                            return Some(t(match m.slope {
                                Slope::Rise => "POS",
                                Slope::Fall => "NEG",
                                Slope::Either => "RFAL",
                            }));
                        }
                        let a = arg.to_ascii_uppercase();
                        m.slope = if a.starts_with("POS") { Slope::Rise } else if a.starts_with("NEG") { Slope::Fall } else if a.starts_with("RFAL") { Slope::Either } else { return None };
                    } else {
                        return None;
                    }
                    return Some(None);
                }
                return None;
            }
            if kw(n0, "ACQuire") && nodes.len() == 2 && !query && kw(nodes[1], "MDEPth") {
                // AUTO or one of the depths of the Acquire menu (`mdepths`), e.g. 1000000 or 1M
                let a = arg.trim().to_ascii_uppercase();
                let want = if a.starts_with("AUTO") {
                    0
                } else {
                    let (v, mul) = match a.chars().last() {
                        Some('K') => (&a[..a.len() - 1], 1e3),
                        Some('M') => (&a[..a.len() - 1], 1e6),
                        _ => (&a[..], 1.0),
                    };
                    (v.parse::<f64>().ok()? * mul).round() as u64
                };
                m.mdepth = mdepths().iter().position(|d| d.1 == want)?;
                return Some(None);
            }
            if kw(n0, "ACQuire") && nodes.len() == 2 && query {
                let (sr, depth) = m.srate_depth();
                if kw(nodes[1], "SRATe") {
                    return Some(t(&e(sr)));
                }
                if kw(nodes[1], "MDEPth") {
                    return Some(t(&e(depth as f64)));
                }
                return None;
            }
            None
        })();
        if let Some(resp) = r {
            if !query { ui.mark_changed(); }
            return resp;
        }
        // measurement, waveform, display, system (need the record / canvas)
        let n0 = nodes.first().copied().unwrap_or("");
        if kw(n0, "MEASure") && nodes.len() == 2 && query {
            if kw(nodes[1], "ITEM") {
                return self.meas_item(ui, arg);
            }
            if kw(nodes[1], "STRip") {
                return t(&meas_strip(ui));
            }
            // every measurement on the screen: the strip, and the eye figures when that page is up
            if kw(nodes[1], "READout") {
                return t(&meas_readout(ui));
            }
        }
        if kw(n0, "EYE") && nodes.len() == 2 && kw(nodes[1], "ITEM") && query {
            return self.eye_item(ui, arg);
        }
        if kw(n0, "MEASure") && nodes.len() == 2 {
            let n1 = nodes[1];
            if kw(n1, "GATing") {
                if query { return t(if ui.m.meas_full_record { "NONE" } else { "SCREEN" }); }
                let full_record = if kw(arg.trim(), "SCReen") { false }
                    else if kw(arg.trim(), "NONE") { true }
                    else {
                        self.err.push(format!("-224,\"Illegal parameter value\" {}", arg));
                        return None;
                    };
                ui.set_measurement_full_record(full_record);
                return None;
            }
            if kw(n1, "REFS") {
                if query {
                    let ten = ui.m.meas_lpct < 0.15 && ui.m.meas_hpct > 0.85;
                    return t(if ten { "10/90" } else { "20/80" });
                }
                let a = arg.trim().replace(' ', "");
                let (lo, hi) = if a == "10/90" || a == "1090" || a == "10" {
                    (0.1, 0.9)
                } else if a == "20/80" || a == "2080" || a == "20" {
                    (0.2, 0.8)
                } else {
                    self.err.push(format!("-224,\"Illegal parameter value\" {}", arg));
                    return None;
                };
                ui.m.meas_lpct = lo;
                ui.m.meas_hpct = hi;
                ui.measure_last();
                ui.mark_changed();
                return None;
            }
            if kw(n1, "TSETup") || kw(n1, "THOLd") {
                let setup = kw(n1, "TSETup");
                if query {
                    return t(&e(if setup { ui.m.meas_tsetup } else { ui.m.meas_thold }));
                }
                match arg.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0) {
                    Some(v) => {
                        if setup {
                            ui.m.meas_tsetup = v;
                        } else {
                            ui.m.meas_thold = v;
                        }
                        ui.mark_changed();
                    }
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            if kw(n1, "ITEM") && query {
                return self.meas_item(ui, arg);
            }
        }
        if kw(n0, "POWer") && nodes.len() == 2 && query {
            if kw(nodes[1], "HARMonics") {
                let src = match self.waves(arg) {
                    Ok(v) => v,
                    Err(s) => {
                        self.err.push(s);
                        return t("9.9E37");
                    }
                };
                return match ui.fact_harmonics(&src) {
                    Ok(h) if h.is_empty() => t("9.9E37"),
                    Ok(h) => t(&h.iter().map(|(n, a)| format!("{},{}", n, e(*a))).collect::<Vec<_>>().join(";")),
                    Err(()) => {
                        self.err.push("-224,\"Illegal parameter value\"".into());
                        t("9.9E37")
                    }
                };
            }
            if kw(nodes[1], "ITEM") {
                return self.sourced(ui, arg, |ui, item, src| ui.fact_power(item, src));
            }
        }
        if kw(n0, "SPECtrum") && nodes.len() == 2 && kw(nodes[1], "ITEM") && query {
            return self.sourced(ui, arg, |ui, item, src| ui.fact_spectrum(item, src));
        }
        if kw(n0, "MAGnetic") && nodes.len() == 2 && kw(nodes[1], "ITEM") && query {
            return self.sourced(ui, arg, |ui, item, src| ui.fact_magnetic(item, src));
        }
        if kw(n0, "INVerter") && nodes.len() == 2 && kw(nodes[1], "ITEM") && query {
            let src = match self.waves(arg_rest(arg)) {
                Ok(v) => v,
                Err(s) => {
                    self.err.push(s);
                    return t("9.9E37");
                }
            };
            return match ui.fact_inverter(&arg_item(arg), &src) {
                Ok(crate::ui::facts::Fact::Word(w)) => t(w),
                Ok(crate::ui::facts::Fact::Num(v)) => t(&shown(v)),
                Err(()) => {
                    self.err.push("-224,\"Illegal parameter value\"".into());
                    t("9.9E37")
                }
            };
        }
        if kw(n0, "DPT") && nodes.len() == 2 && kw(nodes[1], "ITEM") && query {
            return self.sourced(ui, arg, |ui, item, src| ui.fact_dpt(item, src));
        }
        if kw(n0, "BANDwidth") && nodes.len() == 2 && kw(nodes[1], "ITEM") && query {
            return self.sourced(ui, arg, |ui, item, src| ui.fact_bandwidth(item, src));
        }
        if kw(n0, "WAVeform") && nodes.len() == 2 {
            let n1 = nodes[1];
            if kw(n1, "SOURce") {
                if query {
                    return t(&self.wav_source_text());
                }
                if let Some(id) = parse_wave(arg) {
                    self.wav_src = id;
                } else if !arg.is_empty() {
                    self.err.push(format!("-224,\"Illegal parameter value\" {}", arg));
                }
                return None;
            }
            if kw(n1, "MODE") {
                if query {
                    return t(if self.wav_raw { "RAW" } else { "NORM" });
                }
                if kw(arg.trim(), "RAW") { self.wav_raw = true; }
                else if kw(arg.trim(), "NORMal") { self.wav_raw = false; }
                else { self.err.push(format!("-224,\"Unsupported waveform mode\" {}", arg)); }
                return None;
            }
            if kw(n1, "FORMat") {
                if query {
                    return t(["BYTE", "WORD", "ASC"][self.wav_fmt as usize]);
                }
                let format = if kw(arg.trim(), "BYTE") { Some(0) } else if kw(arg.trim(), "WORD") { Some(1) }
                    else if kw(arg.trim(), "ASCii") { Some(2) } else { None };
                if let Some(format) = format { self.wav_fmt = format; }
                else { self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)); }
                return None;
            }
            let (pts, pre) = self.wave_params(ui);
            if kw(n1, "POINts") && query {
                return t(&format!("{}", pts));
            }
            if kw(n1, "PREamble") && query {
                return t(&pre.0);
            }
            if kw(n1, "DATA") && query {
                return Some(self.wave_data(ui, pre.1));
            }
        }
        if kw(n0, "REFerence") {
            return self.ref_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "SIM") {
            return self.sim_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "DISPlay") && nodes.len() == 2 && kw(nodes[1], "DATA") && query {
            return Some(block(&bmp(cv)));
        }
        // OOS extensions: analyser view, Identify, self-calibration, Bode
        if kw(n0, "VIEW") && nodes.len() == 1 {
            const VIEWS: [(&str, AView); 8] = [("SCOPe", AView::Scope), ("SPECtrum", AView::Spectrum), ("EYE", AView::Eye), ("POWer", AView::Power), ("BODE", AView::Bode), ("MAGnetic", AView::Magnetic), ("INVerter", AView::Inverter), ("DPT", AView::Dpt)];
            if query {
                return t(&format!("{:?}", ui.m.view).to_ascii_uppercase());
            }
            match VIEWS.iter().find(|(k, _)| kw(arg, k)) {
                Some((_, v)) => ui.set_analysis_view(*v),
                None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
            }
            return None;
        }
        if kw(n0, "IDENtify") && query {
            return t(&ui.identify_text());
        }
        if kw(n0, "CALibrate") && nodes.len() == 2 {
            if kw(nodes[1], "STATus") && query {
                return t(&ui.cal_status_text());
            }
            // :CALibrate:STARt 0..4[,ZERO]   (0 = all channels; the inputs must be disconnected)
            if kw(nodes[1], "STARt") {
                let mut it = arg.split(',');
                let ch = it.next().unwrap_or("0").trim();
                ui.selfcal_remote(ch, it.next().is_some_and(|z| kw(z.trim(), "ZERO")));
                return None;
            }
            if kw(nodes[1], "CANCel") {
                ui.selfcal_remote("cancel", false);
                return None;
            }
        }
        if kw(n0, "BODE") && nodes.len() == 2 {
            if kw(nodes[1], "RUN") {
                ui.bode_run(parse_bool(arg).unwrap_or(true));
                return None;
            }
            if kw(nodes[1], "DATA") && query {
                return t(&ui.bode_text());
            }
            if kw(nodes[1], "MODE") {
                const MODES: [(&str, oos_dsp::bode::Mode); 3] = [("LOOP", oos_dsp::bode::Mode::Loop), ("PSRR", oos_dsp::bode::Mode::Psrr), ("IMPedance", oos_dsp::bode::Mode::Z)];
                if query {
                    let name = match ui.m.bode_mode {
                        oos_dsp::bode::Mode::Loop => "LOOP",
                        oos_dsp::bode::Mode::Psrr => "PSRR",
                        oos_dsp::bode::Mode::Z => "IMP",
                    };
                    return t(name);
                }
                match MODES.iter().find(|(k, _)| kw(arg, k)) {
                    Some((_, m)) => {
                        ui.m.bode_mode = *m;
                        ui.mark_changed();
                    }
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            if kw(nodes[1], "GENerator") {
                if query {
                    return t(&ui.m.bode_addr);
                }
                // one word, like the setup file keeps it
                let a = arg.trim().trim_matches('"');
                if a.contains(char::is_whitespace) {
                    self.err.push(format!("-224,\"Illegal parameter value\" {}", arg));
                } else {
                    ui.m.bode_addr = a.to_string();
                }
                return None;
            }
        }
        if suffix(n0).1.is_none_or(|n| (1..=gen_count().max(1)).contains(&n)) && kw(suffix(n0).0, "SOURce") && nodes.len() >= 2 {
            if !oos_hal::model::platform().caps.has_gen() {
                self.err.push(format!("-241,\"Hardware missing\" no generator on the {}", oos_hal::model::platform().caps.name));
                return None;
            }
            // :SOURce<n> talks to generator n, whichever one the page shows
            let g = suffix(n0).1.unwrap_or(1) - 1;
            if g != ui.m.gen_sel {
                let shown = ui.m.gen_sel;
                ui.m.gen_select(g);
                let r = self.exec_cmd(ui, cv, cmd);
                ui.m.gen_select(shown);
                return r;
            }
            let has = |k: &str| nodes[1..].iter().any(|n| kw(suffix(n).0, k));
            let m = &ui.m;
            let x = arg.trim().parse::<f64>().ok().filter(|v| v.is_finite());
            if has("OUTPut") {
                if query {
                    return t(if m.afg_on { "1" } else { "0" });
                }
                match parse_bool(arg) {
                    Some(on) => ui.afg_set(Some(on), None, None, None),
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            // modulation (APPL style): :MOD[:STATe] ON|OFF, :MOD:TYPe AM|FM|PM, :MOD:AM:DEPTh <%>, :MOD:FM:DEViation <Hz>,
            // :MOD:PM:DEViation <deg>, :MOD:<type>:INTernal:FREQuency <Hz>, :MOD:<type>:INTernal:FUNCtion <shape>
            if has("MOD") {
                const SHAPES: [&str; 6] = ["SINusoid", "SQUare", "TRIangle", "RAMP", "NRAMp", "NOISe"];
                let a = arg.trim();
                if has("INTernal") && has("FUNCtion") {
                    if query {
                        return t(&SHAPES[m.afg_mod_shape.min(5)].chars().filter(|c| c.is_ascii_uppercase()).collect::<String>());
                    }
                    match SHAPES.iter().position(|s| kw(a, s)) {
                        Some(i) => ui.m.afg_mod_shape = i,
                        None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                    }
                    return None;
                }
                let (cur, key): (f64, u8) = if has("INTernal") && has("FREQuency") {
                    (m.afg_mod_rate, 0)
                } else if has("DEPTh") {
                    (m.afg_mod_depth, 1)
                } else if has("DEViation") && has("PM") {
                    (m.afg_pm_dev, 3)
                } else if has("DEViation") {
                    (m.afg_fm_dev, 2)
                } else if has("TYPe") {
                    if query {
                        return t(["NONE", "AM", "FM", "PM"][m.afg_mod.min(3)]);
                    }
                    match ["AM", "FM", "PM"].iter().position(|k| kw(a, k)) {
                        Some(i) => ui.m.afg_mod = i + 1,
                        None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                    }
                    ui.afg_set(None, None, None, None);
                    return None;
                } else {
                    // :MOD[:STATe]
                    if query {
                        return t(if m.afg_mod > 0 { "1" } else { "0" });
                    }
                    match parse_bool(arg) {
                        Some(on) => ui.m.afg_mod = if on { ui.m.afg_mod.max(1) } else { 0 },
                        None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                    }
                    return None;
                };
                if query {
                    return t(&e(cur));
                }
                match x {
                    Some(v) => {
                        let mm = &mut ui.m;
                        match key {
                            0 => mm.afg_mod_rate = v.clamp(0.002, 1e6),
                            1 => mm.afg_mod_depth = v.clamp(0.0, 120.0),
                            2 => mm.afg_fm_dev = v.max(0.01),
                            _ => mm.afg_pm_dev = v.clamp(0.0, 360.0),
                        }
                        ui.afg_set(None, None, None, None);
                    }
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            // :FUNCtion:SQUare:DUTY / :FUNCtion:PULSe:DUTY and :FUNCtion:RAMP:SYMMetry in %
            // arbitrary wave: :ARB:DATA <v>,<v>,... (one period, any scale), :ARB:CAPTure CHAN<n> (the screen of a channel)
            if has("ARB") {
                if has("DATA") && !query {
                    let v: Vec<f32> = arg.split(',').filter_map(num).collect();
                    if !ui.afg_arb_load(&v) {
                        self.err.push(format!("-224,\"Illegal parameter value\" {} samples", v.len()));
                    }
                } else if has("CAPTure") && !query {
                    match parse_ch(arg).ok_or("no channel".to_string()).and_then(|c| ui.afg_arb_capture(c)) {
                        Ok(_) => {}
                        Err(e) => self.err.push(format!("-224,\"Illegal parameter value\" {}", e)),
                    }
                } else {
                    self.err.push("-113,\"Undefined header\" :SOURce:ARB".into());
                }
                return None;
            }
            if has("FUNCtion") && (has("DUTY") || has("SYMMetry")) {
                let duty = has("DUTY");
                if query {
                    return t(&e(if duty { m.afg_duty } else { m.afg_symmetry }));
                }
                match x {
                    Some(v) if duty => ui.m.afg_duty = v.clamp(5.0, 95.0),
                    Some(v) => ui.m.afg_symmetry = v.clamp(0.0, 100.0),
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            if has("FUNCtion") {
                const SHORT: [&str; 7] = ["SIN", "SQU", "RAMP", "PULS", "NOIS", "DC", "ARB"];
                const LONG: [&str; 7] = ["SINusoid", "SQUare", "RAMP", "PULSe", "NOISe", "DC", "ARB"];
                if query {
                    return t(SHORT[m.afg_wave.min(6)]);
                }
                match LONG.iter().position(|l| kw(arg.trim(), l)) {
                    Some(i) => {
                        ui.m.afg_wave = i;
                        ui.afg_set(None, None, None, None);
                    }
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            let (want, cur): (u8, f64) = if has("FREQuency") {
                (0, m.afg_freq)
            } else if has("VOLTage") && has("OFFSet") {
                (2, m.afg_offset)
            } else if has("VOLTage") {
                (1, m.afg_ampl)
            } else {
                (9, 0.0)
            };
            if want < 9 {
                if query {
                    return t(&e(cur));
                }
                match x {
                    Some(v) => ui.afg_set(None, (want == 0).then_some(v), (want == 1).then_some(v), (want == 2).then_some(v)),
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
        }
        // test access: any key of a setup file on the live settings (`:SYST:SET ch1.bw = 20M`), its value, the state
        if kw(n0, "SYSTem") && nodes.len() == 2 {
            if kw(nodes[1], "SET") && !query {
                let r = arg.split_once('=').ok_or("no '='".to_string()).and_then(|(k, v)| crate::store::set(&mut ui.m, k.trim(), v.trim()));
                match r {
                    Ok(()) => ui.mark_changed(),
                    Err(e) => self.err.push(format!("-224,\"Illegal parameter value\" {}", e)),
                }
                // a setup file may hold keys of newer versions, here a typo must not pass for a setting; the keys that
                // are only written when they differ from their default do not show
                let k = arg.split('=').next().unwrap_or("").trim();
                let optional = ["vfine", "unit", "bias", "skew", "label", "tfine"].iter().any(|s| k.ends_with(s)) || k.starts_with("math_expr") || k == "cat";
                if !optional && !crate::store::setup_text(&ui.m).lines().any(|l| l.split('=').next().unwrap_or("").trim() == k) {
                    self.err.push(format!("-224,\"Illegal parameter value\" unknown key {}", k));
                }
                return None;
            }
            if kw(nodes[1], "GET") && query {
                let text = crate::store::setup_text(&ui.m);
                let v = text.lines().find_map(|l| l.split_once('=').filter(|(k, _)| k.trim() == arg.trim()).map(|x| x.1.trim().to_string()));
                return t(&v.unwrap_or_default());
            }
            if kw(nodes[1], "STATe") && query {
                return t(crate::cmd::stat_text(ui).trim());
            }
            if kw(nodes[1], "LOG") && !query {
                match parse_bool(arg) {
                    Some(on) => ui.log_set(on),
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
        }
        if kw(n0, "DECode") && nodes.len() == 2 && kw(nodes[1], "EVENts") && query {
            return t(&ui.events_text());
        }
        if kw(n0, "SYSTem") && nodes.len() == 2 && kw(nodes[1], "ERRor") && query {
            return t(&if self.err.is_empty() { "0,\"No error\"".to_string() } else { self.err.remove(0) });
        }
        self.err.push(format!("-113,\"Undefined header\" {}", cmd));
        None
    }

    fn decode_cmd(&mut self, ui: &mut Ui, node: &str, query: bool, arg: &str) -> Option<Vec<u8>> {
        if kw(node, "SRATe") { return self.decode_cphy_rate_cmd(ui, query, arg); }
        let text = |value: String| Some(value.into_bytes());
        let illegal = |s: &mut Self| { s.err.push(format!("-224,\"Illegal decoder parameter\" {}", arg)); None };
        if kw(node, "EVENts") && query {
            return match ui.decode_events_result() {
                Ok(events) => text(events),
                Err("Decode computing") => text("BUSY".into()),
                Err(error) => { self.err.push(format!("-230,\"{}\"", error)); None }
            };
        }
        if kw(node, "BUSY") && query { return text(if ui.decode_busy() { "1" } else { "0" }.into()); }
        let role = if kw(node, "SOURce") || kw(node, "SOURce1") || node.eq_ignore_ascii_case("SOUR1") { Some(0) }
            else if kw(node, "SOURce2") || node.eq_ignore_ascii_case("SOUR2") { Some(1) }
            else if kw(node, "CS") { Some(2) }
            else if kw(node, "SOURce4") || node.eq_ignore_ascii_case("SOUR4") { Some(3) }
            else if kw(node, "DIFFerential") { Some(4) } else { None };
        if let Some(role) = role {
            if query {
                if ui.m.dec_on_la() && role < 4 {
                    return text(ui.m.dec_la[role].map_or("NONE".into(), |line| format!("D{line}")));
                }
                let source = match role { 0 => Some(ui.m.dec_src), 1 => Some(ui.m.dec_src2), 2 => ui.m.dec_cs, 3 => ui.m.dec_src4, _ => ui.m.dec_diff };
                return text(source.map_or("NONE".into(), decode_source_name));
            }
            if ui.m.dec_on_la() && role < 4 {
                let source = arg.strip_prefix('D').or_else(|| arg.strip_prefix('d')).and_then(|v| v.parse::<u8>().ok()).filter(|&v| v < 16);
                if source.is_none() && !(role >= 2 && arg.eq_ignore_ascii_case("NONE")) { return illegal(self); }
                ui.m.dec_la[role] = source;
                ui.mark_changed();
                return None;
            }
            let source = if arg.eq_ignore_ascii_case("NONE") { None }
                else { match parse_wave(arg) { Some(WaveId::Ch(c)) => Some(c), Some(WaveId::Ref(i)) => Some(DEC_REF_BASE + i), None => return illegal(self) } };
            if !ui.configure_decoder_source(role, source) { return illegal(self); }
            ui.mark_changed();
            return None;
        }
        if kw(node, "PROTocol") || node.eq_ignore_ascii_case("PROTO") {
            if query { return text(ui.m.dec_proto.name().into()); }
            let compact = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).flat_map(char::to_uppercase).collect::<String>();
            let name = compact(arg);
            let Some(protocol) = PROTOS.iter().copied().find(|p| compact(p.name()) == name) else { return illegal(self) };
            // SCPI configures an instrument, not the demo's suggested sources/trigger.
            ui.m.dec_proto = protocol;
            if protocol == Proto::Cphy { ui.m.dec_thr = 0.02; ui.m.dec_la_in = false; }
            ui.mark_changed();
            return None;
        }
        if kw(node, "STATe") {
            if query { return text(if ui.m.dec_on { "1" } else { "0" }.into()); }
            let Some(on) = parse_bool(arg) else { return illegal(self) };
            ui.m.dec_on = on;
            ui.mark_changed();
            return None;
        }
        if kw(node, "SELect") {
            if query { return text((ui.m.dec_sel + 1).to_string()); }
            let Some(slot) = arg.parse::<usize>().ok().filter(|n| (1..=DECODERS).contains(n)) else { return illegal(self) };
            ui.m.dec_select(slot - 1);
            return None;
        }
        if kw(node, "THReshold") {
            if query { return text(e(ui.m.dec_thr as f64)); }
            let Some(threshold) = num(arg) else { return illegal(self) };
            ui.m.dec_thr = threshold;
            ui.mark_changed();
            return None;
        }
        if kw(node, "INPut") {
            if query { return text(if ui.m.dec_la_in { "LA" } else { "WAVEFORM" }.into()); }
            if arg.eq_ignore_ascii_case("LA") && ui.m.la_on() && ui.m.dec_proto != Proto::Cphy { ui.m.dec_la_in = true; }
            else if arg.eq_ignore_ascii_case("WAVEFORM") || arg.eq_ignore_ascii_case("ANALOG") { ui.m.dec_la_in = false; }
            else { return illegal(self); }
            ui.mark_changed();
            return None;
        }
        self.err.push(format!("-113,\"Undefined header\" DECode:{}", node));
        None
    }

    fn decode_cphy_rate_cmd(&mut self, ui: &mut Ui, query: bool, arg: &str) -> Option<Vec<u8>> {
        if query { return Some(if ui.m.dec_cphy_rate == 0.0 { b"AUTO".to_vec() } else { e(ui.m.dec_cphy_rate).into_bytes() }); }
        match crate::decode::parse_cphy_rate(arg) {
            Ok(rate) => { ui.m.dec_cphy_rate = rate; ui.mark_changed(); }
            Err(_) => self.err.push(format!("-224,\"Invalid C-PHY symbol rate\" {}", arg)),
        }
        None
    }

    fn wav_source_text(&self) -> String {
        match self.wav_src {
            WaveId::Ch(c) => format!("CHAN{}", c + 1),
            WaveId::Ref(i) => format!("R{}", i + 1),
        }
    }

    /// (points, (preamble string, (yinc, yorigin, yref)))
    fn wave_params(&self, ui: &Ui) -> (usize, (String, (f64, f64, f64))) {
        let (pts, dt, t0, vpd, offset) = match self.wav_src {
            WaveId::Ch(c) => {
                let ch = &ui.m.ch[c];
                let info = if self.wav_raw { ui.raw_info(c) } else { None };
                let (pts, dt, t0) = info.or_else(|| ui.record_info(c)).unwrap_or((0, 0.0, 0.0));
                (pts, dt, t0, ch.volts_per_div() as f64, ch.offset as f64)
            }
            WaveId::Ref(i) => ui.ref_info(i).unwrap_or((0, 0.0, 0.0, 1.0, 0.0)),
        };
        let (yinc, yref) = match self.wav_fmt {
            1 => (vpd * VDIV as f64 / 60000.0, 32768.0),
            _ => (vpd * VDIV as f64 / 250.0, 128.0),
        };
        let yorg = if yinc == 0.0 { 0.0 } else { (offset / yinc).round() };
        let pre = format!("{},{},{},1,{},{},0,{},{},{}", self.wav_fmt, if self.wav_raw { 2 } else { 0 }, pts, e(dt), e(t0), e(yinc), yorg, yref);
        (pts, (pre, (yinc, yorg, yref)))
    }

    /// `:LA:...` (the stock names where they exist): state, lines shown, pod thresholds, and the record of the last
    /// acquisition: `:LA:DATA?` is an IEEE block of u16 words, little endian, bit n = Dn, sample k at
    /// t0 + k dt from `:LA:PREamble?` (points,dt,t0).
    fn la_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        use crate::ui::la;
        let t = |s: String| Some(s.into_bytes());
        if !la::available() {
            self.err.push(format!("-241,\"Hardware missing\" no logic analyser port on the {}", oos_hal::model::platform().caps.name));
            return None;
        }
        let bad = |e: &mut Vec<String>| {
            e.push(format!("-224,\"Illegal parameter value\" {}", arg));
            None
        };
        let (k, idx) = suffix(n[0]);
        if n.len() == 1 && (kw(n[0], "STATe") || kw(n[0], "ENABle")) {
            if query {
                return t(if ui.m.la.on { "1" } else { "0" }.into());
            }
            match parse_bool(arg) {
                Some(on) => {
                    ui.m.la.on = on;
                    ui.mark_changed();
                    return None;
                }
                None => return bad(&mut self.err),
            }
        }
        if n.len() == 2 && kw(k, "DIGital") && kw(n[1], "DISPlay") {
            let Some(line) = idx.filter(|&i| i < 16) else { return bad(&mut self.err) };
            if query {
                return t((ui.m.la.show >> line & 1).to_string());
            }
            match parse_bool(arg) {
                Some(on) => {
                    ui.m.la.show = ui.m.la.show & !(1 << line) | (on as u16) << line;
                    return None;
                }
                None => return bad(&mut self.err),
            }
        }
        if n.len() == 2 && kw(k, "POD") && kw(n[1], "THReshold") {
            let Some(pod) = idx.filter(|&i| i == 1 || i == 2).map(|i| i - 1) else { return bad(&mut self.err) };
            if query {
                return t(format!("{:e}", ui.m.la.thr(pod)));
            }
            match arg.trim().parse::<f32>() {
                Ok(v) if v.abs() <= la::THR_MAX => {
                    (ui.m.la.preset[pod], ui.m.la.user[pod]) = (la::USER, v);
                    ui.mark_changed();
                    return None;
                }
                _ => return bad(&mut self.err),
            }
        }
        if n.len() == 1 && query && (kw(n[0], "POINts") || kw(n[0], "PREamble") || kw(n[0], "DATA")) {
            let Some(r) = ui.la_record() else {
                self.err.push("-230,\"Data corrupt or stale\" no LA record".into());
                return if kw(n[0], "DATA") { Some(block(&[])) } else { t("0".into()) };
            };
            if kw(n[0], "POINts") {
                return t(r.words.len().to_string());
            }
            if kw(n[0], "PREamble") {
                return t(format!("{},{:e},{:e}", r.words.len(), r.dt, r.t0));
            }
            return Some(block(&r.words.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>()));
        }
        self.err.push(format!("-113,\"Undefined header\" :LA:{}", n.join(":")));
        None
    }

    fn waves(&mut self, arg: &str) -> Result<Vec<WaveId>, String> {
        let mut v = Vec::new();
        for s in arg.split(',') {
            let s = s.trim();
            if s.is_empty() {
                continue;
            }
            // the item name is not a source; callers pass only the source list
            match parse_wave(s) {
                Some(id) => v.push(id),
                None => return Err(format!("-224,\"Illegal parameter value\" {}", s)),
            }
        }
        Ok(v)
    }

    fn sourced(&mut self, ui: &Ui, arg: &str, f: impl FnOnce(&Ui, &str, &[WaveId]) -> Result<f64, ()>) -> Option<Vec<u8>> {
        let src = match self.waves(arg_rest(arg)) {
            Ok(v) => v,
            Err(s) => {
                self.err.push(s);
                return Some(b"9.9E37".to_vec());
            }
        };
        self.one_fact(f(ui, &arg_item(arg), &src))
    }

    fn one_fact(&mut self, r: Result<f64, ()>) -> Option<Vec<u8>> {
        match r {
            Ok(v) => Some(shown(v).into_bytes()),
            Err(()) => {
                self.err.push("-224,\"Illegal parameter value\"".into());
                Some(b"9.9E37".to_vec())
            }
        }
    }

    fn ref_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        if kw(n0, "SLOT") {
            if query {
                return t(&format!("R{}", ui.ref_slot() + 1));
            }
            return match parse_slot(arg) {
                Some(i) => {
                    ui.ref_slot_set(i);
                    ui.mark_changed();
                    None
                }
                None => {
                    self.err.push(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if kw(n0, "SHOW") {
            if query {
                return t(if ui.ref_shown() == Some(true) { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) if ui.ref_show_set(on) => {
                    ui.mark_changed();
                    None
                }
                _ => {
                    self.err.push(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if kw(n0, "CLEar") && !query {
            ui.ref_clear_all();
            ui.mark_changed();
            return None;
        }
        if kw(n0, "SAVE") && !query {
            return match ui.ref_save_slot() {
                Ok(()) => {
                    ui.mark_changed();
                    None
                }
                Err(e) => {
                    self.err.push(format!("-224,\"Illegal parameter value\" {}", e));
                    None
                }
            };
        }
        if kw(n0, "LOAD") && !query {
            if arg.trim().is_empty() {
                self.err.push("-224,\"Illegal parameter value\" no file".into());
                return None;
            }
            let r = ui.ref_load_named(arg);
            return self.ref_loaded(ui, r);
        }
        if kw(n0, "IMPort") && !query {
            let r = if arg.trim().is_empty() { ui.ref_import_slot() } else { ui.ref_load_named(arg) };
            return self.ref_loaded(ui, r);
        }
        self.err.push(format!("-113,\"Undefined header\" :REF:{}", n.join(":")));
        None
    }

    fn ref_loaded(&mut self, ui: &mut Ui, r: Result<(), String>) -> Option<Vec<u8>> {
        match r {
            Ok(()) => {
                ui.mark_changed();
                None
            }
            Err(e) if e.starts_with("missing ") => {
                self.err.push(format!("-256,\"File name not found\" {}", e.trim_start_matches("missing ")));
                None
            }
            Err(e) => {
                self.err.push(format!("-224,\"Illegal parameter value\" {}", e));
                None
            }
        }
    }

    fn sim_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "BUSY") && query {
            return t(if ui.sim_generating() { "1" } else { "0" });
        }
        if n.len() == 2 && kw(n0, "SSC") && kw(n[1], "MODE") {
            if query {
                return t(if !ui.m.sim_ssc { "OFF" } else if ui.m.sim_ssc_center { "CENTER" } else { "DOWN" });
            }
            match arg.trim().to_ascii_uppercase().as_str() {
                "OFF" => ui.m.sim_ssc = false,
                "DOWN" => { ui.m.sim_ssc = true; ui.m.sim_ssc_center = false; }
                "CENTER" | "CENT" => { ui.m.sim_ssc = true; ui.m.sim_ssc_center = true; }
                _ => { self.err.push("-224,\"Illegal parameter value\" SSC mode".into()); return None; }
            }
            ui.mark_changed();
            return None;
        }
        if n.len() == 2 && (kw(n0, "SSC") || kw(n0, "JITTer")) {
            let setting = if kw(n0, "SSC") {
                if kw(n[1], "DEViation") { Some((&mut ui.m.sim_ssc_deviation, 0.0, 0.05)) }
                else if kw(n[1], "FREQuency") { Some((&mut ui.m.sim_ssc_hz, 1e3, 1e6)) }
                else { None }
            } else if kw(n[1], "DCD") { Some((&mut ui.m.sim_dcd_pp, 0.0, 100e-12)) }
            else if kw(n[1], "CORRelated") { Some((&mut ui.m.sim_corr_rj, 0.0, 50e-12)) }
            else if kw(n[1], "CORNer") { Some((&mut ui.m.sim_corr_hz, 1e3, 100e6)) }
            else { None };
            if let Some((value, min, max)) = setting {
                if query { return t(&e(*value)); }
                match arg.trim().parse::<f64>() {
                    Ok(v) if v.is_finite() && v >= min && v <= max => *value = v,
                    _ => { self.err.push("-224,\"Illegal parameter value\" SIM setting".into()); return None; }
                }
                ui.mark_changed();
                return None;
            }
        }
        if n.len() == 1 && (kw(n0, "PATTern") || n0.eq_ignore_ascii_case("PAT")) {
            if query { return t(&format!("PRBS{}", ui.m.sim_prbs)); }
            let a = arg.trim().to_ascii_uppercase();
            let order = a.strip_prefix("PRBS").unwrap_or(&a).trim_start_matches('-').parse::<u8>().ok();
            if let Some(p @ (7 | 9 | 13 | 15 | 23 | 31)) = order {
                ui.m.sim_prbs = p;
                ui.mark_changed();
            } else {
                self.err.push("-224,\"Illegal parameter value\" PRBS order".into());
            }
            return None;
        }
        if kw(n0, "PRESet") {
            if query {
                return t(ui.sim_preset_name());
            }
            return if ui.sim_preset_set(arg) {
                ui.mark_changed();
                None
            } else {
                self.err.push(format!("-224,\"Illegal parameter value\" {}", arg));
                None
            };
        }
        if kw(n0, "SLOT") {
            if query {
                return t(&format!("R{}", ui.sim_slot() + 1));
            }
            return match parse_slot(arg) {
                Some(i) => {
                    ui.sim_slot_set(i);
                    ui.mark_changed();
                    None
                }
                None => {
                    self.err.push(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if kw(n0, "PLAY") {
            if query {
                return t(if ui.sim_playing() { "1" } else { "0" });
            }
            let on = if arg.trim().is_empty() { Some(true) } else { parse_bool(arg) };
            return match on {
                Some(on) => {
                    match ui.sim_play_set(on) {
                        Ok(()) => ui.mark_changed(),
                        Err(error) => self.err.push(format!("-224,\"Illegal parameter value\" {}", error)),
                    }
                    None
                }
                None => {
                    self.err.push(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if kw(n0, "SHOW") {
            if query {
                return t(if ui.sim_shown() == Some(true) { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) if ui.sim_show_set(on) => {
                    ui.mark_changed();
                    None
                }
                _ => {
                    self.err.push(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        self.err.push(format!("-113,\"Undefined header\" :SIM:{}", n.join(":")));
        None
    }

    fn wave_data(&self, ui: &Ui, p: (f64, f64, f64)) -> Vec<u8> {
        let (yinc, yorg, yref) = p;
        let v = match self.wav_src {
            WaveId::Ch(c) => {
                if self.wav_raw { ui.raw_volts(c).unwrap_or_else(|| ui.record_volts(c)) } else { ui.record_volts(c) }
            }
            WaveId::Ref(i) => ui.ref_volts(i),
        };
        match self.wav_fmt {
            2 => {
                let s: Vec<String> = v.iter().map(|&x| e(x as f64)).collect();
                block(s.join(",").as_bytes())
            }
            1 => {
                let mut b = Vec::with_capacity(v.len() * 2);
                for x in v {
                    let code = ((x as f64 / yinc) + yorg + yref).round().clamp(0.0, 65535.0) as u16;
                    b.extend_from_slice(&code.to_le_bytes());
                }
                block(&b)
            }
            _ => {
                let b: Vec<u8> = v.iter().map(|&x| ((x as f64 / yinc) + yorg + yref).round().clamp(0.0, 255.0) as u8).collect();
                block(&b)
            }
        }
    }

    /// :MEASure:ITEM? <item>[,CHANn|Rn[,src]]
    fn meas_item(&mut self, ui: &Ui, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let mut parts = arg.split(',');
        let item = parts.next().unwrap_or("").trim();
        if item.is_empty() {
            self.err.push("-224,\"Illegal parameter value\"".into());
            return t("9.9E37");
        }
        if kw(item, "SETHold") || item.eq_ignore_ascii_case("SHOLD") {
            let waves = match self.waves(&parts.collect::<Vec<_>>().join(",")) {
                Ok(waves) => waves,
                Err(error) => {
                    self.err.push(error);
                    return t("9.9E37");
                }
            };
            let (clock, data) = match waves.as_slice() {
                [] => ui.meas_pair_sources(),
                [clock, data] => (*clock, *data),
                _ => {
                    self.err.push("-224,\"Illegal parameter value\"".into());
                    return t("9.9E37");
                }
            };
            return t(&shown(ui.fact_setup_hold(clock, data)));
        }
        // no source: CH1, as before. A word that is not CHANn or R1–R10 is an error.
        let src = match parts.next().map(str::trim).filter(|s| !s.is_empty()) {
            None => WaveId::Ch(0),
            Some(s) => match self.named_source(s) {
                Some(id) => id,
                None => return t("9.9E37"),
            },
        };
        if let Some(k) = meas_kind(item) {
            return t(&shown(ui.fact_meas(k, src)));
        }
        if let Some(k) = pair_kind(item) {
            let Some(word) = parts.next().map(str::trim).filter(|s| !s.is_empty()) else {
                self.err.push("-224,\"Illegal parameter value\"".into());
                return t("9.9E37");
            };
            let Some(b) = self.named_source(word) else { return t("9.9E37") };
            return t(&e(ui.pair_at(k, src, b)));
        }
        self.err.push("-224,\"Illegal parameter value\"".into());
        t("9.9E37")
    }

    fn named_source(&mut self, s: &str) -> Option<WaveId> {
        match parse_wave(s) {
            Some(id) => Some(id),
            None => {
                self.err.push(format!("-224,\"Illegal parameter value\" {}", s));
                None
            }
        }
    }

    /// :EYE:ITEM? <figure> — height, width, unit interval, total jitter, and the other figures on the eye page.
    fn eye_item(&mut self, ui: &Ui, arg: &str) -> Option<Vec<u8>> {
        let item = arg.split(',').next().unwrap_or("").trim();
        let Some(name) = crate::ui::EYE_ITEMS.iter().find(|n| kw(item, n)) else {
            self.err.push("-224,\"Illegal parameter value\"".into());
            return Some(b"9.9E37".to_vec());
        };
        let v = ui.eye_readings().iter().find(|(n, _)| n == name).map(|(_, v)| *v).unwrap_or(f64::NAN);
        Some(e(v).into_bytes())
    }
}

/// The measurement strip: each row, then the DVM and the counter when they are on.
fn meas_strip(ui: &Ui) -> String {
    let mut p = Vec::new();
    for &(k, id) in &ui.m.meas {
        p.push(format!("{},{},{}", meas_token(k), wave_tok(id), e(ui.fact_meas(k, id))));
    }
    for &(k, a, b) in &ui.m.meas2 {
        p.push(format!("{},{},{},{}", pair_token(k), wave_tok(a), wave_tok(b), e(ui.pair_at(k, a, b))));
    }
    for (id,[a,b,c,d],value) in ui.catalog_export_values() {
        p.push(format!("CAT{},{},{},{},{},{}",id,wave_tok(a),wave_tok(b),wave_tok(c),wave_tok(d),e(value)));
    }
    if ui.m.dvm_on {
        let mode = ["DC", "ACRMS", "ACDC"][ui.m.dvm_mode.min(2)];
        p.push(format!("DVM,{},{},{}", mode, wave_tok(WaveId::Ch(ui.m.dvm_src.min(NCH - 1))), e(ui.dvm_value())));
    }
    if ui.m.cnt_on {
        let mode = ["FREQUENCY", "PERIOD", "TOTALIZE"][ui.m.cnt_mode.min(2)];
        p.push(format!("COUNTER,{},{},{}", mode, wave_tok(WaveId::Ch(ui.m.cnt_src.min(NCH - 1))), e(ui.counter_value())));
    }
    p.join(";")
}

/// Strip rows, and the eye figures when the eye page is the one on screen.
fn meas_readout(ui: &Ui) -> String {
    let mut parts = Vec::new();
    let strip = meas_strip(ui);
    if !strip.is_empty() {
        parts.push(strip);
    }
    if ui.m.view == AView::Eye {
        let eye = crate::ui::EYE_ITEMS
            .iter()
            .map(|name| {
                let v = ui.eye_readings().iter().find(|(n, _)| n == name).map(|(_, v)| *v).unwrap_or(f64::NAN);
                format!("{},{}", name.to_ascii_uppercase(), e(v))
            })
            .collect::<Vec<_>>()
            .join(";");
        parts.push(eye);
    }
    parts.join(";")
}

/// IEEE 488.2 definite-length block: #9 + 9 digits + data.
fn block(d: &[u8]) -> Vec<u8> {
    let mut o = format!("#9{:09}", d.len()).into_bytes();
    o.extend_from_slice(d);
    o
}

/// 24-bit BMP of the canvas (what :DISPlay:DATA? returns on the stock firmware).
pub fn bmp(cv: &Canvas) -> Vec<u8> {
    let (w, h) = (cv.w as usize, cv.h as usize);
    let row = (w * 3 + 3) & !3;
    let size = 54 + row * h;
    let mut b = Vec::with_capacity(size);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&(size as u32).to_le_bytes());
    b.extend_from_slice(&[0, 0, 0, 0]);
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&(w as i32).to_le_bytes());
    b.extend_from_slice(&(h as i32).to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    b.extend_from_slice(&[0; 24]);
    for y in (0..h).rev() {
        for x in 0..w {
            let p = cv.px[y * w + x];
            b.extend_from_slice(&[p as u8, (p >> 8) as u8, (p >> 16) as u8]);
        }
        b.resize(b.len() + row - w * 3, 0);
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wave::Source;

    fn session() -> (Scpi, Ui, Canvas) {
        (Scpi::open(0), Ui::new(Source::Demo { t: 0.0 }), Canvas::new(crate::ui::W, crate::ui::H))
    }

    fn line(s: &mut Scpi, ui: &mut Ui, cv: &Canvas, cmd: &str) -> String {
        String::from_utf8(s.exec_line(ui, cv, cmd)).unwrap()
    }

    #[test]
    fn compatibility_trigger_modes_roundtrip_actual_types() {
        let (mut s,mut ui,cv)=reference_session();
        for (token,reply,kind) in [("EDGE","EDGE",TrigType::Edge),("PULSe","PULS",TrigType::Pulse),("SLOPe","SLOP",TrigType::Slope),
            ("VIDeo","VID",TrigType::Video),("PATTern","PATT",TrigType::Pattern),("DURation","DUR",TrigType::Duration),
            ("TIMeout","TIM",TrigType::Timeout),("RUNT","RUNT",TrigType::Runt),("WINDow","WIND",TrigType::Window),
            ("DELay","DEL",TrigType::Delay),("SETup","SET",TrigType::SetupHold),("NEDGe","NEDG",TrigType::NthEdge),("BUS","BUS",TrigType::Bus)] {
            line(&mut s,&mut ui,&cv,&format!(":TRIG:MODE {token}"));assert_eq!(ui.m.trig_type,kind);
            assert_eq!(line(&mut s,&mut ui,&cv,":TRIG:MODE?"),reply);
            line(&mut s,&mut ui,&cv,&format!(":TRIG:MODE {reply}"));
            assert_eq!(line(&mut s,&mut ui,&cv,":TRIG:TYPE?"),kind.name());assert_eq!(ui.m.trig_ref,Some(0));
        }
        assert!(line(&mut s,&mut ui,&cv,":SYST:ERR?").starts_with("0,"));
        line(&mut s,&mut ui,&cv,":TRIG:MODE MADEUP");assert_eq!(ui.m.trig_type,TrigType::Bus);
        assert!(line(&mut s,&mut ui,&cv,":SYST:ERR?").starts_with("-224,"));
    }

    #[test]
    fn compatibility_waveform_metadata_and_invalid_enums() {
        let (mut s,mut ui,cv)=reference_session();line(&mut s,&mut ui,&cv,":WAV:SOUR R1;:WAV:FORM WORD");
        for (mode,kind) in [("RAW",2),("NORM",0)] {
            line(&mut s,&mut ui,&cv,&format!(":WAV:MODE {mode}"));
            let pre=line(&mut s,&mut ui,&cv,":WAV:PRE?");let f:Vec<_>=pre.split(',').collect();
            assert_eq!(f.len(),10);assert_eq!(f[0],"1");assert_eq!(f[1],kind.to_string());assert_eq!(f[2],"4000");
            assert!((f[4].parse::<f64>().unwrap()-1e-9).abs()<1e-15);
            assert_eq!(s.exec_line(&mut ui,&cv,":WAV:DATA?").len(),8011);
        }
        line(&mut s,&mut ui,&cv,":WAV:MODE RAW");
        for command in [":WAV:MODE MAX",":WAV:MODE WRONG",":WAV:MODE RAWsuffix",":WAV:FORM WRONG",":WAV:FORM WORDsuffix"] {
            line(&mut s,&mut ui,&cv,command);assert_eq!(line(&mut s,&mut ui,&cv,":WAV:MODE?;:WAV:FORM?"),"RAW;WORD");
            assert!(line(&mut s,&mut ui,&cv,":SYST:ERR?").starts_with("-224,"),"{command}");
        }
    }

    #[test]
    fn compatibility_opc_single_waits_across_ticks_and_resumes_without_replaying_writes() {
        let (mut s,mut ui,cv)=reference_session();line(&mut s,&mut ui,&cv,":TRIG:EDGE:LEV 10");
        let mut p=Program::new(":SINGLE;:TRIG:STAT?;*OPC?;:TRIG:STAT?");assert!(s.poll_program(&mut ui,&cv,&mut p).is_none());
        for _ in 0..3 {ui.step(0.2);assert!(s.poll_program(&mut ui,&cv,&mut p).is_none());}
        assert_eq!(captures(&ui),0.0);line(&mut s,&mut ui,&cv,":TRIG:FORC");ui.step(0.2);
        assert_eq!(s.poll_program(&mut ui,&cv,&mut p),Some(b"WAIT;1;STOP".to_vec()));assert_eq!(captures(&ui),1.0);
        ui.step(0.2);assert_eq!(captures(&ui),1.0);line(&mut s,&mut ui,&cv,":RUN");
        assert_eq!(line(&mut s,&mut ui,&cv,"*OPC?"),"1");
    }

    #[test]
    fn compatibility_opc_events_clear_reset_and_immediate_adapter() {
        let (mut s,mut ui,cv)=reference_session();line(&mut s,&mut ui,&cv,":TRIG:EDGE:LEV 10;:SINGLE;*OPC");
        assert_eq!(line(&mut s,&mut ui,&cv,"*ESR?"),"0");line(&mut s,&mut ui,&cv,"*CLS");
        line(&mut s,&mut ui,&cv,":TRIG:EDGE:LEV 0");ui.step(0.2);assert_eq!(line(&mut s,&mut ui,&cv,"*ESR?"),"0");
        line(&mut s,&mut ui,&cv,":TRIG:EDGE:LEV 10;:SINGLE;*OPC");assert_eq!(line(&mut s,&mut ui,&cv,"*OPC?"),"");
        assert!(line(&mut s,&mut ui,&cv,":SYST:ERR?").starts_with("-221,"));
        line(&mut s,&mut ui,&cv,"*RST");assert_eq!(line(&mut s,&mut ui,&cv,"*ESR?"),"0");
        line(&mut s,&mut ui,&cv,":STOP;*OPC");assert_eq!(line(&mut s,&mut ui,&cv,"*ESR?;*ESR?"),"1;0");
    }

    #[test]
    fn compatibility_raw_tcp_pending_opc_does_not_block_other_clients() {
        let (mut s,mut ui,cv)=reference_session();line(&mut s,&mut ui,&cv,":TRIG:EDGE:LEV 10");
        let mut waiting=TcpStream::connect(("127.0.0.1",s.port().unwrap())).unwrap();waiting.set_nonblocking(true).unwrap();
        waiting.write_all(b":SINGLE;*OPC?;:TRIG:STAT?\n").unwrap();s.poll(&mut ui,&cv);ui.step(0.2);s.poll(&mut ui,&cv);
        let mut bytes=[0;128];assert_eq!(waiting.read(&mut bytes).unwrap_err().kind(),ErrorKind::WouldBlock);
        let mut control=TcpStream::connect(("127.0.0.1",s.port().unwrap())).unwrap();control.write_all(b":TRIG:FORC\n").unwrap();
        s.poll(&mut ui,&cv);ui.step(0.2);s.poll(&mut ui,&cv);let n=waiting.read(&mut bytes).unwrap();
        assert_eq!(&bytes[..n],b"1;STOP\n");assert_eq!(captures(&ui),1.0);
    }

    #[test]
    fn compatibility_cancelled_single_interrupts_query_and_does_not_follow_new_arm() {
        for stored in [false,true] {
            let (mut s,mut ui,cv)=if stored {reference_session()} else {session()};
            line(&mut s,&mut ui,&cv,":TRIG:EDGE:LEV 10;:SINGLE;*OPC");
            let mut p=Program::new("*OPC?;:RUN");assert!(s.poll_program(&mut ui,&cv,&mut p).is_none());
            ui.step(0.02);assert!(s.poll_program(&mut ui,&cv,&mut p).is_none(),"initial hardware stamp must not complete {stored:?} Single");
            line(&mut s,&mut ui,&cv,":STOP;:SINGLE");
            assert_eq!(s.poll_program(&mut ui,&cv,&mut p),Some(Vec::new()));
            assert!(line(&mut s,&mut ui,&cv,":SYST:ERR?").starts_with("-410,"));
            assert_eq!(line(&mut s,&mut ui,&cv,"*ESR?"),"0");assert_eq!(ui.m.trig_mode,TrigMode::Single);
            let mut fresh=Program::new("*OPC?");assert!(s.poll_program(&mut ui,&cv,&mut fresh).is_none());
        }
    }

    #[test]
    fn compatibility_hislip_defers_entire_multiline_payload_with_original_message_id() {
        let (mut s,mut ui,cv)=reference_session();line(&mut s,&mut ui,&cv,":TRIG:EDGE:LEV 10");
        let mut server=crate::hislip::Server::open(0);
        let mut client=TcpStream::connect(("127.0.0.1",server.port().unwrap())).unwrap();client.set_nonblocking(true).unwrap();
        client.write_all(&crate::hislip::encode(0,0,0x0100_0000,b"hislip0")).unwrap();
        server.poll_async(&mut |p|s.poll_program(&mut ui,&cv,p));
        let mut bytes=[0;1024];let n=client.read(&mut bytes).unwrap();let init=crate::hislip::take_message(&mut bytes[..n].to_vec()).unwrap().unwrap();assert_eq!(init.typ,1);
        client.write_all(&crate::hislip::encode(7,0,77,b":SINGLE;:TRIG:STAT?;*OPC?\n:TRIG:STAT?\n")).unwrap();
        server.poll_async(&mut |p|s.poll_program(&mut ui,&cv,p));ui.step(0.2);server.poll_async(&mut |p|s.poll_program(&mut ui,&cv,p));
        assert_eq!(client.read(&mut bytes).unwrap_err().kind(),ErrorKind::WouldBlock);
        line(&mut s,&mut ui,&cv,":TRIG:FORC");ui.step(0.2);server.poll_async(&mut |p|s.poll_program(&mut ui,&cv,p));
        let n=client.read(&mut bytes).unwrap();let response=crate::hislip::take_message(&mut bytes[..n].to_vec()).unwrap().unwrap();
        assert_eq!((response.typ,response.param),(7,77));assert_eq!(response.data,b"WAIT;1\nSTOP\n");
    }

    #[test]
    fn compatibility_disconnected_raw_client_cannot_resume_trailing_writes() {
        let (mut s,mut ui,cv)=reference_session();line(&mut s,&mut ui,&cv,":TRIG:EDGE:LEV 10");
        let mut client=TcpStream::connect(("127.0.0.1",s.port().unwrap())).unwrap();
        client.write_all(b":SINGLE;*OPC?;:RUN\n").unwrap();s.poll(&mut ui,&cv);assert!(s.clients[0].program.is_some());
        client.shutdown(std::net::Shutdown::Both).unwrap();drop(client);
        line(&mut s,&mut ui,&cv,":TRIG:EDGE:LEV 0");ui.step(0.2);s.poll(&mut ui,&cv);
        assert!(s.clients.is_empty());assert_eq!(ui.m.run,RunState::Stop);assert_eq!(captures(&ui),1.0);
    }

    #[test]
    fn compatibility_program_command_count_is_bounded_before_execution() {
        let (mut s,mut ui,cv)=session();let old=ui.m.trig_level;
        let message=format!("{}:TRIG:EDGE:LEV 2", ";".to_string()+&":TRIG:STAT?;".repeat(1025));
        assert!(s.exec_line(&mut ui,&cv,&message).is_empty());assert_eq!(ui.m.trig_level,old);
        assert!(line(&mut s,&mut ui,&cv,":SYST:ERR?").starts_with("-363,"));
    }

    #[test]
    fn compatibility_opc_waits_for_actual_large_sim_generation_but_not_future_captures() {
        let (mut s,mut ui,cv)=session();let mut p=Program::new(":SIM:PLAY ON;*OPC?;:SIM:BUSY?");
        assert!(s.poll_program(&mut ui,&cv,&mut p).is_none());assert!(ui.sim_generating());
        let deadline=std::time::Instant::now()+Duration::from_secs(15);
        let response=loop {
            ui.step(0.01);
            if let Some(response)=s.poll_program(&mut ui,&cv,&mut p) {break response;}
            assert!(std::time::Instant::now()<deadline,"SIM operation did not finish");std::thread::sleep(Duration::from_millis(2));
        };
        assert!(response.starts_with(b"1;"));assert!(ui.ref_info(0).unwrap().0>262_144,"exercise real background default SSC");
        line(&mut s,&mut ui,&cv,":STOP");
        let (mut s,mut ui,cv)=session();let mut p=Program::new(":SIM:PLAY ON;*OPC?;:RUN");
        assert!(s.poll_program(&mut ui,&cv,&mut p).is_none());line(&mut s,&mut ui,&cv,":SIM:PLAY OFF");
        assert_eq!(s.poll_program(&mut ui,&cv,&mut p),Some(Vec::new()));
        assert!(line(&mut s,&mut ui,&cv,":SYST:ERR?").starts_with("-410,"));
    }

    #[test]
    fn compatibility_newly_accepted_arm_cannot_complete_a_cancelled_prior_fence() {
        let (mut s,mut ui,cv)=reference_session();line(&mut s,&mut ui,&cv,":TRIG:EDGE:LEV 10;:SINGLE");
        let mut old=Program::new("*OPC?;:RUN");assert!(s.poll_program(&mut ui,&cv,&mut old).is_none());
        line(&mut s,&mut ui,&cv,":STOP;:TRIG:EDGE:LEV 0;:SINGLE");ui.step(0.2);assert_eq!(captures(&ui),1.0);
        assert_eq!(s.poll_program(&mut ui,&cv,&mut old),Some(Vec::new()));
        assert!(line(&mut s,&mut ui,&cv,":SYST:ERR?").starts_with("-410,"));assert_eq!(ui.m.run,RunState::Stop);
    }

    #[test]
    fn compatibility_hislip_sync_and_async_clear_discard_pending_tail_without_stale_reply() {
        fn read_message(socket:&mut TcpStream)->crate::hislip::Message {
            let mut b=[0;4096];let n=socket.read(&mut b).unwrap();crate::hislip::take_message(&mut b[..n].to_vec()).unwrap().unwrap()
        }
        for asynchronous in [false,true] {
            let (mut scpi,mut ui,cv)=reference_session();line(&mut scpi,&mut ui,&cv,":TRIG:EDGE:LEV 10");
            let mut server=crate::hislip::Server::open(0);let port=server.port().unwrap();
            let mut sync=TcpStream::connect(("127.0.0.1",port)).unwrap();sync.set_nonblocking(true).unwrap();
            sync.write_all(&crate::hislip::encode(0,0,0x0100_0000,b"hislip0")).unwrap();server.poll_async(&mut|p|scpi.poll_program(&mut ui,&cv,p));
            let session=read_message(&mut sync).param as u16;
            sync.write_all(&crate::hislip::encode(7,0,42,b":SINGLE;*OPC?;:RUN\n")).unwrap();server.poll_async(&mut|p|scpi.poll_program(&mut ui,&cv,p));
            if asynchronous {
                let mut control=TcpStream::connect(("127.0.0.1",port)).unwrap();control.set_nonblocking(true).unwrap();
                control.write_all(&crate::hislip::encode(17,0,session as u32,b"")).unwrap();server.poll_async(&mut|p|scpi.poll_program(&mut ui,&cv,p));
                assert_eq!(read_message(&mut control).typ,18);
                control.write_all(&crate::hislip::encode(19,0,0,b"")).unwrap();server.poll_async(&mut|p|scpi.poll_program(&mut ui,&cv,p));
                assert_eq!(read_message(&mut control).typ,23);
            } else {
                sync.write_all(&crate::hislip::encode(8,0,0,b"")).unwrap();server.poll_async(&mut|p|scpi.poll_program(&mut ui,&cv,p));
                assert_eq!(read_message(&mut sync).typ,9);
            }
            line(&mut scpi,&mut ui,&cv,":TRIG:EDGE:LEV 0");ui.step(0.2);server.poll_async(&mut|p|scpi.poll_program(&mut ui,&cv,p));
            assert_eq!(ui.m.run,RunState::Stop);let mut b=[0;128];assert_eq!(sync.read(&mut b).unwrap_err().kind(),ErrorKind::WouldBlock);
            sync.write_all(&crate::hislip::encode(7,0,44,b"*IDN?\n")).unwrap();server.poll_async(&mut|p|scpi.poll_program(&mut ui,&cv,p));
            assert!(read_message(&mut sync).data.starts_with(b"OOS,"));
        }
    }

    #[test]
    fn compatibility_sweep_single_allocates_new_arm_after_a_completed_single() {
        let (mut scpi,mut ui,cv)=reference_session();line(&mut scpi,&mut ui,&cv,":SINGLE");ui.step(0.2);assert_eq!(captures(&ui),1.0);
        let mut program=Program::new(":TRIG:SWE NORM;:RUN;:TRIG:EDGE:LEV 10;:TRIG:SWE SING;*OPC?");
        assert!(scpi.poll_program(&mut ui,&cv,&mut program).is_none(),"a previous Single receipt must not complete the SWEEP arm");
        ui.step(0.2);assert!(scpi.poll_program(&mut ui,&cv,&mut program).is_none());assert_eq!(captures(&ui),1.0);
        assert_eq!((ui.m.run,ui.m.status),(RunState::Run,TrigStatus::Wait));
    }

    #[test]
    fn compatibility_pending_and_invalid_program_errors_stay_bounded() {
        let (mut scpi,mut ui,cv)=reference_session();line(&mut scpi,&mut ui,&cv,":TRIG:EDGE:LEV 10;:SINGLE");
        for _ in 0..100 {assert!(scpi.exec_line_checked(&mut ui,&cv,"*OPC?").is_err());}
        assert!(scpi.err.len()<=ERR_MAX);
        let invalid=":TRIG:STAT?;".repeat(1025);
        for _ in 0..100 {scpi.exec_line(&mut ui,&cv,&invalid);}
        assert!(scpi.err.len()<=ERR_MAX);assert!(scpi.err.iter().all(|error|error.len()<=ERR_LEN));
    }

    #[test]
    fn compatibility_setup_single_rearms_with_a_fresh_operation_identity() {
        let (mut scpi,mut ui,cv)=reference_session();line(&mut scpi,&mut ui,&cv,":SINGLE");ui.step(0.2);
        let mut program=Program::new(":TRIG:SWE NORM;:RUN;:TRIG:EDGE:LEV 10;:SYST:SET trig_mode = Single;*OPC?");
        assert!(scpi.poll_program(&mut ui,&cv,&mut program).is_none());ui.step(0.2);
        assert!(scpi.poll_program(&mut ui,&cv,&mut program).is_none());assert_eq!(captures(&ui),1.0);
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
        for _ in 0..4 { ui.step(0.05); }
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
        s.wav_raw = true;
        // An independently pending client request must not be consumed or
        // cancelled while another client polls configuration/status.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let _peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        let pending = b":MEAS:ITEM? VPP,CHAN1".to_vec();
        s.clients.push(Client { s: stream, buf: pending.clone(), program: None });
        for query in [":TRIG:STAT?", ":TIM:SCAL?", ":CHAN1:DISP?", ":CHAN1:OFFS?", ":ACQ:SRAT?", ":ACQ:MDEP?"] {
            assert!(!line(&mut s, &mut ui, &cv, query).is_empty(), "{query} must return its existing reply");
            assert_eq!(ui.measurement_statistics_snapshot_for_test(), statistics, "{query} must preserve every statistic bit-for-bit");
            assert_eq!(crate::store::setup_text(&ui.m), configuration, "{query} must not change configuration");
            assert_eq!(ui.workflow_state(), workflow, "{query} must not change acquisition/run state");
            assert_eq!(ui.m.hw_stamp, acquisition_stamp);
            assert_eq!(s.err, errors, "status reads must not consume queued errors");
            assert_eq!(s.clients.len(), 1);
            assert_eq!(s.clients[0].buf, pending);
            assert_eq!((s.wav_src, s.wav_fmt, s.wav_raw), (WaveId::Ref(2), 1, true));
        }
        line(&mut s, &mut ui, &cv, ":TIM:SCAL 0.000005");
        assert!(ui.measurement_statistics_snapshot_for_test().is_empty(), "a successful setting still invalidates statistics");
        assert!((ui.m.time_per_div() as f64 - 5e-6).abs() < 1e-12);
        assert_eq!(s.err, errors);
        assert_eq!(s.clients[0].buf, pending);
    }


    #[test]
    fn unsupported_write_only_queries_preserve_records_statistics_and_error_queue() {
        for run in [RunState::Run, RunState::Stop] {
            let (mut s, mut ui, cv) = session();
            for _ in 0..4 { ui.step(0.05); }
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
                assert!(line(&mut s, &mut ui, &cv, query).is_empty(), "{query} has no defined query response");
                assert_eq!(crate::store::setup_text(&ui.m), configuration, "{query} must not execute its action");
                assert_eq!(ui.workflow_state(), workflow, "{query} must preserve Run/Stop and acquisition state");
                assert_eq!(ui.measurement_statistics_snapshot_for_test(), statistics, "{query} must preserve all acquired statistics");
                assert_eq!(ui.record_volts(0).iter().map(|value| value.to_bits()).collect::<Vec<_>>(), record, "{query} must not clear data");
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
        for (bit, high) in bits.enumerate() { values[100 + bit * 20..100 + (bit + 1) * 20].fill(if high { 3.3 } else { 0.0 }); }
        ui.commit_waveform_traces(vec![crate::store::WaveTrace { v: values, dt: 1.0 / (115_200.0 * 20.0), t0: -0.008, label: "UART reference".into() }], "reference", true).unwrap();
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
        assert_eq!(line(&mut s, &mut ui, &cv, ":DEC:CS NONE;:DEC:SOUR4 NONE;:DEC:DIFF NONE;:DEC:CS?;:DEC:SOUR4?;:DEC:DIFF?"), "NONE;NONE;NONE");
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
        let mut symbols = vec![3; 12]; symbols.extend([3,4,4,4,4,4,3]);
        symbols.extend([0,2,2,4,1,0,0]); symbols.extend([4; 7]);
        let states = oos_dsp::cphy::states_for_symbols(0, &symbols);
        let traces = (0..3).map(|wire| crate::store::WaveTrace {
            v: states.iter().flat_map(|&state| std::iter::repeat(oos_dsp::cphy::wire_values(state)[wire]).take(20)).collect(),
            dt: 5e-12, t0: -12e-9, label: format!("C-PHY {}", ['A','B','C'][wire]),
        }).collect();
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
        assert_eq!(line(&mut s, &mut ui, &cv, ":DEC:PROTO CPHY;:DEC:CPHY:RATE 2.25e9;:DEC:CPHY:RATE?;:DEC:SRAT?"), "2.250000E+09;2.250000E+09");
        let configured = crate::store::setup_text(&ui.m);
        assert_eq!(crate::store::parse_setup(&configured).unwrap().dec_cphy_rate, 2.25e9);
        let old_setup = configured.lines().filter(|line| !line.trim_start().starts_with("dec_cphy_rate") && !line.contains(".dec_cphy_rate")).collect::<Vec<_>>().join("\n");
        assert_eq!(crate::store::parse_setup(&old_setup).unwrap().dec_cphy_rate, 0.0, "old setups keep Auto");
        for bad in ["-1", "NaN", "inf", "1e13", "garbage"] {
            line(&mut s, &mut ui, &cv, &format!(":DEC:CPHY:RATE {bad}"));
            assert_eq!(crate::store::setup_text(&ui.m), configured);
            assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"));
            let mut restored = ui.m.clone();
            assert!(crate::store::set(&mut restored, "dec_cphy_rate", bad).is_err());
            assert_eq!(restored.dec_cphy_rate, 2.25e9);
        }
        assert_eq!(line(&mut s, &mut ui, &cv, ":DEC:SEL 2;:DEC:CPHY:RATE?;:DEC:SEL 1;:DEC:CPHY:RATE?"), "AUTO;2.250000E+09");
        assert_eq!(line(&mut s, &mut ui, &cv, ":DEC:CPHY:RATE AUTO;:DEC:CPHY:RATE?"), "AUTO");
        assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
    }

    #[test]
    fn write_only_actions_without_query_suffix_still_execute() {
        let (mut s, mut ui, cv) = session();
        for _ in 0..4 { ui.step(0.05); }
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
        assert_eq!(line(&mut s, &mut ui, &cv,
            ":TRIG:EDGE:SOUR?;:TRIG:SOUR?;:TRIG:SOUR2?"), "D7;D7;CHAN4");
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
        let (mut s, mut ui, cv)=session();
        for amplitude in [2.0,0.5] {
            let mut csv=String::from("Time (s),Volts\n");
            for i in 0..1000 { csv.push_str(&format!("{:.12e},{}\n",i as f64*1e-6,amplitude*(i as f64*std::f64::consts::TAU/20.0).sin())); }
            ui.load_waveform_auto(csv.as_bytes(),"power.csv").unwrap();
        }
        ui.set_offline(true);ui.m.meas_full_record=true;ui.m.meas.clear();ui.m.meas2.clear();
        assert_eq!(line(&mut s,&mut ui,&cv,":SYST:SET cat = 90:R1:R2:3:4;:SYST:ERR?"),"0,\"No error\"");
        let end=std::time::Instant::now()+Duration::from_secs(10);
        let answer=loop {
            let answer=line(&mut s,&mut ui,&cv,":MEAS:STRIP?");
            let value=answer.rsplit(',').next().unwrap().parse::<f64>().unwrap();
            if value.abs()<1e30 { assert!((value-0.5).abs()<0.01,"{answer}");break answer; }
            assert!(std::time::Instant::now()<end,"{answer}");ui.step(0.0);std::thread::sleep(Duration::from_millis(1));
        };
        assert!(answer.starts_with("CAT90,R1,R2,CHAN3,CHAN4,"),"{answer}");
        assert_eq!(line(&mut s,&mut ui,&cv,":MEAS:READOUT?"),answer);
        assert_eq!(captures(&ui),0.0,"SCPI row exports are not acquisitions");
        assert_eq!(line(&mut s,&mut ui,&cv,":SYST:SET cat = ;:SYST:ERR?"),"0,\"No error\"");
        assert!(ui.m.cat.is_empty());assert_eq!(line(&mut s,&mut ui,&cv,":MEAS:STRIP?"),"");
    }

    #[test]
    fn strip_and_readout_export_the_configured_row_levels_and_physical_time() {
        let (mut s, mut ui, cv) = session();
        let mut csv = String::from("Time (s),Volts\n");
        for i in 0..800 {
            let k = i % 400;
            let v = match k { 0..=99 => -0.5, 100..=199 => -0.5 + (k - 100) as f64 / 99.0,
                200..=299 => 0.5, _ => 0.5 - (k - 300) as f64 / 99.0 };
            csv.push_str(&format!("{:.12e},{v}\n", 0.001 + i as f64 * 1e-9));
        }
        ui.load_waveform_bytes(csv.as_bytes(), "row-levels.csv").unwrap();
        ui.m.run = RunState::Stop; ui.m.meas_full_record = true;
        let id = WaveId::Ref(0);
        ui.m.meas = vec![(Meas::Rise, id), (Meas::TVmax, id)]; ui.m.meas2.clear();
        ui.set_meas_refs(0, 0.2, 0.8); ui.set_meas_refs(1, 0.2, 0.8);
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
        assert!((first - 0.5).abs() < 1e-6, "first screen Vpp {first}, timebase {:?}, delay {}, zoom {}, state {}", ui.m.time_per_div(), ui.m.delay, ui.m.zoom, ui.workflow_state());
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
        for _ in 0..3 { s.poll(&mut ui, &cv); }
        let mut response = std::io::BufReader::new(client.try_clone().unwrap());
        let mut first = String::new();
        std::io::BufRead::read_line(&mut response, &mut first).unwrap();
        assert!(first.starts_with("OOS,") && first.ends_with(";1\n"), "{first:?}");
        client.write_all(b"*OPC?\n").unwrap();
        for _ in 0..3 { s.poll(&mut ui, &cv); }
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
            (":SIM:SSC:DEV?", 0.007), (":SIM:SSC:FREQ?", 40000.0),
            (":SIM:JITT:DCD?", 4e-12), (":SIM:JITT:CORR?", 2e-12), (":SIM:JITT:CORN?", 2e6),
        ] {
            let actual: f64 = line(&mut s, &mut ui, &cv, command).parse().unwrap();
            assert!((actual - expected).abs() <= expected.abs() * 1e-6, "{command}: {actual}");
        }
        assert!((0..NREF).all(|slot| ui.ref_volts(slot).is_empty()), "configuration must not replace sources until Play");
        for command in [":SIM:SSC:MODE BAD", ":SIM:SSC:DEV NaN", ":SIM:SSC:FREQ 0", ":SIM:JITT:DCD -1", ":SIM:JITT:CORR inf", ":SIM:JITT:CORN 1e30", ":SIM:PATT PRBS8"] {
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
        let dir = std::env::temp_dir().join(format!("oos-ref-{}", std::process::id()));
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
}
