# ADR 0004: Two draw paths: Standard by default, Batch to choose

- Status: accepted, to be reviewed (when Batch holds the frame rate with four channels at 10 us/div)
- Date: 2026-11-14 (both selectable), measured on the unit 2026-11-15
- Category: code

## Context

The FPGA delivers the acquired frames in batches. There are two ways to get them onto the screen; the acquisition and
the record for measurements and decoders (the newest frame of a batch) are the same in both.

- **Standard** (`FRAMes`): the hardware thread decodes the frames and queues them for the UI, which draws as many per
  refresh as it has time for (`Ui::fast_draw`). Frames that do not fit the queue are dropped.
- **Batch** (`BATCh`, from PR #7, `ui/src/drawpipe.rs`): the hardware thread decodes only the newest frame and lends
  the read-out buffer with all the others to deposit workers, one per little core. They decode and draw with the same
  function into maps of their own; the UI adds the maps once per refresh.

For the same frames the picture is the same (host tests).

Measured on a DHO924S, square wave 100 kHz on CH1, trigger Auto (2026-11-15), frames a second / frames drawn a second:

| Channels | Time/div | Standard | Batch |
|---|---|---|---|
| 1 | 100 ns | 29.1 / 1467 | 30.0 / 5189 |
| 4 | 100 ns | 29.7 / 712 | 29.2 / 1129 |
| 4 | 1 us | 29.6 / 1104 | 27.0 / 3509 |
| 4 | 10 us | 26.1 / 106 | 18.2 / 318 |
| 4 | 500 us | 24.0 / 32 | 27.9 / 56 |

## Decision

Both paths stay in the tree. Standard is the default; Batch is chosen with Acquire > Draw path, `:SYSTem:PIPeline`,
or `acq.pipeline = batch` in `system.conf`. Setups keep the choice (`draw_path`).

## Consequences

- Batch draws 1.5 to 4 times as many of the acquired frames, so rare events show more often; it is not the default
  because the screen falls to 18 frames a second with four channels at 10 us/div. 30 frames a second come first.
- Two paths mean two sets of guards: the picture of Batch equals that of Standard over the same frames, and both count
  acquired / used / dropped frames (`:SYSTem:STATe?`, System info "Frames used").
- Review: once Batch holds the frame rate in every row above, it becomes the default and Standard can go. The cause of
  the drop at 10 us/div is not yet found.

## References

[fork/perf-plan.md](../../fork/perf-plan.md) (item 1), `ui/src/drawpipe.rs`, `ui/src/model.rs` (`draw_batch`).
