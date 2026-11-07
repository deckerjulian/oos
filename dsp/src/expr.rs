// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Math expressions over the channels, sample by sample: `CH1*CH2/50`, `sqrt(CH1^2+CH2^2)`, `sin(2*pi*1000*t)`.
//!
//! Names: CH1 to CH4, t (seconds, relative to the trigger), pi. Operators: + - * / ^ and unary minus. Functions:
//! sin cos tan sqrt abs ln log exp, min(a, b), max(a, b). Case does not matter. A sample of a function outside its
//! domain is NaN, which the plot leaves out.

/// Longest expression. The text comes from a keyboard or a setup file, both can be anything.
pub const EXPR_MAX: usize = 64;
const DEPTH_MAX: usize = 24;
const STACK: usize = 32;

#[derive(Clone, Copy, Debug)]
enum Op {
    Num(f64),
    Ch(usize),
    T,
    Neg,
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Min,
    Max,
    Fn(fn(f64) -> f64),
}

const FUNCS: [(&str, fn(f64) -> f64); 8] = [
    ("sin", f64::sin),
    ("cos", f64::cos),
    ("tan", f64::tan),
    ("sqrt", f64::sqrt),
    ("abs", f64::abs),
    ("ln", f64::ln),
    ("log", f64::log10),
    ("exp", f64::exp),
];

/// A parsed expression, in postfix order.
#[derive(Clone, Debug)]
pub struct Expr {
    ops: Vec<Op>,
    /// the channels it reads
    pub used: [bool; 4],
}

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
    ops: Vec<Op>,
    used: [bool; 4],
}

impl Parser<'_> {
    fn peek(&mut self) -> Option<u8> {
        while self.s.get(self.at) == Some(&b' ') {
            self.at += 1;
        }
        self.s.get(self.at).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        let hit = self.peek() == Some(c);
        if hit {
            self.at += 1;
        }
        hit
    }

    fn need(&mut self, c: u8) -> Result<(), String> {
        if self.eat(c) {
            Ok(())
        } else {
            Err(format!("Expected {}", c as char))
        }
    }

    fn sum(&mut self, depth: usize) -> Result<(), String> {
        if depth > DEPTH_MAX {
            return Err("Nested too deep".into());
        }
        self.product(depth)?;
        loop {
            if self.eat(b'+') {
                self.product(depth)?;
                self.ops.push(Op::Add);
            } else if self.eat(b'-') {
                self.product(depth)?;
                self.ops.push(Op::Sub);
            } else {
                return Ok(());
            }
        }
    }

    fn product(&mut self, depth: usize) -> Result<(), String> {
        self.unary(depth)?;
        loop {
            if self.eat(b'*') {
                self.unary(depth)?;
                self.ops.push(Op::Mul);
            } else if self.eat(b'/') {
                self.unary(depth)?;
                self.ops.push(Op::Div);
            } else {
                return Ok(());
            }
        }
    }

    fn unary(&mut self, depth: usize) -> Result<(), String> {
        if depth > DEPTH_MAX {
            return Err("Nested too deep".into());
        }
        if self.eat(b'-') {
            self.unary(depth + 1)?;
            self.ops.push(Op::Neg);
            return Ok(());
        }
        self.eat(b'+');
        self.atom(depth)?;
        // right associative, and binds tighter than a minus before it: -2^2 is -4
        if self.eat(b'^') {
            self.unary(depth + 1)?;
            self.ops.push(Op::Pow);
        }
        Ok(())
    }

    fn atom(&mut self, depth: usize) -> Result<(), String> {
        let Some(c) = self.peek() else { return Err("Unexpected end".into()) };
        if self.eat(b'(') {
            self.sum(depth + 1)?;
            return self.need(b')');
        }
        if c.is_ascii_digit() || c == b'.' {
            return self.number();
        }
        if !c.is_ascii_alphabetic() {
            return Err(format!("Unexpected {}", c as char));
        }
        let from = self.at;
        while self.s.get(self.at).is_some_and(|x| x.is_ascii_alphanumeric()) {
            self.at += 1;
        }
        let name = String::from_utf8_lossy(&self.s[from..self.at]).to_lowercase();
        if self.eat(b'(') {
            return self.call(&name, depth);
        }
        match name.as_str() {
            "t" => self.ops.push(Op::T),
            "pi" => self.ops.push(Op::Num(std::f64::consts::PI)),
            "ch1" | "ch2" | "ch3" | "ch4" => {
                let k = (name.as_bytes()[2] - b'1') as usize;
                self.used[k] = true;
                self.ops.push(Op::Ch(k));
            }
            _ => return Err(format!("Unknown name {}", name)),
        }
        Ok(())
    }

    fn call(&mut self, name: &str, depth: usize) -> Result<(), String> {
        self.sum(depth + 1)?;
        if name == "min" || name == "max" {
            self.need(b',')?;
            self.sum(depth + 1)?;
            self.ops.push(if name == "min" { Op::Min } else { Op::Max });
        } else {
            let Some(f) = FUNCS.iter().find(|f| f.0 == name) else { return Err(format!("Unknown function {}", name)) };
            self.ops.push(Op::Fn(f.1));
        }
        self.need(b')')
    }

    fn number(&mut self) -> Result<(), String> {
        let from = self.at;
        let digits = |p: &mut Self| {
            while p.s.get(p.at).is_some_and(|x| x.is_ascii_digit() || *x == b'.') {
                p.at += 1;
            }
        };
        digits(self);
        if matches!(self.s.get(self.at), Some(b'e' | b'E')) {
            let sign = matches!(self.s.get(self.at + 1), Some(b'+' | b'-')) as usize;
            if self.s.get(self.at + 1 + sign).is_some_and(|x| x.is_ascii_digit()) {
                self.at += 1 + sign;
                digits(self);
            }
        }
        let t = String::from_utf8_lossy(&self.s[from..self.at]);
        let v: f64 = t.parse().map_err(|_| format!("Bad number {}", t))?;
        self.ops.push(Op::Num(v));
        Ok(())
    }
}

