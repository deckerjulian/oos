# Transfer efficiency: batching, frame size, DMA, polling

Status: specification from static analysis of the stock driver library, the stock start scripts, the OOS sources and
`lspci -vvv` of the unit (2026-11-12). No measurement on the unit yet.

## 1. What the stock firmware does

### 1.1 Live loop (`CDrvScope::run`, `ReadNormTrace`, `RequestNormTrace`, `RunScope`)

Per cycle, in RUN:

1. Request: read 0x4080 (bit 30 = done), 0x4010, 0x4014; count = `0x4010 - 0x4014 + 1`. Not done: sleep 9 ms
   (SINGLE 10 ms) and poll again.
2. Stop sequence (`StopScope`: poll 0x4034 bit 13 up to 10 x 1 ms, then the writes with `usleep(1)` between groups).
3. Type-4 play of the batch `first .. first + count - 1` of the current ring slot.
4. One blocking `read()` on `/dev/xdma0_c2h_0` of `count * (16 + 2 * ratio * len)` bytes (+ zoom part). Header on.
5. De-interleave into the waveform cache.
6. Re-arm immediately (sleep 0) on the other ring slot.

Ring slots: slot 0 starts at frame 0, slot 1 at frame `max_frames` (half the ring). The slots alternate per batch, so
the previous batch stays intact in FPGA memory for STOP and history plays.

### 1.2 Frames per batch (`CDrvParam::PrepareAcquireParam`)

Documented in OOS `horizontal-config.md` 1.5. Summary: about 40 ms of acquisition time per batch, at most
`2000 / ratio` frames, at most about 2 million points per batch, halved for DHO800/900, halved again with zoom, at most
half the frame ring.

### 1.3 Frame size

- TX length (0x11f0) is the display trace length rounded up to a multiple of 8 points (`DevAcquireSpu_SetTxInfo(0,
  (len + 7) & ~7)`). The FPGA pads frames in the stream to 8 bytes; with the rounding there is never padding.
- The display trace is compressed in the FPGA (0x1170, min/max pairs) to the screen-related length, for example
  625,000 points at 200 us/div with 2.5 M memory (OOS `horizontal-config.md`). Raw memory is read only for export and
  the 1 M RMS path.
- 16-byte header per frame (0x1010 bit 31 = 0). Header words 5..7 carry a 48-bit value kept with each waveform (see
  `03-segmented-timestamps.md` 1.6).

### 1.4 DMA driver

- `/rigol/shell/load_pcie.sh` loads `/rigol/driver/xdma.ko` (Xilinx reference v2019.2.51) without parameters:
  `poll_mode = 0`, interrupt completion. `lspci` shows MSI disabled on the endpoint and legacy INTx (IRQ 241) in use.
- Only `/dev/xdma0_bypass` (register mmap) and `/dev/xdma0_c2h_0` (one blocking read per batch) are used. No events
  device, no H2C.
- PCIe: Gen2 x2. Endpoint MaxPayload capability 512 B, root port capability 256 B, both set to 128 B. MaxReadReq 512 B
  (irrelevant for C2H, which is posted writes).

## 2. What OOS does today

| item | where | behaviour |
|---|---|---|
| batch size | `ui/src/board/dho/hw.rs:299` `batch_plan` | at most 4 MB per read, `max_frames`, `1000 / ratio` |
| deep records | `hw.rs:301` | FPGA compression only above 1 M points, to about 625 k points per frame |
| batch cycle | `hal/src/dho/seq.rs:498` `acquire_batch` | arm, poll 0x4080 every 500 us, then wait up to 25 ms (`hw.rs:680`) polling 0x4010 every 200 us for more frames, then play and read |
| read | `hal/src/dho/bus.rs:27` `read_dma` | new zero-filled `Vec` of `bytes + 8 KiB` per read, page-aligned offset, then `copy_within` to the front and `truncate` |
| padding | `seq.rs:385`, `seq.rs:408` | window lengths are not multiples of 8, so every frame is padded in the stream and `unpad` moves every frame again |
| decode | `hw.rs:261` `decode` | `deinterleave` to `Vec<u16>` per channel, then `volts` to `Vec<f32>` per channel |
| order | `hw.rs:680-720` | re-arm only after decode and queueing: decode time is dead time |
| header | `hw.rs:144` | off unless `OOS_FRAME_HEADER` is set |
| driver | `rootfs/board/dho/bin/oos-fpga-up:21` | `insmod xdma.ko poll_mode=1`: the completion thread busy-polls the write-back word during every transfer |
| ring | `hz.rs:195` | one slot (always first frame 0) |

