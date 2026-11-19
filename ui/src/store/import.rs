// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Waveform files of other instruments and programs, recognised by their header (`parse_wave`), and `Fields`, the
//! checked reader of their binary fields.

use super::*;

/// A stored waveform, recognized by its header:
/// `OOSW`; `:WFM#001`, `:WFM#002` or `:WFM#003`; `:WFMPRE:`; `WAVEDESC`; cookie `AG`;
/// a `$&%#` block (`9601`, `9007`, or `2001`) or a `Title:` / `Plotname:` transient (`Binary:` or `Values:`);
/// XML `WaveformInternal` or `WaveformPatternInternal`; or a CSV time column.
/// Anything else is refused.
pub fn parse_wave(bytes: &[u8], name: &str) -> Result<Vec<WaveTrace>, String> {
    if bytes.len() > MAX_WAVEFORM_BYTES {
        return Err("waveform exceeds the 128 MiB import limit".into());
    }
    if bytes.starts_with(b"OOSW") {
        return parse_oosw(bytes);
    }
    if looks_wfm(bytes) {
        return parse_wfm(bytes).map(|t| vec![t]);
    }
    if looks_isf(bytes) {
        return parse_isf(bytes);
    }
    if looks_wavedesc(bytes) {
        return parse_wavedesc(bytes);
    }
    if looks_ag(bytes) {
        return parse_ag(bytes);
    }
    if looks_tr0(bytes) {
        return parse_tr0(bytes);
    }
    if looks_wfmx(bytes) {
        return parse_wfmx(bytes);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return parse_wave_csv(text, name);
    }
    Err("that file is not a waveform".into())
}

pub(super) fn looks_wfm(bytes: &[u8]) -> bool {
    bytes.len() >= 10
        && (bytes[2..].starts_with(b":WFM#") || bytes[2..].starts_with(b"WFM#") || bytes.starts_with(b":WFM#"))
}

pub(super) fn parse_oosw(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    if b.len() < 36 || &b[..4] != b"OOSW" {
        return Err("not an OOS waveform".into());
    }
    let f = Fields::new(b, true);
    let ver = f.u32(4)?;
    if ver != 1 {
        return Err(format!("OOS waveform version {ver}"));
    }
    let nch = f.u32(8)? as usize;
    let n = usize::try_from(f.u64(12)?).map_err(|_| "OOS waveform is too large".to_string())?;
    let (dt, t0) = (f.f64(20)?, f.f64(28)?);
    if n > MAX_WAVEFORM_SAMPLES {
        return Err("waveform exceeds the 32-million-sample import limit".into());
    }
    if nch == 0 || nch > 8 || !oosw_timing_ok(n, dt, t0) {
        return Err("that file holds no waveform".into());
    }
    let need = n
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(4))
        .and_then(|bytes| bytes.checked_mul(nch))
        .and_then(|bytes| bytes.checked_add(36))
        .ok_or("OOS waveform is too large")?;
    if b.len() < need {
        return Err("truncated OOS waveform".into());
    }
    let mut out = Vec::with_capacity(nch);
    let mut p = 36;
    for _ in 0..nch {
        let ch = f.u32(p)?;
        p += 4;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(f.f32(p)?);
            p += 4;
        }
        out.push(WaveTrace { t0, dt, v, label: format!("CH{ch}") });
    }
    Ok(out)
}

/// A time column, then one trace per further column. The columns are kept as the samples they become, and a line wider
/// than the reference slots ends the parse: a single line of many columns no longer makes as many traces.
pub(super) fn parse_wave_csv(text: &str, name: &str) -> Result<Vec<WaveTrace>, String> {
    let mut columns: Vec<Vec<f32>> = Vec::new();
    // a stretch of lines whose time does not go back, as (first time, last time, lines): the one being read, and the
    // longest of those before it that advance. A file of one sweep is one stretch
    let mut stretch: (f64, f64, usize) = (f64::NAN, f64::NAN, 0);
    let mut longest = stretch;
    let mut first = f64::NAN;
    let mut rows = 0usize;
    for line in text.lines() {
        let mut cols = line.split(',').map(|s| s.trim().parse::<f64>().ok());
        let Some(Some(t)) = cols.next() else { continue };
        let mut rest = cols.map(|x| x.unwrap_or(f64::NAN)).peekable();
        if rest.peek().is_none() {
            continue;
        }
        if rows >= MAX_WAVEFORM_SAMPLES {
            return Err("waveform exceeds the 32-million-sample import limit".into());
        }
        let mut k = 0;
        for x in rest {
            if k >= NREF {
                return Err("The file has more traces than the remaining reference slots".into());
            }
            if k == columns.len() {
                // a column that starts on a later line: the lines before it have no sample there
                columns.push(vec![f32::NAN; rows]);
            }
            columns[k].push(x as f32);
            k += 1;
        }
        for c in &mut columns[k..] {
            c.push(f32::NAN);
        }
        if rows == 0 {
            first = t;
        }
        if stretch.2 > 0 && t < stretch.1 {
            // the time goes back: the next sweep or segment of the file begins
            if stretch.1 > stretch.0 && stretch.2 > longest.2 {
                longest = stretch;
            }
            stretch = (t, t, 1);
        } else {
            stretch = (if stretch.2 == 0 { t } else { stretch.0 }, t, stretch.2 + 1);
        }
        rows += 1;
    }
    if stretch.1 > stretch.0 && stretch.2 > longest.2 {
        longest = stretch;
    }
    // the first time of a stretch and its last, over the steps between them. Not the first step alone: a file that
    // rounds its times to a few digits has a first step that is off by a percent, or no step at all between its first
    // two lines. Not the first time of the file and its last either: in a file of several sweeps the time starts
    // again, and their distance is not that of the lines between them
    let dt = match longest {
        (a, b, lines) if lines >= 2 => (b - a) / (lines - 1) as f64,
        _ => f64::NAN,
    };
    // the time base the binary readers ask for: a time of inf gives an infinite step or start
    if !oosw_timing_ok(rows, dt, first) {
        return Err("that file holds no waveform".into());
    }
    let ntrace = columns.len();
    Ok(columns
        .into_iter()
        .enumerate()
        .map(|(k, v)| {
            let label = if ntrace == 1 { name.into() } else { format!("CH{}", k + 1) };
            WaveTrace { t0: first, dt, v, label }
        })
        .collect())
}

