# Built-in generator (AFG, "GI/GO" BNC, DHO924S)

Scope: the generator of the "S" models. Sections 1-11 describe the hardware and the required sequences. Section 14
lists what was verified on the unit. Sections 15-16 are host-tested only and marked as such.

Related docs: `self-cal.md` (container format 7.1, factory-only AFG cal, section 6), `cold-clock.md` 2.3 (module init
levels), `../hardware/gpio-map.md` (afg_in1..5, FG_M2C), `cold-boot.md` (never-do list), `board.md`.

## Short answer

- **DDS in the FPGA, everything through BAR1 0x3000..0x3ffc** (32-bit writes, the same 32 KiB `/dev/xdma0_bypass` map
  as the rest). One 8192-sample s16 waveform RAM (carrier), one 4096-entry modulation RAM, a 56-bit phase increment at
  a **156.25 MHz** DDS clock (2^52 = one cycle), a 16-bit phase offset, and two 16-bit DAC words (amplitude, offset)
  written as `0xffff - code`. Sine, square, ramp, pulse, noise, DC and arbitrary are all just different tables in the
  same RAM; only noise uses a fixed phase increment.
- **Analog path**: 5 relay/attenuator lines `afg_in1..5` (GPIO 122..126, under stock `/dev/afg_gpio`, one byte = 5 bits)
  select -10 dB / -20 dB / x10 / -20 dB / always-on; `FG_M2C_GPIO` (GPIO 149, under stock `/dev/fg_m2c_gpio`) is the
  generator supply switch, on only while the output is enabled. The gear is picked from amplitude and offset thresholds.
- **Output enable** = afg_gpio 0 -> FG_M2C 1 -> relay code (bits 0..3 first, 3 ms, then bit 4) -> FPGA 0x3000 = 1.
- **Calibration** `cal_afg.hex`: 24-byte header + 40 records of 2408 bytes, each a list of up to 100 ranges with a
  linear correction `y = k*x + b` (f32 k, b). Records 0..15 offset per gear, 16..29 amplitude per gear,
  30..33 flatness below 100 kHz, 34..39 flatness from 100 kHz. Header +0x10/+0x14 = frequency k/b (OOS does not apply it).
- **Minimal 1 kHz / 1 Vpp / 0 V sine**: complete list in section 9, values computed with an example cal file.
  The amplitude is peak-to-peak into high impedance, and the 156.25 MHz clock runs under OOS: both verified on the
  unit (section 14).
- **No sync output, no burst, no sweep, no FSK in hardware**: modulation is AM / FM / PM with shapes sine, square,
  triangle, up-ramp, down-ramp, noise. A Bode plot sweeps by setting frequencies from software.
- **Frequency**: OOS caps sine, noise and DC at 60 MHz: the 156.25 MHz DDS at 2.6 samples per period. The amplitude is
  capped to 2.5 V above 30 MHz. The factory flatness tables end at 25 MHz, so above that no flatness correction exists.

## 1. Layers

| layer | what it does |
|---|---|
| limits | clamps frequency, amplitude and offset (section 7) |
| generator state | keeps the settings (section 7), builds tables, computes gears and DAC codes |
| FPGA / GPIO helpers | register sequences (section 2.4), GPIO writes (section 4) |
| bus | 32-bit BAR1 store |

The sequences need no 64-bit store and no read of the 0x3xxx block. Do not read it (section 11).

The generator is initialised once at start: FPGA tables loaded, output off. Under stock the relay state of a running
unit matches this (4.2).

## 2. FPGA register block (BAR1)

### 2.1 Bank selects

Three strobe/bank registers frame every access: write 1, write the data registers, write 0. The offsets
0x3000..0x307c are shared between banks (0x3000, 0x3030 and 0x3034 mean different things in bank B and bank M), so the
bank register selects which block receives the write (the sequences are certain; this meaning is assumed).

| bank | select | contents |
|---|---|---|
| B (basic / DDS) | 0x3ff0 | output control, waveform RAM port, phase, phase increment |
| M (modulation) | 0x3ff4 | modulation type, deviation, modulation rate, two gain words |
| A (amplitude) | 0x3ff8 | amplitude and offset DAC words |

Some sequences write the select twice (1, 1) or leave it at 1 between two blocks; it always ends at 0.

### 2.2 Registers

