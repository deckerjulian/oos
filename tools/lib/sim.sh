# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# sim.sh - sourced by the simulations of tools/ (fw-sim.sh, ssh-sim.sh, usb-sim.sh, wlan-sim.sh): the image's busybox,
# and a small virtual machine with Alpine's LTS kernel for the tests that need a kernel of their own. Sources
# tools/lib/docker.sh; the caller sets root. The functions keep their variables to the sim_ prefix.
#
#   sim_busybox DIR                       DIR/busybox: the busybox of the image (the initramfs image's)
#   sim_vm_initramfs DIR PACKAGES MODULES SCRIPT [docker run options]
#                                         in an arm64 Alpine container with the LTS kernel and PACKAGES: the VM's root
#                                         in $r with the busybox of DIR and DIR/init as /init, SCRIPT run (sh -eu, DIR
#                                         is /w), then the kernel MODULES and what they need in $r/mods, numbered in
#                                         the order to load; writes DIR/initramfs.gz and DIR/kernel
#   sim_qemu DIR MEMORY [qemu options]    boots them (hvf on an arm64 Mac), the console in DIR/log
. "$root/tools/lib/docker.sh"

sim_busybox() {
	docker run --rm --platform linux/arm64 "$(initramfs_image)" cat /bin/busybox > "$1/busybox"
	chmod +x "$1/busybox"
}

sim_vm_initramfs() {
	sim_w=$1 sim_pkgs=$2 sim_mods=$3 sim_script=$4
	shift 4
	# HOST_IDS: what the container makes in the work folder belongs to the user that runs QEMU on it afterwards. On
	# Linux the container's root would own it, and QEMU could not open a disk image for writing; on macOS Docker
	# maps the owner by itself.
	docker run --rm --platform linux/arm64 -e PKGS="$sim_pkgs" -e MODS="$sim_mods" -e SCRIPT="$sim_script" \
		-e HOST_IDS="$(id -u):$(id -g)" -v "$sim_w:/w" \
		"$@" "$OOS_ALPINE" sh -euc '
	apk add --no-cache linux-firmware-none cpio xz zstd $PKGS >/dev/null
	apk add --no-cache --no-scripts linux-lts >/dev/null
	r=/tmp/r; rm -rf $r; mkdir -p $r/bin $r/etc $r/mods
	cp /w/busybox $r/bin/busybox
	for a in $($r/bin/busybox --list | grep -vx busybox) sh; do ln -sf busybox $r/bin/$a; done
	cp /w/init $r/init; chmod +x $r/init
	eval "$SCRIPT"
	# each module after what it needs (modules.dep lists the last one to load first)
	M=$(ls -d /lib/modules/*)
	n=10
	for want in $MODS; do
		line=$(grep "/$want.ko[^:]*:" $M/modules.dep || true)
		[ -n "$line" ] || continue
		for f in $(echo "${line#*:}" | tr " " "\n" | sed "/^$/d" | sed "1!G;h;\$!d") "${line%%:*}"; do
			b=$(basename $f); b=${b%%.ko*}
			ls $r/mods/*-$b.ko >/dev/null 2>&1 && continue
			case "$f" in *.gz) gzip -dc $M/$f;; *.xz) xz -dc $M/$f;; *.zst) zstd -dc $M/$f;; *) cat $M/$f;; esac > $r/mods/$n-$b.ko
			n=$((n + 1))
		done
	done
	(cd $r && find . | cpio -o -H newc -R 0:0 --quiet | gzip -1) > /w/initramfs.gz
	cp /boot/vmlinuz-lts /w/kernel
	chown -R "$HOST_IDS" /w
'
}

sim_qemu() {
	sim_w=$1 sim_mem=$2
	shift 2
	sim_accel="-cpu max"
	[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] && sim_accel="-accel hvf -cpu host"
	qemu-system-aarch64 -M virt $sim_accel -m "$sim_mem" -smp 2 -nographic -no-reboot -kernel "$sim_w/kernel" \
		-initrd "$sim_w/initramfs.gz" -append "console=ttyAMA0 rdinit=/init quiet panic=1" -device virtio-rng-pci "$@" |
		tee "$sim_w/log"
}
