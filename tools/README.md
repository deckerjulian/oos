# OOS tools

| Tool | Touches the scope? | Purpose |
|---|---|---|
| `sdcard-clone.sh list / read / write / verify` | card in a reader | clone the boot SD card to an image and onto the second card; refuses non-removable, virtual and system disks; `write` asks you to type the disk id |
| `sdcard-layout.py IMAGE [--extract DIR] [--compare DIR]` | no (reads an image or `/dev/rdiskN`) | IDB, vendor storage (MAC), PARM partition table, partition contents |
| `device-test.py --host IP` | over the network, read-only on the card | acceptance test of a running OOS with nobody at the scope; `--sim` runs it against `oos-ui --serve` on this computer |
| `scpi_validate.py --web-port 18080` | local desktop only | uploads analytic waveforms into R1-R3 and validates numeric SCPI, thresholds, source isolation, stopped panning and trigger/run/stop state; writes evidence to `local/scpi-validation.json` |
| `sim_validate.py --web-port 18080` | local desktop only | replaces R8 to validate full-cycle SSC, advanced jitter configuration, background generation/cancellation and safe generation limits; writes evidence to `local/sim-validation.json` |
| `device-deploy.sh IP [--restore]` | `/tmp` only, gone at the next power cycle | runs the oos-ui of this tree on a running OOS |
| `device-survey.sh [ip] [--keys]` | read-only over adb | inventory of the running stock system: USB/HID, input, GPIO owners, pinmux, ttyS0, PCIe, partitions |
| `platform-probe.sh IP` | read-only over adb, removes its files again | description of another model (MHO900, DHO1000, ...) for a port, ready to share: device tree, buses, display, partition table, `/rigol` by name; serial number and MAC addresses replaced ([docs/porting.md](../docs/porting.md)) |

`device-test.py` also has the tiers `la` (`--pico <port>`: D0-D2 on GP2-GP4 of a Pico with the openSciLab firmware) and `rf`
(`--rf <port>`: an HT008 on `--rf-ch`, CH2 by default). `--busy 3` names other channels with a cable, which are no open inputs.

`../boot/oos-bootimg.py` builds the `recovery` boot image for the stock loader. `../kernel/build-base.sh` rebuilds
the GPL kernel in Docker (everything in `../kernel/`).
