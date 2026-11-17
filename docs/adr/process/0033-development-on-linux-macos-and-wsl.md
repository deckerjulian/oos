# ADR 0033: OOS is developed on Linux, macOS and Windows with WSL; there is no native Windows build

- Status: proposed
- Date: 2026-11-17
- Category: process

## Context

Since 2026-11-13 the tree had a second way to run `oos-ui`: a desktop window for Windows, with two PowerShell
scripts, the crates `minifb` and `windows-sys`, and a branch of the code for every place where Windows differs from
Unix. ADR 0021 allowed it, because the port existed and was used.

What it costs, counted on 2026-11-17 (commit 09248b1):

- 1 609 lines of Rust and PowerShell that run on no scope: the window, its input, its file dialog, the network code
  for Windows, and a stand-in for every Unix call (sockets, `statvfs`, `flock`, usbfs, signals).
- 701 lines of `ui/Cargo.lock` for five crates that only this build pulls in. ADR 0013 names `libc` as the only
  dependency; ADR 0021 had to make an exception.
- A second set of rules: no check of the tree builds or tests the Windows path (CI runs on Linux), so a change could
  break it without anyone seeing, and items of the user interface that only the window used had to be kept alive
  with `allow(dead_code)`.
- Two ways to do the same thing on a computer: the window, and `oos-ui --serve` with the web remote, which works on
  every host and is what the tests use.

The scope runs Linux. A Unix host builds the same code with the same tools as CI and as the unit.

## Decision

- **OOS is developed on Linux, on macOS, and on Windows inside WSL 2.** These are the hosts on which
  `tools/check.sh` has to pass. CI runs Ubuntu 24.04, the system a current WSL 2 installation gives.
- **There is no native Windows build.** `build-windows.ps1`, `run-windows.ps1`, the desktop window
  (`ui/src/desktop.rs`, `ui/src/desktop/`), the network code for Windows and the stand-ins for hosts that are not
  Unix are removed, and so are the crates `minifb` and `windows-sys`.
- **On a computer, OOS is tried with `oos-ui --serve`** and the web remote in a browser: the same screen, touch, keys
  and knobs, on every host.
- **`libc` is the only dependency of the four crates again**, on every host (ADR 0013). `tools/check.sh target`
  checks it as before.
- Tools for people who use a scope and never build OOS (backing up a card, checking a download) keep their
  instructions for Windows.

## Alternatives

- **Keep the Windows build and test it in CI.** A Windows job would catch build breaks, not the behaviour of a window
  nobody tests; the second code path and the extra crates stay.
- **A desktop window for every host instead.** A real improvement for trying OOS, but a project of its own; it would
  need a decision record on which crates it may use. The web remote does the job today.
- **Move the window into a repository of its own.** Possible for whoever wants to keep it; it can build on `oos-ui`
  as a library or talk to `--serve`. Nothing here stands in the way.

## Consequences

- The tree loses about 1 600 lines of code and 700 lines of lock file, and every `cfg(windows)`, `cfg(unix)` and
  `cfg(not(unix))` in the four crates.
- Someone who used the window on Windows installs WSL 2 and uses a browser. The mouse wheel shortcuts, the keyboard
  shortcuts and the native file dialog of the window are gone; the web remote takes files by selection and
  drag-and-drop.
- A few items of the user interface were only used by the window and are now only used by tests (the wheel event,
  the destination dialog of a waveform load, the panel button). They stay for now and can be removed in a change of
  their own, which the comparison of pictures can prove.
- WSL has not been tried by the maintainer: that `tools/check.sh` passes there is assumed from CI, which runs the
  same Ubuntu. Reports from WSL are welcome.
- When this record is accepted, ADR 0021 becomes "superseded by ADR 0033" and ADR 0013 "accepted" without the
  amendment.

## References

ADR 0013, ADR 0021, `ui/Cargo.toml`, `ui/src/main.rs` (`--serve`), [CONTRIBUTING.md](../../../CONTRIBUTING.md),
[README.md](../../../README.md).
