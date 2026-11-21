#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# check.sh - the host checks of OOS, layer by layer (docs/adr/process/0020). CI runs the same layers.
#
#   tools/check.sh                 every enforced layer, in order; stops at the first that fails
#   tools/check.sh test target     only these layers
#   tools/check.sh test:ui         a part of a layer: test:hal test:dsp test:ui test:fpgad test:tools. CI runs the
#                                  parts a change can reach next to each other (tools/ci-parts.sh).
#   tools/check.sh list            the layers, and which are enforced
#   tools/check.sh families        the board families the layers build for
#
# The layers clippy, test and target build hal, ui and fpgad for every board family (ADR 0067): those of
# OOS_FAMILIES (CI: the ones a change reaches, tools/ci-parts.sh), else every family of rootfs/board/, dho first.
# dho is built as without OOS_BOARD, in the crate's own target folder; another family with OOS_BOARD=<name> in
# target/board-<name>, so the builds of two families do not replace each other.
#
# Not layers here, because they need a baseline from before the change, a quiet computer or a scope (ADR 0020):
# ui/tools/layout-check.sh and tools/plan-matrix.sh (tools/compare-base.sh runs both against a commit before the
# change, and so does CI for a pull request), tools/perf-guard.sh, tools/device-test.py.
#
# Layers: fmt clippy text syntax test target ratchet sim. A compiler warning fails a layer like an error (ADR 0017).
# Needs a Rust tool chain with the aarch64-unknown-linux-musl target, python3 and git; touches no scope. The layer sim
# also needs Docker that runs arm64 containers, qemu-system-aarch64 and the builds it names; it is run by name.
set -e
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

crates="hal dsp ui fpgad"
layers="fmt clippy text syntax test target ratchet sim"
# sim needs Docker and builds that CI's check job lacks: it is run by name
enforced="fmt clippy text syntax test target ratchet"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
. tools/lib/board.sh

say() {
    printf '\n== %s\n' "$*"
}

fail() {
    echo "check.sh: $*" >&2
    exit 1
}

# the board families the layers build for: OOS_FAMILIES, each a family of the tree, else all of them; read once into
# $fams before the layers run, so a wrong name stops the run
families() {
    if [ -z "${OOS_FAMILIES:-}" ]; then
        oos_families .
        return
    fi
    for f in $OOS_FAMILIES; do
        OOS_BOARD=$f oos_board . || fail "OOS_FAMILIES: $f"
    done
}

# in_family FAMILY CRATE COMMAND...: COMMAND in the folder of CRATE, built for FAMILY (see the top)
in_family() {
    (
        f=$1
        cd "$2" || exit 1
        shift 2
        if [ "$f" = dho ]; then
            unset OOS_BOARD
        else
            OOS_BOARD=$f
            CARGO_TARGET_DIR=target/board-$f
            export OOS_BOARD CARGO_TARGET_DIR
        fi
        "$@"
    )
}

# the families a crate is built for: dsp knows no board
families_of() {
    case $1 in dsp) echo dho ;; *) echo "$fams" ;; esac
}

# " for the board family F", for a message, when it is not dho
of_family() {
    [ "$1" = dho ] || echo " for the board family $1"
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
        for f in $(families_of "$c"); do
            in_family "$f" "$c" strict cargo clippy --locked --workspace --all-targets -- -D warnings ||
                fail "clippy: $c$(of_family "$f")"
        done
    done
    # the code only Linux compiles (the scope's board, its buses) is not seen on another host: there, Clippy also
    # reads the crates as built for the scope, as CI on Linux does
    if [ "$(uname -s)" != Linux ]; then
        for c in hal ui fpgad; do
            for f in $(families_of "$c"); do
                in_family "$f" "$c" strict cargo clippy --locked --workspace --all-targets \
                    --target aarch64-unknown-linux-musl -- -D warnings ||
                    fail "clippy: $c for the scope$(of_family "$f")"
            done
        done
    fi
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

# every tracked shell script of our own: the *.sh files and every file whose first line starts a shell (rcS, init,
# the programs of the image, kernel/tools/git), so a new script is checked without a list to keep
shell_scripts() {
    git ls-files | outside | while IFS= read -r f; do
        [ -f "$f" ] || continue
        case $f in
        *.sh) echo "$f" ;;
        *) if head -n 1 "$f" 2>/dev/null | grep -qE '^#!.*[/ ](ba|da)?sh( |$)'; then echo "$f"; fi ;;
        esac
    done
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
    { git ls-files '*.rs' '*.py' '*.c' '*.h' | outside; shell_scripts; } | sort -u | while IFS= read -r f; do
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
    # the index of the decision records is what the records say, and every change in changelog.d/ has its form
    # (ADR 0065)
    python3 -I tools/adr-index.py --check || fail "text: run tools/adr-index.py"
    python3 -I tools/changelog.py --check || fail "text: a file in changelog.d/ (changelog.d/README.md)"
    # every relative link and every named file of the documents exists
    python3 -I tools/link-check.py || fail "text: a document names something that is not there (tools/link-check.py)"
    # blocks of code in two or more places (issue 120): counted, not yet enforced
    python3 -I tools/duplicates.py || true
}

