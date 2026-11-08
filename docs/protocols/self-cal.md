# Self-calibration

The code is `hal::dho::selfcal`, `hal::dho::calfile`, `hal::dho::adc::measure`, the commands `oos-fpgad selfcal ...` and the page
System > Self-cal of `oos-ui`.

Status: the vertical and AFE zero calibrations ran on the unit (section 13). The ADC core calibration is host-tested
only and not yet verified on hardware (section 15).

Related docs: `afe-settings.md` (cal file formats, scale and offset, AFE primitives, block groups),
`cold-boot.md` (boot-time ADC calibrations), `afe-spi.md`, `adc-mode.md`, `adc-spu-init.md`.
Register notation as in `afe-settings.md`: `rNN` = AFE shadow of register NN, `W(NN)` = write the whole shadow word.

## Short answer

- A self-calibration runs up to seven items in a fixed order (1.2). **Default in OOS**: Vertical for the selected channels.
  *ADC cores* (ADC gain & offset) and *AFE zero* are options, off by default. *ADC data line* runs at every boot. *ADC
  phase*, *Ext* and *DDR* are not offered.
- **No external equipment and no precision reference.** The only stimulus is the AFE's own 16-bit offset DAC
  (AFE reg 0x28). Gain is measured as "ADC codes per offset-DAC step" and converted to volts with the factory LSB table
  `cal_lsb.hex` (V per DAC code at the BNC, per channel and input-gain path). So the self-cal *can* calibrate gain,
  but only relative to the factory-measured DAC LSB. `cal_lsb.hex` itself is factory-only.
- The AFE zero item does not use the ADC at all: it trims internal amplifier offsets by binary searches against
  comparators inside the AFE (status bits of AFE reg 0).
- Results have the format of `cal_adc.hex`, `cal_afe_zero.hex`, `cal_vertical.hex`, in the checked container (7.1, 2 x
  CRC32). OOS writes them to `/oos/cal` only.
- Never-do items: the *DDR* item (FPGA SPI-flash erase/program, then a forced power cycle), every write to `/rigol/data`,
  deleting calibration files (section 8).

## 1. Control flow

### 1.1 UI side

System > Self-cal offers: Channels (All, CH1..CH4), ADC cores (Off/On), AFE zero (Off/On), Start, Cancel, Status. Start
must be pressed twice within 10 s; the first press shows the preconditions (section 9). The run takes over the FPGA and the
AFEs in the acquisition thread. A roll acquisition is stopped first. Status shows configurations done of total, the
current channel and pass. Cancel ends the run at the next configuration and nothing is kept.

On success the results are saved to `/oos/cal` and take effect at the next set-up. `oos-fpgad selfcal
entry|probe|zero|vertical|all` does the same from the command line. `--save` writes to `/oos/cal` only.

### 1.2 Items and order

Run the items in this order. An error stops all later items.

1. **ADC data line**: sync delay sweep, lane eye scan, lane delay load, core alignment. Same steps as at boot (`cold-boot.md`
   3.3 to 3.5). **Nothing is saved.** OOS runs it at boot only.
2. **ADC gain & offset**: section 14. Saves `cal_adc.hex`.
3. **ADC phase**: per selected channel, saves `cal_adc.hex`. Needs an external signal (section 5, 14.11).
4. **AFE zero**: section 3, per selected channel. On success saves `cal_afe_zero.hex`.
5. **Vertical**: section 2, per selected channel. On success saves `cal_vertical.hex`. Every pass of every selected
   channel must have succeeded.
6. **Ext**: saves `cal_ext.hex`.
7. **DDR**: **never-do** (section 8).

Channel loops cover 4 channels, 2 on hardware version 4 (2-channel models). On those models the AFE zero item also runs
for channel index 3.

`hal::dho::selfcal::run_all_with` runs: ADC cores for the channel set (optional), then per channel AFE zero (optional) and the four
vertical passes.

## 2. Vertical calibration

Purpose: for every analog gain configuration measure the real V/div and the offset-DAC code that centres the trace,
then rebuild the 801-row scale table of one bandwidth block group.

### 2.1 Setup

1. ADC input mode "this channel only" (mode n = CHn+1 only, `adc-mode.md`) with the stored parameters of the mode.
2. FPGA read-path gain 1.0 (0x400 on all four) and offset 0 are the neutral setting. They have no effect on the measure
   unit (section 13), so OOS leaves the values of the normal set-up.
3. `hz_enable(ch, 1)`.
4. Per pass: set the bandwidth by the normal path (`afe-settings.md` 5.1). Passes and target block group (variant):

| pass | bandwidth value v | filter triple | block group written |
|---|---|---|---|
| 0 | 2 | (0, 0, 5) "S20M" | 0 |
| 1 | 5 | (0, 0, 31) "155M" | 1 |
| 2 | 6 | (0, 0, 63) "315M" | 2 |
| 3 | 0xa | (3, 3, 7) "350M" | 3 |

Note: group 3 is measured with the (3, 3, 7) filter, but the 350M setting maps to group 0 and OFF / >= 400 MHz map to group 3
(`afe-settings.md` 5.2).

### 2.2 Per configuration

The configurations are the distinct gain tuples (IG, VGA, ATT, SGA, SGAR, MUX, MUXR) from about 1 mV/div (IG 0) to about
10 V/div (IG 3). A filled file holds 76 of them. OOS derives the list from the active `cal_vertical.hex`: the union of the
tuples of all block groups of the channel, sorted by scale, the same list for every pass (group 3 is empty in factory
files).

For each entry:

1. Set the configuration exactly as normal operation does (`Afe::set_config`): tuple, `hz_output(ch, 0, 1)`,
   `hz_output(ch, 1, 0)`, gear GPIO high, zero trims (`afe-settings.md` 3.1 step 8) and the bandwidth trims of the
   input-gain path (step 9).
