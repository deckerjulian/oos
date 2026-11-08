#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# probe.sh OUTDIR - the part of platform-probe.sh that runs on the scope. READ ONLY: it reads files under /proc and
# /sys, the partition table and the first 2 KiB of the boot partitions, and lists /rigol by name and size. It reads no
# register, no calibration, licence or key file, and writes nothing outside OUTDIR (except mounting debugfs, a pseudo
# filesystem that is gone after the next start).
# Runs under Android's toybox sh and under busybox sh.
set -u
out=${1:?usage: probe.sh OUTDIR}
mkdir -p "$out" || exit 1

run() { # run NAME 'command'
	sh -c "$2" > "$out/$1.txt" 2>&1
	echo "$1 exit $?" >> "$out/index.txt"
}

# a block device for a partition name, from the usual by-name links
part() {
	for d in /dev/block/by-name /dev/block/platform/*/by-name /dev/block/platform/*/*/by-name; do
		[ -e "$d/$1" ] && { echo "$d/$1"; return; }
	done
}

hexdump2k() { # first 2 KiB of a device as hex, 16 bytes a line
	dd if="$1" bs=512 count=4 2>/dev/null | od -An -tx1 -v
}

run id 'uname -a; echo; cat /proc/version; echo; cat /proc/device-tree/model 2>/dev/null; echo; cat /proc/device-tree/compatible 2>/dev/null | tr "\0" " "; echo'
run cpu 'cat /proc/cpuinfo; echo; ls /sys/devices/system/cpu/; cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_available_frequencies 2>/dev/null'
run memory 'cat /proc/meminfo; echo; cat /proc/iomem 2>/dev/null'
run cmdline 'cat /proc/cmdline'
run getprop 'getprop 2>/dev/null || echo "no getprop (not Android)"'
run partitions 'cat /proc/partitions; echo; for d in /dev/block/by-name /dev/block/platform/*/by-name /dev/block/platform/*/*/by-name; do [ -d $d ] && ls -l $d; done; echo; readlink -f /sys/block/mmcblk*/device 2>/dev/null'
run mounts 'cat /proc/mounts; echo; df 2>/dev/null'
run modules 'cat /proc/modules'
run platform-devices 'for d in /sys/bus/platform/devices/*; do echo "$(basename $d) driver=$(basename "$(readlink $d/driver 2>/dev/null)" 2>/dev/null)"; done'
run drivers 'ls /sys/bus/platform/drivers/; echo; ls /sys/bus/pci/drivers/ 2>/dev/null; echo; ls /sys/bus/spi/devices/ 2>/dev/null'
run pci 'for d in /sys/bus/pci/devices/*; do [ -e $d ] || continue; echo "$(basename $d) vendor=$(cat $d/vendor) device=$(cat $d/device) class=$(cat $d/class) link=$(cat $d/current_link_speed 2>/dev/null) x$(cat $d/current_link_width 2>/dev/null) driver=$(basename "$(readlink $d/driver 2>/dev/null)" 2>/dev/null)"; cat $d/resource; done'
run i2c 'for b in /sys/bus/i2c/devices/*; do echo "$(basename $b) name=$(cat $b/name 2>/dev/null) driver=$(basename "$(readlink $b/driver 2>/dev/null)" 2>/dev/null)"; done'
run usb 'for d in /sys/bus/usb/devices/*; do [ -f $d/idVendor ] || continue; echo "$(basename $d) $(cat $d/idVendor):$(cat $d/idProduct) class=$(cat $d/bDeviceClass) speed=$(cat $d/speed) \"$(cat $d/product 2>/dev/null)\""; done'
run input 'cat /proc/bus/input/devices'
run display 'for c in /sys/class/drm/*; do echo "== $c"; cat $c/status $c/modes 2>/dev/null; done; for f in /sys/class/graphics/fb*; do echo "== $f"; cat $f/virtual_size $f/bits_per_pixel $f/modes 2>/dev/null; done; true'
run thermal 'for z in /sys/class/thermal/thermal_zone*; do echo "$(basename $z) $(cat $z/type) $(cat $z/temp)"; done'
run net 'for n in /sys/class/net/*; do echo "$(basename $n) driver=$(basename "$(readlink $n/device/driver 2>/dev/null)" 2>/dev/null) speed=$(cat $n/speed 2>/dev/null)"; done'
run tty 'cat /proc/tty/drivers; ls -l /dev/tty[SA]* 2>/dev/null; cat /proc/tty/driver/serial 2>/dev/null'
run misc 'cat /proc/misc; echo; ls -l /dev | grep -vE " (tty[0-9]|loop|ram|pts)"'
run interrupts 'cat /proc/interrupts'
grep -q " /sys/kernel/debug " /proc/mounts || mount -t debugfs none /sys/kernel/debug 2>/dev/null
run gpio 'cat /sys/kernel/debug/gpio'
run pinmux 'for f in /sys/kernel/debug/pinctrl/*/pinmux-pins; do echo "== $f"; cat $f; done'
run clocks 'cat /sys/kernel/debug/clk/clk_summary'
run regulators 'cat /sys/kernel/debug/regulator/regulator_summary'
run dmesg 'dmesg'
# the device tree as text: properties that hold an address or serial number are left out
run device-tree 'find /proc/device-tree/ -type f | sort | while read f; do
	case ${f##*/} in
	*mac-address*|*macaddr*|serial-number|*serial_number|*serialno*) echo "${f#/proc/device-tree} <left out>";;
	*) if [ -z "$(tr -d "\040-\176\000" < "$f")" ] && [ -s "$f" ]; then echo "${f#/proc/device-tree} = \"$(tr "\000" "|" < "$f")\""; else echo "${f#/proc/device-tree} = <$(od -An -tx1 -v "$f" | tr -s " \n" " ")>"; fi;;
	esac
