# Hardware SPI and I2C triggers

The SPI and I2C protocol triggers in the TPU's receivers, next to the UART (`hwuart.md`). Opt-in with the same switch,
`trig.serial_hw = on` in system.conf. Code: `hal/src/dho/serialtrig.rs` (words), `tpu::spi`, `tpu::i2c`,
`seq::Trig::Spi`/`I2c`, `ui/src/board/dho/hw.rs` (`spi_of`, `i2c_of`, `serial_debug`). Register facts: private RE notes
`docs/fpga.md` "Serial trigger", SPI and I2C (netlist simulation).

## Hardware

- SPI (mux 12): clock on event A (sampled on the rising edge), data on R (MSB first), chip select on B (active low).
  Bit 23 of 0x201c / 0x2034 / 0x204c inverts its line, so falling-edge clock = A inverted, active-high CS = B inverted.
  0x2178: framing [0] (0 CS, 1 idle timeout in 0x217c), relation [3:1], width - 1 [8:4]; 0x2180 min, 0x2184 max,
  0x2188 mask. The word is the last `width` bits, right-aligned. The event fires on the clock edge of the last bit.
- I2C (mux 13): SCL on A, SDA on B. 0x218c: condition [2:0] (start, stop, repeated start, missing ACK, address,
  data, address + data), address relation [5:3], data relation [8:6], direction [10:9], 10-bit [11], data bytes - 1
  [14:12]; 0x2190 address min/max/mask; 0x2194..0x21a0 data min/max/mask, big-endian over the first 1..5 bytes after
  the address. Start at the SDA fall; address and data at the 8th SCL rise of the last compared byte; missing ACK at
  the ACK's SCL rise.
- Each line as the stock drivers route it (libscope): the channel's level-A comparator on both inputs of its event,
  pattern (2, invert, 2, invert): SPI data (2, 0, 2, 0), clock and CS with their inversion, I2C SCL and SDA (2, 0, 2, 0).
  Only bits 23:20 reach the line (bit 23 the inversion); bits 19:16 are the stock values. The threshold is the
  trigger level of the protocol trigger (Decode threshold) on every line.

## What OOS does

| Decoder | UI condition | receiver |
|---|---|---|
| SPI with CS | Frame | every word (relation ==, mask 0) |
| SPI with CS | Data = value (fits the bits) | ==, mask of the width |
| I2C | Frame | start |
| I2C | ID/Addr = 7-bit address | address ==, mask 0x7f, either direction |
| I2C | Error | missing ACK |

The menu settings beyond these (relations, limits, masks, CS polarity, the timeout without CS, I2C direction, 10-bit
addresses, data bytes) are in `serial-ui.md`, with what the receivers take. Everything else stays with the edge trigger
and the software decoder: SPI ID and Error, I2C data in any byte, the LA as input, and any set-up whose lines are not
all acquired channels.

Trigger position: SPI words and I2C start and missing ACK fire where the software trigger point is. The I2C address
fires at the 8th SCL rise, one SCL period before the software point (the ACK bit). The record path re-aligns on the
software point next to the hardware trigger, so the screen agrees; no shift is applied.

`:SYSTem:DEBug:SERial?` (also `UART?`) gives the receiver and its words for the current settings.

## Assumptions

- One SPI word per CS frame or a sliding compare inside a long frame: open. With a sliding compare, Data = value can
  fire inside words the software splits differently.

## Bench set-up

SPI: a 3.3 V SPI master, for example a Raspberry Pi (spidev0.0: SCLK pin 23, MOSI pin 19, CE0 pin 24, GND pin 25) or an
FT232H/CH341A in SPI mode. Probes: CH1 on MOSI, CH2 on SCLK, CH3 on CE0, grounds on GND. Send `0x5a 0xa5 0x3c` every
10 ms at 1 MHz, mode 0 (Raspberry Pi: `python3 -c "import spidev,time;s=spidev.SpiDev();s.open(0,0);s.max_speed_hz=1000000;[(s.xfer2([0x5a,0xa5,0x3c]),time.sleep(0.01)) for _ in iter(int,1)]"`).
Scope: CH1..CH3 1 V/div (10x), 2 us/div; Decode SPI: data CH1, clock CH2, CS CH3, mode 0, 8 bits, threshold 1.65 V.

I2C: a 3.3 V I2C bus with a device, for example a Raspberry Pi (SDA pin 3, SCL pin 5) with any sensor or EEPROM at
address 0x50, or an Arduino as master. Probes: CH1 on SDA, CH2 on SCL. Read the device every 10 ms
(`i2cget -y 1 0x50 0x00` in a loop). Scope: 1 V/div, 20 us/div; Decode I2C: SDA CH1, SCL CH2, threshold 1.65 V.

`trig.serial_hw = on`, restart oos-ui. Note `:SYSTem:DEBug:SERial?` and the trigger rate at each step.

## On-unit procedure

1. SPI Frame: stable triggers, one per word at 300 words/s (3 per 10 ms); debug `on,proto=SPI,...,mux=12`.
2. SPI Data = 0xa5: only the second word at the trigger point, 100 triggers/s. Data = 0x42: no triggers. Mode 1 on
   the scope (falling edge) with the Pi in mode 0: decoded values change, triggers follow the scope's mode.
3. SPI with CS removed from the decoder: debug `off`, software trigger as before.
4. I2C Frame: a trigger at every start (one per read, two with the repeated start of `i2cget`: note which).
5. I2C ID = 0x50: triggers; ID = 0x51: none. Unplug the device (or ID 0x51 with a non-existent device in the loop):
   Error triggers on the missing ACK.
6. With CH3 off while SPI uses it: debug shows the receiver, but the trigger falls back to edge (lines not acquired).
7. `trig.serial_hw = off`: everything as before.

Record the results in `docs/fork/unit-dho804.md`.
