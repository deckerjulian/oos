#!/bin/sh
# Copyright (C) 2026 OOS contributors
# SPDX-License-Identifier: GPL-2.0-or-later
# selfheal-sim.sh - tests /bin/oos-crash-save and /bin/oos-watchdog (docs/boot.md, "Restart after a panic or a hang")
# without a scope: a busybox container, a fake pstore, /oos as a 16 MiB and a 2 MiB tmpfs, /dev/null as the watchdog
# device and a fake busybox `watchdog` that exits at once. Checks: a record is copied whole and pstore cleared only
# after; no record, nothing done; too little space, nothing done and pstore kept; an interrupted save is kept as a
# partial; no more than 4 directories; the kicker is started again with growing delays; a kexec handoff request stops
# the kicker with SIGKILL (no magic close), is acknowledged, and the kicker starts again at once when the request is
# gone, also when the request comes during a backoff sleep; oos-oneshot succeeds only with KRNL in place, the bootargs
# have no panic= and rcS arms the restarts only after oos-oneshot succeeded; a USR1 during the backoff's log line or
# right before its `wait` is answered at once; second handoffs survive cancellation and ack cleanup; restore write,
# sync, cached-only read-back, corrupt sectors, failed reads and short reads all leave restarts disarmed; crash-record
# metadata and parent-directory sync errors preserve pstore. Runs under busybox ash (alpine) and dash (debian). Needs Docker.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
bin="$root/rootfs/initramfs/bin"

cat > "${TMPDIR:-/tmp}/oos-selfheal-sim.test" <<'TEST'
set -u
ok=1
fail() { echo "FAIL: $*"; ok=0; }
export OOS_TEST=1 OOS_KMSG=/tmp/kmsg OOS_PSTORE=/ps
# the fake kickers, from /proc (no pgrep in every image)
kickers() {
	for c in /proc/[0-9]*/cmdline; do
		tr '\0' ' ' < "$c" 2>/dev/null | grep -q '^[^ ]* /fake/watchdog' && { c=${c#/proc/}; echo "${c%/cmdline}"; }
	done
}
kicking() { [ -n "$(kickers)" ]; }
kicking_with_pid() { [ -s /tmp/wd.pid ] && kicking; }
ack_gone() { [ ! -e /tmp/wd.ack ]; }
acked() { [ "$(cat /tmp/wd.ack 2>/dev/null)" = "$1" ]; }
trig_fired() { [ ! -e /tmp/trig ]; }
backoff_reset() { grep -A9 "started again" /tmp/kmsg | grep -q "again in 1 s"; }
# Waits for an event, not for a time: the limit only stops a hung run, and is far above anything a correct script
# needs on a loaded computer (ADR 0020).
await() {
	i=0
	until "$@"; do
		i=$((i + 1))
		[ $i -lt 1200 ] || return 1
		sleep 0.1
	done
}
records() {
	rm -rf /ps; mkdir -p /ps
	printf 'Panic#1 Part1\nKernel panic - not syncing: %s\n' "$1" > /ps/dmesg-ramoops-0
	head -c 100000 /dev/urandom > /ps/console-ramoops-0
}
save() { OOS_CRASH_MNT=$1 /bin/oos-crash-save /tmp/st > /tmp/out 2>&1; }

echo "== oos-oneshot exit status"
board=/tmp/board
printf 'OOS_PARM_MARKS="0x00002000@0x00006000(misc)"\nOOS_PARM_LBA=8\nOOS_BCB_LBA=16\nOOS_BOOT_LBA=24\n' > $board
card() {
	rm -f /tmp/card; dd if=/dev/zero of=/tmp/card bs=512 count=32 2>/dev/null
	printf 'PARM 0x00002000@0x00006000(misc)' | dd of=/tmp/card bs=512 seek=8 conv=notrunc 2>/dev/null
	printf '%s' "$1" | dd of=/tmp/card bs=512 seek=24 conv=notrunc 2>/dev/null
}
oneshot() { OOS_TEST=1 OOS_BOARD_FILE=$board OOS_DEV=/tmp/card OOS_LOG=/tmp/oneshot.log "${OOS_ONESHOT:-/bin/oos-oneshot}"; }
card OOSK; oneshot || fail "OOSK: not 0 after the restore ($(cat /tmp/oneshot.log))"
[ "$(dd if=/tmp/card bs=512 skip=24 count=1 2>/dev/null | head -c 4)" = KRNL ] || fail "OOSK: not restored"
card KRNL; oneshot || fail "KRNL: not 0"
card XXXX; ! oneshot || fail "unexpected magic: 0"
if [ "${OOS_SELFHEAL_ONLY:-}" != oneshot ]; then
	rm -f /tmp/card; ! oneshot || fail "no card: 0"
fi

echo "== restore durability faults"
mkdir -p /fake
cat > /fake/dd <<'DD'
#!/bin/sh
# /tmp/card models the page cache, /tmp/card.disk the durable card. Only these test files are intercepted.
input= output= seek= skip= direct=0
for arg do
	case "$arg" in
		if=*) input=${arg#if=};; of=*) output=${arg#of=};;
		seek=*) seek=${arg#seek=};; skip=*) skip=${arg#skip=};;
		iflag=direct) direct=1;;
	esac
done
if [ "$input" = /tmp/card ]; then
	# Rewrite the input and remove the direct flag for the fake card, including on non-Linux test hosts.
	n=$#
	while [ "$n" -gt 0 ]; do
		arg=$1; shift; n=$((n - 1))
		case "$arg" in
			if=/tmp/card) [ "$direct" = 0 ] || arg=if=/tmp/card.disk;;
			iflag=direct) continue;;
		esac
		set -- "$@" "$arg"
	done
