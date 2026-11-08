# Roll and scan mode (slow time bases)

Status: sections 8 and 9 hold the results on the unit: the roll stream (sequence of section 6) and scan mode are verified there.
Anything else is not yet verified on hardware unless it says so. "Assumed" marks an inference.

Builds on `acquisition-start.md`, `record-play.md` and `horizontal-config.md`; conventions as there: times in fs, `sN` = software
copy of register N, memsel = record length, ratio = interleave factor, dot = sample period, len = 0x11f0 length.

## 0. Result in short

1. Roll and scan are a separate acquisition mode of the FPGA. There is no arm/trigger/play cycle. The SCU block sets 0x4008
   bit17 (roll) or bit22 (scan). The start is only the stop sequence and the FSM start (0x4008[3:0] = mask, 0x4004 |= 1).
   After that the FPGA streams fixed-size **chunks** into the c2h FIFO all the time. The record set-up and
   0x4010/0x4014/0x4018 are not written.
2. Each read: pulse 0x1010 bit28, read **0x11e8** = FIFO fill level in 8-byte units. Then read as many whole chunks as are
   there. One chunk = 8-word (16-byte) header + 100 data words (all channels interleaved, 100/ratio points per channel),
   i.e. **216 bytes**. If 0x11e8 = 0, sleep 9 ms. Otherwise read and loop at once.
3. Display: a sliding buffer of 1000 chunks per channel, newest data at the right end, older data shifted left
   (2.4). The time per point is 10 x ratio x k x dot / f (k = compression, f = linear interpolation
   factor, see 3). With the parameters of 3.2 one chunk is one screen column (time/div / 100) in most single-channel cases.
4. Register differences from a normal RUN: the acquisition block is the same (analog). In the horizontal block the window covers the
   whole memory (0x1108 = 0, 0x1100 = memsel + 0x248 + k), **0x1170 = 0xc0000000 | 10·ratio·k**, **0x11fc = 100/ratio**
   (normally 1000), the linear interpolator is switched on with a fraction that brings the stream to 1000 points per column
   before the /10, and 0x11f0 = roundup8(len). SCU block: zoom off, mode bits cleared, roll bit. Auto-trigger registers are
   written as usual.
5. Stop = stop sequence + average clear + read 0x1208 (number of recorded points, bit31 = memory full). The roll bit stays set.
   It is cleared when the time mode leaves roll and by the prologue of the next normal arm.
6. Switching rule (only while running): **Auto Roll on → roll from 50 ms/div**. Auto Roll off → **scan from
   200 ms/div** (not with average mode). The states RUN, SINGLE and STOP are the normal ones. The time mode is separate: YT, XY, ROLL,
   SCAN. OOS rolls from 200 ms/div (section 8).

## 1. When to switch to roll / scan

### 1.1 Time modes

YT, ROLL, SCAN (and XY, which plays no role here). A change of the time mode needs a stop, reconfigure and re-arm cycle.

### 1.2 Decision

This runs only while running, and also on RUN. Thresholds: 5e13 fs = **50 ms/div**, 2e14 fs = **200 ms/div**.

| current mode | Auto Roll | new time base | new mode |
|---|---|---|---|
| YT | on | >= 50 ms | ROLL |
| YT | off | >= 200 ms and scan allowed | SCAN |
| ROLL | on | < 50 ms | YT (offset reset to 0, wave clear) |
| ROLL | off | < 200 ms → YT; >= 200 ms → SCAN | |
| SCAN | off | < 200 ms | YT |
| SCAN | on | >= 50 ms | ROLL |

Scan allowed = acquire mode is not average. It is not checked on the Auto Roll path. A further condition (assumed: mask or math on)
is not known exactly. `hw::slow_mode` in oos-ui follows this table with `hw::ROLL_TDIV` and `hw::SCAN_TDIV`.

### 1.3 Side effects of entering ROLL/SCAN