## 3. Proposed changes, in order of value per effort

### 3.1 Interrupt completion instead of poll mode

- Load `xdma.ko` with `poll_mode=0` (stock behaviour). If legacy INTx does not reach the CPU under the OOS kernel, try
  `interrupt_mode=1` (MSI; the endpoint advertises one MSI vector).
- Keep `poll_mode=1` as a fallback in `oos-fpga-up` when a test read times out.
- Gain: the poll loop occupies a core for the whole DMA time. 4 MB at about 600 MB/s is about 7 ms per batch, so at
  20 batches/s about 14 % of one A72 core plus scheduler churn. With interrupts this is close to zero.
- Risk: interrupt latency adds tens of microseconds per batch. Negligible against 7 ms.

### 3.2 No padding: round the window to the frame alignment

- Choose window lengths so that `2 * ratio * n` is a multiple of 8 bytes (n multiple of 4 / ratio, use 8 for all
  ratios to match the stock TX rounding). `window()` in `seq.rs` widens n by up to 7 points.
- Then `frame_stride(frame) == frame` and `unpad` returns at once. Keep `unpad` for safety.
- Gain: one memmove of the whole batch (up to 4 MB) per read.

### 3.3 Reuse the DMA buffer, no zero fill, no front copy

- `bus::DmaBuf`: one page-aligned buffer allocated once (largest batch), owned by the acquisition thread.
- `read_dma_into(&mut DmaBuf, bytes) -> &[u8]`: reads at offset 0 of the aligned region, no `copy_within`.
- Gain: a 4 MB memset and a 4 MB memmove per batch, plus allocator traffic. On the RK3399 that is in the order of
  2 to 4 ms of CPU per 4 MB batch.

### 3.4 Fused decode

- One pass from the DMA buffer to per-channel `f32` (or keep `u16` for the fast display path and convert lazily).
  Avoids the intermediate `Vec<u16>` per channel.
- Gain: one write and one read pass over the samples, plus allocations.

### 3.5 Re-arm before decode

- After the read returns, issue the next arm on the other ring slot (slot 1 = `max_frames`), then decode and queue
  while the FPGA records.
- Requires: per-slot `first` in `seq::State`; the play of slot s uses `first = s * max_frames`. This is the stock
  ping-pong of 1.1.
- Gain: decode time (milliseconds per batch) leaves the dead time. Waveform rate rises at short time bases where decode
  dominates.
- Care: SINGLE and STOP must not re-arm; segmented mode (`03`) uses the whole ring and no ping-pong.

### 3.6 Display-length compression for live view

- When no consumer needs the raw record (no FFT, no decode, no measurement on full resolution, no export), set the
  compression k so that the frame is a small multiple of the screen width (e.g. 4 x 1000 columns = 4,000 points as
  min/max pairs). Fetch the raw record on STOP or every Nth batch for measurements. Details in
  `04-peak-decimation.md`.
- Gain: at 1 M points per frame, 250 x less transfer and conversion per frame, and far more frames per batch.

### 3.7 MaxPayload 256 B

- Set the endpoint and the root port to 256 B (root port limit) at boot, before `xdma` loads (`pci=pcie_bus_perf` on
  the kernel command line, or `setpci` on both devices in `oos-fpga-up`).
- Gain: TLP efficiency for 128 B payloads with about 24 B overhead is about 84 %; with 256 B about 91 %. About 8 % more
  usable link throughput.
- Risk: the RK3399 root complex must handle 256 B completions and writes cleanly; test with a long read and data
  check.

