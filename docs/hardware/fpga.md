# Acquisition FPGA

What a host program needs to know to drive the DHO800/DHO900 acquisition FPGA well: what it is, how it boots, how the
RK3399 reaches it, what the data path costs, how the registers behave, and what can hang the scope. It is an index
and a reference for implementers. The register sequences of each function stay in `../protocols/`; this page links
to them and states only what they do not.

Every statement carries one of the three marks defined in [../README.md](../README.md):

- **verified on the unit**: seen on a running scope. Unless named otherwise the unit is a DHO804 with the stock FPGA
  image (version 0xe2072115) under OOS, November 2026. "OOS code" after a statement is short for "verified on the unit
  (OOS code: the named file)": OOS relies on the fact, and the feature that uses it works on the scope. "With the
  OOS fork" means the code that showed it is in a fork of OOS and not in this tree.
- **assumed**: concluded, not observed.
- **not yet verified on hardware**: written down or coded, never run on a scope (host tests at most).

## Overview

| Part | What | How known |
|---|---|---|
| FPGA | Xilinx Zynq-7000 XC7Z015, CLG485 package. PS: two Cortex-A9; only the SPI boot monitor runs there. PL: the whole acquisition design | part marking; `../protocols/fpga-image.md` |
| Flash | 16 MiB QSPI flash on the Zynq side, three boot images (slots 0x000000, 0x400000, 0x800000) | verified on the unit (external reader, 2026-11-12) |
| Host link | PCIe endpoint (Xilinx XDMA) to the RK3399; register window and one card-to-host DMA channel | verified on the unit |
| Boot control | SPI from the RK3399 to the monitor on the Zynq PS | verified on the unit, `../protocols/fpga-boot.md` |
| ADC | RT8847, eight interleaved cores, 1.25 GSa/s together | verified on the unit, `../protocols/adc-spu-init.md`; 156.25 MSa/s per core is assumed (Clocks and time units) |
| Sample clock | LMX2582, programmed by the RK3399 | verified on the unit (`../protocols/pll.md`) |
| Front end | one RT1642 per analog channel (four on four-channel models), programmed by the RK3399, not through the FPGA | verified on the four-channel unit (`../protocols/afe-spi.md`); two-channel models not yet verified on hardware (`../models.md`) |
| Acquisition memory | DDR3 on the PL side; holds at least 125 M points; a segment of 2^24 bursts was accepted | verified on the unit: 100 M and 125 M records without wrap (`../protocols/acquisition-start.md`) |
| Generator DAC | DAC behind the FPGA (DHO900 S models with the generator fitted) | verified on a DHO924S (`../protocols/afg.md`) |
| Logic analyser | 16 inputs into the FPGA (DHO900 / LA option) | verified on a DHO924S (`../protocols/la.md`) |

### Functional block map

```
           LMX2582 (1.25 GHz)          RK3399: AFE SPI, PLL SPI, GPIO
                 |
 RT8847 ---> capture, lane alignment ---> per-core gain (0x1058, 0x105c) ---> decimation / peak (0x10d0, 0x10d8, 0x1004)
 8 cores        (0x1000-0x103c)                |                                     |
                                               +--> measure unit (0x1040-0x1048)     v
                                               +--> DVM (0x2240-0x2248)        acquisition memory (DDR3)
                                               +--> trigger unit TPU (0x2000-0x22ff)  |   frame ring, segments
                                                      |                              v
                                                SCU FSM (0x4000-0x40ff) <---> play chain: window (0x1100, 0x1108),
                                                      |                       compression (0x1170), sinc/linear
                                                AUX OUT (0x1430)              interpolation, read-path gain (0x14c8),
                                                mask / zone (0x1184-0x11dc)   framing (0x11f0, header 0x1010)
                                                                                     |
                                                                         XDMA C2H channel 0 ---> /dev/xdma0_c2h_0
 registers: BAR1 (32 KiB) <--- /dev/xdma0_bypass (mmap)                       AWG (0x3000-0x3fff) ---> DAC
```

The names are OOS's. The block boundaries are what the register sets and OOS's sequences show, not a published
design.

### Register ranges

| Range | Unit | Reference |
|---|---|---|
| 0x1000-0x10ff | SPU: resets, ADC data path, lane alignment, gains, rate dividers, memory set-up | `../protocols/adc-spu-init.md`, `../protocols/adc-mode.md`, `../protocols/horizontal-config.md`, `../protocols/cold-boot.md` |
| 0x1100-0x11ff | play chain and read-out, mask and zone units | `../protocols/record-play.md`, `../protocols/horizontal-config.md`, `../fork/hwmask.md`, `../fork/hwzone.md` |
| 0x1200-0x12ff | status and counters (0x1208, 0x1210), LA | `../protocols/cold-clock.md`, `../protocols/la.md` |
| 0x1400-0x14ff | front-end side controls, AUX OUT select, read-path gain and offset | `../protocols/afe-settings.md`, `../fork/auxout.md` |
| 0x2000-0x22ff | trigger unit (TPU), serial receivers, counter, DVM | `../protocols/trigger.md`, `../protocols/counter.md`, `../fork/dvm.md`, `../fork/hwuart.md`, `../fork/hwserial.md` |
| 0x3000-0x3fff | AWG | `../protocols/afg.md` |
| 0x4000-0x40ff | SCU: version, FSM, channel mask, frame ring, auto trigger, status, time stamps | `../protocols/acquisition-start.md`, `../protocols/roll-mode.md` |

