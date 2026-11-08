# Sample clock: TI LMX2582 (and LMK04828)

## LMX2582, 3-wire bus

- Pins: LE GPIO0_A6, SCLK GPIO0_A3, SDIO GPIO0_A2. Stock device `/dev/spi_3wires_lxm2582`. Each `write()` of 3 bytes is one
  24-bit LMX2582 word: `[R/W=0 | addr(7)] [data15..8] [data7..0]`, LE pulsed around it, `udelay(1)` steps.
- The table has 46 words, highest register first, no reset word:

```
R70=0000 R69=0000 R68=0089 R64=0077 R62=0000 R61=0001 R59=0000 R48=03fc R47=00df R46=0f63
R45=0000 R44=0000 R43=0000 R42=0000 R41=0004 R40=0000 R39=8204 R38=0032 R37=4000 R36=0c21
R35=029b R34=c3ea R33=2a0a R32=210a R31=0601 R30=0034 R29=0084 R28=2924 R25=0000 R24=0509
R23=8842 R22=2300 R20=012c R19=0965 R14=0420 R13=4000 R12=7001 R11=0018 R10=10d8 R9=0302
R8=1084  R7=28b2  R4=1943  R2=0500  R1=0808  R0=221c
```

  R0 last with FCAL_EN set starts the VCO calibration. Result: 1.25 GHz ADC clock.
- The stock OS programs the LMX2582 at boot (`start_rigol_app.sh`). The older `spi2pll` for `/dev/spi_3wires` is commented
  out there.

## Under OOS

`oos-hw pll-init` drives the three lines through the GPIO character device, found by their DT line names (`spi_le` = gpio0
line 6, `spi_sclk` = line 3, `spi_sdio` = line 2): LE low, MSB first, data on the rising SCLK edge, LE high pulse after each
24-bit word, 50 ms between words. There is no readback (SDIO is write-only on this board). `hal::dho::boot` does the same
(`PLL_TABLE`).

The write path is verified on the unit: in a warm state, one word R0 = 0x221d (POWERDOWN) drops the counter 0x1210 at once,
and the full table brings it back (`cold-clock.md` E2).

## LMK04828: not used on this board revision

`/dev/spi_3wires` (module `spi2pll_gpio.ko`, ioctls 0x7800/0x7801 status/0x7802) is not loaded by the stock start script
(commented out) and does not exist on the unit. `init_all_clock.txt` is a leftover. OOS does not program a LMK04828.

## LMK04828 (jitter cleaner / clock distribution), original notes

- `resource/conf/init_all_clock.txt` is its register list (R0 reset 0x90, then R0..R8191). The stock OS has a 114-entry table
  for this part too; its writes fail silently because the device node is missing.

## Lock check without readback

The FPGA counts the ADC data clock in 0x1210. A read of 0x1210 (`oos-hw rd 0x1210`, read only) with 62494..62506 as the
good window (`(v - 62494) < 13`) is the check that the write-only LMX2582 bus cannot give. In a cold start the counter only
runs after the ADC sync and the eye scan (`cold-clock.md`), so read it after those steps.

## Other output frequencies (offline, untested)

Nothing in this section has been written to a unit. Field names, positions and limits are from TI's LMX2582 datasheet
**SNAS680E** (rev. E, August 2022; register descriptions Tables 7-5..7-50, input path Table 7-1, MASH order Table 7-2,
channel divider Table 7-4). Code: `oos_hal::dho::lmx2582` (decode and `table_for`), dry run `oos-fpgad pll --freq <Hz> --dry`.

### Decode of the 1.25 GHz table

