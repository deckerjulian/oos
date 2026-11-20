# ADR 0034: Standing agents get their task from a machine and open one pull request; a person merges

- Status: superseded by ADR 0039 (no standing agents; one development agent in a session the maintainer started)
- Date: 2026-11-17
- Category: process

## Context

OOS is written by one maintainer with coding agents, and has grown fast: 143 394 lines of Rust on 2026-11-17, nine
files of more than 2 000 lines, `ui/src/ui.rs` with 13 747, and 338 findings of Clippy. Three kinds of work do not
get done by themselves:

- **Cleaning up.** The findings of a lint, dead code, files too large to review. Each piece is small and mechanical;
  together they decide whether the tree can still be changed safely.
- **Keeping the texts true.** README, `docs/`, the wiki and the changelog are written when a feature is new and fall
  behind with the next change of the code.
- **Listening.** Problems and wishes are reported in a forum thread, and a fork of OOS fixes things that are wrong
  here too. Both reached the issues only when the maintainer went and read.

A first draft of this record gave each of the three to an agent that chose its own work once a week. Three things
spoke against it:

- An agent that may choose between a lint, dead code, a missing test and a fix opens pull requests nobody can
  predict, and each has to be reviewed from nothing. An agent does best with a task that says what it covers and
  when it is done.
- Texts fall behind at the moment a change is merged, not once a week.
- Reading the forum needs no model. A page with summaries, reviewed as a pull request and then copied into issues by
  hand, is more work than reading the posts.

What GitHub offers, read in its documentation on 2026-11-17 (not all of it tried yet):

- Copilot's cloud agent works on an issue that is assigned to it, in an environment of its own, for at most 59
  minutes. It pushes to one new branch `copilot/...`, opens one pull request per task, and cannot approve or merge.
  Workflows do not run on its pull request until a person with write access allows it.
- A profile in `.github/agents/NAME.agent.md` gives an agent its role; it can be chosen when an issue is assigned,
  also through the API.
- A task can only be started for a person: with a token of a user, not with a workflow's own token. GitHub's own
  scheduled automations are not offered for public repositories.
- The agent has no setting that keeps it out of certain paths.
- The maintainer's plan includes 1 500 AI credits a month; every run of an agent and every automatic review is paid
  from them. What one run costs is not known yet.

## Decision

- **Two agents, each with one kind of task, and the task is given to them:**
  - `cleaner`: mechanical clean-up that changes no behaviour. Once a week `tools/agent-target.py` chooses the task
    by counting: the lint with the most findings in the crate that is closest to clean, and when no finding is left,
    one part to be moved out of the largest file. Findings and files in the paths that are only proven on a scope
    (those the status `unit` of ADR 0038 waits for) come last, since a task there waits for the maintainer at a
    unit (added 2026-11-17, after the first task landed in `fpgad/`). The issue says what is covered, when it is
    done, and that behaviour stays as it is.
  - `docs-follower`: the texts for one merged change. It is started after a merge to main that changed code and no
    text, with the commit named in its issue. Clean-up, tests and changes of the pipeline do not start it.
- **No agent chooses its own work, fixes an error or adds a feature by itself.** An error an agent finds is a
  sentence in its pull request. Fixes and features are issues written by a person (the form "Task") and assigned
  one at a time.
- **The scout is a script, not an agent.** Once a week `tools/scout.py` lists the new replies of the forum thread as
  links and what the fork merged and opened as titles, in one issue. Nothing of the posts is read into the issue;
  a person reads them and decides what becomes an issue. No model is involved, so nothing a stranger wrote can
  steer one.
- **An agent opens a pull request and nothing else.** It never merges and never pushes to main. The maintainer
  reviews and merges, with the waiting times of ADR 0031.
