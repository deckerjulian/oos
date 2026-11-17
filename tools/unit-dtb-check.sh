#!/bin/sh
# Copyright (C) 2026 OOS contributors
# SPDX-License-Identifier: GPL-2.0-or-later
# unit-dtb-check.sh - tests the unit's DTB step (boot/unit-hook-dtb.sh) and how boot/unit-hook.sh resolves `eth.mac`
# (through oos-mac), without a scope: the image's build container patches a copy of each base DTB
# (kernel/out/base, OOS_KBASE names another) with a test address, reads it back, and checks that `cid`, `vendor` and
# addresses oos-mac refuses give no MAC file. Uses example addresses only.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
kbase="${OOS_KBASE:-$root/kernel/out/base}"
w="${TMPDIR:-/tmp}/oos-unit-dtb"
rm -rf "$w"; mkdir -p "$w"
ls "$kbase"/*.dtb >/dev/null 2>&1 || { echo "no DTB in $kbase (kernel/build-base.sh)" >&2; exit 1; }
ok=1
fail() { echo "FAIL: $*"; ok=0; }
# the host side: which eth.mac values become a DTB address
resolve() {
	printf 'eth.mac = %s\n' "$1" > "$w/system.conf"
	OOS_CONF_FILES="$w/system.conf" OOS_CID_FILE=/nonexistent sh "$root/rootfs/initramfs/bin/oos-mac" 2>/dev/null || true
}
[ "$(resolve 02:11:22:33:44:5A)" = 02:11:22:33:44:5A ] || fail "a fixed address"
for v in cid vendor 01:11:22:33:44:55 00:00:00:00:00:00 02:11:22:33:44 junk; do
	[ -z "$(resolve "$v")" ] || fail "eth.mac = $v gave $(resolve "$v")"
done
echo 02:11:22:33:44:5A > "$w/dtb.mac"
cp "$kbase"/*.dtb "$w/"
docker run --rm --platform linux/arm64 -v "$w:/w" -v "$root/boot:/b:ro" oos-initramfs:ubuntu sh -euc '
	. /b/unit-hook-dtb.sh
	for d in /w/*.dtb; do
		cp $d $d.orig
		unit_dtb_mac $d /w/dtb.mac
		[ "$(fdtget -t bx $d /ethernet@fe300000 local-mac-address)" = "2 11 22 33 44 5a" ] || { echo "FAIL: $d read-back"; exit 1; }
		# no MAC file: the DTB stays as it was
		cp $d.orig $d.none
		unit_dtb_mac $d.none /w/none.mac
		cmp -s $d.orig $d.none || { echo "FAIL: $d changed without a MAC file"; exit 1; }
	done
	echo "dtb: patched and read back"
' || ok=0
[ $ok = 1 ] && echo "unit-dtb-check: all passed" || { echo "unit-dtb-check: FAILED"; exit 1; }
