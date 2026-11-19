# ADR 0072: The tests inside the hardware paths need no run on a unit

- Status: accepted (2026-11-19, by the maintainer)
- Date: 2026-11-19
- Category: process

## Context

The status `unit` holds a pull request that touches a hardware path (ADR 0038, ADR 0052) until a run on a unit, the
comparison of the programs (ADR 0041, ADR 0068) or the comparison of ADR 0042 lets it pass. Five files in those paths
hold only tests: `hal/src/dho/boot/tests.rs`, `hal/src/dho/seq/tests.rs`, `hal/src/update/tests.rs`,
`hal/src/update/hostile_tests.rs` and `ui/src/board/dho/hw/tests.rs`. Their parent modules bring them in with
`#[cfg(test)]`, so only `cargo test` compiles them, and no program for the scope holds them.

On 2026-11-19 a pull request that compiles every decoder in (ADR 0071) changed one line in
`ui/src/board/dho/hw/tests.rs`: an attribute that named the feature `decoders`, which is gone. Its other changes are
outside the hardware paths. They change `oos-ui` (the features of the crate change its layout) but nothing that only
a unit proves: `compare` found every picture and every register sequence equal. The one test line still made the pull
request wait for a run on a unit, and that run could not have told anything about it.

## Decision

**For the status `unit`, a file of the hardware paths that only tests compile is not a hardware path. `tools/pr-gate.sh`
names them in `$testonly` (`tests.rs` and `*_tests.rs` under `hal/src/dho/`, `hal/src/update/`, `ui/src/board/` and
`fpgad/src/`) and leaves them out before it asks the other questions.**

- `tools/tests/test_pr_scripts.py` fails when such a file is not brought in by its parent only under `#[cfg(test)]`,
  so a module named like a test that is part of a program is caught.
- The other files of the pull request are judged as before: a change of the code beside the tests still needs a unit,
  the comparison of the programs, or ADR 0042.
- `$testonly` is a class of its own, before `$live` and `$proven`; `seq/tests.rs` leaves `$proven`.

## Alternatives

- **Put the test files into `$proven`** (the suggestion this came from). A pull request would then still need the title
  `Refactor:` or `Lints:` and a calibrated comparison (ADR 0042) for a change of tests only. A run on a unit cannot
  prove a test any more than the comparison can.
- **Leave it as it is.** Then a change of a test next to the board code waits for a unit that cannot see it, or the
  test moves out of the hardware path to get past the gate.

## Consequences

- A pull request that changes only tests inside the hardware paths, besides files outside them, passes `unit` without
  a scope. A change of the board code next to its tests is judged as before.
- The tests themselves still run in CI (`cargo test`).
- A test module that is not `#[cfg(test)]` cannot be called `tests.rs` in these paths without the test failing.

## References

ADR 0038, ADR 0041, ADR 0042, ADR 0052, ADR 0068, ADR 0071, `tools/pr-gate.sh`, `tools/tests/test_pr_scripts.py`.
