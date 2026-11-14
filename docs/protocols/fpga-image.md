# FPGA boot image: SPI monitor, PS configuration, PL bitstream

What a developer needs to run on the FPGA side or to build an own image: flash slots, the SPI boot monitor, the PS
configuration and the bitstream properties. Only the `BOOT` command is verified on the unit. OOS never sends `PROG`, and it
runs no own image, so everything else here has not been verified on hardware.

## Short answer

1. **The monitor cannot load a PL bitstream from RAM.** The SPI monitor of the stock image knows exactly two commands, `PROG`
   (erase and program the QSPI flash) and `BOOT` (set the devcfg MULTIBOOT register and soft-reset the whole PS). It has no
   RAM, PCAP or devcfg-DMA command and no other receive path (section 2). The FSBL loads the PL only from the boot image
   in QSPI (assumed). `VER?` is not handled by this monitor: it is ignored, and the monitor never transmits anything. So
   `VER?` must be answered by the golden image in slot 0x000000 (assumed).
   Ways to an own bitstream, by cost:
   - **JTAG** on the Zynq (volatile, no flash), if the board brings out the JTAG pins. Not checked.
   - **One write to a spare flash slot** (for example 0x800000) with an own `BOOT.bin`. Its own PS monitor can then receive
     bitstreams over SPI (or later PCIe) and load them through PCAP from DDR/OCM. After that, no more flash writes. Slot
     0x000000 (golden) and 0x400000 (stock) stay untouched. A power cycle always comes back to the golden image (assumed).
     This is an FPGA flash write: OOS itself never does it (`cold-boot.md`, never-do list).
2. **PS side of an own design:** PS_CLK 33.333 MHz (as configured), CPU 500 MHz, DDR3 32 bit at 533 MHz (DDR3-1066, 512 MiB
   geometry), QSPI quad on MIO1-6, **SPI0 slave on MIO16/17/18/21** (the RK3399 link), UART1 115200 on MIO28/29 (debug
   console), nothing else on MIO (section 3). The FSBL sets up the PS DDR, but the monitor runs in OCM and does not use it,
   so it is free for an own design (assumed).
3. **Fabric clocks (FCLK) from the PS:** FCLK0 125 MHz, FCLK1 200 MHz, FCLK2 222.22 MHz, FCLK3 250 MHz (IO PLL 2000 MHz, /16,
   /10, /9, /8) (section 3.2). Level shifters are enabled, FCLK resets released. The ADC data clocks come in from the LMX2582 on
   PL pins (`pll.md`).
4. **Bitstream:** plain full XC7Z015 bitstream, not encrypted, not compressed, CRC checked, no PL multiboot (WBSTAR 0, TIMER 0),
   STARTUP on CCLK with the toolchain defaults (section 4).

## 2. SPI boot monitor

A standalone BSP application on the PS, the third partition of the boot image. It runs completely from OCM.

### 2.1 Peripherals used

| what | setting |
|---|---|
| SPI slave | **SPI0, 0xE0006000**, ref 100 MHz, options CLK_ACTIVE_LOW + CLK_PHASE_1, no MASTER bit: **slave, CPOL=1 CPHA=1** (mode 3). RX threshold reg (+0x2c) = 100, enable (+0x14) = 1 |
| QSPI flash | 0xE000D000, ref 100 MHz, single flash, options 0x130 (manual start, force SS, HOLD_B), prescaler 1 = /4 = 25 MHz |
| console | UART1 0xE0001000, polled TX |
| timer | global timer, 250,000,000 counts/s (500 MHz CPU) |

The host uses SPI mode 0 (`fpga-boot.md`). Mode 0 and mode 3 both sample on the rising edge, so the mismatch is harmless for
host-to-FPGA bytes. MISO is never driven with data (nothing writes the TX data register +0x1c).

Receive primitive `spi_rx(n)`: RX threshold = min(n,100), busy-poll SR (+0x04) bit 4 (RX FIFO >= threshold), then read n bytes
from +0x20. No timeout, no overflow handling.

### 2.2 Command parser

