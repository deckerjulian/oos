# Installing OOS without opening the scope

The stock system's own update (Storage > Upgrade) installs OOS from a USB stick: no card reader, no second card, no
computer at the scope.

Tested on a file laid out like a stock card, with the stock update script, in a Linux container with the paths of the stock
system (`tools/gel-sim.sh`), and by unit tests. **Not yet verified on hardware.**

Compared with the card route (`README.md`, Installation):

| | USB stick | second card |
|---|---|---|
| Open the scope | no | yes |
| Original card | is the card OOS goes on | stays untouched in a drawer |
| Copy of the whole card | no: a rescue set of what changes, on the stick | yes |
| Kernel hangs before the first script runs | the card has to come out: **the scope must be opened after all** | swap the cards |

That last row is the price. The stock loader has no start counter: when `boot` selects OOS, every power-on starts the OOS
kernel until the first program of the image has run and put the stock selection back. A corrupt image is no problem, the
loader refuses it and starts the stock copy. A kernel that accepts the image and hangs on the way to the first script is.
This is why only models OOS has run on are accepted (section 4).

## 1. Steps

1. Build the image (`README.md`, Installation, steps 3 to 5). It leaves `boot/out/DHO800_DHO900_OOS_Install.GEL`.
2. A FAT32 USB stick with at least 200 MiB free. Copy the file to its top folder. The name has to end in `.GEL` and
   contain `DHO800_DHO900`, or the update ignores it. Keep the name.
3. **Recommended, first a dry run:** an empty file `oos-dry-run.txt` on the stick runs all checks and saves the rescue set, without
   writing the card.
4. On the scope: stick into the front port, Storage > Upgrade, choose the file, start. It takes about a minute. The
   screen says at the end whether it worked.
5. Read `oos-install.log` on the stick.
6. Delete `oos-dry-run.txt` and run the update again.
7. **Keep the stick's folder `OOS-rescue`, ideally copy it to a computer.** It holds your calibration and the stock `recovery`.
8. Switch the scope off and on. OOS starts.

OOS starts once, in one-shot mode: the following power-on starts stock again. To start OOS again run the same update: it
writes nothing but the selection. To stay on OOS: System > Boot mode > Persist (`README.md`, Installation, step 8).

The same install also runs over ADB, without a stick: `README.md`, Over ADB. `oos-update gel-install` is the same code,
only `/rigol/data` has to be saved by hand. That way of starting it has not been tested at all.

## 2. What it does

The update file is a gzip tar. The stock update unpacks it to `/mnt/tmp` and starts its `shell/do_extract.sh` as root. In a normal
update that script brings the new firmware; ours replaces it, so **none of the stock update steps run**: no FPGA flash, no
change under `/rigol`.

`boot/gel/do_extract.sh` copies `/rigol/data` to the stick and starts `oos-update gel-install`
(`hal/src/update/first.rs`):

1. Checks, nothing written: model, signature and image, partition table, boot magic, `backup` empty or holding the stock
   copy, `recovery` holding something known.
2. Rescue set on the stick, each file read back. A complete set is kept: after the first install `recovery` holds OOS.
3. If OOS is selected and `recovery` changes: stock selected first.
4. Stock copy of `boot` to the start of `backup`, read back. The loader's fallback when it refuses `recovery`.
5. Image to `recovery`, read back.
6. Last, one sector: `boot` selects OOS.

A failure or power cut before step 6 leaves a card that starts stock as before. The same file finishes the job.

Written on the card: 4 bytes of `boot`, `recovery`, the first 32 MiB of `backup`. Nothing else: not `uboot`, `trust`,
`parameter`, `/rigol/data` or the FPGA flash. `/oos` is formatted by OOS at its first start, as with the card route.

## 3. On the stick afterwards

| File | Content |
|---|---|
| `oos-install.log` | the report of every run |
| `OOS-rescue/rigol-data/` | copy of `/rigol/data`: calibration, licences |
| `OOS-rescue/unit.txt` | serial number of the scope the set is from: a stick with the set of another scope stops the install |
| `OOS-rescue/card-head.bin` | the first 36 MiB of the card: loader, vendor storage, `parameter`, `uboot`, `trust`, `misc`, `resource` |
| `OOS-rescue/boot.bin`, `recovery.bin` | those partitions as they were |
| `OOS-rescue/SHA256SUMS` | hashes; the set counts as complete when this file exists |

The stock `recovery` is replaced. Whatever starts into it no longer works; everyday stock use does not.

## 4. Models

Only models OOS has run on: `first::VERIFIED_MODELS`, today the DHO924S. On another model the update stops with a message in the
log. An empty file `oos-any-model.txt` on the stick overrides this. Do that only if you are ready to open the scope.

## 5. Going back

Stock starts by itself after every OOS start in one-shot mode. In persist mode: System > Boot mode > One-shot, then power-cycle. OOS stays in `recovery` and does nothing there.

Putting the stock `recovery` back needs the card in a reader (`card-install.py restore`, with `OOS-rescue/recovery.bin`).
There is no update file for this yet.

If the scope shows only a black screen: `README.md`, If something goes wrong.

## 6. To check on the unit

On a card that can be taken out, not on an only card:

1. Dry run: the screen says it worked, `oos-install.log` and `OOS-rescue` are complete, `oos-update status` later shows nothing
   written. The first real test of: `/dev/block/mmcblk0` as the card, the static tool on the stock kernel, reading `vendor.bin`.
2. Install, power-cycle: OOS starts, with `/oos` formatted if it was empty.
3. Next power-on: stock, with calibration and licences as before.
4. The same update again: "already holds", OOS selected.
5. Damage test: a refused image in `recovery` (card in a reader) starts the stock copy in `backup`.