fi
# Emit plausible data but fail, or return a short sector successfully.
if [ "$input" = /tmp/card ] && [ "$skip" = 24 ] && { [ -e /tmp/card.written ] || [ "$OOS_FAULT" = initial-read ] || [ "$OOS_FAULT" = initial-short ]; }; then
	case "$OOS_FAULT" in
		read|initial-read) "$OOS_REAL_DD" "$@"; exit 1;;
		short|initial-short) # honour of= just as dd does
			if [ -n "$output" ]; then printf KRNL > "$output"; else printf KRNL; fi
			exit 0;;
	esac
fi
"$OOS_REAL_DD" "$@" || exit $?
if [ "$output" = /tmp/card ] && [ "$seek" = 24 ]; then
	: > /tmp/card.written
	case "$OOS_FAULT" in
		write) exit 1;;
		cache) ;; # a successful cached write, but the card still selects OOSK
		*) cp /tmp/card /tmp/card.disk || exit 1
			if [ "$OOS_FAULT" = corrupt ]; then
				printf X | "$OOS_REAL_DD" of=/tmp/card.disk bs=1 seek=12388 conv=notrunc 2>/dev/null || exit 1
			fi;;
	esac
fi
DD
cat > /fake/sync <<'SYNC'
#!/bin/sh
[ "$OOS_FAULT" != sync ]
SYNC
chmod +x /fake/dd /fake/sync
export OOS_REAL_DD=$(command -v dd)
for fault in write sync cache read short corrupt initial-read initial-short; do
	case "$fault" in initial-*) card KRNL;; *) card OOSK;; esac
	cp /tmp/card /tmp/card.disk; rm -f /tmp/card.written
	if PATH=/fake:$PATH OOS_FAULT=$fault oneshot; then
		fail "$fault: accepted an unconfirmed restore"
	fi
	case "$fault" in
		write) why='write or fsync FAILED';;
		sync) why='sync FAILED';;
		cache) why='RESTORE FAILED';;
		read) why='RESTORE read FAILED';;
		short) why='RESTORE short read';;
		corrupt) why='RESTORE sector differs';;
		initial-read) why='boot magic read FAILED';;
		initial-short) why='boot magic short read';;
	esac
	grep -q "$why" /tmp/oneshot.log || fail "$fault: missing diagnostic ($(cat /tmp/oneshot.log))"
done
rm -f /fake/dd /fake/sync /tmp/card /tmp/card.disk /tmp/card.written