/// Tek reference waveform version 001/002/003, YT vector, first frame.
/// Version 001 uses an 88-byte reference block, 156-byte explicit dimensions,
/// and 132-byte implicit dimensions. Later versions use 90/160/136 bytes.
/// See Tektronix Reference Waveform File Format (077-0220-10/11).
pub(super) fn parse_wfm(b: &[u8]) -> Result<WaveTrace, String> {
    if b.len() < 78 {
        return Err("truncated WFM".into());
    }
    let le = match Fields::new(b, true).u16(0)? {
        0x0F0F => true,
        0xF0F0 => false,
        _ => return Err("header is not a Tek WFM version 001, 002 or 003".into()),
    };
    let f = Fields::new(b, le);
    let ver = &b[2..10];
    if ver != b":WFM#001" && ver != b":WFM#002" && ver != b":WFM#003" {
        return Err("header is not a Tek WFM version 001, 002 or 003".into());
    }
    let bpp = b[15] as usize;
    if bpp == 0 || bpp > 8 {
        return Err("WFM sample size".into());
    }
    let curve = usize::try_from(f.i32(16)?).map_err(|_| "WFM curve offset is negative")?;
    let label = b[40..72].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect::<String>().trim().to_string();
    let (reference_size, explicit_size, implicit_size) =
        if ver == b":WFM#001" { (88, 156, 132) } else { (90, 160, 136) };
    // Packed header follows the common 78-byte static block.
    let mut i = 78usize;
    let need = |n: usize| -> Result<(), String> {
        if b.len() < n {
            Err("truncated WFM".into())
        } else {
            Ok(())
        }
    };
    need(i + reference_size)?;
    i += reference_size;
    need(i + explicit_size)?;
    let vscale = f.f64(i)?;
    let voff = f.f64(i + 8)?;
    let fmt = f.u32(i + 72)?; // after scale, offset, size, units, 4 doubles
    i += explicit_size;
    need(i + explicit_size)?;
    i += explicit_size;
    need(i + implicit_size)?;
    let dt = f.f64(i)?;
    let t0 = f.f64(i + 8)?;
    i += implicit_size;
    need(i + implicit_size + 12 + 12 + 24 + 30)?;
    i += implicit_size + 12 + 12 + 24;
    let data_start = f.u32(i + 14)? as usize;
    let post_start = f.u32(i + 18)? as usize;
    if !dt.is_finite() || dt <= 0.0 || !t0.is_finite() || !vscale.is_finite() || !voff.is_finite() {
        return Err("WFM time and voltage scaling must be finite with a positive time step".into());
    }
    if curve < i + 30 || data_start > post_start {
        return Err("WFM curve is empty".into());
    }
    let expected_bpp = match fmt {
        0 => 2,
        1 | 2 | 4 => 4,
        3 | 5 => 8,
        6 | 7 => 1,
        _ => return Err("WFM sample format".into()),
    };
    if bpp != expected_bpp {
        return Err("WFM sample format does not match bytes per point".into());
    }
    let start = curve.checked_add(data_start).ok_or("WFM curve offset overflow")?;
    let stop = curve.checked_add(post_start).ok_or("WFM curve offset overflow")?;
    if stop > b.len().saturating_sub(8) {
        return Err("truncated WFM curve".into());
    }
    if stop <= start || (stop - start) % bpp != 0 {
        return Err("WFM curve is empty".into());
    }
    let n = (stop - start) / bpp;
    if n < 2 {
        return Err("WFM curve is empty".into());
    }
    if n > MAX_WAVEFORM_SAMPLES {
        return Err("waveform exceeds the 32-million-sample import limit".into());
    }
    let mut v = Vec::with_capacity(n);
    for k in 0..n {
        let o = start + k * bpp;
        let code = match fmt {
            0 if bpp >= 2 => f.i16(o)? as f64,
            1 if bpp >= 4 => f.i32(o)? as f64,
            2 => f.u32(o)? as f64,
            3 => f.u64(o)? as f64,
            4 if bpp >= 4 => f.f32(o)? as f64,
            5 if bpp >= 8 => f.f64(o)?,
            6 if bpp >= 1 => b[o] as f64,
            7 if bpp >= 1 => b[o] as i8 as f64,
            _ => return Err("WFM sample format".into()),
        };
        v.push((code * vscale + voff) as f32);
    }
    Ok(WaveTrace { t0, dt, v, label })
}

pub(super) fn looks_isf(b: &[u8]) -> bool {
    let s = if b.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
    b.get(s..).is_some_and(|x| x.starts_with(b":WFMPRE:") || x.starts_with(b"WFMPRE:"))
}

