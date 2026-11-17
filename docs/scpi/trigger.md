# Trigger

Trigger type, sweep, source and level, and one subsection per trigger type. The typed subtrees set the same fields as the menu pages (and the `:SYSTem:SET trig_*` keys); they do not change the trigger type, `:TRIGger:MODE` does. Times are seconds, 1 ns to 10 s. Pulse width conditions, slope and delay times, the window time, the runt width and the duration times share two fields: "lower" (`WIDTh`, `LWIDth`, `TIME`, `TLOWer`, `WLOWer`) is the menu's Time and "upper" (`UWIDth`, `TUPPer`, `WUPPer`) its Upper limit. Source and level of every type are `:TRIGger:EDGE:SOURce`, `:TRIGger:SOURce2` and `:TRIGger:EDGE:LEVel` (also `:TRIGger:<type>:SOURce`). Reference triggering is a software search over the stored samples; SIM cannot qualify LA, Pattern or Duration inputs.

Menu location: Trigger (Type, Source, Level, Slope, Sweep, Holdoff, Coupling, Noise reject and the page of each type).

Part of the [SCPI reference](README.md).

## Commands

### Type, sweep and levels

These apply whatever the type.

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TRIGger:MODE` | Set / Query | Actual trigger type. Same OOS handler as `:TRIGger:TYPe[?]`. | `EDGE` | Trigger > Type | Rigol-compatible |
| `:TRIGger:SWEep` | Set / Query | `AUTO\|NORMal\|SINGle`; independent of trigger type. | `AUTO` | Trigger > Sweep | Rigol-compatible |
| `:TRIGger:STATus` | Query | `STOP\|TD\|AUTO\|WAIT`. A completed Single reports STOP. | `STOP` | Run state (status line) | Rigol-compatible |
| `:TRIGger:HOLDoff` | Set / Query | Seconds, 0 through 10. | `0.000000E+00` | Trigger > Holdoff | Rigol-compatible |
| `:TRIGger:NREJect` | Set / Query | Boolean. | `0` | Trigger > Noise reject | Rigol-compatible |
| `:TRIGger:SOURce2` | Set / Query | Secondary CH/ref source for applicable OOS trigger types. | `CHAN4` | Trigger > Source B | OOS extension |
| `:TRIGger:LEVel` | Set / Query | Volts; the first level whatever the type. | `1.500000E+00` | Trigger > Level | OOS extension |
| `:TRIGger:LEVel2` | Set / Query | Volts; the second level whatever the type (alias of the slope, window and runt B level). | `5.000000E-01` | Trigger > Level 2 | OOS extension |
| `:TRIGger:LEVel:AUTO` | Action | Level to the middle of the source (the menu's Level 50 %, `Item::Trig50`) | - | Trigger > Level 50 % | OOS extension |
| `:TRIGger:COUPling` | Set / Query | Coupling of the edge trigger on a channel: `DC`, `AC`, `LFR` (low-frequency reject), `HFR`. | `DC` | Trigger > Coupling | Rigol-style (command family of the DHO guide) |
| `:TRIGger:FORCe` | Action | Same as `:TFORce`. | - | Run control > Force trigger | Rigol-compatible |
| `:TRIGger:TYPe` | Set / Query | `<type>` `EDGE`, `PULSE`, `SLOPE`, `VIDEO`, `PATTERN`, `DURATION`, `TIMEOUT`, `RUNT`, `WINDOW`, `DELAY`, `SETUPHOLD`, `NTHEDGE`, `BUS`. The query answers the UI's descriptive type name. | `Edge` | Trigger > Type | Rigol-compatible. Same handler as `:TRIGger:MODE`, which answers short tokens. |
| `:TRIGger:DELay:SA`, `:SB` | Set / Query | First and second source, `CHAN1`..`CHAN4` | `CHAN1`, `CHAN4` | Trigger > Delay > sources | fork command; name not verified against the DHO guide |
| `:TRIGger:DELay:ASLop`, `:BSLop` | Set / Query | `POSitive`, `NEGative` | `POS` | Trigger > Delay > slopes | fork command; name not verified against the DHO guide |
| `:TRIGger:DELay:TYPE` | Set / Query | `GREater`, `LESS`, `GLESs`, `GOUT` | `LESS` | Trigger > Delay > when | fork command; name not verified against the DHO guide |
| `:TRIGger:DURation:TYPE` | Set / Query | One of `H`, `L`, `X` per channel, comma separated | - | Trigger > Duration > pattern | fork command; name not verified against the DHO guide |
| `:TRIGger:SLOPe:POLarity` | Set / Query | `POSitive`, `NEGative`, `RFALl` | `POS` | Trigger > Slope > slope | fork command; name not verified against the DHO guide |
| `:TRIGger:SLOPe:WINDow` | Query | `TAB`: both levels are used | `TAB` | - | fork command; name not verified against the DHO guide |
| `:TRIGger:VIDeo:POLarity` | Query | `POSitive`; a set of another value is refused | `POS` | - | fork command; name not verified against the DHO guide |
| `:TRIGger:SHOLd:DSRC`, `:CSRC` (also `:TRIGger:SETup:`) | Set / Query | Data and clock source, `CHAN1`..`CHAN4` | - | Trigger > Setup/hold > sources | fork command; name not verified against the DHO guide |
| `:TRIGger:SHOLd:TYPE` | Query | `SETHold`: setup and hold are both active | `SETH` | - | fork command; name not verified against the DHO guide |
| `:TRIGger:ZONE<n>:STATe` | Set / Query | Zone 1 or 2 on or off | `0` | Trigger > Zone | fork command; name not verified against the DHO guide |
| `:TRIGger:ZONE<n>:SOURce` | Set / Query | `CHAN1`..`CHAN4` | - | Trigger > Zone > source | fork command; name not verified against the DHO guide |
| `:TRIGger:ZONE<n>:MODE` | Set / Query | `INTersect`, `NOTintersect` | `INT` | Trigger > Zone > must / must not | fork command; name not verified against the DHO guide |
| `:TRIGger:ZONE<n>:PLACement` | Set / Query | A box: `t1,v1,t2,v2` in seconds and volts | - | drawn on the screen | fork command; name not verified against the DHO guide |
| `:TRIGger:ZONE<n>:POINts` | Set / Query | An outline: `t,v` pairs, at least three | - | drawn on the screen | fork command; name not verified against the DHO guide |
| `:TRIGger:ZONE<n>:CLEar` | Action | Remove the zone | - | Trigger > Zone > clear | fork command; name not verified against the DHO guide |

### Edge

Source `CHANn|Rn`; available LA inputs can use `D0` to `D15`.

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TRIGger:EDGE:SOURce` | Set / Query | `CHANn\|Rn`; available LA inputs can use `D0` through `D15`. | `CHAN1` | Trigger > Source | Rigol-compatible |
| `:TRIGger:EDGE:LEVel` | Set / Query | Volts. | `1.500000E+00` | Trigger > Level | Rigol-compatible |
| `:TRIGger:EDGE:SLOPe` | Set / Query | `POS\|NEG\|RFAL`. | `POS` | Trigger > Slope | Rigol-compatible |