2. Settle 1 s.
3. Two DAC searches (2.4): code c_hi with ADC mean a_hi near **55268**, code c_lo with ADC mean a_lo near **10268**
   (= 32768 +- 3 divisions of 7500 codes). DAC search window 20000..40000, settle 30 ms per step. The attenuated paths
   (IG != 0) invert, cf. the LSB sign in `cal_lsb.hex`.
4. Results:
   - codes per division: `cpd = (c_hi - c_lo) / ((a_hi - a_lo) / 7500)`
   - measured scale: `S = |lsb[ch][IG] * cpd| * 1e9` nV/div (lsb from `cal_lsb.hex`, V per code)
   - zero code: `Z = c_hi - trunc((a_hi - 32752) * (c_hi - c_lo) / (a_hi - a_lo))` (DAC code for ADC 32752)
5. Refine the zero on every path, up to 4 steps: `set_dac(ch, Z)`, wait 300 ms, measure; done if the mean is within
   max(8, 0.6 DAC steps) of 32752, else correct Z by the error over the slope (a_hi - a_lo) / (c_hi - c_lo). If it did not
   settle, repeat steps 3-5 (up to 20 rounds).
6. After 20 rounds keep the last result.
7. After all entries of the pass: rebuild the table (2.5) on a scratch copy.

Cancel ends the run. An aborted or failed run keeps nothing.

### 2.3 Acceptance limits summary

| check | limit | consequence |
|---|---|---|
| DAC search | \|mean - target\| <= 16 codes, max 18 bisection steps | the last code and mean are used |
| DAC search, target outside the window | both window ends on the same side of the target | the closer end is used |
| reading stable | two consecutive means within 100 codes (5 ms apart) | re-read |
| zero refinement | within max(8 codes, 0.6 DAC steps) of 32752, 4 steps, max 20 rounds | the last result is kept |

### 2.4 Measurement primitives

**ADC measure unit** (`hal::dho::adc::measure`, `oos-hw adc-meas`):
`0x1040 <- (value & 0x77fff0e0) | 15`, then the same `| 0x80000000` (start). 15 is the averaging exponent (assumed: 65536
samples per core at 156.25 MSa/s, about 0.42 ms). Poll 0x1040 bit 30 every 10 ms, max 5 polls. Per core n = 0..7:
`0x1040[11:8] = n`, read 0x1044 (read again once if 0xeeeeeeee), also 0x1048 (peak). Output: 8 core means.

Channel value: in the four-channel mode the mean of cores k and k+4, k = per-channel slot of the core order (14.3). In
channel-only mode all 8 cores sample the channel and OOS averages all 8 (section 13).

**DAC search** (`hal::dho::selfcal::dac_for`, target -> code and mean): measure both ends of the window 20000..40000, then
bisect on the sign of the error, with each step `set_dac`, settle, read until stable, stop at +-16 codes. This works for
both path polarities. A fixed-direction bisection needs a coarse stage first: from DAC 32768 step by +-100 n (alternating,
max 20 steps) until the mean is inside 10000..60000.

### 2.5 Table rebuild (`hal::dho::selfcal::rebuild_scales`)

For every row i with key K (nV/div): choose the configuration with the **smallest measured S > K**; write the 56-byte
record (`afe-settings.md` 1.3): key K, IG/VGA/ATT/SGA/SGAR/MUX/MUXR of that configuration, digital gain = S / K (always
> 1), zero = Z of that configuration. +0x24 and +0x34 are left unchanged (0). Rows without a candidate keep their old
content. Destination: block (variant * 4 + ch).

Verified on the unit's files: in every block each tuple has one constant S = dgain * key and one zero code, minimum dgain
1.0000x. Example: tuple (0,0,2,0,0,3,1), nominal 1 mV/div, S about 1.08 mV, zero about 32750; tuple (3,2,2,2,0,2,0),
nominal 2.604 V, S about 2.77 V, zero about 32766. The rebuild reproduces the 12 filled factory blocks byte-exact from
their configurations (section 13).

## 3. AFE zero calibration

Trims the internal offsets of the AFE amplifier chain with the AFE's own comparators. No ADC, no DAC measurement.
Results: the 20 ints per channel of `cal_afe_zero.hex` (`afe-settings.md` 1.3/3.1 step 8).

### 3.1 Sequence

1. ADC mode of the channel; `dac_enable(ch, 0)` (r27 bits 15/7 = 0, offset DAC off);
   **gear GPIO low** (the only place gear 0 is used); `cz_buf(ch, 8, 8)` (r33[7:0] = 0x88 = its init value, W(33)).
2. Three configurations: nominal 1 mV/div (IG 0, VGA 0, ATT 2, SGA 0, SGAR 0, MUX 3, MUXR 1), 41.3 mV/div (IG 0, VGA 1,
   ATT 2, SGA 1, SGAR 0, MUX 2, MUXR 1), 103.7 mV/div (IG 1, VGA 2, ATT 2, SGA 3, SGAR 0, MUX 3, MUXR 0). For each: set the
   tuple, `hz_output(0, 1)`, `(1, 0)`, wait 1 s, read the input mode (r07[14:13]):
   - mode 0x4000 (IG 0, non-inverting): first stage (type 0), VGA (type 0), comparator, SGA (type 0), MUX (type 0);
   - mode 0 (IG 1..3, inverting): first stage (type 1), comparator, SGA (type 1), MUX (type 1);
   - else stop with an error.
   Wait 1 s.
3. Gear GPIO high, `dac_enable(ch, 1)`. There is no measurement-based failure.

Which result slot a step fills depends on the type and, for type 0, on the VGA gain (r29[9:8]: 0 -> 25, 1 -> 2.5 in mode
0x4000):

