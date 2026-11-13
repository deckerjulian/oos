# Hardware averaging

Status: specification from static analysis of the stock driver library. Nothing here ran on the unit.

## 1. What the stock firmware does

Acquire mode numbers in the stock driver: 0 normal, 1 peak, 2 average, 3 high resolution, 7 ultra acquisition.
Average is mode 2. Function names below are the stock symbol names, given as references only.

### 1.1 Configuration (`CDrvScope::ConfigAcquire`, on every RUN and every settings change)

Order of the writes that differ from normal mode or matter for it:

| step | register | value | function |
|---|---|---|---|
| 1 | 0x1004 | shadow with bits 1, 3 and 12 cleared, then bit 3 set | `DevAcquireSpu_SetAcqMode(2)` |
| 2 | 0x10b0 | count - 1, but at least 1 (count 1 and 2 both give 1) | `DevAcquireSpu_setAvgPara` |
| 3 | 0x1148 | 640 / m + 32, m = channel mode 1, 2 or 4 (written in every mode) | `DevAcquireSpu_setAvgPara` |
| 4 | 0x404c | 5, then 10 in average mode (pre-sample compensation) | `DevAcquireSPU_SetPreSaComp` |
| 5 | 0x10e4, 0x4050/0x4054 | record depth, pre and post points, as in normal mode | |
| 6 | 0x10e0 | segment size from the average memory layout (1.3) | `DevAcquireSpu_SetMemSize` |

Mode bit summary for 0x1004: bit 1 = peak, bit 3 = average, bit 12 is cleared by every mode change and never set by
this library. Peak falls back to normal (no bit) when the sample rate equals the maximum rate. Average has no such
fallback.

### 1.2 Horizontal block (`CDrvScope::ConfigHorizontal` -> `DevAcquireSPU_SetWaveRange`)

In average mode only, the read window gets an extra "average offset" word:

- 0x1110 for the main window, 0x1114 for the zoom window.
- Input: memory start `s` of the window (points) and channel mode `m`. With 5 points per half burst:
  `q = (m * s) / 5`, `qa = q` rounded down to a multiple of 4.
  Word = `(qa & 0xfffffc) << 8` OR `((s - qa * 5 / m) * m) & 0xff`.
- In other words: bits 31:8 carry the half-burst address of the window start (4-aligned), bits 7:0 the remaining
  offset in samples times m. The average buffer is addressed in half bursts, not in points.

0x10a0 (second-filter output shift, see `02-hires-fir.md`) is 16 in all modes, 8 only when the second filter is on and
the mode is average.

### 1.3 Memory layout

The averaging memory needs twice the bursts of a normal segment plus a fixed margin. Series parameters for
DHO800/900 (`HDO900Conf`, read with `docs/re/libscope/scripts/series_params.py` in the analysis repo):
10 points per DDR burst, 16,777,216 bursts SPU memory, average segment burst 32, average base 5,242,880 bursts.

Segment size for `n = points * ratio`:

| mode | formula (bursts) | then |
|---|---|---|
| normal | `(n + 384) / 10`, at least 32 | next power of two below 25 M points, cap 7,864,320 |
| average | `((n + 383) / 320 + 2) * 64`, at least 32 | same rounding and cap |

Example 1,000 points, ratio 1: normal 256 bursts, average 512 bursts.

Frame ring: live batches use half the memory, `(16,777,216 / 2 - 1,048,576) / seg`. With averaging, the record-mode
frame count (`DrvRecord_GetMaxRecordFrame`) uses `(16,777,216 - 5,242,880 - 1,048,576) / seg`. The 5,242,880 bursts
at the top are the average base region, presumably the accumulator.

### 1.4 Arm, done, read

- Arm (`CDrvScope::RunScope`, mode 2): always ring slot 0, first frame 0, count = frames per batch, segment size as in
  1.3. Same register sequence as a normal type-2 arm (`acquisition-start.md`).