| bank | off | written values | meaning |
|---|---|---|---|
| B | 0x3000 | 0, 1, 0x80 then 0x81 | output control: bit 0 = output on; bit 7 set together with bit 0 when FM with shape 5 (noise) is active. Written 0 at the start of every RAM upload and every modulation setting |
| B | 0x3030 | 0 / 1 | RAM select: 0 = carrier RAM, 1 = modulation RAM (meaning assumed) |
| B | 0x3034 | 0x1fff | RAM last address (also for the 4096-entry modulation RAM; meaning assumed) |
| B | 0x3038 | sample stream | RAM data port, auto-increment; carrier samples are s16 **sign-extended** to 32 bits, AM table u16 zero-extended |
| B | 0x3040 | `(int)(phase_mdeg * 0.001 * 65536 / 360)` | start phase, 16 bit |
| B | 0x3048 / 0x304c / 0x3050 / 0x3054 | inc bits 0-15 / 16-31 / 32-47 / 48-55 | phase increment |
| M | 0x3000 | 0, 0x82, 0x84, 0x88 | modulation: 0 none, 0x82 AM, 0x84 FM, 0x88 PM |
| M | 0x3004 / 0x3008 / 0x300c | 48-bit deviation word | FM: `(d << 1)` split 16/16/16 with d = `dev_uHz / 156.25e12 * 2^48`; PM: see 2.3 |
| M | 0x3010 | 0xffff (basic, AM), 0 (FM, PM) | gain word (meaning assumed) |
| M | 0x3014 | 0x7fde (basic), 0 (AM), 0xffff (FM, PM, end of init) | gain word (meaning assumed) |
| M | 0x302c / 0x3030 / 0x3034 | 48-bit rate word | modulation rate: `f_uHz / 78.125e12 * 2^48` (16/16/16) |
| A | 0x306c, 0x3074 | 0xffff | written only at init; unused DAC channels or limits (assumed) |
| A | 0x3070 | `0xffff - ampl_code` | amplitude DAC |
| A | 0x3078 | `0xffff - offset_code` | offset DAC |

No other offset in 0x3000..0x3fff is used.

### 2.3 Formulas (units: frequency in uHz, phase in milli-degrees)

- Carrier phase increment: `inc = trunc((f_uHz / 156250000000000.0) * 2^52)` (double), written as 56 bits. With noise
  (type 4) the increment is fixed `0x4000000000` (2^38). Clock 156.25 MHz, one table cycle = 2^52; 8192 samples are
  probably addressed by the top 13 bits (assumed; noise is then replayed at 78.125 MSa/s and repeats every 8192
  samples, about 9.5 kHz).
- Modulation rate (AM/FM/PM): `trunc((f_uHz / 78125000000000.0) * 2^48)` -> 0x302c/0x3030/0x3034. With modulation shape 5
  (noise) a constant is written instead: AM 0x16cdf93e9e, FM 0, PM 0x346dc5d6388.
- FM deviation: `d = trunc((dev_uHz / 156250000000000.0) * 2^48)`; 0x3004 = `(d & 0x7fff) << 1`, 0x3008 = `(d >> 15) & 0xffff`,
  0x300c = `(d >> 31) & 0xffff`.
- PM deviation: `v = trunc(dev_mdeg * 0.001 * 2^64 / 360)`, word = `v / 65535` if `v >= 0xffff` else 1, split 16/16/16
  into 0x3004/0x3008/0x300c.
- Phase: see table. Amplitude and offset codes: section 5.

### 2.4 Helper sequences (each item is one 32-bit write; `B=1` means 0x3ff0 <- 1 etc.)

- Set frequency (f, noise): B=1, 0x3048, 0x304c, 0x3050, 0x3054, B=0.
- Set phase (p): B=1, 0x3040, B=0.
- Set amplitude (c): A=1, 0x3070 <- 0xffff - c, A=0. Set offset (c): A=1, 0x3078 <- 0xffff - c, A=0.
- Upload carrier (n, s16 samples): B=1, 0x3000 <- 0, 0x3030 <- 0, 0x3034 <- 0x1fff, n x 0x3038 <- sample, B=0.
  n = 8192.
- Upload modulation table (AM and PM noise: u16, PM: s16): same with 0x3030 <- 1, n = 4096.
  FM (s16 and u16): same but only n - 1 = 4095 samples.
