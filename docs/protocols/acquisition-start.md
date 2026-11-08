# Acquisition start (RUN / SINGLE / STOP, auto trigger)

Status: arming, the done flag, batches, deep records and the read rules at the end of this file are verified on the unit where
they say so. Anything else is not yet verified on hardware. "Assumed" marks an interpretation.

The read-out is the **play** path (0x400c play mode, then FSM start). The FSM replays a stored frame. A new acquisition needs the
**record** set-up (0x400c record bits + the frame window in 0x4010/0x4014/0x4018) before the FSM start. So a repeated read-out
gives no new frames. Every frame needs a full stop/arm cycle, also in RUN.

Conventions below: `sN` = the software copy of register N (last value written; for 0x4034 and 0x4080 the copy holds the last
value *read*). "RMW" = write the copy with bits changed. Time values are in **femtoseconds**, the register ticks are 4 ns,
6.4 ns and 1.6 ns.

## 1. Sequences and register writes

### States

| operation | what it does |
|---|---|
| STOP | read 0x4034 and clear the status (2), stop sequence, clear the averages (0x10b4 pulse), read the recorded range (`roll-mode.md` 4.1) |
| RUN | configuration, run sequence with type 2 |
| SINGLE | stop sequence, display clear, clear the averages, configuration, run sequence with type 3 |
| re-configuration | stop sequence, then RUN again (SINGLE if the sweep is single) |
| scan sweep done | stop sequence, configuration (`roll-mode.md` 5) |

Run sequence types: 2 RUN, 3 SINGLE, 4 play window, 5 play last, 11 record (segmented memory, not needed here).
Trigger sweep: AUTO, NORMAL, SINGLE.

### Configuration (once per RUN/SINGLE command, not per frame)

Compute the parameters (`hz::prep`), then for RUN/SINGLE the SPU filter (ADC part), the acquisition block, the horizontal block,
WPU, WAU, and always the SCU block.

- Acquisition block (`acq::configure`): channel mode, ADC order, LA enable, analog/LA stream select (0x4008 bit23/24),
  channel sample delays, pre-processing reset, invert, band filter, sample rate (0x10d0-0x10d8), LA sample rate, acquire mode,
  average parameters, pre-sample compensation (0x404c), anti-alias, record depth (0x10e4 = memsel), pre/post (0x4050/0x4054),
  **segment size (0x10e0)**, DVM, and in some branches the channel scale (AFE, out of scope here).
- Horizontal block: wave range (0x1100/0x1108/...), sinc/linear interpolation, delays, compression,
  **TX length: 0x11f0 = roundup8(len)** (live TX size per channel group).
- SCU block: zoom enable, roll/scan bits (0x4008 bit17/22), **auto trigger** (section 3).

All of these are configuration registers that keep their value. After a warm hand-over they are still what the stock system set.

### Run sequence (per command AND per frame)

```
chmask = the 0x4008[3:0] nibble (1 = CH1 only, 0xf = all)
stop sequence
if roll or scan: FSM start with chmask; return                 ; roll/scan: FSM start only
s     = ping-pong slot 0/1
n     = (type == 3) ? 1 : frames per batch (>= 1)               ; also 1 in mask/pass-fail mode
first = (s == 0) ? 0 : max_frames
slot[s] = {first, first, first+n-1, segment size}
set_run(chmask, type, memsel, n, w4=first, w5=first, w6=first+n-1, x7=0, stack=0)
```
(stack = the ultra acquisition value, in ultra mode only. One case needs a single extra 20 ms sleep here, its condition is not known.)

Evidence for the slot model, verified on the unit: in STOP 0x4014 = 0x1380, 0x4010 = 0x141f (160 frames from max_frames 0x1380);
while running 0x4010/0x4014 alternate between {0..0x9f} and {0x1380..0x141f}.

### Set-up `set_run(fsm, type, memsel, n, w4, w5, w6, x7, stack)`

