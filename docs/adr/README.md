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

| ADR | Category | Decision | Status |
|---|---|---|---|
| [0001](code/0001-three-crates-with-pure-hal-and-dsp.md) | code | Three crates; the hardware layer and the signal processing are pure functions | accepted |
| [0002](code/0002-extension-by-registered-modules.md) | code | Decoders, measurements and analyser pages are modules behind one registration | accepted |
| [0003](code/0003-gpu-drawing-in-a-helper-process.md) | code | The GPU draws from commands in a helper process; the CPU stays the fallback | accepted |
| [0004](code/0004-standard-draw-path-by-default.md) | code | Two draw paths: Standard by default, Batch to choose | accepted, to be reviewed |
| [0005](ui-ux/0005-one-measurement-page.md) | ui-ux | One measurement page; one tap adds or removes | accepted |
| [0006](ui-ux/0006-lists-scroll-by-bar-and-finger.md) | ui-ux | Lists scroll by a scroll bar and by the finger, not by buttons | accepted |
| [0007](ui-ux/0007-top-bar-with-four-entries.md) | ui-ux | The top bar holds four entries; everything else starts from the start menu | accepted |
| [0008](ui-ux/0008-own-visual-style.md) | ui-ux | An own look: neutral colours, white accent, round shapes, no empty areas | accepted |
| [0009](hardware/0009-areas-oos-never-writes.md) | hardware | OOS never writes the FPGA flash, `/rigol/data`, `uboot`, `trust` or `parameter` | accepted |
| [0010](hardware/0010-unverified-hardware-functions-are-opt-in.md) | hardware | A hardware function that is not verified on a unit is off by default | accepted |
| [0011](system/0011-oos-in-recovery-stock-stays-bootable.md) | system | OOS lives in `recovery`; the stock system stays on the card and bootable | accepted |
| [0012](system/0012-signed-updates-with-trial-start-and-rollback.md) | system | Updates are signed, started on trial and rolled back when they fail | accepted |
| [0013](process/0013-no-vendor-code-only-libc.md) | process | No vendor code or data in the tree; `libc` is the only dependency | accepted, amended by ADR 0021 |
| [0014](process/0014-refactorings-keep-every-pixel.md) | process | A refactoring keeps every pixel and every register write | accepted |
| [0015](process/0015-say-how-you-know.md) | process | Every statement about the hardware says how it is known | accepted |
| [0016](process/0016-rust-code-formatted-by-rustfmt.md) | process | Rust code is formatted by rustfmt with the tree's settings, and CI checks it | proposed |
| [0017](process/0017-lints-clippy-warnings-are-errors.md) | process | Clippy runs in CI; its warnings are errors, with a short list of allowed lints | proposed |
| [0018](process/0018-one-agents-md-for-people-and-agents.md) | process | One `AGENTS.md` holds the instructions for coding agents; it points to the rules | accepted |
| [0019](process/0019-one-style-for-text-files-and-commits.md) | process | One style for files, documents and commits, checked where a tool can | accepted |
| [0020](process/0020-checks-in-layers-one-command-on-the-host.md) | process | The checks come in layers; one command runs every host layer, the same as CI | accepted |
| [0021](process/0021-libc-only-on-the-scope-platform-crates-on-the-desktop.md) | process | What runs on the scope depends on `libc` only; a desktop build may use its platform's crates | accepted |

## Reading order for agents

An agent (or a new contributor) reads [AGENTS.md](../../AGENTS.md) first, then this index, then the records that
touch the change: ADR 0009, 0010, 0013, 0015 and 0021 for anything near the hardware or the dependencies, ADR 0014 and
0020 for how to show that a change is safe, ADR 0016 to 0019 for how code and text look. The records are short on
purpose; reading the ones that apply costs minutes and saves a review round.

## Writing one

1. Copy [template.md](template.md) into the category's folder as `NNNN-short-title.md`. `NNNN` is the next free number
   over all categories, so a number names one record wherever it is filed.
2. Fill it in: the context (what forced a decision, the measurements), the decision in one or two sentences, the
   alternatives that were looked at and why they lost, the consequences (also the unwelcome ones).
3. Add its line to the index above, in the same pull request as the change it belongs to.

Status values: `proposed` (under discussion), `accepted`, `accepted, to be reviewed` (with the condition for the review
in the record), `accepted, amended by ADR NNNN` (it holds, with a limit or an exception that the newer record
states), `superseded by ADR NNNN`, `rejected` (kept, so the idea is not tried a second time without new facts).

A record is not rewritten when the decision changes. Two kinds of change are made in place, with the date: a
correction that leaves the decision as it was (a file that moved, a name that changed, a case the record missed while
the code already had it), and a list the record keeps up to date (such as the keys of ADR 0010). Anything else is a
new record.

The rules of the tree hold here too ([CONTRIBUTING.md](../../CONTRIBUTING.md)): English, no vendor code or data, and
for every statement about the hardware how it is known.

A new rule that agents must follow gets one line in [AGENTS.md](../../AGENTS.md) that names its record (ADR 0018).

To disagree with a record, open an issue or a pull request against it. A record that is `proposed` is the cheapest
place to change course: nothing is built on it yet.
