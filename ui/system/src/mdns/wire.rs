// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The DNS wire format of the responder: names and their labels, records (`Rec`, `Data`) and how they are written
//! (`Wr`, `responses`), and the reader of a packet (`parse`, `Msg`). It knows nothing of probing or interfaces; a new
//! record type gets its `T_` constant in `mdns.rs`, a `Data` variant here and its case in `Wr::rec` and `read_rdata`.

use super::*;

// ------------------------------------------------------------------------------------------------ names on the wire

/// A domain name as its labels.
pub type Name = Vec<String>;

pub(super) fn dname(parts: &[&str]) -> Name {
    parts.iter().map(|s| s.to_string()).collect()
}

/// Names compare without regard to ASCII case (RFC 6762 §16).
pub(super) fn same(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

fn label_bytes(l: &str) -> &[u8] {
    let b = l.as_bytes();
    &b[..b.len().min(63)]
}

/// A name uncompressed, its labels as they are (at most 63 bytes each).
pub fn write_name(n: &[String], out: &mut Vec<u8>) {
    for l in n {
        let b = label_bytes(l);
        out.push(b.len() as u8);
        out.extend(b);
    }
    out.push(0);
}

/// A name uncompressed and in lower case: the form names are compared in, also inside record data.
pub(super) fn canon_name(n: &[String], out: &mut Vec<u8>) {
    for l in n {
        let b = label_bytes(l);
        out.push(b.len() as u8);
        out.extend(b.iter().map(|c| c.to_ascii_lowercase()));
    }
    out.push(0);
}

pub(super) fn reverse4(a: Ipv4Addr) -> Name {
    let o = a.octets();
    let mut n: Name = o.iter().rev().map(|b| b.to_string()).collect();
    n.extend(["in-addr".to_string(), "arpa".to_string()]);
    n
}

/// FNV-1a: a fingerprint of a packet.
pub(super) fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf29ce484222325u64, |h, &c| (h ^ c as u64).wrapping_mul(0x100000001b3))
}

/// The NSEC type bitmap of window 0 (RFC 4034 §4.1.2).
fn type_bitmap(types: &[u16]) -> Vec<u8> {
    let Some(&max) = types.iter().filter(|t| **t < 256).max() else { return Vec::new() };
    let n = max as usize / 8 + 1;
    let mut v = vec![0u8; 2 + n];
    v[1] = n as u8;
    for &t in types.iter().filter(|t| **t < 256) {
        v[2 + t as usize / 8] |= 0x80 >> (t % 8);
    }
    v
}

pub fn txt_bytes(t: &[String]) -> Vec<u8> {
    if t.is_empty() {
        // an empty TXT record is one empty string (RFC 6763 §6.1)
        return vec![0];
    }
    let mut v = Vec::new();
    for s in t {
        let b = &s.as_bytes()[..s.len().min(255)];
        v.push(b.len() as u8);
        v.extend(b);
    }
    v
}

/// The data of a record of the scope.
#[derive(Clone, Debug, PartialEq)]
pub enum Data {
    A(Ipv4Addr),
    /// not sent while the services listen on IPv4 only (`Responder::records`), kept for when they do
    Aaaa(Ipv6Addr),
    Ptr(Name),
    Srv {
        port: u16,
        target: Name,
    },
    Txt(Vec<String>),
    Nsec {
        next: Name,
        types: Vec<u16>,
    },
}

impl Data {
    pub(super) fn rtype(&self) -> u16 {
        match self {
            Data::A(_) => T_A,
            Data::Aaaa(_) => T_AAAA,
            Data::Ptr(_) => T_PTR,
            Data::Srv { .. } => T_SRV,
            Data::Txt(_) => T_TXT,
            Data::Nsec { .. } => T_NSEC,
        }
    }

    /// The data uncompressed, names in lower case: compared with a received record's `Rr::canon`.
    pub(super) fn canon(&self) -> Vec<u8> {
        let mut v = Vec::new();
        match self {
            Data::A(a) => v.extend(a.octets()),
            Data::Aaaa(a) => v.extend(a.octets()),
            Data::Ptr(n) => canon_name(n, &mut v),
            Data::Srv { port, target } => {
                v.extend([0, 0, 0, 0]);
                v.extend(port.to_be_bytes());
                canon_name(target, &mut v);
            }
            Data::Txt(t) => v = txt_bytes(t),
            Data::Nsec { next, types } => {
                canon_name(next, &mut v);
                v.extend(type_bitmap(types));
            }
        }
        v
    }
}

