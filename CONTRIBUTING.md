# Contributing to OOS

Reports, measurements and code are all welcome. The most useful thing right now is a test on hardware: OOS has run on
a DHO924S, a DHO804 and a DHO802 so far. A report from a DHO914/DHO924 without generator or an EDU model makes
that whole hardware variant verified ([docs/models.md](docs/models.md) 2.1).

## Ground rules

- **Be kind.** Issues, pull requests, reviews and the forum follow the
  [code of conduct](CODE_OF_CONDUCT.md) (the Contributor Covenant 2.1). Where to ask what: [SUPPORT.md](SUPPORT.md).
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

## Adding something

Each kind of extension has one place where it is declared, a few files that follow by hand, a command for what is
generated, and a test that fails, with the name of the item and of the missing part, when one of them is forgotten
([ADR 0046](docs/adr/process/0046-every-kind-of-extension-has-a-recipe-and-a-check.md)). A part that cannot be there
stands in a list in that test with its reason; the list only gets shorter. [AGENTS.md](AGENTS.md#adding-something)
has the short form, [docs/architecture.md](docs/architecture.md#where-a-change-goes) says where every other kind of
change goes. Whatever you add brings its texts along (AGENTS.md, "Keeping the texts true"), and a file in
`changelog.d/` when a user notices it ([changelog.d/README.md](changelog.d/README.md)).

### A new setting

- **Declare it** as one row in its group's file of `ui/src/settings/`: the key in setup files, the type with its
  range or choices, the menu label, the SCPI command or why it has none, and its docs text. The setup file, the
  remote command, the menu row and the table in `docs/scpi.md` follow from the row
  ([ADR 0043](docs/adr/code/0043-one-declaration-per-setting.md),
  [ADR 0044](docs/adr/code/0044-settings-table-real-numbers-and-row-steps.md)). The groups with a table so far: Display,
  the trigger values, acquisition, math, the meters, the channels, mask, histogram, Bode, the logic
  analyser's switch, spectrum, eye and power. A setting of the other groups is still written out in each of those
  places.
- **Also touch** its field of the model (`ui/src/model/`), its `Item` and its place in a menu (`ui/src/ui/item.rs`,
  `ui/src/ui/menu_items.rs`, `ui/src/ui/setting_items.rs`), its query in `ui/tests/scpi.txt`, and its probes in
  `ui/src/store/settings_snapshot/setup_probes.rs` (with a command also in
  `ui/src/store/settings_snapshot/scpi_probes.rs`).
- **Generate** its line of the docs table with `(cd ui && OOS_BLESS=1 cargo test --locked -q settings::tests)` and
  its lines of `ui/tests/golden/settings.txt` with `(cd ui && OOS_BLESS=1 cargo test --locked -q settings_snapshot)`.
- **The check:** `ui/src/settings/tests.rs::every_row_is_complete` fails on a row without its command or the reason,
  its docs text or its query; `ui/src/ui/setting_items.rs::every_row_has_its_item_and_a_step_changes_it` on a row
  that is in no menu; `ui/src/store/settings_snapshot/setup_probes.rs::every_key_the_setup_writes_or_reads_is_probed`
  on a key without probes.

### A new measurement

- **Declare it** as a `Def` among the `defs` of a measurement module in `ui/src/measures/`: its number, name, unit,
  drawn mark, source roles, its place on the measurement page and one sentence on what it measures. A new module is
  one more file there and its line in `MODULES` (`ui/src/measures/mod.rs`). The tile, the Selected list, statistics,
  logger columns, setups and SCPI (`CAT<id>`) follow. [docs/measurement-modules.md](docs/measurement-modules.md) walks
  through it.
- **Also touch** the signal processing in `dsp/src/` with its test in `dsp/tests/` when it is more than a few lines,
  a test of the module with known numbers next to it, and the row of its category in the table of
  `docs/reference-analysis.md` when it opens a new group.
- **Generate** its line of `ui/tests/golden/measurement-ids.txt` with
  `(cd ui && OOS_BLESS=1 cargo test --locked -q measures::tests)`. The list only gains lines: a number never changes
  and is never given to another measurement.
- **The check:** `ui/src/measures/mod.rs::every_measurement_is_complete` fails on a measurement without a name, a
  description, its group in the docs table or its line in the list of numbers, and on a line of that list that is no
  longer registered as it stands; `ui/src/measures/mod.rs::modules_are_well_formed` on a number or a name used twice;
  `ui/src/measures/mod.rs::modules_compute_without_panicking` runs every module on a square wave. That a module has a
  test with known numbers is not checked yet.

### A new protocol decoder

- **Declare it** twice. In `oos-dsp`, what the decoder is: its `ModuleInfo` next to the decoder in the file of its
  family under `dsp/src/decode/` (key, name, family, lines, the call of the decoder with its options and shared rows,
  its bit time, a trigger rule of its own and the trigger's start value), with its line at the end of `MODULES`
  (`dsp/src/protocols.rs`). In `oos-ui`, what the scope shows of it: a type that implements `Protocol` in a file of
  `ui/src/protocols/` that names the `ModuleInfo` and brings the option rows, labels, the first settings, the demo
  traffic and a view, with its line in `MODULES` (`ui/src/protocols/mod.rs`). The Decode menu, setups, the protocol
  trigger, the labels, the demo traffic and its view follow. [docs/protocol-modules.md](docs/protocol-modules.md)
  walks through it.
- **Also touch** the decoder in the file of its family under `dsp/src/decode/`, its generator in
  `dsp/src/decode/gen.rs` or `dsp/src/decode/gen/` and its tests in the family's module of `dsp/tests/decode/`; for a
  new file of `ui/src/protocols/` its `mod` line; with demo traffic its case in `ui/src/ui/tests.rs::extended_decoders_decode_their_demo`; its name
  under Protocol decoders in `wiki/Features.md`, and the number of decoders there and in `README.md`.
- **Generate** its line of `ui/tests/golden/decoder-keys.txt` with
  `(cd ui && OOS_BLESS=1 cargo test --locked -q protocols::tests)`. The list only gains lines: a key never changes.
- **The check:** `ui/src/protocols/mod.rs::every_decoder_is_complete` fails on a decoder without its key in the list,
  its `ModuleInfo` in the registry of `oos-dsp` (or out of its order), its name in the wiki, demo traffic (or the
  reason in `NO_DEMO`), its case on the demo screen or the feature of its file, on a decoder of `oos-dsp` without a
  module, and on a number of decoders in the texts that is not the number registered;
  `dsp/src/protocols.rs::every_declared_module_is_registered` on a `ModuleInfo` that is not in the registry of
  `oos-dsp`, `dsp/src/protocols.rs::modules_are_well_formed` on a key or a name used twice or an info that is not in
  the file of its family, `dsp/src/protocols.rs::every_module_has_its_decoder_and_bit_time` on an info without the
  call of its decoder or without its bit time;
  `ui/src/protocols/mod.rs::modules_are_well_formed` on a key or a name used twice;
  `ui/src/protocols/mod.rs::modules_decode_their_demo` on demo traffic the module does not decode;
  `dsp/tests/decode/main.rs::every_decoder_family_has_its_tests` on a family of `dsp/src/decode/` without tests.

### A new trigger type

- **Declare it** as a variant of `TrigType` with its place in `TRIG_TYPES` and its name (`ui/src/model/trigger.rs`).
  The type picker of the Trigger menu and the word in setup files follow. Trigger types have no registry yet: the
  rest is written by hand, and the compiler names most of it (a `match` without the new variant does not build).
- **Also touch** the software trigger in `dsp/src/trigger.rs` with its test in `dsp/tests/triggers.rs`; the hardware
  trigger as a register sequence (below: `hal/src/dho/tpu.rs`, `Trig` in `hal/src/dho/seq/settings.rs`,
  `docs/protocols/trigger.md`) and its mapping in `ui/src/board/dho/hw/settings.rs`; the rows of its menu
  (`ui/src/ui/menu_items.rs`); the two lists of words of `:TRIGger:MODE` in `ui/src/scpi/dispatch.rs`; the words in
  `docs/scpi.md` and `wiki/SCPI-and-VXI-11.md`, its name under Trigger in `wiki/Features.md`; its pictures in
  `ui/tests/triggers.txt`; its row in `TYPES` of the check.
- **Generate** nothing.
- **The check:** `ui/src/model/trigger.rs::every_trigger_type_is_complete` fails on a type that is not in
  `TRIG_TYPES`, has no name, whose stored word or SCPI words differ from its row in `TYPES` or do not set it, whose
  words the docs or the wiki do not name, or that has no picture (or the reason in `NOT_SHOT`);
  `ui/src/ui/tests.rs::every_trigger_type_triggers_on_the_demo` on a type that does not trigger on the demo signals.

### A new menu page

- **Declare it** as a module in `ui/src/ui/` with the functions of its page (its rows, its title, what its pickers
  do), and its line in `PAGES` (`ui/src/ui/pages.rs`,
  [ADR 0060](docs/adr/code/0060-a-menu-page-is-declared-once-in-a-table-of-pages.md)). The shared menu code asks
  `PAGES` for its title, rows, anchor and pickers, and for its `sources`, the signals it reads: a channel among them
  that is off gets a button on the page to switch it on (`off_sources`). The layout check opens it. A page of only a
  title, rows and sources is one call of `page!`.
- **Also touch** its `MenuId` and its `mod` line (`ui/src/ui.rs`), its rows as `Item`s with their `Part`
  (`ui/src/ui/item.rs`), the row that opens it on its parent page, its longest states in `LAYOUTS` (`pages.rs`), and
  its page in the wiki.
- **Generate** nothing.
- **The check:** `ui/src/ui/pages.rs::every_page_is_complete` fails on a kind of `MenuId` that is not in `PAGES` or
  is there twice, and on a page without a title or rows (but Add measurement, a layout of its own);
  `ui/src/ui/pages.rs::a_picker_has_one_page` on a picker two pages answer for;
  `ui/src/ui/lint/menu_layout.rs::every_menu_fits` on a page or a state of `LAYOUTS` that does not fit a screen.
  `tools/code-metrics.py` counts the lines of the shared menu files that still name a page (`menu-arms`, layer
  `ratchet`); it only goes down.

### A new service

- **Declare it** as a `Service<Job, Report>` in its part's state (`ui/src/ui/service.rs`,
  [ADR 0061](docs/adr/code/0061-a-service-of-the-screen-has-one-shape-and-the-frame-loop-polls-one-list.md)), with
  the worker function of its thread, and its poll in `POLLS` (`ui/src/ui/acquire.rs`). A job of one input and one
  result is a `Worker<J, R>` (`ui/src/ui/worker.rs`) instead.
- **Also touch** the threads of `docs/architecture.md`.
- **Generate** nothing.
- **The check:** `ui/src/ui/acquire.rs::every_poll_is_in_polls` fails on a `fn *_poll(&mut self ...)` that is in
  neither `POLLS` nor `POLLED_ELSEWHERE`, on a listed one that is gone, and on one listed twice.

### A new analyser page

- **Declare it** as a type that implements `Analyser` at the end of its file in `ui/src/ui/`, with its line in
  `ANALYSERS` (`ui/src/ui/analysers.rs`). The start menu, System > Features, the view bar, `:VIEW`, setups and the
  drawing follow. [docs/analyser-modules.md](docs/analyser-modules.md) walks through it.
- **Also touch** its variant of `AView` and of `AView::ALL` (`ui/src/model/analysers.rs`), its `MenuId` and its `mod`
  line (`ui/src/ui.rs`), its menu rows (`ui/src/ui/item.rs`, `ui/src/ui/menu_items.rs`) and its settings as rows of a
  table (above), the signal processing in `dsp/src/` with its tests, its menu in the list of `menus_fit_at`
  (`ui/src/ui/lint/menu_layout.rs`), its keyword in the `:VIEW` rows of `docs/scpi.md` and
  `wiki/SCPI-and-VXI-11.md`, its name under Analysers in `wiki/Features.md`, and its row in `PAGES` of the check.
- **Generate** nothing.
- **The check:** `ui/src/ui/analysers.rs::every_page_is_complete` fails on a page whose feature id, view word or
  keyword differ from its row in `PAGES`, whose keyword the docs or the wiki do not name, whose name the wiki's
  features lack, or whose menu the layout check does not open; `ui/src/ui/analysers.rs::analysers_are_well_formed` on
  an id, a name, a menu or a keyword used twice;
  `ui/src/ui/analysers.rs::every_page_runs_and_is_reached_the_same_way` runs and draws every page on the demo. No
  page is in the pictures of the comparison yet.

### A new remote command

- **Declare it** in the one interpreter, in the file of its subsystem under `ui/src/scpi/` (`ui/src/scpi/dispatch.rs`
  for the trees without a file of their own), never in a transport
  ([ADR 0022](docs/adr/code/0022-one-scpi-interpreter-transports-only-frame.md)). It takes the stock name where the
  stock firmware has one ([ADR 0023](docs/adr/code/0023-scpi-stock-names-first-oos-identifies-as-oos.md)). A command
  that only sets a setting is no code of its own: it is the command of the setting's row (above).
- **Also touch** its row in `docs/scpi.md`, its query in `ui/tests/scpi.txt` (the comparison then sees a change of
  its answer), for a setter with a query its probes in `ui/src/store/settings_snapshot/scpi_probes.rs`, and its row
  in `wiki/SCPI-and-VXI-11.md`.
- **Generate** its lines of `ui/tests/golden/settings.txt` with
  `(cd ui && OOS_BLESS=1 cargo test --locked -q settings_snapshot)`.
- **The check:** `tools/scpi-coverage.py` with `--enforce` (the layer `ratchet` of `tools/check.sh`) fails on a
  command word `docs/scpi.md` does not name and on a documented query `ui/tests/scpi.txt` does not ask; a query that
  cannot be asked stands in its `NOT_ASKED` with the reason.
  `ui/src/store/settings_snapshot/scpi_probes.rs::every_documented_setter_is_probed` fails on a setter without
  probes. The wiki's row is not checked yet.

### A new register sequence

- **Declare it** as a function without side effects in the module of its FPGA block under `hal/src/dho/` that
  returns the steps as `(offset, value)` writes
  ([ADR 0001](docs/adr/code/0001-three-crates-with-pure-hal-and-dsp.md)); a new block is a new module with its line in
  `hal/src/dho/mod.rs`.
- **Also touch**, first, the page of `docs/protocols/` that describes the registers as behaviour and says how each
  fact is known ([ADR 0030](docs/adr/process/0030-independent-reimplementation-from-black-box-descriptions.md)); the
  module's header names that page. Then a test of the steps in the module, the place that uses them (the acquisition
  sequence in `hal/src/dho/seq.rs`, a command of `oos-fpgad` in `fpgad/src/`, the board in `ui/src/board/dho/`), and
  a line in the board's matrix, `tools/board/dho/plan-matrix.sh` (run by `tools/plan-matrix.sh`), that prints the
  sequence, so the comparison sees it.
- **Generate** nothing.
- **The check:** `hal/tests/register_modules.rs::every_module_names_its_page_and_has_a_test` fails on a module whose
  header names no page of `docs/` (or a page that is not there) and on one without a test, unless it stands in
  `NO_PAGE` or `NO_TEST` with the reason. `tools/compare-base.sh` compares the sequences `tools/plan-matrix.sh`
  prints with those of the base; a sequence it does not print is not compared, and nothing checks that yet. A change
  of a sequence is tried on a unit before it is merged (AGENTS.md).

### A new board family

- **Declare it** as a `Target` in `hal/src/<name>/target.rs`, with `name` equal to the folder name, listed in
  `FAMILIES` of `hal/src/target.rs`, and its line `pub mod <name>;` in `hal/src/lib.rs`
  ([ADR 0067](docs/adr/hardware/0067-a-board-family-is-a-module-behind-target-and-board-one-image-each.md)).
- **Also touch** `ui/src/board/<name>/` with its `Board` and its line in `ui/src/board/mod.rs`; `rootfs/board/<name>/`
  with `etc/oos-board` (the keys of `rootfs/board/dho/etc/oos-board`) and an executable `bin/oos-board` that answers
  every command `rcS` asks; the family's row in [docs/porting.md](docs/porting.md#board-families), in the state
  `in development`; its matrix of register sequences `tools/board/<name>/plan-matrix.sh`, which `tools/plan-matrix.sh`
  and the comparison run with `OOS_BOARD=<name>`; its device tree under `kernel/`; and the pages of `docs/protocols/`
  its hardware code is written from. Build its image with `OOS_BOARD=<name>`. The maintainer changes the state to
  `ready`.
- **Generate** nothing.
- **The check:** `hal/tests/board_families.rs::every_board_family_is_complete` fails, naming the family and the part,
  when a folder of `rootfs/board/` lacks its `Target` in `FAMILIES`, its `impl Board for`, its `plan-matrix.sh`, a key
  of `etc/oos-board`, a command of `bin/oos-board` or its row in docs/porting.md, and when a row or a `Target` has no
  folder. The tests of `hal/src/target.rs` compare each family's `etc/oos-board` with its `Target`. `oos-fpgad` is not
  yet one of the parts: it is a DHO800/DHO900 tool until it is split into a neutral part and one module per family
  (ADR 0067).

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

It runs every host check CI runs, layer by layer: the layout of the code, Clippy, files, script syntax, the tests of
all four crates, the scope's build, the decoder builds, and the numbers that only go down (`tools/ratchet.txt`, ADR
0037) ([ADR 0020](docs/adr/process/0020-checks-in-layers-one-command-on-the-host.md)). A compiler
warning fails it like an error. `tools/check.sh test` runs one layer, `tools/check.sh list` names them. Tests that
need the calibration files of a real unit skip themselves without them. Start reading with the
[hardware overview](https://dekajoule.github.io/oos/hardware/overview.html) and
[docs/architecture.md](docs/architecture.md); to run OOS on your computer without a scope, see
[wiki/Building.md](wiki/Building.md#try-it-without-a-scope).

Each crate builds on its own. The hardware code is Linux-only, so it is checked for the target as well:

```sh
(cd ui && cargo test)                                 # one crate
(cd ui && cargo check --target aarch64-unknown-linux-musl)
```

- **Calibration files.** To run the tests that need them, copy `/rigol/data` of your scope to `local/cal-full` and
  `local/cal` (the folder `local/` stays out of git). `OOS_CAL_DIR` overrides this for most of them.
- **Register sequences.** `tools/plan-matrix.sh` prints the register sequences of a fixed set of acquisitions without
  touching the hardware. Run it with two builds and compare the output to prove that a change leaves the analog path
  alone; `tools/compare-base.sh` does that for you. Without the files of a unit, `oos-fpgad made-up-cal DIR` writes a
  made-up calibration set for it (`OOS_CAL_DIR=DIR`): invented values that mean nothing for a scope, enough to see
  that a change moves a calibrated register value ([self-cal.md 16](docs/protocols/self-cal.md)).
- **Menus and pictures.** The test `every_menu_fits` walks all menus and fails on text wider than its control or a
  dialog off the screen. `OOS_GALLERY=<dir> cargo test gallery -- --ignored` in `ui/` writes every screen as a picture.
- **Simulations.** `tools/gel-sim.sh` runs the USB install on a file laid out like a stock card. `tools/usb-sim.sh`
  tests the handling of USB sticks in a virtual machine, with disks that get pulled. `tools/wlan-sim.sh` tests the
  WLAN path on simulated radios in a virtual machine, without a stick; it needs Docker, `qemu-system-aarch64` and one
  finished image build. All host tools: [tools/README.md](tools/README.md).

Say in the pull request which checks ran and where: on a unit (which model), only on the host, or not at all.

CI also compares every pull request with its base: the pictures of the screen and the register sequences
(`tools/compare-base.sh`, about three minutes on a laptop). The register sequences are printed with a made-up
calibration set, because CI has no files of a unit: it shows that a calibrated value moves, not what a real unit
gets. The result is in the summary of the `compare` job, with the pictures that differ to download. A difference is
not an error; a pull request that says `Behaviour: unchanged` while something differs is.

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
- A pull request can be merged when six checks have passed
  ([ADR 0038](docs/adr/process/0038-auto-merge-for-the-maintainer-and-the-bots.md),
  [ADR 0039](docs/adr/process/0039-one-development-agent-with-its-own-identity-merges-under-the-checks.md)): `test` and
  `compare` from CI, and four statuses: `waited` (the waiting time is over), `owner` (it is the maintainer's or the
  development agent's, or the maintainer approved its newest commit), `unit` (it changes nothing that is only proven
  on a scope, it was tried on one, the programs for the scope are byte-identical to the base's, or it is a
  refactoring of register-sequence code the comparison found equal) and `review` (GitHub Copilot reviewed it and
  its comments are answered). A pull request from a fork needs the maintainer's approval and is merged by the
  maintainer.
- No waiting time for a pull request that only corrects documents, or that says `Behaviour: unchanged` and whose
  comparison with the base found nothing, outside the hardware paths, the pipeline, the tools and the rules. The
  label `hold` stops any pull request.
- The pull request opened first is merged first when two touch the same code; the later one is rebased. Allow edits by
  maintainers on your pull request and the rebase is done for you.
- Large changes that do not change behaviour (formatting, moves) are announced in an issue with their day, and wait
  for the open pull requests.
- Every pull request gets an automatic review (GitHub Copilot code review) once it is ready for review, one per
  pull request. Its comments are advice: take the ones that are right, and say so where one is wrong. It does not
  approve and it does not merge. What it is asked to look for is in
  [.github/instructions/review.instructions.md](.github/instructions/review.instructions.md).

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
  [ADR 0017](docs/adr/process/0017-lints-clippy-warnings-are-errors.md)). Both are enforced: `tools/check.sh`
  fails on code that `cargo fmt` would change and on a Clippy warning (on a host that is not Linux also for the code
  only the scope's build compiles). A branch from before 2026-11-17 runs `cargo fmt` on itself first and rebases
  then.
- Commits: `Area: what changed, as a statement`, one topic each, the body says why and how it was tested; formatting
  and lint fixes in commits of their own. A change people would notice on the scope gets a file in `changelog.d/`
  ([ADR 0019](docs/adr/process/0019-one-style-for-text-files-and-commits.md),
  [ADR 0065](docs/adr/process/0065-lists-every-change-adds-to-are-written-by-a-tool-or-kept-as-a-file-per-entry.md)).

A change that decides something others will build on (a structure, a rule of the screen, a way to treat the hardware)
comes with a decision record: [docs/adr](docs/adr/README.md).

## Working with a coding agent

Most code here is written with one. [AGENTS.md](AGENTS.md) holds the instructions for all of them (`CLAUDE.md` only
points to it); point your tool to it if it does not read it by itself. The agent's report counts like yours: it says
what was tested on a unit, what only on the host, and what was not tested. An agent does not write to a scope unless
its owner asked for it ([ADR 0018](docs/adr/process/0018-one-agents-md-for-people-and-agents.md)).

Pull requests with the label `agent` come from the maintainer's development agent
([ADR 0039](docs/adr/process/0039-one-development-agent-with-its-own-identity-merges-under-the-checks.md)), a coding
agent working on a task the maintainer gave it. They follow the same rules and checks as yours, and Copilot reviews
them. If one of them is wrong, say so in the pull request. Once a week a script lists what is new in the forum and in
the fork as an issue with the label `scout`.

## Licence

By contributing you agree that your work is published under the licence of the project,
[GPL-2.0-or-later](LICENSE).
