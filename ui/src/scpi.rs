// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! SCPI server (TCP 5555, the port of the stock firmware), a stock-compatible subset:
//! *IDN? *RST *OPC? *CLS *ESR?, :RUN :STOP :SINGle :TFORce :AUToscale :CLEar,
//! :CHANnel<n>:DISPlay|SCALe|OFFSet|POSition|COUPling|BWLimit|PROBe|INVert|UNITs|VERNier|TCALibrate|LABel:SHOW|LABel:CONTent,
//! :TIMebase[:MAIN]:SCALe|OFFSet, :TIMebase:DELay:ENABle|SCALe|OFFSet, :TIMebase:VERNier, :TIMebase:HREFerence:MODE, :TIMebase:XY:ENABle,
//! :TRIGger:MODE? :TRIGger:EDGE:SOURce|LEVel|SLOPe, :TRIGger:SWEep, :TRIGger:HOLDoff, :TRIGger:STATus?,
//! :ACQuire:SRATe?|MDEPth[?] (AUTO, 1k..125M)|TYPE NORM/PEAK/AVER/HRES|AVERages|ULTRa:MAXFrame,
//! :MEASure:ITEM[?] <item>[,CHANn|Rn[,src]] (the set form adds a readout row), :MEASure:SOURce, :MEASure:CLEar,
//! :MEASure:STRip?, :MEASure:READout?, :EYE:ITEM? <figure>,
//! :MEASure:STATistic:DISPlay|RESet|ITEM, :MEASure:REFS 10/90|20/80, :MEASure:TSETup, :MEASure:THOLd,
//! :CURSor:MODE OFF|MAN|TRAC|XY, :CURSor:MANual:TYPE TIME|AMPLitude, :CAX|:CBX|:CAY|:CBY, :AXValue|:AYValue|:BXValue|:BYValue, :XDELta|:IXDelta|:YDELta,
//! :DVM:ENABle|SOURce|MODE|CURRent?, :COUNter:ENABle|SOURce|MODE|NDIGits|CURRent?,
//! :MASK:ENABle|SOURce|X|Y|CREate|RESet|OPERate|FAILed?|PASSed?|TOTal?,
//! :POWer:ITEM? <item>[,src...], :POWer:HARMonics? [src], :SPECtrum:ITEM? <item>[,src], :BODE:MODE LOOP|PSRR|IMPedance,
//! :MAGnetic:ITEM? <item>[,V,I], :INVerter:ITEM? <item>[,src...], :DPT:ITEM? <item>[,V,I[,G]], :BANDwidth:ITEM? <item>[,src],
//! :WAVeform:SOURce CHANn|Rn|MODE NORM/MAX/RAW|FORMat BYTE/WORD/ASC|STARt|STOP|POINts|DATA?|PREamble?,
//! :DISPlay:DATA? [BMP|PNG],
//! :REFerence:SLOT R1..R10, :CURRent, :SOURce, :VSCale, :VOFFset, :COLor, :RESet, :LABel:CONTent, :LOAD <file>, :IMPort [file], :SAVE, :SHOW ON|OFF, :CLEar,
//! :MEASure:SETup:MAX|MIN (percent thresholds),
//! :SIM:PRESet <name>, :SIM:SLOT R1..R10, :SIM:PLAY ON|OFF, :SIM:SHOW ON|OFF,
//! :SIM:SSC:MODE OFF|DOWN|CENTER, :SIM:SSC:DEViation <fraction>, :SIM:SSC:FREQuency <Hz>,
//! :SIM:JITTer:DCD <seconds pp>|:CORRelated <seconds RMS>|:CORNer <Hz>, :SIM:PATTern PRBS7..31.
//! SIM configuration is applied together on the next :SIM:PLAY ON. :SIM:BUSY? reports background generation.
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