```
cnt = max(1, min(n, w6 - w5 + 1))
0x4008 = s4008 &= ~(1<<17)          ; roll off
0x4008 = s4008 &= ~(1<<22)          ; scan off
switch type:
  2, 11: record_seq(x7, w5, cnt, stack)       ; RUN
  3:     record_seq(x7, w5, cnt, 0)           ; SINGLE
  4:     play window (below)                  ; read-out of stored frames
  5:     0x4004 = s4004 &= ~4; 0x4004 = s4004 |= 4   ; play-last pulse
  6..10: nothing
0x4008 = s4008 = (s4008 & ~0xf) | fsm
0x4004 = s4004 |= 1                 ; FSM start
```

`record_seq(interval, first, cnt, t)` (RUN/SINGLE):

| # | register | value | note |
|---|---|---|---|
| 1 | 0x406c | `(t >> 11) / 3125` (= t / 6.4e6) with bit31 = (t > 6399999) | 0 in normal RUN |
| 2 | 0x4068 | `interval / 6.4e6` | 0 in normal RUN |
| 3 | 0x400c | `s400c = (s400c & 0x89ffffff) \| 0x30000000` | clears bits 30,29,28,26,25, sets 29+28 |
| 4 | 0x4010 | first | |
| 5 | 0x4014 | first | |
| 6 | 0x4018 | first + cnt - 1 | last frame index of the batch |
| 7 | 0x400c | `s400c &= ~0x10000000` | bit28 low again: bit28 is assumed to be the window load strobe |
| 8 | 0x400c | `s400c \|= 0x22000000` | record-seq mode (bit29 + bit25) |

Type 4 (play window): 0x4068 = x7/6.4e6, 0x406c = 0 (copy too),
0x400c = `(s & 0x89ffffff) | 0x30000000`, 0x4010 = w4, 0x4014 = w5, 0x4018 = w6, 0x400c &= ~bit28,
0x400c = `(s & ~bit26) | 0x60000000` (play mode, bit30 + bit29).

0x400c bits: bit31 segmented record (0xa0000000 | bit26), bit30 play, bit29 enable (assumed), bit28 window strobe,
bit26 option flag, bit25 record-seq (normal acquisition ring), bit24 seq play. Values: 0x22000000 in STOP (last write = arm),
0x60000000 while running (last write = play of a frame).

### Stop sequence `stop(f)`

```
poll 0x4034 bit13 (FSM stopped): if 0, sleep 1 ms and read again, at most 10 reads (~10 ms); continue anyway
set_stop(0)                      ; 0x4004 = s&0x7ffffffe ; 0x4004 = s ; 0x4004 = s&0x7ffffffb
if f: set_stop(1); info clear    ; set_stop(1) = same 3 writes with bit31 set in the 2nd; 0x4080 bit31 pulse
else: 0x4004 bit30 pulse
SPU reset                        ; 0x1000 bit0 pulse
TX reset                         ; 0x1000 bit4: clear, set, clear
set_stop(0)
```
`stop(1)` does **not** pulse 0x4004 bit30, only `stop(0)` does. "Stop sequence" without an argument means `stop(1)`
(`scu::stop_sequence`, 17 writes plus the poll, see `record-play.md` 1.4). `set_stop` clears 0x4004 bit2 (play-last), so an arm after a
read-out starts clean.

### SINGLE / STOP

- SINGLE = RUN with type 3 and n = 1; register-wise identical to a type-2 arm with cnt = 1. After the first successful
  frame go to STOP.
- STOP = stop sequence (plus a status read and 0x4034 clear for the UI). Nothing else is written.

## 2. Detecting the finished acquisition, fetching the frame, re-arming

Completion is polled. OOS uses `/dev/xdma0_bypass` and `/dev/xdma0_c2h_0` only, and the read is a plain blocking `read()`.

### Run loop

At start: 0x1010 bit31 = 0 (header on). Loop while RUN or SINGLE and channel mask != 0:
1. Read a frame (analog; the chunk read for roll/scan, `roll-mode.md` 2.2).
2. Result:
   - frame read: for RUN, not roll: **run sequence = immediate re-arm**, no sleep. For SINGLE: STOP.
   - nothing ready: sleep 9 ms and try again; SINGLE: 10 ms.
   - others: retry immediately.