| z index | step | register written | read-back bit (AFE reg 0) |
|---|---|---|---|
| z0, z1 | first stage, type 0, VGA 25 (sign, magnitude of the non-inverting offset trim) | r23[10], r23[9:0] | bit 2 |
| z2 | VGA, VGA 25 (non-inverting high-gain trim) | r35[8:0] | bit 1 |
| z3, z4 | SGA, type 0, VGA 25 (P, N) | r27[14:8], r27[6:0] | bit 3 |
| z5, z6 | MUX, type 0, VGA 25 | r06[6:0], r06[14:8] | bit 3 |
| z7, z8 | first stage, type 0, VGA 2.5 | r23[10], r23[9:0] | bit 2 |
| z9 | VGA, VGA 2.5 (non-inverting low-gain trim) | r29[15:10] + r35[11:9] (sel 0) | bit 1 |
| z10, z11 | SGA, type 0, VGA 2.5 | r27 | bit 3 |
| z12, z13 | MUX, type 0, VGA 2.5 | r06 | bit 3 |
| z14, z15 | first stage, type 1 (sign, magnitude of the inverting offset trim) | r26[10], r26[9:0] | bit 1 |
| z16, z17 | SGA, type 1 | r27 | bit 3 |
| z18, z19 | MUX, type 1 | r06 | bit 3 |

### 3.2 The searches

All are binary searches on a search index x with a comparator read after each step (10 ms settle): read 1 -> upper
bound = x, read 0 -> lower bound = x, next x = (lower + upper) / 2. The final x is written once more and stored. OOS takes
the majority of 5 comparator reads per step (section 13).

- **First stage** (type): mode = 0x4000 for type 0, 0 for type 1. Self-check on before, off after. x in 0..2047, start
  1023, 14 halvings. x is a sign-magnitude trim: x < 1024 -> sign 1, magnitude 1023 - x; else sign 0, magnitude x - 1024.
  Written with `cal_zero_first` (reg 0x23 for mode 0x4000, 0x26 for mode 0). Read: reg 0 bit 2 for mode 0x4000, bit 1 for
  mode 0.
- **VGA** (type 0 only): sel = 1 for VGA 25, 0 for 2.5. First run the first stage of type 1 (trims the inverting first
  stage, result z14/z15, later overwritten by configuration 3), then zero mode and VGA output (r29[3:0] = 0 for sel 1, 1
  for sel 0). x in 0..224, start 112, 8 halvings. 9-bit trim code: x <= 63 unchanged, above that
  x + 41 * (1 + (x - 64) / 23) (integer division). Written with `cal_zero_vga4000(ch, 0x4000, code, sel)`, read reg 0 bit 1.
- **Comparator**: comparator self-check (r2a |= 0x160), x in 0..127 start 64, 7 halvings, written to r07[6:0], read reg 0
  bit 3. **The result is not stored** in any file. It calibrates the comparator used by the next two steps and stays in
  r07 until restart.
- **SGA** (type): comparator input SGA (r2a[6:5]), x in 0..255 start 127, 8 halvings. Two one-sided 7-bit trims: x < 128
  sets P = 127 - x (N keeps its last value), x >= 128 sets N = x - 128 (P keeps its last value). Written with `cal_zero_sga`.
  OOS starts P and N at 0 before each search.
- **MUX** (type): comparator input MUX (r2a[6:5] = 1), x in 0..255 start 127, 8 halvings, written with `cal_zero_mux`.
  The mapping is the other way round: x < 128 sets the second value (r06[14:8]) = 127 - x, x >= 128 the first (r06[6:0]) =
  x - 128 (verified on the unit: the SGA mapping drives every MUX search to the rail). OOS starts both at 0 before each search.

Primitive details (names of `hal::dho::afe::Afe`):

| primitive | effect |
|---|---|
| `cz_self_check(ch, mode, en)` | mode 0x4000: en 1: r24 = (r24 & ~0x7800) \| 0x4400, r23 \|= 0x2000, W(23), W(24); en 0: r24 &= 0xbbff, W(24). mode 0: en 1: r26 = 0x9800 \| (r26 & 0x7ff); en 0: r26 &= 0x37ff; W(26) |
| `cz_mode(ch)` | r26 = 0x5800 \| (r26 & 0x7ff), W(26) |
| `comp_self_check` | r2a \|= 0x160, W(2a) |
| `comp_input(mux = false)` | r2a[6:5] = 2 for the input modes 0 and 0x4000, W(2a) |
| `comp_input(mux = true)` | r2a[6:5] = 1, W(2a) |
| first-stage read | AFE read of reg 0: bit 2 (mode 0x4000) / bit 1 (mode 0) |
| comparator read | AFE read of reg 0, bit 3 |
| `comp_trim` | r07[6:0] = v, W(07) |
| `vga_output(ch, sel)` | mode 0x4000: r29[3:0] = sel ? 0 : 1, W(29) |
| `cz_buf(ch, a, b)` | r33[7:0] = a \| b << 4, W(33) |

Side effects of the sequence: r23 bit 13 stays set, r26[15:11] ends as 0b00010 (init 0x3200 has bit 13 set), r29[3:0] ends at
0/1 (init 2), r2a keeps 0x160 + the last input select, r07[6:0] keeps the comparator trim. Whether these matter in normal
operation is not known. OOS runs the zero calibration on a copy of the AFE shadows and does not restore these bits
explicitly.

## 4. ADC gain & offset - summary

Matches the 8 interleaved ADC cores to each other. No external signal, the offset DAC again provides two DC levels. All
channels at IG 0, VGA 1, ATT 1, SGA 1, SGAR 0, MUX 3, MUXR 0 (about 50 mV/div), gear high, DAC 32768. The result is
`cal_adc.hex` (1848 B, `adc-mode.md` 3.1). Register level: section 14.

## 5. Factory-only vs. user calibrations