| field | register[bits] | value | meaning |
|---|---|---|---|
| OSC_2X | R9[11] | 0 | reference doubler off (REF_EN R9[9] = 1) |
| PLL_R_PRE | R12[11:0] | 1 | bypassed |
| MULT | R10[11:7] | 1 | bypassed |
| PLL_R | R11[11:4] | 1 | bypassed, so f_PD = f_OSC |
| PFD_CTL | R13[1:0] | 0 | dual PFD (5..200 MHz); CP_EN R13[14] = 1 |
| CP_IUP / CP_IDN / CP_ICOARSE | R14[6:2] / R14[11:7] / R14[1:0] | 8 / 8 / 0 | 2.5 mA up and down, x1 |
| PLL_N_PRE | R37[12] | 0 | prescaler /2 |
| PLL_N | R38[12:1] | 25 | |
| PLL_NUM | R44 (31:16), R45 (15:0) | 0 | |
| PLL_DEN | R40 (31:16), R41 (15:0) | 4 | |
| MASH_ORDER | R46[2:0] | 3 | 3rd order modulator running with NUM 0 (effectively integer-N) |
| MASH_DITHER / MASH_SEED | R30[10] / R42, R43 | 0 / 0 | |
| PFD_DLY | R39[13:8] | 2 | 6 cycles, the Table 7-2 value for 2nd/3rd order |
| VTUNE_ADJ | R30[7:6] | 0 | correct below 6.5 GHz VCO (3 from 6.5 GHz) |
| CHDIV_EN | R34[5] | 1 | channel divider on |
| CHDIV_SEG1 / SEG1_EN | R35[2] / R35[1] | 0 (/2) / 1 | |
| CHDIV_SEG2 / SEG2_EN | R35[12:9] / R35[7] | 1 (/2) / 1 | |
| CHDIV_SEG3 / SEG3_EN | R36[3:0] / R35[8] | 1 (/2) / 0 | not in the path |
| CHDIV_SEG_SEL | R36[6:4] | 2 | segments 1 + 2: CHDIV = 2 x 2 = 4 |
| VCO_DISTA_PD / VCO_DISTB_PD / CHDIV_DIST_PD | R31[9] / R31[10] / R31[7] | 1 / 1 / 0 | VCO-direct buffers off, divider buffer on |
| CHDIV_DISTA_EN / CHDIV_DISTB_EN | R36[10] / R36[11] | 1 / 1 | |
| OUTB_PD / OUTB_MUX / OUTB_POW | R46[7] / R48[1:0] / R47[5:0] | 0 / 0 / 31 | **output B = the ADC clock**, from the channel divider, top of the normal power range |
| OUTA_PD / OUTA_MUX / OUTA_POW | R46[6] / R47[12:11] / R46[13:8] | 1 / 0 / 15 | output A off |
| FCAL_EN / ACAL_EN | R0[3] / R0[4] | 1 / 1 | R0 (last word) starts frequency and amplitude calibration |
| FCAL_HPFD_ADJ / FCAL_LPFD_ADJ | R0[8:7] / R0[6:5] | 0 / 0 | PFD between 10 and 100 MHz |
| LD_EN / MUXOUT_SEL / LD_TYPE | R0[13] / R0[2] / R61[0] | 1 / 1 / 1 | MUXout = Vtune lock detect (not readable here) |
| CAL_CLK_DIV | R1[2:0] | 0 | calibration clock = OSCin |
| ACAL_CMP_DLY | R4[15:8] | 25 | default |
| VCO_IDAC_OVR / VCO_CAPCTRL_OVR | R8[13] / R8[10] | 0 / 0 | automatic calibration, no forced core |
| VCO_IDAC / ACAL_VCO_IDAC_STRT / VCO_CAPCTRL | R19[11:3] / R20[8:0] / R22[7:0] | 300 / 300 / 0 | defaults |
| FCAL_VCO_SEL_STRT / VCO_SEL / VCO_SEL_FORCE | R23[14] / R23[13:11] / R23[10] | 0 / 1 / 0 | calibration starts at VCO core 7 (default) |
| ACAL_FAST / FCAL_FAST / AJUMP / FJUMP | R64[9] / R64[8] / R64[7:5] / R64[3:0] | 0 / 0 / 3 / 7 | slow (default) calibration |