impl Expr {
    pub fn parse(text: &str) -> Result<Expr, String> {
        if text.trim().is_empty() {
            return Err("Empty formula".into());
        }
        if text.len() > EXPR_MAX {
            return Err("Formula too long".into());
        }
        let mut p = Parser { s: text.as_bytes(), at: 0, ops: Vec::new(), used: [false; 4] };
        p.sum(0)?;
        if let Some(c) = p.peek() {
            return Err(format!("Unexpected {}", c as char));
        }
        // the evaluation stack has a fixed size
        let (mut d, mut most) = (0usize, 0usize);
        for op in &p.ops {
            match op {
                Op::Num(_) | Op::Ch(_) | Op::T => d += 1,
                Op::Neg | Op::Fn(_) => {}
                _ => d -= 1,
            }
            most = most.max(d);
        }
        if most > STACK {
            return Err("Formula too deep".into());
        }
        Ok(Expr { ops: p.ops, used: p.used })
    }

    /// One value for `ch` at sample `k`.
    fn at(&self, ch: &[&[f32]; 4], k: usize, t: f64) -> f64 {
        let mut st = [0f64; STACK + 1];
        let mut n = 0;
        for op in &self.ops {
            match *op {
                Op::Num(v) => {
                    st[n] = v;
                    n += 1;
                }
                Op::Ch(c) => {
                    st[n] = ch[c].get(k).map_or(f64::NAN, |&v| v as f64);
                    n += 1;
                }
                Op::T => {
                    st[n] = t;
                    n += 1;
                }
                Op::Neg => st[n - 1] = -st[n - 1],
                Op::Fn(f) => st[n - 1] = f(st[n - 1]),
                _ => {
                    n -= 1;
                    let (a, b) = (st[n - 1], st[n]);
                    st[n - 1] = match *op {
                        Op::Add => a + b,
                        Op::Sub => a - b,
                        Op::Mul => a * b,
                        Op::Div => a / b,
                        Op::Pow => a.powf(b),
                        Op::Min => a.min(b),
                        _ => a.max(b),
                    };
                }
            }
        }
        st[0]
    }

    /// The waveform: as long as the shortest channel it reads, `fallback` samples when it reads none. Empty when a
    /// channel it reads has no data.
    pub fn eval(&self, ch: [&[f32]; 4], t0: f64, dt: f64, fallback: usize) -> Vec<f32> {
        let n = (0..4).filter(|&c| self.used[c]).map(|c| ch[c].len()).min().unwrap_or(fallback);
        (0..n).map(|k| self.at(&ch, k, t0 + k as f64 * dt) as f32).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(s: &str) -> f64 {
        Expr::parse(s).unwrap().eval([&[], &[], &[], &[]], 0.0, 1.0, 1)[0] as f64
    }

    #[test]
    fn the_rules_of_arithmetic() {
        for (s, v) in [("1+2*3", 7.0), ("(1+2)*3", 9.0), ("2^3^2", 512.0), ("-2^2", -4.0), ("2^-1", 0.5), ("10/4/5", 0.5), ("1-2-3", -4.0), ("1.5e3", 1500.0), ("2e-3*1000", 2.0), ("--3", 3.0), ("+4", 4.0), ("max(1,min(5,3))", 3.0), ("abs(-2)", 2.0), ("log(1000)", 3.0), ("ln(exp(2))", 2.0), (" sqrt( 16 ) ", 4.0)] {
            assert!((one(s) - v).abs() < 1e-3, "{} = {}", s, one(s));
        }
        assert!((one("SIN(PI/2)") - 1.0).abs() < 1e-6);
    }

    #[test]
    fn channels_and_time() {
        let (a, b) = ([1.0f32, 2.0, 3.0], [4.0f32, 5.0]);
        let e = Expr::parse("ch1*CH2+t").unwrap();
        assert_eq!(e.used, [true, true, false, false]);
        assert_eq!(e.eval([&a, &b, &[], &[]], 10.0, 1.0, 99), vec![14.0, 21.0]);
        assert!(e.eval([&a, &[], &[], &[]], 0.0, 1.0, 99).is_empty());
        assert_eq!(Expr::parse("t").unwrap().eval([&[], &[], &[], &[]], 1.0, 0.5, 3), vec![1.0, 1.5, 2.0]);
        assert!(one("sqrt(-1)").is_nan());
        assert!(one("1/0").is_infinite());
    }

    #[test]
    fn what_is_not_a_formula() {
        for s in ["", "   ", "1+", "(1", "1)", "foo", "foo(1)", "ch5", "ch1 ch2", "1..2", "min(1)", "sin(1,2)", "$", "\u{e4}", "1 2", "*3", "t(1)"] {
            assert!(Expr::parse(s).is_err(), "{:?}", s);
        }
        assert!(Expr::parse(&"1+".repeat(40)).is_err());
    }

    #[test]
    fn nothing_breaks_it() {
        // right-leaning and deep ones, within the length limit: an error or a value, never a panic
        for s in ["(".repeat(64), "-".repeat(64), "2^".repeat(31) + "2", "1+(".repeat(16) + "1", "max(1,".repeat(10) + "1", "1+(1+(1+(1+(1+(1+(1+(1+(1+(1+(1+(1+(1+(1+(1+(1)))))))))))))))".to_string()] {
            if let Ok(e) = Expr::parse(&s) {
                e.eval([&[], &[], &[], &[]], 0.0, 1.0, 2);
            }
        }
    }
}
