# Cold boot: from start-up to the first acquisition

Status: the OOS cold boot of section 4 is verified on the unit (`hal::dho::boot`, `oos-fpgad cold-boot`): it runs from a power
cycle to live frames without the stock OS ever running. Where a detail has not been run, the text says so.

Notation: `sN` = software shadow of register N (last value written; for 0x1008 the last value read). ADC words go through
0x1034 = `0x01000000 | reg << 16 | data` (read: bit 23 set, data in 0x1038 after 100 us).

A cold start needs, beyond FPGA boot, PLL, AFE, ADC, SPU and SCU init:
1. **runtime calibrations on every start**: ADC sync-delay sweep with the SPI/SPU sync sequence, the 28-lane IDELAY eye
   scan, the ADC core (preamble) alignment register **0x1020**, the ADC-internal trim, the stored ADC calibration of mode
   0 (0x1058 and 24 ADC words);
2. **once-only FPGA writes**: 0x1428 (DAC polling), 0x1418..0x1424 (AFE offset), 0x1000 mem-reset bits, the sinc
   coefficient RAM (0x1140/0x1144), SCU 0x4008 bits 23/24/26, health checks 0x1008 bit 29 and 0x1210;
3. **configuration registers of the first acquisition**: the read-path digital gain/offset (WPU block: 0x14c8.., 0x14d0..,
   zoom copies, 0x105c), the display-trace part of the horizontal block (sinc/linear interpolation, compression, delays,
   0x131c, 0x10a0, 0x11fc), the sample delays 0x1080/0x1084, and the ADC input mode words.

Group 3 matters because a cold FPGA holds power-on values in these registers. The read-path gain is the clearest case: the
memory holds raw 12-bit codes and the read-out words are about 17.23 x raw + offset (`adc-spu-init.md`), so a digital
gain/offset is applied on the way out. OOS must write its own values: inherited live values shift the codes (verified on
the unit).

## 1. Start-up sequence

### 1.1 Before the register steps (`fpga-boot.md`, `pll.md`)

| # | step | writes / polls | OOS |
|---|---|---|---|
| S1 | `spi2boot 0x400000` | `/dev/spidev1.0` mode 0, 8 bit, 10 MHz: `\rBOOT\r` + 00 40 00 00; fixed `sleep 10` | `oos-hw fpga-boot`, verified on the unit |
| S2 | `insmod pcie-rockchip.ko xdma.ko` | PCIe link, BAR1 = FPGA registers; 0x4000 = 0xe2072115 | `oos-fpga-up`, verified on the unit |
| S3 | `insmod spi2afe_gpio fg_m2c_gpio afg_gpio dac_gpio beeper_gpio fan_gpio_clt spi2pll_lxm2582_gpio` | GPIO drivers; OOS uses gpiochip lines instead | partly (AFE, PLL, fan in the DT) |
| S4 | `spi2pll_lxm2582` | 46 LMX2582 words, R0 (FCAL) last, 50 ms apart; no read-back | `oos-hw pll-init`, verified on the unit |
| S5 | register access | `/dev/xdma0_bypass`, 32 KiB mapped | |

### 1.2 Order

The steps of 1.3 run in one piece. Channel set-up, restored settings and the first RUN follow afterwards through the normal
paths of `horizontal-config.md`, `trigger.md`, `afe-settings.md`, `acquisition-start.md`.

Not part of the start-up: LMK04828 clock init (see `pll.md`), and any ADC bit-slip calibration.

### 1.3 Ordered steps

