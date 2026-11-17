// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The setup keys and the values that probe them. The values come from each key's parse code in `store::set`: the
//! choices of a choice key, the edges of a range, values past them, and text that is no value at all.

use super::{show, unique};
use crate::store::{parse_setup_lenient, setup_text};
use std::collections::{HashMap, HashSet};

/// What a key takes, and so which values probe it (besides `EVERY`).
#[derive(Clone, Copy)]
enum P {
    /// a boolean in its spellings
    Bool,
    /// a float; the slice adds the edges of its range and values past them
    Num(&'static [&'static str]),
    /// an unsigned integer; the slice adds edges
    Int(&'static [&'static str]),
    /// an index below the number
    Idx(usize),
    /// exactly these values: the choices of a choice key, or text
    Is(&'static [&'static str]),
    /// fields separated by spaces: each field of the default value replaced by `BAD_FIELD`s, one field less, one more,
    /// and the values of the slice
    Fields(&'static [&'static str]),
}

use P::*;

/// Every key also gets these: empty, garbage, negative, huge.
const EVERY: [&str; 4] = ["", "x", "-1", "1e9"];
const BOOL: [&str; 8] = ["true", "false", "on", "off", "1", "0", "TRUE", "yes"];
const NUM: [&str; 7] = ["1.5", "-2.25", "0", "3e38", "1e39", "inf", "NaN"];
const INT: [&str; 5] = ["0", "7", "4294967295", "4294967296", "1.5"];
const BAD_FIELD: [&str; 3] = ["x", "-1", "1e9"];

const WAVES: &[&str] = &["0", "1", "4", "5", "R1", "R10", "R11", "r2", "CH1"];
const DEC_SOURCES: &[&str] = &["0", "3", "4", "19", "20", "29", "30", "R1", "R10", "R11", "r1"];
const DEC_OPTIONAL: &[&str] = &["none", "0", "1", "4", "5", "20", "21", "30", "31", "R1", "R11"];
const TDIV: &[&str] = &["5e-11", "1000", "2000", "1e-11", "0.0002", "0.0002001", "0.0002003", "3e-4", "1"];
const SLOPES: &[&str] = &["Rise", "Fall", "Either", "rise"];
const RELS: &[&str] = &["Eq", "Ne", "Gt", "Lt", "In", "Out", "eq"];
const WINDOWS: &[&str] = &["Rect", "Hann", "Hamming", "Blackman", "Flattop", "hann"];
const MATH_OPS: &[&str] = &[
    "Off", "Add", "Sub", "Mul", "Div", "Conv", "Intg", "Diff", "Abs", "Sqrt", "Lg", "Ln", "Exp", "Lin", "Expr", "Lpf",
    "Hpf", "Bpf", "Bsf", "Fir", "Fft", "And", "Or", "Xor", "Nand", "Nor", "Not", "A2d", "D2a", "Unnrzi", "add",
];
const BWS: &[&str] = &[
    "Auto", "Full", "20 MHz", "GND", "5 MHz", "10 MHz", "15 MHz", "25 MHz", "30 MHz", "35 MHz", "40 MHz", "50 MHz",
    "60 MHz", "70 MHz", "75 MHz", "80 MHz", "90 MHz", "100 MHz", "125 MHz", "150 MHz", "175 MHz", "200 MHz", "250 MHz",
    "300 MHz", "310 MHz", "400 MHz", "600 MHz", "800 MHz", "1 GHz", "1.5 GHz", "2 GHz", "20MHz", "auto",
];
const PROTOS: &[&str] = &[
    "Uart",
    "I2c",
    "Spi",
    "Can",
    "Lin",
    "Parallel",
    "Cphy",
    "Dmx512",
    "Spdif",
    "UsbPd",
    "Dcc",
    "Manchester",
    "Mlt3",
    "Uart9",
    "ModbusRtu",
    "ModbusAscii",
    "OneWire",
    "Ds2482",
    "Swd",
    "Jtag",
    "Qspi",
    "I3c",
    "CanFd",
    "FlexRay",
    "Sent",
    "Arinc429",
    "Mil1553",
    "I2s",
    "Usb",
    "Example",
    "uart",
    "UART",
];

