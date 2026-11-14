//! Waveform files into traces: CSV, OOS BIN and Tek WFM, plus the other headers a bench writes (ISF, WAVEDESC, AG,
//! TR0, RAW, WFMX). The header chooses the parser; anything else is refused.

/// One trace taken from a file: volts, the sample step, and the time of the first sample.
#[derive(Clone, Debug)]
pub struct WaveTrace {
    pub t0: f64,
    pub dt: f64,
    pub v: Vec<f32>,
    pub label: String,
}

/// Import bounds shared by native readers and remote uploads. They include the
/// supplied 80 MB / 20-million-point captures without unbounded allocations.
pub const MAX_WAVEFORM_BYTES: usize = 128 << 20;
pub const MAX_WAVEFORM_SAMPLES: usize = 32_000_000;

/// Extensions shown by native and browser waveform import. The header chooses the parser.
pub const WAVE_EXTS: &[&str] = &["wfm", "bin", "csv", "trc", "isf", "tr0", "wfmx"];

/// A stored waveform, recognized by its header:
/// `OOSW`; `:WFM#001`, `:WFM#002` or `:WFM#003`; `:WFMPRE:`; `WAVEDESC`; cookie `AG`;
/// a `$&%#` block (`9601`, `9007`, or `2001`) or a `Title:` / `Plotname:` transient (`Binary:` or `Values:`);
/// XML `WaveformInternal` or `WaveformPatternInternal`; or a CSV time column.
/// Anything else is refused.
pub fn parse_wave(bytes: &[u8], name: &str) -> Result<Vec<WaveTrace>, String> {
    if bytes.len() > MAX_WAVEFORM_BYTES { return Err("waveform exceeds the 128 MiB import limit".into()); }
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

fn looks_wfm(bytes: &[u8]) -> bool {
    bytes.len() >= 10 && (bytes[2..].starts_with(b":WFM#") || bytes[2..].starts_with(b"WFM#") || bytes.starts_with(b":WFM#"))
}

fn parse_oosw(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    if b.len() < 36 || &b[..4] != b"OOSW" {
        return Err("not an OOS waveform".into());
    }
    let ver = u32::from_le_bytes(b[4..8].try_into().unwrap());
    if ver != 1 {
        return Err(format!("OOS waveform version {ver}"));
    }
    let nch = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
    let n = usize::try_from(u64::from_le_bytes(b[12..20].try_into().unwrap())).map_err(|_| "OOS waveform is too large".to_string())?;
    let dt = f64::from_le_bytes(b[20..28].try_into().unwrap());
    let t0 = f64::from_le_bytes(b[28..36].try_into().unwrap());
    if n > MAX_WAVEFORM_SAMPLES { return Err("waveform exceeds the 32-million-sample import limit".into()); }
    if nch == 0 || nch > 8 || n < 2 || !dt.is_finite() || dt <= 0.0 || !t0.is_finite() {
        return Err("that file holds no waveform".into());
    }
    let need = n.checked_mul(4).and_then(|bytes| bytes.checked_add(4)).and_then(|bytes| bytes.checked_mul(nch)).and_then(|bytes| bytes.checked_add(36)).ok_or("OOS waveform is too large")?;
    if b.len() < need {
        return Err("truncated OOS waveform".into());
    }
    let mut out = Vec::with_capacity(nch);
    let mut p = 36;
    for _ in 0..nch {
        let ch = u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
        p += 4;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(f32::from_le_bytes(b[p..p + 4].try_into().unwrap()));
            p += 4;
        }
        out.push(WaveTrace { t0, dt, v, label: format!("CH{ch}") });
    }
    Ok(out)
}

fn parse_wave_csv(text: &str, name: &str) -> Result<Vec<WaveTrace>, String> {
    let mut rows: Vec<(f64, Vec<f64>)> = Vec::new();
    for line in text.lines() {
        let mut cols = line.split(',').map(|s| s.trim().parse::<f64>().ok());
        let Some(Some(t)) = cols.next() else { continue };
        let rest: Vec<f64> = cols.map(|x| x.unwrap_or(f64::NAN)).collect();
        if rest.is_empty() {
            continue;
        }
        if rows.len() >= MAX_WAVEFORM_SAMPLES { return Err("waveform exceeds the 32-million-sample import limit".into()); }
        rows.push((t, rest));
    }
    let dt = match rows.as_slice() {
        [a, b, ..] if b.0 > a.0 => b.0 - a.0,
        _ => return Err("that file holds no waveform".into()),
    };
    let ntrace = rows.iter().map(|r| r.1.len()).max().unwrap_or(0).max(1);
    let mut traces = Vec::with_capacity(ntrace);
    for k in 0..ntrace {
        let v: Vec<f32> = rows.iter().map(|r| r.1.get(k).copied().unwrap_or(f64::NAN) as f32).collect();
        let label = if ntrace == 1 { name.into() } else { format!("CH{}", k + 1) };
        traces.push(WaveTrace { t0: rows[0].0, dt, v, label });
    }
    Ok(traces)
}

