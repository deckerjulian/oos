#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# build.sh: oos-ui for the scope (static aarch64 musl) and for the host (off-screen test runner).
#   ./build.sh            both
#   ./build.sh test       host build + render the test scripts in tests/ to target/test-shots/*.png (needs Pillow)
set -e
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
[ -f assets/font.bin ] || python3 tools/mkfont.py assets/font.bin
cargo build --release --target aarch64-unknown-linux-musl
cargo build --release
ls -l target/aarch64-unknown-linux-musl/release/oos-ui
if [ "$1" = test ]; then
	out=target/test-shots; mkdir -p $out
	for t in tests/*.txt; do sed "s#\$S#$out#g" "$t" > $out/script.txt; ./target/release/oos-ui --script $out/script.txt; done
	python3 -c "import glob; from PIL import Image; [Image.open(f).save(f[:-4]+'.png') for f in glob.glob('$out/*.ppm')]"
	ls $out/*.png
fi
