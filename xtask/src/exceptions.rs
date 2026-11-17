// Copyright (C) 2026 Jeroen
// SPDX-License-Identifier: GPL-2.0-or-later

//! The reviewed exceptions of the test-time check: one line per function and kind, each with its reason. An entry
//! that no longer matches fails the check too, so the list holds only what is still there.

use crate::test_time::Finding;

pub const FILE: &str = "xtask/test-time-exceptions.txt";

#[derive(Debug, PartialEq, Eq)]
pub struct Exception {
    pub rel: String,
    pub function: String,
    pub kind: String,
}

/// `<file> <function> <kind> <reason>`; `#` starts a comment line.
pub fn parse(text: &str) -> Result<Vec<Exception>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        match (words.next(), words.next(), words.next(), words.next()) {
            (Some(rel), Some(function), Some(kind), Some(_reason)) => {
                out.push(Exception { rel: rel.into(), function: function.into(), kind: kind.into() })
            }
            _ => return Err(format!("{FILE}:{}: want <file> <function> <kind> <reason>", n + 1)),
        }
    }
    Ok(out)
}

/// The findings no exception covers, and the exceptions no finding needs.
pub fn apply(allowed: &[Exception], found: &[Finding]) -> Vec<String> {
    let covers = |e: &Exception, f: &Finding| e.rel == f.rel && e.function == f.function && e.kind == f.kind;
    let mut out: Vec<String> = found
        .iter()
        .filter(|f| !allowed.iter().any(|e| covers(e, f)))
        .map(|f| format!("{}:{}: {} {}", f.rel, f.line, f.function, f.kind))
        .collect();
    out.extend(
        allowed
            .iter()
            .filter(|e| !found.iter().any(|f| covers(e, f)))
            .map(|e| format!("{FILE}: {} {} {} matches nothing any more", e.rel, e.function, e.kind)),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(function: &str) -> Finding {
        Finding { rel: "ui/src/a.rs".into(), line: 3, function: function.into(), kind: "sleep" }
    }

    #[test]
    fn an_exception_needs_a_reason() {
        assert!(parse("ui/src/a.rs tests::f sleep\n").is_err());
        let e = parse("# comment\n\nui/src/a.rs tests::f sleep the slow round\n").unwrap();
        assert_eq!(e, [Exception { rel: "ui/src/a.rs".into(), function: "tests::f".into(), kind: "sleep".into() }]);
    }

    #[test]
    fn findings_without_an_exception_and_exceptions_without_a_finding_fail() {
        let allowed = parse("ui/src/a.rs tests::f sleep reason\nui/src/a.rs tests::gone sleep reason\n").unwrap();
        let out = apply(&allowed, &[finding("tests::f"), finding("tests::g")]);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out[0].starts_with("ui/src/a.rs:3: tests::g sleep"));
        assert!(out[1].contains("tests::gone sleep matches nothing"));
    }
}
