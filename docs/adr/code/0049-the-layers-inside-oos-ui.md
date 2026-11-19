# ADR 0049: The modules of `oos-ui` are in layers; a table says what a layer may name, and the crossings only go down

- Status: accepted (2026-11-18, by the maintainer), amended by ADR 0064 (a layer whose count is 0 may leave as a
  crate)
- Date: 2026-11-18
- Category: code

## Context

`oos-ui` is one program of 68 726 lines of running code (`tools/code-size.py`, 2026-11-18, commit 604a2771), 69 % of
the four crates: the model, the settings table, the setup files, the SCPI interpreter, the screen, the network
services and the board interface. The crates have a direction (ADR 0001) and `tools/layers.py` keeps it at 0
crossings. Inside `oos-ui` nothing says what may name what. Goal S3 of issue #120 asks for it: "ui is split into
model, view, control and transports, the board behind one interface. Measure: imports across the layers. Target 0
crossings, enforced."

Measured on that commit, from the paths written from `crate`, `super` and `self` and the leaves of every `use`,
without test code:

- 24 of the 47 modules at the top of `ui/src` name each other in a circle: `board`, `cmd`, `compress`, `deepmeas`,
  `drawpipe`, `fastdraw`, `framestats`, `hislip`, `http`, `math`, `measures`, `model`, `protocols`, `remote`, `scpi`,
  `settings`, `sparse`, `ssh`, `store`, `trig`, `trignear`, `ui`, `vxi11`, `wave`. The other 23 are in none.
- The circle is held by few paths against many. `ui` names `store` 81 times, `store` names `ui` 18 times; `ui` names
  `board` 64 times, `board` names `ui` 8 times; `scpi` names `ui` 45 times and `ui` never names `scpi`; `store` names
  `settings` 56 times, `settings` names `store` 12 times. `vxi11` and `hislip` name only `scpi` and the network
  helpers, as ADR 0022 asks.
- What goes against the direction is mostly a thing in the wrong file: the settings type and the record of the logic
  analyser are in the page's file (`ui/la.rs`) and are named by the model, the board, `wave` and `store`; limits of the
  model (`VIEWS_MAX`, `PANES_N`) are constants of the screen; the parsers of a value are in `scpi.rs` and `store.rs`
  and are named by the settings table; thread pinning and base64 are in `http.rs`.

So the code has layers already, and nothing keeps a new path from going the other way.

## Decision

Every module of `oos-ui` is in one layer, and a layer names only itself and the layers its row allows. The table is
`UI_LAYERS` in `tools/layers.py`; it follows what the code does today, not an ideal.

| Layer | Modules (`ui/src/`) | What it is | May name |
|---|---|---|---|
| base | `clock` `drm` `ethlink` `font` `gfx` `hostname` `json` `layout` `mdns` `net` `net6` `netacl` `placement` `remote` `sntp` `ssh` `sysconf` `system` `usbtmc` `vpu` | helpers and services without the scope's state: drawing, fonts, the clock, `system.conf`, the network helpers, the services the screen only switches | nothing else |
| model | `model` `math` `zone` `decode` | what the user has set, its ladders and number formats; no files, no screen | base |
| settings | `settings` | one row per setting (ADR 0043) | base, model |
| board | `board` | the one door to the hardware (`board::Board`) | base, model |
| signal | `wave` `trig` `protocols` `measures` `compress` `deepmeas` `drawpipe` `fastdraw` `framestats` `sparse` `trignear` | acquisition and analysis: sources, records, the software trigger, decoders and measurements as modules, the trace maps | base, model, board |
| store | `store` | setup and waveform files | base, model, settings, signal |
| screen | `ui` | view and control: menus, pages, drawing, input | base, model, settings, board, signal, store |
| command | `scpi` `cmd` | the interpreters: SCPI and the control commands | base, model, settings, board, signal, store, screen |
| transport | `http` `vxi11` `hislip` `announce` | frame the messages of a computer | base, command |
| main | `main.rs` `input` `gpu` | the wiring: frame loop, inputs, GPU helper | everything |

- **The interpreter is above the screen.** It gets `&mut Ui` from the frame loop for each program
  (`Scpi::poll_program`, called in `main.rs`) and calls the screen's methods; the screen keeps what a command waits for
  (`ui/scpi_operations.rs`) and names no interpreter. A transport gets a closure from `main.rs` that runs a program,
  so it names `scpi::Program` and nothing of the screen (ADR 0022).
- **View and control stay one layer.** Of the 88 files below `ui/src/ui/`, 82 hold a part of `impl Ui` and 83 take
  the names of their parent by `use super::*`; their paths cannot tell a view from a control.
- **A file's layer is its module's row.** A row names a module from the crate root, and the longest row that fits
  places a file, so `ui` places every file below `ui/src/ui/`. A new module at the top of `ui/src` has no row:
  `ui-unplaced` counts it, and the test `every ui module has a layer` of `tools/tests/test_layers.py` fails, until it
  is placed.
- **The crossings are numbers that only go down** (ADR 0037). `tools/layers.py` counts, per layer, the paths that name
  a layer its row does not allow, and `tools/layers.txt` holds the counts: `ui-base-up`, `ui-model-up`,
  `ui-settings-up`, `ui-board-up`, `ui-signal-up`, `ui-store-up`, `ui-screen-up`, `ui-command-up`,
  `ui-transport-skip` (a transport that names the model, the store or the screen instead of an interpreter) and
  `ui-unplaced`. A path counts where it is written from `crate`, `super` or `self`, a `use` tree as its leaves. The
  layer `ratchet` of `tools/check.sh` fails on a count above its number; raising one is the maintainer's decision.
