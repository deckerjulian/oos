# Hardware mask test

The mask (pass/fail) test in the FPGA's mask unit. Opt-in: `mask.hw = on` in system.conf, read once at start. Off by
default until the steps below confirm the assumptions. Register description: `specs/06-mask-unit.md`. Code:
`hal/src/dho/mask.rs`, `seq::State::mask_steps`, `ui/src/ui/hwmask.rs`.

## What it does

- The software mask (`dsp/src/mask.rs`) stays the source of truth. `Mask::columns(1000)` reduces it to 1000 column
  bands over the record, each the union of the bands it covers, so the table never forbids what the software allows.
- `mask::table` turns the volt bands into 8-bit limits for the source channel's current V/div and offset. Limits are
  rounded outwards; a limit at or beyond the code range is disabled. A V/div or offset change reloads the table.
- With the FSM stopped (acquisition block of `configure`): 0x4020 bit 15 off, the table (1000 writes to 0x1194, then
  0x1194 = 0), 0x1184 = samples per column (points / 1000, bit 31 when more than 1), the slot in 0x1190, 0x4020 bits
  15..13 on and bit 12 = stop on fail. Nothing is written while the test runs as wanted.
- Counters are cleared on the first start, a new slot, Create mask, Load mask, Reset and Clear display. A table reload
  keeps them, as the software test keeps its counts when the tolerances change.
- After every batch: latch (0x1190 bit 28: 0, 1, 0), read 0x11a0/0x11a4 (frames) and 0x11b0/0x11b4 (fails), 48 bits.
- The mask statistics show the hardware counts (`Mask (HW)`), passed = frames - fails. A new failure stops the
  acquisition when "stop on fail" is set. The fast display path stays on.
- Off when `mask.hw` is off, the mask is off, the source channel is not acquired or the source is not live hardware.
  Then the software test runs as before. While the hardware tests, the software test still runs on every full record
  that reaches the UI (outside the fast path), for comparison only.
- AUX OUT pass/fail (System > AUX out, `:SYSTem:AOUTput PFAil`) is offered with `mask.hw = on`: 0x119c = 1 us high
  pulse on a failed frame, 0x1430 = 3 (`auxout.md`).
- Fork-local `:SYSTem:DEBug:MASK?`: `ch,slot,ctrl,frames_lo,frames_hi,fails_lo,fails_hi,frames,fails,n`, then the
  software counts and the pipeline counters (`sw_pass,sw_fail,acquired,used,records`). `off,enabled,active,...`
  without a hardware reading.

## Assumptions

| assumption | where | how the bench checks it |
|---|---|---|
| The comparator sees the read-out word >> 8 (32768 at the offset, 7500 codes per division) | `mask::MID`, `CODES_PER_DIV`, `SHIFT` | step 3 and 4 |
| Slot = stream slot (libscope): 0 with one channel; lower of two 0, higher 1; the channel with 3 or 4 | `mask::slot` | step 5 |
| 1000 columns spread over the whole record, n = points / 1000 per column | `mask::per_column` | step 4, a failure late in the record |
| One counted frame per acquired frame | | step 2 |
| Stop on fail (0x4020 bit 12) stops the FSM without hanging the acquisition thread | | step 6 |

Only bits 9:0 and 15 of 0x1184 reach the comparator logic. Records above 1023 x 1000 points may mis-count columns.

## Sweep of the mapping (unit round 2)

Round 2 (private RE notes `docs/logs/oos_unitlab_20261113.md`): slot 0 and the frame count are right (171/s against
171/s), but every frame fails on a clean signal while the software passes; it passes only at X 1 div and Y 3 div. So
the value mapping and the column layout are both off. Two likely causes:
- The record at 200 us/div, Auto depth is 2.5 Mpts: 2500 samples per column, but only bits 9:0 and 15 of 0x1184 reach
  the comparator, so the strobe count the hardware sees is not 2500.
- The DVM block on the same front end sees 12-bit codes around 2048 with about 428 codes per division at 1 V/div
  (about 200 at 100 mV/div): the scale before the digital gain (`7500 / gain` in 16 bits), not the read-out's 7500.

The mapping is `mask::Map`, shared by the mask test and the zone trigger. It is set at start from system.conf lines
`mask.map.<key> = <value>` and at run time with `:SYSTem:DEBug:MASK:MAP <key>=<value>` (no rebuild, no restart; the
next batch reloads the table). `:SYSTem:DEBug:MASK:MAP?` reads it back.