R68..R70 are read-only (rb_LD_VTUNE, rb_VCO_SEL, rb_VCO_CAPCTRL); writing them does nothing. Everything else is "program to
default" and matches the datasheet register map.

**OSCin = 100 MHz (inferred).** With the reference path bypassed, f_VCO = f_OSC x PLL_N_PRE x (PLL_N + NUM/DEN) = f_OSC x 2 x 25
= 50 x f_OSC and f_OUT = f_VCO / 4 = 12.5 x f_OSC. Exactly 1250 MHz needs f_OSC = 100 MHz, which gives f_VCO = 5000 MHz (inside
3550..7100). The rest of the table agrees: FCAL_HPFD_ADJ 0 means PFD <= 100 MHz, FCAL_LPFD_ADJ 0 means >= 10 MHz, CAL_CLK_DIV 0
needs OSCin <= 200 MHz, and ACAL_CMP_DLY 25 is above the datasheet's "> 10 for 100 MHz". The VCO range alone would allow
71..142 MHz. The oscillator itself has not been identified on the PCB.

**The stock table is outside rev. E of the datasheet.** Table 7-4 limits CHDIV 4 to VCO 3552..4736 MHz (output 888..1184 MHz) and
puts 1250 MHz on CHDIV 3 (VCO 3750 MHz). The table runs /4 at 5000 MHz. Rev. E "changed the channel divider requirement"; the
table is probably older. It works on every DHO800/900, so `table_for` admits CHDIV 4 up to exactly this 5000 MHz and no further.

**Frequency-dependent fields** (the only ones `table_for` changes): PLL_N (R38), PLL_NUM (R44/R45), PLL_DEN (R40/R41), the
channel divider (R35: SEG1, SEG1_EN, SEG2, SEG2_EN, SEG3_EN; R36: SEG_SEL, SEG3) and VTUNE_ADJ (R30). These depend on the
frequency too but stay as they are, because the reference path does not change: FCAL_HPFD_ADJ / FCAL_LPFD_ADJ, CAL_CLK_DIV and
ACAL_CMP_DLY (PFD and OSCin), PFD_DLY and the minimum N (MASH order, kept at 3). The outputs (B through the divider) and the VCO
calibration set-up do not change either; the calibration searches for the VCO core itself.

### Choice rule (`table_for`)

Start from `PLL_TABLE`, keep the reference path (f_PD = 100 MHz, VCO step 200 MHz per unit of N). Try every CHDIV of Table 7-4
whose VCO range (and 3.55..7.1 GHz) contains f_OUT x CHDIV. Prefer, in this order: integer-N; the VCO closest to the 5 GHz of the
stock table (the loop filter, charge pump and calibration are proven there); the smaller divider. The fraction is reduced, and if
its denominator divides the DEN 4 of the stock table, DEN 4 is kept. PLL_N must be >= 18 (Table 7-2, 3rd order). The function
refuses non-integral Hz, anything above 3.55 GHz (that would need the VCO-direct path, OUTB_MUX = 1), and anything no divider can
reach (below 20 MHz: /192 needs VCO >= 3840 MHz). `table_for(1.25e9)` reproduces `PLL_TABLE` byte for byte (unit test).

| f_OUT | CHDIV | f_VCO | PLL_N + NUM/DEN | mode | words changed vs. 1.25 GHz |
|---|---|---|---|---|---|
| 1.250 GHz | 4 | 5000 MHz | 25 + 0/4 | integer (stock) | - |
| 1.375 GHz | 3 | 4125 MHz | 20 + 5/8 | fractional | R45 R41 R38 R36 R35 |
| 1.500 GHz | 3 | 4500 MHz | 22 + 2/4 | fractional | R45 R38 R36 R35 |
| 1.625 GHz | 3 | 4875 MHz | 24 + 3/8 | fractional | R45 R41 R38 R36 R35 |
| 1.750 GHz | 3 | 5250 MHz | 26 + 1/4 | fractional | R45 R38 R36 R35 |
| 2.000 GHz | 2 | 4000 MHz | 20 + 0/4 | integer | R38 R36 R35 |

