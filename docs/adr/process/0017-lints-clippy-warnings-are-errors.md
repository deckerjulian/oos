# ADR 0017: Clippy runs in CI; its warnings are errors, with a short list of allowed lints that says why

- Status: accepted (2026-11-17, with the last lint pull request; the layer `clippy` is enforced in CI), amended by ADR 0045 (the tool chain is pinned) and ADR 0048 (`too_many_arguments` in hal)
- Date: 2026-11-15
- Category: process

## Context

Until now CI only built and tested; nothing looked at the code itself. Measured on main at 84deb24 (2026-11-15),
Clippy 0.1.100, `cargo clippy --all-targets` over the four crates: about 570 warnings (hal 64, dsp 176, fpgad 2,
ui 329). By kind:

| Lint | Count | What it is here |
|---|---|---|
| `neg_cmp_op_on_partial_ord` | 253 | `!(dt > 0.0)`: deliberate, it is also true for NaN, and the signal paths rely on that |
| `needless_range_loop` | 120 | `for i in 0..n` over several buffers of the same length |
| `clone_on_copy`, `unnecessary_map_or`, `manual_is_multiple_of`, `chunks_exact_to_as_chunks`, ... | about 170 | style; a newer, shorter way to write the same |
| `too_many_arguments`, `type_complexity` | 66 | builders that take the fields of one register; already allowed in 26 places by hand |
| `suspicious_assignment_formatting`, `possible_missing_else`, `precedence`, `if_same_then_else`, `approx_constant` | 30 | worth reading; all read so far were harmless (ADR 0016), some are leftovers to tidy |

None of it was a bug that changes what the scope does. But with 570 warnings a new one is not seen, and the lints
that do find bugs (a missing `else`, a `-=` written as `= -`, an operator precedence that reads differently than it
works) only help while the list is empty.

## Decision

- Every crate runs `cargo clippy --all-targets -- -D warnings` in CI: a warning fails the build.
- What is allowed for the whole crate stands in the `[lints.clippy]` table of its `Cargo.toml`, each line with a
  comment that says why. The list is short and the same in every crate:

  ```toml
  [lints.clippy]
  # `!(dt > 0.0)` is meant: it is also true for NaN, and the signal paths rely on that
  neg_cmp_op_on_partial_ord = "allow"
  # index loops over several buffers of the same length read better than zipped iterators
  needless_range_loop = "allow"
  # step builders take the fields of one register; a struct for each would only move the list
  too_many_arguments = "allow"
  ```

- Anything else is fixed, or allowed at the one place with `#[allow(clippy::...)]` and a comment that says why.
  `#[allow]` without a reason is not merged.
- Compiler warnings are errors in CI too: `tools/check.sh` fails a layer when cargo prints a warning (not through
  `RUSTFLAGS`, which would build every crate a second time next to the normal build). `dead_code` that stays on
  purpose (a format field, a table kept whole) carries `#[allow(dead_code)]` with its reason, as it does now.
- The tool chain is the current stable Rust. A new Clippy that brings new warnings is fixed in a separate commit, not
  inside a feature.
- The other languages: shell scripts stay POSIX `sh`, because they run in the scope's busybox, and CI checks them with
  `sh -n`; Python is parsed (`ast.parse`, which writes no `.pyc`) and has to run on the Python 3.9 of macOS: no
  backslash inside an f-string, no `match`. `shellcheck` and a Python linter are welcome on a developer's computer;
  they are not required until they run clean over the tree.

## Alternatives

- **`clippy::pedantic` on top:** several thousand more warnings, many against the way signal code is written (casts
  between integer and float everywhere). It would make the list longer, not more useful.
- **Warnings shown, not enforced:** the state of the last weeks in all but name. The count only grows.
- **A count that must not grow (a ratchet):** simple to start with, but it needs a script that is easy to get around,
  and it keeps the 570 forever. The clean-up is small enough to do once.

## Consequences

- **Rollout:** the allow list is in the four `Cargo.toml` with this record; it takes the count from about 570 to about
  320 (hal 45, dsp 60, fpgad 2, ui 217). Compiler warnings: none on main, and `tools/check.sh` fails on a new one from
  now on. Next the formatting commit (ADR 0016); without it a part of the warnings are only layout. Then the rest
  fixed in small commits by kind (`Lints: clone_on_copy in ui`),
  each one shown to change nothing with the checks of ADR 0014. The last of them turns on `-D warnings` in CI and sets
  this record to "accepted".
- A new Clippy version can break a build that worked yesterday. That is the price for staying on stable without
  pinning a version; the fix is a lint commit, and it is usually small.
- The allow list is a decision in its own right: a new crate-wide allow needs a change of this record.
- Clippy says nothing about whether a register write is right. That stays with the tests, `plan-matrix.sh` and a unit
  (ADR 0014, ADR 0020).

## References

ADR 0014, ADR 0016, ADR 0020, `hal/Cargo.toml`, `dsp/Cargo.toml`, `ui/Cargo.toml`, `fpgad/Cargo.toml`,
[.github/workflows/ci.yml](../../../.github/workflows/ci.yml).
