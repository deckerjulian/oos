#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# compare-base.sh - what a change does to the pictures, to the files OOS stores, to the answers to remote commands
# and to the register sequences, against a commit before it (docs/adr/process/0014, 0032). One command for the proofs
# of a refactoring; CI runs it for every pull request.
#
#   tools/compare-base.sh [BASE]     BASE: a commit; default: where this branch left origin/main
#
# Checks BASE out into a tree of its own (ui/target/compare-base/tree, a git worktree that stays for the next run),
# builds it, records its pictures, stored files and remote answers (ui/tools/layout-check.sh) and its register
# sequences (tools/plan-matrix.sh), does the same with the working tree as it is, uncommitted changes included, and
# compares.
#
# The register sequences are printed with the calibration files in OOS_CAL_DIR, else in local/cal of this tree, else
# with a made-up calibration set: `oos-fpgad made-up-cal` of this tree writes it once into the work folder, and both
# sides read it. Its values are invented (docs/protocols/self-cal.md 16), so the comparison sees every register value
# that calibration changes, but not the values of a real unit. CI has no files of a unit and takes this way. Only an
# oos-fpgad from before the made-up set leaves the calibrated values out of both sides.
#
# Exit 0: nothing differs. 1: a picture or a register sequence differs. 2: the comparison could not be made.
# The report is in ui/target/compare-base/: summary.md, pictures.txt, registers.diff and differing/ with both
# versions of the first pictures that differ. Touches no scope.
set -u
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
root=$PWD
work=$root/ui/target/compare-base
tree=$work/tree

stop() {
	echo "compare-base.sh: $*" >&2
	exit 2
}

base=${1:-}
if [ -z "$base" ]; then
	base=$(git merge-base origin/main HEAD 2>/dev/null) || stop "no origin/main to compare with: name a commit"
fi
base=$(git rev-parse --verify --quiet "$base^{commit}") || stop "not a commit: ${1:-}"

mkdir -p "$work"
if [ -e "$tree/.git" ]; then
	git -C "$tree" checkout --quiet --detach --force "$base" || stop "cannot check out $base in $tree"
else
	git worktree prune
	git worktree add --quiet --detach --force "$tree" "$base" || stop "cannot make the tree of $base in $tree"
fi

if [ -n "${OOS_CAL_DIR:-}" ]; then
	cal=$(cd "$OOS_CAL_DIR" 2>/dev/null && pwd) || stop "OOS_CAL_DIR is not a folder: $OOS_CAL_DIR"
	cal_note="with the calibration files in OOS_CAL_DIR"
elif [ -f "$root/local/cal/cal_adc.hex" ]; then
	cal=$root/local/cal
	cal_note="with the calibration files in local/cal"
else
	# decided when this tree's oos-fpgad is built
	cal=
fi

echo "== base $(git log -1 --format='%h %s' "$base")"
echo "== pictures of the base"
(cd "$tree" && ui/tools/layout-check.sh record) || stop "the base does not render"
echo "== pictures of this tree"
pictures=0
ui/tools/layout-check.sh check target/compare-base/tree/ui/target/layout-base >"$work/pictures.txt" 2>&1 || pictures=$?
cat "$work/pictures.txt"
[ "$pictures" -le 1 ] || stop "this tree does not render"
# the first pictures and files that differ, both versions, for the report
rm -rf "$work/differing"
grep -E '^\./[^ ]+ +([0-9]+ px|line [0-9]+:)' "$work/pictures.txt" | head -n 12 | while read -r name _; do
	for side in base new; do
		mkdir -p "$work/differing/$side/$(dirname "$name")"
	done
	cp "$tree/ui/target/layout-base/$name" "$work/differing/base/$name"
	cp "ui/target/layout-new/$name" "$work/differing/new/$name"
done

for t in "$tree" "$root"; do
	(cd "$t/fpgad" && cargo build --release --locked --quiet) || stop "oos-fpgad does not build in $t"
done
if [ -z "$cal" ]; then
	# no files of a unit: the made-up set, written new by this tree's oos-fpgad and read by both sides. An oos-fpgad
	# that does not know the command prints its help and writes nothing.
	cal=$work/made-up-cal
	rm -rf "$cal"
	if fpgad/target/release/oos-fpgad made-up-cal "$cal" >/dev/null 2>&1 && [ -f "$cal/cal_adc.hex" ]; then
		cal_note="with the made-up calibration set"
	else
		rm -rf "$cal"
		cal=$work/no-cal
		mkdir -p "$cal"
		cal_note="without calibration files"
	fi
fi
echo "== register sequences, $cal_note"
OOS_CAL_DIR=$cal "$tree/tools/plan-matrix.sh" "$tree/fpgad/target/release/oos-fpgad" >"$work/registers-base.txt"
OOS_CAL_DIR=$cal tools/plan-matrix.sh fpgad/target/release/oos-fpgad >"$work/registers-new.txt"
lines=$(wc -l <"$work/registers-new.txt" | tr -d ' ')
[ "$lines" -gt 1000 ] || stop "tools/plan-matrix.sh printed only $lines lines"
registers=0
diff -u "$work/registers-base.txt" "$work/registers-new.txt" >"$work/registers.diff" || registers=1
if [ "$registers" = 0 ]; then
	registers_line="all $lines lines equal ($cal_note)"
else
	changed=$(grep -c '^[-+][^-+]' "$work/registers.diff")
	registers_line="$changed lines differ ($cal_note), see registers.diff"
	head -n 40 "$work/registers.diff"
fi
echo "$registers_line"

{
	echo "### Against $(git log -1 --format='%h' "$base")"
	echo
	echo "- Pictures, stored files, remote answers: $(tail -n 1 "$work/pictures.txt")"
	echo "- Register sequences: $registers_line"
	if [ "$pictures" = 1 ]; then
		echo
		echo '```'
		head -n 60 "$work/pictures.txt"
		echo '```'
	fi
	if [ "$registers" = 1 ]; then
		echo
		echo '```diff'
		head -n 60 "$work/registers.diff"
		echo '```'
	fi
} >"$work/summary.md"

[ "$pictures" = 0 ] && [ "$registers" = 0 ]