| # | step | what it writes / polls |
|---|---|---|
| 2 | AFE init, channels 0..3 | AFE bus, 24-word table, r06/r07/r27/r28/r2b, efuse read, r2f..r34 (`afe-spi.md`); verified on the unit |
| 3 | ADC init | ADC shadows to their start values (reg 0x00 = 0xe4, 0x06 = 0x01c3, 0x0d = 0x620f, 0x0f = 0x7a01, 0x13+8n = 0x1b9b, 0x16+8n = 0x44, ...); 104 words twice, then reg 0x0f `0x010f0201`. Timing per word: 1.1 ms before, 100 us after (verified on the unit); 100 us before and after also works |
| 4 | SPU init | 0x1000: 0x1, 0x0, 0x0, 0x10, 0x0; 0x105c = 0x55555555; 0x1010 = 0x20000000; 0x1014 = `(s1014 & 0xff0fffff) \| 0xf << 20 \| 1` = **0x00f00001** (shadow 0 at start) |
| 6 | SCU init | 0x4004 = 0x80000000, then 0x00000000; verified on the unit |
| 7 | panel MCU | UART `FA 05 00 27 AF` (`board.md`) |
| 8 | versions | read 0x4000, 0x401c (log only) |
| 9 | DAC polling | 0x1428 low half = `(5e9/5e7) << 8 \| (5.5e9/5e7)` = **0x646e** (upper half from the shadow, 0); verified on the unit |
| 10 | MIG results | **read only**: 0x14a0 [5:0] wrlvl_coarse, 0x14a4 [11:0] mpr_cpt, 0x1498 [11:0] wrlvl_cpt, 0x14ac [11:0] prbs_final_dqs, 0x14b4 [9:0] dq_idelay (logged) |
| 11 | memory reset off | 0x1000 = s1000 & ~0xc (bit 2, bit 3 = 0) |
| 12 | self test | read 0x1008: **bit 29 = FPGA memory DDR init pass**; read 0x1210: **ADC clock counter, pass if 62494..62506** (log only: section 3.1). Example 0x1008 in the running state: 0x24000201 |
| 13 | retry loop | if bit 29 = 0: up to 200 x (sleep 20 ms, self test); still 0 after 4 s: 0x1008 = last read \| bit 31 (fallback, acquisition memory in BRAM, assumed). OOS never sets this bit |
| 14 | calibration | section 1.3.1 |
| 15 | ADC normal data | ADC reg 0x0d: `0x010d520f` |
| 16 | core alignment | second run, same as 1.3.1 step k |
| 17 | sleep 1 ms | |
| 18 | AFE offset words | 0x1418, 0x141c, 0x1420, 0x1424 = 0x8000 |
| 19 | ADC bit range | 0x1014 = (s & 0xff0fffff) \| 0xf << 20 \| 1 (unchanged value: the mask at start is CH1, assumed) |
| 20 | AFE CH1 | 100 mV/div (`afe-settings.md`) |
| 21 | sinc RAM | 0x1138 = 0x80c8c8c8 twice, plus the coefficient RAM load for factor 200, sel 2 and sel 3 (section 3.8) |
| 24 | AFE Hz enable, channels 0..3 | AFE r23/r29/r2a (`afe-settings.md`) |
| 25 | SCU enables | 0x4008 bit 26 = 0, bit 24 = 0, bit 23 = 1 (three RMW writes) |

After these steps, write 0x1010 bit 31 = 0 (frame header on, `acquisition-start.md`).

#### 1.3.1 Calibration steps

| step | action | hardware effect |
|---|---|---|
| a | load `cal_lsb.hex` (128 B) | none |
| b | load `cal_vertical.hex` (0x836a0 or 0xaf380 B) | none |
| c | load `cal_adc.hex` (1848 B) | none |
| d | stored ADC calibration of mode 0 (`adc-mode.md` 2.2/5.3) | ref word, **0x1058**, 8 gains, 8 offsets, 4 sample-clock delay words, clock delay word; no input-mode words |
| e | load `cal_afe_zero.hex`, `cal_afe_bandwidth.hex` (320 B each) | none |
| f | load `cal_ext.hex` (160 B; may be absent) | none |
| g | load `cal_ddr.hex`; if present, DDR skip taps | 0x1000 \|= 0x8; 0x1470, 0x1474, 0x147c, 0x1484, 0x148c, 0x1494 (skip taps); 0x1000 &= ~0x8 (section 3.2) |
| h | load `cal_afg.hex` (96344 B), `cal_la.hex` (8 B) | none |
| i | ADC sync delay sweep | section 3.3 |
| j | lane eye scan and IDELAY load | section 3.4 |
| k | ADC core alignment | section 3.5; result 0x1020 = 0x1010, 0x1014 = 0x14f01301 (verified on the unit) |
| l | ADC internal trim | section 3.6 |

