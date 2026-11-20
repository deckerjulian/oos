# ADR 0014: A refactoring keeps every pixel and every register write

- Status: accepted, amended by ADR 0032 (the two proofs are one command, and CI runs it against the base)
- Date: 2026-11-12 (layout check); the register comparison from the first public version
- Category: process

## Context

Most changes cannot be tried on a scope when they are made. A change that is meant to alter nothing (moving code into
modules, a faster drawing path, a new layout engine) is the kind that breaks something unnoticed: a row a few pixels
lower, one register write more in the analog path.

## Decision

A change that is not meant to change behaviour has to show that it does not, by comparison with the state before it:

- **Screen:** `ui/tools/layout-check.sh record` before the change and `check` after it. It renders every script in
  `ui/tests` at the three text sizes on the host and compares the pictures pixel by pixel; a pure refactoring reports
  nothing. A change that is meant to be seen names the pictures that differ and records them again.
- **Analog path:** `tools/plan-matrix.sh` prints the register sequences of a fixed set of acquisitions. It is run
  before and after, and the pull request says whether the output differs.
- **Values:** computations that move are compared bit for bit against the code before the move, over the same inputs.

## Consequences

- Large moves became possible without a unit on the desk: the measurements as modules (same values, bit for bit), the
  analyser registry and the border drawing for the GPU (same pixels) were merged on this evidence.
- The layout scripts must be deterministic: an empty storage directory, a fixed clock, menus opened by name and not by
  coordinates.
- The recorded pictures are a local baseline (`target/layout-base`), not part of the tree: they are recorded from the
  commit before the change.
- Pixel equality on the host says nothing about timing on the unit; frame rate and load are held by
  `tools/device-test.py` on a scope.

## References

`ui/tools/layout-check.sh`, `tools/plan-matrix.sh`, [CONTRIBUTING.md](../../../CONTRIBUTING.md). (Corrected
2026-11-16: a link to `docs/local-validation.md`, a work log that was removed, is gone.)