/// Tek reference waveform version 001/002/003, YT vector, first frame.
/// Version 001 uses an 88-byte reference block, 156-byte explicit dimensions,
/// and 132-byte implicit dimensions. Later versions use 90/160/136 bytes.
/// See Tektronix Reference Waveform File Format (077-0220-10/11).
fn parse_wfm(b: &[u8]) -> Result<WaveTrace, String> {
    if b.len() < 78 {
        return Err("truncated WFM".into());
    }
    let le = match u16::from_le_bytes(b[0..2].try_into().unwrap()) {
        0x0F0F => true,
        0xF0F0 => false,
        _ => return Err("header is not a Tek WFM version 001, 002 or 003".into()),
    };
    let get_u32 = |o| {
        let v = u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        if le { v } else { v.swap_bytes() }
    };
    let get_i32 = |o| get_u32(o) as i32;
    let get_f64 = |o| {
        let mut raw = [0u8; 8];
        raw.copy_from_slice(&b[o..o + 8]);
        if !le {
            raw.reverse();
        }
        f64::from_le_bytes(raw)
    };
    let ver = &b[2..10];
    if ver != b":WFM#001" && ver != b":WFM#002" && ver != b":WFM#003" {
        return Err("header is not a Tek WFM version 001, 002 or 003".into());
    }
    let bpp = b[15] as usize;
    if bpp == 0 || bpp > 8 {
        return Err("WFM sample size".into());
    }
    let curve = usize::try_from(get_i32(16)).map_err(|_| "WFM curve offset is negative")?;
    let label = b[40..72].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect::<String>().trim().to_string();
    let (reference_size, explicit_size, implicit_size) = if ver == b":WFM#001" { (88, 156, 132) } else { (90, 160, 136) };
    // Packed header follows the common 78-byte static block.
    let mut i = 78usize;
    let need = |n: usize| -> Result<(), String> { if b.len() < n { Err("truncated WFM".into()) } else { Ok(()) } };
    need(i + reference_size)?;
    i += reference_size;
    need(i + explicit_size)?;
    let vscale = get_f64(i);
    let voff = get_f64(i + 8);
    let fmt = get_u32(i + 72); // after scale, offset, size, units, 4 doubles
    i += explicit_size;
    need(i + explicit_size)?;
    i += explicit_size;
    need(i + implicit_size)?;
    let dt = get_f64(i);
    let t0 = get_f64(i + 8);
    i += implicit_size;
    need(i + implicit_size + 12 + 12 + 24 + 30)?;
    i += implicit_size + 12 + 12 + 24;
    let data_start = get_u32(i + 14) as usize;
    let post_start = get_u32(i + 18) as usize;
    if !dt.is_finite() || dt <= 0.0 || !t0.is_finite() || !vscale.is_finite() || !voff.is_finite() {
        return Err("WFM time and voltage scaling must be finite with a positive time step".into());
    }
    if curve < i + 30 || data_start > post_start {
        return Err("WFM curve is empty".into());
    }
    let expected_bpp = match fmt { 0 => 2, 1 | 2 | 4 => 4, 3 | 5 => 8, 6 | 7 => 1, _ => return Err("WFM sample format".into()) };
    if bpp != expected_bpp { return Err("WFM sample format does not match bytes per point".into()); }
    let start = curve.checked_add(data_start).ok_or("WFM curve offset overflow")?;
    let stop = curve.checked_add(post_start).ok_or("WFM curve offset overflow")?;
    if stop > b.len().saturating_sub(8) { return Err("truncated WFM curve".into()); }
    if stop <= start || (stop - start) % bpp != 0 {
        return Err("WFM curve is empty".into());
    }
    let n = (stop - start) / bpp;
    if n < 2 {
        return Err("WFM curve is empty".into());
    }
    if n > MAX_WAVEFORM_SAMPLES { return Err("waveform exceeds the 32-million-sample import limit".into()); }
    let mut v = Vec::with_capacity(n);
    for k in 0..n {
        let o = start + k * bpp;
        let code = match fmt {
            0 if bpp >= 2 => {
                let s = i16::from_le_bytes(b[o..o + 2].try_into().unwrap());
                (if le { s } else { s.swap_bytes() }) as f64
            }
            1 if bpp >= 4 => {
                let s = i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
                (if le { s } else { s.swap_bytes() }) as f64
            }
            2 => get_u32(o) as f64,
            3 => {
                let mut raw: [u8; 8] = b[o..o + 8].try_into().unwrap();
                if !le { raw.reverse(); }
                u64::from_le_bytes(raw) as f64
            }
            4 if bpp >= 4 => {
                let mut raw = [0u8; 4];
                raw.copy_from_slice(&b[o..o + 4]);
                if !le {
                    raw.reverse();
                }
                f32::from_le_bytes(raw) as f64
            }
            5 if bpp >= 8 => get_f64(o),
            6 if bpp >= 1 => b[o] as f64,
            7 if bpp >= 1 => b[o] as i8 as f64,
            _ => return Err("WFM sample format".into()),
        };
        v.push((code * vscale + voff) as f32);
    }
    Ok(WaveTrace { t0, dt, v, label })
}

fn looks_isf(b: &[u8]) -> bool {
    let s = if b.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
    b.get(s..).is_some_and(|x| x.starts_with(b":WFMPRE:") || x.starts_with(b"WFMPRE:"))
}

