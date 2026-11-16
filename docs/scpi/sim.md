# Signal simulator (SIM)

SIM generates a synthetic high-speed signal into a reference slot. The knob settings apply on the next `:SIM:PLAY ON`. A candidate replaces accepted data only when its trigger qualifies, Auto permits fallback, or Force is requested. Supported PRBS orders: 7, 9, 13, 15, 23, 31.

Menu location: Simulator (Preset, Slot, Pattern, Rate, Levels, Rise, Bandwidth, De-emphasis, Noise, Spread spectrum, Jitter, Play, Show, Defaults, Add to R, Load file, Live).

Part of the [SCPI reference](README.md).

## Commands

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:SIM:PRESet` | Set / Query | `<name>` Select a supported preset by its UI name. | `PCIe 4.0` | SIM > Preset | OOS extension |
| `:SIM:SLOT` | Set / Query | `R1..R10` Generated waveform destination. | `R1` | SIM > Slot | OOS extension |
| `:SIM:PATTern` | Set / Query | `PRBS7` Supported orders: 7, 9, 13, 15, 23, 31. | `PRBS31` | SIM > Pattern | OOS extension |
| `:SIM:PLAY` | Set / Query | `ON\|OFF` Start/apply the configured simulation or stop/cancel it. | `0` | SIM > Play | OOS extension |
| `:SIM:BUSY` | Query | Whether generation is pending. | `0` | SIM > Play | OOS extension |
| `:SIM:SHOW` | Set / Query | `ON\|OFF` Visibility of the generated reference slot. | `0` | SIM > Show | OOS extension |
| `:SIM:SSC:MODE` | Set / Query | `OFF\|DOWN\|CENTER`. | `DOWN` | SIM > Spread spectrum | OOS extension |
| `:SIM:SSC:DEViation` | Set / Query | Fraction, 0 through 0.05. | `5.000000E-03` | SIM > Spread spectrum | OOS extension |
| `:SIM:SSC:FREQuency` | Set / Query | Hz, 1e3 through 1e6. | `3.300000E+04` | SIM > Spread spectrum | OOS extension |
| `:SIM:JITTer:DCD` | Set / Query | Peak-to-peak seconds, 0 through 100e-12. | `0.000000E+00` | SIM > Jitter | OOS extension |
| `:SIM:JITTer:CORRelated` | Set / Query | RMS seconds, 0 through 50e-12. | `0.000000E+00` | SIM > Jitter | OOS extension |
| `:SIM:JITTer:CORNer` | Set / Query | Hz, 1e3 through 100e6. | `1.000000E+06` | SIM > Jitter | OOS extension |
| `:SIM:DEFault` | Action | The SIM knobs back to the preset | - | Simulator > Defaults | OOS extension |
| `:SIM:ADDRef` | Action | Add the generated signal to the SIM slot's reference | - | Simulator > Add to R | OOS extension |
| `:SIM:LOAD` | Action | `"path"` Load a waveform file into the SIM slot | - | Simulator > Load file | OOS extension |
| `:SIM:LIVE` | Action | Leave SIM mode | - | Simulator > Live | OOS extension |
| `:SIM:RATE` | Set / Query | Bits per second, 1e8 to 6.4e10. Range 1e8 to 64e9. | `1.600000E+10` | Simulator > Rate | OOS extension |
| `:SIM:LEVel:HIGH` | Set / Query | Volts, -2 to 2. Range -2.0 to 2.0. | `5.000000E-01` | Simulator > High | OOS extension |
| `:SIM:LEVel:LOW` | Set / Query | Volts, -2 to 2. Range -2.0 to 2.0. | `-5.000000E-01` | Simulator > Low | OOS extension |
| `:SIM:RISE` | Set / Query | 20-80 % rise time, 5 ps to 2 ns. Range 5e-12 to 2e-9. | `2.000000E-11` | Simulator > Rise | OOS extension |
| `:SIM:BWLimit` | Set / Query | Hz. Range 10e6 to 50e9. | `1.200000E+10` | Simulator > Bandwidth | OOS extension |
| `:SIM:DEEMphasis` | Set / Query | DB, 0 to 12. Range 0.0 to 12.0. | `4.500000E+00` | Simulator > De-emph | OOS extension |
| `:SIM:NOISe` | Set / Query | Volts RMS. Range 0.0 to 50e-3. | `5.000000E-03` | Simulator > Noise | OOS extension |
| `:SIM:JITTer:RANDom` | Set / Query | Random jitter, seconds RMS. Range 0.0 to 50e-12. | `1.200000E-12` | Simulator > RJ RMS | OOS extension |
| `:SIM:JITTer:PERiodic:AMPLitude` | Set / Query | Periodic jitter, seconds peak. Range 0.0 to 50e-12. | `4.000000E-12` | Simulator > PJ peak | OOS extension |
| `:SIM:JITTer:PERiodic:FREQuency` | Set / Query | Hz. Range 1e5 to 50e6. | `8.000000E+06` | Simulator > PJ freq | OOS extension |

## Examples

Generate a PRBS signal into R1.

```text
:SIM:SLOT R1
:SIM:PATTern PRBS7
:SIM:JITTer:RANDom 2e-12
:SIM:PLAY ON
:SIM:BUSY?                             -> 1
:SIM:PATTern?                          -> PRBS7
:SIM:JITTer:RANDom?                    -> 2.000000E-12
```

Defaults and leaving SIM.

```text
:SIM:DEFault
:SIM:RATE?                             -> 1.600000E+10
:SIM:LIVE
```

