# ADR 0063: The status boxes of the top bar come from one list, right of the four entries

- Status: proposed
- Date: 2026-11-18
- Category: ui-ux

Amends ADR 0007, which is about the buttons of the top bar and says nothing of the boxes beside them.

## Context

ADR 0007 keeps the top bar to four entries, Measure, Cursor, Display and Storage, and a Live button in record mode.
Right of them the bar has grown boxes that show a state and open its page on a tap: LAN always, WLAN while a WLAN
stick is there, BT while a Bluetooth stick is there (#216), USB while a USB stick is there. Each came in with its own
function in `ui/src/ui/bars.rs` (`net_box`, `wlan_box`, `bt_box`, `usb_box`), its own position code that knows which of
the others are there, its own hit test in `input.rs`, its own lit state (#216 asked the wrong menu for it) and its
own combinations in the layout lint (`screen_layout.rs` lists the eight mixes of BT, WLAN and USB by hand). A fifth box
would touch all of them again and double the combinations.

## Decision

- **The boxes are a list**, `STATUS_BOXES` in `ui/src/ui/bars.rs`, in the order they stand from the left: LAN, WLAN,
  BT, USB. An entry says when the box is there, its label, its dot (off, on, warning), the page it opens and the menu
  root that lights it (ADR 0060). Placing, drawing, hit testing and lighting are one function each for every box.
- **They are status, not entries.** They do not count against the four entries of ADR 0007. A box shows the state of
  something plugged in or connected and opens its page; a function that is not a state gets no box (ADR 0007 still
  holds: it starts as a row of its menu).
- **At most five boxes.** The bar's room on the smallest screen decides; a sixth needs a new record.
- **The check.** The layout lint places every subset of `STATUS_BOXES` at every screen size and text size, from the
  list, so a new box is covered without a line in the lint. A test fails on a box whose page is not in `PAGES`.

## Alternatives

- **One box for all connections** (a single "Network" box with a menu of states). It hides at a glance what the
  separate boxes show, a stick that is missing or a device that is not connected.
- **Leaving each box its own code.** Each new one costs the edits of #216 again, and the hand-written combinations in
  the lint grow as powers of two.

## Consequences

- A new box is one entry; its place, its hit area and its lit state follow.
- The lint covers 2^n mixes by itself; with five boxes, 32.
- The `AGENTS.md` rule "the top bar keeps its four entries" is read as ADR 0007's buttons; the boxes are this record.
- Worth a second look if a box needs a shape the list cannot give (two lines, a value that changes every frame).

## References

`ui/src/ui/bars.rs`, `ui/src/ui/input.rs`, `ui/src/ui/lint/screen_layout.rs`, ADR 0007, ADR 0008, ADR 0060, pull
request #216.
