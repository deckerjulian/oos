# Front panel (MCU, keys, knobs, LEDs)

Verified on the unit: pressing keys for 40 s raised the UART0 receive counter from 0 to 310 bytes and produced no Linux
input events.

## Physical layer

- UART0 (`serial@ff180000`, pins GPIO2_C0/C1), Linux `/dev/ttyS0`.
- **1 500 000 baud, 8N1**, raw mode, opened `O_RDWR|O_NOCTTY`.
- One reader owns the port. A process that only sends LED or power frames can open it write-only for each command.
  The MCU sends nothing until a key is pressed (`rx:0` after boot).

## Frame (both directions)

```
FA  cmd  data  crc  AF          5 bytes
crc = CRC-7/MMC over [cmd, data]: poly 0x09, init 0, MSB first, result 7 bit (0..0x7f)
```

Reference: `c = 0; for b in (cmd, data): c ^= b; 8x: c = ((c << 1) ^ 0x12) if c & 0x80 else (c << 1); c &= 0xfe`;
result `c >> 1`.

## MCU -> RK3399 (keys and knobs)

- Check head 0xFA, tail 0xAF and the CRC of every frame.
- `cmd == 6` with `data & 0x70 != 0` is the motor of other Rigol products; it is not used on the DHO.
- A row/column form of the code also exists: `row | col << 4` (+0x200 on press), from a per-row state table. The data
  byte below is what counts.
- To record codes under OOS (where nothing else reads the port): `tools/frontpanel.py dump`.

Recorded under OOS (`stty -F /dev/ttyS0 1500000 raw` + `dd`), all 18 frames with a valid CRC.
The MCU sends keys without any initialisation from the host.

**Key code = the DATA byte** of the frame, for keys and knobs alike. Apart from the special value 6, the `cmd` byte is
ignored (it varies per press, meaning unknown). A key sends its code on press and `data 0x00` on release; a knob sends
one frame per detent.
Verified on the unit: Run/Stop, Single, Auto, GI, Horizontal Menu, Navigate, H-Position CW, H-Scale CW/CCW, V-Scale CW.
The other codes are not yet verified on hardware.

| Code | Element | | Code | Element |
|---|---|---|---|---|
| 0x01 | Ref | | 0x11 | Default |
| 0x02 | LA | | 0x12 | Analyse |
| 0x03 | GI (generator, AFG) | | 0x13 | Touch Lock |
| 0x04 | Horizontal Menu | | 0x14 | Quick |
| 0x05 | Navigate | | 0x15-0x18 | CH1-CH4 |
| 0x06 / 0x07 / 0x08 | navigation forward / play / back | | 0x19 | Math |
| 0x09 | Trigger Slope | | 0x1a | Trigger Menu |
| 0x0a | Trigger Force | | 0x1b | Power |
| 0x0b | Clear | | 0xf1-0xf4 | CH1-CH4 input overload: not sent by the MCU, never occur on the DHO924S (`board.md` 6) |
| 0x0c | Run/Stop | | | |
| 0x0d | Auto | | | |
| 0x0e | Single | | | |
| 0x0f | Measure | | | |
| 0x10 | Cursor | | | |

| Knob | clockwise | counter-clockwise | pressed |
|---|---|---|---|
| Horizontal POSITION | 0x8a | 0x8b | 0x8c |
| Vertical POSITION | 0x9a | 0x9b | 0x9c |
| Multipurpose 2 | 0xaa | 0xab | 0xac |
| Multipurpose 1 | 0xba | 0xbb | 0xbc |
| Horizontal SCALE | 0xca | 0xcb | 0xcc |
| Vertical SCALE | 0xda | 0xdb | 0xdc |
| Trigger LEVEL | 0xea | 0xeb | 0xec |

Codes of other Rigol models (menu keys F1-F7, 0x3x-0x7x) do not occur on the DHO. `tools/frontpanel.py decode <hex bytes>`
names recorded frames.

LEDs: `cmd 1/2/3, data 0xff` switched all front-panel LEDs on (verified on the unit, by eye).

## RK3399 -> MCU (LEDs, power)

- LEDs: three frames `cmd 1, 2, 3` with `data` = bitmask of hardware LEDs 0-7, 8-15, 16-18. A set bit is on; there is no
  colour control. The MCU chooses the channel colours.

| Element | hw bit | | Element | hw bit |
|---|---|---|---|---|
| CH1-CH4 | 10-13 | | TOUCH | 0 |
| RUN (green) | 16 | | A up/down | 7 |
| STOP (red) | 17 | | B left/right | 5 |
| SINGLE (red) | 15 | | SLOPE1 | 9 |
| SINGLE (orange) | 14 | | SLOPE2 | 8 |
| LA | 1 | | A left/right | 6 |
| G1 (GI) | 2 | | B up/down | 3 |
| G2 | none | | INTENSITY | 4 |

  RUN, STOP and both SINGLE bits are driven together as the run state. The slope pair (bits 9 and 8) follows the trigger
  slope. Verified on the unit, by eye: bit 4 lights the blue Intensity LED; CH1-CH4 (in their channel colours), RUN/STOP
  green, the rising-slope LED.
- Power commands, the power LED and the MCU firmware version: `board.md` section 4. Motor functions exist for other
  Rigol products and are unused on the DHO.

## Other inputs seen on the unit

- `rk29-keypad` (`event0`): the SARADC keys of the Rockchip reference design and the power key GPIO0_A5.
- `fts_ts` (`event1`): FocalTech touch on I2C2 @0x38.
- USB: the front USB-A port is host port 3-1 (controller fe900000). The unit has no built-in Wi-Fi; the only device seen there was a plugged-in Realtek RTL8188EU Wi-Fi stick (0bda:8179). No HID device.
