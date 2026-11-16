# ADR 0020: The checks come in layers; one command runs every host layer, the same as CI

- Status: accepted
- Date: 2026-11-15
- Category: process

## Context

OOS has many checks by now, but they grew one at a time and are spread over the tree: `cargo test` in four crates
(about 1 500 tests), the target build, the decoder feature builds, the syntax checks of the scripts in CI, the layout
check (`ui/tools/layout-check.sh`), the register sequences (`tools/plan-matrix.sh`), the menu geometry check
(`ui/src/ui/lint.rs`), the interplay tests, and the acceptance test on a unit (`tools/device-test.py`). Which of
them to run before a pull request was written in `CONTRIBUTING.md` as five commands, and CI ran a different set.
A contributor, and even more an agent, ran what it found, said "tests pass", and CI or the scope found the rest.

The forum asked for a test suite "for host and device, good for manual and AI testing" (2026-11-15). Most of it
exists; what is missing is one way in.

## Decision

The checks are named by layer. Each layer says what it can show and what not:

| Layer | Command | Shows | Does not show |
|---|---|---|---|
| format | `tools/check.sh fmt` | layout as rustfmt wants it (ADR 0016) | anything else |
| lint | `tools/check.sh clippy` | Clippy clean (ADR 0017) | whether the code is right |
| text | `tools/check.sh text` | file rules of ADR 0019: licence line, line ends, trailing spaces | the content |
| syntax | `tools/check.sh syntax` | shell and Python scripts parse | that they work |
| test | `tools/check.sh test` | the unit tests, the interplay tests and the menu geometry of all four crates; the tests of the Python host tools | the hardware |
| target | `tools/check.sh target` | the scope's build (`aarch64-unknown-linux-musl`) compiles and depends on `libc` only (ADR 0021) | that it runs |
| decoders | `tools/check.sh decoders` | the core-only build and every optional decoder alone | |
| sim | `tools/check.sh sim` (by name: needs Docker, QEMU and image builds) | the scripts of the image in arm64 containers and virtual machines: firewall, SSH, USB stick, WLAN, the unit's DTB step, the USB install, the GPU picture against the CPU's | the scope's hardware and drivers |
| pixels | `ui/tools/layout-check.sh record` / `check` | a refactoring leaves every picture the same (ADR 0014) | timing |
| registers | `tools/plan-matrix.sh` before and after | the analog path writes the same sequences (ADR 0014) | what the hardware does with them |
| speed | `tools/perf-guard.sh` | the fast paths stay faster than the paths they replaced (ratios, release build) | times on a unit |
| unit | `tools/device-test.py --host IP` | the scope as a user meets it: values, triggers, frame rate, no panic | other models, unless run there |

- `tools/check.sh` with no argument runs every host layer that is enforced, in the order above, and stops at the first
  that fails. CI calls the same script, layer by layer, so "passes here" and "passes in CI" mean the same.
- `fmt` and `clippy` can be run now but join the default set with the rollout of ADR 0016 and ADR 0017; until then
  `tools/check.sh` names them as not yet enforced. `sim` is run by name (corrected 2026-11-16: the simulations
  existed as scripts of `tools/` and `gpu/` and were missing from the table).
- `pixels` and `registers` compare against the commit before a change and need a baseline; they are run for every
  change that is not meant to change the screen or the analog path, and the pull request says that they were.
  `speed` measures and needs a computer that does nothing else; it is run for a change of a fast path.
- `unit` needs a scope and its owner's consent (ADR 0018). A pull request says whether it was run and on which model
  (ADR 0015).
- A bug that was found on a unit gets a host test where it can, so the next change cannot bring it back without
  `tools/check.sh` failing.

## Alternatives

- **The commands only in `CONTRIBUTING.md`:** the state before; the list and CI drifted apart.
- **A Makefile or `cargo xtask`:** `make` is not on every Windows computer, and `xtask` would be a fifth crate for
  what a POSIX shell script does. The script runs on macOS, Linux and in CI as it is.
- **Everything in CI only:** CI cannot render against a baseline from before the change, and it has no scope. The
  host layers have to run on the contributor's computer anyway.

## Consequences

- Before a pull request, and before an agent says it is done: `tools/check.sh`. One line in `CONTRIBUTING.md` and
  `AGENTS.md` instead of five.
- A new kind of check becomes a new layer in the script and a line in the table above, not a new command to remember.
- A test holds on a busy computer: the tests run in parallel, and CI shares its machine. A test that fails only
  under load is fixed so it does not depend on time, not run again until it passes (2026-11-15:
  `fast_path_on_several_cores_draws_as_one_by_one` compared the take of a step with the take after it).
- The host layers take minutes; the test layer is most of it. A layer can be run alone (`tools/check.sh test`).
- The unit layer stays the only one that sees the hardware. Many changes cannot be tried on a scope when they are
  made, so the pull request has to say which layers ran.

## References

`tools/check.sh`, [.github/workflows/ci.yml](../../../.github/workflows/ci.yml), `ui/tools/layout-check.sh`,
`tools/plan-matrix.sh`, `tools/perf-guard.sh`, `tools/device-test.py`, [CONTRIBUTING.md](../../../CONTRIBUTING.md),
ADR 0014, ADR 0015, ADR 0016, ADR 0017, ADR 0018, ADR 0019, ADR 0021.
