# AFE per-setting writes (V/div, offset, coupling, bandwidth, invert)

Complements `afe-spi.md` (bus, AFE write, init, efuse). The code is `hal::afe`, the read path `hal::wpu`.

Status: OOS drives the AFEs as described here, and the self-calibration ran through this path on the unit
(`self-cal.md` 13). The individual write sequences are not yet verified on hardware word by word.

Units: scale and offset are `int64` **nanovolts at the BNC** (probe ratio not applied, see 3.7). The ADC/FPGA code scale is
**7500 codes per division** around 32768.

## 1. Data structures

### 1.1 AFE shadows

AFE registers are not read back. Every setter keeps a per-channel 16-bit shadow of the register (`rNN` = AFE register
0xNN), modifies bits and writes the **whole shadow word**. The shadows start at the init values and are changed by the
efuse read (`afe-spi.md`). State after init (all channels) for the registers the setting functions touch:

| reg | shadow | reg | shadow | reg | shadow | reg | shadow |
|---|---|---|---|---|---|---|---|
| 0x06 | 0x1d00 | 0x22 | 0x5a00 | 0x27 | 0x80c4 | 0x2b | **0x4c5f** |
| 0x07 | 0x8440 | 0x23 | 0x1000 | 0x28 | 0x8000 | 0x2c | 0x000c |
| 0x20 | 0x0008 | 0x24 | 0x0000 | 0x29 | 0x0282 | 0x2d | 0x0000 |
| 0x21 | 0x0a91 | 0x25 | 0x0000 | 0x2a | **0x3c44** | 0x2e | 0x1ff9 |
| 0x26 | 0x3200 | 0x35 | 0x0000 | 0x36 | 0x0000 | 0x37 | 0xc000 |

r2a/r2b differ from the init table because the efuse read leaves its bit selects behind: r2a[2:0] = 4 (last bit
of efuse word 0x64), r2b[6:4] = (0x64 + 1) & 7 = 5, r2b[3:0] = 15 (last bit of word 0x63), r2b[9:8] cleared at the end.
These two post-init values are not yet verified on hardware.

Several setters are **read-modify-write on bits left by earlier calls** (e.g. input gain 2/3 keep r20[15:13] from a previous
input gain 1), so the write values depend on history. Keep the same shadows across commands.

### 1.2 Channel settings

Per channel: scale (s64, nV/div), offset (s64, nV), position (s64, nV), invert, impedance, band limit (u32, 5.1),
coupling (0 DC, 1 AC, 2 GND), digital gain and original digital gain (f64), probe ratio (f32).

Offset sign = invert ? -1 : +1. AFE ground level = offset * 7500 / scale + 0x8000.

### 1.3 Calibration data

All calibration files have a 28-byte header (`self-cal.md` 7.1). Payloads:

| content | file |
|---|---|
| LSB table `f64 lsb[4 ch][4 inputgain]`, V per offset-DAC code | `cal_lsb.hex` (128 B) |
| vertical table, 16 blocks of 801 x 56 B, block = variant * 4 + ch | `cal_vertical.hex`, 16 x 801 x 56 B |
| older vertical table, 12 blocks | `cal_vertical.hex`, 12 x 801 x 56 B |
| bandwidth trim, 4 ch x 20 s32 | `cal_afe_bandwidth.hex` (320 B) |
| zero (offset) trim, 4 ch x 20 s32 | `cal_afe_zero.hex` (320 B) |

File header (7 x u32): [0] header CRC32, [1] 0x14, [2] 0, [3] time, [4] date (0x7dd01 = January 2013), [5] payload size,
[6] payload CRC32. Location order: `/oos/cal`, `/rigol/data`, `/rigol/data/default`.

A 12-block `cal_vertical.hex` has no group 3. `hal::afe::Cal` reads the 16-block layout.

In `cal_afe_bandwidth.hex`, ints 4/10/16 of each channel are replaced by 110/97/97 after loading. The stock system also
saves them back to the file. Under OOS these ints have no effect (5.4).

**Vertical record (56 bytes)**, `<q8i d i i>`:

| off | type | meaning | used by |
|---|---|---|---|
| +0x00 | s64 | key = scale in nV/div | index search |
| +0x08 | s32 | input gain (IG) 0..3 (input attenuator path) | `hz_input_gain`, LSB index, zero/bw trim |
| +0x0c | s32 | VGA 0..3 | `hz_vga_gain` |
| +0x10 | s32 | ATT 0..3 | `hz_att_gain` |
| +0x14 | s32 | SGA 0..3 | `hz_sga_gain` |
| +0x18 | s32 | SGAR 0..3 | `hz_sgar_gain` |
| +0x1c | s32 | MUX 0..7 | `mux` |
| +0x20 | s32 | MUXR 0..3 | `muxr` |
| +0x24 | s32 | 0 (unused) | |
| +0x28 | f64 | digital gain (FPGA) | read-path gain (4.1) |
| +0x30 | s32 | DAC zero code (~32768) | offset (3.2) |
| +0x34 | s32 | 0 | |