### Frame read (analog)

```
0x4008 bit23 = 1, bit24 = 0                    ; analog stream on, LA stream off
cnt = frame request (below)                    ; < 1 -> nothing ready (sleep 10 ms first if timebase > 1 ms/div)
if len < 8 -> nothing ready
read(c2h, cnt * (16 + 2*ratio*len [+ 16 + 2*ratio*zoomlen if zoom]))
de-interleave, header = 8 words                ; 16-byte header per frame
```
with ratio = the interleave factor of the channel set, len = the analog display length. With 0x1010 bit31 = 0 the FPGA sends a
16-byte header in front of every frame (live path); with bit31 = 1 there is none (export path; the read-out verified on the
unit has no header). So bit31 = 1 means "no header".

### Frame request (RUN, SINGLE, record)

```
read: valid = 0x4080 bit30, count = rd(0x4010) - rd(0x4014) + 1
(record or ultra mode: if (rd(0x4034) & 0x2320) == 0 -> valid = 0)   ; still running
if !valid: return 0               ; nothing ready -> poll again in 9 ms
if count < 0: count = 0
stop sequence
s = cur slot; mark slot s full, store count; cur slot = !s
count = min(count, frames per batch);  SINGLE: count = 1
set_run(fsm=chmask, type 4, memsel, count, w4=slot[s].first, w5=slot[s].first, w6=first+count-1, x7=0)
return count
```
The live path runs the stop sequence a second time before `set_run` (`record-play.md` 1.3).

So **done = 0x4080 bit30 set**, polled every ~9 ms. The number of frames is 0x4010 - 0x4014 + 1 (the FPGA updates the
0x4010 read-back to the last written frame). The stop sequence clears it again (info clear, 0x4080 bit31 pulse).

### Per-frame cycle in RUN (summary)

```
arm:   stop sequence; set_run(type 2) = 0x4008 roll/scan off, record_seq (8 writes), 0x4008[3:0] = chmask, 0x4004 |= 1
wait:  poll rd 0x4080 bit30 every 9 ms
play:  stop sequence; set_run(type 4) = play window 0x4010/14/18 + 0x400c play, 0x4008[3:0], 0x4004 |= 1
read:  one read() of cnt*(16 + 2*ratio*len) bytes from /dev/xdma0_c2h_0
arm again (ping-pong slot toggled)
```

### 0x4034 status (read; write only through the status clear)

| bit | use | observed |
|---|---|---|
| 13 | FSM stopped/idle; the stop sequence waits for it | STOP = 0x2000 |
| 4 | status 3 (lowest priority) | RUN 0x12 (armed, waiting for trigger) |
| 7 | status 2 | in 0x31c0 / 0x11c7 |
| 6 | status 4 (highest priority) | in 0x31c0 / 0x11c7 |
| 5 | status 0 | |
| 5, 8, 9, 13 | none set = acquisition still running | |
| 31, 30 | status clear (a, b): write `(last_read & 0x3fffffff) \| a<<31 \| b<<30`, then `& 0x3fffffff` | |

Status priority: bit6 -> 4, bit7 -> 2, bit5 -> 0, bit4 -> 3, else 1. Assumed names (the `:TRIG:STAT?` set): 4 AUTO, 2 TD
(triggered), 3 WAIT, 1 RUN, 0 STOP. The UI status path uses the status clear with a = b = 1, it is not part of the arm/read cycle.
Other observed values: 0x31c0 (bits 6,7,8,12,13), 0x11c7, 0x7, 0x12.

## 3. Trigger set-up the start depends on

### SCU auto trigger

| register | value |
|---|---|
| 0x4020 | `s4020` with bit0 = auto (sweep == AUTO); RMW of the low halfword. Hand-over value 0x2001 in RUN and STOP (bit13 from the mask functions) |
| 0x4060 | t1 / 4e6, t1 = clamp(20 x timebase, 1e13, 5e14) fs = 10 ms .. 500 ms (4 ns ticks) |
| 0x4064 | t2 / 4e6, t2 = max(holdoff, clamp(10 x timebase, 1e14, 2.5e14)) fs = 100 .. 250 ms |

