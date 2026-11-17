# ADR 0027: a session is one file of the setup, the record and the references

- Status: accepted, to be reviewed
- Date: 2026-11-17
- Category: code

## Context

A setup file holds the settings and how the reference slots look, but no samples. To come back to a measurement
later, or to hand it to someone else, the setup, the waveform (CSV or binary) and each reference had to be saved and
loaded one by one, and a recalled setup only restored the look of a reference that was loaded again by hand first.
The parts were all there: the setup text (`setup_text_full`), the binary record (`wave_bin`, "OOSW"), the reference
slots and the stopped scope that shows a record. What was missing is one file that holds them together. Host-tested
only; not yet verified on hardware.

## Decision

A session is one file, `session-NNN.oss`: the tag "OOSS" and a version, the setup as the same text a setup file has,
the record on the screen as an OOSW blob when there is one, and every loaded reference slot with its samples. Loading
it stops the scope, applies the setup as Recall setup does, puts the record on the screen and replaces the reference
slots by those of the file. The exact layout is in the comment of `store::session_bin`.

## Alternatives

- A folder with a setup file, a waveform file and one file per reference. Nothing new to parse, but a session that
  loses one of its files loads half, and it cannot be sent as one thing.
- An archive format (zip, tar). It needs a dependency on the scope or an own reader, for no gain over three
  length-prefixed parts.
- Adding to what is loaded, leaving the other reference slots as they are. A session is the whole bench as it was
  saved: a reference left over from before would be measured and decoded beside it without being part of it.

## Consequences

- The file is as large as its samples: 4 bytes a point for every shown channel and every reference.
- The setup inside is the setup text, so it follows that format and its keys without a second copy of the rules. A
  session of a newer build with keys this build does not know loads as a setup file with those keys would.
- The version is checked on load; a change of the layout raises it, and a reader says which version it met.
- Every length in the file is checked against the bytes that are there and against the import limits before memory
  is taken for it.
- Not in a session: the fast-frame history, statistics, the picture on the screen, and anything of the network or
  the system. Math, measurements and decoders are settings and come back with the setup, computed again from the
  record and the references.
- Worth a second look when the logic analyser lines or the history should travel with it.

## References

`ui/src/store.rs` (`session_bin`, `parse_session`), `ui/src/ui.rs` (`session_save`, `session_load`),
`docs/scpi/storage.md` (`:SAVE:SESSion`, `:LOAD:SESSion`).
