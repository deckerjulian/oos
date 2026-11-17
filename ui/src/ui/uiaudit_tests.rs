// SPDX-License-Identifier: GPL-2.0-or-later
//! UI audit: repro of the Add measurements overlap and the big walk (ignored; run `uiaudit -- --ignored`).
use super::*;

fn frame(u: &mut Ui) {
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    u.step(0.05);
}

fn touch(u: &mut Ui, phase: Phase, x: i32, y: i32) {
    u.handle(Ev::Touch { id: 0, phase, x, y });
    u.step(0.01);
}

fn tap_at(u: &mut Ui, x: i32, y: i32) {
    touch(u, Phase::Down, x, y);
    touch(u, Phase::Up, x, y);
    frame(u);
}

fn shot_to(u: &mut Ui, path: &std::path::Path) {
    for _ in 0..4 { frame(u); }
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    std::fs::write(path, crate::store::png_of(&cv)).unwrap();
}

#[test]
fn repro_add_measurements() {
    use super::meascatalog::{cat as Cat, cat_from};
    let out = std::env::var("OOS_SHOTS").unwrap_or("/tmp".into());
    for (n, ts) in [TextSize::Normal, TextSize::Large, TextSize::Larger].into_iter().enumerate() {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.text_size = ts;
        for _ in 0..10 { u.step(0.05); }
        u.key(key::MEASURE);
        note(&format!("ts={n} menu={:?} count={}", u.menu, u.measurement_count()));
        shot_to(&mut u, &std::path::Path::new(&out).join(format!("audit-repro-ts{n}.png")));
        u.cat_sel = Cat::Area;
        for k in 1..8 { if let Some(c) = cat_from(k) { u.catalog_add(c); } }
        note(&format!("count={}", u.measurement_count()));
        shot_to(&mut u, &std::path::Path::new(&out).join(format!("audit-repro-ts{n}-b.png")));
    }
}

#[test]
fn repro_add_measurements_scroll() {
    let out = std::env::var("OOS_SHOTS").unwrap_or("/tmp".into());
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    for _ in 0..10 { u.step(0.05); }
    u.key(key::MEASURE);
    let up = Rect::new(965, 288, 40, 42);
    tap_at(&mut u, up.x + 5, up.y + 5);
    note(&format!("scroll after Up at top = {}", u.cat_scroll));
    shot_to(&mut u, &std::path::Path::new(&out).join("audit-repro-up.png"));
    let dn = Rect::new(965, 510, 40, 42);
    tap_at(&mut u, dn.x + 5, dn.y + 5);
    note(&format!("scroll after Down = {}", u.cat_scroll));
    shot_to(&mut u, &std::path::Path::new(&out).join("audit-repro-down.png"));
}

fn note(s: &str) {
    use std::io::Write;
    let dir = std::env::var("OOS_SHOTS").unwrap_or("/tmp".into());
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(std::path::Path::new(&dir).join("audit-notes.txt")).unwrap();
    let _ = writeln!(f, "{s}");
}

#[test]
fn repro_add_measurements_open_groups() {
    let out = std::env::var("OOS_SHOTS").unwrap_or("/tmp".into());
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    for _ in 0..10 { u.step(0.05); }
    u.key(key::MEASURE);
    u.cat_open = [u16::MAX; crate::measures::CATEGORIES.len()];
    shot_to(&mut u, &std::path::Path::new(&out).join("audit-repro-open.png"));
    for _ in 0..2 { tap_at(&mut u, 985, 530); }
    note(&format!("open groups scroll={}", u.cat_scroll));
    shot_to(&mut u, &std::path::Path::new(&out).join("audit-repro-open-scrolled.png"));
}

// ------------------------------------------------------------------------------------------------ the walk
//
// `cargo test uiaudit -- --ignored` walks the screens of every layout and text size from a set of seeds (every menu,
// the top and bottom bars, the pop-overs), taps every button it can see (two levels deep), writes one picture per
// new state and checks the drawing log of the frame (`Canvas::au`, test-only):
//   a  text wider than its button / outside its clip      b  two visible text runs on top of each other
//   c  a modal not opaque (draw it on two backgrounds)    d  modal content outside the modal (bars, screen)
//   e  a drawn button smaller than 24 px                  f  internal names, "?" or a missing glyph in text
//   p  a panic while opening or drawing a state           g  a modal that shows no text at all
use crate::gfx::Au;
use std::collections::{BTreeMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};

