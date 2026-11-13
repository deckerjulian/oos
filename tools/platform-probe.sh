#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# platform-probe.sh - READ-ONLY description of a scope running its stock software, for porting OOS to it
# (MHO900, DHO1000, ...). Run it on a PC with adb, the scope on the same network with network adb on.
#
#   platform-probe.sh <scope-ip>[:port]
#
# It copies tools/platform-probe/probe.sh to /data/local/tmp on the scope, runs it there, fetches the result and
# removes both from the scope. probe.sh only reads (see its header). On the PC the serial number and MAC addresses
# are replaced before the archive is made; look through the files before you share them anyway.
#
# Result: platform-probe-<model>-<date>.tar.gz in the current directory.
set -eu
target=${1:?usage: platform-probe.sh <scope-ip>[:port]}
case "$target" in *:*) ;; *) target="$target:55555";; esac
here=$(cd "$(dirname "$0")" && pwd)
command -v adb >/dev/null || { echo "adb not found (Android platform tools)"; exit 1; }
command -v perl >/dev/null || { echo "perl not found (needed to remove the serial number)"; exit 1; }

adb connect "$target" >/dev/null
adb -s "$target" root >/dev/null 2>&1 || true
sleep 2
adb connect "$target" >/dev/null
A="adb -s $target"
[ "$($A shell id -u | tr -d '\r')" = 0 ] || { echo "no root shell on the scope (adb root refused)"; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
$A push "$here/platform-probe/probe.sh" /data/local/tmp/oos-probe.sh >/dev/null
echo "running on the scope (about a minute) ..."
$A shell "sh /data/local/tmp/oos-probe.sh /data/local/tmp/oos-probe"
$A pull /data/local/tmp/oos-probe "$tmp/" >/dev/null
$A shell "rm -rf /data/local/tmp/oos-probe /data/local/tmp/oos-probe.sh"
d="$tmp/oos-probe"
[ -f "$d/index.txt" ] || { echo "the scope gave no result"; exit 1; }

# serial numbers: every value of a property or command-line word that names one
{
	grep -i 'serial' "$d/getprop.txt" 2>/dev/null | sed -n 's/^\[[^]]*\]: \[\(.*\)\]$/\1/p'
	tr ' ' '\n' < "$d/cmdline.txt" | grep -i 'serial' | sed 's/^[^=]*=//'
} | tr -d '\r' | awk 'length($0) >= 4' | sort -u > "$tmp/serials"
for f in "$d"/*.txt; do
	SERIALS="$tmp/serials" perl -0777 -pi -e '
		BEGIN { local $/ = "\n"; open(my $h, "<", $ENV{SERIALS}); @s = map { chomp; $_ } <$h>; }
		for my $s (@s) { s/\Q$s\E/<serial>/g }
		s/\b([0-9a-fA-F]{2}[:-]){5}[0-9a-fA-F]{2}\b/<mac>/g;
	' "$f"
done

model=$(tr -cd 'A-Za-z0-9 ' < "$d/getprop.txt" | grep -E -o 'MHO[0-9A-Z]+|DHO[0-9A-Z]+|HDO[0-9A-Z]+|MSO[0-9A-Z]+' | head -1 || true)
name="platform-probe-${model:-scope}-$(date +%Y%m%d-%H%M%S)"
mv "$d" "$tmp/$name"
tar czf "$name.tar.gz" -C "$tmp" "$name"
echo "written: $name.tar.gz ($(grep -c exit "$tmp/$name/index.txt") parts; $(wc -l < "$tmp/serials" | tr -d ' ') serial value(s) replaced)"
echo "Look through it before you share it: it describes the hardware, not you, but check."
