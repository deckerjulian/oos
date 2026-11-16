# MHO900 acquisition: start-up, registers, read-out, measurements

Status: **not yet verified on hardware.** Nothing here has run on an MHO900. It describes how the stock MHO900 system
(00.01.00.00.26) starts the acquisition board and uses its FPGA, compared with the DHO800/DHO900 pages in this folder.
"Assumed" marks an interpretation. OOS does not drive this board yet (ADR 0028, [mho.md](../mho.md)); the facts below
are kept in `hal/src/mho/acq.rs` as data, so the bring-up at the bench starts from them.

In one sentence: the main board, the register window, the read-out path and the sample format are the DHO's; the
start-up of the board, the ADCs and their clock, the front ends and the mask block are new.

## 1. Start-up of the acquisition board (stock order)

The stock start script reads the board's **hardware code** first (a GPIO strap driver, `hdcode`), and several steps
depend on it. Code 1 is an early board; codes 2 and up are later ones.

| # | step | how |
|---|---|---|
| 1 | hardware code | `/sys/class/hdcode/hdcode_dev/hdcode_value` (0 when the driver gives none) |
| 2 | fan | code 1: an on/off line; codes 3 and up: PWM1, `pwm-fan` (as in OOS's device tree) |
| 3 | ADC supply | GPIO0_A1 (gpio 1) = 1 (not on code 1) |
| 4 | front-end supply | GPIO0_A2 (gpio 2) = 1 (not on code 1) |
| 5 | Ethernet PHY | Motorcomm driver, link down and up again |
| 6 | sample clock | a **LMK04821** on a GPIO 3-wire bus (`/dev/spi_3wires`), one register table per hardware code (codes 1, 2, 3 and up) |
| 7 | FPGA supply | GPIO1_A0 (gpio 32) = 1 |
| 8 | FPGA image | a Kintex-7 in slave serial mode: PROGRAM_B, INIT_B and DONE on GPIOs (`/dev/spi2k7_ctl`: GPIO1_B5, GPIO2_C7, GPIO2_C4 from the platform probe), the bitstream streamed over SPI (`/dev/spidev1.0`; `/dev/spidev5.0` on code 1); then 5 s |
| 9 | PCIe | `pcie-rockchip`, then `xdma`; when no `/dev/xdma0_*` appears, both are unloaded and loaded again, up to 10 times |
| 10 | front-end bus, generator | the AFE bus on GPIOs (`/dev/spi_simulate`); the generator board on `ttyS4` (codes 2 and up) |
| 11 | others | DAC range driver, LA probe detect GPIO3_D0 (gpio 120, input), front-end overload inputs (`afe_rms`) |
| 12 | application | per channel the front-end init, the SCU init, then the JESD204 link (section 4) |

The clock tables are TI register words: 136 lines `R<n> 0x<aaaadd>`, 24 bits = register address (bits 23:8) and data
(bits 7:0), the first line a reset of R0. OOS ships none of them; their content is to be measured or derived at the
bench.

The FPGA image is chosen by the hardware code; the file is the unit's own under `/rigol/FPGA` (OOS may read it at run
time, it cannot ship it, ADR 0013):

| hardware code | image |
|---|---|
| 1 | `sparrow3_K160T.bit` |
| 2 to 7 | `sparrow3_RT688.bit` |
| 8 to 11 | `sparrow3_K160T_LR.bit` |

## 2. Register window and read-out stream

The same devices as on the DHO800/DHO900 (acquisition-start.md, section "read rules"):

