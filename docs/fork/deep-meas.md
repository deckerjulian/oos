# Mean-type measurements of deep records from raw samples

On by default since the unit test of 2026-11-15 (below); `meas.deep_raw = off` in /oos/system.conf, or
`OOS_DEEP_RAW=0` for oos-ui alone, turns it off.

## Problem

A record above 1 M points is read for the screen through the FPGA compression (`hw::batch_plan`: the whole record
to ~625 k min/max points) unless the screen is a small part of it. Since `fix/envelope-stats` the measurements of such
a record leave out what pairs cannot give: Mean, RMS, AC RMS, Std dev, Area and the cycle variants show NaN
(`framestats::needs_samples`).

## Design

- The UI publishes `Shared::deep_raw` (`deepmeas::want`): on, Normal acquisition, and a shown measurement or the
  software DVM needs samples. `every` in Single.
- After a compressed deep batch (`deep_raw_settings`: compression on, no window, record above 1 M, Normal, no LA) the
  acquisition thread reads the screen window of the newest frame once more, raw (`seq::read_raw_again` with a window),
  before the next arm, as the live compression's raw newest frame does. Window: `screen_window`'s rule (screen and a
  quarter screen each side, at least 4096 points, the trigger point inside), or the whole record when that reaches
  past its end; at most `deepmeas::MAX_BYTES` = 24 MiB for all channels (12 M points of one channel). The C2H path
  splits it into reads of at most 4 MiB.
- Rate: once per `deepmeas::PERIOD` = 1 s while running; every batch in Single.
- The raw frame replaces the newest frame of its batch, so its record is an ordinary raw record: measurements,
  cursors, FFT, decode and history get samples. The picture of that record goes through the record path like the
  pair records (same style).
- While raw deep reads come (`deepmeas::hold`: running, not Single, a pair record whose window fits the budget) the UI
  does not measure the pair records in between. Every value shown belongs to one raw record, at most about 1 s old,
  and the Measure statistics count each raw record once. Max, Min and Pk-pk then update once a second instead of per
  record.
- The raw record measured is the newest raw one of the UI step, not the newest record: at 2 ms/div and up (10 M) a
  batch is one frame, so the next pair frame is often queued behind the raw one. The UI drew the raw frame on the fast
  path (or replaced it as the record) and the value stayed at the last raw record's for good (unit round 5). A raw
  record that comes while the background worker is busy waits for it (`meas_pending`); Clear drops it with the record
  it belongs to.
- Off, in Peak (the memory holds peak pairs), above the budget, or with the deep screen-window read (short time base,
  already raw): as before.

## What STOP shows

Since `fix/stop-exact` (docs/fork/stop-exact.md): STOP holds the acquisition thread, which reads the newest frame of
its last batch raw when the record shown is of pairs; that frame becomes the record on screen, so the measurements
belong to it. With `acq.prearm = on` such batches are not armed ahead, so the replay works there too.

## Constants

| constant | where | value |
|---|---|---|
| `PERIOD` | `ui/src/deepmeas.rs` | 1 s |
| `MAX_BYTES` | `ui/src/deepmeas.rs` | 24 MiB |
| window rule | `hw::window_below` | screen + 2 x max(screen / 4, 4096), trigger point +-4096 |

## Evidence

| claim | level |
|---|---|
| a frame of the ring can be played again after a batch read | unit for the LA read and the segment stamp plays; the compressed-then-raw replay of the live compression is host only (compress.md step 2) |
| a raw window read of a deep record (screen_window) | unit, existing deep-memory path |
| replay of a deep frame with a window after a compressed read | not shown |
| cost: about 0.1 s of read per second at 10 M, 1 channel | estimate from the PCIe rate, not measured |

## Host tests

- `ui/src/deepmeas.rs`: opt-in parsing, `want` (Max alone needs nothing, Single sets `every`, Peak and channel off
  refuse, the software DVM asks), `hold` (only running, not Single, pair records of deep frames within the budget),
  budget and rate.
- `ui/src/board/dho/hw.rs` `deep_raw_window_for_the_measurements`: 10 M at 1 ms/div: window holds screen and trigger
  point, within 24 MiB, decodes to a raw frame with the right trigger point; none for 2 channels at 10 M, 25 M, the
  short-time-base window read, 1 M, Peak.
