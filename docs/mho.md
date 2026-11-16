# MHO900: the platform without acquisition

An OOS build for the Rigol MHO900 (MHO934, MHO954, MHO984, MHO98): the screen, touch, network, the web remote, SCPI,
storage and updates of OOS on the MHO900's main board, with the **demo signal** in place of an acquisition. OOS has no
code for the MHO900's acquisition board, and this build touches none of it: no FPGA, no front end, no clock, no
generator, no front panel. The decision is
[ADR 0028](adr/system/0028-mho900-platform-port-without-acquisition.md); how a port is built in general is in
[porting.md](porting.md).

**Not yet verified on hardware: none of this has started on an MHO900.** It is built and checked on the host only. What
it rests on is the platform probe of one MHO954 (firmware 00.01.00, hardware 3, GitHub issue #4), compared with the
stock survey of a DHO924S. The probe of one MHO98 (the same firmware and hardware code) shows the same board: its device
tree, I2C devices, GPIO use, partitions and drivers are the MHO954's; only the licence files under `/rigol/data`
differ.

## What the MHO900 shares with the DHO800/DHO900

From the probe (reported by one unit, not checked on hardware by us):

| Part | MHO954 (probe) | Same as DHO800/DHO900 |
|---|---|---|
| SoC, memory | RK3399, 4 GB, the same memory map | yes |
| stock system | Android, kernel 4.4.126, started from the SD card | yes |
| card layout | `mtdparts` word for word as on the DHO (misc, boot, recovery, backup ... rigol at the same sectors) | yes |
| PMIC | RK808 with every rail at the same voltages, two SYR82x with the VSEL pins GPIO1_C7 / GPIO1_B6 | yes |
| panel | 1024x600 DSI, ID 93 65 04 (JD9365), the same timings and reset pin | yes |
| touch | FocalTech on i2c2 at 0x38, interrupt GPIO4_A6, reset GPIO4_A7 | yes |
| other I2C | ADC128D818, five TMP421, an MB85RC64 FRAM, RX8010 RTC | yes |
| Ethernet | RK3399 GMAC, Motorcomm PHY, reset GPIO3_B7 | yes |
| keys | `rk29-keypad` (SARADC keys of the reference design, power key) | yes |

## What differs

| Part | MHO954 (probe) | DHO800/DHO900 |
|---|---|---|
| FPGA | a bitstream the SoC loads at every start (`sparrow3_K160T.bit`, `sparrow3_RT688.bit`), over GPIOs (`programb` GPIO1_B5, `initb` GPIO2_C7, `done` GPIO2_C4) | Zynq XC7Z015 from its own flash |
| PCIe | XDMA `10ee:7024`, PERST# on GPIO1_A3 | `10ee:7022`, PERST# on GPIO2_C2 |
| sample clock | a 3-wire bus on GPIO1_C1..C3 and a `sysref_reset` line (GPIO1_C6): JESD204 ADCs (assumed from the name) | LMX2582 on GPIO0_A2/A3/A6 |
| front ends | the same AFE chip selects, plus four sets of select lines (`a`, `b`, `c` on GPIO3_D2..D7, GPIO4_A1..A3, GPIO4_C5, GPIO4_D0, GPIO4_D1) and four `afe_rms` inputs (GPIO4_D2..D5) | four RT1642 on the AFE bus |
| generator | an option on its own board with its own MCU firmware (`afg_uart`) | on the main board |
| fan | PWM1 (GPIO4_C6), 25 kHz, `pwm-fan` | on/off line GPIO4_A5 |
| beeper | not on GPIO4_A3 (that pin is an AFE select line) | `pwm-gpio` on GPIO4_A3 |
| backlight enable | none (GPIO1_C2 is the PLL's chip select) | GPIO1_C2 |
| calibration | other files (`cal_ac_chan*`, `cal_hfflat_chan*`, `cal_lfflat_chan*`, `cal_offset_chan*`, `cal_afe_bandwidth` ...) | `hal/src/dho/calfile.rs` |
| WLAN stick shipped | RTL8821CU (`0bda:c820`) | RTL8188EU |

## What the build does

- **Kernel**: the same kernel as the DHO, with the device tree `kernel/dts/rk3399-rigol-mho900.dts`. It takes the DHO
  tree and leaves out what is the DHO's: no PCIe, no spi1, no beeper on GPIO4_A3, no backlight enable on GPIO1_C2, no
  fan line on GPIO4_A5. The fan runs on PWM1 at full speed from the start (`pwm-fan`, `CONFIG_SENSORS_PWM_FAN`). The
  MHO's acquisition pins are not named and not driven.
- **Image**: `rootfs/board/mho`: no PCIe or xdma module, the touch driver, `/rigol` mounted read only (vendor.bin
  for the model and the MAC). `oos-board has-hardware` says no, so rcS starts no bring-up and oos-ui starts without
  `--hw`.
- **oos-ui**: `ui/src/board/mho` behind `Board`. It starts no acquisition thread, applies nothing, drives no generator
  and opens no front panel port (its protocol on the MHO900 is not known): touch is the input, and a USB mouse or
  keyboard where the build has them. Scales 500 uV/div to 10 V/div, couplings DC, AC, GND and 50 ohm, depths to 100 M:
  from the data sheet, **assumed**; they only shape the demo. A notice on the screen says that OOS does not acquire.
- **oos-hal**: `hal/src/mho` describes the board for the neutral code (`Target`): the card layout (the DHO's), the
  models, `acquisition: false`. `Platform::drives_hardware` is false in this build, so `oos-fpgad` refuses every
  hardware command and oos-ui drops `--hw`, whatever the vendor file says.
- **Updates**: packages of this build are for `mho900-recovery`; a DHO package is refused on an MHO and the other way
  round. The first install takes no MHO900 model by itself (no variant has run OOS): it needs `--any-model`
  (`oos-any-model.txt` on the stick).

Not in it: power off from the front panel (the panel MCU is not spoken to; switch off at the power key or the
mains), the beeper, the FPGA temperature, the MHO's WLAN stick (RTL8821CU needs `rtw88`, not in this image yet).

## Build

```sh
kernel/build-base.sh                         # one kernel for both boards; the build makes every device tree
OOS_BOARD=mho boot/build-dev-image.sh        # boot/out/oos-dev.img for the MHO900, and MHO900_OOS_Install.GEL
```

The Rust crates take the board as a feature: `cargo build --features board-mho` in `ui/` and `fpgad/` (it passes
`oos-hal/board-mho` on). `tools/check.sh` builds that for the host and the scope in its `target` layer and runs the
board's own tests in `test`. The UI's tests are those of the DHO800/DHO900 and run without the feature.

## Trying it on a unit

Nobody has. Whoever tries takes a risk the DHO800/DHO900 does not have: the kernel and device tree have never started
on this board. What keeps it small:

- Work on a **copy of the SD card** (docs/sdcard-backup.md) and keep the original card aside. The install writes the
  boot selection and the `recovery` and `backup` partitions of the card it runs on; the original stays untouched.
- The first install starts OOS **once** (one-shot): the next power-on starts the stock system again. An OOS that hangs
  is left by switching off.
- The stock MHO900 update may not take the `.GEL` file: it is not known which file names it accepts. Over ADB the
  same install runs without it (`oos-update gel-install ... --any-model`, docs/install-usb.md).
- What to look for: the panel lights up and shows OOS, touch works, Ethernet gets an address, the fan runs, and
  `dmesg` from a shell (telnet or SSH) says what did not come up.

## Next: the acquisition

[protocols/mho-acquisition.md](protocols/mho-acquisition.md) describes the acquisition board as the stock MHO900 system
uses it (**not yet verified on hardware**), and `hal/src/mho/acq.rs` keeps it as data: the start-up order (supplies on
GPIO0_A1, GPIO0_A2 and GPIO1_A0, the LMK04821 clock, the Kintex-7 image per hardware code, PCIe), the register blocks,
the ADCs' SPI bridge at 0x3000 and the read-out format. The main result for a port: the register window, the
read-out stream, the run and read-out registers of the SCU and the sample format (7500 codes per division around
32768, the DHO's interleave) are the DHO800/DHO900's. OOS's decode and its measurements therefore carry over once the
board delivers frames.

An owner can check these assumptions under the stock system with `tools/mho-check.py <scope-ip>` (read-only on the
hardware; over SCPI it changes settings and sets them back) and attach the report to the GitHub issue.

Still to be measured on a unit at the bench: the clock and ADC settings, the JESD204 link, the front ends and their
select lines, the calibration files, the generator board and the front panel. Nothing of it is turned on before it
has run on a unit (ADR 0010); `Target::acquisition` stays false until then.
