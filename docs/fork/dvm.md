# DVM from the FPGA block

Status: opt-in. Result layout and units measured on the unit (DHO804, 2026-11-13, round 2); decode checked
against the software DVM in round 5 (below). The register fields, the lanes and the scale were measured again on a
DHO924S on 2026-11-16 ("On the DHO924S" below).
Next-round table row 2.5.

## What it does

With `dvm.hw = on` in system.conf the DVM (Menu > Instruments > DVM / counter) reads the FPGA's DVM block instead of
the measurements of the transferred records. The block sums 2^exp samples of one ADC lane, so the reading does not
depend on the time base, the record length or what part of the record is transferred. Default off: the software DVM
stays as it was, and with `dvm.hw` off OOS never writes the block.

Code:

| part | where |
|---|---|
| registers, writes, read, decode, volts | `hal/src/dho/dvm.rs` |
| start, stop, poll in the acquisition thread; `dvm.hw` | `ui/src/board/dho/hw.rs` (`dvm_step`, `dvm_reading`, `dvm_hw_on`) |
| UI value and fallback | `ui/src/ui/tools.rs` (`dvm_poll`, `dvm_value`) |
| raw dump | SCPI `:SYSTem:DEBug:DVM?` |

The acquisition thread starts the block when the DVM is on with a hardware source, and restarts it after every
acquisition configuration (the stock firmware rewrites the DVM word on every configuration too) and when the source
or its lane changes. Start and stop happen with nothing armed. It reads the block 0.5 s after the start and every 0.5 s
after that. A reading the decode refuses, a reading of another channel, or no reading: the UI shows the software DVM.

## Register facts

