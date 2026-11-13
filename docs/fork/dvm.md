# DVM from the FPGA block

Status: implemented on `feat/dvm`, opt-in, not verified on the unit. Spec: `specs/08-counter-dvm.md` section 1.2.
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
| 0x2244 [5:1] | log2 of the samples per window; stock default 28 | netlist-proven (sim); meaning from the stock driver's division by 2^exp |
| 0x2244 [9:8] | lane: the channel with one, three or four channels; with two channels 0 for the lower, 2 for the higher | netlist-proven (sim) for the bits; lane choice from the stock driver |
| 0x2244 read | [9:8,5:0] read back, the other bits status flip-flops of unknown meaning | netlist-proven (sim) |
| 0x2248 [0] | latch: write 1 then 0 before reading the results | netlist-proven (sim) |
| 0x224c, 0x2250 [31:16] | first result, 48 bits (0x224c = bits 31:0, 0x2250 [31:16] = bits 47:32); 0x2250 [15:0] reads 0 | bit layout netlist-proven (sim): only [31:16] have flip-flops |
| 0x2254, 0x2258 | second result, 64 bits (0x2254 low) | netlist-proven (sim) as 64 status bits |

Not known, assumed in the decode (`hal/src/dho/dvm.rs`):

- The first result is the sum of 16-bit sample codes, the second the sum of their squares. The stock driver divides
  both by 2^exp and calls the second one an rms; that it is a sum of squares (and not, say, a sum of absolute values or
  of squares around a reference) is a guess.
- The samples are on the read-out scale, 7500 codes per division around 32768 (`Scale::ReadOut`). The alternative is
  the measure unit's scale before the digital gain (`Scale::PreGain`, as `levels.rs`).
- A window of 2^28 samples ends within 0.5 s and the latched result is a whole window.
- The block needs the channel in the acquired set, and keeps summing in STOP.

The decode refuses a reading whose sums cannot be a sum and a sum of squares of 16-bit codes (zero, mean outside the
code range, mean square below the mean squared), so a wrong guess shows up as the software value, not as a wrong one.

## Fixing the decode after the unit test

All of it is in `hal/src/dho/dvm.rs`, each with its table test:

- result layout: `Raw::sum`, `Raw::sq`, `SUM_HI_SHIFT`;
- sample scale: `SCALE`, `MID`, `FULL`, `CODES_PER_DIV`; 12-bit codes would show as a mean near 2048 and are refused
  only if they break the plausibility checks, so check the mean in codes first;
- meaning of the second result: `volts` (the moments `m1`, `m2`) and the check in `decode`;
- window length: `EXP` here, `DVM_READ_S` in `hw.rs`.

## On-unit test procedure

Needs the unit with an image of this branch, a DC source (a bench supply or a battery, measured with a handheld DMM)
and the probe compensation output (1 kHz square). Over SCPI port 5555, one open socket, about 150 ms between
commands. Record every answer of `:SYSTem:DEBug:DVM?` in the log.

Format of the answer: `ch,lane,exp,n` (n raises with every reading), `cfg` (0x2244 read back), the four result words,
`hw` = DC;AC rms;AC+DC rms in volts (or `none` when refused), `mode`, `sw` = the software DVM of the same source and
mode. `off,sw=...` without a reading.

1. Baseline with `dvm.hw` off (default): DVM on CH1, `:SYSTem:DEBug:DVM?` answers `off,...`. Nothing else changed.
2. Set `dvm.hw = on` in system.conf, restart oos-ui. `:SYST:SET dvm = true 0 0` (DVM on, CH1, DC).
3. Idle check: CH1 open, 1 V/div, offset 0. Read the dump 10 times, 0.6 s apart. Expect n to rise, `cfg` to read
   back 0x38 in [9:0] (lane 0, exp 28), the mean in codes (sum / 2^28) near 32768. Note how often the result words
   change between readings: that gives the window length.
4. DC: apply the reference to CH1 at 1 V/div (about 2 V), then 10 records worth of readings (10 dumps, one per
   record, `:SINGle` + `:SYSTem:DEBug:DVM?` each). Compare `hw` DC with `sw` and the DMM. Repeat at 200 mV/div with
   offset -2 V (the reference near the middle of the screen). If `hw` and `sw` differ by the digital gain (1.0x to
   1.5x), switch `SCALE` to `PreGain`.
5. Square: probe compensation on CH1, 500 mV/div, DVM modes 0, 1, 2 (`:SYST:SET dvm = true 0 <mode>`), 10 dumps each.
   Expect AC rms about half the amplitude, AC+DC rms sqrt(mean^2 + ac^2). If the second result divided by 2^28 is
   not near mean^2 + (amplitude/2)^2 in codes, it is not a sum of squares: note what it is.
6. Time base independence: repeat 4 at 5 ns/div and at 50 ms/div. `hw` must stay within its noise, `sw` may move.
7. Lanes: CH1 + CH2 on, square on CH2, DVM source CH2 (`:SYST:SET dvm = true 1 0`): expect lane 2. CH2 alone: lane 1.
   Four channels, source CH4: lane 3. The value must follow the signal on the source each time.
8. STOP: press STOP, read 5 dumps. Note whether n and the result words keep changing.
9. Off: DVM off. `:SYSTem:DEBug:DVM?` must answer `off`; the last write to 0x2244 is the stop pulse.

Record the results in `docs/fork/unit-dho804.md` and update the evidence column above.