- Output (en, fmnoise): B=1; if en && fmnoise: 0x3000 <- 0x80, 0x3000 <- 0x81; else 0x3000 <- en; B=0.
- Basic mode (no modulation): M=1, 0x3000 <- 0, 0x3014 <- 0x7fde, 0x3010 <- 0xffff, M=0.
- AM setting (f, depth, DSSC): B=1, 0x3000 <- 0, B=0; M=1, rate words, M=0; M=1, 0x3000 <- 0x82;
  M=1, M=1, 0x3010 <- 0xffff, M=0; M=1, M=1, 0x3014 <- 0, M=0, M=0 (depth/DSSC only go into the AM table).
- FM setting (f, dev): B=1, 0x3000 <- 0, B=0; M=1; M=1 rate words M=0; M=1, 0x3000 <- 0x84; M=1 deviation
  words M=0; M=1, M=1, 0x3010 <- 0, M=0; M=1, M=1, 0x3014 <- 0xffff, M=0, M=0.
- PM setting (f, dev): as FM with 0x3000 <- 0x88 and the PM deviation word.
- Init (wave, modulation wave, amplitude code, offset code, f): listed in full in section 9 (steps F1..F10).

## 3. Waveform tables

Wave types:

| type | wave | table (8192 samples, s16) | frequency cap |
|---|---|---|---|
| 0 | sine | `trunc(32767 * sinf(2*pi*i/8192))` (float angle and sine, double product) | 60 MHz |
| 1 | square | amplitude 32767, frequency, duty; rebuilt on every frequency change, so the edges depend on the frequency. OOS's edges: section 15 | 15 MHz |
| 2 | ramp | symmetry | 150 kHz under stock, 5 MHz in OOS (`models.md`) |
| 3 | pulse | width, duty, frequency, two edge times | 2 MHz under stock, 5 MHz in OOS |
| 4 | noise | uniform random in -32767..32767, fixed increment 2^38 | 60 MHz (limit irrelevant) |
| 5 | DC | constant; the level comes from the offset DAC | 60 MHz |
| 6 | arbitrary | samples scaled to +-32767 and resampled to 8192 points (section 16) | 10 MHz |

Minimum frequency 2000 uHz (2 mHz) for all types. OOS can upload any s16[8192] it computes itself; nothing in the
FPGA path depends on the table type except noise (fixed increment) and FM-noise (bit 7 of 0x3000).

Modulation (type 0 AM, 1 FM, 2 PM; shape 0 sine, 1 square, 2 triangle, 3 up-ramp, 4 down-ramp, 5 noise):
AM table = u16[4096], for sine without DSSC `(1 + m*sin(2*pi*i/4096)) * (m > 1 ? 1/(1+m) : 0.5) * 32767` with
m = depth / 100000; FM/PM tables are s16[4096]; noise shapes use a u16 table.

## 4. Analog path GPIOs

### 4.1 Devices

- `/dev/afg_gpio` (stock kernel module `afg_gpio`, misc device): `write` of one byte sets GPIO 122 + n to bit n (n = 0..4,
  afg_in1..5); `read` returns the five levels packed the same way. Module init: 122..125 output low; afg_in5 (126)
  stays an input until the first write.
- `/dev/fg_m2c_gpio` (stock kernel module `fg_m2c_gpio`): byte != 0 -> GPIO 149 high, 0 -> low; init low.

Under OOS both are plain GPIO lines (section 9).

### 4.2 Gear selection

A = amplitude, O = offset (units nV; see section 7). Thresholds are exact integers.

| condition | code (afg_in5..1) | gear | path gain |
|---|---|---|---|
| dc, \|O\| <= 2 V | 0x1b | 13 (offset only) | - |
| dc, \|O\| > 2 V | 0x1f | 14 | - |
| A > 6 V | 0x14 | 1 | 10 |
| 2 V < A <= 6 V | 0x15 | 0 | 3.16227766 |
| A <= 2 V, \|O\| <= 2 V, A <= 5 mV | 0x1b | 2 | 0.003162277 |
| ... 5 mV < A <= 16.5 mV | 0x1a | 3 | 0.01 |
| ... 16.5 mV < A <= 54 mV | 0x19 | 4 | 0.0316227766 |
| ... 54 mV < A <= 180 mV | 0x18 | 5 | 0.1 |
| ... 180 mV < A <= 590 mV | 0x11 | 6 | 0.316227766 |
| ... 590 mV < A <= 2 V | 0x10 | 7 | 1.0 |
| A <= 2 V, \|O\| > 2 V, A <= 54 mV | 0x1f | 9 | 0.0316227766 |
| ... 54 mV < A <= 180 mV | 0x1e | 10 | 0.1 |
| ... 180 mV < A <= 590 mV | 0x1d | 11 | 0.316227766 |
| ... 590 mV < A <= 2 V | 0x1c | 12 | 1.0 |

