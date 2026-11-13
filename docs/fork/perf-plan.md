# Performance plan (P2, 2026-11-13)

Goal: the highest waveform rate per time base and channel count, and a responsive UI, on this DHO804. Inputs: the
fork at 0.1.11 + fork features, `fpga.md` (private RE notes), specs 01-10, `upstream-audit.md` (P1). Static
analysis plus host timings. Nothing here is measured on the unit yet.

## Where the time goes today

Data path per batch (`ui/src/board/dho/hw.rs`, `hal/src/dho/seq.rs`):

1. hw thread: arm, poll 0x4080 every 500 us, let the ring fill up to 25 ms, play, one C2H read of up to 4 MB.
2. hw thread: `decode` every frame of the batch to `Vec<f32>` per channel (up to `1000 / ratio` frames).
3. hw thread: push into `Shared::queue`, then trim it to `QUEUE_MAX` = 96. At short time bases 1000 frames are
   decoded and about 900 are dropped unseen.
4. UI thread, once per screen frame: `drain_but_last` (up to 95 frames), per frame `edge_smooth` + `deposit`
   (dense frames) or `Frame::clone` + `to_record` + `draw_ex` (sparse frames, `x_per_sample > 0.5`, envelope).
   Then `acquire` makes the newest frame a record for measurements, decay, layers, GPU.
5. Re-arm only after decode, unless `OOS_PREARM`.

Consequence: displayed wfm/s is capped at about 95 x UI frame rate (about 3k to 6k), whatever the FPGA and PCIe
deliver. `fpga.md` puts the PCIe ceiling near 80k frames/s at 1 ch for 0x11f0-point frames; stock reaches about 190k
at its fastest setting. The host software is the bottleneck, as `fpga.md` already concludes.

All threads of oos-ui inherit the A72 affinity (`pin_to_big_cores` in `main.rs` before the source starts): UI loop,
hw thread and GPU helper share cpu4/cpu5. Web encoder and `MeasWorker` sit on the A53s. The xdma completion thread
(`poll_mode=1`) is unpinned and busy-polls for the whole DMA time.

## Host measurements

Apple arm64 host, release build, scratch copy (not committed). Unit estimates assume an A72 at 1.8 GHz is about 4x
slower on these loops. Replace with the same benches on the unit.

| path | 1408 pt (100 ns/div) | 12.5 k pt (1 us/div) | 100 k pt (10 us/div) |
|---|---|---|---|
| `decode_volts`, 1 frame 1 ch | 0.5 us | 4.2 us | 34 us |
| `edge_smooth`, span half screen (today) | 3.6 us | 29 us | 229 us |
| `edge_smooth`, span 16 columns + 64 | n/a | 1.3 us | 8.1 us |
| `deposit` f32 into 970 x 448 | 5.7 us | 8.0 us | 17.5 us |
| `to_record` (sparse/envelope fallback) | 36 us | 54 us | 382 us |
| `draw_ex` of the 3600-point record | 7.2 us | 7.5 us | 7.8 us |

- Decode 4 MB, 1 ch: current f64 per sample 0.75 ms, f32 affine 0.15 ms.
- `Phosphor::decay`, 4 maps 970 x 448: 279 us per screen frame (branch per pixel blocks vectorising; off channels
  decayed too).
- `edge_smooth` is 70-90 % of the dense fast path, and 60 % of `to_record` at 100 k points.

## Ranked plan

Gain is the expected effect on the unit; "est" means derived from the host numbers and the code, not measured.

