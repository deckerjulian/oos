#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# ci-parts.sh - which jobs of CI a change can reach (docs/adr/process/0040). Reads the names of the changed files on
# standard input, one a line, and prints two lines for $GITHUB_OUTPUT:
#
#   matrix=[{"name": ..., "layers": ..., "cache": ...}, ...]   the entries of CI's job `check`: parts of
#                                                tools/check.sh, and the name of the build cache each one uses
#   compare=true|false                           whether the job `compare` runs
#
#   git diff --name-only BASE...HEAD | tools/ci-parts.sh
#   tools/ci-parts.sh --all                      every part (a push to main, the weekly run)
#
# What reaches what: the crates depend on each other as hal <- ui, hal <- fpgad, dsp <- ui; keys/ is compiled into
# hal, the tests of hal read rootfs/. A change of how the checks run (this file, tools/check.sh, the workflow,
# rustfmt.toml, the list of decoders) reaches everything, and so does a name nothing here knows outside the paths
# that hold no code for the host checks (documents, tools that have their own tests in the job `files`, the image).
# In doubt: everything. Touches no scope and no network.
set -eu
all=0
case ${1:-} in
--all) all=1 ;;
'') ;;
*) echo "usage: $0 [--all] < changed-file-names" >&2; exit 2 ;;
esac
hal=0 haltests=0 dsp=0 ui=0 fpgad=0 rust=0 compare=0
[ $all = 0 ] || { hal=1 dsp=1 ui=1 fpgad=1; }
if [ $all = 0 ]; then
	while IFS= read -r f; do
		case $f in
		'') ;;
		.github/workflows/ci.yml | tools/check.sh | tools/ci-parts.sh | rustfmt.toml | ui/tools/decoder-features.sh)
			hal=1 dsp=1 ui=1 fpgad=1 ;;
		hal/* | keys/*) hal=1 ;;
		# read by the tests of hal, not built into anything
		rootfs/*) haltests=1 ;;
		dsp/*) dsp=1 ;;
		ui/*) ui=1 ;;
		fpgad/*) fpgad=1 ;;
		tools/compare-base.sh | tools/plan-matrix.sh | tools/code-size.py) compare=1 ;;
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
fi
if [ $ui = 1 ]; then
	add "decoders, core" decoders:core core
	add "decoders, each 1" decoders:each/1 each1
	add "decoders, each 2" decoders:each/2 each2
fi
echo "matrix=[$entries]"
[ $compare = 1 ] && echo compare=true || echo compare=false
