Added:

- Hardware segments use the acquisition memory above 256 MiB: twice as many segments of a size. The stock software
  uses only 256 MiB; the other half was found by observation and measurement on a unit. A short probe at the first
  recording that needs it checks that the unit has it, and System info shows whether it is in use ("Optional
  memory") ([horizontal-config.md](docs/protocols/horizontal-config.md#18-frame-memory-above-256-mib)) (unit).
