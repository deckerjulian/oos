# Bandwidth test of the front end (procedure)

Goal: the real analog bandwidth of the DHO924S under OOS with the AFE filter open ("Full"). The rated bandwidth is
250 MHz. The ADC (1.25 GSa/s, Nyquist 625 MHz) and the front end set the real limit. Tools in OOS:
Analysers > Spectrum (average trace), Analysers > Bandwidth (step response, `ui/src/ui/bwtest.rs`,
`oos_dsp::bandwidth`), Analysers > Bode (external generator over LAN).

## Common set-up

- Only CH1 on (1.25 GSa/s). Bandwidth limit **Full**, DC coupling, a V/div that fills about 6 divisions.
- Memory depth at least 1 M (better 10 M): the longer the record, the finer the folding and the lower the noise.
- Note V/div for every result. The bandwidth can depend on the range (different attenuator and gain paths).
  Measure at least 10 mV, 100 mV and 1 V/div.

## Test A: noise spectrum (no extra equipment)

1. CH1 open or with a 50 Ohm terminator, smallest V/div that does not clip.
2. Spectrum: span 0..625 MHz, RBW about 1 MHz, trace **Average**, let it settle for 100 records.
3. Read where the noise floor falls off. Repeat with filter 250 MHz and 100 MHz. Those curves have to fall off
   earlier, which shows the filter works and the "Full" curve is the front end.

Limits: this shows only the stages after the noise source (AFE amplifier, ADC driver), not the input divider and
relays. It is a first figure, not the bandwidth.

## Test B: step response (the real measurement)

Needs a source of fast edges with a stable period:
- a pulse generator with a rise time well below 300 ps (e.g. Leo Bodnar, about 30-100 ps, about 10 MHz), or
- a fast logic output (74LVC1G, ~1 ns: then only good for "more than ~250 MHz")

Connection: as short as possible. The DHO900 only has 1 MOhm inputs, so put a 50 Ohm feed-through terminator at the
BNC. Without it, reflections on the cable look like overshoot.

1. Square wave on CH1, filter Full, time base so a few periods show, depth 1 M or more.
2. Analysers > Bandwidth: Source CH1, View On. Signal freq **Auto**: the fundamental is below 625 MHz.
   Bins/period: choose so that "picture step" is at most a tenth of the expected rise time (10 MHz square wave,
   2000 bins: 50 ps).
3. Read: -3 dB, rise 10-90 %, overshoot. The edge picture must be clean. If it is smeared, the period was not found
   exactly or the generator jitters against the ADC clock.
4. Repeat for 10 mV / 100 mV / 1 V/div and for CH2..CH4.

Correction for the generator: rise times add roughly in quadrature,
t_scope = sqrt(t_measured^2 - t_generator^2). With 1.25 GSa/s the folding has to be exact: the period comes from
the record's strongest tone, refined to about 0.05 Hz in 0.8 ms (host test). Because the generator is asynchronous
to the ADC clock, the samples land at random phases. That is what makes equivalent time work.

What the result means:
- f3db > 300 MHz: the 250 MHz rating is a licence limit, not a hardware limit.
- Single pole: rise about 0.35/f3db, no overshoot. Steeper filter: 0.4-0.45/f3db, a few % overshoot.

## Test C: sine sweep (cross-check, needs an RF generator)

Analysers > Bode with an external generator (SCPI over LAN), output = input = CH1. Sweep 1 MHz..600 MHz, constant
amplitude at the generator, 50 Ohm terminator at the BNC. Above Nyquist (625 MHz with one channel) the sine aliases.
For a single frequency, Analysers > Bandwidth with **Signal freq** = the generator frequency still shows the folded
sine and its amplitude.

## Test D: generator above the 60 MHz cap (needs nothing extra)

`/oos/limits.conf`: `afg.sine_max_hz = 75e6` (marked experimental). Cable GI/GO -> CH1, 50 Ohm terminator.
Sine at 20 / 40 / 50 / 60 / 65 / 70 / 75 MHz. Spectrum: level of the fundamental and of the image at
156.25 MHz - f. Expected without a reconstruction filter: hold droop -0.4 dB (25 MHz) .. -2.2 dB (60 MHz) .. -3.6 dB
(75 MHz), image -14 dB (25 MHz) .. -4 dB (60 MHz) below the fundamental. A strong analog filter shows as a lower image.

## Results

