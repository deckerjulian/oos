# Cursors, DVM and counter

Manual, track and XY cursors with their readouts, the digital voltmeter and the frequency counter. Cursor changes preserve capture statistics and an armed Single. Track values use the selected native or reference waveform and physical origin. DVM and counter sources are acquired channels, not references.

Menu location: Cursors (Mode, Type, Source, Cursor A / B, Y1 / Y2, readout) and DVM and counter.

Part of the [SCPI reference](README.md).

## Commands

### Cursors

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:CURSor:MODE` | Set / Query | `OFF\|MANual\|TRACk\|XY`; MAN keeps the selected time/amplitude type. | `OFF` | Cursors > Mode | Rigol-compatible |
| `:CURSor:MANual:TYPE` | Set / Query | `TIME\|AMPLitude`. | `TIME` | Cursors > Type | Rigol-compatible |
| `:CURSor:MANual:SOURce` | Set / Query | `<wave>` Selected `CHANn\|Rn`, without changing acquisition. SOURce1 aliases it; a different SOURce2 is unsupported. | `CHAN1` | Cursors > Source | Rigol-compatible |
| `:CURSor:MANual:CAX` | Set / Query | `seconds` Cursor A physical time; clips to the visible span. | `9.9E37` | Cursors > Cursor A position | Rigol-compatible |
| `:CURSor:MANual:CBX` | Set / Query | `seconds` Cursor B physical time; clips to the visible span. | `9.9E37` | Cursors > Cursor B position | Rigol-compatible |
| `:CURSor:MANual:CAY` | Set / Query | `volts` Cursor A volts in the selected source's displayed lane, including display zoom. | `9.9E37` | Cursors > Cursor A position | Rigol-compatible |
| `:CURSor:MANual:CBY` | Set / Query | `volts` Cursor B volts in the selected source's displayed lane, including display zoom. | `9.9E37` | Cursors > Cursor B position | Rigol-compatible |
| `:CURSor:TRACk:SOURce` | Set / Query | `<wave>` Selected `CHANn\|Rn`, without changing acquisition. SOURce1 aliases it; a different SOURce2 is unsupported (see `:CURSor:TRACk:SOURce2`). | `CHAN1` | Cursors > Source | Rigol-compatible |
| `:CURSor:TRACk:CAX` | Set / Query | `seconds` Cursor A physical time; clips to the visible span. | `9.9E37` | Cursors > Cursor A position | Rigol-compatible |
| `:CURSor:TRACk:CBX` | Set / Query | `seconds` Cursor B physical time; clips to the visible span. | `9.9E37` | Cursors > Cursor B position | Rigol-compatible |
| `:CURSor:TRACk:CAY` | Set / Query | `volts` Cursor A volts in the selected source's displayed lane, including display zoom. | `9.9E37` | Cursors > Cursor A position | Rigol-compatible |
| `:CURSor:TRACk:CBY` | Set / Query | `volts` Cursor B volts in the selected source's displayed lane, including display zoom. | `9.9E37` | Cursors > Cursor B position | Rigol-compatible |
| `:CURSor:TRACk:SOURce1` | Set / Query | `<wave>` Track cursor A source (alias of `:CURSor:TRACk:SOURce`). | `CHAN1` | Cursors > Source A | Rigol-compatible |
| `:CURSor:TRACk:SOURce2` | Set / Query | `<wave>` Track cursor B source `CHANn\|Rn`; unset B follows A. `YDELta` is unavailable when the units differ. | `CHAN1` | Cursors > Source B | Rigol-compatible |
| `:CURSor:XY:AX` | Set / Query | `number` Independent X/Y coordinate on one selected waveform, not independent input sources. Setting both time and voltage positions promotes the UI to XY cursors. | `9.9E37` | Cursors > XY | Rigol-compatible |
| `:CURSor:XY:BX` | Set / Query | `number` Independent X/Y coordinate on one selected waveform, not independent input sources. Setting both time and voltage positions promotes the UI to XY cursors. | `9.9E37` | Cursors > XY | Rigol-compatible |
| `:CURSor:XY:AY` | Set / Query | `number` Independent X/Y coordinate on one selected waveform, not independent input sources. Setting both time and voltage positions promotes the UI to XY cursors. | `9.9E37` | Cursors > XY | Rigol-compatible |
| `:CURSor:XY:BY` | Set / Query | `number` Independent X/Y coordinate on one selected waveform, not independent input sources. Setting both time and voltage positions promotes the UI to XY cursors. | `9.9E37` | Cursors > XY | Rigol-compatible |
| `:CURSor:{MANual\|TRACk\|XY}:{AXValue\|BXValue\|AYValue\|BYValue\|XDELta\|IXDelta\|YDELta}` | Query | Cursor values and deltas. IXDelta is 1/abs(time delta). A missing value is `9.9E37`. | `9.9E37` | Cursor readout | Rigol-compatible. One row for the 21 query headers. |
| `:CURSor:MEASure:INDicator` | Set / Query | Alias of `:MEASure:INDicator`. | `0` | Measurements > Labels on trace | OOS extension |

### DVM

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:DVM:ENABle` | Set / Query | Boolean. | `0` | DVM and counter > DVM on/off | Rigol-compatible |
| `:DVM:SOURce` | Set / Query | `<channel>` Acquired CHANn, not a reference. | `CHAN1` | DVM and counter > DVM source | Rigol-compatible |
| `:DVM:MODE` | Set / Query | `DC\|ACRMS\|ACDC` (`ACDCrms` is an alias of `ACDC`). | `DC` | DVM and counter > DVM mode | Rigol-compatible |
| `:DVM:CURRent` | Query | Current DVM reading in volts. | `1.250042E+00` | DVM and counter > DVM reading | Rigol-compatible |

### Counter

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:COUNter:ENABle` | Set / Query | Boolean. | `0` | DVM and counter > Counter on/off | Rigol-compatible |
| `:COUNter:SOURce` | Set / Query | `<channel>` Acquired CHANn, not a reference. | `CHAN1` | DVM and counter > Counter source | Rigol-compatible |
| `:COUNter:MODE` | Set / Query | `FREQuency\|PERiod\|TOTalize`. | `FREQ` | DVM and counter > Counter mode | Rigol-compatible |
| `:COUNter:NDIGits` | Set / Query | Digits, 3 to 7. | `5` | DVM and counter > Counter digits | Rigol-compatible |
| `:COUNter:CURRent` | Query | Current counter result. | `9.9E37` | DVM and counter > Counter reading | Rigol-compatible |
| `:COUNter:TOTalize:CLEar` | Action | Reset the accumulated counter statistics. | - | DVM and counter > Clear counter | Rigol-compatible |

## Examples

Manual time cursors.

```text
:CURSor:MODE MANual
:CURSor:MANual:TYPE TIME
:CURSor:MANual:SOURce CHAN1
:CURSor:MANual:CAX -1e-3
:CURSor:MANual:CBX 1e-3
:CURSor:MANual:XDELta?                 -> 2.000000E-03
:CURSor:MANual:IXDelta?                -> 5.000000E+02
:CURSor:MODE OFF
```

DVM and counter.

```text
:DVM:ENABle ON
:DVM:SOURce CHAN1
:DVM:MODE DC
:DVM:CURRent?                          -> 1.250042E+00
:COUNter:ENABle ON
:COUNter:MODE FREQuency
:COUNter:CURRent?                      -> 9.999857E+02
```

