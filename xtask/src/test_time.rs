// Copyright (C) 2026 Jeroen
// SPDX-License-Identifier: GPL-2.0-or-later

//! The test-time check (docs/adr/process/0020). In test code a limit on the real clock is only a hang guard of at
//! least a minute: a deadline, a comparison with an elapsed time, the time-out of a wait. A sleep only paces a loop
//! that polls for an event; a sleep outside a loop stands for an event the test does not wait for. An assertion on
//! the clock that is no such limit fails as well. A test waits for the event it needs (a channel, a counter a worker
//! updates, a reply) and supplies the time it wants to see.

use crate::sources::{cfg_test, Source};
use std::collections::{BTreeMap, BTreeSet};
use syn::parse::Parser;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

/// The shortest time-out that counts as a hang guard, in milliseconds.
const HANG_GUARD_MS: u64 = 60_000;

/// One place in test code that depends on the speed of the computer.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub rel: String,
    pub line: usize,
    /// the enclosing functions and modules, `tests::a_test`
    pub function: String,
    pub kind: &'static str,
}

type Args = syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>;

fn is_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("test")) || cfg_test(attrs)
}

fn ordering(op: &syn::BinOp) -> bool {
    matches!(op, syn::BinOp::Lt(_) | syn::BinOp::Le(_) | syn::BinOp::Gt(_) | syn::BinOp::Ge(_))
}

fn equality(op: &syn::BinOp) -> bool {
    matches!(op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_))
}

/// `+=`, `-=` and the other assignments that change a value in place
fn compound_assignment(op: &syn::BinOp) -> bool {
    matches!(
        op,
        syn::BinOp::AddAssign(_)
            | syn::BinOp::SubAssign(_)
            | syn::BinOp::MulAssign(_)
            | syn::BinOp::DivAssign(_)
            | syn::BinOp::RemAssign(_)
            | syn::BinOp::BitXorAssign(_)
            | syn::BinOp::BitAndAssign(_)
            | syn::BinOp::BitOrAssign(_)
            | syn::BinOp::ShlAssign(_)
            | syn::BinOp::ShrAssign(_)
    )
}

/// The local a place expression names: `wait` in `wait = ..`, not `self.wait` or `waits[0]`.
fn local(e: &syn::Expr) -> Option<String> {
    match e {
        syn::Expr::Path(p) => p.path.get_ident().map(|i| i.to_string()),
        syn::Expr::Paren(p) => local(&p.expr),
        _ => None,
    }
}

/// Whether an expression reads the real clock: `.elapsed()`, `Instant::now()` (with `now`), or a binding that
/// holds one. A value passed into a function is not followed: what the function returns is not a clock.
struct ClockUse<'n> {
    names: &'n BTreeSet<String>,
    now: bool,
    found: bool,
}

