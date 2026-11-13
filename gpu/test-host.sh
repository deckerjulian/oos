#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# test-host.sh: the GPU picture against the CPU picture, without a scope. In an arm64 container with Mesa's software
# renderer (llvmpipe), the scope's oos-ui binary renders every script of ui/tests twice: on the CPU, and in layers put
# together by oos-gpu (OOS_GPU_SOFT=1). gpu/compare.py then names the pictures that differ more than rounding.
#
#   gpu/test-host.sh            needs: ui built for aarch64-unknown-linux-musl, Docker (arm64 host or QEMU)
set -eu
cd "$(dirname "$0")/.."
out=$PWD/ui/target/gpu-check
rm -rf "$out"
mkdir -p "$out/cpu" "$out/gpu"
image=oos-gpu-host-test
docker image inspect $image >/dev/null 2>&1 || docker build -t $image - >&2 <<'EOF'
FROM ubuntu:26.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      gcc libc6-dev libegl-dev libgles-dev libegl-mesa0 libgles2 libegl1 mesa-libgallium \
    && rm -rf /var/lib/apt/lists/*
EOF
docker run --rm -v "$PWD:/o" -v "$out:/out" $image sh -euc '
	gcc -std=gnu11 -O2 -Wall -o /tmp/oos-gpu /o/gpu/oos-gpu.c -lEGL -lGLESv2
	ui=/o/ui/target/aarch64-unknown-linux-musl/release/oos-ui
	for t in /o/ui/tests/*.txt; do
		n=$(basename "$t" .txt)
		for mode in cpu gpu; do
			# the same storage path for both: its name appears in some pictures
			w=/tmp/w/$n; rm -rf "$w"; mkdir -p "$w/store"
			sed "s#\$S#/out/$mode#g" "$t" > "$w/script.txt"
			g=""; [ $mode = gpu ] && g="--gpu /tmp/oos-gpu"
			OOS_GPU_SOFT=1 OOS_FIXED_TIME=1762862400 $ui --script "$w/script.txt" --store-dir "$w/store" $g > /dev/null 2> "/tmp/err-$mode-$n" || { echo "$mode $n failed:"; tail -5 "/tmp/err-$mode-$n"; exit 1; }
		done
	done
	grep -h "oos-gpu:" /tmp/err-gpu-* | sort -u | head -3
'
python3 "$(dirname "$0")/compare.py" "$out/cpu" "$out/gpu"
