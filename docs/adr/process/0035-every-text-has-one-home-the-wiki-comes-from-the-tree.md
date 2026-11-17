# ADR 0035: Every text has one home; the wiki is published from the tree, and a change brings its texts along

- Status: accepted (2026-11-17, by the maintainer, with the pipeline as a whole and without the waiting time of
  ADR 0031; an objection is an issue and can still change it)
- Date: 2026-11-17
- Category: process

## Context

OOS is described in four places: `README.md` (869 lines), `docs/`, the wiki of the repository (13 pages) and
`CHANGELOG.md`. They overlap: installing, the first steps and the remote commands are written both in the README and
in the wiki, the commands a third time in `docs/scpi.md`. Three copies do not stay the same.

The wiki is a repository of its own. A pull request cannot change it, so a change of the code never brings its wiki
page along; a person had to remember it afterwards. Its last edit was on 2026-11-16, and nothing shows which
release a page describes. Coding agents, which write most changes here, cannot reach it at all.

`AGENTS.md` already says that documents change in the same pull request as the code. It did not say which document
belongs to which change, and `docs/architecture.md` drew the structure in text art that is hard to keep right.

## Decision

- **Every text has one home.**
  - `README.md`: what OOS is, its state, how to get it, where to read on.
  - `wiki/`: the guide for people who use a scope.
  - `docs/`: the technical reference: hardware, protocols, architecture, how to extend OOS.
  - `docs/adr/`: the decisions. `CHANGELOG.md`: what a user notices, per release.

  A fact is written in its home; the other places link to it.
- **The wiki is a folder of the tree.** Its pages are in `wiki/` and change through pull requests like everything
  else. `.github/workflows/wiki.yml` publishes the folder as the wiki of the repository when a change to it reaches
  main. Nobody edits the wiki itself; what is edited there is overwritten by the next publication.
- **A change brings its texts along.** `AGENTS.md` has the table that says which text follows which change (a remote
  command, a menu, an option, a hardware fact, a tool, the structure). A pull request without the text is not done.
- **Diagrams are Mermaid, by the C4 levels**: the scope and who uses it, the programs of the image, the parts inside
  the programs; one level per diagram, drawn as a flowchart, with the source files named under it.
  `docs/architecture.md` holds them.
- **The docs follower is started for a merge that left its texts behind** (ADR 0034).

## Alternatives

- **Keep editing the wiki by hand.** The state before: it lags, and no pull request or agent can help.
- **No wiki, everything in `docs/`.** One place less, but the wiki is where people who use a scope look, and its
  pages are linked from the forum.
- **A site built from `docs/`** (GitHub Pages with a generator). Better navigation and search; a build step and a
  theme to keep. Possible later on top of the same folders.
- **A check that fails a pull request which changes code and no text.** A machine cannot tell whether a change needs
  a text; the check would be satisfied by any edit. The review instructions ask for the text instead, and the docs
  steward looks afterwards.

## Consequences

- The pages of the wiki are in the tree as they were on 2026-11-17 (one link text reworded). Moving the guide out of
  the README into them, so the README becomes the entry page, is work that follows, page by page.
- A change that a user sees touches `wiki/` in the same pull request, and review can ask for it.
- The wiki's own history gets one commit per publication, made by the workflow; its older history stays.
- Not tried yet: that the workflow's token may push to the wiki's repository. GitHub's documentation does not say.
  If it may not, the workflow needs a token of the maintainer, and this record is corrected.
- Whether GitHub draws every diagram as written has to be looked at on the page; Mermaid's own C4 diagram types are
  not used because they are experimental.

## References

ADR 0018, ADR 0019, ADR 0034, [AGENTS.md](../../../AGENTS.md), [wiki/README.md](../../../wiki/README.md),
`.github/workflows/wiki.yml`, [docs/architecture.md](../../architecture.md).
