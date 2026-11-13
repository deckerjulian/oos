# Hardware zone trigger

The zone trigger in the FPGA. Opt-in: `zone.hw = on` in system.conf, read once at start, off by default. Code:
`hal/src/dho/zone.rs`, `hal/src/dho/zonegeom.rs`, `seq::State::zone_steps`, `ui/src/ui/hwzone.rs`. Register facts:
private RE notes `docs/fpga.md` "Zone trigger" (static analysis).

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

Readback (regmap, netlist): the arm bit (0x4030[28]) and the pass flag (0x4038[19]) are right but clear whenever no
acquisition cycle runs (0x4030 bit 8 = 0, FSM state 0), which is when OOS reads them, after the batch. The debug line
now shows `cycle` and `state`: `armed` and `pass` mean something only with `cycle=1`. The FSM decides in state 5 once
the record is complete: pass goes on to completion, fail re-arms in hardware (14, 15, 0). A rejected frame costs a
whole acquisition. 0x11a0 is a snapshot of qualified frames (latch 0x1190 bit 28, clear bit 30); its strobe likely
needs the mask column strobe, so it depends on the column layout of `mask::Map` too.

regmap reads the 29/s as most frames being rejected rather than lost in re-arm. Both are checked on the bench: with
the box on the trace, latch `frames` over 10 s in mode intersect, then not intersect. If the two counts do not add up
to the triggers in that time, the table limits or columns are off (sweep with `hwmask.md` "Sweep of the mapping");
if they add up and used/s stays low, the batch wait is the cause.

Bench: the round 3 box case with `zone.batch` unset, then 4 and 16; note used/s, `batch` and `batch_ms` for each,
and the software-filter rate for comparison. Goal: at least the software rate.

## Assumptions

| assumption | where | bench step |
|---|---|---|
| Limits in the read-out word >> 8 (Rigol writes the low byte of a 16-bit level; that does not fit an 8-bit sample) | `mask::code` | 3, 4 |
| 1000 columns of points / 1000 samples over the record | `zone::columns` | 4 |
| Column times relative to the trigger at sample `pre` | `zone::table` | 4 |
| A failing frame is not accepted; the FSM re-arms | | 2, 3 |
| Slot = stream slot, as the mask test (CH2 alone 0, CH2 of CH1+CH2 1) | `mask::slot` | 5 |

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
