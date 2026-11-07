# Cold start: why 0x1210 (ADC clock counter) reads 0

Statements marked "verified on the unit" were seen there. Everything else is assumed or not yet verified on hardware, as
stated. Steps and tables refer to `cold-boot.md`.

## Short answer

0x1210 counts the ADC data clock. That clock runs only after the ADC sync and the lane eye scan, so a cold FPGA reads 0
until then, with a correctly programmed PLL (verified on the unit):

| before step (`cold-boot.md` section 4) | 0x1210 | 0x1008 |
|---|---|---|
| 9 ADC calibration, 10 DDR taps, 11 sync | 0 | 0x20000000 |
| 12 eye scan (after the sync-delay sweep + SPI/SPU sync) | 17 774 | 0x20004061 |
| 13 core alignment (after the eye scan) | **62 497** | 0x2000f021 |
| 14..17 | 62 498 | 0x2000f021 |

Consequences for the boot order:
1. The self test at step 8 can only log 0x1210. Nothing may act on its value there.
2. The real check (window 62494..62506) comes after step 12.
3. Reprogramming the PLL alone does not bring 0x1210 up in a cold state: 0x1210 stays exactly 0, also with all GPIO levels
   of the stock OS applied (verified on the unit). The first sync must run.

The OOS PLL write path does reach the LMX2582 (verified on the unit, E2 in section 6).

## 1. Pinmux / pinconf of the PLL pins

### 1.1 Stock

With the stock OS running, after the PLL table was written (verified on the unit):

| pin | pinmux-pins | pinconf-pins | gpio debugfs |
|---|---|---|---|
| GPIO0_A2 (2) `spi_sdio` | MUX UNCLAIMED, gpio0:2 | pull down, 5 mA, level 0 | out lo |
| GPIO0_A3 (3) `spi_sclk` | MUX UNCLAIMED, gpio0:3 | pull down, 5 mA, level 0 | out lo |
| GPIO0_A6 (6) `spi_le` | MUX UNCLAIMED, gpio0:6 | pull down, 5 mA, level 0 | out lo |

The pulls and drive strengths of all GPIO0_A pins (A0 up, A1 up, A2 down, A3 down, A4 down, A5 up, A6 down, A7 up, 5 mA
each) are the RK3399 PMU-GRF reset defaults. No DT has pinctrl for these pins, so neither U-Boot nor the kernel changes
them (assumed: reset defaults per the TRM pattern). In the 4.4 kernel, "pin output (n level)" in pinconf-pins is the pad
level of every GPIO-muxed pin, not its direction.

`pwm@ff420030` (pwm3a on GPIO0_A6) is `okay` in the stock DT. The pin is still unclaimed because the stock kernel has no
pwm-fan driver (`board.md`), so pwm3 never takes it.

### 1.2 OOS

- DT: `pwm@ff420030` is disabled. No node refers to the pinctrl groups on GPIO0_A0/A1/A3/A4/A6/A7/B0 (`clk-32k`,
  `test-clkout0`, `sdio0-wp`, `pwm3a-pin`, ...). No hogs on gpio0. Pulls and drive therefore stay at the same reset
  defaults as stock.
- Kernel 6.12 (Rockchip BSP): a GPIO request goes through `gpiochip_generic_request` -> pinctrl
  `rockchip_pmx_gpio_request_enable`, which **forces the mux to GPIO**. The gpio range is registered with
  `base = pin_base` (CONFIG_GPIO_SYSFS=y), so line N of gpiochip0 really is GPIO0 pin N. Unlike 4.4, the direction set
  does not touch the mux, but the request already did.
- IO domains: same supplies (`pmu1830-supply` = `vcc_3v0` LDO8, 3.0 V) and same regulator flags as stock. All RK808 rails are
  `always-on`/`boot-on` in both DTs, with identical voltages (the only difference is `vcc_lcd` `enable-active-high`).

**Verdict:** no difference in function, pull, drive or IO voltage for GPIO0_A2/A3/A6 (assumed for the pad state, E4
checks it).

### 1.3 Other GPIO-muxed pins whose level contradicts their pull in stock (candidates for status inputs)

Unclaimed pins whose level is set from outside (verified on the unit, stock OS running):