## Boot and image

Sequences: `../protocols/fpga-boot.md` (BOOT command, bring-up order, lost BOOT after power-on) and
`../protocols/fpga-image.md` (monitor, PS configuration, bitstream properties).

- After power-on the Zynq loads the factory image from slot 0x000000. The RK3399 then sends `BOOT 0x400000`, and the
  PS resets into the main image. The PL is reconfigured, so the PCIe endpoint disappears and comes back. Verified on
  the unit.
- 0x4000 reads the image version. OOS accepts only 0xe2072115 (`hal/src/dho/bus.rs`, `ui/src/board/dho/fpga.rs`) and
  treats any other value as "BOOT not heard" (`hal/src/dho/boot/io.rs`). OOS code. On another unit 0x4000 read
  0xe1060816 when BOOT was lost; that this is its factory image version is assumed (`../protocols/fpga-boot.md`).
  Whether the factory image is the same build on every unit is not known; the check does not depend on it.
- The monitor hears BOOT only after the factory image has started, about 11 s after power-on. A BOOT sent earlier is
  lost. OOS repeats it up to twice, each time with xdma and the PCIe host driver unloaded first
  (`hal/src/dho/boot/io.rs`). Verified on another unit with serial consoles on both sides
  (`../protocols/fpga-boot.md`).
- A warm restart of the RK3399 does not reset the Zynq. The main image keeps running with its register state, its
  DMA engine state and its frame memory. Verified on the unit (BOOT is heard at once, version correct at the first
  try).
- OOS sends only `BOOT`, only to slots 0x0 and 0x400000 (`hal/src/dho/boot/regs.rs`). It never writes the flash and
  never writes 0x1008. OOS code.
- Reading slot 0 back over SPI is not possible: the monitor transmits nothing. A backup of the factory slot needs an
  external flash reader. Verified on the unit.

## Clocks and time units

Only what is confirmed on the unit or by OOS's working use of a register. The clock structure inside the PL is not
documented here.

| Clock or unit | Value | How known |
|---|---|---|
| ADC sample clock | 1.25 GHz from the LMX2582 | verified on the unit (`../protocols/pll.md`) |
| ADC core rate | 156.25 MSa/s per core (1.25 GSa/s / 8) | assumed from the ADC clock and the eight cores; the rates 1.25 G, 625 M and 312.5 MSa/s give a correct time axis on the unit (`../protocols/acquisition-start.md`, time axis table) |
| Slow rate path | rate = 62.5 MSa/s / 0x10d8 at and below 62.5 MSa/s | verified on the unit: 1 kHz measured as 1000.000 Hz at 10 M points, 62.5 MSa/s (`../protocols/acquisition-start.md`, time axis table) |
| Time stamps 0x40b0-0x40c4 | 0.8 ns per tick | verified on the unit: 1 ms spacing reads as 1 250 000 ticks (`hal/src/dho/seg.rs`, `LATCH_TICK_FS`) |
| Sample delay 0x1080/0x1084 | one step moves a channel by 1.6 ns at 2 channels; at 4 channels every second step moves it, by 6.4 ns | verified on the unit (one source on two channels, `hal/src/dho/acq.rs`) |
| TPU event limits t1/t2 (A, B, R events, from 0x2020) | 1.6 ns per tick | OOS code: pulse, window, timeout and delay triggers use it (`hal/src/dho/tpu.rs`, `TICK_FS`; `../protocols/trigger.md`) |
| TPU holdoff 0x2084/0x2088, 0x4068, 0x406c, AUX pulse 0x119c | 6.4 ns per tick | assumed; the holdoff is in use on the unit, its exact unit not measured |
| Auto trigger 0x4060/0x4064 | 4 ns per tick | assumed; auto trigger works on the unit with OOS's values, the unit not measured |
| Measure unit 0x1040 | one sample per core per 156.25 MHz clock; e = 18 gives 2^18 samples, 1.68 ms | assumed; the run completes on the unit, its length not measured |
| ADC data clock counter 0x1210 | about 62 500 when the clock runs, 0 before ADC sync and lane training | verified on the unit (`../protocols/cold-clock.md`); what exactly it counts is not confirmed |

## PCIe and XDMA

### Link

| Item | Value | How known |
|---|---|---|
| Endpoint | 0000:01:00.0, `10ee:7022`, class 0x070001; root port `1d87:0100` | verified on the unit (lspci) |
| Width | x2 (the RK3399 port is x4 capable, `num-lanes = 4`) | verified on the unit |
| Speed | endpoint capable of 5 GT/s; this tree trains 5 GT/s (`max-link-speed = <2>` in `kernel/dts/rk3399-rigol-dho900.dts`), `<1>` caps it at 2.5 GT/s | verified on the unit: 5 GT/s (`../protocols/fpga-boot.md`), 2.5 GT/s with the OOS fork (2026-11-15) |
| Mainline kernel | commit 84d08ff902f1 drives the RK3399 port at 2.5 GT/s (in 6.18.63, queued for 6.12 stable) | from the commit, not a unit fact |
| BAR0 | 64 KiB, XDMA engine registers (`/dev/xdma0_control`) | verified on the unit |
| BAR1 | 32 KiB, the FPGA register window (`/dev/xdma0_bypass`) | verified on the unit |
| MaxPayload / MaxReadReq | 256 B / 512 B with `pci=pcie_bus_perf` on the kernel command line (the stock system ran 128 B), ASPM off | verified on the unit with the OOS fork |
| Interrupts | one MSI vector offered; MSI-X not offered; the stock system used legacy INTx | verified on the unit |
| Reset | no FLR; a warm reboot of the RK3399 does not reset the endpoint | verified on the unit |
| Other capabilities | MSI capability at config offset 0x48 (enable visible at 0x4a: 0x0081 with MSI on); device serial number capability present, value 0; no AER | verified on the unit (lspci, config space) |

