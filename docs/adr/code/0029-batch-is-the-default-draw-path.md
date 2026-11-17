# ADR 0029: Batch is the default draw path

- Status: accepted
- Date: 2026-11-16
- Category: code

## Context

ADR 0004 kept two ways to put the acquired frames on the screen: Standard (`FRAMes`, the UI thread draws the frames
of the queue it has time for) as the default, and Batch (`BATCh`, `ui/src/drawpipe.rs`: deposit workers on the little
cores draw every frame of a batch into maps of their own). Batch drew more of the frames, but on 2026-11-15 the screen
fell to 18 frames a second with four channels at 10 us/div. The record asked for a review once Batch holds the frame
rate in every row of its table.

Measured on a DHO924S on 2026-11-16 (verified on the unit): a generator's 100 kHz square wave on CH1, trigger Auto,
no HDMI monitor, 1.25 GHz. Screen frames a second / acquired frames drawn a second:

| Channels | Time/div | Standard | Batch |
|---|---|---|---|
| 1 | 100 ns | 29.9 / 91 | 29.9 / 126 |
| 4 | 100 ns | 29.9 / 75 | 29.9 / 117 |
| 4 | 1 us | 30.0 / 73 | 29.9 / 115 |
| 4 | 10 us | 29.1 / 62 | 30.0 / 86 |
| 4 | 500 us | 25.3 / 32 | 29.4 / 38 |
| 1 | 1 us, 1M points | 30.1 / 17 | 30.0 / 17 |
| 4 | 1 ms, 10M points | 28.9 / 2 | 29.3 / 2 |

The same with the PCIe link at 2.5 GT/s. The drop to 18 frames a second at four channels and 10 us/div no longer
occurs. For the same frames the picture of both paths is the same (host tests in `drawpipe.rs`).

The absolute rates are far below those in ADR 0004's table for the same unit (1467 / 5189 frames drawn a second with
one channel at 100 ns/div): the acquisition delivered about four frames per batch at about 30 batches a second with
three quarters of the processor idle, under 0.1.13 as well. The cause is not known yet. Both paths were measured in
the same set-up one after the other, so the comparison between them holds.

## Decision

Batch is the default draw path. Standard stays selectable as the way back: Acquire > Draw path, `:SYSTem:PIPeline
FRAMes`, or `acq.pipeline = frames` in `system.conf` (ADR 0010). This supersedes ADR 0004.

## Alternatives

- Keep Standard as the default: it draws fewer of the acquired frames in every row and holds the screen rate no
  better, so rare events show less often for nothing.
- Remove Standard now: one measurement day is little for a path that every unit runs; it goes once Batch has run as
  the default for a while without a report against it.

## Consequences

- Batch draws 1.2 to 1.6 times as many of the acquired frames from 100 ns to 500 us/div, as many with deep memory,
  and keeps about 30 frames a second in every measured row, also where Standard fell to 25.
- The draw path at the start comes from `system.conf` (Batch unless `acq.pipeline = frames`), not from the setup the
  scope restores: setups saved before this change hold `draw_path = standard`, the old default, and would keep every
  unit on Standard. The menu and `:SYSTem:PIPeline` switch for the session, a recalled setup still sets its path, and
  the next start begins with the path of `system.conf` again.
- Both paths stay guarded until Standard goes: the Batch picture equals the Standard one over the same frames, and both
  count acquired / used / dropped frames (`:SYSTem:STATe?`, System info "Frames used").
- Not measured: the logic analyser, hardware segments and the zone trigger with Batch at these time bases; where the
  fast path does not apply, Batch queues every frame as Standard does.

## References

[ADR 0004](0004-standard-draw-path-by-default.md), [ADR 0010](../hardware/0010-unverified-hardware-functions-are-opt-in.md),
[fork/perf-plan.md](../../fork/perf-plan.md) (item 1), `ui/src/drawpipe.rs`, `ui/src/model.rs` (`draw_batch`).