/// A record of the scope. `unique`: only the scope has records of this name and type (cache flush, probed); the
/// others are shared (PTR of a service type).
#[derive(Clone, Debug, PartialEq)]
pub struct Rec {
    pub name: Name,
    pub data: Data,
    pub ttl: u32,
    pub unique: bool,
}

impl Rec {
    pub(super) fn new(name: Name, data: Data, ttl: u32, unique: bool) -> Rec {
        Rec { name, data, ttl, unique }
    }

    /// The same record as a received one (name, type, class, data; not the TTL).
    pub(super) fn matches(&self, r: &Rr) -> bool {
        r.rtype == self.data.rtype() && r.class == CLASS_IN && same(&r.name, &self.name) && r.canon == self.data.canon()
    }

    pub(super) fn same_rec(&self, o: &Rec) -> bool {
        same(&self.name, &o.name) && self.data.rtype() == o.data.rtype() && self.data.canon() == o.data.canon()
    }
}

// ------------------------------------------------------------------------------------------------ reading packets

/// A question.
#[derive(Clone, Debug, PartialEq)]
pub struct Q {
    pub name: Name,
    /// the name as it came, uncompressed: what a legacy answer repeats (`name` reads bytes that are no UTF-8 as
    /// U+FFFD, three bytes each, so written from `name` it can be another name and longer than 255 bytes)
    pub wire: Vec<u8>,
    pub qtype: u16,
    /// the class without the QU bit
    pub qclass: u16,
    pub qu: bool,
}

/// A received record; names inside its data are expanded (`canon`).
#[derive(Clone, Debug, PartialEq)]
pub struct Rr {
    pub name: Name,
    pub rtype: u16,
    /// the class without the cache-flush bit
    pub class: u16,
    pub flush: bool,
    pub ttl: u32,
    pub canon: Vec<u8>,
}

/// A received message.
#[derive(Clone, Debug, PartialEq)]
pub struct Msg {
    pub id: u16,
    pub flags: u16,
    pub qs: Vec<Q>,
    pub an: Vec<Rr>,
    pub ns: Vec<Rr>,
    pub ar: Vec<Rr>,
}

impl Msg {
    pub fn is_response(&self) -> bool {
        self.flags & 0x8000 != 0
    }

    /// Opcode 0, a standard query or its answer; mDNS ignores the others (RFC 6762 §18.3).
    pub fn standard(&self) -> bool {
        (self.flags >> 11) & 0xf == 0
    }
}

fn u16_at(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(p)?, *b.get(p + 1)?]))
}

fn u32_at(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(p..p + 4)?.try_into().ok()?))
}

/// The name at `start` and where the bytes after it begin (`read_labels`).
fn read_name(b: &[u8], start: usize) -> Option<(Name, usize)> {
    let (labels, end) = read_labels(b, start)?;
    Some((name_of(&labels), end))
}

/// Labels as text: bytes that are no UTF-8 become U+FFFD.
fn name_of(labels: &[&[u8]]) -> Name {
    labels.iter().map(|l| String::from_utf8_lossy(l).into_owned()).collect()
}

/// The labels of the name at `start` as they are, and where the bytes after it begin. Compression pointers (RFC 1035
/// §4.1.4) must each point before the place the previous jump went to, so a loop cannot be built; a name stays within
/// 255 bytes.
fn read_labels(b: &[u8], start: usize) -> Option<(Vec<&[u8]>, usize)> {
    let mut labels = Vec::new();
    let (mut p, mut end, mut floor, mut total) = (start, None, start, 0usize);
    loop {
        let len = *b.get(p)? as usize;
        match len & 0xc0 {
            0x00 if len == 0 => {
                return Some((labels, end.unwrap_or(p + 1)));
            }
            0x00 => {
                let l = b.get(p + 1..p + 1 + len)?;
                total += len + 1;
                if total > 254 {
                    return None;
                }
                labels.push(l);
                p += 1 + len;
            }
            0xc0 => {
                let to = ((len & 0x3f) << 8) | *b.get(p + 1)? as usize;
                if to >= floor {
                    return None;
                }
                end.get_or_insert(p + 2);
                floor = to;
                p = to;
            }
            _ => return None,
        }
    }
}

