# Board management (fan, temperatures, voltages, power key / MCU power commands, beeper, probes)

Certainty: "verified on the unit" means seen on a running DHO924S. Anything else in this file was not exercised under
OOS unless stated (section 8.6 lists what is built).

## Fan under OOS: verdict

**The fan should run at full speed under OOS from early kernel boot, with no userspace daemon.** Not yet verified on
hardware by ear or airflow.

Why:
- On the DHO924S the fan is one **on/off line, GPIO4_A5 (global 133, `fan_power`)**. There is no speed control. The stock
  DT also has a `pwm_fan` node on PWM3 (`pwm@ff420030`, pin GPIO0_A6, 10 kHz, inverted), but the stock 4.4 kernel has
  **no `pwm-fan` driver**, `/dev/pwm_fan` does not exist (verified on the unit), and GPIO0_A6 is actually
  the LMX2582 PLL latch enable (`spi_le`, out low, verified on the unit).
- Under stock, the kernel module `fan_gpio_clt` (loaded about 35 s after boot) sets GPIO 133 to output high in its init
  and to low only in its exit function. Nothing else touches the pin (its misc device `/dev/fan_gpio_clt` has no ioctl).
  A running unit shows `gpio-133 fan_power out hi` (verified on the unit). So **the fan is always on at full speed
  while the system runs**.
- No software fan control exists on this hardware (section 1).
- OOS: the DT has `&gpio4 { fan-power-hog { gpio-hog; gpios = <5 0>; output-high; line-name = "fan_power"; } }`. The line
  goes high when the gpio4 controller probes (early kernel boot, earlier than under stock) and the hog keeps it claimed,
  so no userspace program can switch the fan off by accident. The `gpio-fan` node is `status = "disabled"`, `pwm3` stays
  off. This is the same electrical state as stock.

What protects the hardware under OOS:
- Only the SoC thermal zones and the FPGA XADC temperature exist on this board: all TMP421 and ADC128D818 chips are
  absent (section 2.1).
- Kernel thermal zones with CPU throttling at 70 / 85 degC (`cpu_alert0/1`, passive), `critical` at 115 degC on CPU and
  GPU (kernel orderly power-off), and the RK3399 TSADC hardware shutdown at 120 degC (`rockchip,hw-tshut-temp = 120000`,
  mode 1 = TSHUT pin to the PMIC). Same values as the stock DT. Whether a plain Linux power-off really removes board
  power (FPGA, AFE) without the MCU frame `FA 05 01` is not yet verified on hardware; without that frame (section 8.3),
  a critical trip may only halt the SoC while the fan keeps running.
- The FPGA die temperature needs its own supervision: read the XADC register (section 2.3) and shut down above a limit
  (section 8.2).

## 1. Fan

| Item | Value |
|---|---|
| Fan supply switch | GPIO4_A5 = global 133, active high, `fan_power` |
| Speed control | none (on/off only): no driver for `pwm_fan`, PWM3 pin used by the PLL |
| Default without any driver | not known (the pin's reset state; GPIO4 pins reset as inputs with the bank's default pull). Under stock the fan is undriven for the first ~35 s of every boot (assumed) |
| OOS state | output high from gpio4 probe, held by a gpio-hog |

The fan needs no software beyond this line: writes to `/dev/pwm_fan` (ioctl 0x7801 set, 0x7802 get) reach nothing,
because the device does not exist on this hardware.

## 2. Temperatures and voltage monitors

### 2.1 Sensor chips in the stock DT and their presence on the DHO924S

| Chip | Bus / address | Stock DT | Present on the DHO924S? | OOS DT / kernel |
|---|---|---|---|---|
| TMP421 x5 | I2C4 @0x1d, 0x1f, 0x2a, 0x4c, 0x4f | `rockchip,tmp421` | **no**: "Could not read configuration register (-6)" for all five, no driver bound (verified on the unit) | nodes `disabled`, `CONFIG_SENSORS_TMP421` not set |
| ADC128D818 | I2C1 @0x37 | `rockchip,adc128d818` | **no**: "adc128 init client error!", no driver bound (verified on the unit) | node `disabled`, `CONFIG_SENSORS_ADC128D818` not set |
| RK3399 TSADC | on-chip | thermal_zone0 = soc (CPU), thermal_zone1 = gpu | yes | `CONFIG_ROCKCHIP_THERMAL=y`, `cpu-thermal`, `gpu-thermal` |
| RK3399 SARADC | on-chip, `iio:device0` | used for `rockchip-key` | yes | `saradc@ff100000` is `disabled` in the OOS DTB |
| Zynq XADC | inside the FPGA, read over PCIe | - | yes (when the FPGA is loaded) | FPGA register 0x1438 |

