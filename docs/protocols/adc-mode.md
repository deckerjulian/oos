# ADC channel mode: what changes when the set of enabled channels changes

The code is in `hal::dho::adc` (mode writes, `cal_adc.hex`) and `hal::dho::seq` (when they run).
Status: the register values below are consistent with live values on the unit for mode 0 (marked in the text). A
complete mode switch is not yet verified on hardware against a known signal (5.4).

## 0. Summary

- A channel on/off does not write the ADC directly. It sets the channel flag and re-applies the AFE scale and offset. On
  the next RUN/SINGLE the channel mask is compared with the last one. If it changed, the ADC mode is switched.
- A mode switch = ADC input mode (2 SPI words, ADC regs 0x54/0x55) + the calibration of the mode = 1 reference word (ADC
  reg 0x09), the SPU fine-gain register **0x1058**, 8 core gains (ADC regs 0x15 + 8n), 8 core offsets (0x14 + 8n), 4
  sample-clock delays (regs 0x04/0x05) and a clock delay (reg 0x01). That is **24 writes to 0x1034 and one to 0x1058**,
  all values from `cal_adc.hex`.
- The calibration format is laid out for two ADC chips with 8 cores each. The DHO900 has one. Everything for "ADC 1"
  never reaches the SPI bus. It only changes the kept register values.
- **No DSP filter is set on a mode change**. OOS never writes 0x1098.
- The cal file holds one 0x68-byte record per ADC mode (offsets) plus per-channel core gains (records 0-3) and a
  per-channel 2-bit fine-gain block. For 2-channel modes the even cores take the gains of the lower channel and the
  odd cores those of the higher one. In 4-channel mode core n takes channel (n mod 4).
- After the mode change the rest of the acquisition set-up follows as in `horizontal-config.md` (ratio-dependent
  0x1004 lanes, 0x1080/0x1084 sample delays, rate divider, depth...). The per-channel digital gain **0x105c** is rewritten
  in the layout of the new mode (section 4.3).

## 1. Call path of a channel-mode change

```
channel on/off
  store the flag, re-apply AFE scale and offset (afe-settings.md 3.8)
  re-arm; the registers follow on the next acquisition set-up
acquisition set-up: ADC mode -> acquire block -> horizontal -> read-path gain (WPU) -> ...
  ADC mode step
    mask = enabled channels, bit n = CHn+1 on
    if mask == stored mask: return
    store mask
    if mask in 1..15:
      input mode (2.1), then calibration of the mode (2.2)
```

Mask -> ADC mode, ratio from the series table (`horizontal-config.md` 1.1):

| mask | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | A | B | C | D | E | F |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ADC mode | 0 | 1 | 4 | 2 | 5 | 7 | 10 | 3 | 6 | 8 | 10 | 9 | 10 | 10 | 10 |
| ratio | 1 | 1 | 2 | 1 | 2 | 2 | 4 | 1 | 2 | 2 | 4 | 2 | 4 | 4 | 4 |

Mode names: 0-3 = CH1..CH4 alone, 4 = CH1+CH2, 5 = CH1+CH3, 6 = CH1+CH4, 7 = CH2+CH3, 8 = CH2+CH4, 9 = CH3+CH4,
10 = all four (also used for three channels).

Notes:
- Mask 0 (no channel) stores 0 and writes nothing.
- The stored mask is assumed to start at 0, so the first RUN applies mode 0 again. This is harmless. Write the mode once at
  start.
- Only the channel mask counts.
- No lane re-alignment, sync or SPU reset follows a mode change: the lane delays from boot (`adc-spu-init.md`) stay.
- The self-calibration also switches modes (`self-cal.md`).

At boot, after the calibration files are loaded, apply the calibration of mode 0 without the input-mode words: the ADC
init list already contains mode 0, `0x01549460/0x01550004`. This comes before the lane calibration.

## 2. Register writes per mode

All ADC accesses go through FPGA 0x1034: `0x01000000 | reg << 16 | data` (bit 24 start, bits 22:16 register,
15:0 data). Reads set bit 23 and return the data in 0x1038. OOS does not poll for completion, it uses the delays below.

### 2.1 Input mode

Two input-select words sel1 and sel2 per mode (`SWONP` in `hal::dho::adc`), then:
sleep 100 us, **0x1034 <- 0x01540000 | (sel1 ^ 0x1041)**, sleep 100 us,
**0x1034 <- 0x01550000 | (sel2 ^ 0x0004)**, sleep 100 us. (0x1041 and 0x0004 are the XOR masks of regs 0x54 and 0x55.)

