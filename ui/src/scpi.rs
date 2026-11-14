// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! SCPI server (TCP 5555, the port of the stock firmware), a stock-compatible subset:
//! *IDN? *RST *OPC[?] *CLS *ESR?, :RUN :STOP :SINGle :TFORce :AUToscale :CLEar,
//! :CHANnel<n>:DISPlay|SCALe|OFFSet|POSition|COUPling|BWLimit|PROBe|INVert|UNITs|VERNier|TCALibrate|BIAS|EXPand|LABel:SHOW|LABel:CONTent,
//! :TIMebase[:MAIN]:SCALe|OFFSet, :TIMebase:DELay:ENABle|SCALe|OFFSet, :TIMebase:VERNier, :TIMebase:HREFerence:MODE, :TIMebase:MODE, :TIMebase:ROLL,
//! :TIMebase:XY:ENABle|X|Y|Z,
//! :TRIGger:MODE[?]|TYPe[?] :TRIGger:EDGE:SOURce|LEVel|SLOPe, :TRIGger:SWEep, :TRIGger:HOLDoff, :TRIGger:STATus?,
//! :ACQuire:SRATe?|MDEPth[?] (AUTO, 1k..125M)|TYPE NORM/PEAK/AVER/HRES|AVERages|ULTRa:MAXFrame,
//! :MEASure:ITEM[?] <item>[,CHANn|Rn[,src]] (the set form adds a readout row), :MEASure:SOURce, :MEASure:CLEar, :MEASure:INDicator,
//! :MEASure:STRip?, :MEASure:READout?, :EYE:ITEM? <figure>,
//! :MEASure:STATistic:DISPlay|RESet|ITEM, :MEASure:REFS 10/90|20/80, :MEASure:TSETup, :MEASure:THOLd,
//! :MEASure:BURSt:IDLE, :MEASure:OUTSide ABOVe|BELow|BOTH, :MEASure:NPERiods, :MEASure:EDGes CLOCk|DATA,
//! :CURSor:MODE OFF|MAN|TRAC|XY, :CURSor:MANual:TYPE TIME|AMPLitude, :CAX|:CBX|:CAY|:CBY, :AXValue|:AYValue|:BXValue|:BYValue, :XDELta|:IXDelta|:YDELta, :CURSor:MEASure:INDicator,
//! :DVM:ENABle|SOURce|MODE|CURRent?, :COUNter:ENABle|SOURce|MODE|NDIGits|CURRent?,
//! :MEASure:GATing SCREEN|NONE,
//! :MASK:ENABle|SOURce|X|Y|CREate|RESet|OPERate|FAILed?|PASSed?|TOTal?,
//! :HISTogram:ENABle|TYPE|SOURce, :HISTogram:STATistics:RESult?,
//! :MATH<n>:DISPlay|OPERator|SOURce1|SOURce2|SCALe|OFFSet|EXPRession, :MATH<n>:FFT:SOURce|WINDow|SCALe, :MATH<n>:FILTer:TYPE|W1|W2,
//! :BUS<n>:MODE|DISPlay|FORMat|EVENt|THReshold|DATA?, :BUS<n>:RS232:TX|RX|BAUD|PARity|DBITs, :BUS<n>:IIC:SCLK:SOURce|SDA:SOURce, :BUS<n>:SPI:SCLK:SOURce|MOSI:SOURce|SS:SOURce|DBITs,
//! :POWer:ITEM? <item>[,src...], :POWer:HARMonics? [src], :SPECtrum:ITEM? <item>[,src], :BODE:MODE LOOP|PSRR|IMPedance,
//! :MAGnetic:ITEM? <item>[,V,I], :INVerter:ITEM? <item>[,src...], :DPT:ITEM? <item>[,V,I[,G]], :BANDwidth:ITEM? <item>[,src],
//! :WAVeform:SOURce CHANn|Rn|MODE NORM/MAX/RAW|FORMat BYTE/WORD/ASC|STARt|STOP|POINts|DATA?|PREamble?,
//! :DISPlay:DATA? [BMP|PNG], :DISPlay:TYPE|GRADing:TIME|WBRightness|GBRightness|GRID|RULers|COLor,
//! :REFerence:SLOT R1..R10, :CURRent, :SOURce, :VSCale, :VOFFset, :COLor, :RESet, :LABel:CONTent, :LOAD <file>, :IMPort [file], :SAVE, :SHOW ON|OFF, :CLEar,
//! :MEASure:SETup:MAX|MIN (percent thresholds), :MEASure:SETup:PSA|PSB|DSA|DSB,
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
use crate::math::{math_scales, MathCh, MathOp, Window, NMATH, MATH_REF_BASE};
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
    /// A completed query may legitimately produce zero payload bytes.
    pub has_reply: bool,
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
        if self.has_reply { self.out.push(self.separator); }
        self.out.extend_from_slice(reply); self.separator = b';'; self.has_reply = true;
    }
    /// Adapter used by transport unit fixtures with a synchronous test interpreter.
    #[cfg(test)]
    pub fn immediate(&mut self, exec: &mut dyn FnMut(&str) -> Vec<u8>) -> Vec<u8> {
        while let Some((command, new_line)) = self.commands.pop_front() {
            if new_line && self.has_reply { self.separator = b'\n'; }
            let reply = exec(&command);
            if !reply.is_empty() || command.split_whitespace().next().is_some_and(|header| header.ends_with('?')) { self.append(&reply); }
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
    /// :WAVeform:MODE: 0 NORMal (screen), 1 MAXimum (memory while stopped), 2 RAW (memory)
    wav_mode: u8,
    /// :WAVeform:STARt and :WAVeform:STOP, 1-based inclusive. `stop == 0` reads through the last point.
    wav_start: u64,
    wav_stop: u64,
    err: Vec<String>,
    event_status: u8,
    error_serial: u64,
    last_error: Option<String>,
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

fn persist_index(arg: &str) -> Option<usize> {
    let a = arg.trim();
    if kw(a, "MIN") {
        return Some(0);
    }
    if kw(a, "INFinite") {
        return Some(PERSIST.len() - 1);
    }
    let v = a.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)?;
    let times = [0.0, 0.1, 0.5, 1.0, 5.0, 10.0];
    times.iter().enumerate().min_by(|(_, a), (_, b)| (*a - v).abs().partial_cmp(&(*b - v).abs()).unwrap_or(std::cmp::Ordering::Equal)).map(|(i, _)| i)
}

fn persist_token(i: usize) -> &'static str {
    match i {
        0 => "MIN",
        1 => "0.1",
        2 => "0.5",
        3 => "1",
        4 => "5",
        5 => "10",
        _ => "INF",
    }
}

fn bus_index(tok: &str) -> Option<usize> {
    let (k, n) = suffix(tok);
    if kw(k, "BUS") { n.filter(|&n| (1..=DECODERS).contains(&n)).map(|n| n - 1) } else { None }
}

fn with_bus(ui: &mut Ui, k: usize, f: impl FnOnce(&mut Model)) {
    let was = ui.m.dec_sel;
    if k != was {
        ui.m.dec_select(k);
    }
    f(&mut ui.m);
    if k != was {
        ui.m.dec_select(was);
    }
    if k == was && ui.m.trig_type == TrigType::Bus { ui.mark_changed(); }
}

fn parse_bus_wave(ui: &Ui, text: &str) -> Option<usize> {
    match parse_wave(text)? {
        WaveId::Ch(c) if c < n_ch() => Some(c),
        WaveId::Ref(i) if ui.ref_info(i).is_some() => Some(DEC_REF_BASE + i),
        _ => None,
    }
}

fn bus_source_name(source: usize) -> String {
    match decode_wave(source) {
        Some(WaveId::Ch(c)) => format!("CHAN{}", c + 1),
        Some(WaveId::Ref(i)) => format!("R{}", i + 1),
        None => "Unavailable".into(),
    }
}

/// Configure the requested decoder slot, preserving the visible/trigger decoder.
/// Only the active decoder's primary role can take over the acquisition source.
fn put_bus_wave(ui: &mut Ui, slot: usize, protocol: Proto, role: usize, source: Option<usize>) -> bool {
    let selected = ui.m.dec_sel;
    if slot != selected { ui.m.dec_select(slot); }
    let previous_protocol = ui.m.dec_proto;
    ui.m.dec_proto = protocol;
    let ok = if slot == selected { ui.configure_decoder_source(role, source) }
        else {
            ui.m.dec_la_in = false;
            match role { 0 => ui.m.dec_src = source.unwrap(), 1 => ui.m.dec_src2 = source.unwrap(),
                2 => ui.m.dec_cs = source, _ => unreachable!() }
            true
        };
    if !ok { ui.m.dec_proto = previous_protocol; }
    if slot != selected { ui.m.dec_select(selected); }
    if ok && slot == selected && ui.m.trig_type == TrigType::Bus { ui.mark_changed(); }
    ok
}