These chips belong to other boards of the same platform. OOS needs no driver for them.

### 2.2 Temperature reading

A temperature file holds the value in milli-degrees: read up to 10 chars, `atoi` / 1000 = degC.

The file follows from the hardware revision (section 7): TMP421 entries are (I2C address, hwmon index, temp index),
formatted into `/sys/bus/i2c/devices/4-00%x/hwmon/hwmon%d/temp%d_input`. If that open fails, a fixed path applies.

| idx | check-table name | TMP421 entry (rev 2/3 table) | fixed fallback path | Reading on the DHO924S |
|---:|---|---|---|---|
| 0 | ADC2_CHIP | none | none | fails (-1.0) |
| 1 | ADC2_AMBIENT | none | none | fails |
| 2 | ADC1_CHIP | 4-001f temp1 | `/sys/bus/i2c/devices/4-001f/hwmon/hwmon0/temp1_input` | fails (chip absent) |
| 3 | ADC1_AMBIENT | 4-001f temp2 | `.../4-001f/hwmon/hwmon0/temp2_input` | fails |
| 4 | CH4_CHIP | 4-002a temp1 | none | fails |
| 5 | CH4_AMBIENT | 4-002a temp2 | none | fails |
| 6 | CH1_CHIP | 4-004c temp1 | none | fails |
| 7 | CH1_AMBIENT | 4-004c temp2 | none | fails |
| 8 | CPU_CHIP | 4-004f temp1 | `/sys/devices/virtual/thermal/thermal_zone0/temp` | **works** via the fallback: RK3399 CPU (soc-thermal) |
| 9 | CPU_AMBIENT | 4-004f temp2 | `/sys/devices/virtual/thermal/thermal_zone1/temp` | **works** via the fallback: RK3399 GPU zone |

(The CHIP/AMBIENT to temp1/temp2 pairing is as listed; TMP421 temp1 is normally the local sensor. Irrelevant on this
board.)

### 2.3 FPGA XADC

One read of register 0x1438 (same register address convention as the other protocol docs):

| Bits | Quantity | Formula |
|---|---|---|
| [11:0] | FPGA die temperature | `code * 503.975 / 4096 - 273.15` degC |
| [27:16] | VCCINT | `code / 4096 * 3.0` V |

These are the standard XADC transfer functions. Check-table items SPU_CHIP (type 10) and SPU_VCCINT (type 11).

On the DHO924S the register reads 0, under OOS and under stock alike (`pcie2dev 0x1438`), so there is no FPGA temperature here and
OOS watches the SoC zones only.

### 2.4 Voltages

| idx | Source | Conversion | Check-table name (type) | Works on the DHO924S |
|---:|---|---|---|---|
| 0 | SARADC `in_voltage0_raw` | raw * 1800 / 1023 / 1000 V | (not used) | yes (assumed) |
| 1 | SARADC `in_voltage2_raw` | same | SPU_MGTAVTT (12) | yes (assumed) |
| 2 | SARADC `in_voltage3_raw` | same | SPU_VCC_DDR (13) | yes (assumed) |
| 3 | ADC128D818 `in4_input` (mV) | / 1000 V | AFE23_VCC (14) | no, chip absent |
| 4 | ADC128D818 `in5_input` | / 1000 V | AFE01_VCC (15) | no |
| 5 | ADC128D818 `in6_input` | / 1000 V | ADC1_VCC (16) | no |
| 6 | ADC128D818 `in7_input` | / 1000 V | ADC0_VCC (17) | no |

