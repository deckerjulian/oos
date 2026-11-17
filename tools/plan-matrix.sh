#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# plan-matrix.sh - register sequences of a fixed set of acquisitions, for comparing two builds of oos-fpgad.
#
#   tools/plan-matrix.sh [path/to/oos-fpgad] > plan-OLD.txt
#
# `oos-fpgad plan` only prints, it touches no hardware, so this is safe on the unit (busybox sh) and on the Mac. Run it
# with the old build still on the unit BEFORE installing a new image, and with the new build, then `diff`: the analog
# path must not change at 1.25 GHz. Needs the calibration files: on the unit
# /rigol/data, on a host computer `OOS_CAL_DIR=local/cal`, and without the files of a unit a made-up set
# (`oos-fpgad made-up-cal DIR`, then `OOS_CAL_DIR=DIR`; invented values, docs/protocols/self-cal.md 16).
#
# Besides `plan` it prints the other dry runs that write nothing: roll and scan, the generator, the LA thresholds, the
# cold boot against the unit model and the PLL table for another clock.
f=${1:-oos-fpgad}
run() {
    echo "### $*"
    "$f" plan "$@" 2>&1
}
dry() {
    echo "### $*"
    "$f" "$@" 2>&1
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
for n in 2 16 1024; do
    run --average "$n"
done
run --average 16 --mask 3 --app-mask 3
run --la
run --la --la-thr 2.5
run --la-alone
run --la --mask 3 --app-mask 3 --tdiv 1e-6
run --la --mask 15 --app-mask 15 --tdiv 1e-3
for tdiv in 5e-2 2e-1 1 10; do
    dry roll --tdiv "$tdiv"
    dry roll --tdiv "$tdiv" --scan
done
dry roll --mask 3 --app-mask 3 --tdiv 2e-1
dry roll --mask 15 --app-mask 15 --tdiv 1 --scan
dry roll --la --tdiv 2e-1
dry roll --tdiv 5e-2 --depth 1000000
dry afg sine --freq 1000 --ampl 1
dry afg square --freq 1e5 --ampl 2 --offset 0.5 --duty 25
dry afg ramp --freq 1e4 --symmetry 30
dry afg pulse --freq 1e6 --duty 10 --edge 1e-8
dry afg noise --ampl 0.5
dry afg dc --offset 1.5
dry afg sine --freq 1e4 --mod am --mod-rate 100 --depth 50
dry afg sine --freq 1e4 --mod fm --mod-rate 100 --dev 1000 --mod-shape triangle
dry afg square --freq 1e4 --mod pm --mod-rate 50 --dev 90 --mod-shape square
dry afg off
dry la thr 1.4 2.0
dry cold-boot
dry pll --freq 1.5e9 --dry