Index: first index whose key >= scale (keys start at 100000 = 100 uV/div and step 2000 x50, 5000 x60, 10000 x50, 20000
x50, 50000 x60, ... up to 10 V/div at index 800). The keys are the same in every block, so search the keys.

**Variant (block group) selection**: OOS selects the block group by the rule of 5.2. In a stock calibration file the
groups belong to these stock band-limit codes (hw = hardware GPIO version of the board):

| stock band limit | variant |
|---|---|
| 2 (20 MHz) | 0 |
| 5 (70 MHz), 7 (125 MHz), any other | 1 |
| 6 (100 MHz), 10 (250 MHz) | 2 |
| 10 and (hw \| 8) == 0xc | 3 |

Group 3 is all zero in a factory file (verified on the unit) and filled after a self-calibration with four passes
(`self-cal.md` 2.1). Loading a zero block gives all-zero gains: check with `Cal::block_filled` before relying on group 3.
A filled file holds 76 distinct gain tuples, the configurations the vertical self-cal measures.

Example rows, variant 2 (values vary per channel and unit):

| V/div | idx | IG | VGA | ATT | SGA | SGAR | MUX | MUXR | dgain | zero |
|---|---|---|---|---|---|---|---|---|---|---|
| 0.5 m | 110 | 0 | 0 | 2 | 0 | 0 | 3 | 1 | ~2.14 | ~32750 |
| 1 m | 160 | 0 | 0 | 2 | 0 | 0 | 3 | 1 | ~1.07 | ~32750 |
| 5 m | 270 | 0 | 0 | 1 | 1 | 0 | 3 | 0 | ~1.09 | ~32760 |
| 10 m | 320 | 0 | 1 | 2 | 0 | 0 | 3 | 1 | ~1.07 | ~32720 |
| 100 m | 480 | 1 | 2 | 2 | 3 | 0 | 3 | 0 | ~1.17 | ~32790 |
| 1 | 640 | 2 | 2 | 1 | 3 | 0 | 3 | 0 | ~1.08 | ~32800 |
| 10 | 800 | 3 | 2 | 1 | 3 | 0 | 3 | 0 | ~1.05 | ~32800 |

The smallest analog range is about 1 mV/div (dgain 1.07). Below that the FPGA digital gain does the rest (0.5 mV/div = 2.14).

The 801 scale keys are the same in every block and form a fine ladder from 100 uV to 10 V: per decade 1.00 by 0.02, 2.00 by
0.05 and 5.0 by 0.1, so 160 rows a decade and 1 to 2.5 % a row. Fine scale walks these rows: each one has its own gain codes
and digital gain, so a fine scale needs nothing beyond the normal scale set-up. The lookup takes the first key >= the
requested scale, which makes the request exact to the nanovolt. A scale held as f32 misses it (100 mV reads 100000001 nV and
lands on the 102 mV row), so OOS accepts a value up to a millionth above a key as that key.

## 2. AFE primitives

Notation: `rNN[h:l] = v` modifies the shadow, `W(NN)` writes the shadow word. mode = r07[14:13] (0x4000 = high-impedance
non-inverting path, 0 = inverting path). Names are those of `hal::afe::Afe` where OOS has the primitive.