```
loop: c = spi_rx(1)
      c == '\r' : if strcmp(buf,"PROG")==0 -> PROG; elif strcmp(buf,"BOOT")==0 -> BOOT
                  else clear buf[5], idx = 0
      c == '\n' : ignored
      else      : buf[idx] = c; idx = (idx <= 3) ? idx+1 : 0
```

So a command is `\r` + 4 ASCII chars + `\r`. The leading `\r` flushes garbage. Anything else (`VER?` included) is dropped
silently. There are **only two commands**.

Both commands then read a **4-byte big-endian address** (`spi_rx(4)`).

### 2.3 `BOOT`

```
devcfg UNLOCK  0xF8007034 = 0x757BDF0D
devcfg MULTIBOOT_ADDR 0xF800702C = addr >> 15     (32 KiB units: 0x400000 -> 0x80)
SLCR_UNLOCK 0xF8000008 = 0xDF0D; PSS_RST_CTRL 0xF8000200 = 1
```

So `BOOT` = full PS system soft reset. The BootROM re-runs, reads the boot header at QSPI offset `MULTIBOOT*32K`, the FSBL there
re-runs `ps7_init`, loads that image's bitstream through PCAP and starts that image's monitor (assumed: Zynq multiboot flow per
UG585). Address bits below 32 KiB are dropped. The PL (and thus the PCIe endpoint) is reconfigured, which is why
`pcie-rockchip`/`xdma` must be reloaded afterwards. Verified on the unit for slot 0x400000 (`fpga-boot.md`).

### 2.4 `PROG`

Frame: `\rPROG\r` + addr (4, BE) + size (4, BE) + `size` raw data bytes. No checksum, no reply, no flow control.

1. Erase:
   - `size == 0x1000000` exactly: **bulk erase (0xC7) of the whole chip, golden image included.**
   - otherwise `(size >> 16) + 1` sector erases (0xD8, 64 KiB) from `addr & ~0xffff` (always one sector more than needed).
   - Flash commands: WREN 0x06, RDSR 0x05 poll WIP, PP 0x02, READ 0x03, RDID 0x9F. All use **3-byte addresses**, so only the
     first 16 MiB are reachable.
2. For each full 256-byte page: 4 x `spi_rx(64)`, page program, read back, compare. A mismatch only increments an error count
   that is printed on the UART.
3. Remainder `size & 0xff`: received in 64-byte chunks and programmed once. Not verified by the monitor.
4. Back to the command loop. **No reboot.**

The host must pace the stream: it has to wait out the erase and the per-page program time. The stock tools split this into
`spi2erase` (sends PROG + addr + size, then waits 20 s for the erase) and `spi2flash` (streams the data). Both refuse addresses
below 0x400000.

### 2.5 Consequences

- Flash slots: 0x000000 golden (answers `VER?`, assumed), 0x400000 stock image, 0x800000 (content not known; the factory DDR
  calibration of the stock OS programs a self-test image there, `cold-boot.md` 3.2). Boot image size 3.46 MiB, slot pitch 4 MiB.
  OOS boots only slots 0x000000 and 0x400000.
- The monitor stays alive while the PL runs. An image can therefore be re-commanded at any time. A stray `\rPROG\r` from the host
  would wait forever for 8 more bytes and then erase; a size of exactly 16 MiB erases the chip. OOS must never send `PROG`
  (`fpga-boot.md`).
- An own monitor that understands the same `\rBOOT\r` + address frame stays compatible with `spi2boot` / `oos-fpgad`.

## 3. PS configuration (`ps7_init`)

The FSBL applies this configuration on every boot. Silicon version 3.1. CoreSight is unlocked with 0xC5ACCE55 at 0xF8898FB0 /
0xF8899FB0 / 0xF8809FB0.

### 3.1 PLLs (PS_CLK = 33.333 MHz as configured)

33.333 MHz is assumed: it is the only input that makes SPI ref 100 MHz, the 250 MHz global timer and UART 115200 consistent.

