# Measurements

Standard and pair measurements, the measurement catalogue, statistics, the gate and the reference levels. Values use base units, phase in degrees and duty or overshoot in percent. Unavailable or pending values are `9.9E37`. `*OPC?` does not wait for these calculations. Readout rows are separated by semicolons: standard rows are `ITEM,SOURCE,value`, pairs `ITEM,A,B,value`, catalogue rows `CAT<id>,A,B,C,D,value`, DVM and counter rows `DVM,mode,SOURCE,value` and `COUNTER,mode,SOURCE,value`, Eye rows `FIGURE,value`.

Menu location: Measurements (Add, Source A / B, Gate, Reference preset, Level type, Low / mid / high level, Statistics, Copy to, Remove, Clear all).

Part of the [SCPI reference](README.md).

## Commands

### Readouts and rows

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:MEASure:SOURce` | Set / Query | `<A>[,<B>]` Default Measure source(s); query returns A. Sources are `CHANn\|Rn`. | `CHAN1` | Measure > Source A | Rigol-compatible |
| `:MEASure:CLEar` | Action | Clear standard, pair and catalog selections through the shared statistics reset. | - | Measurements > Clear all | Rigol-compatible |
| `:MEASure:STRip` | Query | Configured standard/pair/catalog rows, then enabled DVM/counter rows. | - | Measurements > Read all measurement values | OOS extension |
| `:MEASure:READout` | Query | Strip rows plus Eye figures when that analysis page is active. | - | Measurements > Read all measurement values | OOS extension |
| `:MEASure:COPY` | Action | `<source>` Copy the measurements of Source A to another source | - | Measurements > Copy to | OOS extension |
| `:MEASure:ITEM:REMove` | Action | `<n>` Remove one row, by its number in `:MEASure:STRip?` order, or by item and source | - | Measurements > Remove | OOS extension |
| `:MEASure:CATalog:ITEM` | Set / Query | `<id>[,A[,B[,C[,D]]]]` Add the catalogue row `id` on the given sources (defaults: the current Source A, B, C, D); a row already there is left. The query `:MEASure:CATalog:ITEM? <id>[,A,...]` answers its value (`9.9E37` when none) | - | Measurements > Add (catalogue) | OOS extension |
| `:MEASure:CATalog:SOURceC` | Set / Query | Source C or D of the Add page. | `CH3` | Measure > Source C | OOS extension |
| `:MEASure:CATalog:SOURceD` | Set / Query | Source C or D of the Add page. | `CH4` | Measure > Source D | OOS extension |
| `:MEASure:ITEM` | Set / Query | `<item>[,<A>[,<B>]]` Set: add a standard or pair readout (duplicates keep statistics and Single). Query `:MEASure:ITEM? <item>[,<source>]` answers a standard scalar, `:MEASure:ITEM? <pair>,<A>,<B>` a pair scalar with physical-time alignment; the default source is the Measure selection. Items: see [Measure](measure.md#items). | - | Measurements > Add standard measurement | Rigol-compatible. `9.9E37` when no value exists. |

### Gate and edge settings

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:MEASure:GATing` | Set / Query | `SCREEN` follows the viewport; `NONE` means Full record. This gate is an OOS extension. | `NONE` | Measure > Gate | OOS extension |
| `:MEASure:REFS` | Set / Query | `10/90\|20/80` edge thresholds. | `10/90` | Measure levels > Preset | OOS extension |
| `:MEASure:TSETup` | Set / Query | Seconds, nonnegative; used by the setup/hold counting results. | `0.000000E+00` | Measure > Tsetup | Rigol-compatible |
| `:MEASure:THOLd` | Set / Query | Seconds, nonnegative; used by the setup/hold counting results. | `0.000000E+00` | Measure > Thold | Rigol-compatible |
| `:MEASure:SETup:MIN` | Set / Query | Global default low threshold in percent. Per-row overrides remain a separate UI feature. | `10` | Measurements > Low level | Rigol-style (command family of the DHO guide) |
| `:MEASure:SETup:MAX` | Set / Query | Global default high threshold in percent. | `90` | Measurements > High level | Rigol-style (command family of the DHO guide) |
| `:MEASure:SETup:PSA` | Set / Query | Phase source A or B, `CHANn\|Rn`; stock digital/Math roles are not implied. | `CHAN1` | Measurements > Source A / B (phase) | Rigol-style (command family of the DHO guide) |
| `:MEASure:SETup:PSB` | Set / Query | Phase source A or B, `CHANn\|Rn`; stock digital/Math roles are not implied. | `CHAN2` | Measurements > Source A / B (phase) | Rigol-style (command family of the DHO guide) |
| `:MEASure:SETup:DSA` | Set / Query | Delay source A or B, `CHANn\|Rn`. | `CHAN1` | Measurements > Source A / B (delay) | Rigol-style (command family of the DHO guide) |
| `:MEASure:SETup:DSB` | Set / Query | Delay source A or B, `CHANn\|Rn`. | `CHAN2` | Measurements > Source A / B (delay) | Rigol-style (command family of the DHO guide) |
| `:MEASure:BURSt:IDLE` | Set / Query | Burst idle interval in seconds. | `5.000000E-05` | Measure > Idle | Rigol-style (command family of the DHO guide) |
| `:MEASure:OUTSide` | Set / Query | `ABOVe\|BELow\|BOTH` outside-level measurement selection. | `ABOV` | Measure > Outside | Rigol-style (command family of the DHO guide) |
| `:MEASure:NPERiods` | Set / Query | Configured period count for relevant measurements. | `6` | Measure > N periods | Rigol-style (command family of the DHO guide) |
| `:MEASure:EDGes` | Set / Query | `CLOCk\|DATA` edge measurement mode. | `CLOC` | Measure > Edges | Rigol-style (command family of the DHO guide) |
| `:MEASure:INDicator` | Set / Query | Measurement labels; `:CURSor:MEASure:INDicator[?]` is an alias. | `0` | Measure > Labels on trace | Rigol-style (command family of the DHO guide) |

