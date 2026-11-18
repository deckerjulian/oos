# ADR 0042: A refactoring of register-sequence code that the comparison found equal needs no run on a unit

- Status: accepted (2026-11-17, by the maintainer), amended by ADR 0052 (what a run on a unit covers by class of path)
- Date: 2026-11-17
- Category: process

## Context

ADR 0041 lets `unit` pass when the programs for the scope are byte-identical to the base's. Refactorings rarely
are: #116 replaced `x % n == 0` with `x.is_multiple_of(n)`, `map_or(true, f)` with `is_none_or(f)` and
`chunks_exact(N)` with `as_chunks::<N>()` in `ui/src/board/` and `hal/src/dho/`, which means the same and still gives
other programs. A run on a unit for such a change costs a quarter of an hour, needs the maintainer's scope, and says
little: the roll time bases fail for any build that changes `ui/src/board/` (#113). The maintainer released #116 by
hand and asked for such changes to need no run on a unit any more.

The comparison with the base (ADR 0032) proves pictures, stored files, remote answers and register sequences, the
calibrated ones included with the made-up calibration set (#98). It does not run the live acquisition on a board
(DMA, the roll stream, timing), the start-up or the update.

## Decision

- **What the comparison proves goes in without a unit when it is a refactoring.** `unit` passes when the title
  begins with `Lints:` or `Refactor:`, the pull request says `Behaviour: unchanged`, CI's job `compare` passed and kept
  nothing **and compared the register sequences with calibration values** (artifact `compare-calibrated`: the
  made-up set of #98; without it CI prints them without calibration, and this rule does not apply), and every path it
  touches that a unit proves is code that builds register sequences: the comparison
  prints them for acquisitions, triggers, mask, logic analyser, generator, roll and scan plans, the PLL table and the
  cold boot against the unit model (`tools/plan-matrix.sh`), so a change of one shows there and the run on a unit is
  asked for again.
- **What only a unit proves still needs one**, refactoring or not, unless the programs are byte-identical (ADR 0041):
  - start and update: `boot/`, `kernel/`, `rootfs/`, `hal/src/update/`, `hal/src/dho/boot/`,
    `fpgad/src/bin/oos-update.rs`;
  - the commands of oos-fpgad that work on the board (`fpgad/src/main.rs`, `board.rs`, and the subcommand modules
    `afg.rs`, `bringup.rs`, `roll.rs`, `selfcal.rs`, `tool.rs`);
  - timing and the live run against the board: `ui/src/board/`, and in `hal/src/dho/` the buses and read paths
    (`bus`, `gpio`, `panel`), the roll stream, segmented memory, self-calibration, core trim, levels, board
    supervision.
  The list is `live` in `tools/pr-gate.sh`.
- Copilot's review stays required (ADR 0039); it reads whether a change called a refactoring is one.

## Alternatives

- **A run on a unit for every change of the programs** (ADR 0041 alone): safe, slow, and for refactorings mostly
  noise (#113).
- **Running the board code on the host** against a simulated FPGA, so the comparison sees the live acquisition too:
  the better proof, and a project of its own; until it exists this record leans on the title, the comparison and
  the review.

## Consequences

- Lint rounds and moves in the register-sequence code go in without the scope; the development agent's refactoring
  runs (ADR 0039) do not wait for the maintainer there. A refactoring of the live paths, the start or the update
  waits for a run on a unit, as #116 would have.
- The brake is the comparison of the register sequences, and the honesty of the title and of `Behaviour:
  unchanged`, read by Copilot. A new file that runs live against the board has to be added to `live`.
- Worth a second look when a merged refactoring had to be reverted after a run on a unit, or when the board code can
  run against a simulated FPGA on the host (then the live paths could join).

## References

ADR 0032, ADR 0038, ADR 0039, ADR 0041, #113, #116, `tools/pr-gate.sh`.
