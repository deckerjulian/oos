#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# pr-labels.sh - the labels a pull request gets by itself, so the list of pull requests shows what each one is:
#
#   area/...        by the paths it changes: hal, fpgad, dsp, ui, boot (boot/, kernel/, rootfs/), gpu, tooling
#                   (tools/), ci (.github/), docs (docs/, wiki/, *.md)
#   type/...        by the word before the colon of its title: CI, Docs, Tests, Process, Build or Image, Lints or
#                   Refactor. Other titles name a part of OOS; whether such a change is a fix or a feature is set by
#                   a person (type/fix, type/feat).
#   agent           opened by a bot: Dependabot, or the development agent (tools/as-agent.sh)
#   risk/hardware   changes what is only proven on a scope (the paths of tools/pr-gate.sh)
#
#   tools/pr-labels.sh NUMBER        one pull request
#   tools/pr-labels.sh --all         every open one
#   tools/pr-labels.sh --dry NUMBER  print the labels, set nothing
#
# Labels are only added, never taken away: what a person set stays. Reads the pull request through GitHub's API and
# never its code. Needs gh (GH_TOKEN) and jq. OOS_REPO names another repository (default $GITHUB_REPOSITORY).
set -eu
repo=${OOS_REPO:-${GITHUB_REPOSITORY:-dekajoule/oos}}
hardware='^(hal/src/dho/|hal/src/update/|ui/src/board/|fpgad/|boot/|kernel/|rootfs/|rust-toolchain\.toml$)'
dry=0

one() {
	n=$1
	pr=$(gh api "repos/$repo/pulls/$n")
	files=$(gh api --paginate "repos/$repo/pulls/$n/files" --jq '.[].filename')
	labels=$(printf '%s\n' "$files" | awk '
		/^hal\//                  { a["area/hal"] }
		/^fpgad\//                { a["area/fpgad"] }
		/^dsp\//                  { a["area/dsp"] }
		/^ui\//                   { a["area/ui"] }
		/^(boot|kernel|rootfs)\// { a["area/boot"] }
		/^gpu\//                  { a["area/gpu"] }
		/^tools\//                { a["area/tooling"] }
		/^\.github\//             { a["area/ci"] }
		/^(docs|wiki)\/|\.md$/    { a["area/docs"] }
		END { for (l in a) print l }')
	case $(printf '%s' "$pr" | jq -r '.title | split(":")[0]') in
	CI) labels="$labels type/ci" ;;
	Docs) labels="$labels type/docs" ;;
	Tests) labels="$labels type/test" ;;
	Process) labels="$labels type/process" ;;
	Build | Image | Kernel) labels="$labels type/build" ;;
	Lints | Refactor) labels="$labels type/refactor" ;;
	esac
	[ "$(printf '%s' "$pr" | jq -r .user.type)" != Bot ] || labels="$labels agent"
	! printf '%s\n' "$files" | grep -Eq "$hardware" || labels="$labels risk/hardware"
	# shellcheck disable=SC2086
	set -- $labels
	if [ $dry = 1 ] || [ $# = 0 ]; then
		echo "#$n: $*"
		return
	fi
	printf '%s\n' "$@" | jq -R . | jq -s '{labels: .}' |
		gh api --method POST "repos/$repo/issues/$n/labels" --input - >/dev/null
	echo "#$n: $*"
}

case ${1:-} in
--all) for n in $(gh api --paginate "repos/$repo/pulls?state=open" --jq '.[].number'); do one "$n"; done ;;
--dry) dry=1; one "${2:?number}" ;;
'' | -*) echo "usage: $0 NUMBER | --all | --dry NUMBER" >&2; exit 2 ;;
*) one "$1" ;;
esac