The gains decode the bits (assumed, but every row is consistent): bit 0 (afg_in1) = -10 dB (x0.316), bit 1 (afg_in2) =
-20 dB, bit 2 (afg_in3) = x10 stage, bit 3 (afg_in4) = -20 dB, bit 4 (afg_in5) = always 1 (supply/enable of the path).
The x10 stage sits after the offset injection: with bit 2 set the offset DAC is scaled by 0.11 instead of 1.1
(section 5) and the waveform is pre-attenuated by bit 3. Record 8 of the cal file (gear 8, A <= 16.5 mV with high
offset) is never used: that case runs on gear 9.

Write sequence: read the current levels; if different from the new code: write `code & 0x0f`, sleep 3 ms, write
`code` (bit 4 last). A running unit under stock shows afg_in1/3/5 high, 2/4 low = **0x15**, FG_M2C low: the init code
for a 6 V default amplitude with the output off (verified on the unit).

## 5. Amplitude / offset -> DAC codes

### 5.1 Gears for the calibration

Three gears, a flag and the path gain follow from A and O: amplitude gear g1, flatness gear g2, offset gear g3,
high-offset flag h, path gain G. Same thresholds as 4.2:

| case | (g1, g2, g3) | h |
|---|---|---|
| A > 6 V | (1, 1, 1) | 1 |
| 2 V < A <= 6 V | (0, 0, 0) | 1 |
| A <= 2 V, \|O\| > 2 V | (9..12, 0, 9..12) as in 4.2 | 1 |
| A <= 2 V, \|O\| <= 2 V | (2..7, 2, 2..7) as in 4.2 | 0 |
| DC | g3 = 13 (\|O\| <= 2 V) or 14; g1/g2 keep their previous values | \|O\| > 2 V |

### 5.2 Calibration and codes

1. Gears as in 5.1 (the DC row for DC).
2. Amplitude record r = 16 + g1 (skipped for noise): first entry i with `lo[i] <= A <= hi[i]` -> `A' = A*k[i] + b[i]`.
3. Flatness, f = stored frequency: if f < 100 kHz record r = 30 + g2, match `first[i] >= f >= second[i]`
   (these records store the upper bound first); else record r = 34 + g2, match `first[i] <= f <= second[i]`.
   `A'' = A' / (f*k[i] + b[i])`. No match (e.g. above 25 MHz) -> no correction.
4. Offset record r = g3: search i over `0 .. n(record 0) - 1` (**the count of record 0**, not of record g3) for
   `lo[i] <= O <= hi[i]`; if `i < n(record g3)`: `O' = O*k[i] + b[i]`. (With an unmatched search i ends at n(record 0)
   and that entry is used if it exists. Reproduce this as is.)
5. Codes (double, truncated toward zero):
   - `ampl_code   = trunc(((A'' * 1e-9 / G) / 1.158 + 2.75) / 5.29 * 65535)`
   - `offset_code = trunc((O' * 1e-9 * (h ? 0.11 : 1.1) + 2.75) / 5.29 * 65535)`

   Registers: 0x3070 <- `0xffff - ampl_code`, 0x3078 <- `0xffff - offset_code` (32-bit subtraction, no clamp).
   Physically: two 16-bit DAC words with 5.29 V full scale and 2.75 V bias (assumed).

Without calibration the same formulas apply with the correction steps left out.

Frequency correction: for f >= 100 kHz, `f' = f*k + b` with header k, b (f32 at payload +0x10/+0x14). OOS does not apply
it. In the cal file used here k = 1.0, b = 0.0.

## 6. `cal_afg.hex`

