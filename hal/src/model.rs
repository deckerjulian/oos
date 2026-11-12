// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Model identity, hardware capabilities and limits of the units of this build's board (docs/models.md); the data of
//! a board is in its `crate::target::Target`.
//!
//! - Identity: `/rigol/data/vendor.bin` (read only): XXTEA, CRC32, then items, each XXTEA again. Item 5 = model
//!   ("DHO924S"), 7 = serial, 9 = LAN MAC. The keys are found at run time (`vendorkeys`).
//! - Capabilities: the model table of the board (`Target::caps`).
//! - Limits: the same for every model of a board (`Limits::standard`). Each one can be
//!   overridden in `/oos/limits.conf` for our own tests; overridden values are marked experimental.
//! - Unit defaults: an image built for one unit can carry `/etc/oos/limits.conf` (`UNIT_CONF`) with capability keys
//!   only. They apply from the first start, before `/oos/limits.conf` exists, and are not experimental: a unit
//!   whose vendor.bin names hardware it lacks boots without generator and LA, and still reports the model vendor.bin names.
//!
//! The process-wide `Platform` is set once at start (`init`); before that `platform()` is the board's assumed model with
//! the standard limits, which is also what the host tests see.

use crate::target::ACTIVE;
use std::sync::OnceLock;

/// What a model has (from the model table and the model name).
#[derive(Clone, Debug, PartialEq)]
pub struct Caps {
    pub name: String,
    /// product series, as a person calls it
    pub series: &'static str,
    /// analog inputs (2 or 4)
    pub channels: usize,
    /// rated bandwidth in MHz (the stock licence bandwidth; OOS does not limit to it)
    pub rated_bw_mhz: u32,
    /// stock bandwidth code of the model table (5 = 70, 6 = 100, 7 = 125, 10 = 250 MHz)
    pub bw_code: u32,
    /// logic analyser port (PLA2216 probe), DHO900 only
    pub la: bool,
    /// built-in generators (GI/GO), "S" models only on the DHO900
    pub generators: usize,
    /// EXT trigger input, 2-channel models only
    pub ext_trig: bool,
    /// EDU variant
    pub edu: bool,
}

impl Caps {
    /// The unit has a built-in generator.
    pub fn has_gen(&self) -> bool {
        self.generators > 0
    }
}

/// Rated bandwidth of a stock bandwidth code (afe-settings.md 3.5; 4 and 9 from the HDO1000 entries of the table).
pub fn bw_code_mhz(code: u32) -> Option<u32> {
    Some(match code {
        2 => 20,
        4 => 50,
        5 => 70,
        6 => 100,
        7 => 125,
        9 => 200,
        10 => 250,
        _ => return None,
    })
}

/// Capabilities of a model name (case-insensitive), `None` if it is not a model of this build's board.
pub fn lookup(name: &str) -> Option<Caps> {
    (ACTIVE.caps)(name)
}

/// Generator frequency caps per wave shape in Hz (afg.md 3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AfgMax {
    pub sine: f64,
    pub square: f64,
    pub ramp: f64,
    pub pulse: f64,
    pub noise: f64,
    pub dc: f64,
    pub arb: f64,
}

/// Acquisition and generator limits. Index `[0]` = 1 channel on, `[1]` = 2, `[2]` = 3 or 4.
#[derive(Clone, Debug, PartialEq)]
pub struct Limits {
    /// maximum sample rate per channel (series table, horizontal-config.md 1.1); fixed by the 1.25 GHz ADC clock
    pub max_srate: [f64; 3],
    /// maximum memory depth per channel, LA off (stock 50 / 25 / 10 M)
    pub max_mem: [u64; 3],
    /// maximum memory depth per channel, LA on
    pub max_mem_la: [u64; 3],
    pub afg_max_hz: AfgMax,
    /// ADC sample clock (LMX2582 table, pll.md); informational until OOS has its own PLL tables
    pub adc_clock_hz: f64,
    /// generator DDS clock (ADC clock / 8)
    pub afg_clock_hz: f64,
}

impl Limits {
    /// The standard limits of this build's board.
    pub fn standard() -> Limits {
        (ACTIVE.limits)()
    }

