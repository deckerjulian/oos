#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""code-metrics.py: long functions, `.unwrap()`, modules without a header, the menu pages not yet in `PAGES` and the
loose fields of the screen's state, in the four crates (issue 120, ADR 0060, ADR 0062).

    tools/code-metrics.py [ROOT]            one line: the five counts
    tools/code-metrics.py --list [ROOT]     each count with what is behind it
    tools/code-metrics.py --enforce [ROOT]  the counts against tools/code-metrics.txt; exit 1 when one differs
    tools/code-metrics.py --lower [ROOT]    write the counts that went down into tools/code-metrics.txt

ROOT is the top of a tree (default: this tree). Read from it as it is: the Rust files below hal/src, dsp/src, ui/src,
ui/system/src, ui/remote/src, ui/model/src, ui/store/src and fpgad/src, without tests, as tools/duplicates.py reads
them (its functions are used here): not a test module inside a file, not a file or folder that is only built for tests,
not a file `tests.rs` or `*_tests.rs`, not a folder `tests/`.
  long-fns  functions of more than 150 lines (goal S2): an `fn` item with a body, from the line of `fn` to the line
            of its closing brace. Braces in strings, characters, raw strings and comments do not count. A function
            inside another counts on its own, and its lines count for the other too. --list: `lines path:line name`,
            the longest first; a method is named with its type (`Model::default`).
  unwraps   calls `.unwrap()` in code, not in a comment or a string (goal M4). --list: the count of each file.
  headers   files without a module header (goal U1): no `//!` line before the first line of code; comments and
            inner attributes (`#![...]`) before it are no code. --list names them.
  menu-arms lines of code that name a `MenuId::` in the shared menu files (MENU_FILES), the part of the pages that
            has not moved into `PAGES` of ui/src/ui/pages.rs yet (ADR 0060). --list: the count of each file.
  ui-fields the fields of `struct Ui` (ui/src/ui.rs) that are not the state of one part: a field whose type is
            `module::Type` is a part's state, every other field is loose (ADR 0062). --list names the loose ones.

Without an option a report: it exits 0 whatever it counts. With --enforce the counts are numbers that only go down
(ADR 0037), kept in tools/code-metrics.txt, one `<count> <key>` per line as in tools/ratchet.txt: a count above its
number fails (make the function shorter, handle the error, write the header, or say in the pull request why the
number is raised by hand), a count below it fails too, so the file never allows more than the tree has: run --lower,
which writes the counts that went down and never one that went up. The layer `ratchet` of tools/check.sh runs
--enforce. Python's standard library; reads files, touches no scope.
"""
import argparse
import importlib.util
import os
import sys

TOOLS = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(TOOLS)
NUMBERS = "tools/code-metrics.txt"  # the counts that may only go down, `<count> <key>` per line
LONG = 150  # a function of more lines is long (goal S2)
# the files that answer for every menu page; what names one page there belongs into `PAGES` (ADR 0060)
MENU_FILES = ("ui/src/ui/menu_actions.rs", "ui/src/ui/menu_dialog.rs", "ui/src/ui/menu_items.rs",
              "ui/src/ui/menu_text.rs")


def load(name):
    """tools/<name>.py as a module, by its path: `python3 -I` puts no folder of the tools on the path."""
    spec = importlib.util.spec_from_file_location(name, os.path.join(TOOLS, name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


dup = load("duplicates")  # the reading of the tree and the tokens of Rust, one place for both
numbers = load("ratchet_numbers")  # the file of the numbers that only go down


def impl_name(toks):
    """The type an `impl` block is for, from the tokens between `impl` and its `{`: `Model` of
    `impl<T> Default for Model<T>`."""
    words, angle, prev = [], 0, ""
    for _, kind, piece in toks:
        if piece == "<":
            angle += 1
        elif piece == ">" and prev != "-":
            angle -= 1
        elif angle == 0 and piece == "for":
            words = []
        elif angle == 0 and piece == "where":
            break
        elif angle == 0 and kind == "ident" and piece not in ("dyn", "mut", "unsafe", "const"):
            words.append(piece)
        prev = piece
    return words[-1] if words else "impl"


def functions(text):
    """[(lines, first line, name)] of every `fn` item with a body in a file's text, in the order they close. The name
    has the block it is in: `Type::name` in an `impl` or `trait`, `outer::name` in another function."""
    toks = list(dup.tokens(text))
    found, scopes = [], []  # scopes: (kind, name, first line, brace depth outside it) of the open blocks with a name
    depth, paren, pending = 0, 0, None  # pending: (kind, name, first line, paren depth, token index) before its `{`
    for i, (line, kind, piece) in enumerate(toks):
        nxt = toks[i + 1] if i + 1 < len(toks) else (0, "", "")
        if kind == "ident" and piece == "fn" and nxt[1] == "ident":
            pending = ("fn", nxt[2], line, paren, i)
        elif kind == "ident" and piece == "impl" and pending is None:
            pending = ("impl", None, line, paren, i)
        elif kind == "ident" and piece == "trait" and nxt[1] == "ident" and pending is None:
            pending = ("trait", nxt[2], line, paren, i)
        elif piece in "([":
            paren += 1
        elif piece in ")]":
            paren -= 1
        elif piece == ";" and pending and paren == pending[3]:
            pending = None  # a declaration without a body: in a trait, an `extern` block
        elif piece == "{":
            if pending and paren == pending[3]:
                what, name, first, _, start = pending
                if what == "impl":
                    name = impl_name(toks[start + 1:i])
                scopes.append((what, name, first, depth))
                pending = None
            depth += 1
        elif piece == "}":
            depth -= 1
            pending = None
            if scopes and scopes[-1][3] == depth:
                what, name, first, _ = scopes.pop()
                if what == "fn":
                    outer = scopes[-1][1] + "::" if scopes else ""
                    found.append((line - first + 1, first, outer + name))
    return found


def unwraps(text):
    """The calls `.unwrap()` in a file's text, outside comments and strings."""
    toks = [piece for _, kind, piece in dup.tokens(text)]
    return sum(1 for i in range(len(toks) - 3) if toks[i:i + 4] == [".", "unwrap", "(", ")"])