- ROLL: if zoom is on, switch it off.
- ROLL and SCAN: acquire mode ultra or average → normal. Peak stays.
- Memory depth: Auto stays Auto, a fixed depth below 1 M becomes 1 M, the limit is 125 M.
- Rates and memsel are **not** special in roll: the normal formula runs (`hz::prep`). The only roll thing is a flag that is set when
  the legal-rate remap changed the rate *and* the mode is roll/scan. That flag drives the interpolation correction in 3.2.

### 1.4 States

Unchanged: RUN, SINGLE and STOP work as in `acquisition-start.md`. Only the read differs: the chunk read of 2.2 replaces the frame
read. "Scan sweep done" (section 5) exists only in scan.

## 2. Register sequence: start and read

### 2.1 Start (RUN in roll)

Same order as normal: parameters, ADC part, acquisition block, horizontal block (roll branch, section 3), WPU, WAU,
(scan only: 0x4058), SCU block. Then the run sequence.

SCU block, roll part:

| # | register | value |
|---|---|---|
| 1 | 0x4008 | bit25 = zoom (0 in roll) |
| 2 | 0x400c | `s400c &= ~0x42060000` (bits 30, 25, 18, 17) |
| 3 | 0x4008 | `s4008 &= ~0x00720000` (bits 22, 21, 20, 17) |
| 4 | 0x4008 | bit22 = 0 |
| 5 | 0x4008 | bit17 = 1 |
| 6-8 | 0x4020, 0x4060, 0x4064 | auto trigger as in normal mode (`acquisition-start.md` 3) |

Scan: rows 4/5 become bit17 = 0, then bit22 = 1.

Run sequence, roll/scan branch:
```
stop sequence                     ; poll 0x4034 bit13 (max ~10 ms) + 17 writes (record-play.md 1.4)
0x4008 = (s4008 & ~0xf) | mask ; 0x4004 = s4004 | 1
```
The normal set-up with its prologue does not run, so the roll bit is **not** cleared and 0x400c / 0x4010-0x4018 / 0x4068 / 0x406c
are not written. After row 2, 0x400c is 0x20000000 if the last normal cycle left 0x60000000 or 0x22000000.
0x1010 bit31 = 0 (frame header on) is written once at the start of the run loop. Nothing in roll changes it, so roll chunks have
headers.

### 2.2 One read

```
0x1010 = s1010 & ~bit28 ; 0x1010 = s1010 | bit28 ; 0x1010 = s1010 & ~bit28     ; latch strobe (assumed)
tx = rd(0x11e8)
if tx == 0: nothing ready                               ; caller sleeps 9 ms
words = 4 * tx                                          ; 0x11e8 counts 8-byte units
n     = 100 x requested time base / roof time base  (= 100 at 1-2-5 time bases)
        (a fine time base that is not a multiple of 4 %: n = 100)
chunk = n + 8                                           ; words per chunk incl. 8-word header
cnt   = words / chunk          (LA on: rounded down to even)
read(c2h_fd, cnt * chunk * 2)                           ; one blocking read(); < 1 → short read
de-interleave: len = n / ratio, count = cnt, header = 8 words   ; same layout as live frames
roll: append to the display buffer (2.4)
scan: rd 0x1204, rd 0x40d0 (section 5)
```
Read size at n = 100: **216 bytes per chunk** for every channel count (100 words = 100/ratio points per
channel, interleaved like `horizontal-config.md` 2.2). 0x11e8 x 8 is 27 x chunks if the FIFO holds only whole
chunks. An odd remainder (if any) stays in the FIFO for the next call. No register write between reads except the
3 latch writes. 0x4008 bit23 is not touched here, the acquisition block set it.

### 2.3 Polling

| result | meaning | next action |
|---|---|---|
| chunks read | | loop again immediately (no sleep); SINGLE in roll also just continues (no auto stop) |
| nothing ready | 0x11e8 = 0 | sleep 9 ms |
| short read | short/failed read | loop again immediately |
| sweep done | scan only | section 5 |