    fn idx(n_on: usize) -> usize {
        match n_on {
            0 | 1 => 0,
            2 => 1,
            _ => 2,
        }
    }
    pub fn srate(&self, n_on: usize) -> f64 {
        self.max_srate[Self::idx(n_on)]
    }
    pub fn mem(&self, n_on: usize, la: bool) -> u64 {
        if la {
            self.max_mem_la[Self::idx(n_on)]
        } else {
            self.max_mem[Self::idx(n_on)]
        }
    }
}

/// Where the model identity came from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// `vendor.bin`, model and serial
    Vendor,
    /// `model = ...` in the limits file (or `--model`)
    Forced,
    /// no readable vendor.bin (host, or the rigol partition is not mounted): DHO924S assumed
    Assumed,
}

/// The unit OOS runs on: identity, capabilities, limits and the experimental overrides in effect.
#[derive(Clone, Debug, PartialEq)]
pub struct Platform {
    pub caps: Caps,
    pub limits: Limits,
    pub serial: Option<String>,
    pub mac: Option<String>,
    pub source: Source,
    /// "key = value" of every override applied (experimental values)
    pub overrides: Vec<String>,
    /// "key = value" of every capability default of the image (`UNIT_CONF`): facts of this unit, not experimental
    pub unit: Vec<String>,
    /// problems found while detecting (unreadable vendor.bin, unknown model, bad lines in the limits file)
    pub warnings: Vec<String>,
    /// the vendor file names a model outside the DHO800/DHO900 family (an MHO900, a DHO1000, ...): another board,
    /// whose hardware OOS does not drive (`drives_hardware`, docs/porting.md). `--model` does not change it.
    pub foreign: Option<String>,
}

impl Default for Platform {
    fn default() -> Platform {
        Platform {
            caps: lookup(ACTIVE.assumed_model).expect("table"),
            limits: Limits::standard(),
            serial: None,
            mac: None,
            source: Source::Assumed,
            overrides: Vec::new(),
            unit: Vec::new(),
            warnings: Vec::new(),
            foreign: None,
        }
    }
}

impl Platform {
    /// Detection from the vendor file bytes (if any), the limits file text (if any) and a forced model name (if
    /// any, e.g. `--model`; it wins over the file's `model =`).
    pub fn build(vendor: Option<&[u8]>, limits_conf: Option<&str>, force: Option<&str>) -> Platform {
        Platform::build_unit(vendor, None, limits_conf, force)
    }

    /// `build` with the capability defaults of the image (`UNIT_CONF`) applied between the vendor file and the limits
    /// file: vendor.bin < unit defaults < limits.conf < forced model. A forced model keeps the unit defaults and the
    /// capability overrides on top. Only capability keys count in the unit file; others are warnings.
    pub fn build_unit(vendor: Option<&[u8]>, unit_conf: Option<&str>, limits_conf: Option<&str>, force: Option<&str>) -> Platform {
        let mut p = Platform::default();
        match vendor.map(decode_vendor) {
            Some(Ok(v)) => {
                p.serial = v.serial.clone();
                p.mac = v.mac.clone();
                match v.model.as_deref().and_then(lookup) {
                    Some(c) => {
                        p.caps = c;
                        p.source = Source::Vendor;
                    }
                    None => {
                        let m = v.model.unwrap_or_default();
                        if (ACTIVE.family)(&m) {
                            p.warnings.push(format!("vendor.bin: model {:?} is not in the model table: {} assumed", m, ACTIVE.assumed_model));
                        } else {
                            p.warnings.push(format!(
                                "vendor.bin: model {:?} is not a {}, another board: OOS does not drive its hardware (docs/porting.md)",
                                m, ACTIVE.family_name
                            ));
                            p.foreign = Some(m);
                        }
                    }
                }
            }
            Some(Err(e)) => p.warnings.push(format!("vendor.bin: {}", e)),
            None => p.warnings.push("no vendor.bin".into()),
        }
        if let Some(text) = unit_conf {
            for (n, k, v) in conf_lines(text, &mut p.warnings, UNIT_CONF) {
                match set_cap(&mut p.caps, k, v) {
                    Ok(true) => p.unit.push(format!("{} = {}", k, v)),
                    Ok(false) => p.warnings.push(format!("{}:{}: {} is not a capability key (channels, la, afg, ext_trig)", UNIT_CONF, n, k)),
                    Err(e) => p.warnings.push(format!("{}:{}: {}", UNIT_CONF, n, e)),
                }
            }
        }
        let mut forced = force.map(str::to_string);
        if let Some(text) = limits_conf {
            for (n, line) in text.lines().enumerate() {
                let line = line.split('#').next().unwrap_or("").trim();
                if line.is_empty() {
                    continue;
                }
                let Some((k, v)) = line.split_once('=') else {
                    p.warnings.push(format!("limits.conf:{}: no '=' in {:?}", n + 1, line));
                    continue;
                };
                let (k, v) = (k.trim(), v.trim());
                if k == "model" {
                    forced = forced.or(Some(v.to_string()));
                    continue;
                }
                match p.set(k, v) {
                    Ok(()) => p.overrides.push(format!("{} = {}", k, v)),
                    Err(e) => p.warnings.push(format!("limits.conf:{}: {}", n + 1, e)),
                }
            }
        }
        if let Some(name) = forced {
            match lookup(&name) {
                Some(mut c) => {
                    // the unit defaults and the capability overrides of the file stay applied on top of the forced model
                    for o in p.unit.iter().chain(&p.overrides) {
                        if let Some((k, v)) = o.split_once(" = ") {
                            let _ = set_cap(&mut c, k, v);
                        }
                    }
                    p.caps = c;
                    p.source = Source::Forced;
                }
                None => p.warnings.push(format!("model {:?}: not a {} name, ignored", name, ACTIVE.family_name)),
            }
        }
        if p.source == Source::Assumed {
            p.warnings.push(format!("model unknown: {} assumed", ACTIVE.assumed_model));
        }
        p
    }

