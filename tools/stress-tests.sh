#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# stress-tests.sh - the tests of all four crates again and again next to busy processes, to find the test that
# depends on how fast the computer is (docs/adr/process/0020: such a test is fixed, not run again).
#
#   tools/stress-tests.sh [ROUNDS]     default 5 rounds; two busy processes for every processor
#
# Builds the tests once, starts the busy processes, runs `cargo test` in each crate ROUNDS times and names every test
# that failed in any round, with the number of rounds it failed in. Exit 1 when one failed. The busy processes are
# ended when the script ends, however it ends. Touches no scope; needs minutes.
set -u
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
rounds=${1:-5}
crates="hal dsp ui fpgad"
tmp=$(mktemp -d)
pids=
stop() {
	# shellcheck disable=SC2086
	[ -z "$pids" ] || kill $pids 2>/dev/null
	rm -rf "$tmp"
}
trap stop EXIT
trap 'exit 130' INT TERM

for c in $crates; do
	(cd "$c" && cargo test --locked --no-run --quiet) || {
		echo "stress-tests.sh: the tests of $c do not build" >&2
		exit 2
	}
done

n=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)
i=0
while [ "$i" -lt $((n * 2)) ]; do
	sh -c 'while :; do :; done' &
	pids="$pids $!"
	i=$((i + 1))
done
echo "== $rounds rounds next to $((n * 2)) busy processes"

: >"$tmp/failed"
r=1
while [ "$r" -le "$rounds" ]; do
	for c in $crates; do
		if ! (cd "$c" && cargo test --locked 2>&1) >"$tmp/out"; then
			grep -E '^test .* \.\.\. FAILED$' "$tmp/out" | sed -E "s/^test (.*) \.\.\. FAILED\$/$c \1/" >>"$tmp/failed"
			grep -qE '^test .* \.\.\. FAILED$' "$tmp/out" || echo "$c (no test named: see the build)" >>"$tmp/failed"
			echo "round $r, $c: failed"
		fi
	done
	echo "round $r done"
	r=$((r + 1))
done

if [ -s "$tmp/failed" ]; then
	echo "== tests that failed, and in how many of $rounds rounds:"
	sort "$tmp/failed" | uniq -c | sort -rn
	exit 1
fi
echo "== every test passed in all $rounds rounds"