Timebase in fs/div. Example 200 us/div: 0x4060 = 2500000 (0x2625a0), 0x4064 = 25000000 (0x17d7840).
NORMAL sweep = the same writes with bit0 = 0. `scu::auto_trigger` writes these.

### TPU edge trigger (source CH n = 1..4, slope rising)

| step | register | edge CH1 rising |
|---|---|---|
| time condition A off | 0x201c[26:24] = 0, 0x2020 = 0, 0x2024 = 0, 0x2028 = 0 (time limits / 1.6e6) | |
| time condition B off | 0x2034[26:24], 0x2038, 0x203c, 0x2040 = 0 | |
| SPU bypass | 0x205c bit12 = (coupling != DC or source > 8) | 0 |
| source A (a, ea, b, eb) | 0x201c[5:0]=a, [7:6]=ea, [13:8]=b, [15:14]=eb (a = 2n-2, b = 2n-1; falling swaps them) | 0x4100 |
| pattern A (2,1,2,1) | 0x201c[23:16] = 0xaa (falling: 2,0,2,0) | |
| mode 0 | 0x205c[4:0] = 0 (1 = both slopes via A and B) | |

Result 0x201c = 0x00aa4100, 0x205c = 0. Level: register CH1 0x2004, CH2 0x200c, CH3 0x20b8, CH4 0x20c0 (level B
0x2008/0x2010/0x20bc/0x20c4), upper code in [15:0], lower code in [31:16]. Code = 32768 + (level + offset) x 7500 / scale / g,
g = the channel's digital gain, clamped 1..0xfffe (7500 codes per division = the preamble's yinc of 1/7500 div). See `trigger.md`
2.3 for the exact formula with the hysteresis. Holdoff: 0x2088[18:16] = 4 (0x40000), 0x2084 / 0x2088[15:0] = holdoff / 6.4e6.
Force trigger: 0x205c bit5 pulse. The TPU registers are write-only (0x2034 reads 0).

## 4. OOS sequence: one auto-trigger acquisition with the left-behind configuration

Precondition: warm hand-over, the stock system configured RUN once (all configuration registers, TPU, auto trigger, TX size
in place), FPGA in STOP. OOS tracks the copies itself, starting from the hand-over values (`scu::Shadows::HANDOVER`):
s4004, s4008, s400c, s1000, s4080 (s4080 = last read of 0x4080). `chmask` from the settings, first = 0, n = 1.

### A. Arm (= run sequence, type 2, slot 0, n = 1), 29 writes

`scu::arm`. Verified on the unit.

| # | reg | value |
|---|---|---|
| 0 | read 0x4034 | until bit13 set (stop poll; OOS allows 30 ms, read only) |
| 1-3 | 0x4004 | s&=0x7ffffffe; s; s&=0x7ffffffb (`set_stop(0)`) |
| 4-6 | 0x4004 | s&=0x7ffffffe; s\|0x80000000; s&=0x7ffffffb (`set_stop(1)`) |
| 7-9 | 0x4080 | s&=0x7fffffff; s\|0x80000000; s&=0x7fffffff (info clear) |
| 10-11 | 0x1000 | s\|=1; s&=~1 (SPU reset) |
| 12-14 | 0x1000 | s&=~0x10; s\|=0x10; s&=~0x10 (TX reset) |
| 15-17 | 0x4004 | `set_stop(0)` again |
| 18 | 0x4008 | s &= ~(1<<17) |
| 19 | 0x4008 | s &= ~(1<<22) |
| 20 | 0x406c | 0 |
| 21 | 0x4068 | 0 |
| 22 | 0x400c | s = (s & 0x89ffffff) \| 0x30000000 |
| 23 | 0x4010 | 0 (first) |
| 24 | 0x4014 | 0 (first) |
| 25 | 0x4018 | 0 (first + n - 1) |
| 26 | 0x400c | s &= ~0x10000000 |
| 27 | 0x400c | s \|= 0x22000000 |
| 28 | 0x4008 | s = (s & ~0xf) \| chmask |
| 29 | 0x4004 | s \|= 1 |

