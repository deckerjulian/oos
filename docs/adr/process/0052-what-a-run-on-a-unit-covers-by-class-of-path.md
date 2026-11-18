# ADR 0052: What a run on a unit has to cover depends on the class of the path

- Status: proposed
- Date: 2026-11-18
- Category: process

Amends ADR 0042.

## Context

The label `unit-tested` says "this was tried on a unit" (ADR 0038, ADR 0039). Three things showed that one label and
one list are not enough to say what was tried:

- The list of live paths in `tools/pr-gate.sh` named files. When `hw.rs` and `fpgad/src/main.rs` were split (#153),
  the board commands of oos-fpgad moved into files the list did not name, and a refactoring of them would have passed
  without a unit. It was noticed by chance.
- The development agent tries a change by running it from RAM (`tools/device-deploy.sh`, then
  `tools/device-test.py --changed`). That restarts `oos-ui`; it does not start the scope. What runs once at boot
  (`oos-fpgad cold-boot` and `fpga-boot` from the installed image, the scripts of the initramfs, the kernel, an
  update) is not reached: from OOS to OOS takes a power cycle, which nobody can do over the network.
- A split of `oos-fpgad`'s subcommands (branch `refactor/long-fns-fpgad`) had equal output on the host for 33
  commands, and still could not be called tried: its boot commands run only at boot, and `selfcal` and `roll`
  write to the board.

## Decision

Every file of the hardware paths has one class, and the class says what a pull request needs:

| Class | Paths | What proves a change |
|---|---|---|
| proven | `$proven` in `tools/pr-gate.sh`: the code that builds register sequences | the comparison with calibration values (ADR 0042); a change of behaviour: a run from RAM |
| live | `$live` without the boot class: `ui/src/board/`, the live paths of `hal/src/dho/`, the step executor, oos-fpgad's board commands | a run from RAM: `tools/device-deploy.sh`, `tools/device-test.py --changed`, `--restore`; the agent may do it and set `unit-tested` |
| boot | `boot/`, `kernel/`, `rootfs/`, `hal/src/update/`, `hal/src/dho/boot/`, `fpgad/src/bin/oos-update.rs`, and what `oos-fpgad cold-boot` and `fpga-boot` run | the image installed and the scope started from power off, with a person at the unit; only then `unit-tested` |

A file of the hardware paths that is in neither `$live` nor `$proven` counts as live, and a test fails until it is
classed. The pull request says which run was made and what it covered. The agent prepares a change of the boot class
(the branch, the host proofs, what to look at on the unit) and leaves the label to the maintainer.

## Alternatives

- **A second label (`boot-tested`) and a second status.** Clearer in the list of pull requests, but one more rule in
  the gate and in the ruleset for a case that is rare. Worth it once the agent can power cycle a unit by itself.
- **Derive the class from a marker in each file.** The class would sit next to the code, but the gate reads a pull
  request through the API and never its code; a list in the gate, kept complete by a test, needs no checkout.

## Consequences

- A split or a new file in the hardware paths cannot fall out of the list unseen.
- `hal/src/dho/seq/exec.rs` is live. Files of `hal/src/dho/` that mostly build steps but also read from the bus
  (`afe`, `avg`, `counter`, `dvm`, `mask`, `zone`, `adc`) stay proven, as they were; whether one of them belongs to
  live is worth a look when it changes.
- A switchable power outlet at the bench, driven by a tool, would let the agent cover the boot class too. That would
  be the moment for the second label.

## References

`tools/pr-gate.sh` (`$hardware`, `$live`, `$proven`), `tools/tests/test_pr_scripts.py`
(`test_every_file_of_the_hardware_paths_is_classed`), `tools/device-deploy.sh`, [docs/device-testing.md](../../device-testing.md),
ADR 0038, ADR 0039, ADR 0041, ADR 0042, pull requests #153 and #167.
