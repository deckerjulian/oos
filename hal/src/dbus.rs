// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! A D-Bus client of its own, for the system bus of docs/bluetooth.md: the wire format of the D-Bus specification
//! (little endian), SASL EXTERNAL, method calls and their replies, and answers to the calls that come in for an object
//! of ours (BlueZ's pairing agent, `bluez.rs`). Only what `bluez.rs` needs; std and libc only (ADR 0013).

use std::io::{Read, Write};
use std::time::{Duration, Instant};

/// The system bus of OOS (`/etc/oos-bt/dbus.conf`).
pub const SYSTEM_BUS: &str = "/run/dbus/system_bus_socket";

pub const METHOD_CALL: u8 = 1;
pub const METHOD_RETURN: u8 = 2;
pub const ERROR: u8 = 3;
pub const SIGNAL: u8 = 4;
const NO_REPLY_EXPECTED: u8 = 1;

/// A value of the D-Bus type system.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Byte(u8),
    Bool(bool),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    Double(f64),
    Str(String),
    Path(String),
    Sig(String),
    /// the signature of one element, the elements
    Array(String, Vec<Value>),
    Struct(Vec<Value>),
    /// one entry of a dictionary (`{kv}`, only inside an array)
    Entry(Box<Value>, Box<Value>),
    Variant(Box<Value>),
}

impl Value {
    pub fn sig(&self) -> String {
        match self {
            Value::Byte(_) => "y".into(),
            Value::Bool(_) => "b".into(),
            Value::I16(_) => "n".into(),
            Value::U16(_) => "q".into(),
            Value::I32(_) => "i".into(),
            Value::U32(_) => "u".into(),
            Value::I64(_) => "x".into(),
            Value::U64(_) => "t".into(),
            Value::Double(_) => "d".into(),
            Value::Str(_) => "s".into(),
            Value::Path(_) => "o".into(),
            Value::Sig(_) => "g".into(),
            Value::Array(e, _) => format!("a{}", e),
            Value::Struct(v) => format!("({})", v.iter().map(Value::sig).collect::<String>()),
            Value::Entry(k, v) => format!("{{{}{}}}", k.sig(), v.sig()),
            Value::Variant(_) => "v".into(),
        }
    }

