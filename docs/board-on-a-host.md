# The board code on a host: plan

**This page is a plan, not a description of what exists.** It belongs to
[ADR 0053](adr/code/0053-the-board-code-runs-on-a-host-against-a-simulated-unit.md) and to goal T1 of
issue #120: the board code runs on a host against a simulated FPGA, so the comparison with the base covers the live
paths. Sections 1 and 2 describe the tree as it is (read on 2026-11-18, main at 604a2771). Sections 3 to 8 say what
is to be built. A name given as a Rust path without a file (`oos_hal::dho::sim`, `hw::run`, `hw::Port`) is planned
and not there yet. When a stage is merged, its part moves to [architecture.md](architecture.md) and the pages of
`docs/protocols/`, and leaves this page.

Statements about the unit say how they are known and name the page they are taken from (ADR 0015). Nothing here was
tried on a unit for this plan.

## 1. What the live paths touch outside the program

The acquisition thread is `spawn` in `ui/src/board/dho/hw.rs`: 1212 lines (`tools/code-metrics.py`), one closure
handed to `std::thread::spawn`, about 50 locals of loop state, 13 `continue` in one loop. It gets four arguments
(`app_mask`, `program_afe`, `cal_dir`, `start_up`); everything else below it constructs itself or takes from a
global. `spawn` and what it calls are behind `cfg(target_os = "linux")` (36 in `ui/src/board/`, 30 in `hal/src/dho/`):
on a host that is not Linux the loop is not compiled, on a Linux host it is compiled and never run by a test.

### 1.1 Devices

| What | Opened by | Where | Behind a trait today |
|---|---|---|---|
| `/dev/xdma0_bypass`: BAR1, 32 KiB mapped | `Bar1::open()` | in `spawn`, after the wait for the start-up | `Bus` (`Bar1`, `Recorder`, two models). `spawn`, six functions and a struct of `hw/` and six functions of `hal/src/dho/seq/exec.rs` still name `Bar1` |
| `/dev/xdma0_c2h_0`: the sample stream | `std::fs::File::open` | in `spawn` | `std::io::Read` in `DmaBuf::read`, `acquire_batch*`, `execute_bus*`, `read_raw_again`, `seg::*`; `std::fs::File` by name in the functions above |
| seven GPIO lines of the front-end SPI (`/dev/gpiochipN`, line-handle ioctls) | `afe::GpioAfe::open()` | in `spawn`, with `--afe` | `AfeBus`; `execute_into_until` and `selfcal::Unit` name `GpioAfe` |
| the gear lines `afe1` to `afe4` (or the ioctl of `/dev/dac_gear_change`) | `afe::set_gear` | a free function called inside `execute_into_until` and `selfcal::Unit`; the lines live in a `static` | no |
| six lines of the generator (relays, power) | `afg::Io::open()` | in the loop, at the first generator request | no (`Io::apply` takes `&mut dyn Bus` for its register writes) |
| the line `LA_PROBE_DETECT` | `la::probe_present()` | in the loop, once a second, opened for each read | no |

The functions that name `Bar1` and `std::fs::File`: `disarm` (`hw/plan.rs`), `seg_abort` and `seg_step`
(`hw/seg.rs`), `levels_measure` and `counter_tick` (`hw/units.rs`), `UnitTrim` and `core_trim` (`hw/trim.rs`); in
hal `execute`, `execute_until`, `execute_into_until`, `prearm`, `disarm`, `acquire_la` and `selfcal::Unit`.

Not touched by the live paths: the h2c channel of XDMA, `/dev/mem`, spidev (only the BOOT command of the cold boot,
`hal/src/dho/boot/io.rs`). The panel UART belongs to the input thread (`ui/src/input.rs`), the thermal zones of
sysfs are read by the System info page through `Board::thermal_zones`; neither is the acquisition thread. The debug
source `ui/src/board/dho/fpga.rs` (`--fpga-live`) opens BAR1 and the stream the same way in a thread of its own.

### 1.2 Time

