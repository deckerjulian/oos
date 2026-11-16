#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# build-wlan-tools.sh - static wpa_supplicant and wpa_cli for the image (docs/wlan.md), in Docker, into
# rootfs/out/. boot/build-dev-image.sh runs it when they are missing or out of date (deps.sh). Only what WPA/WPA2
# personal needs: nl80211, the control socket, the internal crypto. No EAP, no SAE, no P2P.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out="$here/out"
. "$here/deps.sh"
. "$here/../tools/lib/docker.sh"
ver=$(dep_ver wpa_supplicant)
dep_fetch wpa_supplicant "$out" >/dev/null
# libnl comes from Alpine's libnl3-static and is linked in (LGPL-2.1-or-later): its source goes with a release
# (THIRD-PARTY-LICENSES.md). Alpine builds the package from these two files; the build stops below when the package
# Alpine installs is another version than the one fetched here (then rootfs/deps.lock follows).
libnl=$(dep_ver libnl)
dep_fetch libnl "$out" >/dev/null
dep_fetch libnl-musl-patch "$out" >/dev/null
# arm64 containers: on an x86-64 host through QEMU (boot/build-dev-image.sh says how); the results belong to the caller
docker run --rm --platform linux/arm64 -e OUT_OWNER="$(id -u):$(id -g)" -e VER="$ver" -e LIBNL="$libnl" -v "$out:/out" "$OOS_ALPINE" sh -euc '
	apk add --no-cache build-base linux-headers pkgconf libnl3-dev libnl3-static >/dev/null
	apk list -I libnl3-static 2>/dev/null | grep -q "^libnl3-static-$LIBNL-r" || { echo "libnl3-static of this Alpine is not $LIBNL (rootfs/deps.lock): $(apk list -I libnl3-static 2>/dev/null)" >&2; exit 1; }
	cd /tmp && tar -xzf /out/wpa_supplicant-$VER.tar.gz && cd wpa_supplicant-$VER/wpa_supplicant
	cat > .config <<CFG
CONFIG_DRIVER_NL80211=y
CONFIG_LIBNL32=y
CONFIG_CTRL_IFACE=y
CONFIG_BACKEND=file
CONFIG_TLS=internal
CONFIG_INTERNAL_LIBTOMMATH=y
CONFIG_NO_RANDOM_POOL=y
CONFIG_GETRANDOM=y
CONFIG_IEEE80211W=y
CONFIG_NO_CONFIG_BLOBS=y
CONFIG_DEBUG_FILE=y
CFG
	export LDFLAGS=-static CFLAGS="-Os -ffunction-sections -fdata-sections"
	make -j"$(nproc)" wpa_supplicant wpa_cli >/tmp/build.log 2>&1 || { tail -30 /tmp/build.log; exit 1; }
	strip wpa_supplicant wpa_cli
	cp wpa_supplicant wpa_cli /out/
	# the licence text itself stands in the README, up to the end of its disclaimer
	sed "/SUCH DAMAGE/q" ../README > /out/wpa_supplicant.COPYING
	chown "$OUT_OWNER" /out/*
'
dep_stamp "$out/wpa_supplicant" "$here/build-wlan-tools.sh" wpa_supplicant libnl libnl-musl-patch
ls -la "$out/wpa_supplicant" "$out/wpa_cli"
