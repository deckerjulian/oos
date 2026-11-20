#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""adr-index.py: the index of the decision records in docs/adr/README.md, written from the records (ADR 0065).

    tools/adr-index.py           write the index between its two marker lines
    tools/adr-index.py --check   exit 1 when the index is not what the records say (tools/check.sh text)

Each record gives its row: the number and the file from its path, the category from its folder, the decision from
its first line (`# ADR NNNN: the decision`), the status from its `- Status:` item, without what stands in brackets
there (dates, reasons), as `accepted`, `proposed`, `superseded by ADR NNNN`, `accepted, amended by ADR NNNN and ADR
MMMM` and the like. The index is not edited by hand: a new record, or a status that changes, is written in the
record and this tool is run. Two pull requests that both add a record meet in the index; after a rebase, the tool is
run again (AGENTS.md). Python's standard library only; reads and writes the tree, nothing else.
"""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ADR = "docs/adr"
README = os.path.join(ADR, "README.md")
BEGIN = "<!-- index: written by tools/adr-index.py from the records; run it instead of editing these lines -->"
END = "<!-- end of the index -->"
HEAD = "| ADR | Category | Decision | Status |\n|---|---|---|---|\n"
TITLE = re.compile(r"# ADR (\d{4}): (.+)$")
REF = re.compile(r"ADR (\d{4})")


def without_brackets(text):
    """`text` without what stands in round brackets, nested ones too, and the spaces before them."""
    while True:
        out = re.sub(r"\s*\([^()]*\)", "", text)
        if out == text:
            return out
        text = out


def status(text):
    """The status of a record's `- Status:` item, without its brackets, in the words of the index."""
    text = " ".join(without_brackets(" ".join(text.split())).split())
    head, amended = text, ""
    m = re.search(r",?\s*amended by\b", text)
    if m:
        head, amended = text[:m.start()], text[m.end():]
    head = head.strip().rstrip(",")
    refs = REF.findall(amended)
    if refs:
        names = ["ADR " + r for r in refs]
        head += ", amended by " + (names[0] if len(names) == 1 else ", ".join(names[:-1]) + " and " + names[-1])
    return head


def record(path):
    """(number, decision, status) of one record; ValueError when its first line or its status is missing."""
    with open(path, encoding="utf-8") as f:
        lines = f.read().split("\n")
    m = TITLE.match(lines[0])
    if not m:
        raise ValueError("%s: the first line is not `# ADR NNNN: the decision`" % path)
    for i, line in enumerate(lines):
        if line.startswith("- Status:"):
            text = line[len("- Status:"):]
            for more in lines[i + 1:]:
                if not more.startswith("  "):
                    break
                text += " " + more.strip()
            return m.group(1), m.group(2), status(text)
    raise ValueError("%s: no `- Status:` item" % path)


def rows(root):
    """The rows of the index, by number."""
    out = []
    for category in sorted(os.listdir(os.path.join(root, ADR))):
        folder = os.path.join(root, ADR, category)
        if not os.path.isdir(folder):
            continue
        for name in sorted(os.listdir(folder)):
            if re.match(r"\d{4}-.+\.md$", name):
                number, decision, state = record(os.path.join(folder, name))
                if not name.startswith(number + "-"):
                    raise ValueError("%s/%s: the file name and the first line name two numbers" % (category, name))
                out.append((number, "%s/%s" % (category, name), category, decision, state))
    numbers = [r[0] for r in out]
    twice = sorted({n for n in numbers if numbers.count(n) > 1})
    if twice:
        raise ValueError("a number names two records: " + ", ".join(twice))
    return sorted(out)


def table(root):
    return HEAD + "".join("| [%s](%s) | %s | %s | %s |\n" % r for r in rows(root))


def written(text, index):
    """`text` of README.md with `index` between the marker lines; ValueError without them."""
    if text.count(BEGIN) != 1 or text.count(END) != 1 or text.index(BEGIN) > text.index(END):
        raise ValueError("%s: the two marker lines of the index are not there once each, in order" % README)
    start = text.index(BEGIN) + len(BEGIN) + 1
    return text[:start] + index + text[text.index(END):]


def main(argv):
    check = argv[1:] == ["--check"]
    if argv[1:] and not check:
        print(__doc__.strip().split("\n\n")[1], file=sys.stderr)
        return 2
    path = os.path.join(ROOT, README)
    with open(path, encoding="utf-8") as f:
        text = f.read()
    try:
        new = written(text, table(ROOT))
    except ValueError as e:
        print("adr-index: %s" % e, file=sys.stderr)
        return 1
    if new == text:
        print("adr-index: the index of %s is what the records say" % README)
        return 0
    if check:
        print("adr-index: the index of %s is not what the records say: run tools/adr-index.py" % README,
              file=sys.stderr)
        return 1
    with open(path, "w", encoding="utf-8") as f:
        f.write(new)
    print("adr-index: wrote the index of %s" % README)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
