# ADR 0013: No vendor code or data in the tree; `libc` is the only dependency

- Status: accepted (`libc` is the only dependency of every build again, ADR 0033)
- Date: 2026-11-07 (first public version; the rule from the start)
- Category: process

## Context

OOS is published under GPL-2.0-or-later and replaces the software of a commercial instrument. It must be clear that
everything in the tree may be there, and the scope's software should be small enough to read and to build without a
network of packages.

## Decision

- No firmware images, no code or binaries from any stock or modified firmware, no calibration files and no
  `vendor.bin` enter the tree. What OOS knows about the hardware is written down in `docs/` as behaviour (registers,
  sequences, formats), with how it is known (ADR 0015).
- The Rust crates depend on `libc` only. What is needed beyond it is written in the tree (the renderer, text drawing,
  HTTP, SCPI, Ed25519 and SHA-2, the decoders). A new dependency needs a strong reason.
- No personal data in issues or commits: no serial numbers, MAC addresses or licence keys.

## Consequences

- `oos-ui` is one static musl binary that builds on any computer with a Rust tool chain, without Docker.
- More code of our own to test: cryptography against the RFC and FIPS vectors, the renderer against pictures.
- What needs a large library runs outside that binary: the GPU helper with Mesa is a separate process (ADR 0003).
- Contributions that copy from other firmware cannot be merged, however useful.

## References

[CONTRIBUTING.md](../../../CONTRIBUTING.md), [architecture.md](../../architecture.md),
[fork/deps.md](../../fork/deps.md), ADR 0021.
