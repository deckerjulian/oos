# ADR 0006: Lists scroll by a scroll bar and by the finger, not by buttons

- Status: accepted
- Date: 2026-11-15
- Category: ui-ux

## Context

Long lists (the measurement page, the Results pane, the event, search and segment tables) had Up and Down buttons. On
the measurement page they failed: after scrolling down, folding a group made the list shorter than its window, the
buttons disappeared, and the list stayed scrolled past its end with no way back. Buttons also say nothing about how
long a list is or where one is in it, and on a touch screen a list is expected to follow the finger.

## Decision

Every list that can be longer than its window has a scroll bar beside it and follows a finger drawn over it. The thumb
can be dragged; a press on the track jumps there. A press that does not move (less than 10 px along the list) is still a
tap on what is under it. There are no Up / Down buttons.

A list's position is limited to its valid range whenever it is read, not only when it is changed.

## Consequences

- One implementation for all lists (`ui/src/ui/scroll.rs`: `Bar`, `Grip`, `Drag::Scroll`); a new list gives its bar
  (track, total, shown, position) and gets the bar, the finger and the tap rule.
- A list cannot stand past its end any more, whatever shortens it.
- The bar takes 40 px of touch width beside the list (10 px drawn); the thumb is at least 40 px high.
- The mouse wheel of the desktop build does not scroll the lists yet.

## References

`ui/src/ui/scroll.rs` (with the tests of the reported case), `ui/src/ui.rs` (`touch_in`).
