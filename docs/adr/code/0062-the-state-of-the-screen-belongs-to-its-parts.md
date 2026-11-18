# ADR 0062: The state of the screen belongs to its parts; the fields of `Ui` itself only get fewer

- Status: proposed
- Date: 2026-11-18
- Category: code

## Context

`struct Ui` in `ui/src/ui.rs` has 224 fields. 16 of them are the state of one part in a struct of its own (`bt:
bt::BtUi`, `wlan: wlan::WlanUi`, `upd: update::UpdateUi`, ...); the other 208 are loose: flags, caches, positions and
handles of many parts side by side, such as `bt_stick` and `bt_inputs` next to `bt`. ADR 0049 names the result: 82 of
the 88 files below `ui/src/ui/` hold a part of `impl Ui`, so view and control cannot be told apart by their paths,
and the screen stays one layer.

A loose field can be read and written by every file of the screen. Who owns it is found only by searching, a test of
one part has to build a whole `Ui`, and every new part adds fields to `ui.rs` and their start values to `init.rs`,
two more shared files a feature pull request touches (ADR 0060).

## Decision

- **A part keeps its state in a struct of its own**, defined in its module, with its start values in its `Default`
  or `new`, and `Ui` holds one field of that type. Fields only that part reads or writes move into it; a field two
  parts use stays in `Ui` or moves to the part that owns it, with a method for the other.
- **A new part adds one field to `Ui`**, its state struct, and none of its loose values.
- **A number that only goes down** (ADR 0037): `tools/code-metrics.py` counts the fields of `Ui` that are not a
  part's state struct (`ui-fields` in `tools/code-metrics.txt`, 208 when this record was written). The layer
  `ratchet` fails when it rises.
- **The move** goes part by part, beside the move of the pages (ADR 0060): a pull request that moves a page into
  `PAGES` moves its loose fields into its state too, as one refactoring that keeps every pixel (ADR 0014).
- **What stays in `Ui`**: what the whole screen shares, the model `m`, the source, the screen size, the open menu and
  the focus. It is named in a list at the top of `struct Ui`, with the reason for each group.

## Alternatives

- **Splitting `Ui` into a few big structs** (view, control, services). It moves the fields without giving them an
  owner; the parts would still reach into each other's groups.
- **Getters and setters on `Ui`** for the loose fields. More code and the same sharing.
- **Leaving it.** The number has only grown; nothing in a review stops a new loose field.

## Consequences

- A part can be read, and tested, with its own state; its tests need less of `Ui`.
- `ui.rs` and `init.rs` stop growing with each feature.
- The path to splitting view and control (ADR 0049) opens once the state has owners: a module whose functions only
  take its own state is no longer bound to `impl Ui`.
- Borrowing gets stricter: a method that needs two parts takes them separately, or is a method of `Ui` that lends
  them out. That is the point, and in a few places it will cost a line or two.

## References

`ui/src/ui.rs` (`struct Ui`), `ui/src/ui/init.rs`, `tools/code-metrics.py`, ADR 0014, ADR 0037, ADR 0049, ADR 0060.