With f_PD fixed at 100 MHz, only 1.25 and 2.0 GHz have an integer-N solution. 1.375/1.625 GHz cannot be integer with any divider,
and 1.5/1.75 GHz cannot be integer with an allowed one (/4 would need VCO 6000/7000 MHz, above the /4 limit). 2.0 GHz could also be
/3 at 6000 MHz, N 30; /2 at 4000 MHz wins the tie-break by the smaller divider (both are 1000 MHz from 5 GHz). Every generated
table passes the datasheet checks in `Fields::notes` (VCO range, Table 7-4, minimum N, PFD_DLY, VTUNE_ADJ).

Generated tables (same order as above, highest register first, R0 last):

1.375 GHz:
```
R70=0000 R69=0000 R68=0089 R64=0077 R62=0000 R61=0001 R59=0000 R48=03fc R47=00df R46=0f63
R45=0005 R44=0000 R43=0000 R42=0000 R41=0008 R40=0000 R39=8204 R38=0028 R37=4000 R36=0c11
R35=021f R34=c3ea R33=2a0a R32=210a R31=0601 R30=0034 R29=0084 R28=2924 R25=0000 R24=0509
R23=8842 R22=2300 R20=012c R19=0965 R14=0420 R13=4000 R12=7001 R11=0018 R10=10d8 R9=0302
R8=1084 R7=28b2 R4=1943 R2=0500 R1=0808 R0=221c
```
1.5 GHz:
```
R70=0000 R69=0000 R68=0089 R64=0077 R62=0000 R61=0001 R59=0000 R48=03fc R47=00df R46=0f63
R45=0002 R44=0000 R43=0000 R42=0000 R41=0004 R40=0000 R39=8204 R38=002c R37=4000 R36=0c11
R35=021f R34=c3ea R33=2a0a R32=210a R31=0601 R30=0034 R29=0084 R28=2924 R25=0000 R24=0509
R23=8842 R22=2300 R20=012c R19=0965 R14=0420 R13=4000 R12=7001 R11=0018 R10=10d8 R9=0302
R8=1084 R7=28b2 R4=1943 R2=0500 R1=0808 R0=221c
```
1.625 GHz:
```
R70=0000 R69=0000 R68=0089 R64=0077 R62=0000 R61=0001 R59=0000 R48=03fc R47=00df R46=0f63
R45=0003 R44=0000 R43=0000 R42=0000 R41=0008 R40=0000 R39=8204 R38=0030 R37=4000 R36=0c11
R35=021f R34=c3ea R33=2a0a R32=210a R31=0601 R30=0034 R29=0084 R28=2924 R25=0000 R24=0509
R23=8842 R22=2300 R20=012c R19=0965 R14=0420 R13=4000 R12=7001 R11=0018 R10=10d8 R9=0302
R8=1084 R7=28b2 R4=1943 R2=0500 R1=0808 R0=221c
```
1.75 GHz:
```
R70=0000 R69=0000 R68=0089 R64=0077 R62=0000 R61=0001 R59=0000 R48=03fc R47=00df R46=0f63
R45=0001 R44=0000 R43=0000 R42=0000 R41=0004 R40=0000 R39=8204 R38=0034 R37=4000 R36=0c11
R35=021f R34=c3ea R33=2a0a R32=210a R31=0601 R30=0034 R29=0084 R28=2924 R25=0000 R24=0509
R23=8842 R22=2300 R20=012c R19=0965 R14=0420 R13=4000 R12=7001 R11=0018 R10=10d8 R9=0302
R8=1084 R7=28b2 R4=1943 R2=0500 R1=0808 R0=221c
```
2.0 GHz:
```
R70=0000 R69=0000 R68=0089 R64=0077 R62=0000 R61=0001 R59=0000 R48=03fc R47=00df R46=0f63
R45=0000 R44=0000 R43=0000 R42=0000 R41=0004 R40=0000 R39=8204 R38=0028 R37=4000 R36=0c11
R35=021b R34=c3ea R33=2a0a R32=210a R31=0601 R30=0034 R29=0084 R28=2924 R25=0000 R24=0509
R23=8842 R22=2300 R20=012c R19=0965 R14=0420 R13=4000 R12=7001 R11=0018 R10=10d8 R9=0302
R8=1084 R7=28b2 R4=1943 R2=0500 R1=0808 R0=221c
```

