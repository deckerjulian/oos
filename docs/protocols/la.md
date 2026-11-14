# Logic analyzer D0..D15 (PLA2216): enable, sample rate, thresholds, read-out

Status: items marked "verified on the unit" were seen working. "Assumed" marks an inference.

**Result of the first run with a probe (OOS, DHO924S, Pico on D0-D2, `tools/device-test.py --tier la`).** Works on the unit: line
order and bit n = Dn, rates of 3.1, pulse widths down to 20 ns, edge and pulse trigger on a line, UART, SPI and I2C on the lines,
LA beside analog channels and alone, SINGLE, depths to 12.5 M words read without a gap, LA on and off.

**Open: the threshold has no effect, under stock as well.** 0x12d4 takes the codes of section 5, but over the whole range of
+-15 V, static and running, in RUN and in STOP, 0x12dc and the record follow a 3.3 V signal the same way. Stock on the same unit and
probe does the same through SCPI: a line held at 3.3 V stays high at +15 V, a 10 kHz line keeps running from -15 to +15 V. (Stock's
`:LA:POD1:THReshold` scales its argument by 1e18, so `4E-9` is 4 V and `4` is clamped to 15 V; both were tried.) So this is no fault
of OOS. Whether the cause is this probe or this port is not known; treat the real threshold as fixed, 3.3 V logic reads right.

**Roll:** the spacing of LA words is the point spacing over the ratio (section 4.4), measured with 1, 2, 3 and 4 channels. OOS rolls
from 200 ms/div (`roll-mode.md` 8).

Conventions as in `horizontal-config.md` / `roll-mode.md`: `sN` = software copy of register N, `memsel` = record length in
points, `ratio` = interleave factor (1/2/4), `len` = analog display length (0x11f0), `k` = analog compression, `pre` / `post` = the
values for 0x4050 / 0x4054.

## 0. Result in short

1. **There is no per-channel or per-pod enable in the hardware.** With the LA on, the FPGA records all 16 lines. Which Dn are
   shown is a display choice.
2. **Enable** = 0x4008 bit26 plus an LA sample-rate block 0x12a4/0x12a8/0x12ac, an LA deskew 0x128c,
   and the "LA doubling": segment size 0x10e0 x2, record depth 0x10e4 x2, pre/post 0x4050/0x4054 x2, display window
   0x1100/0x1108 x2. 0x4008 bit24 does not enable anything. It picks which stream the TX engine sends
   (analog or LA) and is only set for the read (normal mode) or permanently in roll/scan.
3. **Data format:** one 16-bit word per LA sample, **bit n = Dn** (n = 0..15), the same for every analog channel set.
   Analog and LA words are never mixed inside one frame. In normal/batch mode they come in two separate reads (the
   analog read, then a play-last read with 0x4008 bit24 = 1). In roll mode they come as separate chunks, and header word 1
   bit 1 tells them apart (1 = LA). Verified on the unit for the normal mode.
4. **LA sample rate** = (analog rate per channel) x m / 2 with m = 1/2/4 (3.1). With one analog channel that is
   **half the analog rate**, so one record holds memsel x m / 2 LA samples and only the first part of a read of memsel words is
   valid. Max 625 MSa/s. Verified on the unit.
5. **Analog rate with LA:** the analog rate is halved only if the LA is on **and** at least one analog channel is
   displayed. memsel is not recomputed, so the record then spans 2x the screen. With "LA alone" the hidden trigger-source channel
   is still acquired (mask 1 verified on the unit). Never run with an empty analog mask.
6. **Thresholds:** one FPGA register 0x12d4: [15:0] = D0-D7 DAC code, [31:16] = D8-D15. code = trunc(zero + V x 310.07),
   V clamped to ±15 V. zero = per-pod calibration code from `cal_la.hex`, for example 8609 (0x21a1) for D0-7 and 8580 (0x2184) for
   D8-15 (the zero codes in 0x12d4 are verified on the unit). Live comparator state is at 0x12dc (byte 0 = D0-7, byte 1 = D8-15).
7. **Probe detection:** GPIO 120 (GPIO3_D0, `/sys/class/gpio/gpio120/value`, `LA_PROBE_DETECT`), 1 = probe present. No probe EEPROM
   or ID.