    /// Reads `<cal_dir>/vendor.bin`, the unit defaults of the image (`UNIT_CONF`) and the limits file (all optional).
    pub fn detect(cal_dir: &str, limits_path: &str, force: Option<&str>) -> Platform {
        let vendor = std::fs::read(format!("{}/vendor.bin", cal_dir)).ok();
        let unit = std::fs::read_to_string(UNIT_CONF).ok();
        let conf = std::fs::read_to_string(limits_path).ok();
        Platform::build_unit(vendor.as_deref(), unit.as_deref(), conf.as_deref(), force)
    }

    /// One override `key = value`.
    fn set(&mut self, k: &str, v: &str) -> Result<(), String> {
        if set_cap(&mut self.caps, k, v)? {
            return Ok(());
        }
        let f = || v.replace('_', "").parse::<f64>().ok().filter(|x| x.is_finite()).ok_or(format!("{}: {:?} is not a number", k, v));
        let l = &mut self.limits;
        let i = |s: &str| match s {
            "1ch" => Some(0),
            "2ch" => Some(1),
            "4ch" => Some(2),
            _ => None,
        };
        if k == "adc.clock_hz" {
            (ACTIVE.set_adc_clock)(l, f()?).map_err(|e| format!("{}: {}", k, e))?;
        } else if let Some(rest) = k.strip_prefix("mem.la_") {
            let ix = i(rest).ok_or(format!("unknown key {}", k))?;
            l.max_mem_la[ix] = f()?.max(1000.0) as u64;
        } else if let Some(rest) = k.strip_prefix("mem.") {
            let ix = i(rest).ok_or(format!("unknown key {}", k))?;
            l.max_mem[ix] = f()?.max(1000.0) as u64;
        } else if let Some(shape) = k.strip_prefix("afg.").and_then(|s| s.strip_suffix("_max_hz")) {
            // the DDS cannot produce anything above half its clock
            let hz = f()?.clamp(0.01, l.afg_clock_hz / 2.0);
            let a = &mut l.afg_max_hz;
            match shape {
                "sine" => a.sine = hz,
                "square" => a.square = hz,
                "ramp" => a.ramp = hz,
                "pulse" => a.pulse = hz,
                "noise" => a.noise = hz,
                "dc" => a.dc = hz,
                "arb" => a.arb = hz,
                _ => return Err(format!("unknown key {}", k)),
            }
        } else {
            return Err(format!("unknown key {} (keys: docs/models.md)", k));
        }
        Ok(())
    }

    /// Short identity line, e.g. "DHO924S  SN DHO9S000000000".
    pub fn ident(&self) -> String {
        let src = match self.source {
            Source::Vendor => "",
            Source::Forced => "  (forced)",
            Source::Assumed => "  (assumed)",
        };
        if let Some(m) = &self.foreign {
            return format!("{}  SN {}  (another board: hardware not driven)", m, self.serial.as_deref().unwrap_or("-"));
        }
        format!("{}  SN {}{}", self.caps.name, self.serial.as_deref().unwrap_or("-"), src)
    }