### 3.8 Smarter waiting

- Done poll: sleep until the expected record time (`memsel / rate`) before the first poll instead of fixed 500 us
  steps at slow time bases.
- Batch fill wait: replace the fixed 25 ms by the expected time for `frames` triggers from the measured trigger rate,
  capped at 25 ms. Lower latency at low trigger rates.
- No FPGA user interrupt for "done" is known (`xdma0_events_*` unused in stock), so polling stays.

## 4. Host test plan

- `seq.rs`: window rounding test: for all ratios and a set of n, `frame_bytes % 8 == 0` and `frame_stride == frame`.
  The existing verified read-out fixture (`hal/testdata/second-frame-writes.txt`, n = 4000) must stay unchanged.
- `bus.rs`: `read_dma_into` with a fake reader that returns short reads of random sizes: data identical, buffer
  reused across calls (same pointer), alignment 4096.
- Decode equivalence: fused decode equals `deinterleave` + `volts` on random data for masks 1, 3, 5, 0xf.
- Ping-pong: step list of two consecutive batches arms slot 0 then slot 1 (0x4010/0x4014 = 0, then `max_frames`), and
  the plays use the matching first frame.
- Benchmark (ignored test or `cargo bench` on the host and on the unit): decode of a 4 MB batch, old vs new path.
- `oos-fpga-up`: shell test in `tools/` that the fallback to `poll_mode=1` triggers on a failing probe read (mock
  `oos-hw`).

## 5. Hardware test checklist

1. Baseline with stock OOS: waveforms/s at 5 ns, 100 ns, 1 us, 10 us, 200 us/div for 1, 2, 4 channels; CPU load per
   core (`top -d 1`); per-batch timing from the `acquire_batch` log.
2. `poll_mode=0`: reads complete, no timeouts over 10 minutes; CPU of the `cmpl_status_th` kernel thread drops.
   `/proc/interrupts` shows the xdma IRQ counting.
3. If INTx fails: `interrupt_mode=1`, check MSI enabled in `lspci`.
4. Padding removal: a 10 MHz sine at 100 ns/div keeps the same phase in every frame of a batch (the original padding
   symptom).
5. Ping-pong re-arm: waveform rate at 100 ns/div before and after; no corrupted frames.
6. MPS 256: `lspci` shows 256 on both ends; a 64 MB read sequence (repeated plays) has no errors; throughput gain.
7. Display compression: frames per second and CPU at 1 M depth, 200 us/div, with and without.
8. Dead time via segmented timestamps (`03`) as an independent check.

## 6. Confidence and open questions

Confidence: high for 3.1 to 3.5 (host-side code and the stock driver behaviour are clear). Medium for 3.6 (compression
verified at k = 4 only) and 3.7 (root complex behaviour).

1. Why did OOS choose `poll_mode=1`? Possibly the interrupt did not arrive under the 6.12 kernel. Check the commit
   history of `oos-fpga-up` and the PCIe DT node (`interrupt-map`) before switching.
2. Usable C2H throughput on this link with 128 B and 256 B payloads.
3. Does the FPGA accept a new arm on slot 1 while slot 0 data is no longer needed, without a play of slot 0 first?
   The stock loop does exactly this, so expected yes.

## 7. Implementation status (2026-11-12)

Fork commits c63b631, 9833321, 5ddc0c6. Host-tested only. Nothing ran on the unit. Hardware steps: `docs/fork/unit-dho804.md`,
section "Transfer efficiency".