### Pulse width

Pulse-width conditions share the lower and upper time fields described above.

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TRIGger:PULSe:POLarity` | Set / Query | `POSitive`, `NEGative` | `POS` | Trigger > Polarity | Rigol-compatible |
| `:TRIGger:PULSe:WHEN` | Set / Query | `GREater`, `LESS`, `RANGe`, `OUTside`; or `PGReater`, `PLESs`, `PGLess`, `NGReater`, `NLESs`, `NGLess` (polarity and condition together) | `LESS` | Trigger > When | Rigol-compatible |
| `:TRIGger:PULSe:WIDTh` | Set / Query | Seconds, 1 ns to 10 s. The lower time limit, the menu's Time. | `2.000000E-05` | Trigger > Time | Rigol-compatible |
| `:TRIGger:PULSe:LWIDth` | Set / Query | Seconds, 1 ns to 10 s. The lower time limit, the menu's Time. | `2.000000E-05` | Trigger > Time | Rigol-compatible |
| `:TRIGger:PULSe:UWIDth` | Set / Query | Seconds, 1 ns to 10 s. The upper time limit, the menu's Upper limit. | `4.000000E-05` | Trigger > Upper limit | Rigol-compatible |
| `:TRIGger:PULSe:LEVel` | Set / Query | Volts | `1.500000E+00` | Trigger > Level | Rigol-compatible |

### Slope

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TRIGger:SLOPe:SLOPe` | Set / Query | `POSitive`, `NEGative`, `RFALl` | `POS` | Trigger > Slope | Rigol-compatible |
| `:TRIGger:SLOPe:WHEN` | Set / Query | `GREater`, `LESS`, `RANGe`, `OUTside` | `LESS` | Trigger > When | Rigol-compatible |
| `:TRIGger:SLOPe:TIME` | Set / Query | Seconds, 1 ns to 10 s. The lower time limit, the menu's Time. | `2.000000E-05` | Trigger > Time | Rigol-compatible |
| `:TRIGger:SLOPe:TLOWer` | Set / Query | Seconds, 1 ns to 10 s. The lower time limit, the menu's Time. | `2.000000E-05` | Trigger > Time | Rigol-compatible |
| `:TRIGger:SLOPe:TUPPer` | Set / Query | Seconds, 1 ns to 10 s. The upper time limit, the menu's Upper limit. | `4.000000E-05` | Trigger > Upper limit | Rigol-compatible |
| `:TRIGger:SLOPe:ALEVel` | Set / Query | Volts. | `1.500000E+00` | Trigger > Level | Rigol-compatible |
| `:TRIGger:SLOPe:BLEVel` | Set / Query | Volts. | `5.000000E-01` | Trigger > Level 2 | Rigol-compatible |

