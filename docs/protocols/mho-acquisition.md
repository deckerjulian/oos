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
| 8 | FPGA image | a Kintex-7 in slave serial mode: PROGRAM_B, INIT_B and DONE on GPIOs (`/dev/spi2k7_ctl`: GPIO1_B5, GPIO2_C7, GPIO2_C4 from the platform probe), the bitstream streamed over SPI (`/dev/spidev1.0` or `/dev/spidev5.0`: on code 3 the stock device tree enables only spi5 with its spidev, and `/dev/spidev1.0` is not there (platform probes of an MHO954 and an MHO98); `/dev/spidev5.0` on code 1); then 5 s |
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

## 7. Checking it on a unit

`tools/mho-check.py <scope-ip>` checks on an MHO900 under its stock system what can be seen without writing to the
hardware, and writes `mho-check-<model>-<date>.txt` to attach to the GitHub issue. It needs Python 3 and adb on the
PC, network adb and SCPI (port 5555) on the scope; like `tools/platform-probe.sh` it restarts adbd as root.

| check | what it looks at | expected |
|---|---|---|
| S1 to S3 | hardware code, `version.txt`, `/rigol/FPGA` | the image table of section 1, this unit's image present |
| S4 | GPIO0_A1, GPIO0_A2, GPIO1_A0, GPIO3_D0 | outputs at 1 (not the supplies on code 1), the probe detect an input |
| S5, S6 | modules and device nodes | the drivers and devices of section 1 and 2; the `/dev/spidev*` there recorded |
| S7, S8 | PCIe | `10ee:7024`, a BAR of 16 MiB |
| S9 | clock tables | 136 TI register words each (only counted, not copied) |
| S10, S11 | open devices, kernel log | recorded; the FPGA configured: its PCIe endpoint enumerates after the load (the DONE the driver logs when it requests the line comes before the load and is 0) |
| R1 to R4 | registers read once | the blocks answer; 0x3000 holds a finished bridge word (strobe 0, one ADC) |
| R5 to R9 | 0x4008 with CH1, CH1+2, CH1 to 4, CH1 to 3 | channel mask 0x1, 0x3, 0xf; roll and scan off at 1 us/div |
| R10 | 0x10e4 at 1k and 1M points | the record depth changes |
| R11 | 0x40b0/0x40b4 while running | the time stamp moves |
| R12 | 0x4008 at 200 ms/div | roll or scan bit set |
| R13 | 0x4004, 0x4034, 0x4100 in STOP | recorded |
| R14 | 0x2438 and 0x6110 with the mask test off, on and running | the mask block follows; unchanged is recorded (the test may need a mask, and the check makes none) |
| R15 | waveform preamble in WORD at 0.1 and 1 V/div | 7500 codes per division, centre 32768 |
| R16 | the settings again | each written back on its own (the memory depth as `10k`, `1M` ...: the number its query answers is refused), then read and compared with before |

Registers are read with the stock tool `/rigol/tools/pcie2dev`, always with exactly one address (it writes when it is
given two), and only at addresses the stock system uses itself: a read of an address the FPGA does not decode could
stop the processor. The settings it changes over SCPI are read first and set back at the end; `--no-scpi` leaves
them alone and skips R5 to R15. Not covered: the read-out stream itself, the interleave of the words and the ADC
bridge's writes in full (they would need the application stopped or writes to the hardware).

`tools/mho-check.py <scope-ip> --map` runs none of the checks but maps what section 8 leaves open about the front ends
and the sample path. It steps through about 50 settings with nothing connected to the inputs: the scale of CH1 from
500 uV/div to 10 V/div, its offset, coupling and bandwidth limit, each other channel on at two scales, the time base
from 2 ns/div to 1 ms/div with one, two and four channels, and the trigger level (`--fifty` adds CH1 at 50 ohm).
After each step it records what changed against the step before:

- the FPGA's configuration registers that the stock system writes itself (analog gain and offset per channel, the
  gain and offsets after the ADC, the AFE and probe offset DACs, sample rate, interpolation, delays, trigger levels,
  the PWM outputs), read back with `pcie2dev`;
