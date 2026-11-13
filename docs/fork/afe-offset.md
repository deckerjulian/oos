# AFE offset accuracy

## Problem

Round 9 (CH3 open, 1 V/div, `:MEAS:ITEM? VAVG,CHAN3`, same in every channel set and trigger source):

| offset | 0 V | +1 V | +2 V | -2 V |
|---|---|---|---|---|
| reading | -1 mV | -17 mV | -27 mV | +7 mV |

About -13 mV per volt on the positive side, about -4 mV per volt on the negative side. Not linear in the offset.

## What is known

- The offset DAC code is `trunc(-(sign * offset + position) / lsb + zero)`: `lsb` from cal_lsb per channel and input
  gain path (record field g[0]), `zero` from the cal_vertical record of the scale. The stock firmware computes the
  same expression from the same fields (checked against its decompiled offset setter). The record block it picks by
  bandwidth limit matches OOS's `block_group` on this unit (hardware code 0x0c selects the fourth block at full
  bandwidth).
- So OOS reproduces the stock offset. The residual is a property of the unit and its factory cal, and the stock
  firmware most likely shows it too (not measured).
- CH3 lsb per path (V per DAC code): ig0 6.095e-5 (10 mV..50 mV/div), ig1 -3.7037e-4 (100, 200 mV/div), ig2
  -1.6996e-3 (500 mV..2 V/div), ig3 -1.6524e-2 (5, 10 V/div). The ig1 value is exactly -1/2700, which looks like
  a default rather than a measured value; CH1, CH2 and CH4 have measured ig1 values within 0.5 % of it.
- Two error sources give the same picture at one scale: the DAC lsb (offset path) and the record gain (signal path).
  The reading is `gain_err * DAC_actual - offset`. They separate across scales: scales that share an input gain
  path share the lsb but not the gain.

## Measurement (unitlab)

CH3, input terminated with 50 Ohm (open if no terminator), Normal acquisition, Auto trigger, VAVG over the screen
averaged over 5 reads. For every scale: offset 0 first, then the listed offsets. Skip an offset the UI clamps and
note the clamped value.

| path | scale | offsets |
|---|---|---|
| ig2 | 500 mV/div | -2, -1, +1, +2 V |
| ig2 | 1 V/div | -4, -2, -1, +1, +2, +4 V |
| ig2 | 2 V/div | -4, -2, +2, +4 V |
| ig1 | 100 mV/div | -1, -0.5, +0.5, +1 V |
| ig1 | 200 mV/div | -1, -0.5, +0.5, +1 V |
| ig0 | 20 mV/div | -0.5, -0.2, +0.2, +0.5 V |
| ig3 | 5 V/div | -20, -10, +10, +20 V |

Plus CH1 at 1 V/div with the same offsets, to see whether the shape is CH3's own.

Reading:

- Error per volt equal across the scales of one path: DAC lsb (or DAC nonlinearity). Fix candidate: an OOS-owned
  lsb trim per channel and path, measured with the input terminated (an overlay like core_offsets.hex; Rigol files
  stay read-only).
- Error per volt scaling with the record gain: signal-path gain error. That is a vertical gain item, not offset.
- Different slope for + and -: DAC or offset-path nonlinearity. A two-slope trim per path, or accept it if it is
  inside the stock specification.
