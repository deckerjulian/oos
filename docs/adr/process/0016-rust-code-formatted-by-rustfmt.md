# ADR 0016: Rust code is formatted by rustfmt with the tree's settings, and CI checks it

- Status: proposed (accepted when the one formatting commit is in main, see Consequences)
- Date: 2026-11-15
- Category: process

## Context

The Rust code has grown from several hands at once: main, two forks, five topic pull requests, and agents working for
each of them. Each wrote in its own layout. Some test files hold ten statements on a line without spaces
(`ui.m.ch[0].offset=-20.81701;`), others follow rustfmt, most follow neither. A pull request that touches such a file
mixes the change with layout, and a review has to find the one line that matters among fifty that only moved.

Measured on main at 84deb24 (2026-11-15), rustfmt 1.11, over `hal`, `dsp`, `ui` and `fpgad` (about 135 000 lines):

| rustfmt settings | places that differ |
|---|---|
| defaults (`max_width = 100`) | 7 307 |
| `max_width = 120` | 5 787 |
| `max_width = 120`, `use_small_heuristics = "Max"` | 3 463 |

No setting comes close to the code as it is, so there is no way around one commit that formats everything.

The dense layout also hides real questions from the lints: Clippy reads `offset=-20.81701` as a possible `-=` typo
(six times in `ui/src/ui/zoom_tests.rs`) and a line with two `if` blocks as a possibly missing `else`. All seven were
false alarms, but each had to be read by hand to know that (ADR 0017).

## Decision

All Rust code in the tree is formatted by `cargo fmt` with the one [rustfmt.toml](../../../rustfmt.toml) at the root of
the tree, stable options only:

```toml
edition = "2021"
max_width = 120
use_small_heuristics = "Max"
```

120 columns is the width the documents and the comments are already wrapped at; `"Max"` keeps short calls and
struct literals on one line where they fit, the closest to the code as it is (table above). CI fails when
`cargo fmt --check` reports a difference in any crate.

Generated tables and register lists whose layout carries meaning may be marked `#[rustfmt::skip]`, with a comment that
says why. Nothing else is skipped.

## Alternatives

- **No formatter, a written style guide:** that is the state now, and it did not hold with more than one writer.
  Agents follow a tool's output better than prose.
- **rustfmt defaults (100 columns):** twice the changed places of the chosen settings, and a width the rest of the tree
  does not use.
- **Format only the files a pull request touches:** spreads the layout noise over many reviews instead of one commit
  nobody has to review, and `cargo fmt --check` cannot be enforced until the last file was touched.
- **Unstable rustfmt options (`imports_granularity`, `group_imports`, `wrap_comments`):** need a nightly tool chain on
  every computer and in CI. Not worth it for this project.

## Consequences

- **Rollout, once:** one commit `Format: cargo fmt over all crates`, made right after a release and after the open pull
  requests are merged, with nothing else in it. Its evidence is ADR 0014: every test passes, `layout-check.sh check`
  reports no picture and `plan-matrix.sh` prints the same register sequences before and after. Its hash goes into
  `.git-blame-ignore-revs`, so `git blame` skips it (`git config blame.ignoreRevsFile .git-blame-ignore-revs`; GitHub
  reads the file by itself). In the same commit CI gets the `cargo fmt --check` step and this record the status
  "accepted".
- **Branches that started before the rollout** run `cargo fmt` on their own branch first (with the new
  `rustfmt.toml`), then rebase. Most conflicts then go away, because both sides have the same layout.
- Layout is no longer a review topic. A comment about it in a review means the tool settings are wrong; change the
  record, not the code by hand.
- Everyone, person or agent, runs `cargo fmt` before a commit. `tools/check.sh fmt` does it for all crates (ADR 0020).
- Worth a second look when rustfmt's stable options change, or when the 120 columns read badly on the screens people
  review on.

## References

[rustfmt.toml](../../../rustfmt.toml), [.editorconfig](../../../.editorconfig), ADR 0014, ADR 0017, ADR 0020,
[CONTRIBUTING.md](../../../CONTRIBUTING.md).
