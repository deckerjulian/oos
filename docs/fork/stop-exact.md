# STOP: the record on screen is the one in memory

Branch `fix/stop-exact`. Default on: the pause applies to every STOP; the raw final frame only with `meas.deep_raw = on`
(docs/fork/deep-meas.md).

## Before

STOP never reached the acquisition thread. The UI stopped taking frames, while the thread kept arming and reading
with the last settings, so the FPGA memory behind the frozen record was overwritten at once. With raw deep
measurements the values after STOP belonged to the last raw record (up to 1 s older), not to the pair record on
screen. Upstream behaves the same: the hardware thread has no STOP state outside roll and scan (`Shared::halted`,
Single scan only).

## Now

- The UI publishes `Shared::hold` = stopped and a record is shown or the screen was cleared (`Source::hw_hold`, every
  step). A source started in STOP still gets its first record; a Clear in STOP keeps the thread held (it armed again
  before, overwrote the stored record and filled the queue).
- The thread, at the top of the normal batch path: with `hold` it disarms anything armed ahead, arms nothing, answers
  `final_ack` and sleeps 5 ms per pass. Roll, scan, segments, self-cal, the auto set-up levels, counter and DVM keep
  their own paths, as before. RUN or SINGLE clear `hold`; the frames queued before are dropped then, by the UI as it
  releases the hold (`Source::hw_hold`: it takes frames in the same step) and by the thread on its next pass.
- A batch already armed when STOP comes is finished (its wait is not cut short) and read.
- Final frame (`Ui::stop_wants_final`): a plain STOP (not the end of Single or segments, not a mask stop, not a
  history view) while the deep measurements want samples and the record shown is of min/max pairs. The UI raises
  `final_req` and keeps taking records (status STOP) until `final_ack` answers it and the queue is empty, at most
  `STOP_FINAL_WAIT` = 3 s. The thread reads the newest frame of the batch it read last once more, raw
  (`final_replay`: deep window, `seq::read_raw_again`), queues it, then answers. That frame is the record on screen;
  its measurements are made from its samples.
- Arming ahead (`acq.prearm = on`, the unit's set-up) is skipped for batches the deep measurements may replay
  (`prearm_keeps_replay`: a compressed deep record with deep measurements on), so the ring of the last batch stays for
  the replay. Prearm's gain is at short time bases; at deep records the read takes most of the batch anyway.
- No replay when the newest frame was already raw (rate-limited deep read, live compression), when the next batch was
  armed ahead anyway, or when anything else ran on the FPGA since the read (the read is only remembered for one
  pass). Then the record shown is the newest one already queued, measured as it is.
- Views after the STOP (`Ui::stop_replay`): the thread reads the stored record again for every view (`stop_plan`),
  the first time right after the STOP. Zoomed out on a record above `LIVE_READ` (625 k points: 10 M at 1 ms/div) that
  read is the whole record through the compression, min/max pairs. Those pairs are drawn (`stop_pic`), but the raw
  record of the STOP (`stop_raw`: the final frame, or a raw record already shown) stays `last[0]`, the record measured
  and read out (`:WAV:MODE RAW`). Before, the pairs replaced it one step after the STOP and Mean, RMS, Std dev and AC
  RMS turned to "----" (A3 of the 2026-11-16 review). Far zoomed in the thread reads every sample of the view; that
  window becomes the record shown and measured, as before, and the raw record of the STOP comes back with the next
  view read as pairs. The measurement gate is unchanged: Screen measures the part of the measured record within the
  view (zoomed out beyond a raw window that is only a part of the record: only that part), Full record all of it. A
  view adds nothing to the statistics (`stop_reads_the_record_again_without_counting_it`); a time base or delay change
  clears them, as every setting does. RUN, Clear and the history let the raw record go.

## Host tests

- `ui/src/ui.rs` `stop_shows_the_final_raw_frame_of_a_deep_record`: STOP on a pair record holds the thread and asks
  for the final frame; the batch's last pair frame and the raw final frame are taken, the raw one is shown and
  measured (Mean of its samples), status STOP; later frames are not taken; RUN releases the hold; without an answer
  the STOP ends after 3 s; a shallow record's STOP holds without a request. Mutation check: without taking records
  while waiting the test fails.
- `ui/src/ui.rs` `stop_measures_the_final_raw_frame_while_the_views_are_drawn_from_pairs`: 10 M at 1 ms/div, the
  final raw frame, then the view read as pairs: the pairs are drawn, Mean, RMS, Std dev, AC RMS and Max stay those of
  the raw frame, the count stays; zoomed in and out (pairs): same values, statistics stay cleared; far zoomed in (raw
  window): that window is measured; out again: the raw frame of the STOP again; RUN lets it go. Fails without the
  change: "the view read as pairs: Vavg NaN instead of 0.5".
- `ui/src/ui.rs` `clear_in_stop_keeps_the_hardware_held`: started in STOP, not held until the first record; held
  after a Clear in STOP; RUN releases it.
- `ui/src/ui.rs` `run_after_stop_takes_no_frame_from_before`: a frame queued in STOP (the batch in progress) is not
  the first record of the next RUN; the first frame after the RUN is.
- `ui/src/board/dho/hw/tests.rs` `final_replay_at_stop`: replay only for a compressed deep batch whose newest frame is not
  raw, nothing armed ahead, deep measurements on; with prearm on, such a batch is not armed ahead, so the replay
  stays possible; prearm stays for shallow records and without deep measurements.

## On-unit verification

1. Baseline any depth: RUN, STOP, RUN, SINGLE, STOP at 1 us/div and 1 ms/div. Picture freezes at STOP, resumes at
   RUN; SINGLE still takes the first trigger after the press. CPU of oos-ui in STOP drops (`top -d 1`): nothing is
   read any more.
2. Settings in STOP (V/div, time base, delay): as before (the record is redrawn, no new acquisition).
3. `meas.deep_raw = on`, 10 M at 1 ms/div, probe comp on CH1, Vavg and Vrms on. STOP: within a fraction of a second
   the values belong to the record shown: `:WAV:MODE RAW;:WAV:DATA?` over the screen, mean computed on the host,
   equal to Vavg. Repeat 10 times at different moments.
   Then the views (A3): 10 M at 1 ms/div, generator 1 kHz square or sine on CH1, Vavg, Vrms, Std dev and AC RMS on
   the list, Measure statistics on, Screen gate. RUN a few seconds, STOP, note the values and the count. Wait 2 s:
   the values and the count stay (no "----"), the picture stays the whole record. Time base to 500 us and 200 us/div
   and back to 1 ms/div: the values stay valid at every view (at 1 ms/div equal to those at the STOP), the
   statistics are cleared by the change and the count stays 0 while nothing else changes. 10 us/div (a raw window):
   values of that window; back to 1 ms/div: the values of the STOP again. Then `Full record`: values of the whole
   record. RUN: the values update again.
4. Normal trigger without a signal, STOP: the STOP completes within 3 s (log: no error), RUN works again.
5. `acq.prearm = on` (the unit's set-up): step 3 again, same result; wfm/s at 1 us/div unchanged against prearm without
   the deep measurements (prearm stays on there); no `disarm` errors.
6. Auto set-up, self-cal, generator changes while stopped: work as before.
