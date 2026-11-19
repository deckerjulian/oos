# Architecture decision records

One file per decision that shapes OOS: what was decided, why, and what follows from it. A record is short and stays as
it was written. A decision that changes gets a new record that supersedes the old one; the old one keeps its text and
gets the status "superseded by ADR NNNN".

The other documents say how things work now. These records say why they are that way, so nobody has to find it out
again from the history.

## Categories

| Folder | What belongs there |
|---|---|
| [code/](code/) | structure of the software: crates, modules, threads, processes, data paths |
| [ui-ux/](ui-ux/) | the screen: layout, operation by touch, keys and knobs, look |
| [hardware/](hardware/) | how OOS treats the scope's hardware: FPGA, front ends, storage, what it never touches |
| [system/](system/) | boot, the card, updates, network, the image |
| [process/](process/) | how the project works: tests, guards, what may enter the tree, documentation |

A decision that touches two categories goes where its consequences are largest and names the other in its text.

## Index

<!-- index: written by tools/adr-index.py from the records; run it instead of editing these lines -->
| ADR | Category | Decision | Status |
|---|---|---|---|
| [0001](code/0001-three-crates-with-pure-hal-and-dsp.md) | code | Three crates; the hardware layer and the signal processing are pure functions | accepted, amended by ADR 0064 |
| [0002](code/0002-extension-by-registered-modules.md) | code | Decoders, measurements and analyser pages are modules behind one registration | accepted, amended by ADR 0058 |
| [0003](code/0003-gpu-drawing-in-a-helper-process.md) | code | The GPU draws from commands in a helper process; the CPU stays the fallback | accepted |
| [0004](code/0004-standard-draw-path-by-default.md) | code | Two draw paths: Standard by default, Batch to choose | superseded by ADR 0029 |
| [0005](ui-ux/0005-one-measurement-page.md) | ui-ux | One measurement page; one tap adds or removes | accepted |
| [0006](ui-ux/0006-lists-scroll-by-bar-and-finger.md) | ui-ux | Lists scroll by a scroll bar and by the finger, not by buttons | accepted |
| [0007](ui-ux/0007-top-bar-with-four-entries.md) | ui-ux | The top bar holds four entries; everything else starts from the start menu | accepted, amended by ADR 0063 |
| [0008](ui-ux/0008-own-visual-style.md) | ui-ux | An own look: neutral colours, white accent, round shapes, no empty areas | accepted |
| [0009](hardware/0009-areas-oos-never-writes.md) | hardware | OOS never writes the FPGA flash, `/rigol/data`, `uboot`, `trust` or `parameter` | accepted |
| [0010](hardware/0010-unverified-hardware-functions-are-opt-in.md) | hardware | A hardware function that is not verified on a unit is off by default | accepted |
| [0011](system/0011-oos-in-recovery-stock-stays-bootable.md) | system | OOS lives in `recovery`; the stock system stays on the card and bootable | accepted |
| [0012](system/0012-signed-updates-with-trial-start-and-rollback.md) | system | Updates are signed, started on trial and rolled back when they fail | accepted |
| [0013](process/0013-no-vendor-code-only-libc.md) | process | No vendor code or data in the tree; `libc` is the only dependency | accepted |
| [0014](process/0014-refactorings-keep-every-pixel.md) | process | A refactoring keeps every pixel and every register write | accepted, amended by ADR 0032 |
| [0015](process/0015-say-how-you-know.md) | process | Every statement about the hardware says how it is known | accepted |
| [0016](process/0016-rust-code-formatted-by-rustfmt.md) | process | Rust code is formatted by rustfmt with the tree's settings, and CI checks it | accepted |
| [0017](process/0017-lints-clippy-warnings-are-errors.md) | process | Clippy runs in CI; its warnings are errors, with a short list of allowed lints that says why | accepted, amended by ADR 0045 and ADR 0048 |
| [0018](process/0018-one-agents-md-for-people-and-agents.md) | process | One `AGENTS.md` holds the project's instructions for coding agents; it points to the rules, it does not repeat them | accepted |
| [0019](process/0019-one-style-for-text-files-and-commits.md) | process | One style for files, documents and commits, written down and checked where a tool can | accepted |
| [0020](process/0020-checks-in-layers-one-command-on-the-host.md) | process | The checks come in layers; one command runs every host layer, the same as CI | accepted, amended by ADR 0032 |
| [0021](process/0021-libc-only-on-the-scope-platform-crates-on-the-desktop.md) | process | What runs on the scope depends on `libc` only; a desktop build may use its platform's crates | superseded by ADR 0033 |
| [0022](code/0022-one-scpi-interpreter-transports-only-frame.md) | code | One SCPI interpreter in the frame loop; every remote transport only frames its messages | accepted |
| [0023](code/0023-scpi-stock-names-first-oos-identifies-as-oos.md) | code | SCPI follows the stock command names where they exist; OOS identifies itself as OOS | accepted |
| [0024](system/0024-vxi11-on-the-scope-hislip-on-a-computer-own-discovery.md) | system | VXI-11 on the scope, HiSLIP on a computer; found by mDNS / DNS-SD and LXI identification | accepted |
| [0025](system/0025-remote-services-are-bench-lan-services.md) | system | Remote services are unauthenticated bench-LAN services, each with a switch, limited by `net.allow` | accepted, amended by ADR 0054 |
| [0026](process/0026-contributions-are-signed-off-and-hold-no-vendor-material.md) | process | A contribution is signed off, holds no vendor material and enters main as reviewed commits | accepted, amended by ADR 0030 |
| [0027](system/0027-newer-wlan-drivers-as-backported-modules.md) | system | A WLAN driver of a newer kernel comes as a module beside 6.12, with a patch of its own | accepted |
| [0029](code/0029-batch-is-the-default-draw-path.md) | code | Batch is the default draw path | superseded by ADR 0059 |
| [0030](process/0030-independent-reimplementation-from-black-box-descriptions.md) | process | OOS is an independent reimplementation: the tree holds black-box descriptions and code written to them | accepted |
| [0031](process/0031-every-change-enters-main-through-a-pull-request.md) | process | Every change enters main through a pull request, the maintainer's too; decision records are decided there | accepted |
| [0032](process/0032-ci-compares-a-pull-request-with-its-base.md) | process | CI compares the pictures and the register sequences of a pull request with its base | accepted |
| [0033](process/0033-development-on-linux-macos-and-wsl.md) | process | OOS is developed on Linux, macOS and Windows with WSL; there is no native Windows build | accepted |
| [0034](process/0034-standing-agents-get-their-task-from-a-machine-a-person-merges.md) | process | Standing agents get their task from a machine and open one pull request; a person merges | superseded by ADR 0039 |
| [0035](process/0035-every-text-has-one-home-the-wiki-comes-from-the-tree.md) | process | Every text has one home; the wiki is published from the tree, and a change brings its texts along | accepted |
| [0036](process/0036-the-rules-of-the-repository-are-files-a-change-by-others-needs-the-maintainers-review.md) | process | The rules of the repository are files in the tree; a change by anyone else needs the maintainer's review | accepted |
| [0037](process/0037-numbers-that-only-go-down.md) | process | Numbers that only go down: large files, switched-off lints, `unsafe` | accepted |
| [0038](process/0038-auto-merge-for-the-maintainer-and-the-bots.md) | process | A pull request of the maintainer or of a bot we set up is merged by itself when three questions are answered | accepted, amended by ADR 0039, ADR 0041 and ADR 0042 |
| [0039](process/0039-one-development-agent-with-its-own-identity-merges-under-the-checks.md) | process | One development agent, with an identity of its own, opens and merges pull requests under the checks | accepted |
| [0040](process/0040-ci-runs-what-a-change-can-reach-and-tests-run-optimised.md) | process | CI runs the parts a change can reach, and the tests run optimised | accepted |
| [0041](process/0041-equal-programs-for-the-scope-need-no-run-on-a-unit.md) | process | A change that leaves the programs for the scope byte-identical needs no run on a unit | accepted |
| [0042](process/0042-a-refactoring-the-comparison-found-equal-needs-no-run-on-a-unit.md) | process | A refactoring of register-sequence code that the comparison found equal needs no run on a unit | accepted, amended by ADR 0052 |
| [0043](code/0043-one-declaration-per-setting.md) | code | A setting is one row of a table, from which the setup file, SCPI, the menu and the docs follow | accepted |
| [0044](code/0044-settings-table-real-numbers-and-row-steps.md) | code | The settings table holds the real numbers of the analyser pages, and a row may step in its own way | accepted |
| [0045](process/0045-the-toolchain-and-the-base-images-are-pinned.md) | process | The Rust tool chain and the base images of the containers are pinned | accepted |
| [0046](process/0046-every-kind-of-extension-has-a-recipe-and-a-check.md) | process | Every kind of extension has one registry, one recipe and one check that names what is missing | accepted |
| [0047](process/0047-parsers-of-outside-input-have-hostile-input-tests.md) | process | Every parser of outside input has a hostile-input test in `cargo test` | accepted |
| [0048](process/0048-too-many-arguments-is-allowed-in-hal-only.md) | process | `too_many_arguments` is allowed for the whole crate in hal only | accepted |
| [0049](code/0049-the-layers-inside-oos-ui.md) | code | The modules of `oos-ui` are in layers; a table says what a layer may name, and the crossings only go down | accepted, amended by ADR 0064 |
| [0052](process/0052-what-a-run-on-a-unit-covers-by-class-of-path.md) | process | What a run on a unit has to cover depends on the class of the path | accepted |
| [0053](code/0053-the-board-code-runs-on-a-host-against-a-simulated-unit.md) | code | The board code runs on a host against a simulated unit | accepted |
| [0054](system/0054-ssh-is-the-only-shell-on-the-network.md) | system | SSH is the only shell on the network; there is no telnet | accepted |
| [0055](code/0055-the-remote-interface-is-stable-and-versioned.md) | code | The remote interface (SCPI and HTTP) is stable and versioned | accepted |
| [0056](code/0056-add-ons-run-outside-the-core-and-use-a-stable-extension-interface.md) | code | Add-ons run outside the core and use a stable extension interface | accepted |
| [0057](system/0057-bluetooth-input-through-bluez.md) | system | Bluetooth mice and keyboards through BlueZ on a system bus of their own | accepted |
| [0058](code/0058-a-protocol-decoder-is-described-and-run-in-oos-dsp-and-shown-in-oos-ui.md) | code | A protocol decoder is described and run in oos-dsp and shown in oos-ui | accepted |
| [0059](code/0059-batch-is-the-only-draw-path.md) | code | Batch is the only draw path | accepted |
| [0060](code/0060-a-menu-page-is-declared-once-in-a-table-of-pages.md) | code | A menu page is declared once, in a table of pages; a new page is one file and one line | accepted |
| [0061](code/0061-a-service-of-the-screen-has-one-shape-and-the-frame-loop-polls-one-list.md) | code | A service of the screen has one shape, and the frame loop polls the parts from one list | accepted |
| [0062](code/0062-the-state-of-the-screen-belongs-to-its-parts.md) | code | The state of the screen belongs to its parts; the fields of `Ui` itself only get fewer | accepted |
| [0063](ui-ux/0063-the-status-boxes-of-the-top-bar-come-from-one-list.md) | ui-ux | The status boxes of the top bar come from one list, right of the four entries | accepted |
| [0064](code/0064-a-layer-of-oos-ui-without-crossings-becomes-a-crate-of-its-own.md) | code | A layer of `oos-ui` without crossings becomes a crate in the workspace `ui/`, the system services first | accepted |
| [0065](process/0065-lists-every-change-adds-to-are-written-by-a-tool-or-kept-as-a-file-per-entry.md) | process | Lists that every change adds to are written by a tool or kept as a file per entry | accepted |
| [0066](code/0066-the-network-services-stay-in-the-frame-loop-and-do-not-wait-for-their-clients.md) | code | The network services stay in the frame loop, and they do not wait for their clients | proposed |
| [0067](hardware/0067-a-board-family-is-a-module-behind-target-and-board-one-image-each.md) | hardware | A board family is a module behind `Target` and `Board`, with an image of its own, and drives hardware once the maintainer calls it ready | proposed |
| [0068](process/0068-the-comparison-of-the-programs-leaves-out-panic-places-and-symbol-names.md) | process | The comparison of the programs for the scope leaves out the places of their panics and their symbol names | proposed |
<!-- end of the index -->

