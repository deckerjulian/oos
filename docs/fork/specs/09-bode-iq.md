# Bode I/Q unit (0x151c..0x152c)

Status: specification from static analysis. Short form. Blocked on this unit: the generator section is not fitted
until hardware mod C, and the unit very likely demodulates against the internal generator.

## 1. What the stock firmware does

| register | bits | content |
|---|---|---|
| 0x151c | 3:0 | input channel (0-based) |
| 0x151c | 7:4 | output channel (0-based) |
| 0x151c | 8 | enable |
| 0x151c | 9 | restart pulse (write 1, then 0) |
| 0x1520 | | decimation count = integration length ("DeciNum" from the Bode application) |
| 0x1524 | 0 | read latch: write 1, then 0 |
| 0x1528 | 27:16, 11:0 | input channel I and Q, signed 12-bit |
| 0x152c | 27:16, 11:0 | output channel I and Q, signed 12-bit |

`DrvBode_Read` per frequency point: write the config with the restart bit set, then cleared. Then 99 times: latch, read
both words, gain = |out| / |in| (magnitude from I and Q), phase from atan2 of the two vectors in degrees, restart
pulse, sleep 1 ms. Gain and phase are the mean of the 99 readings. About 0.1 s per point plus generator settling.

The library has no local oscillator setting for this unit. The reference phase therefore comes from elsewhere in the
FPGA, most likely the generator's DDS. Then the unit only works with the internal generator as stimulus.

## 2. What OOS does today

- `dsp/src/bode.rs:9` `sine_fit` and `bode.rs:58` `gain_phase` on transferred records, with an external generator over
  LAN or the internal generator. Works on any unit with a generator.

## 3. Proposed change

Defer until the generator is fitted (mod C). Then:

- `hal/src/dho/bode.rs`: `config(in, out, en, deci)`, `restart`, `read(bar) -> ((i, q), (i, q))`.
- Bode analyser: hardware path when the stimulus is the internal generator, software path otherwise.
- Gain: no record transfer per point; faster sweeps at low frequencies.

## 4. Host test plan

- Write-sequence test for config and restart.
- Sign extension of the 12-bit fields; gain and phase from synthetic I/Q pairs, including zero input.

## 5. Hardware test checklist (after mod C)

1. Generator to CH1 and CH2 through a known RC: compare hardware and software Bode.
2. Dependence of the I/Q values on the generator frequency setting (confirms the DDS reference).
3. Decimation count vs noise of the readings.
4. External generator as stimulus: does the unit still give stable phase? Expected no.

## 6. Confidence and open questions

Register fields high. Reference source of the demodulator unknown; usability on this unit currently nil.
