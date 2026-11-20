// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:MATH<n>`: `DISPlay`, `OPERator`, `SOURce1|SOURce2`, `SCALe`, `OFFSet`, `EXPRession`, `FFT:SOURce|SCALe` and
//! `FILTer:TYPE|W1|W2` of the eight math traces (the traces themselves are `crate::math`), and `FFT:WINDow`, declared
//! in settings/math.rs. A new `:MATH` setting of one value is a row of that table, another command goes into
//! `math_cmd` here; either with its query in ui/tests/scpi.txt, docs/scpi.md gets the row's line from the table, the
//! other's by hand (AGENTS.md).

use super::{e, kw, nearest_index, parse_bool, parse_wave, suffix, t, Scpi};
use crate::math::{math_scales, MathCh, MathOp, MATH_REF_BASE, NMATH};
use crate::model::*;
use crate::ui::Ui;

pub(super) fn math_index(tok: &str) -> Option<usize> {
    let (k, n) = suffix(tok);
    if kw(k, "MATH") {
        n.filter(|&n| (1..=NMATH).contains(&n)).map(|n| n - 1)
    } else {
        None
    }
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
    if src < NCH {
        format!("CHAN{}", src + 1)
    } else if src < MATH_REF_BASE {
        format!("MATH{}", src - NCH + 1)
    } else {
        format!("R{}", src - MATH_REF_BASE + 1)
    }
}

