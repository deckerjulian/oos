# oos-dsp: signal processing of OOS

Rust, no dependencies, platform independent. Used by `oos-ui` (demo and replay sources, measurements, math channel)
and meant for any software-side trigger/measurement in OOS.

| module | content |
|---|---|
| `edges` | comparator with hysteresis -> edges with interpolated crossing times |
| `trigger` | edge, pulse width, slope (transition time), video (PAL/NTSC: lines, line n, odd/even/all fields), pattern (H/L/X/rise/fall per channel), duration, timeout, runt, window (enter/exit/inside), delay (A -> B), setup/hold, Nth edge after idle; time conditions >, <, inside, outside |
| `measure` | 35 measurements: max, min, pk-pk, top, base, amplitude, upper/middle/lower, mean, RMS, cycle mean/RMS, AC RMS, std dev, overshoot, preshoot, area, cycle area, period, frequency, rise, fall, +/- width, +/- duty, +/- pulse and edge counts, t(max), t(min), +/- slew rate; delay and phase between channels |
| `decode` | UART (5-9 data bits, parity none/even/odd, inverted), I2C (start, repeated start, address + R/W, data, ACK/NACK, stop), SPI (modes 0-3, MSB/LSB first, MOSI + MISO, frames by CS or clock pause), CAN 2.0A/B (hard sync on SOF, resync on edges, de-stuffing, CRC-15, ACK; RX/TX level or CAN_H), LIN (break, sync, protected ID with parity, data, classic/enhanced checksum), I2S (SD on rising SCK, one-clock delay after WS, left/right, MSB-aligned words), MIL-STD-1553 (Manchester II 1 Mbit/s, command/status and data sync, 16 bits, odd parity); `decode_ext3` also NMRA DCC (rail half-bit timing, preamble, address/data, XOR) |
| `mask` | mask (pass/fail) test: envelope around a reference waveform (sliding min/max over +-tol_x samples, +-tol_y volts, O(n) monotonic deque), test of a record against it (records of another length mapped proportionally, NaN skipped; verdict with violation count and first violating sample), band of a column range for drawing |
| `interp` | linear and Lanczos-windowed sinc interpolation (exact and table kernel) for sparse screen views |
| `math` | A+B, A-B, AxB, A/B, integral, derivative, abs, sqrt, lg, ln, exp, 1st-order low/high-pass, FFT with Rect/Hann/Hamming/Blackman/Flattop (amplitude-correct), dBV, peak search |
| `sim` | generators: pulse trains with glitches/runts/missing pulses, composite video PAL/NTSC with field-distinguishing vertical interval, UART / I2C / SPI bus waveforms, sampling, noise |

Conventions: waveforms are `&[f32]` volts every `dt` seconds; trigger points are fractional sample indices at the moment
the condition is decided (closing edge of a pulse, end of a transition, expiry of a timeout, ...); measurements follow
Rigol / IEEE 181 (top/base from the histogram, 10/50/90 % levels).

```
cargo test     # 33 tests: every trigger type, measurement, decoder and math function against signals with known answers;
               # mask: the same signal passes (other noise, trigger jitter), a runt and a glitch fail, envelope/mapping
```