## 1. Registers

| reg | bits / value | function |
|---|---|---|
| 0x4008 | bit26 | LA enable |
| 0x4008 | bit24 | TX engine sends the LA stream; bit23 = analog stream |
| 0x12a4 / 0x12a8 / 0x12ac | divider block | LA sample rate, section 3.2 |
| 0x128c | 424 or 145 | LA channel delay (deskew): 424 if m == 1, else 145 |
| 0x1290 / 0x1294 | [15:0] | LA display delay main / zoom (halfword, upper half kept) |
| 0x1298 / 0x129c | 0x40000000 \| en<<31 \| k (bits 28-29 kept) | LA compression main / zoom, same layout as 0x1170 |
| 0x1280 | bit2 / bit3 | LA linear interpolation off (1) / on (0), main / zoom (byte RMW of the copy) |
| 0x12b0 / 0x12b4 | 0 / (2^32-1) x den / num | LA interpolation step, main (0xffffffff when off) |
| 0x12b8 / 0x12bc | as above | zoom |
| 0x12c0 / 0x12c4 | words | LA TX length main / zoom |
| 0x12cc | words | LA chunk length in roll |
| 0x12d4 | [15:0] D0-7, [31:16] D8-15 | threshold DAC codes, one shared register (software default before any threshold is set: 0x21b721b4) |
| 0x12dc | read: byte0 D0-7, byte1 D8-15 | live comparator levels; the bit order (bit n = Dn) is assumed |
| 0x10e0 / 0x10e4 / 0x4050 / 0x4054 / 0x1100 / 0x1108 | x2 with LA | see 3.1 |

After a cold boot bit26 and bit24 are clear and bit23 is set (= `cold-boot.md` step 25).

## 2. Probe detection, probe ID

- Read `/sys/class/gpio/gpio120/value` (3 bytes, `atoi`): **1 = present**. Value 1 after absent = probe plugged in. Value != 1 after
  present = probe removed, and if the LA is on it is switched off.
- OOS reads the GPIO once a second. Without a probe the LA key, the LA card and the start menu refuse to switch the LA on, and a probe pulled while the LA is on switches it off (`board.md` 6). SCPI `:LA:STATe ON` is not refused: it is the way out if the detection reads wrong, because the polarity is not yet verified on hardware.
- No EEPROM, 1-wire or ID path exists for the LA probe. The probe type cannot be identified.

## 3. Enabling the LA (normal time mode)

### 3.1 What changes with the LA on

Let mask = analog FSM mask, ratio = the interleave factor (1/2/4), m = sample mode from this table (`la::sample_mode`):

| mask | 0, 1, 2, 4, 8 | 3, 5, 6, 9, 10, 12 | 7, 11, 13, 14, 15 |
|---|---|---|---|
| m | 1 | 2 | 4 |

Note: the stock table has m = 4 for masks 3 and 12 too. On the unit CH1+CH2 at m = 4 gave LA words at half the real spacing, with
seams in the record, at every time base. With m = 2 the record is right (test T3, verified with masks 3, 5, 12 and 7).

| step | effect with LA |
|---|---|
| rate | if LA on **and** any analog channel displayed: rate = rate/2, but 156.25 M becomes 62.5 M (not 78.125 M). memsel is **not** recomputed |
| segment | segsize = 2 x segment size for (memsel x ratio) (always with LA, roll too) -> 0x10e0, max_frames from the doubled size |
| window | first screen point x2 and screen points x2 (-> 0x1108 and 0x1100 = screen points + (0x48 \| ratio<<9) + k) |
| LA interp / compression | ratio 1: LA fraction = 2 x sinc x num / den, k_la = k. Ratio 2: sinc x num / den, k_la = k. Ratio 4: sinc x num / den, k_la = 2k |
| 0x4008 | bit26 = 1, bit23 = 1, bit24 = (LA && roll/scan) |
| 0x128c | 424 (m == 1) / 145 |
| LA rate | r_la = rate/2 (m 1), rate (m 2), 2 x rate (m 4) -> 3.2 |
| 0x10e4 | 2 x memsel (LA and not roll), else memsel |
| 0x4050 / 0x4054 | **2 x pre, 2 x post** (no roll test) |
| 0x131c | 0x02580000 \| x << 4 \| 4 with x = 0 / 3 / 6 for ratio 1 / 2 / 4 (x = 4 without LA, `la::wave_early`) |
| LA interp | 0x1280 and step registers: interpolation on if the fraction != 1, zoom with its own fraction |
| LA delay | 0x1290 = trunc((f + c) x num/den), f = the display delay value of the horizontal block (exact meaning not known), c = 0 (ratio 1), 2 (ratio 2), for ratio 4 the value is 2f + 6. Zoom copy to 0x1294 |
| LA compression + TX | 0x1298 with en = (k_la > 1), k_la; zoom 0x129c. 0x12c0 = roundup8(len) (= the analog display length 0x11f0), 0x12c4 = roundup8(zoom length) |

