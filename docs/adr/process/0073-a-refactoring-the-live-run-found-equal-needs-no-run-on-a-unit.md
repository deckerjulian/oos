# ADR 0073: A refactoring the live run found equal needs no run on a unit for the board code it reaches

- Status: accepted (2026-11-19, by the maintainer, who left the gate's rules for stage 4 of ADR 0053 to the agent)
- Date: 2026-11-19
- Category: process

Amends ADR 0042 and ADR 0052. Stage 4 of ADR 0053.

## Context

ADR 0042 lets a refactoring pass `unit` without a scope when the comparison proves it: the title says `Refactor:` or
`Lints:`, the text says `Behaviour: unchanged`, the comparison with calibration values found nothing, and the pull
request touches only `$proven`, the code that builds register sequences. The board code is all in `$live`
(ADR 0052): `ui/src/board/` and the live paths of `hal/src/dho/`. Every refactoring of it waits for the scope on the
bench, because nothing on a host saw what the loop does to the unit.

ADR 0053 changed that. Since stages 2 and 3 (`docs/board-on-a-host.md`), `tools/compare-base.sh` runs the acquisition
loop against the simulated unit on both sides and compares, case by case:
- the unit's trace: every write, read, stream read, recorded frame and play, on the virtual clock;
- the frames delivered: their fields and a checksum of every sample.

Its cases cover AUTO, NORMAL, SINGLE and STOP; calibration; live compression; STOP views; a deep record's screen
window; roll and scan with and without the logic analyser; hardware segments. A change that moves a write, a wait or
a sample shows. The files are the same in two runs and in a release build. Section 7 of the plan counts 16 of the
loop's 28 paths as reached. The abort of a segmented recording makes 17, the target of goal T1, once the steps of
`seg` run on the loop's clock (stage 3e).

The plan's stage 4 says what follows from it. The files of the board code whose every path the live run reaches can
leave `$live` for a refactoring that the comparison, live run included, found equal. A file with a path the run does
not reach stays.

## Decision

**A third class of the hardware paths, `$looped` in `tools/pr-gate.sh`: the files whose every path of the loop, as
section 7 of `docs/board-on-a-host.md` counts them, the live run reaches. For `unit`, a refactoring of them passes
as one of `$proven` does under ADR 0042, and in addition only when the comparison ran the live run on both sides and
found every case equal.**

- **The sign.** CI's job `compare` keeps the artifact `compare-live` when its summary says "all N cases equal" for
  the live run, as it keeps `compare-calibrated` for the calibration values.
- **No sign without the comparison.** A base from before the live run, a case that differs, or a case only on one
  side gives no artifact, and the pull request waits for a unit.
- **The class comes first.** `$looped` is a class of its own, after `$testonly` and before `$live` and `$proven`, so
  it can take a file out of `ui/src/board/`. `tools/tests/test_pr_scripts.py` checks:
  - every name in `$looped` is a file of the tree;
  - no file is in both `$looped` and `$proven`;
  - a refactoring of a `$looped` file passes only with `compare-live`.
- **The first member** is `hal/src/dho/roll.rs`, whose paths the live run reaches in full: the roll stream, scan in
  RUN and SINGLE, the LA chunks. It leaves `$live`.
- **Next to join** are `ui/src/board/dho/hw/seg.rs` and `hal/src/dho/seg.rs`, with stage 3e. That stage puts the
  steps of `seg` on the loop's clock and adds the abort to the live run.
- **What stays live.** `ui/src/board/dho/hw.rs`, `hw/plan.rs`, `hw/decode.rs`, `hw/settings.rs` and
  `hal/src/dho/seq/exec.rs` hold paths the run does not reach yet:
  - the generator;
  - arming ahead;
  - the logic analyser's read of a record;
  - the core trim and levels;
  - the mask and zone units;
  - self-calibration.
  The scope's port (`hw/port.rs`) and `fpga.rs` stay live in any case. A file joins `$looped` in a pull request that
  shows the live run reaches all of its paths. Splitting `spawn` (goal S2) is how the reached parts get files of their
  own.
- **What does not change.** A pull request that is not a refactoring by its title and text needs a unit for `$looped`
  as for `$live`. So does one whose programs for the scope differ, unless the label `unit-tested` is set (ADR 0041,
  ADR 0069). The other classes keep their rules.

## Alternatives

- **Take all of `ui/src/board/` out of `$live` now.** The live run does not reach the generator, arming ahead, the
  core trim, the mask units or self-calibration. A refactoring of those would pass on a comparison that never ran them.
- **One list of functions instead of files.** It would be finer, but the gate reads a pull request's files through
  the API and never its code (ADR 0052), so the class has to be a file.
- **Leave `$live` as it is until `spawn` is split.** The split itself is the refactoring that needs this rule most:
  every step of it would wait for the scope.

## Consequences

- A `Refactor:` or `Lints:` pull request of a `$looped` file passes `unit` on CI alone when the comparison, live run
  included, found nothing. Its behaviour on the unit is then known as far as the model of `sim::Unit` goes. The pull
  request says so.
- The model of the unit is now part of what proves a change. A wrong model can hide a difference. Where a fact is
  assumed, `hal/src/dho/sim.rs` says so, and the hal tests hold the model to `docs/protocols/`. A change of the model
  is not a refactoring: it changes the files of the live run, so `compare` keeps `compare-base` and gives no
  `compare-live`.
- A case of the live run that becomes flaky takes the sign away for every pull request. ADR 0020 already requires
  such a test to be fixed, not rerun.

## References

ADR 0020, ADR 0038, ADR 0041, ADR 0042, ADR 0052, ADR 0053, ADR 0069, ADR 0072, `docs/board-on-a-host.md` (sections
6 and 7), `tools/pr-gate.sh`, `tools/compare-base.sh`, `.github/workflows/ci.yml`, `tools/tests/test_pr_scripts.py`.
