# ADR 0030: OOS is an independent reimplementation: the tree holds black-box descriptions and code written to them

- Status: accepted (2026-11-17, by the maintainer; in force since that day, and an objection is an issue and can
  still change it)
- Date: 2026-11-17
- Category: process

## Context

OOS replaces the software of a closed product. Everything it does with the hardware has to be known first: which
register starts an acquisition, how the front ends are set, what the generator expects. ADR 0013 keeps vendor code and
data out of the tree, ADR 0026 keeps vendor material out of the texts around it, and ADR 0015 asks every statement
about the hardware to say how it is known. None of them says what a description of the hardware in the tree may
consist of, how it relates to the code, and what a reviewer can check.

ADR 0026 also keeps "the names of its functions, classes and files" out, meaning the stock firmware's. The documents
name things that anyone sees on a running unit: the kernel modules the stock system has loaded, its device nodes, the
programs and scripts its start-up runs. A description of the start-up cannot do without them, and they are
observations, not material taken out of the firmware.

## Decision

OOS is an independent reimplementation. This record is a rule for what the tree holds and for what enters it:

- **Descriptions are black-box.** [docs/](../../README.md) describes the scope by what goes in and what comes out at
  its interfaces: the values a register takes and returns, the words on the buses to the front ends, ADC, PLL and
  panel as they appear on a running unit, the signals at the inputs and outputs, the answers of the stock SCPI, and
  what the vendor publishes for users (the programming guide, the manuals, the data sheets of the parts). A
  description is behaviour, in the author's own words, with how it is known (ADR 0015).
- **The code is written to the descriptions.** The tree holds only such descriptions and code written to them. A
  hardware fact the code needs and `docs/` lacks goes into `docs/` first, in the same pull request.
- **What a running unit shows may be named.** Names that are observed on a running unit count as behaviour: loaded
  kernel modules, device nodes, running programs and the scripts of the start-up, the properties of the device tree
  it exposes. This limits the list of ADR 0026: names from inside the firmware's programs (functions, classes, source
  files, symbols) stay out, as do code, listings, copied tables and strings.
- Since ADR 0026 (2026-11-16) the same holds for every text around the code: commit messages, pull request
  descriptions and issues.

A reviewer can check this: every hardware fact a pull request uses is in `docs/` as behaviour, with its mark, and the
code does what that text says.

## Alternatives

- **Patching or porting the stock software.** Fast, but the result could not be published under GPL-2.0-or-later
  and would tie OOS to one firmware version.
- **Code without a description.** Fewer files, but the knowledge would sit in register constants that nobody can review
  against anything, and a later contributor could not tell a measured value from a guess.
- **Calling it a clean-room rebuild.** That term stands for a process with separate people for analysis and for
  implementation. This project does not have that separation and does not claim it; the rule above is what can be
  checked in the tree.

## Consequences

- `docs/protocols/`, `docs/models.md` and the hardware pages are the contract between what is known and what is built.
  They grow before the code does, and a pull request that changes a register sequence changes its page too.
- Someone without a unit can implement from `docs/` alone, and someone with a unit can check `docs/` without reading
  the code.
- What is not described cannot be built. A function that is not understood yet stays out, or comes behind its switch as
  not yet verified (ADR 0010).
- The rule holds for the tree as it is and for what enters it. The history of main before 2026-11-16 was not written
  under ADR 0026 and is not rewritten.
- When this record is accepted, ADR 0026 gets the status "accepted, amended by ADR 0030" for the names a running unit
  shows.

## References

ADR 0013, ADR 0015, ADR 0026, [docs/README.md](../../README.md), [CONTRIBUTING.md](../../../CONTRIBUTING.md),
[AGENTS.md](../../../AGENTS.md).