Writes 1-17 are `scu::stop_sequence`, the same as the first part of the read-out. 0x4008 bit23 (analog stream) stays as the read-out
left it; it is also set while running (0x4008 = 0x4800001 observed).

### B. Wait

Poll `rd(0x4080)` every 5-10 ms until bit30 = 1 (store the read value as s4080), then `count = rd(0x4010) - rd(0x4014) + 1`
(expect 1). In AUTO the FPGA triggers by itself after t1 (10 ms at 200 us/div), so the wait should end within
~10 ms + pre/post-trigger time. OOS waits at least 2 s, and at least twice the record time + 500 ms. Diagnostic only: `rd(0x4034)` (bit4
waiting, bit6/bit7 triggered, bit13 FSM idle). If it never completes in NORMAL mode: 0x205c bit5 pulse (force trigger).

Verified on the unit: the arm works, done comes after 162 ms in that test and the FSM stops with 0x4034 = 0x3180.

### C. Read

**Before any raw read the pass-through writes are required** (`scu::passthrough`, `record-play.md` 6.1). Verified on the unit: without
them a read after an own arm stalls in all variants below, with and without a frame header: every play moves 0x4034 to 0x1187 and
the c2h read times out. The cause is the live compression, which leaves fewer TX points than 0x11f0 asks for (`record-play.md`
section 0).

Variant 1 (play last, `scu::readout`): the 28-write read-out, then one `read()` of 2N bytes. It plays the last recorded frame via
the play-last pulse. Its `0x400c &= ~bit25` turns the record-seq value 0x22000000 into 0x20000000 before the play-last. That
play-last plays the frame just recorded by A with bit30 clear is assumed.

Variant 2 (type-4 play, `scu::readout_play4`, the default in `seq::Settings`): window writes (analog stream on: 0x4008 bit23 = 1,
bit24 = 0; 0x1100, 0x1108, 0x11f0; 0x1010 bit31 = 1 for no header; 0x400c bit25 off), then the stop sequence (writes 1-17 of table
A), then the type-4 play: 0x4008 &= ~bit17, &= ~bit22, 0x4068 = 0, 0x406c = 0, 0x400c = (s & 0x89ffffff) | 0x30000000,
0x4010 = 0, 0x4014 = 0, 0x4018 = count - 1, 0x400c &= ~bit28, 0x400c = (s & ~bit26) | 0x60000000,
0x4008 = (s & ~0xf) | chmask, 0x4004 |= 1; then one read of exactly 2 x ratio x 0x11f0 bytes.

Live-equivalent variant (no OOS window): leave 0x1100/0x1108/0x11f0 as the horizontal block set them, 0x1010 bit31 = 0,
and read `count * (16 + 2*ratio*len)` bytes with an 8-word header per frame. Needs `len`.

### D. Next frame

Repeat A (it begins with the stop sequence). Two slots, slot 0 (first = 0) and slot 1 (first = max_frames), keep the previous batch
alive. OOS uses slot 0 only, which is simpler. Throughput note: at fast time bases read several frames per batch in one
`read()` (see "Batches of frames on the unit").

### Out of scope (not touched by A-D)

AFE (channel scale in the acquisition block), ADC SPI (0x1034), clocks (LMK/LMX), SPI flash, calibration files.
A-D only use 0x1000, 0x4004, 0x4008, 0x400c, 0x4010-0x4018, 0x4068, 0x406c, 0x4080 (+ the read-out's 0x1010, 0x1100,
0x1108, 0x11f0).

## Not yet verified on hardware

1. Whether the read-out's own 0x1100/0x1108/0x11f0 values affect the next recording. They are read-out window and TX size, and
   the live loop does not rewrite them per frame.