| function | args | effect |
|---|---|---|
| `hz_enable` | ch, en | m = r07 & 0x4000 (if r07 bit 13 set: m \|= ((read of reg 0 << 9) & 0x2000), then m ^= 0x2000). en=1, m=0x4000: r23[13:11]=5 W(23); r29 &= ~0x300 W(29); r2a &= 0x0fff W(2a). en=1, m=0: r23[13:11]=2 W(23); r29[9:8]=2 W(29); r2a = 0x2000 \| (r2a & 0xfff) W(2a). en=0 (m 0 or 0x4000): r23 &= ~0x3800 W(23); r29 \|= 0x300 W(29); r2a \|= 0xc000 W(2a). other m: error |
| `hz_input_gain` | ch, g 0..3 | g=0: r07[14:13]=2 W(07); r23[13:11]=5 W(23). g=1: r07[14:13]=0 W(07); r20 = (r20 & 0x1ff8) \| 0xa000; r23[13:11]=2; W(20) W(23). g=2/3: r07[14:13]=0 W(07); r20[2:0] = 4/6; r23[13:11]=2; W(20) W(23) |
| `hz_vga_gain` | ch, v 0..3 | v<2: r07[14:13]=2 W(07); r29[9:8]=v. v=2: r07[14:13]=0 W(07); r29[9:8]=2. v=3: r29 \|= 0x300. then W(29) |
| `hz_att_gain` | ch, v | r29[7:6]=v W(29) |
| `hz_sga_gain` | ch, v 0..3 | r07[14:13] = (v>=2) ? 0 : 2 W(07); r2a[13:12]=v W(2a) |
| `hz_sgar_gain` | ch, v | r2a[15:14]=v W(2a) |
| `mux` | ch, v | r2e[13:11]=v W(2e) |
| `muxr` | ch, v | r2e[15:14]=v W(2e) |
| `hz_output` | ch, which, en | which 0: r2b[10]=en; which 1: r2b[11]=en; W(2b) |
| buffer | ch, en | r2b[10],[11],[14] = en W(2b) (not used in the normal path) |
| `coupling_dc` | ch, dc | r07[15] = dc & 1 W(07) |
| input mode | ch, m | r07 = (r07 & ~0x6000) \| m W(07) (calibration only) |
| `dac_enable` | ch, en | r27 = (r27 & 0x7f7f) \| en<<15 \| en<<7 W(27) (only the AFE zero calibration) |
| `set_dac` | ch, code | r28 = code W(28): **offset DAC inside the AFE** |
| `hz_filter` | ch, a, b, c | r2e = (r2e & 0xf900) \| (a&3)<<6 \| (b&3)<<9 \| (c&0x3f) W(2e) |
| `cal_zero_first` | ch, mode, a, b | mode 0: reg 0x26, mode 0x4000: reg 0x23; r = (r & 0xf800) \| (a&1)<<10 \| (b&0x3ff); W |
| `cal_zero_vga4000` | ch, mode, v, sel | mode 0x4000, sel 1: r35[8:0]=v W(35). mode 0x4000, sel 0: r29 = (r29 & 0x3ff) \| ((v<<7) & 0xfc00); r35[11:9]=v&7; W(29) W(35). (modes 0/0x2000/0x6000 touch 0x2f/0x2c/0x32/0x36, not used by the setting path) |
| `cal_zero_sga` | ch, mode, a, b | mode 0/0x4000: r27 = (r27 & 0x8080) \| (a&0x7f)<<8 \| (b&0x7f) W(27); mode 0x2000/0x6000: same into r2d (& 0xc000, a<<7) |
| `cal_zero_mux` | ch, a, b | r06 = (r06 & 0x8080) \| (b&0x7f)<<8 \| (a&0x7f) W(06) |
| `cal_bw` | ch, sel, v | 0: r22[7:0]; 1: r22[15:8]; 2: r2f[6:5]; 3: r2a[11:10]; 4: r20[2:0]; 5: r37[15:12]; 6: r20[6:4]; 7: r21[11:7]; 8: r21[6:0]; 9: r21[15:12]; 10: r24[9:0]; 11: r25[9:0]; then W(reg) |

The r07 bit 13 branch of `hz_enable` is never reached, because the normal setters do not set that bit. OOS does not
implement it.

Other devices:
- Gear: one GPIO per channel, GPIO4_D1/D2/D5/D6 (153/154/157/158), line names `afe1`..`afe4`. Under the stock system
  `open("/dev/dac_gear_change")`, `ioctl(fd, 0x7801, &u8)` with u8 = ch | gear << 4 sets line ch = u8 & 0xf (0..3) to
  (u8 >> 4) & 1. Without that device OOS drives the line directly (`hal::afe::set_gear`). Gear 1 (high) is used by the
  scale setting and all vertical/ADC calibrations, gear 0 only inside the AFE zero calibration (`self-cal.md` 3.1). All
  four lines are output high on a running unit (verified on the unit).
- FPGA (BAR1): see section 4.

## 3. Per setting

### 3.1 V/div

Rewrite the offset DAC after every V/div change (3.2).

1. If the acquisition runs, pause it.
2. idx = index search (1.3), rec = vertical table[variant(bw)][ch][idx].
3. `hz_enable(ch, 1)`
4. `hz_input_gain(rec.IG)`, `hz_vga_gain(rec.VGA)`, `hz_att_gain(rec.ATT)`, `hz_sga_gain(rec.SGA)`,
   `hz_sgar_gain(rec.SGAR)`, `mux(rec.MUX)`, `muxr(rec.MUXR)`
5. `hz_output(ch, 0, 1)`, `hz_output(ch, 1, 0)` (r2b[10]=1, r2b[11]=0)
6. Remember rec.IG for the channel (no hardware write).
7. Gear GPIO of the channel high.
8. Zero trims, z = cal_afe_zero[ch][0..19]:
   - IG 0, VGA 0: `first(0x4000, z0, z1)` (reg 0x23), `vga(0x4000, z2, sel 1)` (0x35), `sga(0x4000, z3, z4)` (0x27), `mux(z5, z6)` (0x06)
   - IG 0, VGA 1: `first(0x4000, z7, z8)` (0x23), `vga(0x4000, z9, sel 0)` (0x29, 0x35), `sga(0x4000, z10, z11)` (0x27), `mux(z12, z13)` (0x06)
   - IG 1..3, VGA 2: `first(0, z14, z15)` (0x26), `sga(0, z16, z17)` (0x27), `mux(z18, z19)` (0x06)
   - any other combination: nothing
