# Segmented memory with hardware trigger timestamps

Status: specification from static analysis. The record engine itself (0x400c, 0x4010..0x4018) is already verified by
OOS for batches; the timestamp registers and the record-mode use are not.

Correction to the plan: 0x10a0..0x10e4 are not segment registers. 0x10a0 is the FIR output shift, 0x10b0/0x10b4
averaging, 0x10d0..0x10dc decimation, 0x10e0 segment size, 0x10e4 record depth. Segmented memory uses the SCU record
engine (0x400c, 0x4010..0x4018, 0x4068, 0x406c) plus the timestamp block 0x40b0..0x40c4.

## 1. What the stock firmware does

The stock "record" function (state 0xb in `CDrvScope::SetState`) is hardware segmented memory: one arm records N
frames into the frame ring, each frame re-armed by the FPGA. Playback reads one frame at a time and latches its
timestamp.

### 1.1 Registers

| register | access | bits | meaning |
|---|---|---|---|
| 0x400c | W | 31:24 | record control (OOS `acquisition-start.md`): 0x30000000 = load, then bit 28 cleared; record-seq = bits 29 and 25; play = bits 30 and 29; bit 26 option flag |
| 0x4010 | RW | | first frame (write); last recorded frame (read) |
| 0x4014 | W/R | | first frame of this run |
| 0x4018 | W | | last frame = first + N - 1 |
| 0x4068 | W | | FSM delay: record interval (record mode) or play interval (playback), in 6.4 ns ticks (value_fs / 6,400,000) |
| 0x406c | W | 30:0, 31 | acquisition timeout in 6.4 ns ticks, bit 31 = enable (set when the time is at least one tick). Used by ultra mode; 0 in record mode |
| 0x40b8 | RW | 0 | timestamp latch: write 0, 1, 0 to copy the current timestamp into 0x40b0/0x40b4 |
| 0x40b8 | R | 1 | first-trigger timestamp valid |
| 0x40b8 | W | 2 | clear the first-trigger timestamp: write 1, then 0 |
| 0x40b0 / 0x40b4 | R | | latched timestamp, low / high 32 bits |
| 0x40c0 / 0x40c4 | R | | first-trigger timestamp, low / high 32 bits |

Timestamp unit: the stock API converts tick differences with 250,000 fs per tick, so 1 LSB = 250 ps (4 ns timer with 4
fraction bits is one plausible origin; not proven). 64 bits do not wrap in practice.

### 1.2 Start (`DrvRecord_SetEn(true)` -> `SetState(0xb)` -> `RunScope(0xb)`)

1. 0x40b8 bit 2 pulse (clear first timestamp).
2. Stop sequence (`StopScope`: poll 0x4034 bit 13 up to 10 x 1 ms, then the 17 writes OOS already has).
3. Run prologue: 0x4008 bit 17 and bit 22 cleared.
4. 0x406c = 0. 0x4068 = record interval / 6.4 ns (0 = back to back).
5. 0x400c = (s & 0x89ffffff) | 0x30000000. 0x4010 = 0x4014 = first (0). 0x4018 = first + N - 1.
6. 0x400c bit 28 cleared, then 0x400c |= 0x22000000.
7. 0x4008[3:0] = channel mask, 0x4004 |= 1.
8. Poll 0x40b8 bit 1 for up to 20 ms; when set, read 0x40c0/0x40c4 as the start tag.

This is the OOS `scu::arm` sequence (`hal/src/dho/scu.rs:180`) plus the interval, the timestamp clear and the start
tag.

### 1.3 During recording

The run loop polls every 9 to 10 ms. `RequestNormTrace` in state 0xb reads 0x4080 and `0x4010 - 0x4014 + 1` (frames
recorded so far, the UI progress), forces "not ready" and transfers nothing. In record and ultra mode the stock
library also treats `(0x4034 & 0x2320) == 0` as still running (`acquisition-start.md` r4). Recording ends when the
count reaches N or the user stops (`DrvRecord_SetEn(false)` -> `SetState(1)` -> stop sequence and average clear).