Order: the LA enable (0x4008 bit26) and the deskew 0x128c go in before the pre-processing reset (0x1000 bit 5)
(`acq::configure_la`).

Verified on the unit (10 ms/div, 100 kpts, both pods, 1 kHz on D0):

| field | LA, all CH hidden | LA + CH1 shown |
|---|---|---|
| mask / ratio | 1 / 1 (CH1 acquired for the trigger) | 1 / 1 |
| rate | 500 kSa/s | 250 kSa/s (halved) |
| memsel | 100,000 | 100,000 (not recomputed) |
| segment | 32768 = 2 x 16384 | same |
| first screen point / screen points | 50,000 / 100,000 (= 2 x 25,000 / 2 x 50,000) | 75,000 / 50,000 (= 2 x 37,500 / 2 x 25,000) |
| LA interp num/den, k_la | 2/1, 1 | 2/1, 1 |
| LA period in a raw export | 4 us = 2 / rate | 8 us = 2 / rate |

### 3.2 LA sample-rate encoding

```
if r_la > 62,500,000:  d = 625,000,000 / r_la
                       0x12a4 = 0x8000 | (d & 0xff)    (d == 5, i.e. 125 MSa/s: 0x8000)
                       0x12ac = 1
else:                  0x12a4 = 0
                       0x12ac = 125,000,000 / r_la
0x12a8 = 0
```
Examples: 625 M -> 0x8001/1, 312.5 M -> 0x8002/1, 62.5 M -> 0/2, 250 k -> 0/500. The fastest LA rate is 625 MSa/s
(m 1 with the analog channel hidden at 1.25 GSa/s, or m 4 at 312.5 MSa/s per channel unhalved). With an analog
channel shown the limit is 312.5 MSa/s. Max memory depth with LA: 31.25 M / 12.5 M / 10 M points for 1 / 2 /
3-4 channels (`horizontal-config.md` 1.1).

### 3.3 Timing relation

- LA samples per record: n_la = memsel x m / 2. LA period = 1 / r_la. Trigger at LA index pre x m / 2 (the index is assumed).
- Deskew between LA and analog is done by the FPGA through 0x128c (424/145) and the display delays 0x1290. Whether
  the raw window reads (4.2) need an extra offset is open (test T1).

## 4. Read-out

### 4.1 Live frames

Per acquisition: first the analog read (0x4008 bit23 = 1, bit24 = 0, frame request, read count x (16 + 2 x ratio x len) bytes,
de-interleave, `acquisition-start.md` 2). If that succeeded and the LA is on:

1. 0x4008 bit23 = 0, bit24 = 1
2. Stop sequence, play-last pulse (0x4004 bit2), FSM start (0x4008[3:0] = mask, 0x4004 |= 1)
3. one read of 2 x (len + 8) bytes (+ 2 x zoomlen + 16 if zoom), len = 0x12c0
4. skip 8 header words, then for each word w and n = 0..15: Dn[i] = (w >> n) & 1 (frame stride len + header)

Before the read loop 0x1010 bit31 = 0 is written once, so every live frame starts with the 16-byte
header (w0 0xfa05, w1 type: 0 analog, 2 LA; verified on the unit). These live LA words are the **display trace**: the FPGA has
already applied k_la, the LA interpolation (x2 for ratio 1) and the delay. So the LA buffer has one word per
analog display point (50,000 / 25,000 words in the table of 3.1; verified on the unit).