9. Bandwidth trims, b = cal_afe_bandwidth[ch][0..19]:
   - IG 1..3: k = 6*IG - 5; `cal_bw(1, b[k])` (r22[15:8]), `(6, b[k+1])` (r20[6:4]), `(7, 17)` (r21[11:7]),
     `(8, 126)` (r21[6:0]), `(9, b[k+4])` (r21[15:12]). Sel 7 and sel 8 are constants (5.4).
   - IG 0: `cal_bw(10, b[18])` (r24[9:0]), `cal_bw(11, b[19])` (r25[9:0])
10. No filter write. The filter stays as the bandwidth setting left it (5.1).
11. Store rec.dgain as digital gain and original digital gain. The FPGA gain is written later by the read-path set-up (4.1).

Write sequence, CH1, 100 mV/div, bandwidth `300M` (value 0x3c0, block group 2), offset 0, starting from the post-init
shadows. The first line is the filter write of 5.1. Trim and DAC values are examples from a calibration file.

```
w 0x2e <- 0x193c   hz_filter(0,0,60)
w 0x23 <- 0x1000   hz_enable   (r07 mode 0 after init)
w 0x29 <- 0x0282
w 0x2a <- 0x2c44
w 0x07 <- 0x8440   hz_input_gain(1)
w 0x20 <- 0xa008
w 0x23 <- 0x1000
w 0x07 <- 0x8440   hz_vga_gain(2)
w 0x29 <- 0x0282
w 0x29 <- 0x0282   hz_att_gain(2)
w 0x07 <- 0x8440   hz_sga_gain(3)
w 0x2a <- 0x3c44
w 0x2a <- 0x3c44   hz_sgar_gain(0)
w 0x2e <- 0x193c   mux(3)
w 0x2e <- 0x193c   muxr(0)
w 0x2b <- 0x4c5f   hz_output(0,1)
w 0x2b <- 0x445f   hz_output(1,0)
gear CH1 = 1       (ioctl(dac_gear_change, 0x7801, 0x10))
w 0x26 <- 0x350e   zero trim: first(0, 1, 270)
w 0x27 <- 0x8b80              sga(0, 11, 0)
w 0x06 <- 0x1000              mux(0, 16)
w 0x22 <- 0x0000   bandwidth trim IG1: 0, 3, 17, 126, 0
w 0x20 <- 0xa038
w 0x21 <- 0x0891
w 0x21 <- 0x08fe
w 0x21 <- 0x08fe
w 0x28 <- 0x8016   offset: DAC = zero 32790
```

CH1, 5 mV/div (IG 0, VGA 0, ATT 1, SGA 1, SGAR 0, MUX 3, MUXR 0), offset +10 mV, same bandwidth, from the same start state:
`07<-c440 23<-2800 07<-c440 29<-0082 29<-0042 07<-c440 2a<-1c44 2a<-1c44 2e<-193c 2e<-193c 2b<-4c5f 2b<-445f`, gear,
`23<-2855 35<-00ef 27<-ae80 06<-1000 24<-0240 25<-024e 28<-7f54` (the leading `2e 23 29 2a` filter and `hz_enable` writes as above).

### 3.2 Offset and position

Run after a V/div change, an offset or position change, invert, channel on.

1. Store the offset. If the channel is off: stop here (**no hardware write while off**; channel on re-applies scale and offset).
2. rec as in 3.1 (same variant selection); IG = rec.IG, zero = rec.zero; lsb = cal_lsb[ch][IG].
3. if lsb != 0: `code = (int)( -(sign * offset + position) / 1e9 / lsb + zero )` (truncation toward zero), sign = invert ? -1 : 1;
   else code = zero.
4. `set_dac(ch, code)` -> **AFE reg 0x28 = code** (the only hardware write).
5. With GND coupling re-apply the read path (4.1), else restart the acquisition counters.

Typical LSB table values (V per code, `cal_lsb.hex`, they vary by about 1 % between channels):

| IG 0 | IG 1 | IG 2 | IG 3 |
|---|---|---|---|
| 6.1e-5 | -3.7e-4 | -1.7e-3 | -1.7e-2 |

So the offset range is about +-2 V (IG 0), +-12 V (IG 1), +-56 V (IG 2), +-550 V (IG 3). The sign flips between IG 0 and the
attenuated paths. Clamp the offset to the range of the scale before.

### 3.3 Coupling

c: 0 = DC, 1 = AC, 2 = GND (DC default, consistent with r07 bit 15 = 1 after init).
- c = 0 or 1: `coupling_dc(ch, c == 0)` -> **r07[15] = 1 for DC, 0 for AC**, W(07). Then re-apply the read path if the
  previous coupling was GND, else restart the acquisition counters.