    /// Capability line, e.g. "250 MHz rated, 4 ch, LA, generator".
    pub fn caps_line(&self) -> String {
        let c = &self.caps;
        let mut s = format!("{} MHz rated, {} ch", c.rated_bw_mhz, c.channels);
        for (on, name) in [(c.la, "LA"), (c.has_gen(), "generator"), (c.ext_trig, "EXT trigger")] {
            if on {
                s += ", ";
                s += name;
            }
        }
        s
    }

    pub fn experimental(&self) -> bool {
        !self.overrides.is_empty()
    }

    /// False on another board (`foreign`): then nothing may touch the acquisition hardware, the generator or the LA.
    pub fn drives_hardware(&self) -> bool {
        self.foreign.is_none()
    }
}

/// The `key = value` lines of a file in the limits format: (line number, key, value); a line without `=` is a warning.
fn conf_lines<'a>(text: &'a str, warnings: &mut Vec<String>, file: &str) -> Vec<(usize, &'a str, &'a str)> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        match line.split_once('=') {
            Some((k, v)) => out.push((n + 1, k.trim(), v.trim())),
            None => warnings.push(format!("{}:{}: no '=' in {:?}", file, n + 1, line)),
        }
    }
    out
}

/// Capability keys of the limits file (`channels`, `afg`, `la`, `ext_trig`); Ok(false) = not a capability key.
fn set_cap(c: &mut Caps, k: &str, v: &str) -> Result<bool, String> {
    let b = || match v.to_ascii_lowercase().as_str() {
        "1" | "yes" | "on" | "true" => Ok(true),
        "0" | "no" | "off" | "false" => Ok(false),
        _ => Err(format!("{}: {:?} is not yes/no", k, v)),
    };
    match k {
        "channels" => {
            c.channels = match v {
                "2" => 2,
                "4" => 4,
                _ => return Err(format!("channels: {:?} (2 or 4)", v)),
            }
        }
        "afg" => c.generators = if b()? { c.generators.max(1) } else { 0 },
        "la" => c.la = b()?,
        "ext_trig" => c.ext_trig = b()?,
        _ => return Ok(false),
    }
    Ok(true)
}

static PLATFORM: OnceLock<Platform> = OnceLock::new();

/// Sets the process-wide platform (once; later calls are ignored and return false).
pub fn init(p: Platform) -> bool {
    PLATFORM.set(p).is_ok()
}

/// The process-wide platform (DHO924S with the standard limits until `init`).
pub fn platform() -> &'static Platform {
    PLATFORM.get_or_init(Platform::default)
}

/// Default location of the limits file (the OOS data partition).
pub const LIMITS_CONF: &str = "/oos/limits.conf";

/// Capability defaults of an image built for one unit (boot/build-dev-image.sh `OOS_UNIT`, docs/models.md 4.2): in the
/// image, so they apply from the first start.
pub const UNIT_CONF: &str = "/etc/oos/limits.conf";

/// `text` of limits.conf with `adc.clock_hz` set to `hz`: the line goes first (the `afg.*_max_hz` lines clamp to the clock
/// read before them), at the standard clock there is none. All other lines stay as they are.
pub fn with_adc_clock(text: &str, hz: f64) -> String {
    let mut out = String::new();
    if hz != ACTIVE.clock_steps[0] {
        out.push_str(&format!("adc.clock_hz = {:e}\n", hz));
    }
    for line in text.lines() {
        let key = line.split(['=', '#']).next().unwrap_or("").trim();
        if key != "adc.clock_hz" {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

// ---- vendor.bin ----

/// The two keys of vendor.bin: the outer layer over the file, the inner one over each item. They are not in this source:
/// see `vendorkeys`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keys {
    pub file: [u32; 4],
    pub item: [u32; 4],
}

/// Made-up keys of the tests.
#[cfg(test)]
pub(crate) const TEST_KEYS: Keys = Keys { file: [0x1111_2222, 0x3333_4444, 0x5555_6666, 0x7777_8888], item: [0x0102_0304, 0x0506_0708, 0x090a_0b0c, 0x0d0e_0f10] };

/// Decoded vendor items.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vendor {
    pub model: Option<String>,
    pub serial: Option<String>,
    pub mac: Option<String>,
    /// every item: (id, string)
    pub items: Vec<(u32, String)>,
}

const DELTA: u32 = 0x9e37_79b9;

fn mx(sum: u32, y: u32, z: u32, p: usize, e: u32, k: &[u32; 4]) -> u32 {
    (((z >> 5) ^ (y << 2)).wrapping_add((y >> 3) ^ (z << 4))) ^ ((sum ^ y).wrapping_add(k[(p & 3) ^ e as usize] ^ z))
}

/// XXTEA (btea) decode in place, the standard 6 + 52/n rounds.
fn xxtea_decode(v: &mut [u32], k: &[u32; 4]) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let rounds = 6 + 52 / n as u32;
    let mut sum = rounds.wrapping_mul(DELTA);
    let mut y = v[0];
    for _ in 0..rounds {
        let e = (sum >> 2) & 3;
        for p in (0..n).rev() {
            let z = v[if p > 0 { p - 1 } else { n - 1 }];
            v[p] = v[p].wrapping_sub(mx(sum, y, z, p, e, k));
            y = v[p];
        }
        sum = sum.wrapping_sub(DELTA);
    }
}

