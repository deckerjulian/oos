# ADR 0010: A hardware function that is not verified on a unit is off by default

- Status: accepted
- Date: 2026-11-13
- Category: hardware

## Context

Much of what OOS knows about the FPGA was worked out and tested on the host, and the forks bring functions that ran on
one model only (a DHO804, a DHO924S). A function that uses an FPGA block in a new way can be right on the host and
wrong on a unit. Example (DHO924S, 2026-11-15): the auto set-up took its signal levels from the FPGA's measurement
unit and got the levels of an earlier signal back for every request, so Auto chose a wrong scale.

## Decision

A function that drives the hardware in a way not yet verified on a unit is merged switched off. It is switched on by a
key in `/oos/system.conf` (or an environment variable for a test), and becomes the default only after it was seen
working on a unit; the document of the function says which unit and when.

Keys of this kind at the time of writing: `autoset.levels = unit`, `acq.prearm = on`, `acq.pipeline = batch` (ADR 0004).

The keys on 2026-11-15, as read by the code (`crate::sysconf::flag` and `setting_of`, until 2026-11-16
`crate::drawpipe::conf_value`; the list above had missed four):

| Key | Function | State |
|---|---|---|
| `acq.pipeline = frames` | the Standard draw path, the way back (ADR 0029) | Batch on by default since it was verified on the unit 2026-11-16 (until then `acq.pipeline = batch`, off by default) |
| `acq.prearm = on` | arm the next batch before the current one is decoded | off by default |
| `acq.compress = on` | min/max pairs from the FPGA instead of every sample, 1 us/div and slower | off by default |
| `acq.interp = fast` | the fast interpolation of sparse frames | off by default |
| `perf.place = on` | the threads and the DMA interrupts on fixed cores | off by default |
| `trig.serial_hw = on` | the UART protocol trigger in the FPGA's receiver | off by default |
| `autoset.levels = unit` | auto set-up levels from the FPGA's measurement unit | off by default; failed on the unit 2026-11-15 |
| `adc.overclock = on` | System > Overclock: an ADC clock above 1.25 GHz from the next start | off by default; every step boots, but records at the full rate lose samples (unit 2026-11-16; added 2026-11-17) |
| `trig.hw_status = off` | trigger status read from the FPGA | on by default since it was verified on the unit 2026-11-13 |
| `meas.deep_raw = off` | mean-type measurements from raw deep reads | on by default since it was verified on the unit 2026-11-15 |

A verified key keeps its `off` for a while as the way back; it is removed with the old path.

## Consequences

- The default behaviour of a release is what ran on a unit.
- Work from forks can be merged before it is verified everywhere, without changing what other owners get.
- Each such key is a second path that has to be kept working and tested; a key that was verified is made the default
  and removed, one that failed is removed with its code, so the list stays short.
- The documents keep the three words apart: verified on the unit, assumed, not yet verified on hardware (ADR 0015).

## References

[network.md](../../network.md) (`/oos/system.conf`), [fork/perf-plan.md](../../fork/perf-plan.md),
`ui/src/sysconf.rs`, `ui/src/ui/autoset.rs`.