- `ui/src/ui.rs` `deep_raw_records_carry_the_measurements_between_pair_records`: nothing on pairs before the first
  raw record, the raw record's Mean, then three pair records keep it and are not counted in the statistics; off: pair
  records measured with Mean NaN. Mutation check: without the hold the test fails.
- `ui/src/ui.rs` `live_raw_deep_records_are_measured_while_the_worker_is_busy`: at 1, 2 and 5 ms/div, a new value
  each round, the worker busy when the raw record comes, pair records after it (at 2 and 5 ms/div one already queued
  behind it in the same step). Fails without `meas_pending` (Mean NaN) and without the step's raw record (NaN at
  2 ms/div).
- `ui/src/ui.rs` `clear_drops_the_record_waiting_for_the_worker`: a raw deep record waits for the busy worker, Clear,
  a shallow record is measured, a pair record after it keeps that record's Mean, not the one cleared away.

## On-unit verification

Probe on the 1 kHz compensation output, CH1, 1 V/div. Baseline first without the key.

1. 10 M depth, 1 ms/div (10 periods on screen), Auto trigger. Measure Vavg, Vrms, Std dev,
   Vmax. Without `meas.deep_raw`: Vavg/Vrms/Std dev show NaN ("----"). Note wfm/s and `:SYSTem:STATe?` (acquired/s).
2. `meas.deep_raw = on`, restart oos-ui. Within about 1 s Vavg, Vrms and Std dev show values; Vavg about half the
   square's amplitude plus its low level, Vrms consistent with a 50 % square. Compare with the same measurements at
   1 M depth (raw records, no compression): within 1 %.
3. Rate: acquired/s against step 1; at most about 10 % lower. `top -H -d 1`: oos-ui CPU, the measure thread on the
   little cores. /tmp/oos-ui.log: no `raw read of the deep window` errors.
4. The raw record is the same acquisition: Single at 10 M, then the record shown (`:WAV:DATA?` RAW over the screen)
   against its Vavg: equal. A slow edge on screen must sit at the same place in the raw record as in the pair picture
   before it (no shift of the window: trigger point check).
5. Budget: 2 channels at 10 M (40 MB) and 1 channel at 25 M: mean-type stay NaN, Max/Min update per record, no raw
   reads in the log.
6. STOP while running at 10 M: see docs/fork/stop-exact.md step 3; then RUN again.
7. Peak mode, roll, zoom: no raw deep reads (log), as before.

Record the results here and in `docs/fork/unit-dho804.md`.

## Result on the DHO924S (2026-11-15)

1 kHz, 2 Vpp sine from the generator on CH1, 1 V/div, VRMS on the measurement list.

- Values: 10, 25, 50, 100 and 125 M at 200 us/div all give 0.711 Vrms (`deep_memory` in tools/device-test.py, three
  runs), as at depth Auto. Max and Pk-pk come from the pairs before the first raw record.
- Found and fixed on the way:
  - The first record after a change of settings, in a batch of one frame (deep records), could be the older memory
    of the settings before: the whole screen window of a 10 M record was the 4 Vpp of the depth-Auto records before
    it (twice the RMS). `hw.rs` leaves that one record out (never two in a row; not in SINGLE or hardware average).
  - While the worker measured a raw window (more than 262 k points) the row showed "not available" for that time,
    once a raw read: the last value now stays until the new one is there.
- Rate: a raw read costs about 3 % of the records per MiB read once a second. The period grows with the window
  (`deepmeas::period`: 1 s per 2 MiB), so the cost stays near 5 %:

  | channels | depth, time base | records/s off / on | fps off / on |
  |---|---|---|---|
  | 1 | 10 M, 1 ms/div | 4.85 / 4.59 | 27.5 / 26.2 |
  | 1 | 10 M, 200 us/div | 3.82 / 3.65 | 29.1 / 28.5 |
  | 2 | 10 M, 200 us/div | 4.65 / 4.59 | 29.7 / 29.0 |

- Budget: windows above `MAX_BYTES` (1 channel at 25 M and 1 ms/div, 2 channels at 10 M and 1 ms/div) keep the
  pair measurement: mean-type not available, no raw reads, the rate unchanged.

## Risks

- Replay of a deep frame after a compressed read is not shown on the unit (step 4 decides); a wrong window would put
  the measurement on the wrong samples.
- A 12 M-point raw frame is copied once into `Shared::frame` (48 MB) and run through `to_record` and the measurement
  threads: about once a second; the measurement of 12 M samples on the little cores may take longer than the period,
  then results come less often.
