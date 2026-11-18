# Testing on a unit

`tools/device-test.py` tests a running OOS over the network, with nobody at the scope. It reads the card and
the calibration but never writes them; the build under test runs from `/tmp`
(`tools/device-deploy.sh IP`, back with `--restore`). A full run of the tiers a0 to a2 takes about 17 minutes.
Most changes need only a part of it, and a measurement on real hardware is sometimes noisy. The options below keep a
run short and its verdict sound, so that it does not have to be repeated by hand.

## A typical run for a pull request

```sh
tools/device-test.py --host IP --changed origin/main --list   # which tests, and why (no scope needed)
tools/device-deploy.sh IP                                    # the build of this tree, in RAM
tools/device-test.py --host IP --changed origin/main --plan plan.txt
tools/device-deploy.sh IP --restore                          # the installed oos-ui again
```

`plan.txt` is `OOS_CAL_DIR=local/cal tools/plan-matrix.sh fpgad/target/release/oos-fpgad > plan.txt` on this
computer, with the calibration files of the same unit.

## SSH on a bench unit

Both tools reach the scope's shell over SSH only, with a trusted key and strict host key checking; there is no telnet
(ADR 0054). `device-test.py` needs it for its read-only commands (the fingerprint of the protected areas, the build,
the log), `device-deploy.sh` for everything. Once per unit, at the scope ([ssh.md](ssh.md), network.md 7):

1. System > Network > Remote services > **Allowed**: the prefix of the bench LAN (`192.0.2.0/24`). SSH listens only
   with an address list.
2. **SSH** on. SSH settings > State says `on: password and keys`; Show password gives the password and the host key
   fingerprint.
3. On the computer: `ssh root@IP` once, compare the fingerprint it shows with the one on the scope and answer `yes`
   (the tools refuse an unknown host key), then `ssh-copy-id root@IP` with the password. A key can also come with the
   image (`OOS_SSH_AUTHORIZED_KEYS`), from a USB stick, or be written to the card on a computer (ssh.md).
4. `ssh -o BatchMode=yes root@IP true` answers without a question: the tools work. `OOS_SSH_IDENTITY` names another
   key file, `OOS_SSH_PORT` another port.

The keys, the host key and the password stay on the data partition (`/oos/ssh`) over updates and power cycles.

## What a run from RAM does not cover

`tools/device-deploy.sh` restarts `oos-ui` with the new programs; it does not start the scope. What runs once at boot
is not reached: `oos-fpgad cold-boot` and `fpga-boot` from the installed image, the scripts of the initramfs, the
kernel, an update. A change there needs the image installed and the scope started from power off, with a person at
the unit, before the label `unit-tested` is set (ADR 0052). `tools/pr-gate.sh` names the classes: `$proven`
(the comparison proves a refactoring), `$live` (a run from RAM or more).

## Only what the change can affect

`--changed BASE` takes the files the tree changes against `BASE`:
- the commits since the common ancestor;
- what is not committed yet;
- new files.

Each file is matched against the rules in `tools/device_select.py`. The first rule that matches gives its areas, and
each area names its tests. Some tests always run: identity, the fingerprint of the protected areas before and after,
the build on the unit, the kernel log, and where the generator reaches.

| A change of | runs |
|---|---|
| documents, CI, host tools, tests of the crates | the smoke tests only |
| `hal/src/dho/roll*` | roll, acquisition, register sequences |
| `hal/src/dho/la*` | logic analyser, register sequences |
| trigger, mask, zone, AUX OUT in `hal/src/dho/` | trigger, register sequences |
| AFE, ADC, calibration, PLL in `hal/src/dho/` | front end, acquisition, register sequences |
| `ui/src/board/` | acquisition, roll, trigger, front end, register sequences |
| `fpgad/` | register sequences |
| SCPI, web, VXI-11, MCP | remote |
| storing, the logger | store |
| `dsp/`, measurements, decoders | measure, decode |
| the measurement code (`dsp/src/measure.rs`, the measurement paths of `ui/`) | measure, acquisition, roll, front end, generator, trigger |
| other parts of `ui/`, `gpu/` | screen, remote |
| `boot/`, `kernel/`, `rootfs/`, `hal/src/update/`, the cold boot, `Cargo.toml`/`Cargo.lock`, the test tools themselves, a file no rule knows | everything |

`--list` prints the files with their areas and the tests that would run, and stops. The changes of the last pull
requests chose between 6 tests (documents, CI) and 13 to 23 (one part of the hardware); all 85 run only when the
image, the update path, the build or the tests themselves change.

## Noise and failures that were there before

- **`--retries N`** (default 1): a test that fails runs again, up to N times. One that passes then is `FLAKY`; its
  first failure stays in the report. A test that fails every time is `FAIL` with each attempt's detail: a retry
  absorbs noise, not a fault that is always there.
- **`--known FILE`** (default `device-test/known.json`): the results of a run of main on the same unit. A failure that
  the known run has too is `KNOWN`, not a failure of the change; a difference of the register sequences and a failed
  smoke test never are. `--save-known` writes the run into that file: do it with the installed image or a build of
  main, after main changed in the hardware paths.

`FLAKY` and `KNOWN` are in the report and do not fail the run (exit code 0). A test that is `FLAKY` again and again
is a fault of its own: of the test, or of OOS. It deserves an issue, not more retries.

## The register sequences

`plan_matches_this_build` (with `--plan`) runs `plan-matrix.sh` on the unit with its calibration and compares the
output with this computer's, section by section. Three differences come from the setup and are not counted:

- where the calibration files lie;
- whether a logic probe is plugged in;
- a section that uses a calibration file one side lacks (this computer often has no `cal_afg.hex`): left out and
  named as `INFO`.

Any other difference fails the test and names the section and the first line that differs.

## When a test fails only with one build

Compare with main on the same unit, the test alone (`--only NAME`), more than once each. Deploy the build of main,
run the test, deploy the change, run it again. A test that passes with main every time and fails with the change
every time points at the change, even when the host checks see no difference: the timing on the unit is part of what
is tested.