| PLL | CTRL / CFG | FDIV | VCO | users |
|---|---|---|---|---|
| ARM | 0xF8000100 = 0x3c000, 0xF8000110 = 0x0fa240 | 60 | 2000 MHz | ARM_CLK_CTRL 0x1f000400: /4 -> CPU_6x4x **500 MHz**, CLK_621_TRUE = 1 (6:2:1: 250 / 166.7 / 83.3 MHz) |
| DDR | 0xF8000104 = 0x20000, 0xF8000114 = 0x12c220 | 32 | 1066.7 MHz | DDR_CLK_CTRL 0x0c200003: DDR_3x /2 = **533.3 MHz**, DDR_2x /3 = 355.6 MHz |
| IO | 0xF8000108 = 0x3c000, 0xF8000118 = 0x0fa240 | 60 | 2000 MHz | peripherals and FCLK |

A 2000 MHz VCO is at the top of the PS PLL range in DS187 and only allowed for the fastest speed grade (assumed). Check the part
marking.

### 3.2 Clock table

| register | value | result |
|---|---|---|
| DCI_CLK_CTRL 0xF8000128 | 0x00700f01 | DDR DCI clock, /15 /7 |
| LQSPI_CLK_CTRL 0xF800014C | 0x1401 | IO /20 = 100 MHz |
| UART_CLK_CTRL 0xF8000154 | 0x1402 | IO /20 = 100 MHz, UART1 only |
| SPI_CLK_CTRL 0xF8000158 | 0x1401 | IO /20 = 100 MHz, SPI0 only |
| PCAP_CLK_CTRL 0xF8000168 | 0x0a01 | IO /10 = 200 MHz |
| **FPGA0_CLK_CTRL 0xF8000170** | 0x00400400 | IO /4/4 = **FCLK0 125.000 MHz** |
| **FPGA1_CLK_CTRL 0xF8000180** | 0x00200500 | IO /5/2 = **FCLK1 200.000 MHz** |
| **FPGA2_CLK_CTRL 0xF8000190** | 0x00100900 | IO /9/1 = **FCLK2 222.222 MHz** |
| **FPGA3_CLK_CTRL 0xF80001A0** | 0x00200400 | IO /4/2 = **FCLK3 250.000 MHz** |
| CLK_621_TRUE 0xF80001C4 | 1 | 6:2:1 |
| APER_CLK_CTRL 0xF800012C | 0x01ec400d | on: DMA, USB0, USB1, SPI0, I2C0, I2C1, UART1, GPIO, LQSPI, SMC; off: GEM0/1, SDIO0/1, SPI1, CAN0/1, UART0 |

USB/I2C/SMC have no MIO pins (3.3). They are EMIO-only or simply left clocked (assumed).

### 3.3 MIO

| MIO | value | function |
|---|---|---|
| 1 | 0x1602 | QSPI0 CS (L0), pull-up |
| 2-5 | 0x0602 | QSPI0 IO0..3 |
| 6 | 0x0602 | QSPI0 SCLK (no feedback clock on MIO8) |
| 16 | 0x16a0 | SPI0 SCLK (L3 = 5), pull-up |
| 17 | 0x16a0 | SPI0 MISO |
| 18 | 0x16a0 | SPI0 SS0 |
| 21 | 0x16a0 | SPI0 MOSI |
| 28 | 0x16e0 | UART1 TX (L3 = 7) |
| 29 | 0x16e1 | UART1 RX (tri-state input) |

All are LVCMOS33 (bank 0 and bank 1 at 3.3 V) and slow slew. Every other MIO stays at reset default: no Ethernet, SD, USB PHY or
MIO GPIO. **SPI0 (MIO16-21) is the slave link to the RK3399 SPI1.**

### 3.4 DDR

- DDRC_CTRL 0xF8006000 = 0x80 (bus width bits 3:2 = 0, **32 bit**); PHY data slices 0xF8006118..0x6124 all in use;
  DDRIOB DATA0/DATA1 (0xF8000B48/4C) = 0x672, DIFF0/1 = 0x674, external VREF (DDR_CTRL 0xF8000B6C = 0x260).
