#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# build-dev-image.sh - the first OOS development boot image for `recovery` (Phase 2).
#
#   boot/build-dev-image.sh            -> boot/out/oos-dev.img (+ .sha256)
#   OOS_DECODERS="usb dcc" / OOS_DECODERS_OFF="avionics" boot/build-dev-image.sh   fewer decoders in oos-ui
#   (ui/tools/decoder-features.sh; the seven core decoders are always in)
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
# the board this image is for: rootfs/board/<OOS_BOARD>, the values come from its etc/oos-board
board="$root/rootfs/board/${OOS_BOARD:-dho}"
[ -f "$board/etc/oos-board" ] || { echo "no board $board" >&2; exit 1; }
. "$board/etc/oos-board"
[ -f "$k/Image" ] || { echo "run kernel/build-base.sh first" >&2; exit 1; }
# Docker on Linux: be in the docker group, not root (under sudo, HOME and PATH are root's, rustup's cargo is gone, and
# the results would belong to root)
if [ "$(id -u)" = 0 ] && [ -n "${SUDO_USER:-}" ]; then
	echo "do not run this with sudo. For Docker without sudo: sudo usermod -aG docker $SUDO_USER, then log in again" >&2
	exit 1
fi
# Rust from rustup: the target's standard library and rust-lld come with it, not with most distribution packages
export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null 2>&1 || { echo "no cargo: install Rust from https://rustup.rs" >&2; exit 1; }
if command -v rustup >/dev/null 2>&1; then
	rustup target list --installed | grep -qx aarch64-unknown-linux-musl || { echo "missing Rust target: rustup target add aarch64-unknown-linux-musl" >&2; exit 1; }
else
	echo "no rustup: a cargo from the distribution usually lacks aarch64-unknown-linux-musl and rust-lld; Rust from https://rustup.rs has them" >&2
