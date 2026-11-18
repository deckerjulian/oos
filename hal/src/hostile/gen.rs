// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The generator of hostile input (ADR 0047): a seeded xorshift generator, mutations of valid inputs, the loop that
//! feeds a parser and names the input it failed on, and the list of findings that still stand. The same file in hal,
//! dsp and ui (`hostile/gen.rs`; ui's test `the_generator_is_one_file` compares the copies): a change goes into all
//! three. Nothing here reads a clock; the same build gives the same inputs on every computer.

use std::fmt::Write as _;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Mutated inputs per parser in `cargo test`; `OOS_HOSTILE_ITERS` raises it for a long run.
const ITERS: usize = 2000;
/// No input grows past this.
const MAX_LEN: usize = 256 << 10;

/// The mutated inputs of one parser: `OOS_HOSTILE_ITERS` or `ITERS`, divided by `div` for a parser that is slow on
/// one input, at least one.
pub fn iters(div: usize) -> usize {
    let base = std::env::var("OOS_HOSTILE_ITERS").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(ITERS);
    (base / div.max(1)).max(1)
}

/// xorshift64*: the same numbers from the same seed.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        // the state must not be zero
        Rng(seed ^ 0x9e37_79b9_7f4a_7c15 | 1)
    }

    /// The seed of a parser's run, from its name (FNV-1a).
    pub fn named(name: &str) -> Rng {
        Rng::new(name.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)))
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number in `0..n`; 0 for `n == 0`.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            ((self.next() >> 16) % n as u64) as usize
        }
    }

    pub fn pick<'a, T>(&mut self, of: &'a [T]) -> &'a T {
        &of[self.below(of.len())]
    }

    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| (self.next() >> 24) as u8).collect()
    }

    /// A float that parsers trip over: not a number, infinite, at the ends of the range, or just any bits.
    pub fn float(&mut self) -> f64 {
        const EDGE: [f64; 14] = [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            -0.0,
            1.0,
            -1.0,
            1e308,
            -1e308,
            1e-320,
            3.5e38,
            -3.5e38,
            1e-45,
            f64::MIN_POSITIVE,
        ];
        match self.below(4) {
            0 => f64::from_bits(self.next()),
            1 => (self.below(2001) as f64 - 1000.0) / 100.0,
            _ => *self.pick(&EDGE),
        }
    }
}

/// Values at the edge of what a text field holds: numbers at the ends of the integer and float ranges, things that
/// are no number, separators, an empty field, characters of several bytes, and bytes that are no UTF-8.
const TOKENS: &[&[u8]] = &[
    b"",
    b"0",
    b"-0",
    b"1",
    b"-1",
    b"255",
    b"256",
    b"65535",
    b"65536",
    b"2147483647",
    b"-2147483648",
    b"4294967295",
    b"4294967296",
    b"18446744073709551615",
    b"18446744073709551616",
    b"NaN",
    b"nan",
    b"inf",
    b"-inf",
    b"infinity",
    b"1e308",
    b"-1e308",
    b"1e309",
    b"1e-320",
    b"3.5e38",
    b"1e38",
    b"0.0000001",
    b"1e",
    b".",
    b"-",
    b"+",
    b"0x7fffffff",
    b"#9999999999",
    b"#0",
    b" ",
    b"\t",
    b"\n",
    b"\r\n",
    b"\r\n\r\n",
    b"\0",
    b",",
    b";",
    b":",
    b"=",
    b"?",
    b"*",
    b"#",
    b"\"",
    b"'",
    b"/",
    b"\\",
    b"..",
    b"%",
    b"%00",
    b"%ff",
    b"&",
    b"<",
    b">",
    b"{",
    b"}",
    b"[",
    b"]",
    "\u{e9}".as_bytes(),
    "\u{20ac}".as_bytes(),
    "\u{1d11e}".as_bytes(),
    "\u{feff}".as_bytes(),
    "\u{202e}".as_bytes(),
    b"\xff",
    b"\x80",
    b"\xc0\x80",
    b"\xe2\x82",
    b"\xed\xa0\x80",
    b"\xf8\x88\x80\x80\x80",
];

/// A very long token: one character many times.
fn long_token(rng: &mut Rng) -> Vec<u8> {
    let c = *rng.pick(b"9A -.:,/\0\xff(");
    vec![c; *rng.pick(&[64, 255, 256, 300, 1025, 5000])]
}

fn text_token(rng: &mut Rng) -> Vec<u8> {
    if rng.below(12) == 0 {
        long_token(rng)
    } else {
        rng.pick(TOKENS).to_vec()
    }
}

