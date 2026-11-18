#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""code-metrics.py: long functions, `.unwrap()` and modules without a header in the four crates (issue 120).

    tools/code-metrics.py [ROOT]            one line: the three counts
    tools/code-metrics.py --list [ROOT]     each count with what is behind it
    tools/code-metrics.py --enforce [ROOT]  the counts against tools/code-metrics.txt; exit 1 when one differs
    tools/code-metrics.py --lower [ROOT]    write the counts that went down into tools/code-metrics.txt

ROOT is the top of a tree (default: this tree). Read from it as it is: the Rust files below hal/src, dsp/src, ui/src
and fpgad/src, without tests, as tools/duplicates.py reads them (its functions are used here): not a test module
inside a file, not a file or folder that is only built for tests, not a file `tests.rs` or `*_tests.rs`, not a folder
`tests/`.
  long-fns  functions of more than 150 lines (goal S2): an `fn` item with a body, from the line of `fn` to the line
            of its closing brace. Braces in strings, characters, raw strings and comments do not count. A function
            inside another counts on its own, and its lines count for the other too. --list: `lines path:line name`,
            the longest first; a method is named with its type (`Model::default`).
  unwraps   calls `.unwrap()` in code, not in a comment or a string (goal M4). --list: the count of each file.
  headers   files without a module header (goal U1): no `//!` line before the first line of code; comments and
            inner attributes (`#![...]`) before it are no code. --list names them.

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

# the reading of the tree and the tokens of Rust: tools/duplicates.py, one place for both
_spec = importlib.util.spec_from_file_location("duplicates", os.path.join(TOOLS, "duplicates.py"))
dup = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(dup)


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


def counts(root):
    """The three counts as (key, what, items, total); items are the lines --list prints. The keys name the counts in
    the summary and in tools/code-metrics.txt."""
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
    long_fns.sort(key=lambda f: (-f[0], f[1], f[2]))
    per_file.sort(key=lambda f: (-f[0], f[1]))
    return [
        ("long-fns", "functions over %d lines" % LONG, len(long_fns),
         ["%d %s:%d %s" % f for f in long_fns]),
        ("unwraps", "calls .unwrap() outside tests", sum(n for n, _ in per_file),
         ["%d %s" % f for f in per_file]),
        ("headers", "files without a //! header, of %d" % len(texts), len(headless), headless),
    ]


def read_numbers(root):
    """tools/code-metrics.txt as {key: number}; empty when there is none. A line that is not `<count> <key>` is an
    error."""
    path = os.path.join(root, NUMBERS)
    if not os.path.exists(path):
        return {}
    out = {}
    with open(path, encoding="utf-8") as f:
        for n, line in enumerate(f.read().split("\n"), 1):
            if not line.strip():
                continue
            parts = line.split()
            if len(parts) != 2 or not parts[0].isdigit():
                raise ValueError("%s:%d: not `<count> <key>`: %r" % (NUMBERS, n, line))
            out[parts[1]] = int(parts[0])
    return out


def enforce(root, now, lower):
    """Compares each count of `now` ({key: count}) with its number in tools/code-metrics.txt and prints a line for
    each that differs; with `lower` writes the counts that went down. Returns 0 when every count is its number (after
    --lower: when none went up), else 1."""
    kept = read_numbers(root)
    bad, out = 0, {}
    for key in sorted(set(kept) | set(now)):
        was, count = kept.get(key), now.get(key)
        if count is None:
            print("unknown: %s %d is no count of this tool: remove its line" % (key, was))
            bad = 1
        elif was is None:
            print("up: %s is new with %d: add `%d %s` to %s" % (key, count, count, key, NUMBERS))
            bad = 1
        elif count > was:
            print("up: %s %d -> %d (--list names them)" % (key, was, count))
            bad = 1
            out[key] = was
        elif count < was and lower:
            print("lowered: %s %d -> %d" % (key, was, count))
            out[key] = count
        elif count < was:
            print("down: %s %d -> %d (run tools/code-metrics.py --lower)" % (key, was, count))
            bad = 1
            out[key] = was
        else:
            out[key] = count
    if lower:
        with open(os.path.join(root, NUMBERS), "w", encoding="utf-8") as f:
            f.write("".join("%d %s\n" % (out[k], k) for k in sorted(out)))
    if not bad:
        print("code-metrics: %d numbers as recorded in %s" % (len(out), NUMBERS))
    return bad


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
        return enforce(args.root, {k: n for k, _, n, _ in result}, args.lower)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