fi
# the initramfs is built in Docker: fail now, not after the Rust builds (a failed late step kept the OLD image). The
# image's busybox, wpa_supplicant and e2fsck come from arm64 containers.
. "$root/tools/lib/docker.sh"
need_docker_arm64
# the sources the image builds from: rootfs/deps.lock, and the stamps that say when a part is built again
DEPS_LOCK="$root/rootfs/deps.lock"
. "$root/rootfs/deps.sh"
sha256() { if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$@"; else sha256sum "$@"; fi; }
owner="$(id -u):$(id -g)"
ui="$root/ui/target/aarch64-unknown-linux-musl/release"
# always rebuild (incremental), so the image never carries a stale binary
feat=$("$root/ui/tools/decoder-features.sh") || exit 1
# the board's code (hal/src/target.rs ACTIVE): the DHO800/DHO900 without a feature, another board by its feature
bfeat=
[ "$OOS_BOARD" = dho ] || bfeat="--features board-$OOS_BOARD"
(cd "$root/ui" && cargo build --release --target aarch64-unknown-linux-musl $feat $bfeat) || exit 1
fpgad="$root/fpgad/target/aarch64-unknown-linux-musl/release"
(cd "$root/fpgad" && cargo build --release --target aarch64-unknown-linux-musl $bfeat) || exit 1
# oos-hw with the kernel's compiler (kernel/build-base.sh image), for arm64 whatever the host is; ELF e_machine 0xb7 is
# AArch64, a binary built for the host by an older script is built again
hw="$root/tools/hw/oos-hw"
if [ ! -x "$hw" ] || [ "$(od -An -tx1 -j18 -N1 "$hw" | tr -d ' ')" != b7 ]; then
	kimage=$("$root/kernel/build-base.sh" image)
	docker run --rm -e OUT_OWNER="$owner" -v "$root/tools/hw:/src" "$kimage" sh -euc 'cd /src && aarch64-linux-gnu-gcc -static -O2 -Wall -o oos-hw oos-hw.c && chown "$OUT_OWNER" oos-hw'
fi
mkdir -p "$out"
# arm64 whatever the host is: its busybox goes into the image (tools/lib/docker.sh)
image=$(initramfs_image)

# The programs built from rootfs/deps.lock are built again when missing, or when their deps.lock lines or their build
# script changed since (deps.sh stamps). WLAN: wpa_supplicant and wpa_cli (docs/wlan.md)
o="$root/rootfs/out"
if [ ! -x "$o/wpa_cli" ] || dep_stale "$o/wpa_supplicant" "$root/rootfs/build-wlan-tools.sh" wpa_supplicant libnl \
	libnl-musl-patch; then
	"$root/rootfs/build-wlan-tools.sh"
fi
# /oos is checked before it is mounted
if dep_stale "$o/e2fsck" "$root/rootfs/build-fsck.sh" e2fsprogs; then "$root/rootfs/build-fsck.sh"; fi
# SSH (Dropbear, built by boot/unit-hook.sh): trusted public keys can be built into the image
# (OOS_SSH_AUTHORIZED_KEYS); host keys and the password are made once on each scope, on /oos.
ssh_seed="$root/rootfs/out/ssh-seed"
mkdir -p "$ssh_seed"
rm -f "$ssh_seed/authorized_keys"
if [ -n "${OOS_SSH_AUTHORIZED_KEYS:-}" ]; then
	python3 -I "$root/tools/ssh-keys.py" validate "$OOS_SSH_AUTHORIZED_KEYS" --out "$ssh_seed/authorized_keys"
fi
# an update with another device tree is tried by kexec_load with that tree (docs/update.md)
if dep_stale "$o/kexec" "$root/rootfs/build-kexec.sh" kexec-tools; then "$root/rootfs/build-kexec.sh"; fi
# System > Graphics = GPU: the helper oos-gpu and Mesa (gpu/README.md), again when gpu/build.sh, oos-gpu.c or the Mesa
# line of rootfs/deps.lock changed since (deps.sh stamps; gpu/build.sh keeps Mesa itself in rootfs/out/mesa)
if dep_stale "$o/gpu/bin/oos-gpu" "$root/gpu/build.sh" "$root/gpu/oos-gpu.c" mesa; then
	"$root/gpu/build.sh"
fi

# the unit profile (OOS_UNIT, docs/models.md 4.2), the firewall and the SSH server (boot/unit-hook.sh)
. "$here/unit-hook.sh"
build_id="$(date -u +%Y%m%d-%H%M%S) kernel $(cat "$k/kernel.release")"
docker run --rm --platform linux/arm64 -e OUT_OWNER="$owner" -e OOS_NO_AUTOACQ="${OOS_NO_AUTOACQ:-}" -v "$root/rootfs/initramfs:/src:ro" -v "$board:/board:ro" -v "$root/rootfs/out:/wlan:ro" -v "$root/tools/hw:/hw:ro" -v "$ui:/ui:ro" -v "$fpgad:/fpgad:ro" -v "$k:/k:ro" -v "$out:/out" -e BUILD_ID="$build_id" "$image" sh -euc '
	r=/tmp/root; rm -rf $r; mkdir -p $r
	. /board/etc/oos-board
	cp -a /src/. $r/
	# what belongs to the board: etc/oos-board and bin/oos-board, which rcS asks
	cp -a /board/. $r/
	mkdir -p $r/bin $r/sbin $r/etc $r/proc $r/sys $r/dev $r/tmp $r/run $r/mnt $r/root $r/lib/modules
	cp /bin/busybox $r/bin/busybox
	cp /hw/oos-hw $r/bin/oos-hw
	cp /ui/oos-ui $r/bin/oos-ui
	cp /fpgad/oos-fpgad $r/bin/oos-fpgad
	cp /fpgad/oos-update $r/bin/oos-update
	cp /wlan/wpa_supplicant /wlan/wpa_cli /wlan/e2fsck /wlan/kexec $r/bin/
	if [ -s /wlan/ssh-seed/authorized_keys ]; then
		mkdir -p $r/etc/oos-ssh; cp /wlan/ssh-seed/authorized_keys $r/etc/oos-ssh/authorized_keys
		chmod 700 $r/etc/oos-ssh; chmod 600 $r/etc/oos-ssh/authorized_keys
	fi
	mkdir -p $r/etc/licenses; cp /wlan/wpa_supplicant.COPYING $r/etc/licenses/wpa_supplicant; cp /wlan/e2fsck.COPYING $r/etc/licenses/e2fsck; cp /wlan/kexec.COPYING $r/etc/licenses/kexec-tools
	# the GPU helper with Mesa and glibc (their own loader; oos-ui and the rest stay static)
	cp -a /wlan/gpu/. $r/
	. /wlan/unit-hook/image.sh
	ln -sf busybox $r/bin/sh
	echo "$BUILD_ID" > $r/etc/oos-build
	# rcS brings the acquisition hardware up at boot (cold boot + oos-ui --hw --afe); OOS_NO_AUTOACQ=1 leaves it out
	[ -n "$OOS_NO_AUTOACQ" ] || touch $r/etc/oos-autoacq
	mkdir -p /tmp/m && tar -C /tmp/m -xzf /k/modules.tar.gz
	rel=$(cat /k/kernel.release); M=/tmp/m/lib/modules/$rel
	mkdir -p $r/lib/modules/$rel
	named=$(for n in $OOS_MODULE_NAMES; do (cd $M && find . -name "$n.ko*" | sed "s|^\./||"); done)
	for f in $OOS_MODULES $named; do
		mkdir -p $r/lib/modules/$rel/$(dirname $f); cp $M/$f $r/lib/modules/$rel/$f
	done
	cp $M/modules.dep $M/modules.alias $M/modules.order $M/modules.builtin* $r/lib/modules/$rel/ 2>/dev/null || true
	# the console node the kernel opens before /init mounts devtmpfs
	mknod -m 600 $r/dev/console c 5 1
	# copies a file sync made in the build outputs (iCloud: "libc.so.6 2", "oos-gpu 2") stay out of the image: 27 MB
	# of them in rootfs/out/gpu made the initramfs of the first 0.1.13 build 10 MB larger (2026-11-15)
	dup=$(cd $r && find . \( -name "* [0-9]" -o -name "* [0-9].*" \) | head -5)
	[ -z "$dup" ] || { echo "build-dev-image.sh: copies of a file sync in the image, remove them from the source: $dup" >&2; exit 1; }
	(cd $r && find . | sort | cpio -o -H newc -R 0:0 --quiet) > /tmp/root.cpio
	gzip -9n < /tmp/root.cpio > /out/initramfs.cpio.gz
	cp /k/$OOS_KERNEL_DTB /out/$OOS_IMAGE_NAME.dtb
	fdtput -t s /out/$OOS_IMAGE_NAME.dtb /chosen bootargs "$OOS_BOOTARGS"
	fdtget /out/$OOS_IMAGE_NAME.dtb /chosen bootargs
	. /wlan/unit-hook/dtb.sh
	# the busybox package in the image, and the glibc it was built with and holds: their sources go with a release
	# (tools/release-sources.sh names that glibc; it follows when this line changes)
	dpkg-query -W -f "\${Package} \${Version}\nBuilt-Using: \${Built-Using}\n" busybox-static > /out/busybox.version
	chown -R "$OUT_OWNER" /out
	ls -la /out
'
python3 -I "$here/oos-bootimg.py" build --kernel "$k/Image" --dtb "$out/$OOS_IMAGE_NAME.dtb" \
	--ramdisk "$out/initramfs.cpio.gz" --name "$OOS_IMAGE_NAME" --out "$out/oos-dev.img"
(cd "$out" && sha256 oos-dev.img > oos-dev.img.sha256 && cat oos-dev.img.sha256)
# signed update package (docs/update.md): install on a running OOS through http://<scope>:8080/update
# your own key if you have one, else the community key every OOS build trusts
key="${OOS_UPDATE_KEY:-$HOME/.config/oos/update-signing.key}"
[ -f "$key" ] || key="$root/keys/community-signing.key"
rm -f "$out/oos-dev.oosupd"
if [ -f "$key" ]; then
	(cd "$root/fpgad" && cargo build --release --bin oos-update $bfeat) || exit 1
	"$root/fpgad/target/release/oos-update" pack --image "$out/oos-dev.img" --build "$build_id" --key "$key" --out "$out/oos-dev.oosupd"
else
	echo "no signing key at $key: no update package (oos-update keygen, docs/update.md)"
fi
# the same image as an update file of the stock system, for an install without opening the scope (docs/install-usb.md)
[ -f "$out/oos-dev.oosupd" ] && python3 -I "$here/build-gel.py"
