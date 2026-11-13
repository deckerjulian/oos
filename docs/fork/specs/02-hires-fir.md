# High resolution and the SPU FIR (0x1098 / 0x109c / 0x10a0)

Status: specification from static analysis. The main result is negative: the stock driver does not use the FIR for
high resolution. The FIR path is real and loadable, but its place in the data path must be measured before OOS relies
on it.

## 1. What the stock firmware does

### 1.1 High resolution has no SPU register of its own

- Acquire mode 3 is high resolution. `DevAcquireSpu_SetAcqMode(3)` clears 0x1004 bits 1, 3 and 12 and sets nothing,
  so register-wise it equals normal mode.
- The UI setting "high-res bits" (`CApiHorizontal::ApiHorizontal_SetHighResBit`) is accepted only in mode 3 and only
  for the values 6 and 8. It is stored (`CAcquire::setHighResBit`) and triggers a reconfiguration. No function in the
  library reads it back: `CAcquire::getHighResBit` has no caller. The same holds for the filter enable flags
  (`getFilter1En`, `getFilter2En`: no callers).
- The UI shows a bandwidth for the high-res setting (`ApiHorizontal_GetHighResBw`): a fixed fraction of the actual
  sample rate per key (keys 1, 2, 4, 6, 8; fractions 1/2, 1/4, 1/5, 1/20 and 1/40 of the rate for DHO800/900). This
  is display text only.
- The only mode-3 effect in the driver: the fine sample delay of the play chain is moved by half a sample period
  (`ConfigHorizontal` and `ConfigueDelay`: delay term `-T/2` when mode is 3 or the second filter is on). That is the
  group-delay correction of a two-tap average or a symmetric even-length filter.

### 1.2 0x1004 bit 2

`DevAcquireSpu_SetAvgSincMode(rate < max_rate / 8)` sets 0x1004 bit 2 in every mode. Name: "AvgSinc mode". OOS already
writes it the same way (`hal/src/dho/acq.rs:67`). Hypothesis: it selects the decimation filter of the acquisition path (averaging
instead of sample picking) at decimation 8 and up. If so, normal mode already behaves like a boxcar high resolution
at slow time bases, and mode 3 needs nothing else. This is the first thing to measure (5.1).

### 1.3 The second filter (FIR)

Registers:

| register | bits | meaning |
|---|---|---|
| 0x1098 | 1:0 | control: 1 = bypass, 2 = coefficient load, 0 = filter running. Reads back; `GetFilterEn` reports enabled when bit 0 is 0 |
| 0x109c | 22:0 | coefficient, 23-bit two's complement, DC gain 2^22 for the stock sets |
| 0x109c | 29:23 | tap index |
| 0x109c | 31:30 | lane (polyphase branch) 0..3 |
| 0x10a0 | | output shift, "throw". 16 by default; 8 only with the filter on in average mode. Written after every 0x1098 change |

Load sequence (`DevAcquireSPU_configDspCoef1/2/3`, selected by the interleave ratio 1/2/4):

1. 0x1098 = shadow with bits 1:0 = 2. 0x10a0 = 16.
2. For each of the 4 lanes, for each tap `t` of 16 (`DevConfig_GetFilter2Tap` = 16 for this series): write 0x109c with
   lane, tap and coefficient. Ratio 4: every lane gets the same coefficient per tap (4 writes per tap). Ratio 2: lanes
   0 and 2 take the even coefficients, lanes 1 and 3 the odd ones, in two halves. Ratio 1: the four lanes take every
   fourth coefficient with a lane-dependent start, in quarter blocks. This is the polyphase split of the 4-lane SPU
   data path (4 samples per clock).
3. 0x1098 bits 1:0 = 0 (run). 0x10a0 = 16.

Bypass: 0x1098 bits 1:0 = 1, then 0x10a0 = 16.

Callers:

- `CDrvScope::ConfigADC` on a channel-set change: `DevAcquireSPU_SetSecondFilter(enable, sample_mode, index)`.
  `enable` and `index` come from two driver-setting fields that no code in the library writes (the only accesses are
  the read in `ConfigADC` and `ConfigueDelay`). OOS observed 0x1098/0x109c = 0 on the unit after stock runs
  (`horizontal-config.md` 2.3), which fits "never loaded".
- `DevAcquireSpu_AdcCoreOrder` (ADC core order change): forces bypass.

Conclusion: in this firmware the FIR is dormant. It is not tied to high resolution.

### 1.4 Coefficient tables in the library

Do not copy these into the fork. Locations in the analysis repo:

- `filterCoef.csv`: 12 sets x 16 taps, keyed 0..11,
  symmetric, DC gain 2^22. Measured -3 dB points (fraction of the input rate): 0.035 to 0.455. Likely bandwidth
  limiters per channel mode. The selection key is the dormant `index` field.
- `.../tables/spu_12bit_fir_coef.csv` .. `spu_16bit_fir_coef.csv`: 5 sets x 32 taps, symmetric, DC gain 2^22,
  -3 dB at 0.200, 0.100, 0.051, 0.035 and 0.031 of the input rate. Names suggest high-res filters for 12..16 bits.
  No code references them. 32 symmetric taps fit the 16-coefficient load if the hardware mirrors them, which is not
  proven.

## 2. What OOS does today

- `ui/src/board/record.rs:105`: high res is a software boxcar mean of the record samples behind each screen point. It
  gains only where the record has more samples than the screen has points.