| calibration | needs | file | OOS |
|---|---|---|---|
| Vertical gain/offset | nothing (inputs open or 50 Ohm terminated) | cal_vertical.hex | implemented, verified on the unit |
| AFE zero | nothing | cal_afe_zero.hex | implemented, option, off by default |
| ADC gain & offset | nothing | cal_adc.hex | implemented, option, off by default, not yet verified on hardware |
| ADC data line | nothing | - (boot does it anyway) | at boot (`cold-boot.md`) |
| ADC phase | external sine | cal_adc.hex | skip |
| Ext | models with EXT input (assumed: not applicable on the DHO924S) | cal_ext.hex | skip |
| DDR | FPGA flash write | cal_ddr.hex | **never** |
| Offset-DAC LSB | external DC reference / DMM: the trace is centred, the LSB value is supplied from outside | cal_lsb.hex | factory, read only |
| Bandwidth trims | swept/step source | cal_afe_bandwidth.hex | factory, read only |
| AFG | external DMM | cal_afg.hex (96344 B) | factory |

## 6. AFG and LA probe (short)

- AFG: factory-only. The operator enters measured values per range. The result is saved in `/rigol/data/cal_afg.hex`
  (payload 0x17858 B) and loaded at boot with the default fallback. See `afg.md`.
- LA probe: separate from the self-cal. Sweep the LA low and high thresholds (start 10000) and read the LA data to find
  the switching points. The result is `/rigol/data/cal_la.hex` (8 B), loaded at boot with the default fallback. Assumed: a
  connected probe is required. See `la.md`.

## 7. Files and container

### 7.1 Container

| offset | type | content |
|---|---|---|
| 0x00 | u32 | CRC32 (zlib polynomial) of bytes 0x08..0x1b |
| 0x04 | u32 | 0x14 (size of the following header) |
| 0x08 | u32 | version, always 0 |
| 0x0c | u32 | time: day << 24 \| hour << 16 \| minute << 8 \| second |
| 0x10 | u32 | date: year << 8 \| month |
| 0x14 | u32 | payload length |
| 0x18 | u32 | CRC32 (zlib) of the payload |
| 0x1c | ... | payload |

Verified on the unit's files (both CRCs). `hal::dho::calfile::build` reproduces them byte for byte.

Stock files are written in place, without a temporary file, so a crash mid-write can leave a broken file. A reader rejects
a file on open failure, short read, wrong header size, CRC or length mismatch, and takes the next location.

OOS writes atomically (`hal::dho::calfile::save`): `name.new`, fsync, the old file to `name.bak`, rename, directory fsync. It
refuses any path under `/rigol`. OOS stamps UTC time, the unit has no time zone set under OOS.

### 7.2 What the self-cal writes

| item | file | payload |
|---|---|---|
| ADC gain & offset, ADC phase | `cal_adc.hex` | 1848 B |
| AFE zero | `cal_afe_zero.hex` | 320 B = 4 ch x 20 s32 (3.1) |
| Vertical | `cal_vertical.hex` | 0xaf380 B = 16 blocks (variant-major: block = variant * 4 + ch) x 801 x 56 B |
| Ext | `cal_ext.hex` | 160 B |
| DDR | `cal_ddr.hex` | 48 B (never-do) |

`cal_lsb.hex` and `cal_afe_bandwidth.hex` are not written by the self-cal (the stock system rewrites the bandwidth file at
boot, `afe-settings.md` 1.3). `/rigol/data/default/` is never written. A 12-block `default/cal_vertical.hex` has a file
size of 538300 bytes (`afe-settings.md` 1.3).

OOS does not read `/rigol/data/cal.bin`. It holds process pointers and must never be loaded raw.

## 8. Never-do items in the self-cal path

- **DDR item**: erases and programs the FPGA SPI flash with a self-test image (`cold-boot.md`), then saves `cal_ddr.hex`,
  waits 8 s and forces a power cycle. Skip entirely. OOS keeps reading the existing `cal_ddr.hex`.
- Every save to `/rigol/data`: `cal_adc.hex`, `cal_afe_zero.hex`, `cal_vertical.hex`, `cal_ext.hex`, `cal_ddr.hex`,
  `cal_la.hex`, `cal_afe_bandwidth.hex`, `cal_lsb.hex`, `cal_afg.hex`.
- Deleting calibration files in `/rigol/data` (`cal_adc`, `cal_lsb`, `cal_afe_zero`, `cal_afe_bandwidth`, `cal_vertical`,
  `cal_ext`, `cal_ddr`, `cal_afg`).
- ADC phase item: re-runs the sync/lane/core-align calibration and changes ADC sample-clock delays based on an
  external signal. Not useful without equipment.

## 9. Duration and preconditions

- There is no temperature reading, no warm-up timer and no detection of a connected input. Warm up at least 15 min, better
  30 min. Disconnect everything; only 50 Ohm BNC terminators are allowed. A connected signal corrupts both the zero cal
  (comparators) and the vertical cal (ADC means). The result is not protected against this: after 20 rounds the last
  zero result is kept.
- Fixed waits: 1 s settle per configuration, 2 x 1 s per AFE zero configuration.
- Duration: vertical about 5 s per configuration, 4.7 min per block on the unit, so about 20 min per channel for four passes
  and about 80 min for CH1..CH4 (the last two figures are extrapolated). AFE zero about 10 s per channel (estimate). ADC
  gain & offset: 14.8.

## 11. OOS implementation

Same file layout, but output only to `/oos/cal`, never `/rigol/data`. OOS reads its own file first and falls back to
`/rigol/data`, then `/rigol/data/default`, both read only.

### 11.1 hal

1. `hal::dho::calfile`: container reader and writer (7.1). `parse` checks both CRCs, `save` writes atomically and refuses any
   path under `/rigol`, `load_first` returns the first valid copy.
