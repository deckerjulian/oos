# ADR 0001: Three crates; the hardware layer and the signal processing are pure functions

- Status: accepted, amended by ADR 0064 (`oos-ui` is a workspace: a layer without crossings is a crate below `ui/`)
- Date: 2026-11-07 (first public version; in use before)
- Category: code

## Context

Most of OOS has to be developed and tested without a scope on the desk: a wrong register write can only be seen on a
unit, and a unit is not always there. The scope's registers are mostly write-only, so the software has to know what it
wrote.

## Decision

The software is three Rust crates with one direction of dependency:

- `oos-hal` builds lists of `(offset, value)` register writes from settings, as pure functions with software shadows
  of the write-only registers. Applying a list is a separate step through the `Bus` trait (`Bar1` on the unit, a
  `Recorder` in tests).
- `oos-dsp` is signal processing without any hardware: waveforms are `&[f32]` volts with a fixed `dt`.
- `oos-ui` is the scope screen, with the sources Demo, Replay, Live and Hw behind one interface.

`oos-fpgad` is the command line on top of `oos-hal`.

## Consequences

- The register sequences are checked on the host against fixtures and the device logs; `tools/plan-matrix.sh` prints
  them for a fixed set of acquisitions, so a change of the analog path shows as a diff (ADR 0014).
- The whole screen runs on a computer (`oos-ui --serve`, `--script`), with the demo source in place of the hardware.
- Everything of the DHO800/DHO900 board is in `oos_hal::dho`; a port to another board family adds a module next to it
  and leaves the other crates alone.
- Code that talks to the hardware directly, outside a step list, does not fit and needs a reason.

## References

[architecture.md](../../architecture.md), [porting.md](../../porting.md), `hal/src/dho/bus.rs`.