const AUDIT_DIR: &str = "/mnt/c/Projects/oss-plus/local/audit/walk";

#[derive(Clone, Debug)]
enum Act {
    Tap(i32, i32),
    Hold(i32, i32),
    Menu(MenuId),
    Pad(MenuId, Item),
    Zoom,
    Slider,
    CatTab(u8),
}

#[derive(Clone, Copy)]
struct Cfg {
    layout: Layout,
    ts: TextSize,
}

impl Cfg {
    fn name(self) -> String {
        let l = match self.layout { Layout::Default => "default", Layout::Studio => "studio", Layout::Minimal => "minimal" };
        match self.ts { TextSize::Normal => l.into(), TextSize::Large => format!("{l}-large"), TextSize::Larger => format!("{l}-larger") }
    }
}

fn fresh(cfg: Cfg, n: usize) -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = cfg.layout;
    u.m.text_size = cfg.ts;
    if cfg.layout == Layout::Studio {
        u.m.ch[1].on = true;
        u.studio.enter = true;
    }
    let dir = std::env::temp_dir().join(format!("oos-uiaudit-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    u.store_dir = dir;
    for _ in 0..10 { u.step(0.05); }
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    u
}

fn settle(u: &mut Ui) {
    for _ in 0..3 { frame(u); }
    u.toast = None;
}

fn apply(u: &mut Ui, a: &Act) {
    match a {
        Act::Tap(x, y) => tap_at(u, *x, *y),
        Act::Hold(x, y) => {
            touch(u, Phase::Down, *x, *y);
            u.step(LONG_PRESS + 0.1);
            touch(u, Phase::Up, *x, *y);
            frame(u);
        }
        Act::Menu(m) => { u.open_menu(*m); frame(u); }
        Act::Pad(m, it) => { u.open_menu(*m); frame(u); u.open_numpad(*it); frame(u); }
        Act::Zoom => { u.set_zoom(true); frame(u); }
        Act::Slider => { u.studio.trig_slider = true; frame(u); }
        Act::CatTab(t) => {
            u.open_menu(MenuId::MeasAdd);
            u.cat_tab = *t;
            u.cat_open[*t as usize] = u16::MAX;
            u.cat_scroll = 0;
            frame(u);
        }
    }
}

fn act_name(a: &Act) -> String {
    match a {
        Act::Tap(x, y) => format!("tap{x}x{y}"),
        Act::Hold(x, y) => format!("hold{x}x{y}"),
        Act::Menu(m) => format!("{m:?}"),
        Act::Pad(m, it) => format!("pad-{m:?}-{it:?}"),
        Act::Zoom => "zoom".into(),
        Act::Slider => "trigslider".into(),
        Act::CatTab(t) => format!("cattab{t}"),
    }
}

// ---- the frame log

struct Frame {
    cv: Canvas,
    au: Vec<Au>,
}

fn record(u: &mut Ui) -> Frame {
    u.toast = None;
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    cv.au_start();
    u.render(&mut cv);
    let au = cv.au.take().unwrap();
    Frame { cv, au }
}

type Box4 = (i32, i32, i32, i32);

fn inter(a: Box4, b: Box4) -> Option<Box4> {
    let r = (a.0.max(b.0), a.1.max(b.1), a.2.min(b.2), a.3.min(b.3));
    (r.2 > r.0 && r.3 > r.1).then_some(r)
}

fn rbox(r: Rect) -> Box4 { (r.x, r.y, r.x + r.w, r.y + r.h) }

/// Per pixel the index of the last opaque painting (fills and images) after which text is hidden.
fn cover_map(au: &[Au], w: i32, h: i32) -> Vec<u32> {
    let mut m = vec![0u32; (w * h) as usize];
    for (i, e) in au.iter().enumerate() {
        if let Au::Fill { r, opaque: true, clip, .. } = e {
            if let Some(b) = inter(inter(rbox(*r), *clip).unwrap_or((0, 0, 0, 0)), (0, 0, w, h)) {
                for y in b.1..b.3 {
                    for x in b.0..b.2 { m[(y * w + x) as usize] = i as u32 + 1; }
                }
            }
        }
    }
    m
}

fn hidden_fraction(cover: &[u32], w: i32, h: i32, ink: Box4, idx: usize) -> f32 {
    let Some(b) = inter(ink, (0, 0, w, h)) else { return 1.0 };
    let (mut n, mut hid) = (0, 0);
    let mut y = b.1;
    while y < b.3 {
        let mut x = b.0;
        while x < b.2 {
            n += 1;
            if cover[(y * w + x) as usize] > idx as u32 + 1 { hid += 1; }
            x += 2;
        }
        y += 2;
    }
    if n == 0 { 1.0 } else { hid as f32 / n as f32 }
}

#[derive(Clone, Debug)]
struct Finding {
    kind: char,
    msg: String,
    rect: String,
}

fn fnd(out: &mut Vec<Finding>, kind: char, msg: String, rect: String) {
    out.push(Finding { kind, msg, rect });
}

fn fb(b: Box4) -> String { format!("[{},{} {}x{}]", b.0, b.1, b.2 - b.0, b.3 - b.1) }
fn fr(r: Rect) -> String { format!("[{},{} {}x{}]", r.x, r.y, r.w, r.h) }

/// The internal-looking text: snake_case, paths, Debug output, CamelCase words, "?" and missing glyphs.
fn internal(s: &str) -> Option<&'static str> {
    if s.contains('?') { return Some("question mark"); }
    if s.contains('\u{fffd}') { return Some("replacement glyph"); }
    if s.contains("::") || s.contains("Some(") || s.contains("{:") { return Some("debug text"); }
    if s.contains("NaN") || s.contains("inf ") || s.trim() == "inf" || s.contains("undefined") || s.contains("TODO") { return Some("raw value"); }
    for word in s.split(|c: char| c == ' ' || c == '/' || c == ',' || c == ':') {
        if word.len() > 3 && word.contains('_') && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') { return Some("snake_case name"); }
        let humps = word.chars().skip(1).filter(|c| c.is_ascii_uppercase()).count();
        if word.len() > 8 && humps >= 2 && word.chars().all(|c| c.is_ascii_alphabetic()) && word.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && word.chars().any(|c| c.is_ascii_lowercase()) { return Some("CamelCase name"); }
    }
    None
}