Path for ADC128: `/sys/bus/i2c/drivers/adc128d818/1-0037/in%d_input`. The SARADC path is
`/sys/bus/iio/devices/iio:device0/in_voltage%d_raw`. Whether SARADC channels 0, 2 and 3 are really wired to these
supplies on this board is not yet verified on hardware. ADC128 in0..in3 are the probe-ID inputs (section 6).

### 2.5 Self-check table

Items in table order for hardware revision 0. min/max are the "pass" limits of a self-test result page, not alarm
limits; the alarm logic is in section 3.

| # | name | type | min | max |
|---:|---|---:|---:|---:|
| 0..9 | ADC2_CHIP, ADC2_AMBIENT, ADC1_CHIP, ADC1_AMBIENT, CH4_CHIP, CH4_AMBIENT, CH1_CHIP, CH1_AMBIENT, CPU_CHIP, CPU_AMBIENT | 0..9 | 0 | 95 degC |
| 10 | SPU_CHIP | 10 | 0 | 95 degC |
| 11 | SPU_VCCINT | 11 | 1.0 | not known |
| 12 | SPU_MGTAVTT | 12 | 1.0 | not known |
| 13 | SPU_VCC_DDR | 13 | 1.45 | not known |
| 14 | AFE23_VCC | 14 | 2.5 | 3.0 |
| 15 | AFE01_VCC | 15 | 2.5 | 3.0 |
| 16 | ADC1_VCC | 16 | 2.0 | 3.0 |
| 17 | ADC0_VCC | 17 | 2.0 | 3.0 |

## 3. Over-temperature supervision

Every period, read all temperatures (types as in section 2). "Hot" = any reading above its limit. Count hot periods in
a row:

| Step | Rule |
|---|---|
| counter >= 3 | alarm tone and a warning in the UI |
| counter >= 11 | power off (MCU power-off frame, section 4) |
| condition gone | stop the tone, counter = 0 |

Period and limits are in section 8.2. With a 1 s period this warns after ~3 s and powers off after ~11 s. A further
condition on the ADC supply readings (items 16, 17) cannot trigger on this board: the ADC128 is absent.

## 4. Front-panel MCU power commands (UART0, `FA cmd data crc7 AF`, see `frontpanel.md`)

CRC = CRC-7/MMC of (cmd, data) as in `frontpanel.md`. The port can be opened write-only for each command.

| Frame | cmd / data | Meaning | Use |
|---|---|---|---|
| `FA 05 00 27 AF` | 5 / 0 | "app UI loaded" | once at start, as the last step of the start-up |
| `FA 05 01 2E AF` | 5 / 1 | power off | send it twice back to back |
| `FA 05 02 35 AF` | 5 / 2 | cancel power-down | on every power-key press, and when a power-off is cancelled |
| `FA 05 03 3C AF` | 5 / 3 | restart | |
| `FA FF 05 31 AF` / `FA FF 04 38 AF` | 0xff / 5 or 4 | power-on mode | data = 5 or 4 by the "power status" setting (value also stored in FRAM). Meaning of 4/5 ("power on at mains" vs "wait for key") is assumed |
| `FA 04 nn crc AF` | 4 / 0..255 | power-LED flash time, nn = seconds * 10 (0..25.5 s) | effect not yet verified on hardware |
| `FA 06 BB 03 AF` | 6 / 0xbb | read keyboard (MCU) firmware version | then poll the key queue up to 100 x 2 ms; the version is the **data byte** of the reply, shown as `data >> 4` ".0" |
| `FA 01/02/03 mask` | LEDs | see `frontpanel.md` | all LEDs off at start |

Power-key flow: the MCU sends key code 0x1b. The host answers at once with the cancel frame (`FA 05 02`) and shows a power
off / restart dialog; then it sends `FA 05 01` (twice) or `FA 05 03`. Because the host answers every key press with a
"cancel power-down", the MCU very likely starts its own power-down countdown when the key is pressed and cuts power if
no cancel arrives (assumed; the timeout is not known).

There is no power controller on I2C at address 0x15 on the DHO (verified on the unit). UART0 is the only effective path.

## 5. Beeper

