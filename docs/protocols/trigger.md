# Trigger unit (TPU, BAR1 0x2000 block)

Status: unless a statement says otherwise, the sequences in this file are not yet verified on hardware. "Assumed" marks an
interpretation of a field or a behaviour. The values called "hand-over state" are the register contents left by the stock
system in its default configuration.

Conventions as in `acquisition-start.md`: time values are in **femtoseconds**, `sN` is the software copy of register N.
Voltages (scale, offset, level) are **int64 nanovolts at the BNC**, probe ratio not applied (see `afe-settings.md`). The TPU
configuration registers are **write-only** (0x2034 reads 0). Registers that change bit by bit need a software copy
(`tpu::Shadows` in `hal/src/dho/tpu.rs`), and the read-modify-write happens on that copy. Registers with no copy are always
written whole.

The notation `source(...)`, `pattern(...)`, `trigger(...)`, `mode(...)` and so on is defined in 10.1. `tools/tpumodel.py`
prints the ordered writes for every type below. `hal/src/dho/tpu.rs` is the implementation.

## 1. TPU register map

| offset | bits | meaning |
|---|---|---|
| 0x2004 / 0x2008 | [15:0] upper threshold code, [31:16] lower threshold code | CH1 comparator A / B (level A / level B) |
| 0x200c / 0x2010 | same | CH2 comparator A / B |
| 0x20b8 / 0x20bc | same | CH3 comparator A / B |
| 0x20c0 / 0x20c4 | same | CH4 comparator A / B |
| 0x2014 | 2 bits per pattern slot n = 0..15 at [2n+1:2n] (slots 0..3 = CH1..CH4, 4..15 = D0..D11, see 10.8) | event A pattern value (0 = X, 2 = H, 3 = L, see 5.2) |
| 0x2018 | [9:0] 2 bits per slot 16..20 (D12..D15, EXT), [11:10] logic op | event A pattern of the remaining slots + logic function (0 AND, 1 OR, 2 NOR, see 5.2) |
| 0x201c | [5:0] src a, [7:6] edge a, [13:8] src b, [15:14] edge b | event A comparator selection |
| 0x201c | [18:16] p0, [19] p1, [22:20] p2, [23] p3 | event A "pattern" (event kind/polarity, see 2.2) |
| 0x201c | [26:24] when | event A time condition (0 >, 1 <, 2 inside, 3 outside, 4 timeout, 5 none) |
| 0x2020 | [31:0] | event A t1 (lower limit) bits 31:0, unit 1.6 ns (t / 1.6e6) |
| 0x2024 | [31:0] | event A t2 (upper limit) bits 31:0, unit 1.6 ns |
| 0x2028 | [15:0] t1[47:32], [31:16] t2[47:32] | event A time high parts |
| 0x202c / 0x2030 | as 0x2014 / 0x2018 | event B pattern (no trigger type below uses it) |
| 0x2034 | as 0x201c | event B selection / pattern / when |
| 0x2038 / 0x203c / 0x2040 | as 0x2020 / 0x2024 / 0x2028 | event B times |
| 0x2044 / 0x2048 | as 0x2014 / 0x2018 | event R pattern (no trigger type below uses it) |
| 0x204c | as 0x201c | event R ("reset"/qualifier, used by video and serial triggers) |
| 0x2050 / 0x2054 / 0x2058 | as 0x2020.. | event R times |
| 0x205c | [4:0] trigger mux (type), see 5.1 | trigger function select |
| 0x205c | bit5 | force trigger: write 1 then 0 |
| 0x205c | bits 6..10 | ABR sequencer enables (5 flags; Nth edge: 1,1,1,0,1, see 10.11) |
| 0x205c | bit11 | ABR sequencer reset: write 1 then 0 (not used) |
| 0x205c | bit12 | "SPU bypass": 1 = coupling filter path / non-analog source, 0 = DC analog |
| 0x2060 / 0x2064 | low32 / high32 of t / 6.4e6 | "delay time" (6.4 ns unit, not used) |
| 0x2068 / 0x206c | low32 / high32 of t / 6.4e6 | R timeout = Nth-edge idle time |
| 0x2070 / 0x2074 | count | ABR event A / B count (Nth edge: A count = N) |
| 0x2080 | count | holdoff in events (not used, OOS uses the time holdoff) |
| 0x2084 | [31:0] | holdoff ticks bits 31:0, ticks = holdoff / 6.4e6 (6.4 ns) |
| 0x2088 | [15:0] holdoff ticks bits 47:32, [18:16] holdoff type | always type 4 (time holdoff) |
| 0x208c | [1:0] coupling code, [3:2] channel (source-1)&3, [17:4] coefficient (always 0x800) | trigger coupling filter |
| 0x2100..0x21bc | | serial protocol trigger units: UART 0x2100/0x2104, I2S 0x2108..0x2114, LIN 0x2118..0x2138, CAN 0x213c..0x2164, FlexRay 0x2168..0x2174, SPI 0x2178..0x2188, I2C 0x218c..0x21a0, video 0x21a4..0x21ac, MIL-1553B 0x21b0..0x21bc |
| 0x2264 | [3:0] read: EXT trigger comparator state; bit16 written 0 | external trigger input; bit16 is always cleared, whatever the noise-reject setting |

Hand-over state (edge, CH1, rising, DC, mode 0): 0x201c = 0x00aa4100, 0x2034 = 0x6060 (an old value from a D0 source, unused
in mode 0), 0x205c = 0, 0x2088 = 0x00040000, 0x208c = 0.

Not trigger related but in the same block: 0x2200..0x2240 frequency counter, 0x2244..0x2258 DVM.

SCU registers that belong to the trigger: 0x4020 bit0 / 0x4060 / 0x4064 (auto trigger, see `acquisition-start.md` section 3),
0x4034 (trigger/run status, ibid. section 2), 0x40b0/0x40b4 trigger timestamp (read only), 0x4050/0x4054 pre/post
trigger length (section 4 below), 0x4058 scan-mode trigger position.

## 2. Edge trigger

### 2.1 Configuration order

A source change does this, in order:

1. Mark the source channel as trigger channel and force its acquisition on (2.4). If the trigger channel mask changed,
   stop, reconfigure and re-arm.
2. Rewrite levels A and B of all 4 channels.
3. Run the write order of 2.2.
4. Apply the coupling again (2.5). For AC line and EXT the coupling is forced to DC.

A slope change runs 2.2 only. A level change writes level A and level B with the same level (2.3).

None of this runs per acquisition. The acquisition configuration and the arm sequence do not touch the TPU, except for the
scan-mode position 0x4058. **The TPU configuration stays as written until something changes it**, so a warm hand-over leaves
the trigger configured.

### 2.2 Write order

Source numbers: 1..4 = CH1..CH4, 9..24 = D0..D15, 25 = AC line, 26 = EXT, 27 = EXT/5.
Comparator codes `cmpA(src)` / `cmpB(src)`:

