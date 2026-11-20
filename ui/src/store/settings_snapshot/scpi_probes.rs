// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The setters of docs/scpi.md that have a query form, and the values that probe them: the choices, the edges of a
//! range and values past them, from the reference and the handlers in `scpi.rs`. Each probe runs on a fresh session;
//! the few setters that need a loaded reference or a mask that is on get it first (`prepare`).

use super::{show, unique};
use crate::ui::testkit::{q, scpi_session};

/// Every setter also gets these: no argument, garbage, negative, huge.
const EVERY: [&str; 4] = ["", "x", "-1", "1e9"];
const BOOL: &[&str] = &["ON", "OFF", "1", "0", "on"];
const SOURCES: &[&str] = &["CHAN1", "CHAN2", "CHAN4", "CHAN5", "R1", "R10", "R11"];
const ROLES: &[&str] = &["CHAN1", "CHAN2", "R1", "R11", "NONE", "OFF"];
const TRIGGER_TYPES: &[&str] = &[
    "EDGE",
    "PULSE",
    "SLOPE",
    "VIDEO",
    "PATTERN",
    "DURATION",
    "TIMEOUT",
    "RUNT",
    "WINDOW",
    "DELAY",
    "SETUPHOLD",
    "NTHEDGE",
    "BUS",
    "edge",
];
const MATH_OPS: &[&str] = &[
    "ADD", "SUBTract", "MULTiply", "DIVision", "AND", "OR", "XOR", "NOT", "FFT", "INTG", "DIFF", "SQRT", "LG", "LN",
    "EXP", "ABS", "LPASs", "HPASs", "BPASs", "BSTop", "AXB", "NAND", "NOR", "CONV", "FIR", "EXPR", "A2D", "D2A",
    "UNNRZI", "OFF", "SUBT",
];
const CURSOR_X: &[&str] = &["1e-4", "0", "1", "-1e-3"];
const CURSOR_Y: &[&str] = &["0.5", "-10", "10"];
const FRACTION: &[&str] = &["0.2", "0", "1", "-0.5", "1.5"];