| Item | Value |
|---|---|
| Pin | GPIO4_A3 = global 131, `PWM_GPIO`, idle output high (verified on the unit). The stock module `beeper_gpio` toggles it from an hrtimer |
| Device (stock) | `/dev/pwm-1` (char major 237 on the unit, class "Beep", node name `pwm-%d`) |
| ioctl 0x40084101 | period in ns, max 1 000 000 (else reset to 0) |
| ioctl 0x40084102 | high time in ns, must be <= period (else 0) |
| ioctl 0x40084103 | start: hrtimer toggles the pin (needs period and duty set, not already running) |
| ioctl 0x40084104 | stop: cancel the hrtimer (pin keeps its last level) |

Timing: period 500 000 ns (2 kHz), duty 100 000 ns (20 %). "once" = start, 20 ms, stop. "long" = 10 x (start, 200 ms,
stop, 200 ms) = 4 s.

OOS: the DT has a `pwm-gpio` node on GPIO4_A3 (`beeper_pwm`, `CONFIG_PWM_GPIO=m`), usable through the PWM sysfs class
(`period` 500000, `duty_cycle` 100000, `enable` 1/0).

## 6. Probes, LA probe, overload

| Function | Mechanism | DHO924S |
|---|---|---|
| Analog probe ID | ADC128D818 `in(4 - ch)_input` (CH1 -> in3 ... CH4 -> in0), default 2323 mV | ADC128 absent: no probe ID on this model (verified on the unit) |
| 1-wire smart probe | FPGA registers | not used (assumed: no probe interface on the DHO800/900 BNCs) |
| `/dev/afe_rms` (ioctl 0x7800 -> one status byte, 2 bits per channel) | input over-voltage / 50 ohm RMS monitor of other models | device absent (verified on the unit) |
| LA probe detect | GPIO3_D0 (global 120, `LA_PROBE_DETECT`). 1 = probe plugged (assumed) | idle 0 without probe (verified on the unit). Under stock the pin is exported to `/sys/class/gpio/gpio120/value` |
| LA probe polling | every 1 s, first after 6 s: on 1 -> 0 switch the LA off if it was on | OOS does the same, and refuses to switch the LA on from the panel or the screen without a probe |
| Channel overload codes 0xf1..0xf4 | see below | never generated on the DHO924S |

**Overload codes 0xf1..0xf4 do not come from the MCU.** They are synthetic key codes of the stock system: it reads the
`/dev/afe_rms` status byte with ioctl 0x7800 and compares it with the previous one; a channel whose 2-bit field
increased adds 1 / 10 / 100 / 1000 (CH1..CH4) to the result, 255 = no change or no device. Every non-zero decimal digit
becomes key 0xf1..0xf4. `/dev/afe_rms` does not exist on the DHO924S (verified on the unit), so the codes never occur.

## 7. Hardware revision (`hdcode_gpio`)

`/dev/hdcode_gpio` read returns one byte `gpio4 | gpio8 << 1 | gpio11 << 2 | gpio12 << 3` (GPIO0_A4, B0, B3, B4, inputs).
The stock module is not loaded (verified on the unit), so the revision reads as 0 (revision group "0/4"). The
revision only selects sensor tables that do not apply to this board.

## 8. Proposed OOS implementation

Items marked "assumed" are design choices or guesses. Nothing here is verified on hardware unless stated.

### 8.1 Fan
- Keep the `fan-power-hog` (output high) in the OOS DT. No daemon needed.
- Do not enable `pwm3` / `pwm_fan` (GPIO0_A6 is the PLL LE).
- Check once on the unit: fan audible / airflow under OOS, and `gpioinfo` shows line 5 of gpio4 as `fan_power` output
  high (consumer = hog). Not yet done. Also not known: the fan state between power-on and the gpio4 probe.
- Optional: if a future board revision needs it, `gpio-fan` + thermal cooling map could switch the fan, but the hardware
  has nothing to gain (on/off only); keep it always on.

### 8.2 Temperature supervision daemon (`oos-boardd` or part of `fpgad`)
- Every 1 s read `/sys/class/thermal/thermal_zone0/temp` (CPU) and `thermal_zone1/temp` (GPU), m degC. Check the zone
  numbering under 6.12 with `type`.