    /// The value inside a variant (or the value itself).
    pub fn inner(&self) -> &Value {
        match self {
            Value::Variant(v) => v.inner(),
            v => v,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self.inner() {
            Value::Str(s) | Value::Path(s) | Value::Sig(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self.inner() {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match *self.inner() {
            Value::Byte(v) => Some(v as i64),
            Value::I16(v) => Some(v as i64),
            Value::U16(v) => Some(v as i64),
            Value::I32(v) => Some(v as i64),
            Value::U32(v) => Some(v as i64),
            Value::I64(v) => Some(v),
            Value::U64(v) => Some(v as i64),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self.inner() {
            Value::Array(_, v) => Some(v),
            _ => None,
        }
    }

    /// The (key, value) pairs of a dictionary (`a{..}`).
    pub fn entries(&self) -> impl Iterator<Item = (&Value, &Value)> {
        self.as_array().unwrap_or(&[]).iter().filter_map(|e| match e {
            Value::Entry(k, v) => Some((k.as_ref(), v.as_ref())),
            _ => None,
        })
    }

    /// The value of key `k` in a dictionary with string keys.
    pub fn get(&self, k: &str) -> Option<&Value> {
        self.entries().find(|(key, _)| key.as_str() == Some(k)).map(|(_, v)| v)
    }
}

fn alignment(c: u8) -> usize {
    match c {
        b'y' | b'g' | b'v' => 1,
        b'n' | b'q' => 2,
        b'x' | b't' | b'd' | b'(' | b'{' => 8,
        _ => 4,
    }
}

fn pad(b: &mut Vec<u8>, n: usize) {
    while !b.len().is_multiple_of(n) {
        b.push(0);
    }
}

fn put(b: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Byte(x) => b.push(*x),
        Value::Bool(x) => put(b, &Value::U32(*x as u32)),
        Value::I16(x) => {
            pad(b, 2);
            b.extend_from_slice(&x.to_le_bytes())
        }
        Value::U16(x) => {
            pad(b, 2);
            b.extend_from_slice(&x.to_le_bytes())
        }
        Value::I32(x) => {
            pad(b, 4);
            b.extend_from_slice(&x.to_le_bytes())
        }
        Value::U32(x) => {
            pad(b, 4);
            b.extend_from_slice(&x.to_le_bytes())
        }
        Value::I64(x) => {
            pad(b, 8);
            b.extend_from_slice(&x.to_le_bytes())
        }
        Value::U64(x) => {
            pad(b, 8);
            b.extend_from_slice(&x.to_le_bytes())
        }
        Value::Double(x) => {
            pad(b, 8);
            b.extend_from_slice(&x.to_le_bytes())
        }
        Value::Str(s) | Value::Path(s) => {
            put(b, &Value::U32(s.len() as u32));
            b.extend_from_slice(s.as_bytes());
            b.push(0);
        }
        Value::Sig(s) => {
            b.push(s.len() as u8);
            b.extend_from_slice(s.as_bytes());
            b.push(0);
        }
        Value::Array(e, items) => {
            pad(b, 4);
            let at = b.len();
            b.extend_from_slice(&[0; 4]);
            pad(b, alignment(e.as_bytes().first().copied().unwrap_or(b'y')));
            let start = b.len();
            for i in items {
                put(b, i);
            }
            let n = (b.len() - start) as u32;
            b[at..at + 4].copy_from_slice(&n.to_le_bytes());
        }
        Value::Struct(v) => {
            pad(b, 8);
            for i in v {
                put(b, i);
            }
        }
        Value::Entry(k, v) => {
            pad(b, 8);
            put(b, k);
            put(b, v);
        }
        Value::Variant(v) => {
            put(b, &Value::Sig(v.sig()));
            put(b, v);
        }
    }
}

/// The length of the first complete type in a signature.
fn one_type(sig: &[u8]) -> Result<usize, String> {
    match sig.first() {
        None => Err("empty signature".into()),
        Some(b'a') => Ok(1 + one_type(&sig[1..])?),
        Some(&open @ (b'(' | b'{')) => {
            let close = if open == b'(' { b')' } else { b'}' };
            let mut i = 1;
            while sig.get(i) != Some(&close) {
                if i >= sig.len() {
                    return Err("unclosed signature".into());
                }
                i += one_type(&sig[i..])?;
            }
            Ok(i + 1)
        }
        Some(_) => Ok(1),
    }
}

/// How deep variants, arrays, structs and entries may nest (the specification: 32 arrays and 32 structs).
const DEPTH_MAX: usize = 64;
/// The longest array of the specification (64 MiB).
const ARRAY_MAX: usize = 1 << 26;

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
    /// containers open around the value being read
    depth: usize,
}

impl Reader<'_> {
    fn align(&mut self, n: usize) {
        self.p = self.p.div_ceil(n) * n;
    }

    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let s = self.b.get(self.p..self.p + n).ok_or("message too short")?;
        self.p += n;
        Ok(s)
    }

    fn u32(&mut self) -> Result<u32, String> {
        self.align(4);
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    /// A string, a path or a signature: UTF-8 as it is (no replacement), and the NUL the specification puts after it.
    fn string(&mut self, len: usize) -> Result<String, String> {
        let s = String::from_utf8(self.take(len)?.to_vec()).map_err(|_| "a string that is not UTF-8")?;
        if self.take(1)? != [0] {
            return Err("a string without its NUL".into());
        }
        Ok(s)
    }

    /// One value of the complete type at the start of `sig`. A message that does not follow the specification is an
    /// error, never a panic or an endless loop: oos-ui aborts on a panic.
    fn value(&mut self, sig: &[u8]) -> Result<Value, String> {
        if self.depth >= DEPTH_MAX {
            return Err("values nested too deep".into());
        }
        self.depth += 1;
        let v = self.one(sig);
        self.depth -= 1;
        v
    }

    fn one(&mut self, sig: &[u8]) -> Result<Value, String> {
        let eight = |r: &mut Reader| -> Result<[u8; 8], String> {
            r.align(8);
            let s = r.take(8)?;
            Ok([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]])
        };
        let Some(&first) = sig.first() else { return Err("empty signature".into()) };
        Ok(match first {
            b'y' => Value::Byte(self.take(1)?[0]),
            b'b' => Value::Bool(self.u32()? != 0),
            b'n' | b'q' => {
                self.align(2);
                let s = self.take(2)?;
                let v = [s[0], s[1]];
                if first == b'n' {
                    Value::I16(i16::from_le_bytes(v))
                } else {
                    Value::U16(u16::from_le_bytes(v))
                }
            }
            b'i' => Value::I32(self.u32()? as i32),
            b'u' | b'h' => Value::U32(self.u32()?),
            b'x' => Value::I64(i64::from_le_bytes(eight(self)?)),
            b't' => Value::U64(u64::from_le_bytes(eight(self)?)),
            b'd' => Value::Double(f64::from_le_bytes(eight(self)?)),
            b's' | b'o' => {
                let n = self.u32()? as usize;
                let s = self.string(n)?;
                if first == b's' {
                    Value::Str(s)
                } else {
                    Value::Path(s)
                }
            }
            b'g' => {
                let n = self.take(1)?[0] as usize;
                Value::Sig(self.string(n)?)
            }
            b'v' => {
                let n = self.take(1)?[0] as usize;
                let s = self.string(n)?;
                // exactly one complete type, and not a dictionary entry outside an array
                if one_type(s.as_bytes())? != s.len() || s.starts_with('{') {
                    return Err(format!("variant signature '{}' is not one type", s));
                }
                Value::Variant(Box::new(self.value(s.as_bytes())?))
            }
            b'a' => {
                let e = &sig[1..1 + one_type(&sig[1..])?];
                let n = self.u32()? as usize;
                if n > ARRAY_MAX {
                    return Err("array longer than the specification allows".into());
                }
                self.align(alignment(e[0]));
                let end = self.p + n;
                let mut items = Vec::new();
                while self.p < end {
                    let at = self.p;
                    items.push(self.value(e)?);
                    if self.p == at {
                        return Err(format!("array of '{}' does not advance", String::from_utf8_lossy(e)));
                    }
                    if self.p > end {
                        return Err(format!(
                            "an element of '{}' past the end of its array",
                            String::from_utf8_lossy(e)
                        ));
                    }
                }
                Value::Array(String::from_utf8_lossy(e).into_owned(), items)
            }
            b'(' => {
                self.align(8);
                let inner = &sig[1..one_type(sig)? - 1];
                if inner.is_empty() {
                    return Err("empty struct".into());
                }
                let mut v = Vec::new();
                let mut i = 0;
                while i < inner.len() {
                    v.push(self.value(&inner[i..])?);
                    i += one_type(&inner[i..])?;
                }
                Value::Struct(v)
            }
            b'{' => {
                self.align(8);
                // `{kv}`: a key and one value, nothing else
                let k_len = one_type(&sig[1..])?;
                if one_type(sig)? != k_len + one_type(&sig[1 + k_len..])? + 2 {
                    return Err("dictionary entry is not a key and a value".into());
                }
                let k = self.value(&sig[1..])?;
                let v = self.value(&sig[1 + k_len..])?;
                Value::Entry(Box::new(k), Box::new(v))
            }
            c => return Err(format!("type '{}' is not supported", c as char)),
        })
    }

    /// The values of a whole signature.
    fn values(&mut self, sig: &str) -> Result<Vec<Value>, String> {
        let s = sig.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < s.len() {
            out.push(self.value(&s[i..])?);
            i += one_type(&s[i..])?;
        }
        Ok(out)
    }
}

/// One message on the bus.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Message {
    pub kind: u8,
    pub flags: u8,
    pub serial: u32,
    pub path: Option<String>,
    pub interface: Option<String>,
    pub member: Option<String>,
    pub error: Option<String>,
    pub reply_serial: Option<u32>,
    pub destination: Option<String>,
    pub sender: Option<String>,
    pub body: Vec<Value>,
}

