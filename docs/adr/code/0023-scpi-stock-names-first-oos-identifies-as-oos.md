# ADR 0023: SCPI follows the stock command names where they exist; OOS identifies itself as OOS

- Status: accepted
- Date: 2026-11-15
- Category: code

## Context

People who switch to OOS have scripts for the stock firmware: `:CHANnel1:SCALe`, `:WAVeform:DATA?`, `:TRIGger:EDGE:*`,
VISA programs that read `*IDN?`. OOS also does things the stock firmware does not (analyser views, references as
sources, SIM, self-calibration, Bode with an external generator) and needs commands for them. The stock syntax is
known from Rigol's public DHO800/DHO900 Programming Guide, not from the firmware; ADR 0013 keeps vendor code out.

Where the guide leaves a detail open, OOS has to pick one: `:WAVeform:FORMat WORD` sends two bytes per point, and the
guide does not say in which order.

## Decision

- A function the stock firmware has is reached by the stock command name and parameters, as the Programming Guide gives
  them. Long and short forms both work; intermediate abbreviations do not.
- A function only OOS has gets a command under its own keyword (`:VIEW`, `:SIM:*`, `:IDENtify?`, `:CALibrate:*`,
  `:ACQuire:CORes`) or under the stock subsystem it extends; [scpi.md](../../scpi.md) marks it as an OOS extension.
- `*IDN?` answers `OOS,<model>,<serial>,oos-ui <version>`. OOS does not claim to be the stock firmware or Rigol, and
  speaks no other vendor's dialect.
- Where the guide is silent, OOS fixes the behaviour once and writes it down as OOS's wire policy (WORD: unsigned
  16-bit little-endian). A command name or a wire format that was released does not change.
- [scpi.md](../../scpi.md) is the reference: a command that is not in it is not promised.

## Alternatives

- **The full stock command tree:** hundreds of commands for functions OOS does not have; answering them would lie.
- **An own command tree only:** every existing script would have to be rewritten.
- **`*IDN?` as the stock firmware answers it:** some programs would take OOS for the stock firmware and rely on
  behaviour it does not have; it would also present OOS as Rigol's product.

## Consequences

- A stock script runs as far as its commands are in [scpi.md](../../scpi.md); the others answer with an error in
  `:SYSTem:ERRor?`, never with silence.
- A program that checks `*IDN?` for `RIGOL` has to be told about OOS.
- Each new stock command needs the guide's section next to it in the reference; each extension needs a name that does
  not collide with a stock one.
- Tests against the interpreter show what OOS does; they do not show that a VISA driver or a stock script behaves the
  same on the stock firmware ([local-validation.md](../../local-validation.md)).

## References

`ui/src/scpi.rs`, [scpi.md](../../scpi.md), ADR 0013, ADR 0022, the
[DHO800/DHO900 Programming Guide](https://download.rigol.com/en/Manual/Digital%20Oscilloscope/DHO800/DHO800900_ProgrammingGuide_EN.pdf).
