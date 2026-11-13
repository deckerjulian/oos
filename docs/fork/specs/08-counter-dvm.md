# Frequency counter and DVM in hardware (0x2200..0x2258)

Status: specification from static analysis. Short form.

Upstream 0.1.9 implements the counter (0x2200..0x2240) in `hal/src/dho/counter.rs`, SCPI `:COUNter`,
docs/protocols/counter.md. The fork takes it and drops its own counter work. Open in this spec: the DVM block
(section 1.2, 0x2244..0x2258), which upstream does not write.

## 1. What the stock firmware does

### 1.1 Counter registers

| register | bits | content |
|---|---|---|
| 0x2200, 0x2204 | | comparator thresholds, 16-bit output codes around 0x8000 (init 0x8000; run 0x8080) |
| 0x2208 | 0 | enable |
| 0x220c | 3:0 | channel enable, one-hot (CH1 = 1 .. CH4 = 8) |
| 0x2210 | 0 | run (0 during clear) |
| 0x2210 | 2 | clear (1 during clear) |
| 0x2210 | 1 | totalizer mode (set only for mode 2) |
| 0x2210 | 4 | mode flag (always 0 in stock) |
| 0x2210 | 9:5 | source: 1..4 channels, 16..31 LA lines D0..D15 |
| 0x2210 | 3 | gate flag (always 0 in stock) |
| 0x2210 | 14:10 | gate source (= source) |
| 0x2214 | | gate time in 6.4 ns ticks (gate_fs / 6.4e6) |
| 0x2218 | 0 | read latch (write 1) |
| 0x221c / 0x2220 | | first-edge offset, 64-bit, bit 63 = valid |
| 0x2224 / 0x2228 | | last-edge offset, 64-bit |
| 0x222c / 0x2230 | | edge count n, 64-bit |
| 0x2234 / 0x2238 | | period count, 64-bit, bit 63 = valid |
| 0x223c / 0x2240 | | event count (totalizer), 64-bit |

Sequences:

- Init (`DrvCounter_Init`): 0x2210 run = 1, clear = 0; thresholds 0x8000/0x8000; enable 1; channel CH1.
- Run (`DrvCounter_Run`): thresholds 0x8080/0x8080, enable, one-hot channel; off = all zero. For LA sources the
  stock code leaves 0x2208 and 0x220c at 0 (the analog comparator path is off; the LA line is selected only in 0x2210).
- Mode (`DrvCounter_SetMode`): mode 0 frequency, 1 period, 2 totalizer; 0x2210 [9:5] = source, bit 1 = mode == 2.
- Gate (`DrvCounter_SetGateTime`): 0x2210 [14:10] = source, 0x2214 = gate / 6.4 ns. Default gate 1 s.
- Clear (`DrvCounter_ClearResult`): 0x2210 (run 0, clear 1), then (run 1, clear 0).
- Read (`DrvCounter_GetValue`): latch, check valid bits, read the words.
  Period in fs = `((gate_fs / 800,000 - first - last) / (n - 1)) * 800,000`, so offsets count 0.8 ns (one ADC sample
  at 1.25 GSa/s). Frequency = 1e15 / period_fs. Fallback when n < 2: period count x 0.8 ns. Results below 100 fs are
  rejected. Totalizer returns the event count.

### 1.2 DVM registers

| register | bits | content |
|---|---|---|
| 0x2244 | 0 | restart pulse: written 1, then 0 with the new fields (not a level enable; the stock shadow clears it) |
| 0x2244 | 5:1 | log2 of the sample count (stock default 28: 2^28 samples, about 0.21 s at 1.25 GSa/s) |
| 0x2244 | 9:8 | lane: 0 = the source's own lane; 2 = the other lane pair in 2-channel mode |
| 0x2248 | 0 | read latch: write 1 then 0 |
| 0x224c, 0x2250 [31:16] | | sum, 48 bits (0x224c low 32, 0x2250 upper half = bits 47:32) |
| 0x2254, 0x2258 | | second result (stock calls it "Drms") |

Mean = sum / 2^exp. Second result / 2^exp. `ConfigAcquire` rewrites the DVM word on every configuration, because the
lane depends on the channel mode.

## 2. What OOS does today

- Counter and DVM in software from the transferred records: `ui/src/ui/tools.rs:117` (`counter_update`) and
  `tools.rs:146` (`dvm_value`).
- `hal`: the DVM block is not written (`acq.rs` header comment), it keeps the stock state.

Gap: resolution and coverage depend on the record length and on what is transferred. A 1 s hardware gate with 0.8 ns
edge interpolation gives about 1e-9 relative resolution independent of the time base.

## 3. Proposed change

- `hal/src/dho/cnt.rs`: `init`, `run(src, on)`, `mode(m)`, `gate(fs)`, `clear`, and `read(bar) -> CounterResult`.
- `hal/src/dho/dvm.rs`: `config(exp, lane)`, `read(bar) -> (mean_code, second)`; `seq` writes `config` with the lane of
  the current channel mode.
- Code to volts via the same mapping as the read path (`seq::volts`), if 5.3 shows the DVM sees output codes.
- UI: hardware values when the source is a channel or LA line; software as fallback (math sources).
- CPU gain small; accuracy gain large for the counter.

## 4. Host test plan

- Write-sequence tests for init, run, mode, gate, clear (orders as in 1.1).
- Result computation: table test with synthetic register values (n, offsets, gate) to frequency and period; invalid
  bit handling; n < 2 fallback.
- DVM: sum assembly from the odd 0x2250 layout, exponent scaling.

## 5. Hardware test checklist

1. Counter on the 1 kHz calibrator: 1 s gate, compare with a known reference (GPSDO or counter).
2. Thresholds 0x8080 vs 0x8000 vs larger hysteresis on a noisy slow edge.
3. Period mode and totalizer on a burst.
4. LA source D0.
5. DVM on a DC level: mean in codes vs `seq::volts`; second result vs a sine of known RMS (is it RMS, mean square, or
   AC RMS?).
6. 0x221c after a UART trigger set-up (`07`): counter offset still valid?

Implementation and unit procedure: `docs/fork/dvm.md` (opt-in `dvm.hw = on`).

## 6. Confidence and open questions

Counter: medium-high (result formula explicit). DVM: medium (second result, 0x2250 layout and units unknown).
