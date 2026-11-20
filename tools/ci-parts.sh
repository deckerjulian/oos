#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# ci-parts.sh - which jobs of CI a change can reach (docs/adr/process/0040). Reads the names of the changed files on
# standard input, one a line, and prints two lines for $GITHUB_OUTPUT:
#
#   matrix=[{"name": ..., "layers": ..., "cache": ...}, ...]   the entries of CI's job `check`: parts of
#                                                tools/check.sh, and the name of the build cache each one uses
#   compare=true|false                           whether the job `compare` runs
#   scope=true|false                             whether the job `scope` runs: the change touches a path that is
#                                                only proven on a unit inside a crate, and nothing of the image
#                                                (boot/, kernel/, rootfs/), so equal programs can stand for a run on
#                                                a unit (tools/scope-binaries.sh, ADR 0041)
#   families=NAME ...                            the board families the job `check` builds for (OOS_FAMILIES of
#                                                tools/check.sh, ADR 0067): those whose own folders the change
#                                                touches when it touches nothing else that is built, else every
#                                                family of rootfs/board/
#
#   git diff --name-only BASE...HEAD | tools/ci-parts.sh
#   tools/ci-parts.sh --all                      every part (a push to main, the weekly run)
#
# What reaches what: the crates depend on each other as hal <- ui, hal <- fpgad, dsp <- ui; keys/ is compiled into
# hal, the tests of hal read rootfs/. A change of how the checks run (this file, tools/check.sh, the workflow,
# rustfmt.toml) reaches everything, and so does a name nothing here knows outside the paths that hold no code for the
# host checks (documents, tools that have their own tests in the job `files`, the image).
# In doubt: everything. Touches no scope and no network.
set -eu
. "$(dirname "$0")/lib/board.sh"
every=$(oos_families "$(dirname "$0")/..")
all=0
case ${1:-} in
--all) all=1 ;;
'') ;;
*) echo "usage: $0 [--all] < changed-file-names" >&2; exit 2 ;;
esac
hal=0 haltests=0 dsp=0 ui=0 fpgad=0 rust=0 compare=0 unitpath=0 image=0
# the families named by the folders of one family the change touches; neutral=1 for any other file that is built
own= neutral=0
[ $all = 0 ] || { hal=1 dsp=1 ui=1 fpgad=1 neutral=1; }
if [ $all = 0 ]; then
	while IFS= read -r f; do
		mine=
		for b in $every; do
			case $f in
			hal/src/"$b"/* | ui/src/board/"$b"/* | fpgad/src/"$b"/* | rootfs/board/"$b"/* | tools/board/"$b"/*) mine=$b ;;
			esac
		done
		case $f in
		'') ;;
		*) if [ -n "$mine" ]; then own="$own $mine"; else neutral=1; fi ;;
		esac
		case $f in
		'') ;;
		# the paths of tools/pr-gate.sh that only a unit proves: inside a crate, or in the image
		hal/src/dho/* | hal/src/update/* | ui/src/board/* | fpgad/* | rust-toolchain.toml) unitpath=1 ;;
		boot/* | kernel/* | rootfs/*) image=1 ;;
		esac
		case $f in
		'') ;;
		.github/workflows/ci.yml | tools/check.sh | tools/ci-parts.sh | rustfmt.toml)
			hal=1 dsp=1 ui=1 fpgad=1 ;;
		hal/* | keys/*) hal=1 ;;
		# read by the tests of hal, not built into anything
		rootfs/*) haltests=1 ;;
		dsp/*) dsp=1 ;;
		ui/*) ui=1 ;;
		fpgad/*) fpgad=1 ;;
		tools/compare-base.sh | tools/plan-matrix.sh | tools/board/*/plan-matrix.sh | tools/code-size.py) compare=1 ;;
		# no code of the host checks: documents, the image, the GPU helper (built and tried in the weekly run), the
		# other workflows, the host tools (their tests and syntax run in the job files)
		*.md | docs/* | wiki/* | boot/* | kernel/* | gpu/* | .github/* | tools/* | LICENSE | THIRD-PARTY-LICENSES.md | \
			.gitignore | .gitattributes) ;;
		*) hal=1 dsp=1 ui=1 fpgad=1 ;;
		esac
	done
fi
# what depends on what
[ $hal = 0 ] || { ui=1; fpgad=1; }
[ $dsp = 0 ] || ui=1
[ $((hal + dsp + ui + fpgad)) = 0 ] || { rust=1; compare=1; }
entries=
# add NAME LAYERS CACHE; the cache is named by the job, not by its layers, so a pull request finds the one main saved
add() { entries="${entries:+$entries,}{\"name\":\"$1\",\"layers\":\"$2\",\"cache\":\"$3\"}"; }
[ $((hal + haltests)) = 0 ] || add hal test:hal hal
[ $ui = 0 ] || add ui test:ui ui
if [ $rust = 1 ]; then
	name=fmt layers=fmt
	[ $dsp = 0 ] || { name="$name, dsp" layers="$layers test:dsp"; }
	[ $fpgad = 0 ] || { name="$name, fpgad" layers="$layers test:fpgad"; }
	[ $((ui + fpgad)) = 0 ] || { name="$name, target" layers="$layers target"; }
	add "$name" "$layers" small
	add clippy clippy clippy
fi
echo "matrix=[$entries]"
# every family when something neutral changed or no family was named; else the named ones, in the order of $every
families=
for b in $every; do
	if [ $neutral = 1 ] || [ -z "$own" ]; then
		families="$families $b"
	else
		case " $own " in *" $b "*) families="$families $b" ;; esac
	fi
done
echo "families=${families# }"
[ $compare = 1 ] && echo compare=true || echo compare=false
[ $unitpath = 1 ] && [ $image = 0 ] && echo scope=true || echo scope=false
