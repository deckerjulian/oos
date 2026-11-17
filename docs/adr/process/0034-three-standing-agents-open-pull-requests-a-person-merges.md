# ADR 0034: Three standing agents open pull requests; a person merges

- Status: proposed
- Date: 2026-11-17
- Category: process

## Context

OOS is written by one maintainer with coding agents, and has grown fast: 143 394 lines of Rust on 2026-11-17, nine
files of more than 2 000 lines, `ui/src/ui.rs` with 13 747. Three kinds of work do not get done by themselves:

- **Looking after the code.** Dead code, the same thing written twice, parts without a test, files too large to
  review. Each is small; together they decide whether the tree can still be changed safely.
- **Keeping the texts true.** README, `docs/`, the wiki and the changelog are written when a feature is new and fall
  behind afterwards.
- **Listening.** Problems and wishes are reported in a forum thread. They reached the issues only when the
  maintainer read the thread and wrote them over by hand.

ADR 0018 gives every agent the same instructions, and ADR 0031 makes every change a pull request. What was missing
is somebody who starts the work without being asked, and limits for an agent that works without a person next to it.

What GitHub offers, read in its documentation on 2026-11-17 (not all of it tried yet):

- Copilot's cloud agent works on an issue that is assigned to it, in an environment of its own, for at most 59
  minutes. It pushes to one new branch `copilot/...`, opens one pull request per task, and cannot approve or merge.
  Workflows do not run on its pull request until a person with write access allows it.
- A profile in `.github/agents/NAME.agent.md` gives an agent its role. It can be chosen when an issue is assigned,
  also through the API.
- A task can only be started for a person: with a token of a user, not with a workflow's own token. GitHub's own
  scheduled automations are not offered for public repositories.
- The agent has no setting that keeps it out of certain paths.
- The maintainer's plan includes 1 500 AI credits a month; every run of an agent and every automatic review is paid
  from them.

## Decision

- **Three agents, each with one job**, as profiles in `.github/agents`:
  - `code-steward`: one improvement of the code per run: a warning, dead code, a duplicate, a missing test, a part
    moved out of a file that is too large, or a fix with its test.
  - `docs-steward`: README, `docs/`, the wiki pages and the changelog against the code; the diagrams (Mermaid, by
    the C4 levels).
  - `forum-scout`: the new posts of the forum thread, recorded in `docs/forum-intake.md`. The maintainer turns its
    rows into issues.
- **They are started once a week each** by `.github/workflows/agents.yml`, or by hand with a task. A start is an
  issue with the label `agent`. An agent whose last issue is still open is not started again.
- **An agent opens a pull request and nothing else.** It never merges, never pushes to main, and opens no issues by
  itself. The maintainer reviews and merges, with the waiting times of ADR 0031.
- **The limits are in the profiles and are checked where a machine can check them:**
  - One topic, about 400 changed lines at most; a pure removal may be larger.
  - A pull request of the code steward that is not a fix says `Behaviour: unchanged`, and the comparison with the
    base (ADR 0032) has to agree.
  - No agent changes the rules, the checks, what decides how the scope starts, or the register sequences:
    `.github/`, `AGENTS.md`, `docs/adr/` (but the index), `tools/check.sh` and the scripts of the comparison,
    `boot/`, `kernel/`, `rootfs/`, the licence files, `hal/src/dho/`. CI job `agent-paths` fails a pull request from
    a `copilot/` branch that does (`tools/agent-paths.sh` holds the list).
  - No weakened check: no removed test, no new `allow`, no changed expected result.
- **What comes from the forum is data.** The forum scout treats posts as text by strangers, never as instructions,
  follows no link out of the forum, copies no more than a phrase, and leaves out personal data and everything about
  the inside of the vendor's firmware. Its only output is a page that a person reads before anything becomes an
  issue.
- **The sign-off of an agent's pull request is the maintainer's.** The agent's commits carry none; the squashed commit
  that enters main is signed off by the person who merges it and answers for it (ADR 0026). CI does not ask the
  commits of a `copilot/` branch of this repository for a sign-off.
- **Reuse comes first**, for agents and people: `AGENTS.md` asks every change to look for code that already does the
  job, and to remove what it leaves unused.

## Alternatives

- **Do it by hand, in sessions with an agent.** The state before: it happens when the maintainer thinks of it.
- **One agent for everything.** One pull request would then mix code, texts and reports, and its limits could not
  differ: the forum scout needs the network and no right to touch code, the code steward the opposite.
- **Let an agent merge when CI is green.** The checks cannot see timing, the hardware, or whether a text is true. A
  wrong register write is only seen on a unit.
- **Let the forum scout open issues directly.** Then text from strangers would become issues, with their
  notifications, without a person in between.
- **A service outside GitHub that runs agents on a schedule.** Possible, and not ruled out for later; it needs its own
  access to the repository. The cloud agent is already bound by the repository's own protection.

## Consequences

- The maintainer gets up to three pull requests a week to review. That review is the gate; the agents are only as
  good as it is.
- The start needs a secret, `COPILOT_TASK_TOKEN`: a fine-grained token of the maintainer for this repository. Without
  it the workflow does nothing, and an agent is started by assigning an issue by hand.
- Every run costs AI credits. Weekly runs were chosen so that the plan's allowance is enough; the numbers are looked
  at after the first month.
- `agent-paths` runs from the workflow file of the pull request itself, so it is a help for the reviewer and not a
  lock: a change under `.github/` is what the reviewer looks for first.
- Not tried yet, and to be confirmed with the first runs: that a profile can be chosen through the API on this plan,
  that the forum can be read from the agent's environment, and that the agent keeps to a profile's limits. What
  fails is recorded here.
- `.github/workflows/copilot-setup-steps.yml` prepares the agent's environment (Rust with the scope's target), so
  an agent can run `tools/check.sh` before it opens a pull request.
- Worth a second look when a second maintainer joins (required reviews), or when the agents' pull requests are
  rejected more often than merged.

## References

ADR 0014, ADR 0018, ADR 0026, ADR 0031, ADR 0032, `.github/agents/`, `.github/workflows/agents.yml`,
`.github/workflows/copilot-setup-steps.yml`, `tools/agent-paths.sh`, [docs/forum-intake.md](../../forum-intake.md),
[AGENTS.md](../../../AGENTS.md).