So the read runs at most every ~9 ms when the FIFO is empty. At 100 ms/div (1000 chunks/s) that is about 9 chunks ≈ 2 kB per read.

### 2.4 How points are appended

Per channel, buffer capacity = 1000 x len points (len = points per channel per chunk). New data = cnt x len points,
copied to the **end**. If old + new fits: old data is moved left by the new length, new data goes after it,
range = (cap - total, total). If it does not fit: the oldest new-length points are dropped, everything moves to index 0,
new data at the end, range = (0, cap). Time axis: `xinc = trace dot time * 1000 / len` with trace dot time =
ceil(dot x k / f), `x0 = -xinc * points` (newest point at t = 0, right edge). A read with **> 500 chunks** is not
added to the display. The data is still consumed. After the start and after a channel on/off/position change, discard the next n
reads.

## 3. Acquisition and horizontal values in roll

### 3.1 Acquisition block

Analog only: **identical to normal mode** (same rate 0x10d0/0x10d8/0x10d4, 0x10e4 = memsel, 0x10e0 = segment size,
0x4050/0x4054 = pre/post, 0x404c = 5, 0x1148, 0x1004). The only roll terms are for LA: 0x4008 bit24 = LA && roll/scan,
and the LA doubling of 0x10e4 is skipped in roll. The FPGA records into memory during roll as well (0x1208 counts it, section 4).

### 3.2 Display-chain parameters

- Memory range in roll: first point = 0, points = memsel.
- s = column time / dot (column = time/div / 100). k = smallest divisor of s with s/k <= 1000
  (as in normal mode). **Roll correction** (flag of 1.3 set): if 1000·s/k < 1,000,000, multiply the linear
  fraction f by old rate / new rate (the ratio of the legal-rate remap). If the product is now > 1,000,000,
  scale f down to exactly 1,000,000; if f < 1, f = 1. Result for all 1-2-5 time bases with one channel:
  1000·s·f/k = 1,000,000, i.e. exactly 1000 points per column before the /10, so **one chunk = one column**.
- len = 1000·s·f/k. Trace dot = ceil(dot·k/f).
- Fine time base with round(100·req/roof) not a multiple of 4: f *= x/100 and n as in 2.2. Edge case, not needed for OOS.

### 3.3 Horizontal block, roll branch

Written by the common part, the same registers as in normal mode but with the roll parameters:

| register | value | note |
|---|---|---|
| 0x1100 / 0x1108 | memsel + (0x48 \| ratio<<9) + k / 0 | window |
| 0x1104 / 0x110c | zoom window | |
| 0x1118 | memsel + 40 | sinc input length |
| 0x1130 | 0x00010101 (sinc factor 1) | |
| 0x1154 / 0x1158 / 0x115c | bit0 = 0 (linear on) if f != 1, 0, (2^32-1)·den/num | linear interpolation |
| 0x1154 / 0x1160 / 0x1164 | zoom copy | |
| 0x1154 bits 8/9 | x2 flag = (sinc·num/den > 1) (integer division) | 8/5 → 0, 2/1 → 1 |
| 0x1138, 0x131c, 0x10a0, delays | as in normal mode | |

Roll branch, in this order (replaces the normal compress/TX block):

| # | register | value |
|---|---|---|
| 1 | **0x11fc** | **100 / ratio** (normal mode: 1000; exactly n / ratio) |
| 2 | 0x11f0 | roundup8(len) |
| 3 | **0x1170** | `(s1170 & 0x30000000) \| 0xc0000000 \| 10·ratio·k` (always enabled) |
| 4 | 0x1174 | zoom compression (zk > 1, zoom parameter copy) |
| (5) | 0x12cc, 0x12c0, ... | LA only: `la.md` 4.4 |
| 6 | WPU | display clear |

