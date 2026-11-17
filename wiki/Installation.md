# Installation

OOS goes on the scope's SD card, next to the stock system. Read the
[disclaimer](https://github.com/dekajoule/oos#disclaimer) before you start. There are three ways:

| Way | Opens the scope | Your original card | You need | State |
|---|---|---|---|---|
| **[From a USB stick](#from-a-usb-stick)** | no | OOS goes on it, beside stock | `DHO800_DHO900_OOS_Install.GEL` of a release | run on a DHO924S with a modified stock system |
| **[On a second card](#on-a-second-card)** | yes | stays untouched in a drawer | `oos-<version>.img` of a release | run on a DHO924S |
| **[Over ADB](#over-adb)** | no | OOS goes on it, beside stock | a build of your own ([[Building]]) | run on a DHO924S with a modified stock system |

A scope that already runs OOS is updated with `oos-<version>.oosupd`: see [[Updates and Going Back]].

## The files of a release

The [latest release](https://github.com/dekajoule/oos/releases/latest) has ready-made files. You need no Rust and no
Docker for them:

| File | What it is for |
|---|---|
| `DHO800_DHO900_OOS_Install.GEL` | install from a USB stick through the stock system, without opening the scope: [From a USB stick](#from-a-usb-stick) |
| `oos-<version>.img` | install on a second card: [On a second card](#on-a-second-card) |
| `oos-<version>.oosupd` | update a scope that already runs OOS: [[Updates and Going Back]]. From 0.1.6 or older, 0.1.7 first |
| `SHA256SUMS` | the checksums of all of them |

Check what you downloaded. The number has to be the same as in the file's line of `SHA256SUMS`:

```sh
shasum -a 256 DHO800_DHO900_OOS_Install.GEL                  # macOS
sha256sum DHO800_DHO900_OOS_Install.GEL                      # Linux
certutil -hashfile DHO800_DHO900_OOS_Install.GEL SHA256      # Windows
```

## From a USB stick

The stock system's own update (**Storage > Upgrade**) installs OOS onto the card that is in the scope.

**Read this before you start**

- **This has run on one DHO924S only, whose stock system is a modified one**; an unmodified stock system has not run it
  yet. Try it on a card you can take out if you can.
- **There is no second card to fall back on.** A refused image is harmless, the loader starts the stock copy. But if
  the kernel hangs before the first script of the image runs, every power-on hangs the same way. The card then has to
  come out, so **the scope has to be opened after all.** Read
  [docs/install-usb.md](https://github.com/dekajoule/oos/blob/main/docs/install-usb.md) first.
- **Models.** The install goes ahead by itself on the models whose hardware OOS has run on: DHO804, DHO814, DHO824 (a
  DHO804 runs OOS) and DHO914S, DHO924S (from 0.1.6; up to 0.1.5 only the DHO924S). On any other model it stops,
  writes nothing, and `oos-install.log` says so: put an empty file `oos-any-model.txt` next to the `.GEL` file to go on
  anyway.
- **What it writes.** `recovery`, the first 32 MiB of `backup` and, as its very last step, the 4 bytes of `boot` that
  select OOS. A failure or power cut before that step leaves a card that starts stock as before, and running the same
  install again finishes the job. No stock update step runs, and the FPGA flash and `/rigol` are not touched.

You need a FAT32 USB stick with at least 200 MiB free.

1. Download `DHO800_DHO900_OOS_Install.GEL` and `SHA256SUMS` from the
   [latest release](https://github.com/dekajoule/oos/releases/latest) and check the file
   ([above](#the-files-of-a-release)), or take it from `boot/out/` of your own build ([[Building]]).
2. Copy the file to the top folder of the stick and keep its name, the scope looks for it. Next to it, create an empty
   file `oos-dry-run.txt`: the first run then only checks and saves a rescue set.
3. Start the stock system and put the stick into the front port. If **Storage** does not show the stick, switch off and
   on and look again.
4. **Storage > Upgrade**, choose the file (about 16 MB; a small file starting with `._` comes from macOS, leave it) and
   start. It takes about a minute, and the screen says at the end whether it worked.
5. Read `oos-install.log` on the stick on a computer. It ends with `exit code 0`.
6. Delete `oos-dry-run.txt` and run the update again. The log now ends with `OOS selected for the next power-on` and
   `exit code 0`.
7. Copy the folder `OOS-rescue` from the stick to a computer and keep it: it holds your calibration and the stock
   `recovery`.
8. Switch off at the mains, wait 30 seconds and switch on. OOS starts. Go on with [[First Steps]].

What each run writes, and what to do when Storage does not show the stick:
[docs/install-usb.md](https://github.com/dekajoule/oos/blob/main/docs/install-usb.md), section 1.

### After the install

OOS starts once, then the next power-on starts stock again ([how OOS starts](#how-oos-starts)). To start OOS once
more, run the same install from the stock system: it writes nothing but the selection. To stay on OOS, set
**System > Boot mode** to **Persist** on OOS.

The stock `recovery` is replaced by OOS. Everyday stock use does not need it, but anything that starts into it no
longer works. Putting it back needs the card in a reader, with `recovery.bin` from the rescue set and
`boot/card-install.py restore`.

## Over ADB

The same install from a computer, with `adb` and a scope that answers on its network port 55555 with root access
(known from a modified stock system only). It needs a build of your own ([[Building]]), and the points under
[From a USB stick](#from-a-usb-stick) hold for it as well. The commands:
[docs/install-usb.md](https://github.com/dekajoule/oos/blob/main/docs/install-usb.md), Over ADB.

## On a second card

**You will open the scope, which voids the warranty.** The boot SD card is inside. Your original card stays untouched
in a drawer: OOS only ever goes on a second card. The whole process takes about an hour, mostly for copying the cards.

### What you need

- A DHO800/DHO900. OOS has run on a DHO924S and on DHO804s (user reports).
- A **second SD card**, best of the same size or larger. OOS is only ever installed on this card. A smaller card works
  too (step 2), down to about 8 GB on a stock card.
- A card reader.
- A computer: macOS with `tools/sdcard-clone.sh`, where these steps were developed, or Windows or Linux with the tools
  [below](#backing-up-and-cloning-the-card). On Linux the card is `/dev/sdX` instead of `/dev/rdiskN`; that path is
  untested.
- Python 3 and this repository: `git clone https://github.com/dekajoule/oos.git`
- The image: `oos-<version>.img` of the [latest release](https://github.com/dekajoule/oos/releases/latest), or a build
  of your own ([[Building]], on macOS or Linux, also on an x86-64 PC).

### Steps

1. **Back up the original card.** This step protects your factory calibration. **Do not skip it.** Switch the scope
   off, unplug the mains cable and all probes, open the instrument and take out the boot SD card. Then, on macOS:

   ```sh
   tools/sdcard-clone.sh list                                       # the card's disk number
   tools/sdcard-clone.sh read diskN ~/oos-backup/sd-original.img     # the whole card, a checksum, a layout report
   ```

   Windows and Linux: [below](#backing-up-and-cloning-the-card). Keep the image in at least two places, then **put the
   original card away**: from here on you only use the second card. Always the whole card, never a partition, and
   never let the computer format or initialise it.
2. **Write the clone to the second card.**

   ```sh
   tools/sdcard-clone.sh write ~/oos-backup/sd-original.img diskM    # asks you to type the disk id, then verifies
   ```

   Cards of the same nominal size differ by a few MB. If the second card is smaller than the image, `write` refuses
   and names the way out: make a copy that fits and write that one. Only the stock `/data` partition at the end gets
   smaller, the original image is only read:

   ```sh
   tools/sdcard-clone.sh fit ~/oos-backup/sd-original.img diskM ~/oos-backup/sd-fit.img
   tools/sdcard-clone.sh write ~/oos-backup/sd-fit.img diskM
   ```

   On Linux, give the card's size in bytes instead of `diskM` (`lsblk -bdno SIZE /dev/sdX`) and write with `dd`.

   Put the second card into the scope, close the instrument and start it once. The stock system must work as before,
   with its calibration and licences. Then unplug the scope and take the card out again.
3. **Install OOS on the second card.** Put it in the reader. Unmount it, then do a dry run, which only checks the card
   and prints the plan. With your own build, write `--image boot/out/oos-dev.img`:

   ```sh
   diskutil unmountDisk /dev/diskM
   sudo python3 boot/card-install.py install /dev/rdiskM --image oos-<version>.img --fallback-stock
   ```

   The tool refuses cards that do not have the stock layout. If the plan looks right, write it:

   ```sh
   sudo python3 boot/card-install.py install /dev/rdiskM --image oos-<version>.img --fallback-stock --execute
   ```

   This saves the current `recovery` partition to `backups/`, writes OOS into `recovery`, reads it back, and copies
   the stock boot image into the empty `backup` partition. That copy is the fallback if the loader refuses OOS.
4. **Select OOS for the next start:**

   ```sh
   sudo python3 boot/switch.py oos /dev/rdiskM --execute
   sudo python3 boot/switch.py show /dev/rdiskM
   ```

5. **First start.** Eject the card, put it into the scope, close the instrument, connect Ethernet and switch on. You see
   the Rigol logo from the stock loader, then the OOS start-up screen with a checklist, then the scope. Go on with
   [[First Steps]].

The stock software has to stay on the card: OOS reads the scope's identity with its help.

### Backing up and cloning the card

Any tool that copies a **whole disk** sector by sector works. Only `tools/sdcard-clone.sh` on macOS has been tested
against a scope's card; the others are common tools that users named.

**Watch out:**

- **The whole card, never a partition.** The card has no MBR and no GPT; its loader and the stock partition table sit
  outside anything a computer shows as a partition. A copy of partitions does not start.
- **Windows cannot read the card and offers to format it**, Disk Management may offer to initialise it: always
  **Cancel**. Each of these writes onto the card.
- **Check the device twice** before you write: an image written to the wrong disk destroys it.
- **Verify the backup**, keep the image in two places, and **keep the original card unchanged in a drawer**. It holds
  your factory calibration and licences. From then on you only use the second card (same size or larger; for a
  smaller one see `tools/sdcard-clone.sh fit` in step 2).

**Windows**

- **Win32 Disk Imager**: the card's drive letter, a new file name ending in `.img`, **Read Only Allocated Partitions**
  unticked, **Read**. Then **Verify Only** with the same card and file: it must say "Verify Successful". For the
  clone: the second card's drive letter, the image, **Write**, then **Verify Only**. If the card gets no drive letter,
  use one of the next tools, which open the physical disk.
- **HDD Raw Copy Tool**: source the card, target FILE (`.img`); for the clone, source the file, target the second card.
- **WinHex**: a disk image of the physical medium (not of a partition); Tools > Disk Tools > Clone Disk for the clone.
- **USBImager**: Read makes an image, Write with Verify ticked makes the clone.
- Checksum of an image: `certutil -hashfile sd-original.img SHA256`.

**macOS**

```sh
tools/sdcard-clone.sh list                                       # the card, e.g. disk4
tools/sdcard-clone.sh read disk4 ~/oos-backup/sd-original.img     # reads, verifies, writes a .sha256
tools/sdcard-clone.sh write ~/oos-backup/sd-original.img disk5    # the second card: asks for the disk id, verifies
```

By hand: `diskutil unmountDisk /dev/diskN`, then `sudo dd if=/dev/rdiskN of=sd-original.img bs=4m status=progress`
(`rdiskN`, the raw device, is much faster than `diskN`). Check: `sudo shasum -a 256 /dev/rdiskN` and
`shasum -a 256 sd-original.img` must be equal.

**Linux**

```sh
lsblk -o NAME,SIZE,MODEL,TRAN                    # the card: /dev/sdX (or /dev/mmcblkN), not sdX1
sudo dd if=/dev/sdX of=sd-original.img bs=4M status=progress conv=fsync
sudo sha256sum /dev/sdX sd-original.img          # must be equal
sudo dd if=sd-original.img of=/dev/sdY bs=4M status=progress conv=fsync      # the clone, second card /dev/sdY
sudo cmp -n "$(stat -c %s sd-original.img)" sd-original.img /dev/sdY && echo same
```

`tools/sdcard-clone.sh` needs macOS, except `fit` (a copy of the image for a smaller card), which takes the card's size
in bytes on Linux. More detail:
[docs/sdcard-backup.md](https://github.com/dekajoule/oos/blob/main/docs/sdcard-backup.md).

## How OOS starts

By default OOS is **one-shot**: every start of OOS restores the stock boot, so the **next power-on starts the stock
system**. This is the safe mode for a new installation.

To start OOS once more, run this in a telnet or SSH session (or on the serial console) and power-cycle the scope:

```sh
oos-boot-oos-again
```

After an install without opening the scope, running the same install again from the stock system does the same
([After the install](#after-the-install)).

When OOS works for you, make it the normal system: set **System > Boot mode** to **Persist**, or run
`oos-update mode persist`. It counts from the next power-on. One-shot goes back, and the next power-on starts stock.

Even then, a start that does not complete leaves the stock boot in place, so the next power-on starts the stock system.
Switch the scope off at the mains and wait 30 seconds between two starts of OOS.

Going back to stock, for one start or for good: [[Updates and Going Back]].
