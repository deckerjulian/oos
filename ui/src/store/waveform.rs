// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! OOS's own waveform files: CSV of a record and of the logic analyser, the binary OOSW record and the OOSF
//! fast-frame file, the read of a waveform file with its limits, and mask files. A new format OOS writes goes
//! here; a reader for another instrument's files goes into import.rs.

use super::*;

/// CSV of the enabled channels of a record: `Time (s),CH1 (V),...`, one row per sample, time relative to the
/// trigger; samples without data (NaN) are empty fields.
pub fn wave_csv(rec: &Record, m: &Model) -> String {
    let chans: Vec<usize> = (0..NCH).filter(|&c| m.ch[c].on && !rec.ch[c].is_empty()).collect();
    let n = chans.iter().map(|&c| rec.ch[c].len()).max().unwrap_or(0);
    let mut s = String::with_capacity(n * (16 + 12 * chans.len()) + 64);
    s.push_str("Time (s)");
    for &c in &chans {
        s.push_str(&format!(",CH{} (V)", c + 1));
    }
    s.push('\n');
    for k in 0..n {
        s.push_str(&format!("{:.9e}", rec.t0 + k as f64 * rec.dt));
        for &c in &chans {
            s.push(',');
            if let Some(v) = rec.ch[c].get(k).filter(|v| v.is_finite()) {
                s.push_str(&v.to_string());
            }
        }
        s.push('\n');
    }
    s
}

/// Logic analyser record as CSV: the lines shown, one row at the record start and one for every change of them, so a
/// record of millions of samples stays small and nothing is lost (the levels hold until the next row). The last row is
/// the end of the record.
pub fn la_csv(la: &crate::ui::la::LaRec, lines: &[usize]) -> String {
    let mask = lines.iter().fold(0u16, |m, &n| m | 1 << n);
    let mut s = String::from("Time (s)");
    for n in lines {
        s.push_str(&format!(",D{}", n));
    }
    s.push('\n');
    let row = |s: &mut String, k: usize, w: u16| {
        s.push_str(&format!("{:.9e}", la.t0 + k as f64 * la.dt));
        for &n in lines {
            s.push_str(if w >> n & 1 == 1 { ",1" } else { ",0" });
        }
        s.push('\n');
    };
    let mut prev = None;
    for (k, &w) in la.words.iter().enumerate() {
        if prev != Some(w & mask) {
            row(&mut s, k, w);
            prev = Some(w & mask);
        }
    }
    if let (Some(&w), true) = (la.words.last(), la.words.len() > 1) {
        row(&mut s, la.words.len() - 1, w);
    }
    s
}

/// The record in a plain binary form of OOS, little endian:
/// "OOSW", u32 version 1, u32 number of channels, u64 samples per channel, f64 dt, f64 t0, then per channel its number
/// (u32, 1..4) and the samples as f32 volts.
pub fn wave_bin(rec: &Record, m: &Model) -> Vec<u8> {
    let chans: Vec<usize> = (0..NCH).filter(|&c| m.ch[c].on && !rec.ch[c].is_empty()).collect();
    let n = chans.iter().map(|&c| rec.ch[c].len()).max().unwrap_or(0);
    let mut b = Vec::with_capacity(36 + chans.len() * (4 + 4 * n));
    b.extend_from_slice(b"OOSW");
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&(chans.len() as u32).to_le_bytes());
    b.extend_from_slice(&(n as u64).to_le_bytes());
    b.extend_from_slice(&rec.dt.to_le_bytes());
    b.extend_from_slice(&rec.t0.to_le_bytes());
    for &c in &chans {
        b.extend_from_slice(&(c as u32 + 1).to_le_bytes());
        for i in 0..n {
            b.extend_from_slice(&rec.ch[c].get(i).copied().unwrap_or(f32::NAN).to_le_bytes());
        }
    }
    b
}

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

/// File systems of the kernel: their files can be regular and still endless (`/proc/kmsg`), or a device.
const NOT_STORAGE: [&str; 3] = ["/proc", "/sys", "/dev"];

/// The bytes of a waveform file named by a remote client (SCPI `:REFerence:LOAD`, also over VXI-11 and MCP) or found
/// beside the setups, read on the frame loop. Refused: a file of `NOT_STORAGE` (by the path it resolves to), and one
/// larger than the import limit, from its size before anything is read; the read stops one byte past the limit should
/// the file grow meanwhile. A path that is not there or not a regular file is `missing <name>`.
pub fn read_waveform_file(p: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let name = file_name(p);
    let missing = || format!("missing {name}");
    let real = p.canonicalize().map_err(|_| missing())?;
    if NOT_STORAGE.iter().any(|d| real.starts_with(d)) {
        return Err(format!("{name}: files under /proc, /sys and /dev are not read"));
    }
    let too_large = || format!("{name} exceeds the {} MiB import limit", MAX_WAVEFORM_BYTES >> 20);
    // before the open: opening a FIFO would wait for a writer
    let meta = std::fs::metadata(&real).map_err(|_| missing())?;
    if !meta.is_file() {
        return Err(missing());
    }
    if meta.len() > MAX_WAVEFORM_BYTES as u64 {
        return Err(too_large());
    }
    let file = std::fs::File::open(&real).map_err(|_| missing())?;
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(MAX_WAVEFORM_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| format!("{name}: {e}"))?;
    if bytes.len() > MAX_WAVEFORM_BYTES {
        return Err(too_large());
    }
    Ok(bytes)
}

