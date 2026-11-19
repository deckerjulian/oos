# Instructions for coding agents (and for people)

OOS is a free operating system for the Rigol DHO800/DHO900 oscilloscopes: a Linux kernel, an initramfs and Rust
programs that drive the scope's FPGA directly. A wrong register write is only seen on a unit, and a unit is not always
there. These instructions are the same for every agent and every person; the reasons are in the decision records
([docs/adr](docs/adr/README.md), ADR 0018). Where this file and an ADR differ, the ADR holds.

## Read first

1. [docs/adr/README.md](docs/adr/README.md): the decisions that shape the tree, one line each. Open the records that
   touch your change.
2. [docs/architecture.md](docs/architecture.md): how the pieces fit, from the card to the screen.
3. [CONTRIBUTING.md](CONTRIBUTING.md): the ground rules, and the recipe for each kind of extension (the table
   "Adding something" below is its short form).
4. The `docs/` page of the part you change (`docs/protocols/` for register sequences).

## Never

- **Write to the FPGA flash, `/rigol/data`, or the `uboot`, `trust` and `parameter` partitions**, nor add code that
  can (ADR 0009).
- **Write to a scope unless its owner asked for it in this session**: no deploy, update, writing `oos-fpgad` command,
  or changing command over adb, telnet or SSH. Reading is fine (ADR 0018).
- **Bring vendor code or data into the tree**: no firmware images, no code or binaries from stock or modified
  firmware, no calibration files, no `vendor.bin`. What is known about the hardware is described in `docs/` as
  behaviour (ADR 0013).
- **Add a dependency** beyond `libc` to one of the four crates without a strong reason stated in the pull request
  (ADR 0013).
- **Add code for a host that is not Unix.** OOS is built on Linux, macOS and Windows inside WSL 2; there is no native
  Windows build, and no `cfg(windows)` (ADR 0033).
