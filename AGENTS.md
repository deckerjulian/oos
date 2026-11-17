# Instructions for coding agents (and for people)

OOS is a free operating system for the Rigol DHO800/DHO900 oscilloscopes: a Linux kernel, an initramfs and Rust
programs that drive the scope's FPGA directly. A wrong register write is only seen on a unit, and a unit is not always
there. These instructions are the same for every agent and every person; the reasons are in the decision records
([docs/adr](docs/adr/README.md), ADR 0018). Where this file and an ADR differ, the ADR holds.

## Read first

1. [docs/adr/README.md](docs/adr/README.md): the decisions that shape the tree, one line each. Open the records that
   touch your change.
2. [docs/architecture.md](docs/architecture.md): how the pieces fit, from the card to the screen.
3. [CONTRIBUTING.md](CONTRIBUTING.md): the ground rules, and how to add a decoder, a measurement or an analyser page.
4. The `docs/` page of the part you change (`docs/protocols/` for register sequences).

## Never

- **Write to the FPGA flash, `/rigol/data`, or the `uboot`, `trust` and `parameter` partitions**, nor add code that
  can (ADR 0009).
- **Write to a scope unless its owner asked for it in this session**: no deploy, update, writing `oos-fpgad` command,
  or changing command over adb, telnet or SSH. Reading is fine (ADR 0018).
- **Bring vendor code or data into the tree**: no firmware images, no code or binaries from stock or modified
  firmware, no calibration files, no `vendor.bin`. What is known about the hardware is described in `docs/` as
  behaviour (ADR 0013).
- **Add a dependency** beyond `libc` to what runs on the scope, or to `oos-hal` and `oos-dsp`, without a strong reason
  stated in the pull request (ADR 0013). A desktop-only build may use its platform's crates, behind its `cfg` and listed
  in `docs/fork/deps.md` (ADR 0021).
- **Turn on a hardware function that is not verified on a unit by default** (ADR 0010).
- **Put personal data** (serial numbers, MAC addresses, licence keys, a scope's address) into code, docs or commits.
- **Put vendor material into a text around the code**: commit messages, pull request descriptions and issues follow
  the same rule as the tree, and every commit is signed off by the person who submits it (ADR 0026).
- **Push to main.** Work on a branch and open a pull request; it is merged squashed when CI is green, after the waiting
  time of ADR 0031. A new decision record comes in as `proposed`.

## Always

- State how a hardware fact is known: **verified on the unit**, **assumed**, or **not yet verified on hardware**
  (ADR 0015). Reports of your own work say the same: what ran on a unit, what only on the host, what not at all.
- Write hardware code from `docs/`: a fact the code needs and `docs/` lacks goes into `docs/` first, as behaviour at
  the scope's interfaces, in the same pull request (ADR 0030).
- A change that is not meant to change behaviour proves it: `ui/tools/layout-check.sh record` before and `check`
  after (same pixels), `tools/plan-matrix.sh` before and after (same register writes) (ADR 0014).
- A remote command goes into the one SCPI interpreter (`ui/src/scpi.rs`), never into a transport; it takes the stock
  name where the stock firmware has one, and [docs/scpi.md](docs/scpi.md) lists it (ADR 0022, ADR 0023). A new network
  service gets its switch under Remote services and the `net.allow` check when it can change the scope (ADR 0025).
- New decoders, measurements and analyser pages come as modules behind their registry (ADR 0002). Identifiers that
  are stored (protocol keys, measurement numbers, feature ids) never change once released.
- Hardware code builds step lists of `(offset, value)` in `oos-hal`; signal processing in `oos-dsp` stays free of
  hardware (ADR 0001).
- Reuse before you add: look for code that already does the job (`git grep` for the idea, not only for a name) and
  call it. A second implementation of something says in the pull request why it has to be one. Remove what your
  change leaves unused, in the same pull request. A change that is not a feature leaves the tree no larger.
- Docs change in the same pull request as the code they describe. A change that decides something others build on
  comes with a new ADR ([template](docs/adr/template.md)); a change of direction supersedes a record, it does not
  edit it.
- Keep the look: neutral colours, white accent, round shapes, no empty areas; colours come from the named constants,
  not literals in drawing code; the top bar keeps its four entries (ADR 0007, ADR 0008). A second UI or a new layout
  of the screen needs a decision record first.

## Standing agents

Two agents work without being asked, each from a profile in `.github/agents`, and neither chooses its own work: the
task is in the issue that starts it. `cleaner` does one piece of mechanical clean-up that changes no behaviour (the
findings of one lint, dead code, one part out of a file that is too large). `docs-follower` brings the texts in line
with one merged change. Everything above holds for them, and more: one task and about 400 changed lines at most, no
fix and no feature on the side, no change to `.github/`, this file, `docs/adr/`, the check scripts, `boot/`,
`kernel/`, `rootfs/` or `hal/src/dho/`, and never a check made to pass by weakening it. They open pull requests and
never merge (ADR 0034).

## Layout of the tree

| Path | What |
|---|---|
| `hal/` | `oos-hal`: register sequences, AFE/ADC/PLL buses, cold boot, panel MCU (pure, with a `Bus` trait) |
| `dsp/` | `oos-dsp`: signal processing on `&[f32]`, decoders, measurements |
| `ui/` | `oos-ui`: the screen, SCPI, web server; sources Demo, Replay, Live, Hw |
| `fpgad/` | `oos-fpgad`: command line on top of `oos-hal` (`plan` only prints) |
| `gpu/` | GPU helper process (C, Mesa), ADR 0003 |
| `boot/`, `kernel/`, `rootfs/` | image, kernel 6.12, initramfs scripts |
| `tools/` | host tools; [tools/README.md](tools/README.md) says which ones touch a scope |
| `docs/` | public documentation; `docs/adr/` the decisions |

What `.gitignore` leaves out (`local/` with calibration files and surveys, `log/`, `device-test/`, backups, notes) is
private to the person who made it; never move its content into the tree.

## Build and check

```sh
tools/check.sh                # every enforced host check, as CI runs them (ADR 0020)
tools/check.sh test           # one layer: fmt clippy text syntax test target decoders (list: tools/check.sh list)
(cd ui && cargo run -- --serve)   # no display, no hardware: demo signals behind SCPI :5555 and the web page :8080
```

Run `tools/check.sh` before you say a change is done, and say which layers ran. A compiler warning fails it. The
layout of Rust code is rustfmt's with the tree's `rustfmt.toml` (ADR 0016). Until the one formatting commit is in main,
do not run `cargo fmt` over existing files, so a pull request does not reformat whole files; keep the layout of the
file you edit. Clippy findings in code you touch are fixed or allowed at the place with a reason (ADR 0017). Tests
that need the calibration files of a real unit skip themselves without `local/cal`. A test must not depend on how
fast the computer is: one that fails only under load is fixed, not run again (ADR 0020).

## Style

English in code, comments, docs and commits. Lines of 120 columns. Files LF, UTF-8, a final newline, no trailing
spaces, an `SPDX-License-Identifier: GPL-2.0-or-later` line in every source file of our own. Commit subjects
`Area: what changed, as a statement` (`SCPI: a query is answered after the client closed its side`), one topic per
commit, the body says why and how it was tested. User-visible changes get a `CHANGELOG.md` entry. Details: ADR 0019.