`oos-fpgad pll --freq <Hz> --dry` prints the same list plus the decode and the datasheet notes. It touches no hardware. Without
`--dry`, any `--freq` other than 1.25 GHz is refused until a table has been tested.

### Caveats

- **VCO calibration.** Each table ends with R0 (FCAL_EN = 1), so the calibration runs again with the new N and divider, from core 7
  down (FCAL_VCO_SEL_STRT 0). The datasheet's frequency-change sequence (N, NUM/DEN, then R0) is a subset of the full table, so
  rewriting all 46 words is safe. The calibration result cannot be read back.
- **No lock readback.** SDIO is write-only on this board, and MUXout (Vtune lock detect) is not readable. Lock can only be shown
  indirectly: by the FPGA clock counter 0x1210, whose window 62494..62506 is the 1.25 GHz value and must scale with the clock (see
  below; gate and divider are not known, `cold-clock.md`), and finally by clean ADC data (sync, eye scan, a known input signal at
  the right frequency).
- **Fractional tables.** The datasheet puts the primary fractional spur at f_PD / DEN from the carrier: 25 MHz for DEN 4 and 12.5
  MHz for DEN 8. The divider lowers its level by 20 log(CHDIV) but keeps the offset. How much of it the loop filter removes is
  unknown, because the filter is not documented. In the sample clock it shows up as spurs or jitter in every acquisition. Integer N
  is only possible at 1.25 and 2.0 GHz. Other integer points would need another f_PD (MULT / PLL_R), which `table_for` deliberately
  does not touch.
- **Loop dynamics.** Charge pump (2.5 mA) and loop filter are designed for 5 GHz / N_total 50. K_VCO rises with frequency (Table
  7-3: about 30 MHz/V at 4 GHz, 47 at 6.2 GHz), roughly as N_total does, so the loop gain stays similar (K_VCO / N_total: 0.72 stock,
  about 0.74 for 4 GHz / N 40). Unverified.
- **FPGA timing.** The stock bitstream is timing-closed for a 156.25 MHz ADC data clock (1.25 GHz / 8). At 2 GHz the same logic runs
  at 250 MHz (+60 %), at 1.5 GHz at 187.5 MHz (+20 %). Whether the capture, the ADC lane deserialisers and the DDR write path keep
  up is unknown. Failures show up as eye-scan failures, wrong data or a hang.
- **ADC.** The ADC's own maximum clock and any rate-dependent register set-up (init table, IDELAY eye at a shorter bit period) are
  unknown. The DHO1000 evidence is only an indication.
- **Generator.** The DDS of the built-in generator runs on the FPGA clock (ADC clock / 8), so its output frequency scales by
  f/1.25 GHz unless the phase increment is recomputed for the new clock.
- **Order of testing.** 1.375 GHz (+10 %) first; check 0x1210 and the eye scan at each step. Keep the stock table at hand: a power
  cycle restores 1.25 GHz, because every boot writes `PLL_TABLE`.

### Places in OOS that assume the 1.25 GHz ADC clock or the 156.25 MHz derived clock

