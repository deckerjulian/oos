#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# build-dev-image.sh - the first OOS development boot image for `recovery` (Phase 2).
#
#   boot/build-dev-image.sh            -> boot/out/oos-dev.img (+ .sha256)
#
# Contents: the kernel and DHO900 DTB from kernel/out/base (kernel/build-base.sh), the DTB's bootargs
# changed to log on the panel (tty0) and UART2, and an initramfs from rootfs/initramfs with a static busybox and the
# modules for later FPGA bring-up (pcierockchiphost, xdma), the touch panel (edt-ft5x06), the beeper (pwm-gpio) and WLAN (rtl8xxxu),
# wpa_supplicant, oos-hw and the scope screen oos-ui (started by etc/rcS).
# The image only exists as a file; installing it is a separate, explicit step.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
k="$root/kernel/out/base"
out="$here/out"
[ -f "$k/Image" ] || { echo "run kernel/build-base.sh first" >&2; exit 1; }
# the initramfs is built in Docker: fail now, not after the Rust builds (a failed late step kept the OLD image)
docker info >/dev/null 2>&1 || { echo "Docker is not running (macOS: open -a Docker)" >&2; exit 1; }
ui="$root/ui/target/aarch64-unknown-linux-musl/release"
# always rebuild (incremental), so the image never carries a stale binary
(cd "$root/ui" && PATH="$HOME/.cargo/bin:$PATH" cargo build --release --target aarch64-unknown-linux-musl) || exit 1
fpgad="$root/fpgad/target/aarch64-unknown-linux-musl/release"
(cd "$root/fpgad" && PATH="$HOME/.cargo/bin:$PATH" cargo build --release --target aarch64-unknown-linux-musl) || exit 1
[ -x "$root/tools/hw/oos-hw" ] || docker run --rm -v "$root/tools/hw:/src" oos-kbuild:ubuntu-gcc15 sh -c "cd /src && gcc -static -O2 -Wall -o oos-hw oos-hw.c"
mkdir -p "$out"
image=oos-initramfs:ubuntu
docker image inspect "$image" >/dev/null 2>&1 || docker build -t "$image" - <<'EOF'
FROM ubuntu:26.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      busybox-static cpio gzip device-tree-compiler && rm -rf /var/lib/apt/lists/*
EOF

# WLAN: wpa_supplicant and wpa_cli (docs/wlan.md)
[ -x "$root/rootfs/out/wpa_supplicant" ] && [ -x "$root/rootfs/out/wpa_cli" ] || "$root/rootfs/build-wlan-tools.sh"
# /oos is checked before it is mounted
[ -x "$root/rootfs/out/e2fsck" ] || "$root/rootfs/build-fsck.sh"

build_id="$(date -u +%Y%m%d-%H%M%S) kernel $(cat "$k/kernel.release")"
docker run --rm -e OOS_NO_AUTOACQ="${OOS_NO_AUTOACQ:-}" -v "$root/rootfs/initramfs:/src:ro" -v "$root/rootfs/out:/wlan:ro" -v "$root/tools/hw:/hw:ro" -v "$ui:/ui:ro" -v "$fpgad:/fpgad:ro" -v "$k:/k:ro" -v "$out:/out" -e BUILD_ID="$build_id" "$image" sh -euc '
	r=/tmp/root; rm -rf $r; mkdir -p $r
	cp -a /src/. $r/
	mkdir -p $r/bin $r/sbin $r/etc $r/proc $r/sys $r/dev $r/tmp $r/run $r/mnt $r/root $r/lib/modules
	cp /bin/busybox $r/bin/busybox
	cp /hw/oos-hw $r/bin/oos-hw
	cp /ui/oos-ui $r/bin/oos-ui
	cp /fpgad/oos-fpgad $r/bin/oos-fpgad
	cp /fpgad/oos-update $r/bin/oos-update
	cp /wlan/wpa_supplicant /wlan/wpa_cli /wlan/e2fsck $r/bin/
	mkdir -p $r/etc/licenses; cp /wlan/wpa_supplicant.COPYING $r/etc/licenses/wpa_supplicant; cp /wlan/e2fsck.COPYING $r/etc/licenses/e2fsck
	ln -sf busybox $r/bin/sh
	echo "$BUILD_ID" > $r/etc/oos-build
	# rcS brings the acquisition hardware up at boot (cold boot + oos-ui --hw --afe); OOS_NO_AUTOACQ=1 leaves it out
	[ -n "$OOS_NO_AUTOACQ" ] || touch $r/etc/oos-autoacq
	mkdir -p /tmp/m && tar -C /tmp/m -xzf /k/modules.tar.gz
	rel=$(cat /k/kernel.release); M=/tmp/m/lib/modules/$rel
	mkdir -p $r/lib/modules/$rel
	for f in kernel/drivers/pci/controller/pcierockchiphost.ko updates/xdma.ko \
	         kernel/lib/crypto/libarc4.ko kernel/net/wireless/cfg80211.ko kernel/net/mac80211/mac80211.ko \
	         kernel/drivers/net/wireless/realtek/rtl8xxxu/rtl8xxxu.ko \
	         $(cd $M && find . -name "edt-ft5x06.ko*" -o -name "pwm-gpio.ko*" | sed "s|^\./||"); do
		mkdir -p $r/lib/modules/$rel/$(dirname $f); cp $M/$f $r/lib/modules/$rel/$f
	done
	cp $M/modules.dep $M/modules.alias $M/modules.order $M/modules.builtin* $r/lib/modules/$rel/ 2>/dev/null || true
	# the console node the kernel opens before /init mounts devtmpfs
	mknod -m 600 $r/dev/console c 5 1
	(cd $r && find . | sort | cpio -o -H newc -R 0:0 --quiet) > /tmp/root.cpio
	gzip -9n < /tmp/root.cpio > /out/initramfs.cpio.gz
	cp /k/rk3399-rigol-dho900.dtb /out/oos-dho900.dtb
	fdtput -t s /out/oos-dho900.dtb /chosen bootargs \
		"earlycon=uart8250,mmio32,0xff1a0000 console=tty0 console=ttyS2,115200n8 loglevel=7 rdinit=/init blkdevparts=mmcblk0:16M@252M(oosdata)"
	fdtget /out/oos-dho900.dtb /chosen bootargs
	ls -la /out
'
python3 -I "$here/oos-bootimg.py" build --kernel "$k/Image" --dtb "$out/oos-dho900.dtb" \
	--ramdisk "$out/initramfs.cpio.gz" --name oos-dho900 --out "$out/oos-dev.img"
(cd "$out" && shasum -a 256 oos-dev.img > oos-dev.img.sha256 && cat oos-dev.img.sha256)
# signed update package (docs/update.md): install on a running OOS through http://<scope>:8080/update
# your own key if you have one, else the community key every OOS build trusts
key="${OOS_UPDATE_KEY:-$HOME/.config/oos/update-signing.key}"
[ -f "$key" ] || key="$root/keys/community-signing.key"
rm -f "$out/oos-dev.oosupd"
if [ -f "$key" ]; then
	(cd "$root/fpgad" && PATH="$HOME/.cargo/bin:$PATH" cargo build --release --bin oos-update) || exit 1
	"$root/fpgad/target/release/oos-update" pack --image "$out/oos-dev.img" --build "$build_id" --key "$key" --out "$out/oos-dev.oosupd"
else
	echo "no signing key at $key: no update package (oos-update keygen, docs/update.md)"
fi
# the same image as an update file of the stock system, for an install without opening the scope (docs/install-usb.md)
[ -f "$out/oos-dev.oosupd" ] && python3 -I "$here/build-gel.py"
