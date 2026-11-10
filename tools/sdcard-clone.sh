#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# sdcard-clone.sh - clone the scope's boot SD card on macOS, and write a clone to the second card.
#
#   sdcard-clone.sh list                       list external/removable disks
#   sdcard-clone.sh read  diskN  out.img       card -> image (+ out.img.sha256, + layout report)
#   sdcard-clone.sh write in.img diskN         image -> card (asks you to type the disk id), then verifies
#   sdcard-clone.sh verify in.img diskN        compare a card with an image (sha256 over the image length)
#   sdcard-clone.sh fit in.img diskN|BYTES out.img
#                                              a copy of a card image for a smaller card: the ext4 of the last
#                                              partition (userdata) shrunk to what is left, in Docker. in.img is only read
#
# Safety: refuses the boot disk and any disk that is not removable/external SD or USB media;
# refuses to write an image that is larger than the card (fit makes one that is not); never runs without an explicit diskN.
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
	[ "$ib" -le "$db" ] || die "image ($ib bytes) is larger than the card ($db bytes); for a smaller card: $0 fit $img $d fitted.img"
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
fit)
	[ $# -eq 4 ] || die "usage: $0 fit in.img diskN|BYTES out.img"
	img=$2; target=$3; out=$4
	[ -f "$img" ] || die "no image $img"
	[ ! -e "$out" ] || die "$out exists, refusing to overwrite"
	case "$target" in
		disk*) check_disk "$target"; tb=$(disk_bytes "$target");;
		""|*[!0-9]*) die "give the card (diskN) or its size in bytes, not '$target'";;
		*) tb=$target;;
	esac
	ib=$(stat -f %z "$img")
	[ "$ib" -gt "$tb" ] || die "the image fits as it is ($ib <= $tb bytes): $0 write $img diskN"
	# the last partition of the stock table (PARM at LBA 8192, offsets relative to it): it has to run to the end of the
	# card ("-"), so a smaller card only makes it shorter, and it has to be the only one past the new end
	last=$(python3 -I - "$img" <<'EOF'
import re, struct, sys
f = open(sys.argv[1], "rb"); f.seek(8192 * 512); raw = f.read(64 * 512)
if raw[:4] != b"PARM":
    sys.exit("no PARM block at LBA 8192: not a DHO card image")
txt = raw[8:8 + struct.unpack_from("<I", raw, 4)[0]].decode("latin1")
m = re.search(r"mtdparts=[^:]+:(\S+)", txt)
parts = [re.match(r"(-|0x[0-9a-fA-F]+)@(0x[0-9a-fA-F]+)\(([^):]+)", p) for p in m.group(1).split(",")] if m else []
if not parts or not all(parts):
    sys.exit("no readable partition table in PARM")
size, off, name = parts[-1].groups()
if size != "-":
    sys.exit(f"the last partition ({name}) has a fixed size, it does not end with the card")
print(name, 8192 + int(off, 16))
EOF
) || die "cannot read the partition table of $img"
	name=${last% *}; lba=${last#* }
	off=$((lba * 512))
	# whole 4 MiB below the end of the new card
	avail=$(( (tb - off) / 4194304 * 4194304 ))
	[ "$avail" -gt 0 ] || die "$name starts at $off bytes, past the end of $tb bytes"
	free=$(df -k "$(dirname "$out")" | awk 'NR==2 {print $4 * 1024}')
	[ "$free" -gt "$tb" ] || die "not enough free space for $tb bytes next to $out"
	docker info >/dev/null 2>&1 || die "Docker is not running"
	echo "$name at LBA $lba: its ext4 shrunk to $avail bytes, for a card of $tb bytes"
	indir=$(cd "$(dirname "$img")" && pwd); outdir=$(mkdir -p "$(dirname "$out")" && cd "$(dirname "$out")" && pwd)
	# the partition is copied into a Docker volume (sparse: only what is used takes room), checked and shrunk there;
	# in.img is mounted read only
	docker run --rm -e IMG="$(basename "$img")" -e OUT="$(basename "$out")" -e OFF="$off" -e AVAIL="$avail" -e NAME="$name" \
		-e OUT_OWNER="$(id -u):$(id -g)" -v "$indir:/in:ro" -v "$outdir:/out" -v oos-sdfit:/w alpine:3.22 sh -euc '
		apk add --no-cache e2fsprogs e2fsprogs-extra coreutils >/dev/null
		p=/w/part; rm -f $p
		trap "rm -f $p" EXIT
		dd if="/in/$IMG" of=$p bs=4M iflag=skip_bytes skip="$OFF" conv=sparse status=none
		fsbytes() { dumpe2fs -h $p 2>/dev/null | awk -F: "/^Block count:/ {c=\$2} /^Block size:/ {s=\$2} END {print c * s}"; }
		[ "$(fsbytes)" -gt 0 ] || { echo "$NAME holds no ext2/3/4 file system"; exit 1; }
		# resize2fs wants a freshly checked file system; this changes only the copy
		rc=0; e2fsck -f -y $p >/w/fsck.log 2>&1 || rc=$?
		[ $rc -le 1 ] || { tail -20 /w/fsck.log; echo "e2fsck returned $rc on $NAME"; exit 1; }
		# 1 is usual: the stock system is switched off at the mains, its journal is replayed
		[ $rc = 0 ] || echo "e2fsck repaired the copy of $NAME, usually the journal of the last power-off (/w/fsck.log in the volume oos-sdfit)"
		if [ "$(fsbytes)" -gt "$AVAIL" ]; then
			resize2fs $p "$((AVAIL / 1024))K" >/w/resize.log 2>&1 || { cat /w/resize.log; echo "the files on $NAME need more than $AVAIL bytes"; exit 1; }
		fi
		e2fsck -f -n $p >/dev/null 2>&1 || { echo "$NAME is not clean after the resize"; exit 1; }
		n=$(fsbytes)
		dd if="/in/$IMG" of="/out/$OUT" bs=4M iflag=count_bytes count="$OFF" status=none
		dd if=$p of="/out/$OUT" bs=4M iflag=count_bytes count="$n" oflag=seek_bytes seek="$OFF" conv=sparse,notrunc status=none
		truncate -s $((OFF + n)) "/out/$OUT"
		chown "$OUT_OWNER" "/out/$OUT"
		echo "$NAME: $n bytes of file system, image $((OFF + n)) bytes"
	' || { rm -f "$out"; die "fit failed, $img is unchanged"; }
	ob=$(stat -f %z "$out")
	[ "$ob" -le "$tb" ] || die "$out came out larger than the card ($ob > $tb)"
	h=$(shasum -a 256 "$out" | cut -d' ' -f1)
	echo "$h  $(basename "$out")" > "$out.sha256"
	echo "ok: $out ($ob bytes, sha256 $h). Next: $0 write $out diskN"
	;;
*)
	sed -n '2,13p' "$0"; exit 1;;
esac