fn proto_token(p: Proto) -> String {
    match p {
        Proto::Parallel => "PAR".into(),
        Proto::Uart => "RS232".into(),
        Proto::Spi => "SPI".into(),
        Proto::I2c => "IIC".into(),
        Proto::Lin => "LIN".into(),
        Proto::Can => "CAN".into(),
        other => other.name().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase(),
    }
}

fn parse_proto(arg: &str) -> Option<Proto> {
    let a = arg.trim();
    if kw(a, "PARallel") { return Some(Proto::Parallel); }
    if kw(a, "RS232") || kw(a, "UART") { return Some(Proto::Uart); }
    if kw(a, "IIC") || kw(a, "I2C") { return Some(Proto::I2c); }
    if kw(a, "LIN") { return Some(Proto::Lin); }
    if kw(a, "SPI") { return Some(Proto::Spi); }
    if kw(a, "CAN") { return Some(Proto::Can); }
    PROTOS.into_iter().find(|p| proto_token(*p).eq_ignore_ascii_case(a))
}

fn nearest_baud(v: f64) -> usize {
    crate::decode::BAUDS.iter().enumerate().min_by(|(_, a), (_, b)| (*a - v).abs().partial_cmp(&(*b - v).abs()).unwrap_or(std::cmp::Ordering::Equal)).map(|(i, _)| i).unwrap_or(0)
}

fn chan_token(c: usize) -> String {
    format!("CHAN{}", c.min(NCH - 1) + 1)
}

fn math_index(tok: &str) -> Option<usize> {
    let (k, n) = suffix(tok);
    if kw(k, "MATH") { n.filter(|&n| (1..=NMATH).contains(&n)).map(|n| n - 1) } else { None }
}

fn put_math(ui: &mut Ui, k: usize, c: MathCh) {
    if k == ui.m.math_sel {
        ui.m.math_lin = c.lin;
        ui.m.math = c.op;
        ui.m.math_a = c.a;
        ui.m.math_b = c.b;
        ui.m.math_scale = c.scale;
        ui.m.math_fc = c.fc;
        ui.m.math_bw = c.bw;
        ui.m.math_offset = c.offset;
        ui.m.fft_win = c.win;
    }
    ui.m.maths[k] = c;
    ui.mark_changed();
}

fn math_op_name(op: MathOp) -> &'static str {
    match op {
        MathOp::Off => "OFF",
        MathOp::Add => "ADD",
        MathOp::Sub => "SUBT",
        MathOp::Mul => "MULT",
        MathOp::Div => "DIV",
        MathOp::And => "AND",
        MathOp::Or => "OR",
        MathOp::Xor => "XOR",
        MathOp::Not => "NOT",
        MathOp::Fft => "FFT",
        MathOp::Intg => "INTG",
        MathOp::Diff => "DIFF",
        MathOp::Sqrt => "SQRT",
        MathOp::Lg => "LG",
        MathOp::Ln => "LN",
        MathOp::Exp => "EXP",
        MathOp::Abs => "ABS",
        MathOp::Lpf => "LPAS",
        MathOp::Hpf => "HPAS",
        MathOp::Bpf => "BPAS",
        MathOp::Bsf => "BST",
        MathOp::Lin => "AXB",
        MathOp::Nand => "NAND",
        MathOp::Nor => "NOR",
        MathOp::Conv => "CONV",
        MathOp::Fir => "FIR",
        MathOp::Expr => "EXPR",
        MathOp::A2d => "A2D",
        MathOp::D2a => "D2A",
        MathOp::Unnrzi => "UNNRZI",
    }
}

fn parse_math_op(arg: &str) -> Option<MathOp> {
    [
        ("ADD", MathOp::Add),
        ("SUBTract", MathOp::Sub),
        ("MULTiply", MathOp::Mul),
        ("DIVision", MathOp::Div),
        ("AND", MathOp::And),
        ("OR", MathOp::Or),
        ("XOR", MathOp::Xor),
        ("NOT", MathOp::Not),
        ("FFT", MathOp::Fft),
        ("INTG", MathOp::Intg),
        ("DIFF", MathOp::Diff),
        ("SQRT", MathOp::Sqrt),
        ("LG", MathOp::Lg),
        ("LN", MathOp::Ln),
        ("EXP", MathOp::Exp),
        ("ABS", MathOp::Abs),
        ("LPASs", MathOp::Lpf),
        ("HPASs", MathOp::Hpf),
        ("BPASs", MathOp::Bpf),
        ("BSTop", MathOp::Bsf),
        ("AXB", MathOp::Lin),
        ("NAND", MathOp::Nand),
        ("NOR", MathOp::Nor),
        ("CONV", MathOp::Conv),
        ("FIR", MathOp::Fir),
        ("EXPR", MathOp::Expr),
        ("A2D", MathOp::A2d),
        ("D2A", MathOp::D2a),
        ("UNNRZI", MathOp::Unnrzi),
    ]
    .into_iter()
    .find(|(name, _)| kw(arg.trim(), name))
    .map(|(_, op)| op)
}

fn math_src_name(src: usize) -> String {
    if src < NCH { format!("CHAN{}", src + 1) }
    else if src < MATH_REF_BASE { format!("MATH{}", src - NCH + 1) }
    else { format!("R{}", src - MATH_REF_BASE + 1) }
}

/// A math input: a live channel, or an earlier math trace. A later trace would be a cycle.
fn parse_math_src(s: &str, k: usize) -> Option<usize> {
    if let Some(id) = parse_wave(s) {
        return match id { WaveId::Ch(c) if c < n_ch() => Some(c), WaveId::Ref(r) => Some(MATH_REF_BASE + r), _ => None };
    }
    let idx = math_index(s.trim())?;
    (idx < k).then_some(NCH + idx)
}