/// The keys of one decoder (`dec_lines`): the selected one as they are, the others as `d<N>.<key>`.
const DECODER: &[(&str, P)] = &[
    ("dec_on", Bool),
    ("dec_src", Is(DEC_SOURCES)),
    ("dec_baud", Idx(10)),
    ("dec_canrate", Idx(5)),
    ("dec_canh", Bool),
    ("dec_fdrate", Idx(5)),
    ("dec_arate", Idx(3)),
    ("dec_tick", Idx(8)),
    ("dec_nibbles", Int(&["1", "6", "7"])),
    ("dec_uspeed", Idx(3)),
    ("dec_cphy_rate", Is(&["0", "AUTO", "auto", "2500000000", "1e12", "1.1e12", "inf"])),
    ("dec_src4", Is(DEC_OPTIONAL)),
    ("dec_diff", Is(DEC_OPTIONAL)),
    ("trig_bus", Is(&["Frame", "ID/Addr", "Data", "Error", "Id", "frame"])),
    ("trig_bus_val", Is(&["0", "18446744073709551615", "18446744073709551616", "0x10", "+5"])),
    ("ser.rel", Is(RELS)),
    ("ser.max", Is(&["0", "18446744073709551615", "18446744073709551616", "0x10"])),
    ("ser.mask", Is(&["0", "18446744073709551615", "18446744073709551616", "0x10"])),
    ("ser.uart_inv", Bool),
    ("ser.uart_stop", Is(&["One", "OneHalf", "Two", "one"])),
    ("ser.uart_err", Is(&["Either", "Parity", "Framing", "parity"])),
    ("ser.spi_cs_high", Bool),
    ("ser.spi_timeout", Num(&["-1", "1e-3"])),
    ("ser.addr_rel", Is(RELS)),
    ("ser.addr_max", Int(&["1023", "1024"])),
    ("ser.dir", Is(&["Either", "Write", "Read", "write"])),
    ("ser.ten_bit", Bool),
    ("ser.bytes", Int(&["5", "6"])),
    ("dec_thr", Num(&[])),
    ("dec_ascii", Bool),
    ("dec_proto", Is(PROTOS)),
    ("dec_opt", Fields(&["1 2 3 4", "255 255 255 255", "256 0 0 0", "0 0 0 0"])),
    ("dec_src2", Is(DEC_SOURCES)),
    ("dec_cs", Is(&["none", "0", "3", "4", "29", "30", "R1", "R11"])),
    ("dec_mode", Idx(4)),
    ("dec_bits", Int(&["4", "32", "3", "33"])),
    ("dec_parity", Is(&["None", "Even", "Odd", "odd"])),
];

