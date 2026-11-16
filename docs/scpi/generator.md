# Built-in generator

Built-in generator commands use `:SOURce[<n>]:...` for a generator available on the model; `:SOURce<n>` talks to generator n, whichever one the page shows. They configure the actual generator when hardware operation is enabled; unsupported models report -241 hardware missing.

Menu location: Generator (Output, Waveform, Frequency, Amplitude / offset, Square duty / ramp symmetry, Arbitrary, Modulation).

Part of the [SCPI reference](README.md).

## Commands

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:SOURce:ARB:LOAD` | Action | `["path"]` Load an arbitrary wave from a file (one number per line or a CSV); default `arb.csv` in the storage folder. | - | Generator > Load arb.csv | name not verified against the DHO guide |
| `:SOURce<n>:ARB:LOAD` | Action | `["path"]` Load an arbitrary wave from a file (one number per line or a CSV); default `arb.csv` in the storage folder. | - | Generator > Load arb.csv | name not verified against the DHO guide |
| `:SOURce[<n>]:OUTPut[:STATe]` | Set / Query | Boolean generator output. | `0` | Generator > Output on/off | OOS extension (Rigol-style spelling). Error -241 on a model without a generator. |
| `:SOURce[<n>]:FUNCtion` | Set / Query | `SINusoid\|SQUare\|RAMP\|PULSe\|NOISe\|DC\|ARB`; the query answers `SIN\|SQU\|RAMP\|PULS\|NOIS\|DC\|ARB`. | `SIN` | Generator > Waveform | OOS extension (Rigol-style spelling) |
| `:SOURce[<n>]:FREQuency` | Set / Query | `hertz` Generator frequency. | `1.000000E+03` | Generator > Frequency | OOS extension (Rigol-style spelling) |
| `:SOURce[<n>]:VOLTage[:AMPLitude]` | Set / Query | `volts` Generator amplitude. | `1.000000E+00` | Generator > Amplitude / offset | OOS extension (Rigol-style spelling) |
| `:SOURce[<n>]:VOLTage:OFFSet` | Set / Query | `volts` Generator offset. | `0.000000E+00` | Generator > Amplitude / offset | OOS extension (Rigol-style spelling) |
| `:SOURce[<n>]:FUNCtion:SQUare:DUTY` | Set / Query | `percent` 5 to 95 (also `:FUNCtion:PULSe:DUTY`). | `5.000000E+01` | Generator > Square duty / ramp symmetry | OOS extension (Rigol-style spelling) |
| `:SOURce[<n>]:FUNCtion:RAMP:SYMMetry` | Set / Query | `percent` 0 to 100. | `5.000000E+01` | Generator > Square duty / ramp symmetry | OOS extension (Rigol-style spelling) |
| `:SOURce[<n>]:ARB:DATA` | Action | `<v>,<v>,...` One period of the arbitrary wave, any scale; error -224 when the samples do not fit. | - | Generator > Arbitrary: capture from / capture screen | OOS extension (Rigol-style spelling) |
| `:SOURce[<n>]:ARB:CAPTure` | Action | `<channel>` Use the screen of a channel as the arbitrary wave. | - | Generator > Arbitrary: capture from / capture screen | OOS extension (Rigol-style spelling) |
| `:SOURce[<n>]:MOD[:STATe]` | Set / Query | Boolean modulation. | `0` | Generator > Modulation on, type, rate, depth/deviation, shape | OOS extension (Rigol-style spelling) |
| `:SOURce[<n>]:MOD:TYPe` | Set / Query | `AM\|FM\|PM`; the query also answers `NONE`. | `NONE` | Generator > Modulation on, type, rate, depth/deviation, shape | OOS extension (Rigol-style spelling) |
| `:SOURce[<n>]:MOD:AM:DEPTh` | Set / Query | `percent` 0 to 120. | `1.000000E+02` | Generator > Modulation on, type, rate, depth/deviation, shape | OOS extension (Rigol-style spelling) |
| `:SOURce[<n>]:MOD:FM:DEViation` | Set / Query | `hertz` At least 0.01. | `1.000000E+02` | Generator > Modulation on, type, rate, depth/deviation, shape | OOS extension (Rigol-style spelling) |
| `:SOURce[<n>]:MOD:PM:DEViation` | Set / Query | `degrees` 0 to 360. | `9.000000E+01` | Generator > Modulation on, type, rate, depth/deviation, shape | OOS extension (Rigol-style spelling) |
| `:SOURce[<n>]:MOD:{AM\|FM\|PM}:INTernal:FREQuency` | Set / Query | `hertz` Modulation rate, 0.002 to 1e6. | `1.000000E+02` | Generator > Modulation on, type, rate, depth/deviation, shape | OOS extension (Rigol-style spelling) |
| `:SOURce[<n>]:MOD:{AM\|FM\|PM}:INTernal:FUNCtion` | Set / Query | `SINusoid\|SQUare\|TRIangle\|RAMP\|NRAMp\|NOISe`; the query answers the short form. | `SIN` | Generator > Modulation on, type, rate, depth/deviation, shape | OOS extension (Rigol-style spelling) |

## Examples

Sine, 1 kHz, 1 Vpp.

```text
:SOURce1:FUNCtion SINusoid
:SOURce1:FREQuency 1000
:SOURce1:VOLTage 1
:SOURce1:VOLTage:OFFSet 0
:SOURce1:OUTPut ON
:SOURce1:FREQuency?                    -> 1.000000E+03
```

Amplitude modulation.

```text
:SOURce1:MOD:TYPe AM
:SOURce1:MOD:AM:DEPTh 50
:SOURce1:MOD:AM:INTernal:FREQuency 10
:SOURce1:MOD:STATe ON
:SOURce1:MOD:TYPe?                     -> AM
```