| pin | stock | possible meaning |
|---|---|---|
| GPIO2_D3 (91) | pull down, **level 1** | unknown board input (assumed: LMX2582 MUXout = lock detect, R0 = 0x221c sets MUXOUT_SEL = 1) |
| GPIO4_C6 (150) | pull down, **level 1** | unknown board input (next to FG_M2C_GPIO GPIO4_C5) |
| GPIO0_A0 (0) | pull up, **level 0** | driven low from outside (PMU domain) |
| GPIO3_A3 (99), GPIO3_C0 (112) | pull up, level 0 | unknown (99 is an unused RGMII pin under RMII) |
| GPIO0_B0 (8), GPIO0_B4 (12) | against the pull | `hd_code1` / `hd_code3` straps, known |

If one of these lines is the PLL lock detect or a power-good signal, its level in a cold and in a warm state gives a direct
indicator (E4).

## 2. Start-up steps that could gate the ADC clock

### 2.1 `spi2boot`

`open("/dev/spidev1.0")`, bits 8, mode 0, 10 MHz; `write(fd, "\rBOOT\r", 6)`; `write(fd, addr BE, 4)`; a fixed wait. No
read. Byte-identical to `regs::boot_message` + `io::fpga_boot`. There is no second command. The script then sleeps 10 s.

Related: the stock `spidev` tool sends `\rVER?\r` and reads 19 bytes (golden-image version; used by `quick_boot_test.sh`).
The boot monitor in slot 0x400000 handles only `PROG` and `BOOT` (`fpga-image.md`). Nothing in the FPGA boot image sets up
board clocks.

### 2.2 Scripts

Stock OS boot chain:
- `start_rigol_app.sh`: focaltech, sleep 10, `spi2boot 0x400000`, sleep 10, `load_pcie.sh`, the seven `insmod`s,
  `spi2pll_lxm2582`, then only chmods/props/app start. No `pcie2dev`, no register write before the scope application.
- `load_pcie.sh`: insmod pcie-rockchip, xdma, sleep 1.5, chmod. Nothing else.
- `reload_fpga.sh`: (optional erase/flash), kill app, rmmod, `spi2boot`, sleep 10, insmod, `pcie2dev 0x4000` (read).
- `quick_boot_test.sh on` (quick power-on after the MCU cut the acquisition board): **`spi2pll_lxm2582` first**, then
  sleep 10 s (only with an old golden image), then `spi2boot`, `load_pcie.sh`. So PLL before FPGA in this path, FPGA
  before PLL in the cold-start script. Both orders are used.

### 2.3 Kernel modules: what they drive at `insmod`

| module | init |
|---|---|
| `spi2pll_lxm2582_gpio` | request 6 `spi_le` out **0**, then **set 6 high**; request 3 `spi_sclk` out 0; 2 `spi_sdio` out 0 |
| `fg_m2c_gpio` | 149 out 0 |
| `afg_gpio` | 122..126 out 0 (later set hi lo hi lo hi) |
| `dac_gpio` | 153, 154, 157, 158 |
| `beeper_gpio` | 131 out 1, set 1 |
| `fan_gpio_clt` | 133 out 1 |
| `spi2afe_gpio` | 76/66/69/68 CS, 75/74/73 bus |

PLL word on the bus: LE low, udelay 1; per byte: SCLK low, per bit MSB first: SDIO = bit, SCLK low, udelay 1, SCLK high,
udelay 1; SCLK low after the byte; then udelay 1, LE high, udelay 1, LE low, udelay 1. No R/W masking (addresses are below
0x80). Identical to `regs::pll_edges`. LE idles high only between module load and the first word. After that it idles low.

`spi2pll_lxm2582`: 46 x `write(fd, word, 3)` on `/dev/spi_3wires_lxm2582` (bytes `46 00 00 45 00 00 44 00 89 ...`, ends
`00 22 1c`). No reset word, no repeat of R0. The gap between words does not matter: OOS uses 50 ms, 1 ms also works.

### 2.4 Does anything else program the LMX2582 or a clock chip?

- No. `/dev/spi_3wires` belongs to the LMK04828, which is not part of the start-up (`pll.md`).
- The host-side FPGA download through `/dev/spi2k7_ctl` belongs to older hardware and is not on the start path.
- ADC words that exist but are not used at start: power mode (reg 0x0f bits 3:2), LVDS output (reg 0x0f bits 11:10),
  status read (reg 0x0f, `0x018f0000`, data ^ 0x7800), ADC-side sync (reg 0x00 bits 7:6 = 10, 10 ms, 00). Regs 0x54/0x55
  change only with the channel set and the band limit.
  So the only ADC writes at start are the init list, the stored calibration of mode 0 and the calibration sequences.

### 2.5 The difference that remains: ADC sync after the self test

