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

## Trigger status from the hardware (FPGA use)

0x4034 bits 6 and 7, traced in the netlist of the 00.01.05.00.01 bitstream (private RE notes
`docs/re/netlist`: `regmap/read_sim.tsv` gives the source FF of each read bit; the cones were dumped with
`regmap/nl.py` over the pickled netlist, register FFs named from `regmap/regmap_adc.tsv` and
`blocks/scu_regs.tsv`):

| Bit | Source FF | Next state | Event |
|---|---|---|---|
| 6 | SLICE_X31Y89_A5FF | `event OR (self AND NOT clr)` | SLICE_X38Y85_A5FF = X62Y75_BFF AND NOT X38Y65_A5FF: rising edge of X62Y75_BFF |
| 7 | SLICE_X34Y90_AFF (FDSE) | set by the event, held until clr | SLICE_X54Y91_AFF = X53Y73_AFF AND NOT X52Y73_C5FF: rising edge of X53Y73_AFF |

- clr = SLICE_X34Y76_AFF, the write FF of 0x4034 bit 31 (regmap: 0x4034 31:30 W, libscope
  `DevSystemSCU_clrStatus`; partial decode, also listed as 0x4030 bit 31).
- SLICE_X62Y75_BFF is the TPU trigger into the SCU FSM: in simulation the edge trigger fires it and then
  X53Y73_AFF (`blocks/datapath.md`, "Trigger path into the SCU FSM"). Its cone reaches no auto-trigger register.
- SLICE_X53Y73_AFF = self OR (X57Y78_AFF AND (X52Y73_A5FF OR 0x400c[18] OR X62Y75_BFF OR X39Y66_CFF)). X57Y78_AFF
  is 0x4034 bit 4 (waiting for a trigger). X39Y66_CFF has 0x4060 (t1) at depth 3, 0x4064 (t2) at depth 2 and
  0x4020 (auto enable) at depth 3 in its cone: the auto-trigger timeout. So X53Y73_AFF is "the FSM accepted a
  trigger", from the TPU, the auto timeout or another source.
- So bit 6 = the TPU fired, bit 7 = a trigger was accepted, both sticky until a status clear. Forced by the
  auto timeout = bit 7 without bit 6. The stock priority (bit 6 first, acquisition-start.md section 2) agrees.
  The assumed names there (bit 6 = AUTO, bit 7 = TD) are the wrong way round.
- On the unit (running build, never cleared): 0x11d2 with the level above the signal, 0x11c2 and others while
  triggering. Bits 6 and 7 set in both: sticky, as traced.

Implementation: `trig.hw_status = on` in system.conf (or OOS_TRIG_HW_STATUS=1). After every batch the
acquisition thread reads 0x4034, writes the stock clear (`scu::clear_status`: the read value with bits 31:30
set, then clear) before the next arm, and reports TPU fired / forced only / nothing. The UI shows Trig'd or
Auto from it whenever frames came. Off by default: the status then comes from the software refinement (fix 4).

Verification on the unit, before turning it on by default:
1. Add `trig.hw_status = on` to /oos/system.conf, restart oos-ui. CH1 on the probe-comp output, edge rising,
   AUTO sweep, 500 us/div.
2. Level 0.15 V (inside the signal): `:TRIGger:STATus?` must read TD, continuously.
3. Level 0.6 V (above the signal): it must read AUTO, continuously; the frames come at the auto rate.
4. NORMAL sweep with level 0.6 V: no frames, WAIT.
5. `oos-fpgad rd 0x4034` a few times in case 2 and 3: bit 6 and 7 must now come and go (cleared after
   every batch) instead of staying set.
6. Remove the key afterwards unless all of 2-5 hold; if they hold, make it the default.

## Autoset levels from the measure unit (FPGA use)

Measured on the unit with fixes 1-3 and 7: autoset 1.18-1.57 s on the probe-comp output; the two levels stages
(Wide, Fit) take 0.6-0.7 s of it, each waiting for a 240 ms record at 20 ms/div.

