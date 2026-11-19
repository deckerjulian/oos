#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""changelog.py: the changes not yet released are files of their own in changelog.d/, one per change (ADR 0065).

    tools/changelog.py                   print the next release's section, `## Unreleased`, with the changes in it
    tools/changelog.py --check           exit 1 when a file of changelog.d/ is not a change as below (check.sh text)
    tools/changelog.py --release X.Y.Z   write them into CHANGELOG.md as `## X.Y.Z` and remove them (the release PR)

A change is a file `changelog.d/<short-name>.md`: a first line that names its part, one of `Added:`, `Interface:`,
`Removed:`, `Changed:`, `Fixed:`, an empty line, then one or more items (`- ` and their text, lines of at most 120
columns, the next lines of an item indented by two spaces). A pull request adds its own file, so two of them never
change the same lines; CHANGELOG.md itself changes only in the pull request of a release. The release's section goes
above the newest one, and the pull request of the release writes its first lines (`Released on ...`) by hand. Should
CHANGELOG.md still have a section `## Unreleased`, it becomes the release's, its items first, and the items of the
files go to the end of their part, in the order of the file names; a part that is missing is added in the order above.
Python's standard library only; touches nothing outside the tree.
"""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FOLDER = "changelog.d"
LOG = "CHANGELOG.md"
PARTS = ("Added:", "Interface:", "Removed:", "Changed:", "Fixed:")
UNRELEASED = "## Unreleased"
WIDTH = 120


def fragments(root):
    """[(name, part, [line, ...])] of the files of changelog.d/, by name; ValueError for one that is not a change."""
    folder = os.path.join(root, FOLDER)
    out = []
    for name in sorted(os.listdir(folder)) if os.path.isdir(folder) else []:
        if name == "README.md" or not name.endswith(".md"):
            continue
        with open(os.path.join(folder, name), encoding="utf-8") as f:
            text = f.read()
        out.append((name,) + parse(name, text))
    return out


def parse(name, text):
    """(part, the lines of its items) of one change; ValueError naming what is wrong."""
    if not text.endswith("\n"):
        raise ValueError("%s/%s: no newline at the end" % (FOLDER, name))
    lines = text.rstrip("\n").split("\n")
    if lines[0] not in PARTS:
        raise ValueError("%s/%s: the first line is not one of %s" % (FOLDER, name, " ".join(PARTS)))
    if len(lines) < 3 or lines[1] != "" or not lines[2].startswith("- ") or not lines[2][2:].strip():
        raise ValueError("%s/%s: an empty line, then an item (`- ...`), after the part" % (FOLDER, name))
    items = lines[2:]
    for k, line in enumerate(items, 3):
        if len(line) > WIDTH:
            raise ValueError("%s/%s:%d: longer than %d columns" % (FOLDER, name, k, WIDTH))
        if line and not (line.startswith("- ") or line.startswith("  ")):
            raise ValueError("%s/%s:%d: neither an item (`- `) nor its next line (two spaces)" % (FOLDER, name, k))
        if line and not line[2:].strip():
            raise ValueError("%s/%s:%d: an item or its next line without text" % (FOLDER, name, k))
    return lines[0], items


def merged(section, changes):
    """The lines of `## Unreleased` (without its heading) with the items of `changes` at the end of their parts."""
    lines = list(section)
    for _, part, items in changes:
        if part in lines:
            at = lines.index(part) + 1
            while at < len(lines) and lines[at] not in PARTS and not lines[at].startswith("## "):
                at += 1
            while at > 0 and lines[at - 1] == "":
                at -= 1
            lines[at:at] = items
        else:
            later = [lines.index(p) for p in PARTS[PARTS.index(part) + 1:] if p in lines]
            at = min(later) if later else len(lines)
            while at > 0 and lines[at - 1] == "" and not later:
                at -= 1
            lines[at:at] = ([""] if at and lines[at - 1] != "" else []) + [part, ""] + items + [""]
    while lines and lines[-1] == "":
        lines.pop()
    return lines


def split(text):
    """(before, the lines of `## Unreleased` without its heading, after) of CHANGELOG.md; without that section, the
    lines before the newest release, none, and the rest."""
    lines = text.rstrip("\n").split("\n")
    if UNRELEASED not in lines:
        start = next((k for k, line in enumerate(lines) if line.startswith("## ")), len(lines))
        return lines[:start], [], lines[start:]
    start = lines.index(UNRELEASED)
    end = next((k for k in range(start + 1, len(lines)) if lines[k].startswith("## ")), len(lines))
    section = lines[start + 1:end]
    while section and section[0] == "":
        section.pop(0)
    while section and section[-1] == "":
        section.pop()
    return lines[:start], section, lines[end:]


def main(argv):
    args = argv[1:]
    try:
        changes = fragments(ROOT)
        if args == ["--check"]:
            print("changelog: %d changes in %s, each one as it should be" % (len(changes), FOLDER))
            return 0
        with open(os.path.join(ROOT, LOG), encoding="utf-8") as f:
            before, section, after = split(f.read())
        lines = merged(section, changes)
        if not args:
            print("\n".join([UNRELEASED, ""] + lines))
            return 0
        if len(args) == 2 and args[0] == "--release" and re.fullmatch(r"\d+\.\d+\.\d+", args[1]):
            out = before + ["## " + args[1], ""] + lines + ([""] + after if after else [])
            with open(os.path.join(ROOT, LOG), "w", encoding="utf-8") as f:
                f.write("\n".join(out) + "\n")
            for name, _, _ in changes:
                os.remove(os.path.join(ROOT, FOLDER, name))
            print("changelog: %d changes written into %s as ## %s" % (len(changes), LOG, args[1]))
            return 0
    except ValueError as e:
        print("changelog: %s" % e, file=sys.stderr)
        return 1
    print(__doc__.strip().split("\n\n")[1], file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