| source | cmp A code | cmp B code |
|---|---|---|
| CHn (1..4) | 2n-2 (0, 2, 4, 6) | 2n-1 (1, 3, 5, 7) |
| Dk (9..24) | k + 32 (src + 0x17) | same |
| AC line (25) | 48 | 48 |
| EXT (26) | 49 | 49 |
| other (EXT/5 ...) | 51, source rejected | 51 |

So each analog channel has two comparators: code 2n-2 uses the level A register, code 2n-1 the level B register.

Writes, in order:

1. `trigger(A; 0, 0, 0)`: 0x201c = s201c with [26:24] = 0; 0x2020 = 0; 0x2024 = 0; 0x2028 = 0.
2. `trigger(B; 0, 0, 0)`: 0x2034 = s2034 with [26:24] = 0; 0x2038 = 0x203c = 0x2040 = 0.
3. `bypass(src > 8 || coupling != 0)`: 0x205c bit12.
4. Source/slope, see the table below (`source` keeps [31:16] of the copy, writes [15:0]).
5. `pattern(A; ...)` (keeps everything except [23:16]).
6. (slope "any" only) `pattern(B; ...)`.
7. `mode(m)`: 0x205c[4:0] = m (rest of the low halfword from the copy).
8. Not TPU: front panel slope LEDs (0x202 = rising LED, 0x201 = falling LED), and force a channel on for AC-line/EXT
   when no channel is on.

Analog source CHn, cmpA = 2n-2, cmpB = 2n-1:

| slope | source A (a, ea, b, eb) | source B | pattern A | pattern B | mode | 0x201c for CH1 |
|---|---|---|---|---|---|---|
| rising | (cmpA, 0, cmpB, 1) | - | (2,1,2,1) = 0xaa | - | 0 | 0x00aa4100 |
| falling | (cmpB, 1, cmpA, 0) | - | (2,0,2,0) = 0x22 | - | 0 | 0x00220041 |
| alternating | not supported: steps 1-3 run, then stop without writing source/pattern/mode | | | | | |
| any (either) | (cmpB, 1, cmpA, 0) | (cmpA, 0, cmpB, 1) | 0x22 | 0xaa | 1 (A or B) | 0x00220041, 0x2034 = 0x00aa4100 |

Digital source Dk (code c = k+32): `source(A; c,0,c,0)`, `source(B; c,1,c,1)` for every slope (D0, c = 32, gives 0x2034 = 0x6060),
then patterns by slope as above. No level write: the LA comparators decide (thresholds in 0x12d4, `la.md` 5.1). AC line:
`source(A; 48,0,48,0)`, `source(B; 48,0,48,0)`. EXT: the same with 49. `tpu::edge` covers CH1..CH4 and D0..D15, not AC line and EXT.

Assumed reading of the fields: [5:0]/[7:6] = the comparator whose crossing fires the trigger plus its direction (0 = upward,
1 = downward), [13:8]/[15:14] = the other comparator of the pair with the opposite direction (arming). p1/p3 repeat the
direction. Supporting this: a positive pulse trigger (fires at the falling edge at the end of the pulse) uses the falling
edge selection (cmpB, 1, cmpA, 0). p0/p2: 2 = edge in all edge-like triggers, 1 = time-qualified (pulse, slope),
3/6 = runt, 0 = pattern/level. See 10.13 for all combinations.

### 2.3 Level and hysteresis

For source n = 1..4 (other sources have no level register, no TPU write):

```
scale  = channel scale            (s64 nV/div)
offset = channel offset           (s64 nV)
g      = channel digital gain     (f64; = cal_vertical record +0x28 of the current scale index, see afe-settings.md)
v      = level + offset           (offset NOT added if trigger type == edge and coupling code == 3 (AC))
code(x) = (u32) trunc( x * 7500.0 / scale / g + 32768.0 )     ; negative -> 0
hyst   = (s64) (noise * (float)scale)                         ; nV, single precision, truncated
```

| | register [15:0] (upper) | register [31:16] (lower) |
|---|---|---|
| level A (dirA = 1) | code(v) | code(v - hyst) |
| level B (dirB = 0) | code(v + hyst) | code(v) |

Each half is then clamped: `min(x, 0xfffe)`, and 0 becomes 1. Register = `upper | lower << 16`.

The direction flag differs only for the slope and window types (10.1). For the edge trigger dirA = 1, dirB = 0, so
comparator A has its hysteresis band below the level (arms below, fires on rising through the level) and comparator B above
the level. The divisor is the digital gain, not the probe ratio: all values are at the BNC, so the probe does not enter.

`noise` (in div):
- Noise reject on = 0.7, off = 0.3. A change rewrites level A and B of all 4 channels and clears 0x2264 bit16, whatever the
  setting.
- For the master source channel (the channel the lookup is based on) with scale <= 2 mV/div (2,000,000 nV) the value comes
  from this table, lo <= scale < hi:

| impedance | scale range (nV/div) | noise reject off | on |
|---|---|---|---|
| 1 MOhm / 50 Ohm | 2,000,000 .. 5,000,000 (only 2 mV/div matches) | 0.5 | 0.7 |
| both | 1,000,000 .. 2,000,000 | 1.0 | 1.0 |
| 1 MOhm | 500,000 .. 1,000,000 | 1.25 | 1.3 |
| 50 Ohm | 500,000 .. 1,000,000 | 1.4 | 1.5 |
| both | 200,000 .. 500,000 | 1.15 | 1.2 |
| both | 100,000 .. 200,000 | 2.3 | 2.35 |

  Otherwise (scale > 2 mV/div) 0.3 / 0.7 applies. The table lookup uses the scale and impedance of the master source, while
  the hysteresis is multiplied by the scale of the channel being written.

In codes: hysteresis = noise x 7500 / g codes, e.g. 2250 codes (0.3 div) at g = 1.

Example (computed): CH1, 500 mV/div (5e8 nV), offset 0, g = 1.0, level +1 V (1e9 nV), noise reject off:
code(v) = 32768 + 2 x 7500 = 47768 = 0xba98, hyst = 0.3 x 5e8 = 1.5e8 nV = 2250 codes.
0x2004 = 0xb1ce_ba98 (upper 0xba98, lower 0xb1ce), 0x2008 = 0xba98_c362. Level 0 V at offset 0:
0x2004 = 0x7736_8000, 0x2008 = 0x8000_88ca.

Levels are limited to the range of the source, except for AC and LF-reject coupling. This limit does not change the register
image.

EXT (26): the level has no TPU register, it needs a calibrated DAC. The EXT input is routed through the CH4 AFE Hi-Z output
only on hardware GPIO version 4, and `afe-settings.md` concludes that the DHO924S is not a version-4 board. So treat EXT as
not available on the DHO924S.

### 2.4 Trigger source must be acquired

The TPU works on the sampled data path. The source channel's acquisition must be on even when the channel is not displayed:
the trigger channel must be in the acquisition channel mask (0x4008[3:0] / chmask). The exact mechanism is assumed.