### 1.4 Playback of one frame (states 0xd / 0xe)

1. Stop sequence.
2. Type-4 play of exactly one frame: 0x4068 = play interval / 6.4 ns, 0x406c = 0, 0x400c load, 0x4010 = 0x4014 =
   frame k, 0x4018 = k, bit 28 cleared, 0x400c play bits (0x60000000 with bit 26 cleared), 0x4008[3:0], 0x4004 |= 1.
3. Read the frame like a live frame (header on).
4. 0x40b8 bit 0 pulse, read 0x40b0 then 0x40b4: timestamp of frame k.
5. Time of frame k relative to the first trigger = (stamp_k - start tag) x 250 ps.

### 1.5 Limits

- Segment size: normal formula of `hz::mem_size` (`(n + 384) / 10` bursts, power of two, at least 32).
- Record-mode frame count uses the whole frame memory, not half of it:
  `(16,777,216 - 1,048,576) / seg` (`DevAcquireSpu_getMaxFrm`, mode 1). With averaging the average base region is
  subtracted (see `01-hw-averaging.md`). Examples: 1,000 points ratio 1 -> 256 bursts -> 61,440 frames; 12,500 points
  -> 2,048 bursts -> 7,680 frames.
- Ultra mode (acquire mode 7, not record mode) additionally caps at `104,857,600 / (points * ratio)` and at the UI
  limit 100,000, and uses 0x406c as the batch timeout.
- OOS raised the burst count to 0x5000000 for the deep-record segment cap (`hz.rs:112`). For the frame ring the stock
  16,777,216 is the safe value until the real memory size is measured.

### 1.6 Frame header words 5..7

`CDrvScope::ReadNormTrace` takes 16-bit header words 5, 6 and 7 of the first frame of every live read, assembles them
as a 48-bit number (word 5 most significant, word 7 least) and stores it with the waveform next to a read counter
(`GetTrace`, waveform attribute). Candidate: the hardware trigger timestamp of that frame, or a trigger counter. If it
is the timestamp, every frame of a batch read with headers carries its own stamp and no per-frame play is needed.

## 2. What OOS does today

- `ui/src/ui.rs:1399-1521`: segments in software. Each record that reaches the UI is pushed into `history`; after
  `m.segments` records the UI stops. Limit `SEG_MAX = 1000` (`ui.rs:85`), kept in RAM as f32 records.
- The hardware batch (`seq::acquire_batch`, `hal/src/dho/seq.rs:498`) already records several frames per arm and plays
  them with type 4, so the record engine works. The ring is half the memory (`hz.rs:195`).
- No timestamps. Dead time between segments is the software loop (arm, poll, play, read, decode), at least one batch
  cycle.
- Frame headers are off by default; `OOS_FRAME_HEADER` enables them for trigger debugging (`hw.rs:236`).

## 3. Proposed change

### 3.1 HAL (`scu.rs`, `hz.rs`, `seq.rs`)

- `scu::record_arm(fsm, sh, n, interval_fs) -> Vec<Write>`: 1.2 steps 2..7 (stop sequence included), first frame 0.
  The existing `scu::arm` becomes the case `interval_fs = 0` plus the 0x406c write.
- `scu::ts_first_clear() -> Vec<Write>`: 0x40b8 bit 2 set, then cleared (shadow `s40b8`).
- `scu::ts_latch() -> Vec<Write>`: 0x40b8 bit 0 cleared, set, cleared.
- New step kind `Step::ReadPair { lo, hi, into }` (or a direct `Bar1` helper) for the two 32-bit reads, low first.
- `hz::max_record_frames(seg) -> u32` with the full-memory formula of 1.5.
- `seq::Settings`: `segments: Option<Segments { n: u32, interval_s: f64 }>`.
- `seq::State::record_steps(s, p)`: configure as today, `ts_first_clear`, `record_arm`.
- `seq::State::segment_read_steps(s, p, first, count)`: pass-through, window, stop, type-4 play of `first..first+count`,
  one read with headers on (header needed for 1.6).
