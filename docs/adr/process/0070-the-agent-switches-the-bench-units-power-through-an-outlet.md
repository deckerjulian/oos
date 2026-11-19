# ADR 0070: The agent switches the bench unit's power through an outlet, and runs the boot class itself

- Status: proposed
- Date: 2026-11-19
- Category: process

## Context

ADR 0052 puts every change of the start and the update (`boot/`, `kernel/`, `rootfs/`, `hal/src/update/`, the cold
boot of the FPGA) in the class `boot`. Such a change is proven only by the image installed and the scope started from
power off. That needs a person at the bench: on 2026-11-18 and 2026-11-19 several pull requests (#240, #288, the
release 0.1.14) waited for one. ADR 0052 named a switchable outlet as the moment to look again. The maintainer decided
on 2026-11-19 to put one at the bench.

## Decision

**The bench unit is powered through an outlet that the agent switches with `tools/bench-power.sh` (Shelly, Tasmota or
any program; its address in `local/bench.conf`, which stays out of git). With it the agent runs the class `boot`
itself: it installs the image, starts the scope from power off and checks it, and then sets `unit-tested`.**

- The same permission as for any run on a unit: the maintainer has said in the session that the agent may use the
  scope (AGENTS.md), and the other sessions agreed.
- The tool cuts the power only when the scope answers over SSH and no `oos-update` runs (an install, a rollback, the
  start-up watcher). An install started by kexec has no SSH and says "Do not power off": the tool refuses then.
- `--force` only when the scope has not answered for 10 minutes and its screen cannot be asked. A unit that is stuck
  is what the outlet is for, and a power cut never touches the FPGA flash, `/rigol/data`, `uboot`, `trust` or
  `parameter` (ADR 0009).
- A run of the class `boot`: install (`oos-update install`, dry run first), power off for 30 s and on (`cycle`), wait
  for the start-up watcher to confirm, `tools/device-test.py --tier a0`. The pull request names each step and the
  commit it ran for (ADR 0069).
- One label, `unit-tested`, for both classes. The pull request text says which class ran.

## Alternatives

- **Keep the boot class with the maintainer.** Every change of the start and the update waits for a person, for days at
  times. The power cut it needs is the same one the maintainer makes by hand.
- **A second label `boot-tested`** (ADR 0052). With the agent running both classes, one label and the pull request's
  text say the same. A second label is one more rule in the gate.
- **A relay driven by a computer's GPIO.** It needs wiring at mains voltage at the bench. A certified plug does the same.

## Consequences

- Changes of the start, the update and the card scripts (#240, #288) can be proven without the maintainer at the bench.
- `tools/bench-power.sh` was written before the outlet was there: its commands for Shelly and Tasmota are not yet
  verified on hardware. The first use checks them, and this record is accepted after that.
- A cold start that leaves the scope in the stock system (one-shot boot mode) is a valid outcome: the scope answers on
  the stock system's ports, not OOS's. A boot-class run sets persist mode first or accepts that.
- The outlet's address is a fact of one bench and stays in `local/`.

## References

ADR 0009, ADR 0018, ADR 0052, ADR 0069, `tools/bench-power.sh`, `tools/tests/test_bench_power.py`,
docs/device-testing.md, docs/update.md.