The case for the cap: the RK3399 datasheet gives 2.5 GT/s for its PCIe port, Rockchip warns that 5 GT/s can fail,
and mainline Linux drives the port at 2.5 GT/s from 6.18.63. Measured cost on the unit with the OOS fork
(2026-11-15, same build, only the cap changed): none. Setup B (CH2, 200 ns/div, about 2400 triggers/s): 30 fps,
about 2420 acquisitions/s, C2H read p50 5.0 ms at both speeds. Four channels, 10 M points, 1 ms/div: read p50
282.9 ms at 5 GT/s, 282.3 ms at 2.5 GT/s.

### Completion: MSI, poll, never INTx

| Mode | Where | Result on the unit |
|---|---|---|
| MSI (`xdma.params = poll_mode=0`) | normal boots, opt-in in `system.conf` | works; 31 interrupts/s at setup B, 8/s at 4 ch 10 M; rates equal to poll mode within 2 %, one A53 5-20 points less busy |
| Poll (`poll_mode=1`) | default without `xdma.params` (`oos-fpga-up`, `oos-fpgad`); forced in a kexec session by the OOS fork | works; one `cmpl_status_th` per channel bound to cpu0/cpu1 burns an A53 during each transfer |
| Legacy INTx (`interrupt_mode=2`) | rejected | about 230 000 interrupts/s on cpu0, C2H time-outs; the revert by warm reboot ended in a halted panic, only a power cycle recovered |
| MSI after kexec | rejected | every C2H read times out (0x4034 = 0x21c0); the interrupt translation tables of the previous kernel stay in force (assumed cause) |

So a kexec session has to load xdma with `poll_mode=1` whatever the configuration says. The OOS fork forces it;
`oos-fpga-up` and `oos-fpgad` in this tree take `xdma.params` as given. The MSI lands on cpu1 by default; with
`perf.place = on` `oos-irq-place` pins every xdma IRQ to the A53s. Verified on the unit with the OOS fork.

### Device files and driver behaviour

- `/dev/xdma0_bypass`: mmap 0x8000 bytes = BAR1. OOS code (`hal/src/dho/bus.rs`).
- `/dev/xdma0_c2h_0`: blocking `read()`; the driver times out after 10 s (`kernel/modules/xdma`). A transfer left
  running when the reading process dies times out 10 s later in the kernel. Verified on the unit.
- An open BAR1 mapping keeps `rmmod xdma` from working. Unmap before unloading. Verified on the unit.
- Never send BOOT while xdma or the PCIe host driver is loaded: reconfiguring the FPGA behind a bound endpoint can
  lock the CPU (`../protocols/fpga-boot.md`). The caller must unmap BAR1 and unload both. OOS checks this only in
  part: the BOOT repeat unloads both and stops if either stays loaded (`pcie_down` in `fpgad/src/main.rs`); the
  `cold-boot` and `fpga-boot` commands refuse only when xdma is loaded, and `--force` skips that check; `fpga_boot`
  in `hal/src/dho/boot/io.rs` checks nothing. OOS code.
- If the link does not train, the RK3399 host driver cannot retrain without a reload. `oos-fpgad` makes at most six
  attempts: each polls for the endpoint every 20 ms for up to 2 s, a failed one unloads the driver and waits 2 s.
  `oos-fpga-up` does the same with fixed 2 s waits. OOS code; the retry timing is not measured on the unit.

## Throughput and what limits it

Measured on the unit with the OOS fork (2026-11-15 and 2026-11-16, its `:SYSTem:STATe?` counters):

| Case | Bytes per read | C2H read | Rate |
|---|---|---|---|
| setup B, 1 ch, 200 ns/div, batch | about 4 MB plan, trigger-bound | p50 5.0-5.5 ms | 30 fps, 2400-2440 acquisitions/s |
| 4 ch, 10 M points, 1 ms/div | 80 MB (4 x 10 M x 2 B) | p50 236-283 ms | about 280-340 MB/s, 3-3.7 acquisitions/s |
| batch of 31 frames, 12 500 points, 1 ch | 775 000 B | 8.6 ms | about 90 MB/s (per-read cost dominates) |
| 1 M points full, `--all` | 2 MB | not timed | |

- The link is not the limit: 2.5 GT/s x2 carries 500 MB/s before packet overhead, and the read rate does not
  change between 2.5 and 5 GT/s. Verified on the unit.
- At deep records the DMA read itself is the limit, not the CPU (verified on the unit with the OOS fork). The
  driver pins and maps the user pages of every read (1024 pages per 4 MB); a persistent mapping is the open
  improvement. Assumed cost 0.5-2 ms per 4 MB read, not measured.
