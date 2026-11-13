#!/bin/sh
# Copyright (C) 2026 OOS contributors
# SPDX-License-Identifier: GPL-2.0-or-later
# build-nft.sh - a static nft for the image (docs/network.md 6, the LAN-only firewall), in Docker, into rootfs/out/.
# boot/unit-hook.sh runs it when it is missing or out of date (deps.sh). libmnl, libnftnl and nftables from
# netfilter.org, no JSON, no readline, mini-gmp. Versions and SHA-256: rootfs/deps.lock (the checksums of the first
# versions were those of the tarballs Alpine 3.22 builds its packages from).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out="$here/out"
. "$here/deps.sh"
for d in libmnl libnftnl nftables; do dep_fetch $d "$out" >/dev/null; done
# arm64 containers: on an x86-64 host through QEMU (boot/build-dev-image.sh says how); the results belong to the caller
docker run --rm --platform linux/arm64 -e OUT_OWNER="$(id -u):$(id -g)" -e MNL="$(dep_ver libmnl)" -e NFTNL="$(dep_ver libnftnl)" \
	-e NFT="$(dep_ver nftables)" -v "$out:/out" alpine:3.22 sh -euc '
	apk add --no-cache build-base linux-headers pkgconf bison flex xz >/dev/null
	p=/tmp/prefix; mkdir -p $p
	export PKG_CONFIG_PATH=$p/lib/pkgconfig CFLAGS="-Os -ffunction-sections -fdata-sections"
	cd /tmp
	tar -xjf /out/libmnl-$MNL.tar.bz2 && (cd libmnl-$MNL && ./configure --prefix=$p --enable-static --disable-shared >/dev/null && make -j"$(nproc)" install >/dev/null)
	tar -xJf /out/libnftnl-$NFTNL.tar.xz && (cd libnftnl-$NFTNL && ./configure --prefix=$p --enable-static --disable-shared >/dev/null && make -j"$(nproc)" install >/dev/null)
	tar -xJf /out/nftables-$NFT.tar.xz && cd nftables-$NFT
	./configure --prefix=$p --enable-static --disable-shared --with-mini-gmp --without-cli --without-json --without-xtables --disable-man-doc >/dev/null
	make -j"$(nproc)" LDFLAGS="-all-static -Wl,--gc-sections" >/tmp/build.log 2>&1 || { tail -30 /tmp/build.log; exit 1; }
	strip -o /out/nft src/nft
	cp COPYING /out/nft.COPYING
	chown "$OUT_OWNER" /out/nft /out/nft.COPYING
'
dep_stamp "$out/nft" "$0" libmnl libnftnl nftables
file "$out/nft" 2>/dev/null || true
ls -la "$out/nft"