/// Preamble `:WFMPRE:` and a `:CURVE` block. `Y = (code - YOFF) * YMULT + YZERO`, `t = XZERO + (i - PT_OFF) * XINCR`.
pub(super) fn parse_isf(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    let s = if b.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
    let curve = b[s..].windows(6).position(|w| w == b":CURVE").map(|p| s + p).ok_or("that file holds no waveform")?;
    let mut kv = std::collections::HashMap::<String, String>::new();
    let pre = String::from_utf8_lossy(&b[s..curve]);
    for part in pre.split([';', '\n']) {
        let part = part.trim().trim_start_matches(':');
        let part = part.split_once(':').map(|(_, r)| r.trim()).unwrap_or(part);
        let mut it = part.split_whitespace();
        let Some(key) = it.next() else { continue };
        let val = it.next().unwrap_or("").trim_matches('"').to_string();
        kv.insert(key.to_ascii_uppercase(), val);
    }
    let num = |k: &str| kv.get(k).and_then(|s| s.parse::<f64>().ok());
    let numeric = |k: &str, default: f64| -> Result<f64, String> {
        match kv.get(k) {
            Some(value) => value.parse().map_err(|_| format!("invalid waveform {k}")),
            None => Ok(default),
        }
    };
    let bpp = numeric("BYT_NR", 1.0)?;
    if !matches!(bpp, 1.0 | 2.0 | 4.0) {
        return Err("that file holds no waveform".into());
    }
    let bpp = bpp as usize;
    let dt = num("XINCR").filter(|x| x.is_finite() && *x > 0.0).ok_or("that file holds no waveform")?;
    let xzero = numeric("XZERO", 0.0)?;
    let pt_off = numeric("PT_OFF", 0.0)?;
    let ymult = numeric("YMULT", 1.0)?;
    let yoff = numeric("YOFF", 0.0)?;
    let yzero = numeric("YZERO", 0.0)?;
    if ![ymult, yoff, yzero, xzero, pt_off].iter().all(|x| x.is_finite()) {
        return Err("that file holds no waveform".into());
    }
    let nr = match kv.get("NR_PT") {
        None => 0,
        Some(value) => value
            .parse::<usize>()
            .ok()
            .filter(|n| (2..=MAX_WAVEFORM_SAMPLES).contains(n))
            .ok_or("invalid waveform point count")?,
    };
    let msb = kv.get("BYT_OR").is_some_and(|s| s.eq_ignore_ascii_case("MSB"));
    let signed = !kv.get("BN_FMT").is_some_and(|s| s.eq_ignore_ascii_case("RP"));
    let ascii = kv.get("ENCDG").is_some_and(|s| s.eq_ignore_ascii_case("ASC"));
    let mut i = curve + 6;
    while b.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    let (raw, vals) = if b.get(i) == Some(&b'#') {
        i += 1;
        let n = b
            .get(i)
            .filter(|c| c.is_ascii_digit())
            .map(|c| (c - b'0') as usize)
            .ok_or("invalid waveform block length")?;
        if !(1..=9).contains(&n) || i + 1 + n > b.len() {
            return Err("truncated waveform".into());
        }
        let len: usize =
            std::str::from_utf8(&b[i + 1..i + 1 + n]).ok().and_then(|s| s.parse().ok()).ok_or("truncated waveform")?;
        let start = i + 1 + n;
        if len > MAX_WAVEFORM_BYTES || start.checked_add(len).is_none_or(|end| end > b.len()) {
            return Err(if len > MAX_WAVEFORM_BYTES { "waveform is too large" } else { "truncated waveform" }.into());
        }
        (start, len)
    } else {
        (i, b.len() - i)
    };
    let body = &b[raw..raw + vals];
    let mut codes = Vec::new();
    if ascii {
        let text = String::from_utf8_lossy(body);
        for tok in text.split(|c: char| c == ',' || c.is_whitespace()) {
            if let Ok(x) = tok.parse::<f64>() {
                codes.push(x);
            }
        }
    } else {
        if vals % bpp != 0 {
            return Err("truncated waveform".into());
        }
        let n = vals / bpp;
        if n > MAX_WAVEFORM_SAMPLES {
            return Err("waveform is too large".into());
        }
        let f = Fields::new(b, !msb);
        for k in 0..n {
            let o = raw + k * bpp;
            let code = match bpp {
                1 => {
                    if signed {
                        b[o] as i8 as f64
                    } else {
                        b[o] as f64
                    }
                }
                2 => {
                    let u = f.u16(o)?;
                    if signed {
                        u as i16 as f64
                    } else {
                        u as f64
                    }
                }
                _ => {
                    let u = f.u32(o)?;
                    if signed {
                        u as i32 as f64
                    } else {
                        u as f64
                    }
                }
            };
            codes.push(code);
        }
    }
    let env = kv.get("PT_FMT").is_some_and(|s| s.eq_ignore_ascii_case("ENV"));
    if env && nr == 0 {
        return Err("an envelope needs a waveform point count".into());
    }
    let needed = if env { nr * 2 } else { nr };
    if codes.len() < needed {
        return Err("truncated waveform".into());
    }
    let t0 = xzero - pt_off * dt;
    let scale = |code: f64| ((code - yoff) * ymult + yzero) as f32;
    if env {
        let min: Vec<f32> = codes.iter().take(nr * 2).step_by(2).copied().map(scale).collect();
        let max: Vec<f32> = codes.iter().skip(1).take(nr * 2).step_by(2).copied().map(scale).collect();
        return Ok(vec![
            WaveTrace { t0, dt, v: min, label: "min".into() },
            WaveTrace { t0, dt, v: max, label: "max".into() },
        ]);
    }
    if nr >= 2 && codes.len() > nr {
        codes.truncate(nr);
    }
    if codes.len() < 2 {
        return Err("that file holds no waveform".into());
    }
    Ok(vec![WaveTrace {
        t0,
        dt,
        v: codes.into_iter().map(scale).collect(),
        label: kv.get("WFID").cloned().unwrap_or_default(),
    }])
}

