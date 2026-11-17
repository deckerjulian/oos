#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# repo-settings.sh - the settings of the repository on GitHub against the files that state them
# (docs/adr/process/0036): .github/repo-settings.json (how pull requests are merged) and .github/rulesets/*.json (what
# protects main).
#
#   tools/repo-settings.sh            compare: say where GitHub differs from the files; changes nothing
#   tools/repo-settings.sh --apply    set GitHub to what the files say (needs admin rights on the repository)
#
# Needs gh (logged in) and jq. OOS_REPO names another repository (default dekajoule/oos). Touches no scope.
set -eu
cd "$(dirname "$0")/.."
repo=${OOS_REPO:-dekajoule/oos}
apply=0
case ${1:-} in
'') ;;
--apply) apply=1 ;;
*) echo "usage: $0 [--apply]" >&2; exit 2 ;;
esac
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
differs=0

# what a ruleset file states; GitHub adds ids, dates and links to its answer
shape='{name, target, enforcement, conditions, bypass_actors, rules}'

compare() { # compare NAME: $tmp/want and $tmp/have
	if diff -u "$tmp/have" "$tmp/want" >"$tmp/diff"; then
		echo "$1: as in the file"
		return 0
	fi
	echo "$1: GitHub (-) differs from the file (+)"
	sed 's/^/  /' "$tmp/diff"
	differs=1
	return 1
}

jq -S . .github/repo-settings.json >"$tmp/want"
keys=$(jq -r 'keys | join(", ")' .github/repo-settings.json)
gh api "repos/$repo" | jq -S "{$keys}" >"$tmp/have"
if ! compare "merge settings" && [ $apply = 1 ]; then
	gh api --method PATCH "repos/$repo" --input .github/repo-settings.json >/dev/null
	echo "  set"
fi

gh api "repos/$repo/rulesets" >"$tmp/list"
for f in .github/rulesets/*.json; do
	name=$(jq -r .name "$f")
	id=$(jq -r --arg n "$name" '.[] | select(.name == $n) | .id' "$tmp/list")
	jq -S "$shape" "$f" >"$tmp/want"
	if [ -z "$id" ]; then
		echo "ruleset $name: not on GitHub"
		differs=1
		if [ $apply = 1 ]; then
			gh api --method POST "repos/$repo/rulesets" --input "$f" >/dev/null
			echo "  created"
		fi
		continue
	fi
	gh api "repos/$repo/rulesets/$id" | jq -S "$shape" >"$tmp/have"
	if ! compare "ruleset $name" && [ $apply = 1 ]; then
		gh api --method PUT "repos/$repo/rulesets/$id" --input "$f" >/dev/null
		echo "  set"
	fi
done
jq -r '.[].name' "$tmp/list" | while read -r n; do
	[ -n "$(grep -l "\"name\": \"$n\"" .github/rulesets/*.json)" ] || echo "ruleset $n: on GitHub, in no file"
done

if gh api "repos/$repo/branches/main/protection" >/dev/null 2>&1; then
	echo "note: main also has a branch protection rule of the older kind; it is not in a file and is left alone"
fi
if [ $apply = 0 ] && [ $differs = 1 ]; then
	echo "nothing was changed; --apply sets GitHub to what the files say"
	exit 1
fi
