# Upstream sync checklist

Every merge of an upstream release into `dho804`. Policy and history: private RE notes `docs/oos-fork.md`, "Upstream
sync". Nothing here touches the unit.

1. Branch `sync/<version>` from `dho804`; `git merge upstream/main` (no rebase).
2. Conflicts in `boot/build-dev-image.sh`: keep upstream's file and re-add the one line
   `. "$here/unit-hook.sh"` (before `build_id=`) and the one line `. /wlan/unit-hook/image.sh` in the container script
   (before `ln -sf busybox`). Everything else of the unit build lives in `boot/unit-hook.sh` and
   `boot/unit-hook-image.sh`. If upstream renamed the `rootfs/out:/wlan` mount, move the staging path in the hook.
3. Conflicts in `hal/src/dho/seq.rs` Poll handling: upstream's Poll body lives in the fork's `plain_step`. Port any
   upstream change there; `execute_until` stays a thin wrapper.
4. Host tests, all must pass: `cargo test` in `hal`, `dsp`, `ui`, `fpgad`.
5. `cargo check --all-targets` of `hal`, `ui`, `fpgad` on linux/arm64 (container), so the `target_os = "linux"` paths
   compile:
   `docker run --rm -v "$PWD:/src" -v oos-target:/tgt -e CARGO_TARGET_DIR=/tgt -w /src/<crate> rust:1-alpine cargo check --all-targets`
6. `tools/perf-guard.sh`: allocation counts and the early trigger end (deterministic), then the timing ratios of the
   fork's fast paths against reference copies of the paths they replaced. A failing ratio means the merge brought an
   upstream path back into the fork's loop, or slowed the fork's one: find which before fast-forwarding.
7. `tools/plan-matrix.sh` with the old and the new `oos-fpgad`, `diff`: the register sequences must not change unless
   the merge meant to.
8. Record the sync in `docs/fork/unit-dho804.md` and private RE notes `docs/oos-fork.md`; fast-forward `dho804`.

## What the guards protect

| guard | where | breaks when |
|---|---|---|
| no allocation per read, one vector per channel per decode, none with kept vectors | `hal/tests/alloc_batch.rs` | `read_dma` or `deinterleave` + `volts` return to the batch loop; the decode allocates scratch space |
| early trigger end for new settings on the generic path | `hal/tests/early_trigger_end.rs` | an upstream Poll edit does not reach `plain_step` |
| fused f32 decode >= 2x upstream's two-pass decode; f32 <= 0.8x the f64 fused decode | `hal/tests/perf_ratio.rs` (ignored) | the decode stops vectorising, goes back to f64, or upstream's path returns |
| bit-level or bounded equivalence of the fast paths | `seq.rs`, `bus.rs` unit tests | a fast path computes something else than the path it replaced |
| oos-ui timing ratios | ignored tests named `perf_guard*` in `ui` | see the tests |