Order (`cold-boot.md` 1.3): ADC init (3), SPU init (4), ..., **self test (12, log only)**, calibration (14): stored ADC
calibration (d), DDR taps (g), **sync delay sweep (i)** = the first `spi_sync(1)` -> 0x1000 bit 1 pulse -> `spi_sync(0)` ->
SPU reset of the boot. The SPI sync occurs only in the sync sweep, the eye scan and the core alignment. The SPU ADC sync
pulse occurs only in the sync sweep and the eye scan.

Consequence for ADC reg 0x00: after the init list it holds `0x05` (shadow 0xe4 ^ 0xe1). After the first sync sequence it
holds `0xc5` (shadow 0x24, verified on the unit). Before the first sync, reg 0x00 bits 7:6 are 00; afterwards they are 11.
A re-run of the init list without a sync does not stop an already running output clock (verified on the unit).

## 3. What 0x1210 is

- A read-only counter of the ADC data clock, window 62494..62506 at the 1.25 GHz clock (`(v - 62494) < 13`). Example value
  62497. It starts with the ADC sync and lane training (section "Short answer").
- Nobody writes 0x1200..0x121c. The neighbours are status registers: 0x1204 (bit 31 + u16, scan info), 0x1208 (bit 31 +
  30-bit, wave range).
- 62500 nominal: divider and gate time are not known. 1.25 GHz / 20 in 1 ms, 156.25 MHz (= 1.25 GHz / 8, one ADC core
  rate) in 0.4 ms and 625 MHz in 0.1 ms all give 62500 (assumed).
- 0x1000 bits: 0 SPU reset, 1 ADC sync, 2/3 mem reset, 4 Tx reset, 5 pre-proc reset. There is no known MMCM/PLL reset or
  clock-enable bit (0x1004 = acq mode/order/inversion, 0x1014 = cal select / bit range / IDELAY cal, 0x1020 = core align).
- 0x1008: bits 3:0 "mem status", bit 28, bit 29 DDR init pass, bit 31 BRAM fallback. Seen on the unit: 0x24000201 warm and
  running under the stock OS, 0x22000000 at the first cold check, then the values of the table above. The other bits
  change with the clock and sync state; their meaning is not known.
- ADC init words: of the 104 words, the clock-related ones are reg 0x01 (clock delay, 0x0000), regs 0x04/0x05 (sample-clock
  delays), reg 0x06 (sync delay 0x01c3), reg 0x0f (0x0201: power mode bits 3:2 = 00, LVDS bits 11:10 = 00) and reg 0x00
  (0x05). No word is known to select a clock source or a power-down. Bit 0 of reg 0x00 (written 1, shadow 0) could be a
  soft reset (assumed).

## 4. Survey / DT hints

- No clock or reset line for the ADC/PLL appears in either DT. The only gpio0 lines in use are the PLL bus and the power
  key (verified on the unit).
- Stock OS boot: 0x1210 reads 0 before `spi2pll_lxm2582` and 62 497 about a second later, with the scope application
  already started (verified on the unit). The FPGA boot is about 12 s earlier.
- The stock GRF io-domain driver logs "Setting to 3000000" twice. The PMU io-domain (ff320000) logs nothing in stock.
  OOS (6.12) sets PMUGRF_SOC_CON0 bit 8 and writes the bit-9 voltage select from `vcc_3v0`. Same voltage, so no effect
  expected (assumed).
- A warm reboot from OOS (FPGA re-initialised by OOS) into the stock OS hung the stock application (`adc-spu-init.md`). Some
  state that OOS leaves behind upsets its start.

## 5. Ruled-out list: re-checked

No difference from stock in pins, pinmux, pulls, drive, IO domain, regulators, the PLL bus protocol, the table or the
start-up order. None of them is the cause. GPIO levels are not the cause either: with afe1..4 = 1, afg_in1..5 = 1/0/1/0/1
and FG_M2C_GPIO = 0 held, a cold FPGA keeps 0x1210 at exactly 0 after the PLL table (verified on the unit).

Note: 0x1210 = 62494..62506 is the pass window, not a value at step 8.

## 6. Experiments on the unit (ranked; dry run first)

Register-level diagnostics for a cold start that fails. E1 (in the form of `--ignore-clock`, below) and E2 are verified on the
unit. The others are not yet verified on hardware.

Shadows at the state after step 7: s00 = 0xe4, s06 = 0x01c3, s1000 = 0, s1014 = 0x00f00001. Every 0x1034 write: 100 us
before and after.