/// The data of a record at `p..p + len` with the names in it expanded and in lower case.
fn read_rdata(b: &[u8], p: usize, len: usize, rtype: u16) -> Option<Vec<u8>> {
    let rd = b.get(p..p + len)?;
    let name_in = |at: usize, out: &mut Vec<u8>| -> Option<usize> {
        let (n, end) = read_name(b, at)?;
        if end > p + len {
            return None;
        }
        canon_name(&n, out);
        Some(end)
    };
    let mut v = Vec::new();
    match rtype {
        // PTR, NS, CNAME
        T_PTR | 2 | 5 => {
            name_in(p, &mut v)?;
        }
        T_SRV => {
            v.extend(rd.get(..6)?);
            name_in(p + 6, &mut v)?;
        }
        T_NSEC => {
            let end = name_in(p, &mut v)?;
            v.extend(&b[end..p + len]);
        }
        _ => v.extend(rd),
    }
    Some(v)
}

/// A message, or None for anything malformed or cut short (never a panic, whatever the bytes).
pub fn parse(b: &[u8]) -> Option<Msg> {
    parse_ends(b).map(|(m, _, _)| m)
}

/// `parse`, with where the questions end and where the last record ends (bytes after it are not looked at).
pub fn parse_ends(b: &[u8]) -> Option<(Msg, usize, usize)> {
    let counts = [u16_at(b, 4)?, u16_at(b, 6)?, u16_at(b, 8)?, u16_at(b, 10)?];
    let mut m =
        Msg { id: u16_at(b, 0)?, flags: u16_at(b, 2)?, qs: Vec::new(), an: Vec::new(), ns: Vec::new(), ar: Vec::new() };
    let mut p = 12;
    for _ in 0..counts[0] {
        let (labels, e) = read_labels(b, p)?;
        let name = name_of(&labels);
        let mut wire = Vec::new();
        for l in &labels {
            wire.push(l.len() as u8);
            wire.extend(*l);
        }
        wire.push(0);
        let (qtype, class) = (u16_at(b, e)?, u16_at(b, e + 2)?);
        m.qs.push(Q { name, wire, qtype, qclass: class & !TOP, qu: class & TOP != 0 });
        p = e + 4;
    }
    let questions_end = p;
    for (sec, &n) in counts[1..].iter().enumerate() {
        for _ in 0..n {
            let (name, e) = read_name(b, p)?;
            let (rtype, class, ttl, len) =
                (u16_at(b, e)?, u16_at(b, e + 2)?, u32_at(b, e + 4)?, u16_at(b, e + 8)? as usize);
            let canon = read_rdata(b, e + 10, len, rtype)?;
            let r = Rr { name, rtype, class: class & !TOP, flush: class & TOP != 0, ttl, canon };
            [&mut m.an, &mut m.ns, &mut m.ar][sec].push(r);
            p = e + 10 + len;
        }
    }
    Some((m, questions_end, p))
}

// ------------------------------------------------------------------------------------------------ writing packets

/// A packet being written, with the names written so far for compression.
pub(super) struct Wr {
    pub(super) b: Vec<u8>,
    names: Vec<(Name, u16)>,
}

impl Wr {
    pub(super) fn new(id: u16, flags: u16) -> Wr {
        let mut b = Vec::with_capacity(512);
        b.extend(id.to_be_bytes());
        b.extend(flags.to_be_bytes());
        b.extend([0u8; 8]);
        Wr { b, names: Vec::new() }
    }

    fn u16(&mut self, v: u16) {
        self.b.extend(v.to_be_bytes());
    }

    pub(super) fn counts(&mut self, c: [usize; 4]) {
        for (i, n) in c.iter().enumerate() {
            self.b[4 + 2 * i..6 + 2 * i].copy_from_slice(&(*n as u16).to_be_bytes());
        }
    }

