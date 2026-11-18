#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""layers.py: the places where code crosses a layer of the crates (ADR 0001, issue 120 goal S3).

    tools/layers.py [ROOT]            one line: the three counts
    tools/layers.py --list [ROOT]     each count with its places, `path:line text  (what it names)`
    tools/layers.py --enforce [ROOT]  the counts against tools/layers.txt; exit 1 when one differs
    tools/layers.py --lower [ROOT]    write the counts that went down into tools/layers.txt

ROOT is the top of a tree (default: this tree). Read from it as it is: the Rust files below hal/src, dsp/src, ui/src
and fpgad/src, without tests, as tools/duplicates.py reads them (its functions are used here), and the dependencies
of hal/Cargo.toml and dsp/Cargo.toml. Comments do not count; strings only where said.
  board   paths of the UI to the code of the DHO board that do not go through `board::Board`: `board::dho`
          (`crate::board::dho`, `super::board::dho`, ...) outside ui/src/board/, and `oos_hal::dho` outside
          ui/src/board/dho/, the same rule as the test `board::boundary` of ui/src/board/mod.rs, read here from
          tokens instead of lines.
  hal-up  hal naming a crate above it: `oos_ui`, `oos_dsp` or `oos_fpgad` in hal/src, and every dependency of
          hal/Cargo.toml but `libc`.
  dsp-hw  dsp naming hardware: `oos_hal`, `libc`, `std::fs`, `std::os`, `ioctl` or `mmap` in a path of dsp/src, `/dev/`
          in a string of it, and every dependency of dsp/Cargo.toml (dsp works on `&[f32]` and depends on nothing).
A path is counted as written, `a::b::c` once, and a `use` tree as its leaves written out (`use oos_hal::dho::{hz,
seq::Settings}` is two paths, `oos_hal::dho::hz` and `oos_hal::dho::seq::Settings`, on the line of each leaf); the same
path twice on one line counts once. A dependency is one of `[dependencies]` and `[build-dependencies]`, also under
`[target.*]`; `[dev-dependencies]` serve the tests and do not count.

Without an option a report: it exits 0 whatever it counts. With --enforce the counts are numbers that only go down
(ADR 0037), kept in tools/layers.txt, one `<count> <key>` per line as in tools/ratchet.txt (tools/ratchet_numbers.py):
a count above its number fails (put the code where its layer is, or say in the pull request why the number is raised
by hand), a count below it fails too, so the file never allows more than the tree has: run --lower, which writes the
counts that went down and never one that went up. The layer `ratchet` of tools/check.sh runs --enforce. Python's
standard library; reads files, touches no scope.
"""
import argparse
import importlib.util
import os
import re
import sys

TOOLS = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(TOOLS)
NUMBERS = "tools/layers.txt"  # the counts that may only go down, `<count> <key>` per line
ABOVE_HAL = ("oos_ui", "oos_dsp", "oos_fpgad")  # the crates of the tree that may use hal; hal names none of them
HARDWARE = ("oos_hal", "libc")  # crates dsp may not name
SYSTEM = (("std", "fs"), ("std", "os"))  # modules of the standard library that reach files and the system
CALLS = ("ioctl", "mmap")  # names of calls into the hardware
DEPENDS = {  # Cargo.toml: (its count, the dependencies it may have)
    "hal/Cargo.toml": ("hal-up", {"libc"}),
    "dsp/Cargo.toml": ("dsp-hw", set()),
}


def load(name):
    """tools/<name>.py as a module, by its path: `python3 -I` puts no folder of the tools on the path."""
    spec = importlib.util.spec_from_file_location(name, os.path.join(TOOLS, name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


dup = load("duplicates")  # the reading of the tree and the tokens of Rust, one place for both
numbers = load("ratchet_numbers")  # the file of the numbers that only go down


def paths(toks):
    """[(line, [name, ...])] of the paths among the tokens of a file (tools/duplicates.py `tokens`): each `a::b::c` as
    written, on the line of its last name; a `use` tree as its leaves written out (`self` in a group is the path
    before it, `*` a name of its own); a leading `::` and an `as` alias left out."""
    out, n = [], len(toks)

    def sep(j):
        return j + 1 < n and toks[j][2] == ":" and toks[j + 1][2] == ":"

    def tree(j, prefix):
        """Adds the leaves of the use tree at toks[j] below `prefix`; returns the index after it."""
        names = list(prefix)
        if sep(j):
            j += 2
        while j < n:
            line, kind, piece = toks[j]
            if piece == "{":
                j += 1
                while j < n and toks[j][2] != "}":
                    j = tree(j, names)
                    if j < n and toks[j][2] == ",":
                        j += 1
                return j + 1
            if piece == "*":
                out.append((line, names + ["*"]))
                return j + 1
            if kind != "ident":
                return j
            if piece != "self":
                names.append(piece)
            j += 1
            if sep(j):
                j += 2
                continue
            if j < n and toks[j][2] == "as":
                j += 2
            out.append((line, names))
            return j
        return j

    i = 0
    while i < n:
        line, kind, piece = toks[i]
        if kind == "ident" and piece == "use":
            i = tree(i + 1, [])
        elif kind == "ident" and not (i >= 3 and sep(i - 2) and toks[i - 3][1] == "ident"):
            names, j = [piece], i + 1
            while sep(j) and j + 2 < n and toks[j + 2][1] == "ident":
                names.append(toks[j + 2][2])
                j += 3
            out.append((toks[j - 1][0], names))
            i = j
        else:
            i += 1
    return out


def board_path(rel, names):
    """The part of a path of the UI that names the DHO board's code where it should not, or None."""
    for i in range(1, len(names)):
        if names[i] != "dho":
            continue
        if i == 1 and names[0] == "oos_hal" and not rel.startswith("ui/src/board/dho/"):
            return "::".join(names)
        if names[i - 1] == "board" and not rel.startswith("ui/src/board/"):
            return "::".join(names)
    return None


