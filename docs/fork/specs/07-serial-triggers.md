# Serial protocol triggers in hardware (0x2100..0x21bc): UART, I2C, SPI

Status: specification from static analysis. Field positions are exact (direct bit packing in the stock driver);
field meanings are partly inferred from the stock log strings and setting names. Other protocols (I2S, LIN, CAN,
FlexRay, 1553B) follow the same pattern and are listed in `trigger.md` 1 and 5.1.

## 1. What the stock firmware does

### 1.1 Common set-up order (`DrvUartTrigger_applyCfg`, `DrvIicTrigger_applyCfg`, `DrvSpiTrigger_applyCfg`)

1. Route the sources (`DrvTrigger_RefreshTrigerSrc`) and per source the input filter word 0x208c
   (`DevSystemTPU_SetTriggerIirset(0, src, 0x800)`: [1:0] = 0, [3:2] from the source, [17:4] = 0x800).
2. Map each source to a comparator (`DrvTrigger_ToCmpASrc`) and set the comparator source registers A, B or R and their
   pattern selection (`SetSourceA/B/R`, `SetSourcePatternA/B/R`). Same registers as the pattern trigger in
   `trigger.md` 10.1 and 10.8.
3. Trigger mux 0x205c [4:0]: UART 4, SPI 12, I2C 13.
4. Protocol registers (below). Levels come from the normal level registers of the sources.

### 1.2 UART (trigger type 13)

| register | bits | content |
|---|---|---|
| 0x2100 | 19:0 | bit time, low part: ticks of 6.4 ns, `round_down(1e15 / (baud * 6.4e6))` |
| 0x2100 | 22:21 | condition type (start, stop, data, error; order unknown) |
| 0x2100 | 25:23 | data width code |
| 0x2100 | 27:26 | error type |
| 0x2100 | 29:28 | "Eof" field (stop bits or end condition) |
| 0x2100 | 31:30 | parity |
| 0x221c | 9:0 | bit time bits 30:21 (shared address with the counter start word on read) |
| 0x2104 | 2:0 | compare relation ("Len" in the log) |
| 0x2104 | 10:3 | data value low bound |
| 0x2104 | 18:11 | data value high bound (stock writes the same value as the low bound) |

Sources: R = the data line with pattern R (0, 0, 2, inverted flag). Bit 20 of the bit time is dropped by the stock code;
irrelevant above about 150 baud.

### 1.3 I2C (trigger type 14)

| register | bits | content |
|---|---|---|
| 0x218c | 2:0, 5:3, 8:6, 10:9, 11, 14:12 | six fields: condition, address mode, data relation, direction, and two flags (order as written by the driver; meanings to be mapped on hardware) |
| 0x2190 | 9:0, 19:10, 29:20 | three 10-bit address values (shifted right by 1 in 8-bit address mode, which includes R/W) |
| 0x2194 | 31:0 | data bytes 0..3, low bound (byte i at bits 8i+7..8i) |
| 0x2198 | 31:0 | data bytes 0..3, high bound |
| 0x219c | 31:0 | data bytes 0..3, mask |
| 0x21a0 | 15:0 | byte 4 of low bound, high bound and mask (shared word; the stock masks keep bits 3 and 11) |

Sources: A and B are SCL and SDA (patterns A and B both (2, 0, 2, 0)), each through its own comparator. Mux 13.

### 1.4 SPI (trigger type 15)

| register | bits | content |
|---|---|---|
| 0x2178 | 0 | framing: 1 = idle timeout, 0 = chip select |
| 0x2178 | 3:1 | clock edge / mode field |
| 0x2178 | 8:4 | data width field |
| 0x217c | 31:0 | idle timeout in 6.4 ns ticks (0 with chip-select framing) |
| 0x2180 | 31:0 | bit vector 1 of the data pattern (one bit per data bit) |
| 0x2184 | 31:0 | bit vector 2 |
| 0x2188 | 31:0 | bit vector 3 |

Sources: R = clock (pattern (2, 0, 2, 0)), A = data with polarity flag, B = chip select with polarity flag. Mux 12.
The three bit vectors come from three per-bit boolean arrays in the stock settings; most likely "1", "0" and "don't
care" or value/mask/valid. To be mapped on hardware.

## 2. What OOS does today

- Mux codes and register ranges are documented (`docs/protocols/trigger.md` 1, 5.1); no protocol register is written.
- Protocol trigger = hardware edge on the frame-start line plus software decode and match per record
  (`ui/src/board/dho/hw.rs:97-122`, `ui/src/ui.rs` bus trigger branch). In Normal/Single the UI waits for a record
  that contains the event; every edge re-arms the hardware.

Gap: rare events are found only if the record that contains them is transferred and decoded. CPU decodes every record.

## 3. Proposed change

- `hal/src/dho/tpu.rs`: `uart(ts, src, baud, cond, width, parity, stop, value)`, `i2c(ts, scl, sda, cond, addr_mode,
  addr, data, mask, dir)`, `spi(ts, clk, data, cs, framing, timeout, width, pattern)`, each returning the ordered write
  list of 1.1 plus the protocol words. Shadows for 0x2100, 0x2104, 0x218c, 0x2190, 0x21a0, 0x221c.
- `seq::Trig`: `Uart {..}`, `I2c {..}`, `Spi {..}`.
- UI: hardware trigger when the bus settings fit the hardware fields; software match stays as fallback and for the
  other 22 decoders.
- Gain: true protocol trigger in Normal/Single at the full trigger rate; decode only for display.

## 4. Host test plan

- Extend `tools/tpu_fixture.py` / `tools/tpumodel.py` with the three protocols and generate expected write lists into
  `hal/testdata/tpu_expected.txt` (same mechanism as the existing trigger types).
- Bit time: 9600, 115200, 1 M, 10 M baud give the expected 0x2100 [19:0] and 0x221c [9:0].
- I2C byte packing: 1..5 data bytes, byte 4 into 0x21a0.
- SPI bit vectors for width 8, 16, 32.

## 5. Hardware test checklist

Field meanings need a matrix per protocol. Signal source: a microcontroller board sending known frames.

1. UART 115200 8N1, trigger on 0x55: enumerate 0x2100 [22:21] values; find the "data" condition; then parity and
   stop-bit fields.
2. Check the bit time with 9600 and 1 M baud.
3. I2C 100 kHz: address 0x50 write, then read. Enumerate 0x218c fields until start, address, address+data conditions
   are identified.
4. SPI mode 0 with CS, 8-bit 0xA5: identify which bit vector is value, mask, and valid.
5. SPI timeout framing with 1 us idle.
6. Read 0x221c after a UART set-up while the counter is off and on (shared address).

## 6. Confidence and open questions

Bit positions high, semantics low to medium. The six I2C config fields and the SPI vector roles need the hardware
matrix. The shared 0x221c (UART divider on write, counter start on read) must be checked for interference with the
frequency counter (`08`).