- Reading a deep record completely is not an option for a display: let the play chain compress it (below).
- Each BAR1 write costs a 10 us pause in OOS (`gap_us` in `hal/src/dho/bus.rs`). With the default 50 us timer
  slack the pause lasts 60-70 us, so a full configuration of about 230 writes takes about 15 ms instead of 2.3 ms.
  Measured on the unit.
- Roll and scan stream chunks of 216 bytes; OOS reads every whole chunk available and sleeps 9 ms when none is
  (`../protocols/roll-mode.md`). Roll is capped at 156.25 MSa/s per stream (`hal/src/dho/roll.rs`).

## Data formats

### Samples on the C2H stream

- One 16-bit little-endian word per sample. Mid-scale is 0x8000. Verified on the unit.
- Channels interleave word by word: 1 channel `s[i]`; 2 channels lower-numbered channel at even words; 3 or 4 channels
  `s[4i + k]` = CH k+1 (3 channels run as 4). Verified on the unit (`../protocols/horizontal-config.md` 2.2).
- A live frame has a 16-byte header unless 0x1010 bit 31 is set. Header words 5..7 hold a 48-bit time stamp; in a
  segmented recording the header of segment k carries the latch of segment k-1 (`hal/src/dho/seg.rs`). Verified on
  the unit.
- **Each frame is padded to 8 bytes in the stream.** N frames of b bytes arrive as N x roundup8(b) bytes; 1250 points
  come at a stride of 1252. Verified on the unit (`../protocols/acquisition-start.md`, C2H read rules).
- LA samples: one 16-bit word per sample, bit n = Dn, never mixed with analog words in one frame
  (`../protocols/la.md`).

### Frame size and the TX length

- 0x11f0 = points per channel to send. A frame of a read ends only when the window can supply all 0x11f0 points;
  if it cannot, the frame never ends and the read times out. A value of 8 is replaced by 128. OOS code
  (`hal/src/dho/scu.rs`), verified on the unit.
- Read sizes per path: `../protocols/record-play.md` 1.6.

### Compression (0x1170 main, 0x1174 zoom)

- 0x1170 = 0xc0000000 | k turns on min/max compression; 0x40000001 is pass-through. OOS code (`hal/src/dho/seq.rs`).
  That k is 14 bits wide (at most 16383) is assumed; OOS keeps k within it.
- Each output pair holds the minimum and the maximum of 2k consecutive stored samples, in the order they occur in
  time, so a pair is (min, max) or (max, min). Blocks are contiguous and start at the window start. Verified on the
  unit for k = 3, 4, 15, 16, 64 and 128 (`hal/src/dho/pairs.rs`), and one compressed point per k samples for
  k up to 200 (`../protocols/acquisition-start.md`).
- k = 0 with the enable bit set is assumed to give no output, so the read would stall. OOS never writes it.
- The compressed length must be a multiple of 8 with points x k <= record length. Asking for more stalls the read
  (k = 256 on a 1 M record timed out twice with 3912 points). Verified on the unit with the OOS fork: with the length
  rounded down (3904 points, `compressed_len` in `hal/src/dho/seq.rs`) k = 256 at 1 M read without a time-out, and
  `oos-fpgad compress-check` passed at k = 4, 16, 64 and 256.
- Compression acts on stored samples: decimation and peak detect happen before the memory, so k counts stored points.
  Assumed.

### Sample rate, decimation and peak detect

- Two rate paths: above 62.5 MSa/s 0x10d0 = 0x8000 | N with N = per-channel maximum rate (1.25 G / 625 M /
  312.5 MSa/s for 1 / 2 / 4 channels) / rate, 0x10d8 = 1; at and below 62.5 MSa/s 0x10d0 = 0 and
  0x10d8 = 62.5 MSa/s / rate (`../protocols/horizontal-config.md` 1.7). OOS code (`hal/src/dho/spu.rs`).
- 0x1004 bit 1 = peak detect, bit 3 = average. OOS code (`hal/src/dho/acq.rs`).
- Which ADC cores feed consecutive samples depends on the rate. The table in `../fork/core-offset.md` is assumed, not
  checked on hardware. A single core's offset is visible per slot through the measure unit: on the unit one slot stood
  out clearly (`:DEBug:SLOTs?` with CH3 alone and open, at 1.25 GSa/s).

### Memory and segments

- Memory unit: a burst of 10 stored sample words, counted over all channels in memory together. 0x10e0 = segment
  size in bursts, sized from points per channel x ratio (1, 2 or 4 channels in memory) plus 384 words of guard,
  rounded up to a power of two (`mem_size` in `hal/src/dho/hz.rs`). 0x10e4 = record depth in points per channel
  (`memsel`, written unchanged by `hal/src/dho/acq.rs`). Average mode sizes its segment in the same unit with about
  twice the bursts: 64 bursts per block of 320 points (32 normal bursts), plus two blocks of margin
  (`mem_size_avg`). The "32" in `../protocols/horizontal-config.md` 1.4 is that block size, not a burst size. OOS code. 10 words per burst is assumed from the code's memory arithmetic, which works.
