#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""link-check.py: every relative link and every named file in the documents exists (layer `text` of tools/check.sh).

    tools/link-check.py            every tracked Markdown file; prints what does not resolve, exit 1 when there is one

Checked:
  - a relative link `[text](path)` or `[text](path#heading)`: the file exists, and the heading when one is named;
    `[text](#heading)` in the file itself;
  - a wiki link `[[Page]]` in wiki/: the page exists;
  - a path in backticks that starts with a folder of the tree (`ui/src/scpi.rs`, `tools/check.sh`): the file or folder
    exists, from the top of the tree or from the folder of the document (a crate's README names its own files
    shortly). A path that .gitignore leaves out is fine: what a build makes is not in the tree. Not checked in the
    decision records and the changelog, which also tell what was there once, and not a path with a placeholder in
    it (`<name>`, `*`, `$VAR`, `...`);
  - an item named behind such a path (`ui/model/src/settings/tests.rs::every_row_is_complete`): the file declares it, as
    a function, a test, a type, a constant or a module. A recipe that names the test that keeps it true is checked
    this way.

Not checked: links to other sites. A path that is meant although nothing is there (a file a build makes, a path on
the scope that looks like one of the tree, a name that is no path) goes into tools/link-check-allow.txt, one a
line. Python's standard library and git; reads the tree only.
"""
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TOP = ("hal/", "dsp/", "ui/", "fpgad/", "gpu/", "boot/", "kernel/", "rootfs/", "tools/", "docs/", "wiki/", ".github/",
       "keys/")
HISTORY = ("docs/adr/", "CHANGELOG.md")
LINK = re.compile(r"(?<!\!)\[[^\]\n]*\]\(([^)\s]+)\)")
IMAGE = re.compile(r"!\[[^\]\n]*\]\(([^)\s]+)\)")
WIKI = re.compile(r"\[\[([^\]|\n]+)(?:\|[^\]\n]*)?\]\]")
TICK = re.compile(r"`([^`\n]+)`")
ITEM = r"\b(?:fn|def|struct|enum|trait|const|static|type|mod|class)\s+%s\b"
PLACEHOLDER = re.compile(r"[<>*$|{}]|\.\.\.|NNNN|\s")


def tracked(pattern):
    out = subprocess.run(("git", "-C", ROOT, "ls-files", pattern), check=True, capture_output=True, text=True).stdout
    return [line for line in out.split("\n") if line]


def slug(heading):
    """The anchor GitHub gives a heading: lower case, punctuation gone, spaces to hyphens."""
    text = re.sub(r"`|\*|_(?=\w)|(?<=\w)_", "", heading.strip())
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text).lower()
    text = re.sub(r"[^\w\- ]", "", text, flags=re.UNICODE)
    return text.replace(" ", "-")


def anchors(path, cache={}):
    if path not in cache:
        found, counts, fence = set(), {}, False
        with open(os.path.join(ROOT, path), encoding="utf-8", errors="replace") as f:
            for line in f:
                if line.startswith("```"):
                    fence = not fence
                m = None if fence else re.match(r"#{1,6}\s+(.*?)\s*#*\s*$", line)
                if m:
                    s = slug(m.group(1))
                    n = counts.get(s, 0)
                    counts[s] = n + 1
                    found.add(s if n == 0 else f"{s}-{n}")
                found.update(re.findall(r'<a (?:id|name)="([^"]+)"', line))
        cache[path] = found
    return cache[path]


def ignored(name):
    """Left out by .gitignore: made by a build, or private to who made it."""
    # with a slash too: a folder that is not there cannot be told from a file
    return any(subprocess.run(("git", "-C", ROOT, "check-ignore", "-q", n), capture_output=True).returncode == 0
               for n in (name, name.rstrip("/") + "/"))


def declares(file, item):
    """Whether a source file declares `item`: a function, a test, a type, a constant or a module of that name."""
    with open(file, encoding="utf-8", errors="replace") as f:
        return re.search(ITEM % re.escape(item), f.read()) is not None


def allowed():
    try:
        with open(os.path.join(ROOT, "tools", "link-check-allow.txt"), encoding="utf-8") as f:
            return {line.strip() for line in f if line.strip() and not line.startswith("#")}
    except FileNotFoundError:
        return set()


def check(path, allow):
    # a change in changelog.d/ is written for CHANGELOG.md (tools/changelog.py), so its links start at the top
    here = "" if path.startswith("changelog.d/") and path != "changelog.d/README.md" else os.path.dirname(path)
    problems, fence = [], False
    with open(os.path.join(ROOT, path), encoding="utf-8", errors="replace") as f:
        lines = f.read().split("\n")
    for number, line in enumerate(lines, 1):
        if line.startswith("```"):
            fence = not fence
            continue
        if fence:
            continue
        for target in LINK.findall(line) + IMAGE.findall(line):
            if re.match(r"[a-z][a-z0-9+.-]*:", target) or target.startswith("//"):
                continue
            file, _, anchor = target.partition("#")
            dest = os.path.normpath(os.path.join(here, file)) if file else path
            if dest.startswith(".."):
                continue  # out of the tree: the wiki's own links to the repository's pages
            if not os.path.exists(os.path.join(ROOT, dest)):
                problems.append((number, f"link to {target}: no such file"))
            elif anchor and dest.endswith(".md") and anchor.lower() not in anchors(dest):
                problems.append((number, f"link to {target}: no heading '{anchor}' in {dest}"))
        if path.startswith("wiki/"):
            for page in WIKI.findall(line):
                if not os.path.exists(os.path.join(ROOT, "wiki", page.strip().replace(" ", "-") + ".md")):
                    problems.append((number, f"wiki link [[{page}]]: no such page"))
        if path.startswith(HISTORY):
            continue
        for name in TICK.findall(line):
            name = name.rstrip(".,:;")
            if not name.startswith(TOP) or PLACEHOLDER.search(name) or name in allow:
                continue
            item = name.rpartition("::")[2] if "::" in name else ""
            name = name.split(":")[0].split("#")[0]
            bases = ("", here, os.path.join(here, "src"))
            found = [os.path.join(ROOT, base, name) for base in bases if os.path.exists(os.path.join(ROOT, base, name))]
            if not (found or ignored(name)):
                problems.append((number, f"`{name}`: no such file or folder"))
            elif item and found and os.path.isfile(found[0]) and not declares(found[0], item):
                problems.append((number, f"`{name}::{item}`: {name} declares no {item}"))
    return problems


def main():
    allow = allowed()
    bad = 0
    files = tracked("*.md")
    for path in files:
        if path.startswith(("kernel/", "rootfs/initramfs/")):
            continue
        for number, text in check(path, allow):
            print(f"{path}:{number}: {text}")
            bad += 1
    if bad:
        print(f"{bad} that do not resolve: correct them, or list a path that is meant in tools/link-check-allow.txt")
        return 1
    print(f"{len(files)} Markdown files: every relative link and named file resolves")
    return 0


if __name__ == "__main__":
    sys.exit(main())
