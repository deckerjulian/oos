# ADR 0015: Every statement about the hardware says how it is known

- Status: accepted
- Date: 2026-11-07 (first public version; the rule from the start)
- Category: process

## Context

The documentation of OOS is the only description of this hardware that others can build on. Its statements come from
very different sources: seen on a running scope, concluded from behaviour, or never run at all. Read without that
difference, a guess looks like a fact, and a wrong fact costs the next person days.

## Decision

A statement about the hardware in `docs/` carries one of three marks: **verified on the unit** (seen on a running
scope; with the model and the date where it matters), **assumed**, or **not yet verified on hardware**. Measurements
name the unit, the set-up and the date. Changes to the hardware knowledge go into `docs/` in the same pull request as
the code that uses them.

The same holds for reports of work: what was tested on a scope, what only on the host, and what was not tested.

## Consequences

- A reader can tell what to rely on and what to check first on another model.
- "Verified" needs a unit: much stays "assumed" until someone with that hardware reports, and a report from a model
  nobody tested is among the most useful contributions.
- A function built on something not yet verified is off by default (ADR 0010).

## References

[docs/README.md](../../README.md), [CONTRIBUTING.md](../../../CONTRIBUTING.md), [models.md](../../models.md).
