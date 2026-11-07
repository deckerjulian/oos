# Horizontal / acquisition configuration (time base, memory depth, channel set, acquire mode)

Status: OOS runs normal-mode acquisitions with this model on the unit (1 to 4 channels, depth up to 125 M). Statements
marked "verified on the unit" were seen there. Everything else in this file has not been verified on hardware.

Conventions: times in femtoseconds (fs), as in `acquisition-start.md`. `tb` = time base in fs/div, `memsel` = points per
channel, `ratio` = interleave factor, `dot` = sample period in fs, `mask` = channel mask (CH1 bit 0 .. CH4 bit 3).
`sN` = software copy of write-only register N. `hal::hz` implements sections 1.2 to 1.6.

## 0. Summary

- A change of time base, depth or acquire mode needs no immediate FPGA write. All registers of this file go out as one
  block before the next RUN/SINGLE. The sample rate is never set directly: it is always derived.
- One computation yields rate, memsel, segment size, frame count, pre/post trigger points, screen/memory window and
  display compression. The acquisition block then writes the acquisition registers, the horizontal block the
  display-trace (read-out) registers.
- Horizontal acquisition registers: **0x10d0/0x10d8/0x10d4** sample-rate divider, **0x10e4** record depth (points per
  channel), **0x10e0** segment size (memory bursts per frame), **0x4050/0x4054** pre/post-trigger points,
  **0x4008[3:0]** channel mask (FSM nibble), **0x1004** lane config (ADC order, invert, BW filter, acquire mode),
  **0x10b0/0x1148** average parameters, **0x404c** pre-SA compensation, **0x1080/0x1084** per-channel sample delay and,
  when the channel set changes, the ADC input-mode SPI words (ADC reg 0x54/0x55 via 0x1034) plus the per-mode ADC
  calibration.
- `ratio` is **not** the number of enabled channels. It is the interleave factor 1/2/4 from a table indexed by the
  channel mask (3 channels -> 4). The stream has `ratio` words per sample time, channel order ascending. The FSM nibble
  is the plain channel mask (verified on the unit).

## 1. Time base + memory depth -> sample rate, memsel, segment size, frame count

### 1.1 Inputs and tables

| item | value |
|---|---|
| time base | fs/div, rounded up to the next 1-2-5 value |
| horizontal position | fs, trigger delay (`offset` below) |
| depth | 0 = Auto, else one entry of the depth list below |
| channel mask | CH1 bit 0 .. CH4 bit 3 |
| ratio | table below |
| max rate | table below |

Depth list (points per channel): 100, 1k, 10k, 100k, 500k, 1M, 1.25M, 2.5M, 5M, 6.25M, 10M, 12.5M, 25M, 31.25M, 50M,
62.5M, 100M, 125M.

Interleave and maximum rate for DHO800/900, by channel mask:

| mask | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | A | B | C | D | E | F |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ratio | 1 | 1 | 2 | 1 | 2 | 2 | 4 | 1 | 2 | 2 | 4 | 2 | 4 | 4 | 4 |
| max rate (MSa/s per channel) | 1250 | 1250 | 625 | 1250 | 625 | 625 | 312.5 | 1250 | 625 | 625 | 312.5 | 625 | 312.5 | 312.5 | 312.5 |

Maximum depth, for Auto and for a fixed depth, by number of enabled channels and LA mode:

| enabled channels | LA off | LA on |
|---|---|---|
| 1 | 125,000,000 | 31,250,000 |
| 2 | 62,500,000 | 12,500,000 |
| 3, 4 | 31,250,000 | 10,000,000 |

### 1.2 Algorithm

```
tb   = time base                    ; fs/div, 1-2-5
mr   = maxrate(mask)                ; per channel
if depth != 0:                      ; fixed depth
    memsel = depth
    rate   = min(depth * 1e15 / tb / 10, mr)          ; exact product over a reduced fraction, then /10
else:                               ; Auto
    pts = ceil_even(10 * tb * mr / 1e15)              ; exact if integer, else rounded up to the next even number
    if pts > max_auto_depth: memsel = max_auto_depth; rate = max_auto_depth * 1e15 / tb / 10
    else:                  memsel = pts;          rate = mr
if rate is one of the "from" values below:
    rate = its "to" value
    if Auto: memsel = 10 * tb * rate / 1e15
if LA on: rate = rate/2 (156.25 M -> 62.5 M special case)    ; analog rate halves with LA
dot  = 1e15 / rate                  ; fs per sample
```

