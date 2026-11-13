# Models, capabilities and limits (`oos_hal::model`)

OOS runs on every DHO800/DHO900. At start it reads which model it is on, what that model has (channels, logic
analyser port, generator, EXT input) and which limits apply. Tests can raise the limits in `/oos/limits.conf`.

## 1. Identity: `/rigol/data/vendor.bin`

Format (the decoder is in `hal/src/model.rs`):

1. The whole file (204 bytes on a DHO924S) is XXTEA (delta 0x9e3779b9, 6 + 52/n rounds) with the file key.
2. Result: `u32 crc32, u32 len, payload[len]`; crc32 (zlib) over the payload.
3. Payload: items `u32 4, u32 id, u32 n, item[n]`.
4. Item: `u32 crc32, u32 len, u32 string length, data[len]`; crc32 over the string-length word and the data. The data
   is XXTEA with the item key.

| id | content |
|---|---|
| 5 | model, e.g. `DHO924S` |
| 7 | serial |
| 9 | LAN MAC |

The format is verified on the unit: its file decodes with valid checksums. OOS only reads the file and never writes it.

A file that names a model outside the DHO800/DHO900 family (an MHO900, a DHO1000, ...) marks another board: then OOS
drives no hardware ([porting.md](porting.md)). A DHO8xx/DHO9xx name missing from the table counts as a DHO924S.

The keys are not part of OOS. At the first start OOS looks for them in the stock software of the unit itself
(`hal/src/vendorkeys.rs`), accepts them only if the unit's own file decodes with them, and keeps them in `/oos/vendor.keys`.
Without the stock software OOS cannot read the file: set `model = ...` in `/oos/limits.conf` then.

## 2. Model table

Each entry has a name, a bandwidth code, the channel count and the series:

| Model | bandwidth code | MHz | channels | series | OOS: LA | OOS: generator | OOS: EXT trigger |
|---|---|---|---|---|---|---|---|
| DHO802 | 5 | 70 | 2 | 800 | - | - | yes |
| DHO804 | 5 | 70 | 4 | 800 | - | - | - |
| DHO812 | 6 | 100 | 2 | 800 | - | - | yes |
| DHO814 | 6 | 100 | 4 | 800 | - | - | - |
| DHO824 | 10 | 250 | 4 | 800 | - | - | - |
| DHO914 | 7 | 125 | 4 | 900 | yes | - | - |
| DHO914S | 7 | 125 | 4 | 900 | yes | yes | - |
| DHO914-EDU | 7 | 125 | 4 | 900 | yes | - | - |
| DHO924 | 10 | 250 | 4 | 900 | yes | - | - |
| DHO924S | 10 | 250 | 4 | 900 | yes | yes | - |
| DHO924-EDU | 10 | 250 | 4 | 900 | yes | - | - |

Bandwidth codes: 5 = 70, 6 = 100, 7 = 125, 10 = 250 MHz (`protocols/afe-settings.md` 3.5). The HDO1000 entries add
4 = 50 and 9 = 200. HDO1000/2000/4000 models are other boards, which OOS does not support (see section 6).

How OOS fills the three right-hand columns: LA = series 900, generator = name ends in "S", EXT trigger = 2 channels.
These come from Rigol's model line-up. Not yet verified on hardware: whether the EDU variants have the LA port.

OOS takes "DHO914S" as DHO914 plus generator.

The bandwidth code is the rated (licence) bandwidth. OOS does not limit to it. "Full" opens the AFE filter on every
model. Whether the DHO800 front end gets as far as the DHO900's is not yet measured.

### 2.1 Hardware variants

The first install from the stock system (`install-usb.md` 4) takes a model without being told only when OOS has run on
a model of the same hardware. Models count as the same hardware when they differ in nothing but the bandwidth code:
the same series, channel count, generator and edition. The bandwidth code is what the stock software makes of the same
front end (it sets the AFE filter from it), and the bandwidth is sold as a licence: a DHO804 has been seen that names
itself a DHO824 in `vendor.bin`. Kept apart, because the hardware is not known to be the same: 2 and 4 channels (two
front ends fewer, an EXT input instead), a DHO900 with and without generator (the output stage), the EDU editions (LA
port not confirmed), DHO800 and DHO900 (LA port, front panel). The table is `VARIANTS` in `hal/src/dho/target.rs`.