The **/10** in 0x1170 together with 0x11fc = 100/ratio gives 100 words per chunk. 0x1170 is per channel (normal mode
uses k for any ratio with 0x11f0 = per-channel length). So per channel the decimation is 10·ratio·k on the
interpolated stream. Per-channel point spacing = 10·ratio·k·dot / f. Chunk duration = 1000·k·dot / f. Per the
compression test in `acquisition-start.md`, the points are assumed to be min/max pairs in roll too.

### 3.4 Computed table (model of 1.2/3.2, Auto depth)

| channels, time/div | rate | memsel | s | k | f | 0x1170 | 0x11fc | 0x11f0 | 0x1100 | chunk | chunks/s |
|---|---|---|---|---|---|---|---|---|---|---|---|
| CH1, 50 ms | 156.25 M | 78,125,000 | 78125 | 125 | 8/5 | 0xc00004e2 | 100 | 1,000,000 | 0x4a81a8d | 0.5 ms | 2000 |
| **CH1, 100 ms** | **62.5 M** | **62,500,000** | 62500 | 100 | **8/5** | **0xc00003e8** | **100** | **0xf4240** | **0x3b9af4c** | **1 ms** | **1000** |
| CH1, 200 ms | 62.5 M | 125,000,000 | 125000 | 125 | 1 | 0xc00004e2 | 100 | 1,000,000 | 0x7735c05 | 2 ms | 500 |
| CH1, 500 ms | 12.5 M | 62,500,000 | 62500 | 100 | 8/5 | 0xc00003e8 | 100 | 1,000,000 | 0x3b9af4c | 5 ms | 200 |
| CH1, 1 s | 12.5 M | 125,000,000 | 125000 | 125 | 1 | 0xc00004e2 | 100 | 1,000,000 | 0x7735c05 | 10 ms | 100 |
| CH1+CH2, 100 ms | 62.5 M | 62,500,000 | 62500 | 100 | 1 | 0xc00007d0 | 50 | 625,000 | 0x3b9b14c | 1.6 ms | 625 |
| CH1-4, 100 ms | 31.25 M | 31,250,000 | 31250 | 50 | 1 | 0xc00007d0 | 25 | 625,000 | 0x1dcdeca | 1.6 ms | 625 |
| CH1, 100 ms, depth 1 M | 500 k | 1,000,000 | 500 | 1 | 2 | 0xc000000a | 100 | 1,000,000 | 0xf4489 | 1 ms | 1000 |

0x115c for f = 8/5: 0x9fffffff. For f = 2: 0x7fffffff. Multi-channel at 100 ms/div gives 1.6 columns per chunk (the correction
only runs when the rate was remapped). The display stays right through xinc. All numbers are computed from the formulas;
`roll::params` is unit-tested against this table.

## 4. Stop and back to normal

### 4.1 STOP from roll

```
rd 0x4034, status clear (UI status)
stop sequence               ; poll 0x4034 bit13 + 17 writes (incl. 0x1000 bit4 TX reset = FIFO flush, assumed)
0x10b4 = 1 ; 0x10b4 = 0     ; average clear
rd 0x1208                   ; bit31 = memory full, [29:0] = words recorded
                            ; columns = full ? memsel : (cnt/ratio) rounded down to memsel/1000
```
The STOP view can then play back the memory with these points. The roll bit (0x4008 bit17) is **not** cleared at STOP. A horizontal
block written in STOP takes the normal compress branch (0x11fc = 1000, normal k), so after a STOP the roll chain values may already
be overwritten (assumed; see experiment E2).

### 4.2 Leaving roll

- Time base below the threshold → reconfigure → RUN: horizontal block normal branch (0x1170 = normal k, 0x11fc = 1000, normal linear
  interpolation), SCU block: bit17 = 0, bit22 = 0, without the mode-bit clear of 2.1. Then the normal run sequence (its prologue
  clears 0x4008 bit17/22 again, the record set-up rewrites 0x400c).