Decimal rates that are not reachable from the 1.25 GHz clock are replaced (only on an exact match). The ratio old/new
is kept as a fraction.

| from | 2.5 k | 200 k | 1 M | 2 M | 5 M | 10 M | 20 M | 25 M | 50 M | 100 M | 125 M | 200 M | 250 M | 400 M | 500 M | 1 G |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| to | 2 k | 125 k | 500 k | 1.25 M | 2.5 M | 6.25 M | 12.5 M | 12.5 M | 31.25 M | 62.5 M | 62.5 M | 156.25 M | 156.25 M | 312.5 M | 312.5 M | 625 M |

An exact match needs an exact time base: a rate of 50,000,001 Sa/s from a rounded time/div misses the table and the FPGA
runs at the wrong rate (verified on the unit). `seq::tdiv_fs` therefore rounds the time base to 4 digits.

With a fixed depth, memsel is not recomputed after the rate correction, so the memory covers `depth / rate` >= 10
divisions. Example: 1 M points at 200 us/div -> 1e6/2e-3 = 500 MSa/s -> 312.5 MSa/s, 3.2 ms of memory for a 2 ms screen.

### 1.3 Auto depth, results (`hal::hz`, LA off)

| time/div | 1 ch: rate / memsel / 0x10e0 | 2 ch: rate / memsel | 4 ch: rate / memsel |
|---|---|---|---|
| 1 ns | 1250 M / 14 / 0x40 | 625 M / 8 | 312.5 M / 4 |
| 10 ns | 1250 M / 125 / 0x40 | 625 M / 64 | 312.5 M / 32 |
| 100 ns | 1250 M / 1250 / 0x100 | 625 M / 625 | 312.5 M / 314 |
| 1 us | 1250 M / 12500 / 0x800 | 625 M / 6250 | 312.5 M / 3125 |
| 2 us | 1250 M / 25000 / 0x1000 | 625 M / 12500 | 312.5 M / 6250 |
| 10 us | 1250 M / 125000 / 0x4000 | 625 M / 62500 | 312.5 M / 31250 |
| 100 us | 1250 M / 1250000 / 0x20000 | 625 M / 625000 | 312.5 M / 312500 |
| 200 us | 1250 M / 2500000 / 0x40000 | 625 M / 1250000 | 312.5 M / 625000 |
| 500 us | 1250 M / 6250000 / 0x100000 | 625 M / 3125000 | 312.5 M / 1562500 |
| 1 ms | 1250 M / 12500000 / 0x200000 | 625 M / 6250000 | 312.5 M / 3125000 |
| 5 ms | 1250 M / 62500000 / 0x800000 | 625 M / 31250000 | 312.5 M / 15625000 |
| 10 ms | 1250 M / 125000000 / 0x1000000 | 625 M / 62500000 | 312.5 M / 31250000 |
| 20 ms | 625 M / 125000000 / 0x1000000 | 312.5 M / 62500000 | 156.25 M / 31250000 |

Verified on the unit: 200 us/div 1 ch -> memsel 0x2625a0 (2.5 M) at 1.25 GSa/s; 500 us/div -> 6.25 M; 2 us/div 1 ch ->
25 k; 2 us/div 4 ch -> memsel 0x186a (6250) per channel at 312.5 MSa/s.
Roll mode starts at 50 ms/div and uses a different path (`roll-mode.md`), not covered here.

### 1.4 Segment size -> 0x10e0

Input `pts = memsel * ratio` (total words) and the acquire mode. A DDR burst is 10 points, an average segment burst 32.
The memory size for the segment cap is 0x5000000 bursts.

```
n = (mode == 2 /*average*/) ? ((pts + 0x17f) / 320 + 2) * 64 : (pts + 0x180) / 10   ; bursts, +384 points guard
n = max(n, 32)
n = next power of two >= n
n = min(n, (0x5000000 - 0x100000) / 2) = 41,418,752
LA on: n *= 2
segsize = n  -> register 0x10e0
```

Example 2.5 M points: (2,500,000 + 384)/10 = 250,038 -> 0x40000.