- **Turn on a hardware function that is not verified on a unit by default** (ADR 0010).
- **Put personal data** (serial numbers, MAC addresses, licence keys, a scope's address) into code, docs or commits.
  `tools/privacy-scan.py` reads the commits of a branch for them, and CI fails a pull request on a finding; run it
  before you push, because a pushed commit stays readable.
- **Put vendor material into a text around the code**: commit messages, pull request descriptions and issues follow
  the same rule as the tree, and every commit is signed off by the person who submits it (ADR 0026).
- **Push to main.** Work on a branch and open a pull request; it is merged squashed when CI is green, after the waiting
  time of ADR 0031. A new decision record comes in as `proposed`.

## Always

- State how a hardware fact is known: **verified on the unit**, **assumed**, or **not yet verified on hardware**
  (ADR 0015). Reports of your own work say the same: what ran on a unit, what only on the host, what not at all.
- Write hardware code from `docs/`: a fact the code needs and `docs/` lacks goes into `docs/` first, as behaviour at
  the scope's interfaces, in the same pull request (ADR 0030).
- A change that is not meant to change behaviour proves it: `tools/compare-base.sh` reports no difference (same
  pictures, stored files, answers to remote commands and register sequences as the commit before the change), and
  the pull request says
  `Behaviour: unchanged`. CI runs the same comparison and fails such a pull request when something differs (ADR 0014,
  ADR 0032).
- A remote command goes into the one SCPI interpreter (`ui/src/scpi.rs`, a subsystem in its file of `ui/src/scpi/`),
  never into a transport; it takes the stock name where the stock firmware has one, [docs/scpi.md](docs/scpi.md) lists
  it, and its query is in `ui/tests/scpi.txt`, so a change of its answer shows in the comparison (ADR 0022, ADR 0023).
  A new network service gets its switch under Remote services and the `net.allow` check when it can change the scope
  (ADR 0025).
- New decoders, measurements and analyser pages come as modules behind their registry (ADR 0002). Identifiers that
  are stored (protocol keys, measurement numbers, feature ids) never change once released.
- Every kind of extension has one place to declare it, a recipe and a test that names what is missing (ADR 0046;
  "Adding something" below). An item that cannot have a part is listed in that test with its reason; such a list is
  never made longer to get a test to pass. A new kind of extension comes with all three.
- Hardware code builds step lists of `(offset, value)` in `oos-hal`; signal processing in `oos-dsp` stays free of
  hardware (ADR 0001).
- Reuse before you add: look for code that already does the job (`git grep` for the idea, not only for a name) and
  call it. A second implementation of something says in the pull request why it has to be one. Remove what your
  change leaves unused, in the same pull request. A change that is not a feature leaves the tree no larger.
- Docs change in the same pull request as the code they describe. A change that decides something others build on
  comes with a new ADR ([template](docs/adr/template.md)); a change of direction supersedes a record, it does not
  edit it.
- Keep the look: neutral colours, white accent, round shapes, no empty areas; colours come from the named constants,
  not literals in drawing code; the top bar keeps its four entries (ADR 0007, ADR 0008), and its status boxes come
  from one list (ADR 0063). A second UI or a new layout of the screen needs a decision record first.

## The development agent

The maintainer starts a coding agent in a session with a task ("consolidate the decoders", "fix #62") and lets it
work on its own (ADR 0039). Everything above holds for it, and:

- It acts on GitHub as the App `dekajoule-oos-agent`: `tools/as-agent.sh git push ...`, `tools/as-agent.sh gh pr
  create ...`. Its commits carry the maintainer's sign-off. Its pull requests need no waiting time and merge by
  themselves once `test`, `compare`, `owner`, `unit` and `review` pass; it switches auto-merge on for each one.
- One topic per pull request, about 400 changed lines; `tools/check.sh`, `tools/compare-base.sh` and
  `tools/privacy-scan.py` before every push, and the maintainer's private check (a local hook). A change that is not
  meant to change behaviour says `Behaviour: unchanged`; one that is says what changes.
- At most five of its pull requests are open at once, and no two change the same file. A file a tool writes from
  others does not count (the index of `docs/adr/README.md`, `tools/*.txt`, `ui/Cargo.lock`): after a rebase it runs
  the tool again (ADR 0065).
- It asks for Copilot's review of each pull request under the maintainer's account (`gh pr edit N --add-reviewer
  @copilot`, not through the App: GitHub asks Copilot by itself only for a person's pull request), reads it, fixes
  what is right, answers what is not, and resolves the threads; `review` passes only then. It never makes a check
  pass by weakening it.
- A change in the hardware paths (`hal/src/dho/`, `hal/src/update/`, `ui/src/board/`, `fpgad/`, `boot/`, `kernel/`,
  `rootfs/`) is tried on the scope on the bench when the maintainer has said in the session that it may use it:
  deploy to RAM, `tools/device-test.py`, restore what was installed. Then it sets `unit-tested` and says in the pull
  request what ran on the unit. The label holds for the commits it was set for: a commit pushed after it (not a merge
  of main) takes it away again (ADR 0069). Without that the pull request waits, unless CI's job `scope` finds the programs
  for the scope byte-identical to the base's, panic places and symbol names left out (`tools/scope-binaries.sh`,
  ADR 0041, ADR 0068), or it is a refactoring of code that builds register sequences, which the comparison proves
  (title `Lints:` or `Refactor:`, `Behaviour: unchanged`; not the start, the update, `ui/src/board/` or the live paths
  of `hal/src/dho/`, ADR 0042): then `unit` passes by itself. Register sequences, timing, start and update are what the run on a unit is for.
- It never removes `hold`, and never merges or asks to merge a pull request of somebody else or of another bot.
- When main is red it fixes that first and does nothing else.
- At the end it reports to the maintainer: what it opened and merged, what ran on the unit, what it left open.

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

## Adding something

One place to declare each kind, the files that follow by hand, and the check that fails, naming the item and the
missing part, when one is forgotten (ADR 0046). The steps, with the commands for what is generated, are in
[CONTRIBUTING.md](CONTRIBUTING.md#adding-something); where every other change goes is in
[docs/architecture.md](docs/architecture.md#where-a-change-goes).

| Kind | Declare it in | Also touch | The check that fails when a part is missing |
|---|---|---|---|
| setting | a row in `ui/src/settings/` | model field, `Item` and menu, `ui/tests/scpi.txt`, probes of the snapshot | `ui/src/settings/tests.rs::every_row_is_complete` |
| measurement | a `Def` in `ui/src/measures/` | `dsp/src/` with its test, a new group in `docs/reference-analysis.md` | `ui/src/measures/mod.rs::every_measurement_is_complete` |
| protocol decoder | a `ModuleInfo` in its family file of `dsp/src/decode/` (C-PHY: `dsp/src/cphy.rs`, line codes: `dsp/src/linecode.rs`), a `Protocol` in `ui/src/protocols/` | `dsp/src/protocols.rs`, `dsp/tests/decode/`, `ui/Cargo.toml`, `wiki/Features.md`, `README.md` | `ui/src/protocols/mod.rs::every_decoder_is_complete`, `dsp/src/protocols.rs::every_declared_module_is_registered` |
| trigger type | `TrigType` in `ui/src/model/trigger.rs` | `dsp/src/trigger.rs`, `hal/src/dho/tpu.rs`, `ui/src/scpi/dispatch.rs`, `docs/scpi.md`, the wiki, `ui/tests/triggers.txt` | `ui/src/model/trigger.rs::every_trigger_type_is_complete` |
| menu page | its module in `ui/src/ui/` and its line in `PAGES` (`ui/src/ui/pages.rs`, ADR 0060) | its `MenuId` (`ui/src/ui.rs`), its `Item`s (`ui/src/ui/item.rs`), its longest states in `LAYOUTS`, the wiki | `ui/src/ui/pages.rs::every_page_is_complete`, `ui/src/ui/lint/menu_layout.rs::every_menu_fits` |
| service (a thread of a part) | a `Service<J, R>` in its part's state (`ui/src/ui/service.rs`, ADR 0061), its poll in `POLLS` (`ui/src/ui/acquire.rs`) | `docs/architecture.md` (its thread) | `ui/src/ui/acquire.rs::every_poll_is_in_polls` |
| analyser page | an `Analyser` and its line in `ui/src/ui/analysers.rs` | `ui/src/model/analysers.rs`, `ui/src/ui.rs`, `ui/src/ui/lint/menu_layout.rs`, `docs/scpi.md`, the wiki | `ui/src/ui/analysers.rs::every_page_is_complete` |
| SCPI command | its file of `ui/src/scpi/` | `docs/scpi.md`, `ui/tests/scpi.txt`, `wiki/SCPI-and-VXI-11.md` | `tools/scpi-coverage.py` (layer `ratchet`) |
| HTTP route or MCP tool | an arm of `Http::answer` in `ui/remote/src/http.rs`, or of `Site::answer` for `site::Ctx` in `ui/src/site.rs` when the screen answers it (an MCP tool: `site::mcp`) | its entry and mark in `docs/http.md` (ADR 0055), `wiki/Remote-Control.md` | `tools/http-coverage.py` (layer `ratchet`) |
| register sequence | its block's module of `hal/src/dho/` | first its page in `docs/protocols/`, a test, its line in the board's matrix `tools/board/dho/plan-matrix.sh` | `hal/tests/register_modules.rs::every_module_names_its_page_and_has_a_test` |
| board family (ADR 0067) | a `Target` in `hal/src/<name>/target.rs`, listed in `FAMILIES` (`hal/src/target.rs`) | `ui/src/board/<name>/`, `rootfs/board/<name>/` (`etc/oos-board`, `bin/oos-board`), its row in `docs/porting.md`, its device tree | `hal/tests/board_families.rs::every_board_family_is_complete` |

`tools/link-check.py` (layer `text`) keeps this table and the recipes true: every file and every test they name
exists.

## Keeping the texts true

Every text has one home (ADR 0035): `README.md` is the entry page, `wiki/` the guide for people who use a scope
(published as the wiki from the tree; never edited on the wiki itself), `docs/` the technical reference, `docs/adr/` the
decisions, `CHANGELOG.md` what a user notices (a change not yet released is a file in `changelog.d/`, ADR 0065). A fact
is written in its home and linked from elsewhere. A pull request that changes one of these brings the text along:

| You change | Bring along |
|---|---|
| a remote command (`ui/src/scpi.rs`, `ui/src/scpi/`) | `docs/scpi.md`, `wiki/SCPI-and-VXI-11.md` |
| an HTTP route or MCP tool (`ui/remote/src/http.rs`, `ui/src/site.rs`, `ui/src/site/`) | `docs/http.md`, `wiki/Remote-Control.md` |
| a menu, a page or a setting a user sees | the page of that part in `wiki/`, a file in `changelog.d/` |
| an option of `oos-ui` or `oos-fpgad` | the list at the top of its `main.rs`, and the page that names the option |
| a register sequence, a hardware fact | the page in `docs/protocols/`, with how the fact is known |
| installing, updating, starting | `docs/boot.md`, `docs/update.md`, `wiki/Installation.md`, `wiki/Updates-and-Going-Back.md` |
| a tool in `tools/` | `tools/README.md` |
| a crate, a program, a thread, what talks to what | `docs/architecture.md` and its diagrams |
| something others build on | a new record in `docs/adr/`, and `tools/adr-index.py` for its row in the index |

Diagrams are Mermaid in a fenced block, one C4 level each (context, programs, parts), as `flowchart` with `subgraph`,
with the source files named under them.

## Build and check

```sh
tools/check.sh                # every enforced host check, as CI runs them (ADR 0020)
tools/check.sh test           # one layer: fmt text syntax test target decoders clippy ratchet (tools/check.sh list)
(cd ui && cargo run -- --serve)   # no display, no hardware: demo signals behind SCPI :5555 and the web page :8080
```

Run `tools/check.sh` before you say a change is done, and say which layers ran. A compiler warning fails it. The layout
of Rust code is rustfmt's with the tree's `rustfmt.toml` (ADR 0016): run `cargo fmt` in the crate you changed, the
layer `fmt` fails otherwise. Clippy findings in code you touch are fixed or allowed at the place with a reason (ADR
0017). The numbers in `tools/ratchet.txt` only go down: a file over 2000 lines does not grow, no new file gets that
large, and a crate gets no more `allow(` and no more `unsafe` than it has; after making one smaller, run
`tools/ratchet.sh --lower`. Raising a number is the maintainer's decision, in a pull request that says why (ADR 0037).
Tests that need the calibration files of a real unit skip themselves without `local/cal`. A test must not depend on how
fast the computer is: one that fails only under load is fixed, not run again (ADR 0020).

## Style

English in code, comments, docs and commits. Lines of 120 columns. Files LF, UTF-8, a final newline, no trailing
spaces, an `SPDX-License-Identifier: GPL-2.0-or-later` line in every source file of our own. Commit subjects
`Area: what changed, as a statement` (`SCPI: a query is answered after the client closed its side`), one topic per
commit, the body says why and how it was tested. User-visible changes get a file in `changelog.d/` (ADR 0065).
Details: ADR 0019.
