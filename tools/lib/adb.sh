# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# adb.sh - sourced by the tools that read a stock system over network adb (device-survey.sh, platform-probe.sh).
#
#   adb_root <scope>[:port]   connect (port 55555 when none is given), restart adbd as root, connect again;
#                             sets target to <scope>:<port>
adb_root() {
	target=$1
	case "$target" in *:*) ;; *) target="$target:55555" ;; esac
	adb connect "$target" >/dev/null
	# adbd restarts as root and drops the connection; a stock system that refuses keeps the shell user
	adb -s "$target" root >/dev/null 2>&1 || true
	sleep 2
	adb connect "$target" >/dev/null
}
