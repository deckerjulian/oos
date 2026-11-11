# OOS architecture

How the pieces of OOS fit together: from the SD card over the kernel to the Rust crates and the GUI. The register-level
details are in `protocols/`.

## Boot and system

```
SD card (stock layout, boot.md)
 ├─ boot       stock Android kernel; magic "KRNL" -> stock boots, "OOSK" -> the loader falls back to recovery
 ├─ recovery   OOS image (kernel 6.12 + DTB + initramfs), built by boot/build-dev-image.sh
 ├─ backup     copy of the stock boot image (fallback), rollback slot, /oos data partition (update.md)
 └─ rigol      stock data incl. /rigol/data/cal_*.hex; OOS mounts it read only (oos-mount-rigol)

initramfs /etc/rcS:  oos-oneshot (KRNL back, next power-on = stock) + start count -> oos-board early (FPGA BOOT command) ->
                     network in the background -> telnetd -> oos-board drivers (touch edt-ft5x06, beeper pwm-gpio) ->
                     oos-ui (start-up screen) -> in the background oos-mount-rigol (read only) +
                     oos-board bring-up (oos-fpgad cold-boot --execute --power-on) (order and status: boot.md)
FPGA:                cold boot by OOS (default); the warm hand-over from the stock system still works
```

Kernel: a 6.12 GPL kernel rebuilt in Docker (`kernel/`; sources and licences in `kernel/README.md`), with the DHO900 DTS (fan hog, pwm-gpio beeper,
AFE/PLL GPIO line names). PCIe host and XDMA are modules, loaded only when the FPGA is up.

## Rust crates

```
            ┌─────────────── oos-ui (ui/) ────────────────┐      ┌── oos-fpgad (fpgad/) ──┐
            │ GUI, SCPI :5555, web :8080, control FIFO    │      │ CLI: plan/run/status/  │
            │ sources: Demo | Replay | Live | Hw          │      │ rd/wr/cold-boot/temp/  │
            └──────┬───────────────────────────┬──────────┘      │ beep/guard             │
                   │                           │                 └───────────┬────────────┘
         ┌─────────▼─────────┐       ┌─────────▼──────────────────────────────▼─────────┐
         │ oos-dsp (dsp/)    │       │ oos-hal (hal/)                                   │
         │ pure signal       │       │ FPGA register sequences, AFE/ADC/PLL buses,      │
         │ processing        │       │ cold boot, panel MCU frames, board management    │
         └───────────────────┘       └───────────────────────┬──────────────────────────┘
                                                             │ Bus trait / GPIO chardev / sysfs
                                     /dev/xdma0_bypass (BAR1), /dev/xdma0_c2h_0 (stream), /dev/gpiochipN,
                                     /dev/ttyS0 (panel MCU), /sys/class/thermal, /sys/class/pwm
```

Rule for all crates: no vendor code is in the tree. Only dependency: `libc`.

### oos-hal: hardware layer

Pure functions build lists of `(offset, value)` writes and update software shadows of write-only registers, so the
host tests check them against fixtures from the Python models (`tools/*model.py`) and the device logs. Applying a list
is a separate step through `dho::bus::Bus` (`Bar1` on the unit, `Recorder` in tests).

The crate root holds what does not depend on the board: `model`, `update`, `wlan`, `bootstatus`, `vendorkeys`. Everything
of the DHO800/DHO900 board is in `oos_hal::dho` (`hal/src/dho/`); a port to another board family adds a module next to
it ([porting.md](porting.md)).

| Module (in `dho` unless noted) | Role |
|---|---|
| `bus`, `gpio` | BAR1 access (mmap of /dev/xdma0_bypass), GPIO character device by DT line name |
| `scu`, `spu`, `hz`, `acq`, `tpu`, `wpu` | FPGA blocks: run/stop/arm/read-out, sample rate, time base, acquisition configuration, trigger unit (12 types), read-path gain |
| `afe`, `adc` | RT1642 front ends (bit-banged SPI, efuse trim, V/div, offset, bandwidth), ADC modes and calibration |
| `seq` | settings -> complete step list (ADC mode, AFE, trigger, acquire, arm, read-out) with a persistent shadow state; execution and channel de-interleaving |
| `boot` | cold boot steps 1-21 (FPGA boot command, PLL, AFE, ADC, SPU/SCU, checks, calibrations) against a `Hw` bundle, plus a unit model for dry runs |
| `panel` | front-panel MCU frames: LEDs, power commands |
| `board` | XADC (FPGA temperature, VCCINT), thermal zones, over-temperature guard, beeper |
| `afg` | built-in generator (`protocols/afg.md`) |
| `clock` | ADC clock domain (`models.md`) |
| `model` (root) | model identity, capabilities and limits (`models.md`) |
| `update` (root) | signed updates with rollback (`update.md`) |
| `wlan` (root) | control socket of wpa_supplicant, its answers, key derivation (`wlan.md`) |

### oos-dsp: signal processing