| key | values | meaning |
|---|---|---|
| `domain` | `readout` (default), `pregain` | 16-bit scale: read-out words, or before the digital gain (codes/div divided by the channel's gain) |
| `cpd` | number, default 7500 | 16-bit codes per division |
| `mid` | number, default 32768 | 16-bit code at the channel's offset (screen centre) |
| `shift` | 0..16, default 8 | right shift to the 8-bit sample (a 12-bit sample: `mid=2048 cpd=468.75 shift=4`) |
| `offs` | number, default 0 | added to every 8-bit code |
| `invert` | 0, 1 | code axis upside down |
| `cols` | `record` (default), `clocks`, `stock`, or a number | 1000 columns of points/1000 samples; the same count in sample clocks (4/ratio samples each); the stock zone set-up (count 1, 1000/(4/ratio) columns); 1000 columns with this strobe count |
| `range` | `a-b`, default `0-999` | only these columns test; the others pass everything |

The hardware gives no per-column result. `:SYSTem:DEBug:MASK:COLumns? a,b` shows the software's view of the same
table: per column `c:lo-hi/<lo en><hi en>:min-max` with the lowest and highest code of the newest raw record's
samples in that column under the current mapping, `!` where it would fail, then `fail=N,first=...`. It needs a raw
record (not compressed, the fast path keeps one per screen update). With `range` the hardware itself localises
failures: halve the range until the fail counter stops rising.

Procedure (probe-comp on CH1, 1 V/div, hardware mask on, Reset between steps, 2000 frames per reading of
`:SYSTem:DEBug:MASK?`):
1. Short record first, to take the strobe count out: 20 us/div, depth 10k (10 samples per column). Create mask with
   the default tolerances. Note `fails/frames` for `domain=readout` and `domain=pregain`.
2. If both fail every frame: `range=0-499` and `range=500-999`, then halve the failing half until a range of about 10
   columns. `:SYSTem:DEBug:MASK:COLumns? a,b` over it shows whether the codes (min-max against lo-hi) or the edges
   (a column whose samples hold both levels) are off. Codes off by a constant: `offs`; by a factor: `cpd`; mirrored:
   `invert=1`. Edges in the wrong column by a constant factor: `cols=clocks` or `cols=stock`, else a count
   (`cols=<n>`) and note which n lines up.
3. When step 1 passes (0 fails in 2000 on the clean signal, fails rising with a 0.2 div offset nudge), repeat at 1, 2
   and 4 channels (the clock layout depends on it) and at 100 mV/div and 2 V/div (the gain).
4. Then the long record: 200 us/div, Auto (2.5 Mpts). With `cols=record` the count 2500 does not fit 10 bits; try
   `cols=` 2500 & 1023 (452) and `cols=1023`, and note whether bit 15 changes anything (`cols=33220` is 452 with bit 15 set).
5. Write the winning map into this file and make it the default in `mask::Map::default()`; the zone trigger takes the
   same map (`hwzone.md`).

## Round 5 and the fix

Round 5 (`mask.map` sweep): `domain=readout` is right (pregain fails). At 20 and 50 us/div with 10k points the mask
works exactly at 1, 2 and 4 channels: 0 fails in about 1800 clean frames, every frame fails with the delay shifted by
2 or 10 us. At 100 and 200 us/div every frame failed with every column layout, and `cols=400`, 452, 1023 and 33220
stalled the acquisition for 15-25 s.

Cause: the software mask spreads over the screen, the table columns over the record, and the record is longer than
the screen (1.6 screens at 10k points from 20 us/div). With a flat signal around the trigger (20 and 50 us/div on a
1 kHz square) the stretch did not show; from 100 us/div the edges at +-0.5 ms landed in the wrong columns. Fixes:
- The table maps by time (`mask::Timing`): column c covers record samples c n .. (c + 1) n at (k - pre) / rate, which
  is placed on the screen (10 divisions around the delay). Columns not wholly on the screen test nothing.
- The strobe count stays within 1023 and within what the record holds for 1000 columns (`Map::layout`); the stalls
  came from counts whose columns ran past the record. Records above 1.02 M points have their first 1.02 M tested.
- The slot is the stream slot (regmap from libscope): CH2 alone is slot 0, CH2 of CH1+CH2 slot 1.

## Stall guard (round 8)

At 200 µs/div with 100k and Auto depth the unit stopped the acquisition: every arm ended with "no acquisition
(0x4080 bit30 not set)". The acquisition thread now counts arms that find no acquisition while the mask or zone unit
runs. After two in a row it drops the unit for the current settings (time base, depth, channel set, `mask.map`) and
publishes the reason in `Shared::unit_blocked`. The UI then runs the software test and filter and shows a toast once.
`:SYSTem:DEBug:MASK?` reports `blocked=1`. Changing any of those settings tries the hardware again. The cause of the
stall is open.

## Strobe sweep (round 8)

regmap: the column strobe comes from a read-back state machine (0x4030 bit 20), not from the ADC stream. A 10-bit
counter of strobes ends the window. With fewer than 1000 x count strobes per record the window never ends and the
FSM waits in state 5: the stall. The strobe unit and the terminal count are open.

`cols=raw:<count>:<samples per strobe>` writes `count` to 0x1184 as is (up to 1023, not kept to the record) and
maps the table by `count x samples per strobe` samples per column. The stall message in /tmp/oos-ui.log carries
0x4030, the read-back bit and the state. A stalled point drops to software for that map; the next value retries.

Grid, one channel, clean probe-comp square, then the +10 µs shift:

- 200 µs/div at 10k, 100k and Auto, and 1 ms/div at 10k.
- count 1, 2, 3, 5, 10, 25, 40, 100, 1023; samples per strobe 1 and 4.
- Per point: stall or not (read-back bit, state), hw and sw fail counts, `:SYSTem:DEBug:MASK:COLumns? 250,750`.

The largest count without a stall gives the strobes per record (count x 1000). The samples per strobe that makes hw
and sw agree on the shift gives the unit.

## On-unit procedure

Probe on CH1 at the probe-comp output. Optional: BNC cable from AUX OUT to CH2 (step 7). Start from a known state:
`mask.hw = on` in system.conf, default `mask.map`, restart oos-ui. Note `:SYSTem:DEBug:MASK?` and `:SYSTem:STATe?`
after every step. The mask is in volts, so an offset change moves mask and trace together: test with delay shifts.

1. CH1 only, 1 V/div, edge CH1 rising at 50 %, Normal sweep. Mask > Create mask with the default tolerances. The pill
   reads `Mask (HW) CH1`; the debug line shows `slot=0`, `ctrl=0x00100000`.
2. Regression grid (round 5): for each of 20, 50, 100, 200 us/div and 1 ms/div at depth 10k, and 200 us/div at 100k
   and Auto: Create mask, Reset, 1000 frames: `fails` 0; delay +10 us: every frame fails; delay back, Reset: 0 again.
   The software counts (`mask.hw = off`) give the same verdicts.
3. Frames counter against acquisition: 30 s, `frames` rate equals the `acquired` rate of `:SYSTem:STATe?`.
4. Scale: Create mask at 1 V/div, switch to 500 mV/div: the table is reloaded and `fails` stays 0 while the trace fits
   on screen; Y tolerance to the minimum and delay +2 us: fails rise.
5. Slot mapping: probe on CH2, CH1 off, mask on CH2: `slot=0`, `ctrl=0x00100000`, 0 fails, delay shift fails. Then
   CH1 and CH2 on (CH1 open), mask on CH2: `slot=1`, `ctrl=0x00200000`, the same results. A wrong slot shows as
   every frame failing (CH1's flat line against CH2's square mask). Four channels, mask on CH3: `slot=2`.
6. Stop on fail: Mask > Stop on fail on, tight mask, delay shift. The acquisition stops with "Mask failed: stopped".
   Then Run: frames continue without a restart of oos-ui. If the acquisition thread hangs, record it: bit 12 then has
   to stay off and the stop stays in software.
7. AUX OUT (cable to CH2, CH2 1 V/div, its own trigger off): System > AUX out = Pass/fail. Clean signal: CH2 flat.
   Delay-shifted: one 1 us high pulse per failed frame on CH2. AUX out = Trigger out afterwards.
8. `mask.hw = off`, restart: the pill reads `Mask CH1`, the software counts run, and the debug line starts with `off`.

Record the results in `docs/fork/unit-dho804.md`.