- Done and count: the same as normal mode, 0x4080 bit 30 and `0x4010 - 0x4014 + 1`.
- Read: unchanged. Stop sequence, type-4 play, one read of `n * (16 + 2 * ratio * len)` bytes, same 16-byte header,
  same 16-bit words, same de-interleave. The library has no average-specific frame decoding.
- Progress: 0x10b0 is readable (`DevAcquireSpu_getAvg`), but nothing in the library calls the reader. The stock UI does
  not take its average progress from the FPGA.

### 1.5 Reset

Average clear = 0x10b4 written 1, then 0 (`DevAcquireSpu_setAvgClr`). Always after a stop (`StopScope`). Callers:

- every settings change while running (`RunConfig`),
- display clear (`Clear`) and the measurement counter reset in average mode (`RstCounter`),
- state changes STOP, SINGLE, and the roll/scan transitions (`SetState` cases 1, 3, 4, 7, 8, 9).

OOS already writes the same pulse in the roll path (`hal/src/dho/roll.rs:184`).

## 2. What OOS does today

- `hal/src/dho/acq.rs:38`: "average is not implemented yet". `acq.rs:98-100` sets only the peak bit.
  `acq.rs:102` always writes 0x10b0 = 1 and `acq.rs:104` 0x404c = 5.
- `hal/src/dho/hz.rs:115` `mem_size` has only the normal layout.
- `ui/src/ui.rs:1471`: averaging in software, an exponential blend `x = prev + (x - prev) / N` on f32 volts, on the
  records that reach the UI. Records the batch reader does not deliver are not averaged.
- `ui/src/board/dho/hw.rs:30`: scan mode is suppressed in average mode, as in stock.

Gap: the FPGA average path is unused. Every frame is transferred and converted although only the average is shown.
Triggers between batches do not contribute.

## 3. Proposed change

### 3.1 HAL

- `acq::Mode` gets `Average { count: u32 }`.
- `acq::configure_la`: for `Average`, 0x1004 bit 3 instead of bit 1, 0x10b0 = `max(count - 1, 1)`,
  0x404c = 10, 0x10e0 = `hz::mem_size_avg(..)`.
- `hz.rs`: `mem_size_avg(points * ratio)` per 1.3, and `max_frames` for average (live: half ring of the average
  segment; record: the formula with the average base).
- `scu.rs` `readout_window`: in average mode also write 0x1110 per 1.2 (0x1114 only once zoom exists).
- `acq::avg_clear() -> Vec<Write>`: `[(0x10b4, 1), (0x10b4, 0)]`, only after the stop sequence.
- `seq::State`: `avg_progress(bar) -> u32` reads 0x10b0, diagnostic only until 5.2 is answered.

### 3.2 Sequence in `seq`

1. Configure (as today plus 3.1).
2. Average clear after the stop poll whenever the configuration changed (any write list differs from the last one),
   on RUN after STOP, on SINGLE and on CLEAR.
3. Arm as today. Batch count: as for normal mode, but the read takes only the last frame of the batch if 5.1 shows
   the ring holds running averages (option A). Otherwise read all frames as today (option B).

### 3.3 UI and model

- `hw.rs` settings: map `AcqMode::Average` to the HAL mode with `m.avg_count`.
- `ui.rs:1471`: skip the software blend when the frame is marked `hw_avg`. Keep it for the demo and replay sources.
- `clear_display` and settings changes must reach the acquisition thread as an average clear request
  (new flag in `HwShared`).
- Status line: show the hardware count only when 0x10b0 reads back as progress (5.2).
- Average count list: keep 2..1024 until the width of 0x10b0 is known.

### 3.4 Expected CPU and acquisition gain

- Option A: one frame read per batch instead of `frames`. At 1 us/div, 1 channel, batches of up to 160 frames:
  transfer and conversion drop by up to two orders of magnitude.
- All triggered frames contribute, not only the ones that fit through the read loop. Convergence follows the trigger
  rate.
