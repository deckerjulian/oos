# ADR 0044: The settings table holds the real numbers of the analyser pages, and a row may step in its own way

- Status: accepted
- Date: 2026-11-18
- Category: code

Amends ADR 0043.

## Context

The settings of the analyser pages (the Bode sweep, the spectrum, the eye and the power analysis) could not become
rows of the table of ADR 0043. Most of their numbers are f64 fields, kept in a setup line of several values in Rust's
shortest exact form of an f64 and read back with `str::parse`, which also takes `inf` and `NaN`. The table knew only
`Val::F32`: a row over such a field would round what a setup stores. Their menu steps follow no rule of a type: a
1-2-5 ladder of the page's own (from 0, or from its top when the value is on none), a clamp after the step
(`max(1.0)`), a clamp against another field (the start of a Bode sweep stays at or below half its stop), or a choice of
two that changes on an odd step only. A few are f32 fields with steps of their own (the sync level in tenths of a
volt, the reference level in steps of 5 dB).

## Decision

The table gets a value kind `Val::F64` and a type `Ty::Real` for a real number as its field keeps it: an f64 field
stored in Rust's shortest exact form and read as `str::parse` reads it (`store::p_f64`, which the setup lines of the
analyser pages use too), an f32 field as `Val::F32`, both within an optional range; the menu shows it by a function of
the row. A row gets a field `step`: the menu's step from the model, for a row that does not step as its type does. The
rows that were there take `None` and step as before.

## Alternatives

- **`Val::F32` for the analyser fields.** Changes what a setup stores (`1e9` is not an f32 number) and what the pages
  compute with.
- **A step enum (linear, ladder, clamped) instead of a function.** The steps found differ in their ladder, the index
  for a value on none, a clamp before or after the step and the field they clamp against; an enum with all of these
  is longer than the few lines of each function and still needs a case for a clamp against another field.
- **The step and the shown value in the type, as `Seconds` has its ladder.** A clamp against another field needs the
  model, and choices and switches need the odd step too; a field of the row serves every type.

## Consequences

- A setting of an analyser page is a row: its field of a setup line, its menu row and its line in `docs/scpi.md`,
  with no command where it has none. Its setup line is still written and read by hand (`store.rs`, as the `la` and
  meter lines), with `store::p_f64` for its numbers.
- A menu value that reads more than the field (the last analysis, a flag beside the value) or a step that needs the
  screen (a list of the waves shown) stays by hand; so do optional values (`-` for none).
- `Ty::Real` rows have a `step`; a test fails otherwise.
- Over SCPI a `Ty::Real` row takes a number and answers it as `1.000000E+03`; none of the analyser rows has a command
  yet.

## References

Issue #120 (goal E1, proposal of round 7); ADR 0043; `ui/src/settings.rs`, `ui/src/settings/bode.rs`,
`ui/src/settings/spectrum.rs`, `ui/src/settings/eye.rs`, `ui/src/settings/power.rs`, `ui/src/ui/setting_items.rs`,
`ui/src/store/setup_read.rs`.