All files come from `/rigol/data/<name>` with fallback `/rigol/data/default/<name>`. Container: 20-byte header, u32
length, u32 crc32, payload. Only steps d and g write hardware from file contents.

## 2. Already in OOS / missing (per step)

"OOS" = `hal::dho::boot` plus the per-acquisition sequence of `hal::dho::seq`.

| step | OOS today |
|---|---|
| S1-S4 FPGA boot, PCIe, PLL | done |
| 2 AFE init x4 | done |
| 3 ADC init | done, 1.1 ms / 100 us timing |
| 4 SPU init | done, writes 0x00f00001; the later steps produce 0x14f01301 (keep the shadow in sync) |
| 6 SCU init | done |
| 9 0x1428 = 0x646e | done |
| 10-11 MIG read-back, 0x1000 &= ~0xc | done |
| 12-13 DDR / ADC-clock self test | poll 0x1008 bit 29; on failure stop and report, no BRAM fallback. 0x1210 is only logged here and checked after the eye scan |
| d stored ADC calibration | done, from `cal_adc.hex` |
| g DDR skip taps | done, from `/rigol/data/cal_ddr.hex` (format 3.2) |
| i sync-delay sweep | done, with the sync sequence (3.3) |
| j lane eye scan | done (3.4), fixed list as fallback |
| k, 16 core alignment 0x1020 | done (3.5), twice |
| l ADC internal trim | done (3.6), ADC 0 only |
| 15 normal data | written after the scan (the scan switches to test pattern) |
| 18 0x1418..0x1424 = 0x8000 | done |
| 19 bit range | done |
| 20 AFE CH1 100 mV/div | not needed at start: OOS configures the AFE of every enabled channel (`hal::dho::afe`) |
| 21 sinc coefficient RAM | not done: only the FPGA's sinc interpolation of the display trace uses it, the raw read-out does not (assumed) |
| 24 AFE Hz enable | done, all four AFEs |
| 25 0x4008 bits 23/24/26 | done |
| first ADC block (input mode) | the first mode set-up is forced (unknown -> mode): at a cold start the ADC is in mode 0 from the init list |
| first acquisition block | `hal::dho::acq::configure`, including 0x1080/0x1084 (3.10); without 0x128c unless the LA is on, without DVM |
| first horizontal block | only the read-out window 0x1100/0x1108/0x11f0, 0x1010 bit 31 and the pass-through writes (`record-play.md`). **Missing:** the rest of 3.9: 0x1118/0x111c, 0x1138, 0x131c, 0x10a0, 0x11fc, delays 0x1120..0x112c |
| first WPU block | gain/offset 0x14c8.. / 0x14d0.., 0x105c, written with the AFE set-up (`hal::dho::wpu`) |
| first WAU block | **missing:** the "mask test off" writes (3.9), low priority |
| SCU auto trigger, TPU edge, arm/read | done (`hal::dho::scu`, `hal::dho::tpu`, `hal::dho::seq`) |

## 3. Recipes for the start-up steps

Every 0x1034 write is preceded and followed by a 100 us sleep unless stated. 0x1038 is never polled for completion.

### 3.1 Once-only writes and health checks (steps 9-13, 18)

```
0x1428 <- (s1428 & 0xffff0000) | 0x646e                  DAC polling: 100 x 50 ns, 110 x 50 ns
read 0x14a0, 0x14a4, 0x1498, 0x14ac, 0x14b4                optional, log the MIG tap results
0x1000 <- s1000 & ~0xc                                      memory reset off
poll 0x1008 bit 29 = 1  (20 ms period, give up after 4 s)   DDR init done
check 62494 <= rd(0x1210) <= 62506                          ADC clock present: after step j, see below
0x1418, 0x141c, 0x1420, 0x1424 <- 0x8000                    after the ADC calibrations, step 18
```