The FPGA's ADC measure unit (private RE notes docs/fpga.md "Measure unit", self-cal.md 2.4) gives every core's
12-bit minimum and maximum (0x1048) and its mean (0x1044, 16-bit scale) over 2^e samples, with no acquisition.
`hal::dho::levels`:
- A run: 0x1040 = (0x1040 & 0x77fff0e0) | 18, then with bit 31; done (bit 30) after 2^18 samples = 1.68 ms;
  per slot 0..7 select 0x1040[10:8], read 0x1044 and 0x1048. The same register handling as the self-cal's
  `adc::measure`, with the shorter exponent and the peaks.
- Slots per channel from the DDR core order (fpga.md): 1 channel all eight, 2 channels 6/4/7/5 (lower) and
  2/0/3/1 (higher), 4 channels CH1 6/7, CH2 4/5, CH3 2/3, CH4 0/1. Assumed to be the measure unit's numbering
  too: not verified on the unit.
- Codes to volts: the unit reads after the ADC x2 of 0x105c and before the Q10 read-path gain (self-cal.md 13:
  about 6970 codes per volt at 1 V/div, x1.077 = the 7500 per division of the records). So a 16-bit code c is the
  read-out word 32768 + (c - 32768) x dg (dg = the cal record's digital gain without the x2), and its volts follow
  from `seq::volts`. Host test: the levels equal the record path's volts for the same codes at several gains,
  scales and offsets.
- Plausibility: a channel's slots must agree (means within a tenth of the swing plus 64 codes; a wrong slot mapping
  shows up there unless the channels carry the same signal), and the mean must lie between the extremes. Codes 0 or
  4095 mark clipping.

The auto set-up (hardware source) asks the acquisition thread for 24 runs (about 40 ms of signal, the full swing
from about 25 Hz) at the wide scale once that scale is applied, rescales on clipping or less than 0.4 div and asks
once more, then fits and goes to the search. Fallback to the record stages: no answer within 0.5 s, a channel
without a usable reading, or still out of range after the second request. `autoset.levels = records` in
system.conf (or OOS_AUTOSET_LEVELS=records) turns the unit off. The thread runs the unit while nothing is armed.

Verification on the unit:
1. CH1 alone on the probe-comp output, `:AUToscale`: the UI log (`oos-ui auto: measure-unit levels`) shows about
   0..0.3 V, and the final 100 mV/div matches the record-based result (`autoset.levels = records`).
2. Time it as before (8 runs): expected about 0.4-0.6 s less than 1.2-1.6 s.
3. CH1 and CH2 on, a signal on CH1 only: CH2 must read flat and CH1 the swing (slot mapping of 2 channels); then all
   four on (4-channel mapping). A refused channel logs `measure unit [..], records`.
4. A 5 V signal: the first answer clips at 1 V/div, the second at 5 V/div fits.

## Autoset search in fewer records

The search went through five time bases (20 ns to 20 ms/div), a record each, from fast to slow so that a coarse rate
cannot fold a fast signal into a period that is not there. On the probe-comp output it took four records, about
0.5-0.6 s. The hardware source now takes three at most (`HW_PLAN` in autoset.rs), on the samples of the acquisition
(the frame behind the record), not the 3600-point screen record:
1. 2 us/div at the full rate: periods up to 8 us (125 kHz and up). It also rules out folding for the next one.
2. One deep record, 1 M points at 2 ms/div (41.7 MSa/s): periods from 0.2 us to 8 ms, so 125 Hz to 125 kHz
   with at least 300 samples per period.
3. 20 ms/div only for a channel the deep record shows fewer than three rises of (or less than 70 % of its height).
The probe-comp output needs two records. Other sources (demo, replay) keep the five time bases. The hardware peak
detect was not used: its window semantics above divider 8 are open (fpga.md).

## On the unit, build d9800d9 (2026-11-13)

