# Other Rigol models

OOS runs on the DHO800 and DHO900 only. **Do not install it on another model**: its acquisition part is another
board, and the USB install and the card layout are verified on the DHO800/DHO900 only. The MHO900 has a build of the
platform alone, without acquisition, that has not run on a unit yet ([mho.md](mho.md)).

If OOS starts on another board anyway, it sees it in `/rigol/data/vendor.bin`: a model outside the DHO800/DHO900
family (not DHO8xx or DHO9xx) makes `oos-fpgad` refuse every hardware command and `oos-ui` show the demo signal
(`hal::model::Platform::drives_hardware`). `--model` and `model =` in limits.conf do not change that. Without a
readable vendor file OOS still assumes a DHO924S.

This page says what would carry over to another model, what would not, and how an owner can describe one without risk.

## What carries over

Reported for the MHO900/MHO98 and the DHO1000/DHO4000: the same RK3399 processor, Android started from an SD card, a
`/rigol` partition with `/rigol/data/vendor.bin`, root over network adb. If that holds, these parts of OOS should carry
over after a check:

- boot and updates (`hal/src/update`, `boot/`): the card layout must be read from the model's own partition table
  first, the offsets of the DHO are not assumed;
- network, WLAN, web remote, SCPI, the file and setup handling;
- most of the GUI (`ui/`) and all of `dsp/` (decoders, maths, measurements).

## Where the board ends

The code keeps the board apart:

- `hal/src/dho/` (`oos_hal::dho`): everything of the DHO800/DHO900 board. The crate root holds only what does not
  depend on it (`model`, `update`, `wlan`, `bootstatus`, `vendorkeys`).
- `ui/src/board/mod.rs`: the trait `Board`, the only way the GUI reaches the hardware: what the acquisition thread
  is to apply (`want`), its start (`spawn`), roll and scan (`slow_mode`), the generator (`gen`, `gen_clamp`, the
  arbitrary wave), the front-end steps, the front panel (port, commands, LEDs, keys) and the board readings. Frames come
  back in volts (`Frame`, made into screen records by the neutral `to_record`).
- `ui/src/board/dho/`: the DHO800/DHO900 behind it, the acquisition thread included. A lint test
  (`board::boundary`) fails when other code of the GUI uses `oos_hal::dho` or `board::dho`.

- `hal/src/target.rs`: the description of a board family (`Target`): card layout, update target, partition table
  marks, boot card controller, hardware variants (and which have run OOS), model table, standard limits, overclock steps. `hal/src/dho/target.rs`
  fills it in, and the neutral code reads `target::ACTIVE`.
- `rootfs/board/<name>/`: what the image takes from the board. `etc/oos-board` holds the same values for the scripts and
  the build (kernel tree, command line, modules, name of the update file); a hal test keeps it equal to `Target`.
  `bin/oos-board` is what `rcS` asks: `has-hardware`, `early`, `drivers`, `ui-args`, `bring-up`. `rcS` and the common
  scripts know no address and no tool of a board (hal test `the_common_scripts_know_no_board`).

