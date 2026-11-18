# ADR 0002: Decoders, measurements and analyser pages are modules behind one registration

- Status: accepted, amended by ADR 0058 (a protocol decoder is described and run in oos-dsp, shown in oos-ui)
- Date: 2026-11-11 (protocol decoders), extended 2026-11-15 (measurements, analyser pages)
- Category: code

## Context

A new decoder, measurement or analyser page used to touch many places: menus, labels, setups, SCPI, the start menu,
System > Features, several `match` lists. Each addition could forget one of them, and nothing checked all of them the
same way.

## Decision

Each of the three kinds is added as a type that implements one trait, plus one line that registers it:

| Kind | Trait | Registered in |
|---|---|---|
| protocol decoder | `protocols::Protocol` | `ui/src/protocols` |
| measurements | `measures::MeasModule` | `ui/src/measures/mod.rs` (`MODULES`) |
| analyser page | `ui::analysers::Analyser` | `ui/src/ui/analysers.rs` (`ANALYSERS`) |

The scope asks the registry for everything around an entry (menu rows, tiles, names, setups, SCPI, Features, drawing).
The tests walk the registry and check every entry the same way.

Identifiers that are stored are stable: a protocol's key, a measurement's number and an analyser's feature id never
change once released and are never given to something else, because setups, SCPI and `system.conf` hold them.

## Alternatives

- Stand-alone analyser pages, like protocol and measurement modules: not done. The pages keep their menu rows as
  `Item`s and their state in `Ui`, as every other menu does; the registry takes only the wiring around a page.
- Loading modules at run time: not looked at further. OOS is one static binary (ADR 0013).

## Consequences

- An addition is one file and one line, and the generic tests cover it without being told.
- The move itself changed nothing visible: the 83 analysis measurements give the same values bit for bit, the pages
  the same pixels (ADR 0014).
- The 60 standard measurements and the 17 two-source variants are still computed by the scope itself; their modules
  only place them on the page. A new measurement comes as a module.

## References

[protocol-modules.md](../../protocol-modules.md), [measurement-modules.md](../../measurement-modules.md),
[analyser-modules.md](../../analyser-modules.md).
