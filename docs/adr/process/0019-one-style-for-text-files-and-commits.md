# ADR 0019: One style for files, documents and commits, written down and checked where a tool can

- Status: accepted
- Date: 2026-11-15
- Category: process

## Context

Most of the conventions of the tree existed before this record, but only in the files themselves: English text, lines
of 120 columns, a licence header in every source file, commit subjects of the form `Area: what changed`. Contributors
who came in through the forks, and their agents, had no place to read them and wrote their own: a test file without a
licence header, a document of 300-column lines, a subject `fix stuff`. The state on main at 84deb24 (2026-11-15):
12 of 272 Rust files and 1 of 56 shell and Python files without a licence line, no trailing spaces outside
`kernel/` and the firmware licence, all files LF (`.gitattributes`).

What a tool can check is checked; the rest is written here once, so a review can point to it.

## Decision

**Files** ([.editorconfig](../../../.editorconfig) sets this for editors, `tools/check.sh text` checks it):

- UTF-8, LF line ends, a newline at the end of the file, no spaces or tabs at the end of a line.
- Rust and Python indent with 4 spaces, YAML with 2. Shell and C keep the indentation the file has (several use tabs);
  a new shell script uses 4 spaces.
- Every source file of our own (`.rs`, `.sh`, `.py`, `.c`, `.h` outside `kernel/`) starts with the licence line
  `SPDX-License-Identifier: GPL-2.0-or-later`, after a `Copyright (C) <year> <name>` line of its author. A file
  taken from elsewhere (the XDMA driver, kernel patches, firmware licences) keeps its own header and format and is
  left out of the checks.
- File names are lower case with `-` (`layout-check.sh`, `protocol-modules.md`); Rust modules with `_`, as Rust wants.

**Code** (besides ADR 0016 and ADR 0017):

- Comments and names in English. A comment says why, or what a register value means; not what the next line does.
- Every statement about the hardware in a comment follows ADR 0015 like one in `docs/`.
- A test says in its name what it holds (`a_trigger_type_change_only_touches_the_trigger`), so a failing test reads
  as the rule that broke.

**Documents** (Markdown in `docs/`, `README.md`, the ADRs):

- English, plain words, short sentences. Units with a space and SI prefixes (`1.25 GHz`, `100 ns/div`, `4 MiB`).
  Menu paths as `Display > Bottom bar`. Code, files and commands in backticks.
- Prose lines wrap at 120 columns. Tables, links and code blocks may be longer. This is kept by review, not by CI,
  because some older documents do not follow it yet.
- A document is changed in the same pull request as the code it describes.

**Commits and pull requests:**

- Subject `Area: what changed, as a statement` in English, without a full stop, about 72 characters
  (`SCPI: a query is answered after the client closed its side`). The area is a part of the scope (`Measurements`,
  `Deep records`, `UI`, `Image`, `docs/adr`), not a file name. A release commit is the bare version (`0.1.13`).
- The body says why, and how it was tested (ADR 0015): on which unit, only on the host, or not.
- One topic per commit; formatting and lint fixes in commits of their own (ADR 0016, ADR 0017).
- Work from others keeps its author: merged forks keep their commits, a reworked commit names the original
  (`from PR #8 9da7396`). Commits written with an agent may say so in a `Co-Authored-By:` line.
- A change somebody using the scope would notice gets its entry in `CHANGELOG.md` under the next version, in the same
  style: what is new or fixed as seen on the scope, with the numbers that show it.

## Alternatives

- **A long style guide for everything:** nobody reads it at the start, and an agent follows it worse than a tool. The
  rules here are the ones a tool cannot check, plus the ones it does, so they are written in one place.
- **Enforce the 120 columns of Markdown in CI:** about 800 lines in older documents are longer, mostly protocol
  descriptions with long tables. Rewrapping them is noise; it is done when a document is changed anyway.
- **Conventional Commits (`feat:`, `fix:`):** a different form than the almost 600 commits already in the history, and
  the area says more about a scope than `feat` does.

## Consequences

- A review can answer "how should this look" with a link.
- `tools/check.sh text` runs in CI from now on. The 13 files that came without a header got the `SPDX` line only,
  in the commit of this record; their authors are in the history, and a `Copyright` line is theirs to add.
- `LICENSE` is the GPL text as published and is left as it is, trailing spaces included.
- The rules for commits are not checked by a tool. If they keep being missed, a commit-message check in CI is the next
  step, and a change of this record.

## References

[.editorconfig](../../../.editorconfig), [.gitattributes](../../../.gitattributes), `tools/check.sh`,
[CONTRIBUTING.md](../../../CONTRIBUTING.md), [CHANGELOG.md](../../../CHANGELOG.md), ADR 0015, ADR 0016, ADR 0017.