Platform independent, no hardware. Software trigger for all 12 types, 35 measurements, math and FFT, UART/I2C/SPI
decoders, mask test, signal generators for tests and the demo source. Waveforms are `&[f32]` volts with a fixed `dt`.

### oos-ui: the scope screen

One process, software renderer on DRM dumb buffers (`drm`, `gfx`, `font`), layout in `ui`, state in `model`.

Protocol decoders that come as modules (`ui/src/protocols`, [protocol-modules.md](protocol-modules.md)) bring their
menu rows, trigger rules, labels, demo traffic and an optional view of their own through one trait, `Protocol`.

Everything the UI asks of the hardware goes through the trait `board::Board` (`ui/src/board/mod.rs`): what the
acquisition thread is to apply, the generator, the front-end steps, the front panel, the board readings. The UI speaks
in the model's terms and gets frames in volts back (`board::Frame`, `board::to_record`). `board::dho` implements it
for the DHO800/DHO900 and is the only part of the UI that uses `oos_hal::dho`; a lint test
(`board::boundary`) keeps it that way. The board is chosen when OOS is built (`board::Active`).

Threads:

| Thread | Job |
|---|---|
| main | input events -> model, acquisition step of the active source, analysis (dsp), render, page flip; reads the web, SCPI and VXI-11 connections without waiting (`net::read_now`): nothing in the loop may block on the network |
| input | front-panel UART (keys, knobs, LED frames) and touch (evdev) |
| hw (`--hw`) | the board's acquisition thread (`Board::spawn`; on the DHO800/DHO900 `board/dho/hw.rs`: `hal::dho::seq` plan/execute in a loop); hands frames over through `board::HwShared` |
| live (`--fpga-live`) | experimental, DHO800/DHO900: repeats the verified read-out (no arming) |
| scpi, http | SCPI server on 5555, web remote on 8080 (polled by the main thread) |
| web-png, web-send | the web remote's PNG of the screen (made only of a changed screen, at most 20 a second and half a core) and the large answers; SCHED_IDLE on the slow cores (A53), so the frame loop and the acquisition never wait for them |
| control FIFO | `/tmp/oos-ui.ctl` text commands (`cmd`), used by remote tests |
| ntp | SNTP exchange and clock step when Time from NTP is on (`sntp`, `ui/time.rs`; `network.md`) |

Sources (`wave::Source`): `Demo` (dsp generators, triggers checked by the software trigger), `Replay` (a saved frame),
`Live` (read-out loop), `Hw` (real acquisitions of the board; on the DHO800/DHO900 the TPU trigger is set to the
selected type, the software trigger finds the point again in the data).

### oos-fpgad: bring-up and service tool

A thin CLI over `hal`: prints any plan without hardware (`plan`, dry runs everywhere), runs it only with `--execute`,
checks the FPGA version before writing, refuses 0x1008 and the excluded FPGA slot. Board commands (`temp`, `beep`,
`guard`). Meant to become the daemon that owns the FPGA once oos-ui and SCPI share one acquisition engine.

## Data path of one acquisition (`hal::dho::seq`)

```
Settings (model) ── plan ──> steps: ADC mode words (0x1034) -> AFE (GPIO SPI) -> read-path gain (WPU) ->
                             trigger (TPU) -> acquisition configuration (SPU/SCU) -> auto trigger -> arm ->
                             wait 0x4080 bit30 -> read-out (play window, TX size) -> c2h read
c2h bytes ── deinterleave ──> per-channel u16 codes ── volts (scale, offset, cal) ──> f32 -> dsp -> screen
```

## Tests

| Crate | Host tests | What they pin down |
|---|---|---|
| hal | `cargo test` in hal/ | write sequences against fixtures and device logs, cold boot against the unit model, panel CRCs, board logic |
| dsp | `cargo test` in dsp/ | every trigger, measurement, decoder and math function against signals with known answers |
| ui | `cargo test` in ui/ | menus, rendering (screenshots), every trigger type triggers on the demo, storage |
| fpgad | `cargo test` in fpgad/ | option parsing, status interpretation |

Builds for the unit: `--target aarch64-unknown-linux-musl` (static, rust-lld).

## Lessons from the first device tests

- Read-out after an own arm needs the play-side chain in pass-through (`scu::passthrough`), else the live compression
  starves the TX and the c2h read times out (`protocols/record-play.md`).
- The read-path gain/offset (WPU) must be OOS's own: values left by the stock system shift the codes.
- 0x1210 counts the ADC data clock, which runs only after the ADC sync and eye scan: check it after step 12.
- UI state: settings survive oos-ui restarts within a boot (`/tmp/oos-ui.setup`); the settings of the scope itself
  (NTP, time zone, Ethernet) are in `/oos/system.conf` (`sysconf`, `network.md`), apart from the setups; the power key
  talks to the panel MCU (`FA 05 02`, dialog, `FA 05 01/03`; `protocols/board.md` 4).
