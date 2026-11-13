#!/bin/sh
# Copyright (C) 2026 OOS contributors
# SPDX-License-Identifier: GPL-2.0-or-later
# deps-check.sh - the pinned sources of rootfs/deps.lock against their upstream release listings (docs/fork/deps.md).
# For each line the directory of its url is read (a version in the directory name, as e2fsprogs' v1.47.2/, reads the
# parent; the sixth column names another listing page), the newest release with the same file name pattern is printed next to the pinned one. Release candidates
# do not match (the version is followed directly by the archive suffix). `upstream:` lines are also compared with the
# ver= and sum= of their script: a difference means upstream moved and deps.lock should follow. Read only; network.
#   tools/deps-check.sh          all lines; exit 1 when an update or a difference is found
set -u
here=$(cd "$(dirname "$0")" && pwd)
lock="$here/../rootfs/deps.lock"
status=0
esc() { printf '%s' "$1" | sed 's/[.[\*^$+?(){}|]/\\&/g'; }
printf '%-16s %-34s %-10s %-10s %s\n' name owner pinned latest note
grep -v '^#' "$lock" | while read -r name owner ver sum url index; do
	[ -n "$name" ] || continue
	dir=${url%/*}; file=${url##*/}
	case "${dir##*/}" in
	*"$ver"*) file="${dir##*/}/"; dir=${dir%/*};;
	esac
	# the file name around the version: prefix, a version, suffix (`mesa-` 26.0.8 `.tar.xz`)
	pre=${file%%"$ver"*}; suf=${file#*"$ver"}
	listing=$(curl -fsSL -m 20 "${index:-$dir/}" 2>/dev/null)
	latest=$(printf '%s' "$listing" | grep -oE "$(esc "$pre")[0-9][0-9A-Za-z.]*$(esc "$suf")" | sed "s|^$(esc "$pre")||" |
		while read -r v; do printf '%s\n' "${v%"$suf"}"; done | sort -uV | tail -1)
	note=
	if [ -z "$latest" ]; then
		note="no listing"; latest=-
	elif [ "$latest" != "$ver" ]; then
		note="UPDATE"; status=1
	fi
	case "$owner" in
	upstream:*)
		s="$here/../${owner#upstream:}"
		sv=$(sed -n 's/^ver=//p' "$s" | head -1); ss=$(sed -n 's/^sum=//p' "$s" | head -1)
		if [ "$sv" != "$ver" ] || [ "$ss" != "$sum" ]; then note="$note script has $sv"; status=1; fi;;
	esac
	printf '%-16s %-34s %-10s %-10s %s\n' "$name" "$owner" "$ver" "$latest" "$note"
	echo $status > "${TMPDIR:-/tmp}/deps-check.$$"
done
st=$(cat "${TMPDIR:-/tmp}/deps-check.$$" 2>/dev/null || echo 0); rm -f "${TMPDIR:-/tmp}/deps-check.$$"
exit "$st"
