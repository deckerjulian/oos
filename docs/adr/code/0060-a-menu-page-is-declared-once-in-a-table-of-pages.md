# ADR 0060: A menu page is declared once, in a table of pages; a new page is one file and one line

- Status: proposed
- Date: 2026-11-18
- Category: code

## Context

The rows of a menu already have a registry: `PARTS` in `ui/src/ui/item.rs` asks the module that owns an `Item` for
its kind, label, value, step, action and options (ADR 0046). The menu itself does not. What a page is, `MenuId`, is
answered by `match` arms spread over the screen: its rows in `menu_items.rs` (93 lines there name a `MenuId`), its
title in `menu_text.rs` (73), where its dialog stands and how wide a picker is in `menu_dialog.rs` (31), what a
picked line does in `menu_actions.rs`, how it opens from the top bar in `input.rs` (30), its state polled in
`acquire.rs` or `save_recall.rs`, and its place in the layout lint in `ui/src/ui/lint/menu_layout.rs`, which is a
list of its own.

System > Network > Bluetooth (#216) is a typical new page: 643 lines in `ui/src/ui/bt.rs` and 1 to 8 lines in each of
11 other files of `ui/src/ui/`. Nothing fails when one of them is forgotten. The layout lint did not open the page
until the review asked for it, and the review found the top bar's box asking for the wrong menu (`menu` instead of
`menu_root()`), a mistake a shared answer would not have let happen. Every such page also makes the next pull request
that touches `menu_items.rs` or `menu_text.rs` conflict with it.

The analyser pages already show the way out (ADR 0002): `ANALYSERS` in `ui/src/ui/analysers.rs` is asked for
everything around a page, and `every_page_is_complete` names what one lacks.

## Decision

Every kind of `MenuId` has one entry in `PAGES`, a table in `ui/src/ui/pages.rs` beside `PARTS`, in the same style: a
struct of optional functions, filled in by the module that owns the page. The kind is the variant: `Channel(c)`,
`ChMore(c)` and `Decoders(k)` are one entry each, and its functions get the `MenuId` with its parameter. `Pick` is no
page: it is the picker of a row, drawn by the shared code from that row's options, and the page that opened it
answers for what a picked line does (`pick` below).

| Field | What it answers | Today in |
|---|---|---|
| `id` | the kind of `MenuId` it answers for | |
| `title` | the title of the dialog | `menu_text.rs` |
| `items` | its rows, in order | `menu_items.rs` |
| `parent` | the menu Back leads to, and the root the top bar lights | `menu_items.rs`, `bars.rs::menu_root` |
| `anchor` | where the dialog stands (beside a top-bar box, a card, the start menu) | `menu_dialog.rs` |
| `pick` | what a picked line of one of its pickers does, and whether the picker is wide | `menu_actions.rs`, `menu_dialog.rs` |
| `layouts` | the states the layout lint opens it in, beyond the default one | `lint/menu_layout.rs` |

The shared code asks `PAGES` and keeps no arm for a single page. A page that needs something the table has no field
for gets the field, for every page, in a pull request of its own.

- **One file and one line.** A new page is its module and its line in `PAGES`; it touches `MenuId` and `Item` for its
  variants and nothing else of the shared menu code.
- **The check.** `pages.rs::every_page_is_complete` walks one `MenuId` of every kind but `Pick` (a list in the test;
  an exhaustive `match` on `MenuId` beside it fails to compile when a new variant is not in it) and names each one
  without an entry, without a title or without rows. The
  layout lint opens every page of `PAGES` with every state of its `layouts` and every picker of its rows, so a page
  cannot be left out of it. The recipe goes into the table of AGENTS.md ("Adding something") and CONTRIBUTING.md.
- **A number that only goes down** (ADR 0037): the pull request that adds `PAGES` adds a count of the lines that name
  a `MenuId` in the shared menu files to `tools/code-metrics.py` (`ui-menu-arms` in `tools/code-metrics.txt`). The
  pages move into the table one group per pull request, each a refactoring that keeps every pixel (ADR 0014), and the
  number goes down with each.
- **Analyser pages stay as they are.** `ANALYSERS` is the registry of a page kind with more parts (SCPI, Features,
  setups, drawing); its pages get their `PAGES` lines from it, not by hand.

## Alternatives

- **A trait `Page` with one type per page.** Closer to `Analyser`, but a page's functions are methods of `Ui` today
  (`impl Ui` in its module), and a trait object would need `&mut Ui` passed in anyway. A table of function pointers
  is what `PARTS` does and needs no new idea; a page that needs its own type later can still have one behind its
  functions.
- **Generating the arms from a list** (a macro or a build script). It keeps the arms and hides them; the reader of
  `menu_items.rs` would still look for a page in a place the code does not show.
- **Leaving it.** Each page costs a dozen small edits in shared files and a review that finds the forgotten one.

## Consequences

- A feature pull request with a page touches its own file, the variants and one line; two feature pull requests no
  longer meet in `menu_items.rs` and `menu_text.rs`.
- The shared menu files shrink to the code that is the same for every page. Their size goes down with
  `ui-menu-arms`.
- A page's behaviour is read in one file. What is still in `ui.rs` about it is its state (ADR 0062).
- The move is many pull requests of no behaviour change; they are announced in an issue (ADR 0031) and follow each
  other, never two at once, because they all touch the same shared files.
- Worth a second look if pages start to need many fields that only one page uses: then a trait is the better shape.

## References

`ui/src/ui/item.rs` (`PARTS`), `ui/src/ui/analysers.rs` (`ANALYSERS`), `ui/src/ui/menu_items.rs`,
`ui/src/ui/menu_text.rs`, `ui/src/ui/menu_dialog.rs`, `ui/src/ui/menu_actions.rs`, `ui/src/ui/lint/menu_layout.rs`,
ADR 0002, ADR 0037, ADR 0046, ADR 0049, pull request #216.
