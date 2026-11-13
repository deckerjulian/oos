# Live display compression (next round 2.2, perf plan item 7)

Branch `feat/compress`. Opt-in: `acq.compress = on` in /oos/system.conf, or `OOS_COMPRESS=1` for oos-ui alone. Off
by default until the procedure below has passed on the unit.

## What it does

At 1 us/div and slower a frame holds more samples than the screen has columns. Without compression every sample is
read, decoded and deposited. With it the FPGA's play chain (0x1170, `seq::Settings::compress` = k) sends min/max pairs:
one point per k samples, `POINTS_PER_COLUMN` = 4 points (two pairs) per screen column. A frame is k times smaller, so
a 4 MB read holds up to k times more frames, and each frame is cheaper to decode and draw.

| time base, Auto depth | record | k | bytes per frame | frames per batch (4 MB read) |
|---|---|---|---|---|
| 1 us/div, 1 ch | 14 k at 1.25 GSa/s | 3 | 28 k -> 9.4 k | 142 -> 426 |
| 2 us/div, 1 ch | 28 k at 1.25 GSa/s | 6 | 56 k -> 9.4 k | 71 -> 426 |
| 10 us/div, 1 ch | 100 k at 625 MSa/s | 15 | 200 k -> 13.3 k | 20 -> 300 |
| 10 us/div, 2 ch | 70 k at 625 MSa/s | 15 | 281 k -> 18.8 k | 14 -> 213 |
| 100 us/div .. 1 ms/div, 1 ch | 100 k | 15 | 200 k -> 13.3 k | 20 -> 300 |

From `live_compression_plan_by_time_base`. The frame ring (`max_frames`, 448 at 100 k points) and the trigger rate
bound the batch too: a 1 kHz signal at 10 us/div gives 1 frame per ms whatever the read size.

Host, release, one core: decode + draw of one 10 us/div frame 117 us raw, 12.9 us compressed (ratio 0.11,
`perf_guard_compressed_frame`). Not measured on the unit.

## When it is on

All of these (`compress::allowed`, `hw::live_compress`):

- `acq.compress = on`;
- not roll or scan (`hw::slow_mode`);
- plain edge trigger, Normal acquisition, not Single, no segments, no mask, no zones, no zoom, no Z input, no LA, every
  shown channel DC (or DC 50 ohm) without the 20 MHz limit: the conditions of the dense fast path (`Ui::fast_ok`) plus
  Single, segments and LA;