Autoset, CH1 alone on the probe-comp output, 6 runs: 0.58, 0.64, 0.73, 0.60, 0.64, 0.74 s (was 1.18-1.57 s with
fixes 1-3, 2.5-5.4 s before them). Search: 2 us/div, then the deep record, sometimes 500 us/div for the final view.
Measure-unit levels -0.05..0.35 V at 1 V/div in the first request, -0.013..0.311 V after the rescale; the records of
the search -0.011..0.312 V. At 1 V/div the unit sees the front end's noise (about +-40 mV) at the full rate; the fit
is the same. The final scale flips between 50 and 100 mV/div because 0.32 Vpp sits on the boundary of `place`.

Several channels:
- CH1 + CH3 (2-channel mode, CH3 open): 2.4 s, trigger level 1.37 V (wrong). The log line was lost in the reboot
  below.
- CH1, CH2 on the probe-comp output, CH3 open (4-channel mode): the measure unit gave CH3 the signal
  (-0.05..0.33 V) and refused CH1 and CH2; the records took over correctly. So the slot map from the DDR core order
  is wrong for 2 and 4 channels (`hal::dho::levels`).
- With 3 and 4 channels the unit rebooted, twice. Both times an autoset ran with mask 0x7 or 0xf. The deep search
  record (1 M points) is then one read of 8 MB (`batch_plan` reads at least one frame whatever its size). That
  read is the suspect; not reproduced on purpose.

Trigger status from the SCU (`trig.hw_status = on`, oos-ui restarted, CH1 probe-comp, 500 us/div, 10 reads of
`:TRIGger:STATus?` each): Auto sweep level 0.15 V TD 10/10 (74 frames/s); level 0.6 V AUTO 10/10 (6 frames/s);
Normal sweep level 0.6 V WAIT 10/10 (0 frames/s); Normal 0.15 V TD. 0x4034 now changes: 0x0c2/0xcd2 while
triggering (bits 6 and 7), 0x012 then 0x492 while auto-firing (bit 7 without bit 6), as traced. All verification
steps hold: the branch makes it the default (`trig.hw_status = off` restores the software status). system.conf was
restored afterwards.

Fix (branch fix/autoset-multich): the measure unit only with one channel on; the deep record at 100 k points with
more than one channel (0.8 MB at 4 channels, 33 samples a period at 125 kHz). Open: whether an 8 MB read itself
crashes the unit (a user-chosen 1 M depth with 4 channels takes the same read), and the real slot map.

## Status (feat/trigger)

| Fix | State |
|---|---|
| 1 fast auto trigger in the levels stages | done, 22e1504 |
| 2 one stamped record per stage, Wide+Fit merged | done, a5a827c |
| 3 keep the fit without a period | done, d0d90f8 |
| 7 level from top/base | done, 8f6f7f9 |
| 4 software comparator = hardware, hysteresis table | done, ed4d03c |
| 8 autoset noise reject, opt-in (Trigger > Auto noise rej.) | done, 417a81c |
| 5 status from 0x4034 | verified on the unit 2026-11-13; default on in fix/autoset-multich |
| 6 counter for the time base | dropped (team-lead, 2026-11-13): after fix 2 a search stage costs one record (tens of ms below 2 ms/div), while the counter needs a 0.25 s gate plus 0.1 s and its threshold (0x2200/0x2204) is not understood (counter.md) |
| (a) autoset levels from the measure unit | done, on by default with the record stages as fallback; on-unit verification pending |
| autoset search in at most three records | done; on-unit timing pending |
| F1 flicker | done: at minimum persistence the picture is graded against the per-frame maximum times the frames in it (`picture_ref` in ui.rs), so equal frames give equal brightness whatever the batch size. F2 (fixed picture window) not needed |

Not yet measured on the unit: autoset time with these changes, and the noisy 5 V signal before and after.

## Open

- Measurement on the user's noisy 5 V signal is pending. It should confirm fix 3 (autoset restore) and fix 4 (false
  TD) as the cause of "hard time locking in".
- Why one autoset run in three finished the 20 ms/div stages in 1.2 s is not explained.
- The stock autoset minimum frequency and its time on the same signal are not measured.
