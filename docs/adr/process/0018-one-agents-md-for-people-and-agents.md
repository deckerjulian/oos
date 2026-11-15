# ADR 0018: One `AGENTS.md` holds the project's instructions for coding agents; it points to the rules, it does not repeat them

- Status: accepted
- Date: 2026-11-15
- Category: process

## Context

A large part of the code in OOS, in main and in the forks, is written with coding agents (Claude Code, Codex, Copilot,
Cursor and others). An agent starts every session without memory. What it does not read at the start, it does not
know: that the scope's registers are mostly write-only, that some partitions must never be written, that a refactoring
has to keep every pixel. Without a file in the tree each contributor gives their agent their own instructions, and the
forks drift apart in exactly the things that should be the same everywhere. That was said plainly in the forum
(2026-11-15): no project-level agent instructions, no lint, no formatting rules.

Each tool looks for its own file name: `AGENTS.md` (read by most tools, the open convention), `CLAUDE.md` (Claude
Code), `.github/copilot-instructions.md`, `.cursor/rules`. Several files with the same content go out of step with
the first change.

## Decision

- [AGENTS.md](../../../AGENTS.md) at the root of the tree is the one file of instructions for agents. It is written
  for people too: a new contributor reads the same page.
- It is short, at most about 150 lines, and it **points** to the rules instead of repeating them: the order to read
  in (this index, [architecture.md](../../architecture.md), [CONTRIBUTING.md](../../../CONTRIBUTING.md)), the rules
  that must never be broken in one line each with their ADR, the commands to build and check, and how to report work.
- Where `AGENTS.md` and an ADR differ, the ADR holds, and `AGENTS.md` is corrected in the same pull request.
- `CLAUDE.md` contains only the line `@AGENTS.md` (Claude Code reads the other file through it). Files for other tools
  are added the same way when a contributor needs them: one line that points to `AGENTS.md`, nothing of their own.
- Personal settings stay out of the tree: an agent's permissions, its memory, local paths, the address of one's own
  scope (`.claude/settings.local.json`, `CLAUDE.local.md` and the like are ignored by git).
- Two rules hold for agents in particular, because an agent can act faster than anyone can watch:
  - An agent does not write to a scope (deploy, update, `oos-fpgad` commands that write, anything over adb, telnet
    or SSH that changes the unit) unless the owner of that scope asked for it in the session. Reading is fine.
  - An agent reports like a person does (ADR 0015): what was tested on a unit, what only on the host, what not at all.

## Alternatives

- **One file per tool with the full text:** the same text three or four times; nobody keeps them equal.
- **The rules only in `CONTRIBUTING.md`:** written for people who read once; it does not have the commands, the
  read order and the hard limits an agent needs at the start of every session, and tools do not load it by
  themselves.
- **A long `AGENTS.md` with everything:** it would grow into a second architecture document, be loaded into every
  session in full and still be out of date. The ADRs and `docs/` are the place for the content.

## Consequences

- An agent that reads `AGENTS.md` first knows the guard rails before it writes a line; a person reviewing its pull
  request can point to a record instead of explaining again.
- A new rule is a new ADR plus one line in `AGENTS.md`. A change of `AGENTS.md` that is not backed by an ADR or by
  `CONTRIBUTING.md` is a sign that a decision is missing.
- The commands in `AGENTS.md` must work as written; `tools/check.sh` (ADR 0020) is the one an agent runs before it
  says it is done.
- Worth a second look when the tools agree on another common file, or when `AGENTS.md` passes 150 lines.

## References

[AGENTS.md](../../../AGENTS.md), [CLAUDE.md](../../../CLAUDE.md), [CONTRIBUTING.md](../../../CONTRIBUTING.md),
ADR 0009, ADR 0013, ADR 0014, ADR 0015, ADR 0020.