fn check_frame(u: &Ui, f: &Frame, out: &mut Vec<Finding>) {
    let (w, h) = (f.cv.w, f.cv.h);
    let cover = cover_map(&f.au, w, h);
    let mut texts: Vec<(usize, Box4, &str, Option<Rect>)> = Vec::new();
    for (i, e) in f.au.iter().enumerate() {
        if let Au::Text { s, ink: Some(ink), bound, clip, missing, .. } = e {
            if s.trim().is_empty() { continue; }
            if hidden_fraction(&cover, w, h, *ink, i) > 0.5 { continue; }
            // what is drawn is the part of the ink inside the clip: a line wholly outside it (a scrolled-off row) is
            // not on the screen, and d and b look at the visible part only
            let Some(vis) = inter(*ink, *clip) else { continue };
            // a: wider than its box
            if let Some(b) = bound {
                let over = (b.x - ink.0).max(ink.2 - (b.x + b.w)).max(0);
                let overv = (b.y - ink.1).max(ink.3 - (b.y + b.h)).max(0);
                if over > 1 || overv > 3 {
                    fnd(out, 'a', format!("text '{s}' wider than its box by {over}px (v {overv}px)"), format!("ink {} box {}", fb(*ink), fr(*b)));
                }
            }
            if ink.0 < clip.0 - 1 || ink.2 > clip.2 + 1 || ink.1 < clip.1 - 2 || ink.3 > clip.3 + 2 {
                // a line cut at the top or bottom of a scroll view is the clip doing its job ('v', reported apart); text cut
                // at its left or right edge is a real truncation ('a')
                let across = ink.0 < clip.0 - 1 || ink.2 > clip.2 + 1;
                fnd(out, if across { 'a' } else { 'v' }, format!("text '{s}' cut by its clip"), format!("ink {} clip {}", fb(*ink), fb(*clip)));
            }
            // d (screen part)
            if vis.0 < 0 || vis.1 < 0 || vis.2 > w || vis.3 > h {
                fnd(out, 'd', format!("text '{s}' off the screen"), format!("ink {}", fb(*ink)));
            }
            // f
            if let Some(why) = internal(s) { fnd(out, 'f', format!("{why}: '{s}'"), fb(*ink)); }
            if !missing.is_empty() { fnd(out, 'f', format!("missing glyph {:?} in '{s}'", missing), fb(*ink)); }
            texts.push((i, vis, s.as_str(), *bound));
        }
    }
    // b: visible text runs on top of each other
    for a in 0..texts.len() {
        for b in a + 1..texts.len() {
            let (ia, ba, sa, _) = &texts[a];
            let (_ib, bb, sb, _) = &texts[b];
            if sa == sb && (ba.0 - bb.0).abs() <= 2 && (ba.1 - bb.1).abs() <= 2 { continue; }
            if let Some(x) = inter(*ba, *bb) {
                if x.2 - x.0 >= 4 && x.3 - x.1 >= 3 && hidden_fraction(&cover, w, h, x, *ia) < 0.5 {
                    fnd(out, 'b', format!("'{sa}' overlaps '{sb}'"), format!("{} / {} common {}", fb(*ba), fb(*bb), fb(x)));
                }
            }
        }
    }
    // e: drawn buttons (a rounded border plus its inset face) and standalone rounded chips
    let mut seen: HashSet<(i32, i32, i32, i32)> = HashSet::new();
    for (i, e) in f.au.iter().enumerate() {
        let Au::Round { r, rad, a: 255, .. } = e else { continue };
        let is_btn = pair_face(&f.au, i, *r);
        if !is_btn && *rad < 3 { continue; }
        if r.w >= w - 4 || r.h >= h / 2 { continue; }
        if !seen.insert((r.x, r.y, r.w, r.h)) { continue; }
        if is_btn && (r.w < 24 || r.h < 24) && !(*rad * 2 >= r.w.min(r.h) && r.w <= 20) {
            fnd(out, 'e', format!("button {}x{} px smaller than 24", r.w, r.h), fr(*r));
        }
    }
    let _ = u;
}