/// The setters (first instance of `<n>`), each with its values.
const SETTERS: &[(&str, &[&str])] = &[
    (":CHANnel1:DISPlay", BOOL),
    (":CHANnel1:SCALe", &["0.5", "0.0001", "10", "20", "0.00005", "0.3"]),
    (":CHANnel1:OFFSet", &["0.25", "-100", "100", "1e39"]),
    (":CHANnel1:COUPling", &["DC", "AC", "GND", "DC50"]),
    (":CHANnel1:BWLimit", &["OFF", "20M", "AUTO", "100M", "\"20 MHz\""]),
    (":CHANnel1:PROBe", &["10", "0.001", "1000", "0", "0.5"]),
    (":CHANnel1:INVert", BOOL),
    (":CHANnel1:POSition", &["1", "-5", "5", "50"]),
    (":CHANnel1:UNITs", &["VOLTage", "AMPere", "WATT", "UNKNown", "VOLT"]),
    (":CHANnel1:VERNier", BOOL),
    (":CHANnel1:TCALibrate", &["1e-9", "1e-7", "-1e-7", "2e-7", "1e-3"]),
    (":CHANnel1:BIAS", &["0.1", "-0.1", "100"]),
    (":CHANnel1:EXPand", &["GND", "CENTer"]),
    (":CHANnel1:LABel:SHOW", BOOL),
    (":CHANnel1:LABel:CONTent", &["\"probe\"", "probe", "\"0123456789AB\""]),
    (":TIMebase:MAIN:SCALe", &["0.001", "5e-11", "1000", "1e4", "3e-4"]),
    (":TIMebase:MAIN:OFFSet", &["0.001", "-1", "1000"]),
    (":TIMebase:DELay:ENABle", BOOL),
    (":TIMebase:DELay:SCALe", &["1e-6", "5e-11", "1", "1e-3"]),
    (":TIMebase:DELay:OFFSet", &["1e-5", "-1", "1"]),
    (":TIMebase:VERNier", BOOL),
    (":TIMebase:HREFerence:MODE", &["CENTer", "LB", "RB", "TRIGger", "LEFT", "RIGHt"]),
    (":TIMebase:MODE", &["MAIN", "XY", "ROLL"]),
    (":TIMebase:ROLL", BOOL),
    (":TIMebase:XY:ENABle", BOOL),
    (":TIMebase:XY:X", &["CHAN1", "CHAN2"]),
    (":TIMebase:XY:Y", &["CHAN2", "CHAN1"]),
    (":TIMebase:XY:Z", &["NONE", "OFF", "CHAN3", "CHAN4", "CHAN5"]),
    (":ACQuire:MDEPth", &["AUTO", "1k", "1000", "10k", "1M", "1000000", "125M", "200M", "7"]),
    (":ACQuire:TYPE", &["NORMal", "PEAK", "AVERages", "HRESolution", "HRES"]),
    (":ACQuire:AVERages", &["16", "2", "1", "3", "1024", "65536"]),
    (":ACQuire:ULTRa:MAXFrame", &["10", "1", "100000", "100001", "0"]),
    (":ACQuire:CORes", &["MATChed", "RAW", "MATC", "ON", "OFF", "1", "0"]),
    (":ACQuire:AVERages:HARDware", BOOL),
    (":ACQuire:SEGMents", &["10", "1", "100000", "100001", "OFF", "ON", "0"]),
    (":ACQuire:SEGMents:HARDware", BOOL),
    (":ACQuire:SEGMents:INTerval", &["1e-3", "0", "10", "11"]),
    (":TRIGger:MODE", TRIGGER_TYPES),
    (":TRIGger:TYPe", TRIGGER_TYPES),
    (":TRIGger:SWEep", &["AUTO", "NORMal", "SINGle"]),
    (":TRIGger:EDGE:SOURce", &["CHAN1", "CHAN2", "CHAN4", "CHAN5", "R1", "R10", "R11", "D0", "D15", "D16"]),
    (":TRIGger:EDGE:LEVel", &["0.5", "-100", "100"]),
    (":TRIGger:EDGE:SLOPe", &["POSitive", "NEGative", "RFALl", "POS", "NEG", "RFAL"]),
    (":TRIGger:HOLDoff", &["1e-6", "0", "10", "11"]),
    (":TRIGger:NREJect", BOOL),
    (":TRIGger:COUPling", &["DC", "AC", "LFReject", "HFReject", "LFR", "HFR"]),
    (":TRIGger:RUNT:UPPer", &["2", "0.4", "-1"]),
    (":TRIGger:RUNT:LOWer", &["0.2", "1", "3"]),
    (":TRIGger:SOURce2", SOURCES),
    (":WAVeform:SOURce", &["CHAN1", "CHAN2", "R1", "R10", "MATH1"]),
    (":WAVeform:MODE", &["NORMal", "MAXimum", "RAW"]),
    (":WAVeform:FORMat", &["BYTE", "WORD", "ASCii"]),
    (":WAVeform:STARt", &["1", "0", "100"]),
    (":WAVeform:STOP", &["1", "100", "0"]),
    (":WAVeform:POINts", &["100", "1", "0"]),
    (":MEASure:SOURce", &["CHAN1", "CHAN2", "R1", "CHAN1,CHAN2", "CHAN5"]),
    (":MEASure:GATing", &["SCREEN", "NONE"]),
    (":MEASure:REFS", &["10/90", "20/80", "30/70"]),
    (":MEASure:TSETup", &["1e-9", "0", "-1e-9"]),
    (":MEASure:THOLd", &["1e-9", "0", "-1e-9"]),
    (":MEASure:STATistic:DISPlay", BOOL),
    (":MEASure:SETup:MIN", &["10", "0", "100", "50"]),
    (":MEASure:SETup:MAX", &["90", "0", "100", "50"]),
    (":MEASure:SETup:PSA", SOURCES),
    (":MEASure:SETup:PSB", SOURCES),
    (":MEASure:SETup:DSA", SOURCES),
    (":MEASure:SETup:DSB", SOURCES),
    (":MEASure:BURSt:IDLE", &["1e-6", "0"]),
    (":MEASure:OUTSide", &["ABOVe", "BELow", "BOTH"]),
    (":MEASure:NPERiods", &["4", "1", "32", "33", "0"]),
    (":MEASure:EDGes", &["CLOCk", "DATA"]),
    (":MEASure:INDicator", BOOL),
    (":CURSor:MEASure:INDicator", BOOL),
    (":VIEW", &["SCOPE", "SPECTRUM", "EYE", "POWER", "BODE", "MAGNETIC", "INVERTER", "DPT"]),
    (":BODE:MODE", &["LOOP", "PSRR", "IMPedance"]),
    (":BODE:GENerator", &["\"10.1.2.3:5555\"", "10.1.2.3:5555", "\"\""]),
    (":CURSor:MODE", &["OFF", "MANual", "TRACk", "XY"]),
    (":CURSor:MANual:TYPE", &["TIME", "AMPLitude"]),
    (":CURSor:MANual:SOURce", SOURCES),
    (":CURSor:MANual:CAX", CURSOR_X),
    (":CURSor:MANual:CBX", CURSOR_X),
    (":CURSor:MANual:CAY", CURSOR_Y),
    (":CURSor:MANual:CBY", CURSOR_Y),
    (":CURSor:TRACk:SOURce", SOURCES),
    (":CURSor:TRACk:SOURce1", SOURCES),
    (":CURSor:TRACk:SOURce2", SOURCES),
    (":CURSor:TRACk:CAX", CURSOR_X),
    (":CURSor:TRACk:CBX", CURSOR_X),
    (":CURSor:TRACk:CAY", CURSOR_Y),
    (":CURSor:TRACk:CBY", CURSOR_Y),
    (":CURSor:XY:AX", FRACTION),
    (":CURSor:XY:BX", FRACTION),
    (":CURSor:XY:AY", FRACTION),
    (":CURSor:XY:BY", FRACTION),
    (":DVM:ENABle", BOOL),
    (":DVM:SOURce", SOURCES),
    (":DVM:MODE", &["DC", "ACRMS", "ACDC"]),
    (":COUNter:ENABle", BOOL),
    (":COUNter:SOURce", SOURCES),
    (":COUNter:MODE", &["FREQuency", "PERiod", "TOTalize"]),
    (":COUNter:NDIGits", &["3", "7", "2", "8", "5"]),
    (":REFerence:SLOT", &["R1", "R10", "R11", "1", "10"]),
    (":REFerence:SHOW", &["ON", "OFF", "1,ON", "10,OFF", "11,ON"]),
    (":REFerence:CURRent", &["1", "10", "11", "0", "R2"]),
    (":REFerence:SOURce", &["CHAN1", "CHAN2", "2,CHAN3", "R1"]),
    (":REFerence:VSCale", &["0.5", "1,0.5", "1e-6"]),
    (":REFerence:VOFFset", &["0.5", "1,0.5"]),
    (":REFerence:LABel:CONTent", &["\"ref\"", "1,\"ref\""]),
    (":REFerence:COLor", &["RED", "GREen", "BLUE", "ORANge", "GRAY", "WHITe", "VIOLet", "1,RED"]),
    (":SIM:PRESet", &["\"PCIe 3.0\"", "PCIe 3.0", "\"10GBASE-KR\"", "\"SATA III\""]),
    (":SIM:SLOT", &["R1", "R10", "R11"]),
    (":SIM:PATTern", &["PRBS7", "PRBS9", "PRBS13", "PRBS15", "PRBS23", "PRBS31", "PRBS8", "7"]),
    (":SIM:SHOW", BOOL),
    (":SIM:SSC:MODE", &["OFF", "DOWN", "CENTER", "CENT"]),
    (":SIM:SSC:DEViation", &["0.01", "0", "0.05", "0.051"]),
    (":SIM:SSC:FREQuency", &["33000", "1000", "1e6", "999", "1000001"]),
    (":SIM:JITTer:DCD", &["1e-11", "0", "100e-12", "1.1e-10"]),
    (":SIM:JITTer:CORRelated", &["1e-11", "0", "50e-12", "5.1e-11"]),
    (":SIM:JITTer:CORNer", &["1e6", "1e3", "100e6", "999", "1.1e8"]),
    (":DECode:SELect", &["1", "4", "5", "0"]),
    (":DECode:PROTocol", &["I2C", "SPI", "C-PHY", "UART", "CAN", "nope"]),
    (":DECode:STATe", BOOL),
    (":DECode:INPut", &["WAVEFORM", "LA", "ANALOG"]),
    (":DECode:SOURce", ROLES),
    (":DECode:SOURce2", ROLES),
    (":DECode:CS", ROLES),
    (":DECode:SOURce4", ROLES),
    (":DECode:DIFFerential", ROLES),
    (":DECode:THReshold", &["1.65", "-10", "10", "100"]),
    (":DECode:CPHY:RATE", &["AUTO", "2.5e9", "0", "1e12", "1.1e12"]),
    (":LA:STATe", BOOL),
    (":LA:DIGital1:DISPlay", BOOL),
    (":LA:POD1:THReshold", &["1.4", "-15", "15", "16"]),
    (":DISPlay:TYPE", &["DOTS", "VECTors"]),
    (":DISPlay:GRADing:TIME", &["MIN", "INFinite", "0.1", "1", "10", "3", "100"]),
    (":DISPlay:WBRightness", &["50", "0", "100", "101", "10", "9"]),
    (":DISPlay:GBRightness", &["50", "0", "100", "101", "10", "9"]),
    (":DISPlay:GRID", &["FULL", "HALF", "NONE"]),
    (":DISPlay:RULers", BOOL),
    (":DISPlay:COLor", BOOL),
    (":DISPlay:VIEW", &["1", "2", "0"]),
    (":DISPlay:VIEW:COUNt", &["1", "2", "4", "5", "0"]),
    (":DISPlay:VIEW:MONitor", &["0", "1", "2"]),
    (":MATH1:DISPlay", BOOL),
    (":MATH1:OPERator", MATH_OPS),
    (":MATH1:SOURce1", &["CHAN1", "CHAN2", "R1", "MATH1", "MATH2"]),
    (":MATH1:SOURce2", &["CHAN1", "CHAN2", "R1", "MATH1", "MATH2"]),
    (":MATH2:SOURce1", &["MATH1", "MATH2", "MATH3"]),
    (":MATH1:SCALe", &["1", "0.001", "1000"]),
    (":MATH1:OFFSet", &["1", "-100"]),
    (":MATH1:EXPRession", &["\"CH1+CH2\"", "\"R1 - R2\"", "\"(((\""]),
    (":MATH1:FFT:SOURce", &["CHAN1", "CHAN2", "R1"]),
    (":MATH1:FFT:WINDow", &["RECTangle", "BLACkman", "HANNing", "HAMMing", "FLATtop"]),
    (":MATH1:FFT:SCALe", &["10", "1", "100"]),
    (":MATH1:FILTer:TYPE", &["LPASs", "HPASs", "BPASs", "BSTop"]),
    (":MATH1:FILTer:W1", &["1e3", "0", "1e12"]),
    (":MATH1:FILTer:W2", &["1e3", "0", "1e12"]),
    (":BUS1:MODE", &["PARallel", "RS232", "UART", "IIC", "I2C", "LIN", "SPI", "CAN"]),
    (":BUS1:DISPlay", BOOL),
    (":BUS1:FORMat", &["HEX", "ASCii"]),
    (":BUS1:EVENt", BOOL),
    (":BUS1:THReshold", &["1.65", "-10", "10"]),
    (":BUS1:RS232:TX", ROLES),
    (":BUS1:RS232:RX", ROLES),
    (":BUS1:RS232:BAUD", &["9600", "115200", "300", "100"]),
    (":BUS1:RS232:PARity", &["NONE", "ODD", "EVEN"]),
    (":BUS1:RS232:DBITs", &["8", "5", "9", "4"]),
    (":BUS1:IIC:SCLK:SOURce", ROLES),
    (":BUS1:IIC:SDA:SOURce", ROLES),
    (":BUS1:SPI:SCLK:SOURce", ROLES),
    (":BUS1:SPI:MOSI:SOURce", ROLES),
    (":BUS1:SPI:SS:SOURce", ROLES),
    (":BUS1:SPI:DBITs", &["8", "4", "32", "3", "33"]),
    (":MASK:ENABle", BOOL),
    (":MASK:SOURce", SOURCES),
    (":MASK:X", FRACTION),
    (":MASK:Y", FRACTION),
    (":MASK:OPERate", &["RUN", "STOP"]),
    (":HISTogram:ENABle", BOOL),
    (":HISTogram:TYPE", &["HORizontal", "VERTical"]),
    (":HISTogram:SOURce", SOURCES),
    (":SYSTem:PIPeline", &["BATCh", "FRAMes", "STANdard", "BATC", "FRAM"]),
    (":SYSTem:GRAPhics", &["CPU", "GPU", "cpu"]),
    (":SYSTem:AOUTput", &["TOUT", "PFAil", "PFA", "OFF", "off"]),
    (":SYSTem:DEBug:DVM:LANE", &["AUTO", "0", "3", "4", "auto"]),
];

