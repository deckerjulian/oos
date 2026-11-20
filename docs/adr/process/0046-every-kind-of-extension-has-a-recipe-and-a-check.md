# ADR 0046: Every kind of extension has one registry, one recipe and one check that names what is missing

- Status: accepted (2026-11-18, by the maintainer)
- Date: 2026-11-18
- Category: process

## Context

OOS grows by seven kinds of addition: a setting, a measurement, a protocol decoder, a trigger type, an analyser page,
a remote command, a register sequence. ADR 0002 put three of them behind a registry and ADR 0043 made a setting one
row of a table. How to add one was prose: sections of CONTRIBUTING.md and the three documents on modules. Prose went
stale and nobody saw it. When each recipe was read against the tree (2026-11-18, on the host, main at 604a2771):

- `docs/protocol-modules.md` sent the author of a decoder to a test in `ui/src/ui.rs`; the test had moved to
  `ui/src/ui/tests.rs` when that file was split.
- The wiki's features and `README.md` counted 26 decoders; 29 are registered. C-PHY, Manchester and MLT-3 were in no
  list. The wiki's analysers lacked the Magnetic, Inverter and Double pulse pages.
- Two decoders with demo traffic (MIL-STD-1553, I2S) had no case in the test that shows the demos on the screen: "add
  it to the list" was a step of the recipe, and nothing checked it.
- Nothing pinned the stored identifiers that ADR 0002 says never change: a measurement's number, a protocol's key, a
  feature id, the words a setup stores for a trigger type and a view. A renumbered measurement passed every test.
- The recipe for a setting did not name the probes of the settings snapshot it needs. A trigger type and a register
  sequence had no recipe at all.

Two kinds did not go stale. `every_row_is_complete` (ADR 0043) fails, with the name of the row and of the part, when
a setting lacks its command or the reason it has none, its docs text or its query. `tools/scpi-coverage.py` does the
same for a remote command, and lists in `NOT_ASKED` the queries that cannot be asked, each with its reason. A recipe
is true while a check keeps it true.

The files a new item touches today, counted by hand on the tree, `CHANGELOG.md` left out (goal E3 of issue #120 asks
for two at most):

| Kind | By hand | Generated |
|---|---|---|
| setting of a group with a table | 8 to 9, and its wiki page | 2 |
| measurement in a module that exists | 1, 3 with new signal processing; a new module 1 more | 1 |
| protocol decoder | 8, 9 with a new file, 11 with a new family | 1 |
| trigger type | 13 to 15 | 0 |
| analyser page | 12 to 14 | 0 |
| remote command | 4 to 5 | 1 |
| register sequence | 4 to 6 | 0 |

## Decision

Every kind of extension has one registry where an item is declared, one recipe that names the registry, the other
files, the command for what is generated and the check, and one completeness test in the crate of the registry that
fails with the name of the item and of the part it lacks. A part an item cannot have is an entry with its reason in
a named list of that test; the list may only get shorter. A new kind of extension comes with all three.

## Alternatives

- **Recipes in prose, kept true by review.** That is what went stale: a reviewer reads the lines of a diff, not the
  line that is missing from the wiki.
- **One tool for all kinds, fed a description of each registry.** The kinds differ in what complete means: a number
  in a list, a word in a row of the docs, a picture of the comparison, a page of `docs/`. A language for that is
  longer than seven tests, and a test in the registry's crate can call the registry instead of parsing it.
- **Generate the texts instead of checking them**, as the settings table writes its docs table. Right where a text
  follows from the registry alone, and the way to fewer files per kind. The wiki's lists are written text with groups
  and remarks; until one is generated the check costs nothing, and it stays as the proof afterwards.
- **Derive first, check later**: a registry of trigger types, a demo that names its own time base, would remove
  parts instead of checking them. Each changes code that runs; it is a pull request of its own, and safer with the
  check in place.
- **A number that only goes down (ADR 0037) instead of named lists.** Right for hundreds of gaps, as the command
  words of `tools/scpi-coverage.py`. For a handful, a list with reasons says what a number cannot.

## Consequences

- The recipes are one section each under "Adding something" in `CONTRIBUTING.md`, a table in `AGENTS.md`, and the map
  "Where a change goes" in `docs/architecture.md`. `tools/link-check.py` checks every file they name and, new, every
  item named behind a file (`ui/src/settings/tests.rs::every_row_is_complete`), so a recipe cannot name a test that
  is gone.
- The checks: `every_measurement_is_complete`, `every_decoder_is_complete`, `every_decoder_family_has_its_tests`,
  `every_trigger_type_is_complete`, `every_page_is_complete`, `every_module_names_its_page_and_has_a_test`, next to
  `every_row_is_complete` and `tools/scpi-coverage.py`. They are tests and helpers of tests; the programs for the
  scope do not change.
- A stored identifier is pinned: measurement numbers and protocol keys by lists in `ui/tests/golden/` that only gain
  lines (`OOS_BLESS=1` adds the line of a new item and changes none), feature ids and the words of trigger types and
  views by tables in their tests. A changed or removed line shows in the pull request.
- The lists of exemptions are the debt in view, as of this record: ten decoders without demo traffic (`NO_DEMO`), the
  protocol trigger without a picture (`NOT_SHOT`), seven modules of `hal/src/dho/` whose header names no page of
  `docs/` and one that names a page that is not in the tree (`NO_PAGE`, `PAGE_NOT_THERE`), two without a test
  (`NO_TEST`). An entry that is no longer true fails its test. Nobody makes a list longer to get a test to pass; a
  new entry is a statement in a pull request, with its reason.
- A check reads a text by its form: the heading of a part of `wiki/Features.md`, a row of `docs/scpi.md`. Renaming
  one fails the test, which is then changed with the text. Names are matched as words, so a name that is a word of a
  longer one (CAN in CAN-FD) is found in that one too.
- Not checked yet: the wiki's row of a remote command, a test with known numbers for each measurement module, the
  pictures of the analyser pages and of most decoders in the comparison, that `tools/plan-matrix.sh` prints a new
  register sequence, and the `CHANGELOG.md` entry.
- This record does not lower the number of files per kind; it makes the number known and each of the files named. A
  trigger type and an analyser page are still written out in more than ten places.
- Worth a second look when a kind's texts are generated from its registry: its check then shrinks to what generation
  cannot give.

## References

Issue #120 (goals E3 and U2); ADR 0002, ADR 0037, ADR 0043; `CONTRIBUTING.md`, `AGENTS.md`, `docs/architecture.md`;
`ui/src/measures/mod.rs`, `ui/src/protocols/mod.rs`, `ui/src/model/trigger.rs`, `ui/src/ui/analysers.rs`,
`ui/src/ui/testkit.rs`, `ui/src/settings/tests.rs`, `dsp/tests/decode/main.rs`, `hal/tests/register_modules.rs`,
`tools/link-check.py`, `tools/scpi-coverage.py`; `ui/tests/golden/measurement-ids.txt`,
`ui/tests/golden/decoder-keys.txt`.
