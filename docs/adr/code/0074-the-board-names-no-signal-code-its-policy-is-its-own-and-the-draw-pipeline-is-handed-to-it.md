# ADR 0074: The board names no signal code: what its thread decides by is its own, and the draw pipeline is handed to it

- Status: proposed
- Date: 2026-11-20
- Category: code

Amends ADR 0049 (how the count `ui-board-up` reaches 0).

## Context

ADR 0049 puts `board` below `signal`: the board code may name base and the model, the signal modules may name the
board. On 2026-11-20 (commit 03f6a357) 39 paths of `ui/src/board/` named signal modules, and ADR 0049 noted that
`board` and `signal` might be one layer "if the acquisition thread turns out to belong to the pipeline". Apart from
the record of a frame (`board/record.rs`, signal code that only sat in the board's folder, now `wave/record.rs`), the
paths are of two kinds:

- **Rules the acquisition thread decides by**, written in the signal modules that use their result: the live
  compression's switch, its factor, its limits and the model conditions that allow it (`compress::enabled`, `k_for`,
  `allowed`, `MAX_DEPTH`, `RAW_PERIOD`, `note_batch_k`), and the raw reads of deep records (`deepmeas::Want`, `due`,
  `MAX_BYTES`). They read only the model, the clock and `system.conf`.
- **The draw pipeline**, which the thread feeds with each batch: `drawpipe::pipe`, its counters, the helper the thread
  draws with while it waits, and the shapes of what it hands over (`CodeLayout`, `CodesInto`, `RawBatch`).

## Decision

The board names no signal code. A rule the acquisition thread decides by lives in the board layer, in
`board::policy` (`policy::compress`, `policy::deep`), and the signal modules that draw and measure what the thread
delivers name it there. What the thread hands to the signal layer goes through an interface the board declares
(types and a trait in the board layer); the signal layer implements it and registers its implementation when it
starts (a `OnceLock` of the board), so the board calls it without naming it.

## Alternatives

- **`board` and `signal` as one layer.** It hides the direction instead of keeping it: the board door
  (`board::Board`) would again be free to name the screen's records, and a later crate of the board (ADR 0064) could
  not be cut.
- **`board` above `signal`** (the acquisition thread as a part of the pipeline). The signal modules name the board's
  `Frame`, `Board` and `Shared` in about 150 places; the direction would turn for many paths to save few.
- **Re-exports under the old names** (`compress::k_for` giving on `board::policy::compress::k_for`). Two names for
  one thing; the callers name the board's module directly.
- **The policy in the model layer.** The model is what the user has set; the rate limits, the read budget and the
  factor of the last batch are the thread's.

## Consequences

- `ui-board-up` goes to 0 in steps, each a refactoring with `Behaviour: unchanged`; the step of the draw pipeline adds
  one dynamic call per batch and per look of the waiting thread, and no allocation per frame.
- A new rule the thread decides by goes into `board::policy`, not into the signal module that uses its result.
- A test switch of such a rule (`policy::compress::force`) is named where the rule is.
- Worth a second look: when the count is 0, whether the board layer can become a crate of its own (ADR 0064).

## References

ADR 0049, ADR 0053, ADR 0064; issue #120 (goal S3), issue #392; `tools/layers.py`, `tools/layers.txt`;
`ui/src/board/policy.rs`, `ui/src/compress.rs`, `ui/src/deepmeas.rs`, `ui/src/drawpipe.rs`.
