# ADR 0031: Every change enters main through a pull request, the maintainer's too; decision records are decided there

- Status: accepted (2026-11-17, by the maintainer; in force since that day, and an objection is an issue and can
  still change it)
- Date: 2026-11-17
- Category: process

## Context

ADR 0020 gives one check command that CI runs, and ADR 0026 lets a pull request from a fork into main only as reviewed,
squashed commits. Both were followed for contributions, not for the maintainer's own work, which went to main
directly:

- On 2026-11-16 and 17, 134 commits went to main by direct push, among them a consolidation of 51 commits in one
  push. That push (2026-11-17 07:33 UTC) left CI and the image build red until a later commit fixed them. The last two
  of the 134 are this record and ADR 0030: they were pushed as proposals, so the rule could be read on main before it
  starts.
- Pull requests from forks that were open at the same time had to be rebased again and again; one contributor reworked
  a set of them three times in a day, then closed them to continue in a fork.
- Decision records were written and accepted in the same direct commit (ADR 0022 to 0025, ADR 0029), and accepted
  records were corrected in place (ADR 0010, 0014, 0020). The README of `docs/adr` allows such corrections, but nobody
  outside saw them before they were in force. The status `proposed` existed and was not used.

The rules were right; they did not hold for everyone. People who contribute cannot plan against a main that moves
without notice.

## Decision

- **main is protected.** Nobody pushes to it, the maintainer included. Every change comes as a pull request, is
  merged squashed (ADR 0026) and only when CI is green: `tools/check.sh` and the sign-off check.
- **The maintainer's pull requests wait like everyone's.** A pull request that changes behaviour, a rule or many files
  stays open at least 24 hours before it is merged, so others see what is coming. A fix for a red main, a security
  fix and a pure documentation correction may be merged as soon as CI is green.
- **Decision records are decided in pull requests.** A new record, or one that supersedes another, comes in with the
  status `proposed`, in a pull request of its own or with the change it belongs to, and stays open at least 72 hours.
  It is merged as `accepted` when no objection is left open, or as `rejected`. The corrections that the README of
  `docs/adr` allows in place go through a pull request too, with their date.
- **Open pull requests come first.** When a change of the maintainer and an open pull request touch the same code, the
  one opened first is merged first, and the later one is rebased on it. Where the contributor allows edits by the
  maintainer, the maintainer rebases the contributor's pull request instead of asking for another round.
- **Large changes are announced.** A change across many files that does not change behaviour (formatting with
  rustfmt, ADR 0016; moves; a consolidation) is announced in an issue with the day it will be merged, and is merged
  when the open pull requests are in or their authors agree.

## Alternatives

- **Keep direct pushes for the maintainer, ask contributors to keep up.** What happened: it works for one person and
  stops working the moment two people change the same files.
- **A second maintainer who must approve every pull request.** Better review, but there is no second maintainer yet.
  Required reviews can be switched on later without a new record.
- **Merge windows (contributions on certain days only).** Fewer rebases, but slower for small fixes; the waiting times
  above give the same notice with less ceremony.

## Consequences

- The history of main stays a line of reviewed, green commits, and a contributor rebases only when a pull request
  that was opened earlier is merged.
- The maintainer's work slows down by the waiting times, and needs branches and pull requests like everyone's.
- The repository settings carry the rule: branch protection on main with the CI check `test` required, squash merge
  only, branches deleted after the merge. Until they are set, this record is followed by hand.
- Coding agents open a branch and a pull request; they never push to main ([AGENTS.md](../../../AGENTS.md)).

## References

ADR 0019, ADR 0020, ADR 0026, [docs/adr/README.md](../README.md), [CONTRIBUTING.md](../../../CONTRIBUTING.md),
`.github/workflows/ci.yml`.
