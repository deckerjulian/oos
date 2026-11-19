# ADR 0069: `unit-tested` holds for the commits it was given for

- Status: accepted (2026-11-19, by the maintainer)
- Date: 2026-11-19
- Category: process

## Context

The label `unit-tested` lets the status `unit` pass (ADR 0038, ADR 0052). It is set after a run on a unit, by the agent
for the live class and by the maintainer for the boot class. The gate reads only whether the label is there. A commit
pushed after the run, a fix from a review for example, kept the label, and `unit` passed for code that never ran on a
unit.

## Decision

**`unit-tested` holds for the commits that were there when it was set. A commit of the branch's own committed after
the label takes it away again. `tools/pr-labels.sh` does this on every run of the gate, before `tools/pr-gate.sh`
sets `unit`.**

- "After" compares the time the label was last set (the pull request's events) with the committer time of the
  branch's commits (the pull request's commits).
- A merge commit does not take it away, so a merge of main to resolve a conflict keeps it. What main brings was proven
  on main.
- With the label gone, `unit` is pending again until a new run on a unit, or until the comparison of ADR 0041/0068 or
  ADR 0042 finds the new state equal.

## Alternatives

- **Remove the label on every push** (the event `synchronize`). A merge of main would cost a run on a unit, and with
  several pull requests open that happens often.
- **Keep the label and trust the author to set it again.** That was the state before. The gate could not tell a tested
  commit from a later one.
- **Record the tested commit in a status or a comment.** That is more exact than times, but it needs a second place for
  the same fact, and a person who sets the label by hand would also have to write the commit.

## Consequences

- A fix after a run on a unit needs another run, unless the comparison proves it equal. The pull request says what ran
  for which commit.
- A merge commit that resolves a conflict by hand inside a hardware path keeps the label. That case is rare, and the
  review sees it.
- The committer time is the author's clock. A commit made long before it is pushed, after the label, is not seen. Such
  commits come from a rebase, and a rebase gives new committer times.

## References

ADR 0038, ADR 0041, ADR 0042, ADR 0052, ADR 0068, `tools/pr-labels.sh`, `tools/pr-gate.sh`,
`tools/tests/test_pr_scripts.py`.