def crossings(rel, text):
    """[(key, line, what it names)] of the crossings in one file's text (tests already blanked)."""
    toks = list(dup.tokens(text))
    found = []
    for line, names in paths(toks):
        if rel.startswith("ui/src/"):
            hit = board_path(rel, names)
            if hit:
                found.append(("board", line, hit))
        elif rel.startswith("hal/src/") and names[0] in ABOVE_HAL:
            found.append(("hal-up", line, "::".join(names)))
        elif rel.startswith("dsp/src/") and (names[0] in HARDWARE or tuple(names[:2]) in SYSTEM
                                             or any(x in CALLS for x in names)):
            found.append(("dsp-hw", line, "::".join(names)))
    if rel.startswith("dsp/src/"):
        found += [("dsp-hw", line, piece) for line, kind, piece in toks if kind in ("str", "raw") and "/dev/" in piece]
    return found


def dependencies(root, rel):
    """[(line, name, text of the line)] of the dependencies of a Cargo.toml that build the crate: `[dependencies]`,
    `[build-dependencies]`, and the same under `[target.*]`; not `[dev-dependencies]`. Read line by line (keys at the
    start of a line, or a table `[...dependencies.<name>]`), so it runs on every Python 3 the tree's hosts have."""
    path = os.path.join(root, rel)
    if not os.path.exists(path):
        return []
    with open(path, encoding="utf-8") as f:
        lines = f.read().split("\n")
    table = re.compile(r"^\s*\[\s*(?:target\.(?:'[^']*'|\"[^\"]*\"|[^.\]]+)\.)?(build-dependencies|dependencies|[^\]]*)"
                       r"(?:\.([A-Za-z0-9_-]+))?\s*\]")
    key = re.compile(r"^\s*([A-Za-z0-9_-]+)\s*[.=]")
    out, inside = [], False
    for i, line in enumerate(lines, 1):
        t = table.match(line)
        if t:
            builds = t.group(1) in ("dependencies", "build-dependencies")
            inside = builds and t.group(2) is None
            if builds and t.group(2):
                out.append((i, t.group(2), line.strip()))
            continue
        k = key.match(line) if inside else None
        if k:
            out.append((i, k.group(1), line.strip()))
    return out


def counts(root):
    """The three counts as (key, what, items); items are the lines --list prints. The keys name the counts in the
    summary and in tools/layers.txt."""
    texts = dup.read_tree(root)
    found = {"board": [], "hal-up": [], "dsp-hw": []}
    for rel in sorted(texts):
        lines = texts[rel].split("\n")
        for key, line, what in sorted(set(crossings(rel, texts[rel])), key=lambda c: (c[1], c[2])):
            found[key].append("%s:%d %s  (%s)" % (rel, line, lines[line - 1].strip(), what))
    for rel, (key, allowed) in DEPENDS.items():
        found[key] += ["%s:%d %s  (dependency %s)" % (rel, line, text, name)
                       for line, name, text in dependencies(root, rel) if name not in allowed]
    return [
        ("board", "paths to the DHO board's code past board::Board", found["board"]),
        ("hal-up", "names of the crates above hal in hal", found["hal-up"]),
        ("dsp-hw", "names of hardware in dsp", found["dsp-hw"]),
    ]


def main(argv):
    ap = argparse.ArgumentParser(description="the places where code crosses a layer of the crates; a report, exit 0, "
                                             "unless --enforce or --lower")
    ap.add_argument("root", nargs="?", default=ROOT, help="top of the tree (default: this tree)")
    ap.add_argument("--list", action="store_true", help="each count with its places")
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--enforce", action="store_true", help="exit 1 when a count differs from %s" % NUMBERS)
    mode.add_argument("--lower", action="store_true", help="write the counts that went down into %s" % NUMBERS)
    args = ap.parse_args(argv)
    result = counts(args.root)
    if args.list:
        for key, what, items in result:
            print("%s: %d %s" % (key, len(items), what))
            for item in items:
                print("  " + item)
    else:
        print("layers: " + ", ".join("%s %d" % (k, len(items)) for k, _, items in result) + " (--list names them)")
    if args.enforce or args.lower:
        return numbers.enforce(args.root, NUMBERS, {k: len(items) for k, _, items in result}, args.lower)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