/// The other keys, in the order of `setup_text`; then legacy keys `set` still reads and keys it refuses.
const KEYS: &[(&str, P)] = &[
    ("deskew_ch1", Num(&["1e-9", "1000", "-1000", "1000.5", "-1000.5"])),
    ("deskew_ch4", Is(&["1e-9"])),
    ("deskew_ref1", Num(&["1e-9", "1000", "-1000", "1000.5"])),
    ("deskew_ref10", Is(&["1e-9"])),
    ("ch1.on", Bool),
    ("ch1.vdiv", Num(&["0.0001", "0.0002", "10", "20", "100", "0.00005", "0.0015", "1.0005", "1.002"])),
    ("ch1.vfine", Num(&["1", "2.5", "0.99", "2.51", "1.25"])),
    ("ch1.unit", Is(&["V", "A", "W", "U", "v", "Volt"])),
    ("ch1.bias", Num(&[])),
    ("ch1.skew", Num(&["1e-7", "-1e-7", "1.1e-7", "-1.1e-7", "1e-9"])),
    ("ch1.offset", Num(&["100", "-100"])),
    ("ch1.coupling", Is(&["Dc", "Ac", "Gnd", "Dc50", "DC"])),
    ("ch1.bw", Is(BWS)),
    ("ch1.probe", Num(&["10", "0.001", "1000", "-10"])),
    ("ch1.invert", Bool),
    ("ch1.label", Is(&["probe", "Ab cd ef gh ij kl", "  padded  ", "0123456789X", "a=b", "#hash"])),
    ("ch4.on", Is(&["true"])),
    ("active", Idx(4)),
    ("cur_b_source", Is(&["none", "0", "1", "4", "5", "R1", "R10", "R11", "CH1"])),
    ("tdiv", Num(TDIV)),
    ("delay", Num(&[])),
    ("trig_src", Idx(4)),
    ("trig_ref", Idx(10)),
    ("trig_ref", Is(&["none", "None"])),
    ("trig_la", Idx(16)),
    ("trig_la", Is(&["none"])),
    ("pat_la", Is(&["HHHHLLLLXXXXHHHH", "HLXRFHLXRFHLXRFH", "hhhhhhhhhhhhhhhh", "HHHH", "HHHHLLLLXXXXHHHHH"])),
    ("trig_level", Num(&[])),
    ("slope", Is(SLOPES)),
    ("trig_mode", Is(&["Auto", "Normal", "Single", "auto"])),
    ("holdoff", Num(&["10", "11"])),
    (
        "trig_type",
        Is(&[
            "Edge",
            "Pulse",
            "Slope",
            "Video",
            "Pattern",
            "Duration",
            "Timeout",
            "Runt",
            "Window",
            "Delay",
            "SetupHold",
            "NthEdge",
            "Bus",
            "edge",
        ]),
    ),
    ("trig_level2", Num(&[])),
    ("trig_nrej", Bool),
    ("auto_nrej", Bool),
    ("trig_coup", Is(&["Dc", "Ac", "LfReject", "HfReject", "dc"])),
    ("trig_src2", Idx(4)),
    ("trig_ref2", Idx(10)),
    ("trig_ref2", Is(&["none"])),
    ("trig_slope2", Is(SLOPES)),
    ("trig_pol", Is(&["Pos", "Neg", "pos"])),
    ("trig_when", Is(&["Off", "Gt", "Lt", "In", "Out", "gt"])),
    ("trig_t1", Num(&[])),
    ("trig_t2", Num(&[])),
    ("video_std", Is(&["Pal", "Ntsc", "pal"])),
    ("video_sync", Is(&["AllLines", "Line", "Odd", "Even", "AllFields", "line"])),
    ("video_line", Int(&[])),
    ("pat", Fields(&["H H H H", "L L L L", "X X X X", "R R R R", "F F F F", "h X X H", "H  X X H"])),
    ("pat_levels", Fields(&["1 2 3 4", "inf 0 0 0"])),
    ("win_mode", Is(&["Enter", "Exit", "Inside", "enter"])),
    ("nth_idle", Num(&[])),
    ("nth_n", Int(&[])),
    ("setup", Num(&[])),
    ("hold", Num(&[])),
    ("acq", Is(&["Normal", "Peak", "Average", "HiRes", "normal"])),
    ("cores", Bool),
    ("avg_count", Int(&["1", "2", "65536"])),
    ("avg_hw", Bool),
    ("segments", Int(&["100000", "100001"])),
    ("seg_hw", Bool),
    ("seg_interval", Num(&["10", "10.5", "-0.5"])),
    ("mdepth", Idx(11)),
    ("persist", Idx(7)),
    ("intensity", Int(&["10", "100", "9", "101"])),
    ("grid", Is(&["Full", "Light", "None", "full"])),
    ("grid_bright", Int(&["10", "100", "9", "101"])),
    ("grid_frame", Bool),
    ("windows", Is(&["One", "Cols", "Rows", "Quad", "one"])),
    ("density", Is(&["Scope", "Full", "Split", "Four", "scope"])),
    ("split_pane", Idx(6)),
    ("ch_window", Fields(&["4 4 4 4", "0 1 1 1", "5 1 1 1", "1 2 3 4"])),
    ("math_window", Fields(&["4 4 4 4 4 4 4 4", "0 1 1 1 1 1 1 1"])),
    ("la_window", Is(&["1", "4", "5", "0"])),
    ("ref_window", Fields(&["4 4 4 4 4 4 4 4 4 4", "0 1 1 1 1 1 1 1 1 1"])),
    ("meas", Is(&["Vpp:1", "Vpp:1 Freq:2", "Vpp:R1", "Vpp", "Nope:1", "Vpp:5", "Vmax:1 Vmin:1 Vpp:1", "vpp:1"])),
    ("meas2", Is(&["0:1:2", "16:1:2", "17:1:2", "0:1", "0:R1:R2", "0:1:2 0:1:2", "0:1:2:3"])),
    ("cat", Is(&["15:1:1:1:1", "1001:1:2:3:4", "0:1:1:1:1", "1002:1:1:1:1", "15:1:1:1", "15:1:1:1:1 15:1:1:1:1"])),
    ("cat", Is(&["15:R1:1:1:1", "15:5:1:1:1", "16:1:1:1:1 40:2:2:2:2"])),
    ("dvm", Fields(&["true 3 2", "on 0 0", "true 4 0", "true 0 3"])),
    ("counter", Fields(&["true 3 2 7", "false 0 0 2", "false 0 0 3", "false 0 0 8"])),
    ("cursor", Is(&["Off", "Time", "Volt", "Track", "Xy", "off"])),
    ("cur_a", Num(&["1", "-0.5", "1.5"])),
    ("cur_b", Num(&["1", "-0.5", "1.5"])),
    ("cur_c", Num(&["1", "-0.5", "1.5"])),
    ("cur_d", Num(&["1", "-0.5", "1.5"])),
    ("touch_lock", Bool),
    ("aux_out", Is(&["TrigOut", "Off", "PassFail", "off"])),
    ("fine_v", Bool),
    ("fine_t", Bool),
    ("tfine", Num(&["0.1", "1", "0.05", "0.5"])),
    ("v_expand_gnd", Bool),
    ("h_expand", Is(&["Center", "Left", "Right", "Trigger", "center"])),
    ("math", Is(MATH_OPS)),
    ("math_a", Idx(22)),
    ("math_b", Idx(22)),
    ("math_scale", Idx(57)),
    ("math_fc", Num(&[])),
    ("math_lin", Fields(&["2 -1", "1", "1  2", "1 2 3"])),
    ("math_expr1", Is(&["CH1+CH2", "  sin(t) ", "(((", "R1 - R2", "a=b"])),
    (
        "math_expr1",
        Is(&[
            "1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1",
            "1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+1+",
        ]),
    ),
    ("math_expr8", Is(&["CH1"])),
    ("log_ivl", Idx(10)),
    ("spec_margin", Num(&["60", "61", "-1"])),
    ("math_offset", Num(&[])),
    ("fft_win", Is(WINDOWS)),
    ("math_bw", Num(&[])),
    ("math_sel", Idx(8)),
    ("m2.op", Is(MATH_OPS)),
    ("m2.a", Idx(22)),
    ("m2.b", Idx(22)),
    ("m2.scale", Idx(57)),
    ("m2.fc", Num(&[])),
    ("m2.bw", Num(&[])),
    ("m2.offset", Num(&[])),
    ("m2.win", Is(WINDOWS)),
    ("m2.lin", Fields(&["2 -1", "1", "1  2", "1 2 3"])),
    ("m8.op", Is(&["Add"])),
    ("m1.op", Is(&["Add"])),
    ("dec_sel", Idx(4)),
    ("bus_on", Bool),
    ("bus_label", Is(&["SDA bus", "0123456789AB", "  pad  "])),
    ("bus_pos", Num(&["-3.6", "3.6", "-4", "4"])),
    ("bus_kind", Is(&["I2c", "Spi", "i2c"])),
    ("bus_clk", Is(WAVES)),
    ("bus_data", Is(WAVES)),
    ("bus_clk_thr", Num(&["40", "-40", "41", "-41"])),
    ("bus_data_thr", Num(&["40", "-40", "41", "-41"])),
    ("bus_rw", Bool),
    ("pane", Idx(6)),
    ("meas_labels", Bool),
    ("strip_dock", Is(&["Under", "Left", "Right", "Above", "under"])),
    ("trend_sel", Fields(&["1025 1025", "1026 0", "1 2"])),
    ("zoom", Bool),
    ("auto_roll", Bool),
    ("zoom_tdiv", Num(TDIV)),
    ("zdelay", Num(&[])),
    ("color_grade", Bool),
    ("dots", Bool),
    ("bar", Is(&["cards", "compact", "Cards"])),
    ("draw_path", Is(&["batch", "standard", "Batch"])),
    ("hdmi", Is(&["Mirror", "Waveform", "Layout", "mirror"])),
    ("hdmi_view", Is(&["0", "1", "4", "5", "+1"])),
    ("view", Is(&["Scope", "Spectrum", "Eye", "Power", "Bode", "Magnetic", "Inverter", "Dpt", "scope"])),
    ("views", Fields(&["4 4", "4 5", "0 1", "1 1", "2 1", "2 0", "5 1"])),
    ("view1", Is(&["zoom=true; pane=2", "xy=1; full=0", "nope=1", "zoom", "zoom=true;; ;"])),
    ("view4", Is(&["zoom=true"])),
    (
        "bode",
        Fields(&["100 1000000 1000 1 1 2 5 10.1.2.3:5555", "100 1000000 1 1 1 2 0", "100 1000000 40 1 R1 R2 0 a b"]),
    ),
    ("bode_link", Is(&["lan", "usb", "LAN"])),
    (
        "bode_sweep",
        Fields(&["0 true 4 Rise 1 1 0", "1 false 0 Fall 1 4 0", "1 true 5 Rise 1 1 0", "1 true 1 Either 1 1 0"]),
    ),
    ("bode_sweep", Is(&["1 true 1 Rise 1 5 0", "inf true 1 Rise 1 1 0", "1e-9 on 1 Fall -2 0 -10"])),
    (
        "spec",
        Fields(&[
            "R1 1e6 2e6 1000 Rect 3 -10 true true",
            "1 0 0 0 Hann 4 0 false false",
            "5 0 0 0 Hann 0 0 false false",
        ]),
    ),
    ("spec_peaks", Fields(&["1 4", "2 0", "0 5"])),
    ("eye", Fields(&["1 2.5e9", "1", "R1 1e9 1", "1 1e9 2", "4 inf 0"])),
    ("eye_place", Is(&["Full", "Side", "Under", "full"])),
    (
        "power",
        Fields(&["1 2 Harmonics", "1 2 Switching", "1 2 Ripple", "1 2 Capacitance", "1 2 Rail", "R1 R2 Quality"]),
    ),
    ("meas_cfg", Fields(&["1e-6 Above 32 1", "0 Below 0 0", "0 Both 33 0 0.2 0.8 1e-9 1e-9", "0 Both 2 0 0.1 0.9"])),
    ("meas_full_record", Bool),
    ("spec_acpr", Fields(&["0 0", "1e3 1e3"])),
    ("bode_mode", Is(&["Loop", "Psrr", "Z", "loop"])),
    ("eye_more", Fields(&["0.001 1 2 1 1 3 2 0", "1e-12 1000 1000000 0 R1 0 R2 1"])),
    ("power_more", Fields(&["50 100000 1 3.3 2", "60 0 R1 - R2"])),
    ("magnetic", Fields(&["1 2 0 1 1 1 BH 1", "1 2 20000 -1 -1 -1 IPsi 0", "R1 R2 5 1 1 1 Bh 0"])),
    (
        "inverter",
        Fields(&[
            "Mech 1 2 3 Hall 32 4 false 1 2 3 1 1 0 true 15 14 13 100000 true 0 1 2",
            "Dq0 1 2 3 Phase 33 4 true off off off 1 1 0 false 0 1 16 0 false 0 1 2",
            "Dq0 1 2 3 Analog 0 4 true off off off 1 1 0 false 0 1 off 100001 false 0 1 2",
            "Dq0 1 2 3 Quad 1 4 true off off off 1 1 0 false 0 1 off 1024 false 0 1 2",
        ]),
    ),
    (
        "dpt",
        Fields(&[
            "1 2 3 Gate 4 0 TurnOn",
            "1 2 3 DiodeV 4 50 TurnOff",
            "1 2 3 DiodeI 4 49 Both",
            "R1 R2 R3 Off R4 1 Both",
        ]),
    ),
    ("sim", Fields(&["11 9", "12 0", "0 10", "3 0 1 7", "0 0"])),
    (
        "sim_extra",
        Fields(&[
            "true 0.05 1000000 1e-10 5e-11 100000000",
            "false 0 1000 0 0 1000",
            "false 0.051 33000 0 0 1000000",
            "false 0.005 999 0 0 1000000",
            "false 0.005 33000 1.1e-10 0 1000000",
            "false 0.005 33000 0 5.1e-11 1000000",
            "false 0.005 33000 0 0 100000001",
        ]),
    ),
    (
        "zone1",
        Fields(&["true 2 false 0.1:0.5 0.2:0.6", "true 0 true", "true 5 true", "true 1 true 0.1", "true 4 true x:1"]),
    ),
    ("zone2", Is(&["true 4 false 0:0"])),
    ("z_src", Is(&["off", "0", "1", "4", "5"])),
    ("hist", Is(&["Off", "Vertical", "Horizontal", "off"])),
    ("mask_on", Bool),
    ("mask_src", Idx(4)),
    ("mask_x", Num(&[])),
    ("mask_y", Num(&[])),
    ("mask_stop", Bool),
    ("mask_inv", Bool),
    (
        "quick",
        Is(&[
            "Screenshot",
            "SavePng",
            "SaveSetup",
            "SaveCsv",
            "Autoset",
            "Single",
            "RunStop",
            "Clear",
            "NextView",
            "single",
        ]),
    ),
    ("text_size", Is(&["Normal", "Large", "Larger", "normal"])),
    ("la", Fields(&["true 00ff 7 6 -15 15 2", "true ffff 8 0 1 1 0", "false 0 0 0 16 -16 3", "false 1ffff 0 0 0 0 0"])),
    ("dec_la", Fields(&["1 0 15 16 -", "0 - - - -", "2 1 2 3 4", "1 a - - -"])),
    // legacy keys: read, never written
    ("ch1.vscale", Idx(15)),
    ("ch1.bw20", Bool),
    ("cur_b_ch", Idx(4)),
    ("tscale", Idx(35)),
    ("zscale", Idx(35)),
    ("hdmi_wave", Bool),
    // keys that are refused or ignored
    ("deskew_ch0", Is(&["1e-9"])),
    ("deskew_ch5", Is(&["1e-9"])),
    ("deskew_ref11", Is(&["1e-9"])),
    ("deskew_chx", Is(&["1e-9"])),
    ("ch0.on", Is(&["true"])),
    ("ch5.on", Is(&["true"])),
    ("chx.on", Is(&["true"])),
    ("ch1.nope", Is(&["1"])),
    ("m0.op", Is(&["Add"])),
    ("m9.op", Is(&["Add"])),
    ("m2.nope", Is(&["1"])),
    ("math_expr0", Is(&["CH1"])),
    ("math_expr9", Is(&["CH1"])),
    ("math_exprx", Is(&["CH1"])),
    ("view0", Is(&["zoom=true"])),
    ("view5", Is(&["zoom=true"])),
    ("zone3", Is(&["true 1 true"])),
    ("d0.dec_on", Is(&["true"])),
    ("d1.dec_on", Is(&["true"])),
    ("d5.dec_on", Is(&["true"])),
    ("d2.nope", Is(&["1"])),
    ("nope", Is(&["1"])),
];