- `hal`: 0x1098, 0x109c and 0x10a0 are never written (`adc-mode.md` 190). 0x10a0 keeps the stock value 16 from the
  hand-over or power-on (`cold-boot.md` 300 lists it as a missing write).
- The UI noise model scales noise by 0.3 in HiRes for the demo (`ui/src/wave.rs:652`).

Gap: no hardware filter before decimation. At decimation by 8 and more the gain depends on what 0x1004 bit 2 does.

## 3. Proposed change

Two steps. Step 2 only if the measurements of section 5 justify it.

### 3.1 Step 1: confirm and expose the existing decimator behaviour (no new registers)

- Keep `acq.rs:67` as is (bit 2 at decimation 8 and up).
- If 5.1 shows bit 2 averages: mark records taken with bit 2 set as `pre_filtered` in the frame, and let
  `record.rs:105` skip the software mean where the hardware already averaged (avoids a double boxcar and a wrong
  bandwidth claim). Report the effective bits in the UI: `12 + 0.5 * log2(decimation)` for a boxcar.
- Apply the half-sample delay correction of 1.1 in the time axis for HiRes, the same way the stock driver shifts the
  play-chain delay.

### 3.2 Step 2: FIR anti-alias filter for decimation 2..7

Only useful where bit 2 is 0 (decimation below 8) and the FIR sits before the decimator.

- New `hal/src/dho/fir.rs`:
  - `bypass(sh) -> Vec<Write>`: 0x1098 bits 1:0 = 1, 0x10a0 = 16.
  - `load(sh, ratio, coefs: &[i32; 16]) -> Vec<Write>`: the sequence of 1.3 with the lane split per ratio.
  - `run(sh) -> Vec<Write>`: bits 1:0 = 0, 0x10a0 = shift.
  - Shadow `s1098` in `scu::Shadows`.
- Coefficients: designed by OOS (windowed sinc, Kaiser, cut-off `0.4 * rate_out` relative to the ADC lane rate),
  quantised to 23 bits with DC gain 2^22. Generated at build time by a small tool, not taken from the stock tables.
  The stock tables serve only as a reference for magnitude and format in a host-side check outside the fork.
- Write it in the ADC block (on channel-set change) and when the decimation crosses 2..7.
- Bandwidth: a 16-tap filter at 1.25 GSa/s cannot go below about 0.03 of the rate. So it covers decimation 2..16 at
  best. Deeper decimation relies on 1.2.

### 3.3 Result width

The output words stay 16-bit. The shift in 0x10a0 decides how many filter bits end up in the word. Hypothesis: the
stock value 16 keeps the input scale (2^22 gain, 6 bits of headroom absorbed by the downstream Q10 gain path). 0x10a0 =
8 in average mode would keep 8 more bits. Measure (5.4) before using any value other than 16.

### 3.4 Expected CPU gain

Small. The software mean in `record.rs` costs one pass over the record. The real gain is quality: proper anti-aliasing
before decimation and correct high-res at slow time bases without needing a deep record.

## 4. Host test plan

- `fir.rs` write-sequence tests for ratio 1, 2 and 4: count of 0x109c writes (64 per load), lane and tap fields in
  bits 31:23, coefficient masked to 23 bits, 0x1098 values 2 then 0, 0x10a0 written after every 0x1098 write.
- Polyphase check: a host model in `tools/` (new `firmodel.py`) that applies the 4-lane polyphase split to a 16-tap
  filter and reproduces the plain 16-tap convolution on an interleaved test signal. Same vector file used by the Rust
  test.
- Coefficient generator test: DC gain exactly 2^22 after quantisation, symmetry, -3 dB within 2 % of the target.
- `record.rs` test: a frame marked `pre_filtered` is not averaged a second time.

## 5. Hardware test checklist

1. Bit 2: CH1 terminated, 50 mV/div, rate = max/16. Measure RMS noise with 0x1004 bit 2 = 1 (stock) and forced 0.
   A boxcar of 16 gives about 1/4 of the noise. Repeat at max/8 and max/4 (bit 2 = 0 there).
2. Same with a 100 kHz sine: amplitude and aliasing with bit 2 on and off.
3. Read 0x1098 after cold boot under OOS: 0, 1 or other? Is the filter running with power-on coefficients?
4. FIR: load a unit impulse (tap 0 = 2^22, rest 0). Output must equal bypass. Then a 2-tap average: check the noise
   drop and the half-sample delay. Then a low-pass at 0.1 of the rate with a swept sine: is the FIR before or after
   the decimator?
5. 0x10a0 = 16 vs 8 with the impulse filter: does the output scale by 256? Any clipping?
6. Repeat 4 at ratio 2 and 4 to validate the lane split.

## 6. Confidence and open questions

Confidence: register format and load order high. Use in the stock firmware: high confidence that it is unused.
Position in the data path and the meaning of 0x1004 bit 2: unknown until measured.

1. Is the FIR before the decimator (anti-alias) or after it (display filter)?
2. Does 0x1004 bit 2 select averaging decimation?
3. What does the filter do with 0x1098 = 0 and never-loaded coefficients?
4. Do 32-tap symmetric sets load as 16 coefficients (hardware mirroring)?
5. Meaning of the high-res keys 6 and 8 in the stock UI (extra bits, or filter selection for a future build).