- c = 2 (GND): **no AFE write**, only re-apply the read path. GND is done in the FPGA: the channel's digital gain is 0
  and its offset is `32768 + 7500 * offset / scale` (a flat line at the offset level, 4.1). The AFE keeps the previous DC/AC state.

### 3.4 Bandwidth limit

OOS follows 5.1 (value encoding, filter table, ADC words, scale and offset afterwards).

The stock calibration set-up uses five band-limit codes: 2 = 20 MHz, 5 = 70 MHz, 6 = 100 MHz, 7 = 125 MHz, 10 = 250 MHz
(`hal::model` holds the rated bandwidth codes). 7 and 10 are assumed. Their filter triples, same for all four channels:

| code | triple | r2e[10:0] |
|---|---|---|
| 2 (20 MHz) | (0, 0, 5) | 0x005 |
| 5, other | (0, 0, 23) | 0x017 |
| 6 | (1, 0, 23) | 0x057 |
| 7 | (2, 0, 23) | 0x097 |
| 10 | (3, 3, 63) | 0x6ff |

r2e bit 8 and bits 15:11 (MUX/MUXR) are kept. With the post-init shadow code 10 gives r2e = 0x1fff, code 2 gives 0x1905.

The variant of the vertical table changes with the bandwidth, but the filter write itself does not rewrite the gains or
the DAC. So run the scale and offset steps at the end (5.1).

### 3.5 Invert

Only if the channel is on: store the flag, rewrite the offset (3.2: DAC code with the sign flipped, so the AFE offset is
mirrored), re-arm. No other AFE write. The waveform inversion itself is in the FPGA:
SPU register **0x1004 bits 19:16**. With 4 channels bit 16+n = invert of CHn+1. One channel on all ADC cores (mask 1/2/4/8):
that channel's bit in all four. Two channels: the two active channels as (a, a, b, b).

`Afe::set_offset` takes the invert flag, `hal::afe::configure_ch` passes it on. Verified on the unit 2026-11-07, with the
trigger: the TPU sees the samples after the inversion, so a level is meant as it shows on the screen.

### 3.6 Impedance

No AFE write. The DHO900 has no 50 Ohm path. The AFE's r07[14:13] mode bits are driven by the gain setters, not by
the impedance setting.

### 3.7 Probe ratio

No hardware write. The AFE code works with the BNC scale. The ratio is only used for the displayed units:
yinc = scale * 1e-9 * ratio / 7500. Divide the user's V/div and offset by the probe ratio before calling the AFE code.

### 3.8 Channel on/off

No AFE power-down. On a change (outside roll/scan) stop the SCU, re-apply scale (3.1) and offset (3.2), then re-arm.

### 3.9 Init order

After the ADC/SPU bring-up: FPGA 0x1418/0x141c/0x1420/0x1424 <- 0x8000 (register 0x1418 + 4*ch), scale of CH1 = 100 mV/div,
acquisition start, then `hz_enable(ch, 1)` for ch 0..3.

## 4. FPGA side of the vertical settings

### 4.1 Digital gain and offset

Per sampled channel during live acquisition (`hal::wpu::analog`):
1. dgo = rec.dgain. If dgo >= 2.0: ADC gain x2 for that channel and dg = dgo / 2; else gain 1 and dg = dgo.
   The gain goes to register **0x105c** in the two-bit-field layout of `adc-mode.md` 4.3; all channels at x1 give 0x5555
   in every mode, and it is rewritten in the layout of the new channel mode after a mode switch.
2. Not GND: g = min(dg, 508.0); **gain = (int)(g * 1024)** (Q10), **offset = (int)(32768 - 32768 * g)**.
   GND: gain = 0, offset = (int)(offset / scale * 7500 + 32768).
3. Analog gain -> **0x14c8, 0x14d4, 0x14e0, 0x14ec**; analog offset -> **0x14d0, 0x14dc, 0x14e8, 0x14f4**; the same values
   to the zoom gain (0x1508..0x1514) and zoom offset (0x14f8..0x1504). With 1 or 2 active channels the values are replicated
   across the four slots like the invert bits.

Stopped or replayed data (the live setting differs from the setting of the acquisition) uses the same formula with
g = scale_acq / scale_live * dgain_acq and an extra offset 7500 * (off_live - off_acq) / scale_live.

### 4.2 Summary of FPGA registers

| reg | purpose | value |
|---|---|---|
| 0x1004[19:16] | invert | per-stream invert |
| 0x105c | ADC gain | 2-bit field per slot: 1 = x1, 2 = x2 (`adc-mode.md` 4.3) |
| 0x1418 + 4*ch | AFE offset word (init only, meaning not known) | 0x8000 |
| 0x14c8/d4/e0/ec | analog gain | Q10 digital gain |
| 0x14d0/dc/e8/f4 | analog offset | 32768 - 32768*g, or the GND line |
| 0x1034 | ADC SPI (bandwidth change) | 0x01545251, 0x0155000c, then the input-mode words |

