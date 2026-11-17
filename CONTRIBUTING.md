# Contributing to OOS

Reports, measurements and code are all welcome. The most useful thing right now is a test on hardware: OOS has run on
a DHO924S and a DHO804 so far. A report from a DHO802/DHO812, a DHO914/DHO924 without generator or an EDU model makes
that whole hardware variant verified ([docs/models.md](docs/models.md) 2.1).

## Ground rules

- **Independent reimplementation.** The tree holds only black-box descriptions of the scope and code written to them.
  A description says what goes in and comes out at the scope's interfaces (registers, buses, signals, the stock SCPI,
  the vendor's published documents), as behaviour, in your own words. A hardware fact your code needs and `docs/`
  lacks goes into `docs/` first, in the same pull request. Names a running unit shows (loaded modules, device nodes,
  programs and scripts of its start-up) may be used; names from inside the firmware's programs may not
  ([ADR 0030](docs/adr/process/0030-independent-reimplementation-from-black-box-descriptions.md)).
- **No vendor code or vendor data.** No firmware images, no code or binaries from any stock or modified firmware, no
  calibration files, no `vendor.bin`. The four crates only depend on `libc`, and it stays that way unless there is a
  strong reason ([ADR 0013](docs/adr/process/0013-no-vendor-code-only-libc.md)).
- **No vendor material in the texts either.** Code, comments, documents, commit messages, pull request descriptions
  and issues describe the hardware as behaviour, in your own words: no listings, copied tables or strings from the
  stock or a modified firmware, and no names of its functions, classes or files. What the vendor publishes for users
  is fine: the command names of the programming guide, the manuals, the data sheets of the parts
  ([ADR 0026](docs/adr/process/0026-contributions-are-signed-off-and-hold-no-vendor-material.md)).
