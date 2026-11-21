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
- **Globals:** `oos_hal::model::platform()` (three uses in `spawn`), `board::pipe::workers()` (the deposit workers of
  `drawpipe`, registered there) and the read-out buffers lent to them, `board::pipe::counters()`,
  `board::policy::compress::note_batch_k`.
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
  `seq::deinterleave` with the core offsets, then the frame's fields), then `wave::to_record` (the software trigger
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
  22), the fill-level strobe (0x1010 bit 28), the generator's block (0x3000..0x307c behind its bank selects 0x3ff0,
  0x3ff4, 0x3ff8: output, phase increment, DAC codes, modulation; afg.md 2) and its lines (relays, supply), which
  make no signal: a test reads what they were told (`sim::GenState`).
- **Told by the test, as a scene:** the volts of each channel at a time, the instants at which the signal triggers,
  and the view: volts per division and offset of each channel, sample period, record length and pre-trigger of the
  settings the test asks for. A new view takes effect at the next arm.
- **Recorded and not interpreted:** the front ends, the ADC (but for its core offset words, 3.3), the read-path gain,
  the trigger unit and the time base.

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
| 0x10b0 | in average mode (0x1004 bit 3) the triggers in the average less one, up to the count written; else the value written | `avg::progress`: that it counts is assumed; once the average is full both readings are the value written |
| 0x221c..0x2240 | the frequency counter's results of its last whole gate, latched by 0x2218: the scene's trigger instants in the gate (ticks to the first, from the last, how many), the trigger period, the instants since the start or the clear; 0 without a whole gate | the registers: verified on the unit (counter.md 4); that the comparator fires where the trigger does is assumed (0x2200 / 0x2204 not known), and so are the zeros |
| 0x2244..0x2258 | the DVM block's sums of its last whole window, latched by 0x2248: windows of 2^exp samples at 156.25 MSa/s back to back from the restart (0x2244 bit 0 cleared), of the channel on the lane written; the samples the scene's volts as stored 12-bit codes (the read-out code / 16); 0x2244 the fields written under the top bits of the sum of squares; 0 without a whole window | the layout, the lanes and the window's length: verified on the unit (docs/fork/dvm.md); assumed: the windows back to back, the latch of the last whole one, the zeros, a read-path gain of 1, and sums of 4096 samples of the window scaled to its 2^exp |
| 0x11a0..0x11b4 | the mask unit's counters latched by the rising 0x1190 bit 28: frames tested and failed while the mask test is on (0x4020 bit 15; a sample of the slot's channel, read-out code >> 8, outside the limits of its column of 0x1184 bits 9:0 samples), with the zone trigger armed (0x4020 bit 1) the frames it took; cleared by 0x1190 bits 29 and 30 | the scale, the slots, the columns at 10k points and one count a frame: verified on the unit (docs/fork/hwmask.md, "On the DHO924S"); the latch and the clear: assumed. Stop on fail and the stall of a strobe count the record does not hold: not modelled |
| 0x1438 | a fixed word inside the valid range | board.md 2.3 |
| 0x1040, 0x1044, 0x1048 | the measure unit: the rising 0x1040 bit 31 starts a run of 2^e samples (bits 4:0) at 156.25 MHz, bit 30 set once it is over; the slot of bits 10:8 shows its channel over the run, 0x1048 the 12-bit maximum and minimum, 0x1044 the mean on the 16-bit scale (the scene's read-out codes); 0xeeeeeeee before a run is over. Where the test gives the cores' own offsets (`Unit::cores`), the slot of core n is moved by 9 codes per step of its offset register (the ADC word 0x1034 to register 0x14 + 8n) against its own offset; else every core is alike | self-cal.md 2.4, the head of `hal/src/dho/levels.rs`; which slot shows which channel is assumed there, and so are the 0xeeeeeeee, a read-path gain of 1 and the values of 1024 samples of the run. The offset word: adc-mode.md 2.2. The step (8 to 10.6 codes) and the slot of each core in mode 2: verified on the unit (docs/fork/core-offset.md, rounds 11 and 12); the same slots in the other modes: assumed. The frames and the DVM block do not carry the core offsets |
| any other | the value last written, else 0 | invented: some registers of the unit do not read back (record-play.md 6.0) |

### 3.4 The stream it sends

| What | Format | How it is known |
|---|---|---|
| the frames of a play | frames `first..=last`, each the words of the window for all channels, a 16-byte header first when 0x1010 bit 31 is clear (word 4 the frame's number, words 5 to 7 the stamp), each frame padded to 8 bytes | header word 4: verified on the unit (acquisition-start.md, "Frames 0 and 1 of a batch"); the padding: found on the unit for one channel, assumed for two ("C2H read rules found on the unit" 1) |
| a frame an arm took | an arm takes frame `first` of the ring at once (its pre-trigger fill), so the record stored there is gone, also when a stop comes before the trigger; played before a trigger records it again, it has no record of the scene (the trigger time 0). The later frames keep theirs until they are recorded | that any arm overwrites the ring from frame `first` on: acquisition-start.md, "Reading a stored frame again"; that the first frame goes at once and the later ones as they are recorded: assumed, as the loop's batch path has it |
| the words | the scene's volts as codes: 32768 at the channel's offset, 7500 a division (`hal/src/dho/readout.rs`), interleaved as `seq::deinterleave` reads them | afe-settings.md 4.1, horizontal-config.md 2.2 |
| compressed frames | min/max pairs of 2k samples in the order the extremes occur (`pairs::model`) | verified on the unit, as the head of `hal/src/dho/pairs.rs` states it |
| averaged frames | in average mode each frame recorded holds the running mean of its own record and those before it since the last clear (the rising edge of 0x10b4 bit 0), of at most 0x10b0 + 1 triggers: `avg::mean_words` for records of one length; a frame recorded outside average mode holds its own samples | the averaging in the FPGA memory is assumed (horizontal-config.md 3); so is that it runs over the triggers recorded (`seq::State::read_steps` assumes the frames played, the same frames while every recorded frame is played), and that a record of another scale without a clear is averaged with the ones before it |
| the raw LA window of a record | with the stream the LA's alone (0x4008 bit 24 set, bit 23 clear) the play-last pulse plays the LA words of the newest frame: 0x1100 / 10 words from LA sample 0x1108, the read rounded up to 4 words, no header; the record has memsel x m / 2 LA samples, m / 2 a sample of the analog record, the trigger at LA sample pre x m / 2; each word the scene's lines at its time, 0 past the record. The FSM run after the pulse plays no analog frame | la.md 4.2 (verified on the unit with N = 1000 and 100,000 after a stock acquisition) and 3.3; the trigger index, the unit of 0x1108 and that the run plays nothing more: assumed (test T1) |
| the zone trigger | with 0x4020 bit 1 set a trigger is recorded only when its frame qualifies: each zone on in 0x11d0 touched (a sample of its columns within its limits, or the trace crossing them) when its mode bit asks for it, not touched else; a trigger not taken costs a whole record before the next is looked at. A zone never qualified records nothing: the loop's arm ends with "no acquisition" | the qualification in both modes: verified on the unit (docs/fork/hwzone.md, "On the DHO924S"); the re-arm after a rejected frame: inferred there, assumed here |
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
| 1a | the executor takes any bus, stream, front-end bus, lines and clock; `selfcal::Unit` and the generator's executor too. Two pull requests: first the clock (`execute_bus_on`, `acquire_batch_on`, every poll and sleep of `exec.rs` through `boot::Clock`; the old names are those with `SysClock`), then the front-end bus, the lines, self-calibration and the generator (`execute_on` over a bus, a front-end bus, a stream, `lines::Lines` and a clock; `prearm`, `disarm` and `acquire_la` over any bus and stream; `selfcal::Unit` over the same five; `afg::run` over a bus, `afg::GenLines` and a clock) | `hal/src/dho/seq/exec.rs`, `hal/src/dho/selfcal.rs`, `hal/src/dho/afg.rs` | 200 | `Refactor:`, `Behaviour: unchanged`, the comparison finds nothing; time-outs tested in virtual time; one run on a unit | medium: the executor runs live with its timing. `exec.rs` is in `live` (`tools/pr-gate.sh`, ADR 0052), so this is a refactoring of a live path and the gate asks for the run on a unit |
| 1b | the seam of section 4: `Port`, `Hw`, `hw::run`, the scope's port. Three steps: 1b.1 every wait and time of the loop and its helpers (`seg_step`, `counter_tick`, `levels_measure`, the core trim, `board::policy::deep::due`, `TrimPending`) on one clock, microseconds of it in place of `Instant`; 1b.2 the bundle (`ui/src/board/dho/hw/port.rs`: `Port`, `Hw`, `Scope`), the generator's and probe's lines behind it, `spawn` opens the scope's sides in the order it always did (the FPGA version before the stream and the front ends) and calls the generic `run`; 1b.3 `run` compiled on a host (`cfg(any(target_os = "linux", test))`: `cargo test` builds it on every host), asking `keep_going` before every pass, and run by a test against the simulated unit (`hw/tests.rs`, `SimPort`) | `ui/src/board/dho/hw.rs`, `hw/plan.rs`, `hw/seg.rs`, `hw/units.rs`, `hw/trim.rs`, `ui/src/deepmeas.rs`, a new module for the port | 300 | as 1a; `tools/device-test.py --changed` on the unit (for `ui/src/board/`: acquisition, roll, trigger, front end, register sequences), and its frame rate against main on the same unit | medium: a unit of time mixed up turns 33 ms into 33 s. No lines move, so the diff can be read |
| 2 | the host run: a test of oos-ui runs `hw::run` against the simulated unit for a matrix of settings and N records each, checks the frames against the scene and writes the trace; the comparison gets a third part. Done: `ui/src/board/dho/hw/liverun.rs`, eight cases (AUTO at 1 us/div with 1, 2 and 4 channels, AUTO at 1 ms/div with a square, NORMAL without a trigger, new settings during that wait, SINGLE, STOP); the same files twice and in a release build; the comparison's part `live`, kept in the cache by `--record-sums`. A deep record through the compression and a STOP view read again came with 3d. The trace holds the unit's accesses only: a wait after which the loop touches the unit no more does not show | a test module of `hw/`, `tools/compare-base.sh`, `.github/workflows/ci.yml`, `tools/README.md`, `docs/architecture.md` | 400 and 60 in the tools | host tests (layer `test`); a test and tools only, so the programs for the scope are expected byte-identical (job `scope`); pictures and register sequences of the comparison equal, the new part reported as new | low for the scope; the run must not flake: virtual clock, one thread, no order taken from a hash map |
| 3 | the roll stream, scan (RUN and SINGLE) and the segmented read in the model and in the matrix. Done in 3a: the roll and scan stream in `sim::Unit` (chunks on the virtual clock at the spacing the display chain written gives, the fill level 0x11e8, a sweep of 1000 chunks with its counter from 1, 0x1204 at the centre column, 0x40d0 four chunks early) and four cases: roll with one and four channels, scan in RUN (done, pause, next sweep) and in SINGLE (halt, next press). The model sends each point as the scene's sample, not as a min/max pair, and its sweep does not wait for a trigger. Done in 3b: the logic analyser's chunks (with 0x4008 bit 24 at the start an LA chunk before each analog chunk, sharing its counter; the lines are the scene's) and two cases, roll and scan in RUN with the LA on; the scan case gets 100,000 LA words a sweep at 200 ms/div, as the unit did. Done in 3c: the stamps of a segmented record in `sim::Unit` as the head of `seg` states them (the latch 0x40b0/0x40b4 the trigger of the frame played last, the first-trigger stamp 0x40c0/0x40c4 that of the first frame since its clear, 0x40b8 bit 1 its validity) and two cases: 20 segments recorded, read back with their times and one shown; 10 with an interval of 1 ms (0x4068, which the model waits after each frame: assumed, as `scu::record_arm` says), 1050 us apart. Done in 3e: the steps of `seg` (start, stop, stamps, read) and `hw::seg_abort` on the loop's clock (`execute_bus_on`; before, on the wall clock, the stop of a recording that still ran polled 0x4034 for a time of the host and its trace differed between builds), and the abort: 1000 segments asked for and stopped after 30, whose stamps are read and whose frames are not delivered. Done in 3d, the paths of stage 2 the eight cases did not reach, a case each: the start with the made-up calibration set and the front ends programmed (through a stub), live compression at 200 us/div with the newest frame read raw and a STOP that asks for it, two views read again in STOP, the raw screen window of a 10 M record for the measurements, the force-trigger pulse (written; the model does not interpret the trigger unit, so it records nothing). Live compression is off by default on the unit (`acq.compress`): the cases switch it on for their thread (`board::policy::compress::force`, tests only). Done in 3f: the hardware average in `sim::Unit` (3.4: a running average of the triggers since the clear, 0x10b0 counting) and three cases with a scene whose offset changes sign from trigger to trigger: the average of 16 with a CLEAR, of 64 cleared by new settings (a vertical scale the frames would otherwise mix with the old one), and Average in software, whose frames keep the offset. The running reading of `avg::Kind` only: the hold reading is a switch of the environment (`OOS_HWAVG`). Done in 3g: the generator in `sim::Unit` (its block read as afg.md 2 describes it, the lines as a side of the unit, `sim::Gen`) and two cases: a sine switched on, retuned without a new table and switched off; a square, then a sine with AM. Done in 3h: the frequency counter in `sim::Unit` (gates on the virtual clock, 3.3) and a case at 1 ms/div: frequency, then totalize, then stopped. Done in 3i: the DVM block in `sim::Unit` (windows on the virtual clock, 3.3) and a case at 1 ms/div with two channels: CH2 with an offset, then CH1, then stopped. Its switch `dvm.hw` is read by the screen's side only (`Board::dvm_hw`): the loop serves `Shared::dvm_want` whatever it says, so the case asks for the block as the screen does with the switch on. Done in 3j: the mask unit's counters and the zone trigger's qualification in `sim::Unit` (3.3, 3.4) and two cases at 1 ms/div, their switches `mask.hw` and `zone.hw` again the screen's alone: a mask band around the signal (no fails), shifted by a quarter period (every frame fails), then off; a zone the trace touches, one it does not touch, then one it must touch and never does, whose arms stall until the stall guard drops it for these settings and the frames come again. Done in 3k: the raw LA window of a record in `sim::Unit` (3.4) and a case at 1 ms/div with the LA on, whose newest frame of every batch carries the LA words of its record. Done in 3l: the measure unit in `sim::Unit` (3.3) and a case at 1 us/div with two channels that asks for the auto set-up's levels and for the debug slot dump (`Shared::levels_want`, `slots_want`). Done in 3m: the frame an arm takes in `sim::Unit` (3.4) and a case at 1 us/div with arming ahead (`acq.prearm`, off by default on the unit: the case switches it on for its thread, `hw::plan::force_prearm`, tests only), new settings and a STOP with a view: each batch armed ahead and waited for without a second arm, the disarm first at the new settings and at the STOP, no view read again from the ring the arm wrote. Since then `seq::prearm` and `seq::disarm` run their steps on the loop's clock, as `seg` does since 3e (before, the disarm's poll of 0x4034 waited on the wall clock). Done in 3n: the cores' offset registers in the measure unit of `sim::Unit` (3.3) and a case at 1 us/div with the core trim (`cal.core_offset = tight`, off by default on the unit: the case switches it on for its thread, `hw::trim::force_core_offset`, tests only) on a quiet CH1 in NORMAL without a trigger and with the made-up calibration set, whose stored offsets leave two cores 3 and 2 register steps off: the trim waits until the configuration has stood 2 s, starts from the stored offsets and brings every core to its own | `oos_hal::dho::sim`, the test module of stage 2 | 500 | as stage 2; new lines only in the live part | low |
| 4 | what leaves `live`, and the record that says so. Done: ADR 0073, the class `$looped` of `tools/pr-gate.sh`, first with `hal/src/dho/roll.rs` (a scan case with the trigger point left of the screen reaches its scan trigger position, 0x4058 enabled), then with 3e `hw/seg.rs` and `hal/src/dho/seg.rs` and the artifact `compare-live` of CI's job `compare`; the rest of the board code stays live until the split of `spawn` gives the reached parts files of their own. The split of `spawn` began with the roll and scan branch of the loop, now `SlowRun` in `hw/slow.rs` (in `$looped`), then the STOP paths (the hold with the final frame, the views read again), `StopRun` in `hw/stop.rs` (in `$looped`), then the batch path (`BatchRun` in `hw/batch.rs`, still `live`: the live run does not reach the clear after a complete hold average and the hand-over to the deposit workers; its arming ahead since 3m): the comparison found its first try different in the three scan cases (an inner `continue` had become a `return`), and the second equal in all 24 and the artifact `compare-live` of CI's job `compare`; then the side jobs (the generator, `AfgRun` in `hw/afg.rs`, reached since 3g but for its lines that fail to open and a model without it; the DVM block, the levels, the debug slots and :TFORce, `UnitJobs` in `hw/units.rs`, of which the live run reaches :TFORce, since 3h the counter (`counter_tick`), since 3i the DVM block (`UnitJobs::dvm`) and since 3l the levels and the debug slots; the core trim, `TrimRun` in `hw/trim.rs`, reached since 3n but for a trim that fails, saves its file or waits for the generator; the self-calibration's run, `self_cal` in `hw.rs`), all still `live`: the live run reaches none of them in full (section 7); then the set-up and the pass: `run` in `hw.rs` makes what the loop owns (`Loop::new`: the calibration files, the shadow state, the parts) and calls `Loop::pass` while `keep_going` says so, a pass ending in `Loop::batch_pass` for a batch; the UI's request of a pass (`Request::read`) and the configuration on a change (`ConfigRun`, `Loop::configure`) went to `hw/config.rs`, still `live`: the live run does not reach a failed configuration (section 7); the rest of the board code stays live until the split of `spawn` gives the reached parts files of their own | a new decision record that amends ADR 0042, `tools/pr-gate.sh`, `AGENTS.md` | 80 | documents and a tool: no unit; decided as a record (ADR 0031) | a file leaves the list although the run does not reach its code: section 7 counts what is reached |

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
ahead and the core trim, switched on for the test's thread since 3m and 3n); the measure unit in the model (done in 3l; the core trim since 3n), so
self-calibration runs in the loop; the simulated unit as a source of host builds of oos-ui (section 8, question 1).

## 7. The measure of goal T1

"Live paths run by host tests" counts these paths of the loop. Today (3n): 25 of 28; 17 was the target of T1.

| Path of the loop | Run by a host test from |
|---|---|
| start: version check, calibration loaded | stage 2 (3d) |
| configuration on a change, with the front-end steps | stage 2 |
| a batch: arm, wait, ring, read, decode, queue | stage 2 |
| SINGLE: one frame, then nothing armed | stage 2 |
| NORMAL without a trigger; new settings during the wait | stage 2 |
| frames left out: the first batch after a change, frames 0 and 1 | stage 2 |
| STOP: the hold and the final frame read raw | stage 2 (3d) |
| STOP: a view read again from the stored record | stage 2 (3d) |
| the newest frame of a compressed batch read raw | stage 2 (3d) |
| the screen window of a deep record for the measurements | stage 2 (3d) |
| the board readings once a second (XADC, logic probe) | stage 2 |
| the force-trigger pulse, the trigger status of a batch | stage 2 (3d) |
| hardware segments: record, progress, stamps, read back, one segment shown, abort | stage 3 (3c, the abort 3e) |
| the roll stream | stage 3 (3a, done) |
| scan in RUN: done, pause, next sweep | stage 3 (3a, done) |
| scan in SINGLE: halt and the next press | stage 3 (3a, done) |
| roll and scan with logic-analyser chunks | stage 3 (3b, done) |
| hardware average and its clear | stage 3 (3f; the running reading, not the hold one) |
| the logic-analyser read of a record | stage 3 (3k) |
| the generator | stage 3 (3g) |
| the frequency counter, the DVM block | stage 3 (the counter 3h, the DVM block 3i) |
| levels of the measure unit, the slot dump | stage 3 (3l) |
| the core trim | stage 3 (3n) |
| the mask and zone units, the stall guard | stage 3 (3j) |
| arming ahead and disarming | stage 3 (3m) |
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
7. This page is in the list of [README.md](README.md) since stage 0a (the inverse codec), and the Tests table of
   [architecture.md](architecture.md) names the simulated unit since stage 2.