    fn mark(&self) -> (usize, usize) {
        (self.b.len(), self.names.len())
    }

    fn back(&mut self, m: (usize, usize)) {
        self.b.truncate(m.0);
        self.names.truncate(m.1);
    }

    /// A name, its longest suffix written before as a pointer (`compress`); every suffix written is kept for later.
    fn name(&mut self, n: &[String], compress: bool) {
        for i in 0..n.len() {
            let key: Name = n[i..].iter().map(|l| l.to_ascii_lowercase()).collect();
            if compress {
                if let Some(&(_, off)) = self.names.iter().find(|(k, _)| *k == key) {
                    self.u16(0xc000 | off);
                    return;
                }
            }
            if self.b.len() < 0x3fff {
                self.names.push((key, self.b.len() as u16));
            }
            let l = label_bytes(&n[i]);
            self.b.push(l.len() as u8);
            self.b.extend(l);
        }
        self.b.push(0);
    }

    pub(super) fn question(&mut self, name: &[String], qtype: u16, class: u16) {
        self.name(name, true);
        self.u16(qtype);
        self.u16(class);
    }

    pub(super) fn rec(&mut self, r: &Rec, ttl: u32, flush: bool) {
        self.name(&r.name, true);
        self.u16(r.data.rtype());
        self.u16(CLASS_IN | if flush { TOP } else { 0 });
        self.b.extend(ttl.to_be_bytes());
        let at = self.b.len();
        self.u16(0);
        match &r.data {
            Data::A(a) => self.b.extend(a.octets()),
            Data::Aaaa(a) => self.b.extend(a.octets()),
            Data::Ptr(n) => self.name(n, true),
            Data::Srv { port, target } => {
                self.b.extend([0, 0, 0, 0]);
                self.u16(*port);
                self.name(target, true);
            }
            Data::Txt(t) => self.b.extend(txt_bytes(t)),
            Data::Nsec { next, types } => {
                // the next name uncompressed: what every reader takes
                self.name(next, false);
                self.b.extend(type_bitmap(types));
            }
        }
        let n = (self.b.len() - at - 2) as u16;
        self.b[at..at + 2].copy_from_slice(&n.to_be_bytes());
    }
}

/// How a record goes into an answer: TTL and cache-flush bit.
#[derive(Clone, Copy)]
pub(super) enum Style {
    /// multicast or QU answer: its TTL, cache flush on unique records
    Normal,
    /// legacy unicast: TTL at most 10, no cache flush
    Legacy,
    /// goodbye: TTL 0
    Goodbye,
}

fn style(r: &Rec, s: Style) -> (u32, bool) {
    match s {
        Style::Normal => (r.ttl, r.unique),
        Style::Legacy => (r.ttl.min(TTL_LEGACY), false),
        Style::Goodbye => (0, r.unique),
    }
}

/// Answers in as many packets as they need, `qs` repeated in each (legacy unicast), the additional records in the first
/// as far as they fit.
pub(super) fn responses(id: u16, qs: &[Q], an: &[Rec], ar: &[Rec], s: Style) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let mut w = Wr::new(id, RESPONSE);
        // each question as it came, uncompressed: the bytes the reader took, so within 255
        for q in qs {
            w.b.extend(&q.wire);
            w.u16(q.qtype);
            w.u16(q.qclass | if q.qu { TOP } else { 0 });
        }
        let mut n_an = 0;
        while i < an.len() {
            let m = w.mark();
            let (ttl, flush) = style(&an[i], s);
            w.rec(&an[i], ttl, flush);
            if w.b.len() > MAX_PKT && n_an > 0 {
                w.back(m);
                break;
            }
            n_an += 1;
            i += 1;
        }
        let mut n_ar = 0;
        if out.is_empty() {
            for r in ar {
                let m = w.mark();
                let (ttl, flush) = style(r, s);
                w.rec(r, ttl, flush);
                if w.b.len() > MAX_PKT {
                    w.back(m);
                    break;
                }
                n_ar += 1;
            }
        }
        w.counts([qs.len(), n_an, 0, n_ar]);
        out.push(w.b);
        if i >= an.len() {
            return out;
        }
    }
}
