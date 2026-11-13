# Fork vs upstream audit (P1, 2026-11-13)

Base: `dho804` after the 0.1.11 merge (089577e). Scope: everything both the fork and upstream implement or touch.
Read-only. Nothing verified on hardware. Host: Apple M3 Max, release build.

## Measurements

Existing bench (`cargo test --release --lib -- --ignored --nocapture bench_batch`), one 4 MB batch:

| case | read old -> new ms | decode old -> new ms |
|---|---|---|
| 1 ch, 1420 x 2816 B | 0.10 -> 0.06 | 1.60 -> 1.38 |
| 1 ch, 20 x 200 kB | 0.08 -> 0.05 | 1.32 -> 1.33 |
| 2 ch, 1000 x 4000 B | 0.07 -> 0.05 | 2.29 -> 1.41 |
| 4 ch, 500 x 8000 B | 0.07 -> 0.06 | 2.21 -> 1.37 |

Scratch bench with a counting global allocator (not committed). Old = upstream path (`read_dma`, `unpad`,
`deinterleave`, `volts`). New = fork path (`DmaBuf`, `unpad_in`, `decode_volts`). Per batch, read + decode:

| case | old ms | new ms | old allocs | new allocs | old MB alloc | new MB alloc |
|---|---|---|---|---|---|---|
| 1 ch live, 1420 x 1408 pt | 1.68 | 1.43 | 4261 | 2840 | 16.1 | 8.0 |
| 2 ch live, 1000 x 2000 pt | 2.40 | 1.47 | 6001 | 3000 | 20.0 | 8.0 |
| 4 ch live, 500 x 2000 pt | 2.39 | 1.44 | 5001 | 2500 | 20.0 | 8.0 |
| 1 ch deep window, 600 k pt | 0.47 | 0.43 | 4 | 2 | 4.8 | 2.4 |
| 4 ch deep window, 150 k pt/ch | 0.67 | 0.43 | 11 | 5 | 6.0 | 2.4 |
| 1 ch compressed 625 k pt | 0.50 | 0.42 | 4 | 2 | 5.0 | 2.5 |

The remaining 8 MB per batch is the f32 output of the frames (inherent to `Frame`). The read gain is understated on
the host: on the unit every fresh 4 MB `Vec` is a new mmap with zero fill and ~1000 page faults on an A72/A53. Not
measured on the unit.

## Acquisition and DMA path

