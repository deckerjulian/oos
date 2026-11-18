# ADR 0043: A setting is one row of a table, from which the setup file, SCPI, the menu and the docs follow

- Status: proposed
- Date: 2026-11-18
- Category: code

## Context

A setting of the screen was written out in up to nine places: its line in `store::setup_text`, its arm in the long
match of `store::set`, its handler in the SCPI interpreter (`ui/src/scpi/`), its `Item` and its place in a menu, its
kind, label and value in `menu_text.rs`, its step in `menu_actions.rs`, its options in `menu_dialog.rs`, its line in
`docs/scpi.md` and its query in `ui/tests/scpi.txt`. Each place parsed and printed the value in its own way, and the
places did not always agree (issue #120). Nothing but review kept a new setting complete in all of them. The snapshot
of `ui/tests/golden/settings.txt` (PR #131) records what every stored key and every documented SCPI setter does with
valid, edge and bad values, so moving the settings is provable byte for byte.

## Decision

A setting is one row of a const table in `ui/src/settings/` (`Setting`: key, SCPI command or the reason it has none,
type with range or choices, menu label, docs text, getter and setter of the model). The setup file, the SCPI handler,
the menu rows (a `Part` in `ui/src/ui/setting_items.rs`) and a generated table in `docs/scpi.md` read it. Rows only
read and write the model; what a change sets off on the screen stays with the caller. Quirks of today's behaviour
(an SCPI input range wider than the stored one, the error text of a setup reader) are kept as fields of the row, not
smoothed away. The Display group is the first table; the other groups follow one pull request each.

## Alternatives

- **A derive macro or a crate for settings.** Would need a dependency or a proc-macro crate (ADR 0013), and hide the
  rows behind generated code. The table is plain data with function pointers, the pattern of `Part`/`PARTS` and of
  the measurement `Def`.
- **A trait per setting.** One type per setting, more code than a row, and no one place that lists them.
- **Keep the places and add a test that they agree.** The test would have to know each setting in each place, which
  is the list this table is.

## Consequences

- A new setting of a group with a table is a row, its `Item` with its place in a menu and in `setting_items.rs`, and
  its query in `ui/tests/scpi.txt`; `OOS_BLESS=1 cargo test --locked -q settings::tests` in `ui/` writes its docs
  line. Tests in `ui/src/settings/tests.rs` fail on a row without a docs line, a command or a reason, or its query.
- The setup keys of a table keep their places in the setup file: a group may be split into slices that
  `setup_text` writes where the keys were.
- A setting whose words follow no common rule is a `Ty::Custom` row with its own codecs; when a group needs many of
  them, the types are missing something.
- The docs texts are compiled into the tests only, so the program for the scope does not carry them.
- `tools/scpi-coverage.py` reads the commands and fields of the table as part of the interpreter.

## References

Issue #120; `ui/src/settings.rs`, `ui/src/settings/display.rs`, `ui/src/ui/setting_items.rs`,
`ui/src/scpi/display.rs`, `ui/src/store.rs`, `ui/src/store/setup_read.rs`; `ui/src/store/settings_snapshot.rs`; ADR
0002, ADR 0013, ADR 0014.
