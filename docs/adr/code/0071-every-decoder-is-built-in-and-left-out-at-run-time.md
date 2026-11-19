# ADR 0071: Every decoder is built into oos-ui; System > Features leaves decoders out at run time

- Status: proposed
- Date: 2026-11-19
- Category: code

## Context

Every decoder module of `ui/src/protocols/` except `core.rs` was a Cargo feature (`dec-<file>`, 14 of them, all on by
default). A build could leave decoders out with `OOS_DECODERS` or `OOS_DECODERS_OFF` (`ui/tools/decoder-features.sh`).
To keep every combination building, CI ran three more jobs on every pull request that reached `oos-ui`:

- `decoders, core`: the ui tests again without the optional decoders, and a check for the scope, about 140 s;
- `decoders, each 1` and `each 2`: every optional decoder alone, about 85 s each.

Measured on run 44513896422 (#309). `decoders, core` was one of the two longest jobs of a pull request.

Since then, System > Features (`ui/src/ui/features.rs`) leaves decoders out at run time. It writes `decoders.off`
into `/oos/system.conf`, and a decoder left out is gone from the pickers and is switched off. That covers what a
user wanted from a build with fewer decoders, without a build of their own. The release image was always built with
every decoder. The maintainer decided on 2026-11-19 that a build no longer leaves decoders out.

## Decision

**Every decoder module is always built into oos-ui. The Cargo features `dec-*` and `decoders`,
`ui/tools/decoder-features.sh`, `OOS_DECODERS` and `OOS_DECODERS_OFF` are gone, and so are the layer `decoders` of
`tools/check.sh` and its three CI jobs. Leaving a decoder out is a run-time setting: System > Features,
`decoders.off`.**

## Alternatives

- **Keep the features and run their jobs less often** (weekly instead of per pull request). This leaves the
  combinations that nobody builds in the tree, and a broken one is found days later. Without a build that leaves
  decoders out, there is nothing to keep building.
- **Keep the features without CI jobs.** Untested combinations break silently.

## Consequences

- A pull request that reaches oos-ui runs three jobs fewer. The `ui` job and the comparison stay as they were.
- The tests no longer skip themselves in a build without every decoder, and the completeness test of the decoders
  checks the whole list every time (`ui/src/protocols/mod.rs::every_decoder_is_complete`).
- A new decoder no longer needs a feature in `ui/Cargo.toml`, only its `mod` line and its place in `MODULES`.
- The image keeps every decoder, as it did. Its size does not change.
- Amends ADR 0020 (the layer `decoders`) and ADR 0040 (its two halves in CI).
- Worth a second look if a decoder needs a large table or a dependency that should not be in every build.

## References

`ui/src/protocols/mod.rs`, `ui/src/ui/features.rs`, `ui/system/src/sysconf.rs` (`decoders.off`),
[protocol-modules.md](../../protocol-modules.md), `tools/check.sh`, `tools/ci-parts.sh`; ADR 0002, ADR 0020,
ADR 0040.