- registers: `/dev/xdma0_bypass`, mapped whole, **16 MiB** (the blocks reach 0x30000; the DHO's window is 32 KiB);
  32-bit words at byte offsets, one lock around each access;
- read-out: `/dev/xdma0_c2h_0`, a plain blocking `read()` of the requested bytes. Analog and LA traces come over the
  same stream, one after the other.

The stock system also uses a memory copy engine of its kernel to move a single-channel record from the read buffer
into the waveform buffer. That is a copy on the processor's side, not part of the FPGA protocol; OOS does not need it.

## 3. Register blocks

| offsets | block | compared with the DHO800/DHO900 |
|---|---|---|
| 0x0004 | FPGA version | |
| 0x1000 to 0x1574 | SPU: ADC lanes, sample rate, memory, filters, interpolation, read-out window, LA, probes | same offsets, some values wider (below) |
| 0x2000 to 0x2274 | TPU: trigger, serial triggers, auto trigger | same layout; the auto trigger has words at 0x226c to 0x2274 |
| 0x2438 | WAU: mask control | new |
| 0x3000, 0x3004 | ADC SPI bridge (section 4) | the DHO has the generator here |
| 0x4000 to 0x40d0, 0x4100, 0x4104 | SCU: run, record and play, status, time stamps, stop reason | same offsets and bits, plus new words |
| 0x5000 to 0x5040 | Bode analysis | new |
| 0x6058 to 0x6064 | WAU: Bode source | new |
| 0x610c to 0x6158 | WAU: mask and pass/fail | the DHO has this block at 0x1184 to 0x11cc |
| 0x10100, 0x10108 | a PWM output (assumed: an offset or level DAC) | new |
| 0x20004 to 0x20818 | JESD204 receiver of the two ADCs | new |
| 0x30000 | multi-channel sync (assumed: the generator board) | new |

### What is the same

Of the 235 register offsets OOS uses on the DHO800/DHO900, 169 are used by the stock MHO900 system at the same
offset for the same function. Those that matter for a first acquisition:

| register | meaning (acquisition-start.md, record-play.md) |
|---|---|
| 0x4004 | bit 0 FSM start, bit 2 play-last pulse, bit 31 stop |
| 0x4008 | bits 3:0 FSM channel mask, bit 17 roll, bit 22 scan, bits 23/24 analog/LA stream, zoom |
| 0x400c | record and play mode (record sequence bits 25, 28, 29) |
| 0x4010, 0x4014, 0x4018 | frame window: first, current, last |
| 0x4030, 0x4034 | status (0x4034 written to clear) |
| 0x404c | pre-sample compensation |
| 0x4050, 0x4054 | pre- and post-trigger samples |
| 0x10d0 to 0x10d8 | sample rate |
| 0x10e0 | segment size |
| 0x10e4 | record depth (memsel) |
| 0x1100 to 0x1114 | wave range of the read-out |
| 0x11e8, 0x11f0 | TX state and TX length |

The record and play words 0x4068 and 0x406c, the frame time stamps 0x40b0 to 0x40c4, the auto-trigger word 0x4064,
the compression 0x1170 and the TX words 0x11fc and 0x12c0 are written as OOS writes them on the DHO.

### What is new

Registers the stock MHO900 system uses that OOS does not use on the DHO800/DHO900 (apart from the blocks of the
table above that are new as a whole):

| register | what the stock system does |
|---|---|
| 0x101c, 0x102c | ADC bit slip: set and read back |
| 0x1098 to 0x10a0 | a second output filter: control, coefficients, bypass |
| 0x10dc | anti-alias setting of the acquisition |
| 0x1104, 0x110c | wave range: start and end as two words each (assumed: 64-bit values for depths over 4 G) |
| 0x111c | the sinc filter's input length, second word |
| 0x1120 to 0x112c | coarse and fine read-out delay |
| 0x1168 | the interpolation factor |
| 0x11ec, 0x11f4, 0x12c4 | more TX information of the read-out (analog and LA) |
| 0x1208, 0x120c | recorded range, normal and roll |
| 0x12e4 | start offset of a channel in the read-out, and the LA fine offset |
| 0x1328 | acquisition delay |
| 0x1408 to 0x1414, 0x142c, 0x1440 to 0x1454 | probe offset DACs, probe power, the probes' 1-wire bus |
| 0x1518 | maximum output |
| 0x155c to 0x1574 | per-channel gain and two offsets after the ADC, with a bypass and mode word |
| 0x2028 to 0x2080 | trigger A, B and R events, logic operations, delay and holdoff event counts |
| 0x210c to 0x21cc | serial triggers: I2S, LIN, CAN, FlexRay, MIL-STD-1553B |
| 0x2264 | external trigger input and its hysteresis |
| 0x226c to 0x2274 | a second auto trigger (enable, status, two words) |
| 0x2438 | mask control of the WAU |
| 0x401c | hardware version |
| 0x4044, 0x4048 | FPGA DNA |
| 0x4100 | stop reason (read, written to clear); 0x4104 an AUX output control |

## 4. ADCs and their link

Two ADCs, interleaved (data sheet: 4 GSa/s), each behind a JESD204 link into the FPGA. The processor reaches their
registers through an SPI bridge in the FPGA:

```
0x3000 = strobe | select | addr << 16 | data     ; strobe = bit 26, select = bit 24 (ADC 0) or bit 25 (ADC 1)
wait 100 us
0x3000 = select | addr << 16 | data              ; the same word without the strobe
(read: bit 23 set in addr and data 0; after a further 100 us the 16 data bits are in 0x3004)
```

The stock system XORs the data with a fixed mask per register before it writes it, as it does for the DHO's ADC
(adc-spu-init.md); the masks are not known yet.

At a cold start the application pulses 0x2003c (1, 5 ms, 0; assumed: the link reset), sets up the receiver
(0x20004 to 0x2002c, 0x20400 to 0x2040c, 0x2080c to 0x20818) and enables the DAC sync for 50 ms. Assumed: the JESD204
link takes the place of the DHO's lane alignment (adc-spu-init.md); the ADC bit slip words 0x101c and 0x102c point to a
remaining alignment step.

## 5. Read-out and sample format

A read-out is requested and read as on the DHO800/DHO900 (record-play.md): set the window, request, then one `read()`
of the length the TX registers say. The words are what OOS already decodes:

- 16 bits, little endian;
- **7500 codes per division around 32768**: volts = (code - 32768) / 7500 x V/div - offset. The stock system's vertical
  increment (V/div / 7500) and ground (32768 + offset in codes) are the DHO's;
- channels share the words as on the DHO (horizontal-config.md 2.2): one channel alone, two on even and odd words,
  three or four in groups of four, word 4i + k = CH k+1.

So `seq::decode_volts` of the DHO800/DHO900 is the decode the MHO900 needs; `hal::mho::acq::volts` holds the same
formula and a test keeps the two equal.

## 6. Measurements

The stock system computes its measurements on the processor, from the decoded read-out, not in the FPGA. It reads two
kinds of data for them:

- the **screen trace**: the compressed read-out the display uses (as on the DHO800/DHO900);
- the **record**: for one measurement mode it sets the wave range and TX information for up to one million points,
  requests a read-out of the memory and splits it by channel as in section 5 (assumed: the "full record" range of
  the measurements).

OOS's measurements already work this way on every board: `oos-dsp` computes them from frames in volts, the screen
gate from the displayed record and the full-record gate from the retained memory (docs/measurement-modules.md). Once
the MHO900 board delivers frames (`Board`, ui/src/board/mho), the whole measurement pipeline carries over unchanged.
What the board must deliver for it: the frames in volts with their sample interval, and the retained deep record for
the full-record gate.

## 7. Open at the bench

- the LMK04821 settings and the ADC register set and masks (sections 1 and 4);
- the JESD204 link values and how to see that it is up;
- the front ends (the `a`, `b`, `c` select lines of the platform probe, the gain tables, the 50 ohm path) and their
  calibration files;
- whether the run and read-out sequences of acquisition-start.md work unchanged with the new words of section 3 left
  at their power-on values.