def has_header(text):
    """Whether an inner doc comment (`//!` or `/*!`) comes before the first code; line and block comments (nested too)
    and inner attributes (`#![..]`) are no code."""
    i, n = 0, len(text)
    while i < n:
        if text[i].isspace():
            i += 1
        elif text.startswith(("//!", "/*!"), i):
            return True
        elif text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j + 1
        elif text.startswith("/*", i):
            depth, i = 1, i + 2
            while i < n and depth:
                if text.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif text.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
        elif text.startswith("#![", i):
            depth, i = 1, i + 3
            while i < n and depth:
                depth += {"[": 1, "]": -1}.get(text[i], 0)
                i += 1
        else:
            return False
    return False


def menu_arms(text):
    """Lines that name a `MenuId::` before a `//` comment."""
    return sum(1 for line in text.splitlines() if "MenuId::" in line.split("//")[0])


UI_STRUCT = "ui/src/ui.rs"  # where `struct Ui` is


def ui_fields(text):
    """The fields of `pub struct Ui` that are not a part's state (`name: module::Type`), by name."""
    loose, inside = [], False
    for line in text.splitlines():
        if line.startswith("pub struct Ui {"):
            inside = True
        elif inside and line.startswith("}"):
            break
        elif inside and line.startswith("    ") and not line.startswith("     "):
            field = line.strip()
            for vis in ("pub(crate) ", "pub(super) ", "pub "):
                field = field.removeprefix(vis)
            name, colon, ty = field.partition(":")
            if not colon or not name.isidentifier():
                continue
            module, sep, typ = ty.strip().rstrip(",").partition("::")
            if not (sep and module.isidentifier() and module.islower() and typ[:1].isupper() and typ.isidentifier()):
                loose.append(name)
    return loose


def counts(root):
    """The counts as (key, what, items, total); items are the lines --list prints. The keys name the counts in the
    summary and in tools/code-metrics.txt."""
    texts = dup.read_tree(root)
    long_fns, per_file, headless = [], [], []
    for rel in sorted(texts):
        text = texts[rel]
        long_fns += [(n, rel, first, name) for n, first, name in functions(text) if n > LONG]
        n = unwraps(text)
        if n:
            per_file.append((n, rel))
        if not has_header(text):
            headless.append(rel)
    arms = [(menu_arms(texts[rel]), rel) for rel in MENU_FILES if rel in texts]
    loose = ui_fields(texts.get(UI_STRUCT, ""))
    long_fns.sort(key=lambda f: (-f[0], f[1], f[2]))
    per_file.sort(key=lambda f: (-f[0], f[1]))
    return [
        ("long-fns", "functions over %d lines" % LONG, len(long_fns),
         ["%d %s:%d %s" % f for f in long_fns]),
        ("unwraps", "calls .unwrap() outside tests", sum(n for n, _ in per_file),
         ["%d %s" % f for f in per_file]),
        ("headers", "files without a //! header, of %d" % len(texts), len(headless), headless),
        ("menu-arms", "lines naming a MenuId in the shared menu files", sum(n for n, _ in arms),
         ["%d %s" % a for a in arms]),
        ("ui-fields", "fields of struct Ui that are not a part's state", len(loose), loose),
    ]


def read_numbers(root):
    """tools/code-metrics.txt as {key: number} (tools/ratchet_numbers.py)."""
    return numbers.read(root, NUMBERS)


def main(argv):
    ap = argparse.ArgumentParser(description="long functions, .unwrap() and modules without a header; a report, "
                                             "exit 0, unless --enforce or --lower")
    ap.add_argument("root", nargs="?", default=ROOT, help="top of the tree (default: this tree)")
    ap.add_argument("--list", action="store_true", help="each count with what is behind it")
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--enforce", action="store_true", help="exit 1 when a count differs from %s" % NUMBERS)
    mode.add_argument("--lower", action="store_true", help="write the counts that went down into %s" % NUMBERS)
    args = ap.parse_args(argv)
    result = counts(args.root)
    if args.list:
        for key, what, count, items in result:
            print("%s: %d %s" % (key, count, what))
            for item in items:
                print("  " + item)
    else:
        print("code-metrics: " + ", ".join("%s %d" % (k, n) for k, _, n, _ in result) + " (--list names them)")
    if args.enforce or args.lower:
        return numbers.enforce(args.root, NUMBERS, {k: n for k, _, n, _ in result}, args.lower)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