/// What a setter needs before it does more than refuse: the reference commands (and the SIM slot's visibility) a
/// loaded R1, saved from CH1 after one frame of the demo signal; the mask's run state a mask that is on.
fn prepare(header: &str) -> &'static [&'static str] {
    if header.starts_with(":REFerence:") || header == ":SIM:SHOW" {
        &[":REFerence:SAVE"]
    } else if header == ":MASK:OPERate" {
        &[":MASK:ENABle ON"]
    } else {
        &[]
    }
}

/// A fresh session: what `prepare` names, `header probe` (left out without a probe), the query, and the error queue
/// until `0,`.
fn run(header: &str, probe: Option<&str>) -> String {
    let (mut s, mut ui, cv) = scpi_session();
    let before = prepare(header);
    if !before.is_empty() {
        ui.step(0.05);
    }
    for line in before {
        q(&mut s, &mut ui, &cv, line);
    }
    let mut parts = Vec::new();
    if let Some(p) = probe {
        let line = if p.is_empty() { header.to_string() } else { format!("{} {}", header, p) };
        let r = q(&mut s, &mut ui, &cv, &line);
        if !r.trim().is_empty() {
            parts.push(format!("set: {}", show(r.trim_end())));
        }
    }
    let a = q(&mut s, &mut ui, &cv, &format!("{}?", header));
    parts.push(format!("query: {}", show(a.trim_end())));
    let mut errors = Vec::new();
    // the queue holds at most 20
    for _ in 0..25 {
        let e = q(&mut s, &mut ui, &cv, ":SYSTem:ERRor?");
        let e = e.trim_end();
        if e.starts_with("0,") {
            break;
        }
        errors.push(show(e));
    }
    parts.push(format!("errors: {}", if errors.is_empty() { "none".to_string() } else { errors.join(" | ") }));
    parts.join("; ")
}

