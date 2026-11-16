# ADR 0026: A contribution is signed off, holds no vendor material and enters main as reviewed commits

- Status: accepted
- Date: 2026-11-16
- Category: process

## Context

ADR 0013 keeps vendor code and data out of the tree, and ADR 0015 asks every statement about the hardware to say how
it is known. Both were written for the files of the tree. Within one week pull requests from forks brought more than
250 commits (pull requests 7 to 13). Three things showed:

- Nobody had been asked where a contribution comes from. The licence note in `CONTRIBUTING.md` says under which
  licence the work is published, not that the contributor may publish it.
- The rules did not name the texts around the code: commit messages, pull request descriptions, issues. Working
  notes of a fork came in with its commits and had to be rewritten for the public tree afterwards.
- A merge keeps every commit of a pull request in the history of main, also the ones a later commit of the same pull
  request corrects. A review of the final state does not see them.

## Decision

- **Sign-off.** Every commit of a contribution carries a `Signed-off-by:` line with the name of the person who
  submits it (`git commit -s`). With it the person states the Developer Certificate of Origin 1.1
  (<https://developercertificate.org>): the work is their own or they have the right to submit it, under
  GPL-2.0-or-later. This holds for work an agent wrote as well; the person who submits it signs.
- **No vendor material, anywhere.** ADR 0013 holds for code, comments, documents, commit messages, pull request
  descriptions and issues alike. The hardware is described as behaviour, in the contributor's own words, with how it
  is known (ADR 0015). Material from the stock or a modified firmware stays out: code, listings, copied tables,
  strings, and the names of its functions, classes and files. What the vendor publishes for users is not meant: the
  command names of the programming guide, the manuals, the data sheets of the parts.
- **Other projects.** Code or text taken from another project names its source and its licence in the pull request,
  and the licence allows GPL-2.0-or-later.
- **One reviewed commit per topic.** A pull request from a fork enters main squashed: one commit per topic
  (ADR 0019), made from the state that was reviewed. The commits of the fork stay in the fork.
- A pull request or an issue that does not keep these rules is not merged, and its text is edited or removed.

## Alternatives

- **A contributor licence agreement.** More than this project needs: the licence stays GPL-2.0-or-later, and nobody
  hands over rights. The sign-off is what the Linux kernel uses for the same question.
- **Merging pull requests with their commits, as before.** Keeps the author's history, but puts unreviewed states on
  main for good. The author is still named: a squashed commit carries the author and the sign-off.

## Consequences

- A contributor adds `-s` to `git commit`; a pull request without sign-off waits until its commits have it.
- The history of main reads as one line of reviewed changes. The detailed history of a larger piece of work stays in
  the fork it came from; the pull request links it.
- The maintainer reads the commits and the texts of a pull request before taking it, not only the final diff.
- The pull request and issue templates state the rules, so nobody has to find them first.

## References

[CONTRIBUTING.md](../../../CONTRIBUTING.md), [AGENTS.md](../../../AGENTS.md),
`.github/PULL_REQUEST_TEMPLATE.md`, `.github/ISSUE_TEMPLATE/`, ADR 0013, ADR 0015, ADR 0019.
