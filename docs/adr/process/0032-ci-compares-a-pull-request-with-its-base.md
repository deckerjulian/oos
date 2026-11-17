# ADR 0032: CI compares the pictures and the register sequences of a pull request with its base

- Status: proposed
- Date: 2026-11-17
- Category: process

## Context

ADR 0014 asks a change that is not meant to change behaviour to prove it: the same pictures
(`ui/tools/layout-check.sh`), the same register sequences (`tools/plan-matrix.sh`). Both proofs were run by hand, in
two steps around the change, and the pull request said in words how they came out. Nothing checked that sentence.
ADR 0020 left both out of CI because they need a baseline from before the change.

Three things have changed:

- Since ADR 0031 every change is a pull request. A pull request has a base commit, and CI can build that commit next
  to the change. The baseline no longer has to come from somewhere.
- The largest files of the tree have to be taken apart (`ui/src/ui.rs` has 13 747 lines), in many small steps that
  each claim to change nothing. Such steps are only safe when the claim is checked every time.
- Coding agents write a growing share of the changes, and some will open pull requests by themselves. A sentence in a
  pull request that an agent wrote is not a proof.

Measured on 2026-11-17 on a laptop (host build, commit 09248b1): building the base and the change and comparing 276
pictures and the register sequences takes about three minutes with a cold build of the base, under two minutes after
that. Without calibration files `tools/plan-matrix.sh` prints 11 894 lines; with the files of one unit 12 460.

The pictures of the storage pages showed the full path of the storage folder, so two trees at different places gave
different pictures of the same source (12 of 276).

## Decision

- **One command for both proofs.** `tools/compare-base.sh [BASE]` checks the base commit out into a tree of its own,
  builds it, records its pictures and its register sequences, does the same with the working tree and compares. It
  names every picture and every register line that differs.
- **CI runs it for every pull request**, against the commit the pull request is merged onto (job `compare`). The result
  is in the summary of the job; the pictures that differ, both versions, and the difference of the register sequences
  can be downloaded from the run.
- **The pull request says which it is.** Its text has the line `Behaviour: unchanged`, or says what changes and why.
  CI fails a pull request that says `unchanged` while a picture or a register sequence differs. Without that line a
  difference is reported and is not an error: most changes are meant to change something.
- **Not a required check at first.** The job becomes a required check of main when it has run on ten pull requests
  without reporting a difference that was not there.
- **The pictures do not depend on where the tree lies.** `ui/tools/layout-check.sh` gives the storage folder by its
  short name.

## Alternatives

- **Reference pictures in the tree.** No second build, but 276 pictures (486 MB as they are rendered) that every
  change of the screen rewrites, and two open pull requests that touch the screen would always conflict.
- **A label instead of a line of text.** Only people with rights on the repository can set a label; the claim belongs
  to the author, who writes the text. An agent can write it too.
- **Fail on every difference.** Most pull requests are meant to change a picture or a sequence; they would all need
  an exception, and the exception would become routine.
- **A set of made-up calibration files in the tree**, so that CI also compares the calibrated values. Possible later;
  it needs its own record, because the files of a real unit never enter the tree (ADR 0013).
- **Keep it by hand.** The state before: the claim was not checked, and the two steps were easy to skip.

## Consequences

- A refactoring step, by a person or an agent, is accepted on a comparison a machine made, not on its description.
- CI has no calibration files, so the calibrated values of the register sequences are compared only by someone who
  runs `tools/compare-base.sh` with the files of a unit (`OOS_CAL_DIR` or `local/cal`). Timing, and what the hardware
  does with a sequence, are outside this comparison as before (ADR 0020).
- A pull request that adds a script to `ui/tests` or a line to the matrix shows new pictures or lines; it says so.
- The pull request that brings this record shows 12 pictures as different: its base still renders the storage pages
  with the full path.
- Every pull request costs a second CI job, which builds two release builds of `oos-ui` and of `oos-fpgad`.
- When this record is accepted, ADR 0014 and ADR 0020 become "accepted, amended by ADR 0032": the two proofs are one
  command, and CI can compare against the commit before a change.

## References

ADR 0014, ADR 0020, ADR 0031, `tools/compare-base.sh`, `ui/tools/layout-check.sh`, `tools/plan-matrix.sh`,
`.github/workflows/ci.yml`, [CONTRIBUTING.md](../../../CONTRIBUTING.md), [AGENTS.md](../../../AGENTS.md).
