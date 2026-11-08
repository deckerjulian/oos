# Other Rigol models

OOS runs on the DHO800 and DHO900 only. **Do not install it on another model**: its acquisition part is another
board, and the USB install and the card layout are verified on the DHO800/DHO900 only.

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

## What does not

Everything that talks to the acquisition board, which on the DHO800/DHO900 is in `hal/`:

| Part | DHO800/DHO900 | to find out per model |
|---|---|---|
| FPGA start, register map | Zynq XC7Z015 over PCIe, `protocols/fpga-boot.md` | the FPGA, how it starts, every register |
| ADC and lanes | one RT8847IQ, 1.25 GSa/s, `protocols/adc-spu-init.md` | ADC count and clock, interleaving |
| front end | four RT1642, `protocols/afe-spi.md` | chips, bus, ranges, 50 Ω path |
| sample clock | LMX2582, `protocols/pll.md` | clock chips and tables |
| trigger, record, play | `protocols/acquisition-start.md`, `record-play.md` | the same blocks in another FPGA |
| calibration | the files in `/rigol/data`, `hal/src/dho/calfile.rs` | file formats |
| generator, logic analyser | `protocols/afg.md`, `protocols/la.md` | other boards |

Reported for the MHO900 (forum, not verified here): two ADCs, interleaved to 4 GSa/s; an FPGA said to be a Fudan
part; a 1 GHz front end with a 50 Ω path; a two-channel generator on its own board; a logic analyser path apart from
the ADC words. So a port means working out the whole acquisition part again, measured on a unit at the bench.

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
the command line and every MAC address are replaced. Look through the files before you share them anyway.

The result is `platform-probe-<model>-<date>.tar.gz`. Attach it to an issue on GitHub.

The script was run on a DHO924S under OOS (the part on the scope); the adb path itself has not run against a scope
yet.
