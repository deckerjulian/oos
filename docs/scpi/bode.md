# Bode and bandwidth test

The frequency-response sweep (loop gain, PSRR, impedance) driven through an external generator, and the step-response bandwidth estimate. `:BODE:RUN` can control the configured generator and the acquisition. Estimator limits apply to the bandwidth figures.

Menu location: Bode (view, Mode, Start / stop frequency, Points, Amplitude, Input, Output, Free sweep, Start / stop) and Bandwidth test (View, Source, Signal frequency, Bins per period).

Part of the [SCPI reference](README.md).

## Commands

### Bode

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:BODE:STARt` | Set / Query | Hz. Range 1e-3 to 1e9. | `1.000000E+02` | Bode > Start freq | OOS extension |
| `:BODE:STOP` | Set / Query | Hz. Range 1e-3 to 1e9. | `1.000000E+06` | Bode > Stop freq | OOS extension |
| `:BODE:POINts` | Set / Query | Points, 5 to 200. Range 5 to 200. | `40` | Bode > Points | OOS extension |
| `:BODE:AMPLitude` | Set / Query | Volts. Range 0.01 to 20.0. | `1.000000E+00` | Bode > Amplitude | OOS extension |
| `:BODE:INPut` | Set / Query | Source; the input is `OFF`-able on the free sweep. | `CH1` | Bode > Input (ref) | OOS extension |
| `:BODE:OUTPut` | Set / Query | Source; the input is `OFF`-able on the free sweep. | `CH2` | Bode > Output | OOS extension |
| `:BODE:SWEep:TIME` | Set / Query | Seconds. Range 1e-3 to 500.0. | `1.000000E+00` | Bode > Free sweep: time | OOS extension |
| `:BODE:SWEep:LOG` | Set / Query | `LIN`, `LOG`. | `LOG` | Bode > log | OOS extension |
| `:BODE:SYNC:SOURce` | Set / Query | Channel `CH1`.. | `CH4` | Bode > Sync | OOS extension |
| `:BODE:SYNC:EDGE` | Set / Query | `RISING`, `FALLING`. | `RISING` | Bode > Sync | OOS extension |
| `:BODE:SYNC:LEVel` | Set / Query | Volts. Range -1e3 to 1e3. | `1.000000E+00` | Bode > Sync | OOS extension |
| `:BODE:REFLevel` | Set / Query | DBV Range -300.0 to 300.0. | `0.000000E+00` | Bode > Ref level | OOS extension |
| `:BODE:RUN` | Set / Query | Start / stop the sweep; the query answers whether it runs | `0` | Bode > Start / Stop | OOS extension |
| `:BODE:MODE` | Set / Query | `LOOP\|PSRR\|IMPedance`; the query answers `LOOP`, `PSRR` or `IMP`. | `LOOP` | Bode > Mode (loop / PSRR / impedance) | OOS extension |
| `:BODE:DATA` | Query | The sweep result as text. | - | Bode > Read sweep data | OOS extension |
| `:BODE:GENerator` | Set / Query | `"ip:port"` Generator address, one word without blanks, or `usb`. | `192.168.1.100:5555` | Bode > Generator address and connection (LAN / USB) | OOS extension |

### Bandwidth test

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:BANDwidth:STATe` | Set / Query | The bandwidth test view (also `:VIEW BWTest`) | `0` | Bandwidth test > View | OOS extension |
| `:BANDwidth:SOURce` | Set / Query | Channel. | `CHAN1` | Bandwidth test > Source | OOS extension |
| `:BANDwidth:FREQuency` | Set / Query | Hz; 0 is auto, else the nearest hint of the menu. | `0.000000E+00` | Bandwidth test > Signal freq | OOS extension |
| `:BANDwidth:BINS` | Set / Query | Bins per period (nearest step). | `2000` | Bandwidth test > Bins / period | OOS extension |
| `:BANDwidth:ITEM` | Query | `<item>[,source]` `RISE`, `BW`, `OVER`. Estimator limits apply. | - | Bandwidth test > Read rise / BW / overshoot estimate | OOS extension |

## Examples

Sweep range and mode.

```text
:BODE:MODE LOOP
:BODE:STARt 100
:BODE:STOP 1e6
:BODE:POINts 50
:BODE:AMPLitude 0.1
:BODE:STARt?                           -> 1.000000E+02
:BODE:MODE?                            -> LOOP
```

Bandwidth step test.

```text
:BANDwidth:SOURce CHAN1
:BANDwidth:FREQuency 0
:BANDwidth:BINS 16
:BANDwidth:STATe ON
:BANDwidth:SOURce?                     -> CHAN1
:VIEW?                                 -> BWTEST
```

