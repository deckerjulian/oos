# ADC (RT8847IQ) and acquisition unit (SPU) init

Init order: AFE x4, **ADC**, **SPU**, WPU (no hardware write), SCU, front-panel status. The complete cold-boot sequence
is in `cold-boot.md`.

## ADC init (implemented as `oos-hw adc-init`, verified on the unit)

The ADC is configured through the FPGA's ADC SPI master: write register 0x1034 =
`0x1000000 | addr << 16 | data` (bit 24 start, bits 22:16 ADC register, 15:0 data). 0x1038 reads back the last word
with bit 26 set when done (seen: 0x050f0201 after init).

The init list has 104 entries of (address, value). A per-register XOR mask applies to the data:
word i = `0x1000000 | (addr[i] & 0x7f) << 16 | (mask[addr[i]] ^ value[i]) & 0xffff`.
The 104 words are written twice (1.1 ms before, 100 us after each), then ADC reg 0x0f once more: `0x010f0201`
(register value 0x7a01 & ~0xc00, XOR 0x7000). The full word list is in `hal/src/boot/regs.rs` and `tools/hw/oos-hw.c`.

## SPU init

1. 0x1000 (SPU control, 0 at start): write 0x1, 0x0, 0x0, 0x10, 0x0 (bit 0 reset pulse, bit 4 Tx reset pulse)
2. 0x105c <- 0x55555555
3. 0x1010 <- value | 0x20000000 (= 0x20000000)
4. 0x1014 <- (value & 0xff0fffff) | (0xf << 20) | 1
5. the value kept for 0x12d4 starts at 0x21b721b4 (no write here)

Register values in STOP after a complete set-up (verified on the unit): 0x1000 = 0, 0x105c = 0x55555555,
0x1010 = 0x20000000, 0x1014 = 0x14f00001 (later steps add bits), 0x4004 = 0, 0x4008 = 0x05000001.

## ADC data lane alignment (needed for valid samples) - a runtime calibration, not a file

Run after the calibration files are loaded. Details and the boot order are in `cold-boot.md` 3.3 to 3.5.
1. Sync delay sweep: step the ADC sync delay, 20 ms per step, each step ADC SPI sync + SPU ADC sync + SPU reset, read
   ADC reg 7, find the best window over the results, set the best sync delay.
2. Eye scan of the 28 LVDS lanes: ADC to test pattern (data mode 2, fixed output code), FPGA Gray decode off, ADC SPI
   sync + SPU ADC sync, SPU reset, ADC calibration select 1. For delay 0..31: set the IDELAY of all lanes to delay,
   enable the IDELAY calibration, read the bit errors per lane. Per lane, take the centre of the error-free window into
   buf[28]. ADC back to normal data (data mode 4).
3. Load the lane delays: ADC calibration select 1, then for lane 0..27 set the IDELAY of the lane to buf[lane].
   The fallback is delay 10 on every lane.

IDELAY write, register 0x1018, three writes:
`delay & 0x1ff`; then `delay | mask` with mask = 0x3ffff000 for all lanes, `(0x1000 << lane) | 0x10000000` for
lanes 0-13, `(0x1000 << (lane - 14)) | 0x20000000` for lanes 14-27; then `(delay | mask) & 0xfff`.
ADC calibration select: 0x1014 bits 19:12 = sel.

## SCU init

0x4004 <- value with bit 31 = 1 (stop), then 0x4004 <- value & 0x7ffffffb.

## First ADC data under OOS

Full sequence on the unit: `oos-fpga-up`, `oos-hw pll-init`, `afe-init all`, `adc-init`, `spu-init`, `scu-init`,
`ddr-cal`, `adc-sync 0`, `adc-idelay` (lane delays from an earlier eye scan), then `adc-meas 0` (measure unit
0x1040/0x1044/0x1048, 8 ADC cores, `self-cal.md` 2.4):

| | core avg (8 cores) | spread | hi | lo |
|---|---|---|---|---|
| normal channel settings, CH1 on the probe-comp signal | 48061..48119 / 61919..61931 | ~60 codes | 0xffc0..0xfff0 | 0x6f30..0x71f0 |
| AFE at init defaults (no V/div set) | 48805..48909 | ~100 codes | 0xc010..0xc130 | 0x0000 |

All cores agree in both cases, so the lane alignment and sync delay work (verified on the unit). The range differs because
V/div, offset and input path were not set in the second case. In that case a second measurement returned identical
numbers. Whether this was a stale result or a DC input is not yet verified on hardware.

## First waveform frame read under OOS

