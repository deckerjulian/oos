#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# scope-binaries.sh - whether a change changes the programs that run on the scope at all (docs/adr/process/0041):
# builds the programs of the image (oos-ui, oos-fpgad, oos-update) for the scope from this commit and from BASE, in
# this same folder one after the other, and compares them byte for byte. Equal programs need no run on a unit.
#
#   tools/scope-binaries.sh [BASE]     BASE: a commit; default: where this branch left origin/main
#
# Both are built as boot/build-dev-image.sh builds them (release for aarch64-unknown-linux-musl, the crates' own
# profile and target flags, the decoders of ui/tools/decoder-features.sh), with the folder's path replaced by /oos.
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
# the decoders the image has: as the image build asks for them, whatever this shell has set
unset OOS_DECODERS OOS_DECODERS_OFF
sums() { # sums NAME: build the programs, print their sums
	# shellcheck disable=SC2046
	(cd ui && cargo build --release --locked --quiet --target "$target" $(tools/decoder-features.sh) \
		--config "target.$target.rustflags=[\"--remap-path-prefix=$root=/oos\"]") || return 1
	# the remap is added to the crate's own target flags (.cargo/config.toml), not put in their place
	(cd fpgad && cargo build --release --locked --quiet --target "$target" \
		--config "target.$target.rustflags=[\"--remap-path-prefix=$root=/oos\"]") || return 1
	shasum -a 256 "ui/target/$target/release/oos-ui" "fpgad/target/$target/release/oos-fpgad" \
		"fpgad/target/$target/release/oos-update" |
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
