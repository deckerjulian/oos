# oos-fpgad: OOS acquisition control and bring-up tool

Command-line tool over `../hal` (`oos_hal::seq`, `oos_hal::boot`), in the OOS image as `/bin/oos-fpgad`. It replaces
the C tool `tools/hw/oos-hw` step by step. Later stage: a daemon that owns the FPGA/AFE and serves frames to `oos-ui`.

```
oos-fpgad plan [options]               # complete acquisition sequence, no hardware access (runs anywhere)
oos-fpgad run  [options] --execute     # on the unit: configure, arm, wait, read; frame to --out
oos-fpgad status                       # read only: FPGA version, FSM, DDR init, ADC clock 0x1210 (= PLL lock), play window
oos-fpgad rd <offset> [count]          # read BAR1
oos-fpgad wr <offset> <value> --execute   # write one BAR1 register (0x1008 refused)
oos-fpgad cold-boot                    # dry run of the cold boot against the unit model (runs anywhere)
oos-fpgad cold-boot --execute [--power-on [--slot 0x400000]]   # cold boot on the unit
oos-fpgad temp                         # read only: SoC thermal zones, FPGA die temperature / VCCINT (XADC 0x1438)
oos-fpgad beep [once|long]             # beeper through pwm-gpio (2 kHz, 20 %): 20 ms click / 4 s alarm
oos-fpgad guard [--period 1] [--power-off]   # over-temperature guard, see below
oos-fpgad help                         # all acquisition options
```

Acquisition sequence (docs/protocols): [ADC mode switch when the channel set differs from `--app-mask`] -> [AFEs with
`--afe`, then read-path gain/offset] -> TPU trigger -> wait FSM stopped -> acquisition configuration (incl. sample delays) ->
SCU auto/normal trigger -> arm -> wait done (0x4080 bit30) -> play-last read-out -> one read of
2 x ratio x n bytes. Defaults reproduce the warm hand-over of 2026-11-02 (99 writes).

Cold boot (`hal::boot`, cold-boot.md): BOOT command (only slots 0x0 and 0x400000; 0x800000 is refused on purpose), PCIe/
xdma bring-up with retries, PLL, AFEs, ADC init, SPU/SCU init, checks (DDR init pass, ADC clock 62494..62506),
calibrations (ADC mode 0, DDR taps, sync delay, lane eye scan, core alignment, ADC internal trim). Never sends any other boot-
monitor command, never writes under /rigol/data.

Over-temperature guard (`hal::board`, board.md sections 3 and 8.2): every period it reads the SoC thermal zones and the
FPGA XADC; "hot" = a SoC zone above 90 C or the FPGA die above 75 C. After 3 hot periods in a row it logs a
warning and plays the long beep, after 11 it powers off through the panel MCU (`sync`, then `FA 05 01` twice on
/dev/ttyS0 like the stock system), but only with `--power-off`; without it the guard only logs. Not started by rcS yet.

Safety: the FPGA version is checked before any write; `run`, `wr` and `cold-boot` write only with `--execute`.
Nothing of this has run on the unit yet.

Suggested first session: `status` (PLL/DDR read only) -> warm hand-over ->
`run --execute` twice (new frames?) -> `oos-ui --hw` -> channel set change -> `--afe` -> cold boot.