| mode | channels | sel1 | sel2 | word reg 0x54 | word reg 0x55 |
|---|---|---|---|---|---|
| 0 | CH1 | 0x8421 | 0 | 0x01549460 | 0x01550004 |
| 1 | CH2 | 0x0842 | 1 | 0x01541803 | 0x01550005 |
| 2 | CH3 | 0x2108 | 4 | 0x01543149 | 0x01550000 |
| 3 | CH4 | 0x1084 | 2 | 0x015400c5 | 0x01550006 |
| 4 | CH1+CH2 | 0x0821 | 1 | 0x01541860 | 0x01550005 |
| 5 | CH1+CH3 | 0x2021 | 4 | 0x01543060 | 0x01550000 |
| 6 | CH1+CH4 | 0x1021 | 2 | 0x01540060 | 0x01550006 |
| 7 | CH2+CH3 | 0x2042 | 4 | 0x01543003 | 0x01550000 |
| 8 | CH2+CH4 | 0x1042 | 2 | 0x01540003 | 0x01550006 |
| 9 | CH3+CH4 | 0x1108 | 2 | 0x01540149 | 0x01550006 |
| 10 | 4 channels | 0x1041 | 4 | 0x01540000 | 0x01550000 |

Assumed: sel1 is made of 5-bit groups with one bit per input (CH1 bit 0, CH2 bit 1, CH4 bit 2, CH3 bit 3 of a group;
0x8421 = CH1 in every group). sel2 takes the values 0, 1, 2, 4.
These are the same words as in `horizontal-config.md` 2.3.

**Interaction with the band-limit path**: a band-limit change first writes reg 0x54 <- 0x4210 and reg 0x55 <- 0x0008
(words 0x01545251, 0x0155000c). At the end the two words of the table are written again from the current sel1 and sel2
(sleep 100, 0x54 word, sleep 100, 0x55 word, sleep 100). So after a band-limit change the ADC is back in the current
input mode. `hal::dho::afe::configure` does this.

### 2.2 Calibration of the mode

Order of execution (cal offsets = byte offsets in the cal_adc payload, layout in section 3; sh = the kept value of the
register):

| # | step | register / word | value |
|---|---|---|---|
| 1 | reference, ADC 0, v = cal[0x48] | 0x1034 <- `0x01090000 \| (sh09 ^ 0x2720)`, sh09 = (sh09 & 0xffc0) \| (v & 0x3f), sleep 100 before/after | v = 32 gives 0x01097700 |
| 2 | reference, ADC 1, v = cal[0x4a] | no write; sh09 bits 5:0 = v | |
| 3 | SPU fine gain | **0x1058** <- sum over k = 0..15 of (buf[k] & 3) << 2k, no delay | section 3.3 |
| 4-19 | core gains, 16 steps (8 per ADC), order below | ADC 0 only: 0x1034 <- `0x01000000 \| (0x15 + 8*core) << 16 \| (shG[core] & 0xff80) \| (g & 0x7f)`, sleep 80 us after; g outside -64..63: no write | section 3.2 |
| 20-35 | core offsets, ADC 0 cores 0..7, then ADC 1 cores 0..7 | ADC 0 only: 0x1034 <- `0x01000000 \| (0x14 + 8*core) << 16 \| (shO[core] & 0xf800) \| (o & 0x7ff)`, sleep 100 before/after; o outside -1024..1023: no write | |
| 36-39 | sample-clock delays, i = 0..3 | i = 0/1: reg 0x04 low/high byte, i = 2/3: reg 0x05 low/high byte; word data = sh ^ 0x8080; sleep 100 after | usually all 0 |
| 40 | clock delay (a, b) | 0x1034 <- `0x01010000 \| (sh01 & 0xe000) \| (a & 7) << 10 \| (b & 0x3ff)`, sleep 100 after | usually 0x01010000 |

Core gain order: modes 0-3 and 10: core pairs (0,4), (1,5), (2,6), (3,7); modes 4-9: (0,4), (2,6), (1,5), (3,7);
within a pair ADC 0 core n, ADC 0 core n+4, ADC 1 core n, ADC 1 core n+4. The register of core n is 0x15 + 8n (gain)
and 0x14 + 8n (offset): cores 0..7 -> gain regs 0x15, 0x1d, 0x25, 0x2d, 0x35, 0x3d, 0x45, 0x4d.