### 2.5 Coupling

| coupling | TPU code (0x208c[1:0]) | 0x205c bit12 |
|---|---|---|
| DC | 0 | 0 |
| AC | 3 | 1 |
| LF reject | 2 | 1 |
| HF reject | 1 | 1 |

GND is not a trigger coupling.

Sequence: if the code equals the stored one, **no write**. Otherwise store, `bypass(coupling != DC)` (0x205c bit12),
`iir(code, source, 0x800)`: 0x208c = (s208c & 0xfffc0000) | code | ((source-1)&3) << 2 | 0x800 << 4.

Consequences: with DC coupling the edge trigger leaves the filter register alone (0x208c = 0 in the hand-over state, so not
even the 0x800 coefficient is set), and changing the source while staying in DC does not update 0x208c[3:2]. So 0x208c is
assumed to matter only when bit12 = 1. The coupling filters are assumed to be digital (an IIR on the samples, the AC case drops
the channel offset from the level). All non-edge types write `iir(0, src, 0x800)`, i.e. DC.

### 2.6 Holdoff

Holdoff t: 0x2088 = s2088 with [18:16] = 4; ticks = t / 6.4e6 (signed, truncated); s2088[15:0] = ticks >> 32; write
0x2084 = ticks & 0xffffffff, then 0x2088 = s2088. A holdoff change then needs a stop, reconfigure and re-arm cycle.
Default holdoff = 8,000,000 fs = 8 ns, so 0x2084 = 1, 0x2088 = 0x00040000.
The holdoff also enters the SCU auto-trigger time t2 = max(holdoff, ...) (see `acquisition-start.md` section 3).
Event holdoff (0x2080) is not used.

### 2.7 Noise reject

Only through the hysteresis (2.3). No separate TPU bit. (0x2264 bit16 is cleared whatever the setting.)

## 3. Sweep (auto / normal / single) and force

- Sweep: AUTO, NORMAL, SINGLE. The TPU itself has **no sweep bit**. AUTO vs NORMAL is only the SCU auto-trigger enable
  0x4020 bit0 plus 0x4060/0x4064, written on every RUN/SINGLE command. See `acquisition-start.md` section 3: auto = (sweep ==
  AUTO), t1 = clamp(20 x tb, 10 ms, 500 ms), t2 = max(holdoff, clamp(10 x tb, 100 ms, 250 ms)). A change to AUTO or NORMAL
  needs a stop, reconfigure and re-arm cycle, a change to SINGLE does not.
- SINGLE = one frame, then STOP, `acquisition-start.md` section 1. The trigger set-up is the same as NORMAL
  (0x4020 bit0 = 0).
- Force: 0x205c = s205c | 0x20, then 0x205c = s205c & ~0x20 (pulse, both writes use the full copy).
- Status: 0x4034 (`acquisition-start.md` section 2: bit4 waiting, bits 6/7 triggered/auto, bit13 stopped).

## 4. Trigger position (pre-trigger)

This is not set in the TPU. The computation:

```
k     = sample period (fs)
D     = record length (samples)
d     = horizontal position of the record start relative to the trigger (fs, <= 0; must be >= -D*k)
ext   = (source == EXT) ? table[chan_count-1] : 0       ; 190/638/296/296 ns latency compensation
t     = D*k + d + ext            (t = 1 if D*k + d == 0)
post  = ceil(t / k)
pre   = max(0, D - post)
fine  = t - post*k               ; sub-sample trigger offset, fs
```

**0x4050 = pre, 0x4054 = post.** Example (assumed): trigger in the middle of the record means d = -D*k/2, so pre = post = D/2.
Also with these: 0x404c (pre-sample compensation) = 5 (10 in average mode). Scan mode: 0x4058 = (enable << 31) | position.
Details of D, k and d are in `horizontal-config.md`.

## 5. Other trigger types (overview)

### 5.1 Type codes

0x205c[4:0] = TPU trigger mux: 0 A, 1 A or B, 2 A and B, 3 ABR sequence, 4 UART, 5 I2S, 6 LIN, 7 CAN, 8 FlexRay,
9 ARINC429, 10 MIL-1553B, 11 SENT, 12 SPI, 13 I2C, 14 Video.

Bit 12 (bypass) details: 10.1. "src" in that column = 1 for sources 9..27 (D0-D15, AC line, EXT), 0 for CH1..CH4.

| type | name | 0x205c mux | bypass bit12 | main TPU use |
|---|---|---|---|---|
| 0 | Edge | 0 / 1 (any) | coupling != DC or digital | event A (+B for any) |
| 1 | Pulse | 0 | src | A: (cmpB,1,cmpA,0) positive / (cmpA,0,cmpB,1) negative, pattern (2,pol,1,0), when + t1/t2 |
| 2 | Slope | 0 | src | A, pattern (2,1,1,0) rising / (2,0,1,0) falling, two levels with direction flags |
| 3 | Video | 14 | never written | A (pattern only) + R, video registers 0x21a4..0x21ac |
| 4 | Pattern | 0 | 1, on the type change | pattern slots per input, A source with eb = 2, when 0 |
| 5 | Duration | 0 | 0, on the type change | as Pattern plus when + t1/t2 |
| 6 | Timeout | 0 / 1 | src | A and B always, when 4 (timeout) with t1 = timeout, t2 = 0 |
| 7 | Runt | 0 / 1 (any) | 0, on the type change | A (B for any), pattern (3,pol,6,0) / (3,x,1,0), when + t1/t2 (when "none" -> 0 with times 0) |
| 8 | Over (window) | 0 / 1 (any) | 0, on the type change | A and B, when 0 or 4 with t1 = t2 = time, two levels with direction flags |
| 9 | Window | no configuration | | |
| 10 | Delay | 0 | 1 if either source is 9..27 | A with two sources (CHx -> CHy), pattern (1,0,1,0), when + t1/t2 |
| 11 | Setup/Hold | 0 / 1 (both) | 1 if either source is 9..27 | A and B, pattern (2,x,3,x) setup / (2,x,6,0) hold, when 1 (<) with the time in t2 |
| 12 | Nth edge | 3 (ABR) | src | A, ABR flags (1,1,1,0,1), 0x2070 = N, 0x2068/0x206c = idle time |
| 13 | RS232/UART | 4 | 1 | R + 0x2100/0x2104, clock divider in 0x221c |
| 14 | I2C | 13 | 1 | A (SCL), B (SDA), 0x218c..0x21a0 |
| 15 | SPI | 12 | 1 | R, A, B, 0x2178..0x2188 |
| 16 | CAN | 7 | 1 | R, 0x213c..0x2164 |
| 17 | CAN FD | no configuration | | |
| 18 | FlexRay | 8 | 1 | R, 0x2168..0x2174 |
| 19 | LIN | 6 | 1 | R, 0x2118..0x2138 |
| 20 | I2S | 5 | 1 | A, B, R, 0x2108..0x2114 |
| 21 | SBUS | no configuration | | |
| 22 | MIL-1553B | 10 | 1 | R, 0x21b0..0x21bc |
| 23..29 | ARINC429, SENT, ..., Area | no configuration (Area/zone uses the SPU mask/zone register 0x11d0 + the SCU mask/zone trigger in 0x4020) | | |

