# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# unit-hook-image.sh - the fork's part of boot/build-dev-image.sh inside the initramfs container, sourced once from
# /wlan/unit-hook/image.sh (staged by boot/unit-hook.sh) when the files of the image are in $r and before the DTB.
d=/wlan/unit-hook
[ -f $d/build.env ] && . $d/build.env
# the LAN-only firewall (net.allow, docs/network.md 6)
cp /wlan/nft $r/bin/
cp /wlan/nft.COPYING $r/etc/licenses/nftables
# the SSH server, started by rcS through oos-ssh (docs/network.md 7)
cp /wlan/dropbearmulti $r/bin/
for p in dropbear dropbearkey scp; do ln -sf dropbearmulti $r/bin/$p; done
cp /wlan/dropbear.COPYING $r/etc/licenses/dropbear
# the unit defaults: /etc/oos/limits.conf (capabilities), /etc/oos/system.conf (network); none without OOS_UNIT
if [ -n "$(ls -A $d/etc-oos)" ]; then mkdir -p $r/etc/oos; cp $d/etc-oos/*.conf $r/etc/oos/; ls -l $r/etc/oos; fi
# extra kernel arguments of the unit (build.env), e.g. pci=pcie_bus_perf for MaxPayload 256 (docs/fork/unit-dho804.md)
OOS_BOOTARGS="$OOS_BOOTARGS${OOS_BOOTARGS_EXTRA:+ $OOS_BOOTARGS_EXTRA}"