pub(super) fn looks_wavedesc(b: &[u8]) -> bool {
    wavedesc_at(b).is_some_and(|i| b.len() >= i + 188 && matches!((b[i + 34], b[i + 35]), (1, 0) | (0, 0)))
}

pub(super) fn wavedesc_at(b: &[u8]) -> Option<usize> {
    let n = b.len().min(64);
    b[..n].windows(8).position(|w| w == b"WAVEDESC")
}

/// Fixed fields of a binary file in its byte order, read with a bounds check: a field past the end of the file is the
/// error "truncated waveform", not a panic.
#[derive(Clone, Copy)]
pub(crate) struct Fields<'a> {
    b: &'a [u8],
    le: bool,
}

impl<'a> Fields<'a> {
    /// `le`: little endian, else big endian.
    pub(crate) fn new(b: &'a [u8], le: bool) -> Self {
        Fields { b, le }
    }

    /// The `N` bytes at `at`, little endian first.
    fn bytes<const N: usize>(&self, at: usize) -> Result<[u8; N], String> {
        let field = at.checked_add(N).and_then(|end| self.b.get(at..end)).ok_or("truncated waveform")?;
        let mut a: [u8; N] = field.try_into().map_err(|_| "truncated waveform")?;
        if !self.le {
            a.reverse();
        }
        Ok(a)
    }

    pub(crate) fn u16(&self, at: usize) -> Result<u16, String> {
        self.bytes(at).map(u16::from_le_bytes)
    }

    pub(crate) fn i16(&self, at: usize) -> Result<i16, String> {
        self.bytes(at).map(i16::from_le_bytes)
    }

    pub(crate) fn u32(&self, at: usize) -> Result<u32, String> {
        self.bytes(at).map(u32::from_le_bytes)
    }

    pub(crate) fn i32(&self, at: usize) -> Result<i32, String> {
        self.bytes(at).map(i32::from_le_bytes)
    }

    pub(crate) fn u64(&self, at: usize) -> Result<u64, String> {
        self.bytes(at).map(u64::from_le_bytes)
    }

    pub(crate) fn f32(&self, at: usize) -> Result<f32, String> {
        self.bytes(at).map(f32::from_le_bytes)
    }

    pub(crate) fn f64(&self, at: usize) -> Result<f64, String> {
        self.bytes(at).map(f64::from_le_bytes)
    }
}

/// `WAVEDESC` block. `volts = code * gain - offset`, time of point 0 is the double at offset 180.
pub(super) fn parse_wavedesc(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    let i = wavedesc_at(b).ok_or("that file is not a waveform")?;
    if b.len() < i + 188 {
        return Err("truncated waveform".into());
    }
    let le = match (b[i + 34], b[i + 35]) {
        (1, 0) => true,
        (0, 0) => false,
        _ => return Err("that file is not a waveform".into()),
    };
    let f = Fields::new(b, le);
    let desc = f.u32(i + 36)? as usize;
    let user = f.u32(i + 40)? as usize;
    let trig = f.u32(i + 48)? as usize;
    let ris = f.u32(i + 52)? as usize;
    let nbytes = f.u32(i + 60)? as usize;
    let count = f.u32(i + 116)? as usize;
    if !(188..=8192).contains(&desc) || !(2..=MAX_WAVEFORM_SAMPLES).contains(&count) {
        return Err("that file holds no waveform".into());
    }
    let bpp = if count > 0 && nbytes.is_multiple_of(count) && matches!(nbytes / count, 1 | 2 | 4) {
        nbytes / count
    } else {
        match f.u16(i + 32)? {
            0 => 1,
            1 => 2,
            _ => return Err("that file holds no waveform".into()),
        }
    };
    let gain = f.f32(i + 156)? as f64;
    let off = f.f32(i + 160)? as f64;
    let dt = f.f32(i + 176)? as f64;
    let t0 = f.f64(i + 180)?;
    if !(dt.is_finite() && dt > 0.0 && gain.is_finite() && off.is_finite() && t0.is_finite()) {
        return Err("that file holds no waveform".into());
    }
    let start = i
        .checked_add(desc)
        .and_then(|x| x.checked_add(user))
        .and_then(|x| x.checked_add(trig))
        .and_then(|x| x.checked_add(ris))
        .ok_or("waveform is too large")?;
    let need = count.checked_mul(bpp).ok_or("waveform is too large")?;
    if start.checked_add(need).is_none_or(|end| end > b.len()) {
        return Err("truncated waveform".into());
    }
    let label = cstr(&b[i + 96..i + 112]);
    let mut v = Vec::with_capacity(count);
    for k in 0..count {
        let o = start + k * bpp;
        let code = match bpp {
            1 => b[o] as i8 as f64,
            2 => f.i16(o)? as f64,
            _ => f.f32(o)? as f64,
        };
        v.push((code * gain - off) as f32);
    }
    Ok(vec![WaveTrace { t0, dt, v, label }])
}