0x1210 counts the ADC data clock, nominal 62500. In a cold start it reads 0 until the ADC sync and the eye scan have run
(`cold-clock.md`, verified on the unit). So at step 12 it is only logged. The check that stops the boot comes after step
j. There it is the best available proof that the LMX2582 programmed correctly: the PLL has no read-back.

The meaning of 0x1428 ("DAC polling") and 0x1418.. ("AFE offset") is not known. Assumed: FPGA-side refresh of a DAC and a
per-channel offset word. Both are constants.

### 3.2 DDR skip taps from `cal_ddr.hex` (step g)

File: container as all cal files, payload 48 bytes:

| payload offset | type | field | register (mask) |
|---|---|---|---|
| 0x00 | u32 (+4 pad) | wrlvl_coarse_tap_cnt | 0x1470 [5:0] (byte shadow, bits 7:6 kept) |
| 0x08 | u64 | wrlvl_cpt_tap_cnt | 0x1474 [11:0] |
| 0x10 | u64 | mpr_cpt_tap_cnt | 0x147c [11:0] |
| 0x18 | u64 | prbs_final_dqs_tap_cnt | 0x1484 [11:0] |
| 0x20 | u64 | dq_idelay_tap_cnt | 0x148c [9:0] |
| 0x28 | u32 (+4 pad) | (always 0) | 0x1494 [11:0] |

There is no `default/cal_ddr.hex`, so skip step g if the file is missing. The registers are 16-bit RMW of shadows that
start at 0, so the written words are the plain values.

Sequence: `0x1000 <- s1000 | 0x8`, the six writes in the order of the table, `0x1000 <- s1000 & ~0x8`. OOS then polls
0x1008 bit 29 again (every 500 ms, up to 51 times) and logs the 0x14a0.. read-back. Whether the reload is needed after
the MIG's own power-on calibration is not known. OOS does it whenever the file exists.

How the file is produced (for the never-do list): the factory DDR calibration of the stock OS runs `spi2erase` +
`spi2flash /rigol/FPGA/BOOT_SelfTest.bin 0x800000`, `rmmod xdma pcie_rockchip`, `spi2boot 0x800000`,
`setprop persist.rigol.fpga.boot.addr 0x400000`, `load_pcie.sh`; then a mem-reset pulse, waits for 0x1008 bit 29 and
stores the 0x14a0.. read-back as the new file. **OOS must never do any of this.**

### 3.3 ADC sync delay sweep

Primitives:
- `sync_delay(n)`: ADC reg 0x06 = (s06 & ~0x700) | n << 8, written **without** XOR: `0x1034 <- 0x01060000 | s06`;
  s06 starts at 0x01c3, so n = 0..7 gives `0x010600c3`, `0x010601c3`, ... `0x010607c3`.
- `spi_sync(1)`: ADC reg 0x00 shadow (start 0xe4) bits 7:6 = 10 -> 0xa4, word data = shadow ^ 0xe1:
  `0x1034 <- 0x01000045`. `spi_sync(0)`: bits 7:6 = 00 -> 0x24: `0x1034 <- 0x010000c5`.
- `spu_adc_sync`: 0x1000 bit 1 pulse: s & ~2, s | 2, s & ~2 (three writes).
- `spu_reset`: 0x1000 bit 0 pulse: s | 1, s & ~1.
- ADC reg 7 read: `0x1034 <- 0x01870000`, sleep 100 us, read 0x1038, value = low 16 bits.

Algorithm:
```
for n in 0..7:
    sync_delay(n); sleep 20 ms
    spi_sync(1); spu_adc_sync; spi_sync(0); spu_reset
    r[n] = bit 1 of ADC reg 7
choose the first n in 0..6 with r[n] = 1 and r[n+1] = 0   (first falling edge 1 -> 0)
if none: the delay stays at 7 from the sweep, no final sync     <- OOS too; it stops only if all 8 reads are equal
sync_delay(n); spi_sync(1); spu_adc_sync; spi_sync(0); spu_reset
```
Example: r = 1,0,1,1,1,0,0,1 -> n = 0, s06 = 0x00c3 (verified on the unit). The sweep takes about 170 ms. The pattern changes from
start to start: 1,1,1,1,1,0,0,1, 1,1,0,1,0,1,0,0 and 0,0,0,1,1,1,1,1 were seen on one morning, the last one without a falling edge.

