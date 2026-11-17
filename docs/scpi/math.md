# Math

Math channels `MATH<n>`. Source names are `CHANn`, a loaded `Rn`, and an earlier `MATHn`; self and forward dependencies are rejected, and a frequency-domain result cannot be a time-domain source. Operators: `ADD`, `SUBTract`, `MULTiply`, `DIVision`, `AND`, `OR`, `XOR`, `NOT`, `NAND`, `NOR`, `FFT`, `INTG`, `DIFF`, `SQRT`, `LG`, `LN`, `EXP`, `ABS`, `LPASs`, `HPASs`, `BPASs`, `BSTop`, `AXB`, `CONV`, `FIR`, `EXPR`, `A2D`, `D2A`, `UNNRZI`. FFT windows: `RECTangle`, `BLACkman`, `HANNing`, `HAMMing`, `FLATtop`.

Menu location: Math (Channel M1..M8, Operation, Source A / B, Scale, Offset, Formula, FFT, Filter, Linear A x + B, Auto scale).

Part of the [SCPI reference](README.md).

## Commands

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:MATH<n>:DISPlay` | Set / Query | Boolean; enable or disable the Math channel. | `0` | Math > Turn on/off | Rigol-style (command family of the DHO guide) |
| `:MATH<n>:OPERator` | Set / Query | Operator; see the list above. The query answers its short token (`SUBT`, `MULT`, `LPAS`, `BST`, `AXB`, ...). | `OFF` | Math > Operation | Rigol-style (command family of the DHO guide) |
| `:MATH<n>:SOURce1` | Set / Query | Source A: `CHANn`, `Rn` or an earlier `MATHn`; keeps native physical timing. | `CHAN1` | Math > Source A | Rigol-style (command family of the DHO guide) |
| `:MATH<n>:SOURce2` | Set / Query | Source B: `CHANn`, `Rn` or an earlier `MATHn`. | `CHAN2` | Math > Source B | Rigol-style (command family of the DHO guide) |
| `:MATH<n>:SCALe` | Set / Query | Math display scale; an FFT uses its dB ladder. | `1.000000E-08` | Math > Scale | Rigol-style (command family of the DHO guide) |
| `:MATH<n>:OFFSet` | Set / Query | Math display offset. | `0.000000E+00` | Math > Offset | Rigol-style (command family of the DHO guide) |
| `:MATH<n>:EXPRession` | Set / Query | Root formula parser, including loaded reference names such as `R1 - R2`. | empty | Math > Formula | Rigol-style (command family of the DHO guide) |
| `:MATH<n>:FFT:SOURce` | Set / Query | FFT input source; selecting it makes the operator FFT. | `CHAN1` | Math > FFT source / dB scale | Rigol-style (command family of the DHO guide) |
| `:MATH<n>:FFT:WINDow` | Set / Query | `RECTangle`, `BLACkman`, `HANNing`, `HAMMing`, `FLATtop`; the query answers `RECT`, `BLAC`, `HANN`, `HAMM`, `FLAT`. | `HANN` | Math > Window | Rigol-style (command family of the DHO guide) |
| `:MATH<n>:FFT:SCALe` | Set / Query | FFT dB display scale. | `1.000000E+01` | Math > FFT source / dB scale | Rigol-style (command family of the DHO guide) |
| `:MATH<n>:FILTer:TYPE` | Set / Query | `LPASs`, `HPASs`, `BPASs`, `BSTop`; the query answers `LPAS`, `HPAS`, `BPAS`, `BST`. | `LPAS` | Math > Filter frequency / bandwidth | Rigol-style (command family of the DHO guide) |
| `:MATH<n>:FILTer:W1` | Set / Query | Corner frequency in Hz (low corner of a band filter). -221 when the operator is not a filter. | `9.9E37` | Math > Filter frequency / bandwidth | Rigol-style (command family of the DHO guide) |
| `:MATH<n>:FILTer:W2` | Set / Query | High corner in Hz of a band-pass or band-stop filter, else -221. | `9.9E37` | Math > Filter frequency / bandwidth | Rigol-style (command family of the DHO guide) |
| `:MATH<n>:AUTO` | Action | Fit scale and offset to the result | - | Math > Auto scale | OOS extension |
| `:MATH<n>:LINear:A` | Set / Query | A of the linear `A x + B`. | `1.000000E+00` | Math > A | OOS extension |
| `:MATH<n>:LINear:B` | Set / Query | B of the linear `A x + B`. | `0.000000E+00` | Math > B | OOS extension |
| `:MATH<n>:RESet` | Action | Auto scale of the math trace; refused while the math is off | - | Math > Auto scale | fork command; name not verified against the DHO guide |

## Examples

Subtract two channels and scale the result.

```text
:MATH1:DISPlay ON
:MATH1:OPERator SUBTract
:MATH1:SOURce1 CHAN1
:MATH1:SOURce2 CHAN2
:MATH1:AUTO
:MATH1:OPERator?                       -> SUBT
:MATH1:SOURce2?                        -> CHAN2
```

FFT with a Hann window.

```text
:MATH2:OPERator FFT
:MATH2:FFT:SOURce CHAN1
:MATH2:FFT:WINDow HANNing
:MATH2:FFT:WINDow?                     -> HANN
:MATH2:OPERator?                       -> FFT
```

A formula over references.

```text
:MATH3:OPERator EXPR
:MATH3:EXPRession "CH1 - CH2"
:MATH3:EXPRession?                     -> CH1 - CH2
```

