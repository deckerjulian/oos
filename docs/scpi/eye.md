# Eye and jitter

Eye diagram and jitter analysis. A full-record Eye of a long reference runs in the background: until it finishes the page and `:EYE:ITEM?` still show the previous result, so check that `EDGES` and `UI` belong to the new source before reading the figures. With an Eye clock source (a strobe such as DQS) the data is folded on each strobe edge itself.

Menu location: Eye and jitter (Source, Bit rate, Clock, Rail, BER, PN band, Notch, PLL bandwidth, NPJ, High-pass, Place, Auto Eye/Jitter, Clear eye).

Part of the [SCPI reference](README.md).

## Commands

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:EYE:ITEM` | Query | `<figure>` Current configured Eye result; does not itself run one-click Auto Eye. | - | Eye and jitter > Read eye / jitter figures | OOS extension |
| `:EYE:AUTO` | Action | Auto Eye/Jitter: measure the bit rate and open the eye. | - | Eye and jitter > Auto Eye/Jitter | OOS extension |
| `:EYE:CLEar` | Action | Clear the eye. | - | Eye and jitter > Clear eye | OOS extension |
| `:EYE:SOURce` | Set / Query | Source `CHANn\|Rn`. | `CH1` | Eye > Source | OOS extension |
| `:EYE:CLOCk` | Set / Query | Source `CHANn\|Rn`, or `OFF`. | `OFF` | Eye > Clock | OOS extension |
| `:EYE:RAIL` | Set / Query | Source `CHANn\|Rn`, or `OFF`. | `OFF` | Eye > Rail | OOS extension |
| `:EYE:RATE` | Set / Query | Bit rate, Hz (switches "from record" off) | `0.000000E+00` | Eye > Bit rate | OOS extension |
| `:EYE:BER` | Set / Query | Target bit error ratio Range 1e-18 to 1e-1. | `1.000000E-12` | Eye > BER | OOS extension |
| `:EYE:PN:LOW` | Set / Query | Hz. Range 1.0 to 1e12. | `1.000000E+03` | Eye > Pn low | OOS extension |
| `:EYE:PN:HIGH` | Set / Query | Hz. Range 1.0 to 1e12. | `1.000000E+06` | Eye > Pn high | OOS extension |
| `:EYE:NOTCh` | Set / Query | Hz. Range 0.0 to 1e12. | `0.000000E+00` | Eye > Notch | OOS extension |
| `:EYE:PLL:BWIDth` | Set / Query | Hz. Range 0.0 to 1e12. | `0.000000E+00` | Eye > PLL bandwidth | OOS extension |
| `:EYE:NPJ` | Set / Query | Boolean. | `0` | Eye > NPJ | OOS extension |
| `:EYE:HPASs` | Set / Query | Boolean. | `1` | Eye > Highpass | OOS extension |
| `:EYE:PLACe` | Set / Query | `FULL`, `SIDE`, `UNDER` | `FULL` | Eye > Place | OOS extension |

## Eye figures

`:EYE:ITEM? <figure>`: `ONE`, `ZERO`, `AMPLITUDE`, `CROSSING`, `RISE`, `FALL`, `HEIGHT`, `WIDTH`, `Q`, `HBER`, `RECORDS`, `EDGES`, `UI`, `SAMPLING`, `TPP`, `TRMS`, `DCD`, `DDJ`, `PJRMS`, `PRMS`, `C2C`, `F2`, `F4`, `F8`, `SRJ`, `PN`, `TPRE`, `TPOST`, `PPRE`, `PPOST`, `DCCM`, `ACCM`, `XOVER`, `TNT`, `SSCPPM`, `SSCRATE`, `TJ`, `RJ`, `DJ`, `OPENING`, `SEEN`, `TJBER`, `EWBER`, `J2`, `J9`, `NPJ`.

With a clock (strobe) source the jitter figures (TIE, TJ, ...) describe the strobe against an ideal clock fitted to each burst separately, not one clock across the whole record; idle time more than 1.5 UI from a strobe edge is left out. Example for DQ in R2 and DQS in R1, over the whole record, using the packed setup keys:

```text
:SYST:SET eye = R2 0 1
:SYST:SET eye_more = 0.000000000001 1000 1000000 0 - 0 R1 1
:VIEW EYE
:EYE:ITEM? WIDTH
```

## Examples

Eye of a data channel.

```text
:EYE:SOURce CHAN1
:EYE:RATE 1e6
:EYE:BER 1e-12
:EYE:PLACe FULL
:EYE:RATE?                             -> 1.000000E+06
:EYE:BER?                              -> 1.000000E-12
```

Clear the eye.

```text
:EYE:CLEar
:EYE:PLACe?                            -> FULL
```