### 3.4 Lane eye scan and IDELAY load

Set-up:
```
data_mode(test):       0x1034 <- 0x010d550f       ADC reg 0x0d test pattern (shadow 0x650f ^ 0x3000)
output_code_zero:      0x1034 <- 0x01160044, 0x011e0044, 0x01260044, 0x012e0044,
                                 0x01360044, 0x013e0044, 0x01460044, 0x014e0044     (reg 0x16+8n, bit 15 = 0)
adc_gray(off):         0x1014 <- s1014 & ~0x8
spi_sync(1); spu_adc_sync; spi_sync(0); spu_reset                (as 3.3)
cal_select(1):         0x1014 <- (s1014 & 0xfff00fff) | 0x1000
```
Scan, t = 0..31, no sleeps:
```
0x1018 <- t;  0x1018 <- t | 0x3ffff000;  0x1018 <- t          IDELAY, all lanes
0x1014 <- s & ~4;  s | 4;  s & ~4                             IDELAY cal enable pulse
a = rd(0x1024); b = rd(0x1028)                                error words
err[t] = (a & 0xffff) + ((b & 0xffff) << 14)                  1 bit per lane, lanes 0-13 from 0x1024, 14-27 from 0x1028
ber[t] = (a >> 16) + ((b >> 2) & 0x3fffc000)                  log only
```
Only `err` is evaluated. Example: err = 0x0fffffff for t = 0, 1, 24..31, 0 for t = 3..21, partial at 2, 22, 23 (verified on
the unit).

Window search (`boot::wnd`):
1. Reference = lane 2. Walk t = 1..31 on lane 2 and take the first run (of 0s or of 1s) longer than 14 that is ended by
   a transition: length L, end position E (index of the first value after the run). Fail if none (L < 15).
   Centre C = E - (L+1)/2, polarity P = lane-2 bit at C, start S = E - L + 1, end E - 1.
2. For every lane k (two calls, lanes 0-13 and 14-27): if bit k at C equals P, count the run of P around C (down to
   index 0, up to 31) -> length n, last index e; delay = max(0, e - n/2). Else if bit k at S equals P, run from S up
   -> e, delay = max(0, e - L/2). Else if bit k at E-1 equals P, run from C+1 up -> e, n; delay = max(0, e - n/2).
   Else the lane fails (the other lanes keep their result).
3. `data_mode(normal)`: `0x1034 <- 0x010d520f` (normal data).

Example result, lanes 0..27: 12 12 13 12 13 12 12 12 12 13 11 11 13 13 12 11 13 13 13 13 13 13 12 11 12 12 12 13. Repeated
scans stay within +-1 (verified on the unit). If any lane fails, OOS loads a stored list. A flat default of 10 for every
lane is the alternative.

Load: `0x1014 <- (s & 0xfff00fff) | 0x1000`, then for lane 0..27 the three 0x1018 writes of `adc-spu-init.md` (`d`,
`d | lane bit | group bit`, `d`).

### 3.5 ADC core alignment

```
0x1014 <- (s1014 & 0x00ffffff) | 0x14000000               bits 31:24 = 0x14
data_mode(normal): 0x1034 <- 0x010d520f
spi_sync(1); 0x1000 bit-1 pulse (3 writes); spi_sync(0); 0x1000 bit-0 pulse (2 writes)
0x1014 <- (s & 0xfff00fff) | 0x1000                       cal select 1
0x1020 <- s1020 & ~0x1010                                 16-bit shadow, bits 4 and 12 off
0x1014 <- (s & 0xfff00fff) | 0x1000                       (again)
for i in 0..3: 0x1014 <- (s & ~0x300) | i << 8; p[i] = rd(0x1030)      preamble code 0..3
Da = p[0] & 0xff, Db = (p[0] >> 16) & 0xff                (warning if Da != Db)
0x1014 <- (s & 0xfff00fff) | 0x1000                       bits 9:8 stay 3
0x1020 <- (s1020 & 0xf0f0) | (Da & 3) | (Db & 3) << 8
0x1020 <- (s1020 & ~0x1010) | 1 << 4 | 1 << 12
```
Result: 0x1020 = 0x1010 (Da & 3 = Db & 3 = 0, enabled), 0x1014 = 0x14f01301 (verified on the unit). Starting from 0x00f00001
(SPU init), the sequence 3.4 + 3.5 ends exactly in 0x14f01301. Run 3.5 twice: after 3.4 and again after step 15. Without it
0x1020 stays at its power-on value on a cold FPGA. A wrong core order is invisible while all eight cores read the same
input; an interleaved 1.25 GSa/s waveform would show it (assumed).