## Reading order for agents

An agent (or a new contributor) reads [AGENTS.md](../../AGENTS.md) first, then this index, then the records that touch
the change: ADR 0009, 0010, 0013, 0015, 0021 and 0030 for anything near the hardware or the dependencies, ADR 0014,
0020 and 0032 for how to show that a change is safe, ADR 0016 to 0019 for how code and text look, ADR 0022 to 0025 for
SCPI, VXI-11, HiSLIP and the other network services, ADR 0026 and 0031 for how a change enters main. The records are
short on purpose; reading the ones that apply costs minutes and saves a review round.

## Writing one

1. Copy [template.md](template.md) into the category's folder as `NNNN-short-title.md`. `NNNN` is the next free number
   over all categories, so a number names one record wherever it is filed.
2. Fill it in: the context (what forced a decision, the measurements), the decision in one or two sentences, the
   alternatives that were looked at and why they lost, the consequences (also the unwelcome ones).
3. Run `tools/adr-index.py`, which writes its line into the index above from the record (ADR 0065), and commit it
   in the same pull request as the change it belongs to.
4. Open the pull request with the status `proposed`. It stays open at least 72 hours for discussion and is merged as
   `accepted` when no objection is left open, or as `rejected` (ADR 0031).

Status values: `proposed` (under discussion), `accepted`, `accepted, to be reviewed` (with the condition for the review
in the record), `accepted, amended by ADR NNNN` (it holds, with a limit or an exception that the newer record
states), `superseded by ADR NNNN`, `rejected` (kept, so the idea is not tried a second time without new facts).

A record is not rewritten when the decision changes. Two kinds of change are made in place, with the date: a
correction that leaves the decision as it was (a file that moved, a name that changed, a case the record missed while
the code already had it), and a list the record keeps up to date (such as the keys of ADR 0010). Anything else is a
new record. Both kinds go through a pull request like every other change (ADR 0031).

The rules of the tree hold here too ([CONTRIBUTING.md](../../CONTRIBUTING.md)): English, no vendor code or data, and
for every statement about the hardware how it is known.

A new rule that agents must follow gets one line in [AGENTS.md](../../AGENTS.md) that names its record (ADR 0018).

To disagree with a record, open an issue or a pull request against it. A record that is `proposed` is the cheapest
place to change course: nothing is built on it yet.
