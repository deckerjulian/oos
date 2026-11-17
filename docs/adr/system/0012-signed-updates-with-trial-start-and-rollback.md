# ADR 0012: Updates are signed, started on trial and rolled back when they fail

- Status: accepted
- Date: 2026-11-07 (first public version); trial start by kexec with a new device tree from 0.1.7
- Category: system

## Context

An update replaces the whole image in `recovery`. The scope has no loader of OOS's own that could count failed starts
(ADR 0011), many owners have no second card, and a damaged or wrong package must not leave a scope that needs a
computer to recover.

## Decision

- An update is one package (`.oosupd`) with hashes and an Ed25519 signature. It is checked completely, also against
  what the stock loader accepts, before anything is written.
- The new image is first started by kexec with `recovery` untouched. Only an image that came up is written, and its
  first start from the card is a trial: the watcher confirms it when the scope screen runs stable, or writes the
  previous image back from the rollback slot.
- While `recovery` is written the boot magic is `KRNL`, so a power loss is followed by the stock system.
- Every build trusts a community key that is in the repository on purpose, so anyone can build an update that installs
  on any OOS scope. An owner who wants only their own builds replaces it with their own key.

Ed25519 and SHA-2 are implemented in the tree (ADR 0013).

## Consequences

- The signature with the community key stops damaged and mistaken packages, not an attacker; the documentation says so.
- Not yet verified on hardware: the automatic rollback of a bad update (installs, the kexec trial start and its
  confirmation ran on a DHO924S, 2026-11-07/08).
- A hardware fault during the cold boot counts as a good start; System > Roll back update is the way back then.
- A private signing key that is lost means installing an image by hand on scopes that trust only that key.

## References

[update.md](../../update.md), `hal/src/update/`, `fpgad/src/bin/oos-update.rs`, `keys/update.pub`.