done'
# card layout: the partition table text and the headers of the boot partitions (magic, sizes)
{
	pp=$(part parameter)
	[ -n "$pp" ] && { echo "== parameter $pp"; dd if="$pp" bs=512 count=8 2>/dev/null | tr -cd "\040-\176\n"; echo; }
	# Rockchip keeps the table at sector 8192 of the card too
	for d in /dev/block/mmcblk0 /dev/block/mmcblk1 /dev/mmcblk0 /dev/mmcblk1; do
		[ -b "$d" ] || continue
		t=$(dd if="$d" bs=512 skip=8192 count=8 2>/dev/null | tr -cd "\040-\176\n")
		case "$t" in PARM*|*CMDLINE*) echo "== sector 8192 of $d ($(readlink -f /sys/block/${d##*/}/device 2>/dev/null))"; echo "$t";; esac
	done
} > "$out/parameter.txt" 2>&1
echo "parameter exit 0" >> "$out/index.txt"
# Rockchip: the command line names the partitions as size@offset in sectors from sector 8192 of the card
card=$(for d in /dev/block/mmcblk0 /dev/mmcblk0; do [ -b $d ] && echo $d && break; done)
for n in boot recovery backup kernel resource misc; do
	p=$(part $n)
	if [ -n "$p" ]; then
		{ echo "== $n $p"; hexdump2k "$p"; } >> "$out/boot-headers.txt" 2>&1
	elif [ -n "$card" ]; then
		off=$(tr " " "\n" < /proc/cmdline | grep "^mtdparts=" | tr "," "\n" | sed -n "s/.*@\(0x[0-9a-fA-F]*\)($n)$/\1/p")
		[ -n "$off" ] && { echo "== $n $card sector $((off + 8192))"; dd if=$card bs=512 skip=$((off + 8192)) count=4 2>/dev/null | od -An -tx1 -v; } >> "$out/boot-headers.txt" 2>&1
	fi
done
echo "boot-headers exit 0" >> "$out/index.txt"
# names and sizes only: no file content
run rigol-tree 'ls -laR /rigol 2>/dev/null | head -2000'
run apps 'ls -la /system/app /system/priv-app /data/app 2>/dev/null | head -200'
echo done >> "$out/index.txt"
