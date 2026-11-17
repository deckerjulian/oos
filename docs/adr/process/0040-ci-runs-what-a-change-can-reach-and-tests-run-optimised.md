# ADR 0040: CI runs the parts a change can reach, and the tests run optimised

- Status: accepted (2026-11-17, asked for by the maintainer)
- Date: 2026-11-17
- Category: process

## Context

A pull request with code ran every part of `tools/check.sh` next to each other and the comparison with its base
(ADR 0020, ADR 0032): about four and a half minutes, whatever it changed. Measured on 2026-11-17 in CI (4 cores, the
build cache of main restored): building took seconds, the tests took the time. `test:ui` ran 152 s, `decoders:core`
147 s (the same tests without the optional decoders), `test:hal` 122 s, nearly all of it in a few tests of
`hal/src/update` that work through card images in an unoptimised build. `decoders:each` (one `cargo check` per
optional decoder, 14 of them) took about 150 s of compiling. A change of a host tool or of fpgad alone ran all of it.

## Decision

- **The tests run with `opt-level = 1`** (`[profile.test]` of each crate). Debug assertions and overflow checks
  stay on. Measured on a laptop: the tests of hal 45 s to 7 s, of ui 79 s to 12 s; building them takes longer once,
  and CI keeps the build in its cache.
- **A pull request runs the parts its changed files can reach** (`tools/ci-parts.sh`, its own tests in
  `tools/tests/test_ci_parts.py`). The crates depend on each other as hal <- ui, hal <- fpgad, dsp <- ui; `keys/` is
  built into hal; the tests of hal read `rootfs/`. A change of how the checks run (the workflow, `tools/check.sh`,
  `tools/ci-parts.sh`, `rustfmt.toml`, the list of decoders), or of a file outside the known paths, runs everything.
  Documents, the host tools (their tests run in the job `files` for every pull request), the image and the GPU
  helper build nothing. The comparison runs when a crate or one of its own scripts changed.
- **`decoders:each` is cut in two halves** that run next to each other (`decoders:each/1`, `decoders:each/2`).
- A push to main and the weekly run still run every part, and they fill the caches that pull requests read; the
  caches are named by job, so a pull request finds the one main saved whatever its parts are.

## Alternatives

- **cargo-nextest with test sharding.** Splits the slow tests over more jobs but leaves them slow, and adds a tool
  to install in every job; optimising the build removed most of the time instead.
- **Choosing single tests by what they cover.** Needs a coverage map kept up to date; the crate graph is coarse but
  cannot miss a dependency.
- **Skipping the comparison for small changes.** It is the proof of `Behaviour: unchanged` (ADR 0032) and stays
  for every change of a crate.

## Consequences

- A change of a host tool or a document is checked in about half a minute; one of fpgad in about one; one of ui or
  hal still runs most parts, each of them shorter.
- A test that relied on the unoptimised build being slow, or on debug-only behaviour other than assertions, can
  behave differently; none did on 2026-11-17.
- A new path that holds code for the host checks has to be named in `tools/ci-parts.sh`; until it is, it runs
  everything, which is slow but safe. Worth a second look when a pull request merged green and main then failed
  because a part was skipped.

## References

ADR 0020, ADR 0032, `tools/ci-parts.sh`, `tools/check.sh`, `.github/workflows/ci.yml`, `tools/tests/test_ci_parts.py`.