- 0x4008 bits 20/21 and 0x400c bits 18/17 are only cleared on entry. Their meaning is unknown, and OOS never sets them.

## 5. Scan mode (differences from roll)

- SCU block: bit17 = 0, bit22 = 1 (0x4008).
- **0x4058** = `pos | en << 31` with pos = (offset − 5·time/div) / dot. en = 0 (and pos 0) if −(screen start / column time) >= 1, i.e.
  when the trigger point is not left of the screen start (assumed meaning: samples from trigger to scan start).
- Read path: the same chunk read, the same chunks, no roll buffer. The picture is drawn left to right with wrap. After every read,
  also when 0x11e8 = 0 or the read failed: rd **0x1204**, bit31 = triggered, [15:0] = position (see 9.1).
  Rd **0x40d0** != 0 → sweep done.
- Sweep done in RUN: stop sequence, average clear, read the recorded range, configuration (horizontal, WPU, WAU, SCU, no acquisition
  block), sleep 500 ms, then RUN (new sweep) and sleep 15 ms. In SINGLE: STOP.

## 6. OOS sequence: roll, CH1, 100 ms/div, Auto depth

Precondition: FPGA stopped (warm hand-over or after own normal acquisitions), ADC in CH1 mode, AFE set, copies in
`scu::Shadows`. Values from 3.4. Registers without an OOS copy (0x1154, 0x1170) are written absolute, with the same assumption as
`scu::passthrough` (upper bits 0). Verified on the unit (section 8).

### A. Configuration (once)

| # | reg | value |
|---|---|---|
| A0 | poll 0x4034 | bit13, max 10 ms |
| A1 | acquisition block | `acq::configure` with `hz::prep(1, 100e12, 0, 0)`: 0x1004 (bit2 = 1: 62.5 M < 156.25 M), 0x10d0 = 0, 0x10d8 = 1, 0x10d4 = 0, 0x10b0 = 1, 0x1148 = 672, 0x404c = 5, 0x10e4 = 0x3b9aca0, 0x4050 = 0x1dcd650, 0x4054 = 0x1dcd650, 0x10e0 = 0x800000 |
| A2 | 0x1100 | 0x3b9af4c (62,500,000 + 0x248 + 100) |
| A3 | 0x1108 | 0 |
| A4 | 0x1118 | 0x3b9acc8 (memsel + 40) |
| A5 | 0x1130 | 0x00010101 |
| A6 | 0x1154 | 0x00000002 (main linear on, zoom off) |
| A7 | 0x1158 | 0 |
| A8 | 0x115c | 0x9fffffff (f = 8/5) |
| A9 | 0x1154 | 0x00000002 (zoom) |
| A10 | 0x1160 | 0 |
| A11 | 0x1164 | 0xffffffff |
| A12 | 0x11fc | 0x64 (100) |
| A13 | 0x11f0 | 0xf4240 (1,000,000) |
| A14 | 0x1170 | 0xc00003e8 |
| A15 | 0x1174 | 0x40000001 |
| A16 | 0x1010 | `s1010 &= ~bit31` (header on; the OOS read-out leaves bit31 = 1) |
| A17 | 0x4008 | `s4008 &= ~bit25` (zoom off) |
| A18 | 0x400c | `s400c &= ~0x42060000` |
| A19 | 0x4008 | `s4008 &= ~0x00720000` |
| A20 | 0x4008 | `s4008 &= ~bit22` |
| A21 | 0x4008 | `s4008 \|= bit17` |
| A22-24 | 0x4020 / 0x4060 / 0x4064 | `scu::auto_trigger(sh, auto, 100e12, holdoff)`: 0x4060 = 0x7735940 (500 ms), 0x4064 = 0x3b9aca0 (250 ms) |

Delays 0x1120-0x112c, 0x1138, 0x131c, 0x10a0 keep their values (same as normal mode; skipping them is assumed to be harmless).

