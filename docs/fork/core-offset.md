# ADC core offset matching

Status: implemented on `feat/core-offset`, opt-in (`cal.core_offset = tight` in system.conf), host-tested only.

## Problem

The RT8847 interleaves eight cores; each has an offset register (0x14 + 8n, 11-bit signed, one ADC step). The stock
calibration (self-cal.md 14.6) trims every core to mid-scale and stops at +-1.5 steps, so two cores can differ by up
to 3 steps. The FPGA has per-core gain trims only (0x1058, 0x105c) and no per-core offset; every rate path decimates
after them (netlist, private RE notes `docs/fpga.md`).

Which cores feed consecutive samples, one channel (netlist simulation):

| rate | cores |
|---|---|
| 1.25 GSa/s (divider 1) | all 8 |
| 625 MSa/s (divider 2) | 4 |
| 312.5 MSa/s (divider 4), 62.5 MSa/s (slow path, 0x10d8 = 1) | 2, alternating |
| 156.25 MSa/s (divider 8), 31.25 MSa/s (0x10d8 = 2) | 1 |

A core offset difference is a fixed pattern at that rate. It hides under the noise of a single frame but stays in the
hardware average (floor near 0.40 mV at 100 mV/div for N = 16 and 256) and in the DVM block, which sums one lane, a
pair of cores (DC 3.1 mV low at 500 mV/div in round 5).

## What the change does

`hal::dho::coretrim`, called from the acquisition thread right after a configuration, with nothing armed, once per
ADC mode and session:

1. `order`: nudges each core's offset by 32 steps in turn and notes which measure slot moves. The slot-to-core mapping
   of the normal modes is not known otherwise. Every nudge is taken back.
2. `trim`: per acquired channel, its slots (`levels::slots`) are matched to the channel's mean, not to mid-scale.
   Correction = rounded difference in steps; up to 8 rounds; tolerance 0.6 steps (the register has 1-step resolution;
   the mean of 2^18 samples per core is good to about 0.05 step). Codes per step: 16, or 32 where a digital gain of 2
   or more puts the ADC x2 of 0x105c before the measure unit.
3. Channels are left alone when they clip, when their mean is more than 256 steps from mid-scale (core gain
   differences would then move the core means by more than 0.25 step), or when their cores start more than 8 steps
   apart (a slot grouping that mixes channels). The mode is then tried again at a later configuration, at most 5
   times per session.
4. The new offsets go into the in-memory cal_adc payload, so later switches back to the mode apply them, and the
   payload is saved as `/oos/cal/cal_adc.hex` when it changed. OOS loads its own file first at the next start.

What it never does: write any Rigol file (`/rigol/data/cal_*.hex`, RKey, vendor data). Back to the stock offsets:
delete `/oos/cal/cal_adc.hex` (OOS then loads the Rigol file again) and turn the key off.

Not changed: the ADC-cores item of the user self-cal keeps the stock 1.5-step tolerance and target; the runtime match
runs after it anyway.

## Unit measurement (before and after)

Raw words via `:WAV:DATA?` (16-bit), input open (or 50 Ohm), 100 mV/div, offset 0, Normal acquisition, trigger
on the measured channel (OOS also acquires a hidden trigger channel). For each case: 10 single frames, then per
frame the mean of each sample phase (period 2 at 62.5 and 312.5 MSa/s, 4 at 625 MSa/s, 8 at 1.25 GSa/s) minus the
frame mean, in codes; report the spread (max - min of the phase means) averaged over the 10 frames.

| rate | time base, depth | channels |
|---|---|---|
| 1.25 GSa/s | 1 us/div, Auto | CH1 |
| 625 MSa/s | 1 us/div 10k | CH1; CH1 + CH2 |
| 312.5 MSa/s | 2 us/div 10k | CH1; CH1 + CH2; CH1..CH4 |
| 156.25 MSa/s | 5 us/div 10k | CH1 (one core: expect no pattern) |
| 62.5 MSa/s | 10 us/div 10k | CH1; CH1 + CH2; CH1..CH4 |
| 31.25 MSa/s | 20 us/div 10k | CH1 (one core) |

1. Before: key off. The table above, plus HW average 16 noise (point-to-point / sqrt 2) at 62.5 MSa/s, and the DVM
   per-lane check (dvm.md, lanes 0..3 against the software value).
2. Turn on: `cal.core_offset = tight`, restart oos-ui, set each channel set once (each ADC mode used). The log shows
   `core offsets of ADC mode m: [..] -> [..], order [..], x steps left, matched .. skipped ..` per mode. Check that
   `/oos/cal/cal_adc.hex` appeared and `/rigol/data/cal_adc.hex` is unchanged (sha256 before and after).
3. After: the same table. Expect the phase spread at 62.5 and 312.5 MSa/s to drop to about 1 step (16 to 19 codes),
   the HW 16 floor at 100 mV/div towards the SW 16 value, the DVM lane values within about 1 step of each other.
4. A restart with the key on: the log shows no change for modes already matched (offsets equal), so nothing is saved
   again.