### 5.2 Time condition and pattern encodings

- Time condition, TPU code in 0x201c[26:24]: none -> 5, more than -> 0, less than -> 1, inside -> 2, outside -> 3, any other
  value -> 6. Timeout uses 4 directly. t1 = lower limit, t2 = upper limit. Unit 1.6 ns, 48 bit.
- Pattern values: H -> 2, L -> 3, X/rise/fall -> 0 (the edge position is chosen separately, 10.8).
  Logic operator: OR -> 1, NOR -> 2, AND and NAND -> 0, so there is no NAND in this path.

## 6. OOS implementation: edge trigger

Goal: CH1, rising, level L (nV at the BNC), DC coupling, noise reject off, holdoff 8 ns, sweep AUTO or NORMAL.
For a warm hand-over the AFE and scale are set by the stock system and CH1 is acquired. All registers are write-only. OOS keeps
copies s201c, s2034, s205c, s2088, s208c, starting from the hand-over state: 0x00aa4100, 0x6060, 0, 0x40000, 0 (`tpu::Shadows::APP`).

Inputs: scale (nV/div), offset (nV), g = CH1 digital gain (cal_vertical +0x28 for the scale index; 1.0 if unknown, which
shifts the level by the gain error), hyst = noise(scale) x scale (2.3).

### 6.1 Ordered write list (FPGA in STOP, before the arm sequence of acquisition-start.md section 4)

| # | reg | value | step |
|---|---|---|---|
| 1 | 0x201c | s201c = s201c & ~0x07000000 (when = 0) | `trigger(A; 0,0,0)` |
| 2 | 0x2020 | 0 | |
| 3 | 0x2024 | 0 | |
| 4 | 0x2028 | 0 | |
| 5 | 0x2034 | s2034 = s2034 & ~0x07000000 | `trigger(B; 0,0,0)` |
| 6 | 0x2038 | 0 | |
| 7 | 0x203c | 0 | |
| 8 | 0x2040 | 0 | |
| 9 | 0x205c | s205c = s205c & ~0x1000 (bypass 0 for DC; change the low halfword of the copy and write all 32 bits) | `bypass(0)` |
| 10 | 0x201c | s201c = (s201c & 0xffff0000) \| 0x4100 (a = 0 (CH1 cmp A), ea = 0, b = 1, eb = 1) | `source(A; 0,0,1,1)` |
| 11 | 0x201c | s201c = (s201c & 0xff00ffff) \| 0x00aa0000 | `pattern(A; 2,1,2,1)` |
| 12 | 0x205c | s205c = (s205c & ~0x1f) \| 0 (mux 0 = event A) | `mode(0)` |
| 13 | 0x2004 | code(L+offset) \| code(L+offset-hyst) << 16 | level A |
| 14 | 0x2008 | code(L+offset+hyst) \| code(L+offset) << 16 | level B |
| 15 | 0x2088 | s2088 = (s2088 & ~0x70000) \| 0x40000 | holdoff type 4 |
| 16 | 0x2084 | holdoff / 6.4e6 (8 ns -> 1) | holdoff time |
| 17 | 0x2088 | s2088 = (s2088 & 0xffff0000) \| ((holdoff/6.4e6) >> 32) | |

End state for CH1 rising: 0x201c = 0x00aa4100, 0x2020..0x2028 = 0, 0x205c = 0, 0x2088 = 0x40000, equal to the hand-over state.
Writes 1-12 are the order of 2.2, 13-14 the level, 15-17 the holdoff. The levels and the event configuration are assumed not
to depend on each other's order.

Other choices:
- CHn: a = 2n-2, b = 2n-1 in write 10 (value (2n-1) << 8 | 0x4000 | (2n-2)), level registers from 1. Also make sure CHn is
  in the acquisition channel mask (2.4). Falling: write 10 = (cmpB) | 0x40 | (cmpA << 8), write 11 = 0x00220000.
  Any: falling in A, rising in B (0x2034 = (s2034 & 0xff000000) | 0x00aa4100 after write 8), write 12 mux = 1.
- AC / LF / HF coupling: write 9 with bit12 = 1, plus 0x208c = (s208c & 0xfffc0000) | code | (n-1) << 2 | 0x8000 (code AC 3,
  LFR 2, HFR 1), and for AC leave the offset out of the level codes.
- Sweep AUTO/NORMAL: 0x4020 bit0 and 0x4060/0x4064 as in `acquisition-start.md`. Force: 0x205c bit5 pulse.

### 6.2 Assumptions

- The end state 0x201c = 0x00aa4100, 0x205c = 0, 0x2088 = 0x40000, 0x208c = 0 (DC) equals the hand-over state.
- Assumed: the TPU takes effect without a TPU reset (the edge trigger needs no reset, the ABR reset is unused); writing in STOP
  is enough (writes while running are also expected to work); the meaning of the pattern/edge fields; the digital-gain source
  for OOS.
- The level registers cannot be read back. A first test: write the computed level with a known DC input and check
  trigger/no trigger in NORMAL.

## 8. Not yet verified on hardware

1. Meaning of p0/p2 and of the edge bits [7:6]/[15:14] beyond the observed combinations (10.13).
2. Which comparator of a channel the pattern H/L test uses (10.8).
3. Whether 0x208c[3:2] (channel) is used when bit12 = 0 (assumed not).
4. Whether a changed source needs anything beyond 0x201c, apart from forcing the channel's acquisition on (2.1).
5. The ground code of `afe-settings.md` is computed without the digital gain, the trigger level divides by it. Which one matches
   the ADC data is not settled.
6. The units of 0x4050/0x4054 (samples at the ADC rate or after decimation), see `horizontal-config.md`.

## 10. Other trigger types (register level)

Numbered 10 because it continues section 6 for the non-edge types. `tools/tpumodel.py` prints the ordered writes for every type
below (`tpumodel.py selftest` checks the edge case against `hal/src/dho/tpu.rs`). Times in fs, levels in nV.

### 10.1 Common elements

**Primitives** (layouts as in section 1). E is an event block: A (0x201c), B (0x2034) or R (0x204c).

