# ADR 0064: A layer of `oos-ui` without crossings becomes a crate in the workspace `ui/`, the system services first

- Status: accepted (2026-11-19, by the maintainer)
- Date: 2026-11-18
- Category: code

## Context

`oos-ui` is one program of 69 213 lines of running code (`tools/code-size.py`, commit d4c66613), 68 % of the four
crates. The screen (`ui/src/ui/` and the drawing beside it) is about half of it. The rest is the model, the settings
table, the setup files, the measurements, the SCPI interpreter, its transports and the services of the system: mDNS,
SNTP, SSH, USBTMC, `net.allow`, the clock, `system.conf`. All of it is compiled as one crate, so nothing but a count
keeps one part from naming another.

ADR 0049 put the modules of `oos-ui` in layers and counts the paths that go against them (`tools/layers.py`). It
named a crate per layer as the alternative that would be a proof instead of a count, and left it "worth a second look
when the counts are 0: then a layer can become a crate, one at a time". In the forum thread, a contributor asked for
the same from the other side: a core with stable interfaces, so that a GUI, an integration or an add-on is built
against them and not inside the core (issues #199, #205, #206; ADR 0055, ADR 0056).

Measured on d4c66613:

- `ui-base-up` was 2: `ssh.rs` named `http::base64`. Moving base64 and the thread pinning out of `http.rs` into base
  (the first commit of this change) made it 0, and `ui-signal-up` 9 → 6 and `ui-screen-up` 5 → 4 with it.
- Of the 21 modules of base, 16 hold no drawing: `base64`, `clock`, `ethlink`, `hostname`, `json`, `mdns`, `net`,
  `net6`, `netacl`, `placement`, `remote`, `sntp`, `ssh`, `sysconf`, `system`, `usbtmc` (3 777 lines of running code).
  They name each other, `oos-hal` (`fsutil`, `update`) and `libc`, and nothing else of `oos-ui`; their tests name
  `ui::testkit::Scratch` (a scratch directory) and the hostile-input generator. The other five (`drm`, `font`,
  `gfx`, `layout`, `vpu`) and `hid` are the screen's own helpers.
- Of the transports, `vxi11`, `hislip` and `announce` name only base and three items of `scpi`: `Program` (what a
  transport hands to the interpreter and keeps until its answer is complete), `RESP_MAX` and `send` (the blocking
  write of an answer). `http` names `Ui` and five items of `store` (`ui-transport-skip` 7).
- `model` 25, `settings` 32, `store` 18, `signal` 6, `board` 47: these are not near 0.

ADR 0049 also listed what a crate costs: path dependencies and lock files through `tools/check.sh`, `ui/build.sh`,
the image build and CI's caches; `pub` in place of `pub(crate)` on what another crate names; and no inlining across
the boundary in the tests, which run at `opt-level = 1` without LTO (ADR 0040).

## Decision

**`ui/` is a Cargo workspace: `oos-ui` is its root package, and a layer of ADR 0049 whose count is 0 leaves
`oos-ui` as a crate of its own in a folder below `ui/`, one pull request per crate. `oos-ui` brings each of its
modules into its crate root with `use`, so the paths of `oos-ui` (`crate::sysconf`) stay as they are.**

- **First `oos-system`** (`ui/system/`, this change): the 16 modules of base above. Its `Cargo.toml` depends on
  `libc` and `oos-hal` only. What the tests of `oos-ui` also use (the scratch directory, the test switches of
  `netacl` and `sysconf`, `J::num`) is behind its feature `testkit`, which `oos-ui` turns on as a dev-dependency, so
  the program for the scope is built without it.
- **Next `oos-remote`** (`ui/remote/`, its own pull request): `vxi11`, `hislip` and `announce`, after `send` and
  `RESP_MAX` move into base (they are about the socket, not the interpreter). A transport is generic over the
  program it carries (a trait with `new(text)`); `oos-ui` names `scpi::Program` there. The tests that run a program
  on the demo screen stay in `oos-ui`; the crate keeps the tests of its framing and its hostile-input tests.
- **Later, each when its count is 0:** `http` joins `oos-remote` once it no longer names `Ui` and `store` (through
  a trait for what the web remote asks of the screen); model and settings become `oos-model`; then `store`. The
  screen, the board, the acquisition and the frame loop stay `oos-ui`.
- **One workspace, one lock file, one target folder.** `cargo test`, `clippy` and `llvm-cov` run with `--workspace`
  in `ui/` (`tools/check.sh`, the workflows, `tools/stress-tests.sh`, `tools/agent-target.py`); `cargo fmt` already
  formats the members. `ui/build.sh`, the image build and `tools/ci-parts.sh` need no change: a crate below `ui/`
  belongs to the part `ui`, and building `oos-ui` builds its members.
- **What a crate below `ui/` may depend on** is the same as for `oos-ui`: `libc` and the crates of the tree (ADR
  0013, ADR 0033); `tools/check.sh target` already checks it through `cargo tree` of `oos-ui`.
- **Not decided here:** how the network services run (a thread of their own, non-blocking I/O, an async runtime;
  issue #192). The crate boundary makes either easier to change; the decision comes with its measurements.

## Alternatives

- **The new crates beside `hal/` and `dsp/`** (`system/` at the top of the tree). Each would have its own lock file
  and target folder, its own entry in `tools/check.sh`, `tools/ci-parts.sh`, `tools/code-size.py`, CI's caches and
  the workflows. Below `ui/` they are reached as part of `ui` and need none of that. They are crates of `oos-ui`'s
  program, not of the tree; `oos-fpgad` does not use them.
- **One crate for the whole base layer, the drawing helpers too.** `gfx`, `font`, `layout`, `drm` and `vpu` are used
  by the screen and the GPU helper only; in a crate of system services they would make it a crate of everything
  below the model. They can become a crate of their own when the screen is split further.
- **Rewriting the paths of `oos-ui` to `oos_system::...`** instead of bringing the modules into the crate root. About
  200 paths in 70 files would change for no gain in what the compiler checks, and every open branch would conflict
  with them. The `use` lines in `main.rs` say where the modules are.
- **Keep counting (ADR 0049) and make no crate.** The count of base was 0, and stays 0 only as long as nobody adds a
  path; a crate cannot get one. A count also cannot tell a contributor which part they may change without building
  the screen, a crate can.
- **One pull request for all crates.** The transports need their program made generic and their tests split; the
  system services need neither. One crate a pull request keeps each one a move that the comparison can show equal.

## Consequences

- `oos-system` names nothing of `oos-ui`, and from now on cannot: a path upward is a compiler error, not a count.
  `tools/layers.py` keeps the rows of its modules (they are still named from the crate root of `oos-ui`), and
  `tools/duplicates.py` and `tools/code-metrics.py` read `ui/system/src` too.
- **`pub` on what crosses.** Four functions of `mdns` (`write_name`, `txt_bytes`, `parse_ends`, `Msg::standard`)
  went from `pub(crate)` to `pub` because `announce` names them. Each later crate adds such a list; it is the
  interface of that crate.
- **Dead code.** The scope build is the guard against dead code (ADR 0017, `main.rs`). In a library a `pub` item
  that nothing uses is not reported, so an unused function of `oos-system` stays unseen. Private items are still
  checked.
- **The program is not byte-identical** to the one before (the paths of the symbols change), so ADR 0041 does not
  apply. The change touches no path that only a unit proves (`tools/pr-gate.sh`), and `tools/compare-base.sh` shows
  every picture and every register write equal.
- **Tests:** a crate below `ui/` runs its own tests in `cargo test --workspace`; across the boundary there is no
  inlining at `opt-level = 1`, while the release build keeps LTO over all crates. `testkit` is the one place for
  test helpers that two crates share.
- A version number: the crates below `ui/` are `0.0.0` and `publish = false`; the release number stays that of
  `oos-ui` (`image.yml` checks the four crates of the tree). The interface they offer each other is internal and
  not part of the public interface of ADR 0055.
- Worth a second look: when `http` is the last thing that keeps `oos-remote` from holding every transport; when the
  drawing helpers are wanted by a second program (a desktop GUI outside the tree, issue #199); when a crate below
  `ui/` is wanted by another crate of the tree, which would move it beside `hal/` and `dsp/`.

## References

ADR 0001 (three crates; amended: `oos-ui` is a workspace of crates), ADR 0013, ADR 0014, ADR 0017, ADR 0022,
ADR 0033, ADR 0040, ADR 0041, ADR 0049 (the layers; amended: a layer at 0 may leave as a crate), ADR 0055, ADR 0056;
issues #120 (goal S3), #188, #192, #199, #205, #206; `ui/Cargo.toml`, `ui/system/`, `ui/src/main.rs`,
`tools/layers.py`, [architecture.md](../../architecture.md) (Layers inside oos-ui).
