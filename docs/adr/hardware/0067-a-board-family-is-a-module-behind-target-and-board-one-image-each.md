# ADR 0067: A board family is a module behind `Target` and `Board`, with an image of its own, and drives hardware once the maintainer calls it ready

- Status: proposed
- Date: 2026-11-19
- Category: hardware

## Context

OOS runs on the DHO800/DHO900. Owners of other Rigol models ask for it, most often the MHO900/MHO98 (forum,
2026-11-18). Reported for the MHO900 and the DHO1000/DHO4000, not verified here: the same RK3399 processor, Android
from an SD card, `/rigol/data/vendor.bin`. Their acquisition part is another board: other ADCs, another FPGA, other
front ends and clocks ([porting.md](../../porting.md)).

The code was prepared for this on 2026-11-08 (83ac73a2, 4bd8298e, 02b74258, 203544df):

- `hal/src/target.rs` describes a board family (`Target`: card layout, update target, partition table marks, models,
  limits). `hal/src/dho/target.rs` fills it in, and the neutral code reads `target::ACTIVE`.
- `ui/src/board/mod.rs` has the trait `Board`, the only way from the screen to the hardware. `board::Active` names the
  DHO800/DHO900 implementation in `ui/src/board/dho/`, and a lint test keeps `dho` out of the rest of the GUI.
- `rootfs/board/<name>/` holds what the image takes from the board: `etc/oos-board` for the scripts and the build, and
  `bin/oos-board`, which `rcS` asks (`has-hardware`, `early`, `drivers`, `ui-args`, `bring-up`, `mac`). A hal test
  keeps `etc/oos-board` equal to `Target`, and another keeps the common scripts free of board addresses.
- A model outside the DHO800/DHO900 family in `vendor.bin` makes OOS drive no hardware and show the demo.

ADR 0001 says this in one sentence ("a port to another board family adds a module next to it"). Nothing decided the
rest: how many images, when a port may drive a unit, what CI checks per board, what becomes of `oos-fpgad` (a
DHO800/DHO900 tool, 127 uses of `oos_hal::dho` in `fpgad/src/`), and how a forgotten part of a port is found. The
maintainer answered these questions in the session of 2026-11-19; this record writes the answers down.

## Decision

1. **One image per board family.** The family is chosen when OOS is built: `OOS_BOARD=<name>` for the image scripts,
   `target::ACTIVE` in `oos-hal` and `board::Active` in `oos-ui`. One image never carries two families. An update
   package names its family (`Target::update_target`) and a scope refuses one for another family. The release files
   carry the family in their names (`OOS_IMAGE_NAME`, `OOS_GEL_STEM`).
2. **A family is ready when the maintainer says so.** A port may enter main early, as a module that builds and is
   tested on the host. Until the maintainer marks it ready, in a pull request of the maintainer's that changes its row
   in [porting.md](../../porting.md#board-families), its image is a development build for a bench, no release carries
   an image for it, and none of its models counts as verified. ADR 0009, ADR 0010 and ADR 0015 hold for every family: no write
   to the flash, `/rigol/data` and the loader's partitions; a hardware function that is not verified on a unit is off
   by default; each hardware statement says how it is known.
3. **CI checks every family.** A family gets its own host build and tests, its own register sequences
   (`tools/plan-matrix.sh`), its own comparison with the base (`tools/compare-base.sh`), its own device-test profile
   and, once ready, its own image in `image.yml`. A change in neutral code is checked against every family. A change
   in one family's module is checked against that family only.
4. **The tools are built for every board.** `oos-fpgad` becomes a neutral command line (arguments, the `--execute`
   rule, `model`, `plan`, reading and writing the register window) with the subcommands of each family in a module of
   its own (`fpgad/src/<name>/`), the way `oos-ui` reaches the hardware through `Board`. The same holds for the host
   tools that know the DHO800/DHO900 (`tools/plan-matrix.sh`, `tools/device-test.py`, the simulations). They take the
   family as an argument and keep `dho` as the default. A family's `bin/oos-board` calls its own subcommands.
5. **A board family is a kind of extension (ADR 0046).** It is declared once, in its `Target`
   (`hal/src/<name>/target.rs`). Its recipe is in [CONTRIBUTING.md](../../../CONTRIBUTING.md#a-new-board-family), and
   `hal/tests/board_families.rs` fails, naming the family and the part, when a folder of `rootfs/board/` lacks its
   `Target`, its `Board` module, a key of `etc/oos-board`, a command of `bin/oos-board` or its row in
   [porting.md](../../porting.md#board-families), and when a row there has no folder.

## Alternatives

- **One image for every family, the board chosen at start.** It would save a release file per family, but every image
  would carry every kernel module, device tree and FPGA path. A wrong guess of the board at start would write to the
  wrong hardware. Choosing the family when OOS is built keeps each image to the parts its board has, and a package for
  another board is refused before anything is written.
- **A port in a fork until it is complete.** It keeps main small. But the neutral code moves every day, and a fork
  that lags behind by weeks cannot be merged. A port in main builds and is tested from its first day, and point 2
  keeps it out of the releases until it is ready.
- **`oos-fpgad` stays the DHO800/DHO900 tool, a second tool per family.** Two tools would grow two copies of the
  argument parsing, the `--execute` rule and the register access, and AGENTS.md asks for reuse first. One neutral
  command line with one module per family is the shape `Board` already has.

## Consequences

- The MHO900/MHO98, or any other family, can start as a pull request with an empty `Target`, a `Board` that returns
  the demo, and its row in porting.md marked "in development". The check names what is still missing.
- The parts that exist today are DHO only. The first port, or a refactoring before it, brings:
  - a build switch that picks `target::ACTIVE` and `board::Active` (today they are constants);
  - the family argument of the host tools;
  - the split of `oos-fpgad` into a neutral part and `fpgad/src/dho/`. That split comes with `Behaviour: unchanged`,
    proven by `tools/compare-base.sh`. Until it lands, `oos-fpgad` is not one of the parts the check asks for.
- CI time grows with each family. ADR 0040 (CI runs what a change reaches) keeps a change in one family's module
  from building the others.
- A unit of each family is needed at a bench before the maintainer calls it ready. Register sequences, timing, start
  and update are only seen on a unit (ADR 0052).
- Worth a second look if two families turn out to share most of their acquisition part. Then a shared module under
  both may be better than two copies.

## References

[porting.md](../../porting.md), [models.md](../../models.md), `hal/src/target.rs`, `hal/src/dho/target.rs`,
`ui/src/board/mod.rs`, `rootfs/board/dho/`, `boot/boardconf.py`, `hal/tests/board_families.rs`; ADR 0001, ADR 0009,
ADR 0010, ADR 0015, ADR 0040, ADR 0046, ADR 0052.