impl<'a> Visit<'a> for ClockUse<'_> {
    fn visit_expr_call(&mut self, e: &'a syn::ExprCall) {
        if let syn::Expr::Path(p) = &*e.func {
            self.found |= self.now && p.path.segments.last().is_some_and(|s| s.ident == "now");
        }
    }
    fn visit_expr_method_call(&mut self, e: &'a syn::ExprMethodCall) {
        self.found |= e.method == "elapsed";
        if e.method != "now" {
            self.visit_expr(&e.receiver);
        }
    }
    fn visit_expr_path(&mut self, e: &'a syn::ExprPath) {
        self.found |= e.path.get_ident().is_some_and(|i| self.names.contains(&i.to_string()));
    }
    // The value of a loop, match, block or closure is not followed: a clock read inside one only steers it.
    fn visit_expr_block(&mut self, _: &'a syn::ExprBlock) {}
    fn visit_expr_closure(&mut self, _: &'a syn::ExprClosure) {}
    fn visit_expr_for_loop(&mut self, _: &'a syn::ExprForLoop) {}
    fn visit_expr_if(&mut self, _: &'a syn::ExprIf) {}
    fn visit_expr_loop(&mut self, _: &'a syn::ExprLoop) {}
    fn visit_expr_match(&mut self, _: &'a syn::ExprMatch) {}
    fn visit_expr_while(&mut self, _: &'a syn::ExprWhile) {}
}

fn reads_clock(e: &syn::Expr, names: &BTreeSet<String>, now: bool) -> bool {
    let mut c = ClockUse { names, now, found: false };
    c.visit_expr(e);
    c.found
}

/// The length of a `Duration` written in the source, in milliseconds: `Duration::from_secs(5)`, `Some(..)` of
/// one, or a name bound to one.
fn duration_ms(e: &syn::Expr, known: &BTreeMap<String, u64>) -> Option<u64> {
    match e {
        syn::Expr::Path(p) => known.get(&p.path.segments.last()?.ident.to_string()).copied(),
        syn::Expr::Call(c) => {
            let syn::Expr::Path(p) = &*c.func else { return None };
            let name = p.path.segments.last()?.ident.to_string();
            let arg = c.args.first()?;
            if name == "Some" {
                return duration_ms(arg, known);
            }
            let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(n), .. }) = arg else { return None };
            let n = n.base10_parse::<u64>().ok()?;
            match name.as_str() {
                "from_secs" => n.checked_mul(1000),
                "from_millis" => Some(n),
                "from_micros" => Some(n / 1000),
                "from_nanos" => Some(n / 1_000_000),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The `const` durations of a file, wherever they stand in it.
#[derive(Default)]
struct Consts(BTreeMap<String, u64>);

impl<'a> Visit<'a> for Consts {
    fn visit_item_const(&mut self, c: &'a syn::ItemConst) {
        if let Some(ms) = duration_ms(&c.expr, &self.0) {
            self.0.insert(c.ident.to_string(), ms);
        }
    }
}

#[derive(Default)]
struct Walk {
    rel: String,
    test: bool,
    /// inside an `#[ignore]` test: a measurement or a manual tool, run by name only
    ignored: bool,
    /// inside the body of a `loop`, `while` or `for` of the current function or closure
    in_loop: bool,
    names: Vec<String>,
    /// names `std::thread::sleep` is imported as
    sleeps: BTreeSet<String>,
    /// bindings that hold a reading of the clock (`Instant::now()`) or an elapsed time
    clocks: BTreeSet<String>,
    elapsed: BTreeSet<String>,
    /// bindings that hold a duration, or a deadline `Instant::now() + duration`: the duration, in milliseconds
    durations: BTreeMap<String, u64>,
    /// locals assigned to (`=`, `+=`, ..) in the current scope: their facts reach the scopes around it
    assigned: BTreeSet<String>,
    found: Vec<Finding>,
}

fn sleep_aliases(tree: &syn::UseTree, out: &mut BTreeSet<String>) {
    match tree {
        syn::UseTree::Rename(r) if r.ident == "sleep" => {
            out.insert(r.rename.to_string());
        }
        syn::UseTree::Path(p) => sleep_aliases(&p.tree, out),
        syn::UseTree::Group(g) => g.items.iter().for_each(|t| sleep_aliases(t, out)),
        _ => {}
    }
}

impl Walk {
    fn record(&mut self, span: proc_macro2::Span, kind: &'static str) {
        if self.test && !self.ignored {
            let function = self.names.join("::");
            self.found.push(Finding { rel: self.rel.clone(), line: span.start().line, function, kind });
        }
    }

    fn timeout<'a>(
        &mut self,
        span: proc_macro2::Span,
        name: &str,
        mut args: impl DoubleEndedIterator<Item = &'a syn::Expr>,
    ) {
        if name.ends_with("_timeout")
            && args.next_back().and_then(|e| duration_ms(e, &self.durations)).is_none_or(|ms| ms < HANG_GUARD_MS)
        {
            self.record(span, "short-timeout");
        }
    }

    /// The limit a clock comparison sets, in milliseconds, when the source states it: a duration, a deadline, or a
    /// number compared with `as_secs()` or `as_millis()` of an elapsed time.
    fn limit(&self, e: &syn::Expr, other: &syn::Expr) -> Option<u64> {
        if let Some(ms) = duration_ms(e, &self.durations) {
            return Some(ms);
        }
        if let syn::Expr::Binary(b) = e {
            if matches!(b.op, syn::BinOp::Add(_)) {
                return self.limit(&b.right, other);
            }
        }
        let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(n), .. }) = e else { return None };
        let syn::Expr::MethodCall(m) = other else { return None };
        let n = n.base10_parse::<u64>().ok()?;
        match m.method.to_string().as_str() {
            "as_secs" => n.checked_mul(1000),
            "as_millis" => Some(n),
            _ => None,
        }
    }

    /// Visits `f` as a nested scope: test context and bindings it adds end with it. An item (`fn`, `mod`, `impl`)
    /// cannot assign to the locals around it; a block can, and may run or not, so what it assigns is merged into the
    /// facts after it: a clock or elapsed time if either side holds one, the shorter duration, none if one is unknown.
    fn scope(&mut self, attrs: &[syn::Attribute], name: Option<String>, item: bool, f: impl FnOnce(&mut Self)) {
        let saved = (self.test, self.ignored, self.clocks.clone(), self.elapsed.clone(), self.durations.clone());
        let outer_assigned = std::mem::take(&mut self.assigned);
        self.test |= is_test(attrs);
        self.ignored |= attrs.iter().any(|a| a.path().is_ident("ignore"));
        let named = name.is_some();
        self.names.extend(name);
        f(self);
        if named {
            self.names.pop();
        }
        let assigned = std::mem::replace(&mut self.assigned, outer_assigned);
        let inner = (std::mem::take(&mut self.clocks), std::mem::take(&mut self.elapsed));
        let inner_durations = std::mem::take(&mut self.durations);
        (self.test, self.ignored, self.clocks, self.elapsed, self.durations) = saved;
        if item {
            return;
        }
        for name in assigned {
            if inner.0.contains(&name) {
                self.clocks.insert(name.clone());
            }
            if inner.1.contains(&name) {
                self.elapsed.insert(name.clone());
            }
            match (self.durations.get(&name), inner_durations.get(&name)) {
                (Some(&a), Some(&b)) => {
                    self.durations.insert(name.clone(), a.min(b));
                }
                _ => {
                    self.durations.remove(&name);
                }
            }
            self.assigned.insert(name);
        }
    }

    /// `elapsed >= limit` or `limit <= elapsed`
    fn lower_bound(&self, b: &syn::ExprBinary) -> bool {
        let elapsed = |e: &syn::Expr| reads_clock(e, &self.elapsed, false);
        match b.op {
            syn::BinOp::Ge(_) | syn::BinOp::Gt(_) => elapsed(&b.left) && !elapsed(&b.right),
            syn::BinOp::Le(_) | syn::BinOp::Lt(_) => elapsed(&b.right) && !elapsed(&b.left),
            _ => false,
        }
    }

    /// Notes what `pat = e` binds: a clock reading, an elapsed time, a duration or a deadline. Tuples pair up, a
    /// type annotation (`guard: Duration`) is looked through. A name bound to anything else loses what it held.
    fn bind(&mut self, pat: &syn::Pat, e: &syn::Expr) {
        match (pat, e) {
            (syn::Pat::Tuple(p), syn::Expr::Tuple(t)) => {
                for (p, e) in p.elems.iter().zip(&t.elems) {
                    self.bind(p, e);
                }
            }
            (syn::Pat::Type(t), e) => self.bind(&t.pat, e),
            (syn::Pat::Ident(p), e) => self.note(p.ident.to_string(), Some(e)),
            _ => {}
        }
    }

    /// Replaces what `name` holds by what `e` is; with no `e`, by nothing known.
    fn note(&mut self, name: String, e: Option<&syn::Expr>) {
        let elapsed = e.is_some_and(|e| reads_clock(e, &self.elapsed, false));
        let clock = e.is_some_and(|e| reads_clock(e, &self.clocks, true));
        let deadline = match e {
            Some(syn::Expr::Binary(b)) if matches!(b.op, syn::BinOp::Add(_)) => duration_ms(&b.right, &self.durations),
            _ => None,
        };
        let ms = e.and_then(|e| duration_ms(e, &self.durations)).or(deadline);
        self.elapsed.remove(&name);
        self.clocks.remove(&name);
        self.durations.remove(&name);
        if elapsed {
            self.elapsed.insert(name.clone());
        }
        if clock {
            self.clocks.insert(name.clone());
        }
        if let Some(ms) = ms {
            self.durations.insert(name, ms);
        }
    }

    /// `place = e`: a local takes what `e` is, tuples pair up.
    fn assign(&mut self, place: &syn::Expr, e: &syn::Expr) {
        if let (syn::Expr::Tuple(p), syn::Expr::Tuple(t)) = (place, e) {
            for (p, e) in p.elems.iter().zip(&t.elems) {
                self.assign(p, e);
            }
        } else if let syn::Expr::Tuple(p) = place {
            for p in &p.elems {
                self.forget(p);
            }
        } else if let Some(name) = local(place) {
            self.note(name.clone(), Some(e));
            self.assigned.insert(name);
        }
    }

    /// `place += e` and the like: a local keeps being a clock or an elapsed time, its duration is no longer known.
    fn forget(&mut self, place: &syn::Expr) {
        if let Some(name) = local(place) {
            self.durations.remove(&name);
            self.assigned.insert(name);
        }
    }

    fn looping(&mut self, in_loop: bool, f: impl FnOnce(&mut Self)) {
        let saved = self.in_loop;
        self.in_loop = in_loop;
        f(self);
        self.in_loop = saved;
    }

    fn walk_macro(&mut self, m: &syn::Macro) {
        if let Ok(args) = Args::parse_terminated.parse2(m.tokens.clone()) {
            let mut args: Vec<&syn::Expr> = args.iter().collect();
            if m.path.is_ident("assert") {
                if let Some(&e) = args.first() {
                    let compares = matches!(e, syn::Expr::Binary(b) if ordering(&b.op) || equality(&b.op));
                    if !compares && reads_clock(e, &self.elapsed, false) {
                        self.record(m.span(), "clock-assertion");
                    }
                    // that a wait took at least its time holds however slow the computer is
                    if let syn::Expr::Binary(b) = e {
                        if self.lower_bound(b) {
                            args.splice(0..1, [&*b.left, &*b.right]);
                        }
                    }
                }
            }
            if (m.path.is_ident("assert_eq") || m.path.is_ident("assert_ne"))
                && args
                    .iter()
                    .take(2)
                    .any(|e| reads_clock(e, &self.elapsed, false) || reads_clock(e, &self.clocks, true))
            {
                self.record(m.span(), "clock-comparison");
            }
            for e in args {
                self.visit_expr(e);
            }
        } else if let Ok(block) = syn::parse2::<syn::Block>(m.tokens.clone()) {
            self.visit_block(&block);
        }
    }
}

impl<'a> Visit<'a> for Walk {
    fn visit_item_use(&mut self, u: &'a syn::ItemUse) {
        sleep_aliases(&u.tree, &mut self.sleeps);
    }
    fn visit_item_mod(&mut self, m: &'a syn::ItemMod) {
        self.scope(&m.attrs, Some(m.ident.to_string()), true, |w| visit::visit_item_mod(w, m));
    }
    fn visit_item_impl(&mut self, i: &'a syn::ItemImpl) {
        self.scope(&i.attrs, None, true, |w| visit::visit_item_impl(w, i));
    }
    fn visit_item_fn(&mut self, f: &'a syn::ItemFn) {
        let name = Some(f.sig.ident.to_string());
        self.scope(&f.attrs, name, true, |w| w.looping(false, |w| w.visit_block(&f.block)));
    }
    fn visit_impl_item_fn(&mut self, f: &'a syn::ImplItemFn) {
        let name = Some(f.sig.ident.to_string());
        self.scope(&f.attrs, name, true, |w| w.looping(false, |w| w.visit_block(&f.block)));
    }
    fn visit_block(&mut self, b: &'a syn::Block) {
        self.scope(&[], None, false, |w| visit::visit_block(w, b));
    }
    fn visit_expr_assign(&mut self, e: &'a syn::ExprAssign) {
        // the value is read before the place takes it
        self.visit_expr(&e.right);
        self.visit_expr(&e.left);
        self.assign(&e.left, &e.right);
    }
    fn visit_local(&mut self, l: &'a syn::Local) {
        let test = self.test;
        self.test |= is_test(&l.attrs);
        if let Some(init) = &l.init {
            self.bind(&l.pat, &init.expr);
        }
        visit::visit_local(self, l);
        self.test = test;
    }
    fn visit_expr_binary(&mut self, e: &'a syn::ExprBinary) {
        let clock = reads_clock(&e.left, &self.clocks, true) || reads_clock(&e.right, &self.clocks, true);
        if ordering(&e.op) && clock {
            let limit = self.limit(&e.left, &e.right).max(self.limit(&e.right, &e.left));
            if limit.is_none_or(|ms| ms < HANG_GUARD_MS) {
                self.record(e.span(), "short-limit");
            }
        }
        // no hang guard: an exact time on the real clock is missed by a computer that is late
        if equality(&e.op) && clock {
            self.record(e.span(), "clock-comparison");
        }
        visit::visit_expr_binary(self, e);
        if compound_assignment(&e.op) {
            self.forget(&e.left);
        }
    }
    fn visit_expr_call(&mut self, e: &'a syn::ExprCall) {
        if let syn::Expr::Path(p) = &*e.func {
            if let Some(s) = p.path.segments.last() {
                let name = s.ident.to_string();
                if !self.in_loop && (name == "sleep" || self.sleeps.contains(&name)) {
                    self.record(e.span(), "sleep");
                }
                self.timeout(e.span(), &name, e.args.iter());
            }
        }
        visit::visit_expr_call(self, e);
    }
    fn visit_expr_method_call(&mut self, e: &'a syn::ExprMethodCall) {
        self.timeout(e.span(), &e.method.to_string(), e.args.iter());
        visit::visit_expr_method_call(self, e);
    }
    fn visit_macro(&mut self, m: &'a syn::Macro) {
        self.walk_macro(m);
    }
    fn visit_expr(&mut self, e: &'a syn::Expr) {
        let attrs: &[syn::Attribute] = match e {
            syn::Expr::Async(e) => &e.attrs,
            syn::Expr::Block(e) => &e.attrs,
            syn::Expr::Call(e) => &e.attrs,
            syn::Expr::Closure(e) => &e.attrs,
            syn::Expr::Macro(e) => &e.attrs,
            syn::Expr::MethodCall(e) => &e.attrs,
            syn::Expr::Unsafe(e) => &e.attrs,
            _ => &[],
        };
        let test = self.test;
        self.test |= is_test(attrs);
        visit::visit_expr(self, e);
        self.test = test;
    }
    fn visit_expr_loop(&mut self, e: &'a syn::ExprLoop) {
        self.looping(true, |w| visit::visit_expr_loop(w, e));
    }
    fn visit_expr_while(&mut self, e: &'a syn::ExprWhile) {
        self.looping(true, |w| visit::visit_expr_while(w, e));
    }
    fn visit_expr_for_loop(&mut self, e: &'a syn::ExprForLoop) {
        self.looping(true, |w| visit::visit_expr_for_loop(w, e));
    }
    // a closure may not run, and only one arm of a match runs: what they assign is merged like a block's
    fn visit_expr_closure(&mut self, e: &'a syn::ExprClosure) {
        self.scope(&[], None, false, |w| w.looping(false, |w| visit::visit_expr_closure(w, e)));
    }
    fn visit_arm(&mut self, a: &'a syn::Arm) {
        self.scope(&a.attrs, None, false, |w| visit::visit_arm(w, a));
    }
    fn visit_stmt_macro(&mut self, m: &'a syn::StmtMacro) {
        self.scope(&m.attrs, None, false, |w| w.walk_macro(&m.mac));
    }
}

/// The places in `source` where a test depends on the speed of the computer.
pub fn findings(source: &Source) -> Vec<Finding> {
    let mut consts = Consts::default();
    consts.visit_file(&source.file);
    let mut walk = Walk { rel: source.rel.clone(), test: source.test, durations: consts.0, ..Walk::default() };
    walk.visit_file(&source.file);
    walk.found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str, test: bool) -> Vec<&'static str> {
        let file = syn::parse_file(text).unwrap();
        findings(&Source { rel: "p/src/a.rs".into(), test, file }).into_iter().map(|f| f.kind).collect()
    }

    fn in_test(body: &str) -> Vec<&'static str> {
        kinds(&format!("#[test] fn f() {{ {body} }}"), false)
    }

    #[test]
    fn production_code_is_not_checked() {
        assert!(kinds("fn f() { thread::sleep(gap); assert!(start.elapsed() < limit); }", false).is_empty());
        assert_eq!(kinds("fn f() { thread::sleep(gap); }", true), ["sleep"]);
        assert_eq!(kinds("#[cfg(all(test, unix))] mod t { fn f() { thread::sleep(gap); } }", false), ["sleep"]);
        assert_eq!(kinds("#[cfg(test)] impl S { fn f() { sleep(gap); } }", false), ["sleep"]);
        assert_eq!(kinds("fn f() { #[cfg(test)] thread::sleep(gap); }", false), ["sleep"]);
    }

    #[test]
    fn a_sleep_outside_a_loop_stands_for_an_event() {
        assert_eq!(in_test("send(); thread::sleep(gap); check();"), ["sleep"]);
        assert_eq!(kinds("use std::thread::sleep as pause; #[test] fn f() { pause(gap); }", false), ["sleep"]);
        assert_eq!(in_test("loop { thread::spawn(|| thread::sleep(gap)); }"), ["sleep"]);
        assert!(in_test("while !done() { thread::sleep(gap); }").is_empty());
    }

    #[test]
    fn a_limit_on_the_clock_is_a_hang_guard_of_a_minute() {
        for body in [
            "let end = Instant::now() + Duration::from_secs(10); while Instant::now() < end {}",
            "let (got, end) = (0, Instant::now() + Duration::from_secs(5)); assert!(Instant::now() < end);",
            "while start.elapsed().as_secs() < 20 {}",
            "assert!(start.elapsed() < Duration::from_millis(250));",
            "let took = start.elapsed(); assert!(took < limit);",
            "if Instant::now() >= deadline { panic!() }",
        ] {
            assert_eq!(in_test(body), ["short-limit"], "{body}");
        }
        for body in [
            "let end = Instant::now() + Duration::from_secs(60); while Instant::now() < end {}",
            "const GUARD: Duration = Duration::from_secs(60); let end = Instant::now() + GUARD; assert!(Instant::now() < end);",
            "while start.elapsed().as_secs() < 60 {}",
            "assert!(start.elapsed() < Duration::from_secs(120));",
        ] {
            assert!(in_test(body).is_empty(), "{body}");
        }
    }

    #[test]
    fn a_wait_that_ran_its_time_holds_however_slow_the_computer_is() {
        assert!(in_test("assert!(t.elapsed() >= Duration::from_millis(2000));").is_empty());
        assert!(in_test("assert!(Duration::from_millis(25) <= t.elapsed());").is_empty());
    }

    #[test]
    fn an_assertion_on_the_clock_is_found() {
        assert_eq!(in_test("assert!(start.elapsed().is_zero());"), ["clock-assertion"]);
        assert_eq!(in_test("assert_eq!(start.elapsed(), Duration::ZERO);"), ["clock-comparison"]);
        assert_eq!(in_test("assert_ne!(Instant::now(), deadline);"), ["clock-comparison"]);
    }

    #[test]
    fn a_time_out_is_a_hang_guard_of_a_minute() {
        for body in [
            "rx.recv_timeout(Duration::from_secs(1));",
            "socket.set_read_timeout(Some(Duration::from_secs(5)));",
            "let wait = Duration::from_millis(500); cv.wait_timeout(guard, wait);",
            "rx.recv_timeout(limit);",
        ] {
            assert_eq!(in_test(body), ["short-timeout"], "{body}");
        }
        assert!(in_test("rx.recv_timeout(Duration::from_secs(60));").is_empty());
        assert!(in_test("socket.set_read_timeout(Some(Duration::from_secs(120)));").is_empty());
    }

    #[test]
    fn a_typed_binding_holds_its_duration() {
        assert!(in_test("let guard: Duration = Duration::from_secs(60); rx.recv_timeout(guard);").is_empty());
        assert!(in_test("let (a, b): (u8, Duration) = (0, Duration::from_secs(60)); rx.recv_timeout(b);").is_empty());
        let body = "let guard: Duration = Duration::from_millis(5); rx.recv_timeout(guard);";
        assert_eq!(in_test(body), ["short-timeout"]);
    }

    #[test]
    fn an_assignment_replaces_what_a_local_held() {
        for body in [
            "let mut wait = Duration::from_secs(60); wait = Duration::from_millis(1); rx.recv_timeout(wait);",
            "let mut wait = Duration::from_secs(60); if fast { wait = Duration::from_millis(1); } rx.recv_timeout(wait);",
            "let mut wait = Duration::from_secs(60); if fast { wait = other(); } rx.recv_timeout(wait);",
            "let mut wait = Duration::from_secs(60); wait -= Duration::from_secs(59); rx.recv_timeout(wait);",
            "let mut wait = Duration::from_secs(60); { wait /= 1000; } rx.recv_timeout(wait);",
            "let wait = Duration::from_secs(60); let wait = other(); rx.recv_timeout(wait);",
            "let mut wait = Duration::from_millis(1); let f = || wait = Duration::from_secs(60); rx.recv_timeout(wait);",
            "let mut wait = Duration::from_millis(1); match x { A => wait = Duration::from_secs(60), _ => {} } \
             rx.recv_timeout(wait);",
        ] {
            assert_eq!(in_test(body), ["short-timeout"], "{body}");
        }
        let body = "let mut end = Instant::now() + Duration::from_secs(60); end = Instant::now(); while Instant::now() < end {}";
        assert_eq!(in_test(body), ["short-limit"]);
        for body in [
            "let mut wait = Duration::from_millis(1); wait = Duration::from_secs(60); rx.recv_timeout(wait);",
            "let mut wait = Duration::from_secs(60); if slow { wait = Duration::from_secs(120); } rx.recv_timeout(wait);",
            "let mut wait = Duration::from_secs(60); let f = || { let wait = 1; }; rx.recv_timeout(wait);",
            "let mut wait = Duration::from_secs(60); match x { A => wait = Duration::from_secs(90), _ => {} } \
             rx.recv_timeout(wait);",
        ] {
            assert!(in_test(body).is_empty(), "{body}");
        }
    }

    #[test]
    fn an_exact_time_on_the_clock_is_no_hang_guard() {
        for body in [
            "assert!(start.elapsed().as_secs() == 60);",
            "assert!(start.elapsed() != Duration::from_secs(120));",
            "if Instant::now() == deadline { panic!() }",
        ] {
            assert_eq!(in_test(body), ["clock-comparison"], "{body}");
        }
        assert!(in_test("assert!(count == 60);").is_empty());
    }

    #[test]
    fn a_clock_that_only_steers_a_loop_does_not_make_its_value_a_clock() {
        let body = "let n = loop { match read() { Ok(n) => break n, Err(_) if t.elapsed().as_secs() < 60 => continue, \
                    Err(e) => panic!() } }; assert_eq!(n, 6);";
        assert!(in_test(body).is_empty());
    }

    #[test]
    fn ignored_tests_comments_and_strings_are_left_alone() {
        assert!(kinds("#[test] #[ignore] fn f() { thread::sleep(gap); }", false).is_empty());
        assert!(in_test(r#"let s = "thread::sleep(d)"; /* thread::sleep(d); */"#).is_empty());
    }

    #[test]
    fn a_finding_names_its_function_and_line() {
        let file =
            syn::parse_file("mod tests {\n    #[test]\n    fn f() {\n        thread::sleep(gap);\n    }\n}\n").unwrap();
        let found = findings(&Source { rel: "p/src/a.rs".into(), test: false, file });
        let want = Finding { rel: "p/src/a.rs".into(), line: 4, function: "tests::f".into(), kind: "sleep" };
        assert_eq!(found, [want]);
    }
}
