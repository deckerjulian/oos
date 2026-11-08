# DHO924S GPIO map

Verified on the unit: every pin below is claimed with the label given here, PCIe PERST# as `ep` (GPIO 82, out high),
GPIO 120 via sysfs (input, low with no LA probe). This is the state under stock.

Numbering: Linux 4.4 Rockchip global number = bank * 32 + offset, offset = letter(A=0, B=8, C=16, D=24) + pin.
Linux 6.12 numbers gpiochips dynamically: in OOS always address pins as `gpiochipN` + offset (libgpiod) or by DT phandle.

## Bit-banged buses (Rigol modules, pins fixed in the module)

| Global | Pin | Module | Label / role | Notes |
|---:|---|---|---|---|
| 2 | GPIO0_A2 | `spi2pll_lxm2582_gpio` | `spi_sdio` | LMX2582 3-wire data (le=6, sclk=3, sdio=2) |
| 3 | GPIO0_A3 | `spi2pll_lxm2582_gpio` | `spi_sclk` | LMX2582 3-wire clock |
| 6 | GPIO0_A6 | `spi2pll_lxm2582_gpio` | `spi_le` | LMX2582 latch enable. **Same pin as `pwm3a`**, which the stock DT gives to `pwm@ff420030` / `pwm_fan` -> conflict, see below |
| 66 | GPIO2_A2 | `spi2afe_gpio` | `spi_afe_cs_1` | AFE CH2 chip select (active low, idle high) |
| 68 | GPIO2_A4 | `spi2afe_gpio` | `spi_afe_cs_3` | AFE CH4 chip select |
| 69 | GPIO2_A5 | `spi2afe_gpio` | `spi_afe_cs_2` | AFE CH3 chip select |
| 73 | GPIO2_B1 | `spi2afe_gpio` | MISO | = `spi2_rxd` |
| 74 | GPIO2_B2 | `spi2afe_gpio` | MOSI | = `spi2_txd` |
| 75 | GPIO2_B3 | `spi2afe_gpio` | SCLK | = `spi2_clk` |
| 76 | GPIO2_B4 | `spi2afe_gpio` | `spi_afe_cs_0` | AFE CH1 chip select, = `spi2_csn0` |

The AFE bus uses exactly the pins of the RK3399 hardware controller SPI2 (`spi@ff1e0000`), plus three extra GPIO
chip selects. OOS can drive it with the real controller: `spi2 { cs-gpios = <&gpio2 RK_PB4 ..>, <&gpio2 RK_PA2 ..>,
<&gpio2 RK_PA5 ..>, <&gpio2 RK_PA4 ..>; }` (the stock DT enables `spi2` with a `spidev`, but the module takes the
pins over as GPIOs). Bus format: see `../protocols/afe-spi.md`.

## Simple GPIOs (Rigol modules)

| Global | Pin | Module / device | Label | Direction | Notes |
|---:|---|---|---|---|---|
| 4 | GPIO0_A4 | `hdcode_gpio` `/dev/hdcode_gpio` | `hd_code0` | in | hardware revision code bit 0 |
| 8 | GPIO0_B0 | `hdcode_gpio` | `hd_code1` | in | bit 1 (pin is also `sdmmc-wp`, unused: `disable-wp`) |
| 11 | GPIO0_B3 | `hdcode_gpio` | `hd_code2` | in | bit 2 |
| 12 | GPIO0_B4 | `hdcode_gpio` | `hd_code3` | in | bit 3 |
| 120 | GPIO3_D0 | exported via sysfs under stock, mode 666 | `LA_PROBE_DETECT` | in | logic-analyser probe detect |
| 122 | GPIO3_D2 | `afg_gpio` `/dev/afg_gpio` | `afg_in1` | out | AFG output path relays / attenuators (5 bits) |
| 123 | GPIO3_D3 | `afg_gpio` | `afg_in2` | out | |
| 124 | GPIO3_D4 | `afg_gpio` | `afg_in3` | out | |
| 125 | GPIO3_D5 | `afg_gpio` | `afg_in4` | out | |
| 126 | GPIO3_D6 | `afg_gpio` | `afg_in5` | out | |
| 131 | GPIO4_A3 | `beeper_gpio` `/dev/pwm-1` (class "Beep") | `PWM_GPIO` | out | beeper, software PWM from an hrtimer |
| 133 | GPIO4_A5 | `fan_gpio_clt` | `fan_power` | out | fan on/off (the real fan control) |
| 149 | GPIO4_C5 | `fg_m2c_gpio` `/dev/fg_m2c_gpio` | `FG_M2C_GPIO` | out/in | generator supply switch |
| 153 | GPIO4_D1 | `dac_gpio` `/dev/dac_gear_change` | AFE 1 gear | out | |
| 154 | GPIO4_D2 | `dac_gpio` | AFE 2 gear | out | |
| 157 | GPIO4_D5 | `dac_gpio` | AFE 3 gear | out | |
| 158 | GPIO4_D6 | `dac_gpio` | AFE 4 gear | out | |

## GPIOs named in the stock device tree

| Pin | Node | Role |
|---|---|---|
| GPIO0_A5 | `rockchip-key/power-key` | power key (active low, wakeup) |
| GPIO0_A7 | `sdmmc-cd` | SD card detect |
| GPIO1_C2 | `backlight enable-gpios` | panel backlight enable (PWM0 on GPIO4_C2 sets the level) |
| GPIO2_C2 | `pcie ep-gpios` | PCIe PERST# to the FPGA. **Also claimed by `uart0-cts`** in the stock DT (`uart0` pinctrl-0 = xfer + cts) |
| GPIO1_C7 / GPIO1_B6 | `syr827/828 vsel-gpios` | CPU-big / GPU regulator voltage select |

## Pin conflicts of the stock device tree

All three are resolved in the board device tree OOS boots; seen on the unit.

1. **GPIO0_A6**: `pwm@ff420030` (pinctrl `pwm3a`) + `pwm_fan` vs. the LMX2582 latch enable. `pwm3` stays disabled, the pin is the
   PLL's `spi_le`, and the fan runs from GPIO4_A5, held on by a GPIO hog (no fan driver).
2. **GPIO2_C2**: `uart0-cts` vs. PCIe `ep-gpios`. The pin is PERST#; the front-panel UART runs on TX/RX alone. Whether CTS/RTS are
   wired is not verified, and nothing needs them.
3. **SPI2 pins**: `spidev` vs. the AFE bus. `spi2` is disabled, the pins are the GPIO lines `spi_afe_*`, bit-banged by `hal::dho::afe::GpioAfe`.
