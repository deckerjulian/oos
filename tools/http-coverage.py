#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""http-coverage.py: every HTTP route and MCP tool of the web remote has its entry and mark in docs/http.md (ADR 0055).

    tools/http-coverage.py [ROOT]          one line when all is well; else each finding, and exit 1
    tools/http-coverage.py --list [ROOT]   also every route and tool with its mark

ROOT is the top of a tree (default: this tree). Read from it as it is:
  the routes     the arms of `match route { ... }` in `fn answer` of ui/src/http.rs: each string pattern is a path.
                 A guard `if post` makes the arm a POST route, `if !post` a GET route (every method but POST is taken
                 as GET); an arm without a guard answers every method, or GET when an earlier arm of the same path
                 took POST. Arms of `_` (the foreign check, not found) are no route. Any other guard has to be in
                 GUARDS below with its reason, so that a new kind of arm is looked at rather than passed over;
  the tools      the names `Some("name") =>` that the arm `"tools/call"` of `fn mcp` in ui/src/http.rs answers, and
                 the names (`"name", J::s("...")`) of the arm `"tools/list"` before it;
  the entries    the headings of docs/http.md of the form `### \\`METHOD /path\\`` and `### MCP tool \\`name\\``, each
                 with one line `- Mark: **stable**`, `**experimental**` or `**internal**` before the next heading.