/// A math input: a live channel, or an earlier math trace. A later trace would be a cycle.
fn parse_math_src(s: &str, k: usize) -> Option<usize> {
    if let Some(id) = parse_wave(s) {
        return match id {
            WaveId::Ch(c) if c < n_ch() => Some(c),
            WaveId::Ref(r) => Some(MATH_REF_BASE + r),
            _ => None,
        };
    }
    let idx = math_index(s.trim())?;
    (idx < k).then_some(NCH + idx)
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\''))) {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

impl Scpi {
    /// One of the eight math traces. Display off is `MathOp::Off`. A math source may be a channel or an earlier math trace.
    pub(super) fn math_cmd(&mut self, ui: &mut Ui, k: usize, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let n0 = n.first().copied().unwrap_or("");
        // the rows of settings/math.rs hold the selected channel: channel k is read and set in a copy of the model
        // that selects it
        if let Some(row) = crate::settings::command(&crate::settings::math::SLICES, n) {
            let mut m = ui.m.clone();
            m.math_select(k);
            if query {
                return t(&row.query(&m, 0));
            }
            if !row.take(&mut m, 0, arg) {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            }
            put_math(ui, k, m.math_ch(k));
            return None;
        }
        if n.len() == 1 && kw(n0, "DISPlay") {
            return self.math_display(ui, k, query, arg);
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
            return self.math_source(ui, k, kw(n0, "SOURce2"), query, arg);
        }
        if n.len() == 1 && kw(n0, "SCALe") {
            return self.math_scale(ui, k, query, arg);
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
            return self.math_fft_source(ui, k, query, arg);
        }
        if n.len() == 2 && kw(n0, "FFT") && kw(n[1], "SCALe") {
            return self.math_fft_scale(ui, k, query, arg);
        }
        if n.len() == 2 && kw(n0, "FILTer") && kw(n[1], "TYPE") {
            return self.math_filter_type(ui, k, query, arg);
        }
        if n.len() == 2 && kw(n0, "FILTer") && (kw(n[1], "W1") || kw(n[1], "W2")) {
            return self.math_filter_corner(ui, k, kw(n[1], "W2"), query, arg);
        }
        self.push_err(format!("-113,\"Undefined header\" :MATH{}:{}", k + 1, n.join(":")));
        None
    }

    /// `:MATH<n>:DISPlay`: the trace on (keeping its operator, else ADD) or off.
    fn math_display(&mut self, ui: &mut Ui, k: usize, query: bool, arg: &str) -> Option<Vec<u8>> {
        if query {
            return t(if ui.m.math_ch(k).op == MathOp::Off { "0" } else { "1" });
        }
        match parse_bool(arg) {
            Some(on) => {
                let mut c = ui.m.math_ch(k);
                c.op = if on {
                    if c.op == MathOp::Off {
                        MathOp::Add
                    } else {
                        c.op
                    }
                } else {
                    MathOp::Off
                };
                put_math(ui, k, c);
                None
            }
            None => {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                None
            }
        }
    }

    /// `:MATH<n>:SOURce1` and `SOURce2`: a channel, an earlier math trace or a filled reference.
    fn math_source(&mut self, ui: &mut Ui, k: usize, second: bool, query: bool, arg: &str) -> Option<Vec<u8>> {
        if query {
            let c = ui.m.math_ch(k);
            return t(&math_src_name(if second { c.b } else { c.a }));
        }
        let Some(src) = parse_math_src(arg, k) else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        };
        if src >= MATH_REF_BASE && ui.ref_info(src - MATH_REF_BASE).is_none() {
            self.push_err("-230,\"Math reference source is empty\"".into());
            return None;
        }
        let mut c = ui.m.math_ch(k);
        if second {
            c.b = src;
        } else {
            c.a = src;
        }
        put_math(ui, k, c);
        None
    }

    /// `:MATH<n>:SCALe`: the nearest step of the trace's scale list (dB for an FFT).
    fn math_scale(&mut self, ui: &mut Ui, k: usize, query: bool, arg: &str) -> Option<Vec<u8>> {
        let c = ui.m.math_ch(k);
        if query {
            let v = if c.op == MathOp::Fft {
                FFT_DB[c.scale.min(FFT_DB.len() - 1)] as f64
            } else {
                math_scales()[c.scale.min(math_scales().len() - 1)]
            };
            return t(&e(v));
        }
        let Some(v) = arg.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0) else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        };
        let mut c = ui.m.math_ch(k);
        c.scale = if c.op == MathOp::Fft {
            nearest_index(&FFT_DB.map(|x| x as f64), v)
        } else {
            nearest_index(&math_scales(), v)
        };
        put_math(ui, k, c);
        None
    }

    /// `:MATH<n>:FFT:SOURce`: the source of an FFT; setting it makes the trace an FFT.
    fn math_fft_source(&mut self, ui: &mut Ui, k: usize, query: bool, arg: &str) -> Option<Vec<u8>> {
        if query {
            return t(&math_src_name(ui.m.math_ch(k).a));
        }
        let Some(src) = parse_math_src(arg, k) else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        };
        if src >= MATH_REF_BASE && ui.ref_info(src - MATH_REF_BASE).is_none() {
            self.push_err("-230,\"Math reference source is empty\"".into());
            return None;
        }
        let mut c = ui.m.math_ch(k);
        c.a = src;
        c.op = MathOp::Fft;
        put_math(ui, k, c);
        None
    }

    /// `:MATH<n>:FFT:SCALe`: the nearest dB step; setting it makes the trace an FFT.
    fn math_fft_scale(&mut self, ui: &mut Ui, k: usize, query: bool, arg: &str) -> Option<Vec<u8>> {
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
        c.scale = nearest_index(&FFT_DB.map(|x| x as f64), v);
        put_math(ui, k, c);
        None
    }

    /// `:MATH<n>:FILTer:TYPE`: low pass, high pass, band pass or band stop.
    fn math_filter_type(&mut self, ui: &mut Ui, k: usize, query: bool, arg: &str) -> Option<Vec<u8>> {
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
        None
    }

    /// `:MATH<n>:FILTer:W1` and `W2`: the corner of a filter, the low and high edge of a band.
    fn math_filter_corner(&mut self, ui: &mut Ui, k: usize, high: bool, query: bool, arg: &str) -> Option<Vec<u8>> {
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
        None
    }
}