2. `hal::dho::adc::measure()`: measure unit (2.4), exponent 15, 5 x 10 ms timeout, 0xeeeeeeee retry, returns 8 core means.
3. `hal::dho::afe` primitives (shadow + whole-word writes): `cz_self_check`, `cz_mode`, `comp_self_check`, `comp_input`,
   `comp_trim` (r07[6:0]), `vga_output` (r29[3:0]), `cz_buf` (r33), `dac_enable` (r27 bits 15/7), `set_dac` (r28), the gear
   line, and the read-backs (AFE reg 0 bits 1/2/3).
4. `selfcal::afe_zero(hw, afe, old) -> [i32; 20]`: section 3 (three configurations, searches, slot table). Slots a
   configuration does not reach keep their value.
5. `selfcal::vertical_entry` + `rebuild_scales`: section 2 on a **scratch copy** of the table, with ADC mode "channel only",
   four passes with the band values 2/5/6/0xa via the normal set-up, the tuple list of 2.2, and the formulas of 2.2.
   Abortable between configurations; on abort or failure nothing is kept.
6. `selfcal::adc_cores`: sections 14 and 15.
7. The new tables take effect at the next set-up: the UI reloads the calibration after a successful run.
8. `afe::Cal::load` order: OOS file (if CRCs are ok), then `/rigol/data`, then `/rigol/data/default`. `load_from` reports the
   source of each file.

### 11.2 UI (oos-ui, System page)

See 1.1. The start dialog states the preconditions of section 9, about 5 min per channel and pass, and the target
directory. The result line gives the configurations measured, the largest relative change of S against the active table, and
the files saved. The status text is also available over SCPI.

## 13. OOS implementation and unit results

Code: `hal::dho::calfile` (container, atomic save to `/oos/cal`, refuses /rigol), `hal::dho::selfcal` (algorithms against the
`CalHw` trait, host model tests, `Unit` on the unit, `run_all`, `save`), `hal::dho::adc::measure`,
`oos-fpgad selfcal entry|probe|zero|vertical|all`, oos-ui System > Self-cal (runs in the acquisition thread). OOS's own
data partition: the last 16 MiB of `backup` (`oos-mount-data`, `blkdevparts=` in the OOS bootargs).
`afe::Cal::load` prefers a valid file in `/oos/cal`, then `/rigol/data`, then `/rigol/data/default`.

Hardware facts found on the unit:
- The comparator self-check and input-select register is **0x2a** (3.2).
- The MUX trim mapping is the reverse of the SGA mapping (3.2).
- The ADC measure unit (0x1044) returns the mean raw code per core directly, before the read-path gain (about 6970
  codes per volt at 1 V/div). In channel-only mode all 8 cores sample the channel; OOS averages all 8.
- The offset DAC needs more than 5 ms to settle (runs with 5 ms contradicted themselves). OOS waits 30 ms.

OOS choices, not all of them verified to be required:
- DAC search on the sign of the error with both window ends measured (works for both path polarities).
- P/N and A/B trims start at 0 before each search.
- Majority of 5 comparator reads. With single reads the high-gain paths scatter by +-20 trim codes between runs.
- The zero code is refined on every path until the ADC mean is within max(8, 0.6 DAC steps) of 32752 (2.2).
- Every configuration is set exactly like normal operation (`Afe::set_config`, including the bandwidth trims of its
  input-gain path). Keeping the trims of the start scale left up to 0.03 div of zero error.

Unit results, verified on the unit (one open channel, block group 2):
- Gain: 76 configurations within -0.50 .. +0.52 % (IG 0) and +0.02 .. +0.12 % (IG 1..3) of the factory table,
  ~5 s per configuration (4.7 min per block).
- Zero codes: +13 .. +73 DAC codes away from the factory values on IG 1..3 (drift).
- Display check at 250 MHz bandwidth (block 2), open channel: factory -0.030 / -0.035 / -0.035 / -0.034 div at 10 mV /
  50 mV / 200 mV / 1 V; first OOS run -0.006 / +0.009 / +0.031 / +0.029 div (the line through the +-3 div points and
  the trims of the start scale left an error on IG 1..3, hence the refinement and `set_config` above).
- `rebuild_scales` model: rebuilds all 12 factory blocks byte-exact from their configurations.
- AFE zero (comparators): first/VGA/inverting trims reproducible within +-2 codes and close to the factory values;
  SGA/MUX on the VGA-25 path (about 1 mV/div) scatter by +-15 codes between runs even with majority reads, so AFE zero is
  an option, off by default.
- Block group 3 is empty in the factory file. OOS uses the union of the configurations of all blocks for every pass.

## 14. ADC gain & offset calibration (register level)

This section gives the complete procedure. It is consistent with `cal_adc.hex` files of a unit, but it has not been run under
OOS: section 15 lists where the OOS code differs and its test status. Word formats of the ADC SPI writes (0x1034) are in
`adc-mode.md` 2.2.

Terms: **slot** = index 0..7 of the FPGA ADC measure unit (0x1040[11:8], value in 0x1044, 2.4);
**core** = ADC core n with gain register 0x15 + 8n and offset register 0x14 + 8n. The calibration keeps a table
`order[n]` = slot that shows core n.

### 14.1 Call tree

Channel mask: bit c = CH c+1.

1. set-up (14.2), core-order mode on, acquisition running, 100 ms;
2. core order (14.3) -> `order[8]`;
3. reference (14.4), payload 0x48;
4. gain of the channels of the mask (14.5), payload `c*0x68 + 0x20`;
5. offset of the ADC modes of the mask (14.6), payload `m*0x68`;
6. core-order mode off, ADC mode 0 (applies the new mode-0 values). Save the payload on success.

### 14.2 Set-up

