// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The words and numbers of a setting, as SCPI and the setup file write them: keyword matching, booleans, numbers in
//! the Rigol format, indices. Below the settings table (ADR 0043), which reads and writes every setting with them, and
//! below the SCPI interpreter and the setup reader, which use them for the rest.

/// SCPI keyword match: the long form or its short form (the upper-case part), case-insensitive.
pub fn kw(tok: &str, long: &str) -> bool {
    let t = tok.to_ascii_uppercase();
    let short: String = long.chars().filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit()).collect();
    t == long.to_ascii_uppercase() || t == short
}

/// The index of the entry of `list` nearest to `v` by difference (`nearest` compares ratios), the first of two as
/// near; 0 for an empty list.
pub fn nearest_index(list: &[f64], v: f64) -> usize {
    list.iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (*a - v).abs().partial_cmp(&(*b - v).abs()).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// An SCPI boolean: `1`, `ON`, `0`, `OFF`, any case.
pub fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_uppercase().as_str() {
        "1" | "ON" => Some(true),
        "0" | "OFF" => Some(false),
        _ => None,
    }
}

/// A finite number ("nan", "inf" and overflows are refused: they would end up in the model and in saved setups).
pub fn num(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

/// Rigol number format: 7 significant digits, two-digit exponent ("2.000000E-04").
pub fn e(v: f64) -> String {
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

/// A boolean of the setup file: `true`, `on`, `1` or `false`, `off`, `0`.
pub fn p_bool(v: &str) -> Result<bool, String> {
    match v {
        "true" | "on" | "1" => Ok(true),
        "false" | "off" | "0" => Ok(false),
        _ => Err(format!("not a boolean: {}", v)),
    }
}

/// A finite number of an f32 field.
pub fn p_f32(v: &str) -> Result<f32, String> {
    v.parse::<f32>().ok().filter(|x| x.is_finite()).ok_or_else(|| format!("not a number: {}", v))
}

/// A number of an f64 field, as `str::parse` reads it: also `inf` and `NaN`, with its error text.
pub fn p_f64(v: &str) -> Result<f64, String> {
    v.parse::<f64>().map_err(|e| e.to_string())
}

/// A whole number of a u32 field.
pub fn p_u32(v: &str) -> Result<u32, String> {
    v.parse::<u32>().map_err(|_| format!("not an integer: {}", v))
}

/// An index below `n`.
pub fn p_idx(v: &str, n: usize) -> Result<usize, String> {
    let i = v.parse::<usize>().map_err(|_| format!("not an index: {}", v))?;
    if i < n {
        Ok(i)
    } else {
        Err(format!("{} out of range (< {})", i, n))
    }
}
