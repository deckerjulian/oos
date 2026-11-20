#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""duplicates.py: the blocks of code that occur in two or more places of the four crates (issue 120).

    tools/duplicates.py [ROOT]                    one line: duplicated blocks and lines, exact and loose
    tools/duplicates.py --list [--loose] [ROOT]   each block, longest first, with its places (file:first-last)
    tools/duplicates.py --min N [ROOT]            blocks of at least N lines (default 8)

ROOT is the top of a tree (default: this tree). Read from it as it is: the Rust files below hal/src, dsp/src, ui/src,
ui/system/src, ui/remote/src, ui/model/src and fpgad/src, without tests, as tools/code-size.py decides: not a test
module inside a file (`#[cfg(test)]` and `mod name {` to its closing brace at the same indentation), not a file or
folder that is only built for tests (`#[cfg(test)] mod name;` in its parent), not a file `tests.rs` or `*_tests.rs`, not
a folder `tests/`. More than tools/code-size.py reads: other attributes of the module, each on a line of its own, may
stand between `#[cfg(test)]` and `mod`, and a `#[path = "file.rs"]` among them names the file that is built for tests
only.

Each line is normalised: comments and white space go, the tokens are joined by one space; a line left empty or only
of brackets, `;`, `,` and `?` (`});`, `}`, `)?;`) is dropped, and a run of equal lines counts as one. In the loose
mode every identifier but a keyword becomes `I` and every literal `L`, so a copy with other names and numbers is
found too. A block is a run of at least N normalised lines that occurs in two or more places, found by hashing every
window of N lines and growing each pair of equal windows as far as the lines stay equal; two places of a block do
not overlap. A block is reported once, with all its places. Its duplicated lines are the lines of its places but the
first; a line in the copies of several blocks counts once. A block with fewer different lines than half its length
is the rows of a table, not code, and is left out.