struct Client {
    s: TcpStream,
    buf: Vec<u8>,
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
    /// :WAVeform:MODE: 0 NORMal (screen), 1 MAXimum (memory while stopped), 2 RAW (memory)
    wav_mode: u8,
    /// :WAVeform:STARt and :WAVeform:STOP, 1-based inclusive. `stop == 0` reads through the last point.
    wav_start: u64,
    wav_stop: u64,
    err: Vec<String>,
    /// IEEE 488.2 event status register. *ESR? returns it and clears it. Bit 7 is set at power on.
    esr: u8,
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
fn ref_color_index(s: &str) -> Option<usize> {
    if kw(s, "GRAY") || kw(s, "WHITe") {
        Some(0)
    } else if kw(s, "RED") {
        Some(1)
    } else if kw(s, "GREen") {
        Some(2)
    } else if kw(s, "BLUE") {
        Some(3)
    } else if kw(s, "ORANge") || kw(s, "VIOLet") {
        Some(4)
    } else {
        None
    }
}

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
        Scpi { l, clients: Vec::new(), wav_src: WaveId::Ch(0), wav_fmt: 0, wav_mode: 0, wav_start: 1, wav_stop: 0, err: Vec::new(), esr: 128 }
    }

    /// Queue a SCPI error and latch the matching *ESR bit. A query that fails still answers with this text,
    /// so a client blocked on a line never waits forever.
    fn push_err(&mut self, s: String) {
        let code = s.split([',', '"']).next().unwrap_or("").trim().parse::<i32>().unwrap_or(0);
        self.esr |= match code {
            -199..=-100 => 32, // command error
            -299..=-200 => 16, // execution error
            -399..=-300 => 8,  // device-specific error
            -499..=-400 => 4,  // query error
            _ => 16,
        };
        self.err.push(s);
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
                    self.clients.push(Client { s, buf: Vec::new() });
                }
            }
        }
        let mut i = 0;
        while i < self.clients.len() {
            let mut tmp = [0u8; 4096];
            let mut closed = false;
            loop {
                match self.clients[i].s.read(&mut tmp) {
                    Ok(0) => {
                        closed = true;
                        break;
                    }
                    Ok(n) => self.clients[i].buf.extend_from_slice(&tmp[..n]),
                    Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => break,
                    Err(_) => {
                        closed = true;
                        break;
                    }
                }
            }
            'lines: while let Some(p) = self.clients[i].buf.iter().position(|&c| c == b'\n') {
                let line: Vec<u8> = self.clients[i].buf.drain(..=p).collect();
                let line = String::from_utf8_lossy(&line).trim().to_string();
                let mut response = self.exec_line(ui, cv, &line);
                if !response.is_empty() {
                    response.push(b'\n');
                    if !send(&mut self.clients[i].s, &response) {
                        closed = true;
                        break 'lines;
                    }
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

    /// One program message (commands separated by ';'): the responses joined by ';' (empty without a query).
    pub fn exec_line(&mut self, ui: &mut Ui, cv: &Canvas, line: &str) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        for cmd in line.split(';') {
            if out.len() >= RESP_MAX {
                self.push_err("-363,\"Input buffer overrun\" rest of the line dropped".into());
                break;
            }
            if let Some(r) = self.exec(ui, cv, cmd.trim()) {
                if !out.is_empty() {
                    out.push(b';');
                }
                out.extend_from_slice(&r);
            }
        }
        out
    }

    fn exec(&mut self, ui: &mut Ui, cv: &Canvas, cmd: &str) -> Option<Vec<u8>> {
        if let Some(error) = ui.take_sim_error() {
            self.push_err(format!("-224,\"SIM generation failed\" {error}"));
        }
        let before = self.err.len();
        let r = self.exec_cmd(ui, cv, cmd);
        // a query with no payload would leave the client blocked on the next line: answer with the error instead
        let query = cmd.split_whitespace().next().unwrap_or("").ends_with('?');
        let r = if query && r.is_none() {
            if self.err.len() == before {
                self.push_err(format!("-113,\"Undefined header\" {}", cmd));
            }
            self.err.last().map(|e| e.as_bytes().to_vec())
        } else {
            r
        };
        // a client that never asks for its errors, or sends megabytes of nonsense, must not fill the memory
        for e in &mut self.err {
            if e.len() > ERR_LEN {
                *e = e.chars().take(ERR_LEN).collect();
            }
        }
        if self.err.len() > ERR_MAX {
            self.err.truncate(ERR_MAX - 1);
            self.push_err("-350,\"Queue overflow\"".into());
        }
        r
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
            "*OPC" => {
                if !query {
                    self.esr |= 1;
                }
                return if query { t("1") } else { None };
            }
            "*CLS" => {
                self.err.clear();
                self.esr = 0;
                return None;
            }
            "*ESR" if query => {
                let v = self.esr;
                self.esr = 0;
                return t(&v.to_string());
            }
            "*RST" => {
                ui.key(crate::ui::key::DEFAULT);
                return None;
            }
            _ => {}
        }
        let nodes: Vec<&str> = head.trim_start_matches(':').split(':').collect();
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
        if nodes.len() == 1 && kw(nodes[0], "CLEar") {
            ui.key(crate::ui::key::CLEAR);
            return None;
        }
        // Rigol: a forced trigger releases Normal or Single when the level never crosses. On a reference or SIM
        // session that is the replay flag; on the live path it is the same key as the front panel.
        if !query && nodes.len() == 1 && kw(nodes[0], "TFORce") {
            if ui.record_mode() {
                ui.record_key(crate::ui::key::FORCE);
            } else {
                ui.key(crate::ui::key::FORCE);
            }
            ui.mark_changed();
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
        let m = &mut ui.m;
        let r: Option<Option<Vec<u8>>> = (|| {
            let n0 = nodes.first().copied().unwrap_or("");
            let (k0, sfx) = suffix(n0);
            if nodes.len() == 1 {
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
            if kw(k0, "CHANnel") && (nodes.len() == 2 || nodes.len() == 3) {
                let c = sfx?.checked_sub(1).filter(|&c| c < NCH)?;
                let n1 = nodes[1];
                if nodes.len() == 3 && kw(nodes[1], "LABel") && kw(nodes[2], "CONTent") {
                    if query {
                        return Some(t(&m.ch_label[c]));
                    }
                    let text = arg.trim().trim_matches('"').chars().take(10).collect::<String>();
                    m.ch_label[c] = text;
                    return Some(None);
                }
                if nodes.len() == 3 && kw(nodes[1], "LABel") && kw(nodes[2], "SHOW") {
                    if query {
                        return Some(t(if m.ch_label[c].is_empty() { "0" } else { "1" }));
                    }
                    return match parse_bool(arg) {
                        Some(false) => {
                            m.ch_label[c].clear();
                            Some(None)
                        }
                        Some(true) => {
                            if m.ch_label[c].is_empty() {
                                m.ch_label[c] = format!("CH{}", c + 1);
                            }
                            Some(None)
                        }
                        None => {
                            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                            Some(None)
                        }
                    };
                }
                if nodes.len() != 2 {
                    return None;
                }
                if kw(n1, "VERNier") {
                    // One fine-adjust switch for every channel: the screen has a single control.
                    if query {
                        return Some(t(if m.fine_v { "1" } else { "0" }));
                    }
                    return match parse_bool(arg) {
                        Some(on) => {
                            m.fine_v = on;
                            if !on {
                                m.ch[c].vfine = 1.0;
                            }
                            Some(None)
                        }
                        None => {
                            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                            Some(None)
                        }
                    };
                }
                let slow_deskew = m.time_per_div() > 10e-6;
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
                } else if kw(n1, "OFFSet") || kw(n1, "POSition") {
                    if query {
                        return Some(t(&e(ch.offset as f64)));
                    }
                    ch.offset = num(arg)?;
                } else if kw(n1, "UNITs") {
                    if query {
                        return Some(t(match ch.unit {
                            AmpUnit::V => "VOLT",
                            AmpUnit::A => "AMP",
                            AmpUnit::W => "WATT",
                            AmpUnit::U => "UNKN",
                        }));
                    }
                    let a = arg.trim();
                    ch.unit = if kw(a, "VOLTage") {
                        AmpUnit::V
                    } else if kw(a, "AMPere") {
                        AmpUnit::A
                    } else if kw(a, "WATT") {
                        AmpUnit::W
                    } else if kw(a, "UNKNown") {
                        AmpUnit::U
                    } else {
                        self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                        return Some(None);
                    };
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
                } else if kw(n1, "TCALibrate") {
                    if query {
                        return Some(t(&e(ch.skew as f64)));
                    }
                    // The manual refuses a deskew change on a slow time base.
                    if slow_deskew {
                        self.push_err("-224,\"Illegal parameter value\" timebase".into());
                        return Some(None);
                    }
                    let Some(v) = num(arg).filter(|v| v.abs() <= SKEW_MAX) else {
                        self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                        return Some(None);
                    };
                    ch.skew = v;
                } else {
                    return None;
                }
                return Some(None);
            }
            if kw(n0, "TIMebase") {
                let sub = nodes.get(1).copied().unwrap_or("");
                // Delayed sweep, XY, the expand point, and fine adjust need the whole screen, not just the model.
                if kw(sub, "DELay") || kw(sub, "XY") || kw(sub, "HREFerence") || kw(sub, "VERNier") {
                    return None;
                }
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
                if nodes.len() == 2 && kw(last, "MODE") {
                    if query {
                        return Some(t("EDGE"));
                    }
                    if kw(arg, "EDGE") {
                        return Some(None);
                    }
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    return Some(None);
                }
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
            if kw(n0, "ACQuire") && nodes.len() == 3 && kw(nodes[1], "ULTRa") && kw(nodes[2], "MAXFrame") {
                if query {
                    return Some(t(&m.segments.to_string()));
                }
                let Some(n) = arg.trim().parse::<usize>().ok() else {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    return Some(None);
                };
                let steps = crate::ui::SEGMENTS;
                m.segments = *steps.iter().min_by_key(|s| s.abs_diff(n)).unwrap_or(&0);
                return Some(None);
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
            if kw(n0, "ACQuire") && nodes.len() == 2 {
                let n1 = nodes[1];
                if kw(n1, "TYPE") {
                    if query {
                        return Some(t(match m.acq {
                            AcqMode::Normal => "NORM",
                            AcqMode::Peak => "PEAK",
                            AcqMode::Average => "AVER",
                            AcqMode::HiRes => "HRES",
                        }));
                    }
                    let a = arg.trim();
                    m.acq = if kw(a, "NORMal") {
                        AcqMode::Normal
                    } else if kw(a, "PEAK") {
                        AcqMode::Peak
                    } else if kw(a, "AVERages") {
                        AcqMode::Average
                    } else if kw(a, "HRESolution") || a.eq_ignore_ascii_case("HRES") {
                        AcqMode::HiRes
                    } else {
                        self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                        return Some(None);
                    };
                    return Some(None);
                }
                if kw(n1, "AVERages") {
                    if query {
                        return Some(t(&m.avg_count.to_string()));
                    }
                    // The Acquire menu's counts. A value that is not one of them snaps down to the next lower power of two.
                    const COUNTS: [u32; 10] = [2, 4, 8, 16, 32, 64, 128, 256, 512, 1024];
                    let Some(n) = arg.trim().parse::<u32>().ok().filter(|n| *n >= 2) else {
                        self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                        return Some(None);
                    };
                    m.avg_count = COUNTS.iter().rev().find(|&&c| c <= n).copied().unwrap_or(2);
                    return Some(None);
                }
                if query {
                    let (sr, depth) = m.srate_depth();
                    if kw(n1, "SRATe") {
                        return Some(t(&e(sr)));
                    }
                    if kw(n1, "MDEPth") {
                        return Some(t(&e(depth as f64)));
                    }
                }
                return None;
            }
            None
        })();
        if let Some(resp) = r {
            ui.mark_changed();
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
        if kw(n0, "MEASure") && nodes.len() == 3 && kw(nodes[1], "STATistic") {
            return self.meas_stat_cmd(ui, nodes[2], query, arg);
        }
        if kw(n0, "MEASure") && nodes.len() == 3 && kw(nodes[1], "SETup") && (kw(nodes[2], "MAX") || kw(nodes[2], "MIN")) {
            return self.meas_threshold(ui, kw(nodes[2], "MAX"), query, arg);
        }
        if kw(n0, "TIMebase") {
            return self.timebase_more(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "MEASure") && nodes.len() == 2 {
            let n1 = nodes[1];
            if kw(n1, "SOURce") {
                if query {
                    return t(&wave_tok(ui.meas_source()));
                }
                let mut parts = arg.split(',');
                let Some(word) = parts.next().map(str::trim).filter(|s| !s.is_empty()) else {
                    self.push_err("-224,\"Illegal parameter value\"".into());
                    return None;
                };
                let Some(a) = self.named_source(word) else { return None };
                ui.set_meas_source(a);
                if let Some(word) = parts.next().map(str::trim).filter(|s| !s.is_empty()) {
                    let Some(b) = self.named_source(word) else { return None };
                    ui.set_meas_source_b(b);
                }
                ui.mark_changed();
                return None;
            }
            if kw(n1, "CLEar") && !query {
                ui.meas_clear_remote();
                ui.mark_changed();
                return None;
            }
            if kw(n1, "ITEM") && !query {
                return self.meas_place(ui, arg);
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
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
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
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                        self.push_err(s);
                        return t("9.9E37");
                    }
                };
                return match ui.fact_harmonics(&src) {
                    Ok(h) if h.is_empty() => t("9.9E37"),
                    Ok(h) => t(&h.iter().map(|(n, a)| format!("{},{}", n, e(*a))).collect::<Vec<_>>().join(";")),
                    Err(()) => {
                        self.push_err("-224,\"Illegal parameter value\"".into());
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
                    self.push_err(s);
                    return t("9.9E37");
                }
            };
            return match ui.fact_inverter(&arg_item(arg), &src) {
                Ok(crate::ui::facts::Fact::Word(w)) => t(w),
                Ok(crate::ui::facts::Fact::Num(v)) => t(&shown(v)),
                Err(()) => {
                    self.push_err("-224,\"Illegal parameter value\"".into());
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
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                }
                return None;
            }
            if kw(n1, "MODE") {
                if query {
                    return t(["NORM", "MAX", "RAW"][self.wav_mode.min(2) as usize]);
                }
                let a = arg.trim();
                self.wav_mode = if kw(a, "RAW") {
                    2
                } else if kw(a, "MAXimum") {
                    1
                } else if kw(a, "NORMal") {
                    0
                } else {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    return None;
                };
                return None;
            }
            if kw(n1, "FORMat") {
                if query {
                    return t(["BYTE", "WORD", "ASC"][self.wav_fmt as usize]);
                }
                let a = arg.to_ascii_uppercase();
                self.wav_fmt = if a.starts_with("WORD") { 1 } else if a.starts_with("ASC") { 2 } else { 0 };
                return None;
            }
            if kw(n1, "STARt") || kw(n1, "STOP") {
                let stop = kw(n1, "STOP");
                if query {
                    let n = self.wave_len(ui);
                    let v = if stop { if self.wav_stop == 0 { n } else { self.wav_stop } } else { self.wav_start };
                    return t(&v.to_string());
                }
                let Some(p) = arg.trim().parse::<u64>().ok().filter(|&p| p >= 1) else {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    return None;
                };
                if stop {
                    self.wav_stop = p;
                    if p < self.wav_start {
                        self.wav_start = p;
                    }
                } else {
                    self.wav_start = p;
                    if self.wav_stop != 0 && self.wav_stop < p {
                        self.wav_stop = p;
                    }
                }
                return None;
            }
            if kw(n1, "POINts") {
                if query {
                    return t(&self.page_count(ui).to_string());
                }
                // Rigol: the count to read, starting at STARt. stop 0 still means "the rest" until this sets it.
                let Some(n) = arg.trim().parse::<u64>().ok().filter(|&n| n >= 1) else {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    return None;
                };
                self.wav_stop = self.wav_start.saturating_add(n - 1);
                return None;
            }
            if kw(n1, "PREamble") && query {
                return t(&self.wave_preamble(ui));
            }
            if kw(n1, "DATA") && query {
                return Some(self.wave_data(ui));
            }
        }
        if kw(n0, "REFerence") {
            return self.ref_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "SIM") {
            return self.sim_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "DISPlay") && nodes.len() == 2 && kw(nodes[1], "DATA") && query {
            let kind = arg.split_whitespace().next().unwrap_or("").trim();
            let up = kind.to_ascii_uppercase();
            if up.starts_with("PNG") {
                return Some(block(&crate::store::png_of(cv)));
            }
            if up.is_empty() || up.starts_with("BMP") {
                return Some(block(&bmp(cv)));
            }
            self.push_err(format!("-224,\"Illegal parameter value\" {}", kind));
            return None;
        }
        // OOS extensions: analyser view, Identify, self-calibration, Bode
        if kw(n0, "VIEW") && nodes.len() == 1 {
            const VIEWS: [(&str, AView); 8] = [("SCOPe", AView::Scope), ("SPECtrum", AView::Spectrum), ("EYE", AView::Eye), ("POWer", AView::Power), ("BODE", AView::Bode), ("MAGnetic", AView::Magnetic), ("INVerter", AView::Inverter), ("DPT", AView::Dpt)];
            if query {
                return t(&format!("{:?}", ui.m.view).to_ascii_uppercase());
            }
            match VIEWS.iter().find(|(k, _)| kw(arg, k)) {
                Some((_, v)) => ui.set_analysis_view(*v),
                None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                } else {
                    ui.m.bode_addr = a.to_string();
                }
                return None;
            }
        }
        if suffix(n0).1.is_none_or(|n| (1..=gen_count().max(1)).contains(&n)) && kw(suffix(n0).0, "SOURce") && nodes.len() >= 2 {
            if !oos_hal::model::platform().caps.has_gen() {
                self.push_err(format!("-241,\"Hardware missing\" no generator on the {}", oos_hal::model::platform().caps.name));
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
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                        None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                        None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                        None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            // :FUNCtion:SQUare:DUTY / :FUNCtion:PULSe:DUTY and :FUNCtion:RAMP:SYMMetry in %
            // arbitrary wave: :ARB:DATA <v>,<v>,... (one period, any scale), :ARB:CAPTure CHAN<n> (the screen of a channel)
            if has("ARB") {
                if has("DATA") && !query {
                    let v: Vec<f32> = arg.split(',').filter_map(num).collect();
                    if !ui.afg_arb_load(&v) {
                        self.push_err(format!("-224,\"Illegal parameter value\" {} samples", v.len()));
                    }
                } else if has("CAPTure") && !query {
                    match parse_ch(arg).ok_or("no channel".to_string()).and_then(|c| ui.afg_arb_capture(c)) {
                        Ok(_) => {}
                        Err(e) => self.push_err(format!("-224,\"Illegal parameter value\" {}", e)),
                    }
                } else {
                    self.push_err("-113,\"Undefined header\" :SOURce:ARB".into());
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
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                    Err(e) => self.push_err(format!("-224,\"Illegal parameter value\" {}", e)),
                }
                // a setup file may hold keys of newer versions, here a typo must not pass for a setting; the keys that
                // are only written when they differ from their default do not show
                let k = arg.split('=').next().unwrap_or("").trim();
                let optional = ["vfine", "unit", "bias", "skew", "label", "tfine"].iter().any(|s| k.ends_with(s)) || k.starts_with("math_expr");
                if !optional && !crate::store::setup_text(&ui.m).lines().any(|l| l.split('=').next().unwrap_or("").trim() == k) {
                    self.push_err(format!("-224,\"Illegal parameter value\" unknown key {}", k));
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
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
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
        if kw(n0, "CURSor") {
            return self.cursor_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "MASK") {
            return self.mask_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "DVM") {
            return self.dvm_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "COUNter") {
            return self.counter_cmd(ui, &nodes[1..], query, arg);
        }
        self.push_err(format!("-113,\"Undefined header\" {}", cmd));
        None
    }

    /// Upper (`high`) or lower threshold, as a percent of the waveform, the same pair `:MEASure:REFS` sets.
    fn meas_threshold(&mut self, ui: &mut Ui, high: bool, query: bool, arg: &str) -> Option<Vec<u8>> {
        if query {
            let pct = if high { ui.m.meas_hpct } else { ui.m.meas_lpct };
            return Some(format!("{}", (pct * 100.0).round() as i32).into_bytes());
        }
        let Some(pct) = arg.trim().parse::<f64>().ok().filter(|v| (0.0..=100.0).contains(v)) else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        };
        let frac = pct / 100.0;
        if high {
            if frac <= ui.m.meas_lpct {
                self.push_err("-224,\"Illegal parameter value\" below the lower threshold".into());
                return None;
            }
            ui.m.meas_hpct = frac;
        } else {
            if frac >= ui.m.meas_hpct {
                self.push_err("-224,\"Illegal parameter value\" above the upper threshold".into());
                return None;
            }
            ui.m.meas_lpct = frac;
        }
        ui.measure_last();
        ui.mark_changed();
        None
    }

    fn meas_stat_cmd(&mut self, ui: &mut Ui, which: &str, query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        if kw(which, "DISPlay") {
            if query {
                return t(if ui.stats_shown() { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.set_stats_shown(on);
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if kw(which, "RESet") && !query {
            ui.stats_reset();
            ui.mark_changed();
            return None;
        }
        if !kw(which, "ITEM") {
            self.push_err(format!("-113,\"Undefined header\" :MEASure:STATistic:{}", which));
            return None;
        }
        let mut parts = arg.split(',').map(str::trim).filter(|s| !s.is_empty());
        let first = parts.next().unwrap_or("");
        if !query {
            let item = first;
            let rest: Vec<&str> = parts.collect();
            let before = self.err.len();
            self.meas_place(ui, &std::iter::once(item).chain(rest).collect::<Vec<_>>().join(","));
            if self.err.len() == before {
                ui.set_stats_shown(true);
                ui.mark_changed();
            }
            return None;
        }
        let kind = if kw(first, "MAXimum") {
            0
        } else if kw(first, "MINimum") {
            1
        } else if kw(first, "CURRent") {
            2
        } else if kw(first, "AVERages") {
            3
        } else if kw(first, "DEViation") {
            4
        } else if kw(first, "CNT") || kw(first, "COUNt") {
            5
        } else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", first));
            return t("9.9E37");
        };
        let Some(item) = parts.next() else {
            self.push_err("-224,\"Illegal parameter value\"".into());
            return t("9.9E37");
        };
        let source = |s: &mut Self, word: Option<&str>, fallback: WaveId| -> Option<WaveId> {
            match word.filter(|w| !w.is_empty()) {
                None => Some(fallback),
                Some(word) => s.named_source(word),
            }
        };
        let (n, current, min, max, mean, dev) = if let Some(k) = meas_kind(item) {
            let Some(src) = source(self, parts.next(), ui.meas_source()) else { return t("9.9E37") };
            ui.meas_remote_stat(k, src)
        } else if let Some(k) = pair_kind(item) {
            let Some(a) = source(self, parts.next(), ui.meas_source()) else { return t("9.9E37") };
            let Some(b) = source(self, parts.next(), ui.meas_source_b()) else { return t("9.9E37") };
            ui.pair_remote_stat(k, a, b)
        } else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", item));
            return t("9.9E37");
        };
        let v = match kind {
            0 => max,
            1 => min,
            2 => current,
            3 => mean,
            4 => dev,
            _ => return t(&e(n as f64)),
        };
        t(&shown(v))
    }

    fn timebase_more(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "VERNier") {
            if query {
                return t(if ui.m.fine_t { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.m.fine_t = on;
                    if !on {
                        ui.m.tfine = 1.0;
                    }
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 2 && kw(n0, "HREFerence") && kw(n[1], "MODE") {
            if query {
                return t(match ui.m.h_expand {
                    HExpand::Center => "CENT",
                    HExpand::Left => "LB",
                    HExpand::Right => "RB",
                    HExpand::Trigger => "TRIG",
                });
            }
            let a = arg.trim();
            ui.m.h_expand = if kw(a, "CENTer") {
                HExpand::Center
            } else if a.eq_ignore_ascii_case("LB") || kw(a, "LEFT") {
                HExpand::Left
            } else if a.eq_ignore_ascii_case("RB") || kw(a, "RIGHt") {
                HExpand::Right
            } else if kw(a, "TRIGger") {
                HExpand::Trigger
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            ui.mark_changed();
            return None;
        }
        if n.len() == 2 && kw(n0, "DELay") && kw(n[1], "ENABle") {
            if query {
                return t(if ui.m.zoom { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.zoom_set_remote(on);
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 2 && kw(n0, "DELay") && kw(n[1], "SCALe") {
            if query {
                return t(&e(tscales()[ui.m.zscale] as f64));
            }
            let Some(v) = num(arg) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            let scales = tscales();
            let mut i = nearest(&scales, v);
            if i > ui.m.tscale {
                i = ui.m.tscale;
            }
            ui.m.zscale = i;
            ui.zoom_clamp();
            ui.mark_changed();
            return None;
        }
        if n.len() == 2 && kw(n0, "DELay") && (kw(n[1], "OFFSet") || kw(n[1], "POSition")) {
            if query {
                return t(&e(ui.m.zdelay as f64));
            }
            let Some(v) = num(arg) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            ui.m.zdelay = v;
            ui.zoom_clamp();
            ui.mark_changed();
            return None;
        }
        if n.len() == 2 && kw(n0, "XY") && kw(n[1], "ENABle") {
            if query {
                return t(if ui.xy_on() { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.xy_set(on);
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        self.push_err(format!("-113,\"Undefined header\" :TIMebase:{}", n.join(":")));
        None
    }

    /// Put a measurement on the readout. The query form stays in `meas_item`.
    fn meas_place(&mut self, ui: &mut Ui, arg: &str) -> Option<Vec<u8>> {
        let mut parts = arg.split(',');
        let item = parts.next().unwrap_or("").trim();
        if item.is_empty() {
            self.push_err("-224,\"Illegal parameter value\"".into());
            return None;
        }
        let source = |s: &mut Self, word: Option<&str>, fallback: WaveId| -> Option<WaveId> {
            match word.map(str::trim).filter(|s| !s.is_empty()) {
                None => Some(fallback),
                Some(word) => s.named_source(word),
            }
        };
        if let Some(k) = meas_kind(item) {
            let Some(src) = source(self, parts.next(), ui.meas_source()) else { return None };
            if ui.meas_add(k, src) {
                ui.mark_changed();
            } else {
                self.push_err("-223,\"Too much data\"".into());
            }
            return None;
        }
        if let Some(k) = pair_kind(item) {
            let Some(a) = source(self, parts.next(), ui.meas_source()) else { return None };
            let Some(b) = source(self, parts.next(), ui.meas_source_b()) else { return None };
            if ui.meas_add_pair(k, a, b) {
                ui.mark_changed();
            } else {
                self.push_err("-224,\"Illegal parameter value\" sources".into());
            }
            return None;
        }
        self.push_err(format!("-224,\"Illegal parameter value\" {}", item));
        None
    }

    fn cursor_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        if n.is_empty() || (n.len() == 1 && kw(n0, "MODE")) {
            if n.is_empty() {
                self.push_err("-113,\"Undefined header\" :CURSor".into());
                return None;
            }
            if query {
                let s = match ui.m.cursor {
                    CursorMode::Off => "OFF",
                    CursorMode::Time | CursorMode::Volt => "MAN",
                    CursorMode::Track => "TRAC",
                    CursorMode::Xy => "XY",
                };
                return t(s);
            }
            let a = arg.trim();
            ui.m.cursor = if kw(a, "OFF") {
                CursorMode::Off
            } else if kw(a, "MANual") {
                if ui.m.cursor == CursorMode::Volt { CursorMode::Volt } else { CursorMode::Time }
            } else if kw(a, "TRACk") {
                CursorMode::Track
            } else if a.eq_ignore_ascii_case("XY") {
                CursorMode::Xy
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            ui.mark_changed();
            return None;
        }
        if n.len() == 2 && kw(n0, "MANual") && kw(n[1], "TYPE") {
            if query {
                return t(if ui.m.cursor == CursorMode::Volt { "AMPL" } else { "TIME" });
            }
            if kw(arg, "TIME") {
                ui.m.cursor = CursorMode::Time;
            } else if kw(arg, "AMPLitude") {
                ui.m.cursor = CursorMode::Volt;
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            }
            ui.mark_changed();
            return None;
        }
        // One source: the active channel. Track's second source has nowhere to live, so a different SOURce2 is an error.
        // Manual X and Y share cur_a/cur_b. XY is the mode that keeps both axes (X in cur_a/cur_b, Y in cur_c/cur_d).
        if n.len() == 2 && (kw(n0, "MANual") || kw(n0, "TRACk")) && (kw(n[1], "SOURce") || kw(n[1], "SOURce1") || kw(n[1], "SOURce2")) {
            if query {
                return t(&format!("CHAN{}", ui.m.active.min(n_ch() - 1) + 1));
            }
            if kw(n[1], "SOURce2") && parse_ch(arg).is_some_and(|c| c != ui.m.active) {
                self.push_err("-224,\"Illegal parameter value\" one cursor source".into());
                return None;
            }
            return match parse_ch(arg) {
                Some(c) => {
                    ui.m.active = c;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() != 2 {
            self.push_err(format!("-113,\"Undefined header\" :CURSor:{}", n.join(":")));
            return None;
        }
        let which = n[1];
        let xy = kw(n0, "XY");
        let known = kw(n0, "MANual") || kw(n0, "TRACk") || xy;
        if !known {
            self.push_err(format!("-113,\"Undefined header\" :CURSor:{}", n.join(":")));
            return None;
        }
        let x = kw(which, "CAX") || (xy && kw(which, "AX"));
        let xb = kw(which, "CBX") || (xy && kw(which, "BX"));
        let y = kw(which, "CAY") || (xy && kw(which, "AY"));
        let yb = kw(which, "CBY") || (xy && kw(which, "BY"));
        if x || xb {
            return self.cursor_axis(ui, query, arg, true, x);
        }
        if y || yb {
            return self.cursor_axis(ui, query, arg, false, y);
        }
        if !query {
            self.push_err(format!("-113,\"Undefined header\" :CURSor:{}", n.join(":")));
            return None;
        }
        let miss = || Some(b"9.9E37".to_vec());
        let (ta, tb) = (ui.cursor_time_at(ui.m.cur_a), ui.cursor_time_at(ui.m.cur_b));
        let y_of = |a: bool| {
            if ui.m.cursor == CursorMode::Xy {
                ui.cursor_volt_at(if a { ui.m.cur_c } else { ui.m.cur_d })
            } else if ui.m.cursor == CursorMode::Volt {
                ui.cursor_volt_at(if a { ui.m.cur_a } else { ui.m.cur_b })
            } else if matches!(ui.m.cursor, CursorMode::Track | CursorMode::Time) {
                ui.wave_at(if a { ta } else { tb }).map(|v| v as f64).unwrap_or(f64::NAN)
            } else {
                f64::NAN
            }
        };
        if kw(which, "AXValue") {
            return if ui.m.cursor == CursorMode::Volt || ui.m.cursor == CursorMode::Off { miss() } else { t(&e(ta)) };
        }
        if kw(which, "BXValue") {
            return if ui.m.cursor == CursorMode::Volt || ui.m.cursor == CursorMode::Off { miss() } else { t(&e(tb)) };
        }
        if kw(which, "AYValue") {
            return t(&shown(y_of(true)));
        }
        if kw(which, "BYValue") {
            return t(&shown(y_of(false)));
        }
        if kw(which, "XDELta") {
            return if matches!(ui.m.cursor, CursorMode::Time | CursorMode::Track | CursorMode::Xy) { t(&e(tb - ta)) } else { miss() };
        }
        if kw(which, "IXDelta") {
            let d = (tb - ta).abs();
            return if d > 0.0 && matches!(ui.m.cursor, CursorMode::Time | CursorMode::Track | CursorMode::Xy) { t(&e(1.0 / d)) } else { miss() };
        }
        if kw(which, "YDELta") {
            let d = y_of(false) - y_of(true);
            return t(&shown(d));
        }
        self.push_err(format!("-113,\"Undefined header\" :CURSor:{}", n.join(":")));
        None
    }

    /// Horizontal (`x`) or vertical cursor. `a` is cursor A.
    fn cursor_axis(&mut self, ui: &mut Ui, query: bool, arg: &str, x: bool, a: bool) -> Option<Vec<u8>> {
        let xy = ui.m.cursor == CursorMode::Xy;
        if query {
            let frac = if x || !xy {
                if a { ui.m.cur_a } else { ui.m.cur_b }
            } else if a {
                ui.m.cur_c
            } else {
                ui.m.cur_d
            };
            let v = if x { ui.cursor_time_at(frac) } else { ui.cursor_volt_at(frac) };
            return Some(e(v).into_bytes());
        }
        let Some(v) = arg.trim().parse::<f64>().ok().filter(|v| v.is_finite()) else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        };
        let frac = if x { ui.cursor_frac_time(v) } else { ui.cursor_frac_volt(v) };
        if x || !xy {
            if a { ui.m.cur_a = frac; } else { ui.m.cur_b = frac; }
        } else if a {
            ui.m.cur_c = frac;
        } else {
            ui.m.cur_d = frac;
        }
        if ui.m.cursor == CursorMode::Off {
            ui.m.cursor = if x { CursorMode::Time } else { CursorMode::Volt };
        }
        ui.mark_changed();
        None
    }

    fn mask_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "ENABle") {
            if query {
                return t(if ui.m.mask_on { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.m.mask_on = on;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "SOURce") {
            if query {
                return t(&format!("CHAN{}", ui.m.mask_src.min(n_ch() - 1) + 1));
            }
            return match parse_ch(arg) {
                Some(c) => {
                    ui.m.mask_src = c;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && (kw(n0, "X") || kw(n0, "Y")) {
            let horizontal = kw(n0, "X");
            if query {
                return t(&e(if horizontal { ui.m.mask_x } else { ui.m.mask_y } as f64));
            }
            let Some(v) = num(arg) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            let ok = if horizontal { ui.mask_tolerance(Some(v), None) } else { ui.mask_tolerance(None, Some(v)) };
            if ok {
                ui.mark_changed();
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            }
            return None;
        }
        if n.len() == 1 && kw(n0, "CREate") && !query {
            if ui.mask_create() {
                ui.mark_changed();
            } else {
                self.push_err("-230,\"Data corrupt or stale\" source has no waveform".into());
            }
            return None;
        }
        if n.len() == 1 && kw(n0, "RESet") && !query {
            ui.mask_counts_reset();
            ui.mark_changed();
            return None;
        }
        if n.len() == 1 && kw(n0, "OPERate") {
            if query {
                return t(if ui.m.mask_on && ui.m.run == RunState::Run { "RUN" } else { "STOP" });
            }
            if !ui.m.mask_on {
                self.push_err("-224,\"Illegal parameter value\" mask is off".into());
                return None;
            }
            let a = arg.trim();
            if kw(a, "RUN") {
                ui.m.run = RunState::Run;
                if ui.m.trig_mode == TrigMode::Single {
                    ui.m.trig_mode = TrigMode::Auto;
                }
                ui.mark_changed();
                None
            } else if kw(a, "STOP") {
                ui.m.run = RunState::Stop;
                ui.mark_changed();
                None
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                None
            }
        } else if n.len() == 1 && query && (kw(n0, "FAILed") || kw(n0, "PASSed") || kw(n0, "TOTal")) {
            let (pass, fail) = ui.mask_counts();
            let n = if kw(n0, "FAILed") { fail } else if kw(n0, "PASSed") { pass } else { pass + fail };
            t(&n.to_string())
        } else {
            self.push_err(format!("-113,\"Undefined header\" :MASK:{}", n.join(":")));
            None
        }
    }

    fn dvm_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "ENABle") {
            if query {
                return t(if ui.m.dvm_on { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.m.dvm_on = on;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "SOURce") {
            if query {
                return t(&format!("CHAN{}", ui.m.dvm_src.min(n_ch() - 1) + 1));
            }
            return match parse_ch(arg) {
                Some(c) => {
                    ui.m.dvm_src = c;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "MODE") {
            const NAMES: [&str; 3] = ["DC", "ACRMS", "ACDC"];
            if query {
                return t(NAMES[ui.m.dvm_mode.min(2)]);
            }
            let a = arg.trim();
            let mode = if kw(a, "DC") {
                Some(0)
            } else if kw(a, "ACRMs") || a.eq_ignore_ascii_case("ACRMS") {
                Some(1)
            } else if kw(a, "ACDC") || kw(a, "ACDCrms") {
                Some(2)
            } else {
                None
            };
            return match mode {
                Some(m) => {
                    ui.m.dvm_mode = m;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "CURRent") && query {
            return t(&shown(ui.dvm_value()));
        }
        self.push_err(format!("-113,\"Undefined header\" :DVM:{}", n.join(":")));
        None
    }

    fn counter_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "ENABle") {
            if query {
                return t(if ui.m.cnt_on { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.m.cnt_on = on;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "SOURce") {
            if query {
                return t(&format!("CHAN{}", ui.m.cnt_src.min(n_ch() - 1) + 1));
            }
            return match parse_ch(arg) {
                Some(c) => {
                    ui.m.cnt_src = c;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "MODE") {
            const NAMES: [&str; 3] = ["FREQ", "PER", "TOT"];
            if query {
                return t(NAMES[ui.m.cnt_mode.min(2)]);
            }
            let a = arg.trim();
            let mode = if kw(a, "FREQuency") {
                Some(0)
            } else if kw(a, "PERiod") {
                Some(1)
            } else if kw(a, "TOTalize") {
                Some(2)
            } else {
                None
            };
            return match mode {
                Some(m) => {
                    ui.m.cnt_mode = m;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "NDIGits") {
            if query {
                return t(&ui.m.cnt_digits.to_string());
            }
            return match arg.trim().parse::<u32>().ok().filter(|d| (3..=7).contains(d)) {
                Some(d) => {
                    ui.m.cnt_digits = d;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "CURRent") && query {
            return t(&shown(ui.counter_value()));
        }
        if n.len() == 2 && kw(n0, "TOTalize") && kw(n[1], "CLEar") && !query {
            ui.counter_clear();
            ui.mark_changed();
            return None;
        }
        self.push_err(format!("-113,\"Undefined header\" :COUNter:{}", n.join(":")));
        None
    }

    fn wav_source_text(&self) -> String {
        match self.wav_src {
            WaveId::Ch(c) => format!("CHAN{}", c + 1),
            WaveId::Ref(i) => format!("R{}", i + 1),
        }
    }

    /// RAW, or MAXimum while stopped: the deep record. NORMal, and MAXimum while running: the screen.
    fn wave_memory(&self, ui: &Ui) -> bool {
        self.wav_mode == 2 || (self.wav_mode == 1 && ui.m.run == RunState::Stop)
    }

    fn wave_len(&self, ui: &Ui) -> u64 {
        match self.wav_src {
            WaveId::Ch(c) => ui.channel_points(c, self.wave_memory(ui)) as u64,
            WaveId::Ref(i) => ui.ref_info(i).map(|(n, _, _, _, _)| n as u64).unwrap_or(0),
        }
    }

    /// How many points the current STARt/STOP page holds, without copying them.
    fn page_count(&self, ui: &Ui) -> u64 {
        let n = self.wave_len(ui);
        let a = self.wav_start.max(1);
        if n == 0 || a > n {
            return 0;
        }
        let b = if self.wav_stop == 0 { n } else { self.wav_stop.clamp(a, n) };
        b - a + 1
    }

    fn wave_scale(&self, ui: &Ui) -> (f64, f64) {
        match self.wav_src {
            WaveId::Ch(c) => {
                let ch = &ui.m.ch[c];
                (ch.volts_per_div() as f64, ch.offset as f64)
            }
            WaveId::Ref(i) => ui.ref_info(i).map(|(_, _, _, vpd, off)| (vpd, off)).unwrap_or((1.0, 0.0)),
        }
    }

    fn y_codes(&self, vpd: f64, offset: f64) -> (f64, f64, f64) {
        let (yinc, yref) = match self.wav_fmt {
            1 => (vpd * VDIV as f64 / 60000.0, 32768.0),
            _ => (vpd * VDIV as f64 / 250.0, 128.0),
        };
        let yorg = if yinc == 0.0 { 0.0 } else { (offset / yinc).round() };
        (yinc, yorg, yref)
    }

    fn wave_preamble(&self, ui: &Ui) -> String {
        let (vpd, offset) = self.wave_scale(ui);
        let (yinc, yorg, yref) = self.y_codes(vpd, offset);
        let (dt, t0) = self.wave_origin(ui);
        format!("{},0,{},1,{},{},0,{},{},{}", self.wav_fmt, self.page_count(ui), e(dt), e(t0), e(yinc), yorg, yref)
    }

    /// Sample interval and the time of the first point of the current page. Does not copy the samples.
    fn wave_origin(&self, ui: &Ui) -> (f64, f64) {
        let (dt, t0) = match self.wav_src {
            WaveId::Ch(c) => ui.channel_origin(c, self.wave_memory(ui)).unwrap_or((0.0, 0.0)),
            WaveId::Ref(i) => ui.ref_info(i).map(|(_, dt, t0, _, _)| (dt, t0)).unwrap_or((0.0, 0.0)),
        };
        let n = self.wave_len(ui);
        let a = self.wav_start.max(1);
        if n == 0 || a > n {
            (dt, t0)
        } else {
            (dt, t0 + (a - 1) as f64 * dt)
        }
    }

    /// The page `:WAVeform:DATA?` returns, and the scale of the source.
    fn wave_page(&self, ui: &Ui) -> (Vec<f32>, f64, f64, f64, f64) {
        let (start, stop) = (self.wav_start.max(1), self.wav_stop);
        match self.wav_src {
            WaveId::Ch(c) => {
                let ch = &ui.m.ch[c];
                let (v, dt, t0) = ui.channel_page(c, self.wave_memory(ui), start, stop).unwrap_or((Vec::new(), 0.0, 0.0));
                (v, dt, t0, ch.volts_per_div() as f64, ch.offset as f64)
            }
            WaveId::Ref(i) => ui.ref_page(i, start, stop).unwrap_or((Vec::new(), 0.0, 0.0, 1.0, 0.0)),
        }
    }

    /// `:LA:...` (the stock names where they exist): state, lines shown, pod thresholds, and the record of the last
    /// acquisition: `:LA:DATA?` is an IEEE block of u16 words, little endian, bit n = Dn, sample k at
    /// t0 + k dt from `:LA:PREamble?` (points,dt,t0).
    fn la_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        use crate::ui::la;
        let t = |s: String| Some(s.into_bytes());
        if !la::available() {
            self.push_err(format!("-241,\"Hardware missing\" no logic analyser port on the {}", oos_hal::model::platform().caps.name));
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
                self.push_err("-230,\"Data corrupt or stale\" no LA record".into());
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
        self.push_err(format!("-113,\"Undefined header\" :LA:{}", n.join(":")));
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
                self.push_err(s);
                return Some(b"9.9E37".to_vec());
            }
        };
        self.one_fact(f(ui, &arg_item(arg), &src))
    }

    fn one_fact(&mut self, r: Result<f64, ()>) -> Option<Vec<u8>> {
        match r {
            Ok(v) => Some(shown(v).into_bytes()),
            Err(()) => {
                self.push_err("-224,\"Illegal parameter value\"".into());
                Some(b"9.9E37".to_vec())
            }
        }
    }

    /// `1,CHAN1` is slot 1 and the rest `CHAN1`. A bare `ON` stays on the current slot. A bad slot number is an error.
    /// Rigol's five reference colors. The screen's fifth color is violet; `ORANge` selects it because that is Rigol's fifth name.
    fn ref_color_name(i: usize) -> &'static str {
        ["GRAY", "RED", "GRE", "BLUE", "ORAN"][i.min(4)]
    }

    fn ref_pair(&mut self, ui: &Ui, arg: &str) -> (Option<usize>, String) {
        let arg = arg.trim();
        let (slot, rest) = if let Some((a, b)) = arg.split_once(',') {
            (parse_slot(a), b.trim().trim_matches('"').to_string())
        } else if arg.is_empty() {
            (Some(ui.ref_slot()), String::new())
        } else if let Some(i) = parse_slot(arg) {
            (Some(i), String::new())
        } else if arg.starts_with(|c: char| c.is_ascii_digit()) || arg.to_ascii_uppercase().starts_with('R') {
            (None, String::new())
        } else {
            (Some(ui.ref_slot()), arg.trim_matches('"').to_string())
        };
        match slot {
            Some(i) if i < NREF => (Some(i), rest),
            _ => {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                (None, String::new())
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
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if kw(n0, "SHOW") || kw(n0, "DISPlay") {
            let (slot, rest) = self.ref_pair(ui, arg);
            let Some(slot) = slot else { return None };
            if query {
                return t(if ui.ref_shown_slot(slot) == Some(true) { "1" } else { "0" });
            }
            return match parse_bool(&rest) {
                Some(on) if ui.ref_show_slot(slot, on) => {
                    ui.mark_changed();
                    None
                }
                _ => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if kw(n0, "CURRent") {
            if query {
                return t(&(ui.ref_slot() + 1).to_string());
            }
            return match parse_slot(arg) {
                Some(i) if ui.ref_slot_set(i) => {
                    ui.mark_changed();
                    None
                }
                _ => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if kw(n0, "SOURce") {
            let (slot, rest) = self.ref_pair(ui, arg);
            let Some(slot) = slot else { return None };
            if query {
                return t(&format!("CHAN{}", ui.ref_source_of(slot) + 1));
            }
            return match parse_ch(&rest) {
                Some(c) if ui.ref_source_set(slot, c) => {
                    ui.mark_changed();
                    None
                }
                _ => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if kw(n0, "VSCale") || kw(n0, "VOFFset") {
            let scale = kw(n0, "VSCale");
            let (slot, rest) = self.ref_pair(ui, arg);
            let Some(slot) = slot else { return None };
            if query {
                let v = if scale { ui.ref_vscale(slot) } else { ui.ref_voffset(slot) };
                return match v {
                    Some(v) => t(&e(v as f64)),
                    None => {
                        self.push_err("-230,\"Data corrupt or stale\" reference is empty".into());
                        None
                    }
                };
            }
            let Some(v) = num(&rest) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            let ok = if scale { ui.ref_vscale_set(slot, v) } else { ui.ref_voffset_set(slot, v) };
            if ok {
                ui.mark_changed();
            } else {
                self.push_err("-230,\"Data corrupt or stale\" reference is empty".into());
            }
            return None;
        }
        if n.len() >= 2 && kw(n0, "LABel") && kw(n[1], "CONTent") {
            let (slot, rest) = self.ref_pair(ui, arg);
            let Some(slot) = slot else { return None };
            if query {
                return match ui.ref_label(slot) {
                    Some(s) => t(&s),
                    None => {
                        self.push_err("-230,\"Data corrupt or stale\" reference is empty".into());
                        None
                    }
                };
            }
            if ui.ref_label_set(slot, rest) {
                ui.mark_changed();
            } else {
                self.push_err("-230,\"Data corrupt or stale\" reference is empty".into());
            }
            return None;
        }
        if kw(n0, "RESet") && !query {
            let (slot, _) = self.ref_pair(ui, arg);
            let Some(slot) = slot else { return None };
            if ui.ref_reset_scale(slot) {
                ui.mark_changed();
            } else {
                self.push_err("-230,\"Data corrupt or stale\" reference is empty".into());
            }
            return None;
        }
        if kw(n0, "COLor") {
            let (slot, rest) = self.ref_pair(ui, arg);
            let Some(slot) = slot else { return None };
            if query {
                return match ui.ref_color(slot) {
                    Some(i) => t(Self::ref_color_name(i)),
                    None => {
                        self.push_err("-230,\"Data corrupt or stale\" reference is empty".into());
                        None
                    }
                };
            }
            let Some(i) = ref_color_index(&rest) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            if ui.ref_color_set(slot, i) {
                ui.mark_changed();
            } else {
                self.push_err("-230,\"Data corrupt or stale\" reference is empty".into());
            }
            return None;
        }
        if kw(n0, "CLEar") && !query {
            ui.ref_clear_all();
            ui.mark_changed();
            return None;
        }
        if kw(n0, "SAVE") && !query {
            if !arg.trim().is_empty() {
                match parse_slot(arg) {
                    Some(i) if ui.ref_slot_set(i) => {}
                    _ => {
                        self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                        return None;
                    }
                }
            }
            return match ui.ref_save_slot() {
                Ok(()) => {
                    ui.mark_changed();
                    None
                }
                Err(e) => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", e));
                    None
                }
            };
        }
        if kw(n0, "LOAD") && !query {
            if arg.trim().is_empty() {
                self.push_err("-224,\"Illegal parameter value\" no file".into());
                return None;
            }
            let r = ui.ref_load_named(arg);
            return self.ref_loaded(ui, r);
        }
        if kw(n0, "IMPort") && !query {
            let r = if arg.trim().is_empty() { ui.ref_import_slot() } else { ui.ref_load_named(arg) };
            return self.ref_loaded(ui, r);
        }
        self.push_err(format!("-113,\"Undefined header\" :REF:{}", n.join(":")));
        None
    }

    fn ref_loaded(&mut self, ui: &mut Ui, r: Result<(), String>) -> Option<Vec<u8>> {
        match r {
            Ok(()) => {
                ui.mark_changed();
                None
            }
            Err(e) if e.starts_with("missing ") => {
                self.push_err(format!("-256,\"File name not found\" {}", e.trim_start_matches("missing ")));
                None
            }
            Err(e) => {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", e));
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
                _ => { self.push_err("-224,\"Illegal parameter value\" SSC mode".into()); return None; }
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
                    _ => { self.push_err("-224,\"Illegal parameter value\" SIM setting".into()); return None; }
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
                self.push_err("-224,\"Illegal parameter value\" PRBS order".into());
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
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
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
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
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
                        Err(error) => self.push_err(format!("-224,\"Illegal parameter value\" {}", error)),
                    }
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
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
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        self.push_err(format!("-113,\"Undefined header\" :SIM:{}", n.join(":")));
        None
    }

    fn wave_data(&self, ui: &Ui) -> Vec<u8> {
        let (vpd, offset) = self.wave_scale(ui);
        let (yinc, yorg, yref) = self.y_codes(vpd, offset);
        let (v, _, _, _, _) = self.wave_page(ui);
        match self.wav_fmt {
            2 => {
                let s: Vec<String> = v.iter().map(|&x| e(x as f64)).collect();
                block(s.join(",").as_bytes())
            }
            1 => {
                // Rigol WORD is high byte first. A 25 Mpoint read pages with STARt and STOP so this buffer is one page.
                let mut b = Vec::with_capacity(v.len() * 2);
                for x in v {
                    let code = ((x as f64 / yinc) + yorg + yref).round().clamp(0.0, 65535.0) as u16;
                    b.extend_from_slice(&code.to_be_bytes());
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
            self.push_err("-224,\"Illegal parameter value\"".into());
            return t("9.9E37");
        }
        if kw(item, "SETHold") || item.eq_ignore_ascii_case("SHOLD") {
            let waves = match self.waves(&parts.collect::<Vec<_>>().join(",")) {
                Ok(waves) => waves,
                Err(error) => {
                    self.push_err(error);
                    return t("9.9E37");
                }
            };
            let (clock, data) = match waves.as_slice() {
                [] => ui.meas_pair_sources(),
                [clock, data] => (*clock, *data),
                _ => {
                    self.push_err("-224,\"Illegal parameter value\"".into());
                    return t("9.9E37");
                }
            };
            return t(&shown(ui.fact_setup_hold(clock, data)));
        }
        // no source: CH1, as before. A word that is not CHANn or R1–R10 is an error.
        let src = match parts.next().map(str::trim).filter(|s| !s.is_empty()) {
            None => ui.meas_source(),
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
                self.push_err("-224,\"Illegal parameter value\"".into());
                return t("9.9E37");
            };
            let Some(b) = self.named_source(word) else { return t("9.9E37") };
            return t(&e(ui.pair_at(k, src, b)));
        }
        self.push_err("-224,\"Illegal parameter value\"".into());
        t("9.9E37")
    }

    fn named_source(&mut self, s: &str) -> Option<WaveId> {
        match parse_wave(s) {
            Some(id) => Some(id),
            None => {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", s));
                None
            }
        }
    }

    /// :EYE:ITEM? <figure> — height, width, unit interval, total jitter, and the other figures on the eye page.
    fn eye_item(&mut self, ui: &Ui, arg: &str) -> Option<Vec<u8>> {
        let item = arg.split(',').next().unwrap_or("").trim();
        let Some(name) = crate::ui::EYE_ITEMS.iter().find(|n| kw(item, n)) else {
            self.push_err("-224,\"Illegal parameter value\"".into());
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
        p.push(format!("{},{},{}", meas_token(k), wave_tok(id), e(ui.meas_at(id, k))));
    }
    for &(k, a, b) in &ui.m.meas2 {
        p.push(format!("{},{},{},{}", pair_token(k), wave_tok(a), wave_tok(b), e(ui.pair_at(k, a, b))));
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

    fn reference_session() -> (Scpi, Ui, Canvas) {
        let (mut s, mut ui, cv) = session();
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
        raw[2..10].copy_from_slice(b":WFM#001");
        std::fs::write(&bad, &raw).unwrap();
        line(&mut s, &mut ui, &cv, &format!(":REF:LOAD \"{}\"", bad.display()));
        let err = line(&mut s, &mut ui, &cv, ":SYST:ERR?");
        assert!(err.contains(":WFM#002"), "{err}");
        assert!(err.contains(":WFM#003"), "{err}");

        line(&mut s, &mut ui, &cv, ":REF:CLE");
        assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:POIN?"), "0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probewright_queries_answer_and_the_rigol_pages_round_trip() {
        let (mut s, mut ui, cv) = session();
        assert_eq!(line(&mut s, &mut ui, &cv, "*ESR?"), "128", "power-on bit until the first read");
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
        assert_eq!(n, 6, "three WORD points, high byte first");
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
        line(&mut s, &mut ui, &cv, ":TIM:MAIN:SCAL 1e-6;:TIM:MAIN:OFFS 0;:CHAN1:SCAL 1;:CHAN1:OFFS 0");
        line(&mut s, &mut ui, &cv, ":CURS:MODE MAN;:CURS:MAN:TYPE TIME;:CURS:MAN:SOUR CHAN1;:CURS:MAN:CAX 0;:CURS:MAN:CBX 1e-6");
        assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MODE?;:CURS:MAN:TYPE?;:CURS:MAN:SOUR?"), "MAN;TIME;CHAN1");
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

        line(&mut s, &mut ui, &cv, ":DVM:ENAB ON;:DVM:SOUR CHAN2;:DVM:MODE ACRMS;:COUN:ENAB ON;:COUN:MODE PER;:COUN:NDIG 6");
        assert_eq!(line(&mut s, &mut ui, &cv, ":DVM:ENAB?;:DVM:SOUR?;:DVM:MODE?;:COUN:ENAB?;:COUN:MODE?;:COUN:NDIG?"), "1;CHAN2;ACRMS;1;PER;6");
        assert!(!line(&mut s, &mut ui, &cv, ":DVM:CURR?;:COUN:CURR?").is_empty());
        line(&mut s, &mut ui, &cv, ":COUN:TOT:CLE");
    }

    #[test]
    fn statistics_channel_labels_and_the_zoom_window_round_trip() {
        let (mut s, mut ui, cv) = reference_session();
        line(&mut s, &mut ui, &cv, ":CHAN1:LAB:CONT Vbus;:CHAN1:UNIT AMP;:CHAN1:VERN ON;:CHAN1:POS 0.2");
        assert_eq!(line(&mut s, &mut ui, &cv, ":CHAN1:LAB:CONT?;:CHAN1:LAB:SHOW?;:CHAN1:UNIT?;:CHAN1:VERN?"), "Vbus;1;AMP;1");
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
    fn acquisition_deskew_reference_color_and_thresholds_round_trip() {
        let (mut s, mut ui, cv) = reference_session();
        line(&mut s, &mut ui, &cv, ":ACQ:TYPE AVER;:ACQ:AVER 100;:ACQ:ULTR:MAXF 100");
        assert_eq!(line(&mut s, &mut ui, &cv, ":ACQ:TYPE?;:ACQ:AVER?;:ACQ:ULTR:MAXF?"), "AVER;64;100");
        line(&mut s, &mut ui, &cv, ":ACQ:TYPE HRES;:ACQ:ULTR:MAXF 0");
        assert_eq!(line(&mut s, &mut ui, &cv, ":ACQ:TYPE?;:ACQ:ULTR:MAXF?"), "HRES;0");

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

        line(&mut s, &mut ui, &cv, ":MEAS:SET:MIN 20;:MEAS:SET:MAX 80");
        assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:SET:MIN?;:MEAS:SET:MAX?"), "20;80");
        line(&mut s, &mut ui, &cv, ":MEAS:SET:MAX 10");
        assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"), "the upper threshold must stay above the lower");
        assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:SET:MAX?"), "80");
    }

    #[test]
    fn a_mask_counts_passes_and_failures() {
        let (mut s, mut ui, cv) = session();
        for _ in 0..20 {
            ui.step(0.05);
        }
        line(&mut s, &mut ui, &cv, "*CLS;:MASK:SOUR CHAN1;:MASK:X 0.5;:MASK:Y 0.5;:MASK:ENAB ON;:MASK:CRE;:MASK:OPER RUN");
        assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"", "the demo record must be there to build a mask");
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
