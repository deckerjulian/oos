# ADR 0038: A pull request of the maintainer or of a bot we set up is merged by itself when three questions are answered

- Status: accepted (2026-11-17, by the maintainer, with the pipeline as a whole and without the waiting time of
  ADR 0031), amended by ADR 0039 (the development agent's pull requests are merged like the maintainer's; `review`
  is required) by ADR 0041 (`unit` passes when the programs for the scope are byte-identical to the base's) and by ADR 0042
  (and for a refactoring of register-sequence code the comparison found equal)
- Date: 2026-11-17
- Category: process

## Context

Since ADR 0031 every change is a pull request that waits 24 or 72 hours. For the maintainer that means coming back
to each one to press the button, and with three standing agents and Dependabot (ADR 0034) there will be more of
them. GitHub can merge a pull request by itself once its required checks have passed. Two things stood in the way:

- **The required checks said too little.** Only `test` was required. A pull request merged on that alone would
  ignore the waiting time, would not ask who wrote it, and would not know that a change of a register sequence is
  only proven on a scope.
- **Who may be merged without a look.** Not a pull request from a fork: its author is a stranger to the checks.
  GitHub's own rule for reviews cannot tell the maintainer's pull requests from others; the maintainer has to pass
  it by, by hand, every time (ADR 0036).

The comparison with the base now sees pictures, stored files, remote answers and register sequences (ADR 0032). It
does not see timing, calibrated values, or what the hardware does.

## Decision

- **Three questions, answered by a machine for every pull request** and written to its newest commit as statuses
  (`tools/pr-gate.sh`, workflow `Gate`):
  - `waited`: it has been open as long as ADR 0031 asks: 72 hours when it brings or changes a decision record, else
    24. The label `no-wait`, which only the maintainer can set, is for what ADR 0031 lets in at once: a fix for a
    red main, a security fix, a correction of a document.
    **No wait at all** (added on 2026-11-17, on the maintainer's decision) for two kinds of pull request that stay
    outside the hardware paths, `.github/`, the tools (but their tests), the decision records and the rule texts
    (`AGENTS.md`, `CONTRIBUTING.md`):
    - one that only changes documents (`*.md`, `docs/`, `wiki/`);
    - one that says `Behaviour: unchanged` and whose comparison with the base ran and found nothing (the job
      `compare` passed and kept nothing). That is what lints, dead code, a move and new tests look like.

    The wait was there so others see a change coming. For these two a machine has checked what a person would look
    for in the wait; what they change shows in the list of merged pull requests.
    **And no wait for the maintainer's own pull requests** (decided on 2026-11-17, later that day): from a branch
    of this repository and not an agent's. ADR 0031 gave the maintainer the same wait as everyone so that a
    contributor could plan against main; the maintainer takes that back. The wait stays for everybody else, for
    agents and for Dependabot.
  - `owner`: it is the maintainer's own; or it is Dependabot's and changes nothing but the commits of actions in
    the workflow files; or the maintainer has approved its newest commit. A pull request from a fork always needs
    that approval. An agent's pull request (a `copilot/` branch of this repository) is opened in the name of the
    person who gave the task, and GitHub does not let that person approve it: it passes when a person has marked
    it ready for review, which an agent cannot do. (Corrected on 2026-11-17 after the first run of an agent: the
    first version took such a pull request for the maintainer's own.)
  - `unit`: it touches nothing that is only proven on a scope (`hal/src/dho/`, `hal/src/update/`, `ui/src/board/`,
    `fpgad/`, `boot/`, `kernel/`, `rootfs/`); or it carries the label `unit-tested`, which the maintainer sets after
    trying it on a unit.
- **A fourth status, `review`** (added on 2026-11-17, after the first automatic reviews): GitHub Copilot has
  reviewed the pull request and left no open issue, or every thread it opened is resolved. A draft has no review
  yet. Shown, not required, until the first reviews have shown how often a comment is right.
- **The label `hold`** stops any pull request: `owner` stays pending while it is set. The maintainer sets it, also
  on the maintainer's own pull requests, to look before something is merged by itself.
- **These three, `test` and `compare` become required checks of main.** A pull request is merged when all five have
  passed, by whoever presses the button or by GitHub.
- **GitHub is asked to merge by itself** (workflow `Auto-merge`) for pull requests from branches of this repository
  by the maintainer and by Dependabot. Never for a fork and never for an agent: there the maintainer looks and
  merges. (Changed on 2026-11-17, on the maintainer's decision: the first version included the agents.)
- **The squashed commit** carries the title of the pull request, the messages of its commits and the sign-off of the
  maintainer, who answers for it (ADR 0026, ADR 0034).
- **The review of ADR 0036 is asked by the status `owner`** once this is switched on; the ruleset `main-review` is
  then removed. The effect is the same (nobody else's change enters without the maintainer's approval), and the
  maintainer's own pull requests no longer need a rule passed by.
- **Switched on in steps**, each by the maintainer: first only the statuses, shown and not required; then the five
  required checks; then auto-merge (the repository setting, the variable `AUTO_MERGE`, the secret
  `MAINTAINER_TOKEN`).

## Alternatives

- **Keep merging by hand.** Safe, and the maintainer's time: every pull request is visited twice.
- **Auto-merge on `test` alone.** The waiting times and the scope would be skipped; this is what the workflow would do
  if it were switched on before the statuses are required, and why it does nothing until then.
- **Let agents' pull requests merge without approval when the comparison says nothing changed.** The aim, and not
  yet: the comparison does not see timing and calibrated values. To be looked at again when about twenty pull
  requests of agents have been merged without a correction.
- **GitHub's merge queue**, which tests each pull request on the main it will land on. Not offered for a repository
  of a personal account. Until then a pull request is tested on the main of its last push; a clash of two green
  pull requests shows in the run on main.
- **A label instead of statuses** ("ready"): a person would still have to set it at the right hour.

## Consequences

- The maintainer opens a pull request and it is merged when CI is green and the wait is over; an agent's is merged
  after the maintainer's approval; a change near the hardware waits for the label `unit-tested`.
- A wrong answer of `tools/pr-gate.sh` merges something too early. The script is short and reads only what GitHub
  says about a pull request; it joins the files an agent must not change (ADR 0034).
- **A limit of the statuses:** a pull request from a branch of this repository brings its own workflow files, and a
  workflow can write a status. Dependabot's workflows have a token that can only read, and an agent's workflows do
  not run until a person allows it, but this is a reason to keep `.github/` out of agents' reach (ADR 0034) and to
  look at every change there.
- The token `MAINTAINER_TOKEN` acts as the maintainer. It is used by one workflow that runs no code of a pull
  request, and it cannot administer the repository.
- A merge by GitHub happens while nobody watches. The run on main after it, and the weekly runs, are what shows a
  problem; a red main is fixed first (ADR 0031).
- **Switched on, 2026-11-17, on the maintainer's instruction:** `test`, `compare`, `waited`, `owner` and `unit` are
  the required checks of main; the ruleset `main-review` is removed, as decided above; the repository allows
  auto-merge. The workflow `Auto-merge` still waits for the maintainer's token; until then auto-merge is asked for
  by hand, pull request by pull request (`gh pr merge --auto --squash`). If the workflow `Gate` should fail, the
  statuses can be written by hand with `tools/pr-gate.sh NUMBER`.
- Tried on 2026-11-17: the statuses and the labels, on Dependabot's first pull request: `owner` passed (only the
  commits of actions), `unit` passed, `waited` was pending, and the labels were set. Not tried yet: auto-merge
  itself, how the agents' pull requests name their author, and whether a merge asked for with the maintainer's token
  starts the workflows on main.

## References

ADR 0026, ADR 0031, ADR 0032, ADR 0034, ADR 0036, `tools/pr-gate.sh`, `.github/workflows/gate.yml`,
`.github/workflows/auto-merge.yml`.
