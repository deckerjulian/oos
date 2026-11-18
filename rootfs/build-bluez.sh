#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# build-bluez.sh - static dbus-daemon, bluetoothd and btmon for the image (docs/bluetooth.md), in Docker, into
# rootfs/out/. boot/build-dev-image.sh runs it when they are missing or out of date (deps.sh). Only what mice and
# keyboards need: the system bus, the core daemon with the HID (classic) and HoG (LE) input profiles, the monitor for
# debugging. No command line client (oos-ui has its own, hal/src/bluez.rs; bluetoothctl would bring readline), no
# audio, no network profiles, no OBEX, no mesh, no udev, no systemd.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out="$here/out"
. "$here/deps.sh"
. "$here/../tools/lib/docker.sh"
dbus=$(dep_ver dbus)
bluez=$(dep_ver bluez)
dep_fetch dbus "$out" >/dev/null
dep_fetch bluez "$out" >/dev/null
# expat (dbus-daemon) and glib with pcre2 and libintl (bluetoothd) come from Alpine's static packages and are linked
# in (THIRD-PARTY-LICENSES.md): a release carries the files Alpine builds them from, which are fetched here, and the
# build stops below when the package Alpine installs is another version (then rootfs/deps.lock follows).
pkgs=""
for d in expat glib glib-patch-1 glib-patch-2 pcre2 gettext gettext-patch-1 gettext-patch-2 gettext-patch-3; do
	dep_fetch "$d" "$out" >/dev/null
done
for d in expat glib pcre2 gettext; do
	pkgs="$pkgs $d-static=$(dep_ver "$d")"
done
# arm64 containers: on an x86-64 host through QEMU (boot/build-dev-image.sh says how); the results belong to the caller
docker run --rm --platform linux/arm64 -e OUT_OWNER="$(id -u):$(id -g)" -e DBUS="$dbus" -e BLUEZ="$bluez" \
	-e PKGS="$pkgs" -e EXPAT="$(dep_ver expat)" -e PCRE2="$(dep_ver pcre2)" -v "$out:/out" "$OOS_ALPINE" sh -euc '
	apk add --no-cache build-base linux-headers pkgconf meson samurai python3 xz file \
		expat-dev expat-static glib-dev glib-static pcre2-static gettext-static >/dev/null
	for p in $PKGS; do
		apk list -I "${p%%=*}" 2>/dev/null | grep -q "^${p%%=*}-${p#*=}-r" ||
			{ echo "${p%%=*} of this Alpine is not ${p#*=} (rootfs/build-bluez.sh): $(apk list -I "${p%%=*}")" >&2; exit 1; }
	done
	export CFLAGS="-Os -ffunction-sections -fdata-sections" LDFLAGS="-static -Wl,--gc-sections"
	cd /tmp && tar -xJf /out/dbus-$DBUS.tar.xz && cd dbus-$DBUS
	meson setup build --prefix=/usr --sysconfdir=/etc --localstatedir=/var -Druntime_dir=/run \
		--default-library=static --prefer-static -Dc_link_args=-static \
		-Dsystemd=disabled -Dx11_autolaunch=disabled -Dselinux=disabled -Dapparmor=disabled -Dlibaudit=disabled \
		-Dmodular_tests=disabled -Ddoxygen_docs=disabled -Dxml_docs=disabled -Dducktype_docs=disabled \
		-Dqt_help=disabled -Dlaunchd=disabled -Duser_session=false -Ddbus_user=root \
		-Dsystem_socket=/run/dbus/system_bus_socket >/tmp/dbus.log 2>&1 || { tail -30 /tmp/dbus.log; exit 1; }
	meson compile -C build >>/tmp/dbus.log 2>&1 && meson install -C build >>/tmp/dbus.log 2>&1 || { tail -30 /tmp/dbus.log; exit 1; }
	cd /tmp && tar -xJf /out/bluez-$BLUEZ.tar.xz && cd bluez-$BLUEZ
	PKG_CONFIG="pkg-config --static" ./configure --prefix=/usr --sysconfdir=/etc --localstatedir=/var \
		--disable-systemd --disable-udev --disable-cups --disable-obex --disable-mesh --disable-midi \
		--disable-manpages --disable-a2dp --disable-avrcp --disable-network --disable-bap --disable-bass \
		--disable-mcp --disable-vcp --disable-micp --disable-ccp --disable-csip --disable-asha \
		--disable-client --enable-monitor >/tmp/bluez.log 2>&1 || { tail -30 /tmp/bluez.log; exit 1; }
	# libtool links dynamically unless it is told -all-static
	make -j"$(nproc)" LDFLAGS="-all-static -Wl,--gc-sections" >>/tmp/bluez.log 2>&1 ||
		{ tail -30 /tmp/bluez.log; exit 1; }
	for b in /usr/bin/dbus-daemon src/bluetoothd monitor/btmon; do
		file "$b" | grep -q "statically linked" || { echo "not static: $b" >&2; exit 1; }
		strip -o /out/$(basename "$b") "$b"
	done
	cp COPYING /out/bluez.COPYING
	cp /tmp/dbus-$DBUS/COPYING /out/dbus.COPYING
	# the licence texts of the linked libraries that LGPL-2.1.txt does not cover
	tar -xJOf /out/expat-$EXPAT.tar.xz expat-$EXPAT/COPYING > /out/expat.COPYING
	tar -xjOf /out/pcre2-$PCRE2.tar.bz2 pcre2-$PCRE2/LICENCE.md > /out/pcre2.COPYING
	chown "$OUT_OWNER" /out/dbus-daemon /out/bluetoothd /out/btmon /out/*.COPYING
	ls -l /out/dbus-daemon /out/bluetoothd /out/btmon
'
dep_stamp "$out/bluetoothd" dbus bluez expat glib glib-patch-1 glib-patch-2 pcre2 gettext gettext-patch-1 gettext-patch-2 \
	gettext-patch-3 "$here/build-bluez.sh"