/// Preamble `:WFMPRE:` and a `:CURVE` block. `Y = (code - YOFF) * YMULT + YZERO`, `t = XZERO + (i - PT_OFF) * XINCR`.
fn parse_isf(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    let s = if b.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
    let curve = b[s..].windows(6).position(|w| w == b":CURVE").map(|p| s + p).ok_or("that file holds no waveform")?;
    let mut kv = std::collections::HashMap::<String, String>::new();
    let pre = String::from_utf8_lossy(&b[s..curve]);
    for part in pre.split(|c| c == ';' || c == '\n') {
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
        Some(value) => value.parse::<usize>().ok().filter(|n| (2..=MAX_WAVEFORM_SAMPLES).contains(n))
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
        let n = b.get(i).filter(|c| c.is_ascii_digit()).map(|c| (c - b'0') as usize)
            .ok_or("invalid waveform block length")?;
        if !(1..=9).contains(&n) || i + 1 + n > b.len() {
            return Err("truncated waveform".into());
        }
        let len: usize = std::str::from_utf8(&b[i + 1..i + 1 + n]).ok().and_then(|s| s.parse().ok()).ok_or("truncated waveform")?;
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
        for k in 0..n {
            let o = raw + k * bpp;
            let code = match bpp {
                1 => if signed { b[o] as i8 as f64 } else { b[o] as f64 },
                2 => {
                    let u = u16::from_le_bytes(b[o..o + 2].try_into().unwrap());
                    let u = if msb { u.swap_bytes() } else { u };
                    if signed { u as i16 as f64 } else { u as f64 }
                }
                _ => {
                    let u = u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
                    let u = if msb { u.swap_bytes() } else { u };
                    if signed { u as i32 as f64 } else { u as f64 }
                }
            };
            codes.push(code);
        }
    }
    let env = kv.get("PT_FMT").is_some_and(|s| s.eq_ignore_ascii_case("ENV"));
    if env && nr == 0 { return Err("an envelope needs a waveform point count".into()); }
    let needed = if env { nr * 2 } else { nr };
    if codes.len() < needed { return Err("truncated waveform".into()); }
    let t0 = xzero - pt_off * dt;
    let scale = |code: f64| ((code - yoff) * ymult + yzero) as f32;
    if env {
        let min: Vec<f32> = codes.iter().take(nr * 2).step_by(2).copied().map(scale).collect();
        let max: Vec<f32> = codes.iter().skip(1).take(nr * 2).step_by(2).copied().map(scale).collect();
        return Ok(vec![WaveTrace { t0, dt, v: min, label: "min".into() }, WaveTrace { t0, dt, v: max, label: "max".into() }]);
    }
    if nr >= 2 && codes.len() > nr {
        codes.truncate(nr);
    }
    if codes.len() < 2 {
        return Err("that file holds no waveform".into());
    }
    Ok(vec![WaveTrace { t0, dt, v: codes.into_iter().map(scale).collect(), label: kv.get("WFID").cloned().unwrap_or_default() }])
}

fn looks_wavedesc(b: &[u8]) -> bool {
    wavedesc_at(b).is_some_and(|i| b.len() >= i + 188 && matches!((b[i + 34], b[i + 35]), (1, 0) | (0, 0)))
}

fn wavedesc_at(b: &[u8]) -> Option<usize> {
    let n = b.len().min(64);
    b[..n].windows(8).position(|w| w == b"WAVEDESC")
}

fn endian_u16(b: &[u8], o: usize, le: bool) -> u16 {
    let u = u16::from_le_bytes(b[o..o + 2].try_into().unwrap());
    if le { u } else { u.swap_bytes() }
}

fn endian_u32(b: &[u8], o: usize, le: bool) -> u32 {
    let u = u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    if le { u } else { u.swap_bytes() }
}

fn endian_f32(b: &[u8], o: usize, le: bool) -> f32 {
    let mut raw = [0u8; 4];
    raw.copy_from_slice(&b[o..o + 4]);
    if !le {
        raw.reverse();
    }
    f32::from_le_bytes(raw)
}

fn endian_f64(b: &[u8], o: usize, le: bool) -> f64 {
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&b[o..o + 8]);
    if !le {
        raw.reverse();
    }
    f64::from_le_bytes(raw)
}

/// `WAVEDESC` block. `volts = code * gain - offset`, time of point 0 is the double at offset 180.
fn parse_wavedesc(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    let i = wavedesc_at(b).ok_or("that file is not a waveform")?;
    if b.len() < i + 188 {
        return Err("truncated waveform".into());
    }
    let le = match (b[i + 34], b[i + 35]) {
        (1, 0) => true,
        (0, 0) => false,
        _ => return Err("that file is not a waveform".into()),
    };
    let desc = endian_u32(b, i + 36, le) as usize;
    let user = endian_u32(b, i + 40, le) as usize;
    let trig = endian_u32(b, i + 48, le) as usize;
    let ris = endian_u32(b, i + 52, le) as usize;
    let nbytes = endian_u32(b, i + 60, le) as usize;
    let count = endian_u32(b, i + 116, le) as usize;
    if desc < 188 || desc > 8192 || count < 2 || count > MAX_WAVEFORM_SAMPLES {
        return Err("that file holds no waveform".into());
    }
    let bpp = if count > 0 && nbytes % count == 0 && matches!(nbytes / count, 1 | 2 | 4) {
        nbytes / count
    } else {
        match endian_u16(b, i + 32, le) {
            0 => 1,
            1 => 2,
            _ => return Err("that file holds no waveform".into()),
        }
    };
    let gain = endian_f32(b, i + 156, le) as f64;
    let off = endian_f32(b, i + 160, le) as f64;
    let dt = endian_f32(b, i + 176, le) as f64;
    let t0 = endian_f64(b, i + 180, le);
    if !(dt.is_finite() && dt > 0.0 && gain.is_finite() && off.is_finite() && t0.is_finite()) {
        return Err("that file holds no waveform".into());
    }
    let start = i.checked_add(desc).and_then(|x| x.checked_add(user)).and_then(|x| x.checked_add(trig)).and_then(|x| x.checked_add(ris)).ok_or("waveform is too large")?;
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
            2 => endian_u16(b, o, le) as i16 as f64,
            _ => endian_f32(b, o, le) as f64,
        };
        v.push((code * gain - off) as f32);
    }
    Ok(vec![WaveTrace { t0, dt, v, label }])
}

fn looks_ag(b: &[u8]) -> bool {
    if b.len() < 12 + 140 || &b[..2] != b"AG" || !b[2].is_ascii_graphic() || !b[3].is_ascii_graphic() {
        return false;
    }
    let nwave = u32::from_le_bytes(b[8..12].try_into().unwrap());
    let hsize = u32::from_le_bytes(b[12..16].try_into().unwrap());
    (1..=64).contains(&nwave) && (140..8192).contains(&hsize)
}

