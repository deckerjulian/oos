# Logic analyser

The 16 digital lines `D0` to `D15` in two pods of eight. `:LA:DATA?` is an IEEE block of little-endian u16 words, bit n representing Dn. LA availability is model-dependent.

Menu location: Logic analyser (LA on/off, Show line D0..D15, Threshold per pod, Pattern per line, Lane height).

Part of the [SCPI reference](README.md).

## Commands

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:LA:DIGital<n>:PATTern` | Set / Query | `H`, `L`, `X` for line n (0 to 15), as the trigger pattern | `X` | Logic analyser > LA pattern | OOS extension |
| `:LA:POD<n>:PRESet` | Set / Query | Threshold preset of pod 1 or 2 by its name (`TTL`, `CMOS`, ..., `USER`) | `TTL_1.4_V` | Logic analyser > Thr D0-7, Thr D8-15 | OOS extension |
| `:LA:SIZE` | Set / Query | Lane height by its name | `MEDIUM` | Logic analyser > Lane height | Rigol-compatible |
| `:LA:STATe` | Set / Query | Boolean. LA availability is model-dependent. | `0` | Logic analyser > LA on/off | OOS extension (Rigol-style spelling) |
| `:LA:DIGital<n>:DISPlay` | Set / Query | Boolean, line n (0 to 15). | `1` | Logic analyser > Show line D0..D15 | OOS extension (Rigol-style spelling) |
| `:LA:POD<n>:THReshold` | Set / Query | `volts` Threshold of pod 1 (D0-7) or 2 (D8-15). | `1.4e0` | Logic analyser > Threshold per pod (user value) | OOS extension (Rigol-style spelling) |
| `:LA:POINts` | Query | Number of points of the logic record. | - | Logic analyser > Read LA record | OOS extension (Rigol-style spelling) |
| `:LA:PREamble` | Query | Preamble of the logic record. | - | Logic analyser > Read LA record | OOS extension (Rigol-style spelling) |
| `:LA:DATA` | Query | IEEE block of little-endian u16 words; bit n is line Dn. | - | Logic analyser > Read LA record | OOS extension (Rigol-style spelling) |

## Examples

Logic lines and pods.

```text
:LA:STATe?                             -> 0
:LA:DIGital1:PATTern H
:LA:DIGital1:PATTern?                  -> H
:LA:SIZE?                              -> MEDIUM
```