**E0: stock cold-boot log (read only).** Power cycle into the stock OS and read the ADC clock, DDR init and sync lines of
its log as soon as the UI is up. Compare with section 3.

**E4: pin and pad read-out in the failed cold state (read only).** Run with the PLL lines still held by OOS or right after
`oos-fpgad pll`: `cat /sys/kernel/debug/pinctrl/*/pinmux-pins`, `.../pinconf-pins`, `cat /sys/kernel/debug/gpio`,
then compare with 1.1. Registers (devmem, read): PMUGRF GPIO0A_IOMUX 0xff320000 (bits 5:4, 7:6, 13:12 = 0 -> GPIO), GPIO0 DR
0xff720000, DDR 0xff720004 (bits 2, 3, 6 = 1), EXT_PORTA 0xff720050. Pad levels of the candidates of 1.3: GPIO2 EXT
0xff780050 bit 27 (D3), GPIO4 EXT 0xff790050 bit 22 (C6), GPIO0 bit 0, GPIO3 EXT 0xff788050 bits 3, 16. Optional pad check
without shifting into the LMX: toggle only `spi_sdio` (no SCLK edge, no LE edge) and read EXT_PORTA bit 2. A pin that reads 0
here but 1 in stock (and again 1 in the warm state) is a lock/power indicator. Reads only; the SDIO toggle is not latched.

**E1: continue the start-up sequence in a failed cold state.** Read 0x1210 and 0x1008 after each sub-step (3 reads, 100 ms
apart):
a) ADC SPI alive: `rd 0x1038` (expect bit 26 + last word 0x...0f0201); `wr 0x1034 0x01870000`, 100 us, `rd 0x1038` (ADC reg
   7); `wr 0x1034 0x018f0000`, `rd 0x1038` (reg 0x0f status, ^0x7800). Compare with the same reads in the warm state. All-0 or
   0xffff means the ADC is not responding or not powered.
b) Stored calibration of mode 0 (`cold-boot.md` 3.7, from `cal_adc.hex`).
c) One sync: `0x1034 <- 0x010600c3`; 20 ms; `0x1034 <- 0x01000045`; `0x1000 <- 0x0, 0x2, 0x0`; `0x1034 <- 0x010000c5`;
   `0x1000 <- 0x1, 0x0`.
d) The full sync sweep (`cold-boot.md` 3.3): r[0..7] (example 1,0,1,1,1,0,0,1), then lane scan, core align, ADC trim, and read
   0x1210 again.

`oos-fpgad cold-boot --ignore-clock` does this: it logs 0x1210 and 0x1008 before each of the steps 9..17 and only warns on a
bad count.

**E2: does the OOS GPIO path reach the LMX2582? (warm state.)** Precondition: 0x1210 in the window.
1) Write one word `[0x00, 0x22, 0x1d]` (R0 = 0x221c | POWERDOWN). Read 0x1210.
2) Full table with `oos-fpgad pll`. Read 0x1210.
Verified on the unit: 1) drops 0x1210 at once to about 17,000 (not 0: some residual clock is still counted); 2) brings it
back to 62,497. So GPIO bus, protocol and table program the LMX2582 completely, including the VCO calibration.

Risk: sample clock off for seconds, the ADC without its clock (its normal state at power-up). Recovery: table rewrite or
power cycle.

**E3: FPGA re-boot with the PLL already programmed (failed cold state).** `oos-fpga-up` again (`\rBOOT\r` 00 40 00 00, 10 s,
PCIe reload), then `cold-boot` steps 3..8 without the PLL step (+ E1c if still 0). This is the quick-power-on order of 2.2
(PLL before BOOT). A count of 62500 here would mean the FPGA design needs the ADC clock present at configuration.

**E5: MCU frames of the start (low prior).** Besides `FA 05 00`: LEDs off (`frontpanel.md`), power-on mode `FA FF 05 31 AF`.
Then PLL table + E1c. `FA 05 00` plus the PLL table alone does not bring up the count in a cold state (verified on the unit).

Not proposed: TI's "R0 again after 10 ms" (covered by repeated table runs); `\rVER?\r` to confirm the slot-0x400000 image runs
(0x4000 = 0xe2072115 and DDR pass already make a missed BOOT unlikely, and it is a monitor command outside OOS's BOOT-only
rule).

## 7. Consequences for `hal::boot`

Step 8 only logs 0x1210. Steps 9..12 run (at least the sync sweep and the eye scan). Then 0x1210 is a hard condition before
step 13, unless `--ignore-clock` is given, which turns the stop into a warning. The PLL stays step 2, after the FPGA boot.
