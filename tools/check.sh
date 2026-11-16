#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# check.sh - the host checks of OOS, layer by layer (docs/adr/process/0020). CI runs the same layers.
#
#   tools/check.sh                 every enforced layer, in order; stops at the first that fails
#   tools/check.sh test target     only these layers
#   tools/check.sh list            the layers, and which are enforced
#
# Not layers here, because they need a baseline from before the change, a quiet computer or a scope (ADR 0020):
# ui/tools/layout-check.sh, tools/plan-matrix.sh, tools/perf-guard.sh, tools/device-test.py.
#
# Layers: fmt clippy text syntax test target decoders. A compiler warning fails a layer like an error (ADR 0017).
# Needs a Rust tool chain with the aarch64-unknown-linux-musl target, python3 and git; touches no scope.
set -e
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

crates="hal dsp ui fpgad"
layers="fmt clippy text syntax test target decoders"
# fmt and clippy join with the rollout of ADR 0016 and ADR 0017
enforced="text syntax test target decoders"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

say() {
    printf '\n== %s\n' "$*"
}

fail() {
    echo "check.sh: $*" >&2
    exit 1
}

# cargo, failing on a compiler or Clippy warning too. Cargo repeats the warnings of cached builds, so a second run
# without changes still fails.
strict() {
    if ! "$@" 2>"$tmp/err"; then
        cat "$tmp/err" >&2
        return 1
    fi
    if grep -q '^warning' "$tmp/err"; then
        cat "$tmp/err" >&2
        echo "check.sh: warnings above ($*)" >&2
        return 1
    fi
}

layer_fmt() {
    for c in $crates; do
        (cd "$c" && cargo fmt --check) || fail "fmt: run 'cargo fmt' in $c"
    done
}

layer_clippy() {
    for c in $crates; do
        (cd "$c" && strict cargo clippy --locked --all-targets -- -D warnings) || fail "clippy: $c"
    done
}

# ADR 0019. Taken from elsewhere, so left out: the licence texts, the XDMA driver, kernel patches, firmware licences,
# the recorded kernel answers.
outside() {
    grep -v -e '^LICENSE$' -e '^kernel/modules/' -e '^kernel/patches/' -e '^kernel/tools/git-log-answers.txt$' \
        -e '^rootfs/initramfs/lib/firmware/' -e '^rootfs/initramfs/etc/licenses/.*\.txt$'
}

# the licence texts the image carries (/etc/licenses) are copies: each must equal the file it is a copy of
licence_copies() {
    while read -r a b; do
        cmp -s "$a" "$b" || echo "$b differs from $a"
    done <<COPIES
LICENSE rootfs/initramfs/etc/licenses/GPL-2.0.txt
THIRD-PARTY-LICENSES.md rootfs/initramfs/etc/licenses/THIRD-PARTY-LICENSES.md
ui/tools/fonts/LICENSE-Roboto.txt rootfs/initramfs/etc/licenses/Apache-2.0.txt
kernel/firmware/LICENSE.wireless-regdb rootfs/initramfs/etc/licenses/wireless-regdb.txt
COPIES
}

layer_text() {
    bad=0
    git ls-files -z | xargs -0 grep -IlE '[[:space:]]+$' 2>/dev/null | outside >"$tmp/trail" || true
    if [ -s "$tmp/trail" ]; then
        echo "spaces or tabs at the end of a line (or CR line ends):"
        sed 's/^/  /' "$tmp/trail"
        bad=1
    fi
    git ls-files -z | xargs -0 grep -Il '' 2>/dev/null | outside >"$tmp/text" || true
    : >"$tmp/eof"
    while IFS= read -r f; do
        [ -s "$f" ] || continue
        [ "$(tail -c 1 "$f" | od -An -c | tr -d ' ')" = '\n' ] || echo "$f" >>"$tmp/eof"
    done <"$tmp/text"
    if [ -s "$tmp/eof" ]; then
        echo "no newline at the end of the file:"
        sed 's/^/  /' "$tmp/eof"
        bad=1
    fi
    git ls-files '*.rs' '*.sh' '*.py' '*.c' '*.h' | outside | while IFS= read -r f; do
        head -n 5 "$f" | grep -q 'SPDX-License-Identifier:' || echo "$f"
    done >"$tmp/spdx"
    if [ -s "$tmp/spdx" ]; then
        echo "no SPDX-License-Identifier line in the first five lines:"
        sed 's/^/  /' "$tmp/spdx"
        bad=1
    fi
    git ls-files '*.rs' | xargs grep -l "$(printf '^\t')" >"$tmp/tabs" 2>/dev/null || true
    if [ -s "$tmp/tabs" ]; then
        echo "Rust indented with tabs:"
        sed 's/^/  /' "$tmp/tabs"
        bad=1
    fi
    licence_copies >"$tmp/copies"
    if [ -s "$tmp/copies" ]; then
        echo "a licence text in the image tree is not the copy it should be (THIRD-PARTY-LICENSES.md):"
        sed 's/^/  /' "$tmp/copies"
        bad=1
    fi
    [ "$bad" = 0 ] || fail "text: see docs/adr/process/0019-one-style-for-text-files-and-commits.md"
}