- The 16-bit output has about 14.6 codes per ADC LSB (7500 codes per division, 12-bit ADC over about 8 divisions),
  so an FPGA average can show up to about 4 bits below the ADC LSB without a format change. To be confirmed (5.4).
- The software blend itself is cheap (one multiply-add per sample). The gain is in transfer and conversion.

## 4. Host test plan

- `acq.rs` table test: Average 16 on CH1 at 200 us/div and 1 us/div gives 0x1004 with bit 3 set and bits 1, 12
  clear, 0x10b0 = 15, last 0x404c = 10, 0x10e0 from the average formula. Counts 1 and 2 give 0x10b0 = 1.
- `hz.rs` table test: `mem_size_avg` for 1,000 / 12,500 / 1,000,000 / 25,000,000 points and ratios 1, 2, 4,
  including the 32-burst floor and the 7,864,320 cap. Extend `tools/hzmodel.py` with the same formula and compare
  both in one test vector file under `hal/testdata/`.
- `scu.rs`: average offset word for `s` in {0, 3, 7, 1,248,000} and m in {1, 2, 4}, hand-computed expected values.
- Order test: `avg_clear` appears after the stop sequence and before the arm in a configure-and-arm step list.
- `testdata/second-frame-writes.txt` must stay identical for normal mode.
- UI test next to `ui.rs:7551`: a fake hardware frame with `hw_avg` set bypasses the blend; a demo frame still blends.

## 5. Hardware test checklist

1. Normal vs Average 16, 1 kHz square from the calibrator, 200 us/div: the RMS noise on a flat part drops by about 4.
2. Read 0x10b0 while running. Does it count up to the configured value, stay constant, or wrap?
3. Batch of 16 frames in average mode: are the 16 frames identical, a running average, or raw frames (option A or B)?
4. After the count is reached: running (exponential or moving) average, or does it stop and hold? Step the input level
   and measure the settling time in frames.
5. Change V/div while running: the average must restart (0x10b4 pulse). Without the pulse: does old data stay?
6. Trigger position with 0x404c = 10 vs 5: the edge must stay at the trigger point.
7. 2 and 4 channel modes: 0x1148 and 0x1110 with m = 2 and 4, window start not a multiple of 5.
8. Deep records: largest depth that still averages correctly with the doubled segment.
9. Read-out window away from the record start: compare with the full-record read to validate 0x1110.

## 6. Confidence and open questions

Confidence: register sequence high (direct from the library). Semantics medium: what the play returns in average mode
is not visible in the library.

1. Running average after the count (exponential) or cumulative and hold? Decides option A or B.
2. Is 0x10b0 progress on read, or only the configured value?
3. Width of the count field. The stock UI offers powers of two; the register limit is unknown.
4. Fractional bits in the averaged 16-bit output.
5. Interaction with peak mode and with the LA: the library does not prevent combinations; OOS should refuse them.

## 7. Implementation status (2026-11-12)

Fork commits f31707b (HAL), d48794a (oos-ui). Host-tested only. Nothing ran on the unit. Hardware steps:
`docs/fork/unit-dho804.md`, section "Hardware averaging".

