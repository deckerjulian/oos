# Decoders and bus

Protocol decoders: the `:DECode:` subtree edits the selected decoder slot, `:BUS<n>:` addresses decoder n directly (the decoder the menu edits stays the same afterwards), and `:BUS:LANE:` is the plot bus (I2C lane). Protocol-specific rows exist only for their protocol: an option the protocol does not have is -224. Decoder parameters take the option text of the menu row, and the query answers it in capitals. Reference sources may be used as roles if their native intervals overlap in physical time; no synthetic LA input is invented for a reference.

Menu location: Decode and bus (Protocol, Source / clock / CS, Threshold, Format, Event table, protocol parameters, Plot bus, Save events).

Part of the [SCPI reference](README.md).

## Commands

### Decoder slot

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:DECode:SELect` | Set / Query | One-based decoder slot. | `1` | Decode and bus > Decoder slot select | OOS extension |
| `:DECode:PROTocol` | Set / Query | `<name>` Protocol UI name; comparison ignores case and punctuation, for example I2C, SPI, C-PHY. `PARallel` is the parallel bus. | `UART` | Decode and bus > Protocol | OOS extension |
| `:DECode:STATe` | Set / Query | Boolean. | `0` | Decode and bus > Decoder on/off | OOS extension |
| `:DECode:INPut` | Set / Query | `WAVEFORM\|LA`; `ANALOG` aliases waveform. LA requires available/enabled logic inputs. | `WAVEFORM` | Decode and bus > Input | OOS extension |
| `:DECode:SOURce` | Set / Query | Main data role; `CHANn\|Rn`. | `CH3` | Decode and bus > Source / clock / CS / source 4 / B line | OOS extension |
| `:DECode:SOURce2` | Set / Query | Clock role; `CHANn\|Rn`. | `CH4` | Decode and bus > Source / clock / CS / source 4 / B line | OOS extension |
| `:DECode:CS` | Set / Query | Chip-select role; optional roles accept `NONE`. Roles depend on the protocol. | `CH2` | Decode and bus > Source / clock / CS / source 4 / B line | OOS extension |
| `:DECode:SOURce4` | Set / Query | Fourth role; optional, `NONE` clears it. | `NONE` | Decode and bus > Source / clock / CS / source 4 / B line | OOS extension |
| `:DECode:DIFFerential` | Set / Query | Differential (B line) role; optional, `NONE` clears it. | `NONE` | Decode and bus > Source / clock / CS / source 4 / B line | OOS extension |
| `:DECode:THReshold` | Set / Query | Configured threshold. For C-PHY this is minimum adjacent-wire level separation. | `1.650000E+00` | Decode and bus > Threshold | OOS extension |
| `:DECode:CPHY:RATE` | Set / Query | `AUTO` or known symbols/second; aliases CPHY:SRATe and DECode:SRATe. | `AUTO` | Decode and bus > C-PHY symbol rate | OOS extension |
| `:DECode:BUSY` | Query | Worker readiness, without changing acquisition. | `0` | Decode and bus > Read decoded events | OOS extension |
| `:DECode:EVENts` | Query | Physical-time event rows, `BUSY` while pending, or an error if unavailable. | empty | Decode and bus > Read decoded events | OOS extension |
| `:DECode:SAVE` | Action | Save the decoded events as CSV. | - | Decode and bus > Export events | OOS extension |

### Bus and common parameters

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:BUS<n>:MODE` | Set / Query | Decoder protocol: `RS232` (alias `UART`), `IIC` (`I2C`), `SPI`, `CAN`, `LIN`, `PARallel`, or another protocol by its UI name without punctuation. The query answers `RS232`, `IIC`, `PAR`, ... | `RS232` | Decode and bus > Protocol | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:DISPlay` | Set / Query | Boolean decoder visibility. | `0` | Decode and bus > Decoder on/off | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:FORMat` | Set / Query | `HEX\|ASCii`. | `ASC` | Decode and bus > Format | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:EVENt` | Set / Query | Shared decode event-table visibility. | `0` | Decode and bus > Event table | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:THReshold` | Set / Query | Decoder threshold in source units. | `1.650000E+00` | Decode and bus > Threshold | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:DATA` | Query | Ready event rows in an IEEE block. `BUSY` while required decoding is pending; unavailable sources return an error. | - | Decode and bus > Read decoded events | Rigol-style (command family of the DHO guide) |

### UART, I2C and SPI

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:BUS<n>:RS232:TX` | Set / Query | Role source `CHANn\|Rn`. UART decoding uses the configured primary input. | `CHAN3` | Decode and bus > Source | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:RS232:RX` | Set / Query | Role source `CHANn\|Rn`. UART decoding uses the configured primary input. | `CHAN4` | Decode and bus > Source | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:RS232:BAUD` | Set / Query | Supported baud ladder entry (the nearest one is used). | `115200` | Decode and bus > UART baud | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:RS232:PARity` | Set / Query | Parity. | `NONE` | Decode and bus > UART parity | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:RS232:DBITs` | Set / Query | Data-bit count. | `8` | Decode and bus > Data bits | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:IIC:SCLK:SOURce` | Set / Query | I2C clock role `CHANn\|Rn`. | `CHAN4` | Decode and bus > Source / clock / CS | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:IIC:SDA:SOURce` | Set / Query | I2C data role `CHANn\|Rn`. | `CHAN3` | Decode and bus > Source / clock / CS | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:SPI:SCLK:SOURce` | Set / Query | SPI role `CHANn\|Rn`. | `CHAN4` | Decode and bus > Source / clock / CS | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:SPI:MOSI:SOURce` | Set / Query | SPI role `CHANn\|Rn`. | `CHAN3` | Decode and bus > Source / clock / CS | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:SPI:SS:SOURce` | Set / Query | SPI select role `CHANn\|Rn`; `OFF` or `NONE` disables it. | `CHAN2` | Decode and bus > Source / clock / CS | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:SPI:DBITs` | Set / Query | Supported SPI word length. | `8` | Decode and bus > Data bits | Rigol-style (command family of the DHO guide) |
| `:BUS<n>:SPI:MODE` | Set / Query | `0` to `3` (clock polarity and phase); an SPI decoder only. | `0` | Decode and bus > Mode | Rigol-compatible |