### B. Start

| # | reg | value |
|---|---|---|
| B0 | poll 0x4034 | bit13, max 10 ms |
| B1-17 | | `scu::stop_sequence` (17 writes) |
| B18 | 0x4008 | `s4008 = (s4008 & ~0xf) \| 1` |
| B19 | 0x4004 | `s4004 \|= 1` |

### C. Read loop

```
loop:
    0x1010 = s1010 & ~bit28 ; 0x1010 = s1010 | bit28 ; 0x1010 = s1010 & ~bit28     (s1010 unchanged afterwards)
    n = rd 0x11e8
    if n == 0: sleep 9 ms; continue
    c = (4 * n) / 108                          ; whole chunks
    read(c2h, 216 * c)                         ; one blocking read
    per chunk: skip 16 bytes header, 100 u16 words = 100 points CH1 (ratio 2/4: interleaved)
    append to a ring of 1000 chunks (100,000 points = 1 s = 10 div at 100 ms/div), newest right
    x spacing = 10 us per point (10·ratio·k·dot/f = 10·100·16 ns / 1.6)
```
Expected: about 1000 chunks/s, about 9 chunks (n ≈ 243) per 9 ms poll, 216 kB/s. Convert to volts as in `seq::volts`
(7500 codes/div; the same scaling as the live path is assumed).

### D. Stop, and back to normal acquisition

| # | reg | value |
|---|---|---|
| D0 | poll 0x4034 | bit13, max 10 ms (in roll it will probably time out, assumed) |
| D1-17 | | `scu::stop_sequence` |
| D18-19 | 0x10b4 | 1, then 0 (average clear) |
| D20 | rd 0x1208 | bit31 = full, [29:0] = words recorded (diagnostic) |
| D21-22 | 0x4008 | `s4008 &= ~bit17`; `s4008 &= ~bit22` |
| D23 | 0x11fc | 1000 (normal value; the OOS read-out never writes 0x11fc; assumed to be required) |

Then the normal path: `acq::configure` for the new time base, `scu::arm` (its prologue clears bit17/22 again and the record
set-up rewrites 0x400c), and the read-out with `scu::passthrough` (which resets 0x1170/0x1174/0x1130/0x1134/0x1154..).

### E. Code notes for `hal`

- `scu::Shadows` needs no field for roll. 0x1154/0x1170 are written absolute (as in `passthrough`). `roll::config` = A,
  `roll::start` = B, `roll::fill_pulse` + read 0x11e8 = C, `roll::stop` = D.
- `roll::params` holds the roll part of 3.2 (s, k, f correction).

## 7. Minimal unit experiments (read-only first)

All of them with the probe compensation signal (1 kHz) on CH1. E4 and E6 are done, see sections 8 and 9.

| # | type | what | expected |
|---|---|---|---|
| E1 | read-only, stock system rolling | BAR1 reads (no writes): 0x11e8, 0x1208, 0x4034 a few times, 0x11fc / 0x11f0 / 0x1170 / 0x10e0 / 0x10e4 if they read back | 0x1208 grows (until bit31); 0x11e8 small and changing (last latched value) |
| E2 | read-only, after a STOP in roll | the same registers again | shows whether STOP overwrote 0x1170/0x11fc/0x1154 (4.1) |
| E3 | write, hand-over with the stock system stopped | only B (start) + C (20 polls, log n and every chunk header) + D | n > 0 within ~10 ms; reads in multiples of 216 bytes; headers show a pattern (counter?); data: 1 kHz square wave as a min/max envelope (100 points per 1 ms = 10 ms in 1000 points) |
| E4 | write | full A-D from an OOS normal state (after an own normal acquisition) | same as E3; afterwards a normal acquisition at 200 us/div still works (proves D) |
| E5 | write, variants | (a) 0x1010 bit31 = 1 → chunk without header (200 bytes?) (b) 50 ms/div and 1 s/div: chunks/s = 2000 / 100 (c) CH1+CH2: 0x11fc = 50, 0x1170 = 0xc00007d0, 625 chunks/s (d) poll slower (100 ms) to see what happens when the FIFO fills up (overflow/drop) | confirms chunk = 100 words and the timing per 3.4 |
| E6 | write | scan: A with A20/A21 swapped (bit22 = 1, bit17 = 0) and 0x4058 = 0 at 200 ms/div, NORMAL sweep, then poll 0x1204 / 0x40d0 after each read | 0x40d0 != 0 after one sweep after a trigger |