| Where | `Instant::now()` | `.elapsed()` | sleeps |
|---|---|---|---|
| `spawn` | 11 | 7 | 13, all but one from 2 ms to 500 ms |
| `hw/seg.rs`, `hw/units.rs`, `hw/trim.rs` | 2, and an `Instant` in `TrimPending` | 1 | 9 |
| `hal/src/dho/seq/exec.rs` | 2 | 5 | 5: 200 us between two looks at the frame ring, 500 us per poll, `Step::Sleep`, 100 us on each side of an ADC word |
| `Bar1::wr` | | | 10 us after every write (`gap_us`) |
| `GpioAfe` | spins 5 us per half clock | | |
| `afg::Io::apply`, `selfcal::Unit` | | | `Op::SleepMs`; the calibration's waits |

Time is handed in at five places already, in three ways: `levels::run(bus, sleep_us)` and
`adc::measure(bus, sleep_ms)` take a closure, `selfcal::CalHw::sleep_ms` and `coretrim::TrimHw::wait_ms` are trait
methods, the cold boot has `boot::Clock` (`SysClock`, `sim::SimClock`). None of them reaches the loop or the
executor.

### 1.3 Files, switches, environment, globals

- **The start-up status file** (`Spawn::start_up`): read every 100 ms until its `end` line (`oos_hal::bootstatus`).
- **Calibration:** the folder of `Spawn::cal_dir` (`--cal`), read by `afe::Cal::load_from`, `adc::CalAdc::load`,
  `la::Cal::load` and `afg::Cal::load`, again after a self-calibration. Each looks in the constant `/oos/cal` first.
  `selfcal::save` and the core trim (`cal.core_offset = tight`) write to `/oos/cal`.
- **Switches of `/oos/system.conf`**, each read once into a `static` (12 `OnceLock` in `hw/`): `acq.prearm`,
  `acq.frame0`, `zone.batch`, `trig.serial_hw`, `trig.hw_status`, `dvm.hw`, `mask.hw`, `zone.hw`, `mask.map.*`,
  `cal.core_offset`.
- **Environment:** `OOS_PREARM`, `OOS_FRAME_HEADER` (which also writes `/tmp/oos-hdr.log`), `OOS_HWAVG`,
  `OOS_TRIG_HW_STATUS`, `OOS_CAL_DEBUG`.
- **Globals:** `oos_hal::model::platform()` (three uses in `spawn`), `drawpipe::pipe()` with its deposit workers and
  the read-out buffers lent to them, `drawpipe::counters()`, `compress::note_batch_k`.
- **Threads:** the one thread, placed by `placement::place`; `HwShared` (`Arc<Mutex<Shared>>`, 41 lock sites in
  `spawn`) with the UI's side in `ui/src/wave.rs` (`acquire_hw`, `Source::hw_*`).
- **Output:** the status line in `Shared`, log lines on stderr.

## 2. What there is to build on

