# Datasheets of the DHO800/DHO900 parts

Here are links to read up on the chips of the DHO800/DHO900 main board. Links only: no datasheet is kept in this
repository. Every link was opened on 2026-11-15 and the file checked for the part and revision. Links marked "browser"
refuse scripted downloads (analog.com, monolithicpower.com, mouser.com) or rate-limit them (the Wayback Machine) but
work in a web browser.

"How we know" says how the part is known to be on the board, in the terms of [CONTRIBUTING](../../CONTRIBUTING.md):

**Verified on the unit** means seen on a scope (ADR 0015); without more, on our DHO924S running OOS.

**Verified on the unit, in the forum** means seen first-hand by someone else on their own scope, with the model and the
link named (a chip marking on a board photo, a board traced or fitted by hand, a dump, a driver bound in the running
system); it is as good as the report, and a part seen on another model can differ on a DHO924S.

**Assumed** means from the stock device tree or a secondhand mention. The stock device tree lists parts that are not
fitted on every unit (`../protocols/board.md` 2.1), so a device tree entry alone counts as assumed. Parts and suppliers
can differ between units.

## RK3399 side (Linux, display, network)

| Part | Role | How we know | Document | Link |
|---|---|---|---|---|
| Rockchip RK3399 | SoC: 2x Cortex-A72 + 4x Cortex-A53, Mali-T860 GPU | verified on the unit: `CPU: rk3399` in U-Boot, 6 cores (A53 `410fd034`, A72 `410fd082`). Plain RK3399, not the RK3399K: verified on the unit, in the forum (DHO800, marking `RK3399 SBGTGAN51` on the [teardown photos](#sources); ours not opened) | Datasheet 2.1, Mar 2020 (also covers the RK3399K and its marking), 75 p | [Wayback copy](https://web.archive.org/web/20240629072726id_/https://armdesigner.com/download/Rockchip_RK3399_Datasheet_V2.1-20200323.pdf) (browser) |
| | | | Datasheet 1.8, May 2018, 75 p | [rockchip.fr](https://rockchip.fr/RK3399%20datasheet%20V1.8.pdf) |
| | | | TRM 1.3 Part 1, Dec 2016, 1045 p: CRU, GRF, cores, DMC, PMU, GIC, DMAC, TSADC, eFuse, WDT | [rockchip.fr](https://rockchip.fr/Rockchip%20RK3399%20TRM%20V1.3%20Part1.pdf) |
| | | | TRM 1.3 Part 2, Dec 2016, 1014 p: SD/MMC, USB, HDMI, MIPI D-PHY, UART, GPIO, I2C, I2S, SPI, GMAC, PCIe, SARADC | [rockchip.fr](https://rockchip.fr/Rockchip%20RK3399%20TRM%20V1.3%20Part2.pdf) |
| | | | TRM 1.4 Part 1, Apr 2017, 824 p (one condensed volume, no PCIe chapter) | [Khadas](https://dl.khadas.com/products/edge1/datasheet/rk3399-trm-v1.4-part1.pdf) |
| CXMT CXDB4ABAM | LPDDR4X for the RK3399, 2 GB each | verified on the unit, in the forum (DHO800, marking on the [teardown photos](#sources)); about 4 GB in all verified on the unit | V1.1, Jun 2020, 85 p | [LCSC](https://datasheet.lcsc.com/datasheet/pdf/8c9312289c15bd2d853fa87111845684.pdf?productCode=C39047067) |
| Rockchip RK808 | PMIC | verified on the unit, in the forum (stock system: the `rk808` interrupt thread in a [process list](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5782673/#msg5782673)); I2C0 in the stock device tree | Rev 1.6, Aug 2019 (RK808-B), 68 p | [LCSC](https://datasheet.lcsc.com/datasheet/pdf/c4ec874a9633efe39fc87c76b49f6c13.pdf?productCode=C427518) |
| Silergy SYR827 / SYR828 | bucks for the A72 cores / the GPU (I2C0 0x40 / 0x41) | assumed: stock device tree | application note Rev 0.1, 15 p (no full datasheet found) | [szlcsc](https://atta.szlcsc.com/upload/public/pdf/source/20170810/C111838_1502335158458879679.pdf) |
| Motorcomm YT8512 | Ethernet PHY, RMII, 10/100 Mbit/s | verified on the unit: `../network.md` | YT8512C/H V1.11, Apr 2020, 77 p | [LCSC](https://datasheet.lcsc.com/datasheet/pdf/69bf4263f0466d6ff91a07764856a68a.pdf?productCode=C2837480) |
| Jadard JD9365 | LCD driver of the 7" 1024x600 panel (MIPI-DSI) | assumed: stock device tree | JD9365DA-H3 V0.01 (preliminary), Aug 2020, 204 p | [Espressif](https://dl.espressif.com/AE/esp-iot-solution/JD9365DA-H3_DS_V0.01_20200819.pdf) |
| FocalTech FT5422 | touch controller (I2C2 0x38) | assumed: a FocalTech controller in the stock device tree; the exact type not checked | not public (given on request by FocalTech) | - |
| Fujitsu MB85RC64 (V or TA) | 64 kbit FRAM (I2C4 0x50) | an FRAM at I2C4 0x50: verified on the unit, in the forum ([dumped](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5308867/#msg5308867) from a running scope); the type from the stock device tree, V or TA assumed | MB85RC64V, 32 p; MB85RC64TA, 55 p | [V](https://www.mouser.com/datasheet/2/1113/MB85RC64V_DS8v1_E-3538525.pdf), [TA](https://www.mouser.com/datasheet/2/1113/MB85RC64TA_DS5v0_E-3538551.pdf) (browser) |
| Epson RX8010SJ | RTC (I2C8 0x32), not fitted on every unit | assumed: stock device tree, which may have taken the node from the DHO1000/4000 ([forum](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5374844/#msg5374844)) | application manual ETM37E-08, 41 p | [Epson](https://download.epsondevice.com/td/pdf/app/RX8010SJ_en.pdf) |

Neither the RK3399 datasheet nor the public TRM parts describe the display controller (VOP) or the video codec (VPU,
JPEG/H.264). For those the Linux drivers (`rockchip_drm`, `hantro`) are the reference.

## Zynq side (acquisition)

| Part | Role | How we know | Document | Link |
|---|---|---|---|---|
| AMD (Xilinx) Zynq-7000 XC7Z015, CLG485 | acquisition FPGA, PCIe endpoint for the RK3399 | verified on the unit (the stock bitstream it loads has IDCODE 0x0373B093, `../protocols/fpga-image.md`); the package verified on the unit, in the forum (DHO800, marking `XC7Z015 CLG485` on the [teardown photos](#sources)). Speed grade not legible, not known | DS190 Zynq-7000 overview 1.11.1, Jul 2018, 25 p | [AMD](https://docs.amd.com/api/khub/documents/juMnxca71Tf2gfjmNyjM8A/content) |
| | | | DS187 DC/AC switching characteristics (Z-7015) 1.21, Dec 2020, 73 p | [AMD](https://docs.amd.com/api/khub/documents/uaBd8Qxmf_K1~~tiVnOxrw/content) |
| | | | UG585 Zynq-7000 TRM 1.13, Apr 2021, 1825 p (51 MB). The current 1.15 is HTML only | [Wayback copy of 1.13](https://web.archive.org/web/20220216101545id_/https://www.xilinx.com/support/documentation/user_guides/ug585-Zynq-7000-TRM.pdf) (browser), [1.15 online](https://docs.amd.com/r/en-US/ug585-zynq-7000-SoC-TRM) |
| | | | UG865 packaging and pinout 1.9, Jul 2021, 124 p | [AMD](https://docs.amd.com/api/khub/documents/kOAtMqkVamwUDaIfCXO91A/content) |
| | | | UG470 7 series configuration (bitstream format) 1.17, Dec 2023, 174 p | [AMD](https://docs.amd.com/api/khub/documents/FOs3lXmlcWxBhTIFxVKyGA/content) |
| | | | UG471 SelectIO (IDELAY/ISERDES, ADC lane alignment) 1.10, May 2018, 188 p | [AMD](https://docs.amd.com/api/khub/documents/IbGcnPFe6eF19RHma_Y~IA/content) |
| | | | UG472 clocking 1.14, Jul 2018, 114 p | [AMD](https://docs.amd.com/api/khub/documents/1kFbRqzm2fhwGy~cLQG2yA/content) |
| | | | UG473 block RAM and FIFO 1.14, Jul 2019, 88 p | [AMD](https://docs.amd.com/api/khub/documents/9gZGbqBxtlKXxBfkBt~lAg/content) |
| | | | UG482 GTP transceivers (the PCIe lanes) 1.9, Dec 2016, 306 p | [AMD](https://docs.amd.com/api/khub/documents/Xk_zMbXsJj92suJ4_0Akkg/content) |
| | | | UG933 PCB design guide (DDR3) 1.13.1, Mar 2019, 82 p | [AMD](https://docs.amd.com/api/khub/documents/emfVNNoDnc5paYCtwGzwPg/content) |
| | | | UG821 software developers guide (boot image, FSBL) 13.0, Sep 2023 | [AMD online](https://docs.amd.com/r/en-US/ug821-zynq-7000-swdev) |
| | | | PG054 7 series integrated block for PCIe 3.3, Dec 2024 | [AMD online](https://docs.amd.com/r/en-US/pg054-7series-pcie) |
| | | | PG195 DMA/Bridge Subsystem for PCIe (XDMA, `/dev/xdma0_*`) 4.2 | [AMD online](https://docs.amd.com/r/en-US/pg195-pcie-dma) |
| Winbond W25Q128JV | QSPI boot flash of the Zynq (FPGA images, `../protocols/fpga-boot.md`) | verified on the unit, in the forum (DHO800, marking `25Q128JVSQ` on the [teardown photos](#sources)) | Rev I, Aug 2021, 79 p | [Winbond](https://www.winbond.com/resource-files/W25Q128JV%20RevI%2008232021%20Plus.pdf) |
| GigaDevice GDP2BFLM-CA or Samsung K4B4G1646E | DDR3L for the FPGA, 4 Gbit x16 | verified on the unit, in the forum: GigaDevice on a DHO800 (marking on the [teardown photos](#sources)), Samsung (3x) on a DHO914 ([forum](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5332838/#msg5332838)); the supplier varies | GDP2BFLM DS-00823 Rev 1.1, Jun 2023, 97 p; K4B4G1646E Rev 1.1, Jun 2016, 71 p | [oneyac (GigaDevice)](https://uploadcdn.oneyac.com/upload/document/1725588034384_6902.pdf), [LCSC (Samsung)](https://datasheet.lcsc.com/datasheet/pdf/a927c01e901237b62bc385890e3f7777.pdf?productCode=C2803247) |
| TI LMX2582 | sample clock PLL | verified on the unit (OOS programs it, `../protocols/pll.md`) | SNAS680E, Aug 2022, 48 p | [TI](https://www.ti.com/lit/ds/symlink/lmx2582.pdf) |
| Rigol RT8847IQ | ADC, 1.25 GSa/s | verified on the unit, in the forum (DHO800, marking `RIGOL RT8847IQ` on the [teardown photos](#sources)); `../protocols/adc-spu-init.md` | Rigol part: no public datasheet | - |
| Rigol RT1642 (four) | analog front ends, one per channel | verified on the unit, in the forum (DHO800: `RIGOL RT1642` on the [teardown photos](#sources), the rest of the marking too small to read); four front ends verified on the unit (OOS programs them, `../protocols/afe-spi.md`) | Rigol part: no public datasheet | - |

## Front panel, generator, logic analyser

| Part | Role | How we know | Document | Link |
|---|---|---|---|---|
| GigaDevice GD32F350K8 | front-panel MCU (keys, LEDs, power; `../protocols/frontpanel.md`) | verified on the unit, in the forum ([front-panel teardown](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg6364746/#msg6364746)) | GD32F350xx datasheet, 85 p (third-party copy) | [chipdip](https://static.chipdip.ru/lib/882/DOC017882831.pdf) |
| Sunmoon SM16306SJ (two) | front-panel LED drivers | verified on the unit, in the forum ([front-panel teardown](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg6364746/#msg6364746)) | V1.3, 16 p (third-party copy) | [aichiplink](https://file.aichiplink.com/static/lcsc/documents/2025-10-10/3d7f66966d9d6a44557e012c671b637f.pdf) |
| Analog Devices AD9744 | generator DAC (S models) | verified on the unit, in the forum (DHO914S: U1 of the [generator board circuit](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5654749/#msg5654749) traced from the board) | data sheet, 32 p | [ADI](https://www.analog.com/media/en/technical-documentation/data-sheets/AD9744.pdf) (browser) |
| 3PEAK TP1282 | dual op-amps of the generator and the logic analyser input | verified on the unit, in the forum (DHO914S [generator board](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5654749/#msg5654749); fitted by hand to DHO800 boards for the logic analyser: [1](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5413268/#msg5413268), [2](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5254209/#msg5254209)) | TP1281/2/4 datasheet, 25 p | [LCSC](https://datasheet.lcsc.com/datasheet/pdf/0c5d86e564be4e40f2d9e6a9a5f4522f.pdf?productCode=C2902348) |
| MPS MPM3630 | buck modules: next to the logic analyser port, and on the generator board | verified on the unit, in the forum (DHO914S [generator board](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5654749/#msg5654749); fitted by hand next to the LA port of DHO800 boards: [1](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5413268/#msg5413268), [2](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5254209/#msg5254209)) | Rev 1.02, 21 p | [MPS](https://www.monolithicpower.com/media/document/MPM3630_r1.0.pdf) (browser) |

## Not covered

Parts in the stock device tree that are not fitted on the DHO924S (TMP421, ADC128D818: `../protocols/board.md` 2.1)
and parts without a known part number yet (eMMC, USB hub, the power stages around the RK3399; their markings need a
board photo with the heatsink off).

## Sources

First-hand reports from the EEVblog forum, each made on the poster's own scope:

- Dave Jones' DHO800 teardown photos (EEVblog), including the main board with the heatsink off (chip markings of the
  RK3399, the Zynq, the ADC, the memories): [Flickr album](https://www.flickr.com/photos/eevblog/albums/72177720310962700),
  linked in the [hacking thread](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5338580/#msg5338580).
- The Zynq's I/O banks against the board layout (DDR3L on bank 13): [hacking thread](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5480500/#msg5480500).
- Front-panel board teardown (GD32F350K8, SM16306SJ): [hacking thread](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg6364746/#msg6364746).
- The parts that fit the empty logic-analyser footprints of a DHO800 (TP1282, MPM3630, DDR3L): [hacking thread](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5413268/#msg5413268).
- DDR3L on a DHO914 (3x K4B4G1646E-BYMA): [hacking thread](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5332838/#msg5332838).
- The generator board of a DHO914S traced into a circuit (AD9744, TP1282, MPM3630): [hacking thread](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5654749/#msg5654749).
- The RK808 driver running on the stock system (process list): [hacking thread](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5782673/#msg5782673).
- The FRAM at I2C4 0x50 read out from a running scope: [hacking thread](https://www.eevblog.com/forum/testgear/hacking-the-rigol-dho800900-scope/msg5308867/#msg5308867).
