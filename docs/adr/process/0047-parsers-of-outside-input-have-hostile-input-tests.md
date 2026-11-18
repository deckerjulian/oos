# ADR 0047: Every parser of outside input has a hostile-input test in `cargo test`

- Status: proposed
- Date: 2026-11-18
- Category: process

## Context

OOS reads what it did not write: setup, waveform, mask and picture files from the save folder and a stick,
`system.conf`, `net.allow`, time zones from a DHCP lease, public keys, calibration files, `vendor.bin`, update
packages; SCPI, HTTP, VXI-11, HiSLIP, mDNS and SNTP from the network (ADR 0025: unauthenticated, on the bench LAN);
the answers of a USB generator and of wpa_supplicant; the frames of the front-panel MCU; and the samples on the probes
and in a loaded reference. The release build ends on a panic (`panic = "abort"`), so a parser that panics on one of
these takes the screen and the keys of the scope with it.

A review on 2026-11-18 found that the reader of OOS's own waveform files (`store::parse_wave_bin`, behind `parse_ff`)
took a time step that is not finite and never checked the start time (fixed since, in a pull request of its own,
#178). Some parsers had a test with damaged input
(`damaged_waveform_files_are_refused_not_a_panic`, the JSON reader, the truncations of the foreign waveform headers);
most had none, nothing asked for one, and none of them checked what a parser accepts.

The constraints: `libc` is the only dependency (ADR 0013); the checks run on a stable tool chain on Linux, macOS and
WSL (ADR 0020, ADR 0033); a test must not depend on how fast the computer is (ADR 0020); `oos-ui`, which holds most of
these parsers, is a program without a library target.

The first run of the tests of this record, on the host on 2026-11-18 (a laptop; nothing ran on a unit): 34 tests over
the parsers of hal, dsp and ui, under a second per crate in `cargo test` (ui 0.7 s, hal 0.13 s, dsp 0.02 s); 13 findings
(the two of `parse_wave_bin` were fixed by #178 since), among them a panic of the power-on picture reader on a header
that ends in a comment, a panic of the QSPI decoder on a record with very large samples, a stack overflow of the device
tree reader on deep nesting, and an endless read from a USB generator that sends empty transfers.

## Decision

Every parser of outside input has a hostile-input test in the test suite of its crate, run by `cargo test` on stable
in every pull request: valid inputs, mutated from a fixed seed, go in; the parser must return, must not panic, and
what it accepts must obey the contract the test states. A new parser of outside input comes with its test. A finding
is fixed in a pull request of its own and stays as a regression input.

How it is done:

- **The generator** is one file, `hal/src/hostile/gen.rs`, which the tests of dsp and ui take with `#[path]` (no
  crate depends on another for it, and the programs do not see it): xorshift64*, seeded from the name of the parser,
  no clock. An input is a valid one changed one to four
  times (bits flipped, cut short, a span doubled, removed or overwritten, a value at the edge of its range put in or
  over a field: 0, -1, the ends of the integer ranges, NaN, inf, 1e308, an empty field, a very long token, bytes that
  are no UTF-8, the tail of another valid input); every sixteenth is bytes of no meaning. The valid inputs and the
  empty input are tried first, as they are.
- **A test** names its parser and states the contract in its doc comment (a trace has a finite `dt` above zero, a
  written setup reads back the same, an answer is a whole frame ...). It has `hostile` in its path, so
  `cargo test hostile` runs them all. It sits in `<crate>/src/hostile.rs` and below, or in the tests of the parser's
  module where it needs private parts. A failure prints the input as hex and as text.
- **How many:** 2000 inputs a parser (fewer for a slow one), under a second per crate. `OOS_HOSTILE_ITERS` raises
  the number; `.github/workflows/hostile.yml` runs each crate for a few minutes once a week and opens an issue when
  one fails. The same number gives the same inputs on every computer.
- **A finding** is not fixed in the test. Its smallest input becomes a test marked `#[ignore = "..."]` that fails
  while the bug is there, and a line of the crate's `KNOWN` list names it; the randomized test steps over that one
  case. `hostile_known_findings_still_stand` runs every such test and fails when one passes: the fix removes the line
  and the `#[ignore]`, so the list only shrinks and the case is tried again from then on.
- **A parser that does not return** shows as a test that does not end, which the time limit of the job ends. No
  watchdog: with `OOS_HOSTILE_TRACE` set every input is printed before it is tried, so the last one printed is the
  one.

## Alternatives

- **cargo-fuzz (libFuzzer).** Stronger: it sees which branches an input reaches and keeps the inputs that reach new
  ones, where mutation alone stops at the first checksum. It needs a library to link: `oos-ui` has none, so its
  parsers could not be reached without restructuring the program. It needs a nightly tool chain and a dependency
  (`libfuzzer-sys`, ADR 0013), and it does not run in `cargo test`. It may come later for hal and dsp, which are
  libraries; the valid inputs of these tests are its corpus then.
- **proptest, quickcheck, arbitrary.** A dependency each, for what 400 lines of test code do here.
- **Examples of bad input only**, written by hand per parser (the state before). They find what their author thought
  of; several findings of the first run were in parsers that had such a test.
- **One crate for the generator.** A fifth crate for test code (as `cargo xtask` in ADR 0020), and the tests of
  `oos-ui` would still have to live in its own tree. One file that three test builds take with `#[path]` costs less.
- **A copy of the generator in each crate**, kept equal by a test. Three files to change for one change, and 442
  lines that only exist to be the same.
- **A watchdog that ends a slow parser.** It would fail under load on a busy runner (ADR 0020).

## Consequences

- A parser of outside input cannot be merged without saying what it promises: the contract is written down once, in
  its test, and tried 2000 times in every pull request.
- The tests are test code only (`#[cfg(test)]`): the programs for the scope stay byte-identical.
- `KNOWN` is debt that everyone sees. A fix has to touch the list, so a finding cannot be fixed and stay stepped
  over, nor come back unnoticed.
- This is not a proof. Mutation without coverage reaches what lies before a checksum or a signature and little behind
  it (calibration files, `vendor.bin`, an update package), and the interpreter is tried one line at a time on one
  screen. A contract in a test is a claim about the parser: one that is too strict shows as a finding that is none,
  and is then corrected in the test, with the reason.
- A stack overflow ends the test process and cannot be caught: a test that can reach deep recursion keeps such inputs
  out before the call (`nesting` in `hal/src/update/hostile_tests.rs`). A parser that is only slow cannot be shown to
  be slow without a clock; such a case is reported and kept out of the run by a named guard.
- Worth a second look when `oos-ui` gets a library target (then cargo-fuzz can reach its parsers), or when a bug in a
  parser is found from outside that these tests ran over.
- The line for `AGENTS.md` (ADR 0018) comes with the pull request that rewrites that file: a new parser of outside
  input comes with a hostile-input test (ADR 0047).

## References

`hal/src/hostile.rs`, `hal/src/update/hostile_tests.rs`, `dsp/src/hostile.rs`, `ui/src/hostile.rs` and
`ui/src/hostile/`, `ui/src/store/hostile_tests.rs`, `ui/src/http/tests/hostile.rs`, the tests `hostile_*` of
`ui/src/hislip.rs`, `ui/src/mdns.rs` and `ui/src/announce.rs`; the generator `hal/src/hostile/gen.rs`;
[.github/workflows/hostile.yml](../../../.github/workflows/hostile.yml). ADR 0013, ADR 0020, ADR 0025.