| item | state | where | default |
|---|---|---|---|
| 3.3 buffer reuse | done | `bus::DmaBuf` (page-aligned region of its own, grows to the largest read, no zero fill, no front move); `seq::execute_into_until`, `seq::unpad_in`; `acquire_batch` reads into a `DmaBuf` the oos-ui thread keeps. `read_dma` is a one-off `DmaBuf` with the old semantics | on |
| 3.2 no padding | not changed, not needed | survey test `hw::tests::live_batches_need_no_stream_padding`: 8250 oos-ui set-ups (all channel sets, time bases below roll, depths, LA on/off) give frames of a multiple of 8 bytes, since `screen_margin` rounds Auto depths to 8 and the fixed depths and the compressed length are multiples of 8. `unpad` returns at once for them. Widening explicit windows (`oos-fpgad --n`) would hand callers more points than asked for and change the verified fixture sequences, for a diagnostic path | - |
| 3.5 re-arm before decode | done, opt-in | `seq::prearm`, `seq::disarm`, `State::arm_only_steps`, `wait_steps`, `disarm_steps`, `acquire_batch(.., armed, ..)`; oos-ui with `OOS_PREARM` set | off |
| 3.1 interrupt completion | configurable | `xdma.params` in `/etc/oos/system.conf` or `/oos/system.conf` (`oos-fpgad` `xdma_params`, `oos-fpga-up`): only `poll_mode=0\|1`, `interrupt_mode=0..3` | `poll_mode=1` |
| 3.7 MaxPayload 256 | build opt-in | `OOS_BOOTARGS_EXTRA="pci=pcie_bus_perf"` in a unit's `build.env` (commented out for the DHO804) | 128 B |
| 3.4 fused decode | done | `seq::decode_volts`, used by `hw::decode` | on |
| 3.6, 3.8 | not started | 3.6 belongs to `04-peak-decimation.md` | - |

Decisions against the proposal:

- 3.5 re-arms slot 0, not the other half of the ring. The batch is already in host memory when the next arm goes out, so
  the record in FPGA memory is not needed any more, and the arm keeps the write sequence that is verified on the unit
  (open question 3 does not arise). The ping-pong slots become relevant with history plays and `03`. The pre-armed loop
  writes exactly what the plain loop writes, in the same order (test `prearm_writes_the_same_sequence_as_the_plain_batch_loop`):
  the arm only moves before the decode, after the LA read of the same record.
- 3.5 is off by default. OOS's hardware thread never left an acquisition armed while it configured, ran self-cal, the
  generator or roll. With an arm ahead, those paths first run `disarm` (the stock StopScope: wait up to 10 ms for
  0x4034 bit 13, then the stop sequence every arm starts with). Whether 0x4034 shows STOPPED afterwards, so that the
  configure poll does not wait its 30 ms, is not verified. SINGLE: a batch armed before a SINGLE press is dropped and
  armed again (the arm-time stamp is compared).
- 3.7 uses the kernel (`pci=pcie_bus_perf` sets the root port to its 256 B and the endpoint to min(512, 256) when
  `pcierockchiphost` enumerates) instead of `setpci` from user space: no tool in the image, and the kernel keeps both
  ends consistent. Side effect: the endpoint's MaxReadReq drops from 512 to 256 B (H2C and descriptor fetches only).
- 3.1 has no automatic fallback to polling. A missing interrupt shows as a C2H read that blocks for `c2h_timeout`
  (10 s) and fails; the setting is removed by hand. Open question 1 stays open: the fork's history starts at the first
  public version, which already had `poll_mode=1`.

Host benchmark, `cargo test --release --lib -- --ignored --nocapture bench_batch` in `hal` (Apple M-series, 4 MB batch
from memory, mean of 20). The RK3399 has far less memory bandwidth; repeat on the unit before drawing conclusions.

| batch | read old -> new | decode old -> new |
|---|---|---|
| 1 ch, 1420 x 2816 B | 0.09 -> 0.07 ms | 1.50 -> 1.41 ms |
| 1 ch, 1600 x 2500 B (padded) | 0.16 -> 0.13 ms | 1.44 -> 1.44 ms |
| 1 ch, 20 x 200 kB | 0.08 -> 0.06 ms | 1.36 -> 1.31 ms |
| 2 ch, 1000 x 4000 B | 0.08 -> 0.06 ms | 2.30 -> 1.43 ms |
| 4 ch, 500 x 8000 B | 0.07 -> 0.05 ms | 2.30 -> 1.40 ms |

"read" includes the copy from the fake stream in both columns. macOS hands out zeroed pages lazily, so the zero fill the
old read path pays on Linux barely shows here.
