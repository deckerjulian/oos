# Installation

This guide is the short way through. The exact commands and every option are in the [README](https://github.com/dekajoule/oos#installation).

There are two ways, both with the files of the [latest release](https://github.com/dekajoule/oos/releases/latest), so you do not have to build anything:

- **[From a USB stick](#from-a-usb-stick)**, with `DHO800_DHO900_OOS_Install.GEL`: the scope stays closed, OOS goes on the card that is inside, beside stock.
- **[On a second card](#on-a-second-card)**, with `oos-<version>.img`: you open the scope, your original card stays untouched in a drawer.

A scope that already runs OOS is updated with `oos-<version>.oosupd`: see [[Updates and Going Back]].

## From a USB stick

You need a FAT32 USB stick with at least 200 MiB free.

1. Download `DHO800_DHO900_OOS_Install.GEL` and `SHA256SUMS` from the [latest release](https://github.com/dekajoule/oos/releases/latest). Compare the checksum: `shasum -a 256 DHO800_DHO900_OOS_Install.GEL` on macOS, `sha256sum` on Linux, `certutil -hashfile DHO800_DHO900_OOS_Install.GEL SHA256` on Windows. It has to match the file's line in `SHA256SUMS`.
2. Copy the file to the top folder of the stick and keep its name, the scope looks for it. Next to it, create an empty file `oos-dry-run.txt`: the first run then only checks and saves a rescue set.
3. Start the stock system and put the stick into the front port. If **Storage** does not show the stick, switch off and on and look again.
4. **Storage > Upgrade**, choose the file (about 16 MB; a small file starting with `._` comes from macOS, leave it) and start. It takes about a minute, and the screen says at the end whether it worked.
5. Read `oos-install.log` on the stick on a computer. It ends with `exit code 0`.
6. Delete `oos-dry-run.txt` and run the update again. The log now ends with `OOS selected for the next power-on` and `exit code 0`.
7. Copy the folder `OOS-rescue` from the stick to a computer and keep it: it holds your calibration and the stock `recovery`.
8. Switch off at the mains, wait 30 seconds and switch on. OOS starts. Go on with [[First Steps]].

OOS starts once: the power-on after it starts stock again. To stay on OOS, set **System > Boot mode** to **Persist**.

This ran on a DHO924S whose stock system is a modified one; an unmodified stock system has not run it yet. The install goes ahead by itself on the models whose hardware OOS has run on: DHO804, DHO814, DHO824 (a DHO804 runs OOS) and DHO914S, DHO924S (from 0.1.6; up to 0.1.5 only the DHO924S). On any other model it stops, writes nothing, and `oos-install.log` says so: put an empty file `oos-any-model.txt` next to the `.GEL` file to go on anyway. A kernel that hangs before the first script would need the card taken out: read [docs/install-usb.md](https://github.com/dekajoule/oos/blob/main/docs/install-usb.md) first.

## On a second card

**You will open the scope, which voids the warranty.** The boot SD card is inside. Your original card stays untouched in a drawer: OOS only ever goes on a second card.

### What you need

- A DHO800/DHO900. OOS has run on a DHO924S and on DHO804s (user reports).
- A second SD card, best of the same size or larger, and a card reader. A smaller card works too, see step 2.
- A computer: macOS with `tools/sdcard-clone.sh`, or Windows or Linux with the tools [below](#backing-up-and-cloning-the-card).
- This repository: `git clone https://github.com/dekajoule/oos.git`

### Steps

1. **Back up the original card.** Take it out and make an image of the whole card: `tools/sdcard-clone.sh` on macOS, [below](#backing-up-and-cloning-the-card) for Windows and Linux. Verify it, keep the image in two places, then put the card away.
2. **Write the clone to the second card.** From here on you only use this card. Cards of the same nominal size differ by a few MB; if `write` says the image is larger than the card, make a copy that fits and write that one. Only the stock `/data` partition at the end gets smaller, the original image is only read:

   ```sh
   tools/sdcard-clone.sh fit ~/oos-backup/sd-original.img diskM ~/oos-backup/sd-fit.img
   tools/sdcard-clone.sh write ~/oos-backup/sd-fit.img diskM
   ```

   On Linux, give the card's size in bytes instead of `diskM` (`lsblk -bdno SIZE /dev/sdX`) and write with `dd`.
3. **Get the image.** Either download `oos-0.1.12.img` from the [release](https://github.com/dekajoule/oos/releases/latest) and check it against `SHA256SUMS`, or build it yourself: [[Building]], on macOS or Linux, also on an x86-64 PC.
4. **Install OOS on the second card.** Do the dry run first, it only prints the plan:

   ```sh
   diskutil unmountDisk /dev/diskM
   sudo python3 boot/card-install.py install /dev/rdiskM --image oos-0.1.12.img --fallback-stock
   ```

   If the plan looks right, add `--execute`. The tool refuses cards without the stock layout.
5. **Select OOS for the next start:**

   ```sh
   sudo python3 boot/switch.py oos /dev/rdiskM --execute
   sudo python3 boot/switch.py show /dev/rdiskM
   ```

6. Put the card into the scope, close it, connect Ethernet and switch on. Go on with [[First Steps]].

The stock software has to stay on the card: OOS reads the scope's identity with its help.

### Backing up and cloning the card

Any tool that copies a **whole disk** sector by sector works. Only `tools/sdcard-clone.sh` on macOS has been tested against a scope's card; the others are common tools that users named.

**Watch out:**

- **The whole card, never a partition.** The card has no MBR and no GPT; its loader and the stock partition table sit outside anything a computer shows as a partition. A copy of partitions does not start.
- **Windows cannot read the card and offers to format it**, Disk Management may offer to initialise it: always **Cancel**. Each of these writes onto the card.
- **Check the device twice** before you write: an image written to the wrong disk destroys it.
- **Verify the backup**, keep the image in two places, and **keep the original card unchanged in a drawer**. It holds your factory calibration and licences. From then on you only use the second card (same size or larger; for a smaller one see `tools/sdcard-clone.sh fit` in the README).

**Windows**

- **Win32 Disk Imager**: the card's drive letter, a new file name ending in `.img`, **Read Only Allocated Partitions** unticked, **Read**. Then **Verify Only** with the same card and file: it must say "Verify Successful". For the clone: the second card's drive letter, the image, **Write**, then **Verify Only**. If the card gets no drive letter, use one of the next tools, which open the physical disk.
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

By hand: `diskutil unmountDisk /dev/diskN`, then `sudo dd if=/dev/rdiskN of=sd-original.img bs=4m status=progress` (`rdiskN`, the raw device, is much faster than `diskN`). Check: `sudo shasum -a 256 /dev/rdiskN` and `shasum -a 256 sd-original.img` must be equal.

**Linux**

```sh
lsblk -o NAME,SIZE,MODEL,TRAN                    # the card: /dev/sdX (or /dev/mmcblkN), not sdX1
sudo dd if=/dev/sdX of=sd-original.img bs=4M status=progress conv=fsync
sudo sha256sum /dev/sdX sd-original.img          # must be equal
sudo dd if=sd-original.img of=/dev/sdY bs=4M status=progress conv=fsync      # the clone, second card /dev/sdY
sudo cmp -n "$(stat -c %s sd-original.img)" sd-original.img /dev/sdY && echo same
```

`tools/sdcard-clone.sh` needs macOS, except `fit` (a copy of the image for a smaller card), which takes the card's size in bytes on Linux. More detail: [docs/sdcard-backup.md](https://github.com/dekajoule/oos/blob/main/docs/sdcard-backup.md).

## Over ADB

The README also describes an install over ADB, which needs a modified stock system and a build of your own.