- **Sign your commits off.** `git commit -s` adds a `Signed-off-by:` line with your name. With it you state the
  [Developer Certificate of Origin](https://developercertificate.org): the work is your own or you have the right to
  submit it, under the licence of the project. This holds for work your agent wrote too. Code or text from another
  project is named in the pull request with its source and licence.
- **Say how you know.** A statement about the hardware in `docs/` is "verified on the unit", or "assumed", or "not yet
  verified on hardware". Keep that honest: a wrong "verified" costs someone else days.
- **Be careful with other people's scopes.** OOS never writes the FPGA flash, `/rigol/data`, or the `uboot`, `trust` and
  `parameter` partitions. Changes that would need any of this will not be accepted without a very good case.
- **Keep behaviour unchanged unless that is the point.** `tools/compare-base.sh` builds the commit before your change
  next to your tree and compares the pictures of the screen and the register sequences of a fixed set of acquisitions.
  Say in your pull request `Behaviour: unchanged`, or what changes and why; CI runs the same comparison
  ([ADR 0032](docs/adr/process/0032-ci-compares-a-pull-request-with-its-base.md)).
- **No personal data in issues or commits:** no serial numbers, MAC addresses or licence keys.

## A new protocol decoder

A decoder comes as a protocol module: one file in `ui/src/protocols`, one line to register it, the decoder itself
usually in `oos-dsp`. [docs/protocol-modules.md](docs/protocol-modules.md) walks through it.

## New measurements

Measurements come as a measurement module: a file in `ui/src/measures` with their tiles and their computation, one
line to register it, the signal processing usually in `oos-dsp`. [docs/measurement-modules.md](docs/measurement-modules.md)
walks through it. An analyser page registers the same way ([docs/analyser-modules.md](docs/analyser-modules.md)).

## Where to develop

On Linux, on macOS, or on Windows inside WSL 2
([ADR 0033](docs/adr/process/0033-development-on-linux-macos-and-wsl.md)). CI runs Ubuntu 24.04. There is no native
Windows build. On Windows:

- Install WSL 2 with a current Ubuntu, and Rust inside it (`rustup`, with the target `aarch64-unknown-linux-musl`).
- Clone into the Linux file system (`~/oos`), not under `/mnt/c`: builds there are much slower.
- `tools/check.sh` and `cargo run -- --serve` are used as on Linux; the browser on Windows reaches the web remote at
  `http://localhost:8080/`.
- The sims (`tools/check.sh sim`) need Docker inside WSL.
- A card reader is not a block device inside WSL: back up and write cards with the Windows tools of
  [docs/sdcard-backup.md](docs/sdcard-backup.md).

WSL has not been tried by the maintainer yet. If a step fails there, open an issue with what you ran and what it
printed.

## Before a pull request

```sh
tools/check.sh
```

It runs every host check CI runs, layer by layer: files, script syntax, the tests of all four crates, the scope's
build, the decoder builds ([ADR 0020](docs/adr/process/0020-checks-in-layers-one-command-on-the-host.md)). A compiler
warning fails it like an error. `tools/check.sh test` runs one layer, `tools/check.sh list` names them. Tests that
need the calibration files of a real unit skip themselves without them. See the Development section of the
[README](README.md). [docs/architecture.md](docs/architecture.md) is the place to start reading.

Say in the pull request which checks ran and where: on a unit (which model), only on the host, or not at all.

CI also compares every pull request with its base: the pictures of the screen and the register sequences
(`tools/compare-base.sh`, about three minutes on a laptop). The result is in the summary of the `compare` job, with the
pictures that differ to download. A difference is not an error; a pull request that says `Behaviour: unchanged` while
something differs is.

A pull request from a fork enters main squashed, one commit per topic, made from the state that was reviewed; your
name and sign-off stay on it. Keep a pull request to one topic where you can, and link the branch of your fork when
its history matters (ADR 0026).

## How a change gets into main

The same way for everyone, the maintainer included
([ADR 0031](docs/adr/process/0031-every-change-enters-main-through-a-pull-request.md)):

- Nobody pushes to main. Every change is a pull request, merged squashed once CI is green. The rules that hold this
  are files: `.github/rulesets/` and `.github/CODEOWNERS`
  ([ADR 0036](docs/adr/process/0036-the-rules-of-the-repository-are-files-a-change-by-others-needs-the-maintainers-review.md)).
  A pull request that is not the maintainer's needs the maintainer's approving review.
- A pull request that changes behaviour, a rule or many files stays open at least 24 hours, so others see it coming.
  Fixes for a red main, security fixes and documentation corrections can go in as soon as CI is green.
- A decision record comes in as `proposed` and stays open at least 72 hours for discussion before it is merged as
  `accepted` or `rejected`.
- A pull request can be merged when five checks have passed
  ([ADR 0038](docs/adr/process/0038-auto-merge-for-the-maintainer-and-the-bots.md)): `test` and `compare` from CI,
  and three statuses: `waited` (the waiting time is over), `owner` (it is the maintainer's, or the maintainer approved
  its newest commit) and `unit` (it changes nothing that is only proven on a scope, or it was tried on one). A pull
  request from a fork needs the maintainer's approval and is merged by the maintainer.
- The pull request opened first is merged first when two touch the same code; the later one is rebased. Allow edits by
  maintainers on your pull request and the rebase is done for you.
- Large changes that do not change behaviour (formatting, moves) are announced in an issue with their day, and wait
  for the open pull requests.
- An automatic review (GitHub Copilot code review) may comment on a pull request. Its comments are advice: take the
  ones that are right, and say so where one is wrong. It does not approve and it does not merge. What it is asked to
  look for is in [.github/instructions/review.instructions.md](.github/instructions/review.instructions.md).

## Coordinating work

- **Before you start something larger, open an issue** (or comment on an existing one) and say what you take. One issue
  per piece of work avoids two people building the same thing.
- **Issues are the to-do list.** Reports from the forum or elsewhere become issues, so they are found and closed in one
  place.
- **Labels say what a thing is**, on issues and pull requests alike: its kind (`type/fix`, `type/feat`, `type/docs`,
  `type/test`, `type/ci`, `type/build`, `type/refactor`, `type/process`), the part of OOS (`area/hal`, `area/ui`,
  `area/dsp`, `area/fpgad`, `area/boot`, `area/gpu`, `area/tooling`, `area/ci`, `area/docs`), `agent` for what a bot
  opened, and `risk/hardware` for what is only proven on a scope. A pull request gets its areas, `agent` and
  `risk/hardware` by itself (`tools/pr-labels.sh`), and its type when its title starts with `CI:`, `Docs:`, `Tests:`,
  `Process:` or `Build:`.
- **Some labels say what an issue needs:** `hardware` (a report from a scope), `agent-ready` (a task with a goal, a
  "done when" and limits, small enough for one pull request, for a person or a coding agent), `needs-unit` (it has to
  be tried on a scope before it is merged or closed), and `flaky` (a test that fails now and then; it is fixed, not run
  again). The issue forms ask for what each kind needs.
- **Draft pull requests are welcome early.** A draft shows where you are going before it is done and lets others plan
  around it.
- **Say which area you work on** in the pull request title (`Trigger:`, `SCPI:`, `Docs:`), as in the commit subjects;
  it makes overlaps visible in the list of open pull requests.

## Style

- Text and code comments are in English. Changes to the hardware knowledge go into `docs/`, in the same pull request.
- Files: UTF-8, LF, a final newline, no trailing spaces, the `SPDX-License-Identifier: GPL-2.0-or-later` line in every
  source file. [.editorconfig](.editorconfig) sets this up in most editors.
- Rust: `cargo fmt` with the tree's [rustfmt.toml](rustfmt.toml), and Clippy clean except for the allowed lints in
  `Cargo.toml` ([ADR 0016](docs/adr/process/0016-rust-code-formatted-by-rustfmt.md),
  [ADR 0017](docs/adr/process/0017-lints-clippy-warnings-are-errors.md)). Both are proposed: until the one formatting
  commit is in main, keep the layout of the file you edit and do not reformat whole files.
- Commits: `Area: what changed, as a statement`, one topic each, the body says why and how it was tested; formatting
  and lint fixes in commits of their own. A change people would notice on the scope gets a `CHANGELOG.md` entry
  ([ADR 0019](docs/adr/process/0019-one-style-for-text-files-and-commits.md)).

A change that decides something others will build on (a structure, a rule of the screen, a way to treat the hardware)
comes with a decision record: [docs/adr](docs/adr/README.md).

## Working with a coding agent

Most code here is written with one. [AGENTS.md](AGENTS.md) holds the instructions for all of them (`CLAUDE.md` only
points to it); point your tool to it if it does not read it by itself. The agent's report counts like yours: it says
what was tested on a unit, what only on the host, and what was not tested. An agent does not write to a scope unless
its owner asked for it ([ADR 0018](docs/adr/process/0018-one-agents-md-for-people-and-agents.md)).

Two agents also work without being asked
([ADR 0034](docs/adr/process/0034-standing-agents-get-their-task-from-a-machine-a-person-merges.md)): one does a piece
of mechanical clean-up a week, one brings the texts in line after a merge that left them behind. Each gets its task
in an issue; neither chooses its own work, fixes errors or adds features. Their pull requests come from `copilot/`
branches, are small, and are reviewed and merged by the maintainer like any other. If one of them is wrong, say so
in the pull request: that is how their instructions in `.github/agents` get better. Once a week a script lists what
is new in the forum and in the fork as an issue with the label `scout`.

## Licence

By contributing you agree that your work is published under the licence of the project,
[GPL-2.0-or-later](LICENSE).
