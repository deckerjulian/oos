# ADR 0041: A change that leaves the programs for the scope byte-identical needs no run on a unit

- Status: accepted (2026-11-17, by the maintainer), amended by ADR 0068
- Date: 2026-11-17
- Category: process

## Context

The status `unit` (ADR 0038) holds every pull request that touches a path only a unit proves (`hal/src/dho/`,
`hal/src/update/`, `ui/src/board/`, `fpgad/`, `boot/`, `kernel/`, `rootfs/`) until it carries `unit-tested`. It asks by
path, not by effect: #112 changed only `[profile.test]` in `fpgad/Cargo.toml` and `hal/Cargo.toml`, which `cargo test`
on a host reads and no build for the scope does, and still waited for a run on a unit. Tests, comments and lint fixes
in those paths wait the same way.

Built twice from the same sources, oos-ui, oos-fpgad and oos-update come out byte for byte the same when the
folder is the same (tried on 2026-11-17: main and #112, built one after the other in one folder, gave the same
SHA-256 for each). Built in two folders they differ: Cargo puts the absolute path of each crate into the names of its
symbols.

## Decision

- **`unit` also passes when the programs for the scope are byte-identical to the base's.** CI's job `scope`
  (`tools/scope-binaries.sh`) builds the programs the image holds from the crates (oos-ui, oos-fpgad, oos-update) as
  `boot/build-dev-image.sh` builds them (release with its link-time optimisation, the crates' target flags, the
  default decoders) from the pull request and from its base, one after the other in the same folder, with the
  folder's path replaced, and compares them. When
  they differ it keeps the artifact `scope-binaries-differ`; `tools/pr-gate.sh` reads both.
- It runs only for a pull request that touches a path of a crate a unit proves and nothing of the image (`boot/`,
  `kernel/`, `rootfs/`): what the image holds besides the two programs is not compared.
- Programs that differ still need a run on a unit and `unit-tested`, as before.

## Alternatives

- **A narrower list of paths** (leaving out `Cargo.toml`, tests): it would still ask by name, and a `Cargo.toml` can
  change what is built for the scope (a dependency, a feature, the release profile).
- **Comparing the programs without the symbol names** (`strip`): two builds in different folders could then be
  compared, but whether the rest of the file is equal then depends on how the linker lays it out; one folder is
  simpler to trust.

## Consequences

- Tests, comments, renames that the compiler leaves without trace, and changes of the host's build settings in the
  hardware paths go in without a unit. A change of a register sequence or of the code that drives the scope still
  needs one.
- The job takes two release builds as the image's (several minutes) and runs next to the others.
- Two programs that differ only in something that never runs still ask for a unit: the comparison is byte for byte.
- The GPU helper (`gpu/`) is not compared; it is not a path of `unit` either.

## References

ADR 0018, ADR 0038, ADR 0040, `tools/scope-binaries.sh`, `tools/pr-gate.sh`, `tools/ci-parts.sh`,
`.github/workflows/ci.yml`.