Warm hand-over: the stock system configures CH1 (1 V/div, offset -1.5 V, 200 us/div, edge trigger CH1 1.5 V,
probe-comp signal 1 kHz, memory depth 100 points), RUN, STOP. Then a warm reboot into OOS, with no power loss. Under OOS: **no
FPGA boot**, only `pcierockchiphost` + `xdma` are loaded. The FPGA keeps its state (0x4000 = 0xe2072115, 0x4034 = 0x2000
STOP, 0x4010 = 8 stored frames). The 28 read-out writes via `oos-hw wr` and ONE
`dd if=/dev/xdma0_c2h_0 bs=200 count=1` return the frame at once (verified on the unit).

Result: 100 words, range 21165..43507 (reference read over SCPI, `:WAV RAW WORD`: 20838..43560). The words are a 10x
linear interpolation of the captured samples: every 10th word matches the reference point within about +-100 codes of
a 22000-code step, the words in between lie on straight lines within 1.4 codes, the edge is at the same place. The
FPGA's interpolation is active at this low sample rate (50 kSa/s). At 1.25 GSa/s frames are word-identical.

This proves the complete read-out path under OOS (own kernel 6.12, xdma, register writes, C2H stream).

Going back from OOS to the stock system must be a power cycle, not `reboot`: after a warm reboot from OOS (with the FPGA
re-initialised by OOS) the stock system hangs at start (verified on the unit).

## Second frame under OOS: word-exact at 1.25 GSa/s

Same warm hand-over: memory depth 2.5 M, 1.25 GSa/s, 200 us/div, probe-comp signal on CH1.
Reference: 4000 points around the trigger (`:WAV:STAR 1248001`, `STOP 1252000`, RAW WORD). Parameters: ratio 1, FSM mode
1, memsel 0x2625a0 (2.5 M), length parameter 0x98968. Writes: 0x1100 = 0x11e8, 0x1108 = 0x130b00 (start point, not
scaled), 0x11f0 = 0xfa0. ONE `dd bs=8000 count=1` under OOS.

Result (verified on the unit): **3991 of 4000 words identical to the reference at shift 0**, same edge positions. Four
reads under OOS (fresh play-last each time) are identical to each other, so the OOS read path is exact and repeatable.
The 9 differing words are isolated: the reference is smooth there, the memory word deviates downwards (by 17 to 35000
codes). See the outlier test below.

## Repeating the read-out does not re-arm the acquisition

Repeating the 28 writes of the read-out (including the final FSM run, `0x4004 |= 1`) plus one read of 2N bytes, every 30
ms, after a warm hand-over (FPGA version 0xe2072115, `0x4034 = 0x2000` STOP), verified on the unit:

- the read path is solid: 540 frames in 18 s, no C2H error, no timeout;
- every frame is identical (0 changed words between frames) and `0x4034` stays `0x2000`: the FSM run at the end of the
  read-out sequence does not start a new acquisition.

Live frames need the acquisition start and trigger arming (`acquisition-start.md`).

## Outlier test: the direct read is word-exact under the stock system too

Same settings as the second frame (CH1 probe-comp signal, 200 us/div, 2.5 Mpts, 1.25 GSa/s, ratio 1, FSM mode 1, the
same 28 writes byte for byte) with no reboot in between: RUN, STOP, the middle 4000 words read twice over SCPI, then the
28 writes and one read of 8000 bytes from `/dev/xdma0_c2h_0`, three times.

Result (verified on the unit): all three direct reads are identical to the SCPI read, 4000/4000 words, and to each
other. So the samples are not altered on the SCPI path. The outliers appear between capture and read, during the warm
reboot into OOS: the acquisition memory is not stable across a reboot. For OOS this matters little, because OOS captures
itself. `tools/frame-check.py` filters such outliers when it compares frames.

### What the 9 outliers are: single raw-ADC bits that fell from 1 to 0

All 9 deviations are negative and come in pairs (-17 twice, -2206 twice, -35296 twice). Divided by a power of two they
give one constant:

| deviation | / 2^k | raw bit |
|---|---|---|
| 17 | 17.0 | 0 |
| 138 | 17.25 | 3 |
| 276 | 17.25 | 4 |
| 2206 | 17.23 | 7 |
| 8824 | 17.23 | 9 |
| 35296 | 17.23 | 11 |

So the acquisition memory holds the **raw 12-bit ADC codes**, and the 16-bit words of the read-out are computed on the
way out (about 17.23 output codes per raw LSB, plus an offset: gain/offset calibration in the read path,
`afe-settings.md` 4.1). Each outlier is exactly one raw bit cleared (1 -> 0), bits 0..11 all affected. Repeated OOS reads
show the identical errors, so they are in the memory, not in the read-out. A few bits flipping 1 -> 0 is the signature of
DRAM cells losing their charge while refresh pauses. With no reboot in between there is no error. Assumed cause: the
FPGA's DDR refresh pauses during the RK3399 warm reboot (not yet verified on hardware).