| Hardware variant | Models | OOS has run on |
|---|---|---|
| DHO800, 4 channels | DHO804, DHO814, DHO824 | DHO804 (user report, several releases) |
| DHO800, 2 channels | DHO802, DHO812 | - |
| DHO900 with generator | DHO914S, DHO924S | DHO924S (the developer's unit) |
| DHO900 without generator | DHO914, DHO924 | - |
| DHO900 EDU | DHO914-EDU, DHO924-EDU | - |

A report of OOS starting on a model of a variant marked "-" makes the whole variant verified.

## 3. Limits (the same for every model)

| Limit | Value | Reference |
|---|---|---|
| max. sample rate per channel, 1 / 2 / 3-4 channels | 1.25 G / 625 M / 312.5 MSa/s | `protocols/horizontal-config.md` 1.1 |
| max. memory depth per channel, LA off | 125 M / 62.5 M / 31.25 M | |
| max. memory depth per channel, LA on | 31.25 M / 12.5 M / 10 M | |
| generator max. frequency | sine/noise/DC 60 MHz, square 15 MHz, ramp 5 MHz, pulse 5 MHz, arb 10 MHz | `protocols/afg.md` 3 |
| ADC clock | 1.25 GHz | LMX2582 table, `protocols/pll.md` |
| generator DDS clock | 156.25 MHz | `protocols/afg.md` |

## 4. Experimental limits: `/oos/limits.conf`

This file lives on the OOS data partition. Its format is `key = value`, and `#` starts a comment. Every key that is
applied is printed at start, shown in System > About as "EXPERIMENTAL limits" and announced with a toast. Unknown keys
and bad values only produce warnings.

| Key | Effect |
|---|---|
| `model = DHO804` | act as this model (also `--model` for oos-ui / oos-fpgad; the argument wins) |
| `channels = 2\|4`, `la = yes\|no`, `afg = yes\|no` (generator present), `ext_trig = yes\|no` | capability override |
| `mem.1ch`, `mem.2ch`, `mem.4ch` | max. memory depth per channel (points), LA off |
| `mem.la_1ch`, `mem.la_2ch`, `mem.la_4ch` | the same with LA on |
| `adc.clock_hz = 2e9` | **experimental ADC clock** 1.25..2.0 GHz, only what `lmx2582::table_for` can make: the cold boot (and `oos-fpgad pll`) writes the generated LMX2582 table, sample rates, roll rate, trigger ticks and the generator DDS clock follow (`hal::dho::clock`); see `protocols/pll.md` and section 4.1 |
| `afg.sine_max_hz`, `afg.square_max_hz`, `afg.ramp_max_hz`, `afg.pulse_max_hz`, `afg.noise_max_hz`, `afg.dc_max_hz`, `afg.arb_max_hz` | generator frequency cap (clamped to DDS clock / 2 = 78.125 MHz) |

Example (test of the generator up to 75 MHz, not yet characterised):

```
# generator above the default cap, measured with the spectrum (image at 156.25 MHz - f)
afg.sine_max_hz = 75e6
```

Once a value has been measured and confirmed, it moves from limits.conf into `Limits` (for one model, or for all),
with the measurement noted in this file.

Measured on a DHO924S, 2 Vpp on an open cable into CH1, 1.25 GSa/s:

| Wave | Limit | What sets it |
|---|---|---|
| Sine | 60 MHz (stock) | The image at 156.25 MHz - f has no filter in its way: -51 dB at 50 MHz, -23 dB at 60, -10 dB at 65, -5 dB at 70, about equal at 77. The frequency itself stays right up to 78 MHz |
| Square | 15 MHz (stock) | Above it the wave is a sine; 25 MHz is one, with 4 Vpp from the cable's resonance |
| Ramp | 5 MHz (stock 150 kHz) | Clean triangle at 5 MHz, tips slightly round |
| Pulse | 5 MHz (stock 2 MHz) | 11 ns edges; 10 MHz already has no flat top, 20 MHz is a sine |

### 4.1 Overclocking test (`adc.clock_hz`)

`hal::dho::clock` keeps every model of the hardware (time base `hz`, trigger `tpu`, generator `afg`, roll) in the nominal
1.25 GHz domain and converts only at its edges: real times are multiplied by f/1.25 GHz before the model, rates come out
multiplied by f/1.25 GHz. The register values stay the same as for the scaled times. At 1.25 GHz every conversion is
the identity, so all host tests are unchanged. `hal/tests/overclock.rs` checks 2 GHz in its own process.

What follows the clock: the PLL table (`boot::pll_table`), sample rates and periods (`hz::prep`), the SPU divider
base (ADC / 20), the roll stream (ADC / 8), trigger t1/t2/holdoff/timeout ticks and the 6.4 ns video tick, the
generator DDS clock (ADC / 8: frequency, modulation rate, FM deviation, edge length), the expected 0x1210 count, and
the eye scan's minimum reference run (14 taps x 1.25 GHz / f).

`adc.clock_hz` belongs first in limits.conf: `afg.*_max_hz` above it are clamped to the DDS limit of the clock in effect
when they are read. Off the 1-2-5 grid the time base takes the next rate the SPU dividers can make (`hz::legal_rate`).

Seen on the unit: the 0x1210 count goes as the period, 1.375 GHz read 56816 (62500 x 1.25 / 1.375 = 56818). Assumed, not yet verified on hardware: the 4 ns
video tick and the auto-trigger 4 ns ticks come from the PS's 250 MHz FCLK3 and do not scale. The AFE "Auto" filter stays
absolute (capped at code 63). Display-only defaults (`--pre` replay preamble 0.8 ns, the demo simulator) are not
parameterised.

System > Overclock sets the clock in steps (1.25, 1.375, 1.5, 1.75, 2 GHz): it writes `adc.clock_hz` first in
/oos/limits.conf, leaves the other lines, and counts from the next power-on. At 1.25 GHz the line is gone.

Test procedure (it leaves the validated register values): go up one step at a time, power off/on. Watch the cold boot log for the eye scan (window width per lane), the 0x1210 count and a
known sine from the generator (its frequency must be right; the generator clock follows). A clean boot and a right
frequency are not enough: also compare the noise of a large sine against 1.25 GHz. Stop at the first step with lane
errors, odd 0x1210 or wrong data, then remove the line and power cycle.

Measured on one DHO924S with a 2 Vpp sine from the generator on CH1 at 0.5 V/div, sine fit over screen records. The
absolute figures include the generator's harmonics; only the comparison counts. The calibration files were made at
1.25 GHz.

| ADC clock | Boots | Frequency | 1 MHz SINAD | 10 MHz SINAD | Data |
|---|---|---|---|---|---|
| 1.25 GHz | yes | right | 39.2 dB | 41.6 dB | clean |
| 1.5 GHz | yes | right | 34.0 dB | 38.8 dB | no jumps, noise up |
| 2 GHz | yes | right | not measured | not measured | single samples off by up to 2.4 V, frequency measurement wrong |

So 2 GHz is not usable, and 1.25 GHz stays the default. 1.375 and 1.75 GHz boot and show the right frequency; their data
was not checked this way.
A start with `adc.clock_hz` that does not reach its end sets the file aside at the next start (`limits.conf.failed`, rcS).

Memory depths above 125 M need more entries in the depth list (`MDEPTHS` in ui/src/model.rs) and a check of the
segment cap (`hal::dho::hz::MEM_BURSTS`).

### 4.2 Unit defaults: `/etc/oos/limits.conf`

`/oos/limits.conf` only exists after the first start. A unit whose `vendor.bin` names other hardware than it has (e.g.
no generator board, no LA connector) would start once as the model the file names: OOS
opens the generator's relay and supply lines and looks for `cal_afg.hex`. An image built for such a unit carries
`/etc/oos/limits.conf` (`model::UNIT_CONF`) with the capability keys of that unit:

```
afg = no
la = no
```

- Only `channels`, `la`, `afg` and `ext_trig`. Other keys are warnings: limits and `adc.clock_hz` stay in `/oos/limits.conf`,
  where the start-up fallback of rcS sees them.
- Order: vendor.bin, then this file, then `/oos/limits.conf`, then `model =` / `--model`. A forced model keeps the unit
  defaults; `/oos/limits.conf` can switch a capability on again (`afg = yes` after a generator board is fitted).
- Facts of the unit, not experimental: no toast, the identity (model name, serial, rated bandwidth, `*IDN?`) stays what
  `vendor.bin` says. `oos-fpgad model` prints them as `unit`, System > System info as "Unit defaults".
- Build: `OOS_UNIT=<name> boot/build-dev-image.sh` copies `rootfs/unit/<name>/*.conf` to `/etc/oos/`. Without `OOS_UNIT`
  the image has no `/etc/oos` and behaves as before.

Without a generator (model table or `afg = no`) nothing opens the generator lines (`afg_in1..5`, `FG_M2C_GPIO`):
`afg::Io::open` refuses (`afg::line_allowed`), the acquisition thread never asks, `oos-fpgad afg` and
`oos-fpgad gpio <generator line>` refuse without `--force`. Verified on the host (`hal` tests), not yet on hardware.

## 5. Where OOS uses it

- `oos_hal::model::platform()` is set once at start (oos-ui `main`, oos-fpgad `main`). Until then, and in host tests,
  it is the DHO924S with the limits of section 3.
- oos-ui: sample rate and depth limits (`Model::max_srate`, `max_mem`); 2-channel models have no CH3/CH4 cards, the
  CH3/CH4 keys only show a message, and `Model::fit_platform` switches off whatever turns them on (setups, autoset,
  demos, SCPI); the generator page, the GI key, built-in Bode and SCPI `:SOURce` exist only on S models, and the
  acquisition thread never touches the generator lines on other models; every channel source list (trigger,
  measurements, decoders, math, zones, mask, reference, analysers, Bode) offers only the existing inputs, and
  `Model::fit_platform` moves sources that point at a missing input (old setups) to CH1 or none; frequency caps from the limits; `*IDN?`
  reports model and serial; System > About shows identity and capabilities, and points to System > System info
  when experimental limits or warnings exist.
- System > System info (`ui/src/ui/sysinfo.rs`), refreshed once a second, in three columns. Unit: model, serial,
  MAC, series, rated bandwidth, channels, LA, generator, EXT, where the identity came from. Software: OOS, kernel,
  FPGA version, uptime, network, source. Temperatures: SoC zones and FPGA die (XADC 0x1438, read by the acquisition
  thread once a second), VCCINT, fan, the alarm limits (SoC 90 degC, FPGA 75 degC, kernel throttling 70/85 degC),
  CPU load and clock, RAM, free space on /oos. Limits per channel count, LA depths, generator caps and clocks, with
  values that differ from section 3 in amber. The current acquisition. Experimental limits and warnings in full.
- oos-fpgad: `oos-fpgad model` prints everything; `oos-fpgad afg` refuses on models without a generator
  (`--force` overrides).

Still open for models other than the DHO924S:
- EXT trigger input of the 2-channel models: not implemented.
- DHO800: a DHO804 runs OOS (user report); front panel, LEDs and display have not been checked in detail.
- Logic analyser in the UI: not implemented on any model (`hal::dho::la` exists).

## 6. Related findings (other Rigol series)

Series configurations:

| Config | ADC chips | cores | LVDS lanes | bits per lane | FPGA clock | gives |
|---|---|---|---|---|---|---|
| HDO900 | 1 | 8 | 28 | 14 | 6.4 ns (156.25 MHz) | 1.25 GSa/s |
| HDO1000 | 1 | 8 | 28 | 14 | 4.0 ns (250 MHz) | 2 GSa/s |
| HDO4000 | 2 | 16 | 56 | 14 | 4.0 ns (250 MHz) | 4 GSa/s |

The DHO1000 has the same ADC interface as the DHO900, but runs at 2 GSa/s. The ADC is reported as RT8847IQ (2 GS/s) in
the DHO800 as well as in the DHO1000/4000 and MHO98 (two of them in the MHO98). Whether the DHO900 runs at more than
1.25 GSa/s is not yet verified on hardware: PLL tables, plus the lane eye scan as the measure.