/// A fixed field at the edge of its range: 1, 2, 4 or 8 bytes of zeros, ones, the sign bit, the largest positive
/// number, or a float that is no number, infinite or huge; little or big endian.
fn binary_token(rng: &mut Rng) -> Vec<u8> {
    let mut v = match rng.below(12) {
        0 => vec![0; 1 << rng.below(4)],
        1 => vec![0xff; 1 << rng.below(4)],
        2 => {
            let mut v = vec![0; 1 << rng.below(4)];
            v[0] = 0x80;
            v
        }
        3 => {
            let mut v = vec![0xff; 1 << rng.below(4)];
            v[0] = 0x7f;
            v
        }
        4 => {
            let mut v = vec![0; 1 << rng.below(4)];
            v[0] = 1;
            v
        }
        5 => f32::NAN.to_be_bytes().to_vec(),
        6 => f32::INFINITY.to_be_bytes().to_vec(),
        7 => f32::MAX.to_be_bytes().to_vec(),
        8 => f64::NAN.to_be_bytes().to_vec(),
        9 => f64::NEG_INFINITY.to_be_bytes().to_vec(),
        10 => 1e308f64.to_be_bytes().to_vec(),
        _ => 0f64.to_be_bytes().to_vec(),
    };
    if rng.below(2) == 0 {
        v.reverse();
    }
    v
}

/// A span `(start, length)` inside `len` bytes, short more often than long.
fn span(rng: &mut Rng, len: usize) -> (usize, usize) {
    if len == 0 {
        return (0, 0);
    }
    let at = rng.below(len);
    let most = (len - at).min(if rng.below(4) == 0 { len } else { 16 });
    (at, 1 + rng.below(most))
}

/// The fields of a text: runs of letters, digits and `_ . + -`, as `(start, length)`.
fn fields(b: &[u8]) -> Vec<(usize, usize)> {
    let word = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'+' | b'-');
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if word(b[i]) {
            let s = i;
            while i < b.len() && word(b[i]) {
                i += 1;
            }
            out.push((s, i - s));
        } else {
            i += 1;
        }
    }
    out
}

/// One change of `b`: bits flipped, cut short, a span doubled, removed or overwritten, a value at the edge of its
/// range put in or over a field, the tail of another valid input.
fn mutate_once(rng: &mut Rng, b: &mut Vec<u8>, seeds: &[&[u8]]) {
    match rng.below(12) {
        0 => {
            for _ in 0..1 + rng.below(8) {
                if !b.is_empty() {
                    let at = rng.below(b.len());
                    b[at] ^= 1 << rng.below(8);
                }
            }
        }
        1 => b.truncate(rng.below(b.len() + 1)),
        2 => {
            let (at, n) = span(rng, b.len());
            let copy = b[at..at + n].to_vec();
            let to = rng.below(b.len() + 1);
            let most = if rng.below(8) == 0 { 64 } else { 2 };
            for _ in 0..1 + rng.below(most) {
                b.splice(to..to, copy.iter().copied());
            }
        }
        3 => {
            let (at, n) = span(rng, b.len());
            b.drain(at..at + n);
        }
        4 => {
            let (at, n) = span(rng, b.len());
            let new = rng.bytes(n);
            b[at..at + n].copy_from_slice(&new);
        }
        5 => {
            let at = rng.below(b.len() + 1);
            b.splice(at..at, text_token(rng));
        }
        6 | 7 => {
            let f = fields(b);
            if f.is_empty() {
                b.extend(text_token(rng));
            } else {
                let (at, n) = *rng.pick(&f);
                b.splice(at..at + n, text_token(rng));
            }
        }
        8 | 9 => {
            let v = binary_token(rng);
            if b.len() >= v.len() {
                // fixed fields mostly start at a multiple of four
                let mut at = rng.below(b.len() - v.len() + 1);
                if rng.below(4) != 0 {
                    at &= !3;
                }
                b[at..at + v.len()].copy_from_slice(&v);
            }
        }
        10 => {
            let at = rng.below(b.len() + 1);
            b.splice(at..at, binary_token(rng));
        }
        _ => {
            let other = *rng.pick(seeds);
            b.truncate(rng.below(b.len() + 1));
            b.extend_from_slice(&other[rng.below(other.len() + 1)..]);
        }
    }
    b.truncate(MAX_LEN);
}

/// Input number `i` of a run: a valid input changed one to four times; every sixteenth is bytes of no meaning.
pub fn input(rng: &mut Rng, seeds: &[&[u8]]) -> Vec<u8> {
    if seeds.is_empty() || rng.below(16) == 0 {
        let most = *rng.pick(&[4, 64, 600]);
        let n = rng.below(most);
        return rng.bytes(n);
    }
    let mut b = rng.pick(seeds).to_vec();
    for _ in 0..1 + rng.below(4) {
        mutate_once(rng, &mut b, seeds);
    }
    b
}

/// `b` as hex and as text, for the message of a failed run.
pub fn show(b: &[u8]) -> String {
    let mut s = format!("{} bytes\n  hex:  ", b.len());
    for x in b {
        let _ = write!(s, "{:02x}", x);
    }
    let _ = write!(s, "\n  text: {:?}", String::from_utf8_lossy(b));
    s
}