impl Message {
    pub fn call(dest: &str, path: &str, iface: &str, member: &str, body: Vec<Value>) -> Message {
        Message {
            kind: METHOD_CALL,
            path: Some(path.into()),
            interface: Some(iface.into()),
            member: Some(member.into()),
            destination: Some(dest.into()),
            body,
            ..Message::default()
        }
    }

    /// The answer to an incoming call.
    pub fn reply(to: &Message, body: Vec<Value>) -> Message {
        Message {
            kind: METHOD_RETURN,
            flags: NO_REPLY_EXPECTED,
            reply_serial: Some(to.serial),
            destination: to.sender.clone(),
            body,
            ..Message::default()
        }
    }

    /// An error as the answer to an incoming call.
    pub fn error(to: &Message, name: &str, text: &str) -> Message {
        Message {
            kind: ERROR,
            flags: NO_REPLY_EXPECTED,
            error: Some(name.into()),
            reply_serial: Some(to.serial),
            destination: to.sender.clone(),
            body: vec![Value::Str(text.into())],
            ..Message::default()
        }
    }

    /// Whether this is a call of `iface.member`.
    pub fn is_call(&self, iface: &str, member: &str) -> bool {
        self.kind == METHOD_CALL && self.interface.as_deref() == Some(iface) && self.member.as_deref() == Some(member)
    }

