# Sinc interpolation in the play chain (0x1130..0x1144)

Status: specification from static analysis. Short form. Recommendation: low priority.

## 1. What the stock firmware does

Four interpolator units. Control words (`DevAcquireSPU_SetSinc(enable, load, unit, factor)`):

| unit | control register | value |
|---|---|---|
| 0 main | 0x1130 | enable << 31, factor f in [23:16], [15:8] and [7:0] |
| 1 zoom | 0x1134 | same |
| 2 | 0x1138 | same, always f = 200 |
| 3 | 0x1138 | enable << 31 with the fixed pattern for f = 200 |

Input length: 0x1118 (main) / 0x111c (zoom) = window points + 40 (LA on: half the points + 40)
(`DevAcquireSPU_SetSincDinLen`). The x2 interpolation flags are 0x1154 bits 8 and 9.

Coefficient load (when `load` is set): factors 2, 4, 5, 8, 10, 16, 20, 25, 32, 40, 50, 80, 100, 160, 200; 16 taps per
phase, f phases, Q15, DC gain f. For each tap row r = 0..15: write 0x1140 = 1 << r (one-hot row select), then f writes
to 0x1144 = bit 28 (write strobe) | unit << 24 | phase << 16 | coefficient [15:0]. The phase field starts at r mod f
and increments modulo f (unit 3: starts at 0). Coefficient index = row x f + phase; the table stores the full
symmetric response (index i >= 8f reads 16f - 1 - i).

Reference table (do not copy into the fork):
`SinCoef.csv`.

When:

- `Drv_Init`: units 2 and 3 enabled and loaded with f = 200 once.
- `ConfigHorizontal`: main and zoom enabled and loaded with their sinc factor when it is above 1; unit 2 enabled when
  either main or zoom interpolates (no reload). The sinc factor comes from the screen points needed vs memory points
  available, so it is above 1 only at the fastest time bases.
- Export (`DrvWaveform_ExportInit`) and raw reads: all off with factor 1, linear interpolation off.

## 2. What OOS does today

- `hal/src/dho/scu.rs:140` `passthrough`: 0x1130/0x1134 = 0x00010101 (off) before every read. OOS always reads raw.
- Software interpolation on the host: `dsp/src/interp.rs` (linear, Lanczos), used by `ui/src/board/record.rs`.
- No 800 ps/div setting (minimum 1 ns/div); unrelated to the sinc unit.

## 3. Proposed change

None for now. At time bases where sinc applies, the raw window is at most a few hundred points; software
interpolation of that costs microseconds. Hardware sinc would add more transferred points, not fewer.

If ever needed (e.g. a display-only fast path at 1..5 ns/div):

- `hal/src/dho/sinc.rs`: `control(unit, en, f)`, `load(unit, f, coefs)`, with OOS-designed coefficients (windowed sinc,
  same Q15 format), not the stock table.
- Keep `passthrough` for all raw reads.

## 4. Host test plan

- Write-sequence test of `load(0, 4, ..)`: 16 row selects, 64 coefficient writes, field layout.
- Generator test: DC gain f, symmetry, passband ripple.
- Outside the fork (analysis repo): compare the generated responses with the stock table numerically.

## 5. Hardware test checklist

1. f = 4 at 5 ns/div on a 100 MHz sine: interpolated output smooth, amplitude flat.
2. Unit 2/3 role: toggle 0x1138 with main sinc on and look for a change in trigger-point alignment.

## 6. Confidence and open questions

Register layout high, purpose of units 2 and 3 unknown (candidates: trigger interpolation or the zoom path).
