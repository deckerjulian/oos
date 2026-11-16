#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# decoder-features.sh: the cargo arguments for the decoders a build should have (ui/Cargo.toml, [features]).
#   OOS_DECODERS="usb i2s dcc"       only these, besides the core ones (UART, I2C, SPI, CAN, LIN, Parallel, C-PHY)
#   OOS_DECODERS=""                  the core ones only
#   OOS_DECODERS_OFF="avionics usb"  all but these
#   neither set                      all (prints nothing: the default features)
# Names are the files in ui/src/protocols, with or without "dec-". `./decoder-features.sh list` lists them.
# Use: cargo build $(tools/decoder-features.sh) ...
set -e
toml="$(dirname "$0")/../Cargo.toml"
all=$(sed -n 's/^decoders = \[\(.*\)\]/\1/p' "$toml" | tr -d '",' | sed 's/dec-//g')
[ -n "$all" ] || { echo "decoder-features.sh: no decoder list in $toml" >&2; exit 1; }
if [ "$1" = list ]; then echo $all; exit 0; fi
check() {
	for d in $1; do
		case " $all " in *" ${d#dec-} "*) ;; *) echo "decoder-features.sh: unknown decoder '$d' (known: $all)" >&2; exit 1 ;; esac
	done
}
if [ -n "${OOS_DECODERS+set}" ]; then
	[ -z "${OOS_DECODERS_OFF+set}" ] || { echo "decoder-features.sh: set OOS_DECODERS or OOS_DECODERS_OFF, not both" >&2; exit 1; }
	want=$OOS_DECODERS
elif [ -n "${OOS_DECODERS_OFF+set}" ]; then
	check "$OOS_DECODERS_OFF"
	want=
	for d in $all; do
		case " $(echo $OOS_DECODERS_OFF | sed 's/dec-//g') " in *" $d "*) ;; *) want="$want $d" ;; esac
	done
else
	exit 0
fi
check "$want"
f=
for d in $want; do f="$f${f:+,}dec-${d#dec-}"; done
echo "--no-default-features${f:+ --features $f}"
