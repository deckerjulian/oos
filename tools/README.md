# OOS tools

Host tools of the tree. The second column says whether a tool touches a scope; one that writes to a scope is only run
when its owner asked for it in the session (AGENTS.md, ADR 0018).

## Checks and tests on this computer

| Tool | Touches a scope? | Purpose |
|---|---|---|
| `check.sh [layer ...]` | no | every host check CI runs, layer by layer (fmt, clippy, text, syntax, test, target, decoders); `check.sh list` names them ([ADR 0020](../docs/adr/process/0020-checks-in-layers-one-command-on-the-host.md)) |
| `plan-matrix.sh [oos-fpgad]` | no; read-only on a scope | the register sequences of a fixed set of acquisitions (`oos-fpgad plan` only prints), to compare two builds (ADR 0014); needs the calibration files (`OOS_CAL_DIR=local/cal`) |
| `compare-base.sh [BASE]` | no | the pictures (`ui/tools/layout-check.sh`) and the register sequences (`plan-matrix.sh`) of the working tree against those of a commit before the change, built in a tree of its own; the proof that a change leaves behaviour as it was (ADR 0014, ADR 0032); CI runs it for every pull request |
| `perf-guard.sh` | no | the ratio guards of the fast paths in a release build (layer `speed` of ADR 0020); needs a quiet computer |
| `pr-labels.sh NUMBER` | no; GitHub only (labels) | sets the labels a pull request gets by itself: areas by path, type by title, `agent`, `risk/hardware`; `--dry` only prints |
| `scpi_validate.py --web-port 18080` | no; a desktop build on this computer | uploads analytic waveforms into R1-R3 and validates numeric SCPI, thresholds, source isolation, stopped panning and trigger/run/stop state; writes evidence to `local/scpi-validation.json` |
| `sim_validate.py --web-port 18080` | no; a desktop build on this computer | replaces R8 to validate full-cycle SSC, advanced jitter configuration, background generation/cancellation and safe generation limits; writes evidence to `local/sim-validation.json` |
| `tests/` | no | the tests of the Python host tools, run by `check.sh test` |
| `oosclient.py` | no (a library) | the SCPI transport of `device-test.py`, `scpi_validate.py` and `sim_validate.py`: lines and definite blocks, read exactly |
| `fw-sim.sh` | no; Docker | oos-firewall's IPv6 part, `udhcpc.script` and `oos-net6` in an arm64 container with the image's busybox and nft |
| `ssh-sim.sh` | no; Docker | `/bin/oos-ssh` and Dropbear in an arm64 container: password, key, scp, sftp and the guards |
| `usb-sim.sh` | no; Docker, QEMU | `/bin/oos-usb` in a virtual machine with simulated disks, pulled while mounted |
| `wlan-sim.sh` | no; Docker, QEMU | the WLAN path in a virtual machine with simulated radios and access points |
| `gel-sim.sh [do_update.sh]` | no; Docker | the USB install file through the steps of the stock update, on a file laid out like a stock card |
| `unit-dtb-check.sh` | no; Docker | the DTB step of a unit profile (`eth.mac` as `local-mac-address`, [models.md 4.2](../docs/models.md)) |
| `image-manifest.py IMAGE DTB INITRAMFS` | no | what an image build put into the image, one line per file, to compare two builds |
| `deps-check.sh` | no; network | the pinned sources of `rootfs/deps.lock` against their upstream release listings |
| `release-sources.sh list / check TAG` | no | the source files a release carries next to its image, from `rootfs/deps.lock` and `kernel/base.sha256`; `check` compares a release on GitHub with the list ([THIRD-PARTY-LICENSES.md](../THIRD-PARTY-LICENSES.md)) |

## Models of the hardware

Reference models that the ports in `oos-hal` are tested against (hal/README.md). They read a unit's calibration files
from `OOS_CAL_DIR` (default `local/cal`) and touch no scope.

| Tool | Purpose |
|---|---|
| `adcmodel.py` | the ADC writes when the set of enabled channels changes ([adc-mode.md](../docs/protocols/adc-mode.md)) |
| `afesim.py` | the AFE write sequence per setting ([afe-settings.md](../docs/protocols/afe-settings.md)) |
| `hzmodel.py` | the horizontal parameters ([horizontal-config.md](../docs/protocols/horizontal-config.md)) |
| `tpumodel.py` | the trigger unit's register writes per trigger type ([trigger.md](../docs/protocols/trigger.md)) |
| `tpu_fixture.py` | writes `hal/testdata/tpu_expected.txt` from `tpumodel.py` |
| `frame-check.py FRAME ...` | a quick look at frames read by `oos-fpgad run --execute` |

## With a scope

| Tool | Touches a scope? | Purpose |
|---|---|---|
| `device-test.py --host IP` | over the network, read-only on the card | acceptance test of a running OOS with nobody at the scope; `--sim` runs it against `oos-ui --serve` on this computer |
| `device-deploy.sh IP [--restore]` | writes `/tmp` only, gone at the next power cycle | runs the oos-ui of this tree on a running OOS, over telnet or with `OOS_SHELL=ssh` over SSH; each file checked with SHA-256 |
| `device-survey.sh [ip] [--keys]` | read-only over adb | inventory of the running stock system: USB/HID, input, GPIO owners, pinmux, ttyS0, PCIe, partitions |
| `platform-probe.sh IP` | read-only over adb, removes its files again | description of another Rigol model (MHO900, DHO1000, ...) for a port, ready to share: device tree, buses, display, partition table, `/rigol` by name; serial number and MAC addresses replaced ([docs/porting.md](../docs/porting.md)); `platform-probe/probe.sh` is its part on the scope |
| `frontpanel.py dump / leds / selftest` | runs on the scope under OOS; `leds` writes the panel's LEDs | the front-panel MCU over UART0 ([frontpanel.md](../docs/protocols/frontpanel.md)) |
| `hw/oos-hw.c` | runs on the scope; `fpga-boot`, `wr` and `pll-init` write | minimal bring-up tool, built into the image by `boot/build-dev-image.sh` |
| `sdcard-clone.sh list / read / write / verify` | the card in the Mac's reader | clone the boot SD card to an image and onto the second card; refuses non-removable, virtual and system disks; `write` asks you to type the disk id |
| `sdcard-layout.py IMAGE [--extract DIR] [--compare DIR]` | no (reads an image or `/dev/rdiskN`) | IDB, vendor storage (MAC), PARM partition table, partition contents |
| `ssh-keys.py --data-dir DIR add / list / remove` | an offline mounted data directory only | manage Ed25519 public keys without a network connection; [SSH setup](../docs/ssh.md) |

`lib/` holds what several scripts source: `adb.sh` (connect to a stock system as root), `sim.sh` (the image's busybox
and the virtual machine of the sims), `docker.sh` (the base images, the arm64 check, and the build images tagged with a
hash of their Dockerfile, so an edit is built again). `check.sh sim` runs every sim and `../gpu/test-host.sh`.

`device-test.py` also has the tiers `la` (`--pico <port>`: D0-D2 on GP2-GP4 of a Pico with the openSciLab firmware) and `rf`
(`--rf <port>`: an HT008 on `--rf-ch`, CH2 by default). `--busy 3` names other channels with a cable, which are no open inputs.

`../boot/oos-bootimg.py` builds the `recovery` boot image for the stock loader. `../kernel/build-base.sh` rebuilds
the GPL kernel in Docker (everything in `../kernel/`).
