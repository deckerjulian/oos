# ADR 0058: A protocol decoder is described and run in oos-dsp and shown in oos-ui

- Status: accepted (2026-11-18, by the maintainer)
- Date: 2026-11-18
- Category: code

Amends ADR 0002.

## Context

ADR 0002 registers a protocol decoder once, as a `protocols::Protocol` in `ui/src/protocols`. Milestone M4 (#187)
moved what a decoder is and does out of oos-ui, in steps (#207, #211, #213, #215): its key, name, family and lines,
the rule and start value of the protocol trigger, its bit time and record length, and the call of the decoder with the
option rows and shared rows of the Decode menu. They stand next to the decoder in the file of its family, where the
tests of oos-dsp reach them without a screen. oos-ui keeps what the scope shows of a decoder: its menu rows, labels and
event table, the hardware start edge, the first settings and old setups, demo traffic and the view. A decoder has two
registrations now, one in each crate, and ADR 0002 names only one.

## Decision

A protocol decoder is described and run in oos-dsp: a `ModuleInfo` in the file of its family (its key, name, lines,
trigger rule, bit time and decoder call), listed in `oos_dsp::protocols::MODULES`. It is shown in oos-ui: a `Protocol`
in `ui/src/protocols` that names that `ModuleInfo` and brings only the presentation, registered in the same order.

## Alternatives

- **Everything in oos-ui, as ADR 0002 has it.** The trigger rules, the bit times and the mapping of the menu settings
  onto a decoder's arguments are signal processing, and stayed out of reach of the tests of oos-dsp; the tables an
  option chooses from (the CAN FD data rates, the Modbus parities) stood in oos-ui, away from the decoder that reads
  them.
- **Everything in oos-dsp, the presentation too.** Menu rows, labels, demo traffic and views are the screen's; oos-dsp
  stays free of them (ADR 0001).
- **One registry, in oos-ui only, that holds the `ModuleInfo`s.** oos-dsp could then not check that every decoder it
  has is offered, nor walk its decoders in a test.

## Consequences

- The stored identifiers do not change: a protocol's key is the `key` of its `ModuleInfo`, and the order of
  `oos_dsp::protocols::MODULES` is the order of `MODULES` in `ui/src/protocols/mod.rs` that setups and SCPI count
  by. A new decoder goes to the end of both.
- What holds the two registries together: `ui/src/protocols/mod.rs::every_decoder_is_complete` fails on a ui module
  whose `ModuleInfo` is not in `oos_dsp::protocols::MODULES` at the same place and, in the whole build, on a dsp
  decoder without a ui module; in oos-dsp, `dsp/src/protocols.rs::every_declared_module_is_registered`,
  `modules_are_well_formed` (keys, names, the file of its family), `every_module_has_its_decoder_and_bit_time` and
  `dsp/tests/decode/modules.rs` (a module hands its option rows and shared rows to its decoder).
- Adding a decoder touches both crates; the recipe ([protocol-modules.md](../../protocol-modules.md)) and the row in
  `AGENTS.md` name both places (ADR 0046).
- Measurements and analyser pages stay as ADR 0002 has them.

## References

ADR 0001, ADR 0002, ADR 0046; issue #187; pull requests #207, #211, #213, #215; `dsp/src/protocols.rs`,
`ui/src/protocols/mod.rs`, `dsp/tests/decode/modules.rs`, [protocol-modules.md](../../protocol-modules.md).