- With a segment of 7 864 320 bursts a deeper record wraps inside its segment (the phase jump lands where the
  arithmetic predicts); with a segment of 2^24 bursts 100 M and 125 M records are complete. Verified on the unit.
- Frame ring: 0x4010 reads back the last recorded frame. A batch waits until 0x4010 - first + 1 reaches the wanted
  count, then plays exactly that range in one read. The done flag 0x4080 bit 30 comes with the first frame, not with
  the batch. Verified on the unit (`../protocols/acquisition-start.md`, Batches).
- Time stamps 0x40b0/0x40b4 (latched), 0x40c0/0x40c4 (first trigger), control 0x40b8: one tick is 0.8 ns. Verified on
  the unit (1 ms trigger spacing read as 1 250 000 ticks).
- In a segmented recording 0x4034 is not a reliable state: count segments instead (`hal/src/dho/seg.rs`). OOS code.

### Channel to core and slot mapping

Three units see per-core or per-stream data. Their numbering differs:

| Unit | 1 channel | 2 channels | 3 or 4 channels | How known |
|---|---|---|---|---|
| measure unit 0x1040 slot k (normal mode) | all 8 | lower channel 0-3, higher 4-7 | CH1 0/2, CH2 1/3, CH3 4/6, CH4 5/7 | verified on the unit for CH1/CH2 in 4-channel mode (2026-11-13); the rest OOS code (`hal/src/dho/levels.rs`) |
| measure unit in core-order mode (self-cal) | | | channel = cores ch and ch+4; core order 0, 2, 1, 3, 4, 6, 5, 7 | verified on the unit (`../protocols/self-cal.md` 14.3) |
| mask/zone source 0x1190[23:20] (slot index s; the field takes 1 << s) | slot 0 | lower 0, higher 1 | CHn: slot n-1 | verified on the unit at 1, 2 and 4 channels (`../fork/hwmask.md`, round 5) |
| DVM lane 0x2244[9:8] | channel - 1 | lower 0, higher 2 | channel - 1 | verified on the unit for CH2 in 2-channel mode (`../fork/dvm.md`) |

The two measure-unit rows describe different ADC modes and have not been compared in one session. A host that needs
per-core values should find the mapping at run time, as `hal/src/dho/coretrim.rs` does.

## Function blocks

### ADC capture and lane alignment

Init, lane eye scan, IDELAY load and core alignment: `../protocols/adc-spu-init.md` and `../protocols/cold-boot.md`
3.3-3.6. Core gain and fine gain: `../protocols/adc-mode.md`, `../protocols/self-cal.md` 14. The FPGA has per-core
gain trims (0x1058, 0x105c) and no per-core offset; per-core offset is an ADC register (`../fork/core-offset.md`).
0x105c holds two bits per core: 0x5555 is x1 on all cores, 0xaaaa x2. OOS code (`hal/src/dho/wpu.rs`,
`wpu_gain` in `hal/src/dho/adc.rs`).

0x1210 counts the ADC data clock and reads 0 until the ADC sync and lane training ran (`../protocols/cold-clock.md`).

### Measure unit (0x1040-0x1048)

- 0x1040: [4:0] exponent e, bit 31 start, bit 30 done (read), slot k written to bits 11:8 (OOS writes 0..7 only).
- 0x1044: mean of 2^e samples of the slot on the 16-bit read-out scale. 0x1048: 12-bit max [27:16] and min [11:0].
- 0xeeeeeeee means "not ready": read again. OOS waits for the done bit instead of timing the run.
- The unit taps after the per-core gain select 0x105c. OOS uses e = 18 for levels and e = 15 for self-cal.
OOS code (`hal/src/dho/levels.rs`, `hal/src/dho/adc.rs`), verified on the unit. The run length of e = 18 (1.68 ms)
is assumed.

### Trigger unit (TPU, 0x2000-0x22ff)

Register map, edge trigger, other types: `../protocols/trigger.md`. Serial receivers: `../fork/hwuart.md` (UART,
mux 4), `../fork/hwserial.md` (SPI mux 12, I2C mux 13); not yet verified on hardware. Counter:
`../protocols/counter.md`.

- The TPU registers are write-only and read back 0 (0x2034 verified on the unit). Keep a shadow of every word and
  write whole words from it. OOS code (`hal/src/dho/tpu.rs`).
- The force pulse is 0x205c = shadow | 0x20, then shadow. A force built from a constant switches the trigger mux
  (serial receivers included). Not yet verified on hardware (`force` in `hal/src/dho/tpu.rs`).
- TPU time units differ per register. The event limits t1/t2 of the A, B and R events (pulse, window, timeout and
  delay triggers, 0x2020 onwards) count 1.6 ns. Holdoff (0x2084/0x2088), the R timeout (0x2068/0x206c) and the delay
  time (0x2060/0x2064) count 6.4 ns. OOS code (`TICK_FS` and `TICK64_FS` in `hal/src/dho/tpu.rs`, register map in
  `../protocols/trigger.md`).
- The serial receivers, the AUX pulse and 0x4068/0x406c count 6.4 ns (156.25 MHz); the auto-trigger times
  0x4060/0x4064 count 4 ns. Assumed (Clocks and time units).

### Mask unit (0x1184-0x11b4, 0x4020 bits 12-15)

