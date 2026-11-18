# ADR 0059: Batch is the only draw path

- Status: accepted (2026-11-18, by the maintainer)
- Date: 2026-11-18
- Category: code

## Context

ADR 0029 made Batch (`ui/src/drawpipe.rs`: deposit workers on the little cores draw every frame of a batch into maps of
their own) the default draw path and kept the frames path (Standard: the frames of a batch go through the UI's queue,
and the frame loop draws as many as it has time for) as the way back, until Batch had run as the default for a while.

Measured on a DHO924S on 2026-11-18 (verified on the unit, issue #186): Batch put every acquired frame into the
picture, the frames path dropped 50 to 76 % of them unseen; at 10 us/div Batch showed 14 to 19 screens a second
against 20 to 25. The owner decided to keep Batch and to raise its screen rate first. With the workers folding their
maps on the little cores once a screen (pull request #222), Batch shows 28.7 to 29.4 screens a second at 10 us/div
with one, two and four channels (verified on the unit, 2026-11-18), as many as the frames path or more.

## Decision

Batch is the only draw path of the hardware: the frames path, its switch (Acquire > Draw path, `acq.pipeline` in
`system.conf`, `OOS_PIPELINE`, the setup key `draw_path`) and what only it needed are removed. When this record is
accepted, ADR 0029 becomes "superseded by ADR 0059".

## Alternatives

- Keep the frames path as the way back (ADR 0029): a second path to keep and guard, which nobody chooses once Batch
  holds the screen rate, and a menu row that only explains the code's history.
- Keep Frames and remove Batch: the screen rate is the same now, and Frames leaves most of the acquired frames out of
  the picture; rare events, persistence and grading need all of them.

## Consequences

- The queue to the frame loop and its fast path (`Ui::fast_draw_all`) stay: they carry the frames of the batches the
  workers do not take (no view while the fast path does not apply, batches of one frame at slow time bases, the
  hardware average), and the newest frame of every batch, the record for measurements and decoders. The demo, replay,
  live and SIM sources have no batches and draw as before.
- What earlier versions stored or sent still works: `acq.pipeline` in `system.conf` is taken and dropped like
  `remote.telnet` (ADR 0054), `draw_path` in a setup is passed over, and `:SYSTem:PIPeline`, released in 0.1.13,
  stays for scripts (ADR 0023; ADR 0055 asks for a deprecation before a removal): its query answers `BATC`, `BATCh`
  is taken, `FRAMes` and `STANdard` are refused with `-224`.
- The status line keeps `pipeline <word>`: `Batch` while the workers run, `Off` with a source other than the hardware.
- Batch's own guards stay: its picture equals `draw_frame` on every frame one by one, and the perf guards compare the
  workers with the queue for the same frames.
- Worth a second look if a time base or a function shows a lower screen rate or fewer frames in the picture with the
  workers than through the queue: the queue is still there to take such batches.

## References

[ADR 0029](0029-batch-is-the-default-draw-path.md), [ADR 0004](0004-standard-draw-path-by-default.md),
[ADR 0023](0023-scpi-stock-names-first-oos-identifies-as-oos.md),
[ADR 0055](0055-the-remote-interface-is-stable-and-versioned.md),
[ADR 0054](../system/0054-ssh-is-the-only-shell-on-the-network.md), issue #186, pull request #222,
[fork/perf-plan.md](../../fork/perf-plan.md) (item 1), `ui/src/drawpipe.rs`, `ui/src/scpi/system.rs`,
`ui/src/sysconf.rs`.
