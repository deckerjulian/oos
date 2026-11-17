#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# ratchet.sh - numbers of the tree that may only go down (docs/adr/process/0037), kept in tools/ratchet.txt:
#
#   lines FILE     the lines of every Rust file with more than 2000 lines; a file without a line here has at most 2000
#   allow CRATE    how often a crate switches a lint off (`allow(`)
#   unsafe CRATE   the lines of a crate with `unsafe` in the code
#
#   tools/ratchet.sh                 measure and compare with tools/ratchet.txt; exit 1 when a number differs
#   tools/ratchet.sh --lower         write the numbers that went down into tools/ratchet.txt (never one that went up)
#   tools/ratchet.sh --base COMMIT   name every number of tools/ratchet.txt that is higher than in COMMIT, or new
#   tools/ratchet.sh --init          write the file from the tree; only when there is none yet
#
# A number that went up fails the check: make the thing smaller, or raise the number in the file by hand and say why
# in the pull request; the raise is then in the diff for everyone to see. A number that went down fails it too, so
# the file never allows more than the tree needs: run --lower. Touches no scope.
set -eu
cd "$(dirname "$0")/.."
file=tools/ratchet.txt
limit=2000
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

measure() {
	git ls-files '*.rs' | while IFS= read -r f; do
		n=$(wc -l <"$f" | tr -d ' ')
		[ "$n" -le $limit ] || echo "$n lines $f"
	done
	for c in hal dsp ui fpgad; do
		git ls-files -- "$c" | grep '\.rs$' >"$tmp/rs"
		# shellcheck disable=SC2046
		echo "$(cat $(cat "$tmp/rs") | grep -c 'allow(') allow $c"
		# `unsafe` before any comment on the line
		# shellcheck disable=SC2046
		echo "$(cat $(cat "$tmp/rs") | grep -cE '^([^/]|/[^/])*\<unsafe\>') unsafe $c"
	done
}

# NUMBER KIND NAME lines -> "KIND NAME<tab>NUMBER", sorted by key
keyed() {
	awk '{ n = $1; $1 = ""; sub(/^ /, ""); print $0 "\t" n }' "$1" | sort
}

case ${1:-} in
--base)
	[ -n "${2:-}" ] || { echo "usage: $0 --base COMMIT" >&2; exit 2; }
	git show "$2:$file" >"$tmp/base" 2>/dev/null || { echo "no $file in $2: every number is new"; exit 0; }
	keyed "$tmp/base" >"$tmp/a"
	keyed "$file" >"$tmp/b"
	join -t "$(printf '\t')" -a 2 -e new -o 0,1.2,2.2 "$tmp/a" "$tmp/b" |
		awk -F '\t' '$2 == "new" { print "new: " $1 " " $3; next } $3 + 0 > $2 + 0 { print "raised: " $1 " " $2 " -> " $3 }'
	exit 0
	;;
--init)
	[ ! -s "$file" ] || { echo "$file exists: numbers only go down (--lower), or are raised by hand" >&2; exit 2; }
	measure | sort -k2 >"$file"
	echo "wrote $(wc -l <"$file" | tr -d ' ') numbers to $file"
	exit 0
	;;
'' | --lower) ;;
*) echo "usage: $0 [--lower | --base COMMIT | --init]" >&2; exit 2 ;;
esac

measure | sort -k2 >"$tmp/now"
[ -f "$file" ] || : >"$file"
keyed "$file" >"$tmp/a"
keyed "$tmp/now" >"$tmp/b"
join -t "$(printf '\t')" -a 1 -a 2 -e - -o 0,1.2,2.2 "$tmp/a" "$tmp/b" >"$tmp/both"
bad=0
: >"$tmp/out"
while IFS="$(printf '\t')" read -r key was now; do
	case $key in lines\ *) floor=$limit ;; *) floor=0 ;; esac
	if [ "$now" = - ]; then
		# gone: a file at or under the limit, or removed
		if [ "${1:-}" = --lower ]; then
			echo "lowered: $key $was -> gone"
		else
			echo "down: $key $was -> at most $floor (run $0 --lower)"
			bad=1
			echo "$was $key" >>"$tmp/out"
		fi
		continue
	fi
	if [ "$was" = - ]; then
		case $key in
		lines\ *) echo "up: $key is new with $now lines, over $limit" ;;
		*) echo "up: $key is new with $now" ;;
		esac
		bad=1
		continue
	fi
	if [ "$now" -gt "$was" ]; then
		echo "up: $key $was -> $now"
		bad=1
		echo "$was $key" >>"$tmp/out"
	elif [ "$now" -lt "$was" ]; then
		if [ "${1:-}" = --lower ]; then
			echo "lowered: $key $was -> $now"
			echo "$now $key" >>"$tmp/out"
		else
			echo "down: $key $was -> $now (run $0 --lower)"
			bad=1
			echo "$was $key" >>"$tmp/out"
		fi
	else
		echo "$now $key" >>"$tmp/out"
	fi
done <"$tmp/both"
if [ "${1:-}" = --lower ]; then
	sort -k2 "$tmp/out" >"$file"
fi
[ $bad = 0 ] && echo "ratchet: $(wc -l <"$file" | tr -d ' ') numbers as recorded"
exit $bad
