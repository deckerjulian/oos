#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# scope-binaries.sh - whether a change changes the programs that run on the scope at all (docs/adr/process/0041,
# 0068): builds the programs of the image (oos-ui, oos-fpgad, oos-update) for the scope from this commit and from
# BASE, in this same folder one after the other, without the places of their panics and their symbol table, and
# compares them byte for byte. Equal programs need no run on a unit.
#
#   tools/scope-binaries.sh [BASE]     BASE: a commit; default: where this branch left origin/main
#   tools/scope-binaries.sh --sums-only            build this commit only, print `PROGRAM SHA-256` for each program
#   tools/scope-binaries.sh --base-sums FILE [BASE]
#                                      take the base's sums from FILE (what --sums-only printed for BASE, e.g. kept by
#                                      CI on the push to main) instead of building it. FILE is only valid for the same
#                                      commit, this script and rust-toolchain.toml: the caller keys it by all three.
#
# Both are built as boot/build-dev-image.sh builds them (release for aarch64-unknown-linux-musl, the crates' own
# profile and target flags), with the folder's path replaced by /oos.
# Cargo puts the absolute path of a crate into the names of its symbols, so the two sides are built in this folder,
# not in two: the tree is checked out at BASE for the second build and back again after it. It refuses a tree with
# changes. Prints the SHA-256 of each program on both sides.
#
# Exit 0: equal. 1: a program differs. 2: the comparison could not be made. Touches no scope.
set -u
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
root=$PWD
stop() {
	echo "scope-binaries.sh: $*" >&2
	exit 2
}
[ -z "$(git status --porcelain --untracked-files=no)" ] || stop "the tree has changes: commit them first"
only=0
base_sums=
base=
while [ $# -gt 0 ]; do
	case $1 in
	--sums-only) only=1 ;;
	--base-sums) base_sums=${2:-}; [ -n "$base_sums" ] || stop "--base-sums needs a file"; shift ;;
	-*) stop "unknown option $1" ;;
	*) base=$1 ;;
	esac
	shift
done
if [ -n "$base_sums" ]; then
	# three lines `PROGRAM SHA-256`, one for each program, nothing else
	[ -f "$base_sums" ] || stop "no file $base_sums"
	awk 'NF == 2 && $1 ~ /^oos-(ui|fpgad|update)$/ && $2 ~ /^[0-9a-f]{64}$/ { n[$1]++; next } { bad = 1 }
		END { exit (bad || n["oos-ui"] != 1 || n["oos-fpgad"] != 1 || n["oos-update"] != 1) }' "$base_sums" ||
		stop "$base_sums does not hold the three sums of --sums-only"
fi
if [ $only = 0 ]; then
	arg=$base
	if [ -z "$base" ]; then
		base=$(git merge-base origin/main HEAD 2>/dev/null) || stop "no origin/main to compare with: name a commit"
	fi
	base=$(git rev-parse --verify --quiet "$base^{commit}") || stop "not a commit: $arg"
fi
here=$(git rev-parse HEAD)
back=$(git symbolic-ref --quiet --short HEAD || echo "$here")
target=aarch64-unknown-linux-musl
# What the comparison leaves out (ADR 0068): the place of each panic in the source (`-Zlocation-detail=none`: file,
# line and column, which every line moved above changes) and the symbol table (`-Cstrip=symbols`: names only). Both
# are data the programs never act on, they only print or name it. The flag is unstable: RUSTC_BOOTSTRAP lets the pinned
# release take it for these two builds, which are compared and thrown away; the image is built without it.
flags="\"--remap-path-prefix=$root=/oos\", \"-Zlocation-detail=none\", \"-Cstrip=symbols\""
sums() { # sums NAME: build the programs, print their sums
	(cd ui && RUSTC_BOOTSTRAP=1 cargo build --release --locked --quiet --target "$target" \
		--config "target.$target.rustflags=[$flags]") || return 1
	# the flags are added to the crate's own target flags (.cargo/config.toml), not put in their place
	(cd fpgad && RUSTC_BOOTSTRAP=1 cargo build --release --locked --quiet --target "$target" \
		--config "target.$target.rustflags=[$flags]") || return 1
	shasum -a 256 "ui/target/$target/release/oos-ui" "fpgad/target/$target/release/oos-fpgad" \
		"fpgad/target/$target/release/oos-update" |
		awk -v n="$1" '{ sub(".*/", "", $2); print n, $2, $1 }'
}
work=$(mktemp -d)
trap 'git checkout -q "$back" 2>/dev/null; rm -rf "$work"' EXIT
sums this >"$work/this" || stop "this commit does not build for the scope"
if [ $only = 1 ]; then
	cut -d' ' -f2- "$work/this"
	exit 0
fi
if [ -n "$base_sums" ]; then
	sed 's/^/base /' "$base_sums" >"$work/base"
else
	git checkout -q --detach "$base" || stop "cannot check out $base"
	sums base >"$work/base" || stop "the base does not build for the scope"
	git checkout -q "$back" || stop "cannot go back to $back"
fi
cat "$work/base" "$work/this"
if [ "$(cut -d' ' -f2- "$work/base" | sort)" = "$(cut -d' ' -f2- "$work/this" | sort)" ]; then
	echo "the programs for the scope are byte-identical to those of $(git log -1 --format='%h %s' "$base")"
	exit 0
fi
echo "the programs for the scope differ from those of $(git log -1 --format='%h' "$base")"
exit 1
