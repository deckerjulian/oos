# Contributing to OOS

Reports, measurements and code are all welcome. The most useful thing right now is a test on hardware: OOS has run on
a DHO924S and a DHO804 so far. A report from a DHO802/DHO812, a DHO914/DHO924 without generator or an EDU model makes
that whole hardware variant verified ([docs/models.md](docs/models.md) 2.1).

## Ground rules

- **No vendor code or vendor data.** No firmware images, no code or binaries from any stock or modified firmware, no
  calibration files, no `vendor.bin`. OOS only depends on `libc`, and it stays that way unless there is a strong reason.
- **Say how you know.** A statement about the hardware in `docs/` is "verified on the unit", or "assumed", or "not yet
  verified on hardware". Keep that honest: a wrong "verified" costs someone else days.
- **Be careful with other people's scopes.** OOS never writes the FPGA flash, `/rigol/data`, or the `uboot`, `trust` and
  `parameter` partitions. Changes that would need any of this will not be accepted without a very good case.
- **Keep the analog path unchanged unless that is the point.** `tools/plan-matrix.sh` prints the register sequences of a
  fixed set of acquisitions. Run it before and after a change and compare the output. Say in your pull request whether they differ.
- **No personal data in issues or commits:** no serial numbers, MAC addresses or licence keys.

## A new protocol decoder

A decoder comes as a protocol module: one file in `ui/src/protocols`, one line to register it, the decoder itself
usually in `oos-dsp`. [docs/protocol-modules.md](docs/protocol-modules.md) walks through it.

## Before a pull request

```sh
(cd hal && cargo test)
(cd dsp && cargo test)
(cd ui && cargo test)
(cd fpgad && cargo test)
(cd ui && cargo check --target aarch64-unknown-linux-musl)
```

Tests that need the calibration files of a real unit skip themselves without them. See the Development section of the
[README](README.md). [docs/architecture.md](docs/architecture.md) is the place to start reading.

Text and code comments are in English. Changes to the hardware knowledge go into `docs/`, in the same pull request.

## Licence

By contributing you agree that your work is published under the licence of the project,
[GPL-2.0-or-later](LICENSE).