/// Cookie `AG`, then one header per waveform and a buffer of float32 samples. Little endian.
fn parse_ag(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    if !looks_ag(b) {
        return Err("that file is not a waveform".into());
    }
    let nwave = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
    let mut p = 12usize;
    let mut out = Vec::with_capacity(nwave);
    for wi in 0..nwave {
        if p + 140 > b.len() {
            return Err("truncated waveform".into());
        }
        let hsize = u32::from_le_bytes(b[p..p + 4].try_into().unwrap()) as usize;
        if !(140..8192).contains(&hsize) || p + hsize > b.len() {
            return Err("truncated waveform".into());
        }
        let nbuf = u32::from_le_bytes(b[p + 8..p + 12].try_into().unwrap()) as usize;
        let npts = u32::from_le_bytes(b[p + 12..p + 16].try_into().unwrap()) as usize;
        let dt = f64::from_le_bytes(b[p + 32..p + 40].try_into().unwrap());
        let t0 = f64::from_le_bytes(b[p + 40..p + 48].try_into().unwrap());
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
            let dsize = u32::from_le_bytes(b[p..p + 4].try_into().unwrap()) as usize;
            let kind = u16::from_le_bytes(b[p + 4..p + 6].try_into().unwrap());
            let bpp = u16::from_le_bytes(b[p + 6..p + 8].try_into().unwrap()) as usize;
            let bsize = u32::from_le_bytes(b[p + 8..p + 12].try_into().unwrap()) as usize;
            if dsize < 12 || p + dsize + bsize > b.len() {
                return Err("truncated waveform".into());
            }
            let data = p + dsize;
            if matches!(kind, 1 | 2 | 3) && bpp == 4 && bsize >= 8 && bsize % 4 == 0 {
                let n = (bsize / 4).min(if npts >= 2 { npts } else { bsize / 4 });
                if n >= 2 && n <= MAX_WAVEFORM_SAMPLES {
                    let v: Vec<f32> = (0..n).map(|k| f32::from_le_bytes(b[data + k * 4..data + k * 4 + 4].try_into().unwrap())).collect();
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

fn block_le(head: &[u8]) -> Option<bool> {
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

fn looks_raw(b: &[u8]) -> bool {
    b.starts_with(b"Title:") && (b.windows(7).any(|w| w == b"Binary:") || b.windows(7).any(|w| w == b"Values:")) && b.windows(9).any(|w| w.eq_ignore_ascii_case(b"Plotname:"))
}

fn looks_tr0(b: &[u8]) -> bool {
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
fn parse_tr0(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    if block_le(b).is_some() {
        return parse_tr0_blocks(b);
    }
    if looks_raw(b) {
        return parse_raw(b);
    }
    parse_tr0_ascii(b)
}

fn post_width(text: &str) -> Result<usize, String> {
    if text.contains("2001") {
        Ok(8)
    } else if text.contains("9601") || text.contains("9007") {
        Ok(4)
    } else {
        Err("that file is not a waveform".into())
    }
}

fn post_vars(text: &str) -> Result<(usize, Vec<String>), String> {
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
        head.as_bytes()[head.len() - nvars * 16..].chunks(16).map(|c| String::from_utf8_lossy(c).trim().to_string()).collect()
    } else {
        (0..nvars).map(|k| if k == 0 { "time".into() } else { format!("CH{k}") }).collect()
    };
    Ok((nvars, names))
}

fn traces_from_columns(cols: &[f64], nvars: usize, names: &[String], time_f32: bool) -> Result<Vec<WaveTrace>, String> {
    if !(2..=64).contains(&nvars) { return Err("invalid waveform column count".into()); }
    let mut n = cols.len();
    if n > 0 && cols[n - 1].abs() >= 1e29 {
        n -= 1;
    }
    let body = if n >= nvars && n % nvars == 0 {
        &cols[..n]
    } else if n > nvars && (n - 1) % nvars == 0 {
        &cols[1..n]
    } else {
        return Err("that file holds no waveform".into());
    };
    let rows = body.len() / nvars;
    if rows < 2 || rows > MAX_WAVEFORM_SAMPLES {
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
        } else { uniform_time(t, t0, dt, r) }
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

fn parse_tr0_blocks(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    let mut pos = 0usize;
    let mut header = String::new();
    let mut data = Vec::new();
    let mut first = true;
    while pos + 16 <= b.len() {
        let le = block_le(&b[pos..pos + 16]).ok_or("that file is not a waveform")?;
        let n = endian_u32(b, pos + 12, le) as usize;
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
    for chunk in data.chunks(width) {
        let x = if width == 8 {
            f64::from_le_bytes(chunk.try_into().unwrap())
        } else {
            f32::from_le_bytes(chunk.try_into().unwrap()) as f64
        };
        if x.abs() >= 1e29 {
            break;
        }
        cols.push(x);
    }
    // The block endian applies to the floats as well. Re-read when the header was big endian.
    if block_le(b) == Some(false) {
        cols.clear();
        for chunk in data.chunks(width) {
            let x = if width == 8 { endian_f64(chunk, 0, false) } else { endian_f32(chunk, 0, false) as f64 };
            if x.abs() >= 1e29 {
                break;
            }
            cols.push(x);
        }
    }
    traces_from_columns(&cols, nvars, &names, width == 4)
}

fn parse_tr0_ascii(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    let text = std::str::from_utf8(b).map_err(|_| "that file is not a waveform".to_string())?;
    let end = text.find("$&%#").ok_or("that file is not a waveform")?;
    let width_bytes = post_width(text)?;
    let (nvars, names) = post_vars(text)?;
    let rest = text[end + 4..].trim_start();
    let field = if width_bytes == 8 { 13 } else { 11 };
    let packed = rest.as_bytes().get(..field).and_then(|b| std::str::from_utf8(b).ok())
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

fn parse_raw(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    let mark = b.windows(7).position(|w| w == b"Binary:" || w == b"Values:").ok_or("that file is not a waveform")?;
    let header = std::str::from_utf8(&b[..mark]).map_err(|_| "that file is not a waveform".to_string())?;
    let nvars = raw_field(header, "No. Variables:").ok_or("that file holds no waveform")?;
    let npts = raw_field(header, "No. Points:").unwrap_or(0);
    if !(2..=64).contains(&nvars) {
        return Err("that file holds no waveform".into());
    }
    let complex = header.to_ascii_lowercase().contains("flags: complex") || header.to_ascii_lowercase().contains("flags:complex");
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
        if points < 2 || points > MAX_WAVEFORM_SAMPLES {
            return Err(if points < 2 { "that file holds no waveform" } else { "waveform is too large" }.into());
        }
        if i + points * nvars * 8 > b.len() {
            return Err("truncated waveform".into());
        }
        for p in 0..points {
            for k in 0..nvars {
                let o = i + (p * nvars + k) * 8;
                let x = if complex { endian_f32(b, o, true) as f64 } else { endian_f64(b, o, true) };
                cols.push(x);
            }
        }
    } else {
        let text = std::str::from_utf8(&b[mark + 7..]).map_err(|_| "that file is not a waveform".to_string())?;
        let nums: Vec<f64> = text.split(|c: char| !(c.is_ascii_digit() || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E')).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
        let stride = nvars + 1;
        let indexed = nums.len() >= stride * 2 && nums[0].abs() < 1e-9 && (nums[stride] - 1.0).abs() < 1e-6;
        if indexed {
            let rows = nums.len() / stride;
            for r in 0..rows {
                cols.extend_from_slice(&nums[r * stride + 1..r * stride + 1 + nvars]);
            }
        } else if nums.len() >= nvars * 2 && nums.len() % nvars == 0 {
            cols = nums;
        } else {
            return Err("that file holds no waveform".into());
        }
    }
    traces_from_columns(&cols, nvars, &names, false)
}

fn raw_field(header: &str, key: &str) -> Option<usize> {
    let lower = header.to_ascii_lowercase();
    let i = lower.find(&key.to_ascii_lowercase())? + key.len();
    let rest = header.get(i..)?.trim_start();
    let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    n.parse().ok()
}

fn raw_names(header: &str, nvars: usize) -> Vec<String> {
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

fn looks_wfmx(b: &[u8]) -> bool {
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

fn xml_text<'a>(hay: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let i = hay.find(&open)? + open.len();
    let j = hay[i..].find(&close)? + i;
    Some(hay[i..j].trim())
}

/// XML header `WaveformInternal` or `WaveformPatternInternal`, then float samples. `XOrg` is the first time, `XInc` the step.
fn parse_wfmx(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    let s = if b.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
    let head_end = b[s..].windows(7).position(|w| w == b"</Data>").map(|p| s + p + 7).unwrap_or(b.len().min(s + 8192));
    let head = String::from_utf8_lossy(&b[s..head_end.min(b.len())]);
    if !(head.contains("WaveformInternal") || head.contains("WaveformPatternInternal")) {
        return Err("that file is not a waveform".into());
    }
    let points: usize = xml_text(&head, "Points").and_then(|s| s.parse().ok()).ok_or("that file holds no waveform")?;
    let t0: f64 = xml_text(&head, "XOrg").and_then(|s| s.parse().ok()).ok_or("that file holds no waveform")?;
    let dt: f64 = xml_text(&head, "XInc").and_then(|s| s.parse().ok()).ok_or("that file holds no waveform")?;
    if points < 2 || points > MAX_WAVEFORM_SAMPLES || !(dt.is_finite() && dt > 0.0 && t0.is_finite()) {
        return Err("that file holds no waveform".into());
    }
    let payload = wfmx_payload(b, points)?;
    let v = wfmx_volts(payload, points, t0, dt)?;
    let label = xml_text(&head, "SourceName").unwrap_or("").to_string();
    Ok(vec![WaveTrace { t0, dt, v, label }])
}

fn wfmx_payload(b: &[u8], points: usize) -> Result<&[u8], String> {
    let lengths = [4usize, 8, 16].map(|width| points.checked_mul(width).ok_or("waveform is too large"));
    let [four, eight, sixteen] = lengths;
    let lengths = [four?, eight?, sixteen?];
    let fits = |p: &[u8]| lengths.contains(&p.len());
    if let Some(start) = b.windows(5).enumerate().find_map(|(i, w)|
        (w == b"<Data" && b.get(i + 5).is_some_and(|c| *c == b'>' || c.is_ascii_whitespace())).then_some(i)) {
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
                return if fits(payload) { Ok(payload) } else { Err("truncated or unsupported waveform payload".into()) };
            }
        }
        if fits(tail) {
            return Ok(tail);
        }
    }
    Err("that file holds no waveform".into())
}

fn wfmx_volts(payload: &[u8], points: usize, t0: f64, dt: f64) -> Result<Vec<f32>, String> {
    let le_f32 = |p: &[u8], be: bool| -> Vec<f32> { (0..p.len() / 4).map(|k| endian_f32(p, k * 4, !be)).collect() };
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
        let xs: Vec<f64> = (0..points).map(|k| endian_f64(payload, k * 8, true)).collect();
        if x_matches(&xs, t0, dt) {
            return Ok((0..points).map(|k| endian_f64(payload, (points + k) * 8, true) as f32).collect());
        }
        let inter: Vec<f64> = (0..points).map(|k| endian_f64(payload, k * 16, true)).collect();
        if x_matches(&inter, t0, dt) {
            return Ok((0..points).map(|k| endian_f64(payload, k * 16 + 8, true) as f32).collect());
        }
    }
    Err("that file holds no waveform".into())
}

fn x_matches_f32(xs: &[f64], t0: f64, dt: f64) -> bool {
    xs.len() >= 2 && xs.iter().enumerate().all(|(i, &x)| {
        let expected = t0 + i as f64 * dt;
        let tolerance = (dt.abs() * 1e-5).max(expected.abs() * f32::EPSILON as f64).max(1e-45);
        x.is_finite() && expected.is_finite() && (x - expected).abs() <= tolerance
    })
}

fn x_matches(xs: &[f64], t0: f64, dt: f64) -> bool {
    xs.len() >= 2 && xs.iter().enumerate().all(|(i, &x)| uniform_time(x, t0, dt, i))
}

/// A WaveTrace has one interval; adaptive/invalid time axes cannot be relabeled
/// uniform without moving their samples. Tolerance covers source float rounding.
fn uniform_time(time: f64, t0: f64, dt: f64, index: usize) -> bool {
    let expected = t0 + index as f64 * dt;
    let tolerance = (dt.abs() * 1e-5).max(expected.abs() * f64::EPSILON * 8.0).max(1e-18);
    time.is_finite() && expected.is_finite() && (time - expected).abs() <= tolerance
}

fn cstr(b: &[u8]) -> String {
    b.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect::<String>().trim().to_string()
}

#[cfg(test)]
mod header_import_tests {
    use super::*;
    #[test]
    fn other_waveform_headers_load_and_unknown_bytes_are_refused() {
        let samples = [0.0_f32, 1.0, -0.5, 0.25];
        let isf = isf_i16(0.0, 1e-9, 1e-3, &[0, 1000, -500, 250]);
        let got = parse_wave(&isf, "a.isf").unwrap();
        assert_close(&got[0], 0.0, 1e-9, &samples);

        let trc = wavedesc_i16(0.0, 1e-9, 1e-3, 0.0, &[0, 1000, -500, 250]);
        assert_close(&parse_wave(&trc, "a.trc").unwrap()[0], 0.0, 1e-9, &samples);

        let ag = ag_f32(-1e-6, 2e-9, &samples, "lane");
        let got = parse_wave(&ag, "a.bin").unwrap();
        assert_eq!(got[0].label, "lane");
        assert_close(&got[0], -1e-6, 2e-9, &samples);

        let tr0 = tr0_f32(0.0, 1e-9, &samples);
        assert_close(&parse_wave(&tr0, "a.tr0").unwrap()[0], 0.0, 1e-9, &samples);
        let mut ascii = String::from("00020000000000009601*x ");
        for name in ["TIME", "v(1)"] {
            ascii.push_str(&format!("{name:<16}"));
        }
        ascii.push_str("$&%#");
        for (k, x) in samples.iter().enumerate() {
            ascii.push_str(&format!("{:11.4e}{:11.4e}", k as f64 * 1e-9, x));
        }
        ascii.push_str(&format!("{:11.4e}", 1.0e30));
        assert_close(&parse_wave(ascii.as_bytes(), "c.tr0").unwrap()[0], 0.0, 1e-9, &samples);
        let raw = raw_f64(0.0, 1e-9, &samples);
        assert_close(&parse_wave(&raw, "b.tr0").unwrap()[0], 0.0, 1e-9, &samples);

        let wfmx = wfmx_f32(-1e-6, 1e-9, &samples);
        assert_close(&parse_wave(&wfmx, "a.wfmx").unwrap()[0], -1e-6, 1e-9, &samples);

        assert!(parse_wave(b"\x00\x01\x02\x03\xff", "no.bin").is_err());
        assert!(parse_wave(b"not a waveform\n", "no.csv").unwrap_err().contains("no waveform"));
        assert!(parse_wave(b"AGxx,1,2\n3,4,5\n", "no.bin").is_err());
    }

    #[test]
    fn header_import_isf_block_digits_and_point_counts_are_checked() {
        for byte in 0..=u8::MAX {
            let mut b = b":WFMPRE:BYT_NR 1;NR_PT 2;XINCR 1e-9;:CURVE #".to_vec();
            b.push(byte);
            b.extend_from_slice(b"000000000");
            assert!(parse_wave(&b, "bad.isf").is_err(), "block byte {byte}");
        }
        for field in ["NR_PT -1", "NR_PT 2.5", "NR_PT 18446744073709551615", "BYT_NR 1.5", "YMULT wrong"] {
            let b = format!(":WFMPRE:BYT_NR 1;XINCR 1e-9;{field};:CURVE #12\0\x01");
            assert!(parse_wave(b.as_bytes(), "bad.isf").is_err(), "{field}");
        }
        assert!(parse_wave(b":WFMPRE:BYT_NR 1;NR_PT 3;XINCR 1;:CURVE #12\0\x01", "short.isf")
            .unwrap_err().contains("truncated"));
    }

    #[test]
    fn header_import_isf_envelope_preserves_pairs_and_requires_complete_data() {
        let header = b":WFMPRE:BYT_NR 1;NR_PT 2;PT_FMT ENV;XINCR 1;:CURVE #14";
        let mut b = header.to_vec();
        b.extend([-2i8 as u8, 2, -1i8 as u8, 1]);
        let traces = parse_wave(&b, "envelope.isf").unwrap();
        assert_eq!(traces.len(), 2);
        assert_eq!(traces[0].v, [-2.0, -1.0]);
        assert_eq!(traces[1].v, [2.0, 1.0]);
        let short = b":WFMPRE:BYT_NR 1;NR_PT 2;PT_FMT ENV;XINCR 1;:CURVE #13\0\x01\x02";
        assert!(parse_wave(short, "short.isf").unwrap_err().contains("truncated"));
    }

    #[test]
    fn header_import_fixed_width_unicode_is_safe_and_invalid_data_is_refused() {
        let text = format!("0002{}x$&%#", "é".repeat(16));
        let (nvars, names) = post_vars(&text).unwrap();
        assert_eq!((nvars, names.len()), (2, 2));
        let text = format!("00020000000000009601*x $&%#{}", "é".repeat(16));
        assert!(parse_wave(text.as_bytes(), "bad.tr0").is_err());
    }

    #[test]
    fn header_import_transient_requires_every_time_sample_to_be_uniform() {
        for time in [3.0, 1.0, f64::NAN, f64::INFINITY] {
            let mut b = raw_f64(0.0, 1.0, &[10.0, 20.0, 30.0, 40.0]);
            let start = b.len() - 4 * 16;
            b[start + 2 * 16..start + 2 * 16 + 8].copy_from_slice(&time.to_le_bytes());
            assert!(parse_wave(&b, "adaptive.tr0").unwrap_err().contains("nonuniform"));
        }
        let values = [10.0, 20.0, 30.0, 40.0];
        let b = raw_f64(-2e-6, 2e-9, &values);
        assert_close(&parse_wave(&b, "regular.tr0").unwrap()[0], -2e-6, 2e-9, &values);
    }

    #[test]
    fn header_import_wfmx_preserves_whitespace_bytes_at_binary_boundaries() {
        for first in [0x3f800020, 0x3f80000a, 0x3f80000d] {
            for last in [0x0a800000, 0x0d800000] {
                let values = [f32::from_bits(first), f32::from_bits(last)];
                let b = wfmx_f32(0.0, 1.0, &values);
                let trace = parse_wave(&b, "bytes.wfmx").unwrap().remove(0);
                assert_eq!(trace.v.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), [first, last]);
                let mut crlf = b.clone();
                let separator = crlf.len() - 8 - 1;
                crlf.insert(separator, b'\r');
                assert_eq!(parse_wave(&crlf, "crlf.wfmx").unwrap()[0].v, values);
            }
        }
    }

    #[test]
    fn header_import_wfmx_embedded_payload_uses_exact_length_not_binary_tag_bytes() {
        let values = [f32::from_bits(0x3f800020), 2.0];
        let mut b = b"<?xml?><DataFile><FileType>WaveformInternal</FileType><Points>2</Points><XOrg>0</XOrg><XInc>1</XInc><Data>".to_vec();
        for value in values { b.extend(value.to_le_bytes()); }
        b.extend_from_slice(b"</Data></DataFile>");
        assert_eq!(parse_wave(&b, "embedded.wfmx").unwrap()[0].v, values);
        let mut b = b"<Data>".to_vec();
        let payload = b"</Data>123456789";
        assert_eq!(payload.len(), 16);
        b.extend_from_slice(payload);
        b.extend_from_slice(b"</Data>");
        assert_eq!(wfmx_payload(&b, 4).unwrap(), payload);
        assert!(wfmx_payload(&b, usize::MAX).is_err());
    }

    #[test]
    fn header_import_wfmx_separator_cannot_replace_a_missing_payload_byte() {
        let mut b = wfmx_f32(0.0, 1.0, &[1.0, 2.0]);
        b.pop();
        assert!(parse_wave(&b, "short.wfmx").unwrap_err().contains("truncated"));
    }

    #[test]
    fn header_import_wfmx_checks_the_whole_explicit_time_axis() {
        let mut payload = Vec::new();
        for value in [0.0f64, 1.0, 3.0, 10.0, 20.0, 30.0] { payload.extend(value.to_le_bytes()); }
        assert!(wfmx_volts(&payload, 3, 0.0, 1.0).is_err());
        payload[16..24].copy_from_slice(&2.0f64.to_le_bytes());
        assert_eq!(wfmx_volts(&payload, 3, 0.0, 1.0).unwrap(), [10.0, 20.0, 30.0]);
    }

    #[test]
    fn header_import_wfmx_f32_axis_accepts_quantization_without_guessing_other_layouts() {
        let n = 100_000;
        let t0 = -37e-6;
        let dt = 1e-9;
        let xs: Vec<f32> = (0..n).map(|i| (t0 + i as f64 * dt) as f32).collect();
        let ys: Vec<f32> = (0..n).map(|i| ((i % 37) as f32 - 18.0) / 19.0).collect();
        for interleaved in [false, true] {
            let mut payload = Vec::with_capacity(n * 8);
            if interleaved {
                for i in 0..n { payload.extend(xs[i].to_le_bytes()); payload.extend(ys[i].to_le_bytes()); }
            } else {
                for x in &xs { payload.extend(x.to_le_bytes()); }
                for y in &ys { payload.extend(y.to_le_bytes()); }
            }
            assert_eq!(wfmx_volts(&payload, n, t0, dt).unwrap(), ys);
            let at = if interleaved { (n - 1) * 8 } else { (n - 1) * 4 };
            payload[at..at + 4].copy_from_slice(&(xs[n - 1] + 10.0 * dt as f32).to_le_bytes());
            assert!(wfmx_volts(&payload, n, t0, dt).unwrap_err().contains("ambiguous"));
        }
        let implicit_f64: Vec<u8> = [1.0f64, 2.0, 3.0].into_iter().flat_map(f64::to_le_bytes).collect();
        assert!(wfmx_volts(&implicit_f64, 3, 0.0, 1.0).is_err());
    }

    #[test]
    fn header_import_long_tr0_f32_times_keep_source_precision_and_reject_adaptive_axis() {
        let values: Vec<f32> = (0..100_000).map(|i| (i % 7) as f32).collect();
        let b = tr0_f32(-37e-6, 1e-9, &values);
        let got = parse_wave(&b, "long.tr0").unwrap();
        assert_eq!(got[0].v, values);
        assert!((got[0].dt - 1e-9).abs() < 1e-16);
        let mut cols = Vec::new();
        for i in 0..1000 { cols.extend([(i as f64 * 1e-9) as f32 as f64, i as f64]); }
        cols[998 * 2] += 5e-9;
        assert!(traces_from_columns(&cols, 2, &[], true).unwrap_err().contains("nonuniform"));
    }

    #[test]
    fn header_import_known_formats_tolerate_all_truncation_boundaries_without_panicking() {
        let values = [0.0, 1.0, -0.5, 0.25];
        let fixtures = [
            isf_i16(0.0, 1e-9, 1e-3, &[0, 1000, -500, 250]),
            wavedesc_i16(0.0, 1e-9, 1e-3, 0.0, &[0, 1000, -500, 250]),
            ag_f32(0.0, 1e-9, &values, "lane"),
            tr0_f32(0.0, 1e-9, &values), raw_f64(0.0, 1e-9, &values), wfmx_f32(0.0, 1e-9, &values),
        ];
        for b in fixtures {
            for end in 0..b.len() { let _ = parse_wave(&b[..end], "header.bin"); }
            assert_eq!(parse_wave(&b, "header.bin").unwrap()[0].v.len(), 4);
        }
    }

    fn assert_close(t: &WaveTrace, t0: f64, dt: f64, v: &[f32]) {
        let tol = dt.abs() * 1e-5 + 1e-15;
        assert!((t.t0 - t0).abs() <= tol, "t0 {} {}", t.t0, t0);
        assert!((t.dt - dt).abs() <= tol, "dt {} {}", t.dt, dt);
        assert_eq!(t.v.len(), v.len());
        for (a, b) in t.v.iter().zip(v) {
            assert!((a - b).abs() < 1e-5, "{a} {b}");
        }
    }

    fn isf_i16(t0: f64, dt: f64, ymult: f64, codes: &[i16]) -> Vec<u8> {
        let mut curve = Vec::new();
        for c in codes {
            curve.extend_from_slice(&c.to_le_bytes());
        }
        let len = curve.len().to_string();
        let mut b = format!(":WFMPRE:BYT_NR 2;ENCDG BIN;BN_FMT RI;BYT_OR LSB;NR_PT {};PT_FMT Y;XINCR {dt:.9e};XZERO {t0:.9e};PT_OFF 0;YMULT {ymult:.9e};YOFF 0;YZERO 0;WFID \"lane\";:CURVE #{}{len}", codes.len(), len.len()).into_bytes();
        b.extend(curve);
        b
    }

    fn wavedesc_i16(t0: f64, dt: f64, gain: f32, off: f32, codes: &[i16]) -> Vec<u8> {
        let desc = 346usize;
        let mut b = vec![0u8; desc + codes.len() * 2];
        b[..8].copy_from_slice(b"WAVEDESC");
        b[32] = 1;
        b[34] = 1;
        b[36..40].copy_from_slice(&(desc as u32).to_le_bytes());
        b[60..64].copy_from_slice(&((codes.len() * 2) as u32).to_le_bytes());
        b[116..120].copy_from_slice(&(codes.len() as u32).to_le_bytes());
        b[96..98].copy_from_slice(b"C1");
        b[156..160].copy_from_slice(&gain.to_le_bytes());
        b[160..164].copy_from_slice(&off.to_le_bytes());
        b[176..180].copy_from_slice(&(dt as f32).to_le_bytes());
        b[180..188].copy_from_slice(&t0.to_le_bytes());
        for (k, c) in codes.iter().enumerate() {
            b[desc + k * 2..desc + k * 2 + 2].copy_from_slice(&c.to_le_bytes());
        }
        b
    }

    fn ag_f32(t0: f64, dt: f64, v: &[f32], label: &str) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"AG10");
        let size_at = b.len();
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        let mut h = vec![0u8; 140];
        h[..4].copy_from_slice(&140u32.to_le_bytes());
        h[4..8].copy_from_slice(&1u32.to_le_bytes());
        h[8..12].copy_from_slice(&1u32.to_le_bytes());
        h[12..16].copy_from_slice(&(v.len() as u32).to_le_bytes());
        h[32..40].copy_from_slice(&dt.to_le_bytes());
        h[40..48].copy_from_slice(&t0.to_le_bytes());
        h[112..112 + label.len()].copy_from_slice(label.as_bytes());
        b.extend(h);
        b.extend_from_slice(&12u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&((v.len() * 4) as u32).to_le_bytes());
        for x in v {
            b.extend_from_slice(&x.to_le_bytes());
        }
        let n = b.len() as u32;
        b[size_at..size_at + 4].copy_from_slice(&n.to_le_bytes());
        b
    }

    fn tr0_block(payload: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"\x04\x00\x00\x00\x00\x00\x00\x00\x04\x00\x00\x00");
        b.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        b.extend_from_slice(payload);
        b.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        b
    }

    fn tr0_f32(t0: f64, dt: f64, v: &[f32]) -> Vec<u8> {
        let mut header = String::from("00020000000000009601*x ");
        for name in ["TIME", "v(1)"] {
            header.push_str(&format!("{name:<16}"));
        }
        header.push_str("$&%#");
        let mut data = Vec::new();
        for (k, x) in v.iter().enumerate() {
            let t = (t0 + k as f64 * dt) as f32;
            data.extend_from_slice(&t.to_le_bytes());
            data.extend_from_slice(&x.to_le_bytes());
        }
        data.extend_from_slice(&1.0e30_f32.to_le_bytes());
        let mut b = tr0_block(header.as_bytes());
        b.extend(tr0_block(&data));
        b
    }

    fn raw_f64(t0: f64, dt: f64, v: &[f32]) -> Vec<u8> {
        let mut b = format!("Title: t\nDate: today\nPlotname: Transient Analysis\nFlags: real\nNo. Variables: 2\nNo. Points: {}\nVariables:\n\t0\ttime\ttime\n\t1\tv(1)\tvoltage\nBinary:\n", v.len()).into_bytes();
        for (k, x) in v.iter().enumerate() {
            b.extend_from_slice(&(t0 + k as f64 * dt).to_le_bytes());
            b.extend_from_slice(&(*x as f64).to_le_bytes());
        }
        b
    }

    fn wfmx_f32(t0: f64, dt: f64, v: &[f32]) -> Vec<u8> {
        let mut b = format!("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<DcaFile><Header><FileType>WaveformInternal</FileType></Header><Points>{}</Points><XOrg>{t0}</XOrg><XInc>{dt}</XInc><Data Lengths=\"{}, 1\"></Data></DcaFile>\n", v.len(), v.len()).into_bytes();
        for x in v {
            b.extend_from_slice(&x.to_le_bytes());
        }
        b
    }

}
