# FPGA offload specifications (fork work stream B)

One file per feature. Each file: what the stock firmware does (register order, values, timing, stock function names
as references), what OOS does today (file:line), the proposed change, a host test plan, a hardware checklist, and
confidence with open questions.

Sources: static analysis of the stock driver library `libscope-auklet.so` (firmware 00.01.05.00.01) in the private
analysis repo, the OOS sources and protocol docs of this
fork, and `lspci -vvv` of the unit. Nothing in these specs ran on hardware. No stock code or coefficient table is copied
here; tables are referenced by their location in the analysis repo.

## Ranked list

Rank = expected gain x feasibility x confidence, for this unit (DHO804 hardware, no generator, no LA).

| rank | file | feature | gain | feasibility | confidence |
|---|---|---|---|---|---|
| 1 | [10-transfer-efficiency.md](10-transfer-efficiency.md) | interrupt DMA completion, no padding, buffer reuse, fused decode, re-arm before decode, MaxPayload 256 | high: frees a core during DMA, removes 2..3 memory passes per batch, shorter dead time | high, host-side only | high |
| 2 | [03-segmented-timestamps.md](03-segmented-timestamps.md) | hardware segmented memory with trigger timestamps | high: us-level dead time, tens of thousands of segments, 250 ps stamps | high, same record engine OOS already drives | medium (timestamp registers unverified) |
| 3 | [04-peak-decimation.md](04-peak-decimation.md) | display-length FPGA compression, anti-alias register | high: up to 250x less transfer per frame at 1 M points | high (compression verified at k = 4) | medium |
| 4 | [01-hw-averaging.md](01-hw-averaging.md) | hardware averaging | medium-high: all triggers averaged, possibly one frame read per batch | high | medium (average semantics unknown) |
| 5 | [08-counter-dvm.md](08-counter-dvm.md) | hardware counter and DVM (counter: upstream 0.1.9; DVM open) | accuracy gain (1 s gate, 0.8 ns interpolation); small CPU gain | high | medium-high counter, medium DVM |
| 6 | [07-serial-triggers.md](07-serial-triggers.md) | UART, I2C, SPI hardware triggers | functional: real protocol trigger in Normal/Single | medium: field meanings need a hardware matrix | bits high, semantics low-medium |
| 7 | [06-mask-unit.md](06-mask-unit.md) | pass/fail mask in the FPGA | medium: coverage and CPU | medium | low-medium |
| 8 | [02-hires-fir.md](02-hires-fir.md) | high resolution, SPU FIR | quality, little CPU | depends on measurements | low: stock does not use the FIR for high-res |
| 9 | [05-sinc-play-chain.md](05-sinc-play-chain.md) | sinc interpolation | none for OOS's raw reads | high | high layout, unknown units 2/3 |
| 10 | [09-bode-iq.md](09-bode-iq.md) | Bode I/Q demodulator | blocked: needs the internal generator (mod C) | low on this unit | high layout, unknown reference |

## Findings that change the plan

- 0x10a0..0x10e4 are not segment registers. Segmented memory uses 0x400c, 0x4010..0x4018, 0x4068, 0x406c and the
  timestamps 0x40b0..0x40c4. 0x10a0 is the FIR output shift.
- High resolution in the stock firmware writes no SPU register of its own. The stored "high-res bits" setting and the
  filter enable flags are never read by the driver. The FIR (0x1098/0x109c) is loaded only from two setting fields no
  code writes; it is dormant. The coefficient tables (`filterCoef`, `spu_1Xbit_fir_coef`) are leftovers or for another
  build. 0x1004 bit 2 ("AvgSinc", set at decimation 8 and up) is the candidate for the real high-res effect.
- Stock loads `xdma.ko` with interrupts; OOS uses `poll_mode=1`.
- Header words 5..7 carry a 48-bit per-waveform value (candidate trigger timestamp).
- Average needs its own segment layout and an extra window word (0x1110); the averaging region takes 5,242,880 bursts
  at the top of the frame memory.

## Suggested order of implementation

1. Transfer efficiency 3.1 to 3.5 (no FPGA unknowns; gives the baseline for everything else).
2. Display compression (04) and segmented memory (03), sharing the frame-ring work (ping-pong slots, full-ring record).
3. Averaging (01) after the hardware questions 5.2 to 5.4 of its spec are answered.
4. DVM (08 section 1.2). The counter is upstream's since 0.1.9.
5. Serial triggers (07) and mask (06) after a hardware session to map field meanings.
6. FIR / high-res (02) only after measurement 5.1 of its spec.

## Test fixtures

All specs use the existing OOS mechanisms: write-list comparisons against files in `hal/testdata/`, the Python models in
`tools/` (`hzmodel.py`, `tpumodel.py`, `tpu_fixture.py`) extended per feature, and the `bus::Recorder` test bus for
register-read scripts. New models proposed: `tools/firmodel.py`, `tools/maskmodel.py`.