/// XXTEA encode (for the tests: builds vendor files).
#[cfg(test)]
fn xxtea_encode(v: &mut [u32], k: &[u32; 4]) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let rounds = 6 + 52 / n as u32;
    let mut sum = 0u32;
    let mut z = v[n - 1];
    for _ in 0..rounds {
        sum = sum.wrapping_add(DELTA);
        let e = (sum >> 2) & 3;
        for p in 0..n {
            let y = v[if p + 1 < n { p + 1 } else { 0 }];
            v[p] = v[p].wrapping_add(mx(sum, y, z, p, e, k));
            z = v[p];
        }
    }
}

fn words(b: &[u8]) -> Vec<u32> {
    b.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}
fn bytes(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
}

/// The payload of vendor.bin under the outer key `k`; `None` if the checksum says it is the wrong key or no such file.
pub(crate) fn vendor_payload(file: &[u8], k: &[u32; 4]) -> Option<Vec<u8>> {
    if file.len() < 8 || file.len() % 4 != 0 {
        return None;
    }
    let mut w = words(file);
    xxtea_decode(&mut w, k);
    let p = bytes(&w);
    let (crc, len) = (u32_at(&p, 0)?, u32_at(&p, 4)? as usize);
    let payload = p.get(8..8usize.checked_add(len)?)?;
    (crate::crc32(payload) == crc).then(|| payload.to_vec())
}

/// Does the inner key `k` turn the model item of `payload` into a known model?
pub(crate) fn item_key_fits(payload: &[u8], k: &[u32; 4]) -> bool {
    items(payload, k).model.is_some_and(|m| lookup(&m).is_some())
}

/// Decodes vendor.bin with the keys of this unit (`vendorkeys`).
pub fn decode_vendor(file: &[u8]) -> Result<Vendor, String> {
    #[cfg(test)]
    let keys = Some(TEST_KEYS);
    #[cfg(not(test))]
    let keys = crate::vendorkeys::for_file(file);
    decode_vendor_with(file, &keys.ok_or("no key for it in the stock software of this unit")?)
}

/// Decodes vendor.bin: XXTEA with the file key over the file; [crc32][len][payload] with crc32 over the payload; payload =
/// items [u32 4][u32 id][u32 n][n bytes]; item bytes = [crc32][len][string length][len bytes XXTEA with the item key],
/// crc32 over the string-length word and the encrypted bytes.
pub fn decode_vendor_with(file: &[u8], k: &Keys) -> Result<Vendor, String> {
    if file.len() < 8 || file.len() % 4 != 0 {
        return Err(format!("{} bytes, not a vendor file", file.len()));
    }
    let payload = vendor_payload(file, &k.file).ok_or("checksum wrong")?;
    Ok(items(&payload, &k.item))
}

