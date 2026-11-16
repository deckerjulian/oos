// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What many tests set up the same way: a demo screen, an SCPI session on it, a scratch directory.

use crate::gfx::Canvas;
use crate::scpi::Scpi;
use super::Ui;
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
