Added:

- Hardware segments can use the acquisition memory above 256 MiB where the unit has it: twice as many segments of a
  size. The stock software uses only 256 MiB; the other half was found by observation and measurement on a unit. It
  is off until **System > Probe optional memory** found it on the unit (next entry), and a short probe at the first
  recording of a segment size the button did not probe checks it again. System info shows whether it is in use
  ("Optional memory") ([horizontal-config.md](docs/protocols/horizontal-config.md#18-frame-memory-above-256-mib))
  (unit).
