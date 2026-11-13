# Two-channel read-path gain lanes (report for upstream)

## Symptom

With two channels acquired at different V/div, each channel's DC level is wrong by an amount proportional to its
distance from mid-screen, and the size depends on the other channel's scale. One channel alone reads right.

Measured on a DHO804 (2026-11-13): CH3 input open, 1 V/div, offset +2 V. With CH1 also acquired at 100 mV/div (shown,
or only as the trigger source) CH3's average reads +143 mV; at offsets 0 / +1 / -2 V it reads +1.5 / +68 / -158 mV,
about 7 % of the offset. With CH3 alone it reads -30 mV.

## Cause

`hal/src/dho/wpu.rs`, `analog_gnd`: the per-lane read-path gain and offset registers (0x14c8 / 0x14d4 / 0x14e0 /
0x14ec, offsets 0x14d0 / 0x14dc / 0x14e8 / 0x14f4, and the zoom copies) are filled with the lane order of the 0x1004
invert bits. With two channels that is lanes 0, 1 = lower channel and 2, 3 = higher channel. The stock firmware
writes the read-path lanes in a different order for two channels: lower, higher, lower, higher. (The invert and filter
bits of 0x1004 do use 0, 1 / 2, 3; only the read path differs.)

So part of each channel's words passes through the other channel's digital gain and offset. The offset register is
centred (32768 - 32768 x gain), so the error is (gain of the other channel / own gain - 1) x the word's distance from
mid-scale: zero at mid-screen, growing with the channel offset or a DC signal, and zero when both channels use the same
digital gain.

## Fix

One line in `analog_gnd`:

```rust
-        2 => [on[0], on[0], on[1], on[1]],
+        2 => [on[0], on[1], on[0], on[1]],
```

One and four channels are unchanged (all lanes = the channel; lane n = CH n+1).

## How to verify

- Host test: with mask 0b1010 and digital gains 1.5 / 1.25, the gain registers must read 1536, 1280, 1536, 1280 (main and
  zoom), the offsets the matching centred values.
- On a unit: one input open, its channel at 1 V/div and +2 V offset; a second channel acquired at 100 mV/div. Before the
  fix the open channel's average sits several percent of the offset away from its single-channel reading; after it the
  two should agree (not yet confirmed on the unit at the time of writing). Setting the second channel to the same V/div
  as the first makes the error vanish even before the fix, which shows it is the gain mix.
