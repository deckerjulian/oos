# DHO924S hardware as seen by the RK3399 (stock device tree)

For a picture of the whole scope and how the parts work together, open the [hardware overview](https://deckerjulian.github.io/oos/hardware/overview.html) in a browser. See also
`gpio-map.md`.

Bootargs: `earlycon=uart8250,mmio32,0xff1a0000 swiotlb=1 coherent_pool=1m cma=514M`.

| Bus / block | Address | Devices | Use |
|---|---|---|---|
| SD (`dwmmc`) | fe320000 | card, 4 bit | system storage, boot |
| SDIO (`dwmmc`) | fe310000 | non-removable, 4 bit | probably unused WiFi slot |
| eMMC (`sdhci`) | fe330000 | enabled, 8 bit | not used for the system |
| GMAC | fe300000 | Motorcomm YT8512 (RMII, 100 Mbit) | Ethernet (`../network.md`) |
| PCIe | f8000000 | Zynq XDMA endpoint, `num-lanes = 4` (trains x2), `ep-gpios` GPIO2_C2 | acquisition data + registers |
| I2C0 | ff3c0000 | SYR827 @0x40 (vdd_cpu_b), SYR828 @0x41 (vdd_gpu), RK808 PMIC | power |
| I2C1 | ff110000 | ADC128D818 @0x37 | board voltage monitor (in the DT, chip absent on the DHO924S: `../protocols/board.md` 2.1) |
| I2C2 | ff120000 | FocalTech touch @0x38, Ilitek touch @0x41 (alternative) | touch |
| I2C4 | ff3d0000 | TMP421 @0x1d, 0x1f, 0x2a, 0x4c, 0x4f; MB85RC64 FRAM @0x50 | temperatures (in the DT, chips absent on the DHO924S: `../protocols/board.md` 2.1), FRAM |
| I2C8 | ff160000 | RX8010SJ RTC @0x32 (`drivers/rigol/rtc`) | RTC (absent on some units) |
| UART0 | ff180000 | front-panel MCU (keys, LEDs, power) | see `../protocols/frontpanel.md` |
| UART2 | ff1a0000 | debug console | earlycon |
| SPI1 | ff1d0000 | `spidev1.0`, 50 MHz max | FPGA boot monitor (`../protocols/fpga-boot.md`) |
| SPI2 | ff1e0000 | `spidev2.0`, pins taken over by `spi2afe_gpio` | AFE bus (`../protocols/afe-spi.md`) |
| PWM0 | ff420000 | backlight (enable GPIO1_C2) | |
| PWM3 | ff420030 | `pwm_fan` on GPIO0_A6 | **conflicts with PLL LE**, fan is really GPIO4_A5 |
| DSI0 | ff960000 | `simple-panel-dsi` | 7" 1024x600 JD9365 panel |
| HDMI | ff940000 | dw-hdmi | HDMI out |
| VOP big/lit | ff900000 / ff8f0000 | | display controllers |
| GPU | ff9a0000 | Mali-T860 | |
| SARADC | ff100000 | `rockchip-key` (vol up/down, menu) | unused keys |
| I2S0 / SPDIF | ff8a0000 / ff870000 | es8323 sound card node, dummy codec | beeper is a GPIO instead |
| USB3/DWC3 | fe800000, fe900000 | + Type-C PHYs, USB2 PHY host port | USB host port on the front (`../protocols/frontpanel.md`) |
| Rigol DMA client | | uses DMA channel 0 of `dma-controller@ff6e0000` | Rigol kernel driver (not in the module list, probably built in or unused) |

Rigol kernel modules of the stock system (all GPL): `xdma`, `pcie-rockchip`, `spi2afe_gpio`,
`spi2pll(_lxm2582)_gpio`, `afg_gpio`, `dac_gpio`, `fg_m2c_gpio`, `hdcode_gpio`, `beeper_gpio`, `fan_gpio_clt`,
`usbtmc_dev`, `usb_gpib`, `libcomposite`, `motorcomm`, `focaltech_ts(_gaozhan)`. Their source can be requested from
Rigol under the GPL.
