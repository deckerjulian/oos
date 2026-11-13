# Copyright (C) 2026 OOS contributors
# SPDX-License-Identifier: GPL-2.0-or-later
# deps.sh - sourced by the fork's build scripts: their sources from rootfs/deps.lock (docs/fork/deps.md).
#   dep_ver NAME                       the pinned version
#   dep_fetch NAME DIR                 download into DIR once, check the SHA-256, print the path
#   dep_stale ARTIFACT SCRIPT NAME...  true when ARTIFACT is missing or was built from other manifest lines or another
#                                      SCRIPT (its build options): boot/unit-hook.sh rebuilds it then
#   dep_stamp ARTIFACT SCRIPT NAME...  record what ARTIFACT was built from (ARTIFACT.stamp), at the end of a build
DEPS_LOCK="${DEPS_LOCK:-$here/deps.lock}"
dep_line() {
	awk -v n="$1" '!/^#/ && $1 == n { print; f = 1 } END { exit !f }' "$DEPS_LOCK" || { echo "deps.lock: no $1" >&2; return 1; }
}
dep_ver() { dep_line "$1" | awk '{ print $3 }'; }
dep_sha256() { { shasum -a 256 "$1" 2>/dev/null || sha256sum "$1"; } | cut -d' ' -f1; }
dep_fetch() {
	l=$(dep_line "$1") || return 1
	sum=$(echo "$l" | awk '{ print $4 }'); url=$(echo "$l" | awk '{ print $5 }')
	t="$2/$(basename "$url")"
	mkdir -p "$2"
	[ -f "$t" ] || curl -fsSL -o "$t" "$url" || { echo "download failed: $url" >&2; rm -f "$t"; return 1; }
	[ "$(dep_sha256 "$t")" = "$sum" ] || { echo "wrong checksum: $t (deps.lock $1)" >&2; rm -f "$t"; return 1; }
	echo "$t"
}
dep_stamp_text() {
	s=$2; shift 2
	echo "script $(dep_sha256 "$s")"
	for n in "$@"; do dep_line "$n" | awk '{ print $1, $3, $4 }'; done
}
dep_stale() {
	a=$1; shift
	[ -e "$a" ] && [ -f "$a.stamp" ] || return 0
	[ "$(cat "$a.stamp")" != "$(dep_stamp_text "$a" "$@")" ]
}
dep_stamp() {
	a=$1; shift
	dep_stamp_text "$a" "$@" > "$a.stamp"
}