The memory size matters for deep records. With 16,777,216 bursts the cap is 7,864,320 bursts = 78,643,200 points. A deeper
record then wraps inside its segment and its first `depth - 78,643,200` points are overwritten by later samples. With
0x5000000 bursts, 100 M and 125 M get 0x10e0 = 0x1000000 (2^24 bursts) and show no wrap (both verified on the unit).

### 1.5 Frames per batch and ring size

```
max_frames = (16,777,216/2 - 0x100000) / segsize = 7,340,032 / segsize      ; start of ring slot 1
n0 = 4e13 / (max(1, sinc) * X + memsel*dot*ratio),  X = tb*4e7*ratio/dot  ; sinc = last sinc factor
n  = min(n0, max_frames, (sweep == SINGLE) ? (ratio == 1) : max_batch/ratio)   ; max_batch default 2000
pts = 10*tb*rate/1e15; if pts*n*ratio*30 > 60,000,000: n = 60e6/pts/ratio/30
if product series == 900 or 800: n /= 2
if zoom on: n /= 2
n = max(n, 1)                                                           ; acquire mode 7 (ultra): see section 3
```

The frame ring keeps the memory size of 16,777,216 bursts, unlike the segment cap of 1.4.

This is only throughput tuning (how many frames one arm records, see `acquisition-start.md`). It does not change what a
frame looks like. Examples: 200 us/div 1 ch -> n = 1 (verified on the unit); 1 us/div 2 ch -> 160 (80 with the series
halving). A batch of 160 frames at 2 us/div was seen on the unit. That fits LA on (rate 625 M) without the series halving
(assumed). OOS sets `frames` itself and caps it by `max_frames`.

### 1.6 Pre/post trigger points -> 0x4050/0x4054

```
mem_start = offset - dot*memsel/2           ; fs relative to the trigger (offset = horizontal position)
mem_end   = mem_start + dot*memsel
post = ceil((mem_end + extra) / dot)        ; -> 0x4054
pre  = max(0, memsel - post)               ; -> 0x4050
(invalid, nothing changes, if mem_start < -memsel*dot, i.e. the whole memory ends before the trigger)
```

`extra` is 0, except for the latency compensation of `trigger.md` section 4 (190/638/296/296 ns for 1/2/3/4 channels).

Centered trigger (offset 0): pre = post = memsel/2. Positive offset (window moved later) -> fewer pre points.
Verified on the unit: a window of 4000 points around memory index 1,250,000 holds the trigger for 2.5 M points.

### 1.7 Sample-rate divider registers

| condition | 0x10d0 | 0x10d8 | 0x10d4 |
|---|---|---|---|
| rate > 62,500,000 | `0x8000 \| (maxrate(ratio) / rate) & 0xff` | 1 | 0 |
| rate <= 62,500,000 | 0 | `62,500,000 / rate` | 0 |

maxrate(ratio) = 1250 M / 625 M / 312.5 M for ratio 1 / 2 / 4.
So 0x10d0 bit15 selects "decimate the full-rate stream by N" (N = 1, 2, 4, 8, 10, 20 ...). Below 62.5 MSa/s the divider
counts in a 62.5 MHz domain.
Examples: 1 ch 1.25 G -> 0x8001; 2 ch 625 M -> 0x8001; 1 ch 312.5 M -> 0x8004; 1 ch 12.5 M -> 0x10d0 = 0, 0x10d8 = 5.

## 2. Channel set -> mask, interleave, ADC mode, stream layout

### 2.1 Mask, ratio, lanes

- mask (bit n = CH n+1 on) goes to 0x4008[3:0] with every arm and play.
- ratio = 1, 2 or 4 (table in 1.1): 1 channel -> all 8 ADC cores on that channel (1.25 GSa/s); 2 channels -> 625 MSa/s
  each; 3 or 4 channels -> 4-way mode, 312.5 MSa/s each. 3 channels behave like 4.
- The SPU has 4 lanes. The per-lane bit fields in 0x1004 (invert [19:16], BW filter [23:20]) and the sample delays use
  this assignment: ratio 1: the channel's bit in all 4 lanes; ratio 2: lanes 0,1 = lower channel, lanes 2,3 = higher
  channel (pairs 12, 13, 14, 23, 24, 34 for masks 3, 5, 9, 6, A, C); ratio 4: lane n = CH n+1.