- nothing else reads the record on screen (`compress::raw_consumers_off`): no protocol decoder, no math channel or
  FFT, the scope view (not spectrum, eye, power, Bode or step), DC trigger coupling (AC, LF and HF reject take the
  software trigger level from the frame's samples);
- the UI's own readers are off: protocol watch and XY publish `Shared::needs_raw`, which the acquisition thread
  honours (`hw::raw_for_ui`);
- at least `K_MIN` = 2 (8 samples per column), record at most `MAX_DEPTH` = 1 M points. Deeper records keep the
  deep-memory read of `hw::batch_plan` (screen window raw, or the whole record compressed to ~625 k points).

Otherwise every sample is read, as before. `hw::tests::modes_that_need_raw_frames_never_compress` checks every mode
that must stay raw (Single, zoom, FFT, math, decode, analyzer views, mask, zones, Peak, Average, High res, AC channel,
20 MHz, trigger AC/LF/HF reject, non-edge trigger, segments, roll, scan, Z input, LA, protocol watch / XY); STOP keeps a
raw final frame (docs/fork/stop-exact.md).

## What stays raw

Every record the UI measures is raw. At most once per `compress::RAW_PERIOD` (200 ms) the acquisition thread plays
the newest frame of a compressed batch once more without compression (`seq::read_raw_again`, before the next arm and
before prearm), and that frame takes the newest frame's place in the queue. `acquire` makes the record from it:
measurements, cursors, FFT, decode, history, autoset. If the second read fails, the compressed frame is used and the
status line says so. A raw read after every batch cost 7-9 % of the frames at 1 channel, 1 and 100 us/div (unitlab
round 2); once per 200 ms it is a few reads a second.

Between two raw records the newest record is of pairs. While running the UI does not measure those
(`compress::hold`): the measurement shown stays that of the last raw record, so every value belongs to one record and
the statistics count each raw record once; measurements update up to 5 times a second. STOP on a pair record asks the
thread for the final frame raw (docs/fork/stop-exact.md: `final_replay` reads the whole newest frame of the last batch
raw), so the record left on screen is raw. Batches of the live compression are not armed ahead (`acq.prearm`), so that
read is always possible.

The compressed frames are only drawn. `drawpipe::draw_frame` sends a frame of the live compression (`compress::live`:
envelope, not roll or scan, at most 1 M deep) through the dense path: trigger refinement on the pairs with an even
smoothing length (`compress::smooth_len`, whole pairs), then `compress::deposit_pairs` (per column the span of the
pairs whose centre lies in it, joined to the nearest end of the last pair before it; in dots mode no join). The
per-frame statistics (`framestats`) skip these frames; the raw newest frame of each batch counts through the record
path.

Visible difference in dots mode: a raw frame deposits one dot per sample (brightness follows dwell), a pair frame one
span per column (brightness uniform along the span), as the record path already draws pairs in dots mode.

## Named constants (correct them from the unit test)

| constant | where | value | meaning |
|---|---|---|---|
| `BLOCK_K` | `hal/src/dho/pairs.rs` | 2 | a pair is min and max of the same 2k samples (1: min of k samples, max of the next k) |
| `ORDER` | `hal/src/dho/pairs.rs` | `Time` | the two points of a pair in the order the extremes first occur |
| `LAYOUT.shift` | `hal/src/dho/pairs.rs` | 0 | points aligned with the record |
| `POINTS_PER_COLUMN` | `ui/src/compress.rs` | 4 | sets k = floor(samples per column / 4) |
| `SCREEN_COLUMNS` | `ui/src/compress.rs` | 1000 | columns k is computed for |
| `K_MIN`, `K_MAX` | `ui/src/compress.rs` | 2, 16383 | smallest useful k; 0x1170 bits 13:0 (`seq::COMPRESS_K_MAX`) |
| `RAW_PERIOD` | `ui/src/compress.rs` | 200 ms | least time between two raw reads of a compressed batch's newest frame |
| `MAX_DEPTH` | `ui/src/compress.rs` | 1 M | deepest record of the live compression |

Hardware guards in `hal/src/dho/seq.rs`: `compress_word` never writes the enable bit with k = 0 (the read stalls) and
clamps k to 16383 (larger values wrap); `compress_k` keeps k <= memsel / 8; `compressed_len` asks for the most
multiples of 8 points with points x k <= memsel. The FPGA delivers exactly 0x11f0 points and waits for samples until it
has them: the stock rounding up asked for 3912 points at k = 256 at 1 M (1 001 472 samples, the window holds 1 000 584)
and the c2h read of 7824 bytes timed out on the unit; k = 128 asked 1 000 448 and fitted. So neither the small read
nor k itself: the length. Now 3904 points at k = 256.

The drawing needs only `PAIR` = 2 (points 2i and 2i + 1 form a pair). `BLOCK_K` decides whether a glitch survives:
with 1, an extreme in the half of a block whose point holds the other extreme is lost. If the unit shows `BLOCK_K` = 1,
set `POINTS_PER_COLUMN` so that a column still holds whole blocks and note the glitch loss here; or keep compression
off.

## Evidence

| claim | level |
|---|---|
| 0x1170 = 0xc0000000 \| k compresses the play to ceil(memsel / k) points, aligned, alternating low/high | unit, k = 4 at 1 M (acquisition-start.md); time axis exact for k 16..200 at deep records |
| pair = min and max of 2k samples from the window start, in time order | unit, 100 % of the points for k 3, 4, 15, 16, 64, 128, 1 and 2 ch (unitlab round 2); netlist simulation (k up to 16383, k = 0 stalls) |
| k = 256 at 1 M time-out = TX length beyond the window | inferred from the sample counts above; unit re-test pending |
| a frame of the ring can be played again after a batch read | unit: the raw replay returns the same record (round 2) |
| k down to 3 (1 us/div) | unit (round 2) |
| picture of pairs = software min/max of the raw record | host model only |
| wfm/s and CPU | unit round 2, raw read every batch: 2 ch 10 us/div 341 -> 409 frames/s, drops 18 -> 0, A53 92 % -> 23 %; 1 ch 1 and 100 us/div -7 to -9 %. With `RAW_PERIOD`: not measured |

## Host tests

- `hal/src/dho/pairs.rs`: the layout model, glitch survival for 2k blocks and loss for k blocks, `check` finds the
  layout (block, order, shift -1) a read was made with at k = 4, 16, 64, 256.
- `hal/tests/compress_raw_again.rs`: FPGA model (`seg::sim::Engine`, compression added): a compressed batch, then the
  newest frame raw; frame index, length, 0x1170 back to off, compressed points equal the model of the raw read.
- `ui/src/compress.rs`:
  - aligned columns: pairs per column equal the software min/max of the raw record within one code (exact here),
    square, noisy square, sine, k = 4, 16, 64, 256;
  - unaligned (970 columns, fractional samples per column, three trigger phases): each column lies within the raw
    column widened by half a block and covers the raw column narrowed by half a block;
  - single-sample glitch, up and down, at four positions per k: in its column's range and lit in the picture, only in
    that column and its neighbours;
  - picture of `deposit_pairs` = `fastdraw::deposit` of the raw record within one row per column (square, sine);
  - end to end through `draw_frame` (decode, trigger on pairs, deposit), 10 us/div, k = 15: at most 4 of 970 columns
    differ by more than one row (host 0, 2, 4; the record path drew 8 to 13 differently), dots and lines;
  - k by time base, smoothing length, opt-in parsing, model conditions;
  - `perf_guard_compressed_frame` (ignored, `tools/perf-guard.sh`): compressed frame <= 0.25 x raw frame.
- `ui/src/board/dho/hw.rs` `live_compression_plan_by_time_base`: k, frame bytes and frames per batch from 1 us/div to
  1 ms/div, 1 and 2 channels; off at 100 ns/div, in Single, above 1 M.
- `fpgad/src/compcheck.rs`: the report names PASS, a different layout, or FAIL.

## On-unit verification

Read-only for storage: nothing is written to the card except /oos/system.conf in step 6. A shell on the unit (SSH).
Probe on the 1 kHz compensation output, CH1 and CH2 on the same probe or a second probe, 1 V/div.

1. Stop oos-ui: `touch /tmp/oos-ui.off`, then stop the process; keep `/tmp/oos-ui.args` for the restart.
2. Pair layout, same record, CH1, 10 ms/div (1 kHz square, 100 periods), Auto trigger on CH1 (default level 1.5 V). For each k:

   ```
   for k in 4 16 64 256; do
     oos-fpgad run --tdiv 1e-2 --depth 1000000 --all --compress $k --raw-too --out /tmp/c$k.bin --execute
   done
   ```

   Each run writes `/tmp/c$k.bin` (compressed) and `/tmp/c$k.bin.raw` (the same frame raw). Copy both to the host and
   run `oos-fpgad compress-check /tmp/c$k.bin.raw /tmp/c$k.bin --k $k` (or on the unit).
   - PASS for every k: the constants stand.
   - LAYOUT DIFFERS: set `pairs::BLOCK_K`, `ORDER`, the shift as reported, rerun the host tests, repeat.
   - FAIL: the raw replay does not return the same record (compare the edge positions of `.raw` and the compressed
     read x k), or k is not honoured at that size. Then compression stays off; note the k where it breaks (`K_MAX`).
3. Small k and the live geometry: repeat step 2 with `--tdiv 1e-6 --compress 3` and `--tdiv 1e-5 --compress 15`
   (Auto depth, the factors oos-ui uses at 1 and 10 us/div).
4. Two channels: `--mask 3` at 10 ms/div with k = 16 and 64, `compress-check ... --mask 3`: both channels PASS
   (interleave of compressed points).
5. Glitch: a pulse source with a narrow pulse (generator option if fitted, or a 20 ns pulse into CH1) at 1 ms/div, k =
   64: the compressed read holds the pulse top in the pair of its block (`compress-check` covers it when the pulse is
   in the compared middle of the record). Without a pulse source: skip and leave the glitch claim at host level.
6. Waveform rate before / after. Restart oos-ui with `acq.pipeline = batch`. Per time base 1 us, 10 us, 100 us/div,
   1 and 2 channels, Auto depth, probe comp on CH1 (CH2), Normal trigger: read `:SYSTem:STATe?` twice 10 s apart
   (frames acquired / used per second) with `acq.compress` absent, then with `acq.compress = on` and a restart.
   Record: frames per batch (`acquire_batch` log line), acquired/s, used/s, dropped/s, CPU of oos-ui (`top -H -d 1`).
7. Picture: screenshots with and without compression at 10 us/div, lines and dots, probe comp on CH1. Edges at the
   same columns, flat tops the same thickness. Then STOP: measurements (Vpp, Vavg, Freq) equal with and without
   compression (the record is raw), and `:WAV:DATA?` in RAW mode returns the full record.
8. Fall-backs: switch on zoom, the mask test, Single, Peak, 20 MHz limit, roll: frames per batch drops back to
   the raw value at once (log), no `raw read of the newest frame` errors in /tmp/oos-ui.log.

Record the results here and in `docs/fork/unit-dho804.md`; flip the default only after steps 2, 6 and 7 pass.

## Risks and open points

- Measurements follow at up to 5 a second while compressed (`RAW_PERIOD`), not per record.
- The mask unit's compression 0x1184 (`hal/src/dho/mask.rs`) is written with a 28-bit k (records / 1000 columns, up to
  125 000 at 125 M). If 0x1184 has the 14-bit field of 0x1170, k above 16383 wraps there. Not changed here.
- Time-ordered pairs carry which extreme came first; the trigger refinement on pairs does not use it yet (it averages
  pairs, good to about a pair).
- Trigger refinement on pairs is good to about one pair (a quarter column): an edge can move by one column between
  frames, a slightly wider edge in persistence than raw at the same rate.
- The Settings change (k) depends on the model, so toggling a raw-only feature reconfigures the acquisition, as any
  settings change does.
- Deep-memory envelope frames (> 1 M) still go to the record path and to `framestats`, which measures Vavg/RMS on
  their min/max points: an existing inaccuracy, not changed here.