**Batch path:** the LA read is a play-last of one frame. With more than one frame per batch only the LA of the **last** frame
arrives. The LA of the other frames stays in their memory segments (the doubled segment). Playing them back with the type-4
window (0x4010/0x4014/0x4018) and 0x4008 bit24 = 1 is plausible but not yet verified on hardware.

### 4.2 Raw LA window (what OOS should use)

Verified on the unit with N = 1000 and N = 100,000: the words are correct, but only right after an acquisition of the stock system.

1. Pass-through additions (`la::passthrough`): 0x1298 = 0x129c = 0x40000001 (keep bits 28-29). 0x1280 bits 2 and 3 = 1,
   0x12b0 = 0x12b8 = 0, 0x12b4 = 0x12bc = 0xffffffff. 0x1290 / 0x1294 low half = 0. Plus the analog `scu::passthrough`.
2. 0x4008: bit23 = 0, bit24 = 1.
3. Window: **0x1100 = 10 x N**, **0x1108 = start**. In this recipe start is not divided by ratio
   (verified only with start 0).
4. 0x12c0 = 4 x roundup4(N) / ratio. 0x1010 bit31 = 1 (no header). 0x400c bit25 = 0.
5. Stop sequence, play-last pulse, FSM start. Poll 0x4034 bit13 before touching the play engine.
6. ONE read of 2 x roundup4(N) bytes. Words 0 .. min(N, n_la) - 1 are consecutive LA samples from the start of the record.
   Words beyond n_la = memsel x m / 2 are stale memory (verified on the unit).

Not yet verified: the unit of 0x1108 for LA. In the live path it is the doubled point index, and the LA advances
m/2 samples per analog point, so a non-zero start may need start x 2, start x m / 2 or start x 4 / ... (test T1). Also
not verified: 0x12c0 = 4N asks the FPGA for more words than are read. No xdma error shows up, and the next stop sequence's TX
reset is assumed to flush the rest. Try 0x12c0 = roundup8(N) with a read of exactly that many words before relying on it.

### 4.3 Analog read with the LA on

The analog record is laid out in the doubled point space too: the first screen point, the screen points (shared window
0x1108/0x1100), 0x10e4 and 0x4050/0x4054 are doubled. For a raw analog read the length is doubled, not the start:
0x1100 = (n << la) / ratio + (0x48 | ratio << 9), 0x1108 and 0x11f0 stay (`scu::Window::la`). Without the doubled length every LA
acquisition times out in the c2h read (assumed consequence, host only). The window start `scu::readout_window` (start/ratio) is
verified with the **LA off** only. With the LA on, check whether 0x1108 needs 2 x start (test T1) before trusting analog windows.

### 4.4 Roll / scan

- Acquisition block: 0x4008 bit23 = 1 **and** bit24 = 1. 0x10e4 not doubled (0x10e0 and 0x4050/0x4054 still are).
- Horizontal block, roll branch: 0x11fc = n / ratio, 0x12cc = ratio x (n / ratio), 0x12c0 = roundup8(len). Analog compression
  0x1170 with 10 x ratio x k. LA compression 0x1298 with (10 x ratio x k_la) >> s, s = 1 for m 1/2, s = 2 for m 4. n = 100 at 1-2-5
  time bases (`roll-mode.md` 2.2).
- Read: chunk = n + 8 words. With LA the chunk count is rounded down to even. One read, then each chunk goes to the analog or the
  LA buffer by **header word 1 bit 1** (set = LA). Then de-interleave the analog chunks (n / ratio points per channel per chunk) and
  take the LA chunks (n LA words per chunk, the header skipped), and build the rolling screens.

## 5. Thresholds

### 5.1 Path and formula

Clamp V to ±15,000,000,000 nV, then for each pod (D0-D7 "low", D8-D15 "high")

