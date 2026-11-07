# AFE bus (4x RT1642)

Register-level settings (V/div, offset, coupling, bandwidth) are in `afe-settings.md`.

## Physical layer

- Pins: SCLK GPIO2_B3, MOSI GPIO2_B2, MISO GPIO2_B1, CS CH1..CH4 = GPIO2_B4, GPIO2_A2, GPIO2_A5, GPIO2_A4
  (see `../hardware/gpio-map.md`). These are the SPI2 controller pins. The bus is bit-banged.
- Byte transfer: MSB first, 8 bits. SPI mode 0: clock idles low, data set before the rising edge. Each half period is
  5 us, so the clock is about 100 kHz.
- Chip selects are outputs, idle high, pulled low around a transfer.
- OOS drives the GPIO lines `spi_afe_sclk`, `spi_afe_mosi`, `spi_afe_miso`, `spi_afe_cs_0..3` directly (`hal::afe::GpioAfe`).
- The stock system exposes the bus as the misc device `/dev/spi_simulate`. It offers four SPI modes (default mode 0).

## Register level

Initialise the hardware in this order: AFE CH1..CH4, ADC, SPU, WPU (no hardware write), SCU, front-panel status
(`adc-spu-init.md`).

An AFE write is one transfer of 3 bytes under one chip select: `{reg, value >> 8, value & 0xff}`, that is **8-bit
register address + 16-bit value, MSB first**. One chip select = one channel. On `/dev/spi_simulate`,
`ioctl(fd, 0x7800, channel)` selects the chip select and `write(fd, buf, 3)` does the transfer.

AFE init per channel:
1. write 24 registers, same values for all channels:

| reg | value | reg | value | reg | value | reg | value |
|---|---|---|---|---|---|---|---|
| 0x20 | 0x0008 | 0x26 | 0x3200 | 0x2c | 0x000c | 0x32 | 0xc000 |
| 0x21 | 0x0a91 | 0x27 | 0x0044 | 0x2d | 0x0000 | 0x33 | 0x0088 |
| 0x22 | 0x5a00 | 0x28 | 0x0000 | 0x2e | 0x1ff9 | 0x34 | 0x0009 |
| 0x23 | 0x1000 | 0x29 | 0x0282 | 0x2f | 0x5980 | 0x35 | 0x0000 |
| 0x24 | 0x0000 | 0x2a | 0x3c40 | 0x30 | 0x0000 | 0x36 | 0x0000 |
| 0x25 | 0x0000 | 0x2b | 0x4c00 | 0x31 | 0x0000 | 0x37 | 0xc000 |

2. per channel: reg 0x06, reg 0x07, reg 0x27 |= 0x8080, reg 0x28 = 0x8000, reg 0x2b;
3. read the factory trim words 0x57 and 0x60..0x64 from the AFE's own efuse;
4. reg 0x2b again, then regs 0x2f..0x34 from the efuse words.

### Complete init sequence (implemented as `oos-hw afe-init <ch|all>`, verified on the unit)

Read: CS low, `reg | 0x80`, 16 bits in (sampled after each rising SCLK edge, MSB first), CS high. Only reg 0 carries
data out. Other addresses return a status pattern (0x0440/0x0000 seen).

Register values at start (per channel): r06 0x1d00, r07 0x8440, r27 0x0044, r2a 0x3c40, r2b 0x4c00. Start values of the
efuse targets: 57: 0x5980, 60: 0, 61: 0, 62: 0xc000, 63: 0x88, 64: 9.

1. the 24-entry table above
2. 0x06 <- 0x1d00, 0x07 <- 0x8440, 0x27 <- 0x80c4 (0x44 | 0x8080), 0x28 <- 0x8000, 0x2b <- 0x4f00 (| 0x300)
3. efuse words a = 0x57, 0x60, 0x61, 0x62, 0x63, 0x64: index
   `idx = ((((a & 0xf0) + 0x1c0) >> 1) & 0xf8) | (a & 0xf)` (0x0f..0x14); r2b |= 0x100, |= 0x200, r2a &= ~0x200,
   r2b[6:4] = (a + 1) & 7; then per bit i (16 bits, 5 for idx 0x14): select i in r2b[3:0] (r2a[2:0] for 0x14),
   r2b &= ~0x1000, r2a &= ~0x200, read reg 0, bit 0 = efuse bit i; afterwards r2b &= ~0x100, &= ~0x200
   (every change of r2a or r2b is written to the chip at once)
4. 0x2b <- r2b & ~0x300; 0x2f <- e57 | 0xc; 0x30 <- e60 & ~0x70; 0x31 <- e61; 0x32 <- 0x8000 | (e62 & 0x3fff);
   0x33 <- e63; 0x34 <- e64

The efuse words differ per chip and read back repeatably (verified on the unit).

## Register level, notes

- Registers are addressed by a byte and carry 16-bit values.
- Under the stock system the tool `spi2afe` reaches the same registers: `-s which_CS -m Mode -f function -r width`,
  `-w` writes a register, `-r` reads.
- Write only the values in this document and in `afe-settings.md`. The meaning of most bits is not known.
- `/dev/dac_gear_change` (GPIO4_D1/D2/D5/D6) switches a line per channel together with the AFE. Assumed: the offset DAC
  range. See `afe-settings.md` section 2.
