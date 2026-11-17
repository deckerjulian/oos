#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# pr-gate.sh - four answers about a pull request that decide whether it may be merged without a person pressing the
# button (docs/adr/process/0038), written to its newest commit as statuses:
#
#   waited   it has been open as long as ADR 0031 asks: 72 hours with a decision record, else 24. No wait at all for
#            the maintainer's own pull requests and those of the maintainer's development agent (both from a branch
#            of this repository; ADR 0039), for one that only changes documents (not the decision records and not
#            the rule texts), and for one that says `Behaviour: unchanged` where the comparison with its base ran and
#            found nothing, the last two outside the hardware paths, the pipeline and the tools (ADR 0038). The
#            label `no-wait` (a fix for a red main, a security fix) takes the wait away too.
#   owner    pending for every pull request with the label `hold`, which the maintainer sets to stop one. Else: it
#            is the maintainer's own or the development agent's, or Dependabot's and changes nothing but the
#            commits of actions, or the maintainer has approved its newest commit.
#   unit     it touches nothing that is only proven on a scope, or it carries the label `unit-tested`
#   review   GitHub Copilot has reviewed it, once per pull request (ADR 0039), and left no open issue, or every
#            thread it opened is resolved; pending until then, and for a draft. When the review was of an earlier
#            commit, the status says so: what came after it was not read by Copilot.
#            Dependabot's pull requests that only move the commits of actions get no review and need none.
#
#   tools/pr-gate.sh NUMBER        one pull request
#   tools/pr-gate.sh --all         every open one (the waits run out without an event)
#   tools/pr-gate.sh --dry NUMBER  print the four answers, write nothing
#
# Reads the pull request through GitHub's API and never its code. Needs gh (GH_TOKEN) and jq. OOS_REPO,
# OOS_MAINTAINER and OOS_AGENT name another repository, maintainer and agent (defaults: $GITHUB_REPOSITORY, its owner
# and the App dekajoule-oos-agent of tools/as-agent.sh).
set -eu
repo=${OOS_REPO:-${GITHUB_REPOSITORY:-dekajoule/oos}}
maintainer=${OOS_MAINTAINER:-${repo%%/*}}
agent_login=${OOS_AGENT:-dekajoule-oos-agent[bot]}
# paths whose effect is only seen on a unit: register sequences and the board, updates, how the scope starts
hardware='^(hal/src/dho/|hal/src/update/|ui/src/board/|fpgad/|boot/|kernel/|rootfs/)'
# what the comparison with the base does not see, so no change there goes in without the wait: the pipeline and its
# scripts, the tools, the rules, the decision records
unseen='^(\.github/|tools/|ui/tools/|docs/adr/|AGENTS\.md$|CLAUDE\.md$|CONTRIBUTING\.md$|LICENSE|THIRD-PARTY)'
dry=0

status() { # status SHA CONTEXT STATE TEXT
	if [ $dry = 1 ]; then
		printf '%-7s %-8s %s\n' "$2" "$3" "$4"
	else
		gh api --method POST "repos/$repo/statuses/$1" -f context="$2" -f state="$3" -f description="$4" >/dev/null
	fi
}

one() {
	n=$1
	pr=$(gh api "repos/$repo/pulls/$n")
	sha=$(printf '%s' "$pr" | jq -r .head.sha)
	author=$(printf '%s' "$pr" | jq -r .user.login)
	here=$(printf '%s' "$pr" | jq -r '.head.repo.full_name == .base.repo.full_name')
	labels=$(printf '%s' "$pr" | jq -r '[.labels[].name] | join(" ")')
	draft=$(printf '%s' "$pr" | jq -r '.draft == true')
	# the maintainer's own, or the development agent's: from a branch of this repository
	own=
	if [ "$here" = true ] && [ "$author" = "$maintainer" ]; then
		own="the maintainer's own"
	elif [ "$here" = true ] && [ "$author" = "$agent_login" ]; then
		own="the maintainer's development agent"
	fi
	files=$(gh api --paginate "repos/$repo/pulls/$n/files" --jq '.[].filename')
	# Dependabot's that changes nothing but lines that name an action, in workflow files
	bump=false
	if [ "$here" = true ] && [ "$author" = 'dependabot[bot]' ]; then
		other=$(printf '%s\n' "$files" | grep -v '^\.github/workflows/' || true)
		lines=$(gh api --paginate "repos/$repo/pulls/$n/files" --jq '.[].patch // ""' |
			grep -E '^[+-]' | grep -Ev '^(\+\+\+|---)' | grep -Evc '^[+-] *(- )?uses: ' || true)
		[ -n "$other" ] || [ "$lines" != 0 ] || bump=true
	fi

	# waited: no wait for the maintainer's own and the agent's, for documents only, or for a change the comparison
	# proved to leave behaviour as it was
	quick=$own
	if [ -z "$quick" ] && ! printf '%s\n' "$files" | grep -Ev '^tools/tests/' | grep -Eq "$hardware|$unseen"; then
		if ! printf '%s\n' "$files" | grep -Evq '\.md$|^docs/|^wiki/'; then
			quick="documents only"
		elif printf '%s' "$pr" | jq -r '.body // ""' | tr -d '\r' | grep -Eiq '^\**Behaviou?r:\** *unchanged'; then
			# the run of CI for this commit: its job compare passed, and kept nothing because nothing differed
			run=$(gh api "repos/$repo/actions/runs?head_sha=$sha&event=pull_request&per_page=20" \
				--jq '[.workflow_runs[] | select(.name == "CI")] | first | .id // empty' 2>/dev/null || true)
			if [ -n "$run" ]; then
				passed=$(gh api "repos/$repo/actions/runs/$run/jobs?per_page=100" \
					--jq '[.jobs[] | select(.name == "compare")] | first | .conclusion // empty' 2>/dev/null || true)
				kept=$(gh api "repos/$repo/actions/runs/$run/artifacts" \
					--jq '[.artifacts[] | select(.name == "compare-base")] | length' 2>/dev/null || echo 1)
				[ "$passed" != success ] || [ "$kept" != 0 ] || quick="behaviour unchanged, the comparison found nothing"
			fi
		fi
	fi
	case " $labels " in
	*" no-wait "*) hours=0 ;;
	*)
		hours=24
		# a decision record, not only the index
		if printf '%s\n' "$files" | grep -E '^docs/adr/.+\.md$' | grep -qv '^docs/adr/README\.md$'; then hours=72; fi
		;;
	esac
	age=$(printf '%s' "$pr" | jq -r '((now - (.created_at | fromdateiso8601)) / 3600) | floor')
	if [ -n "$quick" ]; then
		status "$sha" waited success "no wait: $quick"
	elif [ "$age" -ge "$hours" ]; then
		status "$sha" waited success "open for $age h, $hours h asked"
	else
		status "$sha" waited pending "open for $age h of $hours h"
	fi

	# owner
	approved=$(gh api --paginate "repos/$repo/pulls/$n/reviews" |
		jq -s --arg m "$maintainer" --arg s "$sha" \
			'add | [.[] | select(.user.login == $m and .commit_id == $s)] | last | .state == "APPROVED"')
	held=false
	case " $labels " in *" hold "*) held=true ;; esac
	if [ $held = true ]; then
		status "$sha" owner pending "held by the maintainer (label hold)"
	elif [ -n "$own" ]; then
		status "$sha" owner success "$own"
	elif [ "$approved" = true ]; then
		status "$sha" owner success "approved by the maintainer"
	elif [ $bump = true ]; then
		status "$sha" owner success "Dependabot: only the commits of actions"
	elif [ "$here" = true ] && [ "$author" = 'dependabot[bot]' ]; then
		status "$sha" owner pending "Dependabot changed more than the commits of actions: needs the maintainer's approval"
	else
		status "$sha" owner pending "needs the maintainer's approving review of this commit"
	fi

	# review: Copilot's newest review; "No open issues" in its overview, or every thread of its resolved
	review=$(gh api --paginate "repos/$repo/pulls/$n/reviews" |
		jq -s 'add | [.[] | select(.is_copilot_code_review == true)] | last // empty')
	if [ "$draft" = true ]; then
		status "$sha" review pending "a draft: the review comes when it is ready for review"
	elif [ $bump = true ] && [ -z "$review" ]; then
		status "$sha" review success "Dependabot: only the commits of actions, no review needed"
	elif [ -z "$review" ]; then
		status "$sha" review pending "no review by Copilot yet"
	else
		query="{repository(owner:\"${repo%%/*}\",name:\"${repo#*/}\"){pullRequest(number:$n){"
		query="${query}reviewThreads(first:100){nodes{isResolved comments(first:1){nodes{author{login}}}}}}}}"
		open_threads=$(gh api graphql -f query="$query" --jq '[.data.repository.pullRequest.reviewThreads.nodes[]
			| select(.isResolved == false) | select(.comments.nodes[0].author.login | test("copilot"; "i"))]
			| length' 2>/dev/null || echo 0)
		effort=$(printf '%s' "$review" | jq -r '.copilot_code_review_effort // "?"')
		[ "$(printf '%s' "$review" | jq -r .commit_id)" = "$sha" ] || effort="$effort, of an earlier commit"
		if printf '%s' "$review" | jq -r .body | grep -q 'No open issues'; then
			status "$sha" review success "Copilot: no open issues ($effort)"
		elif [ "$open_threads" = 0 ]; then
			status "$sha" review success "Copilot's comments are all resolved"
		else
			status "$sha" review pending "Copilot left $open_threads open comment(s): answer and resolve them"
		fi
	fi

	# unit
	if printf '%s\n' "$files" | grep -Eq "$hardware"; then
		case " $labels " in
		*" unit-tested "*) status "$sha" unit success "tried on a unit (label unit-tested)" ;;
		*) status "$sha" unit pending "changes what is only proven on a scope: needs the label unit-tested" ;;
		esac
	else
		status "$sha" unit success "nothing that needs a scope"
	fi
}

case ${1:-} in
--all) for n in $(gh api --paginate "repos/$repo/pulls?state=open" --jq '.[].number'); do one "$n"; done ;;
--dry) dry=1; one "${2:?number}" ;;
'' | -*) echo "usage: $0 NUMBER | --all | --dry NUMBER" >&2; exit 2 ;;
*) one "$1" ;;
esac
