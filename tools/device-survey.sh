#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# device-survey.sh - READ-ONLY inventory of the running stock system (Android 7.1) over adb.
#
#   device-survey.sh [ip[:port]] [--keys]
#
# Collects into local/device-survey/<date>/: properties, kernel cmdline, partitions, mounts, modules,
# input devices, USB devices, PCIe, GPIO ownership (debugfs), pinmux, serial ports, ttyS0 line settings,
# interrupts, dmesg. With --keys it records 20 s of raw input events while you press every front-panel
# key and turn every knob once (to find the device that delivers the panel keys).
#
# Nothing is written on the scope except: debugfs is mounted on /sys/kernel/debug if it is not mounted
# yet (a pseudo filesystem, gone after reboot).
set -eu
target=${1:?usage: device-survey.sh <scope>[:port] [--keys]}
case "$target" in *:*) ;; *) target="$target:55555";; esac
keys=0; for a in "$@"; do [ "$a" = "--keys" ] && keys=1; done

here=$(cd "$(dirname "$0")" && pwd)
out="$here/../local/device-survey/$(date +%Y-%m-%d_%H%M%S)"
mkdir -p "$out"

adb connect "$target" >/dev/null
adb -s "$target" root >/dev/null 2>&1 || true
sleep 2
adb connect "$target" >/dev/null
A="adb -s $target shell"

run() { # run name 'command'
	printf '%-22s ' "$1"
	if $A "$2" > "$out/$1.txt" 2>&1; then echo ok; else echo "exit $? (kept output)"; fi
}

run id                'id; uname -a; cat /proc/device-tree/model; echo; cat /proc/cpuinfo | tail -5'
run getprop           'getprop'
run cmdline           'cat /proc/cmdline'
run partitions        'cat /proc/partitions; echo; ls -l /dev/block/platform/*/by-name/ /dev/block/by-name/ 2>/dev/null'
run mounts            'cat /proc/mounts; echo; df'
run modules           'lsmod'
run input-devices     'cat /proc/bus/input/devices; echo; ls -l /dev/input/'
run getevent-p        'getevent -pl 2>&1 & p=$!; sleep 2; kill $p 2>/dev/null; true'
run usb-devices       'for d in /sys/bus/usb/devices/*; do [ -f $d/idVendor ] || continue; echo "$d $(cat $d/idVendor):$(cat $d/idProduct) class=$(cat $d/bDeviceClass) \"$(cat $d/manufacturer 2>/dev/null)\" \"$(cat $d/product 2>/dev/null)\" speed=$(cat $d/speed)"; for i in $d/*:*; do [ -f $i/bInterfaceClass ] && echo "   $(basename $i) class=$(cat $i/bInterfaceClass) sub=$(cat $i/bInterfaceSubClass) proto=$(cat $i/bInterfaceProtocol) driver=$(basename $(readlink $i/driver 2>/dev/null) 2>/dev/null)"; done; done'
run hid-descriptors   'for r in /sys/bus/hid/devices/*/report_descriptor; do echo "== $r"; od -An -tx1 -v $r; done'
run pci               'lspci 2>/dev/null; echo; for d in /sys/bus/pci/devices/*; do echo "$d vendor=$(cat $d/vendor) device=$(cat $d/device) class=$(cat $d/class) link=$(cat $d/current_link_speed 2>/dev/null) x$(cat $d/current_link_width 2>/dev/null)"; cat $d/resource; done'
$A 'mountpoint -q /sys/kernel/debug 2>/dev/null || grep -q " /sys/kernel/debug " /proc/mounts || mount -t debugfs none /sys/kernel/debug' >/dev/null 2>&1 || true
run gpio-debugfs      'cat /sys/kernel/debug/gpio'
run pinmux            'for f in /sys/kernel/debug/pinctrl/*/pinmux-pins; do echo "== $f"; cat $f; done'
run pinconf           'for f in /sys/kernel/debug/pinctrl/*/pinconf-pins; do echo "== $f"; cat $f; done'
run gpio-sysfs        'ls /sys/class/gpio; for g in /sys/class/gpio/gpio*; do [ -f $g/value ] && echo "$g dir=$(cat $g/direction) val=$(cat $g/value)"; done'
run serial            'cat /proc/tty/driver/serial 2>/dev/null; ls -l /dev/ttyS*; for t in /dev/ttyS*; do echo "== $t"; stty -F $t -a 2>&1 || busybox stty -F $t -a 2>&1; done'
run fuser-tty         'for p in /proc/[0-9]*; do ls -l $p/fd 2>/dev/null | grep -E "ttyS|spidev|xdma|spi_|afg|dac_|fg_m2c|hdcode|i2c" | sed "s|^|$(cat $p/comm 2>/dev/null) $p: |"; done'
run misc-devices      'cat /proc/misc; ls -l /dev | grep -E "spi|xdma|afg|dac|fg_|hd|pwm|i2c|vendor|tty|beep|Beep"'
run i2c-sysfs         'for b in /sys/bus/i2c/devices/*; do echo "$b name=$(cat $b/name 2>/dev/null) driver=$(basename $(readlink $b/driver 2>/dev/null) 2>/dev/null)"; done'
run interrupts        'cat /proc/interrupts'
run dmesg             'dmesg'
run stock-tree        'ls -laR /rigol | head -400; echo; getprop | grep -i rigol'
run fpga-version      '/rigol/tools/pcie2dev 0x4000; /rigol/tools/pcie2dev 0x401c'

if [ "$keys" -eq 1 ]; then
	echo "recording input events for 20 s: press every key and turn every knob once now ..."
	$A 'getevent -lt 2>&1 & p=$!; sleep 20; kill $p 2>/dev/null; true' > "$out/getevent-keys.txt" || true
	echo "saved getevent-keys.txt ($(wc -l < "$out/getevent-keys.txt") lines)"
fi
echo "survey in $out"