/// The modal: the largest opaque painting that is not the whole screen.
fn modal_rect(au: &[Au], w: i32, h: i32) -> Option<(usize, Rect)> {
    let mut best: Option<(usize, Rect, i64)> = None;
    for (i, e) in au.iter().enumerate() {
        let r = match e {
            Au::Round { r, a: 255, .. } => *r,
            Au::Fill { r, opaque: true, .. } => *r,
            _ => continue,
        };
        let area = r.w as i64 * r.h as i64;
        if area >= (w as i64 * h as i64) * 95 / 100 || area < 6000 { continue; }
        if best.as_ref().map_or(true, |b| area > b.2) { best = Some((i, r, area)); }
    }
    best.map(|b| (b.0, b.1))
}

/// Checks c, d and g for an open menu: draws it alone (twice, on two backgrounds).
fn check_modal(u: &Ui, out: &mut Vec<Finding>) {
    if !u.menu_open { return; }
    let (w, h) = (u.scr.w, u.scr.h);
    let draw = |bg: u32, rec: bool| {
        let mut cv = Canvas::new(w, h);
        cv.clear(bg);
        if rec { cv.au_start(); }
        u.draw_menu(&mut cv);
        cv
    };
    let mut a = draw(0xff00ff, true);
    let b = draw(0x00ff00, false);
    let au = a.au.take().unwrap();
    let Some((mi, m)) = modal_rect(&au, w, h) else { return };
    // c: pixels inside the modal (not at its rounded corners) that still show the background
    let inner = m.inset(10);
    let mut bad = 0;
    let mut bb: Option<Box4> = None;
    for y in inner.y..inner.y + inner.h {
        for x in inner.x..inner.x + inner.w {
            let i = (y * w + x) as usize;
            if a.px[i] != b.px[i] {
                bad += 1;
                let p = (x, y, x + 1, y + 1);
                bb = Some(match bb { None => p, Some(q) => (q.0.min(p.0), q.1.min(p.1), q.2.max(p.2), q.3.max(p.3)) });
            }
        }
    }
    if bad > 0 { fnd(out, 'c', format!("modal not opaque: {bad} px show what is underneath"), format!("modal {} bleed {}", fr(m), fb(bb.unwrap()))); }
    // d: what is drawn after the modal's ground lies outside it
    let mrect = rbox(m);
    let lim = (mrect.0 - 2, mrect.1 - 2, mrect.2 + 2, mrect.3 + 2);
    let mut texts_in = 0;
    for e in &au[mi..] {
        match e {
            Au::Text { s, ink: Some(ink), clip, .. } => {
                if s.trim().is_empty() { continue; }
                let seen = inter(*ink, *clip);
                let Some(seen) = seen else { continue };
                if inter(seen, mrect).is_some() { texts_in += 1; }
                if seen.0 < lim.0 || seen.1 < lim.1 || seen.2 > lim.2 || seen.3 > lim.3 {
                    fnd(out, 'd', format!("modal text '{s}' drawn outside the modal"), format!("ink {} modal {}", fb(seen), fr(m)));
                }
            }
            Au::Fill { r, opaque: true, clip, .. } => {
                let Some(c) = inter(rbox(*r), *clip) else { continue };
                if c.0 < lim.0 || c.1 < lim.1 || c.2 > lim.2 || c.3 > lim.3 {
                    fnd(out, 'd', "modal fill drawn outside the modal".into(), format!("fill {} modal {}", fb(c), fr(m)));
                }
            }
            _ => {}
        }
    }
    if texts_in == 0 { fnd(out, 'g', "modal shows no text".into(), fr(m)); }
    // modal over the bars
    if m.w > 200 && (m.y < TOP.h - 2 || m.y + m.h > BOTTOM.y + 2) && u.m.layout != Layout::Studio {
        fnd(out, 'd', "modal reaches into the top or bottom bar".into(), format!("modal {} bars y<{} y>{}", fr(m), TOP.h, BOTTOM.y));
    }
}

