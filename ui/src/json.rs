// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Minimal JSON (RFC 8259) value, parser and writer for the MCP endpoint (the crate has no dependencies).

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn get(&self, k: &str) -> Option<&J> {
        match self {
            J::Obj(v) => v.iter().find(|(a, _)| a == k).map(|(_, b)| b),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn num(&self) -> Option<f64> {
        match self {
            J::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn obj(kv: &[(&str, J)]) -> J {
        J::Obj(kv.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
    }
    pub fn s(v: &str) -> J {
        J::Str(v.to_string())
    }

    pub fn write(&self, o: &mut String) {
        match self {
            J::Null => o.push_str("null"),
            J::Bool(b) => o.push_str(if *b { "true" } else { "false" }),
            J::Num(n) if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 => o.push_str(&format!("{}", *n as i64)),
            J::Num(n) if n.is_finite() => o.push_str(&format!("{}", n)),
            J::Num(_) => o.push_str("null"),
            J::Str(s) => {
                o.push('"');
                for c in s.chars() {
                    match c {
                        '"' => o.push_str("\\\""),
                        '\\' => o.push_str("\\\\"),
                        '\n' => o.push_str("\\n"),
                        '\r' => o.push_str("\\r"),
                        '\t' => o.push_str("\\t"),
                        c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
                        c => o.push(c),
                    }
                }
                o.push('"');
            }
            J::Arr(v) => {
                o.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        o.push(',');
                    }
                    x.write(o);
                }
                o.push(']');
            }
            J::Obj(v) => {
                o.push('{');
                for (i, (k, x)) in v.iter().enumerate() {
                    if i > 0 {
                        o.push(',');
                    }
                    J::Str(k.clone()).write(o);
                    o.push(':');
                    x.write(o);
                }
                o.push('}');
            }
        }
    }
    pub fn to_string(&self) -> String {
        let mut s = String::new();
        self.write(&mut s);
        s
    }
}

/// Deepest nesting taken (a request is 4 levels deep): the parser recurses, the input comes from the network.
const MAX_DEPTH: usize = 64;

pub fn parse(s: &str) -> Option<J> {
    let b = s.as_bytes();
    let mut p = 0;
    let v = value(b, &mut p, 0)?;
    ws(b, &mut p);
    (p == b.len()).then_some(v)
}

fn ws(b: &[u8], p: &mut usize) {
    while *p < b.len() && b[*p].is_ascii_whitespace() {
        *p += 1;
    }
}

fn value(b: &[u8], p: &mut usize, depth: usize) -> Option<J> {
    if depth > MAX_DEPTH {
        return None;
    }
    ws(b, p);
    match *b.get(*p)? {
        b'{' => {
            *p += 1;
            let mut v = Vec::new();
            ws(b, p);
            if b.get(*p) == Some(&b'}') {
                *p += 1;
                return Some(J::Obj(v));
            }
            loop {
                ws(b, p);
                let J::Str(k) = string(b, p)? else { return None };
                ws(b, p);
                if b.get(*p) != Some(&b':') {
                    return None;
                }
                *p += 1;
                v.push((k, value(b, p, depth + 1)?));
                ws(b, p);
                match b.get(*p)? {
                    b',' => *p += 1,
                    b'}' => {
                        *p += 1;
                        return Some(J::Obj(v));
                    }
                    _ => return None,
                }
            }
        }
        b'[' => {
            *p += 1;
            let mut v = Vec::new();
            ws(b, p);
            if b.get(*p) == Some(&b']') {
                *p += 1;
                return Some(J::Arr(v));
            }
            loop {
                v.push(value(b, p, depth + 1)?);
                ws(b, p);
                match b.get(*p)? {
                    b',' => *p += 1,
                    b']' => {
                        *p += 1;
                        return Some(J::Arr(v));
                    }
                    _ => return None,
                }
            }
        }
        b'"' => string(b, p),
        b't' if b[*p..].starts_with(b"true") => {
            *p += 4;
            Some(J::Bool(true))
        }
        b'f' if b[*p..].starts_with(b"false") => {
            *p += 5;
            Some(J::Bool(false))
        }
        b'n' if b[*p..].starts_with(b"null") => {
            *p += 4;
            Some(J::Null)
        }
        _ => {
            let st = *p;
            while *p < b.len() && matches!(b[*p], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                *p += 1;
            }
            std::str::from_utf8(&b[st..*p]).ok()?.parse().ok().map(J::Num)
        }
    }
}

fn string(b: &[u8], p: &mut usize) -> Option<J> {
    if b.get(*p) != Some(&b'"') {
        return None;
    }
    *p += 1;
    let mut out: Vec<u8> = Vec::new();
    loop {
        let c = *b.get(*p)?;
        *p += 1;
        match c {
            b'"' => return String::from_utf8(out).ok().map(J::Str),
            b'\\' => {
                let e = *b.get(*p)?;
                *p += 1;
                match e {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'u' => {
                        let h = std::str::from_utf8(b.get(*p..*p + 4)?).ok()?;
                        *p += 4;
                        let mut cp = u32::from_str_radix(h, 16).ok()?;
                        // surrogate pair
                        if (0xd800..0xdc00).contains(&cp) && b.get(*p..*p + 2) == Some(b"\\u") {
                            let lo = u32::from_str_radix(std::str::from_utf8(b.get(*p + 2..*p + 6)?).ok()?, 16).ok()?;
                            *p += 6;
                            cp = if (0xdc00..0xe000).contains(&lo) { 0x10000 + ((cp - 0xd800) << 10) + (lo - 0xdc00) } else { 0xfffd };
                        }
                        let ch = char::from_u32(cp).unwrap_or('\u{fffd}');
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                    c => out.push(c),
                }
            }
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let t = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"scpi","arguments":{"command":":TIM:SCAL 1e-3"}},"x":[true,null,-1.5e3,"a\"é"]}"#;
        let v = parse(t).unwrap();
        assert_eq!(v.get("id").and_then(J::num), Some(3.0));
        assert_eq!(v.get("params").and_then(|p| p.get("arguments")).and_then(|a| a.get("command")).and_then(J::str), Some(":TIM:SCAL 1e-3"));
        assert_eq!(parse(&v.to_string()), Some(v.clone()));
        assert!(parse("{\"a\":}").is_none() && parse("[1,2").is_none());
    }

    #[test]
    fn hostile_input_is_refused_without_a_crash() {
        // 1 MB of open brackets, the largest request the web remote takes
        assert!(parse(&"[".repeat(1 << 20)).is_none());
        assert!(parse(&"{\"a\":".repeat(1 << 18)).is_none());
        assert!(parse(&format!("{}1{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH))).is_some());
        // a high surrogate with anything but a low one after it
        assert_eq!(parse(r#""\ud800\u0041""#), Some(J::s("\u{fffd}")));
    }
}
