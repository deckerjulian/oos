# Hardware zone trigger

The zone trigger in the FPGA. Opt-in: `zone.hw = on` in system.conf, read once at start, off by default. Code:
`hal/src/dho/zone.rs`, `hal/src/dho/zonegeom.rs`, `seq::State::zone_steps`, `ui/src/ui/hwzone.rs`. Register facts:
assumed, not yet verified on hardware unless a step below says so. What the logic does with a frame (qualification,
both modes, two zones, mapping in time and value, slots) is verified on the unit (DHO924S, 2026-11-16, "On the
DHO924S" below).

## Hardware

- Two zone tables on the mask unit's 8-bit sample path and column strobe: the source slot in 0x1190 bits 23:20 and
  the strobe count in 0x1184 apply as for the mask test. Each zone has its own column counter.
- Table word (0x11d8 zone 1, 0x11dc zone 2): 7:0 lower limit, 15:8 upper limit, 16 = the column belongs to the zone,
  27:18 column, 28 write enable (level-sensitive).
- A zone is touched in a frame when, in a column of the zone, a sample lies within lower..upper (limits included), or
  the trace jumps from below to above the zone (or back) between consecutive samples.
- 0x11d0: bits 24/25 zone 1/2 on, bits 26/27 mode (1 = pass when touched, 0 = pass when not touched). The result is
  the AND of both zones; a zone that is off passes.
- 0x4020 bit 1 arms the logic (readable at 0x4030 bit 28). The latched pass flag (0x4038 bit 19) feeds the trigger
  FSM: the zone qualifies the trigger. That a failing frame is not accepted and the FSM re-arms is inferred, not
  measured. The normal trigger stays configured; the zone acts on top of it.
- The mask frame counter (0x11a0/0x11a4) counts zone-qualified frames.

## What OOS does

- The software zones (`ui/src/zone.rs`) stay the reference. A box becomes its four corners; for each table column the
  vertical extent of the zone within the column's time strip (`zonegeom::extent`) gives the limits.
- Codes: the same assumed scale as the mask (`mask::code`: read-out word >> 8 at the channel's V/div and offset),
  rounded outwards and clipped to 0..255. A column the zone does not reach has bit 16 clear.
- Columns: 1000 over the record, `points / 1000` samples per column (0x1184), as the mask test. Column c covers
  samples c n .. (c + 1) n at times (k - pre) / rate relative to the trigger. The stock system instead leaves 0x1184
  at one count and writes 1000 / (4 / ratio) columns (250, 500, 1000 for 1, 2, 4 channels). The bench decides which
  holds; `zone::columns` is the one place to change.
- With the FSM stopped (acquisition block of `configure`): 0x4020 bit 1 off, the tables that changed, 0x11d0, 0x1184,
  the slot (counter cleared on the first start, a new slot, Clear display), 0x4020 bit 1 on. Nothing is written while
  it runs as wanted. A V/div, offset or time base change reloads the tables.
- One mask unit: the hardware mask test wins. While it runs the zones fall back to software.
- Active when `zone.hw = on`, the source is live hardware, at least one zone is usable, all usable zones are on one
  channel, that channel is acquired, and the hardware mask test is not running. Else the software filter alone.
- While active the fast display path stays on (only qualified frames arrive). Live compression stays off with zones
  (`compress::allowed`). The software filter still runs on every full record that reaches the UI and counts its
  drops (`sw_drops`).
- AUX OUT pass/fail (offered with `zone.hw = on`) takes the zone result: 0x119c bit 29 set, 1 us high pulse.
- Fork-local `:SYSTem:DEBug:ZONE?`: `ch,slot,ctrl,s4030,s4038,frames_lo,frames_hi,fails_lo,fails_hi,armed,pass,
  frames,n`, then `sw_drops,acquired,used,records`. `off,enabled,active,...` without a hardware reading.

The value and column mapping is `mask::Map`, shared with the mask test; sweep it with the mask first
(`hwmask.md` "Sweep of the mapping"). `:SYSTem:DEBug:MASK:MAP` changes it for both.

## Rate

Round 3 on the unit: qualification right in every case, but 29 passing frames/s against 187/s with the software
filter (200 us/div, 1 kHz probe-comp, box on the trace). 1 / 29 s is the 25 ms the batch read waits for the ring to
fill plus about 9 ms of arm and read: with the zone logic armed the ring got one frame per arm, and every batch waited
out the 25 ms for the rest. A batch is now `zone.batch` frames while the zone trigger runs (system.conf, default 1), so
the read follows the first qualified frame at once. `:SYSTem:DEBug:ZONE?` reports the last batch as `batch=<read>/
<planned>,batch_ms=<arm to read>`; with `zone.batch = 8` the bench sees whether the ring holds more than one
qualified frame per arm after all.

Readback (assumed): the arm bit (0x4030[28]) and the pass flag (0x4038[19]) are right but clear whenever no
acquisition cycle runs (0x4030 bit 8 = 0, FSM state 0), which is when OOS reads them, after the batch. The debug line
now shows `cycle` and `state`: `armed` and `pass` mean something only with `cycle=1`. The FSM decides in state 5 once
the record is complete: pass goes on to completion, fail re-arms in hardware (14, 15, 0). A rejected frame costs a
whole acquisition. 0x11a0 is a snapshot of qualified frames (latch 0x1190 bit 28, clear bit 30); its strobe likely
needs the mask column strobe, so it depends on the column layout of `mask::Map` too.

Assumed: at 29/s most frames are rejected rather than lost in re-arm. Both are checked on the bench: with
the box on the trace, latch `frames` over 10 s in mode intersect, then not intersect. If the two counts do not add up
to the triggers in that time, the table limits or columns are off (sweep with `hwmask.md` "Sweep of the mapping");
if they add up and used/s stays low, the batch wait is the cause.

Bench: the round 3 box case with `zone.batch` unset, then 4 and 16; note used/s, `batch` and `batch_ms` for each,
and the software-filter rate for comparison. Goal: at least the software rate.

## Assumptions

| assumption | where | bench step |
|---|---|---|
| Limits in the read-out word >> 8 (Rigol writes the low byte of a 16-bit level; that does not fit an 8-bit sample) | `mask::code` | 3, 4: verified on the unit, to 0.1 div |
| 1000 columns of points / 1000 samples over the record | `zone::columns` | 4: verified on the unit at 10k points |
| Column times relative to the trigger at sample `pre` | `zone::table` | 4: verified on the unit, to 40 us at 200 us/div, before and after the trigger |
| A failing frame is not accepted; the FSM re-arms | | 2, 3: verified on the unit (no acquisition completes, FSM state 14) |
| Slot = stream slot, as the mask test (CH2 alone 0, CH2 of CH1+CH2 1) | `mask::slot` | 5: verified on the unit, also with four channels |

## Stall guard (round 8)

At 200 µs/div with 100k and Auto depth the unit stopped the acquisition: every arm ended with "no acquisition
(0x4080 bit30 not set)". The acquisition thread now counts arms that find no acquisition while the mask or zone unit
runs. After two in a row it drops the unit for the current settings (time base, depth, channel set, `mask.map`) and
publishes the reason in `Shared::unit_blocked`. The UI then runs the software test and filter and shows a toast once.
`:SYSTem:DEBug:MASK?` reports `blocked=1`. Changing any of those settings tries the hardware again. The cause of the
stall is open.

## On-unit procedure

Probe on CH1 at the probe-comp output (1 kHz square). `zone.hw = on` in system.conf, restart oos-ui. Note
`:SYSTem:DEBug:ZONE?` and `:SYSTem:STATe?` after each step. Hardware mask test off.

1. CH1 only, 1 V/div, 200 us/div, edge CH1 rising at 50 %, Normal sweep. Draw zone 1 as a box on the high level
   right after the trigger, mode "intersect". The debug line shows `slot=0`; `armed=1` and `pass=1` only on readings with `cycle=1`.
   The picture keeps updating at the normal rate.
2. Move the box above the trace (no touch). Expect no new frames: the status stays WAIT and `frames` stops while
   `:SYSTem:STATe?` shows no new acquisitions accepted. If frames keep arriving and `pass=0`, the zone only flags:
   record it; then the software filter must stay on (set `zone.hw = off`).
3. Mode "not intersect" with the box above the trace: frames arrive again. Box back on the trace: they stop.
   Compare each case with `zone.hw = off` (software filter): the same frames pass, `sw_drops` rises where the hardware
   withholds frames.
4. Mapping in time and value: a narrow box (2 % of the screen wide) across the rising edge at the trigger, between
   40 % and 60 % of the amplitude, intersect: frames pass. Move it 1 div right onto the flat high level, still between
   40 % and 60 %: no frames. Move it to the last division of the screen across a falling edge: frames pass again
   (columns reach the end of the record). If a box only works when shifted, record the shift: it gives the column
   layout or the time origin.
5. Slot mapping: probe on CH2, zone on CH2, CH1 off: `slot=0`. CH1 and CH2 on, zone on CH2: `slot=1`; the zone must
   follow CH2, not CH1 (offset CH1 by 2 div: no effect on qualification).
6. Two zones: zone 1 intersect on the high level, zone 2 not intersect above the trace: frames pass. Zone 2 moved onto
   the trace: no frames.
7. AUX OUT (optional, cable to CH2, CH2 not acquired by the zone): System > AUX out = Pass/fail. One pulse per
   evaluated frame of the zone result, per the polarity in `auxout.md`. AUX out = Trigger out afterwards.
8. Mask test on (hardware mask): the debug line turns `off` and the software zone filter takes over; mask off: the
   zone hardware returns (`armed=1`).

Record the results in `docs/fork/unit-dho804.md`.

## On the DHO924S (2026-11-16)

`zone.hw = on`. The probe compensation output (1 kHz square, 0.03 to 2.98 V) on CH2, 1 V/div, 200 us/div, 10k points,
edge trigger on CH2 rising at 1.5 V, Normal sweep. The edges are at 0 and +-0.5 ms.

A zone that no frame passes ends every arm without an acquisition, which is what the stall guard counts: after two
arms it drops the unit to software for these settings. So "the hardware rejects" shows as the guard's log line
(`no acquisition ...; 0x4030 = 0x5f87e90e, readback 0, state 14`), and each case below started with a fresh guard key
(a `mask.map` change). "passes" means frames arrive with the unit active (`slot=` in the debug line).

| step | case | hardware | software filter |
|---|---|---|---|
| 1 | box on the high level, intersect | passes, 78 frames/s | passes |
| 2 | box above the trace, intersect | rejects every frame, status WAIT | rejects |
| 3 | box above the trace, not intersect | passes | passes |
| 3 | box on the high level, not intersect | rejects | rejects |
| 4 | box 40 us wide, 1.2 to 1.8 V, across the edge at 0, at +0.5 ms, at -0.5 ms | passes, each | passes |
| 4 | the same box on the flat parts at +0.2, +0.7, +0.96, -0.3, -0.7 ms | rejects, each | rejects |
| 4 | the same box 40 us before and 40 us after the edge at +0.5 ms | rejects, both | rejects |
| 4 | box 0.2 V high over the high level (2.98 V): 2.8 to 3.0 and 2.9 to 3.1 V | passes | passes |
| 4 | 2.2 to 2.4, 2.6 to 2.8, 3.1 to 3.3, 3.2 to 3.4, 3.4 to 3.6 V | rejects | rejects |
| 4 | 3.0 to 3.2 V | passes 48 frames/s | drops 30 of them a second: the limits are rounded outwards |
| 4 | box over the low level (0.03 V): -0.1 to 0.1 and 0.0 to 0.2 V pass; -0.4 to -0.2, 0.1 to 0.3, 0.3 to 0.5 V reject; -0.2 to 0.0 V as 3.0 to 3.2 V above | as listed | the same |
| 5 | CH1 + CH2, zone on CH2: `slot=1`; box on CH2's high level passes, on its flat part at 1.2 to 1.8 V rejects; CH1's offset moved by 4 V changes nothing | as listed | the same |
| 5 | four channels, zone on CH2: `slot=1`, the same two verdicts | as listed | the same |
| 6 | zone 1 on the high level with zone 2 "not intersect" above the trace, or "intersect" on the low level | passes | passes |
| 6 | zone 1 on the high level with zone 2 "not intersect" on the trace, or "intersect" above it | rejects | rejects |
| 7 | AUX OUT pass/fail from the zone result (0x119c = 0x2000009c, 0x11d0 = 0x05000000): no pulse on accepted frames with event = fail; with event = pass a 1 us pulse per accepted frame (76 frames/s: rms 29.4 mV on the DVM block, `auxout.md`); none with the mask as the source | as listed | |
| 8 | mask test on: the zone debug line turns `off`, the mask unit runs; mask off: the zone unit is back | as listed | |

Rate: 78 frames/s with the zone trigger (`zone.batch` unset, `batch_ms` 10 to 14) against 157 acquired/s without a
zone and 127 used/s with the software filter. The rate goal of "Rate" above is not met on this unit either.

Open: a zone that rejects for longer than two arms loses the hardware for these settings (the guard cannot tell a
rejecting zone from a stall), so in normal use the unit falls back to software as soon as the signal leaves the zone.
`zone.batch` 4 and 16 were not tried.
