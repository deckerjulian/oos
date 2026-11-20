#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# layout-check.sh: the guard of a layout change. Renders every script in tests/ at the three text sizes on the host and
# compares the pictures with a saved set, pixel by pixel. Next to the pictures it keeps and compares what a script
# stores (set-ups, CSV, PNG) and the answers to remote commands that a script asks for (`scpi`, tests/scpi.txt).
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
		# run inside the work folder with the storage folder by its short name: the storage pages show that name, and
		# the pictures must not depend on where the tree lies (tools/compare-base.sh compares two trees)
		# shellcheck disable=SC2086
		(
			cd "$work" &&
				OOS_FIXED_TIME=1762862400 "$bin" --script script.txt --store-dir store $size_arg > /dev/null 2> err.txt
		) || {
			echo "$ts $name: oos-ui failed, see $work/err.txt" >&2
			exit 1
		}
		# what the script stored: compared like the pictures
		if [ -n "$(ls -A "$work/store")" ]; then
			mkdir -p "$out/$ts/stored-$name"
			cp -R "$work/store/." "$out/$ts/stored-$name/"
		fi
	done
done
(cd "$out" && find . -type f | sort | xargs shasum) > "$out.sums"
n=$(grep -c '\.ppm$' "$out.sums")
m=$(grep -vc '\.ppm$' "$out.sums" || true)
if [ "$mode" = record ]; then
	echo "recorded $n pictures and $m other files in $out"
	exit 0
fi
[ -f "$base.sums" ] || { echo "no saved set: run '$0 record $base' first" >&2; exit 2; }
if cmp -s "$base.sums" "$out.sums"; then
	echo "all $n pictures and $m other files equal to $base"
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
    if not k.endswith('.ppm'):
        # a stored file or the answers to remote commands: the first line that differs
        la, lb = (open(os.path.join(d, k), 'rb').read().split(b'\n') for d in (base, new))
        i = next((i for i, (x, y) in enumerate(zip(la, lb)) if x != y), min(len(la), len(lb)))
        show = lambda l: (l[i][:60].decode('utf-8', 'replace') if i < len(l) else '(end)')
        print('%-40s line %d: %s -> %s' % (k, i + 1, show(la), show(lb)))
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
print('%d of %d pictures and files differ (saved: %s, new: %s)' % (bad, len(b), base, new))
sys.exit(1)
EOF
