# ADR 0008: An own look: neutral colours, white accent, round shapes, no empty areas

- Status: accepted
- Date: 2026-11-07
- Category: ui-ux

## Context

OOS is to be recognised as itself. The arrangement of a scope screen is given by the work (plot, channel cards, time
base and trigger at the edges), the look is not.

## Decision

- The arrangement of the screen stays as it is.
- Colours are neutral: near-black backgrounds and greys, with white as the accent (the selected entry, the frame of a
  dialog, a tile that is on).
- Shapes are round: buttons, dialogs and cards have rounded corners. (Square corners were tried and dropped.)
- No unused black areas, also not above or beside the cards; with every channel, the logic analyser and the generator
  on, no text of a card is cut off.

Ideas for functions may come from anywhere; a look or an arrangement is not copied from another product.

## Consequences

- Colours are set only through the constants at the top of `ui/src/ui.rs` (`BG`, `DLG_BG`, `ACCENT`, `BTN` ...); a
  colour of one page only is a named constant with a comment that says why (`BUS` in `ui/src/ui/bus.rs`), not a
  literal in the drawing code.
- A new page is checked at the three text sizes with everything switched on (`ui/tools/layout-check.sh`, ADR 0014).

## References

`ui/src/ui.rs` (colour constants, `btn`), `ui/tools/layout-check.sh`.
