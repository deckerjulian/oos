# ADR 0021: What runs on the scope depends on `libc` only; a desktop build may use its platform's crates

- Status: superseded by ADR 0033 (there is no desktop build with platform crates any more)
- Date: 2026-11-15
- Category: process

## Context

ADR 0013 says that `libc` is the only dependency of the Rust crates. Since the Windows desktop port (fe016e4,
2026-11-13) that is no longer true for every build: `ui/Cargo.toml` has

```toml
[target.'cfg(windows)'.dependencies]
minifb = "0.27"
windows-sys = { version = "0.61", features = [...] }
```

for the window, the input and the network interfaces of the desktop build (`ui/src/desktop.rs`,
`ui/src/announce/windows.rs`, `build-windows.ps1`). Windows has no `libc` in the sense the rest of the code uses it,
and its window and network API through hand-written bindings would be several thousand lines nobody tests on the
scope. Measured with `cargo tree -e normal` on 2026-11-15: for `aarch64-unknown-linux-musl`, `oos-ui` and
`oos-fpgad` pull in `libc` and the crates of the tree, nothing else; for `x86_64-pc-windows-msvc`, `oos-ui` pulls in
five more (`minifb`, `windows-sys` and through them `raw-window-handle`, `winapi`, `windows-link`).

The reasons of ADR 0013 are about the scope: a static binary that anyone can read and build, nothing in the image that
was not written for it. They do not need the desktop build to be bare.

## Decision

- Everything that runs on the scope (`oos-ui` and `oos-fpgad` for `aarch64-unknown-linux-musl`) and the crates
  `oos-hal` and `oos-dsp` on every platform depend on `libc` and the crates of the tree only.
- A build that only runs on a computer (today the Windows desktop build) may depend on the crates of its platform's
  window, input and network API, in a `[target.'cfg(...)'.dependencies]` section of `ui/Cargo.toml` and behind the
  same `cfg` in the code. Each such crate is named in [fork/deps.md](../../fork/deps.md) with what it is for.
- `tools/check.sh target` fails when the scope's build, `oos-hal` or `oos-dsp` gains any other dependency.

## Alternatives

- **Bindings of our own for Windows:** keeps the letter of ADR 0013 at the price of a large, untested piece of
  `unsafe` code that only the desktop build uses.
- **No Windows desktop build:** the port exists and is used; dropping it to keep a rule whose reasons do not apply was
  not wanted.
- **Leave ADR 0013 as it is:** then the tree breaks a rule it states, and the next contributor cannot tell which
  dependencies are fine.

## Consequences

- ADR 0013 holds for the scope as it was meant; its status says "amended by ADR 0021".
- The Windows desktop build needs a network for its first build; the scope's build does not.
- A new platform crate for a desktop build is a pull request that says why, and a line in `fork/deps.md`. A crate that
  would be compiled into the scope's build is still ADR 0013's case and needs a strong reason.
- Worth a second look if a desktop build for another platform needs more than its window and network API.

## References

ADR 0013, `ui/Cargo.toml`, `tools/check.sh` (layer `target`), [fork/deps.md](../../fork/deps.md),
[architecture.md](../../architecture.md).