```
code = trunc( zero_pod + (f32)(f32(V_nV) x 1e-9f) x 310.07 )        (the sum in double, truncated to u32)
0x12d4 = code_D8_15 << 16 | code_D0_7                               (one register, both halves)
```
zero_pod = the pod's zero code (5.2). So that is **310.07 codes per volt**, with 0 V near the middle of the DAC range (≈0x2180,
assumed to be a 14-bit-like span). The DAC sits behind the FPGA (register write only; no SPI/GPIO from the SoC). Examples with zero
codes 8609 / 8580: 1.4 V (TTL) -> 9043 / 9014 (0x2353 / 0x2336), so 0x12d4 = 0x2336_2353. 0 V -> 8609 / 8580. D8-15 at 2.5 V -> 9355
(0x248b). D0-7 at +15 V -> 13260, at -15 V -> 3957 (truncated toward zero). Until a threshold is set, 0x12d4 holds the two zero
codes, i.e. 0 V (0x218421a1 with these codes, verified on the unit).

### 5.2 `cal_la.hex` (8-byte payload, container `self-cal.md` 7.1)

File: `/rigol/data/cal_la.hex`, else `/rigol/data/default/cal_la.hex`. 36 bytes. Payload = two u32
{zero D0-7, zero D8-15}, for example **0x000021a1 (8609), 0x00002184 (8580)**.

### 5.3 LA probe calibration

Per pod, with the probe inputs at 0 V: set both codes to 10000 (≈ +4.5 V), wait 10 ms, read 0x12dc. If the pod byte
is not 0 (some line above threshold), that pod's result is 0 (failure). Otherwise step the code down by 10 (10 ms per
step) until the byte reads 0xff (all eight lines high), then up by 1 until it reads 0 again. Result = about the middle
of that band + 5 codes. Store it, write both codes to 0x12d4 and save to **/rigol/data/cal_la.hex**. OOS must not run the saving
part (section 7).

## 6. Data format summary (for OOS)

| situation | stream | per word | words |
|---|---|---|---|
| raw window (4.2) | LA only, no header | bit n = Dn | N read, the first n_la = memsel x m / 2 are the record |
| live frame (4.1) | LA only, 8-word header | bit n = Dn, display-processed | len (= analog display length) |
| roll (4.4) | analog and LA chunks, 8-word header each, w1 bit1 = LA | bit n = Dn | n per LA chunk |

LA words never interleave with analog words inside a chunk or frame. The number of analog channels changes only the LA
rate (m) and the deskew, not the word layout.

## 7. Never do / read-only checks

Never:
- write `/rigol/data/cal_la.hex` or `/rigol/data/default/*`, for example by saving the calibration of 5.3. A RAM-only zero
  sweep would be possible but is not needed (the factory file exists).
- anything from the `cold-boot.md` never-do list (FPGA flash, `/rigol/data`, uboot/trust/parameter).
- arm the FSM with an empty analog mask (0x4008[3:0] = 0). Keep the trigger channel acquired, and skip reading when the mask is 0.
- drive the LA inputs negative from the generator during tests: it can hurt the probe's level-shifter inputs. Stay at 0..3.3 V.
- run a new register write on the unit without a dry run first.

Read-only checks (no register writes):
1. `cat /sys/class/gpio/gpio120/value` with and without the PLA2216 plugged -> confirms 1 = present.
2. Read 0x12dc with D0 at 0 V and at 3.3 V (TTL threshold set) -> byte0 bit0 should follow. That confirms bit order, pod mapping and
   that 0x12dc is readable outside the calibration.

## 8. OOS implementation plan

hal (module `la.rs`, small changes in `hz`, `acq`, `scu`, `seq`):
1. `la::Cal::load()` reads `cal_la.hex` through `calfile.rs` (fallback `default/`, fallback zero 0x2180 + a warning).
   `la::code(v, zero)` follows 5.1 bit-exactly (f32 product, then x 310.07 in f64, truncate, clamp ±15 V).
   `la::thresholds(...)` gives the write of 0x12d4, written on change and once after every OOS start.
2. `la::prep`: halve the rate by the rule of 3.1 (156.25 M -> 62.5 M), keep memsel, segment
   x2. `la::timing`: `m`, the LA rate, `n_la = memsel x m / 2`. Max depth caps for LA (31.25/12.5/10 M).