### 2.2 Multi-channel frame layout

| ratio | stream (16-bit words) |
|---|---|
| 1 | `s[i]` = the one channel |
| 2 | `s[2i]` = lower-numbered channel, `s[2i+1]` = higher |
| 4 | `s[4i+k]` = CH(k+1), k = 0..3 |

A live frame is `header (8 words) + ratio*len words`. With 0x1010 bit31 = 1 (read-out) there is no header.
The ratio 4 layout is verified word-exact on the unit. For 3 channels the 4th slot is CH4 (off) (assumed, from the lane
mapping).

### 2.3 ADC input mode, only when the mask changes

Compare the mask and the input impedance mask (assumed meaning) with the last values. On a change: set the ADC input
mode, then write the per-mode ADC calibration: ADC reference, per-core gain/offset and SPU fine gain 0x1058 from
`cal_adc.hex` (`adc-mode.md`). No DSP filter is written on a channel-set change: 0x1098/0x109c stay 0 (verified on the unit).

ADC input mode = two ADC SPI writes through 0x1034, 100 us apart (each preceded by a 100 us sleep):
`0x1034 = 0x01540000 | A` (ADC reg 0x54), then `0x1034 = 0x01550000 | B` (ADC reg 0x55):

| mask | ADC mode | channels | A (reg 0x54) | B (reg 0x55) |
|---|---|---|---|---|
| 1 | 0 | CH1 | 0x9460 | 0x0004 |
| 2 | 1 | CH2 | 0x1803 | 0x0005 |
| 4 | 2 | CH3 | 0x3149 | 0x0000 |
| 8 | 3 | CH4 | 0x00c5 | 0x0006 |
| 3 | 4 | CH1+CH2 | 0x1860 | 0x0005 |
| 5 | 5 | CH1+CH3 | 0x3060 | 0x0000 |
| 9 | 6 | CH1+CH4 | 0x0060 | 0x0006 |
| 6 | 7 | CH2+CH3 | 0x3003 | 0x0000 |
| A | 8 | CH2+CH4 | 0x0003 | 0x0006 |
| C | 9 | CH3+CH4 | 0x0149 | 0x0006 |
| 7, B, D, E, F | 10 | all four | 0x0000 | 0x0000 |

The ADC init of `adc-spu-init.md` leaves the ADC in whatever mode its register list sets (mode 0 after a cold boot).
After a warm hand-over it is in the mode of the last channel set of the stock OS.

## 3. Acquire modes, interpolation: FPGA vs software

Acquire mode numbers: 0 normal, 1 peak detect, 2 average, 3 high resolution (assumed), 7 ultra acquisition.

| item | registers | notes |
|---|---|---|
| normal (0) | 0x1004 bits 0,1,3,12 cleared | |
| peak (1) | 0x1004 bit1 = 1 | use normal (0) when rate == max rate: there is no decimation to detect over. Verified on the unit 2026-11-07: a 2.4 us pulse at 50 kSa/s shows in full, in normal mode not at all |
| average (2) | 0x1004 bit3 = 1; 0x10b0 = count-1 (1 for count 1 or 2); 0x1148 = 640/ratio + 32; 0x404c = 10 instead of 5; segment size uses the average layout (1.4); 0x1110/0x1114 = average offset; 0x10b4 = 1, then 0 (average clear) on STOP/SINGLE | averaging runs in the FPGA memory (assumed, from the register set) |
| high res (3) | no SPU mode bit | OOS does it in software: each screen point is the mean of the record's samples behind it, so it gains only where the record has more samples than the screen has points |
| ultra (7) | own frame count, 524,288,000 / (memsel*ratio) | not analysed, not used by OOS |
| average/sinc select | 0x1004 bit2 = (rate < maxrate/8) | set in all modes |
| anti-aliasing | 0x10dc = 1 only when anti-aliasing is off | OOS does not write 0x10dc |
| interpolation (sinc) | 0x1130 (main) / 0x1134 (zoom) = en<<31 \| f<<16 \| f<<8 \| f with f = sinc factor, en = f > 1; coefficient table via 0x1140/0x1144 when enabled; 0x1118/0x111c = memory points + 40; 0x1138 = en<<31 \| 0xc8c8c8 | FPGA, display trace only |
| interpolation (linear) | 0x1154 bit0 (main) / bit1 (zoom) = NOT enabled; 0x1158/0x1160 = 0; 0x115c/0x1164 = (2^32-1)*den/num or 0xffffffff; 0x1154 bits 8/9 = x2 interpolation | FPGA, display trace only |

