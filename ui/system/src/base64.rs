// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Standard base64 (RFC 4648): the screenshot of MCP and the web remote, the keys of SSH.

/// `d` as base64, with padding.
pub fn encode(d: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::with_capacity(d.len().div_ceil(3) * 4);
    for c in d.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            o.push(if k <= c.len() { T[(n >> (18 - 6 * k) & 63) as usize] as char } else { '=' });
        }
    }
    o
}

fn value(c: u8) -> Option<u32> {
    Some(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    } as u32)
}

/// Standard base64 (RFC 4648), padding optional; None for anything else.
pub fn decode(s: &str) -> Option<Vec<u8>> {
    let t = s.trim_end_matches('=');
    if s.len() - t.len() > 2 || t.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(t.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for &c in t.as_bytes() {
        acc = (acc << 6) | value(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // the bits left over must be zero (one encoding per key)
    (acc == 0).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4648() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn round_trip_and_rejects() {
        for v in [&b""[..], b"f", b"fo", b"foo", b"foobar", &[0u8, 255, 128, 7, 9]] {
            assert_eq!(decode(&crate::base64::encode(v)).as_deref(), Some(v));
        }
        assert_eq!(decode("Zm9vYg").as_deref(), Some(&b"foob"[..]));
        for bad in ["Zm9v!", "Z", "Zm9vY===", "Zh=="] {
            assert_eq!(decode(bad), None, "{}", bad);
        }
    }
}
