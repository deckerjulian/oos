# Frequency counter (FPGA, DHO800/DHO900)

Scope: the hardware counter behind Menu > Instruments > DVM / counter on the unit. Code: `hal/src/dho/counter.rs` (registers and
the conversion), `ui/src/board/dho/hw.rs` (`counter_tick`, in the acquisition loop), `ui/src/ui/tools.rs` (the strip
row, statistics, SCPI `:COUNter`). Off the hardware (host runs, demo source) the counter is worked out from the records.

## Short answer

- **A reciprocal counter on one channel's comparator**, all through BAR1 0x2200..0x2240. Over a gate it counts the
  signal's edges and measures, in 0.8 ns ticks, where the first and the last edge lie in the gate. The period is the
  time between them divided by the edges less one, so the resolution is about 1.6 ns over the gate at every
  frequency: 6 digits and more at 1 s. Its accuracy is that of the scope's clock.
- **Slow signals**: it also keeps one whole period; below about 625 Hz (fewer than two edges in a short gate) that one
  counts, from 1.6 ms up.
- **Totalize**: a running count of edges since the counter was started or cleared.
- **The source channel must be acquired** (on), as for the trigger.

## 1. Registers (BAR1, 32 bit)

| Offset | Dir | Meaning |
|---|---|---|
| 0x2200, 0x2204 | W | 0x8080 while counting (comparator set-up; meaning of the value not known) |
| 0x2208 | W | 1: an analog source |
| 0x220c | W | the source channel as a mask: CH1..CH4 = 1, 2, 4, 8 |
| 0x2210 | W | control: bit 0 run, bit 1 totalize, bit 2 clear; the source number in [9:5] and again in [14:10] (CH1..CH4 = 1..4) |
| 0x2214 | W | gate in 6.4 ns steps (1 s = 156 250 000) |
| 0x2218 | W | 1: latch the results of the last gate for reading |
| 0x221c, 0x2220 | R | start: 0.8 ns ticks from the gate's start to the first edge; 0x2220 bit 31 = a frequency result |
| 0x2224, 0x2228 | R | end: ticks from the last edge to the gate's end |
| 0x222c, 0x2230 | R | edges in the gate |
| 0x2234, 0x2238 | R | one whole period, ticks; 0x2238 bit 31 = valid |
| 0x223c, 0x2240 | R | total edges since the start |

Each result is 64 bit, the low word first; bit 31 of the high word is the valid flag where the table says so.

## 2. Sequence

1. Start (and at every change of source, mode or gate): 0x2200 = 0x2204 = 0x8080, 0x2208 = 1, 0x220c = mask,
   0x2210 = control without run, 0x2214 = gate, 0x2210 with clear, 0x2210 with run.
2. Every gate plus 0.1 s: 0x2218 = 1, then read 0x221c..0x2240.
3. Stop: 0x2210 = 0.

Example, CH1, frequency, 1 s gate: 0x220c = 1, 0x2210 = 0x0420, 0x2214 = 0x09502f90, 0x2210 = 0x0424, 0x2210 = 0x0421.

## 3. The period

With G = gate / 0.8 ns: if the frequency result is valid and there are at least two edges,

    period = (G - start - end) / (edges - 1) * 0.8 ns

else, if the single period is valid and at least 2 000 000 ticks (1.6 ms), period = that * 0.8 ns; else no result.

Resolution sets the gate: 0.25 s for 3 digits, 0.5 s for 4, 1 s for 5 and more.

## 4. Verified on the unit (DHO924S, 2026-11-12)

Built-in generator on CH1, 2 Vpp sine:

| Signal | Counter |
|---|---|
| 12345.6 Hz, 1 s gate | 12345.6001 Hz and 12345.5994 Hz from the raw registers; 12345.60 Hz through the UI (6 digits) and with a 0.5 s gate |
| 1.5 Hz | 1.500046 Hz (single period) |
| 9876543.2 Hz | 9.876543 MHz |
| 1 kHz, period mode | 1.000000 ms |
| 1 kHz, totalize | about 1000 edges a second; clear starts it again |

Not tried yet: the meaning of 0x2200/0x2204 (a threshold near the screen centre, or following the trigger level),
digital sources (D0..D15 = source numbers 16..31, mask 0), the tick with more channels on.
