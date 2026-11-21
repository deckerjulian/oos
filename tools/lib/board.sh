# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# board.sh - sourced by the host tools that work for one board family (ADR 0067): which family, and which families
# the tree has. The family is OOS_BOARD, the switch the build reads (hal/board_cfg.rs), `dho` when it is not set; the
# families are the folders of rootfs/board/, which hal/tests/board_families.rs keeps equal to `FAMILIES` of
# hal/src/target.rs. The functions keep their variables to the oos_ prefix: they run in the caller's shell.
#
#   oos_board ROOT        prints the family of OOS_BOARD, `dho` without it; fails, saying why on standard error, for
#                         a name that is no folder of ROOT/rootfs/board/ with its etc/oos-board
#   oos_families ROOT     prints every family of ROOT/rootfs/board/, one a line, `dho` first, the rest by name

oos_board() {
	oos_b=${OOS_BOARD:-dho}
	# the letters spelt out: in a UTF-8 locale a range such as a-z can take capitals too, and a file system that
	# ignores case (macOS) would then find rootfs/board/DHO/ for dho
	case $oos_b in
	*[!abcdefghijklmnopqrstuvwxyz0123456789_-]* | -*)
		echo "OOS_BOARD='$oos_b' is no name of a board family" >&2
		return 1
		;;
	esac
	if [ ! -f "$1/rootfs/board/$oos_b/etc/oos-board" ]; then
		echo "OOS_BOARD='$oos_b': no board family rootfs/board/$oos_b/ (has: $(oos_families "$1" | tr '\n' ' '))" >&2
		return 1
	fi
	echo "$oos_b"
}

oos_families() {
	for oos_d in "$1"/rootfs/board/*/; do
		[ -f "${oos_d}etc/oos-board" ] && basename "$oos_d"
	done | LC_ALL=C sort | awk '$0 == "dho" { print; next } { rest[n++] = $0 } END { for (i = 0; i < n; i++) print rest[i] }'
}