- When the FPGA is loaded, read register 0x1438 and convert as in 2.3 (FPGA die temperature and VCCINT).
- Alarm logic of section 3: 3 consecutive hot samples -> warning in the UI + beeper tone; 11 consecutive -> orderly
  shutdown and `FA 05 01`. Limits: 75 degC for the FPGA at a 1 s period; 90 degC for the SoC zones, because the kernel
  already throttles at 70/85 degC. The limits are a design choice.
- No TMP421 / ADC128 handling (chips absent).
- Optional diagnostics: SARADC channels 2/3 as "MGTAVTT" / "VCC_DDR" (needs `&saradc { status = "okay"; vref-supply }`
  in the OOS DT, which has it disabled; check for plausible values first).

### 8.3 Power key, power off, restart, MCU handshake (UART0 owner, e.g. the oos-ui front-panel thread)
- After boot, when the UI is up: send `FA 05 00 27 AF` once ("app UI loaded"). Its effect on the MCU is not known (maybe
  ends power-LED blinking or arms power-key handling; assumed).
- On key code 0x1b (power key press, data != 0): immediately send `FA 05 02 35 AF` (cancel the MCU's power-down), then
  show a power-off / restart dialog. That the MCU would otherwise cut power by itself is assumed. To test: press the key
  once with nothing listening on ttyS0 and time it (risk: hard power cut of a running Linux, use a read-only or synced
  system).
- Power off: sync and unmount file systems, stop acquisition, then send `FA 05 01 2E AF` twice back to back. Do this from
  a shutdown hook as the very last step (e.g. a systemd `poweroff` target unit or the init's shutdown path), so that
  `poweroff` from a shell also cuts power. Whether Linux needs to be halted first or the MCU waits is not known.
- Restart: `FA 05 03 3C AF` (or a plain Linux `reboot`, which probably works without the MCU).
- Power-on mode setting: `FA FF 05 31 AF` / `FA FF 04 38 AF` (data 5 when the "power status" flag is true, 4 when
  false). Which one means "power on when mains is applied" must be tested.
- MCU firmware version: send `FA 06 BB 03 AF`, read the next frame from the MCU, version = data >> 4. Whether the reply
  arrives with cmd 6 is not known.
- Over-temperature shutdown (8.2) uses the same power-off path.

### 8.4 Beeper
- Load `pwm-gpio`, export channel 0 of the beeper PWM chip, set period 500000 ns, duty_cycle 100000 ns, then toggle
  `enable` for 20 ms ("key click" / once) or 10 x 200 ms on / 200 ms off ("long" alarm).
- Make sure the line idles low after `enable 0` if the beeper draws DC current when high (the stock driver leaves the
  last level, the unit idles high). Assumed need.

### 8.5 Probes and overload
- LA probe: OOS polls GPIO3_D0 (`LA_PROBE_DETECT`, libgpiod) every 1 s (`la.md` 2).
- Overload: nothing to do. The DHO924S has no overload detection the RK3399 can see (no `/dev/afe_rms`, and no 0xf1..0xf4
  codes).
- No analog probe ID on this model.

### 8.6 Implementation status
- `hal::dho::panel`: all power frames of section 4 as constants / builders (`UI_LOADED`, `POWER_OFF`, `CANCEL_POWER_DOWN`,
  `RESTART`, `KEYBOARD_VERSION`, `power_on_mode`, `flash_time`, `KEY_POWER`), CRCs tested against the table.
- `hal::dho::board`: XADC decoding (`xadc`, `read_xadc` rejects an unloaded FPGA), thermal-zone reader, `Guard` (warn at 3,
  power off at 11 hot periods, cleared on the first cool period), beeper over the pwm-gpio sysfs class (`Beep::Once` /
  `Long`).
- `oos-fpgad temp | beep | guard`; `status` also prints the XADC line. pwm-gpio.ko is in the dev image and loaded by rcS.
- oos-ui (the front-panel UART owner): `FA 05 00` once at start; power key (0x1b) -> `FA 05 02` at once and a dialog
  Power off / Restart / Cancel; Power off = LEDs off, `sync`, `FA 05 01` twice; Restart = `FA 05 03`. Built and
  unit-tested, dialog rendered; the key press is not yet verified on hardware.