- the GPIO lines of `/sys/kernel/debug/gpio`, among them the front ends' select lines;
- the words seen in the ADC bridge (0x3000) while the setting changed, about 120 reads: some of the ADC register
  writes the change makes, as written (with the stock system's mask applied);
- new lines of the kernel log.

The settings it changes are read first and set back one by one at the end, and compared again. The report,
`mho-map-<model>-<date>.txt`, ends with every register per step as a table. Each channel is switched on before its
own settings are written back: on an MHO98 the stock system did not take the scale of a channel that was off, and
CH2 to CH4 stayed at 10 mV/div.

`tools/mho-check.py <scope-ip> --afe` records what `--map` cannot see: the words the stock system sends to the front
ends (driver `spi2afe_gpio`: chip selects gpio 76, 66, 69, 68 for cs0 to cs3, clock 75, data out 74, data in 73) and
to the clock chip (3-wire bus: chip select 50, clock 49, data 51), and the select lines `a`, `b`, `c`. It switches on
the kernel's trace event `gpio_value`, filtered to these lines, records it around each setting, and switches it off
again with the trace settings as they were; tracefs lives in memory, nothing else is written. The words are decoded on
the PC: one frame per chip-select low, the data line at the rising and at the falling clock edge (the clock phase is
not known yet), and what the driver reads. The steps are those of `--map`, an idle step first (what the stock system
writes by itself), then the offset and AC coupling of CH1 at 10 mV/div and 1 V/div, and scale, offset, coupling and
bandwidth limit of CH2 to CH4. It needs the event in the kernel (4.4 has it when tracing is built in; the stock
kernel mounts tracefs) and adb root. The report is `mho-afe-<model>-<date>.txt`, with the raw events at its end.

## 8. Seen on units (stock system 00.01.00, hardware code 3)

From `mho-check.py` on an MHO954 and an MHO98 and `--map` on the MHO98 (issue #4); **verified on the unit** means
read on these units under the stock system, nothing of it driven by OOS:

- All 25 checks of the MHO954 as assumed, with the spidevs `/dev/spidev2.0` and `/dev/spidev5.0`. The MHO98's three
  differences were the check's own (S6, S11) or are explained by the mask block below (R14).
- Mask block: 0x6110 is 0x00100000 while the mask test runs, 0 when it is off or on and stopped; 0x2438 stays 0
  (verified on the unit, MHO954).
- The analog gain and offset words 0x14c8 to 0x14f4 belong to the four ADC **lanes**, not to the channels: with CH1
  alone all four hold CH1's values, with two channels lanes 1 and 3 hold CH1's and lanes 2 and 4 the other's, with four
  channels each its own (verified on the unit). With GND coupling the lane's gain is 0 and its offset 0x8000.
- AC coupling changes nothing `--map` reads; the offset changes no register but the trigger level words, and moves
  the data line of the front ends' bus (verified on the unit). Assumed: both are set over that bus; `--afe` is to show
  the words.
- Bandwidth limit 20 MHz on CH1: bits 23:20 of 0x1004 and 0x1014 go to 0xf (verified on the unit; which bit is which
  channel not yet).
- Select lines: gpio 153 (`c`) is high at 0.1 and 0.2 V/div of CH1, gpio 149 (`a`) at 5 and 10 V/div, both low
  between; at 50 ohm 153 stays low and 149 is high at 1 V/div (verified on the unit). Assumed: the group 149, 152,
  153 belongs to CH1.
- Sample rate: one channel 4 GSa/s from 2 to 100 ns/div, with interpolation at 2 and 10 ns/div (0x1154, 0x115c,
  0x1168), 1 GSa/s at 1 us/div; 0x10d0 is 0x8001, 0x8002, 0x8004 or 0x8008 at the higher rates and 0 below them,
  where 0x10d8 divides 250 MSa/s (5, 25, 250 give 50, 10, 1 MSa/s). The segment size 0x10e0 is 0x200, 0x400 and
  0x800 with one, two and three or four channels (verified on the unit).
- 0x4008 changes between two values while nothing is changed (0x08805181 and 0xc880b1a1 at 1 us/div): it holds state
  bits besides the channel mask, so `--map` lists it at steps that do not touch it.

## 9. Open at the bench

- the LMK04821 settings and the ADC register set and masks (sections 1 and 4);
- the JESD204 link values and how to see that it is up;
- the front ends: the words on their bus per setting (`--afe`), the select lines of CH2 to CH4, the 50 ohm path and
  their calibration files;
- whether the run and read-out sequences of acquisition-start.md work unchanged with the new words of section 3 left
  at their power-on values.