## Not yet verified on hardware

1. What the latch pulse (0x1010 bit28) does exactly. The 8-byte unit of 0x11e8 is confirmed by section 8.
2. Words 1, 2, 5-7 of the 16-byte chunk header, and whether 0x1010 bit31 = 1 removes it in roll too.
3. FIFO depth and behaviour when the host reads too slowly (drop vs. stall).
4. Whether the stream comes from the memory ring (0x1100/0x1108 = whole memory, 0x1208 counts) or straight from the
   ADC path. It decides whether memsel/0x10e0 matter for roll (assumed: memory ring).
5. Meaning of 0x4008 bits 20/21, 0x400c bits 17/18 and the 0x4058 sign convention.

## 8. Unit result: roll streaming works

`oos-fpgad roll --afe --scale 1 --offset -1.5 --tdiv 0.1 --secs 3 --execute` (OOS after the cold boot, `hal::dho::roll`:
normal configuration, then A, B, C, D of section 6; parameters from `roll::params`):

- **1001 chunks per second** (1 ms per chunk as computed), 2992 chunks in 3.0 s, 399 reads + 300 empty 9-ms polls;
  0x11e8 = 270 -> 10 chunks per read: the 4 n / 108 rule and the 216-byte chunk are right.
- Chunk header (8 words): `fa05 0084 007b <t> <counter> 046b 400b 6e82`; word 4 counts the chunks (1, 2, 3, ...,
  0x01f5 = 501, 0x0baf = 2991), word 3 grows too (time stamp?). The counter allows detecting lost chunks.
- Data: 100 words of CH1 per chunk, 10 us apart; the concatenated stream has exactly **1000.0 Hz** (2992 rising
  edges in 2.99 s), levels 20 812..44 252 like the normal read-out with OOS' AFE set-up.
- After `roll::stop` 0x1208 = 0x832ebd54 (bit 31 = memory full, as expected after > 1 s at 62.5 MSa/s).
- **Stream rate limit:** the stream carries at most one sample per 156.25 MHz clock. At 1.25 GSa/s the point spacing came out 8x, at
  625 MSa/s 4x the computed value. `roll::MAX_RATE` = 156.25 MSa/s. Auto depth reaches exactly this rate at 50 ms/div.
- **oos-ui:** the hardware thread streams in roll mode from 200 ms/div (`hw::ROLL_TDIV`). Between 5 and 100 ms/div the stream ran too
  fast for its time axis with two or four channels, with one channel at 50 ms/div too, LA on or off (a 100 Hz sine read 170 to 280 Hz);
  the cause is not known, a normal acquisition is right on all of them. Roll uses Auto depth (no live-depth cap, the memory is not read): 100 ms/div = 10 us per point / 1 ms chunks, 500 ms/div = 50 us / 5 ms, 1 s/div = 100 us /
  10 ms. Ring of 10 divisions, ~30 pictures per second, newest point at the right edge, no software trigger (status AUTO). Roll is
  stopped (`roll::stop`) before any re-configuration and when the time base goes below the threshold; normal triggered acquisitions
  resume (checked at 200 us/div). Measurements on the min/max stream are only meaningful when the signal is resolved (1 kHz at
  100 ms/div: 999.9 Hz; at 0.5 / 1 s/div the band is drawn correctly but Freq is nonsense, as on any scope).