A report only: it exits 0 whatever it finds. Python's standard library; reads files, touches no scope.
"""
import argparse
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# ui/system, ui/remote, ui/model: crates of the workspace ui/ (ADR 0064)
SOURCES = ("hal/src", "dsp/src", "ui/src", "ui/system/src", "ui/remote/src", "ui/model/src", "fpgad/src")
MIN = 8

TEST_MOD = re.compile(r"^(\s*)(?:pub(?:\([a-z]+\))? )?mod ([A-Za-z0-9_]+) \{$")
TEST_FILE = re.compile(r"^\s*(?:pub(?:\([a-z]+\))? )?mod ([A-Za-z0-9_]+);")
PATH_ATTR = re.compile(r"^\s*#\[path\s*=\s*\"([^\"]+)\"\]\s*$")
TOKEN = re.compile(r"""
    (?P<space>\s+)
  | (?P<line>//[^\n]*)
  | (?P<block>/\*)
  | (?P<raw>b?r(?P<hashes>\#*)")
  | (?P<str>[bc]?"(?:[^"\\]|\\.)*")
  | (?P<char>b?'(?:[^'\\\n]|\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.))')
  | (?P<life>'[A-Za-z_][A-Za-z0-9_]*)
  | (?P<num>0[xob][0-9a-fA-F_]+[a-z0-9]*|[0-9][0-9_]*(?:\.[0-9][0-9_]*)?(?:[eE][+-]?[0-9_]+)?[a-z0-9]*)
  | (?P<ident>(?:r\#)?[A-Za-z_][A-Za-z0-9_]*)
  | (?P<punct>.)
""", re.VERBOSE | re.DOTALL)
KEYWORDS = frozenset("""
    as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut
    pub ref return self Self static struct super trait true type unsafe use where while
""".split())
TRIVIAL = frozenset("{}()[];,?")


def is_test_name(rel):
    parts = rel.split("/")
    name = parts[-1]
    return "tests" in parts[:-1] or name == "tests.rs" or name.endswith("_tests.rs")


def rust_files(root):
    """The Rust files below SOURCES, relative to root, in the order of their paths."""
    found = []
    for src in SOURCES:
        for folder, dirs, names in os.walk(os.path.join(root, src)):
            dirs[:] = sorted(d for d in dirs if d not in ("target", "tests", "testdata"))
            found += [os.path.relpath(os.path.join(folder, n), root).replace(os.sep, "/")
                      for n in sorted(names) if n.endswith(".rs")]
    return sorted(found)


def strip_tests(rel, lines, test_paths):
    """The lines of a file with its test modules blanked; adds the files and folders it declares as built for tests
    only to `test_paths` (relative to the root, without `.rs`). Between `#[cfg(test)]` and its `mod` other attributes
    may stand, each on a line of its own: they are blanked too, and a `#[path = "file.rs"]` among them names the file
    (from the folder of this one) instead of the module's name."""
    here, stem = os.path.dirname(rel), os.path.splitext(os.path.basename(rel))[0]
    below = here if stem in ("mod", "lib", "main") else here + "/" + stem
    out, end, held, path = [], None, False, None
    for line in lines:
        if end is not None:
            out.append("")
            if line == end:
                end = None
            continue
        if held and line.lstrip().startswith("#["):
            m = PATH_ATTR.match(line)
            path = m.group(1) if m else path
            out.append("")
            continue
        if held:
            held = False
            m = TEST_MOD.match(line)
            if m:
                end = m.group(1) + "}"
                out.append("")
                continue
            m = TEST_FILE.match(line)
            if m and path:
                test_paths.add(os.path.splitext(os.path.normpath(here + "/" + path).replace(os.sep, "/"))[0])
            elif m:
                test_paths.add(below + "/" + m.group(1))
        held, path = line.strip() == "#[cfg(test)]", None
        # the attribute is test syntax too: blank, so it never lengthens a block of production code
        out.append("" if held else line)
    return out


def tokens(text):
    """(line number from 1, kind, text) of every token of Rust source; comments and white space are left out."""
    pos, line, n = 0, 1, len(text)
    while pos < n:
        m = TOKEN.match(text, pos)
        kind = m.lastgroup if m.lastgroup != "hashes" else "raw"
        start_line = line
        if kind == "block":
            depth, i = 1, m.end()
            while i < n and depth:
                if text.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif text.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
            end = i
        elif kind == "raw":
            close = '"' + m.group("hashes")
            i = text.find(close, m.end())
            end = n if i < 0 else i + len(close)
        else:
            end = m.end()
        piece = text[pos:end]
        line += piece.count("\n")
        pos = end
        if kind in ("space", "line", "block"):
            continue
        yield start_line, kind, piece


def normalise(text, loose):
    """[(line number, normalised line)] of a file's text: no empty or trivial lines, a run of equal lines once."""
    by_line = {}
    for line, kind, piece in tokens(text):
        if loose:
            if kind in ("raw", "str", "char", "num"):
                piece = "L"
            elif kind == "ident" and piece not in KEYWORDS:
                piece = "I"
            elif kind == "life":
                piece = "'I"
        by_line.setdefault(line, []).append(piece)
    out = []
    for line in sorted(by_line):
        toks = by_line[line]
        if all(t in TRIVIAL for t in toks):
            continue
        norm = " ".join(toks)
        if out and out[-1][1] == norm:
            continue
        out.append((line, norm))
    return out


def read_tree(root):
    """{relative path: text with test modules blanked} of the files that are no tests."""
    texts, test_paths = {}, set()
    for rel in rust_files(root):
        with open(os.path.join(root, rel), encoding="utf-8", errors="replace") as f:
            lines = f.read().split("\n")
        texts[rel] = "\n".join(strip_tests(rel, lines, test_paths))
    return {rel: text for rel, text in texts.items()
            if not is_test_name(rel)
            and not any(rel == t + ".rs" or rel.startswith(t + "/") for t in test_paths)}


def find(texts, minimum, loose):
    """[(length, [(path, first line, last line), ...])] of the duplicated blocks, longest first, and the number of
    duplicated lines."""
    ids, seq, where = {}, [], []
    for f, rel in enumerate(sorted(texts)):
        for line, norm in normalise(texts[rel], loose):
            seq.append(ids.setdefault(norm, len(ids)))
            where.append((rel, line))
        # a separator equal to nothing: no window and no match crosses into the next file
        seq.append(-1 - f)
        where.append((rel, 0))
    n = len(seq)
    windows = {}
    run = 0  # lines since the last separator
    for i in range(n):
        run = run + 1 if seq[i] >= 0 else 0
        if run >= minimum:
            windows.setdefault(tuple(seq[i - minimum + 1:i + 1]), []).append(i - minimum + 1)
    blocks = {}  # the lines of a block -> its starts
    for starts in windows.values():
        if len(starts) < 2:
            continue
        for a in range(len(starts)):
            p = starts[a]
            for q in starts[a + 1:]:
                # only a pair that cannot grow backwards: the longer pair before it holds this one
                if p > 0 and q > 0 and seq[p - 1] == seq[q - 1] and seq[p - 1] >= 0:
                    continue
                k = minimum
                while q + k < n and seq[p + k] == seq[q + k] and seq[p + k] >= 0:
                    k += 1
                if where[p][0] == where[q][0]:
                    k = min(k, q - p)  # two places of one block do not overlap
                if k < minimum:
                    continue
                places = blocks.setdefault(tuple(seq[p:p + k]), set())
                places.update((p, q))
    found, copies = [], set()
    for key, starts in blocks.items():
        starts = sorted(starts)
        # places of one block in one file that overlap: keep the first
        kept = []
        for s in starts:
            if not kept or s >= kept[-1] + len(key) or where[s][0] != where[kept[-1]][0]:
                kept.append(s)
        # rows of a table, not code: fewer different lines than half the block
        if len(kept) < 2 or 2 * len(set(key)) < len(key):
            continue
        for s in kept[1:]:
            copies.update(range(s, s + len(key)))
        found.append((len(key), [(*where[s], where[s + len(key) - 1][1]) for s in kept]))
    found.sort(key=lambda b: (-b[0], -len(b[1]), b[1]))
    return found, len(copies)


def summary(blocks, lines, minimum, mode):
    return f"{len(blocks)} blocks, {lines} duplicated lines ({mode}, >= {minimum} lines)"


def main(argv):
    ap = argparse.ArgumentParser(description="Blocks of code that occur in two or more places (a report).")
    ap.add_argument("root", nargs="?", default=ROOT, help="the top of a tree (default: this tree)")
    ap.add_argument("--min", type=int, default=MIN, metavar="N", help=f"lines a block has at least (default {MIN})")
    ap.add_argument("--loose", action="store_true", help="list the blocks of the loose mode")
    ap.add_argument("--list", action="store_true", help="each block with its places, longest first")
    args = ap.parse_args(argv[1:])
    minimum = max(args.min, 1)
    texts = read_tree(args.root)
    exact = find(texts, minimum, False)
    loose = find(texts, minimum, True)
    if args.list:
        for length, places in (loose if args.loose else exact)[0]:
            print(f"{length} lines, {len(places)} places:")
            for rel, first, last in places:
                print(f"  {rel}:{first}-{last}")
    print(f"duplicates: {summary(*exact, minimum, 'exact')}; loose mode: {summary(*loose, minimum, 'loose')}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