| primitive | effect |
|---|---|
| `source(E; a, ea, b, eb)` | copy[15:0] = a[5:0] \| ea[1:0]<<6 \| b[5:0]<<8 \| eb[1:0]<<14, [31:16] from the copy, write. `ea`/`eb` are 2-bit fields (the pattern trigger uses eb = 2) |
| `pattern(E; p0, p1, p2, p3)` | copy[23:16] = p0[2:0]<<16 \| p1<<19 \| p2[2:0]<<20 \| p3<<23, write |
| `trigger(E; when, t1, t2)` | Tn = trunc(tn / 1.6e6) (signed, rounds toward 0); copy[26:24] = when, write base; base+4 = T1[31:0]; base+8 = T2[31:0]; base+0xc = T1[47:32] \| T2[47:32]<<16 |
| `mode(m)` | 0x205c[4:0] = m, all other bits from the copy |
| `bypass(b)` | 0x205c bit12 = b, all other bits from the copy |
| `abr(a,b,c,d,e)` | 0x205c bits 6,7,8,9,10 = a,b,c,d,e |
| `iir(c, src, k)` | 0x208c = (s & 0xfffc0000) \| c \| ((src-1)&3)<<2 \| k<<4 |
| `logic(i, v, op)` | i <= 15: 0x2014[2i+1:2i] = v, write 0x2014; 16 <= i <= 20: copy of 0x2018[2(i-16)+1:2(i-16)] = v (no write); then always 0x2018[11:10] = op, write 0x2018 |
| `r_timeout(t)` | 0x2068 = (t/6.4e6)[31:0], 0x206c = bits 63:32 |
| `abr_count(n)` | 0x2070 = n |
| `video1(m, line)` | 0x21a4 = m[1:0] \| line[10:0]<<2 |
| `video2(t1, t2)` | 0x21a8 = (t1/clk)[15:0] \| (t2/clk)[15:0]<<16, clk = FPGA clock cycle (10.12) |
| `video3(a, n, c, d)` | 0x21ac = a[10:0] \| n[10:0]<<11 \| c<<22 \| d<<23 |

The event B and R pattern registers (0x202c/0x2030, 0x2044/0x2048) are not used by any type in this section.

**Levels.** Two-level types (slope, runt, window): **level A = high level, level B = low level.** High must stay >= low + 0.1 div
and low <= high - 0.1 div. All other types use one level per source (A and B equal). The hysteresis side depends on a
direction flag:

| dir | register [15:0] (upper) | [31:16] (lower) | comparator (assumed reading) |
|---|---|---|---|
| 1 | code(v) | code(v - h) | switches high exactly at v rising, low at v - h |
| 0 | code(v + h) | code(v) | switches low exactly at v falling, high at v + h |

| type | dirA | dirB |
|---|---|---|
| 2 slope | slope != falling | slope == rising |
| 8 window | event != exit | event == exit |
| all others | 1 | 0 |

"levels" in the tables below means: write the level registers of all four channels, A and B. Levels are written again whenever a
level moves. `hal/src/dho/tpu.rs` writes the levels of the sources only.

**Bit 12 (SPU bypass):** pulse, slope, timeout, Nth edge: each sequence writes 9 <= src < 28 (D0..D15, AC line, EXT),
so 0 for CH1..CH4. Delay, setup/hold: 1 if either source is in that range. Pattern: 1, written on the type change, before
the sequence. Duration, runt, window: 0, written on the type change, the sequence does not touch it (so a source/polarity change
keeps the old value). Video: never written.

**Coupling filter:** every type writes `iir(0, src, 0x800)`, so 0x208c = 0x8000 | (src-1)<<2 (DC). Pattern and
duration write it for src 1, 2, 3, 4 (end value 0x800c).