pub(super) fn looks_ag(b: &[u8]) -> bool {
    if b.len() < 12 + 140 || &b[..2] != b"AG" || !b[2].is_ascii_graphic() || !b[3].is_ascii_graphic() {
        return false;
    }
    let f = Fields::new(b, true);
    let (nwave, hsize) = (f.u32(8).unwrap_or(0), f.u32(12).unwrap_or(0));
    (1..=64).contains(&nwave) && (140..8192).contains(&hsize)
}

/// Cookie `AG`, then one header per waveform and a buffer of float32 samples. Little endian.
pub(super) fn parse_ag(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    if !looks_ag(b) {
        return Err("that file is not a waveform".into());
    }
    let f = Fields::new(b, true);
    let nwave = f.u32(8)? as usize;
    let mut p = 12usize;
    let mut out = Vec::with_capacity(nwave);
    for wi in 0..nwave {
        if p + 140 > b.len() {
            return Err("truncated waveform".into());
        }
        let hsize = f.u32(p)? as usize;
        if !(140..8192).contains(&hsize) || p + hsize > b.len() {
            return Err("truncated waveform".into());
        }
        let nbuf = f.u32(p + 8)? as usize;
        let npts = f.u32(p + 12)? as usize;
        let (dt, t0) = (f.f64(p + 32)?, f.f64(p + 40)?);
        let label = cstr(&b[p + 112..p + 128]);
        if !(1..=32).contains(&nbuf) || !(dt.is_finite() && dt > 0.0 && t0.is_finite()) {
            return Err("that file holds no waveform".into());
        }
        p += hsize;
        let mut normal: Option<Vec<f32>> = None;
        let mut other: Option<Vec<f32>> = None;
        for _ in 0..nbuf {
            if p + 12 > b.len() {
                return Err("truncated waveform".into());
            }
            let dsize = f.u32(p)? as usize;
            let kind = f.u16(p + 4)?;
            let bpp = f.u16(p + 6)? as usize;
            let bsize = f.u32(p + 8)? as usize;
            if dsize < 12 || p + dsize + bsize > b.len() {
                return Err("truncated waveform".into());
            }
            let data = p + dsize;
            if matches!(kind, 1..=3) && bpp == 4 && bsize >= 8 && bsize.is_multiple_of(4) {
                let n = (bsize / 4).min(if npts >= 2 { npts } else { bsize / 4 });
                if (2..=MAX_WAVEFORM_SAMPLES).contains(&n) {
                    let v = (0..n).map(|k| f.f32(data + k * 4)).collect::<Result<Vec<f32>, String>>()?;
                    if kind == 1 {
                        normal = Some(v);
                    } else if other.is_none() {
                        other = Some(v);
                    }
                }
            }
            p = data + bsize;
        }
        let v = normal.or(other).ok_or("that file holds no waveform")?;
        let label = if label.is_empty() { format!("CH{}", wi + 1) } else { label };
        out.push(WaveTrace { t0, dt, v, label });
    }
    if out.is_empty() {
        return Err("that file holds no waveform".into());
    }
    Ok(out)
}

pub(super) fn block_le(head: &[u8]) -> Option<bool> {
    if head.len() < 16 {
        return None;
    }
    let mark = |o: usize| match &head[o..o + 4] {
        b"\x04\x00\x00\x00" => Some(true),
        b"\x00\x00\x00\x04" => Some(false),
        _ => None,
    };
    mark(0).or_else(|| mark(8))
}

pub(super) fn looks_raw(b: &[u8]) -> bool {
    b.starts_with(b"Title:")
        && (b.windows(7).any(|w| w == b"Binary:") || b.windows(7).any(|w| w == b"Values:"))
        && b.windows(9).any(|w| w.eq_ignore_ascii_case(b"Plotname:"))
}

pub(super) fn looks_tr0(b: &[u8]) -> bool {
    if looks_raw(b) {
        return true;
    }
    let head = &b[..b.len().min(8192)];
    let marked = head.windows(4).any(|w| w == b"$&%#");
    let version = head.windows(4).any(|w| matches!(w, b"9601" | b"2001" | b"9007"));
    if !(marked && version) {
        return false;
    }
    block_le(b).is_some() || b.first().is_some_and(u8::is_ascii_digit)
}

/// `$&%#` post file (`9601` / `9007` float32, `2001` float64) or a `Title:` transient with `Binary:` or `Values:`.
pub(super) fn parse_tr0(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    if block_le(b).is_some() {
        return parse_tr0_blocks(b);
    }
    if looks_raw(b) {
        return parse_raw(b);
    }
    parse_tr0_ascii(b)
}

pub(super) fn post_width(text: &str) -> Result<usize, String> {
    if text.contains("2001") {
        Ok(8)
    } else if text.contains("9601") || text.contains("9007") {
        Ok(4)
    } else {
        Err("that file is not a waveform".into())
    }
}

pub(super) fn post_vars(text: &str) -> Result<(usize, Vec<String>), String> {
    let end = text.find("$&%#").ok_or("that file is not a waveform")?;
    // Name fields are 16 bytes and may end in spaces, so only a trailing newline is dropped.
    let head = text[..end].trim_end_matches(['\r', '\n', '\0']);
    let digits: String = head.chars().skip_while(|c| c.is_whitespace()).take(4).collect();
    if digits.len() != 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err("that file is not a waveform".into());
    }
    let nvars: usize = digits.parse().unwrap_or(0);
    if !(2..=64).contains(&nvars) {
        return Err("that file holds no waveform".into());
    }
    let names = if head.len() >= nvars * 16 {
        head.as_bytes()[head.len() - nvars * 16..]
            .chunks(16)
            .map(|c| String::from_utf8_lossy(c).trim().to_string())
            .collect()
    } else {
        (0..nvars).map(|k| if k == 0 { "time".into() } else { format!("CH{k}") }).collect()
    };
    Ok((nvars, names))
}