### Video

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TRIGger:VIDeo:STANdard` | Set / Query | `PAL`, `NTSC` | `PAL` | Trigger > Standard | Rigol-compatible |
| `:TRIGger:VIDeo:MODE` | Set / Query | `ALINes`, `LINE`, `ODDField`, `EVENfield`, `AFIelds` | `AFI` | Trigger > Sync | Rigol-compatible |
| `:TRIGger:VIDeo:LINE` | Set / Query | Line number, 1 to (lines / 2 - 10) of the standard | `10` | Trigger > Line | Rigol-compatible |
| `:TRIGger:VIDeo:LEVel` | Set / Query | Volts | `1.500000E+00` | Trigger > Level | Rigol-compatible |

### Pattern

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TRIGger:PATTern:PATTern` | Set / Query | `"HLXRF..."`: one character per channel (H, L, X, R rising, F falling), optionally followed by the 16 logic lines D0..D15 (H, L, X) | `"HXXH"` | Trigger > CH1..CH4, LA pattern | Rigol-compatible |
| `:TRIGger:PATTern:LEVel` | Set / Query | `<ch>,<volts>`; the query takes `<ch>` (`1` or `CHAN1`) | `1.250000E+00` | Trigger > Level (per channel) | Rigol-compatible |

### Duration

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TRIGger:DURation:PATTern` | Set / Query | As the pattern trigger; R and F count as X. | `"HXXH"` | Trigger > Pattern per channel and level | Rigol-style spelling, not listed in the coverage report |
| `:TRIGger:DURation:LEVel` | Set / Query | As the pattern trigger; R and F count as X. | `1.250000E+00` | Trigger > Pattern per channel and level | Rigol-style spelling, not listed in the coverage report |
| `:TRIGger:DURation:WHEN` | Set / Query | `GREater`, `LESS`, `RANGe`, `OUTside` (as the pulse trigger). | `LESS` | Trigger > When | Rigol-style spelling, not listed in the coverage report |
| `:TRIGger:DURation:TLOWer` | Set / Query | Seconds, 1 ns to 10 s. The lower time limit, the menu's Time. | `2.000000E-05` | Trigger > Time | Rigol-style spelling, not listed in the coverage report |
| `:TRIGger:DURation:TUPPer` | Set / Query | Seconds, 1 ns to 10 s. The upper time limit, the menu's Upper limit. | `4.000000E-05` | Trigger > Upper limit | Rigol-style spelling, not listed in the coverage report |

### Timeout

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TRIGger:TIMeout:SLOPe` | Set / Query | `POSitive`, `NEGative`, `RFALl`. | `POS` | Trigger > Slope | Rigol-style spelling, not listed in the coverage report |
| `:TRIGger:TIMeout:TIME` | Set / Query | Seconds, 1 ns to 10 s. The lower time limit, the menu's Time. | `2.000000E-05` | Trigger > Time | Rigol-style spelling, not listed in the coverage report |
| `:TRIGger:TIMeout:LEVel` | Set / Query | Volts. | `1.500000E+00` | Trigger > Level | Rigol-style spelling, not listed in the coverage report |