/// The lines of every setter and probe.
pub(super) fn lines() -> Vec<String> {
    let mut out = Vec::new();
    for &(header, values) in SETTERS {
        out.push(format!("scpi {} default -> {}", header, run(header, None)));
        for v in unique(values.iter().chain(&EVERY).map(|v| v.to_string())) {
            out.push(format!("scpi {} {:?} -> {}", header, v, run(header, Some(&v))));
        }
    }
    out
}

/// Setters of docs/scpi.md with a query form that are not probed, and why. `every_documented_setter_is_probed`
/// requires each documented one in `SETTERS` or here.
const LEFT_OUT: &[(&str, &str)] = &[
    (":SIM:PLAY", "starts a generation in the background: its answer would depend on how fast that runs"),
    (":SYSTem:SET", "writes the setup keys, which setup_probes covers through the same reader"),
    (":SYSTem:GET", "reads the setup keys, which setup_probes covers through setup_text"),
    (":SOURce<n>:*", "the generator: docs/scpi.md lists its commands without a query form"),
    (
        ":SYSTem:DEBug:MASK:MAP",
        "the comparator mapping is a state of the whole process, which other tests read at the same time; the test \
         of ui/src/scpi/system.rs sets it and puts it back",
    ),
];

/// The first node of a command a table row or a sentence of docs/scpi.md starts with; a later one in the same line
/// with fewer nodes than that is relative to it (`:CHANnel<n>:LABel:SHOW[?]`, `:LABel:CONTent[?]`).
const ROOTS: &[&str] = &[
    "ACQuire",
    "BODE",
    "BUS",
    "CHANnel",
    "COUNter",
    "CURSor",
    "DECode",
    "DISPlay",
    "DVM",
    "HISTogram",
    "LA",
    "MASK",
    "MATH",
    "MEASure",
    "REFerence",
    "SIM",
    "SYSTem",
    "TIMebase",
    "TRIGger",
    "VIEW",
    "WAVeform",
];

