#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# gel-sim.sh - the install without opening the scope, on a file laid out like a stock card: a Linux container with the
# paths of the stock system, a USB stick folder and mksh as /system/bin/sh, running the USB install file of the board
# family OOS_BOARD (dho by default, tools/lib/board.sh), boot/out/<OOS_GEL_STEM>_Install.GEL, through the steps of the
# stock update. The name of the file, the stock partition table (the loader's uboot and trust, then the marks of
# OOS_PARM_MARKS) and where boot, recovery and backup lie come from the family's rootfs/board/<name>/etc/oos-board.
# Needs Docker and a finished boot/build-dev-image.sh of the same family.
#
#   tools/gel-sim.sh [stock do_update.sh]    the real script of a stock update file, if at hand; else the same steps here
#
# OOS_CAL_DIR: a folder with vendor.bin, for the model check. Without it the run uses oos-any-model.txt.
# OOS_GEL_OLD: the GEL of an older build, installed first: the run then shows the update of an older build.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
. "$root/tools/lib/docker.sh"
. "$root/tools/lib/board.sh"
board=$(oos_board "$root") || exit 1
. "$root/rootfs/board/$board/etc/oos-board"
G=${OOS_GEL_STEM}_Install.GEL
gel="$root/boot/out/$G"
[ -f "$gel" ] || { echo "run boot/build-dev-image.sh first" >&2; exit 1; }
w="${TMPDIR:-/tmp}/oos-gel-sim"
rm -rf "$w"; mkdir -p "$w/rigol-data"
[ -z "${1:-}" ] || cp "$1" "$w/do_update.sh"
[ -n "${OOS_CAL_DIR:-}" ] && cp "$OOS_CAL_DIR/vendor.bin" "$w/rigol-data/" 2>/dev/null || true
echo calibration > "$w/rigol-data/cal_example.hex"
cp "$gel" "$w/"
[ -z "${OOS_GEL_OLD:-}" ] || cp "$OOS_GEL_OLD" "$w/old.GEL"
python3 -I - "$w/card" "$OOS_PARM_LBA" "$OOS_BOOT_LBA" "$OOS_RECOVERY_LBA" "$OOS_PARM_MARKS" <<'PY'
import sys
S = 512
card, parm_lba, boot, recovery, marks = sys.argv[1], *map(int, sys.argv[2:5]), sys.argv[5]
parm = (b"PARM\0\0\0\0FIRMWARE_VER:9.0\nCMDLINE:mtdparts=rk29xxnand:0x00002000@0x00002000(uboot),0x00002000@0x00004000(trust),"
        + ",".join(marks.split()).encode() + b"\n")
with open(card, "wb") as f:
    f.truncate(600000 * S)
    for lba, b in ((64, b"LOADER"), (parm_lba, parm), (parm_lba + 0x2000, b"UBOOT"), (parm_lba + 0x4000, b"TRUST"),
                   (boot, b"KRNL\0\0\x10\0stock boot" + bytes(range(256)) * 40),
                   (recovery, b"KRNL\0\0\x20\0stock recovery")):
        f.seek(lba * S); f.write(b)
PY

docker run --rm --platform linux/arm64 -v "$w:/w" -e G="$G" -e PARM="$OOS_PARM_LBA" -e BOOT="$OOS_BOOT_LBA" \
	-e BOOTN="$OOS_BOOT_SECTORS" -e REC="$OOS_RECOVERY_LBA" -e BAK="$OOS_BACKUP_LBA" "$OOS_UBUNTU" bash -euc '
	apt-get update >/dev/null; DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends mksh >/dev/null
	mkdir -p /system/bin /rigol/shell /mnt/media_rw/ABCD-1234 /rigol
	ln -s /usr/bin/mksh /system/bin/sh; ln -s /bin/bash /system/bin/bash
	# the busybox of the stock system unpacks gzip by itself
	printf "#!/bin/sh\nshift\nexec tar \"\$@\"\n" > /system/bin/busybox; chmod +x /system/bin/busybox
	cp -r /w/rigol-data /rigol/data
	if [ -f /w/do_update.sh ]; then cp /w/do_update.sh /rigol/shell/; else cat > /rigol/shell/do_update.sh <<UPD
#!/system/bin/bash
mkdir -p /mnt/tmp /mnt/bak; cd /mnt/tmp
/system/bin/busybox tar -xvf "\$1" >/dev/null 2>&1 || exit 1
/mnt/tmp/shell/do_extract.sh "\$2"
UPD
	fi
	chmod +x /rigol/shell/do_update.sh
	S=/mnt/media_rw/ABCD-1234; cp /w/$G $S/
	[ -f /rigol/data/vendor.bin ] || touch $S/oos-any-model.txt
	export OOS_GEL_CARD=/w/card
	magic() { dd if=/w/card bs=512 skip=$1 count=1 2>/dev/null | head -c 4; echo; }
	upd() { su -s /bin/sh -c "OOS_GEL_CARD=/w/card /rigol/shell/do_update.sh $S/$G" root; }
	sums() { dd if=/w/card bs=512 skip=$1 count=$2 2>/dev/null | sha256sum | cut -c1-16; }

	if [ -f /w/old.GEL ]; then
		echo "== an older build first"; cp /w/old.GEL $S/$G; upd >/dev/null; echo "exit $?"
		printf KRNL | dd of=/w/card bs=512 seek=$BOOT conv=notrunc 2>/dev/null; cp /w/$G $S/
	fi
	echo "== dry run"; touch $S/oos-dry-run.txt; h0=$(sha256sum < /w/card); upd; echo "exit $?"; rm $S/oos-dry-run.txt
	[ "$h0" = "$(sha256sum < /w/card)" ] && echo "card unchanged"
	[ -f $S/OOS-rescue/SHA256SUMS ] && (cd $S/OOS-rescue && sha256sum -c SHA256SUMS)
	ls /rigol/data $S/OOS-rescue/rigol-data | head -8

	echo "== install"; b=$(sums $BOOT $BOOTN); upd; echo "exit $?"
	echo "boot magic: $(magic $BOOT)"; echo "backup magic: $(magic $BAK)"
	[ "$b" = "$(sums $BAK $BOOTN)" ] && echo "backup = stock boot"
	echo "recovery: $(dd if=/w/card bs=512 skip=$REC count=1 2>/dev/null | head -c 8)"
	grep -E "newer|kept, only" $S/oos-install.log || true
	ls /mnt/tmp

	echo "== after a start of OOS (one-shot), the file selects it again"; printf KRNL | dd of=/w/card bs=512 seek=$BOOT conv=notrunc 2>/dev/null
	upd; echo "exit $?"; echo "boot magic: $(magic $BOOT)"

	echo "== a card that is not stock"; dd if=/dev/zero of=/w/card bs=512 seek=$PARM count=4 conv=notrunc 2>/dev/null
	set +e; upd; echo "exit $?"; set -e
	echo "== log"; tail -22 $S/oos-install.log
'
