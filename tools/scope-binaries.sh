#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# scope-binaries.sh - whether a change changes the programs that run on the scope at all (docs/adr/process/0041):
# builds oos-ui and oos-fpgad for the scope from this commit and from BASE, in this same folder one after the other,
# and compares them byte for byte. Equal programs need no run on a unit.
#
#   tools/scope-binaries.sh [BASE]     BASE: a commit; default: where this branch left origin/main
#
# Both are built the same way: release for aarch64-unknown-linux-musl, without the release's slow link-time
# optimisation (the same sources give the same programs either way), and with the folder's path replaced by /oos.
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
base=${1:-}
if [ -z "$base" ]; then
	base=$(git merge-base origin/main HEAD 2>/dev/null) || stop "no origin/main to compare with: name a commit"
fi
base=$(git rev-parse --verify --quiet "$base^{commit}") || stop "not a commit: ${1:-}"
here=$(git rev-parse HEAD)
back=$(git symbolic-ref --quiet --short HEAD || echo "$here")
target=aarch64-unknown-linux-musl
export CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
sums() { # sums NAME: build both programs, print their sums
	for c in ui fpgad; do
		(cd "$c" && cargo build --release --locked --quiet --target "$target" \
			--config "target.$target.rustflags=[\"--remap-path-prefix=$root=/oos\"]") || return 1
	done
	shasum -a 256 "ui/target/$target/release/oos-ui" "fpgad/target/$target/release/oos-fpgad" |
		awk -v n="$1" '{ sub(".*/", "", $2); print n, $2, $1 }'
}
work=$(mktemp -d)
trap 'git checkout -q "$back" 2>/dev/null; rm -rf "$work"' EXIT
sums this >"$work/this" || stop "this commit does not build for the scope"
git checkout -q --detach "$base" || stop "cannot check out $base"
sums base >"$work/base" || stop "the base does not build for the scope"
git checkout -q "$back" || stop "cannot go back to $back"
cat "$work/base" "$work/this"
if [ "$(cut -d' ' -f2- "$work/base")" = "$(cut -d' ' -f2- "$work/this")" ]; then
	echo "the programs for the scope are byte-identical to those of $(git log -1 --format='%h %s' "$base")"
	exit 0
fi
echo "the programs for the scope differ from those of $(git log -1 --format='%h' "$base")"
exit 1