## 5. Bandwidth control

### 5.1 Bandwidth setting

The bandwidth setting is a value v. `hal::afe` uses it unchanged: no clamp to the model bandwidth and no forcing to 20 MHz
at small scales.

Sequence (registers and order):
1. Store v as the channel's band limit.
2. Filter triple (a, b, c):

| v | (a, b, c) | meaning |
|---|---|---|
| 1 | (3, 3, 63) | OFF (largest filter, nominal 3150 MHz) |
| 2 | (0, 0, 5) | S20M |
| 0x11 | (0, 0, 0) | GND (filter closed; blocks the signal) |
| 5 | (0, 0, 31) | 155M |
| 6 | (0, 0, 63) | 315M |
| 7 | (0, 0, 25) | 125M |
| 0xa | (3, 3, 7) | 350M |
| 0x12 | (0, 0, clamp(Fs / 19841269, 1, 63)) | Auto: Fs = sample rate in Sa/s; 19841269 = 1.25e9/63, so 1.25 GSa/s -> 63, 625 M -> 31, 312.5 M -> 15, 156.25 M -> 7, 62.5 M -> 3, 31.25 M -> 1 |
| any other | a = v[27:20], b = v[19:12], c = v[11:4] | encoded value |

   `hz_filter` then uses a & 3, b & 3, c & 0x3f (r2e[7:6], r2e[10:9], r2e[5:0]; r2e[15:11] and bit 8 kept).
   The filter is analog (about Fs / 4 in absolute hertz), so the Auto constant stays at any ADC clock. Above 1.25 GSa/s the
   code is capped at its maximum 63 (filter open).
3. ADC: reg 0x54 <- 0x4210 and reg 0x55 <- 0x0008 -> FPGA 0x1034 <- **0x01545251**, **0x0155000c**
   (100 us before/after each), on every call. The word is 0x1000000 | reg << 16 | (XOR mask of reg ^ value).
4. `hz_filter(ch, a, b, c)` -> W(2e).
5. Scale (5.3) and offset (3.2): rewrites gains and trims for the new variant, and the DAC.
6. Restore the ADC input mode of the current channel set (`adc-mode.md` 2.1), re-arm, restart the acquisition counters.

Full table (value as sent; triple after decoding; r2e[10:0] after masking; vertical block group by 5.2):

<details><summary>84 entries</summary>

