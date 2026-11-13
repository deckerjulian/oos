#!/bin/sh
# Copyright (C) 2026 OOS contributors
# SPDX-License-Identifier: GPL-2.0-or-later
# build-nft.sh - a static nft for the image (docs/network.md 6, the LAN-only firewall), in Docker, into rootfs/out/.
# boot/build-dev-image.sh runs it when it is missing. libmnl, libnftnl and nftables from netfilter.org, no JSON,
# no readline, mini-gmp. The checksums are those of the tarballs Alpine 3.22 builds its packages from.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out="$here/out"
mkdir -p "$out"
fetch() { # name version suffix sha256
	t="$out/$1-$2.tar.$3"
	[ -f "$t" ] || curl -fsSL -o "$t" "https://www.netfilter.org/projects/$1/files/$1-$2.tar.$3"
	echo "$4  $t" | { shasum -a 256 -c - 2>/dev/null || sha256sum -c -; } >/dev/null || { echo "wrong checksum: $t" >&2; rm -f "$t"; exit 1; }
}
fetch libmnl 1.0.5 bz2 274b9b919ef3152bfb3da3a13c950dd60d6e2bcd54230ffeca298d03b40d0525
fetch libnftnl 1.2.9 xz e8c216255e129f26270639fee7775265665a31b11aa920253c3e5d5d62dfc4b8
fetch nftables 1.1.3 xz 9c8a64b59c90b0825e540a9b8fcb9d2d942c636f81ba50199f068fde44f34ed8
# arm64 containers: on an x86-64 host through QEMU (boot/build-dev-image.sh says how); the results belong to the caller
docker run --rm --platform linux/arm64 -e OUT_OWNER="$(id -u):$(id -g)" -v "$out:/out" alpine:3.22 sh -euc '
	apk add --no-cache build-base linux-headers pkgconf bison flex xz >/dev/null
	p=/tmp/prefix; mkdir -p $p
	export PKG_CONFIG_PATH=$p/lib/pkgconfig CFLAGS="-Os -ffunction-sections -fdata-sections"
	cd /tmp
	tar -xjf /out/libmnl-1.0.5.tar.bz2 && (cd libmnl-1.0.5 && ./configure --prefix=$p --enable-static --disable-shared >/dev/null && make -j"$(nproc)" install >/dev/null)
	tar -xJf /out/libnftnl-1.2.9.tar.xz && (cd libnftnl-1.2.9 && ./configure --prefix=$p --enable-static --disable-shared >/dev/null && make -j"$(nproc)" install >/dev/null)
	tar -xJf /out/nftables-1.1.3.tar.xz && cd nftables-1.1.3
	./configure --prefix=$p --enable-static --disable-shared --with-mini-gmp --without-cli --without-json --without-xtables --disable-man-doc >/dev/null
	make -j"$(nproc)" LDFLAGS="-all-static -Wl,--gc-sections" >/tmp/build.log 2>&1 || { tail -30 /tmp/build.log; exit 1; }
	strip -o /out/nft src/nft
	cp COPYING /out/nft.COPYING
	chown "$OUT_OWNER" /out/nft /out/nft.COPYING
'
file "$out/nft" 2>/dev/null || true
ls -la "$out/nft"