# The durability cases also run alone with command shims on hosts without Linux direct I/O.
[ "${OOS_SELFHEAL_ONLY:-}" != oneshot ] || { [ "$ok" = 1 ]; exit $?; }

printf 'simulation build\n' > /etc/oos-build

echo "== no record"
rm -rf /ps; mkdir -p /ps
OOS_CRASH_MNT=/oos /bin/oos-crash-save > /tmp/out 2>&1 || fail "exit status"
grep -q "no crash record" /tmp/out || fail "no record: $(cat /tmp/out)"
[ ! -e /oos/crash ] || fail "no record: /oos/crash made"

echo "== one record"
records one
cp /ps/dmesg-ramoops-0 /tmp/dmesg
tail -c 65536 /ps/console-ramoops-0 > /tmp/console
save /oos || fail "exit status"
cmp -s /tmp/dmesg /oos/crash/1/dmesg-ramoops-0 || fail "dmesg copy"
cmp -s /tmp/console /oos/crash/1/console-ramoops-0 || fail "console copy (last 64 KiB)"
[ -s /oos/crash/1/saved-at-uptime ] || fail "saved-at-uptime"
[ ! -e /ps/dmesg-ramoops-0 ] && [ ! -e /ps/console-ramoops-0 ] || fail "pstore not cleared"
[ ! -e /oos/crash/.tmp ] || fail ".tmp left"
grep -q "Crash record" /tmp/st || fail "no status line"
grep -q "/oos/crash/1" /tmp/kmsg || fail "no kernel log line"

echo "== warning before the archive I/O"
mkdir -p /oos/early
records early
rm -f /tmp/st
cat > /fake/cp <<'CP'
#!/bin/sh
grep -q "Crash record" /tmp/st 2>/dev/null || exit 1
exec /bin/cp "$@"
CP
chmod +x /fake/cp
PATH=/fake:$PATH save /oos/early || fail "exit status"
[ ! -e /ps/dmesg-ramoops-0 ] || fail "no Crash record line before the first copy: $(cat /tmp/out)"
[ "$(grep -c "Crash record" /tmp/st)" = 2 ] || fail "early and final Crash record lines: $(cat /tmp/st)"
rm -f /fake/cp

echo "== too little space"
records small
save /small || fail "exit status"
grep -q "not saved" /tmp/out || fail "small: $(cat /tmp/out)"
[ -e /ps/dmesg-ramoops-0 ] || fail "small: pstore cleared"
[ ! -e /small/crash ] || fail "small: /small/crash made"

echo "== build id read failure keeps pstore"
records metadata
cat > /fake/cat <<'CAT'
#!/bin/sh
[ "$1" != /etc/oos-build ] || exit 1
exec /bin/cat "$@"
CAT
chmod +x /fake/cat
PATH=/fake:$PATH save /oos || fail "exit status"
[ -e /ps/dmesg-ramoops-0 ] || fail "metadata read failure: pstore cleared"
grep -q 'copy of build id failed' /tmp/out || fail "metadata read failure: no diagnostic"
rm -f /fake/cat
# This failure left .tmp, which the next case exercises as an interrupted save.

echo "== interrupted save"
mkdir -p /oos/crash/.tmp
echo half > /oos/crash/.tmp/dmesg-ramoops-0
records two
save /oos || fail "exit status"
[ "$(cat /oos/crash/.partial-2/dmesg-ramoops-0)" = half ] || fail "interrupted save not kept as .partial-2"
grep -q "two" /oos/crash/3/dmesg-ramoops-0 || fail "record after the partial not in 3"

echo "== at most 4"
for n in 4 5 6; do records "r$n"; save /oos || fail "exit status"; done
left=$(ls -a /oos/crash | grep -E '^(\.partial-)?[0-9]+$' | tr '\n' ' ')
[ "$left" = "3 4 5 6 " ] || fail "kept: $left"

