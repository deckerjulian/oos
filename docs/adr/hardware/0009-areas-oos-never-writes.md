# ADR 0009: OOS never writes the FPGA flash, `/rigol/data`, `uboot`, `trust` or `parameter`

- Status: accepted
- Date: 2026-11-07 (first public version; the rule from the start)
- Category: hardware

## Context

OOS runs on scopes that belong to other people, and it is installed without opening the scope. Three areas cannot be
restored by the owner when they are damaged:

- the FPGA's configuration flash (`BOOT.bin`): without it the acquisition hardware does not start;
- `/rigol/data` with the calibration files of that one unit (`cal_*.hex`): they exist nowhere else;
- the partitions `uboot`, `trust` and `parameter`: the stock loader and its table; damage here ends every boot.

## Decision

OOS never writes any of them. The stock data partition is mounted read only (`oos-mount-rigol`). The FPGA is started
with the image already in its flash (the BOOT command), and calibration results of OOS go to `/oos`, not to
`/rigol/data`. A change that would need to write one of these areas is not accepted without a very good case.

## Consequences

- Whatever OOS does wrong, the stock system with its calibration is still on the card and bootable (ADR 0011).
- OOS cannot ship an FPGA image of its own, and it cannot repair or replace the stock calibration; functions that
  would need either are out of reach by this decision.
- The acceptance test checks it: `tools/device-test.py` takes checksums of the protected areas before and after a run
  (`protected_areas_before`, `protected_areas_after`).

## References

[CONTRIBUTING.md](../../../CONTRIBUTING.md) (ground rules), [architecture.md](../../architecture.md) (card layout),
`tools/device-test.py`.
