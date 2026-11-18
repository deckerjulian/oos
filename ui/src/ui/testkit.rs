// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What many tests set up the same way: a demo screen, an SCPI session on it, a scratch directory.

use super::Ui;
use crate::gfx::Canvas;
use crate::scpi::Scpi;
use crate::wave::Source;
use std::path::{Path, PathBuf};

/// A screen on the demo source, as it starts.
pub fn demo_ui() -> Ui {
    Ui::new(Source::Demo { t: 0.0 })
}

/// The demo screen with the compact bottom bar (Display > Bottom bar) or the cards, laid out, after `steps` frames of
/// 50 ms.
pub fn fresh(compact: bool, steps: usize) -> Ui {
    let mut u = demo_ui();
    u.m.bar_compact = compact;
    u.relayout();
    for _ in 0..steps {
        u.step(0.05);
    }
    u
}

/// An SCPI interpreter (no port), a demo screen and a canvas of the panel.
pub fn scpi_session() -> (Scpi, Ui, Canvas) {
    (Scpi::open(0), demo_ui(), Canvas::new(crate::ui::W, crate::ui::H))
}

/// One command line through `s`, its answer as text.
pub fn q(s: &mut Scpi, ui: &mut Ui, cv: &Canvas, line: &str) -> String {
    String::from_utf8(s.exec_line(ui, cv, line)).unwrap()
}

/// A directory path of one test under the system's temporary directory, with nothing in it at the start (the
/// directory itself is not made) and removed with the guard. The name holds the process and a counter: two tests of
/// one run never share it, also with the same tag.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("oos-{}-{}-{}", tag, std::process::id(), n));
        let _ = std::fs::remove_dir_all(&p);
        Scratch(p)
    }
}

impl std::ops::Deref for Scratch {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl AsRef<Path> for Scratch {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A document of the tree by its path from the top (`wiki/Features.md`).
pub fn document(path: &str) -> String {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path);
    std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {}", path, e))
}

/// The part of `text` under the line that starts with `head`: that line and the ones after it, up to the next line
/// that starts a part (`**` or `#`).
pub fn section(text: &str, head: &str) -> String {
    let lines = text.lines().skip_while(|l| !l.starts_with(head));
    let part: Vec<&str> =
        lines.enumerate().take_while(|(i, l)| *i == 0 || !l.starts_with(['*', '#'])).map(|x| x.1).collect();
    part.join("\n")
}

/// Whether `text` names `word`, ignoring case, as words of its own: no letter or digit stands next to it. A name that
/// is a word of a longer name (`CAN` in `CAN-FD`) is found in that one too.
pub fn names(text: &str, word: &str) -> bool {
    let (text, word) = (text.to_lowercase(), word.to_lowercase());
    let alone = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
    text.match_indices(&word)
        .any(|(i, w)| alone(text[..i].chars().next_back()) && alone(text[i + w.len()..].chars().next()))
}

/// The number in front of `what` in `text` (`29 protocol decoders`), or after it in brackets (`decoders (29)`).
pub fn count_of(text: &str, what: &str) -> Option<usize> {
    let i = text.find(what)?;
    let digits = |s: &str| s.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok();
    let before = text[..i].split_whitespace().next_back().and_then(digits);
    before.or_else(|| text[i + what.len()..].strip_prefix(" (")?.split(')').next().and_then(digits))
}

/// Every key of the exemption list `list` is one of `all` and stands in it once: an entry left behind by a removed or
/// renamed item fails, so the list holds no debt that is not there (docs/adr/process/0046). `whole` is false for a
/// build that leaves items out; the keys are then only checked for doubles.
pub fn exempt_once<T: PartialEq + std::fmt::Debug>(list: &str, keys: &[T], all: &[T], whole: bool) {
    for (i, k) in keys.iter().enumerate() {
        assert!(!whole || all.contains(k), "{}: {:?} is no longer registered, take it out", list, k);
        assert!(!keys[..i].contains(k), "{}: {:?} stands twice", list, k);
    }
}

/// Checks the names a kind of extension stores against its list `ui/tests/golden/<file>`, one a line (`#` starts a
/// comment). Every name of `registered` has its line, and with `whole` (a build that has every module) every line is
/// still registered as it stands: a stored name never changes and is never given to something else
/// (docs/adr/code/0002). The list only gains lines: with `OOS_BLESS=1` the missing ones are added at its end, and
/// none is changed or removed.
pub fn stored_names(file: &str, registered: &[String], whole: bool) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("ui/tests/golden/{}: {}", file, e));
    let listed: Vec<&str> = text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    let new: Vec<&str> = registered.iter().map(String::as_str).filter(|r| !listed.contains(r)).collect();
    let gone: Vec<&str> = listed.iter().copied().filter(|l| whole && !registered.iter().any(|r| r == l)).collect();
    let bless = std::env::var("OOS_BLESS").is_ok_and(|v| v == "1");
    if bless {
        let lines: String = new.iter().map(|l| format!("{}\n", l)).collect();
        std::fs::write(&path, format!("{}{}", text, lines)).unwrap();
    }
    assert!(
        (bless || new.is_empty()) && gone.is_empty(),
        "ui/tests/golden/{}\n  registered, without a line: {:?}\n  a line, no longer registered as it stands: {:?}\n\
         A stored name never changes and is never given to something else (docs/adr/code/0002). OOS_BLESS=1 with this \
         test adds the line of what is new and changes none.",
        file,
        new,
        gone
    );
}