Verified on the unit with OOS's hardware mask test (`../fork/hwmask.md`, round 5: 0 fails in about 1800 clean frames,
every frame fails with the delay shifted, at 1, 2 and 4 channels):

- Table port 0x1194: [7:0] lower limit, [15:8] upper limit, [25:16] column, bit 26/27 lower/upper enable, bit 28
  write enable. Write 1000 columns with bit 28 set, then 0x1194 = 0. Limits are 8-bit, on the read-out word >> 8.
- 0x1184: samples per column in [9:0], bit 31 when more than one, bit 30 set. Only bits 9:0 and 15 act, so records
  above 1023 x 1000 points mis-count columns (assumed from the failing 2.5 M case in round 2).
- 0x1190: [23:20] one-hot source slot; bit 28 rising edge latches the counters; bits 29/30 clear the fail/frame
  counters (level, write 1 then 0).
- 0x11a0/0x11a4 frames tested, 0x11b0/0x11b4 frames failed, 48 bits each, read after the latch.
- 0x4020: bit 15 test enable, bits 14/13 keep pass/fail frames, bit 12 stop on fail.
- The comparator sees the read-out word >> 8 (after the read-path gain), not the value before the digital gain.
  Verified on the unit (round 5: the read-out domain passes, the pre-gain domain fails).
- Column strobe: one per stored record sample at 200 us/div and 1 ms/div (the time bases tested). The largest count
  per column that runs is record points / 1000; one more stalls. Verified on the unit with the OOS fork (round 19).
- The table is compared about 6.5 record samples earlier than the software trigger point puts the trace: the OOS
  fork shifts the table by that constant. Measured 6.2-6.8 at 20 us, 200 us and 1 ms/div with 10k and 100k points,
  with the OOS fork (round 20).
- After a stall the read-back stays in state 5 across a reconfiguration. Verified on the unit with the OOS fork
  (round 20). It cleared during the software batches of a block; that one batch without the unit is enough is assumed.
- The column window runs on a read-back of the stored record. If it never completes, the FSM waits in state 5
  (0x4030[3:0]) and every arm ends with "no acquisition". OOS's guard drops the unit after two such arms
  (`ui/src/board/dho/hw.rs`). Verified on the unit (`../fork/hwmask.md`, round 8).

### Zone unit (0x11d0-0x11dc)

Seen on the unit (round 3, `../fork/hwzone.md` "Rate"): with OOS's tables the zone qualifies the trigger correctly in
every tested case (box on or above the trace, intersect and not intersect, a narrow box on an edge, the last
division, two zones); blocked frames are not accepted. What this confirms:

- Table ports 0x11d8 (zone 1) and 0x11dc (zone 2): [7:0] lower limit, [15:8] upper limit, bit 16 column belongs to the
  zone, [27:18] column, bit 28 write enable. Limits on the same 8-bit scale as the mask.
- 0x11d0: bits 24/25 zone 1/2 on, bits 26/27 mode (1 = pass when touched, 0 = pass when not touched).
- 0x4020 bit 1 arms the zone logic. The zone uses the mask unit's source slot 0x1190[23:20] and column count 0x1184;
  it writes no slot of its own.

Not working: the zone frame counter (0x11a0) stays 0 (round 20, with the OOS fork), and the pass rate is far below the
software zones (29 against 187 frames/s in round 3, 141 against 777 in round 20 with the OOS fork). Keep it opt-in
(`../fork/hwzone.md`).

### AUX OUT (0x1430, 0x119c)

0x1430[2:0] selects the rear BNC source: 0 trigger out (power-up value), 2 constant low, 3 pass/fail pulse configured
by 0x119c. Details and the check procedure: `../fork/auxout.md`. Not yet verified on hardware.

### DVM (0x2240-0x2248)

Sums 2^n samples of one lane; result layout measured on the unit. `../fork/dvm.md`.

### Hardware average (0x1004 bit 3, 0x10b0, 0x10b4)

0x10b0 = N - 1 for N >= 2 (OOS writes at least 1, so N = 1 and 2 both write 1; a value of 0 is untested), 0x10b4 =
clear pulse with the FSM stopped, 0x1004 bit 2 set in average mode. Verified on the unit (round 4,
`hal/src/dho/acq.rs`): without bit 2 every averaged frame above 62.5 MSa/s (156.25 MSa/s and up) is
zero; with it averaging works at 156.25, 312.5, 625 and 1250 MSa/s (312.5 MSa/s: noise 1.08 -> 0.46 mV at N = 16). At
62.5 MSa/s the noise falls 1.07 -> 0.72 / 0.53 / 0.42 / 0.40 mV at N = 2 / 4 / 16 / 256 (100 mV/div): a floor near
N = 16 that scales with V/div; that it is the per-core offset pattern is assumed (`../fork/core-offset.md`). OOS keeps
the software average as default.

### Roll and scan, LA, generator

`../protocols/roll-mode.md`, `../protocols/la.md`, `../protocols/afg.md`.

## Registers OOS relies on

Registers whose function OOS uses, with how that use is confirmed. Sequences and values: the linked pages. "Works on
the unit" means OOS's feature that depends on the field behaves correctly on the scope; it does not prove every bit.