### 3.6 ADC internal trim

```
for n in 0..7: qi_trim(n, 1): sleep 200 us; 0x1034 <- 0x01000000 | (0x13+8n) << 16 | 0x4000; sleep 800 us
               (words 0x01134000 0x011b4000 0x01234000 0x012b4000 0x01334000 0x013b4000 0x01434000 0x014b4000)
t0 = now
for n in 0..7:
    repeat: 0x1034 <- 0x01800000 | (0x16+8n) << 16; sleep 100 us; done = bit 6 of rd(0x1038)
            until done or (now - t0) > 2 s  -> error
    qi_trim(n, 0): 0x1034 <- 0x01000000 | (0x13+8n) << 16 (data 0); 200 / 800 us
```
The DHO900 has one ADC. OOS runs this for ADC 0 only, with a wall-clock timeout of 2 s, and goes on after a timeout. All
eight cores report done (verified on the unit). The result is not stored in any calibration file.

### 3.7 Stored ADC calibration of mode 0 (step d)

The list of `adc-mode.md` 5.3 "CH1 only" without the two input-mode words, from `cal_adc.hex` (`hal::dho::adc::Adc::parameter(cal, 0)`):
reference word (ADC reg 0x09), 0x1058, 8 gain words (order c0 c4 c1 c5 c2 c6 c3 c7, 80 us after each), 8 offset words
(100 us before/after), 4 sample-clock delay words (regs 0x04, 0x04, 0x05, 0x05), clock delay word (reg 0x01). With zero
delays the delay words are `0x01040080 0x01048080 0x01058080 0x01058080 0x01010000` (the first reg-0x04 word is 0x0080
because the shadow starts at 0x8000). This runs before the DDR taps and the lane calibration.

### 3.8 Sinc coefficient RAM (step 21)

Register by sel: 0 -> 0x1130, 1 -> 0x1134, 2 and 3 -> 0x1138; value `en << 31 | f << 16 | f << 8 | f` (sel 3: fixed
0x00c8c8c8 \| en << 31). Coefficient sets exist for the factors f = 2, 4, 5, 8, 10, 16, 20, 25, 32, 40, 50, 80, 100, 160,
200, with 16 f entries of 16 bits each. With load = 1 a set is written:
for phase i = 0..15: `0x1140 <- 1 << i`, then for j = 0..f-1: `0x1144 <- 0x10000000 | (sel & 3) << 24 | k << 16 | c`,
c = coef[i*f + j] (index folded to 16f-1-idx above 8f), k = (i + j) mod f for sel 0-2, j mod f for sel 3.
Start-up loads the 200 set for sel 2 and sel 3 (2 x 3216 writes). The horizontal block loads the sel 0/1 sets when its
sinc factor is > 1. Only the display-trace sinc interpolation uses this RAM. The OOS read-out of raw memory does not need
it (assumed). OOS ships no coefficient values.

### 3.9 Registers of the first configuration

After a warm hand-over these hold what the stock OS wrote, after a cold FPGA boot their power-on values. Formulas are in the
referenced docs. Write them once before the first arm, and again when their inputs change. "Missing" in section 2 applies
to the horizontal rows except 0x1130/0x1134/0x1154../0x1170/0x1174, which the pass-through writes cover, and to the WAU row.

