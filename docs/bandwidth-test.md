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
| | | | | | | | |
