# Square wave at slow time bases

## Report

CH1 on the 1 kHz probe-compensation square, 100 mV/div, Normal, Min persistence, Auto depth. "Zoom out a bit more
and the signal looks strange".

## Observation (unit, oos-ui 0.1.11, 2026-11-13)

Sweep over SCPI, screenshot (`:DISPlay:DATA?`) plus NORM and RAW waveform data per step. Lit pixels between the two
levels (the edges), counted in the screenshots:

| Time base | Rate (top bar) | Edge pixels | Picture |
|-----------|----------------|-------------|---------|
| 200 us | 31.2 MSa/s | 361 | correct, solid edges |
| 500 us | 12.5 MSa/s | 369 | correct |
| 1 ms | 6.25 MSa/s | 275 | edges dotted, faint |
| 2 ms | 2.5 MSa/s | 53 | no edges, one stray dot per edge, plateaus as two thin zigzag lines |
| 5 ms and slower | 1.25 MSa/s .. 125 kSa/s | 0 | only short dashes on the two levels |
| 10 ms, fixed 1M / 10M | 6.25 / 62.5 MSa/s | 0 | same: depth and rate do not matter |

A correct picture: vertical edges every half period at every time base; from about 5 ms/div (fewer than ~20 columns
per period) a filled band from low to high level. At 50 ms/div there are two periods per column, so every column
should be filled.

The data is fine. The NORM screen record holds min/max pairs, and a column with an edge holds the full span
(e.g. 71/167 at 10 ms/div). Only the drawing loses it.

## Root cause

Display "Dots" is on (`Model::dots`, default `true` upstream, saved in the settings).

1. The newest frame of each step takes the record path (`Source::acquire` -> `board::to_record` -> `draw_ex`); the
   fast path only draws the older queued frames and the deposit workers' batches. At Min persistence a new record
   clears the picture. At slow time bases a batch holds one frame, so the screen is the record path.
2. With two or more samples per record point (`dt / f.dt >= 2`), `to_record` builds peak-detect pairs: point 2j the
   minimum, 2j+1 the maximum of one span. 3600 points, 1800 pairs, about 1.8 per screen column.
3. `Phosphor::draw_ex` with `m.dots` deposited one pixel per point and no segments. A pair became two pixels, one on
   each level. The span between them, which is where the edge is, was never drawn.

At 200 us/div a pair covers 1.1 us and the 3 us edge falls into several pairs whose extremes lie on it, so it looks
right. From 2 ms/div a pair covers 11 us or more and its extremes lie on the plateaus. Persistence does not fill the
edge: the probe square is coherent with the sample clock (RAW at 50 ms/div: the same edge sample value in every
period), so the pairs fall on the same rows in every frame.

Upstream: the `draw_ex` dots branch and the peak-detect pairs of `to_record` are identical in upstream 0.1.12. The bug
predates the fork. The fork's batch pipeline and sparse path neither cause nor fix it.

## Fix (fix/zoomout)

In dots mode a peak-detect pair is not two samples but the range of the samples it stands for. It is drawn as one
vertical span over the columns it covers, from its minimum to its maximum.

- `wave::Record::pairs`, set by `to_record` when it builds the pairs (not in High Res, which averages).
- `Phosphor::draw_ex(.., pairs)`: with `m.dots && pairs` one span per pair. Vectors and plain dots unchanged.
- Callers pass `rec.pairs`: `drawpipe::draw_frame`, `ui.rs` (main and zoom, running and stopped). `sparse::draw` passes
  false (interpolated points).
- Test `board::record::dots::dots_draw_min_max_pairs_as_spans`: a 1 kHz square with 3 us edges at 2 and 50 ms/div,
  every edge lit from low to high level; the old two-dot drawing lights none.

The `dots: true` default stays (upstream's choice; the saved setting overrides it anyway). Display > Dots off was the
workaround.

## SCPI rate and depth

`:ACQuire:SRATe?` and `:ACQuire:MDEPth?` returned `Model::srate_depth`, the stock system's choice for the settings
(25 MSa/s and 500k at 2 ms/div, Auto). The top bar shows the live hardware record (`Source::hw_rate_depth`: 2.5 MSa/s,
100 kpts). Nothing in docs/ or the SCPI comments documents the model's numbers as the intended answer, and the RAW
waveform's x increment follows the hardware. Both queries now answer what the top bar shows
(`Source::rate_depth`, shared by the top bar, the system info page and SCPI); without hardware frames that is still
the model's choice.