| block | registers | values for a raw read-out (proposal) | reference |
|---|---|---|---|
| WPU read-path gain/offset | 0x14c8/0x14d4/0x14e0/0x14ec (Q10 gain), 0x14d0/0x14dc/0x14e8/0x14f4 (offset), zoom 0x1508..0x1514 / 0x14f8..0x1504 | gain = digital gain x 1024, offset = 32768 - 32768 x gain, replicated per lane like the invert bits | `afe-settings.md` 4.1 |
| WPU ADC gain | 0x105c (0x5555 for x1) | 0x5555 | `adc-mode.md` 4.3 |
| horizontal: interpolation off | 0x1130 = 0x00010101, 0x1134 (zoom, disabled), 0x1118/0x111c = points + 40, 0x1138 = 0x00c8c8c8, 0x1154 bit 0 = 1 (linear off) and bits 8/9 = 0, 0x1158 = 0, 0x115c = 0xffffffff, same for zoom 0x1160/0x1164 | as listed | `horizontal-config.md` 4.1 |
| horizontal: compression / TX | 0x1170 = 0x40000001 (k = 1) or the k of 3.1 there, 0x1174, 0x11fc = 1000, 0x11f0 (OOS window) | | `horizontal-config.md` 3.1/4.1 |
| horizontal: misc | 0x131c = (s & 0xf000) \| 0x2580044 (wave-early word 4, 4), 0x10a0 = 16 (filter control) | | |
| horizontal: delays | 0x1120/0x1124 (coarse: header write `en << 31 \| x << 30`, then one write per lane with lane select in bits 19:16 and the value in 15:0), 0x1128/0x112c (fine, same layout), 0x1290/0x1294 (LA) | values (3 x dot / 2 + deskew) not known | |
| acquisition: sample delays | 0x1080/0x1084 | 3.10 | |
| WAU (mask test off) | 0x4020 bit 15 = 0 (16-bit RMW), 0x119c bit 29 = 0, 0x1184 = 0x40000001 (mask compression off) | as listed | |
| ADC block | input mode words 0x54/0x55 and the stored calibration of the mode | force on the first RUN | `adc-mode.md` |

### 3.10 Sample delays 0x1080/0x1084

Base b = 480 / 240 / 120 for ratio 1 / 2 / 4, T = 600,000 fs on the stock system (`horizontal-config.md`), d = per-channel deskew in fs
(0 without deskew). Measured on the unit with one source on two channels, T is 800,000 fs: a step moves a channel by 1.6 ns at ratio 2,
and at ratio 4 only every second step counts, 6.4 ns. A positive d moves the trace to earlier times, under stock as well (`:CHANnel<n>:TCALibrate`, same 6.4 ns grid at 312.5 MSa/s). OOS uses
800,000.
- ratio 1: 0x1080[31:16] = b + d/T, 0x1080[15:0] and 0x1084 rewritten from their shadows -> zero deskew:
  0x1080 = 0x01e00000 \| (s1080 & 0xffff), 0x1084 = s1084 (start 0);
- ratio 2: 0x1080 = (b + d_lo/2T) << 16 \| (b + d_hi/2T) = 0x00f000f0; 0x1084 = s1084;
- ratio 4: 0x1080 = (120 + d1/4T) << 16 \| (120 + d2/4T), 0x1084 = (120 + d3/4T) << 16 \| (120 + d4/4T) = 0x00780078 each.

## 4. Proposed OOS cold-boot sequence

Keep shadows for 0x1000, 0x1014, 0x1020, 0x1428, 0x4004, 0x4008 and the ADC registers 0x00, 0x06, 0x0d, 0x13+8n, 0x16+8n
(start values of 3.3-3.6), so that every RMW is exact.