fn nearest_step(steps: &[f64], want: f64) -> usize {
    steps.iter().enumerate().min_by(|(_, a), (_, b)| (*a - want).abs().partial_cmp(&(*b - want).abs()).unwrap_or(std::cmp::Ordering::Equal)).map(|(i, _)| i).unwrap_or(0)
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\''))) { &s[1..s.len() - 1] } else { s }
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
        Scpi { l, clients: Vec::new(), wav_src: WaveId::Ch(0), wav_fmt: 0, wav_mode: 0, wav_start: 1, wav_stop: 0, err: Vec::new(), event_status: 0, error_serial: 0, last_error: None, opc_events: Vec::new() }
    }

    /// Queue a SCPI error and latch the matching *ESR bit. A query that fails still answers with this text,
    /// so a client blocked on a line never waits forever.
    fn push_err(&mut self, mut message: String) {
        if message.len() > ERR_LEN {
            let mut end = ERR_LEN;
            while !message.is_char_boundary(end) { end -= 1; }
            message.truncate(end);
        }
        let code = message.split(',').next().unwrap_or("").trim().parse::<i32>().unwrap_or(0);
        self.event_status |= match code {
            -199..=-100 => 32,
            -299..=-200 => 16,
            -399..=-300 => 8,
            -499..=-400 => 4,
            _ => 16,
        };
        self.error_serial = self.error_serial.wrapping_add(1);
        self.last_error = Some(message.clone());
        if self.err.len() < ERR_MAX { self.err.push(message); }
        else {
            self.err[ERR_MAX - 1] = "-350,\"Queue overflow\"".into();
            self.event_status |= 8;
        }
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
                if program.has_reply {
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
        self.exec_message_checked(ui, cv, line).map(|(response, _)| response)
    }

    pub fn exec_message_checked(&mut self, ui: &mut Ui, cv: &Canvas, line: &str) -> Result<(Vec<u8>, bool), &'static str> {
        let mut program = Program::new(line);
        match self.poll_program(ui, cv, &mut program) {
            Some(response) => Ok((response, program.has_reply)),
            None => {
                let error = "Operation pending; use raw TCP or HiSLIP for *OPC?";
                self.push_err(format!("-221,\"{}\"", error));
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

    fn append_program_reply(&mut self, program: &mut Program, response: &[u8]) -> bool {
        let separator = usize::from(program.has_reply);
        if response.len() > RESP_MAX.saturating_sub(program.out.len().saturating_add(separator)) {
            self.push_err("-363,\"Output buffer overrun; remaining commands dropped\"".into());
            program.commands.clear();
            return false;
        }
        program.append(response);
        true
    }

    fn advance_program(&mut self, ui: &mut Ui, cv: &Canvas, program: &mut Program) -> Option<Vec<u8>> {
        self.poll_events(ui);
        if program.invalid { self.push_err("-363,\"Program message too large or too many commands\"".into()); return Some(Vec::new()); }
        if let Some(fence) = &program.waiting {
            match fence.state(ui) {
                crate::ui::OperationState::Pending => return None,
                crate::ui::OperationState::Complete => {
                    program.waiting = None;
                    if !self.append_program_reply(program, b"1") { return Some(std::mem::take(&mut program.out)); }
                },
                crate::ui::OperationState::Cancelled => {
                    self.push_err("-410,\"OPC query interrupted by cancelled operation\"".into());
                    program.commands.clear(); program.out.clear(); program.has_reply = false; program.waiting = None;
                    return Some(Vec::new());
                }
            }
        }
        while let Some((command, new_line)) = program.commands.pop_front() {
            if new_line && program.has_reply { program.separator = b'\n'; }
            if program.out.len() >= RESP_MAX {
                self.push_err("-363,\"Input buffer overrun\" rest of the line dropped".into());
                program.commands.clear(); break;
            }
            if command.eq_ignore_ascii_case("*OPC?") {
                let fence = ui.scpi_operations();
                if fence.state(ui) == crate::ui::OperationState::Pending { program.waiting = Some(fence); return None; }
                if !self.append_program_reply(program, b"1") { break; }
            } else if let Some(response) = self.exec(ui, cv, &command) {
                if !self.append_program_reply(program, &response) { break; }
            }
        }
        Some(std::mem::take(&mut program.out))
    }

    fn exec(&mut self, ui: &mut Ui, cv: &Canvas, cmd: &str) -> Option<Vec<u8>> {
        if let Some(error) = ui.take_sim_error() {
            self.push_err(format!("-224,\"SIM generation failed\" {error}"));
        }
        let before = self.error_serial;
        let r = self.exec_cmd(ui, cv, cmd);
        // a query with no payload would leave the client blocked on the next line: answer with the error instead
        let query = cmd.split_whitespace().next().unwrap_or("").ends_with('?');
        let r = if query && r.is_none() {
            if self.error_serial == before {
                self.push_err(format!("-113,\"Undefined header\" {}", cmd));
            }
            self.last_error.as_ref().map(|error| error.as_bytes().to_vec())
        } else {
            r
        };
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
            self.push_err("-350,\"Queue overflow\"".into());
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
                else { self.push_err("-350,\"Too many pending OPC events\"".into()); }
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
            ui.force_trigger();
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
        // Rigol: a forced trigger releases Normal or Single when the level never crosses. On a reference or SIM
        // session that is the replay flag. On the live hardware the front-panel key also asks the acquisition
        // thread to pulse the FPGA force bit.
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
                    if !ui.set_trigger_source(id, secondary) { self.push_err("-224,\"Trigger source is unavailable\"".into()); }
                    ui.mark_changed();
                    return None;
                }
                // Leave D0..D15 to the existing LA source handler.
                if !arg.trim().to_ascii_uppercase().starts_with('D') {
                    self.push_err("-224,\"Invalid trigger source\"".into());
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
                else { self.push_err("-224,\"Invalid trigger type\"".into()); }
                return None;
            }
            if nodes.len() == 3 && kw(nodes[1], "RUNT") {
                let value = if kw(last, "UPPer") || kw(last, "ALEVel") { Some(&mut ui.m.trig_level) }
                    else if kw(last, "LOWer") || kw(last, "BLEVel") { Some(&mut ui.m.trig_level2) } else { None };
                if let Some(value) = value {
                    if query { return t(&e(*value as f64)); }
                    if let Some(number) = num(arg) { *value = number; ui.mark_changed(); }
                    else { self.push_err("-224,\"Invalid runt threshold\"".into()); }
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
                if kw(n1, "EXPand") {
                    // One switch for every channel: scale around ground, or around the screen center.
                    // The manual spells this GND|CENTer on math. This screen keeps it on the channel.
                    if query {
                        return Some(t(if m.v_expand_gnd { "GND" } else { "CENT" }));
                    }
                    if kw(arg, "GND") {
                        m.v_expand_gnd = true;
                    } else if kw(arg, "CENTer") {
                        m.v_expand_gnd = false;
                    } else {
                        self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    }
                    return Some(None);
                }
                let expand_gnd = m.v_expand_gnd;
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
                    let old = ch.volts_per_div();
                    ch.vscale = nearest(&VSCALES, num(arg)? / ch.probe);
                    ch.vfine = 1.0;
                    if expand_gnd && old > 0.0 {
                        ch.offset *= ch.volts_per_div() / old;
                    }
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
                } else if kw(n1, "BIAS") {
                    // The screen's zero correction, subtracted from the channel reading.
                    // The DHO programmer manual has no header for it.
                    if query {
                        return Some(t(&e(ch.bias as f64)));
                    }
                    let Some(v) = num(arg) else {
                        self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                        return Some(None);
                    };
                    ch.bias = v;
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
        if kw(n0, "MEASure") && nodes.len() == 3 && kw(nodes[1], "BURSt") && kw(nodes[2], "IDLE") {
            if query {
                return t(&e(ui.m.meas_idle));
            }
            match arg.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0) {
                Some(v) => {
                    ui.m.meas_idle = v;
                    ui.measure_last();
                    ui.mark_changed();
                }
                None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
            }
            return None;
        }
        if kw(n0, "MEASure") && nodes.len() == 3 && kw(nodes[1], "STATistic") {
            return self.meas_stat_cmd(ui, nodes[2], query, arg);
        }
        if kw(n0, "MEASure") && nodes.len() == 3 && kw(nodes[1], "SETup") {
            let which = nodes[2];
            if kw(which, "MAX") || kw(which, "MIN") {
                return self.meas_threshold(ui, kw(which, "MAX"), query, arg);
            }
            if kw(which, "PSA") || kw(which, "DSA") || kw(which, "PSB") || kw(which, "DSB") {
                let second = kw(which, "PSB") || kw(which, "DSB");
                if query {
                    return t(&wave_tok(if second { ui.meas_source_b() } else { ui.meas_source() }));
                }
                let Some(id) = self.named_source(arg) else { return None };
                if second {
                    ui.set_meas_source_b(id);
                } else {
                    ui.set_meas_source(id);
                }
                ui.mark_changed();
                return None;
            }
        }
        if kw(n0, "TIMebase") {
            return self.timebase_more(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "MEASure") && nodes.len() == 2 {
            let n1 = nodes[1];
            if kw(n1, "GATing") {
                if query { return t(if ui.m.meas_full_record { "NONE" } else { "SCREEN" }); }
                let full_record = if kw(arg.trim(), "SCReen") { false }
                    else if kw(arg.trim(), "NONE") { true }
                    else {
                        self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                        return None;
                    };
                ui.set_measurement_full_record(full_record);
                return None;
            }
            if kw(n1, "SOURce") {
                if query {
                    return t(&wave_tok(ui.meas_source()));
                }
                let words: Vec<_> = arg.split(',').map(str::trim).collect();
                if words.is_empty() || words.len() > 2 || words.iter().any(|word| word.is_empty()) {
                    self.push_err("-224,\"Illegal parameter value\"".into());
                    return None;
                }
                let Some(a) = self.named_source(words[0]) else { return None };
                let b = if words.len() == 2 {
                    let Some(b) = self.named_source(words[1]) else { return None };
                    Some(b)
                } else { None };
                ui.set_meas_source(a);
                if let Some(b) = b { ui.set_meas_source_b(b); }
                ui.mark_changed();
                return None;
            }
            if kw(n1, "CLEar") && !query {
                ui.meas_clear_remote();
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
            if kw(n1, "INDicator") {
                return self.meas_indicator(ui, query, arg);
            }
            if kw(n1, "OUTSide") || kw(n1, "NPERiods") || kw(n1, "EDGes") {
                return self.meas_gate(ui, n1, query, arg);
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
                let format = if kw(arg.trim(), "BYTE") { Some(0) } else if kw(arg.trim(), "WORD") { Some(1) }
                    else if kw(arg.trim(), "ASCii") { Some(2) } else { None };
                if let Some(format) = format { self.wav_fmt = format; }
                else { self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)); }
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
                return self.wave_data(ui);
            }
        }
        if kw(n0, "REFerence") {
            return self.ref_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "SIM") {
            return self.sim_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "DISPlay") && !(nodes.len() == 2 && kw(nodes[1], "DATA")) {
            return self.display_cmd(ui, &nodes[1..], query, arg);
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
            if kw(nodes[1], "STARt") && !query {
                let mut it = arg.split(',');
                let ch = it.next().unwrap_or("0").trim();
                ui.selfcal_remote(ch, it.next().is_some_and(|z| kw(z.trim(), "ZERO")));
                return None;
            }
            if kw(nodes[1], "CANCel") && !query {
                ui.selfcal_remote("cancel", false);
                return None;
            }
        }
        if kw(n0, "BODE") && nodes.len() == 2 {
            if kw(nodes[1], "RUN") && !query {
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
                let optional = ["vfine", "unit", "bias", "skew", "label", "tfine"].iter().any(|s| k.ends_with(s)) || k.starts_with("math_expr") || k == "cat";
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
        if kw(n0, "HISTogram") {
            return self.hist_cmd(ui, &nodes[1..], query, arg);
        }
        if let Some(k) = math_index(n0) {
            return self.math_cmd(ui, k, &nodes[1..], query, arg);
        }
        if let Some(k) = bus_index(n0) {
            return self.bus_cmd(ui, k, &nodes[1..], query, arg);
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
            let before = self.error_serial;
            self.meas_place(ui, &std::iter::once(item).chain(rest).collect::<Vec<_>>().join(","));
            if self.error_serial == before { ui.set_stats_shown(true); }
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
        if n.len() == 1 && kw(n0, "ROLL") {
            // Auto roll: the screen enters roll at 50 ms/div while this is on.
            if query {
                return t(if ui.m.auto_roll { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.m.auto_roll = on;
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
            // MAIN is YT. XY is the screen's XY plot. This screen has no forced roll mode; auto roll is :TIMebase:ROLL.
            // The query names XY while that plot is on, so a controller can read the mode it just set.
            if query {
                return t(if ui.xy_on() { "XY" } else { "MAIN" });
            }
            if kw(arg, "MAIN") {
                ui.xy_set(false);
            } else if arg.trim().eq_ignore_ascii_case("XY") {
                ui.xy_set(true);
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            }
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
        if n.len() == 2 && kw(n0, "XY") && (kw(n[1], "X") || kw(n[1], "Y")) {
            // The plot is CH1 against CH2. Those sources are not selectable.
            let x = kw(n[1], "X");
            if query {
                return t(if x { "CHAN1" } else { "CHAN2" });
            }
            let want = if x { 0 } else { 1 };
            if parse_ch(arg) == Some(want) {
                return None;
            }
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        }
        if n.len() == 2 && kw(n0, "XY") && kw(n[1], "Z") {
            if query {
                return t(&ui.m.z_src.map(chan_token).unwrap_or_else(|| "NONE".into()));
            }
            if kw(arg, "NONE") || kw(arg, "OFF") {
                ui.m.z_src = None;
            } else if let Some(c) = parse_ch(arg) {
                ui.m.z_src = Some(c);
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            }
            ui.mark_changed();
            return None;
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
            if parts.next().is_some() {
                self.push_err("-224,\"Too many measurement sources\"".into());
                return None;
            }
            if let Err(reason) = ui.meas_add(k, src) {
                self.push_err(format!("-223,\"{}\"", reason));
            }
            return None;
        }
        if let Some(k) = pair_kind(item) {
            let Some(a) = source(self, parts.next(), ui.meas_source()) else { return None };
            let Some(b) = source(self, parts.next(), ui.meas_source_b()) else { return None };
            if parts.next().is_some() {
                self.push_err("-224,\"Too many measurement sources\"".into());
                return None;
            }
            if let Err(reason) = ui.meas_add_pair(k, a, b) {
                self.push_err(format!("-224,\"{}\"", reason));
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
            return None;
        }
        if n.len() == 2 && kw(n0, "MEASure") && kw(n[1], "INDicator") {
            return self.meas_indicator(ui, query, arg);
        }
        if n.len() == 2 && kw(n0, "MANual") && kw(n[1], "TYPE") {
            if query {
                return t(if ui.m.cursor == CursorMode::Volt { "AMPL" } else { "TIME" });
            }
            if kw(arg, "TIME") {
                if ui.m.cursor != CursorMode::Xy {
                    ui.m.cursor = CursorMode::Time;
                }
            } else if kw(arg, "AMPLitude") {
                if ui.m.cursor != CursorMode::Xy {
                    ui.m.cursor = CursorMode::Volt;
                }
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            }
            return None;
        }
        if n.len() == 2 && (kw(n0, "MANual") || kw(n0, "TRACk")) && (kw(n[1], "SOURce") || kw(n[1], "SOURce1") || kw(n[1], "SOURce2")) {
            let second = kw(n[1], "SOURce2");
            let track = kw(n0, "TRACk");
            if query { return t(&wave_tok(if second && track { ui.cursor_b_source() } else { ui.cursor_source() })); }
            let Some(source) = parse_wave(arg) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)); return None;
            };
            let ok = if second && track { ui.set_cursor_b_source(source) }
                else if second { source == ui.cursor_source() }
                else { ui.set_cursor_source(source) };
            if !ok { self.push_err("-224,\"Cursor source is unavailable or unsupported\"".into()); }
            return None;
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
                let source = if !a && ui.m.cursor == CursorMode::Track { ui.cursor_b_source() } else { ui.cursor_source() };
                ui.wave_at_source(source, if a { ta } else { tb }).map(|v| v as f64).unwrap_or(f64::NAN)
            } else { f64::NAN }
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
            if ui.m.cursor == CursorMode::Track && ui.cursor_source_unit(ui.cursor_source()) != ui.cursor_source_unit(ui.cursor_b_source()) {
                return miss();
            }
            let d = y_of(false) - y_of(true);
            return t(&shown(d));
        }
        self.push_err(format!("-113,\"Undefined header\" :CURSor:{}", n.join(":")));
        None
    }

    /// Horizontal (`x`) or vertical cursor. `a` is cursor A.
    fn cursor_axis(&mut self, ui: &mut Ui, query: bool, arg: &str, x: bool, a: bool) -> Option<Vec<u8>> {
        let place = |ui: &Ui, x: bool, a: bool| -> Option<f32> {
            Some(match (x, ui.m.cursor) {
                (true, CursorMode::Volt | CursorMode::Off) => return None,
                (false, CursorMode::Time | CursorMode::Track | CursorMode::Off) => return None,
                (false, CursorMode::Xy) => if a { ui.m.cur_c } else { ui.m.cur_d },
                (false, CursorMode::Volt) => if a { ui.m.cur_a } else { ui.m.cur_b },
                (true, _) => if a { ui.m.cur_a } else { ui.m.cur_b },
            })
        };
        if query {
            let Some(frac) = place(ui, x, a) else { return Some(b"9.9E37".to_vec()) };
            let v = if x { ui.cursor_time_at(frac) } else { ui.cursor_volt_at(frac) };
            return Some(e(v).into_bytes());
        }
        let Some(v) = arg.trim().parse::<f64>().ok().filter(|v| v.is_finite()) else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        };
        let frac = if x { ui.cursor_frac_time(v) } else { ui.cursor_frac_volt(v) };
        match (x, ui.m.cursor) {
            (true, CursorMode::Volt) => {
                ui.m.cur_c = ui.m.cur_a;
                ui.m.cur_d = ui.m.cur_b;
                if a { ui.m.cur_a = frac; } else { ui.m.cur_b = frac; }
                ui.m.cursor = CursorMode::Xy;
            }
            (false, CursorMode::Time | CursorMode::Track) => {
                if a { ui.m.cur_c = frac; } else { ui.m.cur_d = frac; }
                ui.m.cursor = CursorMode::Xy;
            }
            (false, CursorMode::Xy) => {
                if a { ui.m.cur_c = frac; } else { ui.m.cur_d = frac; }
            }
            (true, CursorMode::Off) => {
                if a { ui.m.cur_a = frac; } else { ui.m.cur_b = frac; }
                ui.m.cursor = CursorMode::Time;
            }
            (false, CursorMode::Off) => {
                if a { ui.m.cur_a = frac; } else { ui.m.cur_b = frac; }
                ui.m.cursor = CursorMode::Volt;
            }
            (true, _) => {
                if a { ui.m.cur_a = frac; } else { ui.m.cur_b = frac; }
            }
            (false, _) => {
                if a { ui.m.cur_a = frac; } else { ui.m.cur_b = frac; }
            }
        }
        None
    }

    /// Waveform drawing, persistence, brightness, and the graticule. Persistence snaps to the times the screen actually keeps.
    fn display_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "TYPE") {
            if query {
                return t(if ui.m.dots { "DOTS" } else { "VECT" });
            }
            ui.m.dots = if kw(arg, "VECTors") {
                false
            } else if kw(arg, "DOTS") {
                true
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            ui.mark_changed();
            return None;
        }
        if n.len() == 2 && kw(n0, "GRADing") && kw(n[1], "TIME") {
            if query {
                return t(persist_token(ui.m.persist));
            }
            let Some(i) = persist_index(arg) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            ui.m.persist = i;
            ui.mark_changed();
            return None;
        }
        if n.len() == 1 && kw(n0, "WBRightness") {
            if query {
                return t(&ui.m.intensity.to_string());
            }
            let Some(v) = arg.trim().parse::<u32>().ok().filter(|v| (1..=100).contains(v)) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            ui.m.intensity = v.clamp(10, 100);
            ui.mark_changed();
            return None;
        }
        if n.len() == 1 && kw(n0, "GRID") {
            if query {
                return t(match ui.m.grid {
                    Grid::Full => "FULL",
                    Grid::Light => "HALF",
                    Grid::None => "NONE",
                });
            }
            ui.m.grid = if kw(arg, "FULL") {
                Grid::Full
            } else if kw(arg, "HALF") {
                Grid::Light
            } else if kw(arg, "NONE") {
                Grid::None
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            ui.mark_changed();
            return None;
        }
        if n.len() == 1 && kw(n0, "GBRightness") {
            if query {
                return t(&ui.m.grid_bright.to_string());
            }
            let Some(v) = arg.trim().parse::<u32>().ok().filter(|v| (0..=100).contains(v)) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            // The screen's grid brightness starts at 10, the same floor as the waveform brightness.
            ui.m.grid_bright = v.clamp(10, 100);
            ui.mark_changed();
            return None;
        }
        if n.len() == 1 && kw(n0, "RULers") {
            if query {
                return t(if ui.m.grid_frame { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.m.grid_frame = on;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "COLor") {
            if query {
                return t(if ui.m.color_grade { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.m.color_grade = on;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        self.push_err(format!("-113,\"Undefined header\" :DISPlay:{}", n.join(":")));
        None
    }

    /// Burst idle, the side Time Outside counts, how many periods Duration N spans, and which edges it uses.
    /// The DHO programmer manual has no header for these Measure-menu settings.
    fn meas_gate(&mut self, ui: &mut Ui, which: &str, query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        if kw(which, "OUTSide") {
            if query {
                return t(match ui.m.meas_out {
                    oos_dsp::measure::Outside::Above => "ABOV",
                    oos_dsp::measure::Outside::Below => "BEL",
                    oos_dsp::measure::Outside::Both => "BOTH",
                });
            }
            ui.m.meas_out = if kw(arg, "ABOVe") {
                oos_dsp::measure::Outside::Above
            } else if kw(arg, "BELow") {
                oos_dsp::measure::Outside::Below
            } else if kw(arg, "BOTH") {
                oos_dsp::measure::Outside::Both
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
        } else if kw(which, "NPERiods") {
            if query {
                return t(&ui.m.meas_n.to_string());
            }
            let Some(n) = arg.trim().parse::<u32>().ok().filter(|n| (1..=32).contains(n)) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            ui.m.meas_n = n;
        } else if kw(which, "EDGes") {
            if query {
                return t(if ui.m.meas_data { "DATA" } else { "CLOC" });
            }
            if kw(arg, "CLOCk") {
                ui.m.meas_data = false;
            } else if kw(arg, "DATA") {
                ui.m.meas_data = true;
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            }
        } else {
            self.push_err(format!("-113,\"Undefined header\" :MEASure:{}", which));
            return None;
        }
        ui.measure_last();
        ui.mark_changed();
        None
    }

    /// The labels the screen draws on the trace for the measurements. Both Rigol headers drive that one switch.
    fn meas_indicator(&mut self, ui: &mut Ui, query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        if query {
            return t(if ui.m.meas_labels { "1" } else { "0" });
        }
        match parse_bool(arg) {
            Some(on) => {
                ui.m.meas_labels = on;
                ui.mark_changed();
                None
            }
            None => {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                None
            }
        }
    }

    /// One of the four decoders. RS232 is the screen's UART. The event table is one window for every bus.
    fn bus_cmd(&mut self, ui: &mut Ui, k: usize, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        let cfg = ui.m.dec_slot(k);
        if n.len() == 1 && kw(n0, "MODE") {
            if query {
                return t(&proto_token(cfg.proto));
            }
            let Some(p) = parse_proto(arg) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            with_bus(ui, k, |m| m.dec_proto = p);
            return None;
        }
        if n.len() == 1 && kw(n0, "DISPlay") {
            if query {
                return t(if cfg.on { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    with_bus(ui, k, |m| m.dec_on = on);
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "FORMat") {
            if query {
                return t(if cfg.ascii { "ASC" } else { "HEX" });
            }
            let ascii = if kw(arg, "HEX") {
                Some(false)
            } else if kw(arg, "ASCii") {
                Some(true)
            } else {
                None
            };
            let Some(ascii) = ascii else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            with_bus(ui, k, |m| m.dec_ascii = ascii);
            return None;
        }
        if n.len() == 1 && kw(n0, "EVENt") {
            if query {
                return t(if ui.decode_table_on() { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.set_decode_table(on);
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "THReshold") {
            if query {
                return t(&e(cfg.thr as f64));
            }
            let num = arg.split(',').next_back().unwrap_or("").trim();
            let Some(v) = num.parse::<f64>().ok().filter(|v| v.is_finite()) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            with_bus(ui, k, |m| m.dec_thr = v as f32);
            return None;
        }
        if n.len() == 1 && kw(n0, "DATA") && query {
            let events = match ui.decode_events_result() {
                Ok(events) => events,
                Err("Decode computing") => return t("BUSY"),
                Err(error) => { self.push_err(format!("-230,\"{}\"", error)); return None; }
            };
            let prefix = format!("D{} ", k + 1);
            let rows = events.lines().filter(|line| line.split_once(',').is_some_and(|(_, rest)| rest.starts_with(&prefix))).collect::<Vec<_>>().join("\n");
            return Some(block(format!("{}\n{}", proto_token(cfg.proto), rows).as_bytes()));
        }
        if n.len() == 2 && kw(n0, "RS232") {
            return self.bus_rs232(ui, k, n[1], query, arg);
        }
        if n.len() == 3 && kw(n0, "IIC") && kw(n[2], "SOURce") && (kw(n[1], "SCLK") || kw(n[1], "SDA")) {
            let clock = kw(n[1], "SCLK");
            if query { return t(&bus_source_name(if clock { cfg.src2 } else { cfg.src })); }
            let Some(source) = parse_bus_wave(ui, arg) else {
                self.push_err(format!("-224,\"Decoder source is unavailable\" {}", arg)); return None;
            };
            if !put_bus_wave(ui, k, Proto::I2c, usize::from(clock), Some(source)) {
                self.push_err("-224,\"Decoder source is unavailable\"".into());
            }
            return None;
        }
        if n.len() == 3 && kw(n0, "SPI") && kw(n[2], "SOURce") && (kw(n[1], "SCLK") || kw(n[1], "MOSI") || kw(n[1], "SS")) {
            let role = if kw(n[1], "SCLK") { 1 } else if kw(n[1], "MOSI") { 0 } else { 2 };
            if query { return t(&match role { 0 => bus_source_name(cfg.src), 1 => bus_source_name(cfg.src2),
                _ => cfg.cs.map(bus_source_name).unwrap_or_else(|| "OFF".into()) }); }
            let source = if role == 2 && (kw(arg, "OFF") || kw(arg, "NONE")) { None }
                else { match parse_bus_wave(ui, arg) { Some(source) => Some(source), None => {
                    self.push_err(format!("-224,\"Decoder source is unavailable\" {}", arg)); return None;
                } } };
            if !put_bus_wave(ui, k, Proto::Spi, role, source) { self.push_err("-224,\"Decoder source is unavailable\"".into()); }
            return None;
        }
        if n.len() == 2 && kw(n0, "SPI") && kw(n[1], "DBITs") {
            if query {
                return t(&format!("{}", cfg.bits));
            }
            let Some(v) = arg.trim().parse::<u32>().ok().filter(|v| (4..=32).contains(v)) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            with_bus(ui, k, |m| {
                m.dec_proto = Proto::Spi;
                m.dec_bits = v;
            });
            return None;
        }
        self.push_err(format!("-113,\"Undefined header\" :BUS{}:{}", k + 1, n.join(":")));
        None
    }

    fn bus_rs232(&mut self, ui: &mut Ui, k: usize, which: &str, query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let cfg = ui.m.dec_slot(k);
        if kw(which, "TX") || kw(which, "RX") {
            let role = usize::from(kw(which, "RX"));
            if query { return t(&bus_source_name(if role == 1 { cfg.src2 } else { cfg.src })); }
            let Some(source) = parse_bus_wave(ui, arg) else {
                self.push_err(format!("-224,\"Decoder source is unavailable\" {}", arg)); return None;
            };
            if !put_bus_wave(ui, k, Proto::Uart, role, Some(source)) { self.push_err("-224,\"Decoder source is unavailable\"".into()); }
            return None;
        }
        if kw(which, "BAUD") {
            if query {
                return t(&format!("{}", crate::decode::BAUDS[cfg.baud.min(crate::decode::BAUDS.len() - 1)] as u64));
            }
            let Some(v) = arg.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            let i = nearest_baud(v);
            with_bus(ui, k, |m| {
                m.dec_proto = Proto::Uart;
                m.dec_baud = i;
            });
            return None;
        }
        if kw(which, "PARity") {
            if query {
                return t(match cfg.parity {
                    oos_dsp::decode::Parity::None => "NONE",
                    oos_dsp::decode::Parity::Odd => "ODD",
                    oos_dsp::decode::Parity::Even => "EVEN",
                });
            }
            let p = if kw(arg, "NONE") {
                Some(oos_dsp::decode::Parity::None)
            } else if kw(arg, "ODD") {
                Some(oos_dsp::decode::Parity::Odd)
            } else if kw(arg, "EVEN") {
                Some(oos_dsp::decode::Parity::Even)
            } else {
                None
            };
            let Some(p) = p else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            with_bus(ui, k, |m| {
                m.dec_proto = Proto::Uart;
                m.dec_parity = p;
            });
            return None;
        }
        if kw(which, "DBITs") {
            if query {
                return t(&format!("{}", cfg.bits));
            }
            let Some(v) = arg.trim().parse::<u32>().ok().filter(|v| (5..=9).contains(v)) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            with_bus(ui, k, |m| {
                m.dec_proto = Proto::Uart;
                m.dec_bits = v;
            });
            return None;
        }
        self.push_err(format!("-113,\"Undefined header\" :BUS{}:RS232:{}", k + 1, which));
        None
    }

    /// One of the eight math traces. Display off is `MathOp::Off`. A math source may be a channel or an earlier math trace.
    fn math_cmd(&mut self, ui: &mut Ui, k: usize, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "DISPlay") {
            if query {
                return t(if ui.m.math_ch(k).op == MathOp::Off { "0" } else { "1" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    let mut c = ui.m.math_ch(k);
                    c.op = if on { if c.op == MathOp::Off { MathOp::Add } else { c.op } } else { MathOp::Off };
                    put_math(ui, k, c);
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "OPERator") {
            if query {
                return t(math_op_name(ui.m.math_ch(k).op));
            }
            let Some(op) = parse_math_op(arg) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            let mut c = ui.m.math_ch(k);
            c.op = op;
            put_math(ui, k, c);
            return None;
        }
        if n.len() == 1 && (kw(n0, "SOURce1") || kw(n0, "SOURce2")) {
            let second = kw(n0, "SOURce2");
            if query {
                let c = ui.m.math_ch(k);
                return t(&math_src_name(if second { c.b } else { c.a }));
            }
            let Some(src) = parse_math_src(arg, k) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            if src >= MATH_REF_BASE && ui.ref_info(src - MATH_REF_BASE).is_none() {
                self.push_err("-230,\"Math reference source is empty\"".into()); return None;
            }
            let mut c = ui.m.math_ch(k);
            if second { c.b = src; } else { c.a = src; }
            put_math(ui, k, c);
            return None;
        }
        if n.len() == 1 && kw(n0, "SCALe") {
            let c = ui.m.math_ch(k);
            if query {
                let v = if c.op == MathOp::Fft { FFT_DB[c.scale.min(FFT_DB.len() - 1)] as f64 } else { math_scales()[c.scale.min(math_scales().len() - 1)] };
                return t(&e(v));
            }
            let Some(v) = arg.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            let mut c = ui.m.math_ch(k);
            c.scale = if c.op == MathOp::Fft {
                nearest_step(&FFT_DB.map(|x| x as f64), v)
            } else {
                nearest_step(&math_scales(), v)
            };
            put_math(ui, k, c);
            return None;
        }
        if n.len() == 1 && kw(n0, "OFFSet") {
            if query {
                return t(&e(ui.m.math_ch(k).offset as f64));
            }
            let Some(v) = arg.trim().parse::<f64>().ok().filter(|v| v.is_finite()) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            let mut c = ui.m.math_ch(k);
            c.offset = v.clamp(-1.0e9, 1.0e9) as f32;
            put_math(ui, k, c);
            return None;
        }
        if n.len() == 1 && kw(n0, "EXPRession") {
            if query {
                return t(&ui.m.math_expr[k]);
            }
            let text = unquote(arg);
            if crate::math::parse_expr(text).is_err() {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            }
            ui.m.math_expr[k] = text.to_string();
            let mut c = ui.m.math_ch(k);
            c.op = MathOp::Expr;
            put_math(ui, k, c);
            return None;
        }
        if n.len() == 2 && kw(n0, "FFT") && kw(n[1], "SOURce") {
            if query {
                return t(&math_src_name(ui.m.math_ch(k).a));
            }
            let Some(src) = parse_math_src(arg, k) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)); return None;
            };
            if src >= MATH_REF_BASE && ui.ref_info(src - MATH_REF_BASE).is_none() {
                self.push_err("-230,\"Math reference source is empty\"".into()); return None;
            }
            let mut c = ui.m.math_ch(k);
            c.a = src;
            c.op = MathOp::Fft;
            put_math(ui, k, c);
            return None;
        }
        if n.len() == 2 && kw(n0, "FFT") && kw(n[1], "WINDow") {
            if query {
                return t(match ui.m.math_ch(k).win {
                    Window::Rect => "RECT",
                    Window::Blackman => "BLAC",
                    Window::Hann => "HANN",
                    Window::Hamming => "HAMM",
                    Window::Flattop => "FLAT",
                });
            }
            let win = if kw(arg, "RECTangle") {
                Window::Rect
            } else if kw(arg, "BLACkman") {
                Window::Blackman
            } else if kw(arg, "HANNing") {
                Window::Hann
            } else if kw(arg, "HAMMing") {
                Window::Hamming
            } else if kw(arg, "FLATtop") {
                Window::Flattop
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            let mut c = ui.m.math_ch(k);
            c.win = win;
            put_math(ui, k, c);
            return None;
        }
        if n.len() == 2 && kw(n0, "FFT") && kw(n[1], "SCALe") {
            if query {
                let c = ui.m.math_ch(k);
                return t(&e(FFT_DB[c.scale.min(FFT_DB.len() - 1)] as f64));
            }
            let Some(v) = arg.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            let mut c = ui.m.math_ch(k);
            c.op = MathOp::Fft;
            c.scale = nearest_step(&FFT_DB.map(|x| x as f64), v);
            put_math(ui, k, c);
            return None;
        }
        if n.len() == 2 && kw(n0, "FILTer") && kw(n[1], "TYPE") {
            if query {
                return t(match ui.m.math_ch(k).op {
                    MathOp::Lpf => "LPAS",
                    MathOp::Hpf => "HPAS",
                    MathOp::Bpf => "BPAS",
                    MathOp::Bsf => "BST",
                    _ => "LPAS",
                });
            }
            let op = if kw(arg, "LPASs") {
                MathOp::Lpf
            } else if kw(arg, "HPASs") {
                MathOp::Hpf
            } else if kw(arg, "BPASs") {
                MathOp::Bpf
            } else if kw(arg, "BSTop") {
                MathOp::Bsf
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            let mut c = ui.m.math_ch(k);
            c.op = op;
            put_math(ui, k, c);
            return None;
        }
        if n.len() == 2 && kw(n0, "FILTer") && (kw(n[1], "W1") || kw(n[1], "W2")) {
            let high = kw(n[1], "W2");
            let c = ui.m.math_ch(k);
            let band = matches!(c.op, MathOp::Bpf | MathOp::Bsf);
            if !c.op.has_fc() || (high && !band) {
                if query {
                    return t("9.9E37");
                }
                self.push_err("-221,\"Settings conflict\"".into());
                return None;
            }
            let (lo, hi) = if band { (c.fc - c.bw / 2.0, c.fc + c.bw / 2.0) } else { (c.fc, c.fc) };
            if query {
                return t(&e(if high { hi } else { lo } as f64));
            }
            let Some(v) = arg.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            let mut c = c;
            if band {
                let (lo, hi) = if high { (lo, v as f32) } else { (v as f32, hi) };
                if lo >= hi {
                    self.push_err("-221,\"Settings conflict\"".into());
                    return None;
                }
                c.fc = (lo + hi) / 2.0;
                c.bw = hi - lo;
            } else {
                c.fc = v as f32;
            }
            put_math(ui, k, c);
            return None;
        }
        self.push_err(format!("-113,\"Undefined header\" :MATH{}:{}", k + 1, n.join(":")));
        None
    }

    /// Vertical or horizontal histogram of one live channel. The source is the measurement source, which is what the screen uses.
    fn hist_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "ENABle") {
            if query {
                return t(if ui.m.hist == Hist::Off { "0" } else { "1" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    if on && ui.m.hist == Hist::Off {
                        ui.m.hist = Hist::Vertical;
                    } else if !on {
                        ui.m.hist = Hist::Off;
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
        if n.len() == 1 && kw(n0, "TYPE") {
            if query {
                return t(if ui.m.hist == Hist::Horizontal { "HOR" } else { "VERT" });
            }
            ui.m.hist = if kw(arg, "HORizontal") {
                Hist::Horizontal
            } else if kw(arg, "VERTical") {
                Hist::Vertical
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            ui.mark_changed();
            return None;
        }
        if n.len() == 1 && kw(n0, "SOURce") {
            if query {
                let c = ui.hist_source().unwrap_or(0);
                return t(&format!("CHAN{}", c + 1));
            }
            return match parse_ch(arg) {
                Some(c) => {
                    ui.m.ch[c].on = true;
                    ui.set_meas_source(WaveId::Ch(c));
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 2 && kw(n0, "STATistics") && kw(n[1], "RESult") && query {
            let blank = "Sum:0,Peaks:0,Max:9.9E37,Min:9.9E37,Pk_Pk:9.9E37,Mean:9.9E37,Median:9.9E37,Mode:9.9E37,Bin width:9.9E37,Sigma:9.9E37";
            let Some(s) = ui.hist_stat() else { return t(blank) };
            let num = |v: f64| if v.is_finite() { e(v) } else { "9.9E37".to_string() };
            let pk = if s.max.is_finite() && s.min.is_finite() { e(s.max - s.min) } else { "9.9E37".to_string() };
            return t(&format!(
                "Sum:{},Peaks:{},Max:{},Min:{},Pk_Pk:{},Mean:{},Median:{},Mode:{},Bin width:{},Sigma:{}",
                s.sum, s.peaks, num(s.max), num(s.min), pk, num(s.mean), num(s.median), num(s.mode), num(s.bin), num(s.sigma)
            ));
        }
        self.push_err(format!("-113,\"Undefined header\" :HISTogram:{}", n.join(":")));
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
            return None;
        }
        self.push_err(format!("-113,\"Undefined header\" :COUNter:{}", n.join(":")));
        None
    }

    fn decode_cmd(&mut self, ui: &mut Ui, node: &str, query: bool, arg: &str) -> Option<Vec<u8>> {
        if kw(node, "SRATe") { return self.decode_cphy_rate_cmd(ui, query, arg); }
        let text = |value: String| Some(value.into_bytes());
        let illegal = |s: &mut Self| { s.push_err(format!("-224,\"Illegal decoder parameter\" {}", arg)); None };
        if kw(node, "EVENts") && query {
            return match ui.decode_events_result() {
                Ok(events) => text(events),
                Err("Decode computing") => text("BUSY".into()),
                Err(error) => { self.push_err(format!("-230,\"{}\"", error)); None }
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
        self.push_err(format!("-113,\"Undefined header\" DECode:{}", node));
        None
    }

    fn decode_cphy_rate_cmd(&mut self, ui: &mut Ui, query: bool, arg: &str) -> Option<Vec<u8>> {
        if query { return Some(if ui.m.dec_cphy_rate == 0.0 { b"AUTO".to_vec() } else { e(ui.m.dec_cphy_rate).into_bytes() }); }
        match crate::decode::parse_cphy_rate(arg) {
            Ok(rate) => { ui.m.dec_cphy_rate = rate; ui.mark_changed(); }
            Err(_) => self.push_err(format!("-224,\"Invalid C-PHY symbol rate\" {}", arg)),
        }
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
        format!("{},{},{},1,{},{},0,{},{},{}", self.wav_fmt, self.wav_mode, self.page_count(ui), e(dt), e(t0), e(yinc), yorg, yref)
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
        let bad = |s: &mut Self| {
            s.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
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
                None => return bad(self),
            }
        }
        if n.len() == 2 && kw(k, "DIGital") && kw(n[1], "DISPlay") {
            let Some(line) = idx.filter(|&i| i < 16) else { return bad(self) };
            if query {
                return t((ui.m.la.show >> line & 1).to_string());
            }
            match parse_bool(arg) {
                Some(on) => {
                    ui.m.la.show = ui.m.la.show & !(1 << line) | (on as u16) << line;
                    return None;
                }
                None => return bad(self),
            }
        }
        if n.len() == 2 && kw(k, "POD") && kw(n[1], "THReshold") {
            let Some(pod) = idx.filter(|&i| i == 1 || i == 2).map(|i| i - 1) else { return bad(self) };
            if query {
                return t(format!("{:e}", ui.m.la.thr(pod)));
            }
            match arg.trim().parse::<f32>() {
                Ok(v) if v.abs() <= la::THR_MAX => {
                    (ui.m.la.preset[pod], ui.m.la.user[pod]) = (la::USER, v);
                    ui.mark_changed();
                    return None;
                }
                _ => return bad(self),
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

    fn wave_data(&mut self, ui: &Ui) -> Option<Vec<u8>> {
        // Bound output before taking a copy of the requested page. Metadata never copies samples.
        let width = match self.wav_fmt { 0 => 1, 1 => 2, _ => 14 };
        if self.page_count(ui).saturating_mul(width).saturating_add(11) > RESP_MAX as u64 {
            self.push_err("-223,\"Too much data; select a shorter waveform page\"".into());
            return None;
        }
        let (vpd, offset) = self.wave_scale(ui);
        let (yinc, yorg, yref) = self.y_codes(vpd, offset);
        let (v, _, _, _, _) = self.wave_page(ui);
        Some(match self.wav_fmt {
            2 => {
                use std::fmt::Write as _;
                let mut text = String::with_capacity(v.len().saturating_mul(14));
                for (index, value) in v.iter().enumerate() {
                    if index != 0 { text.push(','); }
                    let _ = write!(text, "{}", e(*value as f64));
                }
                block(text.as_bytes())
            }
            1 => {
                // OOS wire policy: unsigned little-endian WORD, retained for existing clients.
                // The Rigol guide specifies two bytes per point but does not specify their order.
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
        })
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
        for command in [":WAV:MODE WRONG",":WAV:MODE RAWsuffix",":WAV:FORM WRONG",":WAV:FORM WORDsuffix"] {
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
            assert_eq!(line(&mut s,&mut ui,&cv,"*ESR?"),"4", "cancelled query latches query-error bit, not OPC");assert_eq!(ui.m.trig_mode,TrigMode::Single);
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

    #[test]
    fn integration_reference_source_switch_interrupts_old_fence_and_new_single_accepts_once() {
        let (mut scpi, mut ui, cv) = session();
        ui.set_offline(true);
        for slot in 0..2 {
            let values = (0..4000).map(|i| if slot == 0 || i % 40 < 20 { -0.5 } else { 0.5 }).collect();
            ui.commit_waveform_traces_at(vec![crate::store::WaveTrace { v: values, dt: 1e-9, t0: 2e-6, label: format!("R{}", slot + 1) }], "source-change", slot).unwrap();
        }
        let original = [ui.ref_volts(0), ui.ref_volts(1)];
        line(&mut scpi, &mut ui, &cv, ":TRIG:SOUR R1;:TRIG:MODE EDGE;:TRIG:EDGE:LEV 0;:TIM:SCAL 1e-8;:TIM:OFFS 3e-6");
        let mut old = Program::new(":SINGLE;*OPC?;:RUN");
        assert!(scpi.poll_program(&mut ui, &cv, &mut old).is_none());
        ui.step(0.2); assert_eq!(captures(&ui), 0.0);
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
        assert_eq!(captures(&ui), 1.0); assert_eq!(ui.m.trig_ref, Some(1));
        ui.step(0.2); assert_eq!(captures(&ui), 1.0, "the cancelled query's trailing RUN must never execute");
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
            assert_eq!(ui.measurement_statistics_snapshot_for_test(), statistics, "{query} must preserve every statistic bit-for-bit");
            assert_eq!(crate::store::setup_text(&ui.m), configuration, "{query} must not change configuration");
            assert_eq!(ui.workflow_state(), workflow, "{query} must not change acquisition/run state");
            assert_eq!(ui.m.hw_stamp, acquisition_stamp);
            assert_eq!(s.err, errors, "status reads must not consume queued errors");
            assert_eq!(s.clients.len(), 1);
            assert_eq!(s.clients[0].buf, pending);
            assert_eq!((s.wav_src, s.wav_fmt, s.wav_mode), (WaveId::Ref(2), 1, 2));
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
                assert!(line(&mut s, &mut ui, &cv, query).starts_with("-113,"), "{query} must answer Undefined header without acting");
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
        line(&mut s, &mut ui, &cv, ":CURS:MODE MAN;:CURS:MAN:TYPE TIME;:CURS:MAN:SOUR R1;:CURS:MAN:CAX 0;:CURS:MAN:CBX 1e-6");
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

        line(&mut s, &mut ui, &cv, ":DVM:ENAB ON;:DVM:SOUR CHAN2;:DVM:MODE ACRMS;:COUN:ENAB ON;:COUN:MODE PER;:COUN:NDIG 6");
        assert_eq!(line(&mut s, &mut ui, &cv, ":DVM:ENAB?;:DVM:SOUR?;:DVM:MODE?;:COUN:ENAB?;:COUN:MODE?;:COUN:NDIG?"), "1;CHAN2;ACRMS;1;PER;6");
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
        scpi.poll(&mut ui, &canvas);
        let mut byte = [0]; client.read_exact(&mut byte).unwrap();
        assert_eq!(&byte, b"\n");
        client.write_all(b":MEAS:STRIP?;*OPC?\n*ESR?\n").unwrap();
        scpi.poll(&mut ui, &canvas);
        let mut reply = [0; 5]; client.read_exact(&mut reply).unwrap();
        assert_eq!(&reply, b";1\n0\n");
        assert_eq!(scpi.exec_message_checked(&mut ui, &canvas, ":MEAS:STRIP?"), Ok((Vec::new(), true)));
        assert_eq!(scpi.exec_message_checked(&mut ui, &canvas, ":STOP"), Ok((Vec::new(), false)));
    }

    #[test]
    fn cursor_setters_preserve_genuine_statistics_and_pending_single() {
        let (mut scpi, mut ui, canvas) = session();
        for _ in 0..4 { ui.step(0.05); }
        let statistics = ui.measurement_statistics_snapshot_for_test();
        assert!(statistics.iter().any(|&(count, _)| count >= 3));
        let record = ui.record_volts(0).to_vec();
        ui.m.trig_level = 100.0;
        ui.key(crate::ui::key::SINGLE);
        let stamp = ui.m.hw_stamp;
        let source = ui.m.trigger_wave();
        let mut waiting = Program::new("*OPC?;:RUN");
        assert!(scpi.poll_program(&mut ui, &canvas, &mut waiting).is_none());
        for command in [":CURS:MODE MAN", ":CURS:MAN:TYPE TIME", ":CURS:MAN:SOUR CHAN1",
            ":CURS:MAN:CAX 0", ":CURS:MAN:CBX 1e-8", ":CURS:MAN:TYPE AMPL", ":CURS:MAN:CAY 0.2",
            ":CURS:MAN:CBY -0.2", ":CURS:MODE XY", ":CURS:XY:AX 0", ":CURS:XY:AY 0.1",
            ":CURS:XY:BX 1e-8", ":CURS:XY:BY -0.1", ":CURS:MODE TRAC", ":CURS:TRAC:SOUR2 CHAN1", ":CURS:MODE OFF"] {
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
        ui.commit_waveform_traces(vec![crate::store::WaveTrace {
            v: vec![0.0, volts(0x8123) as f32, volts(0x2345) as f32, 0.0],
            dt: 5e-12, t0: -2e-9, label: "Known WORD codes".into(),
        }], "wire-policy.csv", true).unwrap();
        line(&mut scpi, &mut ui, &canvas, ":REF:VSC 1,1;:REF:VOFF 1,0;:WAV:SOUR R1;:WAV:MODE RAW;:WAV:FORM WORD;:WAV:STAR 2;:WAV:STOP 3");
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
        for _ in 0..ERR_MAX { scpi.push_err("-113,\"Old command error\"".into()); }
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
        line(&mut s, &mut ui, &cv, ":CURS:MODE MAN;:CURS:MAN:CAX 0;:CURS:MAN:CBX 1e-6;:CURS:MAN:CAY 0.5;:CURS:MAN:CBY -0.5");
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
        line(&mut s, &mut ui, &cv, "*CLS;:CURS:MODE TRAC;:CURS:TRAC:SOUR1 CHAN1;:CURS:TRAC:SOUR2 CHAN2;:CURS:TRAC:CAX 0;:CURS:TRAC:CBX 1e-6");
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
        assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"), "a measurement source is a channel or a reference");
        assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:SOUR?"), "CHAN2");

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
        line(&mut s, &mut ui, &cv, "*CLS;:MATH1:DISP ON;:MATH1:OPER ADD;:MATH1:SOUR1 CHAN1;:MATH1:SOUR2 CHAN2;:MATH1:SCAL 0.2;:MATH1:OFFS 1");
        assert_eq!(line(&mut s, &mut ui, &cv, ":MATH1:DISP?;:MATH1:OPER?;:MATH1:SOUR1?;:MATH1:SOUR2?"), "1;ADD;CHAN1;CHAN2");
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
        assert_eq!(line(&mut s, &mut ui, &cv, ":BUS1:RS232:TX?;:BUS1:RS232:RX?;:BUS1:RS232:BAUD?;:BUS1:RS232:PAR?;:BUS1:RS232:DBIT?"), "CHAN1;CHAN2;9600;EVEN;8");
        let thr: f64 = line(&mut s, &mut ui, &cv, ":BUS1:THR?").parse().unwrap();
        assert!((thr - 1.25).abs() < 1e-6, "{thr}");
        line(&mut s, &mut ui, &cv, ":BUS2:MODE SPI;:BUS2:DISP ON;:BUS2:SPI:SCLK:SOUR CHAN2;:BUS2:SPI:MOSI:SOUR CHAN1;:BUS2:SPI:SS:SOUR CHAN3;:BUS2:SPI:DBIT 12");
        assert_eq!(line(&mut s, &mut ui, &cv, ":BUS2:MODE?;:BUS2:SPI:SCLK:SOUR?;:BUS2:SPI:MOSI:SOUR?;:BUS2:SPI:SS:SOUR?;:BUS2:SPI:DBIT?"), "SPI;CHAN2;CHAN1;CHAN3;12");
        assert_eq!(line(&mut s, &mut ui, &cv, ":BUS1:MODE?;:BUS1:RS232:BAUD?"), "RS232;9600", "the second bus must not replace the first");
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
        assert_eq!(line(&mut s, &mut ui, &cv, ":TIM:ROLL?;:TIM:MODE?;:TIM:XY:ENAB?;:TIM:XY:X?;:TIM:XY:Y?;:TIM:XY:Z?"), "0;XY;1;CHAN1;CHAN2;CHAN3");
        line(&mut s, &mut ui, &cv, ":TIM:MODE ROLL");
        assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"), "roll is the auto-roll switch, not a timebase mode");
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
        ui.commit_waveform_traces_at(vec![crate::store::WaveTrace { v: vec![0.0,1.0,0.0,1.0],dt:1e-9,t0:2e-6,label:"math R2".into() }],"R2",1).unwrap();
        line(&mut scpi,&mut ui,&canvas,":MATH1:OPER SUBT;:MATH1:SOUR1 R1;:MATH1:SOUR2 R2;:MATH2:OPER ADD;:MATH2:SOUR1 MATH1;:MATH2:SOUR2 R1");
        assert_eq!(line(&mut scpi,&mut ui,&canvas,":MATH1:SOUR1?;:MATH1:SOUR2?;:MATH2:SOUR1?;:MATH2:SOUR2?"),"R1;R2;MATH1;R1");
        assert_eq!((ui.m.math_ch(0).a,ui.m.math_ch(0).b),(MATH_REF_BASE,MATH_REF_BASE+1));
        assert_eq!(ui.m.math_ch(1).a,NCH);
        line(&mut scpi,&mut ui,&canvas,":MATH1:EXPR R1 - R2");
        assert_eq!(ui.m.math_ch(0).op,MathOp::Expr);
        assert_eq!(line(&mut scpi,&mut ui,&canvas,":MATH1:EXPR?"),"R1 - R2");
        let old=ui.m.math_ch(0);
        line(&mut scpi,&mut ui,&canvas,":MATH1:SOUR1 MATH1");assert_eq!(ui.m.math_ch(0),old);
        assert!(line(&mut scpi,&mut ui,&canvas,":SYST:ERR?").starts_with("-224,"));
        line(&mut scpi,&mut ui,&canvas,":MATH1:FFT:SOUR R2");assert_eq!(ui.m.math_ch(0).a,MATH_REF_BASE+1);
        assert_eq!(line(&mut scpi,&mut ui,&canvas,":SYST:ERR?"),"0,\"No error\"");
    }

    #[test]
    fn frozen_bus_scpi_preserves_reference_roles_and_inactive_slot_acquisition() {
        let (mut scpi,mut ui,canvas)=reference_session();
        ui.commit_waveform_traces_at(vec![crate::store::WaveTrace {v:vec![0.,1.,0.,1.],dt:1e-9,t0:0.,label:"clock".into()}],"R2",1).unwrap();
        line(&mut scpi,&mut ui,&canvas,":BUS1:IIC:SDA:SOUR R1;:BUS1:IIC:SCLK:SOUR R2");
        assert_eq!(line(&mut scpi,&mut ui,&canvas,":BUS1:IIC:SDA:SOUR?;:BUS1:IIC:SCLK:SOUR?"),"R1;R2");
        assert_eq!((ui.m.dec_src,ui.m.dec_src2),(DEC_REF_BASE,DEC_REF_BASE+1));
        line(&mut scpi,&mut ui,&canvas,":TRIG:EDGE:LEV 100;:SINGLE");
        let before=ui.workflow_state();let source=ui.m.trigger_wave();let mut pending=Program::new("*OPC?");
        assert!(scpi.poll_program(&mut ui,&canvas,&mut pending).is_none());
        line(&mut scpi,&mut ui,&canvas,":BUS2:SPI:MOSI:SOUR R2;:BUS2:SPI:SCLK:SOUR R1;:BUS2:SPI:SS:SOUR OFF");
        assert_eq!(line(&mut scpi,&mut ui,&canvas,":BUS2:SPI:MOSI:SOUR?;:BUS2:SPI:SCLK:SOUR?;:BUS2:SPI:SS:SOUR?"),"R2;R1;OFF");
        assert_eq!(ui.m.dec_sel,0);assert_eq!(ui.m.trigger_wave(),source);assert_eq!(ui.workflow_state(),before);
        assert!(scpi.poll_program(&mut ui,&canvas,&mut pending).is_none(),"inactive decoder configuration must not rearm the accepted acquisition context");
        let old=ui.m.dec_slot(1);
        line(&mut scpi,&mut ui,&canvas,":BUS2:SPI:MOSI:SOUR R10");
        assert_eq!(ui.m.dec_slot(1).src,old.src);
        assert!(line(&mut scpi,&mut ui,&canvas,":SYST:ERR?").starts_with("-224,"));
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