| item | state | where | default |
|---|---|---|---|
| mode and count | done | `acq::Mode::Average { count }`: 0x1004 bit3 (bits 0, 1, 3, 12 cleared first), 0x10b0 = `avg_count_reg` (count - 1, at least 1), 0x404c = 10 | - |
| memory layout | done | `hz::mem_size_avg`, `hz::max_frames_avg`, `hz::avg_params` (applied in `seq::State::configure`); `tools/hzmodel.py avg` writes `hal/testdata/avg-memsize.txt`, the Rust test compares | - |
| window word | done | `scu::avg_offset`, written as 0x1110 right after 0x1108 when `Window::avg` | - |
| clear | done | `acq::avg_clear` (0x10b4 1, 0); in `configure` after the stop poll, before the acquisition writes; `seq::State::avg_clear_steps` (stop poll, pulse) for CLEAR, RUN, SINGLE and a complete hold average | - |
| read | done | every recorded frame is played, the newest shown (option B, as stock; unit 2026-11-13 B1, section 8) | - |
| semantics | both | `avg::Kind::Running` (show every batch) and `avg::Kind::Hold` (show when `count` triggers are in, then clear), `avg::Tracker`; oos-ui picks Running unless `OOS_HWAVG=hold` | Running |
| progress | diagnostic | `avg::progress` reads 0x10b0; oos-ui logs it with the first words of the first 32 batches after each clear | - |
| model | done | `seg::sim::Engine` averages `Engine::raw` per trigger in average mode (running or hold, cleared by the 0x10b4 pulse); `acquire_batch` is generic over the bus and runs against it | - |
| fixture | done | `hal/testdata/avg-writes.txt`: acquisition block, arm and read of Average 16, CH1, 100 ns/div, 1000 points, a batch of 10 with 4 recorded. Checked line by line against 1.1 to 1.5; the test also checks that it differs from normal mode only in the clear pulse, 0x1110, 0x1004, 0x10b0, 0x404c, 0x10e0 and the frame played | - |
| oos-fpgad | done | `plan`/`run` take `--average N` | - |
| oos-ui | done, opt-in | Acquire > HW average (shown in Average mode with the hardware source); setup key `avg_hw`; SCPI `:ACQuire:TYPE`, `:ACQuire:AVERages`, `:ACQuire:AVERages:HARDware`; top bar "Average N HW" while the FPGA averages | off |

Behaviour:

- Hardware averaging is used only when all hold (`hw::hw_avg_count`): Average mode, HW average on, a count that is a
  power of two from 2 to 1024 (`avg::hw_count`; the menu offers only these, SCPI refuses others), ordinary triggered
  records (not roll or scan), LA off, hardware segments off, and a record whose average segment leaves at least one
  frame in the live ring (`hz::max_frames_avg` > 0: raw points up to about 21 M, and from 25 M to about 36 M, where the
  stock rounding gives a smaller segment than the power of two below). Otherwise the software blend runs as before. `seq::State::configure` refuses Average with the LA, with hardware segments, or with a record that does not
  fit, so no caller can send the combination by mistake.
- Hardware segments and hardware averaging are mutually exclusive. With both switched on, segments win and the
  average is the software one over the segments read back.
- A change of the count, time base, depth, channels, trigger or any other setting makes a new `Settings`; the thread
  configures again, which clears the average. CLEAR, RUN after STOP and SINGLE bump `Model::hw_avg_clear`; the thread
  clears before the next arm. Self-cal and the generator end the configuration, the next one clears.
- Frames the FPGA averaged carry `Frame::hw_avg`; `ui.rs` skips the software blend for them. Demo, replay and
  non-averaged hardware frames still blend.
- The batch length in average mode is limited by the frame ring and 1000 / ratio only, not by the 4 MB read size: one
  frame is read whatever the batch holds. Arming ahead (`OOS_PREARM`) is off in average mode, so a clear never has to
  stop an acquisition armed ahead.
- SINGLE clears and shows the first batch (Running) or the first complete average (Hold).

Decisions against the proposal:

- The 0x1004 mask stays 0x100b (bit 0 cleared too), as OOS already writes it in every mode. Stock clears only bits 1,
  3, 12. Unchanged so normal mode stays byte-identical.
- Above 25 M raw points `mem_size_avg` follows the stock rounding to the next multiple of 2048 bursts (an exact
  multiple gets one block more), not the power of two. Between 21 M and 25 M raw points the power of two hits the
  cap and leaves no frame in the live ring; these depths average in software.
- The record-mode frame limit with the average base (1.3, last formula) is not implemented: averaging and hardware
  segments are exclusive, so nothing would call it.