| Date | Channel | V/div | Test | -3 dB | Rise 10-90 | Overshoot | Note |
|---|---|---|---|---|---|---|---|
| 2026-11-21 | CH1 | 0.5 mV | A | about 310 MHz (Full) | | | first figure, see below |

### Test A, 2026-11-21 (DHO924S, CH1 open, 0.5 mV/div, 1.25 GSa/s)

Verified on the unit, as a first figure. Six records of 32768 raw samples per setting (`:WAV:MODE RAW`, Run then
`:STOP;*OPC?`), Welch spectrum (1024-point Hann segments, half overlap), each bandwidth limit divided by Full at the
same frequency. Behind the filter the ADC and its driver add their own noise, about 20 dB under the front end's at
0.5 mV/div (8 dB at 2 mV/div); that floor (the level every limit reaches at 500 to 600 MHz) is taken off both before
the ratio. Full itself is read against its own level from 50 to 150 MHz.

| limit (filter code c) | -3 dB | at 156.25 MHz | at 312.5 MHz |
|---|---|---|---|
| 20 MHz (5) | (33 MHz here; 23.7 MHz with the sine below) | -13 dB | -17 dB |
| 50 MHz (10) | about 62 MHz | -8 dB | -14 dB |
| 75 MHz (15) | about 93 MHz | -6 dB | -11 dB |
| 100 MHz (20) | about 111 MHz | -5 dB | -10 dB |
| 150 MHz (30) | about 147 MHz | -3 dB | -7 dB |
| 200 MHz (40) | about 161 MHz | -3 dB | -7 dB |
| 300 MHz (60) | about 219 MHz | -2 dB | -5 dB |
| Full | about 310 MHz (-2 dB at 300, -6 dB at 325 MHz) | | -21 dB at 600 MHz |

- The limits fall about as one pole (the 20 MHz limit loses 10 dB from its corner to 156 MHz, a single pole 11 dB).
  Up to code 20 the corners lie 10 to 30 % above 5c MHz; from code 40 the front end's own roll-off takes over.
- For Auto (afe-settings.md 5.1): with four channels (312.5 MSa/s, code 15) the corner is about 93 MHz, fs / 3.4,
  and the filter takes only about 6 dB off at Nyquist (156 MHz); with two (625 MSa/s, code 31) about 7 dB at 312.5 MHz.
  A signal just above Nyquist folds back at about half its amplitude (0.5 at -6 dB, 0.45 at -7 dB).
- Limits of the method: it sees only the stages after the noise source (not the input divider and relays), and below
  about 15 MHz the noise is not flat, so the low corners are the least sure: the 20 MHz limit read 33 MHz here and
  23.7 MHz with the sine below.

### Sine through each limit, 2026-11-21 (DHO924S, CH1, built-in generator)

Verified on the unit. Sine of 1.6 Vpp at 0.25 V/div, 1 to 60 MHz (the generator's limit), one channel at 1.25 GSa/s;
per frequency and limit two records (Run, then `:STOP;*OPC?`), the amplitude by a sine fit at the known frequency on
16384 raw samples, divided by the amplitude with Full at the same frequency, so the generator's own response falls out.

| limit (code c) | nominal 5c MHz | -3 dB | at 60 MHz |
|---|---|---|---|
| 5 MHz (1) | 5 | 3.9 MHz | -20.4 dB |
| 10 MHz (2) | 10 | 9.0 MHz | -13.9 dB |
| 15 MHz (3) | 15 | 13.9 MHz | -10.6 dB |
| 20 MHz and 25 MHz (5) | 25 | 23.7 MHz | -6.8 dB |
| 30 MHz (6) | 30 | 36.1 MHz | -5.5 dB |
| 35 MHz (7) | 35 | 42.8 MHz | -4.6 dB |
| 40 MHz (8) | 40 | 49.3 MHz | -3.9 dB |
| 50 MHz (10) | 50 | about 62 MHz | -2.9 dB |
| 60 / 75 MHz (12 / 15) | | above 60 MHz | -2.2 / -1.6 dB |

- The 20 MHz limit is code 5, the same filter as 25 MHz (afe-settings.md 5.1): both read the same to 0.01 dB.
- Up to code 5 the corner lies a little below 5c MHz (0.78 to 0.95 of it), from code 6 about 1.2 times above; the
  step between code 5 and 6 is larger than one code. The noise figures above (code 15 about 93 MHz, 1.24 times)
  continue the second group.
- Far above the corner the limits fall less steeply than one pole (code 1: -20.4 dB at 15 times its corner, where one pole gives
  -23.8 dB), as if a small part of the signal went around the filter.