# every tracked script: Python by its name or its first line, shell the same way (busybox sh included)
scripts() {
    git ls-files | outside | while IFS= read -r f; do
        case $f in
        *.py) echo "py $f" ;;
        *.sh) echo "sh $f" ;;
        *)
            [ -f "$f" ] || continue
            first=$(head -n 1 "$f")
            case $first in
            '#!'*python*) echo "py $f" ;;
            '#!'*/sh | '#!'*/sh\ * | '#!'*\ sh | '#!'*\ sh\ *) echo "sh $f" ;;
            esac
            ;;
        esac
    done
}

layer_syntax() {
    scripts >"$tmp/scripts"
    while read -r kind f; do
        if [ "$kind" = py ]; then
            python3 -c 'import ast, sys; ast.parse(open(sys.argv[1], encoding="utf-8").read(), sys.argv[1])' "$f" ||
                fail "syntax: $f"
        else
            sh -n "$f" || fail "syntax: $f"
        fi
    done <"$tmp/scripts"
}

layer_test() {
    for c in $crates; do
        say "test $c"
        (cd "$c" && strict cargo test --locked) || fail "test: $c"
    done
    # the host tools' own tests: offline, no scope, no listener
    say "test tools"
    (cd tools && python3 -m unittest -q test_scpi_validate) || fail "test: tools/test_scpi_validate.py"
    python3 -m unittest discover -q -s tools/tests || fail "test: tools/tests"
}

layer_target() {
    for c in ui fpgad; do
        (cd "$c" && strict cargo check --locked --target aarch64-unknown-linux-musl) || fail "target: $c"
    done
    # ADR 0021: what runs on the scope depends on libc only; hal and dsp on every platform
    for c in hal dsp ui fpgad; do
        case $c in hal | dsp) t=all ;; *) t=aarch64-unknown-linux-musl ;; esac
        other=$(cd "$c" && cargo tree --locked --target "$t" -e normal --prefix none | awk '{print $1}' | sort -u |
            grep -v -e '^libc$' -e '^oos-') || true
        [ -z "$other" ] || fail "target: $c depends on $(echo $other) (docs/adr/process/0021)"
    done
}

# the core decoders only, and every optional decoder alone (tests that need every decoder skip themselves)
layer_decoders() {
    (cd ui && strict cargo test --locked --no-default-features) || fail "decoders: core only"
    (cd ui && strict cargo check --locked --target aarch64-unknown-linux-musl --no-default-features) ||
        fail "decoders: core only, target"
    for d in $(ui/tools/decoder-features.sh list); do
        (cd ui && strict cargo check --locked --all-targets --no-default-features --features "dec-$d") ||
            fail "decoders: dec-$d alone"
    done
}

if [ "$1" = list ]; then
    for l in $layers; do
        case " $enforced " in *" $l "*) echo "$l" ;; *) echo "$l (not yet enforced)" ;; esac
    done
    exit 0
fi

run=${*:-$enforced}
for l in $run; do
    case " $layers " in *" $l "*) ;; *) fail "unknown layer '$l' (layers: $layers)" ;; esac
done
for l in $run; do
    say "$l"
    "layer_$l"
done
say "passed: $run"
if [ $# = 0 ]; then
    for l in $layers; do
        case " $enforced " in *" $l "*) ;; *) echo "not yet enforced, run by name: $l" ;; esac
    done
fi