| Register | Field | Meaning | Confirmation |
|---|---|---|---|
| 0x1000 | bit 0, 4, 5 | SPU, TX and pre-processing reset pulses (part of the stop sequence) | works on the unit |
| 0x1004 | bit 1, bit 3, bit 2 | peak detect, average, play-side mode needed by average above 62.5 MSa/s | works on the unit (bit 2: averages are all zero from 156.25 MSa/s up without it) |
| 0x1004 | [19:16], [23:20], bit 26 | per-lane invert, per-lane 20 MHz limit, ADC order | works on the unit |
| 0x1008 | bit 29 | DDR initialised (polled at cold boot) | verified on the unit (`../protocols/cold-clock.md`) |
| 0x1010 | bit 31, bit 28 | frame header off; pulse before reading the roll fill level | works on the unit |
| 0x1034 | | ADC configuration word port | works on the unit (`../protocols/adc-spu-init.md`) |
| 0x1040-0x1048 | | measure unit (section above) | works on the unit |
| 0x1058, 0x105c | | per-core fine gain, per-core x1/x2 select | works on the unit |
| 0x1080, 0x1084 | | per-channel sample delay | measured on the unit (Clocks) |
| 0x10b0, 0x10b4 | | average count - 1, average clear pulse (FSM stopped) | works on the unit at 62.5 MSa/s and, with 0x1004 bit 2, above |
| 0x10d0, 0x10d8 | | rate divider, two paths | works on the unit |
| 0x10e0, 0x10e4 | | segment size in bursts of 10 stored words over all channels together (Memory and segments); record depth in points per channel | works on the unit (no wrap at 2^24 bursts) |
| 0x1100, 0x1108 | | play window length and start | works on the unit |
| 0x1170, 0x1174 | | compression, main and zoom | verified on the unit (main) |
| 0x11e8 | | roll/scan FIFO fill level in 8-byte units | works on the unit |
| 0x11f0 | | points per channel per frame to send | works on the unit |
| 0x11fc | | points per chunk: 100/ratio in roll and scan, 1000 otherwise | works on the unit |
| 0x1204, 0x1208 | | scan trigger position and flag; recorded points after a roll stop | verified on the unit with the OOS fork (scan mode run after a cold boot, 1 kHz on CH1) |
| 0x12d4, 0x12dc | | LA threshold DAC codes; live LA comparator state | 0x12dc and the zero codes in 0x12d4 verified on a DHO924S; a threshold sweep had no effect there, under stock as well (`../protocols/la.md`) |
| 0x1430 | [2:0] | AUX OUT source; power-up value 0 | not yet verified on hardware (`hal/src/dho/auxout.rs`) |
| 0x2000-0x208c | | TPU levels, sources, coupling, holdoff | works on the unit (`../protocols/trigger.md`) |
| 0x205c | [4:0], bit 5, bit 12 | trigger type mux, force pulse, coupling-filter path | [4:0] and bit 12 work on the unit; the force pulse (bit 5) is not yet verified on hardware |
| 0x2244, 0x2248 | | DVM configuration and latch | layout measured on the unit (`../fork/dvm.md`) |
| 0x4000 | | image version | verified on the unit |
| 0x4004 | bit 0, bit 31, bit 2 | FSM run, stop, play-last pulse | works on the unit |
| 0x4008 | [3:0], 17, 22, 23, 24, 26 | channel mask, roll, scan, channel processing, LA stream select, LA enable | works on the unit (LA on a DHO924S) |
| 0x4010-0x4018 | | frame ring: first, last; 0x4010 reads the last recorded frame | verified on the unit |
| 0x4020 | bit 0, bit 1, bits 12-15 | auto trigger, zone arm, mask test controls | works on the unit |
| 0x4030 | [3:0] | FSM state | verified on the unit (state 5 during a mask stall) |
| 0x4034 | bits 4, 6, 7, 13 | waiting for trigger, sticky trigger flags, stopped | verified on the unit |
| 0x404c, 0x4050, 0x4054 | | pre-sample compensation, pre- and post-trigger points | works on the unit |
| 0x4058, 0x40d0 | | scan trigger position; scan sweep done | 0x40d0 verified on the unit with the OOS fork; a non-zero 0x4058 not yet |
| 0x4080 | bit 30, bit 31 | acquisition done; play-info clear pulse | verified on the unit |
| 0x40b0-0x40c4, 0x40b8 | | time stamps and their latch control | verified on the unit (tick, count) |

## Access rules

- 32-bit accesses only, 4-byte aligned, offsets below 0x8000. OOS asserts this (`hal/src/dho/bus.rs`).
- Most control registers are write-only or read back something else. Keep shadows (0x1000, 0x1004, 0x1010, 0x4004,
  0x4008, 0x400c, 0x4020, 0x4080, 0x40b8, all TPU words) and never read-modify-write through the register. OOS code
  (`hal/src/dho/scu.rs`, `hal/src/dho/tpu.rs`).
- **No reads of 0x205c.** A project rule for every tool that runs next to oos-ui. The TPU reads back 0, so a read
  followed by a write clears the trigger mux and force bits. Whether a read alone has a side effect is not known.
- **No hand-written FPGA register writes while oos-ui runs.** oos-ui keeps shadows and assumes it is the only writer;
  BAR1 has no lock between processes, and `oos-fpgad` maps it separately. Stop oos-ui first.
