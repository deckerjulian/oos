# ADR 0053: The board code runs on a host against a simulated unit

- Status: accepted (2026-11-18, by the maintainer)
- Date: 2026-11-18
- Category: code

Touches the process too: it says when the list `live` of ADR 0042 may shrink.

## Context

ADR 0001 made the hardware layer pure: step lists, applied through `Bus`. That holds for building the lists. It
does not hold for running them. The acquisition thread (`spawn` in `ui/src/board/dho/hw.rs`) is 1212 lines, the
longest function left (issue #120, goals S2 and T1), and no host test runs it. Read on 2026-11-18, main at 604a2771:

- It opens what it needs itself: BAR1, the sample stream, the seven lines of the front-end bus, the generator's six
  lines, the gear lines and the logic-probe line. `Bus`, `AfeBus` and `std::io::Read` exist, but `spawn`, six
  functions and a struct of its modules and six functions of the executor (`hal/src/dho/seq/exec.rs`) name `Bar1`,
  `std::fs::File` and `GpioAfe`. The gear, the probe and the generator's lines are behind no trait.
- It takes time from the system: 11 `Instant::now()` and 13 sleeps in `spawn`, 9 more sleeps in its modules, 5 in
  the executor. A wait for a trigger lasts up to 2 s of real time.
- It is behind `cfg(target_os = "linux")` (36 in `ui/src/board/`): on another host it is not even compiled.

So every change of `ui/src/board/` waits for the scope on the bench, and ADR 0042 keeps the live paths out of what
the comparison with the base may prove. That record names this as the better proof and as a project of its own.

Two models of the unit exist. `boot::sim::Unit` answers the reads of the cold boot, with virtual time
(`boot::Clock`). `seg::sim::Engine` models the record engine and the play path; `seq::acquire_batch` and the
segmented read are generic and run against it in hal's tests. Its samples are a pattern, not a signal, and it has no
clock. The comparison (`tools/compare-base.sh`) runs the screen on the Demo and Replay sources and prints step lists;
it never runs a loop that reads. The survey, with every seam, is in
[board-on-a-host.md](../../board-on-a-host.md) 1 and 2.

## Decision

- **A simulated unit in hal** (`oos_hal::dho::sim`): one model behind the register window, the sample stream, the
  front-end bus, the single lines and a virtual clock. `seg::sim::Engine` moves into it. It takes from the registers
  what decides which bytes come (arm, stop, the frames of a play, lengths, header, compression, roll and scan), and
  answers the reads the loop polls as the pages of `docs/protocols/` describe them. Its samples come from a scene the
  test gives (volts of a channel at a time, trigger instants) through the inverse of the decode, which goes into hal
  next to the decode with round-trip tests. A host run is then: settings, step list, simulated unit, bytes, decode,
  frame, picture.
- **Time is virtual.** The loop and the executor take now and sleep from a clock; the simulated FPGA runs on the same
  one, and every bus access costs a fixed time. A host run uses no thread and no wall clock (ADR 0020).
- **One seam in the production code.** The loop becomes a function generic over a bundle of bus, stream, front-end
  bus, lines and clock, as the cold boot has `boot::Hw`; `spawn` opens the scope's bundle and calls it. The opens and
  the clock move behind the bundle; nothing else moves. Compiled for the one bundle of the scope, with no call
  through a table.
- **The comparison gets a third part.** A test of oos-ui runs the loop against the simulated unit for a fixed set of
  settings and writes a trace (writes, reads, stream reads, virtual time, the frames) and pictures;
  `tools/compare-base.sh` runs it on both sides. The simulator is not in the programs for the scope.
- **`live` shrinks by a record of its own**, after the roll stream and the segmented read run on a host, and only
  for the files whose code the host run reaches.

The stages, each a pull request with its proof, are in [board-on-a-host.md](../../board-on-a-host.md) 6: the
simulator and the inverse codec (no change of the scope's programs, ADR 0041), the seam (one run on a unit), the host
run in the comparison, roll and segments, the record on `live`.

## Alternatives

- **QEMU with a device model of the FPGA.** It would run the real programs with the real kernel and the XDMA driver,
  and so also the paths this record leaves out. The model would be C in another tree, and it would have to act as
  a PCIe endpoint with DMA to be worth more than a `Bus`: far more to write and to keep true than what it adds,
  and no faster to say what a register does. The layer `sim` of `tools/check.sh` already runs scripts of the image
  in containers and virtual machines, without an FPGA; a device model can be added there later and would reuse this
  model's answers.
- **Recording the bus traffic of a unit and replaying it.** True to the unit at the moment of the recording. But a
  recording answers only the sequence that was recorded: a refactoring that reorders two reads, or any new setting,
  falls off it. Recordings hold the samples and the calibrated values of one unit, which do not enter the tree
  (ADR 0013), so CI could not use them. Single observations from a unit stay what the model is written from, as
  behaviour in `docs/protocols/`.
- **The unit as the only proof** (today). Sound, and the reason `spawn` is still one function: each step of a split
  costs a run on the bench, and the roll time bases fail there for any build that changes `ui/src/board/` (#113).
- **A trait object for the bundle.** Simpler signatures, and a call through a table for every register access and
  every look at the clock in the loop that waits for frames. The generic form costs one type parameter.
- **The simulated unit as a source of oos-ui** (`--script`, `--serve`). The scripts of `ui/tests/` and
  `tools/device-test.py --sim` would then run through the loop. It puts the simulator into the scope's programs, so
  every change of it would wait for a unit, or behind a Cargo feature and a second build variant. Left open: it can
  follow without changing what is built before (board-on-a-host.md 8).

## Consequences

- A refactoring of the acquisition loop can be proven on a host, so `spawn` can be split (goal S2) without the
  bench. The loop compiles and is tested on every host, macOS included.
- A host run proves that the code and the pages of `docs/protocols/` agree. It does not prove the pages. Where a page
  says assumed or not yet verified, the simulator answers the assumption; the model says for each answer how it is
  known (ADR 0015).
- Only a unit still proves: timing against the FPGA, DMA and the driver, what the front-end, ADC and trigger writes
  do to a signal, calibration, the start and the update, the stray frames 0 and 1 of a batch and the roll rate
  between 5 and 100 ms/div, whose causes are not known. `hal/src/dho/bus.rs`, `gpio.rs`, `panel.rs`, `board.rs`,
  `selfcal.rs`, `coretrim.rs`, `levels.rs`, the scope's side of the bundle and the commands of `oos-fpgad` that work
  on the board stay in `live`.
- The seam itself changes `ui/src/board/` and the executor: it needs its run on a unit, and it is the one step of
  this plan that can break the scope.
- A change of how the unit behaves has two homes from now on: the page in `docs/protocols/` and the model. A new
  path of the loop comes with its answers in the model, or the host run does not reach it; the measure of goal T1
  counts the paths reached (0 of 28 today, 17 with the roll stream and the segments).
- The comparison builds a test binary of oos-ui on each side: more minutes per pull request, to be measured.
- The model is code that never runs on a scope, in hal. It follows hal's rules (pure, `libc` only) and must stay out
  of the scope's programs; the job `scope` shows it.
- Worth a second look when a refactoring that the host run found equal had to be reverted after a run on a unit, or
  when the model needs more lines a year than the runs on the bench it saves.

## References

[board-on-a-host.md](../../board-on-a-host.md) (the plan: seams, design, stages, measure, open questions); issue #120
(goals T1 and S2); ADR 0001, ADR 0014, ADR 0015, ADR 0020, ADR 0032, ADR 0041, ADR 0042; `ui/src/board/mod.rs`,
`ui/src/board/dho/hw.rs`, `hal/src/dho/bus.rs`, `hal/src/dho/seq/exec.rs`, `hal/src/dho/seg.rs`,
`hal/src/dho/boot/sim.rs`, `hal/src/dho/madeup.rs`, `ui/src/wave.rs`, `tools/compare-base.sh`,
`tools/plan-matrix.sh`, `tools/pr-gate.sh`; [acquisition-start.md](../../protocols/acquisition-start.md),
[record-play.md](../../protocols/record-play.md), [roll-mode.md](../../protocols/roll-mode.md).
