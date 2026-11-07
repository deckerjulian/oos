#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# device-deploy.sh - runs the oos-ui of this tree on a running OOS, without touching the card: it goes to /tmp, which is
# RAM, and the next power cycle is back to the installed image. No reboot is needed, and none would work: from OOS to
# OOS takes a power cycle.
#
#   tools/device-deploy.sh <ip>             upload oos-ui, oos-fpgad and plan-matrix.sh, switch to the new oos-ui
#   tools/device-deploy.sh <ip> --restore   back to the installed oos-ui
#
# Build first: (cd ui && cargo build --release --target aarch64-unknown-linux-musl), same for fpgad.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
host=${1:?usage: device-deploy.sh <ip> [--restore]}
port=${OOS_TELNET_PORT:-23}
ui=${OOS_DEPLOY_UI:-$here/ui/target/aarch64-unknown-linux-musl/release/oos-ui}
fpgad=${OOS_DEPLOY_FPGAD:-$here/fpgad/target/aarch64-unknown-linux-musl/release/oos-fpgad}

# one command on the telnet shell; the pauses keep nc from dropping what comes back
tel() {
	(sleep 1; echo "$1"; sleep "${2:-2}"; echo exit) | nc -w $((${2:-2} + 5)) "$host" "$port" | LC_ALL=C tr -d '\r'
}

send() {
	echo "sending $(basename "$1") -> /tmp/$2"
	tel "(sleep 120 | nc -l -p 7777 > /tmp/$2) &" 1 >/dev/null
	sleep 1
	nc -w 3 "$host" "${OOS_NC_PORT:-7777}" < "$1"
	want=$(md5 -q "$1" 2>/dev/null || md5sum "$1" | cut -d' ' -f1)
	tel "md5sum /tmp/$2" 3 | grep -q "$want" || { echo "error: /tmp/$2 differs after the upload" >&2; exit 1; }
	# the listener holds the file open while its stdin lives: "text file busy" otherwise
	tel "kill \$(pidof nc) 2>/dev/null; chmod +x /tmp/$2" 1 >/dev/null
}

loop() {
	# as rcS starts it, with another program and another stop file; the hardware is up, so no start-up screen
	tel "(setsid sh -c 'while [ ! -e $2 ]; do $1 --hw --afe >> /tmp/oos-ui.log 2>&1; sleep 2; done' </dev/null >/dev/null 2>&1 &)" 1 >/dev/null
}

if [ "${2:-}" = --restore ]; then
	tel "touch /tmp/oos-ui.off2; kill \$(pidof oos-ui.new)" 3 >/dev/null
	tel "rm -f /tmp/oos-ui.off" 1 >/dev/null
	loop /bin/oos-ui /tmp/oos-ui.off
	echo "installed oos-ui running again"
	exit 0
fi

[ -x "$ui" ] && [ -x "$fpgad" ] || { echo "error: build first (see the top of this script)" >&2; exit 1; }
tel "cat /proc/version; ls -l /bin/oos-ui; pidof oos-ui oos-ui.new; df -k /tmp | tail -1" 2
# a build from an earlier deploy still runs from /tmp: "text file busy" for the upload otherwise
tel "touch /tmp/oos-ui.off2; kill \$(pidof oos-ui.new) 2>/dev/null" 3 >/dev/null
send "$ui" oos-ui.new
send "$fpgad" oos-fpgad.new
send "$here/tools/plan-matrix.sh" plan-matrix.sh
tel "touch /tmp/oos-ui.off; kill \$(pidof oos-ui)" 3 >/dev/null
tel "rm -f /tmp/oos-ui.off2" 1 >/dev/null
loop /tmp/oos-ui.new /tmp/oos-ui.off2
sleep 6
tel "pidof oos-ui.new; tail -3 /tmp/oos-ui.log" 2
