# DVM from the FPGA block

Status: opt-in. Result layout and units measured on the unit (2026-11-13, round 2: private RE notes
`docs/logs/oos_unitlab_20261113.md`); decode fixed on `fix/dvm-decode`, not yet re-checked against the software DVM. Spec: `specs/08-counter-dvm.md` section 1.2.
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
| 0x2244 [0] | restart: written 1, then 0 together with the new fields | write path netlist-proven (sim); the pulse is how the stock driver writes it, the restart meaning is inferred |
| 0x2244 [5:1] | log2 of the samples per window; stock default 28 | netlist-proven (sim); unit: N = 2^28 |
| 0x2244 [9:8] | lane: the channel with one, three or four channels; with two channels 0 for the lower, 2 for the higher | netlist-proven (sim) for the bits; unit: with CH1 + CH2 the lane-2 reading follows CH2 |
| 0x2244 read | not the configuration: `SQ_HI` with bits 7:0 replaced | measured |
| 0x2248 [0] | latch: write 1 then 0 before reading the results | netlist-proven (sim) |
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

Open: CH2 alone reported lane 2 where `dvm::lane` gives 1 (one channel: every lane carries it, so the reading is
right either way; the log line may have come from a set-up with CH1 still acquired). Four channels not tried.

## Re-check after the decode fix

1. `dvm.hw = on`, CH1 probe comp at 500 mV/div, offset 0, 10 dumps of `:SYSTem:DEBug:DVM?` per mode (0, 1, 2):
   `hw` within 2 % of `sw` (DC about 0.15 V, AC rms about 0.15 V).
2. The same at 100 mV/div, offset -0.15 V, and at 1 V/div with offset +2 V and the input open. `hw` against `sw` at
   each: a factor 2 at 100 mV/div means the samples sit before the ADC x2 after all; the range's digital gain is in
   the configuration note `AFE CH1: ... digital gain`.
3. STOP, then RUN: no wrong value on the strip (the zero window shows the software value for one reading).

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

Record the results in `docs/fork/unit-dho804.md` and update the evidence column above.