3. `acq::configure_la` and `la::config`: bit26 = la, bit24 = la && roll, 0x128c (424/145 from m), 0x12a4/0x12a8/0x12ac (3.2),
   0x10e4 x2 (not in roll), 0x4050/0x4054 x2. LA off: bit26 = 0, keep 0x128c.
4. `la::passthrough` (4.2 step 1) and `la::readout` (4.2 steps 2-5, start/N in LA samples
   once T1 has fixed the unit). The read is one `Step::Read` of 2 x roundup4(N) bytes. Afterwards restore bit23 = 1 / bit24 = 0
   (`la::restore`).
5. `seq`: `Settings::la`. Per acquisition: the analog window read (start x 2
   if T1 says so), then the LA window read of the same time span (N_la = n_analog x m / 2). Batch: one frame per batch while the LA is
   on until a per-frame type-4 LA play is tested. Mask: if no analog channel is shown, still
   acquire the trigger source channel hidden, unhalved rate.
6. Decoding: keep the packed words (`Vec<u16>`). Channel n = bit n. For the screen, compute per column and per channel
   (any 0, any 1) in one pass; both set = draw a transition. Time of word j = (j - pre x m / 2) / la_rate (assumed until T1).
7. Roll: 0x4008 bit24 stays 1, split chunks by header w1 bit1, LA chunk = n words (section 13).

UI: LA key (front panel `frontpanel.md`, key 0x02). Menu: LA on/off, D0-D15 show toggles, pod 1/2 thresholds
(presets TTL 1.4 V, CMOS 5 V 2.5 V, CMOS 3.3 V 1.65 V, LVCMOS 2.5 V 1.25 V, ECL -1.3 V, user ±15 V in 10 mV steps),
trace height. Probe state from gpio 120. Digital trigger sources D0-D15 are in
`trigger.md` (codes 32+n). The trigger is assumed to use the pod comparators, so no extra level is needed.

Device tests (after a dry run): **T1** the same 1 kHz 0..3.3 V square on CH1 and D0, LA on, mask 1,
2 us and 1 ms/div. Read analog with start S and 2S and LA with several start encodings, then compare edge times. That
fixes the window units and the LA-analog skew. **T2** threshold sweep through 0x12d4 with a DC level on D0, reading
0x12dc (checks 310.07/V and the zero codes). **T3** mask 3 + LA: is the m = 4 LA rate right (edge spacing)?

## 9. Not yet verified on hardware

1. Unit of 0x1108 (and 0x1100) for the LA stream in a raw read, and whether analog windows need 2 x start with the LA on (T1).
2. Mask 3 -> m = 4 while ratio = 2 (also 0x128c = 145, roll LA compression): does this match the hardware (T3)?
3. Exact frame length the FPGA sends for the raw LA read with 0x12c0 = 4N (4.2).
4. (unused)
5. The value f in the LA delay formula (3.1), and whether the deskew constants 424/145 also shift the raw window.
6. Per-frame LA of a batch (type-4 play with 0x4008 bit24 = 1).

## 10. OOS implementation (host only, not yet verified on hardware)

Built from sections 3-6 and the plan in section 8 without the probe at hand. Everything runs only while the LA is
switched on; with the LA off the analog path writes exactly what it writes otherwise (the acquisition block clears 0x4008
bit26 itself, 0x128c and the rate block keep their last values, as section 8.3 says).

