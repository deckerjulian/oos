# ADR 0005: One measurement page; one tap adds or removes

- Status: accepted
- Date: 2026-11-15
- Category: ui-ux

## Context

Measurements were reached in several places: a catalog page for the analysis measurements, a separate "All 60 / Pairs"
page with older short names for the standard ones, and Analysis > Measure in the start menu. Adding one took two steps
(pick the tile, then Add). With 160 measurements the page was hard to read.

## Decision

There is one page for every measurement. The categories stand down the left (Basic, Two sources, Serial & PAM, Jitter &
eye, Power, Motor drive, Power rails, Double pulse, and under them Selected); right of them the sources and the groups
of the category, one open at a time; at the bottom the last tile tapped, with what it measures.

One tap on a tile adds the measurement, a second removes it; a tile that is on is filled. The Measure key, the Measure
button and Analysis > Measure in the start menu all open this page. The settings are behind Settings on the page, in
named groups (Sources, Measure over, Display, Parameters, More).

## Consequences

- Every measurement has one place and one full name (Peak-to-Peak, Upper Level ...), also the 60 standard ones.
- A tap acts at once, so a tile tapped by mistake is on; the filled tile and the second tap undo it. Selected lists
  what is on, with Remove, Set and Clear all.
- Where a tile sits comes from its module (`Def::places`, ADR 0002): a new measurement names its category and group
  and needs no change of the page.
- The page covers the plot while it is open.

## References

`ui/src/ui/meascatalog.rs`, `ui/src/measures/mod.rs` (`CATEGORIES`),
[measurement-modules.md](../../measurement-modules.md), [reference-analysis.md](../../reference-analysis.md).