- **`boot::sim::Unit`** (`hal/src/dho/boot/sim.rs`, 237 lines): a `Bus` that answers the reads the cold boot steers
  on (version, 0x1008 after a number of busy reads, the ADC clock counter, the eye scan by the delay last written, the
  ADC's read-back with the trim done after a number of polls) and records every write and read. It is scripted by
  public fields set before the run, and by counts of reads, not by time. Time is `sim::SimClock`: `sleep_us` adds,
  `now_us` returns the sum; the boot takes time only through `boot::Clock`, in the bundle `boot::Hw { bus, clock,
  pll, afe, early }`. `boot::dry_run` prints the whole boot against it (`oos-fpgad cold-boot`, a section of
  `tools/plan-matrix.sh`).
- **`seg::sim::Engine`** (in `hal/src/dho/seg.rs`, 245 lines; named `dho::sim::Engine` too since stage 0b): a model of
  the record engine and the play path. Arm, frames recorded per read of 0x4080, progress in 0x4010 and 0x4014, 0x4034,
  stop, the play of frames `first..=last` into a byte stream with the frame header and its stamp, the FPGA compression
  (`pairs::point`), the hardware average (`avg::mean_words`), the stamp latch. `SimBus` and `SimC2h` are its two sides.
  `seq::acquire_batch` and all of `seg::*` are generic and run against it in the tests of `hal/src/dho/seg.rs` and
  `hal/src/dho/avg.rs`. Its sample words are a pattern (the frame's number in the high byte), not a signal; it has no
  clock and no stream padding.
- **The demo source** (`ui/src/wave.rs`): `demo_signal(cfg, ch, t)` gives the volts of a channel at a time (the
  probe-compensation square, the demo lines of the protocol modules, the generator looped back). `acquire_demo`
  samples it on the screen grid, triggered by oos-dsp's software trigger, with jitter and noise from a seeded
  generator, and makes a `Record` at once: no `Frame`, no bytes. **Replay** turns the words of one captured CH1 frame
  and its preamble into volts and a `Record`.
- **Bytes to a frame:** `hw/decode.rs` (`decode_with`: the 16-byte header, `seq::decode_volts_into` or
  `seq::deinterleave` with the core offsets, then the frame's fields), then `board::to_record` (the software trigger
  nearest the hardware point, the screen window). The codec is `hal/src/dho/seq/samples.rs`, the sizes are
  `hal/src/dho/seq/frame.rs` (`frame_bytes`, `frame_stride`, `read_len`, `unpad_in`, `window`), the pairs
  `hal/src/dho/pairs.rs` (`model`), the roll chunks `roll::split`, `split_la` and `counter`. There is no encoder: the
  pair model and `Engine` are the only code that makes data in the unit's format.
- **`tools/plan-matrix.sh`:** 99 sections of `oos-fpgad plan` and the other dry runs, 11 894 lines without
  calibration files and 12 460 with those of one unit (ADR 0032, 2026-11-17). It prints step lists; nothing is run,
  and no read is answered except in the cold boot's dry run.
- **`tools/compare-base.sh`:** the base in a tree of its own; `ui/tools/layout-check.sh` runs the release build of
  `oos-ui --script` for the 23 scripts of `ui/tests/` at three text sizes and compares pictures, stored files and
  the answers to remote commands; then `plan-matrix.sh` on both sides with the same calibration set (a unit's, or the
  made-up one of `oos-fpgad made-up-cal`, `hal/src/dho/madeup.rs`). Every script runs on the Demo or the Replay
  source; `Board::spawn` never runs, and the comparison runs no test binary.
- **`tools/device-test.py --sim`:** starts `oos-ui --serve` (the demo source) on this computer and runs the device
  tests that need no scope over SCPI and the web page. It reaches the UI and the interpreter, not the board code.
- **Host tests of the board code:** `ui/src/board/dho/hw/tests.rs` (1164 lines) tests what `spawn` calls (plans,
  settings, guards, the decode); the loop itself has no test.

## 3. The simulated unit

### 3.1 Shape

One model, `oos_hal::dho::sim::Unit`, in hal: no hardware, no dependency on oos-dsp or oos-ui (ADR 0001). The loop gets
it through handles for its sides (register window, stream, front-end bus, single lines, clock) that share the model, as
`SimBus` and `SimC2h` share `Engine` today. `Engine` is named in its module first (stage 0b, `hal/src/dho/sim.rs`) and
is not copied: its tests keep running, its word pattern becomes one of the scenes below. `boot::sim::SimClock` is its
clock. `boot::sim::Unit` stays as it is: it answers another phase of the unit's life, and joining the two is worth it
only when a host run starts at power-on.

### 3.2 What it is told, and what it reads from the registers

- **Read from the registers written**, as the pages of `docs/protocols/` describe them and as `Engine` reads them
  today: arm and stop (0x4004, 0x400c), the frames of a play (0x4010, 0x4014, 0x4018), the length sent (0x11f0), the
  read-out window, the frame header (0x1010 bit 31), the compression (0x1170), the average (0x1004 bit 3, 0x10b0,
  0x10b4), the stamp latch (0x40b8), the channel set (0x4008 bits 3:0), the roll and scan bits (0x4008 bits 17 and
  22), the fill-level strobe (0x1010 bit 28).
- **Told by the test, as a scene:** the volts of each channel at a time, the instants at which the signal triggers,
  and the view: volts per division and offset of each channel, sample period, record length and pre-trigger of the
  settings the test asks for. A new view takes effect at the next arm.
- **Recorded and not interpreted:** the front ends, the ADC, the read-path gain, the trigger unit and the time base.

The reason for the split: reading the front-end, trigger and time-base registers back into volts and seconds would be
a second implementation of pages on which much is marked assumed, and the simulator would confirm an assumption
with itself. The writes of those blocks are compared line by line by `tools/plan-matrix.sh` already, the calibrated
ones included. What the simulator takes from the registers is what decides how many bytes come and which frames
they are. Each block it later reads itself (first the record length and the pre-trigger) makes the run independent of
the code under test in one more respect.

### 3.3 The reads it answers

| Read | Answer | How the unit's behaviour is known |
|---|---|---|
| 0x4000 | the version hal was written against; a test sets another and sees that nothing is written | `bus::FPGA_VERSION` |
| 0x4034 | bit 13 while nothing records, bit 4 while armed | observed values in acquisition-start.md, "0x4034 status"; bits 0 to 2, 8, 9, 12 and whether 6 and 7 stay set: not yet verified on hardware |
| 0x4080 bit 30 | set when the first frame of the armed batch is recorded | verified on the unit (acquisition-start.md, "Batches of frames on the unit") |
| 0x4010, 0x4014 | last frame recorded, first frame | same section; record-play.md 1.3 |
| 0x40b8, 0x40b0/b4, 0x40c0/c4 | the stamps: the latch holds the trigger time of the last frame played in ticks of 0.8 ns, the header of frame k that of frame k - 1 | unit 2026-11-13, as the head of `hal/src/dho/seg.rs` states it |
| 0x11e8 | 27 for each whole chunk that waits | verified on the unit (roll-mode.md 8); the strobe before the read is assumed (roll-mode.md 2.2) |
| 0x1204, 0x40d0 | scan: bit 31 and the column at the trigger; done a few chunks before the last column | verified on the unit (roll-mode.md 9.1); a trigger position other than 0 in 0x4058: not yet verified on hardware |
| 0x1438 | a fixed word inside the valid range | board.md 2.3 |
| 0x1040, 0x1044, 0x1048 | not in the first stages (measure unit) | self-cal.md 2.4; which slot shows which channel is assumed (`hal/src/dho/levels.rs`) |
| any other | the value last written, else 0 | invented: some registers of the unit do not read back (record-play.md 6.0) |

### 3.4 The stream it sends

| What | Format | How it is known |
|---|---|---|
| the frames of a play | frames `first..=last`, each the words of the window for all channels, a 16-byte header first when 0x1010 bit 31 is clear (word 4 the frame's number, words 5 to 7 the stamp), each frame padded to 8 bytes | header word 4: verified on the unit (acquisition-start.md, "Frames 0 and 1 of a batch"); the padding: found on the unit for one channel, assumed for two ("C2H read rules found on the unit" 1) |
| the words | the scene's volts as codes: 32768 at the channel's offset, 7500 a division (`hal/src/dho/readout.rs`), interleaved as `seq::deinterleave` reads them | afe-settings.md 4.1, horizontal-config.md 2.2 |
| compressed frames | min/max pairs of 2k samples in the order the extremes occur (`pairs::model`) | verified on the unit, as the head of `hal/src/dho/pairs.rs` states it |
| averaged frames | `avg::mean_words` over the triggers since the last clear | the averaging in the FPGA memory is assumed (horizontal-config.md 3) |
| roll and scan chunks | 216 bytes: 8 header words (word 1 bit 1 marks a logic-analyser chunk, word 4 counts from 1, in scan from 1 in every sweep), 100 words; one chunk per 100 / ratio points at the spacing of `roll::params` | verified on the unit (roll-mode.md 8 and 9.1, "Scan with the LA on"; the marker bit as la.md 4.4 describes it); that roll points are min/max pairs is assumed (roll-mode.md 3.3) |
| a read with nothing played | fails at once | not modelled: on the unit such a read ends with the driver's time-out (architecture.md, "Lessons from the first device tests") |

The bytes are made by the inverse of the decode, in hal beside it (`hal/src/dho/encode.rs`, since stage 0a): volts to a
code, channels to interleaved words, the padding, the header, a chunk. It is built for the tests only (`cfg(test)` and
the feature `sim` of hal, since stage 0b), so the scope's programs stay the same programs. Round-trip tests hold the two
together for what the decode keeps whole (raw words, padding, header, roll chunk): bytes, decoded and encoded again, are
the same bytes for every channel set, with each of the 65536 codes in every word of a group; volts, encoded and decoded,
come back within half a code. Compressed frames have no inverse, since a block keeps only its extremes: the model makes
them with `pairs::model` from the scene's samples, and a test checks that their decode gives the extremes of each block
of the scene, in the order they occur.

So a host run goes: settings, step list, simulated unit, bytes, decode, `Frame`, `Record`, picture.

### 3.5 Time

- **One virtual clock** in the model, in microseconds. A sleep adds its time. Every register write costs 10 us (the
  `gap_us` of `Bar1`), every read 1 us, a stream read its bytes at 100 MB/s. These costs are invented (the unit read
  775 000 bytes in 8.6 ms, acquisition-start.md); they make time pass in a loop that never sleeps, and the same on
  every computer.
- **The FPGA runs on the same clock.** After an arm a frame is recorded at each trigger instant of the scene, or
  after an invented auto time-out without one; the done flag, 0x4010 and the roll fill level are functions of the
  clock. A wait of 2 s for a trigger that never comes takes no time in a test.
- **No thread.** The test calls the loop itself; the loop asks a closure before every pass, and that closure is the
  UI's side: it sets `Shared::want`, takes the frames, counts the passes and ends the run. Nothing waits on the wall
  clock and nothing depends on the scheduler (ADR 0020).
- **The trace** a run leaves: every write, every read with its answer (repeats of a poll folded), every stream read
  with its length, the virtual time at each configuration, arm and read, and for every frame delivered its fields and
  a checksum of its samples.

## 4. The seam in the production code

One bundle of what the loop needs from a unit, as the cold boot has `boot::Hw`:

```rust
pub trait Port {
    type Bus: Bus;              // the register window
    type Stream: std::io::Read; // the sample stream
    type Afe: AfeBus;           // the four front ends
    type Lines: Lines;          // gear, logic probe, generator relays and power
    type Clock: Clock;          // now and sleep
}
pub struct Hw<P: Port> {
    pub bar: P::Bus,
    pub c2h: P::Stream,
    pub afe: Option<P::Afe>,
    pub lines: P::Lines,
    pub clock: P::Clock,
}
```

`spawn` keeps its signature and its thread: it waits for the start-up, opens the unit's bundle and calls
`hw::run(hw, options, &shared, || true)`. `run` is the loop as it is today, generic over `Port`, with the closure
asked before every pass. One type parameter, compiled for the one port of the scope: no call through a table in
the read path.

**What moves behind the seam** (into the scope's port, a new module next to `hw/plan.rs`, the only part left behind
`cfg(target_os = "linux")`):

1. `Bar1::open()`, `File::open` of the stream, `afe::GpioAfe::open()`;
2. `afe::set_gear`, `la::probe_present()`, `afg::Io` (open at the first request, relays, power): the `Lines` of
   the port;
3. every `Instant::now()`, `.elapsed()` and sleep of 1.2 in the loop, its modules and the executor: the clock. The
   stored instants (`published`, `xadc_at`, `done_seen`, `deep_last`, `comp_last`, the counter, the DVM,
   `TrimPending`) become microseconds of the clock.

In hal, `execute`, `execute_until`, `execute_into_until`, `prearm`, `disarm`, `acquire_la` and `selfcal::Unit`
become generic like `acquire_batch` already is, and the executor takes the clock: the cold boot's `boot::Clock`,
used where it is, so the start is not touched. The functions `oos-fpgad` calls keep their names and arguments as
wrappers with the system clock, so `fpgad/` is not touched either.

**What stays where it is in the first stages:** the calibration folder (a host run hands in the made-up set), the
switches of system.conf and the environment (a host run sees the defaults), `model::platform()`, the batch draw
path's workers (off in a host run), the thread placement (in `spawn`, not in `run`), the log lines.

**What stays the same, and what does not:** the step lists (every line of `tools/plan-matrix.sh`), the pictures, and
by construction the order of the bus accesses. The programs for the scope do not stay byte-identical: the seam is a
change of `ui/src/board/` and of the executor and needs its run on a unit. The split of the loop into functions (goal
S2) is not part of the seam; it comes after stage 2, when a host run can prove each step.

## 5. What a host run cannot prove

- **Timing against the hardware:** whether 10 us between writes, the poll periods, the 25 ms for the frame ring and
  the time-outs fit the FPGA; frame rate and load (`tools/device-test.py` on a unit, ADR 0014).
- **DMA:** the XDMA driver, the alignment and the pages of `DmaBuf` (the heap corruption of 2026-11-04), short reads
  and split reads beyond what the tests of `hal/src/dho/bus.rs` feed, a read that stalls because the length asked
  for does not fit the window.
- **Register semantics that are assumed:** the simulator answers as the pages describe. Where a page says assumed
  or not yet verified, the simulator's answer is that assumption, and a run against it proves only that the code and
  the page agree.
- **The analog path:** what the front-end, ADC and gain writes do to a signal, calibration, the core offsets, the
  trigger unit's levels and hysteresis. The scene's volts become codes by the settings asked for.
- **What the unit does that nobody can predict yet:** the stray frames 0 and 1 of a batch (cause not known,
  acquisition-start.md), the roll stream that ran too fast between 5 and 100 ms/div (cause not known, roll-mode.md
  8). A scene can inject such a frame to test the guard; it cannot say when the unit makes one.
- **The system around the loop:** the GPIO lines being free after the cold boot, the kernel, the thread placement,
  the deposit workers of the batch draw path, the start-up wait.
- **Self-calibration, core trim, levels:** their logic has host tests with stand-ins (`CalHw`, `TrimHw`); whether
  they converge on the unit's noise only a unit shows.

So the list `live` of `tools/pr-gate.sh` keeps: the start and the update, the commands of `oos-fpgad` that work on
the board, `hal/src/dho/bus.rs`, `gpio.rs`, `panel.rs`, `board.rs`, `selfcal.rs`, `coretrim.rs`, `levels.rs`, the
scope's port and `ui/src/board/dho/fpga.rs`.

## 6. Stages

Each stage is one or two pull requests that can be proven by themselves. Sizes are estimates of changed lines.

| Stage | What | Files | Size | Proof | Risk |
|---|---|---|---|---|---|
| 0a | the inverse codec: volts to code, interleave, padding, header, roll chunk; round-trip tests | `hal/src/dho/encode.rs` (new, `cfg(test)`) | 100 and 150 of tests | host tests; the comparison finds nothing; the module is built for the tests only, so the job `scope` finds the programs byte-identical and `unit` passes by ADR 0041 (the first try put the additions into the decode's files: the programs differed, as the lines of the code after them moved) | low |
| 0b | the simulated unit for triggered acquisitions: the model with clock, scene and trace; `Engine` moves into it. Two pull requests: first the module `sim` behind the feature `sim` of hal (switched on by hal's dev-dependency on itself), which names `Engine`, then the model (`sim::Unit` with its `Scene`, `View`, trace and clock `Time`; `seq::acquire_batch` reads batches, compressed ones too, against it in the tests of `sim.rs`). Moving `Engine`'s code out of `seg.rs` changed the programs for the scope although they never call it, so it moves with stage 1, which runs on a unit anyway | `oos_hal::dho::sim` (new), `hal/src/dho/seg.rs`, `hal/src/dho/avg.rs` (their tests) | 450 and 250 of tests | as 0a; `seq::acquire_batch` and `seg::*` run against it in hal's tests | low for the scope; the model can be wrong, see 5 |
| 1a | the executor takes any bus, stream, front-end bus, lines and clock; `selfcal::Unit` and the generator's executor too | `hal/src/dho/seq/exec.rs`, `hal/src/dho/selfcal.rs`, `hal/src/dho/afg.rs` | 200 | `Refactor:`, `Behaviour: unchanged`, the comparison finds nothing; time-outs tested in virtual time; one run on a unit | medium: the executor runs live with its timing. `exec.rs` is in `live` (`tools/pr-gate.sh`, ADR 0052), so this is a refactoring of a live path and the gate asks for the run on a unit |
| 1b | the seam of section 4: `Port`, `Hw`, `hw::run`, the scope's port | `ui/src/board/dho/hw.rs`, `hw/plan.rs`, `hw/seg.rs`, `hw/units.rs`, `hw/trim.rs`, `ui/src/deepmeas.rs`, a new module for the port | 300 | as 1a; `tools/device-test.py --changed` on the unit (for `ui/src/board/`: acquisition, roll, trigger, front end, register sequences), and its frame rate against main on the same unit | medium: a unit of time mixed up turns 33 ms into 33 s. No lines move, so the diff can be read |
| 2 | the host run: a test of oos-ui runs `hw::run` against the simulated unit for a matrix of settings and N records each, checks the frames against the scene and writes the trace; the comparison gets a third part | a test module of `hw/`, `tools/compare-base.sh`, `.github/workflows/ci.yml`, `tools/README.md`, `docs/architecture.md` | 400 and 60 in the tools | host tests (layer `test`); a test and tools only, so the programs for the scope are expected byte-identical (job `scope`); pictures and register sequences of the comparison equal, the new part reported as new | low for the scope; the run must not flake: virtual clock, one thread, no order taken from a hash map |
| 3 | the roll stream, scan (RUN and SINGLE) and the segmented read in the model and in the matrix | `oos_hal::dho::sim`, the test module of stage 2 | 500 | as stage 2; new lines only in the live part | low |
| 4 | what leaves `live`, and the record that says so | a new decision record that amends ADR 0042, `tools/pr-gate.sh`, `AGENTS.md` | 80 | documents and a tool: no unit; decided as a record (ADR 0031) | a file leaves the list although the run does not reach its code: section 7 counts what is reached |

**Stage 2, the comparison.** The matrix follows `tools/plan-matrix.sh`: one, two and four channels; a short time
base with batches, 1 ms/div, a deep record through the compression; AUTO, NORMAL without a trigger, SINGLE; new
settings during a wait; STOP with a view read again. The test writes its trace and one picture per run into a folder
named by an environment variable. `tools/compare-base.sh` runs it in the base's tree and in this one and compares:
`live.diff`, and one more line in `summary.md`. The pictures and the register sequences are made as before, so their
output does not change by a byte. A base from before the test has no live run: the part is then left out on both
sides and the summary says so, as the comparison does for a base from before the made-up calibration set. The
comparison keeps an artifact that says the live run was compared, as `compare-calibrated` says for the calibration.
The cost is a test build of oos-ui in each tree, to be measured in that pull request.

**Stage 4, what leaves `live`.** For a pull request titled `Refactor:` or `Lints:` that says `Behaviour: unchanged`,
where the comparison ran the live run on both sides and found nothing: the files of `ui/src/board/` whose every
path the live run reaches, as section 7 counts them, and `hal/src/dho/roll.rs`, `hal/src/dho/seg.rs`,
`hal/src/dho/seq/exec.rs` once that holds for them too. A file with a path the run does not reach stays in `live`:
the scope's port, `fpga.rs`, and the files of the paths left for later below (the generator, the DVM and the counter,
levels, core trim, mask and zone, arming ahead, self-calibration). Only then is
`spawn` split into functions (goal S2), each step proven by the comparison.

**Later, not part of goal T1:** the switches as options of the loop, so the opt-in paths run on a host (arming
ahead, the mask and zone units, the DVM, the core trim); the measure unit in the model, so levels, core trim and
self-calibration run in the loop; the simulated unit as a source of host builds of oos-ui (section 8, question 1).

## 7. The measure of goal T1

"Live paths run by host tests" counts these paths of the loop. Today: 0 of 28.

| Path of the loop | Run by a host test from |
|---|---|
| start: version check, calibration loaded | stage 2 |
| configuration on a change, with the front-end steps | stage 2 |
| a batch: arm, wait, ring, read, decode, queue | stage 2 |
| SINGLE: one frame, then nothing armed | stage 2 |
| NORMAL without a trigger; new settings during the wait | stage 2 |
| frames left out: the first batch after a change, frames 0 and 1 | stage 2 |
| STOP: the hold and the final frame read raw | stage 2 |
| STOP: a view read again from the stored record | stage 2 |
| the newest frame of a compressed batch read raw | stage 2 |
| the screen window of a deep record for the measurements | stage 2 |
| the board readings once a second (XADC, logic probe) | stage 2 |
| the force-trigger pulse, the trigger status of a batch | stage 2 |
| hardware segments: record, progress, stamps, read back, one segment shown, abort | stage 3 |
| the roll stream | stage 3 |
| scan in RUN: done, pause, next sweep | stage 3 |
| scan in SINGLE: halt and the next press | stage 3 |
| roll and scan with logic-analyser chunks | stage 3 |
| hardware average and its clear | later (the model has it from `Engine`) |
| the logic-analyser read of a record | later |
| the generator | later |
| the frequency counter, the DVM block | later |
| levels of the measure unit, the slot dump | later |
| the core trim | later |
| the mask and zone units, the stall guard | later |
| arming ahead and disarming | later |
| self-calibration | later; on a unit for whether it converges |
| the batch draw path's hand-over to the workers | a unit |
| the wait for the start-up | a unit |

The target of T1, the acquisition loop and the roll stream, is reached with stage 3: 17 of 28.

## 8. Open questions for the maintainer

1. **Where the simulated run lives.** Proposed: a test of oos-ui that `tools/compare-base.sh` runs (nothing of the
   simulator in the scope's programs, so a change of the simulator passes `unit` by ADR 0041; two more test builds in
   each comparison). The other way: a source of host builds of oos-ui behind a Cargo feature. Then the scripts of
   `ui/tests/`, `oos-ui --serve` and `tools/device-test.py --sim` run through the real loop, and the pictures come
   from `ui/tools/layout-check.sh` as they are; the price is a build flag in the tools and a second variant to
   check. It can follow stage 3 without changing what is built before.
2. **The scene tells the simulator the vertical scale and the time axis** (3.2). Which block should it read from the
   registers first?
3. **`hal/src/dho/seq/exec.rs` and `live`.** It is in `$live` of `tools/pr-gate.sh` since #170, so stage 1a needs
   its run on a unit by the gate as well as by this plan; it leaves in stage 4 with the files named there.
4. **Stage 1 on the unit:** one run of the tip of 1b for both pull requests, or one each.
5. **The measure** of section 7: the list of 28 paths, and 17 as the target.
6. **`boot::sim::Unit`** stays apart from the new model. Joining them would let a host run start at power-on; not
   planned here.
7. This page is in the list of [README.md](README.md) since stage 0a (the inverse codec). The Tests table of
   [architecture.md](architecture.md) names the simulated unit with stage 2, which brings the host run into the
   comparison.
