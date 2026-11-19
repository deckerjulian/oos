#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# compare-base.sh - what a change does to the pictures, to the files OOS stores, to the answers to remote commands
# and to the register sequences, against a commit before it (docs/adr/process/0014, 0032). One command for the proofs
# of a refactoring; CI runs it for every pull request.
#
#   tools/compare-base.sh [BASE]     BASE: a commit; default: where this branch left origin/main
#   tools/compare-base.sh --base-sums DIR [BASE]
#                                    the same, with what --record-sums kept of BASE: when the pictures, stored files
#                                    and remote answers of this tree have the same sums as BASE's and DIR holds BASE's
#                                    oos-fpgad, BASE is not built. A difference falls back to building BASE, so the
#                                    report names every picture with the box of its change as before
#   tools/compare-base.sh --record-sums DIR
#                                    keeps in DIR what --base-sums reads, of this tree as committed (HEAD, no changes):
#                                    the sums of its pictures, stored files and remote answers, its oos-fpgad, the files
#                                    of its live run (live/), the commit (CI: the push to main fills a cache that the pull
#                                    requests onto it read)
#
# Checks BASE out into a tree of its own (ui/target/compare-base/tree, a git worktree that stays for the next run),
# builds it, records its pictures, stored files and remote answers (ui/tools/layout-check.sh) and its register
# sequences (tools/plan-matrix.sh), does the same with the working tree as it is, uncommitted changes included, and
# compares. Then it runs the acquisition loop against the simulated unit on both sides (the live run, ADR 0053:
# `board::dho::hw::liverun`, a test of oos-ui that writes one file per case of its matrix: the unit's trace and the
# frames delivered) and compares those files. A base from before the live run has none: that part is then left out and
# the summary says so.
#
# The register sequences are printed with the calibration files in OOS_CAL_DIR, else in local/cal of this tree, else
# with a made-up calibration set: `oos-fpgad made-up-cal` of this tree writes it once into the work folder, and both
# sides read it. Its values are invented (docs/protocols/self-cal.md 16), so the comparison sees every register value
# that calibration changes, but not the values of a real unit. CI has no files of a unit and takes this way. Only an
# oos-fpgad from before the made-up set leaves the calibrated values out of both sides.
#
# Exit 0: nothing differs. 1: a picture, a register sequence or the live run differs. 2: the comparison could not be
# made. The report is in ui/target/compare-base/: summary.md, pictures.txt, registers.diff, live.diff and differing/
# with both versions of the first pictures that differ. Touches no scope.
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

