#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# plan-matrix.sh - the register sequences of a fixed set of acquisitions of the board family OOS_BOARD (default dho),
# for comparing two builds of oos-fpgad. The matrix of each family is tools/board/<family>/plan-matrix.sh (ADR 0067);
# this script runs it with the same arguments. On a unit, tools/device-deploy.sh uploads the family's matrix itself.
#
#   tools/plan-matrix.sh [path/to/oos-fpgad] > plan-OLD.txt
board=${OOS_BOARD:-dho}
case $board in
*[!a-z0-9_-]* | -*) echo "plan-matrix.sh: OOS_BOARD='$board' is no name of a board family" >&2; exit 2 ;;
esac
m="$(dirname "$0")/board/$board/plan-matrix.sh"
[ -f "$m" ] || { echo "plan-matrix.sh: no matrix for the board family '$board' ($m)" >&2; exit 2; }
exec sh "$m" "$@"