The choice between sinc and linear interpolation follows from the samples per screen column alone (3.1).

**Memory content is raw samples in every mode except average/peak**: interpolation, compression and the screen window
only affect the display trace (0x1100.., 0x1130.., 0x1170, 0x11f0), not what is stored. The OOS read-out window
(0x1100/0x1108/0x11f0, `acquisition-start.md` / `adc-spu-init.md`) reads raw memory (verified on the unit).

### 3.1 Display trace (what the live path transfers)

Screen: 1000 columns, column time = tb/100, screen start = offset - 5*tb. From this follow the memory points on screen
and the first memory point on screen.

For 1 <= samples per column:
- samples per column s = column time / dot (as a reduced fraction);
- if s is an integer >= 1001: compression k = smallest divisor of s with s/k <= 1000; if s is an integer <= 1000: k = 1;
- if s is not an integer: resample to the next integer number of points per column (fraction kept for the linear
  interpolator), k = 1;
- len = (points on screen) / k, i.e. 1000 columns x points per column.

Registers: 0x1100 = points on screen + (0x48 | ratio<<9) + k, 0x1108 = first memory point on screen,
0x1170 = 0x40000000 | (k > 1)<<31 | k (bits 28-29 kept), 0x11f0 = roundup8(len), 0x11fc = 1000.
Examples: 200 us/div 1 ch: s = 2500, k = 4, len = 625,000 (0x98968); 2 us/div 1 ch: s = 25, k = 1, len = 25,000 (both
verified on the unit).

## 4. Ordered register writes for two examples

Both: RUN from STOP, warm hand-over not assumed, LA off, no deskew, no invert, BW limit off, acquire mode normal,
average count 2 (default), horizontal position 0, edge trigger.

Block order: compute the parameters, then ADC block, acquisition block, horizontal block, WPU block, WAU block, SCU block,
then the arm (`acquisition-start.md`). WPU/WAU = display engines, SCU = auto trigger, roll/scan and zoom bits
(`acquisition-start.md` section 3). The TPU (trigger) registers are written separately (`trigger.md`).

### 4.1 Example 1: CH1 only, 200 us/div, Auto -> 2.5 Mpts at 1.25 GSa/s

Derived: mask 1, ratio 1, rate 1.25e9, dot 800,000 fs, memsel 2,500,000 (0x2625a0), segsize 0x40000, max_frames 28,
frames/batch 1, pre = post = 1,250,000 (0x1312d0), screen points 2,500,000, k = 4, len 625,000 (0x98968), sinc factor 1,
no linear interpolation. Ratio, FSM nibble, memsel, len and frames/batch are verified on the unit.