A finding: a route or tool without an entry; an entry without a mark, with more than one, or with another word; an
entry whose route the code does not answer with that method (an arm without a guard takes either), or whose tool it
does not have; a tool that tools/list and tools/call do not name alike; an unknown guard. With none missing this is
a plain check, not a number that only goes down: the layer `ratchet` of tools/check.sh runs it. Python's standard
library; reads files, touches no scope.
"""
import argparse
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HTTP_RS = "ui/src/http.rs"
HTTP_MD = "docs/http.md"
MARKS = ("stable", "experimental", "internal")

# The guards of an arm that is no route of its own but an answer of routes listed elsewhere, each with its reason
# (whitespace collapsed). A guard not listed here and not `post` or `!post` is a finding.
# a guard this check passes over, with the paths of its arm: an arm with that guard and other paths is a finding, so a
# route added to such an arm is not passed over with it
GUARDS = {
    '(post || route == "/mcp") && !peer_ok': (
        {"/update", "/mcp"},
        "the 403 of POST /update and /mcp outside net.allow (docs/http.md, Common answers)",
    ),
}

ENTRY = re.compile(r"^### `(GET|POST|PUT|DELETE|HEAD|PATCH|OPTIONS) (/[^`\s]*)`\s*$")
TOOL_ENTRY = re.compile(r"^### MCP tool `([^`]+)`\s*$")
MARK = re.compile(r"^- Mark: \*\*([a-z]+)\*\*")
TOKEN = re.compile(r"""
    (?P<comment>//[^\n]*|/\*.*?\*/)
  | (?P<raw>b?r(?P<hashes>\#*)".*?"(?P=hashes))
  | (?P<str>b?"(?:\\.|[^"\\])*")
  | (?P<char>b?'(?:\\.|[^'\\])')
  | (?P<arrow>=>)
  | (?P<punct>[{}()\[\],|!])
  | (?P<word>[A-Za-z_][A-Za-z0-9_]*|[0-9]+|&&|\|\||==|!=|\S)
""", re.VERBOSE | re.DOTALL)


def read(root, rel):
    with open(os.path.join(root, rel), encoding="utf-8", errors="replace") as f:
        return f.read()


def tokens(src, start=0):
    """The tokens of Rust source from `start`, as (kind, text, begin, end); comments are left out, strings and
    characters are one token each, so a brace or an apostrophe inside them counts for nothing."""
    out, pos = [], start
    while pos < len(src):
        if src[pos].isspace():
            pos += 1
            continue
        m = TOKEN.match(src, pos)
        kind = m.lastgroup if m.lastgroup != "hashes" else "raw"
        if kind != "comment":
            out.append((kind, m.group(0), m.start(), m.end()))
        pos = m.end()
    return out


def block(src, open_at):
    """The tokens inside the brace that opens at `open_at`, to its closing brace."""
    toks, depth = tokens(src, open_at), 0
    for i, t in enumerate(toks):
        if t[1] == "{":
            depth += 1
        elif t[1] == "}":
            depth -= 1
            if depth == 0:
                return toks[1:i]
    return toks[1:]


def fn_tokens(src, name):
    """The tokens of the body of `fn name` (a method or a free function), empty without one."""
    m = re.search(r"\bfn " + re.escape(name) + r"\b", src)
    if not m:
        return []
    return block(src, src.index("{", m.end()))


def match_arms(src, toks, scrutinee):
    """The arms of the first `match <scrutinee> {` among `toks`: (patterns, guard) per arm, where patterns are the
    string literals of the pattern (None for `_`) and guard the text after `if` with its whitespace collapsed."""
    for i in range(len(toks) - 2):
        if toks[i][1] == "match" and toks[i + 1][1] == scrutinee and toks[i + 2][1] == "{":
            break
    else:
        return []
    body = block(src, toks[i + 2][2])
    arms, i = [], 0
    while i < len(body):
        j = i
        while j < len(body) and body[j][0] != "arrow":
            j += 1
        if j == len(body):
            break
        pat = body[i:j]
        cut = next((k for k, t in enumerate(pat) if t[1] == "if"), len(pat))
        guard = " ".join(src[pat[cut + 1][2]:pat[-1][3]].split()) if cut < len(pat) else ""
        strings = [t[1][1:-1] for t in pat[:cut] if t[0] == "str"]
        arms.append((None if pat[0][1] == "_" else strings, guard))
        # the arm's expression: a block, or up to the comma at this level
        k, depth = j + 1, 0
        if k < len(body) and body[k][1] == "{":
            depth = 0
            while k < len(body):
                depth += body[k][1] in "{(["
                depth -= body[k][1] in "})]" and body[k][0] == "punct"
                k += 1
                if depth == 0:
                    break
            if k < len(body) and body[k][1] == ",":
                k += 1
        else:
            while k < len(body) and not (depth == 0 and body[k][1] == ","):
                if body[k][0] == "punct" and body[k][1] in "{([":
                    depth += 1
                elif body[k][0] == "punct" and body[k][1] in "})]":
                    depth -= 1
                k += 1
            k += 1
        i = k
    return arms


def routes(src):
    """The routes of Http::answer: {path: set of methods it is answered for, as entries name them}, the arms as
    (path, methods) in their order, each of which needs an entry of one of its methods, and the findings (unknown
    guards). An arm without a guard is {"GET", "POST"}, or {"GET"} after an arm of its path took POST."""
    out, arms, findings, taken = {}, [], [], {}
    for patterns, guard in match_arms(src, fn_tokens(src, "answer"), "route"):
        if patterns is None:
            continue
        if guard in GUARDS:
            if set(patterns) != GUARDS[guard][0]:
                findings.append("%s: arm %s has the guard `if %s`, which is known for %s only (GUARDS)" %
                                (HTTP_RS, " | ".join(patterns), guard, " | ".join(sorted(GUARDS[guard][0]))))
            continue
        if guard == "post":
            methods = {"POST"}
        elif guard == "!post":
            methods = {"GET"}
        elif guard == "":
            methods = None
        else:
            findings.append("%s: arm %s has a guard this check does not know: `if %s` (GUARDS)" %
                            (HTTP_RS, " | ".join(patterns), guard))
            continue
        for p in patterns:
            m = methods if methods is not None else {"GET", "POST"} - taken.get(p, set())
            out.setdefault(p, set()).update(m)
            taken.setdefault(p, set()).update(m)
            arms.append((p, m))
    return out, arms, findings


def tools(src):
    """The MCP tools: (the names tools/call answers, the names tools/list announces), in the order of the file."""
    body = fn_tokens(src, "mcp")
    text = src[body[0][2]:body[-1][3]] if body else ""
    # the arm of tools/list runs to the arm of tools/call, which runs to the end (serverInfo has a "name" too)
    at_list, at_call = text.find('"tools/list" =>'), text.find('"tools/call" =>')
    listing = text[at_list:at_call] if 0 <= at_list < at_call else ""
    called = list(dict.fromkeys(re.findall(r'\bSome\("([^"]+)"\)\s*=>', text[max(at_call, 0):])))
    listed = list(dict.fromkeys(re.findall(r'\(\s*"name"\s*,\s*J::s\("([^"]+)"\)\s*\)', listing)))
    return called, listed


def entries(md):
    """The entries of docs/http.md: [(kind, key, marks, line)], kind "route" with key (METHOD, path) or "tool" with
    key name; marks are the words of the `- Mark:` lines before the next heading."""
    out, fenced = [], False
    for n, line in enumerate(md.split("\n"), 1):
        if line.startswith("```"):
            fenced = not fenced
        if fenced:
            continue
        if line.startswith("#"):
            m, t = ENTRY.match(line), TOOL_ENTRY.match(line)
            if m:
                out.append(["route", (m.group(1), m.group(2)), [], n])
            elif t:
                out.append(["tool", t.group(1), [], n])
            else:
                out.append(None)
            continue
        mark = MARK.match(line)
        if mark and out and out[-1] is not None:
            out[-1][2].append(mark.group(1))
    return [tuple(e) for e in out if e is not None]


def check(root):
    """(the findings, the routes and tools with their marks)."""
    src, md = read(root, HTTP_RS), read(root, HTTP_MD)
    have, arms, findings = routes(src)
    called, listed = tools(src)
    if not have:
        findings.append("%s: no route found in `match route` of fn answer" % HTTP_RS)
    if not called:
        findings.append("%s: no MCP tool found in fn mcp" % HTTP_RS)
    for name in called:
        if name not in listed:
            findings.append("%s: MCP tool `%s` is answered by tools/call but not in tools/list" % (HTTP_RS, name))
    for name in listed:
        if name not in called:
            findings.append("%s: MCP tool `%s` is in tools/list but tools/call does not answer it" % (HTTP_RS, name))
    found, seen = [], set()
    for kind, key, marks, line in entries(md):
        where = "%s:%d" % (HTTP_MD, line)
        name = "%s %s" % key if kind == "route" else "MCP tool `%s`" % key
        if (kind, key) in seen:
            findings.append("%s: %s has a second entry" % (where, name))
        seen.add((kind, key))
        if kind == "route" and key[1] not in have:
            findings.append("%s: %s: no such route in fn answer of %s" % (where, name, HTTP_RS))
        elif kind == "route" and key[0] not in have[key[1]]:
            findings.append("%s: %s: fn answer of %s takes only %s on %s" %
                            (where, name, HTTP_RS, " and ".join(sorted(have[key[1]])), key[1]))
        if kind == "tool" and key not in called + listed:
            findings.append("%s: %s: no such tool in fn mcp of %s" % (where, name, HTTP_RS))
        if len(marks) != 1 or marks[0] not in MARKS:
            findings.append("%s: %s needs one line `- Mark: **stable**` (or experimental, internal), has %s" %
                            (where, name, ", ".join(marks) or "none"))
        found.append((name, ", ".join(marks) or "-"))
    documented = {key for kind, key in seen if kind == "route"}
    for path, methods in arms:
        if not any(p == path and m in methods for m, p in documented):
            findings.append("%s: route %s %s has no entry `### \\`%s %s\\`` in %s" %
                            (HTTP_RS, "/".join(sorted(methods)), path, min(methods), path, HTTP_MD))
    for name in called:
        if ("tool", name) not in seen:
            findings.append("%s: MCP tool `%s` has no entry `### MCP tool \\`%s\\`` in %s" %
                            (HTTP_RS, name, name, HTTP_MD))
    return findings, found


def main(argv):
    ap = argparse.ArgumentParser(description="every HTTP route and MCP tool has its entry and mark in " + HTTP_MD)
    ap.add_argument("root", nargs="?", default=ROOT, help="top of the tree (default: this tree)")
    ap.add_argument("--list", action="store_true", help="every entry with its mark")
    args = ap.parse_args(argv)
    findings, found = check(args.root)
    if args.list:
        for name, marks in found:
            print("%-28s %s" % (name, marks))
    for f in findings:
        print(f)
    if findings:
        print("http-coverage: %d findings (ADR 0055)" % len(findings), file=sys.stderr)
        return 1
    print("http-coverage: %d entries of %s, each with its route or tool and its mark" % (len(found), HTTP_MD))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