// ---- targets

struct Target {
    rect: Rect,
    label: String,
}

fn targets(f: &Frame) -> Vec<Target> {
    let (w, h) = (f.cv.w, f.cv.h);
    let cover = cover_map(&f.au, w, h);
    let mut rects: Vec<Rect> = Vec::new();
    for (i, e) in f.au.iter().enumerate() {
        let Au::Round { r, rad, a: 255, .. } = e else { continue };
        let is_btn = pair_face(&f.au, i, *r);
        if !(is_btn || *rad >= 3) || r.w < 14 || r.h < 14 || r.w >= w - 4 || r.h >= h / 2 { continue; }
        // only what is on top: its centre is not painted over later
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        if cx < 0 || cy < 0 || cx >= w || cy >= h { continue; }
        if cover[(cy * w + cx) as usize] > i as u32 + 3 { continue; }
        if !rects.contains(r) { rects.push(*r); }
    }
    let all = rects.clone();
    rects.retain(|r2| !all.iter().any(|r| *r != *r2 && (r.inset(2) == *r2 || r.inset(1) == *r2)));
    let deny = ["power", "shut", "restart", "reboot", "update", "rollback", "format", "erase", "factory", "stock", "default setup", "delete", "forget"];
    let mut out = Vec::new();
    for r in rects {
        let mut label = String::new();
        for (ei, e) in f.au.iter().enumerate() {
            if let Au::Text { s, ink: Some(ink), .. } = e {
                if inter(*ink, rbox(r)).is_some() && hidden_fraction(&cover, w, h, *ink, ei) < 0.5 && !label.contains(s.as_str()) { label.push_str(s); label.push(' '); }
            }
        }
        let l = label.trim().to_string();
        if deny.iter().any(|d| l.to_lowercase().contains(d)) { continue; }
        out.push(Target { rect: r, label: l });
    }
    out
}

fn centre(r: Rect) -> (i32, i32) { (r.x + r.w / 2, r.y + r.h / 2) }

