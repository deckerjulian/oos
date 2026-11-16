# ADR 0028: The MHO900 gets OOS's platform first, with no acquisition, as a board chosen at build time

- Status: proposed
- Date: 2026-11-16
- Category: system (touches ADR 0009, ADR 0010, ADR 0013, ADR 0015)

## Context

An owner of an MHO954 ran `tools/platform-probe.sh` (GitHub issue #4: firmware 00.01.00, hardware 3). Compared with the
stock survey of a DHO924S, the main board is the same as far as the probe shows: the RK3399 and its memory map, the
PMIC with every rail at the same voltage, the DSI panel and the touch on the same pins, the I2C devices, Ethernet,
and a card whose partitions sit at the same sectors. The acquisition board is another: a Kintex-7 the SoC loads at
every start over GPIOs, XDMA `10ee:7024` with PERST# on another pin, JESD204 ADCs behind another clock bus, other
front-end lines, other calibration files, a generator board with its own MCU. Several pins the DHO's device tree
drives are acquisition lines on the MHO (GPIO4_A3, GPIO1_C2). Not yet verified on hardware: nothing of OOS has run on
an MHO900, and no MHO900 is at hand.

`docs/porting.md` already keeps the board behind `Target` (hal) and `Board` (ui) and names the build-time choice.

## Decision

- A board is chosen when OOS is built, by a Cargo feature: none for the DHO800/DHO900, `board-mho` for the MHO900
  (`oos-hal`, passed on by `oos-ui` and `oos-fpgad`), and `OOS_BOARD=mho` for the image. One kernel serves both; each
  board has its device tree.
- The MHO900 starts as a port of the platform alone: `Target::acquisition` is false, so `Platform::drives_hardware`
  is false whatever vendor.bin says. Nothing touches the FPGA, the front ends, the clocks, the generator or the front
  panel; the screen shows the demo signal. The device tree derives from the DHO's and drops every DHO line that is
  something else on the MHO; the MHO's acquisition pins are left undriven.
- The first install from stock takes no MHO model by itself (no variant has run OOS), and MHO packages carry their
  own update target.

## Alternatives

- **Wait until the acquisition is worked out:** nothing to try on a unit before that, and the platform questions
  (kernel, panel, touch, card) would come mixed with the hard ones.
- **One image for both boards, chosen at run time** (vendor.bin or the device tree): one build, but both boards'
  code in every image, the board decided where a wrong answer drives the wrong hardware, and the kernel's device tree
  still has to be chosen before anything runs.
- **Treat the MHO as a "foreign" model of the DHO build** (what the DHO build does with an MHO's vendor file): demo
  already, but with the DHO's device tree, which drives MHO acquisition lines (beeper, backlight enable, fan line).

## Consequences

- Board code is compiled per feature in `oos-ui`; the UI's own tests reach into the DHO's internals in places and run
  without the feature only. `tools/check.sh` builds the MHO900 for the host and the scope and runs `oos-hal`'s board
  tests with the feature; the testable parts of the MHO's board (scale steps, thermal zones) live in `oos-hal`.
- The MHO900 build is not tested on a unit: the first start needs a person with an MHO900 and a copy of the card
  (docs/mho.md).
- Acquisition on the MHO900 is a new piece of work at the bench, as for any port (porting.md).
- Worth a second look when the acquisition is worked out (`acquisition: true` and a `Board` with a thread), or when a
  third board makes a run-time choice cheaper than a build per board.

## References

`hal/src/target.rs`, `hal/src/mho/`, `ui/src/board/mho/`, `rootfs/board/mho/`, `kernel/dts/rk3399-rigol-mho900.dts`,
[mho.md](../../mho.md), [porting.md](../../porting.md), ADR 0009, ADR 0010, ADR 0013, ADR 0015.
