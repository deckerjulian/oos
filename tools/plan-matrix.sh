#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# plan-matrix.sh - register sequences of a fixed set of acquisitions, for comparing two builds of oos-fpgad.
#
#   tools/plan-matrix.sh [path/to/oos-fpgad] > plan-OLD.txt
#
# `oos-fpgad plan` only prints, it touches no hardware, so this is safe on the unit (busybox sh) and on the Mac. Run it
# with the old build still on the unit BEFORE installing a new image, and with the new build, then `diff`: the analog
# path must not change at 1.25 GHz. Needs the calibration files: on the unit
# /rigol/data, on a host computer `OOS_CAL_DIR=local/cal`.
f=${1:-oos-fpgad}
run() {
    echo "### $*"
    "$f" plan "$@" 2>&1
}
for mask in 1 2 3 5 15; do
    # the trigger source has to be on: CH2 for mask 2, else CH1
    src=1
    [ "$mask" = 2 ] && src=2
    for tdiv in 1e-9 1e-8 1e-7 1e-6 2e-4 1e-3 5e-2; do
        run --mask "$mask" --app-mask "$mask" --src "$src" --tdiv "$tdiv"
    done
    for depth in 1000 100000 1000000 10000000; do
        run --mask "$mask" --app-mask "$mask" --src "$src" --tdiv 1e-3 --depth "$depth"
    done
done
for slope in rise fall either; do
    run --slope "$slope" --level 0.5
done
run --normal
run --play-last
run --frames 8
run --compress 4
run --afe --bw OFF
run --afe --bw 20M
run --afe --bw AUTO