| part | where | status |
|---|---|---|
| sample mode m, LA rate, n_la, trigger index | `hal::dho::la::sample_mode`, `timing` | 3.1 table including mask 3; trigger index pre x m / 2 is assumed (T1) |
| analog parameters with the LA | `hal::dho::la::prep` | rate halved with a channel shown (156.25 M -> 62.5 M), memsel kept, segment x2, frame ring from the doubled segment (assumed) |
| acquisition additions | `hal::dho::acq::configure_la`, `hal::dho::la::config` | 0x4008 bit26/bit23/bit24 (roll only), 0x128c 424/145, rate block 3.2, 0x131c, 0x10e4 x2 (not roll), 0x4050/0x4054 x2, 0x10e0 doubled |
| thresholds | `hal::dho::la::thresholds`, with every set-up while the LA is on | 5.1 bit-exact; zero codes from `cal_la.hex` (fallback 0x2180) |
| raw LA read | `hal::dho::la::passthrough`, `readout`, `restore`; `seq::State::la_read_steps`, `seq::acquire_la` | 4.2 steps 1-6, then 0x4008 bit23 = 1 / bit24 = 0 |
| batch | `seq::frames` | one frame per batch while the LA is on (only the last frame of a batch has its LA, 4.1) |
| depth / rate limits | `oos-ui` `Model::max_mem` (`Limits::mem(n, la)`), `max_srate` halved, `board::dho::hw::settings` caps the depth | 31.25 / 12.5 / 10 M |
| probe detect | `board/dho/hw.rs`, GPIO `LA_PROBE_DETECT` once a second | LA page title; without a probe the LA does not come on from the panel or the screen |
| UI | `ui/src/ui/la.rs` (LA key, Analysers > Logic analyser) | lanes, thresholds per pod, line selection, lane height, setups (`la = ...`), demo lines |
| roll / scan with the LA | section 13 | chunk split by header w1 bit1 |
| LA lines as decoder / trigger sources | section 11 | |

Assumptions in the code, to settle with the device tests of section 8:

1. **Play-last for the LA read.** OOS reads the analog record with the type-4 play, then plays the LA with the play-last
   pulse of 4.2 (verified on the unit only right after an acquisition of the stock system). Play-last right after a type-4 play is
   also the order of the live path (4.1). If the frame never ends, try the type-4 play (0x4010/0x4014/0x4018) with 0x4008 bit24 = 1
   instead (section 9 item 6).
2. **Window units (T1).** The LA read starts at word 0 and takes at most `la::READ_MAX` (1,000,000) words, so a longer record shows
   its first part only. The start of the analog read stays as it is and its length is doubled (4.3); whether the doubled point
   space also needs 2 x start is open.
3. **0x12c0 = 4 x roundup4(N) / ratio** as in the recipe, read 2 x roundup4(N) bytes (section 9 item 3).
4. **0x1280 / 0x1290 / 0x1294 / 0x1298 / 0x129c** written as whole words with the unknown bits 0 (no OOS copy).
5. **Rate block code for 125 MSa/s** (d = 5) written as 0x8000, as in 3.2.
6. **LA time axis.** Word j of the read is at (start + j - pre x m / 2) / r_la from the hardware trigger point and moves
   with the software trigger like the analog record. A constant LA-analog skew (deskew 424/145, display delay 0x1290)
   will show in T1.

First device test (after a dry run with `oos-fpgad plan --la --all`): T1 with a 1 kHz 0..3.3 V square on CH1 and D0,
LA on in oos-ui, 1 ms/div and 2 us/div; check that the LA read returns (no c2h timeout), that D0 follows CH1 in time,
and the threshold through 0x12dc (T2).

## 11. Display, decoders and remote access (host only)

- **Lane strip:** the lanes have their own strip under the graticule (`Ui::plot_rect`, `la_strip_h`), at most half of the
  height shared with it. The analog picture, its labels and markers shrink with the graticule, so nothing hides behind the
  lanes. No strip in the analysers and in XY.
- **Decoders:** Decode > Input = LA (shown while the LA is on). The packets are drawn in a row of their own under the lanes (with no lanes shown, in the band at the bottom of the plot). Each decoder role takes a line (`Model::dec_la`: source,
  second, third, fourth role, `None` where optional). `trig::decode_la` turns the lines into 0 / 1 waves and runs the
  normal decoders at threshold 0.5: the LA comparators have already set the levels, so the Threshold and B line rows are
  hidden. Parallel takes up to 16 lines upwards from the main source, without the clock line. The packets are cached per
  record and decoder settings. Saved in setups (`dec_la`).
- **Protocol trigger:** with Input = LA the Bus trigger follows the decoder onto the lines: a hardware edge on the line
  where frames start (SPI / I2S chip select, else the data line), the matching field found by `trig::decode_la` in the
  LA record, the record moved to it (`to_record`).
