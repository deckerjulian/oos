# ADR 0063: The status boxes of the top bar come from one list, right of the four entries

- Status: accepted (2026-11-19, by the maintainer)
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

- **The boxes are a list**, `STATUS_BOXES` in `ui/src/ui/bars.rs`, in the order they stand from the right edge of
  the bar, as they do today: LAN, then WLAN, BT and USB to its left. An entry says when the box is there, its label,
  its width, whether it is a small box of one line or one of two lines, how it draws its dot and text, and the page
  it opens, whose open menu also lights it (ADR 0060). Placing, drawing, hit
  testing and lighting are one function each for every box.
- **They are status, not entries.** They do not count against the four entries of ADR 0007. A box shows the state of
  something plugged in or connected and opens its page; a function that is not a state gets no box (ADR 0007 still
  holds: it starts as a row of its menu).
- **At most five boxes.** The bar's room on the smallest screen decides; a sixth needs a new record.
- **The check.** The layout lint places every mix of the boxes that come and go at every screen size and text size,
  and checks that a box of two lines has room for an address. It switches each such box on by the state its `shown`
  reads, one line each; a new box without its line fails the lint's count. A test fails on a box whose page is not in
  `PAGES`.

## Alternatives

- **One box for all connections** (a single "Network" box with a menu of states). It hides at a glance what the
  separate boxes show, a stick that is missing or a device that is not connected.
- **Leaving each box its own code.** Each new one costs the edits of #216 again, and the hand-written combinations in
  the lint grow as powers of two.

## Consequences

- A new box is one entry; its place, its hit area and its lit state follow.
- The lint covers 2^n mixes by itself; with five boxes, 32.
- The `AGENTS.md` rule "the top bar keeps its four entries" is read as ADR 0007's buttons; the boxes are this record.
- Worth a second look if a box needs a shape the list cannot give (a value that changes every frame).

## References

`ui/src/ui/bars.rs`, `ui/src/ui/input.rs`, `ui/src/ui/lint/screen_layout.rs`, ADR 0007, ADR 0008, ADR 0060, pull
request #216.