### Runt

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TRIGger:RUNT:UPPer` | Set / Query | Absolute high threshold; alias `ALEVel`. A threshold below the low one is refused with `-224`. | `1.500000E+00` | Trigger > Level | Rigol-compatible |
| `:TRIGger:RUNT:LOWer` | Set / Query | Absolute low threshold; alias `BLEVel`. A threshold above the high one is refused with `-224`. | `5.000000E-01` | Trigger > Level 2 | Rigol-compatible |
| `:TRIGger:RUNT:POLarity` | Set / Query | `POSitive`, `NEGative` | `POS` | Trigger > Polarity | Rigol-compatible |
| `:TRIGger:RUNT:WHEN` | Set / Query | `NONE`, `GREater`, `LESS`, `RANGe`, `OUTside` | `LESS` | Trigger > When | Rigol-compatible |
| `:TRIGger:RUNT:WLOWer` | Set / Query | Seconds, 1 ns to 10 s. The lower time limit, the menu's Time. | `2.000000E-05` | Trigger > Time | Rigol-compatible |
| `:TRIGger:RUNT:WUPPer` | Set / Query | Seconds, 1 ns to 10 s. The upper time limit, the menu's Upper limit. | `4.000000E-05` | Trigger > Upper limit | Rigol-compatible |

### Window

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TRIGger:WINDow:SLOPe` | Set / Query | `POSitive`, `NEGative`, `RFALl` | `POS` | Trigger > Slope | Rigol-compatible |
| `:TRIGger:WINDow:POSition` | Set / Query | `ENTer`, `EXIT`, `TIMe` (also `INSide`) | `ENT` | Trigger > Trigger on | Rigol-compatible |
| `:TRIGger:WINDow:ALEVel` | Set / Query | Volts. | `1.500000E+00` | Trigger > Level (Level A on Delay) | Rigol-compatible |
| `:TRIGger:WINDow:BLEVel` | Set / Query | Volts. | `5.000000E-01` | Trigger > Level 2 (Level B on Delay) | Rigol-compatible |
| `:TRIGger:WINDow:WHEN` | Set / Query | `GREater`, `LESS`, `RANGe`, `OUTside` (as the pulse trigger), used for the `TIMe` position. | `LESS` | Trigger > When | Rigol-compatible |
| `:TRIGger:WINDow:TIME` | Set / Query | Seconds, 1 ns to 10 s. The lower time limit, the menu's Time. | `2.000000E-05` | Trigger > Time | Rigol-compatible |