| item | upstream | fork | verdict | evidence | action |
|---|---|---|---|---|---|
| C2H read buffer | `read_dma`: new zero-filled Vec per read, then move to front | `DmaBuf` kept across batches, aligned region of its own, `read_dma` a one-shot wrapper | fork | -1 alloc/batch, -4 MB zero fill; same DMA isolation; `dma_buf_is_reused_aligned_and_exact_with_short_reads` | keep; protect (guard 1) |
| Decode | `deinterleave` to u16 per channel, then `volts` | `decode_volts`: one pass, bit-identical | fork | 1.6x at 2/4 ch, 1.1x at 1 ch; -50% allocated bytes; `fused_decode_equals_deinterleave_and_volts` | keep; protect (guard 1, 2) |
| `decode_volts` internals | n/a | builds `on: Vec<usize>` per call | fork, fixable | 1 heap alloc per frame: 1420 per 1 ch batch | replace `on` with a `[bool; 4]` or bitmask iterator |
| Unpad | `unpad` on Vec | `unpad_in` on slice, `unpad` wraps it | fork | `unpad_in_place_equals_unpad` | keep |
| Step runner | `execute_until` holds the Poll/Read logic | `plain_step` shared by `execute_bus_until` (generic, host-testable) and `execute_into_until` | fork | lets `acquire_batch` run on `seg::sim::Engine` | keep; see merge cost below |
| `acquire_batch(_until)` | `Bar1` + `File`, returns `(Vec<u8>, u32)`, linux only | generic `Bus`/`Read`, writes into `DmaBuf`, `armed` flag, returns count | fork | host model runs it; zero-copy into the decode | keep; highest merge-conflict spot (upstream edited it in f1b2d6c) |
| Early trigger end (NEW_SETTINGS) | upstream f1b2d6c, in `execute_until` Poll | ported into `plain_step` Poll, same 10 ms check | equal | same code moved | keep; guard 3 so a future upstream edit to the Poll body is not lost |
| Prearm | none | `arm_only_steps` after the read (and LA read), `disarm_steps` before any other FPGA write; opt-in `OOS_PREARM` | fork, unverified | `prearm_writes_the_same_sequence_as_the_plain_batch_loop` | keep opt-in until the unit check; see open risk below |
| LA read | `execute` -> new buffer per read | unchanged (`acquire_la` still allocates) | equal | n/a on this unit (`la = no`) | low: pass a second `DmaBuf` |
| Roll/scan read | `read_dma` + `deinterleave` + `volts` per chunk read | unchanged upstream path | upstream path left | small reads, ~30/s | low: switch to a kept `DmaBuf` and `decode_volts` |
| xdma params | `insmod xdma.ko poll_mode=1` fixed | `xdma.params` system key, default `poll_mode=1`, validated in `oos-fpga-up` (sh) and `fpgad::xdma_params` (Rust) | fork, opt-in, unverified | default identical to upstream | keep; validation exists twice, add one test that feeds both the same bad inputs |
| MaxPayload 256 | none | `OOS_BOOTARGS_EXTRA` in unit `build.env`, commented out | fork, opt-in | spec 10, 3.7 | keep |

## Settings, deep memory, counter, segments, averaging

| item | upstream | fork | verdict | evidence | action |
|---|---|---|---|---|---|
| Deep-memory screen window (8ecd182) | `batch_plan` + `screen_window`: reads the screen window instead of the compressed record | uses upstream's code; fused decode applies to it | upstream adopted | 4 ch window 0.67 -> 0.43 ms | keep upstream's; FPGA peak path (spec 04) is the next step for the compressed case |
| `batch_plan` frames | 4 MB / frame bytes | plus: average mode reads one frame, so no size limit | fork extension | `batch_read` | keep |
| Header logging | header only with `OOS_FRAME_HEADER`, logged on every decode | logged only with `header_probe()` | fork | segment read-back turns the header on; upstream would log every segment | keep |
| Frequency counter | `counter_tick` once per loop pass, mutex lock per pass | same; fork B8 dropped | upstream | cost: one lock per pass, register reads only after the gate | keep upstream's. Unverified: counter start/stop writes land while a prearmed batch records. Check on the unit with `OOS_PREARM` |
| Segments | software segments in the UI | B3 hardware segments, opt-in (`seg_hw`), fall back to software where `seg_max` is None | both, layered | `hardware_segments_only_where_they_record`, `record_steps_*` | keep both; hardware path unverified |
| Averaging | software average in the UI | B1 hardware average, opt-in (`avg_hw`), `Frame::hw_avg` makes the software average skip it | both, layered | `avg.rs` tests, `avg-writes.txt` fixture | keep both; hardware path unverified |

## Remote services, network, image