Container as in `self-cal.md` 7.1 (both CRCs verified on the unit's file, payload 0x17858 = 96344 bytes).
Loaded from `/rigol/data/cal_afg.hex`, fallback `/rigol/data/default/...`. Written only by the factory procedure.

Payload layout:

| off | type | content |
|---|---|---|
| 0x00 | 16 bytes | zero in the file seen (not used) |
| 0x10 | f32 | frequency k (1.0) |
| 0x14 | f32 | frequency b (0.0) |
| 0x18 + r*0x968 | record r, r = 0..39 | |

Record (0x968 = 2408 bytes): `+0x000 s32 n`, `+0x004 s32 pad`, `+0x008 s64 lo[100]`, `+0x328 s64 hi[100]`,
`+0x648 f32 k[100]`, `+0x7d8 f32 b[100]`. Units of lo/hi: nV for amplitude/offset records, uHz for flatness records.

| records | use | typical content |
|---|---|---|
| 0..14 | offset correction, gear g3 = r | ranges +-2 V (gears 2..7, 13) or +-10 V (0, 1, 8..12, 14); k about 0.92..0.98, b about -20 mV (low gears) / -190 mV (high gears) |
| 15 | unused | n = 0 |
| 16..28 | amplitude correction, gear g1 = r - 16 | e.g. record 23 (gear 7): three ranges, 591 mV..1 V, 1..1.35 V, 1.35..2 V |
| 29 | unused | n = 0 |
| 30..32 | flatness < 100 kHz, gear g2 = r - 30 | 12-13 bands, b 0.993..1.001, k about -1e-13 per uHz |
| 33 | unused | |
| 34..36 | flatness >= 100 kHz, gear g2 = r - 34 | 16-26 bands up to 25 MHz, b = 1.0 except the top bands |
| 37..39 | unused | |

Factory procedure (for reference only): a 100 kHz sine at each gear's reference amplitude (or DC) goes to a DMM, the
operator enters the reading, and k/b follow from it. Set points 100 kHz / 2..10 V.

## 7. Generator state and units

| parameter | default |
|---|---|
| output on | off |
| basic wave type | 0 sine |
| sysmode (0 basic, 1 modulation) | 0 |
| square duty / pulse duty (1/1000 %) | 50000 / 50000 |
| pulse width, edge times, period | |
| frequency (uHz) | 1e9 (1 kHz) |
| amplitude (nV) | 6e9 |
| offset (nV) | 0 |
| phase (mdeg) / ramp symmetry (1/1000 %) | 0 / 50000 |
| modulation type | 0 AM |
| AM DSSC / depth (1/1000 %) / rate (uHz) / shape | 0 / 100000 / 1e8 / 0 |
| FM deviation / rate / shape | 1e9 / 1e8 / 0 |
| PM deviation (mdeg) / rate / shape | 90000 / 1e8 / 0 |

Units: frequencies uHz, amplitude and offset nV. The amplitude is peak-to-peak into high impedance (section 14).

Limits, required for a safe output: amplitude 2 mV .. 10 V (f <= 10 MHz), 5 V (10..30 MHz), 2.5 V (above 30 MHz);
offset limited to `+-(Vmax - A)/2` with Vmax = 10 V (f <= 10 MHz) else 5 V; DC offset +-5 V.

## 8. Driver flows

- Init (reset: defaults first): build sine table and AM table -> relays for (A, O, no DC) -> calibration ->
  FPGA init (section 9) -> output flag = 0 -> supply off -> output off.
- Output on: afg_gpio <- 0x00, supply on, relays for (A, O, type == DC), output on. Off: supply off, output off (relays
  stay). fmnoise = (FM shape == 5 && modulation type == FM && sysmode == 1).
- Wave type t: for DC or when leaving DC: relays, calibration, amplitude and offset words; store t; set frequency;
  build table t; upload 8192 samples; if the output is on, output on again.
- Frequency f: store; square -> rebuild and upload table; set frequency (noise flag = t == noise); if the output is on,
  output on again.
- Amplitude A: store; relays for (A, O, no DC); calibration; rewrite 0x3070. Rewrite 0x3078 too when the offset gear
  changes.
- Offset O: store; relays for (A, O, t == DC); calibration; rewrite 0x3078.
- Phase, square duty, ramp symmetry, pulse duty: store, rebuild/upload the table when that type is active, re-enable
  the output if on.
- System mode m or modulation type: m = 0 -> basic mode sequence; else the AM/FM/PM setting, build and upload the
  modulation table, write the rate again; re-enable the output if on. AM depth/DSSC/shape changes only rebuild the AM
  table; AM/FM/PM rate and deviation setters write their words directly.
- Every RAM upload and every modulation setting writes B 0x3000 <- 0, i.e. the output is off until the following
  output-on sequence (glitch on every table change).

## 9. Minimal sequence: 1 kHz, 1 Vpp, 0 V offset, sine (running unit, after the scope cold boot)

Values computed from the formulas above with an example `cal_afg.hex`: gear 7 (code 0x10, G = 1.0, h = 0);
amplitude record 23 entry 0 -> A' = 973.27 mV after flatness record 32 entry 10 (factor 0.99992); offset record 7
entry 0 -> O' = -23.897 mV. `ampl_code` = 44480 -> **0x3070 <- 0x523f**; `offset_code` = 33742 -> **0x3078 <- 0x7c31**
(uncalibrated values would be 0x5121 / 0x7aeb). Phase increment for 1 kHz = 28823037615 = 0x6_b5fc_a6af.
This is the init of section 8 with A = 1e9, O = 0, f = 1e9, followed by output on.

GPIO preparation (OOS DT/libgpiod): GPIO 122..126 outputs low, GPIO 149 output low.

| step | op | value | part |
|---|---|---|---|
| G1 | afg_gpio byte | 0x00 (if the current levels differ from 0x10), 3 ms, then 0x10 | relays (init) |
| F1 | 0x3ff4=1; 0x3000=0; 0x3014=0x7fde; 0x3010=0xffff; 0x3ff4=0 | | FPGA init |
| F2 | 0x3ff0=1; 0x3048=0xa6af; 0x304c=0xb5fc; 0x3050=0x0006; 0x3054=0x0000; 0x3ff0=0 | 1 kHz | |
| F3 | 0x3ff0=1; 0x3040=0; 0x3ff0=0 | phase 0 | |
| F4 | 0x3ff0=1; 0x3000=0; 0x3030=0; 0x3034=0x1fff; 8192 x 0x3038 = sine[i] sign-extended; 0x3ff0=0 | carrier RAM | |
| F5 | 0x3ff0=1; 0x3000=0; 0x3030=1; 0x3034=0x1fff; 4096 x 0x3038 = AM table (u16); 0x3ff0=0 | mod RAM, content irrelevant in basic mode (assumed) | |
| F6 | 0x3ff4=1; 0x3010=0; 0x3ff4=0; 0x3ff4=1; 0x3014=0xffff; 0x3ff4=0 | | |
| F7 | 0x3ff8=1; 0x306c=0xffff; 0x3074=0xffff; 0x3ff8=0 | | |
| F8 | 0x3ff8=1; 0x3070=0x523f; 0x3ff8=0 | amplitude | |
| F9 | 0x3ff8=1; 0x3078=0x7c31; 0x3ff8=0 | offset | |
| F10 | 0x3ff0=1; 0x3000=0; 0x3ff0=0 | end of FPGA init | |
| P0 | fg_m2c byte 0; 0x3ff0=1; 0x3000=0; 0x3ff0=0 | output off (end of init) | |
| (S) | 0x3ff4=1; 0x3000=0; 0x3014=0x7fde; 0x3010=0xffff; 0x3ff4=0 | optional: basic mode sequence, in place of the F6 values (which of the two is needed is not known) | |
| O1 | afg_gpio byte 0x00 | | output on |
| O2 | fg_m2c byte 1 (GPIO 149 high) | generator supply | |
| O3 | afg_gpio byte 0x00 (code & 0xf), 3 ms, 0x10 | relays | |
| O4 | 0x3ff0=1; 0x3000=1; 0x3ff0=0 | output on | |

No other register, GPIO or SPI write is involved; the AFG has no SPI device. The sequence needs no delay between O2 and
O4; OOS waits 5 ms there as supply settle margin.

Off: fg_m2c byte 0; 0x3ff0=1; 0x3000=0; 0x3ff0=0. Changing only the frequency: F2 (+ O4 again if a table was uploaded).
Changing only the amplitude: G1-style relay update + F8 (and F9 if the gear changed).

## 11. Risks and never-do

- Writing `cal_afg.hex`, deleting it or running the factory AFG calibration changes `/rigol/data`: never do it. OOS only
  reads `cal_afg.hex`. Nothing in the AFG path touches SPI flash.
- The GI/GO BNC can deliver up to 10 Vpp (+offset) within the limits of section 7: only enable with nothing sensitive
  connected (or into a scope channel at a safe V/div). Clamp A and O to the limits of section 7.
- 0x3070/0x3078 are `0xffff - code` without clamp: out-of-range A/O wrap to a wrong (possibly maximal) level. Clamp
  codes to 0..0xffff before writing.
- Always end each block with the bank select at 0: a select left at 1 makes later writes to 0x3000..0x307c land in the
  wrong block (e.g. 0x3000 of bank B vs M).
- Relay switching while the output is on produces glitches. OOS can turn the output off around gear changes. Keep the
  break-before-make order (bits 0..3, 3 ms, bit 4).
- Every table upload disables the output (B 0x3000 = 0); re-enable afterwards.
- Do not read the 0x3000 block: a read of the data port 0x3038 may have side effects (assumed).

## 13. OOS shape

`hal::afg`, in the style of `afe.rs`/`scu.rs`: pure functions return `Vec<Op>` with `Op::Bar1 { off, val }`,
`Op::Gpio(u8)` (afg_in1..5), `Op::Power(bool)` (FG_M2C), `Op::SleepMs(u32)`. `Cal` is parsed from `cal_afg.hex`
(section 6); `gear` (4.2/5.1), `codes` (5.2, including the record-0 count), `clamp` (section 7), `set_freq`,
`set_phase`, `upload`, `output`, `relays`, `start`, `stop`, `idle`. Test vectors: the section 9 values.

`afg::start` writes in this order: relays, basic mode sequence, frequency, phase, carrier upload, 0x306c/0x3074, DAC
codes, modulation (if any), supply on, 5 ms, output on.

## 14. First output on the unit

Verified on the unit. Cable GI/GO -> CH1 (1 MOhm, no terminator), factory `cal_afg.hex`, `hal::afg` through
`oos-fpgad afg sine` and oos-ui (Analysers > Generator, SCPI `:SOURce`). Scope readings (mean of the screen, Auto BW):

| Set | Gear | Scope | Note |
|---|---|---|---|
| 1 kHz, 10 mVpp | 0x1a | 15 mVpp, 4.1 mVrms | noise-limited at 2 mV/div |
| 1 kHz, 100 mVpp | 0x18 | 107 mVpp, 35.5 mVrms | |
| 1 kHz, 400 mVpp | 0x11 | 400 mVpp | |
| 1 kHz, 1 Vpp | 0x10 | 0.987 Vpp at 200 mV/div; 0.3541 Vrms at 500 mV/div (ideal 0.3536) | 999.98 Hz, mean -0.06 mV |
| 1 kHz, 1.5 Vpp, +0.5 V | 0x10 | 1.524 Vpp, mean 0.499 V | |
| 1 kHz, 4 Vpp | 0x15 | 4.054 Vpp, 1.4165 Vrms | |
| 1 kHz, 1 Vpp, +3 V | 0x1c | 0.996 Vpp, mean 3.020 V | high-offset gear |
| 1 kHz, 8 Vpp | 0x14 | 8.089 Vpp, 2.836 Vrms (ideal 2.828) | |
| 100 kHz / 1 MHz / 10 MHz, 1 Vpp | 0x10 | 1.02 / 0.99 / 1.07 Vpp | |

The amplitude is right within 0.3 % where the scope range is good; the -2 % at 100/200 mV/div is CH1's factory
vertical calibration (the same signal reads 0.3457 Vrms at 200 mV/div and 0.3541 at 500 mV/div). The gears of this table
and the offset path work as described in sections 4-5; the record-0 count and the 0x523f / 0x7c31 codes of section 9
match the hardware. Response with the built-in Bode (CH1 alone, gain against the set amplitude, 100 Hz..20 MHz, 25 points):
within +-0.2 dB to 2.6 MHz, then +0.3 dB at 7.2 MHz, +0.9 dB at 12 MHz, +1.3 dB at 20 MHz (unterminated cable; the
direct reading at 12 MHz agrees). The roll-off above 25 MHz is not yet measured.

oos-ui owns the AFG GPIO lines from its first generator request on (at start: "off", which also switches off an
output left running), so `oos-fpgad afg` then fails with EBUSY instead of fighting over the lines.

Open observation: with the output switched off (relays 0x00) and the cable still on CH1, CH1 shows a recurring
disturbance of about +-10 mV at ~5 MHz (the edge trigger at 0 V fires on it, so every frame shows it right after the
trigger point); with the output on, a few % of 100 ns/div frames carry ~80 ns bursts of ~89 MHz at about 40 mV. The open
CH2 input stays quiet (+-30 codes) at the same time. With the cable removed (output off, CH1 at 10 mV/div, 100 ns, 1 us
and 10 us/div, several captures) CH1 is as quiet as the other inputs (+-2 codes of the 8-bit read-out, 0.07 mV rms): the
disturbance comes in through the generator output and the cable. Under stock the relays keep the code of the last
amplitude (e.g. 0x15, afg_in5 high) with the supply off, while all relays low was the state tested here. oos-ui's off
state is now `afg::idle`: output and supply off, relays on the gear of the set levels (0x15 for 6 V). This is
host-tested; its effect with the cable connected is not yet verified on hardware. `oos-fpgad afg off` still only
switches output and supply off.

Changing a running output: a full start writes all tables, about 0.9 s with a gap in the signal. `afg::retune` writes only the
frequency and the two DAC words when the gear and the wave table stay the same and there is no modulation. A change of gear
(the relays switch), a square or ramp whose table depends on the frequency, and noise take the full start. Verified on the
unit with sine offset steps; amplitude steps across a gear still take 0.9 s.

Open observation: above 2 Vpp (gears 0x15, 0x14) and for DC above 2 V (gear 0x1f) the output is sometimes 80 to 120 mV low, on
two channels on one cable alike, independent of the amplitude before. In the same start it read +3 to -17 mV soon after power-on
and -79 to -105 mV twelve minutes and a test run later. Up to 2 Vpp the two states do not differ. The cause is not known:
temperature and an earlier generator operation are both open.

## 15. Other wave shapes in OOS

Host-tested, not yet verified on hardware.

`hal::afg::Wave` builds OOS's own tables (section 3: the FPGA only cares about noise): square (duty 1..99 %) and pulse
(duty, rise/fall times) as trapezoids whose edges span at least one DAC clock (8192 * f / 156.25 MHz table samples,
so a 10 MHz square has ~524-sample edges instead of a 1-clock jump that would move by one clock from period to
period), ramp with symmetry, noise from a fixed-seed xorshift (uniform -32767..32767, fixed increment 2^38, no
amplitude correction, see 5.2), DC as a zero table on the offset-only gears 0x1b / 0x1f. Frequency caps per shape as in
section 3 (`Wave::max_hz`). `afg::start` covers every shape; `oos-fpgad afg square|ramp|pulse|noise|dc ...` drives them.
Edge shapes and the flatness of non-sine shapes against the factory flatness table (measured with sines) are not yet
verified on hardware.

Modulation (verified on the unit 2026-11-07: AM depth, FM deviation 1 to 20 kHz and PM 20 to 180 degrees within 5 %, sine shape): `afg::Mod` (AM depth, FM deviation in Hz, PM deviation in degrees; rate; shape
sine/square/triangle/up/down ramp) and `afg::modulate` write the AM/FM/PM sequences of section 2.4 word for word, upload
the 4096-entry modulation RAM (FM 4095 entries) and write the rate again, before the output goes on (`start_mod`). The AM
table uses the sine formula of section 3 with the other shapes in place of the sine (assumed for those shapes); the FM/PM
table holds the shape at full scale in s16.
UI: Generator page Modulation / Mod. rate / Depth or deviation / Mod. shape; SCPI `:SOURce:MOD...` (Rigol DG style);
`oos-fpgad afg ... --mod am|fm|pm`.

## 15a. Noise modulation

Host only, not yet verified on hardware.

`ModShape::Noise` (shape 5): a fixed pseudo-random table instead of the shape, the constant rate words of section 2.3
(AM 0x16cdf93e9e, FM 0, PM 0x346dc5d6388) instead of the Rate setting, the FM output enable with bit 7 (2.4 Output:
0x3000 <- 0x80, 0x81). Tables: the FM noise table is offset binary around 32768, bell-shaped, sigma about 9900; OOS
generates its own with these statistics. PM uses a unipolar noise table over the s16 upload. In the Generator page,
SCPI `:MOD:<type>:INTernal:FUNCtion NOISe`.

## 16. Arbitrary waves

Host only, not yet verified on hardware.

Nothing in the FPGA depends on the table's content, so OOS uploads its own. `hal::afg::arb_table` resamples any sample
list linearly to 8192 points and scales it to +-32767, so the Amplitude setting is the peak-to-peak output whatever the
source's level. `arb_set` keeps the table for the acquisition thread and returns an id; `Wave::Arb { id }` carries it,
so a new table is a new request and a running output changes at once. Frequency cap 10 MHz (`afg.arb_max_hz` in
limits.conf).

Sources: Generator > Wave = Arb, "Capture the screen" (all raw samples of the screen of a channel = one period, also
saved as `arb.csv` in the storage directory), "Load arb.csv" (one value per line, or any CSV: the last number of each
line counts, so OOS's own wave CSV loads; `:` also separates), SCPI `:SOURce:ARB:DATA <v>,<v>,...`
and `:SOURce:ARB:CAPTure CHAN<n>`. The table is not part of a setup.

A sharp edge in the table is limited by the DDS: one sample per 6.4 ns, and table samples are skipped above
156.25 MHz / 8192 = 19 kHz, so no anti-aliasing is applied to the table (assumed to show as images in the spectrum).