# The live run of tree $1 into folder $2 (made new). Returns 1 when that tree has no live run.
live_run() {
	[ -f "$1/ui/src/board/dho/hw/liverun.rs" ] || return 1
	rm -rf "$2" && mkdir -p "$2" || stop "cannot make $2"
	# absolute: the test runs in $1/ui
	set -- "$1" "$(cd "$2" && pwd)"
	(cd "$1/ui" && OOS_LIVE_OUT=$2 cargo test --locked --quiet --bin oos-ui -- board::dho::hw::liverun) \
		>"$2.log" 2>&1 || stop "the live run fails in $1: $(tail -n 3 "$2.log")"
	ls "$2"/*.txt >/dev/null 2>&1 || stop "the live run of $1 wrote no file"
}

sums=
case ${1:-} in
--record-sums)
	dir=${2:?--record-sums DIR}
	# untracked files too: layout-check.sh renders every ui/tests/*.txt, committed or not
	[ -z "$(git status --porcelain)" ] || stop "--record-sums needs a tree without changes or untracked files"
	mkdir -p "$dir" || stop "cannot make $dir"
	(cd ui && tools/layout-check.sh record target/layout-sums) || stop "this tree does not render"
	(cd fpgad && cargo build --release --locked --quiet) || stop "oos-fpgad does not build"
	cp ui/target/layout-sums.sums "$dir/layout-base.sums" && cp fpgad/target/release/oos-fpgad "$dir/oos-fpgad" &&
		git rev-parse HEAD >"$dir/commit" || stop "cannot write $dir"
	live_run "$root" "$dir/live" || rm -rf "$dir/live"
	echo "kept the sums of $(git log -1 --format='%h %s') in $dir"
	exit 0
	;;
--base-sums)
	sums=${2:?--base-sums DIR [BASE]}
	shift 2
	;;
esac
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
# what --record-sums kept of the base, if it is of this base and complete
cached=0
if [ -n "$sums" ] && [ "$(cat "$sums/commit" 2>/dev/null)" = "$base" ] && [ -s "$sums/layout-base.sums" ] &&
	[ -x "$sums/oos-fpgad" ]; then
	cached=1
fi
pictures=
if [ "$cached" = 1 ]; then
	echo "== pictures of this tree, against the sums kept of the base"
	(cd ui && tools/layout-check.sh record target/layout-new) >"$work/pictures.txt" 2>&1 || stop "this tree does not render"
	if cmp -s "$sums/layout-base.sums" ui/target/layout-new.sums; then
		n=$(grep -c '\.ppm$' ui/target/layout-new.sums)
		m=$(grep -vc '\.ppm$' ui/target/layout-new.sums || true)
		echo "all $n pictures and $m other files equal to the sums kept of the base" >"$work/pictures.txt"
		pictures=0
	else
		echo "the pictures differ from the sums kept of the base: the base is built for the report"
	fi
fi
if [ -z "$pictures" ]; then
	echo "== pictures of the base"
	(cd "$tree" && ui/tools/layout-check.sh record) || stop "the base does not render"
	echo "== pictures of this tree"
	pictures=0
	ui/tools/layout-check.sh check target/compare-base/tree/ui/target/layout-base >"$work/pictures.txt" 2>&1 ||
		pictures=$?
fi
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

# the base's oos-fpgad: the one kept with the sums, else built in its tree
base_fpgad=$tree/fpgad/target/release/oos-fpgad
if [ "$cached" = 1 ]; then
	base_fpgad=$sums/oos-fpgad
else
	(cd "$tree/fpgad" && cargo build --release --locked --quiet) || stop "oos-fpgad does not build in $tree"
fi
(cd "$root/fpgad" && cargo build --release --locked --quiet) || stop "oos-fpgad does not build in $root"
if [ -z "$cal" ]; then
	# no files of a unit: the made-up set, written new by this tree's oos-fpgad and read by both sides. A failure
	# stops the comparison: without the set the calibrated values would silently not be compared.
	cal=$work/made-up-cal
	rm -rf "$cal"
	fpgad/target/release/oos-fpgad made-up-cal "$cal" >"$work/made-up-cal.txt" 2>&1 ||
		stop "the made-up calibration set could not be written: $(tail -n 1 "$work/made-up-cal.txt")"
	# every file the command names is there and not empty
	grep '^/' "$work/made-up-cal.txt" >"$work/made-up-files.txt"
	[ -s "$work/made-up-files.txt" ] || stop "oos-fpgad made-up-cal named no file"
	while read -r f; do
		[ -s "$f" ] || stop "the made-up calibration set lacks $f"
	done <"$work/made-up-files.txt"
	cal_note="with the made-up calibration set"
fi
echo "== register sequences, $cal_note"
OOS_CAL_DIR=$cal "$tree/tools/plan-matrix.sh" "$base_fpgad" >"$work/registers-base.txt"
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

echo "== the acquisition loop on the simulated unit (live run)"
live=0
if live_run "$root" "$work/live-new"; then
	# the base's files: kept with the sums, else run in its tree; a base from before the live run has none
	live_base=
	if [ "$cached" = 1 ] && [ -d "$sums/live" ]; then
		live_base=$sums/live
	elif live_run "$tree" "$work/live-base"; then
		live_base=$work/live-base
	fi
	cases=$(find "$work/live-new" -name '*.txt' | wc -l | tr -d ' ')
	if [ -z "$live_base" ]; then
		live_line="not compared: the base has no live run ($cases cases in this tree)"
	elif diff -ru "$live_base" "$work/live-new" >"$work/live.diff"; then
		live_line="all $cases cases equal (the unit's trace and the frames delivered)"
	else
		live=1
		differ=$(grep -c '^diff ' "$work/live.diff" || true)
		only=$(grep -c '^Only in ' "$work/live.diff" || true)
		live_line="$((differ + only)) of $cases cases differ, see live.diff"
		head -n 40 "$work/live.diff"
	fi
else
	live_line="not run: this tree has no live run"
fi
echo "$live_line"

{
	echo "### Against $(git log -1 --format='%h' "$base")"
	echo
	echo "- Pictures, stored files, remote answers: $(tail -n 1 "$work/pictures.txt")"
	echo "- Register sequences: $registers_line"
	echo "- Live run of the acquisition loop: $live_line"
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
	if [ "$live" = 1 ]; then
		echo
		echo '```diff'
		head -n 60 "$work/live.diff"
		echo '```'
	fi
} >"$work/summary.md"

[ "$pictures" = 0 ] && [ "$registers" = 0 ] && [ "$live" = 0 ]