- What the GUI takes from the board: the channel slots (`Board::MAX_CHANNELS`, the unit's own count from `Caps::channels`),
  the generators (`Board::MAX_GENERATORS`, `Caps::generators`; with more than one the generator page gets a selector and
  SCPI takes `:SOURce<n>`) and the couplings (`Board::couplings`, `Coupling::Dc50` for a 50 ohm path). The memory
  depths of the Acquire menu are data of the board in hal (`target::DEPTHS`), so the model reads them without the
  board. `oos_dsp::CHANNELS` must equal `MAX_CHANNELS`, the compiler checks it. With 2 generators, and with 2 or 6
  channels, the neutral code was tried on the host: it builds, and with 2 generators all tests pass.

A port adds `hal/src/<board>/` with its `Target`, `ui/src/board/<board>/` with its `Board` and `rootfs/board/<board>/`,
and points `target::ACTIVE`, `target::DEPTHS` and `board::Active` at them (the hal test
`the_depths_come_from_the_active_family` holds the first two together); the image is built with `OOS_BOARD=<board>`. The
compiler names every part of the interface still missing. `oos-fpgad` is a DHO800/DHO900 tool and stays one; only that
board's `oos-board` calls it. A board whose acquisition OOS cannot drive yet says so in `Target::acquisition`: then
`Platform::drives_hardware` is false and the screen shows the demo signal.

The MHO900 is the first such port, of the platform alone: `hal/src/mho/`, `ui/src/board/mho/`, `fpgad/src/mho/`,
`rootfs/board/mho/`, `kernel/dts/rk3399-rigol-mho900.dts` ([mho.md](mho.md), ADR 0028).

## Board families

One image per family, chosen when OOS is built; a family is ready when the maintainer says so in a pull request that
changes its row here, and only a ready family gets a release image
([ADR 0067](adr/hardware/0067-a-board-family-is-a-module-behind-target-and-board-one-image-each.md)). The recipe is in
[CONTRIBUTING.md](../CONTRIBUTING.md#a-new-board-family); `hal/tests/board_families.rs` reads this table and fails
when a row has no folder in `rootfs/board/` or a folder has no row.

| Family | Folder | Models | State |
|---|---|---|---|
| DHO800/DHO900 | `dho` | the variants in `hal/src/dho/target.rs` ([models.md](models.md)) | ready |
| MHO900 | `mho` | MHO934, MHO954, MHO984, MHO98 ([mho.md](mho.md)) | in development |

## What does not carry over

Everything that talks to the acquisition board, which on the DHO800/DHO900 is in `hal/src/dho/`:

| Part | DHO800/DHO900 | to find out per model |
|---|---|---|
| FPGA start, register map | Zynq XC7Z015 over PCIe, `protocols/fpga-boot.md` | the FPGA, how it starts, every register |
| ADC and lanes | one RT8847IQ, 1.25 GSa/s, `protocols/adc-spu-init.md` | ADC count and clock, interleaving |
| front end | four RT1642, `protocols/afe-spi.md` | chips, bus, ranges, 50 Ω path |
| sample clock | LMX2582, `protocols/pll.md` | clock chips and tables |
| trigger, record, play | `protocols/acquisition-start.md`, `record-play.md` | the same blocks in another FPGA |
| calibration | the files in `/rigol/data`, `hal/src/dho/calfile.rs` | file formats |
| generator, logic analyser | `protocols/afg.md`, `protocols/la.md` | other boards |

Reported for the MHO900 (forum, not verified here): two ADCs, interleaved to 4 GSa/s; a 1 GHz front end with a 50 Ω
path; a two-channel generator on its own board; a logic analyser path apart from the ADC words. The platform probe of
an MHO954 (GitHub issue #4) adds: a Kintex-7 bitstream the SoC loads at every start over GPIOs, XDMA `10ee:7024`, a
`sysref_reset` line (JESD204 ADCs), other front-end lines and calibration files ([mho.md](mho.md)). So a port means
working out the whole acquisition part again, measured on a unit at the bench.

The model limits (sample rates, memory depths, channel count, generator) live in `hal/src/model.rs`; the GUI takes
them from there.

## Describing a model: `tools/platform-probe.sh`

An owner of another model can run, on a PC with adb and the scope on the network with network adb on:

```sh
tools/platform-probe.sh <scope-ip>
```

It copies `tools/platform-probe/probe.sh` to `/data/local/tmp` on the scope, runs it, fetches the result and removes
both again. What it reads: `/proc` and `/sys` (CPU, memory map, buses, bound drivers, PCIe, I2C, USB, input,
display, clocks, regulators, GPIO and pin owners), the kernel log, the device tree as text, the partition table, the
first 2 KiB of the boot partitions, and the names and sizes of the files under `/rigol`.

What it does not: read any register, read the content of calibration, licence or key files, write anything outside
its own folder (it mounts debugfs, a pseudo filesystem that is gone after the next start). Device tree properties
named after a MAC address or serial number are left out; on the PC, the serial numbers it finds in the properties and
the command line, the SoC's serial number (`Serial` in `/proc/cpuinfo`) and every MAC address are replaced. Look
through the files before you share them anyway.

The result is `platform-probe-<model>-<date>.tar.gz`. Attach it to an issue on GitHub.

The script was run on a DHO924S under OOS (the part on the scope); the adb path itself has not run against a scope
yet.