### Reference levels

Low, mid and high are percent or volts depending on `:MEASure:LEVel:TYPE`.

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:MEASure:LEVel:TYPE` | Set / Query | Reference levels as `PERCent` of the waveform or `VOLTs`. Query answers `PERC` or `VOLT`. | `PERC` | Measure levels > Level type | OOS extension |
| `:MEASure:LEVel:PRESet` | Set / Query | `10/90`, `20/80` or `CUSTom` (the Reference preset row). | `10/90` | Measure levels > Preset | OOS extension |
| `:MEASure:LEVel:LOW` | Set / Query | Reference level, percent or volts (by `:MEASure:LEVel:TYPE`). | `10` | Measure levels > Low | OOS extension |
| `:MEASure:LEVel:MID` | Set / Query | Reference level, percent or volts (by `:MEASure:LEVel:TYPE`). | `50` | Measure levels > Mid | OOS extension |
| `:MEASure:LEVel:HIGH` | Set / Query | Reference level, percent or volts (by `:MEASure:LEVel:TYPE`). | `90` | Measure levels > High | OOS extension |
| `:MEASure:LEVel:HYSTeresis` | Set / Query | Hysteresis of the edge detection, percent or volts. | `5` | Measure levels > Hysteresis | OOS extension |
| `:MEASure:LEVel:BASEtop` | Set / Query | Base/top method: `AUTO`, `MINMax`, `MEAN`, `MODE`. | `AUTO` | Measure levels > Base / top | OOS extension |
| `:MEASure:LEVel:FALL:SEParate` | Set / Query | `ON` gives the falling edge its own levels. | `0` | Fall levels > Fall levels | OOS extension |
| `:MEASure:LEVel:FALL:LOW` | Set / Query | Falling-edge level, used when `:MEASure:LEVel:FALL:SEParate` is ON. | `10` | Fall levels > Fall low | OOS extension |
| `:MEASure:LEVel:FALL:MID` | Set / Query | Falling-edge level, used when `:MEASure:LEVel:FALL:SEParate` is ON. | `50` | Fall levels > Fall mid | OOS extension |
| `:MEASure:LEVel:FALL:HIGH` | Set / Query | Falling-edge level, used when `:MEASure:LEVel:FALL:SEParate` is ON. | `90` | Fall levels > Fall high | OOS extension |

### Statistics

Statistics count accepted observations. A stopped reference can have a valid current value with count 0 after a reset.

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:MEASure:STATistic:DISPlay` | Set / Query | Statistics summary visibility. Toggling it retains accumulated observations. | `0` | Statistics > Statistics | Rigol-style (command family of the DHO guide) |
| `:MEASure:STATistic:RESet` | Action | Central statistics reset, including catalog/deferred accounting. It does not create an acquisition. | - | Measurements > Statistics | Rigol-style (command family of the DHO guide) |
| `:MEASure:STATistic:POPulation` | Set / Query | `CURRent` (the statistics of the shown acquisition run) or `ACCumulated`. A change resets the statistics. | `ACC` | Statistics > Population | OOS extension |
| `:MEASure:STATistic:SHOW` | Set / Query | Statistics columns: any of `SDEViation`, `MINimum`, `MAXimum` comma-separated, or `NONE`. | `SDEV,MIN,MAX` | Measurements > Statistics columns | OOS extension |
| `:MEASure:STATistic:ITEM` | Set / Query | `<item>[,source[,source]]` Set: add the readout and show statistics. Query `:MEASure:STATistic:ITEM? <field>,<item>[,source[,source]]` with field `MAXimum\|MINimum\|CURRent\|AVERages\|DEViation\|CNT` (`COUNt` alias). | - | Measurements > Statistics | Rigol-compatible |