## 9. Scan mode in OOS (verified on the unit)

- `hal::dho::roll::config_scan(sh, rp, memsel, pos)` = A of section 6 with the last write (roll bit) replaced by bit22,
  then **0x4058** = `scan_trig_pos(delay, time/div, dot)`: 0 unless the screen start is at least one column (time/div / 10 in the
  code) right of the trigger point, else `(delay - 5 time/div) / dot | 1 << 31` (dot = sample period `hz::Params::dot_fs`, assumed). A
  non-zero 0x4058 is not yet verified on hardware.
- After every read (also an empty poll): `SCAN_INFO_REGS` = 0x1204, 0x40d0 -> `scan_info` (triggered, position, done).
- Done in RUN: `scan_done` (stop sequence, average clear), 500 ms, `config_scan` + `start` again. Done in SINGLE: stop until the next
  SINGLE press. Leaving scan: `stop` (clears bit17 and bit22, 0x11fc = 1000).
- oos-ui (`hw::Slow`, `hw::slow_mode`): Horizontal > Auto Roll on -> roll; off -> scan from 200 ms/div
  unless averaging (table 1.2). The sweep picture is one screen per channel, written from the left with
  a wrap at the right edge and a 2 % gap ahead of the write position; the picture is "partial" until done (SINGLE
  stops only on a complete sweep).
- Device test (E6): `oos-fpgad roll --scan --afe --scale 1 --offset -1.5 --tdiv 0.2 --secs 10 --execute` logs every
  change of 0x1204 / 0x40d0 with the chunk count of the sweep; expected: one sweep = 10 div = 1000 chunks at
  200 ms/div (2 ms per chunk), then done.

### 9.1 Unit result (OOS after the cold boot, 1 kHz probe comp on CH1)

`oos-fpgad roll --scan --afe --scale 1 --offset -1.5 --tdiv 0.2 --secs 10 --execute` (0x4058 = 0):

- 500 chunks/s (2 ms per chunk, 20 us per point) as computed; **one sweep = exactly 1000 chunks = 10 div**.
- **0x1204** = 0x800001f4 after 500 chunks: bit31 set at the screen centre (the trigger point with delay 0), [15:0] =
  0x1f4 = **500 = trigger position in chunks (columns)**. Bit31 clears again with done.
- **0x40d0** = 1 after 1000 chunks (done); 0 again after `scan_done` + 500 ms + `config_scan` + `start`. 4 sweeps in
  10 s (2 s sweep + 0.5 s pause), 4000 chunks, no loss.
- **The chunk counter (header word 4) restarts at 1 with every sweep**: `counter - 1` is the column. oos-ui uses it as
  the write position (`roll::counter`), so lost chunks cannot shift the picture.
- Data: 1000.0 Hz in every sweep. The edges are not phase-aligned between sweeps (phase within a 2-ms chunk varies by
  up to 20 points): the scan trigger aligns at most to a chunk, not to a sample. Invisible at >= 200 ms/div.
- oos-ui (Auto Roll off, 200 ms/div): sweep with the gap at the write position, new sweep after done; SINGLE stops on
  the complete sweep (STOP), the next SINGLE starts one more. 0x40d0 reports done ~3-4 chunks before the last column
  arrives: the thread reads on until the column counter wraps (0 ms extra on the unit), else the right edge stays empty.
- Auto Roll off at 100 ms/div: normal triggered 1-s records (1 wfm/s); WAIT only after 1.5 record lengths.

## Scan with the LA on

LA and analog chunks alternate and share the chunk counter, so analog chunk n carries the counter 2n (`roll::scan_column`).
Counting them as analog columns writes the sweep with gaps. The LA words go to the place of the analog point beside them.
Verified on the unit: 100,000 LA words per sweep at 200 and 500 ms/div, 50 Hz on D0 reads 50.0 Hz.
