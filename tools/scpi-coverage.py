#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""scpi-coverage.py: what a remote command or a setting lacks of the parts it should have (AGENTS.md, ADR 0022/0023).

    tools/scpi-coverage.py [ROOT]            one line: the four counts, missing/total
    tools/scpi-coverage.py --list [ROOT]     each count with the names behind it
    tools/scpi-coverage.py --enforce [ROOT]  the counts against tools/scpi-coverage.txt; exit 1 when one differs
    tools/scpi-coverage.py --lower [ROOT]    write the counts that went down into tools/scpi-coverage.txt

ROOT is the top of a tree (default: this tree). Read from it as it is:
  words-docs     the command words of the interpreter, `kw(<node>, "Word")` in ui/src/scpi.rs and the files of its
                 folder ui/src/scpi/, where <node> holds a node of the header (NODE below; not an argument such as
                 `arg` or `item`), and the nodes of the commands of the settings table (`Scpi::Cmd("A:B")` in
                 ui/src/settings/), that docs/scpi.md does not name. A word is named in a file when `[:*]Word\\b` is
                 in it, ignoring case, or when it follows `{` or `|` as one of the choices of `{A|B}`;
  words-tests    the same words that ui/tests/scpi.txt does not name;
  queries-tests  the queries of docs/scpi.md (the first command of a table row, without its arguments, when it ends
                 in `?` or `[?]`) that no `scpi` line of ui/tests/scpi.txt asks, in the long or the short form;
  fields-scpi    the fields of the model that ui/src/store.rs writes to a setup file (`m.<field>` in `setup_text`
                 and in the functions it hands `m` to, and the fields of the rows of the settings table) that the
                 interpreter never names as `.field` and no row with a command reaches. An upper bound:
                 a field can be set by a command under another name, and some fields are no setting a remote
                 command should have.

