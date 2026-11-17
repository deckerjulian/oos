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

Measured on a DHO924S on 2026-11-17 (verified on the unit): a generator's 100 kHz square wave on CH1, trigger Auto,
no HDMI monitor, 1.25 GHz, horizontal offset 0, Auto depth unless named. Screen frames a second / acquired frames
drawn a second:

| Channels | Time/div | Standard | Batch |
|---|---|---|---|
| 1 | 100 ns | 29.3 / 1563 | 30.0 / 5269 |
| 4 | 100 ns | 29.5 / 769 | 30.0 / 1213 |
| 4 | 1 us | 29.6 / 1320 | 29.3 / 3840 |
| 4 | 10 us | 28.0 / 108 | 26.2 / 319 |
| 4 | 500 us | 25.2 / 32 | 28.7 / 38 |
| 1 | 1 us, 1M points | 29.8 / 17 | 29.8 / 17 |
| 4 | 1 ms, 10M points | 28.8 / 2 | 28.8 / 2 |

The acquisition took about 33,000 frames a second with one channel at 100 ns/div (1408 points). With the PCIe link
at 2.5 GT/s and at 5 GT/s the two paths compare the same. At four channels and 10 us/div Batch keeps 26 frames a
second (18 on 2026-11-15) and draws three times as many acquired frames. For the same frames the picture of both
paths is the same (host tests in `drawpipe.rs`).

A horizontal offset makes every record reach from the trigger to the shown window: with 132 us of offset at
100 ns/div the Auto depth is 330,256 points, and both paths fall to about 140 acquired frames a second (91 drawn with
Standard, 126 with Batch, 2026-11-16). Rates are only comparable at the same offset.

## Decision

Batch is the default draw path. Standard stays selectable as the way back: Acquire > Draw path, `:SYSTem:PIPeline
FRAMes`, or `acq.pipeline = frames` in `system.conf` (ADR 0010). This supersedes ADR 0004.

## Alternatives

- Keep Standard as the default: it draws fewer of the acquired frames in every row and holds the screen rate no
  better, so rare events show less often for nothing.
- Remove Standard now: one measurement day is little for a path that every unit runs; it goes once Batch has run as
  the default for a while without a report against it.

## Consequences

- Batch draws 1.2 to 3.4 times as many of the acquired frames from 100 ns to 500 us/div and as many with deep memory.
  It keeps about 30 frames a second in most rows; at four channels and 10 us/div it holds 26 where Standard holds 28,
  and at 500 us/div 29 where Standard falls to 25.
- The draw path at the start comes from `system.conf` (Batch unless `acq.pipeline = frames`), not from the setup the
  scope restores: setups saved before this change hold `draw_path = standard`, the old default, and would keep every
  unit on Standard. The menu and `:SYSTem:PIPeline` switch for the session, a recalled setup still sets its path, and
  the next start begins with the path of `system.conf` again.
- Both paths stay guarded until Standard goes: the Batch picture equals the Standard one over the same frames, and both
  count acquired / used / dropped frames (`:SYSTem:STATe?`, System info "Frames used").
- Not measured: the logic analyser, hardware segments and the zone trigger with Batch at these time bases; where the
  fast path does not apply, Batch queues every frame as Standard does.

## References

[ADR 0004](0004-standard-draw-path-by-default.md),
[ADR 0010](../hardware/0010-unverified-hardware-functions-are-opt-in.md), [fork/perf-plan.md](../../fork/perf-plan.md)
(item 1), `ui/src/drawpipe.rs`, `ui/src/model.rs` (`draw_batch`).
