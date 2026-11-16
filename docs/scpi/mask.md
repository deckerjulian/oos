# Mask, zones and histogram

Pass/fail mask testing on a channel, the two zone outlines and the histogram. Mask sources are physical channels, not references or Math. A mask needs accepted channel data; enabling the page or querying a counter does not prove a valid mask. The histogram describes the rendered display-density distribution and the current view, not an exact full-record population.

Menu location: Mask, zones, histogram (Mask test on/off, Mask source, Tolerance X / Y, Create, Clear, Invert, On fail, Save / load mask, Zones, Histogram).

Part of the [SCPI reference](README.md).

## Commands

### Mask

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:MASK:CREate:REF` | Action | `<Rn>` Mask from a reference | - | Mask, zones, histogram > Create from ref | OOS extension |
| `:MASK:CLEar` | Action | Clear the mask | - | Mask, zones, histogram > Clear mask | OOS extension |
| `:MASK:INVert` | Set / Query | Boolean. | `0` | Mask, zones, histogram > Invert | OOS extension |
| `:MASK:OUTPut` | Set / Query | `STOP`, `CONTinue` (on fail). | `CONT` | Mask, zones, histogram > On fail | name not verified against the DHO guide |
| `:MASK:SAVE` | Action | `["path"]` Mask file `["path"]`; without a path the slot's `mask<n>.txt` in the save folder. | - | Mask, zones, histogram > Save mask | OOS extension |
| `:MASK:LOAD` | Action | `["path"]` Mask file `["path"]`; without a path the slot's `mask<n>.txt` in the save folder. | - | Mask, zones, histogram > Load mask | OOS extension |
| `:MASK:ENABle` | Set / Query | Boolean. Enabling a page does not prove a valid mask. | `0` | Mask, zones, histogram > Mask test on/off | Rigol-style (command family of the DHO guide) |
| `:MASK:SOURce` | Set / Query | `<channel>` Physical CH role only; not a reference or Math input. | `CHAN1` | Mask, zones, histogram > Mask source | Rigol-style (command family of the DHO guide) |
| `:MASK:X` | Set / Query | `number` Horizontal tolerance, as the menu row. | `2.000000E-01` | Mask, zones, histogram > Tolerance X / Y | Rigol-style (command family of the DHO guide) |
| `:MASK:Y` | Set / Query | `number` Vertical tolerance, as the menu row. | `4.000000E-01` | Mask, zones, histogram > Tolerance X / Y | Rigol-style (command family of the DHO guide) |
| `:MASK:CREate` | Action | Build the mask from the source's waveform; error -230 when it has none. | - | Mask, zones, histogram > Create mask from channel | Rigol-style (command family of the DHO guide) |
| `:MASK:RESet` | Action | Reset the pass/fail counters. | - | Mask, zones, histogram > Reset counts | Rigol-style (command family of the DHO guide) |
| `:MASK:OPERate` | Set / Query | `RUN\|STOP`; the query answers `RUN` only when the mask is on and the scope runs. Error -224 when the mask is off. | `STOP` | Mask, zones, histogram > Run / stop mask test | Rigol-style (command family of the DHO guide) |
| `:MASK:FAILed` | Query | Number of failed acquisitions. | `0` | Mask, zones, histogram > Read pass / fail / total | Rigol-style (command family of the DHO guide) |
| `:MASK:PASSed` | Query | Number of passed acquisitions. | `0` | Mask, zones, histogram > Read pass / fail / total | Rigol-style (command family of the DHO guide) |
| `:MASK:TOTal` | Query | Number of tested acquisitions. | `0` | Mask, zones, histogram > Read pass / fail / total | Rigol-style (command family of the DHO guide) |

### Histogram

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:HISTogram:ENABle` | Set / Query | Boolean; ON selects the vertical histogram when it was off. | `0` | Mask, zones, histogram > Histogram | Rigol-style (command family of the DHO guide) |
| `:HISTogram:TYPE` | Set / Query | `HORizontal\|VERTical`; the query answers `HOR` or `VERT`. | `VERT` | Mask, zones, histogram > Histogram | Rigol-style (command family of the DHO guide) |
| `:HISTogram:SOURce` | Set / Query | `<channel>` CH roles only; turns the channel on and makes it the Measure source. | `CHAN1` | Mask, zones, histogram > Histogram | Rigol-style (command family of the DHO guide) |
| `:HISTogram:STATistics:RESult` | Query | `Sum,Peaks,Max,Min,Pk_Pk,Mean,Median,Mode,Bin width,Sigma` as `name:value` pairs; `9.9E37` where no figure exists. Describes the rendered display-density distribution, not the full native record. | - | Mask, zones, histogram > Histogram | Rigol-style (command family of the DHO guide) |

### Zones

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:ZONE<n>:STATe` | Set / Query | Boolean; zone n is 1 or 2. | `0` | Mask, zones, histogram > Zone on | OOS extension |
| `:ZONE<n>:SOURce` | Set / Query | `CHANn`. | `CHAN1` | Mask, zones, histogram > source | OOS extension |
| `:ZONE<n>:CONDition` | Set / Query | `INTersect` or `NOTintersect`. | `INT` | Mask, zones, histogram > condition | OOS extension |
| `:ZONE<n>:BOX` | Set / Query | `<t1>,<v1>,<t2>,<v2>`: seconds from the trigger and volts of two corners (the query only for a box) | - | Mask, zones, histogram > Draw zone box | OOS extension |
| `:ZONE<n>:POINts` | Set / Query | `<t>,<v>,...`: a polygon of three or more points | empty | Mask, zones, histogram > Draw zone shape | OOS extension |
| `:ZONE<n>:CLEar` | Action | Delete the outline, keep the source | - | Mask, zones, histogram > Delete zone | OOS extension |

## Examples

Create a mask from a channel and run the test.

```text
:STOP
:MASK:SOURce CHAN1
:MASK:X 0.3
:MASK:Y 0.3
:MASK:CREate
:MASK:ENABle ON
:MASK:OUTPut STOP
:MASK:OPERate RUN
:MASK:TOTal?                           -> 0
:MASK:FAILed?                          -> 0
```

Histogram.

```text
:HISTogram:SOURce CHAN1
:HISTogram:TYPE VERTical
:HISTogram:ENABle ON
:HISTogram:ENABle?                     -> 1
```

Zone 1 as a box.

```text
:ZONE1:SOURce CHAN1
:ZONE1:CONDition INTersect
:ZONE1:BOX 0,0.5,1e-3,1.5
:ZONE1:STATe ON
:ZONE1:BOX?                            -> 0.000000E+00,5.000000E-01,1.000000E-03,1.500000E+00
:ZONE1:CLEar
```

