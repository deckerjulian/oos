#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# Static key-only Dropbear for the initramfs. Does not install or start a service.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out="$here/out"
ver=2026.94
sum=e098034a843699200c8c977a991fff73159735bf795d5f72ef672c41a6b1ae81
mkdir -p "$out"
tarball="$out/dropbear-$ver.tar.bz2"
if [ ! -f "$tarball" ]; then
	curl -fsSL -o "$tarball.part" "https://matt.ucc.asn.au/dropbear/releases/dropbear-$ver.tar.bz2" ||
		curl -fsSL -o "$tarball.part" "https://dropbear.nl/mirror/releases/dropbear-$ver.tar.bz2"
	mv "$tarball.part" "$tarball"
fi
echo "$sum  $tarball" | { shasum -a 256 -c - 2>/dev/null || sha256sum -c -; } >/dev/null || {
	echo "wrong checksum: $tarball (remove it and retry)" >&2; exit 1;
}
command -v docker >/dev/null 2>&1 || { echo 'Docker is required to build static arm64 SSH tools (no service was started)' >&2; exit 1; }
# Same arm64 Docker/QEMU build path as WLAN and fsck. No host/private keys enter the image.
docker run --rm --platform linux/arm64 -e OUT_OWNER="$(id -u):$(id -g)" -e VER="$ver" -v "$out:/out" alpine:3.22 sh -euc '
	apk add --no-cache build-base linux-headers >/dev/null
	cd /tmp; tar -xjf /out/dropbear-$VER.tar.bz2; cd dropbear-$VER
	cat > localoptions.h <<CFG
#define DROPBEAR_SVR_PASSWORD_AUTH 0
#define DROPBEAR_SVR_PAM_AUTH 0
#define DROPBEAR_SVR_PUBKEY_AUTH 1
#define DROPBEAR_SFTPSERVER 0
#define DROPBEAR_X11FWD 0
#define DROPBEAR_SVR_AGENTFWD 0
#define DROPBEAR_SVR_LOCALTCPFWD 0
#define DROPBEAR_SVR_REMOTETCPFWD 0
#define DROPBEAR_SVR_LOCALSTREAMFWD 0
#define DROPBEAR_SVR_REMOTESTREAMFWD 0
CFG
	./configure --enable-static --disable-zlib --disable-syslog --disable-lastlog \
		--disable-utmp --disable-utmpx --disable-wtmp --disable-wtmpx CFLAGS=-Os >/tmp/conf.log 2>&1 || { tail -20 /tmp/conf.log; exit 1; }
	make -j"$(nproc)" PROGRAMS="dropbear dropbearkey" >/tmp/build.log 2>&1 || { tail -30 /tmp/build.log; exit 1; }
	strip dropbear dropbearkey
	cp dropbear dropbearkey /out/
	cp LICENSE /out/dropbear.COPYING
	cp libtomcrypt/LICENSE /out/dropbear-libtomcrypt.COPYING
	cp libtommath/LICENSE /out/dropbear-libtommath.COPYING
	printf "%s\n" "$VER key-only, no forwarding" > /out/dropbear.version
	chown "$OUT_OWNER" /out/dropbear /out/dropbearkey /out/dropbear*.COPYING /out/dropbear.version
'
ls -la "$out/dropbear" "$out/dropbearkey"
