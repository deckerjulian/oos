# ADR 0039: One development agent, with an identity of its own, opens and merges pull requests under the checks

- Status: accepted (2026-11-17, by the maintainer)
- Date: 2026-11-17
- Category: process

## Context

ADR 0034 set up two standing agents (`cleaner`, `docs-follower`) that GitHub's Copilot cloud agent ran from issues,
and ADR 0038 let the maintainer's own pull requests merge themselves. After the first runs:

- **The standing agents did not get through.** The first cleaner pull request (#88) needed a person to mark it ready,
  to approve its workflow runs by hand, and a unit test for its one file in `fpgad/`; after the formatting commit it
  no longer merged at all. An agent that cannot run the checks, the comparison or a scope before it pushes hands
  every one of those steps to the maintainer.
- **A session of a coding agent at the maintainer's computer can do all of it**: `tools/check.sh`,
  `tools/compare-base.sh`, the maintainer's private check before every push, and, with the scope on the bench, a run
  on the unit. Its pull requests were opened with the maintainer's token, so `tools/pr-gate.sh` took them for the
  maintainer's own: nothing told a person's change from an agent's.
- Copilot's code review runs on every pull request that is ready for review (ADR 0036) and is a second reader from
  another vendor. Its status `review` was shown, not required.

## Decision

- **The standing agents are gone.** No agent starts on a schedule or from an issue; the profiles in
  `.github/agents`, `tools/agent-start.sh`, `tools/agent-paths.sh`, the CI job `agent-paths` and the Copilot setup
  workflow are removed. The scout stays (workflow `Scout`): it is a script without a model.
- **The development agent is a coding agent working in a session the maintainer started**, with a task the
  maintainer gave (for example "consolidate the decoders"). It acts on GitHub as the App `dekajoule-oos-agent`
  (`tools/as-agent.sh`), so its pull requests are its own and the list shows them with the label `agent`. Its
  commits carry the maintainer's sign-off: the maintainer answers for them (ADR 0026).
- **Its pull requests need no waiting time** (`waited` passes for them as for the maintainer's own) and **merge by
  themselves** once the required checks pass: `test`, `compare`, `owner`, `unit` and now **`review`**: Copilot has
  reviewed it (once per pull request) and every comment it left is answered and resolved. The label `hold` stops any of them.
- **A change in the hardware paths is tried on the unit first.** When the maintainer has put the scope on the bench
  for it in that session (AGENTS.md, "Never"), the agent tries the change there (deploy to RAM,
  `tools/device-test.py`, restore what was installed) and then sets `unit-tested`, saying in the pull request what
  ran on the unit. Without a unit the label stays off and the pull request waits for one.
- The Auto-merge workflow asks for the merge with a token of the App (secret `AGENT_APP_KEY`, variable
  `AGENT_APP_CLIENT_ID`) in place of a token of the maintainer.

## Alternatives

- **Keep the standing agents and give them a token.** They still could not run the comparison or a scope before
  pushing, and each pull request would still need the maintainer's clicks.
- **The agent's pull requests under the maintainer's name, as before.** Simpler, but the history and the gate could
  not tell who wrote a change, and `hold` would be the only brake.
- **A required approval by the maintainer.** It is what the waiting time was for; the maintainer chose speed and the
  checks over a click per pull request.

## Consequences

- The checks are the whole brake: what `compare`, the tests, Clippy, the ratchet and Copilot do not see goes in.
  Timing and calibrated values are not compared (ADR 0032); a hardware path is only as good as the run on the unit.
- Copilot's review becomes a gate: when it does not come (credits used up, an outage), the agent's pull requests
  wait. GitHub does not ask Copilot by itself for a pull request opened by an App (the review is counted against
  the licence of the person who asks for it), so the agent asks with `gh pr edit N --add-reviewer @copilot` under
  the maintainer's account. One review per pull request keeps the credits low; what is pushed after it is read by
  the checks only. Dependabot's pull requests that only move the commits of actions get no review and need none.
- The App's private key lives with the maintainer (outside the tree) and as a secret of the repository; whoever has
  it can open and merge pull requests that pass the checks.
- Worth a second look when a merged change of the agent has to be reverted, or when the agent works without the
  maintainer at the computer.

## References

ADR 0026, ADR 0031, ADR 0032, ADR 0034 (superseded), ADR 0036, ADR 0038 (amended), `tools/as-agent.sh`,
`tools/pr-gate.sh`, `.github/workflows/auto-merge.yml`, `.github/rulesets/main.json`, [AGENTS.md](../../../AGENTS.md).
