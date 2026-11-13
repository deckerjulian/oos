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
| Slot = ADC lane: the channel with 1, 3 or 4 channels; lower of two = 0, higher = 2 | `mask::slot` | step 5 |
| 1000 columns spread over the whole record, n = points / 1000 per column | `mask::per_column` | step 4, a failure late in the record |
| One counted frame per acquired frame | | step 2 |
| Stop on fail (0x4020 bit 12) stops the FSM without hanging the acquisition thread | | step 6 |

Only bits 9:0 and 15 of 0x1184 reach the comparator logic. Records above 1023 x 1000 points may mis-count columns.

## On-unit procedure

Probe on CH1 at the probe-comp output. Optional: BNC cable from AUX OUT to CH2 (step 7). Start from a known state:
`mask.hw = on` in system.conf, restart oos-ui. Note `:SYSTem:DEBug:MASK?` and `:SYSTem:STATe?` after every step.

1. CH1 only, 1 V/div, 200 us/div, edge CH1 rising at 50 %, Normal sweep, Auto depth. Mask > Create mask with the
   default tolerances. The pill reads `Mask (HW) CH1`. `:SYSTem:DEBug:MASK?` shows `slot=0`, `ctrl=0x00100000`.
2. Frames counter against acquisition: let it run 30 s. `frames` rises steadily. Compare its rate with the `acquired`
   counter of `:SYSTem:STATe?` over the same interval. Record the ratio (expected 1: one tested frame per acquired
   frame; batches may make `acquired` count frames the FPGA tested before the read).
3. Clean signal: `fails` stays 0 for at least 10,000 frames. A non-zero count means the code scale or the column
   spread is off. Compare with `mask.hw = off` (software test) on the same signal.
4. Tighten: Y tolerance to the minimum, then move the CH1 offset by 0.2 div. `fails` must rise, and the software
   counts must agree in direction. Undo the offset, Reset: `fails` back to 0 and stays there. Then Create mask at
   1 V/div and switch to 500 mV/div: the table is reloaded for the new scale and `fails` stays 0 while the trace fits
   on screen.
5. Slot mapping: probe on CH2, CH1 off, mask on CH2: expect `slot=1`, `ctrl=0x00200000`, frames rising, fails 0.
   Then CH1 and CH2 on (2-channel mode), mask on CH2: expect `slot=2`, `ctrl=0x00400000`. With the probe on CH2 and
   CH1 open, `fails` stays 0 and a 0.2 div offset change on CH2 makes it rise. If CH1's offset change makes it rise
   instead, the slot is wrong: record which slot follows CH2 and fix `mask::slot`. Repeat with mask on CH1 in the
   2-channel mode (slot 0).
6. Stop on fail: Mask > Stop on fail on, tight mask, offset nudge. The acquisition stops with "Mask failed: stopped".
   Then Run: frames continue without a restart of oos-ui. If the acquisition thread hangs (status line stops, no new
   frames after Run), record it: bit 12 then has to stay off and the stop stays in software.
7. AUX OUT (cable to CH2, CH2 1 V/div, its own trigger off): System > AUX out = Pass/fail. Clean signal: CH2 flat.
   Tight mask: one 1 us high pulse per failed frame on CH2 (single-shot on CH2 to catch it). AUX out = Trigger out
   afterwards.
8. `mask.hw = off`, restart: the pill reads `Mask CH1`, the software counts run, and the debug line starts with `off`.

Record the results in `docs/fork/unit-dho804.md`.