/// Extensions shown by native and browser waveform import. The header chooses the parser.
pub const WAVE_EXTS: &[&str] = &["wfm", "bin", "csv", "trc", "isf", "tr0", "wfmx"];

/// One OOSW blob: the record and the number of bytes consumed.
pub fn parse_wave_bin(b: &[u8]) -> Result<(Record, usize), String> {
    if b.len() < 36 || &b[..4] != b"OOSW" {
        return Err("not an OOS waveform file".into());
    }
    let f = Fields::new(b, true);
    let ver = f.u32(4).unwrap_or(0);
    if ver != 1 {
        return Err(format!("waveform version {}", ver));
    }
    let nch = f.u32(8).unwrap_or(0) as usize;
    // the sample count comes from the file: bounded before it sizes anything, so a damaged or hostile count is an
    // error and not an overflow or an allocation the size of the count
    let n = usize::try_from(f.u64(12).unwrap_or(0)).unwrap_or(usize::MAX);
    let dt = f.f64(20).unwrap_or(0.0);
    let t0 = f.f64(28).unwrap_or(0.0);
    if nch == 0 || nch > NCH || dt <= 0.0 {
        return Err("waveform header".into());
    }
    if n > MAX_WAVEFORM_SAMPLES {
        return Err("waveform exceeds the 32-million-sample import limit".into());
    }
    let mut off = 36;
    let mut rec =
        Record { t0, dt, ch: Default::default(), triggered: true, partial: false, raw: None, la: None, pairs: false };
    for _ in 0..nch {
        let c = f.u32(off)? as usize;
        off += 4;
        if !(1..=NCH).contains(&c) {
            return Err(format!("channel {}", c));
        }
        if b.len().saturating_sub(off) < 4 * n {
            return Err("truncated waveform".into());
        }
        let mut v = vec![0f32; n];
        for i in 0..n {
            v[i] = f.f32(off + 4 * i).unwrap_or(f32::NAN);
        }
        rec.ch[c - 1] = v;
        off += 4 * n;
    }
    Ok((rec, off))
}

/// Fast-frame file: "OOSF", u32 version 1, u32 frame count, then that many OOSW blobs.
pub fn ff_bin(recs: &[Record], m: &Model) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"OOSF");
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&(recs.len() as u32).to_le_bytes());
    for rec in recs {
        b.extend_from_slice(&wave_bin(rec, m));
    }
    b
}

pub fn parse_ff(b: &[u8]) -> Result<Vec<Record>, String> {
    if b.len() >= 4 && &b[..4] == b"OOSW" {
        return parse_wave_bin(b).map(|(r, _)| vec![r]);
    }
    if b.len() < 12 || &b[..4] != b"OOSF" {
        return Err("not a fast-frame file".into());
    }
    let f = Fields::new(b, true);
    let ver = f.u32(4).unwrap_or(0);
    if ver != 1 {
        return Err(format!("fast-frame version {}", ver));
    }
    let n = f.u32(8).unwrap_or(0) as usize;
    // every frame is at least a header and one channel number: a count the bytes cannot hold is refused before it
    // sizes the list
    if n > (b.len() - 12) / 40 {
        return Err("truncated fast-frame file".into());
    }
    let mut off = 12;
    let mut recs = Vec::with_capacity(n);
    for _ in 0..n {
        let (r, used) = parse_wave_bin(b.get(off..).unwrap_or(&[]))?;
        recs.push(r);
        off += used;
    }
    Ok(recs)
}

/// Mask file: optional header (`# OOS mask` then `key=value` lines and `---`) then one volt per line.
pub struct MaskFile {
    pub src: Option<usize>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub inv: Option<bool>,
    pub v: Vec<f32>,
}

pub fn mask_text(src: usize, x: f32, y: f32, inv: bool, v: &[f32]) -> String {
    let mut s = String::from("# OOS mask\n");
    s.push_str(&format!("src={}\n", src + 1));
    s.push_str(&format!("x={}\n", x));
    s.push_str(&format!("y={}\n", y));
    s.push_str(&format!("inv={}\n", if inv { 1 } else { 0 }));
    s.push_str("---\n");
    for x in v {
        s.push_str(&format!("{:.9e}\n", x));
    }
    s
}

pub fn parse_mask(text: &str) -> Result<MaskFile, String> {
    let mut lines = text.lines().peekable();
    let mut src = None;
    let mut x = None;
    let mut y = None;
    let mut inv = None;
    if lines.peek().is_some_and(|l| l.starts_with("# OOS mask")) {
        lines.next();
        for line in lines.by_ref() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line == "---" {
                break;
            }
            let Some((k, v)) = line.split_once('=') else {
                return Err(format!("mask header: {}", line));
            };
            match k.trim() {
                "src" => {
                    let n: usize = v.trim().parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
                    if !(1..=NCH).contains(&n) {
                        return Err(format!("mask src {}", n));
                    }
                    src = Some(n - 1);
                }
                "x" => x = Some(v.trim().parse().map_err(|e: std::num::ParseFloatError| e.to_string())?),
                "y" => y = Some(v.trim().parse().map_err(|e: std::num::ParseFloatError| e.to_string())?),
                "inv" => inv = Some(matches!(v.trim(), "1" | "true" | "on")),
                _ => {}
            }
        }
    }
    let mut v = Vec::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        v.push(line.parse::<f32>().map_err(|e| e.to_string())?);
    }
    if v.is_empty() {
        return Err("empty mask".into());
    }
    Ok(MaskFile { src, x, y, inv, v })
}
