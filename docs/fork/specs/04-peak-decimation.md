# Peak detect and decimation paths in the SPU

Status: specification from static analysis plus OOS's own unit results. Short form.

## 1. What the stock firmware does

Two separate places reduce data:

Acquisition side (what is stored in memory):

| register | value | function |
|---|---|---|
| 0x10d0 | above 62.5 MSa/s: bit 15 set, [7:0] = max_rate / rate; else 0 | `DevAcquireSPU_SetSampleRate` |
| 0x10d8 | above 62.5 MSa/s: 1; else 62.5 M / rate | |
| 0x10d4 | 0 | |
| 0x1004 bit 2 | rate < max_rate / 8 ("AvgSinc mode", see `02-hires-fir.md`) | `DevAcquireSpu_SetAvgSincMode` |
| 0x1004 bit 1 | peak detect (mode 1), forced off at the maximum rate | `DevAcquireSpu_SetAcqMode` |
| 0x10dc | 1 when anti-aliasing is off; never written otherwise | `DevAcquireSPU_SetAcqAntiAlias` |

Play side (what is transferred, `ConfigHorizontal`):

| register | layout | use |
|---|---|---|
| 0x1170 / 0x1174 | bit 31 enable, bit 30 always 1, bits 29:28 kept from the shadow, [27:0] factor k | main / zoom compression to min/max pairs (`DevAcquireSPU_SetCompress`) |
| 0x1184 | same layout | compression feeding the mask unit (`06`) |
| 0x1298 / 0x129c | same idea | LA main / zoom compression (`SetLaCompress`) |
| 0x11f0 | TX length, rounded up to 8 | |

The stock live path always compresses the display trace to the screen-related length. Raw memory is read only for
export and the 1 M RMS path. The anti-alias register is written only when anti-aliasing is switched off, so after one
"off" it stays 1 until a power cycle in the stock code (likely a stock bug, or the FPGA resets it on reconfiguration).

## 2. What OOS does today

- Peak: `hal/src/dho/acq.rs:98` sets bit 1, normal at the maximum rate (verified on the unit 2026-11-07).
- Compression: only for records above 1 M points, to about 625 k points (`ui/src/board/dho/hw.rs:301`); otherwise the
  raw window is transferred.
- Display reduction in software: `ui/src/board/record.rs` builds min/max pairs per screen column from the raw record.
- 0x10dc is never written.

## 3. Proposed change

- `hw.rs` `batch_plan`: live compression target `L` points per frame (default 4 x screen columns). k = ceil(memsel / L)
  whenever k >= 2 and no raw consumer is active (FFT, protocol decode, eye, full-resolution measurements, export,
  zoom beyond the compressed resolution). Raw record on STOP and every Nth batch when measurements run.
- `Frame.envelope = true` (already set for k > 1) lets `record.rs` draw the pairs directly.
- Peak plus compression: the FPGA pairs of peak data remain a valid envelope; allow both.
- Anti-aliasing setting in the acquire menu: write 0x10dc = 0 (on) or 1 (off) explicitly in `acq::configure_la`.
- CPU: at 1 M points per frame and L = 4,000, transfer and conversion drop by 250x per frame; the waveform rate is then
  bound by triggers and the play engine, not the host.

## 4. Host test plan

- `batch_plan` table test: k and frame bytes for depths 10 k .. 125 M, with and without raw consumers.
- `record.rs`: an envelope frame of L pairs renders the same column min/max as the raw record it came from (synthetic
  data, compression modelled in the test).
- `acq.rs`: 0x10dc written in both states.

## 5. Hardware test checklist

1. 1 M points, 200 us/div, 1 kHz square plus a 20 ns glitch every 100 ms: the glitch shows with k = 250.
2. Order of min and max in a pair (rising vs falling edges) and behaviour of bits 29:28 of 0x1170.
3. Peak mode plus compression: envelope of a modulated sine.
4. 0x10dc on/off at 1 kSa/s with a 1.001 MHz sine: aliased tone vs noise-like smear.
5. Waveform rate and CPU with L = 4,000 vs raw.

## 6. Confidence and open questions

Medium. Compression is verified at k = 4. Larger k, pair order and 0x10dc semantics are not.

1. Meaning of 0x1170 bits 29:28.
2. Does 0x10dc randomise the decimation phase (classic anti-alias dither)?
3. Maximum k (28-bit field, but the FPGA may limit it).