- Channel count 4 (2 on hardware version 4).
- FPGA analog gain 1024 (= 1.0) and analog offset 0 for all four channels.
- Every channel: AFE input gain 0, VGA 1, ATT 1, SGA 1, SGAR 0, MUX 3, MUXR 0 (about 50 mV/div), both
  `hz_output` calls ((0, 1) and (1, 0)), gear high, offset DAC 32768.
- Core-order mode on: 0x1004 bit 0 = 1 (cleared again on leaving; the normal acquisition mode also clears it,
  `horizontal-config.md` 3: "0x1004 bits 0,1,3,12 cleared"), invert bits 0x1004[19:16] all 0,
  sample delays 0x1080/0x1084 all 0, 0x10d0 = 0x8000 | (max rate / 1.25 GSa/s), 0x10d8 = 1, 0x10d4 = 0, and
  on entry only the wave delays all 0 (coarse and fine delay, registers not known).
  Assumed meaning of 0x1004 bit 0: raw core order in the sample stream and measure unit. The slot mapping of 14.3 belongs
  to this state.
- Record length 0, main time scale 1e10 (assumed: ps units, 10 ms/div), main offset 0, run, sleep 100 ms.

The AFE and read-path settings are not restored at the end. Re-apply the channel settings afterwards.

### 14.3 Core order

Runs in the ADC mode active before step 3. All 8 core offsets = 0, then DAC search (CH1, target 32768, settle 20 ms, 2.4),
2 ms, one measurement as the base. For core n = 0..7:
core n offset = +1023 (maximum), 2 ms, measurement; every slot k whose mean moved by more than 7936 codes
(|delta| / 16 > 496) gives `order[n] = k` (the last such slot wins; no slot -> 0); core n offset back to 0.
Default order: **0, 2, 1, 3, 4, 6, 5, 7**.

Verified on the unit: the order in core-order mode is 0, 2, 1, 3, 4, 6, 5, 7, so slot 1 shows core 2 and slot 2 core 1
(also 5/6). Consequence: the channel value (mean of slots `order[ch]` and `order[ch] + 4`) is cores ch and ch+4,
exactly the cores that sample CH ch+1 in the four-channel mode (`adc-mode.md` 3.2).

### 14.4 Reference

ADC mode 0; payload 0x48 / 0x4a = 32 / 32; reference write (word 0x01097700 from the start value); core offset and core
gain of all 8 cores = 0 (ADC 0 only, 16 SPI words). **The reference is fixed to 32**, nothing is calibrated here.

### 14.5 Gain

Per channel c (gain record c = payload `c*0x68 + 0x20`, fine block c = payload `0x478 + 64*c`):

1. Single-channel mode c (all 8 cores sample CH c+1; this also writes the old stored values of mode c), then core
   offset n = 0 and core gain n = 0 for n = 0..7 (g[n] = 0 in the payload), and **0x1058 = 0**.
2. DAC search for 55268 -> DAC code H, for 10268 -> DAC code L (32768 +- 22500 codes, 20 ms settle each step).
3. Span measurement (H, L): DAC = H, 100 ms, sum of 10 measurements; DAC = L, 100 ms, 10 more;
   **span[k] = mean_H[k] - mean_L[k]** per slot (about 45000 codes); also **mid = (max span + min span) / 2** (mid-range,
   not the mean).
4. **Fine gain**: r[k] = max(span) / span[k] - 1 per slot; fine[k] = 3 if r >= 1.2 %, 2 if 0.4 % <= r < 1.2 %, 1 if
   0.2 % <= r < 0.4 %, else 0. fine[k] goes to payload `0x478 + 64*c + 4*k` (slots 0..7; 8..15 untouched);
   0x1058 <- packed value (2 bits per slot); 200 ms. So the slot with the largest span gets 0 and the others are raised in
   steps. Assumed: codes 1/2/3 = about +0.2/+0.4..0.8/+1.2 % digital gain in the SPU per slot.
5. Reference: span measurement again -> **ref = mid** (with the fine gains active).
6. **Iteration**, at most 12 rounds: write the 8 core gains g[n] (0x1034, regs 0x15 + 8n), span measurement, then per
   core n with its slot k = order[n]: e = (span[k] - ref) / 16; if |e| > 2 (so |span - ref| > 32 codes, about 0.07 %):
   **g[n] = clamp(trunc(g[n] - 2048 * (span[k] - ref) / ref), -64, 63)** and count the core.
   Stop when no core was corrected. The factor implies **1 gain LSB = 1/2048 = 0.049 % relative gain** (assumed, unit loop
   gain).
7. If the last round still corrected cores: failure.
8. Success: DAC search for 32768 -> DAC code that centres CH c+1 at mid-scale. Keep it for 14.6, it is not saved.

Note: the gains are indexed by core, the fine gains and spans by slot. Mixing them up (wrong `order`) makes the gain
loop correct the wrong cores (cores 1/2 and 5/6) and fail after 12 rounds.

### 14.6 Offset

Per ADC mode m (offset record m = payload `m*0x68`):

1. ADC mode m (input mode + the stored parameters of mode m, so the gains just calibrated, `adc-mode.md` 2.2/3.2 including
   the mode-9 exception); 0x105c fields of CH1 = 0, in single-channel modes the whole word = 0 (assumed to have no effect on
   the measure unit, which reads before the read-path gain, section 13); core offsets n = 0 (payload and 0x1034).
2. Offset DAC of CH1..CH4 = the four centre codes of 14.5 step 8 (all four channels, whether or not the mode
   uses them; only modes whose channels were gain-calibrated in the same run are calibrated).