| item | upstream | fork | verdict | evidence | action |
|---|---|---|---|---|---|
| Remote services | on/off switches for web, SCPI, VXI-11, telnet (`remote.*`) | `netacl` source ACL (`net.allow`) on HTTP update/MCP, SCPI, VXI-11 | both, orthogonal | switch = whether it listens, ACL = who; handles v4-mapped v6 | keep both |
| Telnet | `remote.telnet` in `/oos/system.conf` | dropped own switch, uses upstream key, `oos-conf` adds `/etc/oos/system.conf` defaults | upstream adopted | `image_defaults_under_the_settings` | done |
| Firewall vs link-local | link-local fallback 169.254/16 when DHCP fails | nftables drop-by-default from `net.allow`, runs before the network | conflict | direct-cable laptop is blocked by nft and by `netacl` | add `169.254.0.0/16` to the unit's `net.allow`: link-local is not routable, and any on-link host can already spoof a 192.0.2.x source, so the exposure does not grow |
| MAC | from SD CID, in rcS | `oos-mac`: CID default, or `eth.mac` (stock or fixed) | fork superset | default unchanged | keep; unit `system.conf` holds the real MAC, remove before any publication |
| Unit defaults | none | `/etc/oos/{limits,system}.conf` via `OOS_UNIT`, `build_unit` | fork | `unit_defaults_tests` | keep |
| Image build | `build-dev-image.sh` | adds `OOS_UNIT`, `build.env`, `nft`, extra bootargs, all DTS variants | fork | conflicted in the 0.1.11 sync | move fork lines into one sourced hook (`rootfs/unit/hook.sh`) to shrink the conflict surface |

## Kept both paths: remove one

| where | what | action |
|---|---|---|
| `hal/src/dho/seq.rs` `execute_into`, `execute_into_until` (public `execute_into`) | `execute_into` has no callers | remove `execute_into`; keep `execute_into_until` (used by `execute`) |
| `hal/src/dho/seq.rs` `execute_until` | upstream API, now a wrapper with no callers in the fork | keep as wrapper: upstream may call it again and the wrapper avoids a conflict |
| `hal/src/dho/bus.rs` `read_dma` | one-shot wrapper over `DmaBuf` | keep for roll, fpgad and `fpga.rs`; move roll to `DmaBuf` later |

## Where upstream is better

The deep-memory window and the frequency counter. Both are adopted. Upstream's telnet key is adopted. No upstream code
path is faster than its fork counterpart.

## Guards against future merges (P3)

1. Allocation test, `hal/tests/alloc_batch.rs` (own binary with a counting `#[global_allocator]`): after one warm-up
   read, `DmaBuf::read` allocates 0 times; `decode_volts` allocates exactly one Vec per enabled channel per frame.
   Deterministic, runs in the normal `cargo test`. Catches a merge that brings back `read_dma` or `deinterleave` +
   `volts` in the batch loop.
2. Same idea in `ui`: `board::dho::hw::decode` on a 1 ch frame allocates one Vec. Catches a revert of `decode` in
   `ui/src/board/dho/hw.rs` (upstream edits that function).
3. Behaviour test for the early trigger end on the generic path: `acquire_batch_until` on `seg::sim::Engine` with no
   trigger and `stop` returning true after 30 ms returns `NEW_SETTINGS` within 50 ms. Keeps upstream's f1b2d6c
   behaviour alive in `plain_step`.
4. Keep `bench_batch_decode` as a sync checklist step, and add a ratio assert (fused >= 1.3x two-pass at 2 and 4
   ch) behind `--ignored`. Ratios are stable across hosts; absolute times are not.
5. Existing equivalence tests already guard correctness: `fused_decode_equals_deinterleave_and_volts`,
   `unpad_in_place_equals_unpad`, `prearm_writes_the_same_sequence_as_the_plain_batch_loop`,
   `dma_buf_is_reused_aligned_and_exact_with_short_reads`.

## Status (2026-11-13, branch perf/hal)

- `decode_volts` `on: Vec`: done, fixed slot table. Then f32 decode (perf-plan item 8), see perf-plan.md Status.
- `execute_into`: removed; `execute_until` kept as wrapper.
- Image build hook: done as `boot/unit-hook.sh` + `boot/unit-hook-image.sh` (not `rootfs/unit/hook.sh`: the hook is
  build logic, the unit directory stays data). Output identical on stub inputs.
- Guards 1, 3, 4: `hal/tests/alloc_batch.rs`, `hal/tests/early_trigger_end.rs`, `hal/tests/perf_ratio.rs`, run by
  `tools/perf-guard.sh` (docs/fork/sync-checklist.md). Guard 2 (ui `decode`) is the ui's.