fn tap_label(u: &mut Ui, label: &str) -> bool {
    let f = record(u);
    for t in targets(&f) {
        if t.label == label || t.label.split(" ").any(|p| p == label) {
            let (x, y) = centre(t.rect);
            tap_at(u, x, y);
            return true;
        }
    }
    false
}

/// The state key: the menu and the text on the screen, digits masked.
fn key_of(u: &Ui, f: &Frame) -> String {
    let mut v: Vec<String> = f.au.iter().filter_map(|e| match e {
        Au::Text { s, .. } if !s.trim().is_empty() => Some(s.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).collect()),
        _ => None,
    }).collect();
    v.sort();
    v.dedup();
    format!("{}|{:?}|{:?}|{}|{}|{}|{}|{}", u.menu_open, u.menu, u.pick.map(|p| p.it), u.numpad.is_some(), u.cat_tab, u.cat_scroll, u.cat_open[u.cat_tab as usize], v.join("\u{1}"))
}

fn slug(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let mut o = String::new();
    for c in t.chars() { if !(c == '-' && o.ends_with('-')) { o.push(c); } }
    o.trim_matches('-').chars().take(70).collect()
}

struct Walk {
    cfg: Cfg,
    dir: std::path::PathBuf,
    seen: HashSet<String>,
    n: usize,
    cap: usize,
    ui_n: usize,
    rows: Vec<String>,
    count: BTreeMap<(String, char), usize>,
    #[allow(dead_code)] // kept in the walk state for a debugger; the walk does not read it back
    queue: Vec<Vec<Act>>,
}

impl Walk {
    fn new(cfg: Cfg, cap: usize) -> Walk {
        let dir = std::path::Path::new(AUDIT_DIR).join(cfg.name());
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Walk { cfg, dir, seen: HashSet::new(), n: 0, cap, ui_n: 0, rows: Vec::new(), count: BTreeMap::new(), queue: Vec::new() }
    }

    fn build(&mut self, path: &[Act]) -> Ui {
        self.ui_n += 1;
        let mut u = fresh(self.cfg, self.ui_n % 1000);
        for a in path { apply(&mut u, a); }
        settle(&mut u);
        u
    }

    /// Looks at the state `path` leads to; true when it is new.
    fn visit(&mut self, path: &[Act], u: &mut Ui, label: &str) -> bool {
        let f = record(u);
        let key = key_of(u, &f);
        if !self.seen.insert(key) { return false; }
        if self.n >= self.cap { return true; }
        self.n += 1;
        let mut finds = Vec::new();
        check_frame(u, &f, &mut finds);
        check_modal(u, &mut finds);
        let name = format!("{:03}-{}", self.n, slug(&if label.is_empty() { path.iter().map(act_name).collect::<Vec<_>>().join("+") } else { label.to_string() }));
        let png = self.dir.join(format!("{name}.png"));
        std::fs::write(&png, crate::store::png_of(&f.cv)).unwrap();
        for x in finds {
            self.rows.push(format!("{}\t{}\t{}\t{}\t{}\t{}", self.cfg.name(), name, x.kind, x.msg.replace('\t', " "), x.rect, png.display()));
            *self.count.entry((self.cfg.name(), x.kind)).or_insert(0) += 1;
        }
        true
    }

    /// Builds the state `path` leads to and visits it; its buttons come back as one-tap children when it is new.
    fn explore(&mut self, path: &[Act], label: &str, want_children: bool) -> Vec<(Vec<Act>, String)> {
        let res = catch_unwind(AssertUnwindSafe(|| {
            let mut u = self.build(path);
            let new = self.visit(path, &mut u, label);
            if !(new && want_children) { return Vec::new(); }
            let f = record(&mut u);
            targets(&f).into_iter().map(|t| (t.rect, t.label)).collect::<Vec<_>>()
        }));
        match res {
            Err(_) => {
                self.rows.push(format!("{}\t{}\tp\tpanic while opening, tapping or drawing\t-\t-", self.cfg.name(), label));
                Vec::new()
            }
            Ok(ts) => ts.into_iter().map(|(r, l)| {
                let (x, y) = centre(r);
                let mut p = path.to_vec();
                p.push(Act::Tap(x, y));
                (p, format!("{label}>{}", if l.is_empty() { format!("{x}x{y}") } else { l }))
            }).collect(),
        }
    }
}

