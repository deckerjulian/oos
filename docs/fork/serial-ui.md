# Protocol trigger settings

Trigger > Protocol beyond condition and value, for UART, SPI and I2C. Model: `SerialTrig` (`m.ser`), setup keys
`ser.*`. Menu: `ui/src/ui/sertrig.rs`. Software trigger: `ui/src/trig.rs` (`matches`, `points`). Hardware:
`ui/src/board/dho/hw.rs` (`uart_of`, `spi_of`, `i2c_of`, with `trig.serial_hw = on`; `hwuart.md`, `hwserial.md`).
SCPI: `ui/src/scpi/ser.rs`.

The value (`trig_bus_val`) is now 40 bits wide (five I2C data bytes); the keypad takes ten hex digits.

## Menu

| Protocol | Condition | Items after Value |
|---|---|---|
| UART | Data | Relation, Max (for <, in range, out of range) |
| UART | Error | Error type (Any, Parity, Framing) |
| UART | all | Polarity (Normal, Inverted), Stop bits (1, 1.5, 2) |
| SPI | Data | Relation, Max, Mask |
| SPI | all | CS (Active low, Active high) with a CS line; Timeout (Off, 100 ns .. 10 ms) without one |
| I2C | ID/Addr | Relation, Max (of the address), Address (7-bit, 10-bit), Direction (Either, Write, Read) |
| I2C | Data | Relation, Max, Mask, Bytes (Any, 1..5), Direction |

Relations: =, !=, > (than the value), < (than Max), in range (value..Max), out of range. The mask applies to the
value, Max and the decoded value alike. Switching to the protocol trigger resets these settings.

## Who evaluates what

| Setting | software trigger | TPU receiver |
|---|---|---|
| UART relation, Max | yes | yes (8 bits) |
| UART mask | yes | no: the software keeps the trigger |
| UART polarity | yes (the decoder inverts too) | yes |
| UART stop bits | no (the decoder reads 1) | yes |
| UART error type | Any only (the decoder has one check flag) | yes |
| SPI relation, Max, Mask | yes | yes (up to 32 bits) |
| SPI CS polarity | yes (CS mirrored at the threshold) | yes |
| SPI timeout without CS | yes (the clock pause) | yes (needed: without a timeout the software keeps it) |
| I2C address relation, 10-bit, direction | yes | yes |
| I2C data, Bytes 1..5 | yes (the first bytes after the address, big-endian) | yes |
| I2C data, Bytes Any | yes (any data byte of a frame, as before) | no |

The software decode stays the reference; the record path places the trigger point on its match.

## SCPI

Stock names (libscope's command table) where they exist, fork additions otherwise. Values decimal or 0x hex.

| Command | Meaning |
|---|---|
| `:TRIGger:RS232:WHEN STARt\|DATA\|ERRor\|CERRor\|EITHer` | Frame, Data, framing error, parity (check) error, either (fork) |
| `:TRIGger:RS232:POLarity POSitive\|NEGative`, `:STOP 1\|1.5\|2`, `:PARity NONE\|EVEN\|ODD`, `:WIDTh 5..8`, `:BAUD <n>`, `:DATA <n>` | as named |
| `:TRIGger:SPI:WHEN CS\|TIMeout`, `:TIMeout <s>`, `:SLOPe POSitive\|NEGative` (clock edge), `:MODE HIGH\|LOW` (CS), `:WIDTh 4..32`, `:DATA <n>` | as named; WHEN CS takes CH3 when the decoder has no CS line |
| `:TRIGger:IIC:WHEN STARt\|ADDRess\|DATA\|NACKnowledge`, `:AWIDth 7\|10`, `:ADDRess <n>`, `:DIRection READ\|WRITe\|RWRite`, `:DBYTes 0..5`, `:DATA <n>` | as named; ADDRess and DATA set the same value; RESTart, STOP, ADATa and AWIDth 8 are refused |
| `:TRIGger:<bus>:RELation EQ\|NE\|GT\|LT\|IN\|OUT`, `:DMAX <n>`, `:MASK <n>` | fork: relation, upper limit, mask of the value |
| `:TRIGger:IIC:ARELation`, `:AMAX <n>` | fork: the address's relation and upper limit |

These commands set the protocol trigger's settings; the trigger type and the decoder's protocol stay as they are.
