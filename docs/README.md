# OOS technical documentation

How the Rigol DHO800/DHO900 hardware works and how OOS drives it. "Verified on the unit" means seen on a running scope;
"assumed" or "not yet verified on hardware" means it was never run.

## System

- [architecture.md](architecture.md): how the SD card, kernel, Rust crates and GUI fit together.
- [boot.md](boot.md): card layout, boot chain, the image format of the stock loader, OOS start-up order.
- [install-usb.md](install-usb.md): installing OOS from a USB stick, without opening the scope.
- [update.md](update.md): signed update packages, trial start and rollback.
- [wlan.md](wlan.md): WLAN through a USB stick.
- [models.md](models.md): model identity, capabilities, limits, overclocking and `/oos/limits.conf`.
- [bandwidth-test.md](bandwidth-test.md): procedure to measure the analog bandwidth.

## Hardware

- [Hardware overview](https://deckerjulian.github.io/oos/hardware/overview.html): **start here.** A picture of the whole scope: components, signal paths,
  how the scope, the logic analyser and the generator work.
- [hardware/README.md](hardware/README.md): buses and on-chip blocks of the RK3399 and what is attached to them.
- [hardware/gpio-map.md](hardware/gpio-map.md): every GPIO with its pin, label and role.

## Protocols

- [acquisition-start.md](protocols/acquisition-start.md): RUN / SINGLE / STOP, auto trigger, frame fetch and re-arm.
- [adc-mode.md](protocols/adc-mode.md): what changes when the set of enabled channels changes.
- [adc-spu-init.md](protocols/adc-spu-init.md): init of the RT8847IQ ADC and the acquisition unit (SPU), data lane alignment.
- [afe-settings.md](protocols/afe-settings.md): AFE writes per setting: V/div, offset, coupling, bandwidth, invert.
- [afe-spi.md](protocols/afe-spi.md): the bus to the four RT1642 front ends.
- [afg.md](protocols/afg.md): built-in generator: DDS registers, relays, calibration, DAC codes.
- [board.md](protocols/board.md): fan, temperatures, voltages, MCU power commands, beeper, probes.
- [cold-boot.md](protocols/cold-boot.md): full sequence from power-on to the first acquisition.
- [cold-clock.md](protocols/cold-clock.md): the ADC clock counter 0x1210 on a cold start.
- [fpga-boot.md](protocols/fpga-boot.md): FPGA boot and PCIe bring-up.
- [fpga-image.md](protocols/fpga-image.md): `BOOT.bin`: SPI monitor, PS configuration, PL bitstream.
- [frontpanel.md](protocols/frontpanel.md): UART frames, key and knob codes, LEDs.
- [horizontal-config.md](protocols/horizontal-config.md): time base, memory depth, channel set, acquire mode.
- [la.md](protocols/la.md): logic analyzer D0..D15: enable, sample rate, thresholds, read-out.
- [pll.md](protocols/pll.md): sample clock: LMX2582 and LMK04828.
- [record-play.md](protocols/record-play.md): the record -> play chain and the read-out after an own arm.
- [roll-mode.md](protocols/roll-mode.md): roll and scan mode at slow time bases.
- [self-cal.md](protocols/self-cal.md): self-calibration and the calibration file container.
- [trigger.md](protocols/trigger.md): trigger unit (TPU, BAR1 0x2000 block).