| register | content | evidence |
|---|---|---|
| 0x2244 [0] | restart: written 1, then 0 together with the new fields | assumed; the restart meaning is inferred. On the DHO924S new fields also take effect without the pulse |
| 0x2244 [5:1] | log2 of the samples per window; stock default 28 | verified on the unit (DHO924S): with 24 and 20 written the sums are 2^4 and 2^8 times smaller and come that much faster |
| 0x2244 [9:8] | lane: the channel with one, three or four channels; with two channels 0 for the lower, 2 for the higher | verified on the unit (DHO924S) with one, two, three and four channels: each reading follows its channel |
| 0x2244 read | not the configuration alone: bits 9:8 and 5:0 read the written fields, bits 7:6 change between readings, bits 31:10 are those of `SQ_HI` | measured (DHO924S; the first session saw bits 7:0 replaced) |
| 0x2248 [0] | latch: write 1 then 0 before reading the results | assumed |
| 0x224c, 0x2250 [31:16] | sum of the samples, 48 bits: `(0x2250 >> 16) << 32 \| 0x224c` | measured |
| 0x2254, 0x2258 | sum of their squares, 64 bits: `0x2258 << 32 \| 0x2254` | measured (a square's sigma matches half its amplitude) |

Samples (measured): stored 12-bit ADC codes, mid-scale 2048 (open input: mean 2046.9, sigma 3.5), before the
read-path gain. Volts: `(code - 2048) x 16 x dg / 7500 x V/div - offset`, dg = the channel's digital gain from the
cal record (halved when 2 or more, the ADC x2 acts before storage). With the unit's cal file that is about 436 codes
per division at 1 V/div and 437 at 500 mV/div (measured 428 and 438), and 400 at 100 mV/div. The unit session
estimated about 200 at 100 mV/div from the probe-comp square, assuming a 0.3 V amplitude; that disagrees by a factor
2 and is the first thing the re-check settles.

Other measured facts: mode and time base change nothing in the words; a new result every 1 to 2 readings (about
0.6 s apart); after STOP one nearly all-zero window (sum 0x900), then the block runs on. `decode` refuses a mean below
one code, at or above 4096, and a mean square below the mean squared.

CH2 alone reported lane 2 in the first session where `dvm::lane` gives 1 (the set-up still had CH1 acquired, see
round 5). On the DHO924S CH2 alone is lane 1, and four channels are lanes 0 to 3 (below).

## Round 5: hardware against software

| setting | hw DC; AC rms; AC+DC | sw | difference |
|---|---|---|---|
| CH1 500 mV/div, offset 0, probe comp | 0.1446; 0.1499; 0.2083 V | 0.1477; 0.1487; 0.2095 | DC -3.1 mV (-2.1 %), AC +0.8 %, AC+DC -0.6 % |
| CH1 100 mV/div, offset -0.15 V | 0.1479; 0.1499; 0.2106 | 0.1482; 0.1497; 0.2107 | DC -0.3 mV, AC +0.1 % |
| CH3 open, 1 V/div, offset +2 V | -0.0337; 0.0056; 0.0341 | (sw showed CH1's signal, see below) | no valid comparison |

Reading:

- Scale: the AC values agree within 0.8 % and 0.1 %. The cal-file gain is right; no factor 2 at 100 mV/div.
- DC at 500 mV/div: -3.1 mV is about 2.7 ADC steps (1 step = 16 x 1.07 x 0.5 / 7500 V = 1.14 mV), while AC agrees.
  The block sums one lane, and with one channel a lane is one pair of the eight ADC cores. The software averages all
  cores. A lane's DC therefore differs from the all-core DC by the cores' offset residuals, which self-cal only trims
  to +-1.5 steps per core (more if the stored offsets belong to another mode or were not loaded after the hand-over).
  The same residuals limit the hardware average. Not a decode error.
- 1 V/div with +2 V: -34 mV is 1.7 % of the 2 V offset. Without a valid software value it cannot be separated from
  the AFE offset accuracy, which the records share. The 428 codes per division of round 2 came from this same reading
  (it assumed the offset was exactly 2 div), so it is not evidence of a scale error either.
- STOP and RUN: no zero reading reached the strip.
- Lane 2 for CH2 alone (round 2): the trigger was on CH1, which OOS acquires even when hidden, so the set was CH1 +
  CH2 and lane 2 is right (Rigol's rule, assumed).

Per-lane check (needs this branch): `:SYSTem:DEBug:DVM:LANE 0..3 | AUTO` forces the lane with one channel acquired
(every lane carries the channel then). CH1 alone with the trigger on CH1 (OOS also acquires a hidden trigger
channel, which makes it a two-channel set), input open, 500 mV/div and 100 mV/div, offset 0; for each lane 0..3
five `:SYSTem:DEBug:DVM?` dumps, then `AUTO`. Expect the four lane DC values to spread by a few ADC steps, with the
software value near their mean. That spread is the per-core-pair offset map, also the input for the core offset fix.

Default: keep `dvm.hw` opt-in until the per-lane check and the core offset fix. With the fix the DC error is bounded
by the trim (about +-0.5 step); then the hardware DVM is the better default: acquisition-independent, 2^28 samples,
no record or clipping dependence.

## On the DHO924S (2026-11-16)

`dvm.hw = on`, the built-in generator on CH1 (DC levels, then a 1 kHz square of 1 Vpp), the probe compensation output
on CH2, CH3 and CH4 open. Hardware (`hw`) against the software DVM (`sw`) and the level set on the generator.

| check | result |
|---|---|
| DC, 500 mV/div, 0 / 0.5 / 1.0 / 1.5 / -1.0 V | hw - sw: -1.5, +0.3, 0.0, -0.4, +2.5 mV; both within 4 mV of the set level |
| DC, 200 mV/div with -1 V offset, 0.6 / 1.0 / 1.4 V | hw - sw: +0.9, -0.1, 0.0 mV |
| DC, 1 V/div, 0 / +2 / -2 V | hw - sw: -2.0, +2.7, +5.4 mV (1 ADC step is 2.3 mV here) |
| DC, 100 mV/div, 0 / 0.2 / -0.3 V | hw - sw: -0.1, +0.1, +0.5 mV |
| codes per division, from two levels | 435 at 1 V/div and 500 mV/div, 402 at 200 mV/div, 401 at 100 mV/div: the cal-file formula |
| 1 kHz square, 1 Vpp, 500 mV/div | DC -0.2 mV, AC rms 0.5027 V, AC+DC 0.5027 V; sw 0.5030 to 0.5033 V. The second result is the sum of squares |
| time base 5 ns/div, 1 us/div, 100 us/div | hw AC+DC 0.50276, 0.50284, 0.50269 V: no dependence |
| time base 50 ms/div (125 kSa/s) | hw AC+DC 0.49936 V, DC +3.5 mV: 0.7 % lower. Not explained |
| window | with 28 a new result every 1.71 s on average: 2^28 samples at 156.25 MSa/s, the rate of one lane |
| first result after a level change | spans the change (a mean between the levels, a large sigma): one window is one sum |
| CH1 + CH2 | CH1 lane 0 reads the generator level, CH2 lane 2 the probe-comp square (DC 1.514 V, sw 1.531 V) |
| CH2 + CH3 | CH2 lane 0, CH3 lane 2 |
| three and four channels | lane = channel; CH1 0.499 V, CH2 1.518 V, CH3 and CH4 their open-input levels, each within 5 mV of sw |
| CH2 alone | lane 1, reads CH2 |
| CH1 alone, lane forced 0 / 1 / 2 / 3 | mean codes 2487.2 / 2484.1 / 2485.3 / 2486.3: every lane carries the channel; 3.2 codes of spread between the core pairs; sw at their mean |
| STOP | the dump stays as it is (the thread does not poll while stopped); after RUN the readings go on. The all-zero window of the first session did not show |

Still assumed: the latch (0x2248) and what the restart bit does beyond loading the fields.

## Original on-unit test procedure (first session)

Needs the unit with an image of this branch, a DC source (a bench supply or a battery, measured with a handheld DMM)
and the probe compensation output (1 kHz square). Over SCPI port 5555, one open socket, about 150 ms between
commands. Record every answer of `:SYSTem:DEBug:DVM?` in the log.

Format of the answer: `ch,lane,exp,n` (n raises with every reading), `cfg` (0x2244 read back), the four result words,
`hw` = DC;AC rms;AC+DC rms in volts (or `none` when refused), `mode`, `sw` = the software DVM of the same source and
mode. `off,sw=...` without a reading.

1. Baseline with `dvm.hw` off (default): DVM on CH1, `:SYSTem:DEBug:DVM?` answers `off,...`. Nothing else changed.
2. Set `dvm.hw = on` in system.conf, restart oos-ui. `:SYST:SET dvm = true 0 0` (DVM on, CH1, DC).
3. Idle check: CH1 open, 1 V/div, offset 0. Read the dump 10 times, 0.6 s apart. Expect n to rise, `cfg` to read
   the mean in codes (sum / 2^28) near 2048. (Done 2026-11-13: mean 2046.9, a new result every 1 to 2 readings.)
4. DC: apply the reference to CH1 at 1 V/div (about 2 V), then 10 records worth of readings (10 dumps, one per
   record, `:SINGle` + `:SYSTem:DEBug:DVM?` each). Compare `hw` DC with `sw` and the DMM. Repeat at 200 mV/div with
   offset -2 V (the reference near the middle of the screen). `hw` and `sw` must agree within 2 %.
5. Square: probe compensation on CH1, 500 mV/div, DVM modes 0, 1, 2 (`:SYST:SET dvm = true 0 <mode>`), 10 dumps each.
   Expect AC rms about half the amplitude, AC+DC rms sqrt(mean^2 + ac^2). If the second result divided by 2^28 is
   not near mean^2 + (amplitude/2)^2 in codes, it is not a sum of squares: note what it is.
6. Time base independence: repeat 4 at 5 ns/div and at 50 ms/div. `hw` must stay within its noise, `sw` may move.
7. Lanes: CH1 + CH2 on, square on CH2, DVM source CH2 (`:SYST:SET dvm = true 1 0`): expect lane 2. CH2 alone: lane 1.
   Four channels, source CH4: lane 3. The value must follow the signal on the source each time.
8. STOP: press STOP, read 5 dumps. Note whether n and the result words keep changing.
9. Off: DVM off. `:SYSTem:DEBug:DVM?` must answer `off`; the last write to 0x2244 is the stop pulse.

Record the results here and update the evidence column above.