pub(super) fn traces_from_columns(
    cols: &[f64],
    nvars: usize,
    names: &[String],
    time_f32: bool,
) -> Result<Vec<WaveTrace>, String> {
    if !(2..=64).contains(&nvars) {
        return Err("invalid waveform column count".into());
    }
    let mut n = cols.len();
    if n > 0 && cols[n - 1].abs() >= 1e29 {
        n -= 1;
    }
    let body = if n >= nvars && n.is_multiple_of(nvars) {
        &cols[..n]
    } else if n > nvars && (n - 1).is_multiple_of(nvars) {
        &cols[1..n]
    } else {
        return Err("that file holds no waveform".into());
    };
    let rows = body.len() / nvars;
    if !(2..=MAX_WAVEFORM_SAMPLES).contains(&rows) {
        return Err(if rows < 2 { "that file holds no waveform" } else { "waveform is too large" }.into());
    }
    let t0 = body[0];
    let dt = if time_f32 { (body[(rows - 1) * nvars] - t0) / (rows - 1) as f64 } else { body[nvars] - t0 };
    if !(t0.is_finite() && dt.is_finite() && dt > 0.0) {
        return Err("that file holds no waveform".into());
    }
    let valid_time = |r: usize| {
        let t = body[r * nvars];
        if time_f32 {
            let expected = t0 + r as f64 * dt;
            let tolerance = (dt.abs() * 1e-5).max(t0.abs().max(expected.abs()) * f32::EPSILON as f64 * 2.0).max(1e-45);
            t.is_finite() && expected.is_finite() && (t - expected).abs() <= tolerance
        } else {
            uniform_time(t, t0, dt, r)
        }
    };
    if !(0..rows).all(valid_time) {
        return Err("nonuniform waveform time axis is not supported".into());
    }
    let mut out = Vec::with_capacity(nvars - 1);
    for k in 1..nvars {
        let v: Vec<f32> = (0..rows).map(|r| body[r * nvars + k] as f32).collect();
        let label = names.get(k).cloned().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("CH{k}"));
        out.push(WaveTrace { t0, dt, v, label });
    }
    Ok(out)
}

pub(super) fn parse_tr0_blocks(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    let mut pos = 0usize;
    let mut header = String::new();
    let mut data = Vec::new();
    let mut first = true;
    while pos + 16 <= b.len() {
        let le = block_le(&b[pos..pos + 16]).ok_or("that file is not a waveform")?;
        let n = Fields::new(b, le).u32(pos + 12)? as usize;
        if n > MAX_WAVEFORM_BYTES || pos + 16 + n > b.len() {
            return Err(if n > MAX_WAVEFORM_BYTES { "waveform is too large" } else { "truncated waveform" }.into());
        }
        let body = &b[pos + 16..pos + 16 + n];
        pos += 16 + n;
        if pos + 4 <= b.len() {
            pos += 4;
        }
        if first {
            header = String::from_utf8_lossy(body).into_owned();
            first = false;
        } else {
            data.extend_from_slice(body);
        }
    }
    if header.find("$&%#").is_none() {
        return Err("that file is not a waveform".into());
    }
    let width = post_width(&header)?;
    let (nvars, names) = post_vars(&header)?;
    if data.len() % width != 0 {
        return Err("truncated waveform".into());
    }
    let mut cols = Vec::with_capacity(data.len() / width);
    // the endian of the first block applies to the floats as well
    let f = Fields::new(&data, block_le(b) != Some(false));
    for k in 0..data.len() / width {
        let x = if width == 8 { f.f64(k * 8)? } else { f.f32(k * 4)? as f64 };
        if x.abs() >= 1e29 {
            break;
        }
        cols.push(x);
    }
    traces_from_columns(&cols, nvars, &names, width == 4)
}

pub(super) fn parse_tr0_ascii(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    let text = std::str::from_utf8(b).map_err(|_| "that file is not a waveform".to_string())?;
    let end = text.find("$&%#").ok_or("that file is not a waveform")?;
    let width_bytes = post_width(text)?;
    let (nvars, names) = post_vars(text)?;
    let rest = text[end + 4..].trim_start();
    let field = if width_bytes == 8 { 13 } else { 11 };
    let packed = rest
        .as_bytes()
        .get(..field)
        .and_then(|b| std::str::from_utf8(b).ok())
        .is_some_and(|s| !s.bytes().any(|b| b.is_ascii_whitespace()) && s.trim().parse::<f64>().is_ok());
    let mut cols = Vec::new();
    if packed {
        let bytes = rest.as_bytes();
        let mut i = 0;
        while i + field <= bytes.len() {
            let s = std::str::from_utf8(&bytes[i..i + field]).unwrap_or("").trim();
            if s.is_empty() {
                break;
            }
            let Ok(x) = s.parse::<f64>() else { break };
            if x.abs() >= 1e29 {
                break;
            }
            cols.push(x);
            i += field;
        }
    }
    if cols.len() < nvars * 2 {
        cols.clear();
        for tok in rest.split_whitespace() {
            let Ok(x) = tok.parse::<f64>() else { break };
            if x.abs() >= 1e29 {
                break;
            }
            cols.push(x);
        }
    }
    traces_from_columns(&cols, nvars, &names, false)
}