3. **Iteration**, at most 20 rounds: write the 8 core offsets (regs 0x14 + 8n), one measurement right after the SPI writes
   (no extra settle), per core n with k = order[n]: e = (mean[k] - 32768) / 16; if |e| > 1.5: **o[n] = trunc(o[n] - e)** (no
   clamp; values outside -1024..1023 are not written and the loop then fails). Stop when no core was corrected, else 50 ms
   and the next round. **1 offset LSB = 16 measure-unit codes = 1 LSB of the 12-bit ADC** (assumed); tolerance +-1.5 LSB
   (+-24 codes) per core, target = absolute mid-scale 32768.
4. Not converged: failure.

### 14.7 Channel mask -> what is calibrated

`selfcal::adc_plan`:

| mask (CH bits) | gain records | offset modes |
|---|---|---|
| 1 / 2 / 4 / 8 | 0 / 1 / 2 / 3 | 0 / 1 / 2 / 3 |
| 3 (CH1+2) | 0, 1 | 0, 1, 4 |
| 5 (CH1+3) | 0, 2 | 0, 2, 5 |
| 9 (CH1+4) | 0, 3 | 0, 3, 6 |
| 6 (CH2+3) | 1, 2 | 1, 2, 7 |
| 0xa (CH2+4) | 1, 3 | 1, 3, 8 |
| 0xc (CH3+4) | 2, 3 | 2, 3, 9 |
| 7 / 0xb / 0xd / 0xe (3 channels) | the three | the three single modes + the three pair modes (no mode 10) |
| 0xf or 0 (default) | 0..3 (0 with mask 0: all channels) | **all 11 modes 0..10** |
| default on hardware version 4 | 0, 1 | 0, 1, 4 |

With all four channels everything in the file is recalibrated except the reference (fixed 32) and the delay fields (ADC
phase only). Gains are calibrated per channel in single-channel mode and reused by all modes (mapping `adc-mode.md` 3.2);
offsets per mode. Order: CH1 gain, CH2 gain, ..., then the modes in the table order. A gain error stops the calibration
before the offsets. An offset error stops the remaining modes.

### 14.8 Errors, progress, abort

- A gain or offset failure fails the item, so nothing is saved. DAC search failures (after 18 bisection steps) are ignored in
  this path.
- A cancel must count as failure and keep nothing. If the loops only skip their measurements on cancel, a count of 0
  corrected cores reads as success and zeroed or partial trims would be saved.
- Duration (estimate): per channel 2 DAC searches (~1 s each), 2 + up to 12 span measurements (~0.4-0.6 s each: 2 x 100 ms
  settle + 20 measure-unit reads of >= 10 ms) and the centre search -> ~5-10 s; per mode up to 20 x ~60 ms ->
  ~0.1-1.3 s; all four channels + 11 modes about **0.5-1.5 min**.

### 14.9 Registers touched (summary)

| target | register | value |
|---|---|---|
| ADC (0x1034) | reg 0x09 reference | 32 (word 0x01097700) |
| ADC (0x1034) | regs 0x15 + 8n core gain | 7-bit two's complement, -64..63, 1/2048 per LSB (assumed) |
| ADC (0x1034) | regs 0x14 + 8n core offset | 11-bit two's complement, -1024..1023, 1 ADC LSB (assumed) |
| ADC (0x1034) | regs 0x54/0x55, 0x04/0x05/0x01 | via the mode switch (input mode, stored delays, unchanged) |
| FPGA | 0x1058 SPU fine gain | 0 during the gain step of a channel, then the packed fine codes of that channel |
| FPGA | 0x1040/0x1044 measure unit | exponent 15, 8 slots (as 2.4) |
| FPGA | 0x1004 bit 0, [19:16]; 0x1080/0x1084; 0x10d0/0x10d8/0x10d4; delay coarse/fine | core-order mode on/off |
| FPGA | 0x105c | CH1 fields 0 in the offset step |
| FPGA | analog gain/offset | 1024 / 0, all channels |
| AFE | IG/VGA/ATT/SGA/SGAR/MUX/MUXR, outputs, gear, DAC (reg 0x28) | 14.2; DAC per search |

No FPGA per-core offset register is used: per-core offsets and gains are trimmed inside the ADC. The only FPGA-side
per-slot correction is the 2-bit fine gain 0x1058.

### 14.10 cal_adc.hex payload (0x738 B) and how it is applied

The payload is `struct { record[11] (0x68 B each, 0x000..0x477); fine[11] (16 x i32 = 64 B each, 0x478..0x737) }`
(11 * 0x68 + 11 * 64 = 0x738; only fine blocks 0..3 are used, 4..10 are always 0). Record m:

| offset in record | type | content | written by | applied by |
|---|---|---|---|---|
| +0x00 | s16[8] | core offsets of ADC 0, mode m (m = 0..10) | offset step of mode m | regs 0x14 + 8n |
| +0x10 | s16[8] | same for "ADC 1" (0) | - | kept value only |
| +0x20 | s16[8] | core gains of ADC 0, channel m (records 0..3 only) | gain step of channel m | regs 0x15 + 8n, mapping `adc-mode.md` 3.2 |
| +0x30 | s16[8] | same for "ADC 1" (0) | - | kept value only |
| +0x40 | s16[4] | records 0..3: sample-clock delays of single mode m; records 4..7: clock delay (b, a) of mode m-4 | ADC phase (assumed: exact fields) | regs 0x04/0x05, reg 0x01 |
| +0x48 | s16[2] | record 0 only: reference ADC 0 / ADC 1 (32 / 32) | reference step | reg 0x09 |
| fine block c (0x478 + 64c) | i32[16] | 2-bit fine code per slot of channel c (slots 8..15 = 2nd ADC, 0) | gain step of channel c | 0x1058 (pair/mode selection `adc-mode.md` 3.3) |

Typical values: gains -16..15 (about +-0.75 %), offsets -74..52 ADC LSB.