/// The items of a payload.
fn items(payload: &[u8], key: &[u32; 4]) -> Vendor {
    let mut v = Vendor::default();
    let mut o = 0;
    while o + 12 <= payload.len() {
        let (id, n) = (u32_at(payload, o + 4).unwrap(), u32_at(payload, o + 8).unwrap() as usize);
        let Some(item) = payload.get(o + 12..(o + 12).saturating_add(n)) else { break };
        o += 12 + n;
        let (Some(icrc), Some(ilen), Some(slen)) = (u32_at(item, 0), u32_at(item, 4), u32_at(item, 8)) else { continue };
        let (ilen, slen) = (ilen as usize, slen as usize);
        let Some(signed) = item.get(8..12 + ilen) else { continue };
        if crate::crc32(signed) != icrc || ilen % 4 != 0 || slen > ilen {
            continue;
        }
        let mut iw = words(&item[12..12 + ilen]);
        xxtea_decode(&mut iw, key);
        let s = String::from_utf8_lossy(&bytes(&iw)[..slen]).into_owned();
        match id {
            5 => v.model = Some(s.clone()),
            7 => v.serial = Some(s.clone()),
            9 => v.mac = Some(s.clone()),
            _ => {}
        }
        v.items.push((id, s));
    }
    v
}

/// Builds a vendor file (tests).
#[cfg(test)]
pub(crate) fn encode_vendor(items: &[(u32, &str)]) -> Vec<u8> {
    encode_vendor_with(items, &TEST_KEYS)
}

