#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# usb-sim.sh - tests /bin/oos-usb without a stick: a virtual machine with three simulated disks: FAT32 with a partition
# table, exFAT without one, and an empty one. The mounted one is pulled while it runs, twice. The USB
# port itself is not part of this. Needs Docker, qemu-system-aarch64 and one finished image build (for its busybox).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
w="${TMPDIR:-/tmp}/oos-usb-sim"
mkdir -p "$w"
docker image inspect oos-initramfs:ubuntu >/dev/null 2>&1 || { echo "build the image once first (boot/build-dev-image.sh)" >&2; exit 1; }
docker run --rm oos-initramfs:ubuntu cat /bin/busybox > "$w/busybox"
chmod +x "$w/busybox"

cat > "$w/init" <<'INIT'
#!/bin/sh
export PATH=/bin
mkdir -p /proc /sys /dev /tmp /mnt
mount -t proc proc /proc; mount -t sysfs sys /sys; mount -t devtmpfs dev /dev
for m in /mods/*.ko; do insmod $m 2>&1 | grep -v "File exists"; done
sleep 2
ls /sys/block | tr '\n' ' '; echo
/bin/oos-usb > /tmp/oos-usb.log 2>&1 &
wait_for() { n=0; while [ $n -lt 20 ]; do eval "$1" && return 0; sleep 1; n=$((n + 1)); done; echo "TIMEOUT: $1"; return 1; }
mounted() { grep -q ' /mnt/usb ' /proc/mounts; }
ok=1
wait_for mounted || ok=0
grep ' /mnt/usb ' /proc/mounts
mkdir -p /mnt/usb/OOS && echo hello > /mnt/usb/OOS/shot-001.png && sync && cat /mnt/usb/OOS/shot-001.png || ok=0
pull() { d=$(sed -n 's|^/dev/\([a-z]*\)[0-9]* /mnt/usb .*|\1|p' /proc/mounts); echo 1 > /sys/block/$d/device/delete; }
pull
wait_for 'grep -q "pulled" /tmp/oos-usb.log && mounted' || ok=0
grep ' /mnt/usb ' /proc/mounts
echo world > /mnt/usb/second.txt && sync || ok=0
pull
wait_for '! mounted' || ok=0
sleep 5
echo ---- log; cat /tmp/oos-usb.log
[ "$(grep -c 'nothing to mount' /tmp/oos-usb.log)" = 1 ] || { echo "the empty disk must be reported once"; ok=0; }
[ $ok = 1 ] && echo "USB TEST PASSED"
poweroff -f
INIT

docker run --rm --platform linux/arm64 -v "$w:/w" -v "$root/rootfs:/rootfs:ro" alpine:3.22 sh -euc '
	apk add --no-cache linux-firmware-none cpio xz zstd dosfstools exfatprogs sfdisk >/dev/null
	apk add --no-cache --no-scripts linux-lts >/dev/null
	cd /w
	truncate -s 64M fat.img
	echo "2048,,c" | sfdisk -q fat.img
	mkfs.vfat -F 32 --offset=2048 fat.img >/dev/null
	truncate -s 64M exfat.img; mkfs.exfat -q exfat.img
	truncate -s 16M empty.img
	r=/tmp/r; rm -rf $r; mkdir -p $r/bin $r/mods
	cp /w/busybox $r/bin/busybox
	for a in $($r/bin/busybox --list | grep -vx busybox) sh; do ln -sf busybox $r/bin/$a; done
	cp /rootfs/initramfs/bin/oos-usb $r/bin/
	cp /w/init $r/init; chmod +x $r/init
	M=$(ls -d /lib/modules/*)
	n=10
	for want in virtio_pci virtio_scsi sd_mod vfat exfat nls_cp437 nls_utf8 nls_iso8859-1; do
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
'
accel="-cpu max"
[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] && accel="-accel hvf -cpu host"
disk() { printf -- '-drive if=none,id=%s,format=raw,file=%s -device scsi-hd,drive=%s,channel=0,scsi-id=0,lun=%s\n' "$1" "$2" "$1" "$3"; }
qemu-system-aarch64 -M virt $accel -m 512 -smp 2 -nographic -no-reboot -kernel "$w/kernel" -initrd "$w/initramfs.gz" \
	-append "console=ttyAMA0 rdinit=/init quiet panic=1" -device virtio-rng-pci -device virtio-scsi-pci \
	$(disk a "$w/fat.img" 0) $(disk b "$w/exfat.img" 1) $(disk c "$w/empty.img" 2) | tee "$w/log"
grep -q 'USB TEST PASSED' "$w/log"