    pub fn encode(&self) -> Vec<u8> {
        let sig: String = self.body.iter().map(Value::sig).collect();
        let mut body = Vec::new();
        for v in &self.body {
            put(&mut body, v);
        }
        let mut fields = Vec::new();
        let mut field =
            |code: u8, v: Value| fields.push(Value::Struct(vec![Value::Byte(code), Value::Variant(Box::new(v))]));
        if let Some(p) = &self.path {
            field(1, Value::Path(p.clone()));
        }
        if let Some(i) = &self.interface {
            field(2, Value::Str(i.clone()));
        }
        if let Some(m) = &self.member {
            field(3, Value::Str(m.clone()));
        }
        if let Some(e) = &self.error {
            field(4, Value::Str(e.clone()));
        }
        if let Some(r) = self.reply_serial {
            field(5, Value::U32(r));
        }
        if let Some(d) = &self.destination {
            field(6, Value::Str(d.clone()));
        }
        if !sig.is_empty() {
            field(8, Value::Sig(sig));
        }
        let mut b = vec![b'l', self.kind, self.flags, 1];
        b.extend_from_slice(&(body.len() as u32).to_le_bytes());
        b.extend_from_slice(&self.serial.to_le_bytes());
        put(&mut b, &Value::Array("(yv)".into(), fields));
        pad(&mut b, 8);
        b.extend_from_slice(&body);
        b
    }

    /// One whole message; the bytes must hold it all (`Conn` reads the lengths first).
    pub fn decode(b: &[u8]) -> Result<Message, String> {
        if b.len() < 16 || b[0] != b'l' {
            return Err("not a little-endian D-Bus message".into());
        }
        let mut r = Reader { b, p: 12, depth: 0 };
        let fields = r.value(b"a(yv)")?;
        r.align(8);
        let mut m = Message {
            kind: b[1],
            flags: b[2],
            serial: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
            ..Message::default()
        };
        let mut sig = String::new();
        for f in fields.as_array().unwrap_or(&[]) {
            let Value::Struct(v) = f else { continue };
            let (Some(Value::Byte(code)), Some(val)) = (v.first(), v.get(1)) else { continue };
            let s = val.as_str().map(str::to_string);
            match code {
                1 => m.path = s,
                2 => m.interface = s,
                3 => m.member = s,
                4 => m.error = s,
                5 => m.reply_serial = val.as_i64().map(|x| x as u32),
                6 => m.destination = s,
                7 => m.sender = s,
                8 => sig = s.unwrap_or_default(),
                _ => {}
            }
        }
        let mut body = Reader { b: b.get(r.p..).unwrap_or_default(), p: 0, depth: 0 };
        m.body = body.values(&sig)?;
        Ok(m)
    }
}