- The averaged words are decoded like normal ones (7500 codes per division around 32768), because the stock library
  has no average-specific decoding (1.4). `avg::mean_words` and its test document what the model assumes: a 16 +
  log2(N) bit accumulator, a shift back to 16 bits, truncation, sub-LSB steps kept (about 14.6 codes per ADC LSB).

Proof that the live path without averaging is unchanged: `tools/plan-matrix.sh` with `oos-fpgad` built before
(db8727e) and after (f31707b): 7,988 lines, byte-identical. `hal/testdata/second-frame-writes.txt` and
`record-arm-writes.txt` pass unchanged.

Not done: zoom window word 0x1114 (OOS has no zoom read-out), a status-line progress from 0x10b0 (waits for 5.2),
counts above 1024.

## 8. Unit results 2026-11-13 (B1)

Unit log: private RE notes `docs/logs/oos_unitlab_20261113.md` (B1 in rounds 1 and 4), dumps in
`docs/logs/unitlab_b1/`.

| finding | cause | state |
|---|---|---|
| averaged frames all zero above 62.5 MSa/s | 0x1004 bit 2 ("AvgSinc") clear. With bit 2 set every rate averages (312.5 MSa/s: 1.08 -> 0.46 mV at N = 16; 1.25 GSa/s 100k: 3.24 -> 1.2 mV); without it every rate above 62.5 MSa/s is zero (156, 312, 625, 1250 tested) | fixed: `acq::configure_la` sets bit 2 in average mode at every rate |
| noise levelled off near 0.5 mV (100 mV/div) | the newest frame alone was played; the netlist couples averaging to the play-out | fixed: every recorded frame is played (750 frames/s at 62.5 MSa/s, 31/s before) |
| HW 16 still above SW 16 (0.42 vs 0.34 mV at 100 mV/div), floor in proportion to V/div | not found yet; not truncation and not an ADC core offset (dump analysis below) | open |
| 0x10b0 reads N - 1 | read-back of the written register | none |

Stock: `ConfigAcquire` sets bit 2 only for rate < max / 8, in every mode. With that rule stock average mode above
62.5 MSa/s would show the same zero frames, unless the stock play path differs there. The netlist routes bit 2 to the
play side only (play gain/zoom, sinc interpolation, trigger), and the stock driver keeps the sinc interpolation chain
running at fast time bases where OOS sets it to pass-through (`scu::passthrough`). Inference, not verified: bit 2
selects whether the averaged data bypasses the sinc chain; stock reaches the play output through the sinc chain with
bit 2 clear, OOS with the chain bypassed needs bit 2 set.

Dump analysis (62.5 MSa/s, 1 channel, 10k points, N = 2 and 16; `avg.debug_raw` of the b1-raw build, since removed):

- `seq` (0x1004 bit 3 cleared, fields in order): even and odd words both carry the signal; no field with low bits.
- Both debug reads show an even/odd offset between neighbouring words: a steady -180 codes (16-bit scale) in `seq`,
  and in `wide` constant within a dump but different between dumps (-124 to -179, once +56). It is not an ADC core
  offset: a core offset would be fixed, and the frames the UI shows have no such pattern (at 100 mV/div it would be
  2.4 mV, measuring at least 1.7 mV point-to-point; Normal measured 1.07 mV, HW 16 0.42 mV). Most likely the two
  parities are the two stored fields of one averaged point. Per parity the noise is 16-19 codes at N = 16 and 50-53 at
  N = 2 against 89-91 raw, close to 1 / sqrt(N): the averaging itself works.
- `wide` (average mode, twice the window): the words from 10240 on are bit-identical in all eight dumps of both runs
  (N = 2 and 16): stale memory behind the frame, not an accumulator or an earlier average. No extra precision there.

Open: the gap between HW 16 and SW 16 as displayed. Unit check queued (round 6): even/odd mean difference and
per-parity noise of a Normal and a HW 16 frame (`:WAV:DATA?`, 62.5 MSa/s, 100 mV/div, input open).
