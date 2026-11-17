# Copyright (C) 2026 OOS contributors
# SPDX-License-Identifier: GPL-2.0-or-later
# unit-hook-dtb.sh - the fork's DTB step of boot/build-dev-image.sh inside the initramfs container, sourced from
# /wlan/unit-hook/dtb.sh right after the image's DTB is written (docs/fork/unit-dho804.md, docs/network.md 8).
#
# A unit whose `eth.mac` is a fixed address (rootfs/unit/<name>/system.conf, resolved by boot/unit-hook.sh into
# dtb.mac) gets it as `local-mac-address` of the Ethernet node. The RK3399 GMAC driver then takes it at probe
# (stmmac_probe_config_dt -> of_get_mac_address), before its fallback to a random address (the vendor storage read
# fails under OOS), so the address is the device's permanent one. IPv6 stable-privacy addresses hash the permanent
# address (addrconf: dev->perm_addr), which `ip link set address` (oos-mac) does not change: with a random one at
# probe the link-local and global addresses changed at every start (unit 2026-11-13, round 7).
#   unit_dtb_mac DTB MACFILE   patch DTB in place; nothing without MACFILE; fails when the read-back differs
unit_dtb_mac() {
	[ -f "$2" ] || return 0
	mac=$(cat "$2")
	node=$(fdtget -t s "$1" /aliases ethernet0) || { echo "unit-hook-dtb: no ethernet0 alias in $1" >&2; return 1; }
	fdtput -t bx "$1" "$node" local-mac-address $(echo "$mac" | tr ':' ' ') || return 1
	got=$(fdtget -t bx "$1" "$node" local-mac-address | tr ' ' '\n' | sed 's/^.$/0&/' | tr '\n' ':' | sed 's/:$//')
	want=$(echo "$mac" | tr 'A-F' 'a-f')
	[ "$got" = "$want" ] || { echo "unit-hook-dtb: $node local-mac-address reads $got, not $want" >&2; return 1; }
	echo "unit-hook-dtb: $node local-mac-address set from eth.mac"
}
# sourced under `sh -e`: the last status is the script's, so no bare `&&` at the end
if [ -n "${OOS_IMAGE_NAME:-}" ] && [ -d /wlan/unit-hook ]; then
	unit_dtb_mac "/out/$OOS_IMAGE_NAME.dtb" /wlan/unit-hook/dtb.mac || exit 1
fi