/// The longest message taken (bytes). The specification allows 128 MiB; what bluetoothd sends to oos-ui is a few
/// KiB, and the length is read before the message, so a broken one would have the program allocate up to 8 GiB.
pub const MAX_LEN: usize = 1 << 20;

/// The length of the whole message from its first 16 bytes; an error above `MAX_LEN`.
pub fn total_len(head: &[u8; 16]) -> Result<usize, String> {
    let body = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    let fields = u32::from_le_bytes([head[12], head[13], head[14], head[15]]) as usize;
    let n = (16 + fields).div_ceil(8) * 8 + body;
    if n > MAX_LEN {
        return Err(format!("bus: a message of {} bytes", n));
    }
    Ok(n)
}

/// What answers the calls that come in for our objects: a reply, or None for "no such method".
pub type Handler<'a> = &'a mut dyn FnMut(&Message) -> Option<Message>;

/// A connection to a bus.
pub struct Conn {
    s: std::os::unix::net::UnixStream,
    serial: u32,
    /// our unique name on the bus
    pub name: String,
}

impl Conn {
    /// Connects to the bus at `path`, authenticates as this process's user and says hello.
    pub fn open(path: &str) -> Result<Conn, String> {
        let s = std::os::unix::net::UnixStream::connect(path).map_err(|e| format!("{}: {}", path, e))?;
        s.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
        let mut c = Conn { s, serial: 0, name: String::new() };
        // the user id for SASL EXTERNAL: the owner of /proc/self is this process's (no unsafe libc call)
        let uid = std::os::unix::fs::MetadataExt::uid(
            &std::fs::metadata("/proc/self").map_err(|e| format!("/proc/self: {}", e))?,
        )
        .to_string();
        let hex: String = uid.bytes().map(|b| format!("{:02x}", b)).collect();
        c.s.write_all(format!("\0AUTH EXTERNAL {}\r\n", hex).as_bytes()).map_err(|e| e.to_string())?;
        let mut line = Vec::new();
        let mut one = [0u8; 1];
        while !line.ends_with(b"\r\n") {
            c.s.read_exact(&mut one).map_err(|e| format!("bus authentication: {}", e))?;
            line.push(one[0]);
            if line.len() > 256 {
                return Err("bus authentication: answer too long".into());
            }
        }
        if !line.starts_with(b"OK ") {
            return Err(format!("bus authentication refused: {}", String::from_utf8_lossy(&line).trim()));
        }
        c.s.write_all(b"BEGIN\r\n").map_err(|e| e.to_string())?;
        let hello =
            Message::call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "Hello", vec![]);
        let r = c.call(hello, Duration::from_secs(5), &mut |_| None)?;
        c.name = r.first().and_then(Value::as_str).unwrap_or_default().to_string();
        Ok(c)
    }

    fn send(&mut self, m: &mut Message) -> Result<u32, String> {
        self.serial = self.serial.wrapping_add(1).max(1);
        m.serial = self.serial;
        self.s.write_all(&m.encode()).map_err(|e| format!("bus: {}", e))?;
        Ok(m.serial)
    }

    /// The next message, waiting up to `t`; None after the time.
    fn read(&mut self, t: Duration) -> Result<Option<Message>, String> {
        self.s.set_read_timeout(Some(t.max(Duration::from_millis(1)))).map_err(|e| e.to_string())?;
        let mut head = [0u8; 16];
        match self.s.read_exact(&mut head) {
            Ok(()) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                return Ok(None)
            }
            Err(e) => return Err(format!("bus: {}", e)),
        }
        // the rest of a message that began to arrive comes with it
        self.s.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
        let mut b = head.to_vec();
        b.resize(total_len(&head)?, 0);
        self.s.read_exact(&mut b[16..]).map_err(|e| format!("bus: {}", e))?;
        Message::decode(&b).map(Some)
    }

    /// Answers a call that came in for one of our objects.
    fn answer(&mut self, m: &Message, h: Handler) -> Result<(), String> {
        let mut r =
            h(m).unwrap_or_else(|| Message::error(m, "org.freedesktop.DBus.Error.UnknownMethod", "no such method"));
        if m.flags & NO_REPLY_EXPECTED == 0 {
            self.send(&mut r)?;
        }
        Ok(())
    }

    /// Calls a method and waits up to `t` for its reply; calls that come in meanwhile go to `h`.
    pub fn call(&mut self, mut m: Message, t: Duration, h: Handler) -> Result<Vec<Value>, String> {
        let serial = self.send(&mut m)?;
        let end = Instant::now() + t;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("{}: no answer", m.member.as_deref().unwrap_or("call")));
            }
            let Some(r) = self.read(left)? else { continue };
            if r.reply_serial == Some(serial) && r.kind == METHOD_RETURN {
                return Ok(r.body);
            }
            if r.reply_serial == Some(serial) && r.kind == ERROR {
                let text = r.body.first().and_then(Value::as_str).unwrap_or_default();
                return Err(format!(
                    "{}{}{}",
                    r.error.as_deref().unwrap_or("error"),
                    if text.is_empty() { "" } else { ": " },
                    text
                ));
            }
            if r.kind == METHOD_CALL {
                self.answer(&r, h)?;
            }
        }
    }

    /// Answers the calls that come in within `t`.
    pub fn serve(&mut self, t: Duration, h: Handler) -> Result<(), String> {
        let end = Instant::now() + t;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(());
            }
            match self.read(left)? {
                Some(r) if r.kind == METHOD_CALL => self.answer(&r, h)?,
                Some(_) => {}
                None => return Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_encodes_as_the_specification_says() {
        let mut m =
            Message::call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "Hello", vec![]);
        m.serial = 1;
        let b = m.encode();
        assert_eq!(&b[..12], &[b'l', 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(b.len() % 8, 0, "the header ends on 8 bytes");
        assert_eq!(Message::decode(&b).unwrap(), m);
    }

    #[test]
    fn values_round_trip_with_their_alignment() {
        let dict = Value::Array(
            "{sv}".into(),
            vec![
                Value::Entry(
                    Box::new(Value::Str("Paired".into())),
                    Box::new(Value::Variant(Box::new(Value::Bool(true)))),
                ),
                Value::Entry(Box::new(Value::Str("RSSI".into())), Box::new(Value::Variant(Box::new(Value::I16(-61))))),
                Value::Entry(
                    Box::new(Value::Str("UUIDs".into())),
                    Box::new(Value::Variant(Box::new(Value::Array("s".into(), vec![Value::Str("1812".into())])))),
                ),
            ],
        );
        let body = vec![
            Value::Path("/org/bluez/hci0/dev_X".into()),
            Value::U32(123456),
            Value::U16(2),
            dict.clone(),
            Value::U64(7),
            Value::Byte(9),
        ];
        let mut m = Message::call("org.bluez", "/", "i.f", "M", body.clone());
        m.serial = 42;
        let back = Message::decode(&m.encode()).unwrap();
        assert_eq!(back.body, body);
        assert_eq!(back.body[3].get("RSSI").and_then(Value::as_i64), Some(-61));
        assert_eq!(back.body[3].get("Paired").and_then(Value::as_bool), Some(true));
        assert_eq!(back.body[3].sig(), "a{sv}");
    }

    #[test]
    fn replies_and_errors_answer_the_call() {
        let mut call = Message::call("x", "/a", "org.bluez.Agent1", "DisplayPasskey", vec![]);
        call.serial = 7;
        call.sender = Some(":1.3".into());
        let r = Message::reply(&call, vec![]);
        assert_eq!((r.kind, r.reply_serial, r.destination.as_deref()), (METHOD_RETURN, Some(7), Some(":1.3")));
        let e = Message::decode(&Message::error(&call, "org.bluez.Error.Rejected", "no").encode()).unwrap();
        assert_eq!(
            (e.kind, e.error.as_deref(), e.body[0].as_str()),
            (ERROR, Some("org.bluez.Error.Rejected"), Some("no"))
        );
        assert!(call.is_call("org.bluez.Agent1", "DisplayPasskey"));
    }

    #[test]
    fn signatures_split_into_complete_types() {
        assert_eq!(one_type(b"a{oa{sa{sv}}}s").unwrap(), 13);
        assert_eq!(one_type(b"(yv)u").unwrap(), 4);
        assert!(one_type(b"(yv").is_err());
        let mut short = Message::call("x", "/", "i", "m", vec![Value::Str("abc".into())]).encode();
        short.truncate(short.len() - 2);
        assert!(Message::decode(&short).is_err());
    }

    /// A reply from the bus with the body signature `sig` and the raw `body`, as a broken or hostile peer might send.
    fn raw(sig: &str, body: &[u8]) -> Vec<u8> {
        let fields = vec![
            Value::Struct(vec![Value::Byte(1), Value::Variant(Box::new(Value::Path("/".into())))]),
            Value::Struct(vec![Value::Byte(8), Value::Variant(Box::new(Value::Sig(sig.into())))]),
        ];
        let mut b = vec![b'l', METHOD_RETURN, 0, 1];
        b.extend_from_slice(&(body.len() as u32).to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        put(&mut b, &Value::Array("(yv)".into(), fields));
        pad(&mut b, 8);
        b.extend_from_slice(body);
        b
    }

    /// The decoder answers a message that breaks the specification with an error: no panic (oos-ui aborts on one),
    /// no endless loop. The inputs of the review of 2026-11-16.
    #[test]
    fn a_broken_message_is_an_error() {
        // a variant with an empty signature, with `{` and with `{s`
        assert!(Message::decode(&raw("v", &[0, 0, 0, 0])).is_err());
        assert!(Message::decode(&raw("v", &[1, b'{', 0, 0, 0, 0, 0, 0])).is_err());
        assert!(Message::decode(&raw("v", &[2, b'{', b's', 0, 0, 0, 0, 0, 1, 0, 0, 0, b'x', 0])).is_err());
        // two types in one variant, a dictionary entry without its value
        assert!(Message::decode(&raw("v", &[2, b's', b's', 0, 1, 0, 0, 0, b'x', 0])).is_err());
        assert!(Message::decode(&raw("a{s}", &[8, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, b'x', 0, 0, 0])).is_err());
        // an array of empty structs with a length: it used to loop for ever, growing its Vec
        let (tx, rx) = std::sync::mpsc::channel();
        let b = raw("a()", &[8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        std::thread::spawn(move || {
            let _ = tx.send(Message::decode(&b).is_err());
        });
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(true));
        // variants in variants, deeper than the specification allows
        let mut deep = Vec::new();
        for _ in 0..200 {
            deep.extend_from_slice(&[1, b'v', 0]);
        }
        deep.extend_from_slice(&[1, b'y', 0, 7]);
        assert!(Message::decode(&raw("v", &deep)).is_err());
        // header fields that end before the padding to 8 bytes, without the padding, and no body
        let cut = [b'l', METHOD_RETURN, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 5, 0, 0, 0, 9, 1, b'y', 0, 7];
        assert!(Message::decode(&cut).unwrap().body.is_empty());
        // a string that is not UTF-8, one without its NUL, and an element past the end of its array (review of #204)
        assert!(Message::decode(&raw("s", &[1, 0, 0, 0, 0xff, 0])).is_err());
        assert!(Message::decode(&raw("s", &[1, 0, 0, 0, b'x', b'y'])).is_err());
        assert!(Message::decode(&raw("as", &[4, 0, 0, 0, 1, 0, 0, 0, b'x', 0])).is_err());
        assert!(Message::decode(&raw("as", &[6, 0, 0, 0, 1, 0, 0, 0, b'x', 0])).is_ok());
        // and what is right still decodes
        let ok = Message::decode(&raw("v", &[1, b'y', 0, 7])).unwrap();
        assert_eq!(ok.body, vec![Value::Variant(Box::new(Value::Byte(7)))]);
    }
}
