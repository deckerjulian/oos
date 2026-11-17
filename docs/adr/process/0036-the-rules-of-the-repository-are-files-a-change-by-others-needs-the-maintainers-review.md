# ADR 0036: The rules of the repository are files in the tree; a change by anyone else needs the maintainer's review

- Status: accepted (2026-11-17, by the maintainer, with the pipeline as a whole and without the waiting time of
  ADR 0031; an objection is an issue and can still change it)
- Date: 2026-11-17
- Category: process

## Context

ADR 0031 protects main: pull requests only, squashed, with a green check. The protection was set by hand on
2026-11-17, with one call to GitHub's API. Two things are missing:

- **Nobody can see or review the rules.** What protects main is a setting only an administrator can read. A change of
  it leaves no trace in the history, and a contributor cannot propose one.
- **No review is required.** The rule asks for zero approvals, because the maintainer cannot approve their own pull
  requests. It then asks for zero from everybody: whoever has write access, a second maintainer later or a program
  with a token today, can merge their own pull request as soon as the check is green.

Today the maintainer is the only account with write access, so the second point has not hurt. It decides whether
agents can be given work (ADR 0034) and whether a second person can be given write access at all.

## Decision

- **The rules are files.** `.github/rulesets/main.json` and `.github/rulesets/main-review.json` hold what protects
  main, `.github/repo-settings.json` how pull requests are merged, `.github/CODEOWNERS` who reviews what. A change of
  a rule is a pull request against these files.
- **`tools/repo-settings.sh` compares GitHub with the files** and names every difference; with `--apply` an
  administrator sets GitHub to what the files say, after the pull request is merged. Nothing sets them automatically:
  a workflow that could change the protection would need a token that can.
- **Two rulesets.**
  - `main` holds for everyone, the maintainer included, and nobody can pass it by: no deletion, no forced push, a
    linear history, changes only through pull requests, squash merge only, the check `test` green.
  - `main-review` adds the review of the owner of every changed file (`CODEOWNERS`: the maintainer for everything).
    Only an administrator passes it by, and only by merging a pull request, never by a push.
- **So:** the maintainer's pull requests are merged as before. A pull request of anyone or anything else needs the
  maintainer's approving review before it can be merged, by whoever merges it.

## Alternatives

- **One required approval for everyone.** The maintainer could not merge anything until there is a second
  maintainer.
- **Keep the settings by hand.** They work; nobody can check them, and the next change is again invisible.
- **Apply the files from a workflow on every merge.** Convenient, but the token of that workflow could remove the
  protection it sets.
- **List only some paths in `CODEOWNERS`** (the checks, the hardware layer). A pull request that touches none of them
  could then still be merged by its author without any review.

## Consequences

- When the maintainer merges their own pull request, GitHub asks them to confirm that a rule is passed by (on the
  command line: `gh pr merge --admin`). That is the review of code owners, nothing else: the check stays required.
- A token of the maintainer acts with the maintainer's rights, the right to pass `main-review` by included. Such a
  token is only given to what must act as the maintainer, and it is not given the right to administer the
  repository.
- The branch protection rule of the older kind stays set until the rulesets have been in force for a while; both ask
  for the same. It is removed by hand then, and `tools/repo-settings.sh` says as long as it is there.
- The files are not in force by being merged. After the merge an administrator runs `tools/repo-settings.sh --apply`
  once, and again after every change of them; run without `--apply`, it shows whether GitHub still matches.
- Tried on 2026-11-17: GitHub took the two rulesets as written, and both are in force. It fills in three fields
  the files did not name (`require_extra_approval_for_unattributed_changes`, `required_reviewers`,
  `do_not_enforce_on_create`); the files name them now, so the comparison is exact.
- Worth a second look when a second maintainer joins: `CODEOWNERS` gets their paths, and a required approval for
  everyone becomes possible.

## References

ADR 0026, ADR 0031, ADR 0034, `.github/rulesets/`, `.github/repo-settings.json`, `.github/CODEOWNERS`,
`tools/repo-settings.sh`.
