# Trigger lock-in (2026-11-13)

User report: a square wave took long to capture, and a noisy 5 V signal hardly locked in, where the stock firmware
locks in fast. This covers the hardware trigger set-up, autoset, the software edge refinement, and where the time
goes. Code at `dho804` 1a4a8ec. Measurements on the unit with the 1 kHz, 300 mVpp probe-comp square on CH1, over SCPI.
Settings were restored after each test.

## Summary

The hardware trigger is not the problem. OOS writes the same TPU register image as the stock system for the same
settings, and on a clean signal it triggers within 0.24 s of a level change with every frame on the edge. The
slowness and the failed lock-in come from autoset and from the software trigger:

1. Autoset takes 2.5 to 5.4 s. Most of it is two stages at 20 ms/div that run on the auto-trigger timeout.
2. Autoset without a clean period restores the whole old set-up. A noisy or irregular 5 V signal then gets no
   scale, offset or level at all.
3. The software comparator uses a narrower, centred band than the hardware. When the hardware does not trigger,
   it still finds "edges" in noise, re-aligns the forced frames to them and reports Trig'd.

## Hardware trigger: OOS against stock

| Item | OOS | Stock | Source |
|---|---|---|---|
| Level code | trunc(v x 7500 / scale / g + 32768), clamped 1..0xfffe | same | `hal/src/dho/tpu.rs` `code`, trigger.md 2.3 |
| Two thresholds | A = code(v) / code(v-h), B = code(v+h) / code(v); rising arms below v-h, fires at v | same | `tpu.rs` `level` |
| Hysteresis | 0.3 div, noise reject 0.7 div; table above 1 div at <= 2 mV/div; scale at the BNC | same | `tpu.rs` `noise_hysteresis`, `seq.rs:292` |
| Coupling | 0x208c code + 0x205c bit12, offset left out for AC and LF reject | same | `tpu.rs` `edge_coupled`, trigger.md 2.5 |
| Holdoff | 8 ns default, 6.4 ns ticks | same | trigger.md 2.6 |
| Auto timeout | t1 = clamp(20 x t/div, 10 ms, 500 ms), t2 = max(holdoff, clamp(10 x t/div, 100 ms, 250 ms)) | same | `hal/src/dho/scu.rs` `auto_trigger` |
| Bandwidth | Auto by default (AFE filter follows the sample rate) | same default | `model.rs` `BW_AUTO` |

Hysteresis is already proportional to V/div, noise reject exists, HF/LF reject exist. None of these needs to change
to match stock.

Measured, level moved off the signal for 1 s and back, NORMAL frames counted by `:SYSTem:STATe?` `edges`
(at pre / other direction / further / none):

| t/div | BW | Trig'd after | Frames in 3 s |
|---|---|---|---|
| 5 us | Auto | 0.23 s | 1605 / 0 / 0 / 0 |
| 5 us | Full | 0.24 s | 1604 / 0 / 0 / 0 |
| 500 us | Auto | 0.24 s | 140 / 0 / 0 / 0 |
| 500 us | Full | 0.24 s | 142 / 0 / 0 / 0 |

The 0.24 s includes the SCPI round trips.

## Autoset

`ui/src/ui/autoset.rs`. Stages: Wide (1 V/div, 20 ms/div, Peak), Fit (same time base, new scale), then Search over
20 ns, 2 us, 200 us, 2 ms, 20 ms/div until `wave::periodic` finds a period with 70 % of the height. Each stage waits
for the first record of its time base, then 3 more records and at least 0.3 s, or gives up after 3 s.

Measured, 3 runs each (`:AUToscale`, poll `:AUToscale?`):

| Run | Total | 20 ms/div stages | Search |
|---|---|---|---|
| 1 | 5.35 s | 3.7 s | 1.6 s |
| 2 | 2.5 s | 1.2 s | 1.2 s |
| 3 | 4.9 s | 3.7 s | 1.2 s |

The result also varies: 50 mV/div in one run, 100 mV/div in the next, for the same signal.

### Why the 20 ms/div stages are slow

Wide sets 1 V/div and offset 0 but keeps the old trigger level. The rising comparator arms only below level - 0.3 div,
here 0.148 - 0.3 = -0.15 V, and the signal never goes there. So every record waits the auto timeout t1 = 400 ms.
Records per second at 20 ms/div, Peak:

| Set-up | Records/s |
|---|---|
| 1 V/div, level 0.148 V (Wide as it runs) | 1.71 |
| 1 V/div, level 0.6 V (above the signal) | 1.93 |
| 0.1 V/div, level 0.15 V (triggers) | 4.35 |

Four to five records at 0.5 s each hit the 3 s give-up. Fit also keeps the old level until it ends. The level is
only set at the end of Fit. For a 5 V signal at 1 V/div with a level near either rail, or outside 0..5 V, both stages
run on the timeout: about 6 s before Search starts.

### Why a noisy signal gets nothing

`auto_end` restores the full old model when no channel has a period. `periodic` needs 3 rises with gaps within
1.3x of each other, on a threshold of mid +- 20 % of min..max. Glitches, ringing or an irregular signal fail that at
every time base. The user then waits up to about 8 s and the screen goes back to how it was. Whether stock keeps
the scale and level for a signal it cannot time is not verified.

Rigol's autoset is in the app, not in libscope, and is not reverse engineered. The comparison is by behaviour only.

## Software trigger refinement

`drawpipe::draw_frame` re-aligns every frame to the edge of the set slope nearest the hardware point `pre`, up to half
a screen away, on the signal smoothed to one column (`fastdraw::edge_smooth`). The fork's `trignear::edge_smooth_near`
only narrows the search first. `near_first_equals_the_wide_search` checks it returns the same edge on a noisy corpus,
so the fork change cannot make lock-in worse.