### Delay

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TRIGger:DELay:SLOPA` | Set / Query | Slope of A: `POSitive`, `NEGative`, `RFALl`. | `POS` | Trigger > Slope | Rigol-style spelling, not listed in the coverage report |
| `:TRIGger:DELay:SLOPB` | Set / Query | Slope of B: `POSitive`, `NEGative`, `RFALl`. | `POS` | Trigger > Slope B | Rigol-style spelling, not listed in the coverage report |
| `:TRIGger:DELay:WHEN` | Set / Query | `GREater`, `LESS`, `RANGe`, `OUTside` (as the pulse trigger). | `LESS` | Trigger > When | Rigol-style spelling, not listed in the coverage report |
| `:TRIGger:DELay:TLOWer` | Set / Query | Seconds, 1 ns to 10 s. The lower time limit, the menu's Time. | `2.000000E-05` | Trigger > Time | Rigol-style spelling, not listed in the coverage report |
| `:TRIGger:DELay:TUPPer` | Set / Query | Seconds, 1 ns to 10 s. The upper time limit, the menu's Upper limit. | `4.000000E-05` | Trigger > Upper limit | Rigol-style spelling, not listed in the coverage report |
| `:TRIGger:DELay:ALEVel` | Set / Query | Volts. | `1.500000E+00` | Trigger > Level (Level A on Delay) | Rigol-style spelling, not listed in the coverage report |
| `:TRIGger:DELay:BLEVel` | Set / Query | Volts. | `5.000000E-01` | Trigger > Level 2 (Level B on Delay) | Rigol-style spelling, not listed in the coverage report |

### Setup and hold

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TRIGger:SHOLd:STIMe` | Set / Query | Setup time, seconds, 1 ns to 10 s. | `1.000000E-06` | Trigger > Setup | Rigol-compatible |
| `:TRIGger:SHOLd:HTIMe` | Set / Query | Hold time, seconds, 1 ns to 10 s. | `1.000000E-06` | Trigger > Hold | Rigol-compatible |
| `:TRIGger:SHOLd:DSOurce` | Set / Query | Data source `CHANn\|Rn`. | `CHAN1` | Trigger > Source | name not verified against the DHO guide |
| `:TRIGger:SHOLd:CSOurce` | Set / Query | Clock source `CHANn\|Rn`. | `CHAN4` | Trigger > Source B | name not verified against the DHO guide |
| `:TRIGger:SHOLd:SLOPe` | Set / Query | Clock edge: `POSitive`, `NEGative`, `RFALl`. | `POS` | Trigger > Slope B | Rigol-compatible |

### Nth edge

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TRIGger:NEDGe:SLOPe` | Set / Query | `POSitive`, `NEGative`, `RFALl` | `POS` | Trigger > Slope | Rigol-compatible |
| `:TRIGger:NEDGe:IDLE` | Set / Query | Seconds | `5.000000E-05` | Trigger > Idle time | Rigol-compatible |
| `:TRIGger:NEDGe:EDGE` | Set / Query | 1 to 65535 | `1` | Trigger > Edge no. | Rigol-compatible |
| `:TRIGger:NEDGe:LEVel` | Set / Query | Volts | `1.500000E+00` | Trigger > Level | Rigol-compatible |

### Bus

Bus trigger uses the configured decoder protocol, roles and match settings; auxiliary sources must have real overlapping data.

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TRIGger:BUS:WHEN` | Set / Query | `FRAMe`, `ID`, `DATA`, `ERRor` | `ID` | Trigger > Match | OOS extension |
| `:TRIGger:BUS:VALue` | Set / Query | Value, decimal or `0x` hexadecimal; the query answers `0x..` | `0x123` | Trigger > Value | OOS extension |

## Examples

Edge trigger on channel 2, rising, 1 V.

```text
:TRIGger:MODE EDGE
:TRIGger:EDGE:SOURce CHAN2
:TRIGger:EDGE:LEVel 1
:TRIGger:EDGE:SLOPe POSitive
:TRIGger:SWEep NORMal
:TRIGger:EDGE:SOURce?                  -> CHAN2
:TRIGger:MODE?                         -> EDGE
```

Pulse width trigger: positive pulses wider than 2 us.

```text
:TRIGger:MODE PULSE
:TRIGger:PULSe:POLarity POSitive
:TRIGger:PULSe:WHEN GREater
:TRIGger:PULSe:WIDTh 2e-6
:TRIGger:PULSe:WHEN?                   -> GRE
:TRIGger:PULSe:WIDTh?                  -> 2.000000E-06
:TRIGger:MODE?                         -> PULS
```

Pattern trigger: CH1 high, CH2 low, CH3 and CH4 do not care.

```text
:TRIGger:MODE PATTERN
:TRIGger:PATTern:PATTern "HLXX"
:TRIGger:PATTern:PATTern?              -> "HLXX"
:TRIGger:MODE?                         -> PATT
```

Put the level in the middle of the source.

```text
:TRIGger:EDGE:SOURce CHAN1
:TRIGger:LEVel:AUTO
:TRIGger:LEVel?                        -> 1.247747E+00
```