2. Exact 0x4034 bits 0-2, 8, 9, 12 and whether bits 6/7 are sticky until the status clear (the arm cycle never clears them).
3. 0x400c bit29 and bit26 meaning; 0x4064 (t2) role (auto re-trigger period?).

## Batches of frames on the unit

`oos-fpgad run --frames 100` (1 us/div, CH1, 12 500 points per frame, probe comp 1 kHz):

- The done flag 0x4080 bit 30 comes with the **first** frame (1.2 ms), not with the whole batch; the FSM keeps
  recording into the ring (0x4034 = 0x11d2) and 0x4010 reads back the last recorded frame. Playing all 100 frames right
  after done returned 3 real frames and 97 frames of old memory.
- `seq::acquire_batch` therefore waits after done until `0x4010 - first + 1` reaches n or `wait_ms` (30 ms) pass, then
  builds the read-out for exactly that count (type-4 play with 0x4018 = first + count - 1, one read of count frames):
  **31 frames in 30 ms** (the 1 kHz signal gives one trigger per ms), all different, trigger edge at points
  6137..6206 of every frame (pre-trigger 6250); the 775 000-byte read took 8.6 ms.
- So one batch costs ~10 ms plus the time the triggers need: about 800 records/s with the probe-comp signal instead
  of ~30 with single frames. `frames` is capped by `max_frames` = 7 340 032 / segment size.

## Memory depth beyond 50 M on the unit

`oos-fpgad run --depth D --tdiv 1e-2 --at A` (CH1, probe comp 1 kHz), 4000-point windows at the start, around the
trigger and at the end of the record:

| depth | rate | start | trigger | end |
|---|---|---|---|---|
| 125 M | 1.25 GSa/s | 21 117..21 531 (low) | 21 962..39 350 (edge) | 21 100..32 450 (next edge begins) |
| 100 M | 625 MSa/s | low | edge | next edge begins |
| 62.5 M | 625 MSa/s | high/edge | edge | next edge begins |

The end windows lie a whole number of signal periods after the trigger (50 ms / 80 ms) and show the beginning of the
next edge, so the end of the record is written by the acquisition, not old memory. (Run with the segment still capped at
7 864 320 bursts, see "Time axis of compressed reads by depth".) `hz::prep` accepts these depths. For the GUI, reading such records
completely (250 MB at 125 M) is not an option: the live path lets the FPGA compress to ~625 k points per frame (0x1170 compression k,
window memsel + 0x248, 0x11f0 = len; `horizontal-config.md`, `record-play.md` section 3.1).

## FPGA compression = min/max (peak) decimation

`oos-fpgad run --depth 1000000 --tdiv 2e-4 --all` (full, 1 M points, 2 MB) and the same with `--compress 4`
(`seq::Settings::compress`: 0x1170 = 0xc0000004, window = whole record, 0x11f0 = ceil(memsel / k) rounded up to 8 =
250 000; 500 kB), verified on the unit:

- rising edges full at 187 621 / 500 121 / 812 621, compressed at 46 903 / 125 029 / 203 155 (x4 = 187 612 /
  500 116 / 812 620): one compressed point per k samples, aligned with the record;
- around the trigger the compressed points alternate low/high (31 726, 31 967, 31 864, 32 157, ...): consecutive
  pairs are the minimum and maximum of a block, i.e. the FPGA does peak-detect decimation (noise of a flat part 127
  codes compressed vs 88 full: the envelope).

So the live path gives a min/max envelope at large depths. For OOS: read huge records for the screen with k so
that ceil(memsel / k) stays near 625 k points (2.5 M at k = 4 gives this value), draw the points as a polyline
(min-max pairs become vertical spans), and read uncompressed windows only for zoom / measurements on raw samples.
Not checked yet: the exact block size of one min/max pair (2k samples?) and the extra + k term in 0x1100 of the live set-up (OOS
writes memsel + 0x248, which works).

### Time axis of compressed reads by depth

`oos-fpgad run --tdiv 1e-2 --depth D --all --compress ceil(D / 625 000)`, probe comp 1 kHz, period measured over all
edges of the 625 000 compressed points, segment capped at 7 864 320 bursts:

