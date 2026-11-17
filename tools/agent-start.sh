#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# agent-start.sh - gives a standing agent one task (docs/adr/process/0034): an issue with the label `agent`, assigned
# to GitHub Copilot's cloud agent with a profile of .github/agents.
#
#   tools/agent-start.sh AGENT FILE     AGENT: cleaner or docs-follower; FILE: the title on the first line, then the
#                                       text of the issue
#
# An agent whose last task is still open is not given another. GitHub starts an agent only for a person: GH_TOKEN has
# to be a token of the maintainer (metadata: read; actions, contents, issues and pull requests: read and write), not
# a workflow's own. Needs gh and jq. OOS_REPO names another repository (default $GITHUB_REPOSITORY).
set -eu
agent=${1:?agent}
file=${2:?file}
repo=${OOS_REPO:-${GITHUB_REPOSITORY:-dekajoule/oos}}
[ -f ".github/agents/$agent.agent.md" ] || { echo "no profile .github/agents/$agent.agent.md" >&2; exit 2; }
open=$(gh issue list --repo "$repo" --state open --label agent --search "in:title [$agent]" --json number --jq length)
if [ "$open" != 0 ]; then
	echo "$agent still has an open task: nothing started"
	exit 0
fi
title="[$agent] $(head -n 1 "$file")"
body="$(tail -n +2 "$file")

This is a task of the standing agent \`$agent\` (ADR 0034, profile \`.github/agents/$agent.agent.md\`). It opens one
pull request; a person reviews and merges it."
jq -n --arg title "$title" --arg body "$body" --arg agent "$agent" --arg repo "$repo" '{
	title: $title, body: $body, labels: ["agent"], assignees: ["copilot-swe-agent[bot]"],
	agent_assignment: {target_repo: $repo, base_branch: "main", custom_agent: $agent}
}' | gh api --method POST "repos/$repo/issues" --input - --jq '"started: \(.html_url)"'
