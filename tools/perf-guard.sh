#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# perf-guard.sh - the fork's performance guards, a step of every upstream sync (docs/fork/sync-checklist.md).
#
#   tools/perf-guard.sh
#
# 1. Deterministic guards (they also run in the normal `cargo test`): allocation counts of the batch read and decode,
#    the early trigger end on the generic batch path, and the equivalence tests of the fork's fast paths.
# 2. Timing guards: `#[ignore]` tests that compare a fork path with a reference copy of the path it replaced, in one
#    release run on one thread. They assert ratios, not times. In oos-ui these are the ignored tests whose name
#    contains `perf_guard`.
# Host only: touches no hardware. Exits non-zero on the first failing guard.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"

echo "== hal: deterministic guards"
(cd "$root/hal" && cargo test --release --quiet --test alloc_batch --test early_trigger_end)
(cd "$root/hal" && cargo test --release --quiet --lib -- \
	fused_decode_equals_deinterleave_and_volts decode_into_kept_vectors_equals_decode unpad_in_place_equals_unpad \
	prearm_writes_the_same_sequence_as_the_plain_batch_loop dma_buf_is_reused_aligned_and_exact_with_short_reads)

echo "== hal: timing guards"
(cd "$root/hal" && cargo test --release --quiet --test perf_ratio -- --ignored --nocapture --test-threads=1)

echo "== ui: timing guards"
(cd "$root/ui" && cargo test --release --quiet -- --ignored --nocapture --test-threads=1 perf_guard)

echo "perf guards: all passed"
