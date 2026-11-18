#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""layers.py: the places where code crosses a layer, of the crates (ADR 0001) and inside oos-ui (ADR 0049); issue 120
goal S3.

    tools/layers.py [ROOT]            two lines: the three counts of the crates, the counts inside oos-ui
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

Inside oos-ui every module has a layer and every layer a list of the layers it may name: the table UI_LAYERS below
(ADR 0049). A file's module is its place below ui/src (`ui/src/store/png.rs` is `store::png`), or what a `#[path =
"file.rs"] mod name;` says of it, below the modules written out around that declaration; its layer is the one of the
longest row that begins its name, so the row `ui` places every file below ui/src/ui/. A path counts where it is
written from `crate`, `super` or `self`, also as a leaf of a `use`, and names a layer the file's layer may not name;
a name a `use` brought in counts at the `use`, not where it is used after it. `super` and `self` are read from the
module the path stands in, also one written out in the file (`mod imp { ... }`); the brackets of a visibility
(`pub(crate)`, `pub(in crate::ui)`) name nothing. One count per layer, for the paths that leave what it may name:
  ui-base-up         base (helpers and services without the scope's state) naming any other layer
  ui-model-up        the model naming more than base
  ui-settings-up     the settings table naming more than base and the model
  ui-board-up        the board interface naming more than base and the model
  ui-signal-up       acquisition and analysis naming more than base, the model and the board
  ui-store-up        the store (setup and waveform files) naming more than base, the model, the settings and signal
  ui-screen-up       the screen naming an interpreter, a transport or the wiring
  ui-command-up      an interpreter (SCPI, the control commands) naming a transport or the wiring
  ui-transport-skip  a transport naming more than base and the interpreters: it hands programs over (ADR 0022)
  ui-unplaced        files of oos-ui whose module has no row in UI_LAYERS: a new module is placed there first
`main` (the crate root with `input` and `gpu`) is the wiring and may name everything.

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
UI = "ui/src/"  # the sources of oos-ui
# the crates of the workspace ui/ that a layer became (ADR 0064): main.rs brings their modules into the crate root of
# oos-ui, so oos-ui names them from there (`crate::sysconf`) and they keep their rows in UI_LAYERS
UI_CRATES = ("ui/system/src/", "ui/remote/src/")
OWN_FOLDER = ("main", "lib", "mod")  # files that are the module of their folder: main.rs, lib.rs, mod.rs
# The layers inside oos-ui, lowest first (ADR 0049): (layer, the key of its count, the other layers it may name, its
# modules). A module is named from the crate root (`board`, or `ui::la` for one file of a folder); the longest row
# that begins a file's module places it. The last row is the wiring: the crate root (main.rs) belongs to it without
# being named, and it may name everything. The system services of base are the crate oos-system (ui/system/,
# ADR 0064): main.rs brings them into the crate root, so oos-ui names them `crate::sysconf` and their rows stay here.
UI_LAYERS = (
    ("base", "ui-base-up", "",
     "base64 clock drm ethlink font gfx hid hostname json layout mdns net net6 netacl placement remote sntp ssh sysconf system"
     " usbtmc vpu"),
    ("model", "ui-model-up", "base", "decode logic math model words zone"),
    ("settings", "ui-settings-up", "base model", "settings"),
    ("board", "ui-board-up", "base model", "board"),
    ("signal", "ui-signal-up", "base model board",
     "compress deepmeas drawpipe fastdraw framestats measures protocols sparse trig trignear wave"),
    ("store", "ui-store-up", "base model settings signal", "store"),
    ("screen", "ui-screen-up", "base model settings board signal store", "ui"),
    ("command", "ui-command-up", "base model settings board signal store screen", "cmd scpi"),
    ("transport", "ui-transport-skip", "base command", "announce hislip http vxi11"),
    ("main", None, None, "gpu input"),
)
WIRING = UI_LAYERS[-1][0]
UNPLACED = "ui-unplaced"  # the count of the files whose module has no row
PLACE = {module: layer for layer, _, _, modules in UI_LAYERS for module in modules.split()}
MAY = {layer: (key, set(may.split())) for layer, key, may, _ in UI_LAYERS if key}
ORDER = [layer for layer, _, _, _ in UI_LAYERS]


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


def ui_modules(toks_of):
    """{file: its module as names from the crate root} of the files of oos-ui (`toks_of`: {file: its tokens}). A
    module is the file's place below ui/src: ui/src/a/b.rs is [a, b], ui/src/a/mod.rs is [a], ui/src/main.rs is [].
    A file another one declares with `#[path = "b.rs"] mod c;` is [.., c] below that file's module instead, with the
    modules written out around the declaration between them (`mod o { #[path = "b.rs"] mod c; }` is [.., o, c]); the
    file is looked for as Rust does, from the folder of the declaring file, inside a module written out from the
    folder of that module."""
    said = {}  # file: (the file that declares it, the names it is declared with below that file's module)
    for rel, toks in toks_of.items():
        inner, toks = inner_modules(toks), without_visibility(toks)
        folder, stem = os.path.dirname(rel), os.path.basename(rel)[:-len(".rs")]
        for i in range(len(toks) - 7):
            if [t[2] for t in toks[i:i + 4]] == ["#", "[", "path", "="] and toks[i + 5][2] == "]":
                j = i + 7 if toks[i + 6][2] == "pub" else i + 6
                if toks[j][2] == "mod" and j + 2 < len(toks) and toks[j + 2][2] == ";":
                    around = innermost(inner, toks[j][0])
                    start = [folder] + ([] if stem in OWN_FOLDER or not around else [stem]) + around
                    file = os.path.normpath(os.path.join(*start, toks[i + 4][2].strip('"')))
                    said[file.replace(os.sep, "/")] = (rel, around + [toks[j + 1][2]])

    def module(rel, seen=()):
        if rel in said and rel not in seen:
            return module(said[rel][0], seen + (rel,)) + said[rel][1]
        names = rel[len(UI):-len(".rs")].split("/")
        return names[:-1] if names[-1] in OWN_FOLDER else names

    return {rel: module(rel) for rel in toks_of}


def crate_modules(rels):
    """The top modules of the crates of UI_CRATES among the paths `rels`: their files and folders below `src/`."""
    out = set()
    for rel in rels:
        for src in UI_CRATES:
            if rel.startswith(src):
                top = rel[len(src):].split("/")[0]
                out.add(top[:-len(".rs")] if top.endswith(".rs") else top)
    return out - {"lib", "testkit"}


def ui_layer(names, tops):
    """The layer of what `names` (from the crate root) say: the one of the longest row of UI_LAYERS that begins them;
    the wiring for the crate root and for a name of its own that is no module (`tops`: the modules declared in it);
    None for a module without a row."""
    for k in range(len(names), 0, -1):
        layer = PLACE.get("::".join(names[:k]))
        if layer:
            return layer
    return None if names and names[0] in tops else WIRING


def inner_modules(toks):
    """[(first line, last line, [name, ...])] of the modules written out in a file (`mod a { mod b { } }` gives [a]
    and [a, b]), by the lines of their braces."""
    out, inside, depth = [], [], 0
    for i, (line, kind, piece) in enumerate(toks):
        if kind != "punct":
            continue
        if piece == "{":
            if i >= 2 and toks[i - 2][2] == "mod" and toks[i - 1][1] == "ident":
                inside.append((depth, line, toks[i - 1][2]))
            depth += 1
        elif piece == "}":
            depth -= 1
            if inside and inside[-1][0] == depth:
                _, first, name = inside.pop()
                out.append((first, line, [n for _, _, n in inside] + [name]))
    return out


def innermost(inner, line):
    """The names of the innermost module of `inner` (see inner_modules) that holds `line`; [] outside all of them."""
    return max((names for first, last, names in inner if first <= line <= last), key=len, default=[])


def without_visibility(toks):
    """The tokens without the brackets of a visibility (`pub(crate)`, `pub(super)`, `pub(in crate::ui)`): they say who
    may name an item and name no module themselves. A field that is a tuple (`pub (crate::a::A, B)`) stays."""
    out, i, n = [], 0, len(toks)
    while i < n:
        out.append(toks[i])
        i += 1
        if out[-1][2] == "pub" and i + 2 < n and toks[i][2] == "(" and (
                toks[i + 1][2] == "in" or toks[i + 1][2] in ("crate", "self", "super") and toks[i + 2][2] == ")"):
            while i < n and toks[i][2] != ")":
                i += 1
            i += 1
    return out


def ui_target(module, names):
    """The names from the crate root of a path written from `crate`, `self` or `super` in the module `module`, a `*`
    at its end left out; None for every other path (a name of a `use`, another crate, a local name) and for one that
    names the crate itself."""
    if names[0] == "crate":
        out = names[1:]
    elif names[0] == "self":
        out = module + names[1:]
    elif names[0] == "super":
        up = next((k for k, name in enumerate(names) if name != "super"), len(names))
        if up > len(module):
            return None
        out = module[:len(module) - up] + names[up:]
    else:
        return None
    return (out[:-1] if out[-1:] == ["*"] else out) or None


def ui_crossings(module, toks, tops):
    """[(key, line, what it names)] of the paths in one file of oos-ui (its module, its tokens) that leave what its
    layer may name, and the file itself under UNPLACED when its module has no row."""
    layer = ui_layer(module, tops)
    if layer is None:
        return [(UNPLACED, 1, "no row for the module %s in UI_LAYERS of tools/layers.py" % "::".join(module))]
    if layer not in MAY:
        return []
    key, may = MAY[layer]
    inner = inner_modules(toks)
    found = []
    for line, names in paths(without_visibility(toks)):
        target = ui_target(module + innermost(inner, line), names)
        named = ui_layer(target, tops) if target else None
        if named and named != layer and named not in may:
            found.append((key, line, "%s names %s: %s" % (layer, named, "::".join(names))))
    return found


def crossings(rel, toks):
    """[(key, line, what it names)] of the crossings of the crates in one file's tokens (tests already blanked)."""
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
    """The counts as (key, what, items), the three of the crates first, then those inside oos-ui (their keys begin
    `ui-`); items are the lines --list prints. The keys name the counts in the summary and in tools/layers.txt."""
    texts = dup.read_tree(root)
    toks_of = {rel: list(dup.tokens(texts[rel])) for rel in sorted(texts)}
    modules = ui_modules({rel: toks for rel, toks in toks_of.items() if rel.startswith(UI)})
    tops = {module[0] for module in modules.values() if module}
    found = {key: [] for key in ["board", "hal-up", "dsp-hw", UNPLACED] + [key for key, _ in MAY.values()]}
    for rel, toks in toks_of.items():
        lines = texts[rel].split("\n")
        here = crossings(rel, toks) + (ui_crossings(modules[rel], toks, tops) if rel in modules else [])
        for key, line, what in sorted(set(here), key=lambda c: (c[1], c[2])):
            found[key].append("%s:%d %s  (%s)" % (rel, line, lines[line - 1].strip(), what))
    for rel, (key, allowed) in DEPENDS.items():
        found[key] += ["%s:%d %s  (dependency %s)" % (rel, line, text, name)
                       for line, name, text in dependencies(root, rel) if name not in allowed]
    return [
        ("board", "paths to the DHO board's code past board::Board", found["board"]),
        ("hal-up", "names of the crates above hal in hal", found["hal-up"]),
        ("dsp-hw", "names of hardware in dsp", found["dsp-hw"]),
    ] + [
        (key, "paths of oos-ui where %s names %s" % (
            layer, "more than " + ", ".join(sorted(may, key=ORDER.index)) if may else "another layer"), found[key])
        for layer, (key, may) in MAY.items()
    ] + [(UNPLACED, "files of oos-ui whose module has no row in UI_LAYERS", found[UNPLACED])]


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
        for title, inside in (("layers", False), ("layers of oos-ui", True)):
            print("%s: %s (--list names them)" % (title, ", ".join(
                "%s %d" % (k, len(items)) for k, _, items in result if k.startswith("ui-") == inside)))
    if args.enforce or args.lower:
        return numbers.enforce(args.root, NUMBERS, {k: len(items) for k, _, items in result}, args.lower)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