| # | item | gain (est) | effort | risk | host test | opt-in | merge cost |
|---|---|---|---|---|---|---|---|
| 1 | Draw every frame: deposit worker fed per batch | displayed wfm/s at <= 10 us/div from ~3-6k to the acquisition rate (5-10x) | L | M | yes | yes, until unit check | M: new module, ~15-line hook in `ui.rs` step |
| 2 | Decode only frames that reach the queue | -90 % hw-thread decode at <= 1 us/div 1 ch (~4 ms per batch on A72), shorter batch cycle | XS | L | yes | no | L (fork's hw.rs loop) |
| 3 | Narrow-first trigger refinement | `edge_smooth` 20-30x cheaper when the hardware point is near the edge; fast path 3x per frame at 1-10 us/div; `to_record` -55 % at 100 k | S | L | yes | no | L-M (upstream `fastdraw.rs`, `record.rs`; offer upstream) |
| 4 | Sparse fast path (no `to_record` per frame) | 4x per frame at <= 200 ns/div 1 ch, where wfm/s peaks | M | M | yes (pixel compare) | until shots match | M (`fast_draw` in `ui.rs`) |
| 5 | Prearm + xdma interrupt mode + MPS 256 on by default | decode leaves the dead time (2-6 ms per batch); frees ~15 % of an A72 from busy-polling; +8 % link | XS (unit session) | M | partly (exists) | already opt-in; flip after check | L |
| 6 | Thread placement and xdma IRQ affinity | stable UI frame time; a second core for deposit; no poll thread on the A72s | S | L | CPU-set logic only | no | L |
| 7 | FPGA compression for live view at >= 2 us/div, envelope in the fast path | 10 us/div 1 ch: 200 kB -> ~8 kB per frame, 20 -> 500 frames per batch; 5-20x rate where record time allows | M | M-H | yes (plan, decode) | yes | M (`batch_plan` shared with upstream) |
| 8 | f32 decode, no per-frame allocations | ~2-3 ms A72 per 4 MB batch; no mmap/page faults for frames >= 32 k samples (musl malloc) | S | L | yes | no | L (fork-owned decode) |
| 9 | Peak decimation (0x1004 bit 1) for deep memory and slow time bases | deep window 1.25 MB -> ~8 kB per frame; true peak detect; records/s at 10 M toward the 8 ms record bound | M | M | plan only | yes | M (upstream deep-window code) |
| 10 | DVM 0x2244-0x2258 for background Vpp/RMS/mean | no full raw record per pass for background stats at deep memory | M | M | plan only | yes | L (new module next to `counter.rs`) |
| 11 | Phosphor decay branchless, on channels only | 0.5-1 ms per screen frame on A72 | XS | L | yes | no | L (offer upstream) |
| 12 | Smarter batch wait (spec 10 3.8) | lower latency at low trigger rates; fewer polls at slow time bases | S | L | yes (sim engine) | no | L |
| 13 | Mali deposit (additive line strips into R16 maps) | only if CPU deposit stays below the PCIe ceiling after 1+6 | L | M | yes (oos-gpu host test) | yes | H (upstream `oos-gpu.c` protocol) |

Not recommended now: FPGA sinc chain (spec 05), hi-res FIR (spec 02, no FPGA mode bit), skipping the 45 dead
register writes (posted writes, microseconds per configure; do it only as cleanup in the fork's step lists).

## Notes per item

1. Deposit worker. A thread owned by the fork (`ui/src/drawpipe.rs`) gets each batch as an `Arc` of the decoded
   channels or of a DMA buffer copy, plus a snapshot of the screen mapping (time/div, delay, V/div, offsets, map size,
   trigger level, slope, hysteresis, dots). It deposits every frame into its own u16 maps. The UI takes the maps once
   per screen frame (swap, saturating add, before decay) and keeps only the newest frame for `acquire`. A mapping
   change bumps a generation counter; stale batches are dropped. Keeps `fast_draw` statistics (`fast_edges`). Per
   frame on the A72 at 1408 points: about 25 us est, 40k frames/s per core; two A53 workers with private maps add
   about the same again. Host test: identical maps to the current UI path on a fixture batch.
2. In the batch loop, `skip(shown - QUEUE_MAX)` before decode, `n` still counts every frame. Superseded by 1 for the
   fast path, still right for the queue.
3. Search `16 x column + 64` samples around `pre` first; fall back to the half-screen search when nothing is found.
   Same answer whenever the near search finds an edge with its lead-in intact; the unit's `fast_edges[0]` rate shows
   the hit ratio. Applies to `fast_draw` and `to_record`.
4. Sparse frames: refine with `edge_near` (narrow), sinc-interpolate only the screen window (polyphase, fixed taps,
   NEON-friendly), then `draw_ex`. Removes the per-frame `Frame::clone` and the 3600-point record. Default once
   screenshots match `to_record` at 5, 20, 100, 200 ns/div.
5. Code exists (B10). Unit checks: `/proc/interrupts` counts on IRQ 241 with `poll_mode=0`; ten minutes without
   time-outs; prearm with a settings change mid-wait; long read with data check at MPS 256.
6. cpu4: UI loop + GPU helper. cpu5: hw thread. Deposit workers, `MeasWorker`, web: A53. `oos-fpga-up` writes the
   xdma IRQ to an A53 (`/proc/irq/241/smp_affinity`). No SCHED_FIFO until the hw thread is shown never to spin.
7. 0x1170 compression to 2-4 points per screen column when frames exceed the screen width. Needs envelope support in
   the fast path (today `f.envelope` falls back to `to_record`) and a raw newest frame for measurements. Open
   hardware question: can the last frame of a batch be played a second time uncompressed? If not, take a raw batch
   every N-th batch.
8. `decode_volts`: precomputed f32 `a`, `b` per channel (f32 rounding differs from today by at most 1 ulp; the
   bit-identity tests need a tolerance). Drop the `on: Vec` (audit). Frame buffers return to the hw thread through a
   pool, so musl's mmap path for blocks >= 128 kB is not hit per frame.
9. At divider 1/4/8 the pairs are exact (min, max over 2N, merged across cores). The low-rate path is 6.25x denser
   with overlapping windows not yet decoded, and divider > 8 semantics are open: opt-in, compare with software peak
   on a glitch fixture on the unit first.
10. The 0x1040-0x1048 measure unit is per ADC core and pre-decimation: calibration and auto-set only.
11. Write `*x = ((*x as u32 * k) >> 16) as u16` without the zero test (0 stays 0); skip maps of channels that are off.
13. Mali-T860 with additive blending could deposit far more polylines than the CPU, but it changes upstream's GPU
    helper protocol. Decide after 1 and 6 are measured.

## Step 0: unit baseline (blocked on SD cards, item C)

Per time base (5 ns, 100 ns, 1 us, 10 us, 200 us, 1 ms/div) x 1/2/4 channels: wfm/s, `:SYSTem:STATe?` parts, the
`acquire_batch` log line (frames, fill time), `top -H -d 1` per thread and core, `/proc/interrupts`. Run the host
benches above on the unit too to replace the 4x assumption.

## P3: benchmark guards for the fork's fast paths

Extends the audit's guards 1-5 (allocation counts, early trigger end, decode ratio). Deterministic guards run in the
normal `cargo test`; timing guards are `#[ignore]` ratio asserts, run by a `tools/perf-guard.sh` step in the sync
checklist.

| guard | kind | protects |
|---|---|---|
| frames decoded per batch <= frames delivered to consumers | count, deterministic | 2 |
| drawpipe maps equal the UI fast path on a fixture batch; frames deposited = frames read | equality + count | 1 |
| narrow-first refinement equals the wide search on fixtures where the edge is within the window | equality | 3 |
| `edge_smooth` narrow / wide time <= 0.2 | ratio, ignored | 3 |
| sparse fast path / (`to_record` + `draw_ex`) time <= 0.4; pixel compare within tolerance | ratio + equality | 4 |
| f32 decode within 1 ulp of the f64 reference; time <= 0.5 x f64 path | equality + ratio | 8 |
| no allocation per frame in the batch loop after warm-up (counting allocator) | count | 1, 8 |
| big/little CPU sets from a fake sysfs tree; hw thread and workers on the intended sets | deterministic | 6 |
| `decay` time per map <= 0.3 x the branchy reference | ratio, ignored | 11 |

Ratios, not absolute times: hosts differ, ratios between two paths on the same host are stable. Keep a reference
copy of each replaced function in the test module so the ratio has a fixed baseline.

## Status

Item 8, HAL part (branch perf/hal, 2026-11-13). Host M3 Max, release, one 4 MB batch:

| case | upstream two-pass ms | f64 fused ms | f32 fused ms | f32 into kept vectors ms |
|---|---|---|---|---|
| 1 ch, 1420 x 2816 B | 1.45 | 0.71 | 0.40-0.44 | 0.37 |
| 1 ch, 20 x 200 kB | 1.31 | 0.67 | 0.33 | 0.32 |
| 2 ch, 1000 x 4000 B | 2.2 | 0.77 | 0.42 | 0.38 |
| 4 ch, 500 x 8000 B | 2.2 | 1.06 | 0.58 | 0.56 |

The f64 fused column is a clean reference copy (collect); the library's f64 version measured 1.38 ms because its
`extend` did not vectorise. f32 error: under 0.01 code. The loop is near memory bandwidth, so f32 gains 1.6-2x, not
the 5x of the arithmetic alone.

- `seq::decode_volts_into(bytes, mask, cal, &mut [Vec<f32>; 4])`: no allocation once the vectors have room.
- `decode_volts` keeps its signature: oos-ui builds unchanged.
- Left for the ui: per-frame allocation in the batch loop goes away only when `hw.rs` `decode` takes recycled
  vectors (a pool the UI thread returns dropped frames' `ch` vectors to) and calls `decode_volts_into`. Until then
  every frame of >= 32 k samples per channel is a fresh musl mmap.

## Status of the raw deposit (branch perf/deposit)

The unit's four A53 workers ran at ~90 % at 10 us/div, 100 k points (unitlab), where compression is not allowed
(mask, zones, zoom, Peak, non-DC). Host profile of one worker frame (M3, release, 100 k points, sine fixture of the
drawpipe tests): decode 17 us, trigger refinement 11 us, deposit 43 us per channel, of which 30 us were the vertical
spans: one read-modify-write per pixel, each a row (1940 bytes) from the last.

- `fastdraw::deposit` works out the spans of a frame first (`spans`, per column rows lo..hi into a per-thread array),
  then adds them column by column as before when they are short, or row by row over the rows the frame touches when
  they are long: a branchless pass per row that adds `wgt` where the row lies in the column's span (NEON: cmge.8h,
  and, uqadd.8h, eight pixels per step; checked in the release asm). Threshold `sweep_cost`: pixels > rows x
  columns / 8 (host-tuned; on the A53, where a scattered pixel costs more against a vector step, the sweep would pay
  off earlier).
- Column min/max: `fastdraw::min_max`, NEON with four min and four max accumulators (fminnm/fmaxnm .4s, 16 samples
  per step) instead of one compare chain per sample.
- Output bit-identical: `deposit_is_bit_identical_to_the_scattered_writes` compares with the old function
  (`deposit_reference`) over 9 signals (incl. NaN gaps, clipping, flat), 3 lengths, 6 mappings, 4 vertical scales and
  saturation; both ways of adding occur in the corpus. `min_max_equals_the_chain`.

Host results: deposit per channel 0.37-0.98 of the old (sine 0.49, noise 0.37, square 0.87, clipped full-height
0.98; `perf_guard_deposit_spans`, guard <= 1.25). Worker frame decode + draw at 10 us/div: 1 ch 114 -> 72 us, 2 ch
181 -> 81 us; `perf_guard_deposit_worker_per_frame` at 125 k points: frames mode 130 -> 85 us, one pipeline core 155
-> 113 us. Not measured on the A53.

Not changed, for the next step:
- Dots (`deposit_dots`, the default display): 130 us per channel at 100 k points, one scattered write per sample.
  Counting rows per column first (lanes against store forwarding) was slower on the host for steep signals (each
  sample on its own row); not kept.
- Memory traffic: decode writes 400 kB of f32 per channel and the deposit reads it back; on the A53 (512 kB L2) that
  is likely the larger cost. A fused decode + deposit from the 16-bit codes (the volt mapping is monotone, so the
  column extremes come from the code extremes exactly) with only the trigger window decoded would cut it about 5x,
  but needs a board hook in drawpipe (codes are board-specific).
- Per-frame statistics (`framestats`, measurements on): +160 us per frame at 125 k points, more than the deposit.

## Status of item 7 (branch feat/compress)

Done, opt-in `acq.compress = on` (`docs/fork/compress.md`): FPGA compression at k = floor(samples per column / 4) for
records up to 1 M points where the dense fast path applies; the newest frame of each batch is read again raw
(`seq::read_raw_again`) for the record; pairs drawn by `compress::deposit_pairs`. Host: 10 us/div 1 ch 20 -> 299
frames per batch, decode + draw 117 -> 12.9 us per frame. Unit procedure in compress.md, not run.

## Status of the UI items (branch perf/ui)

- 2 reverted (every acquired frame is to be used, see the deposit pipeline).
- 3 done, default on: `trignear::edge_smooth_near` in `fast_draw` and `to_record`. The near answer is kept only when
  the comparator state proves it equals the wide search (module comment), else the wide search runs. Guards: equality
  over a synthetic corpus (42 k cases, half answered near), two adversarial fallback frames; ignored timing test, host
  ratio 0.04-0.05 (30 -> 1.4 us at 12.5 k points, 231 -> 8.5 us at 100 k).
- 6 done, opt-in `perf.place = on` in system.conf (or OOS_PLACE=1 for oos-ui alone): `placement::place` puts the
  frame loop and GPU helper on cpu4, the hw thread on cpu5; workers run on the A53s. Guard: plan from a fake sysfs
  tree. xdma interrupts: `rootfs/board/dho/bin/oos-irq-place`, run by `oos-board bring-up` after the cold boot (one
  fork line in `oos-board`), writes the little-core mask (0xf on the RK3399, from `cpu_capacity`) to
  `/proc/irq/<n>/smp_affinity` of every `xdma` line in `/proc/interrupts`; a no-op without `perf.place = on`, never
  fails the bring-up. Checked on the host with a fake /proc and /sys under busybox sh. With poll_mode=1 (the default)
  there is no xdma interrupt: the completions run in xdma's own per-CPU kernel threads, which the script cannot move
  (they are bound per CPU); the IRQ step matters with `xdma.params = poll_mode=0` (item 5). Unit check: `top -H -d 1`
  per core, `/proc/interrupts`, UI frame time with and without the setting.
- 4 done, default on, bit-identical: `sparse::draw` replaces `Frame::clone` + `to_record` + `draw_ex` for sparse
  frames with an edge trigger found on a channel (else `to_record` as before). Most of the cost is `lanczos8`
  (30 of 40 us per channel at 3600 points), not the clone (5 %). `sparse::lanczos8_many` sums eight points at a time,
  each in `lanczos8`'s order, so the points are the same bits. Host ratio 0.60-0.62 (40 -> 24 us per channel). The
  plan's 4x needs a polyphase kernel over the screen window only, which changes pixels: not done, it needs a
  screenshot comparison on the unit. Guards: pixel equality over 612 set-ups (450 answered by the new path), bitwise
  kernel equality incl. record ends and NaN samples, ignored timing ratio <= 0.8.
- 1, batch pipeline (`drawpipe.rs`), selectable: `acq.pipeline = batch` in system.conf or `OOS_PIPELINE=batch`;
  `frames` (default) is the old path. Every frame is used: the hw thread decodes only the newest frame of a batch for
  the queue and lends the read-out buffer itself (a pool of 3 `DmaBuf`, no copy; `DmaBuf` is a plain heap buffer, the
  xdma read can target any of them) with the other frames to 2 deposit workers on the A53s. Each decodes its share
  into kept vectors (`hw::decode_into`, `decode_volts_into`), refines the trigger and draws with `draw_frame` (the body
  of `Ui::fast_draw`) into its own maps, counting the trigger statistics. Once per refresh the UI adds the maps and
  the statistics (`fast_edges`, `fast_off` cover every frame). Busy workers, older settings and an older mapping drop
  work, counted. Where the fast path does not apply the hw thread queues every frame as before.
  Counters acquired / used / dropped (both modes): `:SYSTem:STATe?` and `stat` (`pipeline ... acquired .. used ..
  dropped ..` at the end of the line), System info "Frames used".
  Guards: batch picture == frames mode over the same frames (dense, sparse, dots, 1-2 channels, split over 2 workers),
  trigger statistics equal, buffers come back, stale/busy work dropped and counted with dropped = fed - taken.
  `perf_guard_deposit_worker_per_frame` (host M3): one worker costs 0.99-1.09 x the frames mode per frame; 2 workers
  40 k frames/s at 100 ns/div (1408 pt, sparse: 3600 interpolated points per frame), 83 k at 1 us/div, 13.5 k at
  10 us/div. A53 estimate (not measured): about 8x the host per frame (A72 about 4x, A53 about 2x an A72), so 2 A53
  workers reach about 5 k frames/s at 100 ns/div and 10 k at 1 us/div. At short time bases they will not keep up with
  the acquisition: the sparse interpolation dominates (polyphase kernel, item 4, needs the unit's screenshot check),
  and the A72 hw thread may take a share.
  Work sharing: one worker per little core (`placement::little_count`, 4 on the RK3399, 2 where all cores are alike).
  Workers claim 4 frames at a time from the batch. The hw thread claims single frames while the ring fills, with or
  without prearm, through `seq::acquire_batch_idle` (hal: an idle hook called instead of the 200 us sleep, the frame
  count read after every call, so the read waits at most one frame; no change when unset; hal test
  `idle_during_ring_fill`). Host per frame on one core (M3, perf_guard): 50 us at 100 ns/div (1408 pt, sparse), 25 us
  at 1 us/div, 163 us at 10 us/div; 4 workers 75 k / 116 k / 16 k frames/s. Estimate (not measured; A72 about 4x the host, A53
  about 8x): 4 A53 reach about 10 k frames/s at 100 ns/div, 20 k at 1 us/div, 3 k at 10 us/div; the A72 helper adds up
  to 5 k / 10 k / 1.5 k when its wait is long.
  Arm ahead, opt-in `acq.prearm = on` in system.conf or OOS_PREARM (any value; set wins over the key): `seq::prearm`
  arms the next batch before the decode of the current one (spec 10 item 3.5). Default off until the unit check
  (unit-dho804.md, arm ahead).
  Fast interpolation, opt-in `acq.interp = fast` / OOS_INTERP=fast (`sparse::fast_many`): the exact 16-tap sinc
  on a grid of 24 points per sample (one fixed filter per phase over consecutive samples, vectorised), linear in
  between, used where the grid has at most 4x the screen's 3600 points (else exact). A 4-lobe kernel at the same
  points was tried first and rejected: up to 1.3 V (72 px) off at 0.45 fs. Guard over 53 sparse set-ups (sines to
  0.45 fs at +-3.8 div, noisy square, 1 and 4 channels, 5-200 ns/div): max 0.0087 V = 0.49 px in the points, 1 px on
  the screen (nearest lit pixel, Chebyshev). Host per sparse frame: 25 -> 15 us at <= 80 samples on screen (5 ns/div
  1 ch, 20 ns/div 4 ch), 0.86 at 20 ns/div 1 ch, no gain from 50 ns/div (1 ch): there the exact kernel already does
  under 3 points per sample and `draw_ex` (7.5 us) is a fixed part. Estimate per sparse frame: A72 about 100 us
  exact / 60 us fast at 5 ns/div, A53 about 200 / 120 us. Default stays exact until a screenshot comparison.
  Measurement statistics over every frame (`framestats.rs`): Max, Min, Pk-pk, Mean, RMS, Std dev / AC RMS of the
  Measure page are made by the workers for every frame, on the samples the record path measures (the record's
  trigger point via `board::trigger_point`, split out of `to_record`; the screen window of the frame when it holds more
  than 3600 samples) and folded into the page's statistics (`Ui::mstat_fold`); other measurements keep counting the
  measured records. Frames whose record is measured on interpolated points (short time bases) are not counted.
  Sums split over 8 lanes (the data is 12 bit; bit identity of f64 sums has no physical meaning). Guard: per-frame
  values equal to `to_record` + `analyze_min`, n and the extremes exact, sums 1e-12 relative. Host cost with all six
  on: +19 us per frame at 12.5 k points, +190 us at 125 k (2 workers: 45 k and 5.9 k frames/s). Short time bases stay
  out (the record is measured on sinc points, which overshoot between samples: raw-sample max/min would be another
  definition); `measured` in the status line and "Frames in statistics" in System info show the coverage.
  Software averaging: deferred until the unit shows whether the FPGA average (B1) works and covers deep records
  (`fpga.md` "Average mode": coupled to the frame read-out, unverified). In batch mode Average stays outside the fast
  path, so every frame goes through the queue as in frames mode.