/// Takes the i-th item of each list in turn.
fn interleave(lists: Vec<Vec<(Vec<Act>, String)>>) -> Vec<(Vec<Act>, String)> {
    let max = lists.iter().map(|l| l.len()).max().unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..max {
        for l in &lists {
            if let Some(x) = l.get(i) { out.push(x.clone()); }
        }
    }
    out
}

fn seeds(cfg: Cfg) -> Vec<(Vec<Act>, String)> {
    let mut v: Vec<(Vec<Act>, String)> = vec![(vec![], "base".into())];
    let mut menus = vec![MenuId::Main, MenuId::Horizontal, MenuId::Trigger, MenuId::Acquire, MenuId::Measure, MenuId::MeasAdd, MenuId::MeasHold,
        MenuId::MeasPair, MenuId::MeasStat, MenuId::MeasLevels, MenuId::MeasFall, MenuId::Dvm, MenuId::Cursor, MenuId::Display, MenuId::Utility,
        MenuId::Math, MenuId::Ref, MenuId::Decode, MenuId::Bus, MenuId::Storage, MenuId::Mask, MenuId::Zone, MenuId::Spectrum, MenuId::Eye,
        MenuId::Power, MenuId::Magnetic, MenuId::Inverter, MenuId::Dpt, MenuId::Identify, MenuId::SelfCal, MenuId::Network, MenuId::Remote,
        MenuId::Time, MenuId::Bode, MenuId::Sim, MenuId::SimAdvanced, MenuId::Afg, MenuId::BwTest, MenuId::ViewMore, MenuId::La, MenuId::LaLines,
        MenuId::LaPat, MenuId::Windows, MenuId::AddPanel, MenuId::MeasLegacy, MenuId::Search, MenuId::Logger];
    for c in 0..NCH { menus.push(MenuId::Channel(c)); menus.push(MenuId::ChMore(c)); }
    for m in menus { v.push((vec![Act::Menu(m)], format!("menu-{m:?}"))); }
    v.push((vec![Act::Pad(MenuId::Trigger, Item::TrigLevel)], "numpad-trigger-level".into()));
    v.push((vec![Act::Pad(MenuId::Horizontal, Item::TrigLevel)], "numpad-h".into()));
    v.push((vec![Act::Zoom], "zoom-on".into()));
    for t in 0..6u8 { v.push((vec![Act::CatTab(t)], format!("catalog-tab{t}-open"))); }
    if cfg.layout == Layout::Studio { v.push((vec![Act::Slider], "trigger-slider".into())); }
    v
}

/// Seeds taken from the bars of the base screen: every top and bottom button, tapped and held.
fn bar_seeds(cfg: Cfg) -> Vec<(Vec<Act>, String)> {
    let mut u = fresh(cfg, 999);
    settle(&mut u);
    let f = record(&mut u);
    let mut v = Vec::new();
    for t in targets(&f) {
        let (x, y) = centre(t.rect);
        let in_bar = t.rect.y < TOP.h || t.rect.y >= BOTTOM.y - 4 || (cfg.layout == Layout::Studio && (t.rect.y > u.scr.h - 60));
        if !in_bar { continue; }
        let l = if t.label.is_empty() { format!("{x}x{y}") } else { t.label.clone() };
        v.push((vec![Act::Hold(x, y)], format!("hold-{l}")));
    }
    // the stock buttons by name
    for (b, label, r) in u.top_buttons() {
        let _ = b;
        let (x, y) = centre(r);
        v.push((vec![Act::Hold(x, y)], format!("hold-top-{label}")));
    }
    v
}

