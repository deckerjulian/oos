#!/system/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# Started by the stock update (Storage > Upgrade) as root, from the unpacked update file in /mnt/tmp: the first install of
# OOS without opening the scope (docs/install-usb.md). The stock update ends here: nothing of it runs, no FPGA
# flash, no /rigol change. The work is done by oos-update gel-install; its exit code shows as success or failure on the
# screen, its report goes to oos-install.log on the stick.
#
# Files on the stick that change what happens:
#   oos-dry-run.txt    all checks and the rescue set, the card is not written
#   oos-any-model.txt  also on a model OOS never ran on
#   oos-reinstall.txt  write the stick's build also when the card holds a valid OOS image (else that one is kept and only
#                      selected: a newer build installed over the network stays)
SRC=/mnt/tmp/oos

# the stick holds the update file, in its top folder
STICK=
for d in /mnt/media_rw/*; do
	for f in "$d"/@OOS_GEL_STEM@*.GEL; do
		[ -f "$f" ] && STICK=$d
	done
done
[ -n "$STICK" ] || exit 3
LOG=$STICK/oos-install.log
exec >> "$LOG" 2>&1 || exit 4
echo "---- OOS install, $(date)"

SAVE=$STICK/OOS-rescue
mkdir -p "$SAVE" || exit 5
# calibration and licences: read here, kept on the stick (a copy that already exists stays as it is)
if [ ! -d "$SAVE/cal-data" ]; then
	cp -R /rigol/data "$SAVE/cal-data.new" && mv "$SAVE/cal-data.new" "$SAVE/cal-data" || { echo "cannot copy /rigol/data"; exit 6; }
fi

args=--execute
[ -f "$STICK/oos-dry-run.txt" ] && args=
[ -f "$STICK/oos-any-model.txt" ] && args="$args --any-model"
[ -f "$STICK/oos-reinstall.txt" ] && args="$args --reinstall"
"$SRC/oos-update" gel-install "$SRC/oos.oosupd" --save "$SAVE" $args ${OOS_GEL_CARD:+--card "$OOS_GEL_CARD"} ${OOS_GEL_VENDOR:+--vendor "$OOS_GEL_VENDOR"}
rc=$?
sync
echo "exit code $rc"
rm -rf "$SRC" /mnt/tmp/shell
exit $rc