The comparator itself differs from the hardware:

| | Hardware (rising) | Software (`oos_dsp::edges`) |
|---|---|---|
| Arm | below level - h | below level - h/2 |
| Fire | crossing level | above level + h/2, time = last level crossing |
| h | 0.3 div (0.7 noise reject) | same h, centred |

Consequence, measured at 1 V/div, level 0.148 V: the hardware never triggers (records at the timeout rate), yet
`:TRIGger:STATus?` says TD. The status comes from the software result. With a level close to a rail on a noisy
signal, the software finds noise crossings that the hardware ignores, places each forced frame on a different one,
and the screen shows a moving picture labelled Trig'd. The user sees "cannot lock in" with no hint that the level is
wrong.

## Where the time goes, connect to stable picture

- Manual, level already inside the signal: under 0.25 s (hardware arm + one frame).
- Manual, level not inside the signal (the default 0 V on a 0..5 V signal is on the low rail): never. Auto mode
  free-runs on t1 and the software alignment hides it. The stock hardware does the same for the same level; its
  status display in that case is not checked.
- Level 50 % (level knob push, `Ui::level_50`): one record later. It uses min/max, so a spike moves the level.
- AUTO key: 2.5 to 6 s, or about 8 s ending in a restore.

## Fixes, by effect

The user's "locking in" means autoset (team-lead, 2026-11-13). Autoset fixes 1, 2, 3 and 6 come first. The user saw
about 5 s on the probe-comp output, which matches the measured 2.5 to 5.4 s. The probe-comp output on this unit
measures 314 mVpp at 1 kHz with a 1x probe setting, not 3 V.

| # | Fix | Effect | Effort |
|---|---|---|---|
| 1 | Autoset: set the trigger level to 0 V (Wide) and to the Wide mid-level before Fit; or pulse Force (0x205c bit5) per record in Wide and Fit | 20 ms/div stages 3.7 s -> about 1.2 s | small |
| 2 | Autoset: merge Wide and Fit when Wide does not clip (12-bit ADC resolves 0.4 div), settle on 1 record after the first fitting one | another 0.5 to 1 s | small |
| 3 | Autoset: without a period keep scale, offset and 50 % level (top/base) for channels with a signal, time base 1 ms/div, toast "no period" | noisy 5 V gets a usable picture instead of a restore | small |
| 4 | Software comparator as the hardware: rising arms below level - h and fires at level; falling mirrored. Update `near_only` band and its tests | no false Trig'd, no random re-alignment of forced frames | medium |
| 5 | Status from the hardware: 0x4034 bit6/bit7 (triggered/auto) of the last acquisition, not the software find | Auto shows when the level is wrong | small to medium |
| 6 | Autoset Search: read the FPGA counter (`hal/src/dho/counter.rs`, gate 20 to 50 ms) at the new level and pick the time base from the frequency; keep `periodic` as a check | Search 1.2 to 2 s -> about 0.2 s | medium |
| 7 | `level_50`: use `Stats.top`/`base` where the histogram is valid, min/max otherwise | level stays mid-swing on spiky signals | trivial |
| 8 | Optional: autoset turns on noise reject when the record noise (std on the flat parts) exceeds 0.3 div | fewer double triggers on ringing | small |

Not needed: Rigol-style hysteresis scaling (already identical), a different auto timeout (already identical), new
HF/LF reject defaults (stock defaults to DC as OOS does).

## Flicker after autoset

The user sees the trace flicker once autoset has settled. Measured at 500 us/div, 100 mV/div, Auto sweep, batch
pipeline, persistence Min (the default, `Model::persist` = 0; not set in system.conf):

- Trigger alignment is stable. `edges` over 2 s: 107 / 0 / 0 / 0 (all frames on the edge at `pre`). Successive
  screenshots show the edges at the same columns. No untriggered frames in steady state. The 599 "further" and 316
  "none" counts in the totals date from the autoset stages.
- Trace brightness is not stable. Bright trace pixels (R, G > 120) per screenshot over 12 + 20 captures: 887 to 6027.
  Some screenshots repeat because the UI ran at 19 to 30 fps during the captures.

Cause: with persistence Min, `Ui::step` replaces the picture with the frames merged since the last refresh
(`drawpipe::merge`, `clear_pending`). At 1 kHz and 500 us/div about 47 frames/s arrive in batches, against 20 to 30
screen refreshes, so one refresh holds 1 frame and the next 3 or more. Intensity is graded against `vmax`, which follows
a higher maximum at once and a lower one at 1/8 per refresh (`follow` in `ui.rs`). A refresh with fewer frames than
the one before is drawn dim against the old maximum. The picture pumps at the batch rate. At short time bases
(hundreds of frames per refresh) the relative variation is small, so this shows mostly at slow time bases and with
trigger-limited signals such as the 1 kHz probe-comp output.

Fixes:

| # | Fix | Effort |
|---|---|---|
| F1 | Persistence Min: grade against the frame count of the picture (counts / frames merged), or let `vmax` follow downwards at once when the picture was replaced | small |
| F2 | Persistence Min: replace on a fixed window (for example 50 ms, at least one batch) rather than per refresh, so every picture holds a similar number of frames | small |

Stock behaviour for comparison was not measured. Comparing with `acq.pipeline = frames` needs an oos-ui restart.

## Open

- Measurement on the user's noisy 5 V signal is pending. It should confirm fix 3 (autoset restore) and fix 4 (false
  TD) as the cause of "hard time locking in".
- Why one autoset run in three finished the 20 ms/div stages in 1.2 s is not explained.
- The stock autoset minimum frequency and its time on the same signal are not measured.
