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

## Specification

DHO800 data sheet, DC offset accuracy: +-(0.1 div + 2 mV + 1.5 % of the offset) at 200 mV/div and below,
+-(0.1 div + 2 mV + 1.0 % of the offset) above. DC gain accuracy +-1 % of full scale above 5 mV/div. Offset range
+-1 V up to 65 mV/div, +-8 V up to 270 mV/div, +-20 V up to 2.75 V/div, +-100 V up to 10 V/div.

Round 9 against it: at 1 V/div and +2 V the limit is 100 + 2 + 20 = 122 mV; the reading is -27 mV. Even the
offset-proportional term alone (1.0 %, 20 mV at 2 V) is of the same size as the error. Inside the specification.

Decision rule: close the item when the sweep stays inside the limit at every point and shows no channel or path far
from the others. An OOS lsb trim only for a clean per-path error well outside the limit.

## Sweep (unit round 11, closed)

Build 20261113-220242. CH3 open, Normal, Auto, 500 us/div, `:MEAS:ITEM? VAVG` mean of 5 reads, mV. Limit = the
data sheet bound at the largest offset of the row.

| scale | path | readings at the offsets | slope | limit at max offset | worst |
|---|---|---|---|---|---|
| 200 mV/div | ig1 | 0: -1.4; -0.5 V: +3.6; +0.5 V: -5.5 | -9.0 mV/V | 29.5 | 5.5 |
| 500 mV/div | ig2 | 0: -3.7; -2: +15.1; -1: +3.9; +1: -11.4; +2: -22.8 | -9.5 mV/V | 72 | 22.8 |
| 1 V/div | ig2 | 0: -8.4; -4: +23.5; -2: +10.5; -1: +0.4; +1: -16.9; +2: -26.4; +4: -43.2 | -8.3 mV/V | 142 | 43.2 |
| 2 V/div | ig2 | 0: -4.6; -4: +30.6; -2: +19.7; +2: -22.2; +4: -38.9 | -8.7 mV/V | 242 | 38.9 |
| 5 V/div | ig3 | 0: -25.1; -20: +135.9; -10: +41.3; +10: -99.8; +20: -204.6 | -8.5 mV/V | 702 | 204.6 |

- At 100 mV/div with +-0.5 V and above, and at 20 mV/div with +-0.2 V and above, the open input lands off screen
  and clips. Only their offset-0 points are valid (-0.4 and -0.1 mV). ig0 has no slope measurement.
- CH3: about -9 mV/V on every path measured, slightly steeper for positive offsets. Equal across scales of one
  path and across paths, so it sits in the offset path (DAC and its scaling), not in the record gains.
- CH1 at 1 V/div (probe square, mean about 149 mV): -4 V: 129.8; -2: 137.0; -1: 144.2; 0: 149.4; +1: 149.8;
  +2: 151.5; +4: 154.0. About +3 mV/V, steep on the negative side (-4.9) and shallow on the positive (+1.2). The
  shape is per channel.

Result: every point is inside the data sheet bound, at most about a third of it. Item closed without a code change.
An OOS lsb trim would remove about 1 % of the offset on CH3 but needs a reference input the unit does not have
(an open input is not a voltage reference for the gain), and the stock firmware has the same behaviour.
