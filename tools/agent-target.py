#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""agent-target.py: the next task of the cleaner (docs/adr/process/0034), chosen by counting, not by the agent.

    tools/agent-target.py        the title of the task on the first line, then the text of its issue

1. A lint. `cargo clippy --all-targets` runs in every crate. Findings in the paths that are only proven on a scope
   (hal/src/dho, hal/src/update, ui/src/board, fpgad, which the status `unit` of a pull request holds for the
   label unit-tested) are left for last: a task there needs the maintainer at a unit. Among the rest, the crate
   with the fewest findings that has any is taken, so one crate gets clean before the next is started, and in it
   the lint with the most findings. A lint with more than 80 findings is given file by file, the files with the
   most first, up to 80.
2. With no finding left: the largest Rust file of more than 2000 lines, one part of which is to be moved out; a
   file in the paths above only when no other is over the limit.
3. With nothing to do: prints nothing and exits 3.

Python's standard library and cargo with clippy; builds the crates, touches no scope and no network.
"""
import collections
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CRATES = ("hal", "dsp", "ui", "fpgad")
MOST = 80
LARGE = 2000
# what is only proven on a scope (the paths of tools/pr-gate.sh): tasks there wait for the maintainer at a unit
UNIT = ("hal/src/dho/", "hal/src/update/", "ui/src/board/", "fpgad/", "boot/", "kernel/", "rootfs/")


def needs_unit(path):
    return path.startswith(UNIT)


def findings(crate):
    """{lint: {file: count}} of one crate; a finding in two targets of the crate counts once."""
    run = subprocess.run(("cargo", "clippy", "--locked", "--all-targets", "--message-format=json"),
                         cwd=os.path.join(ROOT, crate), capture_output=True, text=True)
    seen, out = set(), collections.defaultdict(collections.Counter)
    for line in run.stdout.splitlines():
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        message = entry.get("message") or {}
        code = (message.get("code") or {}).get("code")
        if entry.get("reason") != "compiler-message" or not code or message.get("level") not in ("warning", "error"):
            continue
        span = next((s for s in message.get("spans", []) if s.get("is_primary")), None)
        if not span:
            continue
        key = (code, span["file_name"], span["line_start"], span["column_start"])
        if key not in seen:
            seen.add(key)
            out[code][os.path.join(crate, span["file_name"])] += 1
    return out


def lint_task():
    per_crate = {c: findings(c) for c in CRATES}
    # the findings outside the paths that need a unit first; those inside only when nothing else is left
    host = {c: {lint: collections.Counter({f: n for f, n in files.items() if not needs_unit(f)})
                for lint, files in f.items()} for c, f in per_crate.items()}
    host = {c: {lint: files for lint, files in f.items() if files} for c, f in host.items()}
    with_any = {c: f for c, f in host.items() if f} or {c: f for c, f in per_crate.items() if f}
    if not with_any:
        return None
    crate = min(with_any, key=lambda c: (sum(sum(f.values()) for f in with_any[c].values()), c))
    lint, files = max(with_any[crate].items(), key=lambda kv: (sum(kv[1].values()), kv[0]))
    total = sum(files.values())
    chosen, n = [], 0
    for name, count in files.most_common():
        if chosen and n + count > MOST:
            break
        chosen.append((name, count))
        n += count
    rows = "\n".join(f"| `{name}` | {count} |" for name, count in chosen)
    if n < total:
        title = f"Lints: {lint} in {crate} ({n} of {total})"
        what = f"in the files below: {n} of the {total} findings in `{crate}/`"
        left = "in these files"
    else:
        title = f"Lints: {lint} in {crate} ({total})"
        what = f"in `{crate}/`: {total} in all"
        left = "any more"
    return (title, f"""**Task:** fix the findings of the lint `{lint}` {what}.

| File | Findings |
|---|---|
{rows}

**Done when:**

- `cargo clippy --all-targets` in `{crate}/` reports no `{lint}` {left};
- `tools/check.sh` passes;
- `tools/compare-base.sh` reports no difference.

**Behaviour:** unchanged.

**Not part of it:** every other lint, the layout of the code, and any other change. A place where the lint's
suggestion would change what the code does stays as it is and is listed in the pull request.""")


def file_task():
    listed = subprocess.run(("git", "-C", ROOT, "ls-files", "*.rs"), capture_output=True, text=True, check=True)
    names = listed.stdout.split()
    sizes = []
    for name in names:
        with open(os.path.join(ROOT, name), encoding="utf-8", errors="replace") as f:
            sizes.append((sum(1 for _ in f), name))
    large = [s for s in sizes if s[0] > LARGE]
    if not large:
        return None
    lines, name = max([s for s in large if not needs_unit(s[1])] or large)
    return (f"Refactor: one part out of {name} ({lines} lines)",
            f"""**Task:** `{name}` has {lines} lines. Move **one** part of it that stands on its own (a group of
functions around one thing, with their types and tests) into a module of its own. Move it as it is; change nothing
in it.

**Done when:**

- the file is shorter by what was moved, and `tools/ratchet.sh --lower` has written the new number, if the tree has
  that script;
- `tools/check.sh` passes;
- `tools/compare-base.sh` reports no difference.

**Behaviour:** unchanged.

**Not part of it:** a second part, new names, changed signatures, any change inside the moved code.""")


def main():
    task = lint_task() or file_task()
    if not task:
        return 3
    print(task[0])
    print(task[1])
    return 0


if __name__ == "__main__":
    sys.exit(main())
