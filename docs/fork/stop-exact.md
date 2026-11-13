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

- The UI publishes `Shared::hold` = stopped and a record is shown (`Source::hw_hold`, every step). A source started
  in STOP still gets its first record.
- The thread, at the top of the normal batch path: with `hold` it disarms anything armed ahead, arms nothing, answers
  `final_ack` and sleeps 5 ms per pass. Roll, scan, segments, self-cal, the auto set-up levels, counter and DVM keep
  their own paths, as before. RUN or SINGLE clear `hold`; the frames queued before the STOP are dropped then.
- A batch already armed when STOP comes is finished (its wait is not cut short) and read.
- Final frame (`Ui::stop_wants_final`): a plain STOP (not the end of Single or segments, not a mask stop, not a
  history view) while the deep measurements want samples and the record shown is of min/max pairs. The UI raises
  `final_req` and keeps taking records (status STOP) until `final_ack` answers it and the queue is empty, at most
  `STOP_FINAL_WAIT` = 3 s. The thread reads the newest frame of the batch it read last once more, raw
  (`final_replay`: deep window, `seq::read_raw_again`), queues it, then answers. That frame is the record on screen;
  its measurements are made from its samples.
- No replay when the newest frame was already raw (rate-limited deep read, live compression), when the next batch was
  armed ahead (`acq.prearm`: the ring is being written), or when anything else ran on the FPGA since the read (the
  read is only remembered for one pass). Then the record shown is the newest one already queued, measured as it is.

## Host tests

- `ui/src/ui.rs` `stop_shows_the_final_raw_frame_of_a_deep_record`: STOP on a pair record holds the thread and asks
  for the final frame; the batch's last pair frame and the raw final frame are taken, the raw one is shown and
  measured (Mean of its samples), status STOP; later frames are not taken; RUN releases the hold; without an answer
  the STOP ends after 3 s; a shallow record's STOP holds without a request. Mutation check: without taking records
  while waiting the test fails.
- `ui/src/board/dho/hw.rs` `final_replay_at_stop`: replay only for a compressed deep batch whose newest frame is not
  raw, nothing armed ahead, deep measurements on.

## On-unit verification

1. Baseline any depth: RUN, STOP, RUN, SINGLE, STOP at 1 us/div and 1 ms/div. Picture freezes at STOP, resumes at
   RUN; SINGLE still takes the first trigger after the press. CPU of oos-ui in STOP drops (`top -d 1`): nothing is
   read any more.
2. Settings in STOP (V/div, time base, delay): as before (the record is redrawn, no new acquisition).
3. `meas.deep_raw = on`, 10 M at 1 ms/div, probe comp on CH1, Vavg and Vrms on. STOP: within a fraction of a second
   the values belong to the record shown: `:WAV:MODE RAW;:WAV:DATA?` over the screen, mean computed on the host,
   equal to Vavg. Repeat 10 times at different moments.
4. Normal trigger without a signal, STOP: the STOP completes within 3 s (log: no error), RUN works again.
5. `acq.prearm = on`: STOP keeps the record shown (no replay, Mean-type values NaN on pairs); no `disarm` errors.
6. Auto set-up, self-cal, generator changes while stopped: work as before.