| # | step | details | status |
|---|---|---|---|
| 1 | FPGA boot | `\rBOOT\r` + 00 40 00 00 on spidev1.0; wait; `pcie-rockchip` host (retry by reload), `xdma poll_mode=1`; check 0x4000 = 0xe2072115 | verified on the unit |
| 2 | sample clock | LMX2582 table (`pll.md`) | verified on the unit |
| 3 | AFE x4 | AFE init | verified on the unit |
| 4 | ADC init | 2 x 104 words + `0x010f0201` | verified on the unit (1.1 ms / 100 us) |
| 5 | SPU init | 0x1000 1,0,0,0x10,0; 0x105c 0x55555555; 0x1010 0x20000000; **0x1014 0x00f00001** | verified on the unit |
| 6 | SCU init | 0x4004 0x80000000, 0 | verified on the unit |
| 7 | once-only | 0x1428 = 0x646e; (read 0x14a0..); 0x1000 &= ~0xc | verified on the unit |
| 8 | checks | poll 0x1008 bit 29 (20 ms, 4 s), on failure stop (no BRAM fallback); log 0x1210 | verified on the unit |
| 9 | ADC cal mode 0 | 3.7 from `cal_adc.hex` | verified on the unit |
| 10 | DDR taps | 3.2 from `cal_ddr.hex` (skip if absent); then poll 0x1008 bit 29 | verified on the unit |
| 11 | sync delay | 3.3 sweep + sync sequence; no falling edge stays at 7, all reads equal stops the boot | verified on the unit |
| 12 | lane delays | 3.4 scan, window search, load; fallback to the stored list; then check 0x1210 in 62494..62506 | verified on the unit, fallback not yet |
| 13 | core align | 3.5 | verified on the unit |
| 14 | ADC trim | 3.6, ADC 0 only, 2 s timeout | verified on the unit |
| 15 | normal data | `0x010d520f` | verified on the unit |
| 16 | core align again | 3.5, then 1 ms | verified on the unit |
| 17 | AFE offset words | 0x1418..0x1424 = 0x8000 | verified on the unit |
| 18 | bit range | 0x1014 = (s & 0xff0fffff) \| 0x00f00001 (value unchanged) | runs in every cold boot, effect not checked on its own |
| 19 | (sinc RAM) | 3.8, only if the FPGA trace sinc path is used | not done |
| 20 | AFE Hz enable | all four channels | runs in every cold boot, effect not checked on its own |
| 21 | SCU | 0x4008 bit 26 = 0, bit 24 = 0, bit 23 = 1 | runs in every cold boot, effect not checked on its own |
| 22 | first configuration | ADC input mode + stored calibration for the wanted mask (forced), AFE per channel (`hal::dho::afe`), TPU edge (`hal::dho::tpu`), acquisition block (`hal::dho::acq`) **plus 0x1080/0x1084**, the block list of 3.9, WPU gain/offset, WAU off, SCU auto trigger | runs on the unit with the gaps of section 2 |
| 23 | acquisition | `hal::dho::seq::acquire` (arm, poll 0x4080 bit 30, read-out window, read) | verified on the unit |

Time budget: ADC init about 45 ms with 100 us / 100 us (270 ms with 1.1 ms), sync sweep about 170 ms, lane scan a few ms
(no sleeps), ADC trim up to 2 s, DDR self test normally one read.

Checkpoints: after step 8 the DDR pass bit; after step 11 the r[0..7] pattern (example 1,0,1,1,1,0,0,1); after step 12 the 28
delays (the stored list within +-1) and 0x1210; after step 13 0x1030 / 0x1020 (expect 0x1010); after step 14 that all eight
cores report done; after step 22 a CH1 frame of the probe-comp signal (`adc-meas` comb, edge shape, amplitude).

### What OOS must never do

- send any boot-monitor command other than `\rBOOT\r` + address on spidev1.0 (`\rPROG\r`, the `spi2flash`/`spi2erase`/
  `spi2fpga` streams) - no FPGA flash erase or program;
- repeat the factory DDR calibration of 3.2 (it erases and programs slot 0x800000 with `BOOT_SelfTest.bin`, boots it and
  sets `persist.rigol.fpga.boot.addr`);
- write anything under `/rigol/data`: no saved calibration, no default-file creation (the stock OS writes a default
  `cal_afe_bandwidth.hex` if that file fails to load), no user calibrations that rewrite cal files. Read the files only;
- run an ADC self test (it writes test values into ADC reg 0x54) or set 0x1008 bit 31 (BRAM fallback);
- touch uboot, trust or parameter partitions, or send MCU commands other than those in `board.md`.
