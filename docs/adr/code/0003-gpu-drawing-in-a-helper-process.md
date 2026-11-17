# ADR 0003: The GPU draws from commands in a helper process; the CPU stays the fallback

- Status: accepted
- Date: 2026-11-12
- Category: code

## Context

The software renderer kept one core busy at about half its time for the frame loop (53 % with four channels in Run,
DHO924S, 2026-11-12), and a monitor in HDMI Layout meant drawing the screen a second time on the CPU. The RK3399 has a
Mali-T860 that was idle. `oos-ui` is one static musl binary; Mesa needs glibc.

## Decision

With System > Graphics = GPU (the default) the CPU only records what is to be drawn, as a list of commands in two
layers (under and over the traces), and leaves the traces as phosphor maps. A separate process, `oos-gpu`, draws the
layers from the commands on the GPU, adds the traces and scales the picture for a monitor. The two share one memory
arena and a socket with a few fixed C structs. Every failure ends the helper, and `oos-ui` draws on the CPU again.

The CPU renderer stays complete. A recorded layer, replayed, gives exactly the pixels of drawing directly; a test holds
that for every kind of screen.

## Alternatives

- Layers as pixels, drawn on the CPU and uploaded every frame (the first version): frame loop 44 % of its core against
  35 % with commands.
- Linking Mesa into `oos-ui`: would end the static musl binary and tie the scope screen to the GPU stack's failures.

## Consequences

- Frame loop 53 % -> 35 % of its core, the helper 8.5 % (same measurement); the monitor's picture costs no CPU work.
- The GPU is no faster than the CPU at the traces. What it draws is paid by area: a command that covers pixels a later
  one covers again costs twice. Buttons and dialog frames therefore draw their border only where the fill leaves it
  (`Canvas::framed`, 2026-11-15: monitor GPU time 38.5 -> 12.3 ms with the measurement page open in HDMI Layout).
- Every new drawing primitive needs its command, its shader case in `oos-gpu.c` and the replay test.
- `tools/device-test.py` `frame_rate_by_graphics_and_monitor` holds the frame rate and the GPU time on the unit.

## References

[gpu/README.md](../../../gpu/README.md), `ui/src/gpu.rs`, `ui/src/gfx.rs` (`Cmd`, `Rec::replay`), `gpu/oos-gpu.c`.
