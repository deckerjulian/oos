---
applyTo: "**"
excludeAgent: "cloud-agent"
---

# Reviewing a pull request to OOS

OOS runs on an oscilloscope: a wrong register write is only seen on a unit, and a unit is not always there. The rules
are in [AGENTS.md](../../AGENTS.md) and the decision records in [docs/adr](../../docs/adr/README.md). A review checks
the change against them. It is advice: the maintainer decides, and a comment that is a guess says so.

## Comment on

- **Danger to a unit.** Code that can write the FPGA flash, `/rigol/data`, or the `uboot`, `trust` and `parameter`
  partitions (ADR 0009). A hardware function that is switched on by default without being verified on a unit
  (ADR 0010). A register value or the order of register writes in `hal/` that changes without the pull request saying
  so.
- **Material that must not be in the tree or in a text.** Code, tables, strings or names from the stock or a modified
  firmware; calibration files; serial numbers, MAC addresses, licence keys, the address of a scope. This holds for
  commit messages and the pull request text as well (ADR 0013, ADR 0026, ADR 0030).
- **Hardware facts.** A statement about the hardware without "verified on the unit", "assumed" or "not yet verified
  on hardware" (ADR 0015). A fact the code relies on that `docs/` does not describe (ADR 0030).
- **A refactoring that is more than one.** A change described as leaving behaviour as it was (ADR 0014) that changes
  what the comparison of pictures and register sequences does not see: timing, replies to remote commands, the format
  of stored files, stored identifiers (protocol keys, measurement numbers, feature ids, ADR 0002).
- **Structure.** More than one topic in a pull request. A remote command outside `ui/src/scpi.rs` (ADR 0022). Hardware
  access outside `oos-hal`, or hardware in `oos-dsp` (ADR 0001). A new dependency of what runs on the scope
  (ADR 0013, ADR 0021). A decision record that is edited where it should be superseded, or a new one that is not
  `proposed` (ADR 0031).
- **What is missing.** Documentation that the change makes wrong. A `CHANGELOG.md` entry for a change a user sees. A
  test, where the change can be tested on the host. A test that depends on how fast the computer is (ADR 0020).
- **Plain errors**: wrong arithmetic, a wrong unit, an index out of range, a lock held across a blocking call, an
  error that is dropped.

## Do not comment on

- The layout of Rust code: rustfmt is not rolled out yet, and a pull request keeps the layout of the file it edits
  (ADR 0016).
- Taste: names, the order of functions, another way to write the same thing.
- Code the pull request does not touch.