echo "== crash directory parent sync failure keeps pstore"
mkdir -p /oos/sync-failure
records parent-sync
cat > /fake/sync <<'SYNC'
#!/bin/sh
for f do [ "$f" != /oos/sync-failure ] || exit 1; done
exec /bin/sync "$@"
SYNC
chmod +x /fake/sync
PATH=/fake:$PATH save /oos/sync-failure || fail "exit status"
[ -e /ps/dmesg-ramoops-0 ] || fail "parent sync failure: pstore cleared"
grep -q 'fsync.*parent failed' /tmp/out || fail "parent sync failure: no diagnostic"
rm -f /fake/sync
rm -rf /oos/sync-failure

echo "== kicker restarts"
mkdir -p /fake
printf '#!/bin/sh\nexit 3\n' > /fake/watchdog
chmod +x /fake/watchdog
rm -f /tmp/kmsg
PATH=/fake:$PATH OOS_WATCHDOG_DEV=/dev/null OOS_DEBUGFS=/nonexistent OOS_CMDLINE=/dev/null /bin/oos-watchdog > /tmp/wd 2>&1 &
pid=$!
await grep -q "again in 4 s" /tmp/kmsg || fail "no 4 s backoff"
kill $pid
grep -q "before open: hw unknown" /tmp/kmsg || fail "no before-open line: $(cat /tmp/kmsg)"
d=$(sed -n 's/.*exited (3) after [0-9]* s, starting it again in \([0-9]*\) s/\1/p' /tmp/kmsg | tr '\n' ' ')
[ "$d" = "1 2 4 " ] || fail "delays: $d"

# The handoff cases hold a backoff sleep until the supervisor kills it (/fake/sleep): an acknowledgement or a kicker
# started again then shows that USR1 cut the backoff short, on a computer of any speed. A backoff sleep that is not
# held returns at once.
hold_sleep() {
	cat > /fake/sleep <<SLEEP
#!/bin/sh
[ "\$1" = 0.1 ] && exec /bin/sleep "\$@"
$1
[ -e /tmp/held ] && exec /bin/sleep 100000
exit 0
SLEEP
	chmod +x /fake/sleep
	rm -f /tmp/held
}

echo "== kexec handoff"
printf '#!/bin/sh\ntrap "echo V >> /tmp/magic; exit 0" TERM\nwhile :; do sleep 1; done\n' > /fake/watchdog
rm -f /tmp/kmsg /tmp/magic /tmp/wd.handoff
PATH=/fake:$PATH OOS_WATCHDOG_DEV=/dev/null OOS_DEBUGFS=/nonexistent OOS_CMDLINE=/dev/null OOS_WATCHDOG_CONTROL=/tmp/wd \
	/bin/oos-watchdog > /tmp/wd 2>&1 &
pid=$!
await kicking_with_pid || fail "no kicker or pid file"
[ "$(cat /tmp/wd.pid)" = $pid ] || fail "pid file: $(cat /tmp/wd.pid)"
echo 1 > /tmp/wd.handoff
kill -USR1 $pid
await grep -q "kicker stopped" /tmp/kmsg || fail "no handoff line"
[ "$(cat /tmp/wd.ack 2>/dev/null)" = 1 ] || fail "no acknowledgement of the request"
! kicking || fail "kicker still running during the handoff"
[ ! -e /tmp/magic ] || fail "kicker got SIGTERM (magic close)"
# a kicker that came back now could only be wrong; waiting longer catches more, never fails a correct supervisor
sleep 1
! kicking || fail "kicker started again while the request exists"
rm /tmp/wd.handoff
await kicking || fail "kicker not started again"
await grep -q "started again" /tmp/kmsg || fail "no resume line"
await ack_gone || fail "acknowledgement left after the request"
! grep -q "exited" /tmp/kmsg || fail "handoff counted as an exit (backoff)"
kill $pid; for k in $(kickers); do kill -KILL $k; done

echo "== handoff during a backoff sleep, kexec start fails"
printf '#!/bin/sh\nexit 3\n' > /fake/watchdog
hold_sleep '[ "$1" = 8 ] && : > /tmp/held'
rm -f /tmp/kmsg /tmp/wd.handoff /tmp/wd.ack /tmp/wd.pid
PATH=/fake:$PATH OOS_WATCHDOG_DEV=/dev/null OOS_DEBUGFS=/nonexistent OOS_CMDLINE=/dev/null OOS_WATCHDOG_CONTROL=/tmp/wd \
	/bin/oos-watchdog > /tmp/wd 2>&1 &