- **A change of the table is a change of this decision**: a module that moves to another row, or a layer that may
  name one more, comes in a pull request that says why. Lowering a number needs no reason.

## Alternatives

- **A crate per layer in a workspace.** The compiler would refuse a crossing, which a count cannot. It cannot be
  switched on: crates may not name each other in a circle, and 145 paths do today. The cost stays after that: a
  workspace, or more path dependencies and lock files, through `tools/check.sh`, `ui/build.sh`, the image build and
  CI's caches; `pub(crate)`, `pub(super)` and `pub(in ..)`, on about 1350 lines of `ui/src`, become `pub` on every item
  another layer names, so the visibility says less than today; and the tests, which run at `opt-level = 1` without
  LTO (ADR 0040), lose inlining across the boundaries, while the release build keeps it. Worth a second look when the
  counts are 0: then a layer can become a crate, one at a time.
- **The four layers of goal S3 as they are named (model, view, control, transports).** View and control cannot be
  told apart (above), and the acquisition, the store and the settings table would have no place.
- **`board` and `signal` as one layer.** The acquisition thread names the draw pipeline and the pipeline names the
  board's frame; one layer would hide the largest tangle (47 paths) instead of counting it.
- **An ideal placement** (`mdns` among the transports, `math` beside the decoders). It would count paths that harm
  nothing: `mdns` names no module of `oos-ui`, `math` names two constants of the model.
- **Counting files or pairs of modules instead of paths.** Coarser: a file with one path left counts like one with
  twenty, so work on it shows late. Paths are also what `--list` can print for a reviewer.

## Consequences

- The counts on 2026-11-18 (commit 604a2771), 145 paths in all:

  | Count | Paths | What they are |
  |---|---|---|
  | `ui-board-up` | 47 | the acquisition thread feeding the draw pipeline (`drawpipe` 13, `compress` 8, `deepmeas` 5, `fastdraw` 2), `board/record.rs` making screen records (`wave`, `trig`, `trignear`: 9), the record of the logic analyser in `ui/la.rs` (8), `protocols` (2) |
  | `ui-settings-up` | 32 | value parsers in `scpi.rs` (17) and `store.rs` (12), three tables of the screen |
  | `ui-model-up` | 25 | the registry of the decoders (`protocols` 16), the board's limits (4), `ui/la.rs` (3), `ui::VIEWS_MAX`, `measures::persisted` |
  | `ui-store-up` | 18 | limits and tables of the screen (14), `ui/la.rs` (2), the views (2) |
  | `ui-signal-up` | 9 | `ui/la.rs` (4), thread pinning in `http.rs` (3), two functions of the screen named by `measures` |
  | `ui-transport-skip` | 7 | `http.rs`: the PNG and the waveform reader of `store` (6), `Ui` |
  | `ui-screen-up` | 5 | `cmd::run` (2), `http::to_little_cores`, `http::STYLE`, `vxi11::CORE_PORT` |
  | `ui-base-up` | 2 | `http::base64` in `ssh.rs` |
  | `ui-command-up`, `ui-unplaced` | 0 | |

- Cheap first, each a move of code without a change of behaviour: base64 and the thread pinning out of `http.rs` into
  base (6 paths in three counts); the value parsers (`scpi::kw`, `num`, `e`, `parse_bool`, `nearest_index`,
  `store::p_bool` and its siblings) below the settings table (29); the limits and tables of the screen that the
  model, the store and the settings name (`VIEWS_MAX`, `PANES_N`, `AVERAGES` and their like) into the model (18); the
  settings type and the record of the logic analyser out of `ui/la.rs` (17). `board/record.rs` says it is
  board-neutral and makes records: in `signal` its 9 paths to `wave`, `trig` and `trignear` are none. Most of what is
  left then is the hard part: the acquisition thread feeding the draw pipeline (28), and the model asking the
  registry of the decoders (16).
- Some files are in two layers and stay counted until they are split: `ui/la.rs` (settings type, record, page),
  `board/record.rs` (signal code in the board's folder), `http.rs` (a transport of SCPI, the web remote that acts on
  `Ui` and reads uploads, thread pinning, base64), `scpi.rs` (the interpreter, the raw socket on port 5555, value
  parsers), `store.rs` (files and value parsers), `math.rs` (types of the model and their computation).
- A new module needs a row before `tools/check.sh` passes; a new path against the table needs an old one removed, or
  a raised number with its reason.
- The count is of imports, as S3 names it, not of uses: `use crate::model::*` is one path however much it brings in,
  a `use` of nine names is nine, and a name that reaches a file through `use super::*` is not seen. It is a number
  that shows the direction, not a proof; a crate boundary would be the proof.
- Worth a second look: the table when a count reaches 0 (the row can then be made stricter, or the layer a crate);
  `board` below `signal` if the acquisition thread turns out to belong to the pipeline; view and control once the
  files of the screen stop sharing their names.

## References

Issue #120 (goal S3, proposal of round 9); ADR 0001, ADR 0002, ADR 0022, ADR 0025, ADR 0037, ADR 0043;
`tools/layers.py` (`UI_LAYERS`), `tools/layers.txt`, `tools/tests/test_layers.py`; `ui/src/main.rs`,
[architecture.md](../../architecture.md) (Layers inside oos-ui).
