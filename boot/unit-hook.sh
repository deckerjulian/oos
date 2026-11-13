# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# unit-hook.sh - the fork's part of boot/build-dev-image.sh on the host, sourced once before the initramfs container
# (docs/fork/unit-dho804.md). Kept out of upstream's script so a sync conflicts on two lines, not on every fork line
# (docs/fork/sync-checklist.md). Uses root, here, k from the script.
#
#   OOS_UNIT=dho804 boot/build-dev-image.sh   the image with the defaults of one unit in /etc/oos
#                                      (rootfs/unit/<name>/: limits.conf, system.conf, build.env)
#
# The container sees rootfs/out as /wlan, so the hook stages its part there: rootfs/out/unit-hook/ with image.sh
# (boot/unit-hook-image.sh), the unit's build.env and its *.conf under etc-oos/. Staged anew on every build.
stage="$root/rootfs/out/unit-hook"
rm -rf "$stage"
mkdir -p "$stage/etc-oos"
# defaults of one unit (docs/models.md 4.2, docs/network.md 6): rootfs/unit/<OOS_UNIT>/ becomes /etc/oos/ in the image
if [ -n "${OOS_UNIT:-}" ]; then
	unit="$root/rootfs/unit/$OOS_UNIT"
	[ -d "$unit" ] || { echo "no unit $unit" >&2; exit 1; }
	# build choices of the unit, e.g. OOS_KERNEL_DTB=rk3399-rigol-dho900-uhs.dtb (plain KEY=value lines)
	if [ -f "$unit/build.env" ]; then
		. "$unit/build.env"
		cp "$unit/build.env" "$stage/"
	fi
	for f in "$unit"/*.conf; do
		[ -f "$f" ] && cp "$f" "$stage/etc-oos/"
	done
fi
[ -f "$k/$OOS_KERNEL_DTB" ] || { echo "no $k/$OOS_KERNEL_DTB: run kernel/build-base.sh again" >&2; exit 1; }
# the LAN-only firewall (net.allow, docs/network.md 6)
[ -x "$root/rootfs/out/nft" ] || "$root/rootfs/build-nft.sh"
cp "$here/unit-hook-image.sh" "$stage/image.sh"
