#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# plan-matrix.sh of the MHO900 (tools/plan-matrix.sh runs it for OOS_BOARD=mho; ADR 0067): OOS builds no register
# sequences for this board yet (docs/mho.md, ADR 0028), so the matrix is what oos-fpgad says about the unit and that
# it refuses every hardware command. Two builds compare equal as long as that stays so.
#
#   OOS_BOARD=mho tools/plan-matrix.sh [path/to/oos-fpgad] > plan-OLD.txt
f=${1:-oos-fpgad}
echo "### help"
"$f" help 2>&1
echo "### plan"
"$f" plan 2>&1
echo "exit $?"