### CAN, SENT, USB and protocol options

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:BUS<n>:CAN:BAUD` | Set / Query | Bit rate, option text of the menu row (a CAN decoder only). | `500_KBIT/S` | Decode and bus > Bit rate | Rigol-compatible |
| `:BUS<n>:CAN:SIGNal` | Set / Query | `RX/TX` or `CAN_H` (a CAN decoder only). | `RX/TX` | Decode and bus > Signal | Rigol-compatible |
| `:BUS<n>:CAN:FD:BAUD` | Set / Query | FD data rate, option text of the menu row. | `2_M` | Decode and bus > FD data rate | Rigol-compatible |
| `:BUS<n>:CAN:FD:ABAUd` | Set / Query | FD arbitration rate, option text of the menu row. | `AUTO` | Decode and bus > Arbitration rate | Rigol-compatible |
| `:BUS<n>:SENT:TICK` | Set / Query | SENT tick, option text of the menu row. | `3_µS` | Decode and bus > Tick | OOS extension |
| `:BUS<n>:SENT:NIBBles` | Set / Query | Nibbles, 1 to 6. | `6` | Decode and bus > Nibbles | OOS extension |
| `:BUS<n>:USB:SPEed` | Set / Query | USB speed, option text of the menu row (a USB decoder only). | `AUTO` | Decode and bus > Speed | OOS extension |
| `:BUS<n>:OPTion1` | Set / Query | Option of the protocol module, in the order of the menu rows; -224 when the protocol has no such option. | empty | Decode and bus > Option | OOS extension |
| `:BUS<n>:OPTion2` | Set / Query | Option of the protocol module, in the order of the menu rows; -224 when the protocol has no such option. | empty | Decode and bus > Option | OOS extension |
| `:BUS<n>:OPTion3` | Set / Query | Option of the protocol module, in the order of the menu rows; -224 when the protocol has no such option. | empty | Decode and bus > Option | OOS extension |
| `:BUS<n>:OPTion4` | Set / Query | Option of the protocol module, in the order of the menu rows; -224 when the protocol has no such option. | empty | Decode and bus > Option | OOS extension |

### Plot bus

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:BUS:LANE:STATe` | Set / Query | Boolean; the plot bus (the I2C lane) on or off. | `0` | Decode and bus > Plot bus | OOS extension |
| `:BUS:LANE:LABel` | Set / Query | Label text, up to 12 characters. | `I2C` | Decode and bus > Plot bus | OOS extension |
| `:BUS:LANE:POSition` | Set / Query | Position in divisions, -3.6 to 3.6. | `-3.000000E+00` | Decode and bus > Plot bus | OOS extension |
| `:BUS:LANE:CLOCk:SOURce` | Set / Query | Source `CHANn\|Rn`. | `CH1` | Decode and bus > Clock | OOS extension |
| `:BUS:LANE:DATA:SOURce` | Set / Query | Source `CHANn\|Rn`. | `CH2` | Decode and bus > Data | OOS extension |
| `:BUS:LANE:CLOCk:THReshold` | Set / Query | Volts. | `1.650000E+00` | Decode and bus > thresholds | OOS extension |
| `:BUS:LANE:DATA:THReshold` | Set / Query | Volts. | `1.650000E+00` | Decode and bus > thresholds | OOS extension |
| `:BUS:LANE:RW` | Set / Query | `LEFT_OUT`, `INCLUDED` (the read/write bit in the address) | `LEFT_OUT` | Decode and bus > R/W bit | OOS extension |

## Examples

UART decoder on channel 1.

```text
:DECode:SELect 1
:DECode:PROTocol UART
:DECode:SOURce CHAN1
:DECode:THReshold 1.5
:BUS1:RS232:BAUD 115200
:BUS1:RS232:PARity NONE
:DECode:STATe ON
:DECode:PROTocol?                      -> UART
```

I2C decoder.

```text
:DECode:PROTocol I2C
:BUS1:IIC:SCLK:SOURce CHAN1
:BUS1:IIC:SDA:SOURce CHAN2
:DECode:STATe ON
:BUS1:FORMat HEX
:BUS1:FORMat?                          -> HEX
:DECode:BUSY?                          -> 0
```

Plot bus lane.

```text
:BUS:LANE:STATe ON
:BUS:LANE:LABel "I2C"
:BUS:LANE:POSition 2
:BUS:LANE:STATe?                       -> 1
:BUS:LANE:LABel?                       -> I2C
```

