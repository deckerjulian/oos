#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# bench-power.sh - switches the bench unit's power through a switchable outlet (ADR 0070), so a cold start (the trial
# start of an update, the boot class of ADR 0052) needs nobody at the bench.
#
#   tools/bench-power.sh status                 whether the outlet is on
#   tools/bench-power.sh on | off               switch it
#   tools/bench-power.sh cycle [--wait S]       off, S seconds (default 30, as docs/update.md asks), on
#
# Before `off` and `cycle` it asks the scope over SSH (OOS_SCOPE, the same key and port as tools/device-deploy.sh)
# whether an oos-update runs: an install, a rollback or the start-up watcher. Then it refuses, and so it does when the
# scope does not answer: an install started by kexec has no SSH and must not lose power. `--force` switches anyway;
# ADR 0070 says when the agent may use it.
#
# The outlet, from OOS_POWER or the file local/bench.conf (a line OOS_POWER=...; local/ stays out of git):
#   shelly:HOST       a Shelly of the second generation or later, through its local RPC (Switch.Set, id 0)
#   tasmota:HOST      a plug with Tasmota (cm?cmnd=Power)
#   cmd:PROGRAM       any program that takes on, off or status and prints on or off for status
# Not yet verified on hardware: written before the outlet was at the bench.
set -eu
cd "$(dirname "$0")/.."
stop() {
	echo "bench-power.sh: $*" >&2
	exit 2
}
[ -n "${OOS_POWER:-}" ] || [ ! -f local/bench.conf ] ||
	OOS_POWER=$(sed -n 's/^OOS_POWER=//p' local/bench.conf | tail -1 | tr -d '"')
power=${OOS_POWER:-}
[ -n "$power" ] || stop "no outlet: set OOS_POWER or write OOS_POWER=... into local/bench.conf"
force=0
wait=30
cmd=${1:-}
[ $# = 0 ] || shift
while [ $# -gt 0 ]; do
	case $1 in
	--force) force=1 ;;
	--wait) wait=${2:?--wait needs seconds}; shift ;;
	*) stop "unknown option $1" ;;
	esac
	shift
done
case $wait in '' | *[!0-9]*) stop "--wait takes whole seconds" ;; esac

# http URL: the outlet's answer on stdout
http() {
	curl -fsS --max-time 10 "$1" || stop "the outlet does not answer ($power)"
}

# outlet on|off|status
outlet() {
	case $power in
	shelly:*)
		h=${power#shelly:}
		case $1 in
		status) http "http://$h/rpc/Switch.GetStatus?id=0" | grep -q '"output": *true' && echo on || echo off ;;
		on) http "http://$h/rpc/Switch.Set?id=0&on=true" >/dev/null ;;
		off) http "http://$h/rpc/Switch.Set?id=0&on=false" >/dev/null ;;
		esac
		;;
	tasmota:*)
		h=${power#tasmota:}
		case $1 in
		status) http "http://$h/cm?cmnd=Power" | grep -qi '"POWER": *"ON"' && echo on || echo off ;;
		on) http "http://$h/cm?cmnd=Power%20On" >/dev/null ;;
		off) http "http://$h/cm?cmnd=Power%20Off" >/dev/null ;;
		esac
		;;
	cmd:*) "${power#cmd:}" "$1" ;;
	*) stop "unknown outlet '$power' (shelly:HOST, tasmota:HOST, cmd:PROGRAM)" ;;
	esac
}

# whether the scope may lose power now: it answers, and no oos-update runs
idle() {
	[ $force = 0 ] || return 0
	[ -n "${OOS_SCOPE:-}" ] || stop "OOS_SCOPE is not set: the scope cannot be asked whether an update runs (--force)"
	busy=$(ssh -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=10 -p "${OOS_SSH_PORT:-22}" \
		${OOS_SSH_IDENTITY:+-i "$OOS_SSH_IDENTITY"} "root@$OOS_SCOPE" 'pidof oos-update || true') ||
		stop "the scope does not answer: it may be installing an update (--force once that is ruled out)"
	[ -z "$busy" ] || stop "oos-update runs on the scope (pid $busy): wait until it is done"
}

case $cmd in
status) outlet status ;;
on) outlet on; echo on ;;
off) idle; outlet off; echo off ;;
cycle)
	idle
	outlet off
	echo "off, on again in $wait s"
	sleep "$wait"
	outlet on
	echo on
	;;
*) stop "usage: bench-power.sh status | on | off | cycle [--wait S] [--force]" ;;
esac