These are parameterised through `hal::dho::clock` (limits.conf `adc.clock_hz`, `models.md` 4.1), except the display-only UI defaults
(`--pre` preamble, demo simulator, `ui/src/board/dho/hw.rs` scan dt guess 0.8 ns) and the tests, which stay at 1.25 GHz. The table lists
the places as they were before that change.

| place | assumption |
|---|---|
| `hal/src/dho/boot/regs.rs` `PLL_TABLE` (and `hal/src/dho/boot/mod.rs` step 2, `fpgad pll`) | the only table ever written: 1.25 GHz |
| `hal/src/dho/boot/mod.rs` `ADC_CLK_MIN/MAX` 62494..62506 (logged in step 8, checked after step 12; `fpgad status`), `hal/src/dho/boot/sim.rs` `adc_clk` 62500 | 0x1210 count at 1.25 GHz; scales with the clock if the gate comes from another clock |
| `hal/src/dho/boot/sim.rs` `UNIT_IDELAY`, `hal/src/dho/boot/wnd.rs` `reference_window` (run > 14 taps), `hal/src/dho/boot/regs.rs` 32-step scan | lane eye centres and minimum window width in IDELAY taps at 1.25 GHz; the tap delay is fixed, the bit period shrinks with the clock |
| `hal/src/dho/hz.rs` `CLOCK` = 1_250_000_000 (`prep`, `requested_rate`) and `modify_rate` | maximum rate = CLOCK / ratio; the remap list of rates the 1.25 GHz clock cannot make |
| `hal/src/dho/acq.rs` `configure`: `max_rate = 1_250_000_000 / ratio` | SPU divider base |
| `hal/src/dho/spu.rs` `sample_rate`: 62.5 MSa/s threshold and `62_500_000 / rate` | 62.5 MHz = 1.25 GHz / 20, a derived clock (assumed) |
| `hal/src/dho/roll.rs` `MAX_RATE` = 156_250_000, `depth_for` (78_125_000) | roll stream: one sample per 156.25 MHz FPGA clock |
| `hal/src/dho/afg.rs` `CLOCK_HZ` = 156.25e6, `increment` and FM deviation (hard-coded `156_250_000_000_000.0`), edge-time clock | generator DDS clock = ADC clock / 8 |
| `hal/src/model.rs` `Limits::standard`: `max_srate` [1.25e9, 625e6, 312.5e6], `adc_clock_hz`, `afg_clock_hz`; `afg.*_max_hz` clamp to afg_clock / 2 | sample-rate and generator limits (no limits.conf key for the clocks) |
| `hal/src/dho/afe.rs` `auto_c`: `fs / 19_841_269` (= 1.25e9 / 63) | AFE auto filter code relative to 1.25 GSa/s |
| `hal/src/dho/tpu.rs` `TICK_FS` 1.6 ns, `TICK64_FS` 6.4 ns (trigger t1/t2, holdoff, R timeout), video `VideoClk::Stock` 6.4 ns | 6.4 ns = one 156.25 MHz clock, 1.6 ns = 2 ADC periods (assumed); `VideoClk::Fast` 4 ns and `scu.rs` auto-trigger 4 ns ticks are probably another (250 MHz) clock (assumed) |
| `ui/src/model.rs` `max_srate` (via `Limits`), `quantize_srate` | follow `Limits` automatically; decimation steps from the maximum rate |
| `ui/src/main.rs` default `--pre` preamble (dt 8e-10), `ui/src/wave.rs` simulator jitter `dt.min(8e-10)`, `ui/src/ui/sysinfo.rs` clock display | 0.8 ns sample period in the replay default and the simulator; display only |
| tests: `hal/src/dho/afe.rs`, `spu.rs`, `hz.rs`, `roll.rs`, `afg.rs`, `model.rs`, `hal/src/dho/boot/tests.rs`, `ui/src/board/dho/hw.rs`, `ui/src/ui.rs`, `fpgad` `status_interpretation` | expected values at 1.25 GHz / 156.25 MHz |