| label | value | (a, b, c) | r2e[10:0] | block |
|---|---|---|---|---|
| FULL | 0x0 | (0, 0, 0) | 0x000 | 0 |
| OFF | 0x1 | (3, 3, 63) | 0x6ff | 3 |
| GND | 0x11 | (0, 0, 0) | 0x000 | 0 |
| 5M | 0x10 | (0, 0, 1) | 0x001 | 0 |
| 8M | 0x100010 | (1, 0, 1) | 0x041 | 3 |
| 10M | 0x20 | (0, 0, 2) | 0x002 | 0 |
| 15M | 0x30 | (0, 0, 3) | 0x003 | 0 |
| 20M | 0x40 | (0, 0, 4) | 0x004 | 0 |
| S20M | 0x2 | (0, 0, 5) | 0x005 | 0 |
| 25M | 0x50 | (0, 0, 5) | 0x005 | 0 |
| 30M | 0x60 | (0, 0, 6) | 0x006 | 0 |
| 35M | 0x70 | (0, 0, 7) | 0x007 | 0 |
| 40M | 0x80 | (0, 0, 8) | 0x008 | 0 |
| 45M | 0x90 | (0, 0, 9) | 0x009 | 0 |
| 50M | 0xa0 | (0, 0, 10) | 0x00a | 0 |
| 55M | 0xb0 | (0, 0, 11) | 0x00b | 0 |
| 60M | 0xc0 | (0, 0, 12) | 0x00c | 0 |
| 65M | 0xd0 | (0, 0, 13) | 0x00d | 0 |
| 70M | 0xe0 | (0, 0, 14) | 0x00e | 0 |
| 75M | 0xf0 | (0, 0, 15) | 0x00f | 0 |
| 80M | 0x100 | (0, 0, 16) | 0x010 | 1 |
| 85M | 0x110 | (0, 0, 17) | 0x011 | 1 |
| 90M | 0x120 | (0, 0, 18) | 0x012 | 1 |
| 95M | 0x130 | (0, 0, 19) | 0x013 | 1 |
| 100M | 0x140 | (0, 0, 20) | 0x014 | 1 |
| 105M | 0x150 | (0, 0, 21) | 0x015 | 1 |
| 110M | 0x160 | (0, 0, 22) | 0x016 | 1 |
| 115M | 0x170 | (0, 0, 23) | 0x017 | 1 |
| 120M | 0x180 | (0, 0, 24) | 0x018 | 1 |
| 125M | 0x7 | (0, 0, 25) | 0x019 | 1 |
| 130M | 0x1a0 | (0, 0, 26) | 0x01a | 1 |
| 135M | 0x1b0 | (0, 0, 27) | 0x01b | 1 |
| 140M | 0x1c0 | (0, 0, 28) | 0x01c | 1 |
| 145M | 0x1d0 | (0, 0, 29) | 0x01d | 1 |
| 150M | 0x1e0 | (0, 0, 30) | 0x01e | 1 |
| 155M | 0x5 | (0, 0, 31) | 0x01f | 1 |
| 160M | 0x200 | (0, 0, 32) | 0x020 | 1 |
| 165M | 0x210 | (0, 0, 33) | 0x021 | 1 |
| 170M | 0x220 | (0, 0, 34) | 0x022 | 1 |
| 175M | 0x230 | (0, 0, 35) | 0x023 | 1 |
| 180M | 0x240 | (0, 0, 36) | 0x024 | 1 |
| 185M | 0x250 | (0, 0, 37) | 0x025 | 1 |
| 190M | 0x260 | (0, 0, 38) | 0x026 | 1 |
| 195M | 0x270 | (0, 0, 39) | 0x027 | 1 |
| 200M | 0x280 | (0, 0, 40) | 0x028 | 1 |
| 205M | 0x290 | (0, 0, 41) | 0x029 | 1 |
| 210M | 0x2a0 | (0, 0, 42) | 0x02a | 1 |
| 215M | 0x2b0 | (0, 0, 43) | 0x02b | 2 |
| 220M | 0x2c0 | (0, 0, 44) | 0x02c | 2 |
| 225M | 0x2d0 | (0, 0, 45) | 0x02d | 2 |
| 230M | 0x2e0 | (0, 0, 46) | 0x02e | 2 |
| 235M | 0x2f0 | (0, 0, 47) | 0x02f | 2 |
| 240M | 0x300 | (0, 0, 48) | 0x030 | 2 |
| 245M | 0x310 | (0, 0, 49) | 0x031 | 2 |
| 250M | 0x320 | (0, 0, 50) | 0x032 | 2 |
| 255M | 0x330 | (0, 0, 51) | 0x033 | 2 |
| 260M | 0x340 | (0, 0, 52) | 0x034 | 2 |
| 265M | 0x350 | (0, 0, 53) | 0x035 | 2 |
| 270M | 0x360 | (0, 0, 54) | 0x036 | 2 |
| 275M | 0x370 | (0, 0, 55) | 0x037 | 2 |
| 280M | 0x380 | (0, 0, 56) | 0x038 | 2 |
| 285M | 0x390 | (0, 0, 57) | 0x039 | 2 |
| 290M | 0x3a0 | (0, 0, 58) | 0x03a | 2 |
| 295M | 0x3b0 | (0, 0, 59) | 0x03b | 2 |
| 300M | 0x3c0 | (0, 0, 60) | 0x03c | 2 |
| 305M | 0x3d0 | (0, 0, 61) | 0x03d | 2 |
| 310M | 0x3e0 | (0, 0, 62) | 0x03e | 2 |
| 315M | 0x6 | (0, 0, 63) | 0x03f | 2 |
| 350M | 0xa | (3, 3, 7) | 0x6c7 | 0 |
| 400M | 0x303080 | (3, 3, 8) | 0x6c8 | 3 |
| 450M | 0x303090 | (3, 3, 9) | 0x6c9 | 3 |
| 500M | 0x3030a0 | (3, 3, 10) | 0x6ca | 3 |
| 550M | 0x3030b0 | (3, 3, 11) | 0x6cb | 3 |
| 600M | 0x3030c0 | (3, 3, 12) | 0x6cc | 3 |
| 650M | 0x3030d0 | (3, 3, 13) | 0x6cd | 3 |
| 700M | 0x3030e0 | (3, 3, 14) | 0x6ce | 3 |
| 750M | 0x3030f0 | (3, 3, 15) | 0x6cf | 3 |
| 800M | 0x303100 | (3, 3, 16) | 0x6d0 | 3 |
| 850M | 0x303110 | (3, 3, 17) | 0x6d1 | 3 |
| 900M | 0x303120 | (3, 3, 18) | 0x6d2 | 3 |
| 1G | 0x303140 | (3, 3, 20) | 0x6d4 | 3 |
| 1.1G | 0x303160 | (3, 3, 22) | 0x6d6 | 3 |
| 1.2G | 0x303180 | (3, 3, 24) | 0x6d8 | 3 |
| Auto | 0x12 | (0, 0, clamp(Fs/19841269, 1, 63)) | c | c<=15: 0, <=42: 1, else 2 |

</details>

