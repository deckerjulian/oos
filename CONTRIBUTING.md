# Contributing to OOS

Reports, measurements and code are all welcome. The most useful thing right now is a test on hardware: OOS has run on
a DHO924S and a DHO804 so far. A report from a DHO802/DHO812, a DHO914/DHO924 without generator or an EDU model makes
that whole hardware variant verified ([docs/models.md](docs/models.md) 2.1).

## Ground rules

- **No vendor code or vendor data.** No firmware images, no code or binaries from any stock or modified firmware, no
  calibration files, no `vendor.bin`. What runs on the scope only depends on `libc`, and it stays that way unless
  there is a strong reason; a desktop-only build may use its platform's crates
  ([ADR 0021](docs/adr/process/0021-libc-only-on-the-scope-platform-crates-on-the-desktop.md)).
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
- **Keep the analog path unchanged unless that is the point.** `tools/plan-matrix.sh` prints the register sequences of a
  fixed set of acquisitions. Run it before and after a change and compare the output. Say in your pull request whether they differ.
- **No personal data in issues or commits:** no serial numbers, MAC addresses or licence keys.

## A new protocol decoder

A decoder comes as a protocol module: one file in `ui/src/protocols`, one line to register it, the decoder itself
usually in `oos-dsp`. [docs/protocol-modules.md](docs/protocol-modules.md) walks through it.

## New measurements

Measurements come as a measurement module: a file in `ui/src/measures` with their tiles and their computation, one
line to register it, the signal processing usually in `oos-dsp`. [docs/measurement-modules.md](docs/measurement-modules.md)
walks through it. An analyser page registers the same way ([docs/analyser-modules.md](docs/analyser-modules.md)).

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

A pull request from a fork enters main squashed, one commit per topic, made from the state that was reviewed; your
name and sign-off stay on it. Keep a pull request to one topic where you can, and link the branch of your fork when
its history matters (ADR 0026).

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

## Licence

By contributing you agree that your work is published under the licence of the project,
[GPL-2.0-or-later](LICENSE).
