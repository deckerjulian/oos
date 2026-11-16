# Copyright (C) 2026 OOS contributors
# SPDX-License-Identifier: GPL-2.0-or-later
# deps.sh - sourced by every build script that downloads a source, and by the scripts that decide whether to run them:
# the sources from rootfs/deps.lock (docs/deps.md). DEPS_LOCK names the file when the caller is not in rootfs/.
#   dep_ver NAME                 the pinned version
#   dep_fetch NAME DIR           download into DIR once, check the SHA-256, print the path
#   dep_stale ARTIFACT INPUT...  true when ARTIFACT is missing or was built from other inputs; an INPUT with a `/` is a
#                                file whose content counts (the build script, a source of the tree), any other INPUT
#                                is the name of a deps.lock line. The caller builds ARTIFACT again then
#   dep_stamp ARTIFACT INPUT...  record what ARTIFACT was built from (ARTIFACT.stamp), at the end of a build
# The functions keep their variables to the dep_ prefix: they run in the caller's shell.
DEPS_LOCK="${DEPS_LOCK:-$here/deps.lock}"
dep_line() {
	awk -v n="$1" '!/^#/ && $1 == n { print; f = 1 } END { exit !f }' "$DEPS_LOCK" || { echo "deps.lock: no $1" >&2; return 1; }
}
dep_ver() { dep_line "$1" | awk '{ print $2 }'; }
dep_sha256() { { shasum -a 256 "$1" 2>/dev/null || sha256sum "$1"; } | cut -d' ' -f1; }
dep_fetch() {
	dep_l=$(dep_line "$1") || return 1
	dep_sum=$(echo "$dep_l" | awk '{ print $3 }'); dep_url=$(echo "$dep_l" | awk '{ print $4 }')
	dep_t="$2/$(basename "$dep_url")"
	mkdir -p "$2"
	[ -f "$dep_t" ] || curl -fsSL -o "$dep_t" "$dep_url" || { echo "download failed: $dep_url" >&2; rm -f "$dep_t"; return 1; }
	[ "$(dep_sha256 "$dep_t")" = "$dep_sum" ] || { echo "wrong checksum: $dep_t (deps.lock $1)" >&2; rm -f "$dep_t"; return 1; }
	echo "$dep_t"
}
dep_stamp_text() {
	for dep_i in "$@"; do
		case $dep_i in
		*/*) echo "script $(dep_sha256 "$dep_i")" ;;
		*) dep_line "$dep_i" | awk '{ print $1, $2, $3 }' ;;
		esac
	done
}
dep_stale() {
	dep_a=$1; shift
	[ -e "$dep_a" ] && [ -f "$dep_a.stamp" ] || return 0
	[ "$(cat "$dep_a.stamp")" != "$(dep_stamp_text "$@")" ]
}
dep_stamp() {
	dep_a=$1; shift
	dep_stamp_text "$@" > "$dep_a.stamp"
}
