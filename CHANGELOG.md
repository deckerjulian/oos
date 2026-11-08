# Changes

## Unreleased

Tested:

- The USB stick install ran on a DHO924S: dry run, first install over the stock `recovery`, and the run again.

Inside:

- The board is kept apart, for a port to another one (docs/porting.md): `oos_hal::dho` holds the DHO800/DHO900, the
  GUI reaches the hardware only through `board::Board` (`ui/src/board/`), and a lint test keeps it that way. No change
  in what the scope does: the register sequences are the same as in 0.1.1.
- A description per board (`hal::target`, `rootfs/board/<name>`): card layout, update target, models and limits, what
  `rcS` has to start. The GUI takes channels, generators, memory depths and couplings from the board.

## 0.1.1

From 0.1.0: install the package as before, then power-cycle the scope. From 0.1.1 on, updates start the new build by
kexec first.

Fixes:

- Protocol trigger in Normal: in roll mode the picture stood still with WAIT although data came in. Roll and scan frames
  are no longer held back, and neither are records without the decoder's input.
- Protocol trigger with Single: it waits for a record that holds the event instead of stopping at the first edge.
- System > Overclock no longer wraps from 1.25 GHz to the 2 GHz test clock on one step down, and leaves a limits.conf
  alone that it cannot read.
- The record history keeps the logic analyser data of records up to 1 M words only. With deep memory it could fill the
  scope's RAM.
- Spectrum and eye take the first channel that is on when their source is off.
- The search for the vendor.bin keys skips libraries without the code pattern, remembers a search that found nothing,
  and stays within the file on damaged headers.

- Updates: selecting OOS by hand after a cut install no longer picks the half-written image; a trial is confirmed
  only by the build that runs; images outside the loader's limits are refused.

New:

- Updates start the new build by kexec before anything is written. Only that start, once its kernel found the card,
  installs the package. A kernel that hangs before the start-up no longer needs the card taken out: power-cycle, and
  the previous build starts again. The power-on after the install is the trial start. Not for a new device tree
  (`--new-dt`). `oos-update kexec-check` loads and unloads a package's kernel without a restart.
- The USB stick install, run again, keeps a valid OOS image in `recovery` and only selects OOS: a newer build installed
  over the network stays. `oos-reinstall.txt` on the stick writes the stick's build; a damaged `recovery` gets the
  rollback image back.
- SCPI `:WAVeform:MODE RAW` reads every sample of the record in the screen window.
- OOS drives no hardware on another board: when `vendor.bin` names a model outside the DHO800/DHO900 family,
  `oos-fpgad` refuses and `oos-ui` shows the demo signal.
- `tools/platform-probe.sh` describes another Rigol model for a port, read only ([docs/porting.md](docs/porting.md)).

Measured:

- The generator's AM, FM and PM are accurate within 3 %. The earlier scatter came from the test, which read the screen
  record.
- The logic analyser loses no data at slow time bases. The gaps seen before came from the test equipment.

## 0.1.0

First public version.