**Not cleared when switching types:** each sequence writes only what it uses. Not reset: event B/R
configuration (0x2034.., 0x204c..), the ABR flags 0x205c[10:6] (only Nth edge writes them, nothing clears them; the ABR
reset bit 11 is unused), 0x2070/0x2068/0x206c, video 0x21a4..0x21ac, and for some types even the event A selection
(pulse "any", duration/pattern without edge, video: 0x201c[15:0] keeps the previous type's value). Whether the TPU
ignores these in mux 0/1 is not known. See 10.15.

**Time condition.** More than, less than, inside, outside, none -> 0, 1, 2, 3, 5. t1 = lower limit, t2 = upper limit.
For a single width w: more than -> lower = w; less than -> upper = w; inside -> lower = w, upper shifted by the same
amount. The other limit keeps its stored value and is written too. Unit 1.6 ns, truncated, 48 bit.
Encoding: 0 `>` (t1), 1 `<` (t2), 2 inside (t1 < t < t2), 3 outside, 4 timeout (t1), 5 none. The semantics of 2/3/4 are assumed.

### 10.2 Pulse width (type 1)

Parameters: source, polarity (positive, negative, any), when, t1 (lower), t2 (upper). A change of when or width needs only
`trigger(A; when, t1, t2)`. A change of source or polarity runs the whole sequence.

| # | write |
|---|---|
| 1 | 0x205c bit12 = (9 <= src < 28) |
| 2 | levels (single level, dirA 1, dirB 0) |
| 3 | 0x208c = 0x8000 \| (src-1)<<2 |
| 4-7 | `trigger(A; when, t1, t2)`: 0x201c[26:24], 0x2020, 0x2024, 0x2028 |
| 8 | positive: `source(A; cmpB,1,cmpA,0)`; negative: `source(A; cmpA,0,cmpB,1)`; any: nothing |
| 9 | positive: `pattern(A; 2,0,1,0)`; negative: `(2,1,1,0)`; any: nothing |
| 10 | 0x205c[4:0] = 0 |

Polarity "any" leaves 0x201c[23:0] as the previous type left it.
Example CH1, positive, > 100 ns: t1 = 62 ticks. End: 0x201c = 0x00120041, 0x2020 = 62 (0x3e), 0x2024 = stored upper,
0x2028 = 0, 0x205c[4:0] = 0. Negative: 0x201c = 0x001a4100.

### 10.3 Slope / transition time (type 2)

Parameters: source, slope (rising, falling), when, t1 (lower), t2 (upper). **The sequence does not write the time condition**;
a change of when or width writes `trigger(A; when, t1, t2)` on its own (the model appends it after the sequence).

| # | write |
|---|---|
| 1 | 0x205c bit12 = (9 <= src < 28) |
| 2 | levels: A = high, B = low; rising dirA = dirB = 1, falling dirA = dirB = 0 |
| 3 | 0x208c = 0x8000 \| (src-1)<<2 |
| 4 | rising: `source(A; cmpA,0,cmpB,0)`; falling: `source(A; cmpB,1,cmpA,1)`; other: stop |
| 5 | rising: `pattern(A; 2,1,1,0)`; falling: `(2,0,1,0)` |
| 6 | 0x205c[4:0] = 0 |
| (7) | `trigger(A; when, t1, t2)` |

Assumed reading: rising = time from the low comparator (B) going high to the high comparator (A) going high; falling = from
A going low to B going low. Both thresholds are exact in the measured direction (dir flags).
Example CH1 500 mV/div, high 1 V, low 0.2 V, rising, < 20 ns: 0x2004 = 0xb1ceba98, 0x2008 = 0x82ee8bb8,
0x201c = 0x011a0100, 0x2020 = 0, 0x2024 = 12, 0x205c[4:0] = 0.

### 10.4 Runt (type 7)

Parameters: source, polarity (positive, negative, any), when, t1 (lower), t2 (upper). Every change runs the whole sequence.

| # | write |
|---|---|
| (0) | 0x205c bit12 = 0 (only on a type change) |
| 1 | levels: A = high (dir 1), B = low (dir 0) |
| 2 | 0x208c = 0x8000 \| (src-1)<<2 |
| 3 | when == 5 (none) -> when = 0, t1 = t2 = 0 |
| 4 | positive: `source(A; cmpA,0,cmpB,0)`, `pattern(A; 3,0,6,0)` |
|   | negative: `source(A; cmpB,1,cmpA,1)`, `pattern(A; 3,1,6,0)` |
|   | any: `source(A; cmpA,0,cmpB,0)`, `pattern(A; 3,0,1,0)` |
| 5 | `trigger(A; when, t1, t2)` |
| 6 | any only: `source(B; cmpB,1,cmpA,1)`, `pattern(B; 3,1,1,0)`, `trigger(B; when, t1, t2)` |
| 7 | 0x205c[4:0] = 0 (positive/negative), 1 (any, A or B) |

Note p2 = 6 for a single polarity but 1 for "any"; why is unknown. Assumed reading: positive runt = B (low) goes high
(start), A (high) does not go high, B goes low again (fire); the time condition applies to the runt width.
Example CH1 500 mV/div, high 2 V, low 0.8 V, positive, no width condition: 0x2004 = 0xec66f530, 0x2008 = 0xaee0b7aa,
0x201c = 0x00630100, 0x2020..0x2028 = 0, 0x205c[4:0] = 0.

### 10.5 Window (type 8 "Over")

The window trigger is type 8. Type 9 has no configuration. Parameters: source, slope (rising, falling, any; alternating is
rejected), event (enter, exit, time), time. Every change runs the whole sequence.

| # | write |
|---|---|
| (0) | 0x205c bit12 = 0 (on a type change) |
| 1 | levels: A = high, B = low; dirA = (event != exit), dirB = (event == exit) |
| 2 | 0x208c = 0x8000 \| (src-1)<<2 |
| 3 | `source(A; cmpA,0,cmpB,0)`, `source(B; cmpB,0,cmpA,0)` |
| 4 | falling only: `source(A; cmpB,0,cmpA,0)` |
| 5 | `trigger(A; w, t, t)` (and for any `trigger(B; w, t, t)`), w = 4 and t = time for event "time", else 0, 0, 0 |
| 6 | pattern A (and B for any), table below |
| 7 | 0x205c[4:0] = 0 (rising, falling), 1 (any) |

| slope \ event | enter | exit | time |
|---|---|---|---|
| rising: A | (2,1,2,1) | (2,0,2,0) | (2,0,2,0), when 4 |
| falling: A (= cmpB,0,cmpA,0) | (2,0,2,0) | (2,1,2,1) | (2,1,2,1), when 4 |
| any: A (cmpA,0,cmpB,0) / B (cmpB,0,cmpA,0) | (2,1,2,1) / (2,0,2,0) | (2,0,2,0) / (2,1,2,1) | (2,0,2,0) / (2,1,2,1), when 4 |

"any" = rising OR falling for enter and exit, consistent. Note that rising "time" uses the rising "exit" pattern
plus the timeout condition; its meaning is open (10.13).
Example CH1 500 mV/div, high 1 V, low -1 V, rising enter: 0x2004 = 0xb1ceba98, 0x2008 = 0x45684e32 (dir 0),
0x201c = 0x00aa0100, 0x2034 = (s2034 & 0xffff0000) | 0x0001, 0x205c[4:0] = 0. Any, time 1 us: 0x201c = 0x04220100,
0x2020 = 0x2024 = 625, 0x2034 = 0x04aa0001, 0x2038 = 0x203c = 625, 0x205c[4:0] = 1.

### 10.6 Timeout (type 6)

Parameters: source, slope (rising, falling, any), timeout T. Every change runs the whole sequence.

| # | write |
|---|---|
| 1 | 0x205c bit12 = (9 <= src < 28) |
| 2 | levels (single level) |
| 3 | 0x208c = 0x8000 \| (src-1)<<2 |
| 4 | `source(A; cmpA,0,cmpB,0)`, `source(B; cmpB,0,cmpA,0)` |
| 5 | `trigger(A; 4, T, 0)`, `trigger(B; 4, T, 0)` (always both) |
| 6 | rising: `pattern(A; 2,0,2,0)`, mode 0 |
|   | falling: `source(A; cmpB,0,cmpA,0)`, `pattern(A; 2,1,2,1)`, mode 0 |
|   | any: `pattern(A; 2,0,2,0)`, `pattern(B; 2,1,2,1)`, mode 1 |

The encodings equal the window "time" ones with both levels equal. Example CH1 rising, 1 us: t1 = 625;
0x201c = 0x04220100, 0x2020 = 0x271, 0x2024 = 0, 0x2034 = 0x04000001 (B configured but unused in mode 0),
0x205c[4:0] = 0.

### 10.7 Delay (type 10)

Parameters: source A, source B, slope A, slope B (rising, falling), when, t1 (lower), t2 (upper). A change of when or width needs
only `trigger(A; when, t1, t2)`.

| # | write |
|---|---|
| 1 | levels of both sources |
| 2 | 0x205c bit12 = 1 if srcA or srcB in 9..27 |
| 3 | 0x208c for srcA, then for srcB |
| 4 | `trigger(A; when, t1, t2)` |
| 5 | `source(A; eB, slopeB, eA, slopeA)` with eX = cmpA(srcX) if rising, cmpB(srcX) if falling |
| 6 | `pattern(A; 1,0,1,0)` (all four slope combinations) |
| 7 | 0x205c[4:0] = 0 |

This is the cleanest indication of the field meaning (10.13): [13:8]/[15:14] = start edge (source A), [5:0]/[7:6] = the
edge that fires (source B), e = 0 rising (comparator A, exact on rising) / 1 falling (comparator B).
Example CH1 rising -> CH2 falling, > 50 ns: t1 = 31; 0x201c = 0x00110043, 0x2020 = 31, 0x205c[4:0] = 0,
0x208c = 0x8004.

### 10.8 Pattern (type 4)

Parameters: one entry per input (H, L, X, rise, fall), 21 entries: index 0..3 = CH1..CH4, 4..19 = D0..D15, 20 = EXT (always X);
and a logic operator. A pattern change writes the levels of the channels that are not X, then runs the sequence.

| # | write |
|---|---|
| (0) | 0x205c bit12 = 1 (on a type change) |
| 1-4 | 0x208c for src 1, 2, 3, 4 (ends 0x800c) |
| 5 | for i = 0..20: `logic(i, sel(pat[i]), fun(op))`, sel: H -> 2, L -> 3, X/rise/fall -> 0; fun: OR -> 1, NOR -> 2, AND/NAND -> 0. 16 writes of 0x2014 and 21 of 0x2018 |
| 6 | first rise/fall entry e: `source(A; c, fall?1:0, c, 2)` with c = `cmpA(e+1)`, then `pattern(A; 2, rise?1:0, 0, 0)`; no edge: only `pattern(A; 0,0,0,0)` |
| 7 | `trigger(A; 0,0,0)` |
| 8 | 0x205c[4:0] = 0 |

Pattern input slot n of 0x2014/0x2018 = array index n, i.e. 0x2014[7:0] = CH1..CH4,
0x2014[31:8] = D0..D11, 0x2018[7:0] = D12..D15, 0x2018[9:8] = EXT. 2-bit value: 0 X, 2 H,
3 L. NAND maps to the AND code, so the TPU has no NAND in this path.

Only CH1..CH4 edges work. In step 6 the argument of `cmpA` is index + 1 read as a source number: an edge on D0..D3 gives comparator
code 51, D4..D15 give the codes of D0..D11 (a D5 edge selects code 33 = D1; the fixture `hal/testdata/tpu_expected.txt` contains
this case). Which comparator of a channel the H/L test uses is not known (both levels are equal for this type).
Example CH1 H, CH2 L, CH3 rise, CH4 X, AND: 0x2014 = 0x0000000e, 0x2018 = 0, 0x201c = (s & 0xff000000) | 0x000a8404
(c = 4 = CH3 cmpA, eb = 2, p0 = 2, p1 = 1), 0x2020..0x2028 = 0, 0x205c = 0x1000.

### 10.9 Duration (type 5)

As pattern, with its own pattern entries, logic operator, when, t1 and t2, and:
(0) bit12 = **0** on a type change; step 6 with an edge: `source(A; c, fall?1:0, c, 2)` with c = `cmpA(e)`
(**index, not index+1**: an edge on CH1 gives code 51, CH2 gives CH1's code 0; `tpu::duration` reproduces this), then
`pattern(A; 0, 0, 2, rise?1:0)`; no edge (the normal case, only H/L/X are offered): only
`pattern(A; 0,0,0,0)`, 0x201c[15:0] keeps the previous value; step 7 = `trigger(A; when, t1, t2)`.
Example CH1 H, CH2 H, > 1 us: 0x2014 = 0xa, 0x201c[26:16] = 0 ([15:0] unchanged), 0x2020 = 625,
0x205c[4:0] = 0, bit12 = 0.

### 10.10 Setup/hold (type 11)

Parameters: clock source, data source, clock slope (rising, falling), data type (H, L), event (setup, hold, setup and hold),
setup time, hold time. Every change runs the whole sequence.
Let C = clock comparator (cmpA(clk), e 0 if rising; cmpB(clk), e 1 if falling), Din = data edge into the valid level
(H: cmpA(data), e 0; L: cmpB(data), e 1), Dout = data edge out of it (H: cmpB(data), e 1; L: cmpA(data), e 0).

| # | write |
|---|---|
| 1 | 0x205c bit12 = 1 if clk or data in 9..27 |
| 2 | levels of clk and data |
| 3 | 0x208c for clk, then data |
| 4 | setup / both: `source(A; C, eC, Din, eDin)`, `pattern(A; 2, clk rising?1:0, 3, L?1:0)`, `trigger(A; 1, 0, setup)` |
|   | hold: `source(A; Dout, eDout, C, eC)`, `pattern(A; 2, L?1:0, 6, 0)`, `trigger(A; 1, 0, hold)` |
| 5 | both only: `source(B; Dout, eDout, C, eC)`, `pattern(B; 2, L?1:0, 6, 0)`, `trigger(B; 1, 0, hold)` |
| 6 | 0x205c[4:0] = 0 (setup, hold), 1 (both) |

The time goes into t2 with when = 1 (<). Assumed reading: setup violation = data entered its level less than t before
the clock edge (start Din, fire C); hold violation = data left its level less than t after the clock edge (start C,
fire Dout).
Example clk CH1 rising, data CH2 H, both, setup 5 ns, hold 2 ns: 0x201c = 0x013a0200, 0x2024 = 3,
0x2034 = 0x01620043, 0x203c = 1, 0x205c[4:0] = 1.

### 10.11 Nth edge (type 12)

Parameters: source, slope (rising, falling), count N, idle time. A source or slope change runs the whole sequence, a count
change writes only 0x2070, an idle time change only 0x2068/0x206c.

| # | write |
|---|---|
| 1 | 0x205c bit12 = (9 <= src < 28) |
| 2 | levels (single level) |
| 3 | 0x208c = 0x8000 \| (src-1)<<2 |
| 4 | `source(A; cmpA, s, cmpB, s)` (s = slope: 0 rising, 1 falling) |
| 5 | falling: `source(A; cmpB,1,cmpA,1)`, `pattern(A; 2,0,2,0)`; rising: `pattern(A; 2,1,2,1)` |
| 6 | `trigger(A; 0,0,0)`, `trigger(B; 0,0,0)` |
| 7 | `abr(1,1,1,0,1)`: 0x205c bits 6, 7, 8, 10 |
| 8 | 0x2070 = N |
| 9 | 0x2068 = (idle / 6.4e6)[31:0], 0x206c = high part |
| 10 | 0x205c[4:0] = 3 (ABR) |

Event B selection and 0x2074 (B count) are not written; event R has no selection either. So the sequencer is assumed to need
only A, its count and the R timeout. The ABR flags stay set after leaving Nth edge (10.1).
Example CH1 rising, N = 3, idle 1 us: 0x201c = 0x00aa0100, 0x2070 = 3,
0x2068 = 156, 0x206c = 0, 0x205c = 0x5c3. Falling N = 5, 2 us: 0x201c = 0x00224041, 0x2068 = 312.

### 10.12 Video (type 3), NTSC and PAL

Parameters: source, polarity (0/1; which one is "positive sync" is not known), standard (NTSC, PAL, HD formats), sync (any line,
line x, odd field, even field), line. Every change runs the whole sequence, then a stop, reconfigure and re-arm cycle.
Odd/even set the line first: NTSC odd 4, even 267; PAL odd 1, even 313 (1080i: 1 / 563).

| # | write |
|---|---|
| 1 | levels (single level) |
| 2 | 0x208c = 0x8000 \| (src-1)<<2 |
| 3 | `source(R; cmpA, 0, cmpA, 0)` |
| 4 | `pattern(R; 0, 0, 2, 0)` |
| 5 | `pattern(A; 2,1,2,1)` if polarity != 0, else `(2,0,2,0)`; **no `source(A; ...)`** (0x201c[15:0] keeps the old value) |
| 6 | 0x205c[4:0] = 14 (video) |
| 7 | `video2(t1, t2)`: NTSC (4.7 us, 58.79 us), PAL (4.7 us, 59.3 us) |
| 8 | `video3`: NTSC (5, 525, 0, 1), PAL (2, 625, 0, 1) |
| 9 | `video1(sync != 0, line)` (HD formats: bit1 also set, 0x21ac bit22 = 1, bit23 = 0) |

Assumed reading: `video3` = (field start offset?, lines per frame, progressive, interlaced); `video2` = sync pulse width and line
period; `video1` = (line mode, line number). Bit12 is never written by the video path.

**Divisor of `video2`:** the FPGA clock cycle. 0x21a8 for both candidates:

| | 6.4 ns (`tpu::VideoClk::Stock`) | 4 ns (`tpu::VideoClk::Fast`) |
|---|---|---|
| NTSC | 734 \| 9185<<16 = 0x23e102de | 1175 \| 14697<<16 = 0x39690497 |
| PAL | 734 \| 9265<<16 = 0x243102de | 1175 \| 14825<<16 = 0x39e90497 |

OOS writes the 4 ns values. Every other TPU time uses the fixed 6.4 ns or 1.6 ns units, so the 6.4 ns values fit the TPU clock and
the 4 ns values are 1.6x larger. Which one the FPGA expects is not yet verified on hardware.
Example PAL, line 100, polarity 0, 4 ns: 0x204c = (s & 0xff000000) | 0x00200000, 0x201c[23:16] = 0x22, 0x205c[4:0] = 14,
0x21a8 = 0x39e90497, 0x21ac = 0x00938802, 0x21a4 = 0x191.

### 10.13 Meaning of the event fields [15:0] and [23:16]

All used combinations (a, ea, b, eb | p0, p1, p2, p3), comparator A = level A register, B = level B:

| type / case | [15:0] | [23:16] | when |
|---|---|---|---|
| edge rising / falling | A,0,B,1 / B,1,A,0 | 2,1,2,1 / 2,0,2,0 | 0, t = 0 |
| Nth rising / falling | A,0,B,0 / B,1,A,1 | 2,1,2,1 / 2,0,2,0 | 0 |
| pulse positive / negative | B,1,A,0 / A,0,B,1 | 2,0,1,0 / 2,1,1,0 | user |
| slope rising / falling (A high, B low) | A,0,B,0 / B,1,A,1 | 2,1,1,0 / 2,0,1,0 | user |
| delay (start src A edge, fire src B edge) | eB,sB,eA,sA | 1,0,1,0 | user |
| setup | C,eC,Din,eDin | 2,clk rising,3,data L | 1 (<) |
| hold | Dout,eDout,C,eC | 2,data L,6,0 | 1 (<) |
| runt positive / negative | A,0,B,0 / B,1,A,1 | 3,0,6,0 / 3,1,6,0 | user (none -> 0, t = 0) |
| runt any (A / B) | A,0,B,0 / B,1,A,1 | 3,0,1,0 / 3,1,1,0 | user |
| window rising enter / exit / time | A,0,B,0 | 2,1,2,1 / 2,0,2,0 / 2,0,2,0 | 0 / 0 / 4 |
| window falling enter / exit / time | B,0,A,0 | 2,0,2,0 / 2,1,2,1 / 2,1,2,1 | 0 / 0 / 4 |
| timeout rising / falling | A,0,B,0 / B,0,A,0 | 2,0,2,0 / 2,1,2,1 | 4 |
| pattern with edge (c = cmpA of CHn) | c,fall,c,2 | 2,rise,0,0 | 0 |
| duration with edge | c,fall,c,2 | 0,0,2,rise | user |
| pattern/duration without edge | unchanged | 0,0,0,0 | 0 / user |
| video A / R | unchanged / A,0,A,0 | 2,pol,2,pol / 0,0,2,0 | - |

What the combinations support:
- [5:0] and [13:8] are comparator codes (CHn level A = 2n-2, level B = 2n-1; D0..D15 = 32..47, AC line 48, EXT 49).
- Assumed: for the two-edge types (pulse, slope, delay, setup, hold, runt) **b ([13:8], eb) = start edge, a ([5:0], ea) = end edge
  that fires the trigger, e = 0 rising / 1 falling**, and the time condition applies to the interval b -> a. The strongest case is
  delay, where a and b come from different channels and e follows the selected slope exactly; setup/hold fit the same reading.
- Assumed: p1 = 1 exactly when the firing edge is rising in edge, Nth, pulse, slope, setup, hold, runt (runt fires on the return
  crossing) and pattern. It does not fit window/timeout, where e = 0 for both comparators and p1/p3 alone carry the direction.
- Assumed: p0/p2 are an event kind: 2/2 with p3 = p1 = single edge or level-crossing state (edge, Nth, window, timeout,
  video); x/1 = time between two edges (pulse, slope, delay); 2/3 and 2/6 = setup and hold; 3/6 = runt (3/1 for runt any);
  0 = pattern/logic. eb = 2 appears only for the pattern and duration edge ("b unused"?).
- Open: the window/timeout encodings. With A = high and B = low the rising "enter" uses the same tuple as Nth rising
  and the rising "time" the same as timeout rising; a consistent per-bit meaning for (2,p,2,p) with e = 0 is not known.
  Note also that the window dir flags put the hysteresis on the inner side of the window for "enter" if A is the high
  level. A bench test (window, CH1, known DC steps) would settle which crossing each tuple selects.

### 10.15 OOS implementation notes

- Keep copies of 0x201c, 0x2034, 0x204c, 0x205c, 0x2088, 0x208c, 0x2014, 0x2018 (all write-only), as in `tpu.rs`.
- `tools/tpumodel.py` and `tpu.rs` replay the sequences above and write only what each type uses. For a clean state on a type
  change it is cheap to additionally write: `trigger(A; 0,0,0)`, `trigger(B; 0,0,0)`, ABR flags 0x205c[10:6] = 0 unless Nth edge,
  mux, bit12 per 10.1. This is assumed to be harmless and avoids inheriting fields no sequence clears.
- Levels: two-level types need both level A (high) and B (low) with the dir flags of 10.1; one-level types write the
  same level to A and B (dirA 1, dirB 0) as for the edge trigger.
- The trigger source(s) must be acquired (2.4); delay and setup/hold need both channels.
- Video: decide between the 6.4 ns and the 4 ns divisor by a bench test (10.12).

### 10.16 Not yet verified on hardware

1. Per-bit meaning of p0/p2, of eb = 2 and of the window/timeout tuples (10.13). Window: does level A act as the high
   threshold in hardware, or as the low one?
2. Whether the TPU ignores stale event B/R fields and ABR flags 0x205c[10:6] in mux 0/1.
3. Video: polarity value meaning, `video3` field meaning, and which clock divisor the FPGA expects (4 ns or 6.4 ns).
4. Runt "any" uses p2 = 1 instead of 6; whether that is required.
5. Window "time" semantics (inside-window time vs. exit-and-timeout).
