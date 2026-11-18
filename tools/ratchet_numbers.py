# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""ratchet_numbers.py: the numbers that only go down (ADR 0037) of the counting tools, one place for their file.

    ratchet_numbers.read(root, "tools/<tool>.txt")                 {key: number}
    ratchet_numbers.enforce(root, "tools/<tool>.txt", now, lower)  0 or 1: --enforce, and --lower when `lower`

A file of numbers is one `<count> <key>` per line, as tools/ratchet.txt; the tool that keeps it is the script of the
same name next to it (`tools/code-metrics.txt` is kept by `tools/code-metrics.py`). Used by tools/scpi-coverage.py
and tools/code-metrics.py, which load it by its path with importlib (they run as `python3 -I`, which puts no folder
of theirs on the path). Not `numbers.py`: that name would hide the module `numbers` of the standard library from a
script with tools/ on its path. Python's standard library; reads and writes that file only.
"""
import os


def read(root, rel):
    """The file `rel` below `root` as {key: number}; empty when there is none. A line that is not `<count> <key>` is
    an error."""
    path = os.path.join(root, rel)
    if not os.path.exists(path):
        return {}
    out = {}
    with open(path, encoding="utf-8", errors="replace") as f:
        for n, line in enumerate(f.read().split("\n"), 1):
            if not line.strip():
                continue
            parts = line.split()
            if len(parts) != 2 or not parts[0].isdigit():
                raise ValueError("%s:%d: not `<count> <key>`: %r" % (rel, n, line))
            out[parts[1]] = int(parts[0])
    return out


def enforce(root, rel, now, lower):
    """Compares each count of `now` ({key: count}) with its number in `rel` and prints a line for each that differs:
    a count above its number, a new key and a number without a count fail; a count below its number fails too, unless
    `lower`, which writes the counts that went down and never one that went up. Returns 0 when every count is its
    number (after `lower`: when none went up), else 1."""
    tool = os.path.splitext(os.path.basename(rel))[0]
    script = os.path.join(os.path.dirname(rel), tool + ".py")
    kept = read(root, rel)
    bad, out = 0, {}
    for key in sorted(set(kept) | set(now)):
        was, count = kept.get(key), now.get(key)
        if count is None:
            print("unknown: %s %d is no count of this tool: remove its line" % (key, was))
            bad = 1
        elif was is None:
            print("up: %s is new with %d: add `%d %s` to %s" % (key, count, count, key, rel))
            bad = 1
        elif count > was:
            print("up: %s %d -> %d (--list names them)" % (key, was, count))
            bad = 1
            out[key] = was
        elif count < was and lower:
            print("lowered: %s %d -> %d" % (key, was, count))
            out[key] = count
        elif count < was:
            print("down: %s %d -> %d (run %s --lower)" % (key, was, count, script))
            bad = 1
            out[key] = was
        else:
            out[key] = count
    if lower:
        with open(os.path.join(root, rel), "w", encoding="utf-8") as f:
            f.write("".join("%d %s\n" % (out[k], k) for k in sorted(out)))
    if not bad:
        print("%s: %d numbers as recorded in %s" % (tool, len(out), rel))
    return bad
