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

## Round 6 (unit, CH3 alone, input open, 100 mV/div)

| rate | cores | Normal | HW 16 |
|---|---|---|---|
| 31.25 / 156.25 MSa/s | one | no even/odd offset | floor 0.12 / 0.14 mV, no offset |
| 62.5 MSa/s | two | even - odd +-11..25 codes, sign changes per frame | -12..-19 codes, fixed sign; floor 0.18-0.23 mV |
| 312.5 MSa/s | two | | +11..+13 codes; floor 0.19 mV |
| 1.25 GSa/s | eight | one phase of eight about +185 codes, the phase moves between frames | two phases 4 apart +67..+87; floor 0.75 mV |

Reading:

- One core is far off. At 1.25 GSa/s the high phase is +185 codes against the frame mean, so +211 codes against the
  other seven. At 100 mV/div the digital gain of CH3 is 1.174 (cal file), so one ADC step is 18.8 codes: the core is
  about **11 steps** high. The other seven sit within about +-1.5 steps of each other, as the stock tolerance
  allows.
- The two cores of the 62.5 and 312.5 MSa/s paths (regmap cores 0 and 1 in the netlist model) differ by 0.7 to 1
  step. The far core is not one of them. It shows only where all eight cores are used.
- The core's register index n is not known: the netlist's core numbers are not mapped to RT8847 registers
  (rt8847.md open item 8). The step procedure below finds it.
- The offsets OOS writes for CH3 alone are the stored mode-2 offsets of the Rigol file (`seq::State::configure`
  writes them on the change to mode 2). In the unit's file (scope_dump 20261113): 32, 66, 33, 73, 53, 85, -32, 62
  steps. The August file had 13, 70, -20, 21, 64, 52, 57, 69, so the cores were recalibrated since. No other
  stored mode written in its place gives one core about 12 steps off with the rest within 1.5 steps; so the stored
  values arrive, and one core has moved by about 12 steps since its calibration, or the measure unit the calibration
  uses sees it differently from the acquisition.
- OOS's own self-cal (the ADC cores item, off by default, never run on this unit) would trim mode 2 to +-1.5 steps if
  the measure unit sees the error. The run-time match of this branch matches the eight cores to their mean. The
  first version skipped a channel whose cores started more than 8 steps apart; with one channel that guard is
  dropped now (it is only for the unverified slot grouping of two and four channels).

Unit step to settle "does the measure unit see it" and find the core (oos-ui stopped, the acquisition left in CH3
alone at 1 us/div, input open):

1. `oos-fpgad adc slots --runs 8`: one slot about +11 steps from the slot average means the measure unit sees the
   error, and both the self-cal and the run-time match can fix it. All slots within 1.5 steps means it does not: the
   data path has an offset the calibration cannot see, and the fix moves to a per-phase correction.
2. With a slot far off: for n = 0..7, `oos-fpgad adc offset n <stored - 16> --execute`, `adc slots`, then
   `adc offset n <stored> --execute`. The slot that drops is core n; the size of the drop gives the measure codes per
   register step (16 if one register step is one ADC step, 8 if it is half a step, see round 9). Stored values: mode
   2 above (CH3 alone). This also gives the core order of mode 2.
3. Start oos-ui with `cal.core_offset = tight`; the log line of mode 2 must show core n's offset lowered by the far
   slot's distance divided by the measured step.

## Round 9 (unit, build 20261113-210854)

- `oos-fpgad adc slots` printed nothing and exited 0: the command dispatch did not route `adc` to the register tool,
  so it fell through to the usage path silently. Fixed: the routed commands are one list (`TOOL_CMDS`) and a test
  checks that every command the tool handles is in it.
- `:SYSTem:DEBug:SLOTs?` (CH3 alone, input open) showed slot 6 at +8.7 to +8.9 steps; the other slots within 1.2
  steps. The measure unit sees the error.
- `cal.core_offset = tight` logged "core order not found" for mode 2 and saved nothing; `/rigol/data/cal_adc.hex` was
  unchanged (sha256 checked). The order search nudged by 32 register steps and expected a move of at least 16 steps.
  The stock firmware's own order search uses a nudge of 1023 and a threshold of 7936 codes, which is about 8 codes
  (half an ADC step) per register step. A 32-step nudge then moves the slot by about 16 steps minus noise, at the
  threshold, so the search failed.
- The open input spans 43 to 61 codes (max - min) at 100 mV/div, so the quietness gate of 64 left almost no margin.

## Round 11 (unit, build 20261113-220242)

- `adc slots --runs 8` and the stepping work. Core to slot: 0-0, 1-2, 2-1, 3-3, 4-4, 5-6, 6-5, 7-7 (the pairs 1/2 and
  5/6 swapped). The far slot 6 is core 5. 16 register steps move a slot by 8 to 9 ADC steps against the slot average
  (core 5: 12.8), so a register step is about half an ADC step.
- `cal.core_offset = tight` found the order, then "did not converge in 8 rounds, 0.77 steps left"; nothing saved,
  cal_adc.hex unchanged.
- Cause: no wait between an offset write and the next measure run. A run that partly sees the old offset makes
  `order` measure too small a register step, and `trim` then over-corrects and oscillates. One run's slot means also
  scatter by about 0.3 step against each other, close to the 0.6 target. Fix: settle waits after writes as the stock
  firmware has, 4 runs per reading, keep the best round within 1 step. The failure log line now includes the order
  (slots and measured codes per register step).

## Round 12 (unit, build 20261113-222353)