| depth | rate | k | frequency | k effective |
|---|---|---|---|---|
| 10 M | 62.5 MSa/s | 16 | 1000.000 Hz | 16.0000 |
| 50 M | 312.5 MSa/s | 80 | 1000.000 Hz | 80.0000 |
| 62.5 M | 625 MSa/s | 100 | 1000.000 Hz | 100.0000 |
| 100 M | 625 MSa/s | 160 | 1001.076 Hz | 160.1722 |
| 125 M | 1.25 GSa/s | 200 | 1000.864 Hz | 200.1728 |

Up to 62.5 M the compression is exact. The deviation at 100 M and 125 M is a **segment wrap, not compression**. The edges of
100 M / 125 M records are regular except for **one phase jump at a fixed point** (compressed point 133 481 at 100 M, 231 785 at
125 M, the same in every acquisition). With a memory size of 16 777 216 bursts the segment size 0x10e0 is capped at
(16 777 216 - 0x100000) / 2 = 7 864 320 bursts = **78 643 200 points**, so a deeper record wraps inside its segment and its first
L - 78 643 200 points are overwritten by later samples. Prediction: jump at (L - 78 643 200) / k = 133 480 / 231 784, size
period - (78 643 200 mod period) = 106 800 samples at both depths; measured 133 481 / 231 785 and ~106 800. The linear fit over all
edges spread this jump as "0.1 %".

With a memory size of 0x5000000 bursts (`hal::dho::hz::MEM_BURSTS`) the cap is 41 418 752 bursts: 100 M and 125 M get
0x10e0 = 0x1000000 (2^24 bursts = 167.8 M points). Verified on the unit: 100 M mean edge spacing 3906.253 compressed points
(expected 3906.25), 125 M 6250.000 (expected 6250), no jump. oos-ui offers 100M / 125M (1 channel). The frame-ring cap of the batch
path (`max_frames`) keeps the 16 777 216-burst memory size.

## C2H read rules found on the unit

1. **Frames are padded to 8 bytes in the stream.** A play-out of N frames of `b` bytes delivers N x roundup8(b)
   bytes. 10 MHz sine, batches of 4..6 frames at 1.25 GSa/s: 1250 points (100 ns/div) come at a stride of 1252 points,
   625 points (50 ns/div) at 628; at that stride the sine has the same phase in every frame (+-1 sample of trigger
   jitter), at the unpadded stride it walks by 2 / 3 samples a frame. 2500 points need no padding. If the padding is ignored,
   every later frame of a batch is shifted and the screen shows two traces and broken edges at <= 100 ns/div.
   `seq::frame_stride` / `read_len` / `unpad`: the read takes the padding, `unpad` drops it, so consumers still see
   `frame_bytes` per frame. Assumed, not measured: the rule for 2-channel frames (the words of all channels together rounded to
   8 bytes).
2. **The read buffer must not share cache lines with other heap data.** The RK3399's PCIe is not cache coherent and
   the XDMA driver maps the user pages for every read. With 2500-byte reads into a plain `Vec` at <= 100 ns/div the heap gets
   corrupted within seconds. `bus::read_dma` reads into a 4 KiB aligned, page-padded region and moves the data to the front; every
   C2H read in OOS goes through it.
3. Remaining at <= 100 ns/div: the record is exactly one screen (memsel = 10 divisions), so after the software trigger
   moves the record by a few samples the last few points of the screen have no data (amplitude of the last eighth 4 %
   low at 100 ns/div). Fix (host-tested, not yet verified on hardware): at Auto depth below the live depth oos-ui
   asks for an eighth more memory (at least 64 points, a multiple of 8) at the same full rate (`hw::screen_margin`),
   e.g. 1408 instead of 1250 points at 100 ns/div; `hz::prep` centres it on the trigger (pre 704). 1408 points are
   2816 bytes, so a one-channel frame needs no stream padding either. Rare single frames
   with a small wiggle on a peak are still visible with persistence; not explained yet.