- MR0 0xb30 (DLL reset, BL8), EMR 0x4 (0xF8006030 = 0x40b30): **DDR3** (assumed: DDR3 mode-register layout).
- Address map 0xF800603C/40/44 = 0x777 / 0xfff00000 / 0x0ff66666: 14 row, 10 column, 8 banks. Together with 32 bit this gives
  **512 MiB** (2 x 16-bit 2 Gbit). The register set equals the ZedBoard DDR3 preset except for timings (assumed).
- Clock 533.3 MHz (DDR3-1066). Last steps: poll DCI done (0xF8000B74 bit 13), start DDRC, poll operating mode (0xF8006054).

### 3.5 Peripherals / post config

- Peripherals: DDRIOB data/diff bits 7:8 set; UART1 BAUDGEN 0x7c, BDIV 6 (100 MHz / (124 * 7) = **115207 baud**), CR 0x17, MR 0x20
  (8N1); QSPI config bit 19 (HOLDB_DR); devcfg CTRL bit 29 (PCFG_POR_CNT_4K) = 0.
- Post config: **LVL_SHFTR_EN 0xF8000900 = 0xF** (PS<->PL level shifters on), **FPGA_RST_CTRL 0xF8000240 = 0** (FCLK resets
  released).
- FSBL and monitor print on UART1: MIO28 at 115200 8N1 is a boot log, if the pin can be reached on the board (not checked).

## 4. PL bitstream

An own bitstream goes into the second partition of a Zynq boot image (FSBL, bitstream, monitor). The stock image has these
properties, and a design for this board can use the same:

| property | value |
|---|---|
| device | XC7Z015, IDCODE 0x0373B093 |
| type | **full** bitstream: FAR = 0, WCFG, one FDRI write over the whole device (type 1 wc 0 + type 2 wc 877,084 = **8684 frames x 101 words**), GRESTORE, START |
| size | 28,085,504 bits (0x3591a0 bytes) |
| compression | none (no MFWR, no multi-frame writes) |
| encryption | none (no DEC bit, no AES packets; boot header key source 0) |
| CRC | on |
| PL multiboot | none: TIMER = 0, WBSTAR = 0, no IPROG. Multiboot is done by the PS (2.3) |
| COR0 | 0x02003fe5: GWE_CYCLE 6, GTS_CYCLE 5, LOCK/MATCH NoWait, DONE_CYCLE 4, **StartupClk = CCLK**, ConfigRate 0, DONE_PIPE = 1, DRIVE_DONE 0 |
| COR1 | 0: no readback CRC, defaults |
| CTL0 | MASK 0x401, CTL0 0x501: GTS_USR_B = 1, ConfigFallback = 1 (the toolchain default; irrelevant for PCAP), GLUTMASK_B |
| CTL1 | MASK 0, CTL1 0 |
| other | no BSPI, no AXSS/USR_ACCESS |

Byte order: the boot image stores each 32-bit configuration word **little-endian** (`66 55 99 AA` = 0xAA995566). Bootgen strips
the `.bit` header.

## 6. Implications for an own FPGA design

All of this is assumed, nothing has been run.

- An own `BOOT.bin` = an FSBL with the same PS7 preset (all values above) + own bitstream + own monitor. The monitor must keep
  the `\rBOOT\r` + BE32 address frame. It may add a RAM-load command (SPI at 10 MHz is about 1.2 MB/s, so ~3 s per full 3.5 MB
  bitstream; PCAP takes chunks from OCM or DDR, 512 MiB DDR3 is available).
- The own PL must at least provide the PCIe endpoint (hard PCIE_2_1 + GTP, Gen2 x2, BAR layout as in `fpga-boot.md`), or OOS loses
  its only data path. FCLK0..3 (125/200/222/250 MHz) are available as fabric clocks; the acquisition clocks come from the board PLL.
- The PL pin and IO-standard assignment is not documented here.
- Never use `PROG` with size 0x1000000 and never write below 0x400000. Keep the golden slot as the recovery path.