pid=$!
# held in the 8 s sleep after the fourth exit
await grep -q "again in 8 s" /tmp/kmsg || fail "no 8 s backoff"
echo 1 > /tmp/wd.handoff
kill -USR1 $pid
await test -e /tmp/wd.ack || fail "no acknowledgement: the backoff was not interrupted"
# the updater gives up: the kicker must come back without the rest of the (held) backoff
rm /tmp/wd.handoff
await grep -q "started again" /tmp/kmsg || fail "kicker not back after the request was withdrawn"
await backoff_reset || fail "backoff not reset after the handoff: $(tail -n 3 /tmp/kmsg)"
kill $pid
rm -f /fake/sleep /tmp/held

# USR1 at a chosen point of the backoff: a fake `tee` (the log line) or a fake `sleep` (right after the backoff sleep
# started, before `wait`) writes the request and signals the supervisor once /tmp/trig names it. From then on the
# backoff sleeps are held, so only USR1 can end the one it interrupts.
usr1_at() {
	echo "== handoff with USR1 $2"
	go='rm -f /tmp/trig; : > /tmp/held; echo 1 > /tmp/wd.handoff; kill -USR1 $(cat /tmp/wd.pid)'
	if [ "$1" = tee ]; then
		hold_sleep ''
		printf '#!/bin/sh\n[ -e /tmp/trig ] && { %s; }\ncat >> "$2"\n' "$go" > /fake/tee
		chmod +x /fake/tee
	else
		hold_sleep "[ -e /tmp/trig ] && { $go; }"
	fi
	printf '#!/bin/sh\nexit 3\n' > /fake/watchdog
	rm -f /tmp/kmsg /tmp/wd.handoff /tmp/wd.ack /tmp/wd.pid /tmp/trig
	PATH=/fake:$PATH OOS_WATCHDOG_DEV=/dev/null OOS_DEBUGFS=/nonexistent OOS_CMDLINE=/dev/null OOS_WATCHDOG_CONTROL=/tmp/wd \
		/bin/oos-watchdog > /tmp/wd 2>&1 &
	pid=$!
	await grep -q "again in 1 s" /tmp/kmsg || fail "USR1 $2: no backoff"
	: > /tmp/trig
	await trig_fired || fail "USR1 $2: trigger did not fire"
	await test -e /tmp/wd.ack || fail "USR1 $2: not acknowledged, the backoff was not interrupted"
	rm -f /tmp/wd.handoff
	await grep -q "started again" /tmp/kmsg || fail "USR1 $2: kicker not back after the request was withdrawn"
	kill $pid
	rm -f /fake/tee /fake/sleep /tmp/held
}
usr1_at tee "during the log line"
usr1_at sleep "just before wait"

# A second updater can request takeover before the old handoff has noticed cancellation, or during ack cleanup.
second_request() {
	echo "== second handoff during $1"
	cat > /fake/$1 <<'SECOND'
#!/bin/sh
case "$(basename "$0"):$*" in
	sleep:0.1|rm:*wd.ack*)
		if [ -e /tmp/second-request ]; then
			/bin/rm -f /tmp/second-request
			printf '2\n' > /tmp/wd.handoff
			kill -USR1 "$(cat /tmp/wd.pid)"
		fi;;
esac
case "$(basename "$0")" in
	sleep) exec /bin/sleep "$@";;
	rm) exec /bin/rm "$@";;