- `seq::State::segment_stamp_steps(k)`: single-frame play of frame k with the TX length set to the minimum (8 points)
  so the read is 16 + 16 bytes, then `ts_latch` and the read pair. Fallback if 1.6 is not the timestamp.
- Executor: `record_progress(bar) -> u32` = `0x4010 - 0x4014 + 1`, and `record_running(bar)` = `(0x4034 & 0x2320) == 0`.

### 3.2 Acquisition thread (`ui/src/board/dho/hw.rs`)

- Segmented RUN: configure, record steps, then poll progress every 10 ms (no data transfer). Show "k of N".
- When done or stopped: read the start tag, then read frames in batches of up to 4 MB with headers and take stamps
  from the headers (or per-frame stamp plays), queue them as `Frame` with `stamp_fs` relative to the first trigger.
- Browsing: frames stay in FPGA memory. The UI keeps the stamps and the last viewed frames; selecting segment k plays
  frame k on demand. History RAM no longer limits the count.

### 3.3 UI and model

- `SEGMENTS` list up to `hz::max_record_frames` for the current depth (UI shows the limit).
- Segment table: index, time since first trigger, delta to previous.
- Interval setting (0 = back to back) mapped to 0x4068.
- `Frame` gets `stamp_fs: Option<u64>`; export (CSV/binary) writes it per segment.

### 3.4 Expected gain

- Dead time between segments: from one software batch cycle (milliseconds) to the FPGA re-arm (holdoff plus record
  length), microseconds. Bursts with gaps of a few microseconds become capturable.
- No transfer and no conversion during capture. CPU idle while recording.
- Segment count from 1,000 (RAM) to tens of thousands at short records.
- Trigger time resolution 250 ps per tick instead of none.

## 4. Host test plan

- `scu.rs` write-sequence test for `record_arm(1, sh, 100, 0)`: identical to today's `arm(1, sh, 0, 100)` apart from
  the 0x406c write. With an interval of 1 ms: 0x4068 = 156,250.
- `ts_first_clear` and `ts_latch` sequences and shadow values (bit 0 and bit 2 never left set).
- `hz.rs` table test for `max_record_frames`: 1,000 / 12,500 / 100,000 points at ratio 1, 2, 4. Mirror in
  `tools/hzmodel.py`.
- Timestamp math: low/high assembly, 250 ps scaling, differences across a 32-bit carry.
- `seq.rs`: `segment_stamp_steps` sets 0x11f0 to the minimum and reads 32 bytes; the window writes match the play of a
  normal batch otherwise.
- Header parser: words 5..7 to a 48-bit value (word 5 high), tested on a synthetic header.
- `bus::Recorder` replay: a scripted register file where 0x4010 advances per poll; the thread logic stops at N and at a
  user stop.

## 5. Hardware test checklist

1. 0x40b8 bit 1 after `ts_first_clear` is 0, and becomes 1 at the first trigger. 0x40c0/0x40c4 non-zero.
2. Pulse source at 1 kHz, N = 100, 1,000 points: all frames recorded, stamp deltas 1 ms. Derive the tick length from
   the measured deltas (expect 250 ps).
3. Compare header words 5..7 of each frame with the latched 0x40b0/0x40b4 after a single-frame play of the same frame.
4. Pulse pairs 2 us apart: both pulses land in consecutive segments (dead-time check).
5. 0x4068 interval = 1 ms with a 10 kHz source: frames 1 ms apart.
6. Stop at k < N: progress and the playable range match k.
7. Largest N at 1,000 points (61,440) records without overwriting frame 0 (compare frame 0 before and after).
8. Minimum-TX single-frame play: the 32-byte read completes and the FSM returns to stopped (0x4034 bit 13).
9. Segments with 2 and 4 channels and with the LA off; LA segments are out of scope.

## 6. Confidence and open questions

Confidence: arm and play sequence high (same engine OOS uses). Timestamp registers and unit medium (names and scaling
from the library, no capture). Header words 5..7 low until 5.3.

1. Which frame does the 0x40b0 latch refer to: the last played frame, the last recorded frame, or "now"? The stock
   code latches right after a single-frame play, which fits "last played".