/// The values of a key of kind `p` whose default line has `default` as its value.
fn probes(p: P, default: Option<&str>) -> Vec<String> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut v = match p {
        Bool => s(&BOOL),
        Num(x) => [s(&NUM), s(x)].concat(),
        Int(x) => [s(&INT), s(x)].concat(),
        Idx(n) => vec!["0".into(), "1".into(), (n - 1).to_string(), n.to_string(), "+1".into()],
        Is(x) => s(x),
        Fields(x) => {
            let mut v = Vec::new();
            if let Some(d) = default {
                let f: Vec<&str> = d.split(' ').collect();
                for i in 0..f.len() {
                    for bad in BAD_FIELD {
                        let mut g = f.clone();
                        g[i] = bad;
                        v.push(g.join(" "));
                    }
                }
                v.push(f[..f.len() - 1].join(" "));
                v.push(format!("{} 0", d));
            }
            v.extend(s(x));
            v
        }
    };
    v.extend(s(&EVERY));
    unique(v)
}

/// `key = value` pairs of a setup text, in its order.
fn pairs(text: &str) -> Vec<(String, String)> {
    text.lines().filter_map(|l| l.split_once(" = ")).map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// What loading `key = value` after `seed` does, against `base` (the setup of `seed` alone).
fn load(seed: &str, base: &HashMap<String, String>, key: &str, value: &str) -> String {
    let (m, bad) = parse_setup_lenient(&format!("{}\n{} = {}\n", seed, key, value));
    let after = pairs(&setup_text(&m));
    let own = after.iter().find(|(k, _)| k == key).map_or("(absent)".to_string(), |(_, v)| show(v));
    let mut out = own;
    for e in bad {
        out.push_str(&format!("; error: {}", show(&e)));
    }
    let keys: HashSet<&str> = after.iter().map(|(k, _)| k.as_str()).collect();
    let mut also: Vec<String> = after
        .iter()
        .filter(|(k, v)| k != key && base.get(k) != Some(v))
        .map(|(k, v)| format!("{} = {}", k, show(v)))
        .collect();
    let mut gone: Vec<String> =
        base.keys().filter(|k| *k != key && !keys.contains(k.as_str())).map(|k| format!("{} (absent)", k)).collect();
    gone.sort();
    also.extend(gone);
    if !also.is_empty() {
        out.push_str(&format!("; also: {}", also.join(", ")));
    }
    out
}

/// The lines of every setup key and probe.
pub(super) fn lines() -> Vec<String> {
    let keys: Vec<(String, P)> = KEYS
        .iter()
        .chain(DECODER)
        .map(|&(k, p)| (k.to_string(), p))
        .chain(DECODER.iter().map(|&(k, p)| (format!("d2.{}", k), p)))
        .collect();
    // a decoder other than the selected one is written once it holds something: its lines are there before the probe
    let seeds = ["", "d2.dec_on = false"];
    let bases: Vec<HashMap<String, String>> =
        seeds.iter().map(|s| pairs(&setup_text(&parse_setup_lenient(s).0)).into_iter().collect()).collect();
    let mut out = Vec::new();
    let mut done = HashSet::new();
    for (key, p) in &keys {
        let (key, p) = (key.as_str(), *p);
        let which = usize::from(key.starts_with("d2."));
        let base = &bases[which];
        let default = base.get(key).map(String::as_str);
        if done.insert(key) {
            out.push(format!("setup {} default -> {}", key, default.map_or("(absent)".to_string(), show)));
        }
        for v in probes(p, default) {
            out.push(format!("setup {} {:?} -> {}", key, v, load(seeds[which], base, key, &v)));
        }
    }
    out
}
