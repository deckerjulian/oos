# oos-hal: hardware layer of OOS

Rust library (only `libc`) shared by `oos-ui` and the coming `oos-fpgad`. It builds the FPGA register sequences of
the DHO800/DHO900 as pure functions (lists of `(offset, value)` plus software shadows of the write-only
registers) and applies them through `bus::Bus` (`bus::Bar1` = `/dev/xdma0_bypass` on the unit, `bus::Recorder` in tests).

No vendor code is used. The hardware is described in `../docs/protocols/`.

| module | content | source doc | state |
|---|---|---|---|
| `scu` | arm an acquisition (record path), stop sequence, play-last read-out, type-4 play | acquisition-start.md, adc-spu-init.md | read-out verified word-exact on the unit; arm not run yet |
| `hz` | time base / depth / channel mask -> rate, memsel, segment size, pre/post | horizontal-config.md (`tools/hzmodel.py`) | matches every value observed on the unit |
| `spu` | sample rate divider 0x10d0/0x10d8/0x10d4 | horizontal-config.md | static |
| `tpu` | all trigger types: edge, pulse, slope, runt, window, timeout, delay, pattern, duration, setup/hold, Nth edge, video (NTSC/PAL), holdoff, force | trigger.md sections 2, 6, 10 (`tools/tpumodel.py`) | static; identical to tpumodel.py in 80 cases; edge end state equals the app's register values |
| `afe` | AFE V/div, offset DAC, coupling, bandwidth filter and cal_vertical block group, with per-channel shadows | afe-settings.md (`tools/afesim.py`) | static; identical to afesim.py for 6 set-ups (test needs the unit's cal files, `OOS_CAL_DIR`) |
| `seq` | settings -> complete step list (ADC mode, AFE, trigger of any type, acquisition configuration, auto trigger, arm, read-out), persistent shadow state, executor, de-interleave, volts | all of the above | static; read-out windows equal the verified runs |
| `adc` | ADC channel mode switch from cal_adc.hex, WPU gain 0x105c | adc-mode.md (`tools/adcmodel.py`) | static; identical to adcmodel.py |
| `wpu` | read-path digital gain (Q10) and offset per lane 0x14c8.. / 0x14d0.., zoom copies, ADC x2 in 0x105c | afe-settings.md 4.1, cold-boot.md 3.9 | written by `seq` when OOS programs the AFEs |
| `panel` | front-panel frames, CRC-7, LED mapping | frontpanel.md | confirmed on the unit |
| `boot` | cold boot from a freshly booted FPGA image to the hand-over to `seq::State::configure`: PLL, AFE/ADC/SPU/SCU init, once-only writes, DDR/ADC-clock checks, ADC parameters of mode 0, DDR taps, sync-delay sweep, 28-lane eye scan + window search, core alignment, ADC internal trim; `dry_run` | cold-boot.md, fpga-boot.md, pll.md, adc-spu-init.md, adc-mode.md | steps 1-6 run on the unit (oos-hw); calibration steps static, tested against a unit model with the log of 2026-11-02 |

## Cold boot (`boot`)

`boot::cold_boot(Hw { bus, clock, pll, afe }, &CalFiles::load("/rigol/data"), &Options::default())` runs steps 1-21 of
cold-boot.md section 4 and returns a `Report` (log of every write/read/sleep, plus `Outcome` with the measured values
and a `seq::State` for the first `configure`). It stops on a failed check (FPGA version, 0x1008 bit 29, 0x1210, no sync
edge) and never sets the BRAM fallback. `boot::io::power_on` (Linux) adds the BOOT command on spidev1.0 (the only
monitor command; no flash access anywhere) and the caller's PCIe bring-up. `boot::dry_run` prints the plan against
`boot::sim::Unit` (the unit's answers of 2026-11-02): `cargo test --release dry_run -- --nocapture`. Cal files are only
read. Open: sinc RAM, AFE Hz enable x4, and the first-configuration registers of cold-boot.md 3.9/3.10
(`boot::Outcome::todo`).

Times are femtoseconds, voltages nanovolts at the BNC.

```
cargo test     # host: 13 tests against values from the unit, the documented examples and the Python models
```

Next: SCU auto trigger 0x4020/0x4060/0x4064, acquisition configuration, FPGA digital gain, AFE SPI
bus access (today in `tools/hw/oos-hw.c`), then `oos-fpgad`.
