# AUX OUT

The rear AUX OUT BNC. System > AUX out, `:SYSTem:AOUTput`, setup key `aux_out`. Code: `hal/src/dho/auxout.rs`,
written by `seq::State::configure`.

## Hardware

Static analysis of the FPGA image. Not measured on the unit yet.

- One FPGA output pin, always driven, combinational from a source multiplexer. That it reaches the BNC through a
  buffer is inferred from the register use, not from a board trace.
- 0x1430 bits 2:0 select the source. The other 29 bits do not reach the pin.

| 0x1430[2:0] | pin |
|---|---|
| 0, 4..7 | trigger out: a positive pulse per trigger, about 16 sample clocks (100 ns) or shorter when the acquisition ends first |
| 1 | a trigger status flag (not offered) |
| 2 | constant low |
| 3 | the pass/fail pulse |

- Power-up value of all 32 bits of 0x1430 is 0 (the flip-flop INIT values of the FPGA image). OOS wrote nothing to
  0x1430 before this change, so AUX OUT has always carried trigger out under OOS. That stays the default.
- No polarity or width register exists for trigger out.

Pass/fail pulse configuration, 0x119c, counted in 6.4 ns sample clocks:

| bits | meaning |
|---|---|
| 23:0 | pulse width in clocks (1 .. 2^24-1) |
| 28:24 | unused, written 0 |
| 29 | source: 1 = zone trigger result, 0 = mask comparator result |
| 30 | idle level: 1 = idle high, low pulse; 0 = idle low, high pulse |
| 31 | event: 1 = pulse on pass, 0 = pulse on fail |

At most one pulse per acquisition: the generator arms when the result of the selected source is available and resets
to the idle level when the acquisition ends. That a comparator hit means "fail" is inferred, not measured.

## What OOS does

- `Settings::aux` (`auxout::AuxOut`): `TriggerOut` (0x1430 = 0), `Off` (0x1430 = 2), `PassFail(PassFail)` (0x119c
  first, then 0x1430 = 3).
- Written in the acquisition block of `configure` when it differs from the last written value. The first set-up after
  start writes it once, so the FPGA state never depends on what an earlier run left behind. A change of the setting
  reconfigures like any other settings change.
- SCPI: `:SYSTem:AOUTput TOUT|OFF`, query returns `TOUT` or `OFF`. `TOUT` and `PFAil` are the stock names; `OFF` is
  an OOS addition (stock offers only trigger out and pass/fail).

## Pass/fail

The pass/fail source is the FPGA mask comparator. A software mask result cannot reach the pin: no register pulses it
directly. So pass/fail works only with the hardware mask test (`mask.hw = on`, `hwmask.md`) or the hardware
zone trigger (`zone.hw = on`, `hwzone.md`, 0x119c bit 29 set).

- hal: `auxout::PassFail { width, source, event, idle_high }`, `PassFail::word()`, field constants in `auxout::pf`,
  `PassFail::DEFAULT` (1 us high pulse on a mask failure, an OOS choice).
- ui: the `Pass/fail` menu choice and `:SYSTem:AOUTput PFAil` are offered only with `mask.hw = on` or
  `zone.hw = on`. A setup file takes the value either way; without a hardware test the output then stays low.
- Width, event and polarity have no menu items yet; the menu uses the default.

## On-unit check

Needs a probe on CH1 (probe compensation output) and a BNC cable from AUX OUT to CH2. Nothing here writes outside
0x1430 and 0x119c.

1. Before changing anything, `:SYSTem:AOUTput?` returns `TOUT`. Over SSH, `oos-fpgad rd 0x1430` (read-only): expect 0x00000000, the power-up value OOS also writes for trigger out.
2. CH1 on the probe-comp square wave, edge trigger CH1 rising at 50 %, Normal sweep, 200 ns/div. CH2 1 V/div DC,
   50 ohm off.
3. Expect on CH2: one positive pulse of about 100 ns per trigger, starting near the trigger point, LVCMOS level after
   the buffer (record the amplitude). Change the trigger to falling: the pulse follows. Single: one pulse per Single.
   Stop: no further pulses. At 1 ms/div with Normal sweep the pulse rate equals the trigger rate on the status bar.
4. `:SYSTem:AOUTput OFF`: CH2 flat at 0 V. `:SYSTem:AOUTput TOUT`: pulses return. Restart oos-ui: the setting comes
   back from the setup and is written once at the first set-up.
5. With the hardware mask test (`hwmask.md`): choose Pass/fail, use a mask the signal violates. Expect one
   1 us high pulse per failed acquisition, none for passed ones. Then check `idle_high` and `event = Pass`.

Record the results in `docs/fork/unit-dho804.md`. If step 3 shows nothing, check with a second scope whether the BNC
is driven at all before suspecting the select value.
