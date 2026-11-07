# FPGA boot and PCIe bring-up

## Who boots what

- The acquisition FPGA is a Zynq XC7Z015 (Rigol calls it "K7"). Its images live in a QSPI flash on the FPGA side, slots at
  0x000000 (factory/default), 0x400000 (the one `start_rigol_app.sh` boots), 0x800000.
- The RK3399 does not see that flash. It talks over **SPI1 (`/dev/spidev1.0`, pins GPIO1_A7/B0/B1/B2)** to a boot monitor
  on the FPGA side, which understands short ASCII commands framed by `\r`.
- After the image is running the FPGA brings up its PCIe endpoint (Xilinx XDMA, 10ee:7022, Gen2 x2). Only then may the
  RK3399 PCIe host driver probe. That is why `pcie-rockchip` is built as a module.

## `spi2boot <hexaddr>` (what boots the scope)

```
open("/dev/spidev1.0", O_RDWR)
SPI_IOC_WR_BITS_PER_WORD = 8      (ioctl 0x40016b03, then the RD variant)
SPI_IOC_WR_MODE          = 0      (ioctl 0x40016b01)
SPI_IOC_WR_MAX_SPEED_HZ  = 10 MHz (ioctl 0x40046b04)
write(fd, "\rBOOT\r", 6)
write(fd, addr as 4 bytes big endian, 4)        e.g. 00 40 00 00 for 0x400000
wait 3 to 10 s                                  (fixed wait for the image to boot; OOS 3 s, Rigol 10 s)
```

No reply is read. Success is only visible afterwards: the PCIe link trains and register 0x4000 reads the FPGA version
(0xe2072115 for the stock image).

The other monitor command is `\rPROG\r` (flash erase/program, `fpga-image.md`). The stock tools `spi2flash`, `spi2erase`,
`spi2fpga` stream bitstreams into the flash. **OOS never sends anything but `BOOT`.**

## Stock bring-up order (`start_rigol_app.sh`, `load_pcie.sh`, `reload_fpga.sh`)

1. `insmod focaltech_ts.ko`, `sleep 10`
2. `spi2boot 0x400000`, `sleep 10`
3. `insmod pcie-rockchip.ko`, `insmod xdma.ko`, `sleep 1.5`, `chmod 666 /dev/xdma0_*`
4. `insmod spi2afe_gpio fg_m2c_gpio afg_gpio dac_gpio beeper_gpio fan_gpio_clt spi2pll_lxm2582_gpio`
5. `spi2pll_lxm2582` (program the ADC sample clock PLL, see `pll.md`)
6. export GPIO 120, `chmod 666` on `/dev/{fg_m2c_gpio,vendor_storage,afg_gpio,dac_gear_change,spi*,pwm-1,i2c*}`
7. start the scope application (which then initialises clocks, ADC, AFE and the acquisition units through the XDMA BAR)

Recovery when PCIe is gone: stop the application, `rmmod xdma pcie_rockchip`, `spi2boot <addr>`, wait, `insmod` both again.

## Confirmed under OOS

Verified on the unit with Linux 6.12.115+, from the OOS development initramfs:
`oos-hw fpga-boot 0x400000` (spidev1.0, mode 0, 8 bit, 10 MHz), 10 s, `insmod pcierockchiphost.ko`: link at first try,
`10ee:7022` endpoint, 5.0 GT/s x2, BAR0 0xfa000000 (64 KiB), BAR1 0xfa010000 (32 KiB), same as stock;
`insmod xdma.ko poll_mode=1`: `/dev/xdma0_{control,bypass,c2h_0,h2c_0,events_*}`; register 0x4000 = **0xe2072115**
(the stock FPGA version), 0x4034 = 0x2020 (stopped), 0x4044/0x4048 = FPGA DNA. Scripted as `oos-fpga-up`, register access as
`oos-hw rd/wr` (`tools/hw/oos-hw.c`).

## OOS plan for this step (`oos-fpgad` start-up)

1. open `/dev/spidev1.0`, send `\rBOOT\r` + 00 40 00 00 (slot from a config file, default 0x400000)
2. poll instead of sleeping: `modprobe pcie_rockchip_host`; if no link, `rmmod` and retry every 500 ms up to 15 s
   (the RK3399 host driver cannot retrain after one failed attempt without a reload)
3. `modprobe xdma poll_mode=1`, check BAR1 0x4000 == expected version, else fall back to slot 0x000000 once
4. never touch the flash commands