pub(super) fn parse_raw(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    let mark = b.windows(7).position(|w| w == b"Binary:" || w == b"Values:").ok_or("that file is not a waveform")?;
    let header = std::str::from_utf8(&b[..mark]).map_err(|_| "that file is not a waveform".to_string())?;
    let nvars = raw_field(header, "No. Variables:").ok_or("that file holds no waveform")?;
    let npts = raw_field(header, "No. Points:").unwrap_or(0);
    if !(2..=64).contains(&nvars) {
        return Err("that file holds no waveform".into());
    }
    let complex =
        header.to_ascii_lowercase().contains("flags: complex") || header.to_ascii_lowercase().contains("flags:complex");
    let names = raw_names(header, nvars);
    let binary = b[mark..].starts_with(b"Binary:");
    let mut cols = Vec::new();
    if binary {
        let mut i = mark + 7;
        if b.get(i) == Some(&b'\r') {
            i += 1;
        }
        if b.get(i) == Some(&b'\n') {
            i += 1;
        }
        let points = if npts >= 2 { npts } else { (b.len() - i) / (nvars * 8) };
        if !(2..=MAX_WAVEFORM_SAMPLES).contains(&points) {
            return Err(if points < 2 { "that file holds no waveform" } else { "waveform is too large" }.into());
        }
        if i + points * nvars * 8 > b.len() {
            return Err("truncated waveform".into());
        }
        for p in 0..points {
            for k in 0..nvars {
                let o = i + (p * nvars + k) * 8;
                let f = Fields::new(b, true);
                let x = if complex { f.f32(o)? as f64 } else { f.f64(o)? };
                cols.push(x);
            }
        }
    } else {
        let text = std::str::from_utf8(&b[mark + 7..]).map_err(|_| "that file is not a waveform".to_string())?;
        let nums: Vec<f64> = text
            .split(|c: char| !(c.is_ascii_digit() || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E'))
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        let stride = nvars + 1;
        let indexed = nums.len() >= stride * 2 && nums[0].abs() < 1e-9 && (nums[stride] - 1.0).abs() < 1e-6;
        if indexed {
            let rows = nums.len() / stride;
            for r in 0..rows {
                cols.extend_from_slice(&nums[r * stride + 1..r * stride + 1 + nvars]);
            }
        } else if nums.len() >= nvars * 2 && nums.len().is_multiple_of(nvars) {
            cols = nums;
        } else {
            return Err("that file holds no waveform".into());
        }
    }
    traces_from_columns(&cols, nvars, &names, false)
}

pub(super) fn raw_field(header: &str, key: &str) -> Option<usize> {
    let lower = header.to_ascii_lowercase();
    let i = lower.find(&key.to_ascii_lowercase())? + key.len();
    let rest = header.get(i..)?.trim_start();
    let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    n.parse().ok()
}

pub(super) fn raw_names(header: &str, nvars: usize) -> Vec<String> {
    let mut names = vec![String::new(); nvars];
    let Some(at) = header.to_ascii_lowercase().find("variables:") else { return names };
    for line in header[at + 9..].lines() {
        let mut it = line.split_whitespace();
        let Some(idx) = it.next().and_then(|s| s.parse::<usize>().ok()) else { continue };
        if let Some(name) = it.next() {
            if idx < nvars {
                names[idx] = name.to_string();
            }
        }
    }
    names
}

pub(super) fn looks_wfmx(b: &[u8]) -> bool {
    let s = if b.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
    let rest = b.get(s..).unwrap_or(b);
    if !rest.starts_with(b"<?xml") {
        return false;
    }
    let n = rest.len().min(8192);
    let t = &rest[..n];
    let has = |needle: &[u8]| t.windows(needle.len()).any(|w| w == needle);
    has(b"WaveformPatternInternal") || has(b"WaveformInternal")
}

pub(super) fn xml_text<'a>(hay: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let i = hay.find(&open)? + open.len();
    let j = hay[i..].find(&close)? + i;
    Some(hay[i..j].trim())
}

/// XML header `WaveformInternal` or `WaveformPatternInternal`, then float samples. `XOrg` is the first time, `XInc` the step.
pub(super) fn parse_wfmx(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    let s = if b.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
    let head_end = b[s..].windows(7).position(|w| w == b"</Data>").map(|p| s + p + 7).unwrap_or(b.len().min(s + 8192));
    let head = String::from_utf8_lossy(&b[s..head_end.min(b.len())]);
    if !(head.contains("WaveformInternal") || head.contains("WaveformPatternInternal")) {
        return Err("that file is not a waveform".into());
    }
    let points: usize = xml_text(&head, "Points").and_then(|s| s.parse().ok()).ok_or("that file holds no waveform")?;
    let t0: f64 = xml_text(&head, "XOrg").and_then(|s| s.parse().ok()).ok_or("that file holds no waveform")?;
    let dt: f64 = xml_text(&head, "XInc").and_then(|s| s.parse().ok()).ok_or("that file holds no waveform")?;
    if !(2..=MAX_WAVEFORM_SAMPLES).contains(&points) || !(dt.is_finite() && dt > 0.0 && t0.is_finite()) {
        return Err("that file holds no waveform".into());
    }
    let payload = wfmx_payload(b, points)?;
    let v = wfmx_volts(payload, points, t0, dt)?;
    let label = xml_text(&head, "SourceName").unwrap_or("").to_string();
    Ok(vec![WaveTrace { t0, dt, v, label }])
}

pub(super) fn wfmx_payload(b: &[u8], points: usize) -> Result<&[u8], String> {
    let lengths = [4usize, 8, 16].map(|width| points.checked_mul(width).ok_or("waveform is too large"));
    let [four, eight, sixteen] = lengths;
    let lengths = [four?, eight?, sixteen?];
    let fits = |p: &[u8]| lengths.contains(&p.len());
    if let Some(start) = b.windows(5).enumerate().find_map(|(i, w)| {
        (w == b"<Data" && b.get(i + 5).is_some_and(|c| *c == b'>' || c.is_ascii_whitespace())).then_some(i)
    }) {
        if let Some(gt) = b[start..].iter().position(|&c| c == b'>') {
            let from = start + gt + 1;
            for length in lengths {
                if let Some(end) = from.checked_add(length) {
                    if b.get(end..).is_some_and(|tail| tail.starts_with(b"</Data>")) {
                        return Ok(&b[from..end]);
                    }
                }
            }
        }
    }
    // Samples follow the header. An empty `<Data>` element means they start after
    // the closing root tag. Consume only the XML separator required by the exact
    // payload size; leading/trailing float bytes can themselves be whitespace.
    if let Some(i) = b.windows(7).position(|w| w == b"</Data>") {
        let mut p = i + 7;
        let mut root = p;
        while b.get(root).is_some_and(u8::is_ascii_whitespace) {
            root += 1;
        }
        if b.get(root) == Some(&b'<') && b.get(root + 1) == Some(&b'/') {
            if let Some(gt) = b[root..].iter().position(|&c| c == b'>') {
                p = root + gt + 1;
            }
        }
        let tail = &b[p..];
        // A single XML line ending is supported after the root. Never trim the
        // binary tail or skip an unbounded run of its possible first bytes. A
        // present separator is structural, never a replacement for missing data.
        for separator in [b"\r\n".as_slice(), b"\n".as_slice()] {
            if let Some(payload) = tail.strip_prefix(separator) {
                return if fits(payload) {
                    Ok(payload)
                } else {
                    Err("truncated or unsupported waveform payload".into())
                };
            }
        }
        if fits(tail) {
            return Ok(tail);
        }
    }
    Err("that file holds no waveform".into())
}

pub(super) fn wfmx_volts(payload: &[u8], points: usize, t0: f64, dt: f64) -> Result<Vec<f32>, String> {
    // the lengths are checked against `points` before each read: no field is past the end
    let le_f32 = |p: &[u8], be: bool| -> Vec<f32> {
        let f = Fields::new(p, !be);
        (0..p.len() / 4).filter_map(|k| f.f32(k * 4).ok()).collect()
    };
    let f64_at = |at: usize| Fields::new(payload, true).f64(at).unwrap_or(f64::NAN);
    let as_y = |ys: &[f32]| ys.iter().take(points).copied().collect::<Vec<_>>();
    if payload.len() == points * 4 {
        let v = le_f32(payload, false);
        if v.iter().filter(|x| x.is_finite()).count() * 2 >= v.len() {
            return Ok(v);
        }
        return Ok(le_f32(payload, true));
    }
    if payload.len() == points * 8 {
        let f32s = le_f32(payload, false);
        let xs: Vec<f64> = f32s[..points].iter().map(|x| *x as f64).collect();
        if x_matches_f32(&xs, t0, dt) {
            return Ok(as_y(&f32s[points..]));
        }
        let inter: Vec<f64> = f32s.iter().step_by(2).copied().map(|x| x as f64).collect();
        if x_matches_f32(&inter, t0, dt) {
            return Ok(f32s.iter().skip(1).step_by(2).copied().collect());
        }
        // Eight bytes per point could also be an implicit f64 Y array. Without
        // a declared layout it cannot be distinguished safely from malformed
        // f32 X/Y data. Never reinterpret a failed explicit time axis as volts.
        return Err("unsupported or ambiguous WFMX sample layout".into());
    }
    if payload.len() == points * 16 {
        let xs: Vec<f64> = (0..points).map(|k| f64_at(k * 8)).collect();
        if x_matches(&xs, t0, dt) {
            return Ok((0..points).map(|k| f64_at((points + k) * 8) as f32).collect());
        }
        let inter: Vec<f64> = (0..points).map(|k| f64_at(k * 16)).collect();
        if x_matches(&inter, t0, dt) {
            return Ok((0..points).map(|k| f64_at(k * 16 + 8) as f32).collect());
        }
    }
    Err("that file holds no waveform".into())
}

pub(super) fn x_matches_f32(xs: &[f64], t0: f64, dt: f64) -> bool {
    xs.len() >= 2
        && xs.iter().enumerate().all(|(i, &x)| {
            let expected = t0 + i as f64 * dt;
            let tolerance = (dt.abs() * 1e-5).max(expected.abs() * f32::EPSILON as f64).max(1e-45);
            x.is_finite() && expected.is_finite() && (x - expected).abs() <= tolerance
        })
}

pub(super) fn x_matches(xs: &[f64], t0: f64, dt: f64) -> bool {
    xs.len() >= 2 && xs.iter().enumerate().all(|(i, &x)| uniform_time(x, t0, dt, i))
}

/// A WaveTrace has one interval; adaptive/invalid time axes cannot be relabeled
/// uniform without moving their samples. Tolerance covers source float rounding.
pub(super) fn uniform_time(time: f64, t0: f64, dt: f64, index: usize) -> bool {
    let expected = t0 + index as f64 * dt;
    let tolerance = (dt.abs() * 1e-5).max(expected.abs() * f64::EPSILON * 8.0).max(1e-18);
    time.is_finite() && expected.is_finite() && (time - expected).abs() <= tolerance
}

pub(super) fn cstr(b: &[u8]) -> String {
    b.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect::<String>().trim().to_string()
}
