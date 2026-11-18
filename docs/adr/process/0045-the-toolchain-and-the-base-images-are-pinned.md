# ADR 0045: The Rust tool chain and the base images of the containers are pinned

- Status: proposed
- Date: 2026-11-18
- Category: process

Amends ADR 0017 (its tool chain was "the current stable Rust").

## Context

Two inputs of every build had a name that moves.

Every job of CI ran `rustup toolchain install stable`: the newest Rust of that minute. A release brings new Clippy
lints and at times another layout of rustfmt, and a warning is an error (ADR 0016, ADR 0017), so the day of a release
could turn every open pull request red for a reason none of them had. CI, a contributor's computer and two runs of the
same commit could each have another compiler. On 2026-11-18 the runners carried 1.99.1 and every job fetched 1.100.0,
six days old.

`tools/lib/docker.sh` named the base images of the build and test containers by tag (`alpine:3.22`, `ubuntu:26.04`).
The registry moves a tag to a new build whenever the distribution publishes one.

## Decision

- **`rust-toolchain.toml` at the root of the tree names the Rust release**, its components and the target of the
  scope: 1.100.0, the release CI and the tree built with on 2026-11-18. rustup reads the file for every `cargo` in
  the tree. The workflows install what it names (`rustup toolchain install` without a channel) and name no version.
- **The base images carry the digest of their index behind the tag** (`name:tag@sha256:...`, `OOS_ALPINE` and
  `OOS_UBUNTU`).
- **Both change only in a pull request.** [docs/fork/deps.md](../../fork/deps.md) says how.
- **The weekly run tries the newest stable Rust** (job `canary`: `tools/check.sh` with `RUSTUP_TOOLCHAIN=stable`) and
  opens its issue when the tree does not pass.

## Alternatives

- **Floating stable**, the state before: no bump to do, but the breakage comes on a day nobody chose, into pull
  requests that did not cause it.
- **Dependabot bumps the pin.** It reads `rust-toolchain.toml` (`package-ecosystem: rust-toolchain`), so it could.
  It does not reach the digests, which are in a shell file and not in a Dockerfile, and a pull request of
  Dependabot is merged by itself (ADR 0038) while a new compiler changes every program for the scope. Not switched on
  with this record; a later one can.
- **An action that installs the tool chain:** one more action to pin and review for what one line of rustup does.

## Consequences

- A new Rust changes nothing until its bump. The bump is one pull request: the new `channel` and the fixes for what
  the new release finds, apart from every feature, as ADR 0017 wanted. It runs every part of CI and the comparison
  (the file is none of the paths `tools/ci-parts.sh` knows).
- The canary says when a bump needs work: it fails and the weekly issue names it. When it passes it only leaves a
  notice in the run that a newer release exists. Nothing forces a bump, so the pin can fall behind.
- A bump changes every program for the scope. `rust-toolchain.toml` is therefore a hardware path of the gate
  (`tools/pr-gate.sh`, in `$live`): a bump needs a run on a unit, unless the programs come out byte-identical
  (ADR 0041).
- The comparisons with the base (ADR 0032, ADR 0041) build each side with the Rust its commit names. The two differ
  only in a pull request that changes the pin, and then the difference is the bump's.
- The digest pins the base image, not the packages a build installs on top (`apt-get install`, `apk add`). Nothing
  looks for a newer digest; a bump is tried by building the image and running the sims, which CI does in the weekly
  run only. The build images are made again after a bump (their tag is a hash of their Dockerfile).
- Building needs rustup, as before: a cargo of the distribution does not read the file. The first `cargo` in the
  tree fetches the release. The workflows need a rustup that installs the active tool chain without a name (1.28 or
  newer; the runners had 1.29.3 on 2026-11-18).

## References

`rust-toolchain.toml`, `tools/lib/docker.sh`, `.github/workflows/ci.yml`, `.github/workflows/weekly.yml`,
`.github/workflows/image.yml`, [docs/fork/deps.md](../../fork/deps.md), ADR 0017, ADR 0020, ADR 0032, ADR 0038,
ADR 0041.