- Mode 2 (CH3 alone): order found again (codes per register step 9.0 to 10.6, about 0.62 ADC step). Two tries ended
  "did not converge", best 5.60 and 1.21 steps; under the 1.0 limit of the time nothing was kept, and slot 6 stayed
  at +9. The limit is now the stock 1.5 and the failure line traces the worst slot per round.
- Modes 0 and 5 (start-up and probe-comp states): order not found. In mode 0 a nudge of core 1 moved every slot by
  about -3690 codes, far more than 64 register steps give (cause not known, possibly clipping); in mode 5 the
  moves varied from try to try (likely the probe-comp signal on the input). Both are refused as
  intended and leave the offsets alone.

## Round 13 (unit, build 20261113-224914): works

CH3 alone, input open, 100 mV/div, 1.25 GSa/s, trigger CH3.

- Mode 2: [32, 66, 33, 73, 53, 85, -32, 62] -> [25, 66, 38, 77, 57, 79, -31, 59], 0.39 steps left, saved to
  `/oos/cal/core_offsets.hex`. At the next start the kept values were applied and matched again (0.46 left, no
  change). `/rigol/data/cal_adc.hex` unchanged (sha256).
- Slot means: slot 6 from +9 steps to all slots within -0.42..+0.55.
- Phase spread at 1.25 GSa/s (10 frames): 230 codes (207..261) -> 21 codes (9..32). Noise p2p 1.36 -> about 1.0 mV.
- DVM lanes 0..3 at 500 mV/div: -4.75 / -0.81 / +8.22 / -6.88 -> -4.43 / -4.50 / -4.92 / -3.12 mV (software -5.4).
  At 100 mV/div: -0.68 / -0.01 / +2.12 / -0.92 -> -0.57 / -0.61 / -0.65 / -0.32 mV (software -0.7).
- A first try during the start-up mode switches did not converge: the worst slot cycled 6, 7, 5 every round, which
  fits a channel set that changed under the trim. It kept nothing and the next configuration succeeded.
- Modes 0 and 5 (CH1 with the probe-comp square) refuse the order every start: the signal moves the slots between
  runs. Harmless. Since then the gates run before the order search: such a mode is skipped without a nudge or a
  log line and tried again at a later configuration.

`cal.core_offset = tight` stays opt-in.

## What the change does

`hal::dho::coretrim`, called from the acquisition thread right after a configuration, with nothing armed, once per
ADC mode and session:

1. `order`: nudges each core's offset by 64 register steps in turn and notes which measure slot moves. The
   slot-to-core mapping of the normal modes is not known otherwise. The nudged slot must move by at least 64 codes
   and every other slot by at most a quarter of that, or the search fails with the deltas in the log. The move
   divided by 64 is that core's measure codes per register step; `trim` uses it instead of an assumed size. Every
   nudge is taken back.
2. `trim`: per acquired channel, its slots (`levels::slots`) are matched to the channel's mean, not to mid-scale.
   Correction = difference divided by the measured codes per register step, rounded; up to 8 rounds; target 0.6
   steps. Each reading averages 4 measure runs, and every round waits 50 ms after its writes (`order` waits 5 ms
   after each nudge), as the stock calibration does. When noise keeps one core above 0.6 in every round, the best
   round is kept if its worst core is within 1.5 steps, the stock calibration's own tolerance. Up to 12 rounds. Codes per ADC step for the tolerance: 16, or
   32 where a digital gain of 2 or more puts the ADC x2 of 0x105c before the measure unit.
3. Quietness gate: a channel is matched only when every slot's samples stay within 128 steps (max - min) and every
   slot's mean moves by at most 1 step over three runs 20 ms apart (one run's slot means scatter by about 0.3 step). Matching to the channel mean assumes each core
   sees the same signal on average; a signal near a multiple of fs/8 would otherwise be trimmed into the offsets.
   One that is not locked to the ADC clock moves the core means from run to run and is refused; one that is locked
   (the built-in generator) cannot be told from an offset, so nothing is trimmed while the generator runs. Inputs open
   or terminated is still the safe state, as for the stock calibration.
4. Channels are left alone when they clip, when their mean is more than 256 steps from mid-scale (core gain
   differences would then move the core means by more than 0.25 step), or, with two or more channels, when their
   cores start more than 8 steps apart (a slot grouping that mixes channels). The mode is then tried again at a later configuration, at most 5
   times per session.
5. The new offsets go into the in-memory cal_adc payload, so later switches back to the mode apply them, and into
   `/oos/cal/core_offsets.hex` (the trimmed modes only, a `calfile` container). That file is read only while the key
   is on and is laid over the cal_adc.hex in use (Rigol's, or the self-cal's `/oos/cal/cal_adc.hex`). At the first
   configuration of a mode the kept values are written to the ADC before the match starts.

What it never does: write or modify any Rigol file (`/rigol/data/cal_*.hex`, RKey, vendor data) or cal_adc.hex. The
Rigol files are only read (`calfile::load_first`, `std::fs::read`); `calfile::save` refuses any path under /rigol.
Undo: turn the key off. The overlay is then not read, and the offsets are those of the cal_adc.hex as before.

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
   `/oos/cal/core_offsets.hex` appeared and `/rigol/data/cal_adc.hex` is unchanged (sha256 before and after).
   With the probe-comp signal on the channel the log must say skipped (quietness gate).
3. After: the same table. Expect the phase spread at 62.5 and 312.5 MSa/s to drop to about 1 step (16 to 19 codes),
   the HW 16 floor at 100 mV/div towards the SW 16 value, the DVM lane values within about 1 step of each other.
4. A restart with the key on: the log shows no change for modes already matched (offsets equal), so nothing is saved
   again.
