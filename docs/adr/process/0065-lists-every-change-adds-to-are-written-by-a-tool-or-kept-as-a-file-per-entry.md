# ADR 0065: Lists that every change adds to are written by a tool or kept as a file per entry

- Status: proposed
- Date: 2026-11-18
- Category: process

## Context

Pull requests meet in a few files that are lists: each one adds a line, and two that add a line at the same place
conflict, although neither changed what the other did. In the last 100 commits of main (2026-11-17 to 2026-11-18,
4e29245d to ef5a71d3):

- 22 changed `docs/adr/README.md`, mostly to add one row to the index of the decision records;
- 20 changed `CHANGELOG.md`, mostly to add an item to `## Unreleased`;
- 28 changed `tools/layers.txt`, `tools/code-metrics.txt` or `tools/ratchet.txt`, numbers a tool writes
  (`--lower`).

AGENTS.md asks the development agent to keep no two of its pull requests on the same file. With these files that
rule makes almost every pair of pull requests wait for each other: ADR 0064 had to wait for the four records of pull
request #230, only because both add rows to the index. A person who works beside the agent meets the same conflicts
and resolves them by hand.

The index is also a second copy: 16 of its 57 rows had a text of their own instead of the record's first line, and
its statuses agreed with the records only because each was copied by hand. Nothing checked that a row and its record
agree.

## Decision

**A list that every change adds to is not edited by hand. The index of the decision records is written from the
records by `tools/adr-index.py`; the changes not yet released are one file each in `changelog.d/`, which
`tools/changelog.py --release` writes into `CHANGELOG.md` in the pull request of a release. A file that a tool writes
from other files does not count for the rule that two pull requests of the agent do not change the same file: after a
rebase, the tool is run again.**

- **The index** is between two marker lines in `docs/adr/README.md`. A row is the number and path of a record, its
  folder, its first line (`# ADR NNNN: the decision`) and its `- Status:` item without what stands in brackets. The
  layer `text` of `tools/check.sh` fails when the index is not what the records say (`--check`). A status changes in
  the record, and the tool writes the row.
- **A change a user notices** is a file `changelog.d/<short-name>.md`: its part (`Added:`, `Interface:`, `Removed:`,
  `Changed:`, `Fixed:`), an empty line, its items, as `changelog.d/README.md` shows. `tools/changelog.py` prints the
  next release's section, `--check` (layer `text`) checks the files, `--release X.Y.Z` writes them into
  `CHANGELOG.md` and removes them. The draft release of a tag (`image.yml`) fails while `changelog.d/` still holds a
  change, so a release cannot go out without its notes.
- **Written by a tool:** the index of `docs/adr/README.md`, `tools/layers.txt`, `tools/code-metrics.txt`,
  `tools/ratchet.txt`, `tools/scpi-coverage.txt` and `ui/Cargo.lock`. Two pull requests that both change one of them
  may be open at once; the second one rebases and runs the tool. A conflict anywhere else still means that one waits.
- `CHANGELOG.md` has no section `## Unreleased` between releases; `tools/changelog.py --release` puts the
  release's section above the newest one, and the pull request of the release writes its first lines by hand.

## Alternatives

- **A merge driver (`merge=union` in `.gitattributes`)** for the two lists. It keeps both sides' lines when git
  merges, but GitHub does not use it when it tells whether a pull request can be merged, and a squash merge of the
  agent runs there.
- **The index only on the wiki, not in the tree.** No file would change; but `docs/adr/README.md` is the first thing
  AGENTS.md tells a reader to open, on GitHub and in an editor, and it would lose the one line per decision.
- **A bot that writes the index on main after each merge.** Main takes changes only through pull requests (ADR 0031);
  a bot's pull request after every record is more waiting, not less.
- **Keep editing by hand and resolve the conflicts.** That is what costs the time now, and it lets an index row and
  its record drift apart.

## Consequences

- A pull request with a new record runs `tools/adr-index.py` and commits the result; one with a change a user notices
  adds a file to `changelog.d/` and leaves `CHANGELOG.md` alone.
- The index shows each record's own first line. Where a row had more text (16 rows), the record is the one to read;
  a record's text is not changed for the index (records are not rewritten, see the README).
- The pull request of a release has one more step (`tools/changelog.py --release X.Y.Z`), and `image.yml` stops a tag
  that forgot it.
- Between releases, `CHANGELOG.md` does not show what is coming; `tools/changelog.py` prints it.
- Worth a second look: when another file turns out to be a list everyone adds to (the module list of `ui/src/main.rs`,
  the table of `docs/scpi.md`); it can get a tool or a folder the same way.

## References

AGENTS.md (The development agent; Keeping the texts true), CONTRIBUTING.md, ADR 0019, ADR 0031, ADR 0035, ADR 0037,
ADR 0039, ADR 0055 (the part `Interface:`); `tools/adr-index.py`, `tools/changelog.py`, `changelog.d/README.md`,
`tools/check.sh` (layer `text`), `.github/workflows/image.yml`.
