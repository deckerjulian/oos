#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""code-size.py: how many lines of code the four crates have, and what a change adds or removes (issue 76).

    tools/code-size.py [TREE]          the lines of code of each crate of TREE (default: this tree)
    tools/code-size.py BASE TREE       a table for a pull request: each crate at BASE, at TREE, and the difference

What counts is code that runs: lines of the Rust files under hal/, dsp/, ui/ and fpgad/ that are not empty and not
only a comment. Tests do not count, so nobody trims them to look better: not a test module inside a file
(`#[cfg(test)] mod name {` to its closing brace at the start of a line), not a file that is only built for tests
(`#[cfg(test)] mod name;` in its parent), not a folder `tests/`.

A number to see, not a gate: a feature adds code. Python's standard library; reads the files of the tree as they are.
"""
import os
import re
import sys

CRATES = ("hal", "dsp", "ui", "fpgad")
TEST_MOD = re.compile(r"^(?:pub(?:\([a-z]+\))? )?mod ([A-Za-z0-9_]+) \{$")
TEST_FILE = re.compile(r"^\s*(?:pub(?:\([a-z]+\))? )?mod ([A-Za-z0-9_]+);")


def count(path, test_files):
    """Lines of code of one file; adds the files it declares as built for tests only to `test_files`."""
    with open(path, encoding="utf-8", errors="replace") as f:
        lines = f.read().split("\n")
    here, stem = os.path.dirname(path), os.path.splitext(os.path.basename(path))[0]
    below = here if stem in ("mod", "lib", "main") else os.path.join(here, stem)
    n, skip, held = 0, False, False
    for line in lines:
        if skip:
            skip = line != "}"
            continue
        if held:
            held = False
            if TEST_MOD.match(line):
                skip = True
                continue
            m = TEST_FILE.match(line)
            if m:
                test_files.add(os.path.join(below, m.group(1) + ".rs"))
                test_files.add(os.path.join(below, m.group(1)))
                continue
        text = line.strip()
        if text == "#[cfg(test)]":
            held = True
            continue
        if text and not text.startswith("//"):
            n += 1
    return n


def crate(tree, name):
    root = os.path.join(tree, name)
    files = []
    for folder, dirs, names in os.walk(root):
        dirs[:] = sorted(d for d in dirs if d not in ("target", "tests", "testdata"))
        files += [os.path.join(folder, f) for f in sorted(names) if f.endswith(".rs")]
    test_files, counts = set(), {}
    for path in files:
        counts[path] = count(path, test_files)
    return sum(n for path, n in counts.items()
               if not any(path == t or path.startswith(t + os.sep) for t in test_files))


def sizes(tree):
    return {name: crate(tree, name) for name in CRATES}


def main(argv):
    if len(argv) == 3:
        base, new = sizes(argv[1]), sizes(argv[2])
        print("### Lines of code\n")
        print("| Crate | Base | This change | Difference |\n|---|---|---|---|")
        for name in CRATES + ("all",):
            a = sum(base.values()) if name == "all" else base[name]
            b = sum(new.values()) if name == "all" else new[name]
            print(f"| {name} | {a} | {b} | {b - a:+d} |")
        print("\nWithout tests, comments and empty lines. A number to see, not a gate.")
        return 0
    tree = argv[1] if len(argv) == 2 else os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    for name, n in sizes(tree).items():
        print(f"{n} {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