One kept value per core serves "ADC 0" and "ADC 1", so after the mode step the gain and offset values hold the ADC-1
fields (0 in normal files). This has no effect, because only the masked upper bits of the kept value reach the next
word.
Start values: gain 0x0002 per core, offset 0, reg 0x04 = 0x8000, reg 0x05 = 0, reg 0x01 = 0, reg 0x09 = 0x5020.

Verified on the unit: after start in mode 0, 0x1058 holds the mode-0 fine word, the low half of reg 0x09 is 0x5020
(reference 32) and regs 0x04/0x05/0x01 are 0.

## 3. cal_adc.hex

Container as all `/rigol/data/cal_*.hex` (`self-cal.md` 7.1): 20-byte header part, u32 payload length, u32 crc32(payload),
payload. The file holds exactly 1848 (0x738) payload bytes. Location order: `/oos/cal`, `/rigol/data`,
`/rigol/data/default` (the cold boot reads the last two only, `self-cal.md` 14.10).

### 3.1 Layout (offsets in the payload, little-endian)

| offset | type | content | used by |
|---|---|---|---|
| m*0x68 + 0x00, m = 0..10 | s16[8] | core offset, ADC 0 cores 0..7, for ADC mode m | core offsets in mode m; offset calibration of mode m writes it |
| m*0x68 + 0x10 | s16[8] | same for ADC 1 (normally 0) | kept value only |
| c*0x68 + 0x20, c = 0..3 | s16[8] | core gain, ADC 0 cores 0..7, for channel CHc+1 | core gains (mapping 3.2); gain calibration of channel c writes it |
| c*0x68 + 0x30 | s16[8] | same for ADC 1 (0) | kept value only |
| m*0x68 + 0x40, m = 0..3 | s16[4] | sample-clock delays for single-channel mode m; modes 4..10 use m = 0 | regs 0x04/0x05 |
| 0x1e0 / 0x248 / 0x2b0 / 0x318 (+0x40 of records 4..7) | s16 b, s16 a | clock delay (b, a) for modes 0 / 1 / 2 / 3; modes 4..10 use 0x1e0 | reg 0x01 |
| 0x48, 0x4a | s16 | ADC reference, "ADC 0" / "ADC 1" (only 0x48 reaches the chip) | reg 0x09; the ADC calibration writes it |
| 0x478 + 64*c, c = 0..3 | i32[16] | SPU fine gain of channel CHc+1, 2 bits per core slot (slots 8..15 = second ADC, 0 here) | 0x1058; gain calibration of channel c writes it |
| 0x578..0x737 | | zero, unused | |

Typical content: reference 32 / 32, all clock and sample-clock delays 0, core gains within about -16..15, core offsets
within about -74..52.

### 3.2 Which gain goes to which core

| ADC mode | core n gain from |
|---|---|
| 0..3 (one channel c) | record c, gain[n] |
| 4..9 (pair lo, hi) | even n: record lo, gain[n]; odd n: record hi, gain[n] |
| 10 (four channels) | record (n mod 4), gain[n] |

Offsets always come from the mode's own record. Exception: in mode 9 (CH3+CH4) cores 3 and 7 take their gain from
payload offsets 0x1c6/0x1ce (record 4 + 0x26/0x2e, normally 0) instead of CH4's 0x15e/0x166. Stored mode-9 offsets in
existing files are assumed to be measured with this mapping, so OOS keeps it (`hal::dho::adc`).

So on this chip core n samples: one-channel mode: every core the same input; two channels: even cores the lower
channel, odd cores the higher; four channels: core n = CH(n mod 4)+1 (assumed, follows from the gain mapping).

### 3.3 SPU fine gain 0x1058

`buf` (16 x i32) is built from the fine-gain blocks: modes 0..9 with channel pair (i, j) =
(0,0) (1,1) (2,2) (3,3) (0,1) (0,2) (0,3) (1,2) (1,3) (2,3): buf[0..7] = block i [0..7], buf[8..15] = block j [8..15];
mode 10: buf[0..3] = CH1[0..3], buf[4..7] = CH2[4..7], buf[8..11] = CH3[8..11], buf[12..15] = CH4[12..15].
0x1058 = OR of (buf[k] & 3) << 2k. Since slots 8..15 are 0 on a one-ADC unit, the second channel of a pair never
contributes. Example: codes 2 2 2 0 3 2 2 2 give 0xab2a.