- **Trigger source (edge, pulse):** with the LA on, Trigger > Source lists D0..D15 after the channels (`Model::trig_la`,
  SCPI `:TRIGger:EDGE:SOURce D<n>`). Hardware: source number 9 + n, comparator code 32 + n, SPU bypass on, no level
  write; `tpu::edge` as in `trigger.md` 2.2 (event A selection (c,0,c,0), event B selection (c,1,c,1) for every slope, the slope
  only in the patterns; D0 gives 0x2034 = 0x6060). The trigger point is the hardware one: the analog samples
  cannot refine it (`to_record`, `fastdraw`), so the jitter is assumed to be one LA sample. Without an edge on the line in the
  record the frame counts as auto-triggered.
- **Pattern trigger:** Trigger > D0-D15 sets H / L / X per line (`Model::pat_la`, TPU slots 4..19, `trigger.md` 10.8), together
  with the channel bits. Edges only on the channels. As above the point is the hardware one; a frame counts as triggered
  when the pattern begins in the LA record. The duration trigger takes the same D lines.
- **SCPI:** `:LA:STATe`, `:LA:DIGital<n>:DISPlay` (n = 0..15), `:LA:POD<1|2>:THReshold` (sets the user preset, +-15 V),
  `:LA:POINts?`, `:LA:PREamble?` (points,dt,t0), `:LA:DATA?` (IEEE block, u16 little endian, bit n = Dn). Models without
  the port answer -241 "Hardware missing".
- **CSV:** Storage > CSV also writes `la-NNN.csv` (`la-<date>-<time>.csv` once the clock is set) while the LA is on:
  the lines shown, a row at the start, one per change and one at the end. A record of millions of samples stays small
  and nothing is lost.

## 12. Review notes

Details that section 10 relies on:

- **Analog window with the LA on:** 0x1100 = (n << la) / ratio + (0x48 | ratio << 9); 0x1108 and 0x11f0 stay (`scu::Window::la`).
- **Order:** LA enable and deskew 0x128c before the pre-processing reset: `acq::configure_la`.
- **0x131c:** `la::wave_early`, x = 4 again when the LA goes off (`State::la_was_on`).
- **Read limit:** the stock system stops at 1,000,000 words, the hardware does not: OOS reads the whole record from word 0.
- **Stop poll** before the LA read (0x4034 bit13, OOS allows 30 ms).
- **Failure paths:** a failed LA read restores the analog stream (`seq::acquire_la`) and after three failures in a row
  the LA read pauses until the next set-up, so that the analog keeps running.
- **Roll / scan:** the trigger stays on the channel there, which also keeps the acquisition running.
- **LA alone:** the trigger source channel is acquired hidden, not always CH1.
- **Slow time bases:** the done poll follows the record length.

## 13. Roll and scan (host only, not yet verified on hardware)

From 4.4. `la::config(roll = true)`: 0x4008 bit 24 on, 0x10e4 not doubled.
`roll::la_config` after `roll::config` / `config_scan`: 0x12cc = ratio x (100 / ratio), 0x12c0 = the length of 0x11f0,
0x1298 = 0xc0000000 | `la_compress`, 0x129c = 0x40000001, LA interpolation with the analog fraction (0x1280 = 0x08 with
interpolation on, 0x0c off; 0x12b0 = 0, 0x12b4 = the step; 0x12b8 = 0, 0x12bc = 0xffffffff), delays 0.

- `la_compress` = (10 x ratio x k_la) >> s, k_la = k, 2 k at ratio 4, s = 1 for m 1 / 2, 2
  for m 4. With m = ratio this gives an LA word every analog spacing / ratio, so a chunk of 100 LA words spans the time of
  an analog chunk; a host test checks that for all three ratios. At mask 3 (m != ratio) the spans differ.
- Read: an even number of chunks; `roll::split_la` separates them by header word 1 bit 1.
- Display: roll keeps a ring of 10 divisions of words, the newest word beside the newest analog point at the right edge
  (`board::dho::hw::roll_la_t0`); scan writes a sweep picture like the analog one, word 0 at the left edge, the part not yet written
  is left out.

Seen on the unit in scan: the chunks alternate LA, analog, and the chunk counter (header word 4) counts both, so analog chunk n
has the counter 2n (`roll::scan_column`). Not yet verified: whether they alternate 1:1 in every mode. The LA fraction at ratio 1
is assumed not to be doubled in live roll.
