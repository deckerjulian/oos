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
# The way in is SSH with a trusted key and strict host identity checking (docs/ssh.md, "Tools over SSH"): the scope's
# host key is known first; OOS_SSH_PORT (22), OOS_SSH_IDENTITY (a key file on this computer). Every file is checked
# with SHA-256 on the scope after the upload.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
host=${1:?usage: device-deploy.sh <ip> [--restore]}
ui=${OOS_DEPLOY_UI:-$here/ui/target/aarch64-unknown-linux-musl/release/oos-ui}
fpgad=${OOS_DEPLOY_FPGAD:-$here/fpgad/target/aarch64-unknown-linux-musl/release/oos-fpgad}

# run CMD: one command on the scope's shell, its output on stdout
# put FILE NAME: FILE to /tmp/NAME on the scope
port=${OOS_SSH_PORT:-22}
# SSH never accepts a changed or unknown identity silently; the optional identity stays on this computer
run() {
	ssh -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=10 -p "$port" \
		${OOS_SSH_IDENTITY:+-i "$OOS_SSH_IDENTITY"} "root@$host" "$1"
}
put() {
	run "umask 077; cat > /tmp/$2.part && mv /tmp/$2.part /tmp/$2" < "$1"
}

sha256() {
	if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1"; else sha256sum "$1"; fi | cut -d' ' -f1
}

send() {
	echo "sending $(basename "$1") -> /tmp/$2"
	put "$1" "$2"
	want=$(sha256 "$1")
	[ -n "$want" ] || { echo "error: no SHA-256 tool on this computer" >&2; exit 1; }
	run "sha256sum /tmp/$2" | grep -q "$want" || { echo "error: /tmp/$2 differs after the upload" >&2; exit 1; }
	run "chmod +x /tmp/$2" >/dev/null
}

# stop NAME FILE: FILE ends the loop that starts NAME (rcS's and loop() below have the form `while [ ! -e FILE ]`),
# NAME is ended, and the next step waits until both the program and its loop are gone (at most 15 s): an oos-ui that
# starts while the last one still holds its ports and the DMA device runs without them (#212: "Resource busy" on
# /dev/xdma0_c2h_0, "SCPI port 5555 not available"), and a loop still alive would start a program again. When they
# are not gone, or the scope does not answer, nothing is started and the script ends with an error.
stop() {
	alive="pidof $1 >/dev/null || ps | grep -v grep | grep -qF 'while [ ! -e $2 ]'"
	run "touch $2; kill \$(pidof $1) 2>/dev/null; i=0; while { $alive; } && [ \$i -lt 15 ]; do sleep 1; i=\$((i+1)); done; ! { $alive; }" >/dev/null ||
		{ echo "error: $1 or its loop still runs after 15 s (or the scope did not answer): nothing started" >&2; exit 1; }
}

loop() {
	# as rcS starts it, with another program and another stop file; the hardware is up, so no start-up screen
	l="while [ ! -e $2 ]; do $1 --hw --afe >> /tmp/oos-ui.log 2>&1; sleep 2; done"
	run "(setsid sh -c '$l' </dev/null >/dev/null 2>&1 &)" >/dev/null
}

if [ "${2:-}" = --restore ]; then
	stop oos-ui.new /tmp/oos-ui.off2
	run "rm -f /tmp/oos-ui.off" >/dev/null
	loop /bin/oos-ui /tmp/oos-ui.off
	echo "installed oos-ui running again"
	exit 0
fi

[ -x "$ui" ] && [ -x "$fpgad" ] || { echo "error: build first (see the top of this script)" >&2; exit 1; }
# the matrix of the board family (ADR 0067), checked before anything is uploaded
. "$here/tools/lib/board.sh"
board=$(oos_board "$here") || exit 1
matrix="$here/tools/board/$board/plan-matrix.sh"
[ -f "$matrix" ] || { echo "error: no matrix for the board family '$board' ($matrix)" >&2; exit 1; }
run "cat /proc/version; ls -l /bin/oos-ui; pidof oos-ui oos-ui.new; df -k /tmp | tail -1"
# a build from an earlier deploy still runs from /tmp: "text file busy" for the upload otherwise
stop oos-ui.new /tmp/oos-ui.off2
send "$ui" oos-ui.new
send "$fpgad" oos-fpgad.new
send "$matrix" plan-matrix.sh
stop oos-ui /tmp/oos-ui.off
run "rm -f /tmp/oos-ui.off2" >/dev/null
loop /tmp/oos-ui.new /tmp/oos-ui.off2
sleep 6
run "pidof oos-ui.new; tail -3 /tmp/oos-ui.log"