2. Are header words 5..7 the trigger timestamp?
3. Tick length: 250 ps exact, or 4 ns with fraction bits that need masking?
4. Does 0x4068 also apply between frames of a normal live batch (the stock code writes 0 there)?
5. Real frame memory size: 16,777,216 bursts (stock) or larger (OOS deep-record finding).

## 7. Implementation status (2026-11-12)

Fork commits 45dc87b (HAL), d925518 (oos-ui). Host-tested only. Nothing ran on the unit. Hardware steps:
`docs/fork/unit-dho804.md`, section "Hardware segments".

| item | state | where | default |
|---|---|---|---|
| record arm, interval | done | `scu::record_arm` (the 29 writes of `arm`, 0x4068 = `interval_ticks`), `seq::State::record_steps` (stamp clear first), fixture `hal/testdata/record-arm-writes.txt` (written by hand from 1.2) | - |
| stamp registers | done | `scu::ts_first_clear`, `scu::ts_latch`, `scu::stamp`, shadow `Shadows::s40b8` | - |
| limits | done | `hz::max_record_frames` (16,777,216 - 1 M bursts over the segment size), `tools/hzmodel.py`; UI cap 100,000 | - |
| progress, stop | done | `seg::status` (0 until 0x4080 bit30, then 0x4010 - 0x4014 + 1; running = `(0x4034 & 0x2320) == 0`), `seg::stop` = `disarm_steps` | - |
| stamps | done | `seg::stamps`: one stamp play per segment (`segment_stamp_steps`), header words 5..7 and the latched 0x40b0/0x40b4 of that play; `seg::Timeline` | - |
| read-back | done | `seq::State::read_steps_at` (type-4 play from frame `first`), `seg::read`, `hw::seg_read_plan` | - |
| model | done | `seg::sim::Engine`: record engine, plays, header and latch, for host runs of the whole sequence | - |
| oos-ui | done, opt-in | Acquire > HW segments, Seg. interval, Segment times (table, CSV export through the table); setup keys `seg_hw`, `seg_interval`; SCPI `:ACQuire:SEGMents`, `:HARDware`, `:INTerval`, `:TIMes?` | off |

Decisions against the proposal:

- No `Step::ReadPair`. The stamp pair is read by `seg::read_pair` on the `Bus` after the steps, low word first. The
  generic executor `seq::execute_bus` runs the segment steps on any `Bus` and C2H reader; `execute_into` uses the same
  per-step code, so the verified write order of the live path is unchanged (`tools/plan-matrix.sh` output identical
  before and after).
- The stamp play does not use an 8-point TX length. The stock raw export never writes 8; it turns it into 128 / ratio
  (record-play.md 7.1). The stamp play reads 128 words of all channels plus the header: 272 bytes.
- Every segment gets a stamp play, also when the frames come with headers. The latch is what the stock record playback
  uses; header words 5..7 are read from the same play and logged next to the latch for the first 8 segments (check 3).
  `Timeline` takes the latch when present, else the header. Cost: one play per segment, about 0.5 ms, so 61,440
  segments take about 30 s to stamp.
- Units: latch 250 ps (1.1), header 1.6 ns (trigger.md 7). The two sources disagree on paper; check 2 decides. Header
  stamps are unwrapped at 48 bits segment by segment; latched stamps are not unwrapped (64 bits).
- The read-back delivers frames in batches of at most 48 (half the UI queue) and waits for the screen, so no segment is
  dropped from the queue. After STOP during recording only the stamps are read; the segments stay in FPGA memory and are
  played one at a time when browsed (history keys, table rows). The RAM history keeps 200 records in this mode.
- Hardware segments are off in roll, scan and with the LA on (`hw::hw_segments_ok`); the software history records
  instead. Any settings change, self-cal or generator change ends a recording and forgets its segments.
- The model answers the open questions one way (latch = last frame of the last play, header = trigger time in 1.6 ns).
  It proves the sequence is consistent, not that the unit behaves so.

Not done: frame export with the stamps per segment (the table export has index and time only), averaging-specific
segment layout (01), LA segments.