**OOS applies every stored value**: cold boot step 9 writes the parameters of mode 0 (`hal::dho::boot`, `adc::Adc::parameter`:
reference, 0x1058, 8 gains, 8 offsets, delays), `seq::State::configure` writes `channel_mode` on every channel-set change
(including the mode-9 exception), and 0x105c is written per mode. Nothing in the file is unused by OOS.
`CalAdc::load` (`oos-fpgad`, `oos-ui`) prefers `/oos/cal`. `boot::cal::CalFiles::load` reads `/rigol/data` and `default/` only.

### 14.11 ADC phase (item 2), for completeness

Needs an external sine. The channel is switched on, core-order mode on, ADC clock delay (reg 0x01) and the four sample-clock
delays (regs 0x04/0x05) set to 0, then the ADC link calibrations are repeated (sync delay, data line, lane delays, core
alignment). Then the per-core phase of the sine is measured and the sample-clock delays are trimmed. Results go to the delay
fields (+0x40). Delay fields are usually 0. OOS: skip, keep 0.

### 14.12 Never-do and risks in this path

- The steps of 14.1 touch neither SPI flash nor DDR training. The only never-do is saving to `/rigol/data/cal_adc.hex` (OOS
  writes `/oos/cal` only).
- ADC phase re-runs the boot link calibrations (not destructive, but it leaves the data path in a different state; skip). The
  ADC data line item is the boot sequence again (`cold-boot.md`), not needed.
- All writes of the ADC cal are volatile (ADC trim registers, 0x1058, 0x1004 bit 0, AFE): a re-apply of the stored
  file (`Adc::channel_mode` / `parameter`) plus a normal configure restores the previous state. 0x1004 bit 0 must
  be 0 again afterwards (assumed: else the sample stream stays in core order).
- Cancel handling (14.8); offsets outside -1024..1023 are not written (14.6); a wrong `order` makes the gain loop diverge (14.5).

### 14.13 OOS implementation

1. `hal::dho::adc`: setters on `Adc` that return `Wr` words: `core_gain(n, g)`, `core_offset(n, o)`. `CalAdc`: field access
   (`gain_at`, `off_at`, `fine_at`) and `payload()` for saving. Start from the current file and change only the fields the run
   calibrated; +0x10/+0x30, delays and unused blocks stay byte-identical.
2. `hal::dho::selfcal::CalHw`: `adc_slots() -> Option<[f64; 8]>` (= `adc::measure`, slots, no averaging) and `adc_op` for ADC and
   FPGA writes.
3. `selfcal::adc_cores(hw, base, chans, progress)`: AFE tuple (0, 1, 1, 1, 0, 3, 0) + gear high + DAC 32768 on all
   channels of the set; core order detection (does not hard-code 0,2,1,3,4,6,5,7); gains per channel with `dac_for` (30 ms DAC
   settle, section 13); gain loop (12 rounds, |span - ref| > 32, step -2048 * rel. error, clamp -64..63); centre codes; offsets
   per mode (20 rounds, |e| > 1.5 LSB, results outside -1024..1023 are a failure); cancel = failure.
4. Host model test: 8 simulated cores with gain/offset errors, a permuted slot order; checks convergence and that a
   cancel returns an error.
5. Save with `calfile::save("/oos/cal", "cal_adc.hex", ..)`.
6. UI (System > Self-cal): item "ADC cores", run before Vertical, because the vertical levels depend on the core gains.

### 14.14 Checks on the unit

Read-only (OOS running, inputs open):
1. `oos-hw rd 0x1004` (bit 0 expected 0) and `oos-hw rd 0x1058` (if readable: the fine word of the mode, `adc-mode.md` 3.3).
2. `oos-hw adc-meas` in CH1-only mode after OOS applied record 0: note the 8 slot means and their spread (about 150 codes on
   an older calibration). A fresh ADC cal promises +-24 codes per core at the cal's set-up (about 50 mV/div, centred), so
   this spread is the figure of merit before and after.

Volatile writes only:
3. ADC register read-back: read requests on 0x1034 (`0x0180_0000 | reg << 16`, then 0x1038) of regs 0x14 + 8n / 0x15 + 8n in
   mode 0. The low 11 / 7 bits should equal record 0 (assumed; the read XOR mask of these registers is not known).
4. With 0x1004 bit 0 = 1 vs 0: `adc-meas` slot pattern with one core offset raised (+1023) to confirm the slot mapping in
   both states.
5. 0x1058 = 0 vs the stored word with a DC level away from mid-scale: does the measure unit see the fine gain (14.5 assumes
   it does).
6. Two DAC levels (55268 / 10268 targets) at the cal set-up with the stored trims: span spread across cores should be <=
   ~64 codes if the stored gains are still valid. The spread at 32768 shows the offset drift.

## 15. OOS ADC core calibration (host-tested only)

`hal::dho::selfcal::adc_cores` follows 14.3/14.5/14.6 with these differences (not yet verified on hardware):
- The core order is detected in the normal acquisition state, in single-channel mode of the first channel. 0x1004 bit 0 is
  never set (read-back on the unit: 0x1004 = 0x04008000, bit 0 = 0).
- The fine gains (0x1058) are set to 0, their payload fields are cleared, and the correction is by the core gains only.
- A cancel is a failure.
- Offsets outside -1024..1023 end the loop as not converged.

`adc_plan` = table 14.7. `run_all_with` runs it before the vertical passes and hands the new trims to `configure`
(`CalHw::use_cal_adc`), `save` writes `/oos/cal/cal_adc.hex` and `CalAdc::load` prefers it. UI: System > Self-cal > ADC cores
(off by default), fpgad `selfcal all --adc`. Host test: an 8-core model with the permuted order 0,2,1,3,4,6,5,7, gain errors
up to 1.2 % and offsets up to 40 LSB converges (gain spread < 0.2 %, every slot within 1.5 LSB of mid-scale). It needs every
input of the channel set disconnected.