- **The limits are in the profiles and are checked where a machine can check them:**
  - One task, about 400 changed lines at most.
  - A pull request of the cleaner says `Behaviour: unchanged`, and the comparison with the base (ADR 0032) has to
    agree.
  - No agent changes the rules, the checks, what decides how the scope starts, or the register sequences:
    `.github/`, `AGENTS.md`, `docs/adr/` (but the index), the check scripts and the scripts named here, `boot/`,
    `kernel/`, `rootfs/`, the licence files, `hal/src/dho/`. CI job `agent-paths` fails a pull request from a
    `copilot/` branch that does (`tools/agent-paths.sh` holds the list).
  - No weakened check: no removed test, no new `allow`, no changed expected result, no raised number.
- **The sign-off of an agent's pull request is the maintainer's.** The agent's commits carry none; the squashed commit
  that enters main is signed off by the person who merges it and answers for it (ADR 0026). CI does not ask the
  commits of a `copilot/` branch of this repository for a sign-off.
- **One at a time, and one agent first.** An agent whose last task is still open gets no new one. The cleaner is
  started first, after the formatting commit (ADR 0016), on the findings of Clippy; the docs follower follows when
  two or three of its runs have shown what a run costs and how good it is.
- **Reuse comes first**, for agents and people: `AGENTS.md` asks every change to look for code that already does the
  job, and to remove what it leaves unused.

## Alternatives

- **Do it by hand, in sessions with an agent.** The state before: it happens when the maintainer thinks of it.
- **Agents that choose their own work once a week** (the first draft): see Context.
- **An agent that reads the forum and writes summaries.** It would read text by strangers with a model, which is
  where an instruction hidden in a post does harm, to save ten minutes of reading.
- **Let an agent merge when CI is green.** The checks cannot see timing, the hardware, or whether a text is true. A
  wrong register write is only seen on a unit. ADR 0038 says under which conditions this is looked at again.
- **A service outside GitHub that runs agents on a schedule.** Possible, and not ruled out for later; it needs its own
  access to the repository. The cloud agent is already bound by the repository's own protection.

## Consequences

- The maintainer gets one pull request of the cleaner a week, and one of the docs follower for a merge that left its
  texts behind. That review is the gate; the agents are only as good as it is.
- The cleaner's pull requests are of one kind and proven by the comparison, so they are quick to review, and they
  are the first that could be merged without a look, once the comparison sees enough (ADR 0038).
- The start needs a secret, `COPILOT_TASK_TOKEN`: a fine-grained token of the maintainer for this repository. Without
  it the workflow prints the task it would have given, and an agent is started by assigning an issue by hand. The
  scout needs no secret.
- Every run of an agent costs AI credits; the numbers are looked at after the first runs.
- `agent-paths` runs from the workflow file of the pull request itself, so it is a help for the reviewer and not a
  lock: a change under `.github/` is what the reviewer looks for first.
- The scout's issue names the fork's work without links, so the fork gets no notices from this repository.
- Tried on 2026-11-17: a task given through the API with the profile `cleaner` started an agent on this plan, and
  it opened a draft pull request on a `copilot/` branch, in the name of the person whose token gave the task. That
  name is why the status `owner` of ADR 0038 goes by the branch, not by the author. Not tried yet: that the forum
  can be read from GitHub's runners (it can from a desktop), and how well an agent keeps to its task. What fails is
  recorded here.
- `.github/workflows/copilot-setup-steps.yml` prepares the agent's environment (Rust with the scope's target), so
  an agent can run `tools/check.sh` before it opens a pull request.
- Worth a second look when a second maintainer joins (required reviews), or when the agents' pull requests are
  rejected more often than merged.

## References

ADR 0014, ADR 0016, ADR 0017, ADR 0018, ADR 0026, ADR 0031, ADR 0032, `.github/agents/`,
`.github/workflows/agents.yml`, `.github/workflows/copilot-setup-steps.yml`, `tools/agent-target.py`,
`tools/agent-start.sh`, `tools/scout.py`, `tools/agent-paths.sh`, [AGENTS.md](../../../AGENTS.md).
