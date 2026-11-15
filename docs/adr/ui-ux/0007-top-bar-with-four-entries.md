# ADR 0007: The top bar holds four entries; everything else starts from the start menu

- Status: accepted
- Date: 2026-11-14
- Category: ui-ux

## Context

The top bar had grown: next to Measure, Cursor, Display and Storage it carried Load file, SIM and System. All three were
also reachable elsewhere or belonged to one menu only, and the bar is the scarcest place on a 1024 x 600 panel.

## Decision

The top bar holds Measure, Cursor, Display and Storage. Everything else starts from the start menu (the Menu button,
bottom left): System is the first entry of its group there, SIM is under Analysis, and Load file is a row of the
Reference menu, where the loaded file is used.

A function gets a place in the top bar only when it is used all the time while measuring; a function that belongs to
one menu is a row of that menu.

While the screen shows a recording instead of the acquisition (record mode: offline waveforms, a taken-over record,
SIM), a Live button stands before the four; it is the way back to the acquisition and goes away with it. (Added
2026-11-15: the button was there before this record, `Ui::top_buttons`, and the record had missed it.)

## Consequences

- The bar keeps its room for what it shows all the time.
- System and SIM are one tap further away than before.
- A new feature starts as an entry of the start menu or a row of its menu, not as a button in the bar.

## References

`ui/src/ui.rs` (`main_groups`, `top_buttons`), CHANGELOG ("Top bar").