esac
SECOND
	chmod +x /fake/$1
	printf '#!/bin/sh\ntrap "echo V >> /tmp/magic; exit 0" TERM\nwhile :; do sleep 1; done\n' > /fake/watchdog
	rm -f /tmp/wd.handoff /tmp/wd.ack /tmp/wd.pid /tmp/second-request /tmp/magic
	PATH=/fake:$PATH OOS_WATCHDOG_DEV=/dev/null OOS_DEBUGFS=/nonexistent OOS_CMDLINE=/dev/null OOS_WATCHDOG_CONTROL=/tmp/wd \
		/bin/oos-watchdog > /tmp/wd 2>&1 &
	pid=$!
	await kicking_with_pid || fail "second handoff: no kicker or pid file"
	echo 1 > /tmp/wd.handoff; kill -USR1 "$pid"
	await acked 1 || fail "first request not acknowledged"
	: > /tmp/second-request
	# For sleep, replace the request while hand_over still waits. For rm, trigger during its ack cleanup.
	[ "$1" != rm ] || /bin/rm -f /tmp/wd.handoff
	await acked 2 || fail "second request lost during $1"
	! kicking || fail "kicker running during second handoff"
	[ ! -e /tmp/magic ] || fail "second handoff sent a magic close"
	/bin/rm -f /tmp/wd.handoff /tmp/second-request
	await kicking || fail "second handoff: no kicker after cancellation"
	kill "$pid"; for k in $(kickers); do kill -KILL "$k"; done
	/bin/rm -f /fake/$1
}
second_request sleep
second_request rm

echo "== automatic restarts only after the boot selection"
grep -qE '[ "]panic=' /etc/oos-board-under-test && fail "panic= in the bootargs: a panic before oos-oneshot would restart the selected image"
grep -q 'softlockup_panic=' /etc/oos-board-under-test && fail "softlockup panic armed before the stock-selection check"
awk '/^if \/bin\/oos-oneshot; then$/ { inif = 1; next } inif && /^else$/ { inif = 0 }
	/kernel\/panic|oos-watchdog/ && !/^#/ && !/^[ \t]*#/ { if (inif) good++; else bad++ }
	END { exit !(good == 2 && bad == 0) }' /etc/rcS-under-test || fail "rcS: panic or watchdog outside 'if /bin/oos-oneshot'"
awk '/^if \/bin\/oos-oneshot; then$/ { inif = 1; next } inif && /^else$/ { inif = 0 }
	/softlockup_panic/ && !/^[ \t]*#/ { if (inif) n++ }
	END { exit !(n == 1) }' /etc/rcS-under-test || fail "rcS: softlockup_panic not armed after safe stock selection"

echo "== failed stock selection warns on the start-up screen"
printf '#!/bin/sh\nexit 1\n' > /fake/oos-oneshot; chmod +x /fake/oos-oneshot
awk '/^# 0b\)/ { exit } { print }' /etc/rcS-under-test | \
	sed 's|/bin/oos-oneshot|/fake/oos-oneshot|g; s|/proc/sys/kernel/|/tmp/selfheal-sysctl-|g; s|/dev/kmsg|/tmp/kmsg|g' > /tmp/rcS-gate
rm -f /tmp/selfheal-sysctl-panic /tmp/selfheal-sysctl-softlockup_panic
sh /tmp/rcS-gate
[ ! -e /tmp/selfheal-sysctl-panic ] && [ ! -e /tmp/selfheal-sysctl-softlockup_panic ] || fail "failed selection armed panic"
grep -q 'warn.*boot.*stock boot selection unconfirmed' /tmp/oos-boot.status || fail "failed selection: no start-up warning"

[ $ok = 1 ] && echo "selfheal-sim: all passed" || { echo "selfheal-sim: FAILED"; exit 1; }
TEST

# busybox ash (as in the image) and dash
for image in alpine:3.20 debian:12-slim; do
echo "-- $image"
docker run --rm --tmpfs /oos:size=16m --tmpfs /small:size=2m \
	-v "$bin/oos-crash-save:/bin/oos-crash-save:ro" -v "$bin/oos-watchdog:/bin/oos-watchdog:ro" \
	-v "$bin/oos-oneshot:/bin/oos-oneshot:ro" -v "$root/rootfs/initramfs/etc/rcS:/etc/rcS-under-test:ro" \
	-v "$root/rootfs/board/dho/etc/oos-board:/etc/oos-board-under-test:ro" \
	-v "${TMPDIR:-/tmp}/oos-selfheal-sim.test:/test:ro" "$image" sh /test
done
