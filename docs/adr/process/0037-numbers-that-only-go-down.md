# ADR 0037: Numbers that only go down: large files, switched-off lints, `unsafe`

- Status: proposed
- Date: 2026-11-17
- Category: process

## Context

Some properties of the tree get worse one harmless step at a time. Measured on 2026-11-17 (commit 786c172):

- Nine Rust files have more than 2 000 lines; `ui/src/ui.rs` has 13 747 and `ui/src/scpi.rs` 6 115. Nobody decided
  that; each change added a little. A file of that size cannot be reviewed as a whole, by a person or by an agent.
- Lints are switched off 96 times (`allow(`): 80 in `oos-ui`, 9 in `oos-hal`, 6 in `oos-fpgad`, 1 in `oos-dsp`.
  ADR 0017 asks for a reason at each; nothing keeps the number from growing.
- `unsafe` is on 139 lines of code: 102 in `oos-ui`, 33 in `oos-hal`, 4 in `oos-fpgad`, none in `oos-dsp`.

A rule in a text ("keep files small") does not hold against this. A limit that the tree already breaks cannot be
switched on either: nine files would fail it on the first day.

## Decision

- **`tools/ratchet.txt` records where the tree stands**, one number a line: the lines of every Rust file over 2 000
  lines, and per crate how often a lint is switched off and how many lines use `unsafe`.
- **`tools/ratchet.sh` compares the tree with the file** (layer `ratchet` of `tools/check.sh`). A number that went up
  fails it. A Rust file that crosses 2 000 lines and has no line in the file fails it. A number that went down fails
  it too, until `tools/ratchet.sh --lower` has written the lower number, so the file never allows more than the tree
  needs.
- **Raising a number is possible and visible**: by hand, in the file, in a pull request that says why. CI names every
  raised or new number of a pull request as a notice. It is the maintainer's decision; a standing agent does not
  change the file except with `--lower` (ADR 0034).
- **Not enforced at first**, like fmt and clippy: CI runs the layer and shows the result without failing the job. It
  becomes an enforced layer when the pull requests that are open on 2026-11-17 are in, and this record is accepted.
- **Not ratcheted:** the total number of lines. A feature adds code, and a number that every feature has to raise
  teaches people to raise numbers.

## Alternatives

- **Hard limits** (no file over N lines, no `unsafe`): right as a goal, impossible to switch on today.
- **Clippy's number of findings as a ratchet**: it changes with every release of Rust, which adds lints, so the check
  would fail without a change of the tree. ADR 0017 makes clippy itself an enforced layer instead.
- **A larger tool that also checks module rules, function sizes and coverage**: more is checked, at the price of a
  program of several thousand lines that has to be kept. The three numbers here take a shell script of about a
  hundred lines; more can be added to it when one of them is missed.
- **Only the reviewer's eye**: what there was.

## Consequences

- Taking a large file apart shows as lower numbers, and the gain cannot be lost again without a visible change of
  the file.
- Two open pull requests that change the same large file both touch its line in `tools/ratchet.txt`; the later one
  is rebased and runs `--lower` again.
- A new `allow(` or `unsafe` needs an old one removed, or a raised number with its reason.
- `unsafe` is counted by lines, with a pattern, not by the compiler: a line with two blocks counts once, a
  multi-line block once. It is a trend, not an audit.
- Worth a second look: the limit of 2 000 lines once no file is over it (lower it), and other numbers that creep
  (functions over a length, `TODO`).

## References

ADR 0016, ADR 0017, ADR 0020, ADR 0034, `tools/ratchet.sh`, `tools/ratchet.txt`, `tools/check.sh`,
`.github/workflows/ci.yml`.