fn run_config(cfg: Cfg, cap: usize, summary: &mut Vec<String>) -> Walk {
    let mut w = Walk::new(cfg, cap);
    // phase A: the seeds themselves
    let mut sd = seeds(cfg);
    sd.extend(bar_seeds(cfg));
    let mut kids = Vec::new();
    for (p, l) in &sd { kids.push(w.explore(p, l, !(l.starts_with("hold") || l.starts_with("catalog")))); }
    // the catalog: every tab, every group open, scrolled page by page
    for t in 0..6u8 {
        let path = vec![Act::CatTab(t)];
        let r = catch_unwind(AssertUnwindSafe(|| {
            let mut u = w.build(&path);
            for page in 0..12 {
                if !tap_label(&mut u, "Down") { break; }
                settle(&mut u);
                let mut p2 = path.clone();
                for _ in 0..=page { p2.push(Act::Tap(985, 531)); }
                w.visit(&p2, &mut u, &format!("catalog-tab{t}-scroll{}", page + 1));
            }
            for _ in 0..12 { if !tap_label(&mut u, "Up") { break; } }
            let mut p3 = path.clone();
            p3.push(Act::Tap(985, 309));
            w.visit(&p3, &mut u, &format!("catalog-tab{t}-up"));
        }));
        if r.is_err() { w.rows.push(format!("{}\tcatalog-tab{t}\tp\tpanic in the catalog\t-\t-", cfg.name())); }
    }
    let a = w.n;
    // phase B: one tap from every seed (round robin over the seeds), until three quarters of the cap
    let mut grand = Vec::new();
    for (p, l) in interleave(kids) {
        if w.n >= cap * 3 / 4 { break; }
        grand.push(w.explore(&p, &l, true));
    }
    let b = w.n;
    // phase C: a second tap
    for (p, l) in interleave(grand) {
        if w.n >= cap { break; }
        w.explore(&p, &l, false);
    }
    summary.push(format!("{}: {} states with pictures (seeds {a}, one tap {}, two taps {}), {} visited keys, {} findings rows", cfg.name(), w.n, b - a, w.n - b, w.seen.len(), w.rows.len()));
    w
}

#[test]
#[ignore]
fn uiaudit_walk() {
    let _ = std::fs::remove_file(std::path::Path::new(AUDIT_DIR).join("findings.tsv"));
    let cfgs = [
        (Cfg { layout: Layout::Default, ts: TextSize::Normal }, 170),
        (Cfg { layout: Layout::Studio, ts: TextSize::Normal }, 190),
        (Cfg { layout: Layout::Minimal, ts: TextSize::Normal }, 150),
        (Cfg { layout: Layout::Default, ts: TextSize::Large }, 70),
        (Cfg { layout: Layout::Default, ts: TextSize::Larger }, 110),
        (Cfg { layout: Layout::Studio, ts: TextSize::Larger }, 110),
        (Cfg { layout: Layout::Minimal, ts: TextSize::Larger }, 90),
    ];
    let only = std::env::var("AUDIT_ONLY").ok();
    let mut summary = Vec::new();
    let mut all_rows = Vec::new();
    for (c, cap) in cfgs {
        if only.as_deref().is_some_and(|o| o != c.name()) { continue; }
        let w = run_config(c, cap, &mut summary);
        all_rows.extend(w.rows);
        let _ = std::fs::write(std::path::Path::new(AUDIT_DIR).join("findings.tsv"), all_rows.join("\n"));
        let _ = std::fs::write(std::path::Path::new(AUDIT_DIR).join("summary.txt"), summary.join("\n"));
    }
}

/// A button is a rounded border followed (after its strips) by the rounded face one or two pixels in.
fn pair_face(au: &[Au], i: usize, r: Rect) -> bool {
    for e in au.iter().skip(i + 1).take(8) {
        if let Au::Round { r: r2, .. } = e { return *r2 == r.inset(2) || *r2 == r.inset(1); }
    }
    false
}

#[test]
fn debug_catalog_down() {
    let mut u = fresh(Cfg { layout: Layout::Minimal, ts: TextSize::Normal }, 998);
    apply(&mut u, &Act::CatTab(0));
    settle(&mut u);
    let f = record(&mut u);
    for t in targets(&f).iter().filter(|t| t.rect.x > 950) { note(&format!("target {} {:?}", t.label, (t.rect.x, t.rect.y, t.rect.w, t.rect.h))); }
    let ok = tap_label(&mut u, "Down");
    note(&format!("down tapped={ok} scroll={}", u.cat_scroll));
}