Regular structure: 5..310 MHz: v = (MHz/5) << 4 (c = MHz/5), except 125M -> 7, 155M -> 5, 315M -> 6, 350M -> 0xa and S20M -> 2
(these have fixed triples); 400 MHz..1.2 GHz: a = b = 3, c = MHz/50; 8M: a = 1, c = 1. FULL = 0
decodes to (0, 0, 0) like GND.

### 5.2 Bandwidth variant (cal_vertical block group)

The scale and offset steps pick the block group from the band limit v (`hal::afe::block_group`):

| bandwidth value v | block group |
|---|---|
| 1 (OFF) | 3 |
| 2, 5, 6 | 0, 1, 2 |
| 7 | 1 |
| 0x12 (Auto) | c <= 15: 0, c <= 42: 1, else 2 (c = Auto filter code of 5.1) |
| other with v & 0xf != 0 (0xa = 350M, 0x11 = GND) | 0 |
| v & 0xf == 0, v <= 0xf0 (FULL, 5..75 MHz) | 0 |
| v <= 0x2a0 (80..210 MHz) | 1 |
| v <= 0x3f0 (215..310 MHz) | 2 |
| larger (8M, 400 MHz..1.2 GHz) | 3 |

So the four block groups mean: 0 = up to 75 MHz (and 350M, GND), 1 = 80..210 MHz (+125M, 155M),
2 = 215..315 MHz, 3 = OFF and the a/b = 3 settings.

### 5.3 Scale

Register sequence as in 3.1 steps 3-9 with the variant of 5.2 and the trims of 5.4. If scale and variant are both unchanged,
no write is needed. `hal::seq` runs this only when settings change and then always writes the full sequence.

### 5.4 Bandwidth trims

IG 1..3: r21[11:7] = 17 and r21[6:0] = 126 instead of the file values, so the load-time override of ints 4/10/16 (1.3)
has no effect. With the post-init r21 = 0x0a91 the IG-1 writes for CH1 become 0x0891, 0x08fe, 0x08fe. IG 0 uses the file
values.

### 5.5 Auto bandwidth refresh

With Auto the filter code follows the sample rate: every set-up recomputes it from the current rate (`hal::seq` passes
the rate to `hal::afe::configure`), and the block group follows (5.2). A change of the Auto code needs the filter write, then
scale and offset of every channel. Do not write 0x10dc while anti-aliasing is off; OOS never writes it.

### 5.6 Self-calibration

The vertical self-cal runs four passes and fills all four block groups. See `self-cal.md` 2.1.

## 6. OOS implementation

The code is in `hal::afe` (primitives, `set_scale`, `set_offset`, `configure`), `hal::wpu` and `hal::seq`.

1. **Persistent shadows**: one `Afe` per channel with the values of 1.1, kept in `seq::State`. Every setting loads, modifies
   and keeps it. Without it the read-modify-write values differ.
2. **Calibration loader**: `Cal::load` reads `cal_vertical.hex`, `cal_lsb.hex`, `cal_afe_zero.hex`,
   `cal_afe_bandwidth.hex` (+ override ints 4/10/16 = 110/97/97), skips the 28-byte header, and takes the first valid copy in
   the location order of 1.3. **Never write `/rigol/data`.**
3. **Primitives**: the table of section 2. Each modifies the shadow, then writes the whole word.
4. **Scale**: index search, variant by 5.2 from the channel's stored bandwidth value, steps 3.1 3-9 with the constants of
   5.4, no filter re-apply, gear line `afeN` high, then the offset step 3.2. Returns the record for the FPGA step.
5. **Offset**: 3.2 with the current scale's record; position = 0 unless needed.
6. **Coupling**: r07[15]; GND = no AFE write, FPGA gain 0 / offset line (4.1).
7. **Bandwidth**: 5.1: store the value, ADC words 0x01545251, 0x0155000c to 0x1034 (100 us gaps), filter triple from the
   table or encoding, then scale and offset, then the input-mode words. Auto needs the sample rate. If block group 3 is empty
   in the `cal_vertical.hex` in use, OFF and >= 400 MHz have no data (1.3).
8. **FPGA step** (registers and formula as in 4.1; the conditions of the stopped branch are assumed): per channel dg ->
   0x105c field (x2 if >= 2), 0x14c8 + 0xc*n gain = dg*1024, 0x14d0 + 0xc*n offset = 32768 - 32768*dg (zoom registers the same),
   invert bits in 0x1004[19:16] (keep the other bits of 0x1004), with the replication of 4.1 when fewer than 4 channels are
   acquired.
9. **Invert**: flip the offset sign in 3.2 and set the FPGA bit.
10. **Probe ratio**: software only, divide V/div and offset by the ratio before steps 4 and 5.
11. **Dry run**: the setters return lists of operations (`hal::afe::Op`), so a set-up can be printed before it touches the hardware.

Not known:
- What the mode bits, r20[15:13], r23[13:11], r2a[15:12] etc. mean physically (attenuator relays, buffers). OOS only reproduces
  the values.