# every tracked Python file, every shell script by its first line (shell_scripts), and the workflow files as YAML
layer_syntax() {
    git ls-files '*.py' | outside >"$tmp/py"
    while IFS= read -r f; do
        python3 -c 'import ast, sys; ast.parse(open(sys.argv[1], encoding="utf-8").read(), sys.argv[1])' "$f" ||
            fail "syntax: $f"
    done <"$tmp/py"
    shell_scripts >"$tmp/sh"
    while IFS= read -r f; do
        case $(head -n 1 "$f") in *bash*) sh=bash ;; *) sh=sh ;; esac
        $sh -n "$f" || fail "syntax: $f"
    done <"$tmp/sh"
    # the workflow files: one that does not parse runs no job at all, and nothing says so
    if command -v ruby >/dev/null 2>&1; then
        for f in .github/workflows/*.yml; do
            ruby -ryaml -e 'YAML.load_file(ARGV[0])' "$f" 2>"$tmp/yaml" || {
                cat "$tmp/yaml" >&2
                fail "syntax: $f"
            }
        done
        yaml="$(ls .github/workflows/*.yml | wc -l | tr -d ' ') workflow files"
    else
        yaml="workflow files not parsed (no ruby)"
    fi
    echo "$(wc -l <"$tmp/py" | tr -d ' ') Python files, $(wc -l <"$tmp/sh" | tr -d ' ') shell scripts, $yaml"
}

# the tests of every crate and of the host tools; with a part (hal dsp ui fpgad tools) only that one. ui is a
# workspace (ADR 0064): its crates are tested, linted and formatted with it
layer_test() {
    for c in $crates; do
        [ -z "$1" ] || [ "$1" = "$c" ] || continue
        for f in $(families_of "$c"); do
            say "test $c$(of_family "$f")"
            in_family "$f" "$c" strict cargo test --locked --workspace || fail "test: $c$(of_family "$f")"
        done
    done
    # the host tools' own tests: offline, no scope, no listener
    if [ -z "$1" ] || [ "$1" = tools ]; then
        say "test tools"
        python3 -m unittest discover -q -s tools/tests || fail "test: tools/tests"
    fi
}

# the programs for the scope build for every board family (families, ADR 0067), dho first
layer_target() {
    for b in $fams; do
        for c in ui fpgad; do
            in_family "$b" "$c" strict cargo check --locked --target aarch64-unknown-linux-musl ||
                fail "target: $c for the board family $b"
        done
    done
    # ADR 0021: what runs on the scope depends on libc only; hal and dsp on every platform
    for c in hal dsp ui fpgad; do
        case $c in hal | dsp) t=all ;; *) t=aarch64-unknown-linux-musl ;; esac
        other=$(cd "$c" && cargo tree --locked --target "$t" -e normal --prefix none | awk '{print $1}' | sort -u |
            grep -v -e '^libc$' -e '^oos-') || true
        [ -z "$other" ] || fail "target: $c depends on $(echo $other) (docs/adr/process/0021)"
    done
}

# the numbers that only go down: the size of the large files, switched-off lints, `unsafe`, what a remote command
# or a setting lacks (docs/scpi.md, the comparison's queries; tools/scpi-coverage.txt), long functions, `.unwrap()`
# and modules without a header (tools/code-metrics.txt), code that crosses a layer (tools/layers.txt) (ADR 0037,
# issue 120); and, with nothing missing a plain check, an HTTP route or MCP tool without its entry and mark in
# docs/http.md (ADR 0055)
layer_ratchet() {
    tools/ratchet.sh || fail "ratchet: see docs/adr/process/0037-numbers-that-only-go-down.md"
    python3 -I tools/scpi-coverage.py --enforce ||
        fail "ratchet: tools/scpi-coverage.py --list names what is missing (ADR 0037)"
    python3 -I tools/http-coverage.py || fail "ratchet: docs/http.md lacks what tools/http-coverage.py names (ADR 0055)"
    python3 -I tools/code-metrics.py --enforce || fail "ratchet: tools/code-metrics.py --list names them (ADR 0037)"
    python3 -I tools/layers.py --enforce || fail "ratchet: tools/layers.py --list names them (ADR 0001, ADR 0037)"
}

# The simulations: the scripts of the image in arm64 containers and virtual machines, without a scope. Each needs what
# its header says: rootfs/out (fw, ssh), kernel/out/base (unit-dtb), a finished boot/build-dev-image.sh (gel), QEMU
# (usb, wlan), oos-ui built for aarch64-unknown-linux-musl (gpu).
layer_sim() {
    for s in tools/fw-sim.sh tools/ssh-sim.sh tools/unit-dtb-check.sh tools/gel-sim.sh tools/usb-sim.sh \
        tools/wlan-sim.sh gpu/test-host.sh; do
        say "sim $s"
        sh "$s" || fail "sim: $s"
    done
}

fams=$(families)
if [ "${1:-}" = families ]; then
    echo "$fams"
    exit 0
fi
if [ "$1" = list ]; then
    for l in $layers; do
        case " $enforced " in *" $l "*) echo "$l" ;; *) echo "$l (not yet enforced)" ;; esac
    done
    exit 0
fi

run=${*:-$enforced}
# a layer, or layer:part
parts_test="$crates tools"
for l in $run; do
    name=${l%%:*}
    case " $layers " in *" $name "*) ;; *) fail "unknown layer '$name' (layers: $layers)" ;; esac
    case $l in
    *:*)
        case $name in test) parts=$parts_test ;; *) parts= ;; esac
        case " $parts " in
        *" ${l#*:} "*) ;;
        *) fail "no part '${l#*:}' in layer '$name' (parts: ${parts:-none})" ;;
        esac
        ;;
    esac
done
for l in $run; do
    say "$l"
    case $l in *:*) "layer_${l%%:*}" "${l#*:}" ;; *) "layer_$l" "" ;; esac
done
say "passed: $run"
if [ $# = 0 ]; then
    for l in $layers; do
        case " $enforced " in *" $l "*) ;; *) echo "not yet enforced, run by name: $l" ;; esac
    done
fi
