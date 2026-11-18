# ADR 0048: `too_many_arguments` is allowed for the whole crate in hal only

- Status: accepted (2026-11-18, by the maintainer)
- Date: 2026-11-18
- Category: process

Amends ADR 0017.

## Context

ADR 0017 allows three lints for the whole crate, "the same list in every crate". The reason it gives for
`too_many_arguments` is "step builders take the fields of one register; a struct for each would only move the list".
Step builders are in `oos-hal` only (ADR 0001). In the other three crates the line switched the lint off for code the
reason does not speak of: a new function with twelve arguments passed unseen, and an
`#[allow(clippy::too_many_arguments)]` at a function did nothing, while `tools/ratchet.txt` counted it (ADR 0037).

Measured on main at 604a2771 (2026-11-18), Clippy 0.1.100 with its limit of seven arguments,
`cargo clippy --all-targets -- -W clippy::too_many_arguments` on the host and for `aarch64-unknown-linux-musl`:

| Crate | Over the limit | With an `#[allow]` at the function | Seen only without the crate's line |
|---|---|---|---|
| `oos-hal` | 20 | 8 | 12 |
| `oos-dsp` | 6 | 0 | 6 |
| `oos-ui` | 8 | 5 | 3 |
| `oos-fpgad` | 0 | 0 | 0 |

In hal ten of the twenty build the register sequence of a trigger type (`hal/src/dho/tpu.rs`), and most of the rest
configure a front end, the acquisition or a read-out. Outside hal the line hid nine functions: the two that refresh
the eye page (8 and 11 arguments), a search oracle of a test (9), and in dsp the decoders `uart`, `spi` and
`modbus_ascii` (8, 10, 8), `eye::psij` (8), `dpt::diode` (10) and the test generator `rdm_packet` (9). None of them
takes the fields of a register.

## Decision

`too_many_arguments` is allowed for the whole crate in `oos-hal` only. In `oos-ui` and `oos-fpgad` the lint is on
with this record; `oos-dsp` keeps its line until its six functions are grouped, in one pull request that then
removes it (its `Cargo.toml` says so). Where the lint is on, a function over the limit gets the arguments that
belong together as one small value, or an `#[allow]` at the function with its reason, which `tools/ratchet.txt`
counts. The other two lines of ADR 0017's list stay the same in every crate.

## Alternatives

- **The same list in every crate, as it was.** One rule to remember, and four tables that can be compared by eye.
  But the reason holds in one crate, the line hid nine functions in the others, and it made the count of `allow(`
  in ui say less than it seemed to: five of its attributes guarded nothing.
- **A higher limit (`too-many-arguments-threshold` in a `clippy.toml`).** A limit that the tree passed on that day
  is 12 or more; it would pass nearly every function that is written from now on. Seven is a fair limit for code
  that is not a step builder.
- **No line in hal either, an `#[allow]` at each of its twelve functions.** The same reason twelve times, and
  `allow hal` raised from 9 to 21 (ADR 0037) in code that needs a run on a unit to change.

## Consequences

- **Rollout.** With this record the line leaves `ui/Cargo.toml` and `fpgad/Cargo.toml`. fpgad had nothing over the
  limit. In ui the facts of the eye page travel as one `FactInput`, the samples of the eye source as one tuple, and
  the test oracle takes the `EdgeSeek` its counterpart takes; `populate_eye_facts` loses its attribute, `allow ui`
  goes from 22 to 21. Four attributes of ui stay and now do their work: `seg_step` (`ui/src/board/`), and
  `DrawPipe::feed_codes`, `trignear::edge_smooth_near` and `Phosphor::draw_ex`, which the hardware paths call.
- **dsp keeps the line for now**, with this reason in its `Cargo.toml`: `uart`, `spi` and `modbus_ascii` are called
  from `ui/src/protocols/`, so they change together with their callers, in a pull request of its own that also
  groups the arguments of the other three and removes the line. An attribute at each instead would raise `allow dsp`
  from 0.
- The four `[lints.clippy]` tables differ from now on; the comment above each says how. No check compared them.
- A new function with more than seven arguments fails the layer `clippy` in ui and fpgad. Grouping is the way out;
  an attribute needs another one removed or a raised number (ADR 0037).
- The eight attributes in hal guard nothing while hal has its line. They stay until someone changes those functions;
  removing them lowers `allow hal` and changes no code.
- Worth a second look: hal's line, once the trigger builders take a value per trigger type instead of its fields.

## References

ADR 0001, ADR 0017, ADR 0037; `hal/Cargo.toml`, `dsp/Cargo.toml`, `ui/Cargo.toml`, `fpgad/Cargo.toml`,
`tools/ratchet.txt`; `ui/src/ui/eyepage/page.rs`, `ui/src/ui/eyepage/update.rs`, `ui/src/fastdraw/validation.rs`;
pull request #169, which removed nine attributes from ui before.