/// Every command of docs/scpi.md written with `[?]` (set and query), as `SETTERS` spells it: `<n>` as 1, an optional
/// node (`[:MAIN]`) in, each alternative of `{A|B}` on its own, a relative command (`, :STOP[?]`) completed from the
/// one before it in the same line.
fn documented_setters() -> Vec<String> {
    let doc = include_str!("../../../../docs/scpi.md");
    let mut out = Vec::new();
    for line in doc.lines() {
        let mut before: Option<Vec<String>> = None;
        for span in line.split('`').skip(1).step_by(2).filter(|s| s.starts_with(':')) {
            let token = span.split_whitespace().next().unwrap_or("");
            let query = token.contains("[?]");
            let token = token.replace("[?]", "").replace(['[', ']'], "");
            let nodes: Vec<String> = token.trim_start_matches(':').split(':').map(str::to_string).collect();
            let first = nodes[0].replace("<n>", "");
            let absolute = nodes.len() >= 2 && ROOTS.contains(&first.trim_end_matches(|c: char| c.is_ascii_digit()));
            let full = match &before {
                Some(b) if !absolute && b.len() >= nodes.len() => [&b[..b.len() - nodes.len()], &nodes[..]].concat(),
                _ => nodes,
            };
            before = Some(full.clone());
            if !query {
                continue;
            }
            let mut names = vec![String::new()];
            for node in &full {
                let node = node.replace("<n>", "1");
                let alternatives: Vec<&str> = node.trim_start_matches('{').trim_end_matches('}').split('|').collect();
                names = names.iter().flat_map(|n| alternatives.iter().map(move |a| format!("{}:{}", n, a))).collect();
            }
            out.extend(names);
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn every_documented_setter_is_probed() {
    let known: std::collections::HashSet<&str> =
        SETTERS.iter().map(|s| s.0).chain(LEFT_OUT.iter().map(|s| s.0)).collect();
    let found = documented_setters();
    for want in [":CHANnel1:SCALe", ":TIMebase:MAIN:SCALe", ":CURSor:TRACk:CBX", ":BUS1:SPI:MOSI:SOURce", ":MASK:Y"] {
        assert!(
            found.iter().any(|f| f == want),
            "the scan of docs/scpi.md found no {}: update documented_setters",
            want
        );
    }
    let missing: Vec<&String> = found.iter().filter(|f| !known.contains(f.as_str())).collect();
    assert!(
        missing.is_empty(),
        "setters of docs/scpi.md with a query form that settings_snapshot/scpi_probes.rs neither probes (SETTERS) nor \
         leaves out with a reason (LEFT_OUT):\n{}",
        missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n")
    );
}