| # | step | register | value |
|---|---|---|---|
| | **ADC block** (only if the mask changed since the last RUN) | | |
| 1 | input mode 0 | 0x1034 | 0x01549460 (ADC reg 0x54) |
| 2 | | 0x1034 | 0x01550004 (ADC reg 0x55) |
| 3 | ADC calibration of mode 0 | 0x1034 / 0x1058 ... | from `cal_adc.hex`, `adc-mode.md` |
| 4 | DSP filter | 0x1098, 0x10a0 | not written (2.3) |
| | **Acquisition block** | | |
| 5 | ADC order | 0x1004 | s \|= bit26 |
| 6 | LA off | 0x4008 | s &= ~bit26 |
| 7 | analog stream on | 0x4008 | s \|= bit23 |
| 8 | LA stream off | 0x4008 | s &= ~bit24 |
| 9 | average/sinc select | 0x1004 | s &= ~bit2 (1.25 G is not < 1.25 G/8) |
| 10 | sample delay | 0x1080 | (s & 0xffff) \| (480 + d1/600000) << 16 = 0x01e0xxxx for d1 = 0 |
| 11 | | 0x1084 | shadow unchanged (written again) |
| 12 | LA delay | 0x128c | `la.md` |
| 13-14 | pre-processing reset | 0x1000 | s \| 0x20, then s & ~0x20 |
| 15 | invert | 0x1004 | [19:16] = 0 (CH1 invert replicated x4) |
| 16 | BW filter | 0x1004 | [23:20] = 0 (CH1 BW limit replicated x4) |
| 17 | sample rate | 0x10d0 | 0x8001 |
| 18 | | 0x10d8 | 1 |
| 19 | | 0x10d4 | 0 |
| 20 | acquire mode normal | 0x1004 | s &= ~0x100b -> 0x04000000 with the bits above (verified on the unit) |
| 21 | average parameters | 0x10b0 | 1 |
| 22 | | 0x1148 | 672 (0x2a0) |
| 23 | pre-SA compensation | 0x404c | 5 |
| (24) | anti-aliasing off only | 0x10dc | 1 (OOS: not written) |
| 25 | record depth | 0x10e4 | 0x2625a0 |
| 26 | pre points | 0x4050 | 0x1312d0 |
| 27 | post points | 0x4054 | 0x1312d0 |
| 28 | segment size | 0x10e0 | 0x40000 |
| 29 | DVM | DVM registers | not written by OOS |
| | **Horizontal block** (normal time mode) | | |
| 30 | main window | 0x1100 | 2,500,000 + 0x248 + 4 = 0x2627ec |
| 31 | | 0x1108 | 0 (first memory point on screen) |
| 32-33 | zoom window | 0x1104, 0x110c | zoom off: values of the zoom parameter copy |
| 34 | zoom sinc input length | 0x111c | zoom points + 40 |
| 35 | zoom sinc | 0x1134 | fz<<16 \| fz<<8 \| fz (disabled) |
| 36 | main sinc input length | 0x1118 | 2,500,040 |
| 37 | main sinc | 0x1130 | 0x00010101 |
| 38-40 | main linear interpolation | 0x1154, 0x1158, 0x115c | bit0 = 1 (off), 0, 0xffffffff |
| 41-43 | zoom linear interpolation | 0x1154, 0x1160, 0x1164 | same for the zoom copy |
| 44 | x2 interpolation off | 0x1154 | bits 8, 9 = 0 |
| 45 | sinc select 2 | 0x1138 | 0x00c8c8c8 |
| 46 | wave-early word (4, 4) | 0x131c | (s & 0xf000) \| 0x2580044 (LA on: 0x2580004, verified on the unit) |
| 47 | filter control (meaning unknown) | 0x10a0 | 16 |
| 48+ | delays, main, then zoom if zoom is on | 0x1120/0x1124, 0x1128/0x112c, 0x1290 | display-trace channel alignment (3*dot/2 + fine deskew), values not known |
| | main compression (k = 4) | 0x1170 | 0xc0000004 (+ kept bits 28-29) |
| | zoom compression | 0x1174 | |
| | TX item | 0x11fc | 1000 |
| | main TX length | 0x11f0 | 0x98968 |
| | zoom TX length | 0x11f4 | |
| | **SCU block** | 0x4008 bits 17/22/25, 0x4020, 0x4060 = 0x2625a0, 0x4064 = 0x17d7840 | see acquisition-start.md |
| | **Arm** | arm sequence, 0x4008[3:0] = 1 | see acquisition-start.md |

### 4.2 Example 2: CH1 + CH2, 1 us/div, Auto

Derived (computed, not seen on the unit): mask 3, ratio 2, rate 625 MSa/s per channel, dot 1,600,000 fs, memsel 6250
(0x186a) per channel (12,500 words), segsize 0x800, max_frames 3584, frames/batch 160 (80 if series 800/900),
pre = post = 3125 (0xc35); screen: 6.25 samples per column -> resampled to 7 points per column, k = 1, len = 7000.

Differences from example 1 (same order):