### 3.4 What produces cal_adc.hex (self-calibration), and what OOS must replay

The ADC gain and offset calibration is in `self-cal.md` 14 and 15. Normal operation only replays the stored values
(section 2.2). The delay fields come from an ADC phase calibration OOS does not run.

Runtime ADC calibrations that are not in the file and that OOS runs at boot (`cold-boot.md` 3.3 to 3.6):
1. sync delay sweep, lane eye scan, lane delay load (`adc-spu-init.md`);
2. ADC core alignment (`cold-boot.md` 3.5);
3. ADC-internal trim, for core n = 0..7:
   trim on = 0x1034 <- `0x01000000 | (0x13 + 8n) << 16 | ((shQ[n] & ~0x4000) | 1 << 14) ^ 0x1b9b`
   (shQ starts at 0x1b9b, so the word data is 0x4000; sleep 200 us before, 800 us after); then per core poll:
   0x1034 <- `0x01800000 | (0x16 + 8n) << 16` (read of ADC reg 0x16 + 8n), sleep 100 us, read 0x1038, done = bit 6;
   when done, trim off (word data 0x0000); timeout 2 s. There is no ADC 1, so OOS runs this for ADC 0 only.

### 3.5 cal.bin is not part of this path

OOS does not read `/rigol/data/cal.bin`.

## 4. DSP filter and the other mode-dependent SPU registers

### 4.1 No filter set-up

Do not touch 0x1098/0x109c: the FPGA keeps its power-on filter state.
0x10a0 <- 16 is still written in the horizontal block (`horizontal-config.md` 4.1 step 47).

### 4.2 Filter registers

Bypass: 0x1098 bits 1:0 = 1, 0x10a0 <- 16. Coefficient set: 0x1098 bits 1:0 = 2, 0x10a0 <- 16, then per tap
0x109c <- (coef & 0x7fffff) | tap << 23 | phase << 30 (phases 0..3 for ratio 1; the coefficient layout differs for ratio
1, 2 and 4). The filter is enabled when 0x1098 bit 0 is 0. OOS uses none of this.

### 4.3 Digital gain 0x105c follows the mode

Per enabled channel: gain 2 if the digital gain of the channel's calibration record is >= 2, else 1 (`afe-settings.md`
4.1). The DHO900 has one ADC, so 0x105c uses a 16-bit layout of 8 fields of 2 bits (1 = x1, 2 = x2):

| ratio | channel -> fields |
|---|---|
| 1 | all 8 fields = g of the one channel (whole value rewritten) |
| 2 | lower channel of the pair: fields 0-3 (low byte), higher channel: fields 4-7 (high byte) |
| 4 | CH1: fields 0, 2; CH2: 1, 3; CH3: 4, 6; CH4: 5, 7 |

All channels x1 gives 0x5555 in every mode (verified on the unit). The SPU init writes 0x55555555 (`adc-spu-init.md`);
the next read-path set-up overwrites it with 0x00005555.

### 4.4 Acquire block and arm

Ratio-dependent writes after the mode change are in `horizontal-config.md` 4.1/4.2 (0x1004 lane fields, 0x1080/0x1084
sample delays, 0x10d0/0x10d8/0x10d4, 0x1148, 0x10e4, 0x4050/0x4054, 0x10e0, read-out window). The arm writes the new
mask to 0x4008[3:0] (`acquisition-start.md`).

## 5. OOS implementation

### 5.1 State to keep

ADC register values: sel1/sel2, reg 0x09, reg 0x04, reg 0x05, reg 0x01, gain[8], offset[8] (start values of 2.2), the
current ADC mode (or "unknown" after a warm hand-over), and the kept value for 0x105c. Load `cal_adc.hex` once
(CRC check; if missing or bad, the next location of section 3).

### 5.2 Ordered write list for a mode switch

On every RUN/SINGLE before the acquisition registers: if the mask changed (or the mode is unknown), and mask != 0:

| step | writes |
|---|---|
| A | input mode, 2 words (2.1 table), sleeps 100/100/100 us |
| B | ref word 0x01090000 \| (sh09 ^ 0x2720) |
| C | 0x1058 fine gain (3.3) |
| D | 8 core gain words in the order of 2.2 and 3.2, 80 us after each |
| E | 8 core offset words from the mode's record, 100 us before/after |
| F | 4 sample-clock delay words (regs 0x04, 0x04, 0x05, 0x05), 1 clock delay word (reg 0x01) |
| G | no DSP filter write |
| H | acquire block for the new ratio (`horizontal-config.md` 4.x) |
| I | 0x105c per enabled channel (4.3), channels ascending (order assumed) |
| J | arm with mask in 0x4008[3:0] |

Whether the delays are needed at all is not yet verified on hardware. 0x1038 bit 26 may serve as a completion flag instead
(`adc-spu-init.md`, 0x1038 = 0x050f0201 after init), assumed.

### 5.3 Write lists

`gggg` is the core gain field (low 7 bits of the stored value), `oooo` the core offset field (low 11 bits). Examples: gain
9 on core 0 gives 0x01150009, gain -4 on core 1 gives 0x011d007c, offset -24 on core 0 gives 0x011407e8. The reference
word and the delay words below are for reference 32 and all delays 0.

CH1 only (mask 1, mode 0; also what boot applies, without the two input-mode words):

```
0x1034 <- 0x01549460, 0x01550004                    input mode 0
0x1034 <- 0x01097700                                ref 32
0x1058 <- fine word of mode 0
0x1034 <- 0x0115gggg 0x0135gggg 0x011dgggg 0x013dgggg 0x0125gggg 0x0145gggg 0x012dgggg 0x014dgggg   gains c0 c4 c1 c5 c2 c6 c3 c7
0x1034 <- 0x0114oooo 0x011coooo 0x0124oooo 0x012coooo 0x0134oooo 0x013coooo 0x0144oooo 0x014coooo   offsets c0..c7
0x1034 <- 0x01048080 0x01048080 0x01058080 0x01058080 0x01010000                                    delays (the boot call writes 0x01040080 first: reg 0x04 starts at 0x8000)
0x105c <- 0x00005555
```

CH1 -> CH1+CH2 (mask 3, mode 4):

```
0x1034 <- 0x01541860, 0x01550005                    input mode 4
0x1034 <- 0x01097700
0x1058 <- fine word of mode 4
0x1034 <- 0x0115gggg 0x0135gggg 0x0125gggg 0x0145gggg 0x011dgggg 0x013dgggg 0x012dgggg 0x014dgggg   gains c0 c4 c2 c6 (CH1) c1 c5 c3 c7 (CH2)
0x1034 <- 0x0114oooo 0x011coooo 0x0124oooo 0x012coooo 0x0134oooo 0x013coooo 0x0144oooo 0x014coooo   offsets record 4
0x1034 <- 0x01048080 0x01048080 0x01058080 0x01058080 0x01010000
0x105c <- 0x00005555 (CH1), 0x00005555 (CH2)
```

CH1+CH2 -> CH1..CH4 (mask F, mode 10):

```
0x1034 <- 0x01540000, 0x01550000                    input mode 10
0x1034 <- 0x01097700
0x1058 <- fine word of mode 10
0x1034 <- 0x0115gggg 0x0135gggg 0x011dgggg 0x013dgggg 0x0125gggg 0x0145gggg 0x012dgggg 0x014dgggg   gains c0 c4 (CH1) c1 c5 (CH2) c2 c6 (CH3) c3 c7 (CH4)
0x1034 <- 0x0114oooo 0x011coooo 0x0124oooo 0x012coooo 0x0134oooo 0x013coooo 0x0144oooo 0x014coooo   offsets record 10
0x1034 <- 0x01048080 0x01048080 0x01058080 0x01058080 0x01010000
0x105c <- 0x00005555 x4
```

Then the acquire block for ratio 1 / 2 / 4 as in `horizontal-config.md` and the arm with chmask 1 / 3 / F.

### 5.4 Verified vs guessed

Verified on the unit: 0x1058 and the ADC register values after start in mode 0, 0x105c = 0x5555.

Assumed: the channel order of the 0x105c writes, the meaning of the sel1/sel2 fields and of the core-to-channel
assignment, whether the sleeps can be replaced by polling 0x1038. The meaning of the fine-gain codes 0..3 is not known.

Not yet verified on hardware: a mode switch against a known signal. Check: with a known signal on CH1 and CH2, switch
1 -> 3 -> F, read frames and compare the per-core offset spread (the 8-core comb visible with `oos-hw adc-meas`) with
the same settings under the stock system.
