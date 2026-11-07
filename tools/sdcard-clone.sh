#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# sdcard-clone.sh - clone the scope's boot SD card on macOS, and write a clone to the second card.
#
#   sdcard-clone.sh list                       list external/removable disks
#   sdcard-clone.sh read  diskN  out.img       card -> image (+ out.img.sha256, + layout report)
#   sdcard-clone.sh write in.img diskN         image -> card (asks you to type the disk id), then verifies
#   sdcard-clone.sh verify in.img diskN        compare a card with an image (sha256 over the image length)
#
# Safety: refuses the boot disk and any disk that is not removable/external SD or USB media;
# refuses to write an image that is larger than the card; never runs without an explicit diskN.
set -eu

die() { echo "error: $*" >&2; exit 1; }
here=$(cd "$(dirname "$0")" && pwd)

info() { diskutil info "/dev/$1" 2>/dev/null; }
field() { info "$1" | awk -F': *' -v k="$2" '$1 ~ "^ *"k"$" {print $2; exit}'; }

check_disk() {
	d=$1
	case "$d" in disk[0-9]*) ;; *) die "give a whole disk like disk4, not '$d'";; esac
	case "$d" in *s[0-9]*) die "'$d' is a partition, give the whole disk";; esac
	info "$d" >/dev/null || die "no such disk: $d"
	boot=$(diskutil info / | awk -F': *' '/Part of Whole/ {print $2}')
	[ "$d" != "$boot" ] || die "$d holds the running macOS"
	proto=$(field "$d" "Protocol")
	removable=$(field "$d" "Removable Media")
	location=$(field "$d" "Device Location")
	virtual=$(field "$d" "Virtual")
	[ "$virtual" != "Yes" ] || die "$d is a virtual disk (disk image)"
	case "$proto" in
		"Secure Digital"|USB) ;;
		*) die "$d protocol is '$proto' (expected Secure Digital or USB)";;
	esac
	case "$removable$location" in
		*Removable*|*External*) ;;
		*) die "$d is neither removable nor external ($removable / $location)";;
	esac
}

disk_bytes() { field "$1" "Disk Size" | sed -E 's/.*\(([0-9]+) Bytes\).*/\1/'; }

describe() {
	d=$1
	printf "%s: %s, %s bytes, protocol %s, %s\n" "$d" "$(field "$d" "Device / Media Name")" "$(disk_bytes "$d")" "$(field "$d" "Protocol")" "$(field "$d" "Removable Media")"
}

copy() { # copy src dst bytes
	if command -v pv >/dev/null 2>&1; then
		pv -s "$3" "$1" | dd of="$2" bs=4m 2>/dev/null
	else
		dd if="$1" of="$2" bs=4m status=progress
	fi
}

cmd=${1:-}
case "$cmd" in
list)
	diskutil list physical | awk '/^\/dev\/disk/ {print $1}' | sed 's|/dev/||' | while read -r d; do
		p=$(field "$d" "Protocol"); case "$p" in "Secure Digital"|USB) describe "$d";; esac
	done
	;;
read)
	[ $# -eq 3 ] || die "usage: $0 read diskN out.img"
	d=$2; out=$3
	check_disk "$d"; describe "$d"
	[ ! -e "$out" ] || die "$out exists, refusing to overwrite"
	bytes=$(disk_bytes "$d")
	free=$(df -k "$(dirname "$out")" | awk 'NR==2 {print $4 * 1024}')
	[ "$free" -gt "$bytes" ] || die "not enough free space for $bytes bytes next to $out"
	diskutil unmountDisk "/dev/$d" >/dev/null
	echo "reading /dev/r$d -> $out ($bytes bytes)"
	if command -v pv >/dev/null 2>&1; then
		sudo sh -c 'pv -s "$1" "$2" > "$3"' sh "$bytes" "/dev/r$d" "$out"
	else
		sudo dd if="/dev/r$d" of="$out" bs=4m status=progress
	fi
	sudo chown "$(id -u):$(id -g)" "$out"
	[ "$(stat -f %z "$out")" -eq "$bytes" ] || die "image size differs from the card size"
	echo "hashing image and card ..."
	h_img=$(shasum -a 256 "$out" | cut -d' ' -f1)
	h_dev=$(sudo dd if="/dev/r$d" bs=4m 2>/dev/null | shasum -a 256 | cut -d' ' -f1)
	[ "$h_img" = "$h_dev" ] || die "sha256 mismatch: image $h_img, card $h_dev"
	echo "$h_img  $(basename "$out")" > "$out.sha256"
	echo "ok: sha256 $h_img"
	py="$here/.venv/bin/python"; [ -x "$py" ] || py=python3
	"$py" "$here/sdcard-layout.py" "$out" | tee "$out.layout.txt"
	;;
write)
	[ $# -eq 3 ] || die "usage: $0 write in.img diskN"
	img=$2; d=$3
	[ -f "$img" ] || die "no image $img"
	check_disk "$d"; describe "$d"
	ib=$(stat -f %z "$img"); db=$(disk_bytes "$d")
	[ "$ib" -le "$db" ] || die "image ($ib bytes) is larger than the card ($db bytes)"
	if [ -f "$img.sha256" ] && [ "${IMAGE_VERIFIED:-0}" != 1 ]; then
		echo "checking $img against $img.sha256 ..."
		(cd "$(dirname "$img")" && shasum -a 256 -c "$(basename "$img").sha256") || die "image does not match its .sha256"
	fi
	echo "ALL DATA ON $d WILL BE REPLACED by $img."
	printf "type the disk id (%s) to continue: " "$d"; read -r answer
	[ "$answer" = "$d" ] || die "aborted"
	diskutil unmountDisk "/dev/$d" >/dev/null
	if command -v pv >/dev/null 2>&1; then
		sudo sh -c 'pv -s "$1" "$2" > "$3"' sh "$ib" "$img" "/dev/r$d"
	else
		sudo dd if="$img" of="/dev/r$d" bs=4m status=progress
	fi
	sync
	"$0" verify "$img" "$d"
	diskutil eject "/dev/$d" >/dev/null && echo "ejected $d"
	;;
verify)
	[ $# -eq 3 ] || die "usage: $0 verify in.img diskN"
	img=$2; d=$3
	check_disk "$d"
	ib=$(stat -f %z "$img")
	diskutil unmountDisk "/dev/$d" >/dev/null 2>&1 || true
	if [ -f "$img.sha256" ] && [ "${IMAGE_VERIFIED:-0}" = 1 ]; then h_img=$(cut -d' ' -f1 "$img.sha256"); else h_img=$(shasum -a 256 "$img" | cut -d' ' -f1); fi
	h_dev=$(sudo dd if="/dev/r$d" bs=4m count=$(( (ib + 4194303) / 4194304 )) 2>/dev/null | head -c "$ib" | shasum -a 256 | cut -d' ' -f1)
	[ "$h_img" = "$h_dev" ] && echo "verify ok: $h_img" || die "verify FAILED: image $h_img, card $h_dev"
	;;
*)
	sed -n '2,10p' "$0"; exit 1;;
esac