- Configure with the FSM stopped. OOS polls 0x4034 bit 13 (stopped) before the acquisition block, the average clear
  and the mask/zone blocks (`hal/src/dho/seq.rs`). Whether a write while acquiring is safe has not been tested. The
  TPU block is written before the stop poll in OOS, and a poll time-out is logged, not fatal: both work on the unit.
- Pulses are two writes: set, then clear (resets in 0x1000, 0x4080 bit 31, 0x10b4, 0x1190 bit 28, the force bit).
- Anything that writes the FPGA while a batch is armed ahead must disarm first (`hal/src/dho/seq.rs`). OOS code.
- Status 0x4034: bit 13 stopped, bit 4 waiting for trigger, bits 6/7 sticky; clear by writing the last value |
  0xc0000000, then the value (`../protocols/acquisition-start.md`).
- The LA read changes the stream select. If it fails, restore the analog stream before the next read
  (`hal/src/dho/seq.rs`). OOS code.

## Performance notes for implementers

- Batch frames. One batch (arm, let the ring fill, one play, one read) costs about 10 ms plus the time the triggers
  need: with a 1 kHz signal about 800 records/s against about 30 with one frame per read. 31 frames of 12 500 points
  read in 8.6 ms. Verified on the unit (`../protocols/acquisition-start.md`, Batches). OOS plans about 4 MB per read.
- Compress deep records in the FPGA: about 625 k points per frame for the screen, raw windows only for zoom and
  measurements.
- Read into page-aligned buffers of their own. The RK3399's PCIe is not cache coherent; a read buffer that shares
  cache lines with heap data corrupted the heap within seconds at short time bases. Verified on the unit
  (`hal/src/dho/bus.rs` `DmaBuf`).
- Cap single reads at 4 MiB and continue on short reads. OOS code.
- Account for the 8-byte frame padding when you index frames of a batch.
- Arm the next batch before decoding the current one (`acq.prearm`, `ui/src/board/dho/hw.rs`); +1 to +8 %
  acquisitions/s on the unit (1 and 2 channels, 100 ns/div and 100 us/div).
- Use MSI, keep the completion off the cores that draw. Poll mode costs an A53 per transfer.
- Minimise BAR writes: skip unchanged words (OOS shadows), and lower the timer slack so the 10 us write pause is
  10 us.
- Nothing in a frame identifies the settings it was recorded with. Header word 4 holds a frame or chunk counter when
  headers are on (`../protocols/trigger.md`, `hal/src/dho/roll.rs`), but it does not change with the settings. Stale
  frames after a settings change are filtered on the host: the OOS fork tags each frame with the (generation, epoch)
  of the settings it was armed with and drops mismatches. A SINGLE armed just before a settings change can deliver
  only an old-generation frame and stay in Wait. Verified on the unit with the OOS fork (2026-11-16).

## Hazards and recovery

| Hazard | Effect | Rule |
|---|---|---|
| `PROG` to the monitor | erases and rewrites flash; size 0x1000000 erases the factory image too | never send it (`../protocols/fpga-image.md` 2.4) |
| BOOT while xdma is bound | can lock the CPU | unload xdma and the host driver first |
| BOOT to an empty or foreign slot | no monitor afterwards, no PCIe | OOS allows slots 0x0 and 0x400000 only; a power cycle returns to slot 0 |
| C2H read the frame cannot satisfy (k = 0, too many compressed points, compression without the pass-through chain) | read blocks 10 s, then times out | keep 0x11f0 within what the window delivers |
| mask/zone window that never completes | FSM waits in state 5, no acquisitions; the state survives a reconfiguration (with the OOS fork, round 20) | stall guard: drop the unit for those settings and acquire without it (`ui/src/board/dho/hw.rs`); a stop alone is not a known recovery |
| interrupt-mode, PCIe or FPGA experiments | FPGA and endpoint keep their state across a warm reboot; the next kernel can meet a device in the experiment's state | recover by a power cycle, not a warm reboot (the INTx case under Completion) |
| kexec with MSI | every C2H read times out | poll mode in kexec sessions (Completion) |
| killing a reader during a read | a transfer that times out 10 s later | stop oos-ui cleanly |

The hardware watchdog of the RK3399 (21.5 s period) resets only the SoC. Like a `reboot`, it leaves the Zynq running
the main image; OOS's start-up sends BOOT again, which reloads it. Verified on the unit for `reboot`, assumed for a
watchdog reset (`../protocols/fpga-boot.md`; the watchdog runs with the OOS fork only).

## Open questions

- Persistent DMA mapping in the xdma driver: expected to raise the deep-record read rate, not measured.
- 0x1184 above 1023 samples per column: which bits the mask unit uses for longer records.
- Zone unit: why its frame counter stays 0 under OOS (`../fork/hwzone.md`).
- Hardware average: the cause of the noise floor near N = 16 at 62.5 MSa/s.
- Peak detect (0x1004 bit 1): which window of samples it covers at decimation factors above 8.
- AUX OUT: pulse width and polarity on the BNC, not measured.
- Serial receivers: no unit check yet.
- Whether configuration writes while the FSM runs are safe.
- The physical size of the acquisition memory: OOS's memory constant (0x5000000 bursts, `hal/src/dho/hz.rs`) is
  larger than the 2^24 bursts that were shown to hold a complete record; the true limit has not been probed.
- The two channel-to-core tables of the measure unit in one session.
