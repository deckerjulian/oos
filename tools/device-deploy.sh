#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# device-deploy.sh - runs the oos-ui of this tree on a running OOS, without touching the card: it goes to /tmp, which is
# RAM, and the next power cycle is back to the installed image. No reboot is needed, and none would work: from OOS to
# OOS takes a power cycle.
#
#   tools/device-deploy.sh <ip>             upload oos-ui, oos-fpgad and plan-matrix.sh, switch to the new oos-ui
#   tools/device-deploy.sh <ip> --restore   back to the installed oos-ui
#
# Build first: (cd ui && cargo build --release --target aarch64-unknown-linux-musl), same for fpgad.
#
# OOS_SHELL picks the way in, as for tools/device-test.py:
#   telnet (default)  the telnet shell (OOS_TELNET_PORT, 23); files go through nc to a listener on the scope
#                     (OOS_NC_PORT, 7777)
#   ssh               key-authenticated SSH with strict host identity checking (docs/ssh.md): the scope's host key is
#                     known first; OOS_SSH_PORT (22), OOS_SSH_IDENTITY (a key file on this computer)
# Either way every file is checked with SHA-256 on the scope after the upload.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
host=${1:?usage: device-deploy.sh <ip> [--restore]}
ui=${OOS_DEPLOY_UI:-$here/ui/target/aarch64-unknown-linux-musl/release/oos-ui}
fpgad=${OOS_DEPLOY_FPGAD:-$here/fpgad/target/aarch64-unknown-linux-musl/release/oos-fpgad}

# run CMD [WAIT]: one command on the scope's shell, its output on stdout (WAIT: seconds the telnet shell is given)
# put FILE NAME: FILE to /tmp/NAME on the scope
case ${OOS_SHELL:-telnet} in
telnet)
	port=${OOS_TELNET_PORT:-23}
	ncport=${OOS_NC_PORT:-7777}
	# the pauses keep nc from dropping what comes back
	run() {
		(sleep 1; echo "$1"; sleep "${2:-2}"; echo exit) | nc -w $((${2:-2} + 5)) "$host" "$port" | LC_ALL=C tr -d '\r'
	}
	# the listener holds the file open while its stdin lives ("text file busy" for the program otherwise), so it is
	# ended once the file is in
	put() {
		run "(sleep 120 | nc -l -p $ncport > /tmp/$2) &" 1 >/dev/null
		sleep 1
		nc -w 3 "$host" "$ncport" < "$1"
		run "kill \$(pidof nc) 2>/dev/null" 1 >/dev/null
	}
	;;
ssh)
	port=${OOS_SSH_PORT:-22}
	# SSH never accepts a changed or unknown identity silently; the optional identity stays on this computer
	run() {
		ssh -o BatchMode=yes -o StrictHostKeyChecking=yes -p "$port" ${OOS_SSH_IDENTITY:+-i "$OOS_SSH_IDENTITY"} \
			"root@$host" "$1"
	}
	put() {
		run "umask 077; cat > /tmp/$2.part && mv /tmp/$2.part /tmp/$2" < "$1"
	}
	;;
*)
	echo "error: OOS_SHELL is telnet or ssh" >&2
	exit 2
	;;
esac

sha256() {
	if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1"; else sha256sum "$1"; fi | cut -d' ' -f1
}

send() {
	echo "sending $(basename "$1") -> /tmp/$2"
	put "$1" "$2"
	want=$(sha256 "$1")
	[ -n "$want" ] || { echo "error: no SHA-256 tool on this computer" >&2; exit 1; }
	run "sha256sum /tmp/$2" 3 | grep -q "$want" || { echo "error: /tmp/$2 differs after the upload" >&2; exit 1; }
	run "chmod +x /tmp/$2" 1 >/dev/null
}

loop() {
	# as rcS starts it, with another program and another stop file; the hardware is up, so no start-up screen
	l="while [ ! -e $2 ]; do $1 --hw --afe >> /tmp/oos-ui.log 2>&1; sleep 2; done"
	run "(setsid sh -c '$l' </dev/null >/dev/null 2>&1 &)" 1 >/dev/null
}

if [ "${2:-}" = --restore ]; then
	run "touch /tmp/oos-ui.off2; kill \$(pidof oos-ui.new) 2>/dev/null || true" 3 >/dev/null
	run "rm -f /tmp/oos-ui.off" 1 >/dev/null
	loop /bin/oos-ui /tmp/oos-ui.off
	echo "installed oos-ui running again"
	exit 0
fi

[ -x "$ui" ] && [ -x "$fpgad" ] || { echo "error: build first (see the top of this script)" >&2; exit 1; }
run "cat /proc/version; ls -l /bin/oos-ui; pidof oos-ui oos-ui.new; df -k /tmp | tail -1" 2
# a build from an earlier deploy still runs from /tmp: "text file busy" for the upload otherwise
run "touch /tmp/oos-ui.off2; kill \$(pidof oos-ui.new) 2>/dev/null || true" 3 >/dev/null
send "$ui" oos-ui.new
send "$fpgad" oos-fpgad.new
send "$here/tools/plan-matrix.sh" plan-matrix.sh
run "touch /tmp/oos-ui.off; kill \$(pidof oos-ui) 2>/dev/null || true" 3 >/dev/null
run "rm -f /tmp/oos-ui.off2" 1 >/dev/null
loop /tmp/oos-ui.new /tmp/oos-ui.off2
sleep 6
run "pidof oos-ui.new; tail -3 /tmp/oos-ui.log" 2