## Items

The canonical long spellings are accepted case-insensitively, as is the short form of the mixed-case spelling (for example `FREQ`, `RTIM`, `PWID`). The strip and readout print the canonical spelling. Aliases: `CYCAmp` = `CAMP`, `CYCPP` = `CPP`, `UI` = `ESTUI`, `LINSLOPE` = `LSLO`, `LININT` = `LINT`, `L0` to `L3` = `PAM0` to `PAM3`, `ONES` = `BITS`.

### Standard measurement items

`VPP`, `VMAX`, `VMIN`, `VAVG`, `VRMS`, `VAMP`, `FREQUENCY`, `PERIOD`, `RTIME`, `FTIME`, `PWIDTH`, `PDUTY`, `NDUTY`, `NWIDTH`, `VTOP`, `VBASE`, `OVERSHOOT`, `PRESHOOT`, `HTIME`, `LTIME`, `BWIDTH`, `TOUTSIDE`, `NPERIODS`, `CAMP`, `CTOP`, `CBASE`, `CPP`, `CMAX`, `CMIN`, `VUPPER`, `VMID`, `VLOWER`, `CAVG`, `CRMS`, `ACRMS`, `SDEVIATION`, `AREA`, `CAREA`, `PPULSES`, `NPULSES`, `PEDGES`, `NEDGES`, `TVMAX`, `TVMIN`, `PSLEW`, `NSLEW`, `ESTUI`, `MEDIAN`, `MODE`, `LSLO`, `LINT`, `PAM0`, `PAM1`, `PAM2`, `PAM3`, `PSEP`, `BITS`, `PLLUI`, `UIWTD`, `FOLD`.

### Pair items

`RRDELAY`, `FFDELAY`, `RFDELAY`, `FRDELAY`, `RRPHASE`, `FFPHASE`, `RFPHASE`, `FRPHASE`, `RRSKEW`, `FFSKEW`, `RFSKEW`, `FRSKEW`, `SETUP`, `HOLD`, `SETHOLD`, `FSETUP`, `FHOLD`, `DSETUP`, `DHOLD`, `DSETHOLD`.

Setup and hold take source A as the clock and B as the data. `SETUP`/`HOLD` latch on rising clock edges, `FSETUP`/`FHOLD` on falling ones, and the `D` items on both, as a DDR strobe (DQS) does. A clock edge counts only when the data changed inside its own cycle: after the previous latching edge and within one typical edge spacing. Data that holds steady between strobe bursts adds no setup or hold. `SETHOLD`/`DSETHOLD` count the edges that break `:MEASure:TSETup`/`:MEASure:THOLd`.

### Catalogue rows

The six-tab catalogue's selected rows are exported by `:MEASure:STRip?` and `:MEASure:READout?` as `CAT<id>,A,B,C,D,value`, including unused stored role columns. Add and query them with `:MEASure:CATalog:ITEM`; the per-row settings card stays on the setup key `row_cfg`.

### Analysis item families

The other analyser pages have their own `:ITEM?` queries: [`:POWer:ITEM?`](power.md), `:SPECtrum:ITEM?` ([spectrum](spectrum.md)), `:MAGnetic:ITEM?`, `:INVerter:ITEM?`, `:DPT:ITEM?` ([power](power.md)), `:BANDwidth:ITEM?` ([bode](bode.md)) and `:EYE:ITEM?` ([eye](eye.md)). See [reference analysis](../reference-analysis.md) for sampling and physical alignment.

## Examples

One-shot measurements.

```text
:STOP
:MEASure:ITEM? VPP,CHAN1               -> 2.564586E+00
:MEASure:ITEM? FREQuency,CHAN1         -> 9.999857E+02
:MEASure:ITEM? VRMS,CHAN1              -> 1.765733E+00
```

Build a measurement strip and read it.

```text
:MEASure:CLEar
:MEASure:SOURce CHAN1
:MEASure:ITEM VPP
:MEASure:ITEM PERiod
:MEASure:STRip?                        -> VPP,CHAN1,2.564586E+00;PERIOD,CHAN1,1.000014E-03
```

Statistics.

```text
:MEASure:STATistic:DISPlay ON
:MEASure:STATistic:ITEM VPP,CHAN1
:MEASure:STATistic:ITEM? CURRent,VPP,CHAN1 -> 2.564586E+00
:MEASure:STATistic:RESet
```

Reference levels.

```text
:MEASure:LEVel:PRESet 20/80
:MEASure:LEVel:PRESet?                 -> 20/80
:MEASure:LEVel:BASEtop MEAN
:MEASure:LEVel:BASEtop?                -> MEAN
:MEASure:GATing SCREEN
:MEASure:GATing?                       -> SCREEN
```

