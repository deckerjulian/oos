# Logic analyser

Models with an LA port work with the PLA2216 probe: lines D0 to D15, hardware trigger on the lines, decoders, roll and scan.

![Logic analyser](https://raw.githubusercontent.com/dekajoule/oos/main/docs/images/logic-analyser.png)

Tap the **LA** card to switch it on and to choose the lines. Trigger and decoders can use the lines as a source: edge, pulse, pattern, duration and protocol trigger.

With the LA on the memory depth is limited to 31.25 M, 12.5 M or 10 M points for one, two or more channels.

## Known limits

- Thresholds are set as under stock, but have no effect on the unit tested, under stock neither.
- Only D0 to D2 have been driven with a signal so far.
- In 0.1.0, a protocol trigger in Normal freezes the picture in roll mode, and Single stops at the first edge. Both are
  fixed in 0.1.1.