Without an option a report: it exits 0 whatever it counts. With --enforce the counts are numbers that only go down
(ADR 0037), kept in tools/scpi-coverage.txt, one `<count> <key>` per line as in tools/ratchet.txt: a count above its
number fails (add the doc row or the query, or say in the pull request why the number is raised by hand), a count
below it fails too, so the file never allows more than the tree has: run --lower, which writes the counts that went
down and never one that went up. The layer `ratchet` of tools/check.sh runs --enforce. Python's standard library;
reads files, touches no scope.
"""
import argparse
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCPI_RS = "ui/src/scpi.rs"
SCPI_DIR = "ui/src/scpi"  # the rest of the interpreter, one file per subsystem
SCPI_MD = "docs/scpi.md"
SCPI_TXT = "ui/tests/scpi.txt"
STORE_RS = "ui/src/store.rs"
SETTINGS_DIR = "ui/src/settings"  # the settings tables: rows with a key, a command or none, and the fields they reach
NUMBERS = "tools/scpi-coverage.txt"  # the counts that may only go down, `<count> <key>` per line

# The expressions scpi.rs passes to kw() that hold a node of the header. Not counted: `a`, `arg`, `s`, `item`, `first`,
# `z`, which hold arguments (`kw(first, "MAXimum")` reads the first argument of :MEASure:STATistic:ITEM?).
NODE = r"n0|n1|k0|k|n|x|last|node|leaf|which|nodes\[\d+\]|n\[\d+\]|suffix\((?:n|n0)\)\.0"
KW = re.compile(r"\bkw\(\s*(?:" + NODE + r")\s*,\s*\"([A-Za-z0-9]+)\"\s*\)")
FIELD = re.compile(r"\bm\.([a-z_][a-z0-9_]*)\b(?!\s*\()")
HANDS_M = re.compile(r"\b([a-z_][a-z0-9_]*)\(\s*m\s*,")
SCPI_LINE = re.compile(r"^scpi\s+\S+\s+(.+)$")
ROW = re.compile(r"^\|\s*`([^`]+)`")
SETTING = re.compile(r"\bSetting\s*\{")
SETTING_CMD = re.compile(r'Scpi::Cmd\("([A-Za-z0-9:]+)"\)')


def read(root, rel):
    with open(os.path.join(root, rel), encoding="utf-8", errors="replace") as f:
        return f.read()


def read_interpreter(root):
    """ui/src/scpi.rs and every .rs file below ui/src/scpi/, in the order of their paths, as one text."""
    text = read(root, SCPI_RS)
    for top, dirs, files in sorted(os.walk(os.path.join(root, SCPI_DIR))):
        dirs.sort()
        for name in sorted(f for f in files if f.endswith(".rs")):
            text += "\n" + read(root, os.path.relpath(os.path.join(top, name), root))
    return text


def named(word, text):
    """`[:*]Word\\b` ignoring case, also with a number after it (`:CHANnel1`, `:MATH2`); also after `{` or `|`, where
    scpi.md lists choices (`:CURSor:XY:{AX|BX}`)."""
    return re.search(r"[:*{|]" + re.escape(word) + r"\d*\b", text, re.IGNORECASE) is not None


def command_words(scpi_rs):
    """The words of kw(<node>, "Word") in scpi.rs, once each (ignoring case), in the order of the file."""
    seen = {}
    for w in KW.findall(scpi_rs):
        seen.setdefault(w.upper(), w)
    return list(seen.values())


def word_pattern(word):
    """A keyword as the interpreter takes it: the long form or the short form (its upper-case letters and digits)."""
    short = "".join(c for c in word if c.isupper() or c.isdigit())
    forms = {word, short} if short else {word}
    return "(?:" + "|".join(re.escape(f) for f in sorted(forms, key=len, reverse=True)) + ")"


def query_pattern(doc):
    """A regular expression for the queries a documented header stands for: `<n>` a number, `[:MAIN]` optional,
    `{A|B}` one of them, `[?]` a `?`."""
    out = []
    for m in re.finditer(r"<[^>]*>|\[:[^\]]+\]|\{[^}]*\}|\[\?\]|[A-Za-z][A-Za-z0-9]*|.", doc):
        t = m.group(0)
        if t.startswith("<"):
            out.append(r"\d+")
        elif t.startswith("[:"):
            out.append("(?::" + query_pattern(t[2:-1]) + ")?")
        elif t.startswith("{"):
            out.append("(?:" + "|".join(query_pattern(a) for a in t[1:-1].split("|")) + ")")
        elif t == "[?]":
            out.append(r"\?")
        elif t[0].isalpha():
            out.append(word_pattern(t))
        else:
            out.append(re.escape(t))
    return "".join(out)


def documented_queries(scpi_md):
    """The first command of each table row of scpi.md, without arguments, when it is a query."""
    out = []
    for line in scpi_md.split("\n"):
        m = ROW.match(line)
        if not m:
            continue
        cmd = m.group(1).split(" ")[0]
        if cmd.endswith(("?", "[?]")) and cmd not in out:
            out.append(cmd)
    return out


def commands(message):
    """The headers of a message: every command of it, split at `;` outside quotes, its first word."""
    parts, cur, quote = [], "", None
    for ch in message:
        if quote:
            quote = None if ch == quote else quote
        elif ch in "\"'":
            quote = ch
        elif ch == ";":
            parts.append(cur)
            cur = ""
            continue
        cur += ch
    parts.append(cur)
    return [p.split()[0] for p in parts if p.strip()]


def asked(scpi_txt):
    """Every command header the `scpi FILE MESSAGE` lines of scpi.txt send, all of each `;` sequence."""
    return [c for m in map(SCPI_LINE.match, scpi_txt.split("\n")) if m for c in commands(m.group(1))]


def fn_body(src, name):
    """The text of a top-level `fn name` of a Rust file, to its closing brace at the start of a line."""
    m = re.search(r"^(?:pub(?:\([a-z]+\))? )?fn " + re.escape(name) + r"\b.*?^\}", src, re.MULTILINE | re.DOTALL)
    return m.group(0) if m else ""


def setup_fields(store_rs):
    """The fields of the model setup_text writes: `m.<field>` in it and in the functions of store.rs it hands `m`
    to. A name followed by `(` is a method, not a field."""
    body = fn_body(store_rs, "setup_text")
    text = body + "".join(fn_body(store_rs, f) for f in sorted(set(HANDS_M.findall(body))))
    seen = []
    for f in FIELD.findall(text):
        if f not in seen:
            seen.append(f)
    return seen


def settings_rows(root):
    """The rows of the settings tables (the .rs files of ui/src/settings/ but its tests), in the order of the files:
    for each its command (None without one) and the fields of the model it reaches (`m.<field>`)."""
    folder = os.path.join(root, SETTINGS_DIR)
    names = os.listdir(folder) if os.path.isdir(folder) else []
    names = sorted(f for f in names if f.endswith(".rs") and f != "tests.rs")
    rows = []
    for name in names:
        for part in SETTING.split(read(root, SETTINGS_DIR + "/" + name))[1:]:
            cmd = SETTING_CMD.search(part)
            rows.append((cmd.group(1) if cmd else None, list(dict.fromkeys(FIELD.findall(part)))))
    return rows


def counts(root):
    """The four gaps as (key, what, missing, total); missing is a list of names. The keys name the gaps in the
    summary and in tools/scpi-coverage.txt."""
    scpi_rs, scpi_md, scpi_txt = read_interpreter(root), read(root, SCPI_MD), read(root, SCPI_TXT)
    rows = settings_rows(root)
    words = command_words(scpi_rs)
    for cmd, _ in rows:
        words += [w for w in (cmd or "").split(":") if w and w.upper() not in (x.upper() for x in words)]
    queries = documented_queries(scpi_md)
    cmds = asked(scpi_txt)
    fields = setup_fields(read(root, STORE_RS))
    fields += [f for f in dict.fromkeys(f for _, fs in rows for f in fs) if f not in fields]
    reached = {f for cmd, fs in rows if cmd for f in fs}
    unasked = []
    for q in queries:
        pat = re.compile(query_pattern(q), re.IGNORECASE)
        if not any(pat.fullmatch(c) for c in cmds):
            unasked.append(q)
    return [
        ("words-docs", "command words of %s not in %s" % (SCPI_RS, SCPI_MD),
         [w for w in words if not named(w, scpi_md)], len(words)),
        ("words-tests", "command words of %s not in %s" % (SCPI_RS, SCPI_TXT),
         [w for w in words if not named(w, scpi_txt)], len(words)),
        ("queries-tests", "queries of %s not asked in %s" % (SCPI_MD, SCPI_TXT), unasked, len(queries)),
        ("fields-scpi", "setup fields of %s not named in %s (an upper bound)" % (STORE_RS, SCPI_RS),
         [f for f in fields if f not in reached and not re.search(r"\." + f + r"\b", scpi_rs)], len(fields)),
    ]


def read_numbers(root):
    """tools/scpi-coverage.txt as {key: number}; empty when there is none. A line that is not `<count> <key>` is an
    error."""
    path = os.path.join(root, NUMBERS)
    if not os.path.exists(path):
        return {}
    out = {}
    for n, line in enumerate(read(root, NUMBERS).split("\n"), 1):
        if not line.strip():
            continue
        parts = line.split()
        if len(parts) != 2 or not parts[0].isdigit():
            raise ValueError("%s:%d: not `<count> <key>`: %r" % (NUMBERS, n, line))
        out[parts[1]] = int(parts[0])
    return out


def enforce(root, result, lower):
    """Compares the count of each key with its number in tools/scpi-coverage.txt and prints a line for each; with
    `lower` writes the counts that went down. Returns 0 when every count is its number (after --lower: when none went
    up), else 1."""
    kept = read_numbers(root)
    now = {key: len(missing) for key, _, missing, _ in result}
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
            print("down: %s %d -> %d (run tools/scpi-coverage.py --lower)" % (key, was, count))
            bad = 1
            out[key] = was
        else:
            out[key] = count
    if lower:
        with open(os.path.join(root, NUMBERS), "w", encoding="utf-8") as f:
            f.write("".join("%d %s\n" % (out[k], k) for k in sorted(out)))
    if not bad:
        print("scpi-coverage: %d numbers as recorded in %s" % (len(out), NUMBERS))
    return bad


def main(argv):
    ap = argparse.ArgumentParser(description="what a remote command or a setting lacks; a report, exit 0, unless "
                                             "--enforce or --lower")
    ap.add_argument("root", nargs="?", default=ROOT, help="top of the tree (default: this tree)")
    ap.add_argument("--list", action="store_true", help="each count with its names")
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--enforce", action="store_true", help="exit 1 when a count differs from %s" % NUMBERS)
    mode.add_argument("--lower", action="store_true", help="write the counts that went down into %s" % NUMBERS)
    args = ap.parse_args(argv)
    result = counts(args.root)
    if args.list:
        for key, what, missing, total in result:
            print("%s: %d of %d %s" % (key, len(missing), total, what))
            for name in missing:
                print("  " + name)
    else:
        print("scpi-coverage: " + ", ".join("%s %d/%d" % (k, len(m), t) for k, _, m, t in result) +
              " missing (--list names them)")
    if args.enforce or args.lower:
        return enforce(args.root, result, args.lower)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