/// Feeds `check` the valid inputs `seeds` as they are, the empty input, and `iters(div)` mutated ones. `check` calls
/// the parser and returns what broke its contract; a panic counts the same. The first input that fails ends the run
/// with the input in the message. With `OOS_HOSTILE_TRACE` set, every input is printed before it is tried, so the
/// input of a parser that does not return is the last one printed (`cargo test hostile -- --nocapture`).
pub fn run(name: &str, seeds: &[&[u8]], div: usize, mut check: impl FnMut(&[u8]) -> Result<(), String>) {
    let trace = std::env::var_os("OOS_HOSTILE_TRACE").is_some();
    let mut rng = Rng::named(name);
    let n = iters(div);
    let fixed = seeds.len() + 1;
    for i in 0..fixed + n {
        let (what, b) = match i {
            _ if i < seeds.len() => (format!("valid input {}", i), seeds[i].to_vec()),
            _ if i < fixed => ("the empty input".to_string(), Vec::new()),
            _ => (format!("mutated input {} of {}", i - fixed, n), input(&mut rng, seeds)),
        };
        if trace {
            eprintln!("{}: {}: {}", name, what, show(&b));
        }
        let why = match catch_unwind(AssertUnwindSafe(|| check(&b))) {
            Ok(Ok(())) => continue,
            Ok(Err(why)) => why,
            Err(_) => "panicked (the message is above)".to_string(),
        };
        panic!("{}: {}: {}\n  {}", name, what, why, show(&b));
    }
}

/// A finding that still stands: the randomized run steps over it by `name`, and `repro` is its smallest input as an
/// ignored test that fails while the bug is there.
pub struct Known {
    pub name: &'static str,
    pub what: &'static str,
    pub repro: fn(),
}

/// Whether `name` is on the list: the run of its parser steps over that case and no other.
pub fn known(list: &[Known], name: &str) -> bool {
    list.iter().any(|k| k.name == name)
}

/// Every finding of `list` still fails its test. One that passes is fixed: its line leaves the list and its test
/// loses `#[ignore]`, so the list only shrinks and the randomized run covers the case again.
pub fn still_stand(list: &[Known]) {
    for (i, k) in list.iter().enumerate() {
        assert!(list[..i].iter().all(|o| o.name != k.name), "{} is on the list twice", k.name);
        assert!(!k.what.is_empty(), "{}: say what is wrong", k.name);
        assert!(
            catch_unwind(k.repro).is_err(),
            "{} ({}) no longer fails: take it off KNOWN and remove #[ignore] from its test",
            k.name,
            k.what
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Break it catches: a generator that depends on the run, so that a failure does not come back
    #[test]
    fn the_same_inputs_every_run() {
        let seeds: [&[u8]; 2] = [b"key = 12.5\nother = on\n", b"OOSW\x01\0\0\0"];
        let take = || {
            let mut rng = Rng::named("x");
            (0..200).map(|_| input(&mut rng, &seeds)).collect::<Vec<_>>()
        };
        let a = take();
        assert_eq!(a, take());
        assert!(a.iter().any(|b| b.is_empty()) && a.iter().any(|b| b.len() > 40));
        assert!(a.iter().any(|b| std::str::from_utf8(b).is_err()));
        assert!(a.iter().any(|b| b.windows(3).any(|w| w.eq_ignore_ascii_case(b"nan") || w == b"inf")));
        let mut rng = Rng::named("x");
        let floats: Vec<f64> = (0..200).map(|_| rng.float()).collect();
        assert!(floats.iter().any(|x| x.is_nan()) && floats.iter().any(|x| x.is_infinite()));
        assert!(floats.iter().any(|x| x.is_finite() && x.abs() < 10.0));
    }

    // Break it catches: a run that swallows a broken contract or a panic, or does not name the input
    #[test]
    fn a_failure_names_its_input() {
        let seeds: [&[u8]; 1] = [b"abc"];
        let fails = |check: fn(&[u8]) -> Result<(), String>| {
            let e = catch_unwind(|| run("gen", &seeds, 100, check)).unwrap_err();
            e.downcast_ref::<String>().cloned().unwrap_or_default()
        };
        let broken = fails(|b| if b == b"abc" { Err("refused".into()) } else { Ok(()) });
        assert!(broken.contains("valid input 0: refused") && broken.contains("616263"), "{broken}");
        let panicked = fails(|b| if b.is_empty() { panic!("boom") } else { Ok(()) });
        assert!(panicked.contains("the empty input: panicked") && panicked.contains("0 bytes"), "{panicked}");
        run("gen", &seeds, 100, |_| Ok(()));
    }

    // Break it catches: a finding that was fixed and stays on the list, so its case is never tried again
    #[test]
    fn a_fixed_finding_must_leave_the_list() {
        fn fails() {
            panic!("still wrong");
        }
        fn passes() {}
        let list = [Known { name: "a", what: "wrong", repro: fails }];
        still_stand(&list);
        assert!(known(&list, "a") && !known(&list, "b"));
        let fixed = [Known { name: "a", what: "wrong", repro: passes }];
        assert!(catch_unwind(|| still_stand(&fixed)).is_err());
    }
}
