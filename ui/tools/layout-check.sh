#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# layout-check.sh: the guard of a layout change. Renders every script in tests/ at the three text sizes on the host and
# compares the pictures with a saved set, pixel by pixel.
#   tools/layout-check.sh record [dir]   render with the current source and keep the pictures (default target/layout-base)
#   tools/layout-check.sh check [dir]    render again and name every picture that differs, with the box of the change
#   SIZE=1920x1080 tools/layout-check.sh record target/layout-1080   another screen size (host only, --size)
# Each script runs with an empty storage directory and a fixed clock (OOS_FIXED_TIME), so two runs of the same source
# give the same pictures. Run `record` before a change and `check` after it: a pure refactoring must report nothing.
set -e
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
mode=${1:-check}
base=${2:-target/layout-base}
case $mode in record | check) ;; *) echo "usage: $0 record|check [dir]" >&2; exit 2 ;; esac
cargo build --release --quiet
bin=$PWD/target/release/oos-ui
if [ "$mode" = record ]; then out=$base; else out=target/layout-new; fi
rm -rf "$out" target/layout-work
mkdir -p "$out" target/layout-work
size_arg=
[ -n "$SIZE" ] && size_arg="--size $SIZE"
for ts in normal large larger; do
	mkdir -p "$out/$ts"
	for t in tests/*.txt; do
		name=$(basename "$t" .txt)
		work=$PWD/target/layout-work/$ts-$name
		mkdir -p "$work/store"
		{ echo "textsize $ts"; sed "s#\$S#$PWD/$out/$ts#g" "$t"; } > "$work/script.txt"
		# shellcheck disable=SC2086
		OOS_FIXED_TIME=1762862400 "$bin" --script "$work/script.txt" --store-dir "$work/store" $size_arg > /dev/null 2> "$work/err.txt" || {
			echo "$ts $name: oos-ui failed, see $work/err.txt" >&2
			exit 1
		}
	done
done
(cd "$out" && find . -name '*.ppm' | sort | xargs shasum) > "$out.sums"
n=$(wc -l < "$out.sums" | tr -d ' ')
if [ "$mode" = record ]; then
	echo "recorded $n pictures in $out"
	exit 0
fi
[ -f "$base.sums" ] || { echo "no saved set: run '$0 record $base' first" >&2; exit 2; }
if cmp -s "$base.sums" "$out.sums"; then
	echo "all $n pictures equal to $base"
	exit 0
fi
python3 -I - "$base" "$out" <<'EOF'
import sys, os
base, new = sys.argv[1], sys.argv[2]
def sums(d):
    return dict(l.split()[::-1] for l in open(d + '.sums'))
a, b = sums(base), sums(new)
def load(p):
    data = open(p, 'rb').read()
    head = data.split(b'\n', 3)
    w, h = map(int, head[1].split())
    return w, h, head[3]
bad = 0
for k in sorted(set(a) | set(b)):
    if a.get(k) == b.get(k):
        continue
    bad += 1
    if k not in a or k not in b:
        print('%-40s %s' % (k, 'only new' if k in b else 'missing'))
        continue
    wa, ha, pa = load(os.path.join(base, k))
    wb, hb, pb = load(os.path.join(new, k))
    if (wa, ha) != (wb, hb):
        print('%-40s size %dx%d -> %dx%d' % (k, wa, ha, wb, hb))
        continue
    xs, ys = [], []
    for i in range(0, len(pa), 3):
        if pa[i:i + 3] != pb[i:i + 3]:
            p = i // 3
            xs.append(p % wa)
            ys.append(p // wa)
    print('%-40s %6d px  x %d..%d  y %d..%d' % (k, len(xs), min(xs), max(xs), min(ys), max(ys)))
print('%d of %d pictures differ (saved: %s, new: %s)' % (bad, len(b), base, new))
sys.exit(1)
EOF
