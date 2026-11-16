# Channels

The vertical settings of the analogue inputs `CHANnel<n>` (n = 1 to the number of channels of the model), and the selected channel. Scales are chosen at the nearest supported step and the probe factor applies. Waveform names in scripts are `CHAN1` to `CHAN4`, not the UI label `CH1`. Probe deskew (`:CHANnel<n>:TCALibrate`) is separate from the analysis deskew in [calibration](calibration.md).

Menu location: Channels (Display, Scale, Offset, Coupling, Bandwidth limit, Probe ratio, Invert, Unit, Fine scale, Deskew, Zero bias, Expand around, Name).

Part of the [SCPI reference](README.md).

## Commands

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:CHANnel<n>:DISPlay` | Set / Query | Boolean. | `1` | Channels > Display on/off | Rigol-compatible |
| `:CHANnel<n>:SCALe` | Set / Query | Volts/div; setter chooses the nearest supported scale. Probe factor applies. | `1.000000E+00` | Channels > Scale (V/div) | Rigol-compatible |
| `:CHANnel<n>:OFFSet` | Set / Query | Volts. | `-1.500000E+00` | Channels > Offset | Rigol-compatible |
| `:CHANnel<n>:COUPling` | Set / Query | `DC\|AC\|GND`; `DC50` only on a board supporting it. | `DC` | Channels > Coupling | Rigol-compatible |
| `:CHANnel<n>:BWLimit` | Set / Query | `OFF\|20M\|AUTO` or a supported filter name. OOS filtering extends stock choices. | `AUTO` | Channels > Bandwidth limit | Rigol-compatible |
| `:CHANnel<n>:PROBe` | Set / Query | Positive attenuation factor. | `1.000000E+00` | Channels > Probe ratio | Rigol-compatible |
| `:CHANnel<n>:INVert` | Set / Query | Boolean. | `0` | Channels > Invert | Rigol-compatible |
| `:CHANnel<n>:POSition` | Set / Query | Vertical position in divisions, converted through that channel's current scale. | `-1.500000E+00` | Channels > Offset | Rigol-style (command family of the DHO guide) |
| `:CHANnel<n>:UNITs` | Set / Query | Supported channel unit. Use the configured unit when interpreting cursor/readout values. | `VOLT` | Channels > Unit | Rigol-style (command family of the DHO guide) |
| `:CHANnel<n>:VERNier` | Set / Query | Boolean fine vertical control. | `0` | Channels > Fine scale | Rigol-style (command family of the DHO guide) |
| `:CHANnel<n>:TCALibrate` | Set / Query | Channel deskew in seconds; native transfer metadata remains unchanged. | `0.000000E+00` | Channels > Deskew | Rigol-style (command family of the DHO guide) |
| `:CHANnel<n>:BIAS` | Set / Query | Software channel zero-bias correction. This is not a calibration procedure. | `0.000000E+00` | Channels > Zero bias | Rigol-style (command family of the DHO guide) |
| `:CHANnel<n>:EXPand` | Set / Query | Vertical expansion reference supported by the model. | `CENT` | Channels > Expand around | Rigol-style (command family of the DHO guide) |
| `:CHANnel<n>:LABel:SHOW` | Set / Query | Boolean label visibility. | `0` | Channels > Name (label) | Rigol-style (command family of the DHO guide) |
| `:CHANnel<n>:LABel:CONTent` | Set / Query | Label text. | empty | Channels > Name (label) | Rigol-style (command family of the DHO guide) |
| `:CHANnel<n>:BIAS:ZERO` | Action | The mean of the last record becomes part of the zero bias | - | Channels > Zero this input | OOS extension |
| `:CHANnel:ACTive` | Set / Query | The selected channel, 1 to the number of channels | `1` | Channels > active channel | OOS extension |

## Examples

Scale, offset and coupling of channel 1.

```text
:CHANnel1:DISPlay ON
:CHANnel1:SCALe 0.5
:CHANnel1:OFFSet 0.25
:CHANnel1:COUPling AC
:CHANnel1:SCALe?                       -> 5.000000E-01
:CHANnel1:COUPling?                    -> AC
```

Probe ratio, unit, label.

```text
:CHANnel2:PROBe 10
:CHANnel2:UNITs AMPere
:CHANnel2:LABel:CONTent "Iload"
:CHANnel2:LABel:SHOW ON
:CHANnel2:UNITs?                       -> AMP
:CHANnel2:LABel:CONTent?               -> Iload
```

Select the active channel.

```text
:CHANnel:ACTive 2
:CHANnel:ACTive?                       -> 2
```