| # | register | value |
|---|---|---|
| 1-2 | 0x1034 | 0x01541860, then 0x01550005 (ADC mode 4, only if the mask changed) |
| 3 | ADC calibration | mode 4 |
| 10 | 0x1080 | (240 + d1/1.2e6) << 16 \| (240 + d2/1.2e6) = 0x00f000f0 for zero delays |
| 11 | 0x1084 | shadow unchanged |
| 15, 16 | 0x1004 | invert/filter [19:16]/[23:20] = CH1, CH1, CH2, CH2 bits |
| 17-19 | 0x10d0 / 0x10d8 / 0x10d4 | 0x8001 / 1 / 0 (625 M = max rate for ratio 2) |
| 22 | 0x1148 | 352 (0x160) |
| 25 | 0x10e4 | 0x186a |
| 26-27 | 0x4050 / 0x4054 | 0xc35 / 0xc35 |
| 28 | 0x10e0 | 0x800 |
| 30 | 0x1100 | 6250 + (0x48 \| 2<<9 = 0x448) + 1 = 7347 (0x1cb3) |
| 31 | 0x1108 | 0 |
| 38-40 | 0x1154 bit0 = 0 (linear on), 0x115c = (2^32-1)*den/num of the resampling fraction | fraction assumed 25/28 |
| | 0x1170 | 0x40000001 (k = 1, compression off) |
| | 0x11f0 | 7000 (0x1b58), assumed (len from 3.1) |
| | SCU 0x4060 / 0x4064 | 1e13/4e6 = 2,500,000 / 1e14/4e6 = 25,000,000 (20 x 1 us clamps to 10 ms, 10 x 1 us to 100 ms) |
| | arm: 0x4008[3:0] | 3 |

Read-out of a 2-channel frame (OOS window method): 0x11f0 = number of sample groups, the read is `2 * 2 * groups` bytes,
words alternate CH1, CH2; 0x1108 = start point per channel. The window registers are verified on the unit with ratio 4.

## 5. Proposed OOS implementation

Precondition: FPGA, clocks, ADC and lane calibration from the cold boot (`cold-boot.md`) or a warm hand-over. OOS keeps
shadows of 0x1000, 0x1004, 0x1080, 0x1084, 0x4008, 0x1154, 0x1170 (start value of 0x1004 after a warm hand-over: 0x04000000).

1. Compute (pure software, sections 1.2-1.6, `hal::hz`): ratio/max rate from the mask table; rate and memsel from the time
   base (Auto with the maximum depth of 1.1, or a fixed depth from the depth list), the rate table of 1.2, segsize,
   pre/post. Use 1000 columns, tb/100 per column. Frame count: 1 is the simplest choice; batches are in
   `acquisition-start.md`.
2. Channel set changed? -> ADC input mode (2.3, two SPI words with 100 us gaps) and the per-mode ADC calibration. Without
   the calibration the interleaved cores of a new mode are not matched (assumed).
3. Write the acquisition block of 4.1 (writes 5-28, without 12, 24, 29) with the computed values (`hal::acq`).
4. The OOS read-out path does not need the display-trace block of the horizontal block: OOS writes its own
   0x1100/0x1108/0x11f0 window before every read. For a live display via the FPGA trace path, write 30-31, 37-40, 44,
   compression 0x1170 and 0x11f0/0x11fc as in 4.1, sinc coefficients only if the sinc factor > 1 (`cold-boot.md` 3.8).
5. SCU auto trigger 0x4060/0x4064 from the time base (`acquisition-start.md` section 3).
6. Arm with chmask = mask in 0x4008[3:0] (acquisition-start.md table A); read `2 * ratio * N` bytes for N points per
   channel and de-interleave as in 2.2.
7. Read-out window for a frame: start = pre - points_before_trigger_wanted (memory index of the trigger = pre).

Assumed, not verified on hardware: acquire mode 3 = high res and its handling, the 3-channel 4th slot, the linear
resampling fraction and len of example 2.

## 6. Field reference

| parameter | meaning | register |
|---|---|---|
| max rate | maximum rate for the mask | |
| rate | sample rate per channel | 0x10d0 / 0x10d8 |
| dot | sample period (fs per sample) | |
| tb | time base (1-2-5) | |
| offset | horizontal offset | |
| memsel | points per channel | 0x10e4 |
| pre / post | pre / post trigger points | 0x4050 / 0x4054 |
| frames | frames per batch | 0x4018 |
| segsize | segment size | 0x10e0 |
| max_frames | ring slot 1 start | |
| ratio | interleave 1/2/4 | |
| mask | channel mask | 0x4008[3:0] |
| mem_start / mem_end | memory start / end time (fs, trigger = 0) | |
| first point | first memory point on screen | 0x1108 |
| screen points | memory points on screen | 0x1100 |
| len | display trace length | 0x11f0 |
| sinc | sinc factor | 0x1130 |
| num / den | linear interpolation fraction | 0x115c |
| k | compression factor | 0x1170 |
