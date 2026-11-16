#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# release-sources.sh - the source files a release carries next to its image (THIRD-PARTY-LICENSES.md), and whether a
# release on GitHub has them. Read only; `check` needs `gh` and the network.
#   tools/release-sources.sh list          the file names and patterns, one per line
#   tools/release-sources.sh check v0.1.13 exit 1 and the missing ones when the release lacks a file
# The names come from rootfs/deps.lock (the archives the image builds from) and kernel/base.sha256 (the kernel
# base). The packages taken from Ubuntu (busybox-static, and glibc, GCC's runtime, libdrm, zlib with the GPU helper)
# go with their source packages, whose versions follow the container image: those are patterns.
# The list is the one of this tree: check an older release from the tree at its tag.
set -eu
# the names below are patterns: no file name expansion against the current directory
set -f
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
# busybox-static 1:1.37.0-7ubuntu1 says "Built-Using: glibc (= 2.42-2ubuntu4)"; follows when the package changes
busybox_glibc=glibc_2.42-2ubuntu4

names() {
    # the kernel base archive
    awk '{ print $2 }' "$root/kernel/base.sha256" | sed 's|.*/||'
    # every pinned archive; Mesa's under either name: up to 0.1.13 it came from Ubuntu's source package, whose archive is
    # upstream's renamed
    grep -v '^#' "$root/rootfs/deps.lock" | while read -r name _ ver _ url _; do
        [ -n "$name" ] || continue
        case "$name" in
        mesa) echo "mesa_$ver.orig.tar.*|mesa-$ver.tar.*" ;;
        *) echo "${url##*/}" ;;
        esac
    done
    # the glibc inside Ubuntu's busybox-static: the one that package was built with (its Built-Using, which
    # boot/out/busybox.version of a build names), not the one of the container
    for f in "$busybox_glibc.dsc" "${busybox_glibc%%-*}.orig.tar.*" "$busybox_glibc.debian.tar.*"; do
        echo "$f"
    done
    # Ubuntu source packages: description, upstream archive, Ubuntu's changes
    for p in busybox glibc gcc-[0-9]* libdrm zlib; do
        echo "${p}_*.dsc"
        echo "${p}_*.orig.tar.*"
        echo "${p}_*.debian.tar.*"
    done
    echo SHA256SUMS
}

# has LIST PATTERN: a name of LIST matches PATTERN (alternatives divided by |)
has() {
    old_ifs=$IFS
    IFS='|'
    for alt in $2; do
        IFS=$old_ifs
        for a in $1; do
            # shellcheck disable=SC2254
            case "$a" in $alt) return 0 ;; esac
        done
        IFS='|'
    done
    IFS=$old_ifs
    return 1
}

case "${1:-}" in
list)
    names
    ;;
check)
    tag=${2:?usage: release-sources.sh check <tag>}
    assets=$(gh release view "$tag" --json assets --jq '.assets[].name')
    [ -n "$assets" ] || { echo "release-sources.sh: no files in release $tag" >&2; exit 1; }
    missing=0
    for n in $(names); do
        has "$assets" "$n" || { echo "missing in $tag: $n"; missing=1; }
    done
    [ "$missing" = 0 ] && echo "$tag: every source file is there"
    exit "$missing"
    ;;
*)
    sed -n '4,11p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
    ;;
esac