/// Builds a vendor file with the keys `k` (tests).
#[cfg(test)]
pub(crate) fn encode_vendor_with(items: &[(u32, &str)], k: &Keys) -> Vec<u8> {
    let mut payload = Vec::new();
    for &(id, s) in items {
        let mut data = s.as_bytes().to_vec();
        data.resize(s.len().div_ceil(4).max(2) * 4, 0);
        let mut iw = words(&data);
        xxtea_encode(&mut iw, &k.item);
        let enc = bytes(&iw);
        let mut signed = (s.len() as u32).to_le_bytes().to_vec();
        signed.extend_from_slice(&enc);
        let mut item = crate::crc32(&signed).to_le_bytes().to_vec();
        item.extend_from_slice(&(enc.len() as u32).to_le_bytes());
        item.extend_from_slice(&signed);
        for x in [4u32, id, item.len() as u32] {
            payload.extend_from_slice(&x.to_le_bytes());
        }
        payload.extend_from_slice(&item);
    }
    let mut f = crate::crc32(&payload).to_le_bytes().to_vec();
    f.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    f.extend_from_slice(&payload);
    f.resize(f.len().div_ceil(4) * 4, 0);
    let mut w = words(&f);
    xxtea_encode(&mut w, &k.file);
    bytes(&w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xxtea_round_trip() {
        let mut v = vec![1u32, 2, 3, 4, 5, 6, 7];
        xxtea_encode(&mut v, &TEST_KEYS.item);
        assert_ne!(v, vec![1, 2, 3, 4, 5, 6, 7]);
        xxtea_decode(&mut v, &TEST_KEYS.item);
        assert_eq!(v, vec![1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn vendor_file_round_trip() {
        let f = encode_vendor(&[(5, "DHO804"), (7, "DHO8A000000001"), (9, "00:19:AF:00:00:01")]);
        let v = decode_vendor(&f).unwrap();
        assert_eq!(v.model.as_deref(), Some("DHO804"));
        assert_eq!(v.serial.as_deref(), Some("DHO8A000000001"));
        assert_eq!(v.mac.as_deref(), Some("00:19:AF:00:00:01"));
        let mut bad = f.clone();
        bad[20] ^= 1;
        assert!(decode_vendor(&bad).is_err() || decode_vendor(&bad).unwrap().model.is_none());
    }

    #[test]
    fn model_table() {
        let c = lookup("dho802").unwrap();
        assert_eq!((c.channels, c.rated_bw_mhz, c.la, c.has_gen(), c.ext_trig, c.series), (2, 70, false, false, true, "DHO800"));
        let c = lookup("DHO814").unwrap();
        assert_eq!((c.channels, c.rated_bw_mhz, c.la, c.ext_trig), (4, 100, false, false));
        let c = lookup("DHO914S").unwrap();
        assert_eq!((c.rated_bw_mhz, c.la, c.has_gen()), (125, true, true));
        let c = lookup("DHO924-EDU").unwrap();
        assert_eq!((c.rated_bw_mhz, c.has_gen(), c.edu), (250, false, true));
        assert!(lookup("HDO1104").is_none() && lookup("MHO934").is_none());
    }

    #[test]
    fn detection_and_overrides() {
        let f = encode_vendor(&[(5, "DHO812"), (7, "DHO8A123")]);
        let p = Platform::build(Some(&f), None, None);
        assert_eq!((p.caps.name.as_str(), p.caps.channels, p.source.clone()), ("DHO812", 2, Source::Vendor));
        assert!(!p.experimental());
        assert_eq!(p.limits, Limits::standard());

        let conf = "# test\nmem.1ch = 200e6\nafg.sine_max_hz = 70e6\nafg.square_max_hz = 1e9  # clamped\nla = yes\nfoo = 1\nbroken line\n";
        let p = Platform::build(Some(&f), Some(conf), None);
        assert_eq!(p.limits.max_mem[0], 200_000_000);
        assert_eq!(p.limits.afg_max_hz.sine, 70e6);
        assert_eq!(p.limits.afg_max_hz.square, 78.125e6);
        assert!(p.caps.la);
        assert_eq!(p.overrides.len(), 4);
        assert_eq!(p.warnings.len(), 2, "{:?}", p.warnings);
        assert!(p.experimental());

        // forced model (file or argument), capability overrides stay on top
        let p = Platform::build(Some(&f), Some("model = DHO924S\nafg = no\n"), None);
        assert_eq!((p.caps.name.as_str(), p.caps.channels, p.caps.has_gen(), p.source.clone()), ("DHO924S", 4, false, Source::Forced));
        let p = Platform::build(None, Some("model = DHO804"), Some("DHO914S"));
        assert_eq!(p.caps.name, "DHO914S");
        let p = Platform::build(None, None, None);
        assert_eq!((p.caps.name.as_str(), p.source.clone()), ("DHO924S", Source::Assumed));
        let p = Platform::build(Some(&[1, 2, 3, 4, 5, 6, 7, 8]), None, None);
        assert_eq!(p.source, Source::Assumed);
        assert_eq!(p.warnings.len(), 2, "{:?}", p.warnings);
    }

    #[test]
    fn another_board_is_not_driven() {
        for m in ["MHO934", "MHO98", "DHO1074", "DHO4204", "HDO1104"] {
            let f = encode_vendor(&[(5, m), (7, "X1234567")]);
            let p = Platform::build(Some(&f), None, None);
            assert_eq!(p.foreign.as_deref(), Some(m));
            assert!(!p.drives_hardware());
            assert!(p.ident().starts_with(m), "{}", p.ident());
            // neither the limits file nor the argument makes it a DHO
            let p = Platform::build(Some(&f), Some("model = DHO924S\n"), Some("DHO924S"));
            assert!(!p.drives_hardware(), "{}", m);
        }
        // the family, also a variant missing from the table
        for m in ["DHO924S", "DHO804", "dho914-edu", "DHO934"] {
            assert!((ACTIVE.family)(m), "{}", m);
            let p = Platform::build(Some(&encode_vendor(&[(5, m)])), None, None);
            assert!(p.drives_hardware(), "{}", m);
        }
        let family = ACTIVE.family;
        assert!(!family("DHO1074") && !family("DHO4204") && !family("DHO9") && !family("MHO934"));
        // no or an unreadable vendor file: as before, DHO924S assumed
        assert!(Platform::build(None, None, None).drives_hardware());
        assert!(Platform::build(Some(&[1, 2, 3, 4, 5, 6, 7, 8]), None, None).drives_hardware());
    }

    #[test]
    fn limits_per_channel_count() {
        let l = Limits::standard();
        assert_eq!((l.srate(1), l.srate(2), l.srate(3), l.srate(4)), (1.25e9, 625e6, 312.5e6, 312.5e6));
        assert_eq!((l.mem(1, false), l.mem(4, true)), (125_000_000, 10_000_000));
    }
}

#[cfg(test)]
mod clock_override_tests {
    use super::*;

    #[test]
    fn adc_clock_override() {
        let p = Platform::build(None, Some("adc.clock_hz = 2e9\n"), None);
        assert_eq!(p.limits.adc_clock_hz, 2e9);
        assert_eq!(p.limits.afg_clock_hz, 250e6);
        assert_eq!(p.limits.max_srate, [2e9, 1e9, 5e8]);
        assert!(p.experimental());
        // refused: out of range, or a clock the PLL generator cannot make
        for bad in ["adc.clock_hz = 3e9", "adc.clock_hz = 1e9", "adc.clock_hz = 1500000000.5"] {
            let p = Platform::build(None, Some(bad), None);
            assert_eq!(p.limits.adc_clock_hz, 1.25e9, "{}", bad);
            assert!(p.warnings.iter().any(|w| w.contains("adc.clock_hz")), "{}: {:?}", bad, p.warnings);
        }
    }
}

#[cfg(test)]
mod bad_limits_tests {
    use super::*;

    /// Values that used to reach `f64::clamp` with min > max or NaN and abort the process.
    #[test]
    fn no_panic_from_the_file() {
        for bad in ["afg.sine_max_hz = nan", "afg.sine_max_hz = inf", "mem.1ch = nan", "adc.clock_hz = nan"] {
            let p = Platform::build(None, Some(bad), None);
            assert_eq!(p.limits, Limits::standard(), "{}", bad);
            assert!(!p.warnings.is_empty());
        }
        let p = Platform::build(None, Some("afg.sine_max_hz = 0"), None);
        assert_eq!(p.limits.afg_max_hz.sine, 0.01);
        assert!(crate::dho::afg::Wave::Sine.max_hz_in(&p.limits) * 1e6 >= 2000.0);
    }
}

#[cfg(test)]
mod unit_defaults_tests {
    use super::*;

    const DHO804_UNIT: &str = "# physically a DHO804: no generator board, no LA connector\nafg = no\nla = no\n";

    #[test]
    fn unit_defaults_change_capabilities_not_identity() {
        let f = encode_vendor(&[(5, "DHO924S"), (7, "DHO9A000000001")]);
        let p = Platform::build_unit(Some(&f), Some(DHO804_UNIT), None, None);
        assert_eq!((p.caps.name.as_str(), p.caps.rated_bw_mhz, p.caps.channels), ("DHO924S", 250, 4));
        assert_eq!((p.caps.has_gen(), p.caps.la, p.source.clone()), (false, false, Source::Vendor));
        assert_eq!(p.unit, vec!["afg = no", "la = no"]);
        // facts of the unit: not experimental, no warnings, the identity line is unchanged
        assert!(!p.experimental() && p.warnings.is_empty(), "{:?}", p.warnings);
        assert_eq!(p.ident(), "DHO924S  SN DHO9A000000001");
        assert_eq!(p.caps_line(), "250 MHz rated, 4 ch");
        // without the unit file, as before
        let p = Platform::build(Some(&f), None, None);
        assert!(p.caps.has_gen() && p.caps.la && p.unit.is_empty());
    }

    #[test]
    fn limits_conf_and_forced_model_on_top() {
        let f = encode_vendor(&[(5, "DHO924S")]);
        // the generator board fitted later: limits.conf switches it on again (experimental)
        let p = Platform::build_unit(Some(&f), Some(DHO804_UNIT), Some("afg = yes\n"), None);
        assert!(p.caps.has_gen() && !p.caps.la && p.experimental());
        // a forced model keeps the unit defaults
        let p = Platform::build_unit(Some(&f), Some(DHO804_UNIT), None, Some("DHO914S"));
        assert_eq!((p.caps.name.as_str(), p.caps.has_gen(), p.caps.la), ("DHO914S", false, false));
        let p = Platform::build_unit(Some(&f), Some(DHO804_UNIT), Some("model = DHO924S\nla = yes\n"), None);
        assert_eq!((p.caps.has_gen(), p.caps.la, p.source.clone()), (false, true, Source::Forced));
        // first start: no vendor keys yet, no limits.conf: the unit defaults hold all the same
        let p = Platform::build_unit(None, Some(DHO804_UNIT), None, None);
        assert!(!p.caps.has_gen() && !p.caps.la && p.drives_hardware());
    }

    #[test]
    fn unit_file_takes_capability_keys_only() {
        let p = Platform::build_unit(None, Some("afg = no\nadc.clock_hz = 2e9\nmem.1ch = 1e9\nmodel = DHO804\nla = maybe\nnonsense\n"), None, None);
        assert_eq!(p.unit, vec!["afg = no"]);
        assert_eq!(p.limits, Limits::standard());
        assert_eq!(p.caps.name, "DHO924S");
        // adc.clock_hz, mem.1ch, model, la = maybe, nonsense; plus "no vendor.bin" and "model unknown"
        assert_eq!(p.warnings.iter().filter(|w| w.starts_with(UNIT_CONF)).count(), 5, "{:?}", p.warnings);
        assert_eq!(p.warnings.len(), 7, "{:?}", p.warnings);
    }
}
