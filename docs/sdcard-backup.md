# Backing up and cloning the SD card

For the card route (`README.md`, On a second card, steps 1 and 2): an image of the scope's original boot card, and a
clone of it on the second card. Any tool that copies a whole disk sector by sector does the job. Tested here:
`tools/sdcard-clone.sh` on macOS. The Windows tools and `dd` on Linux are common ways that users named or that do the
same thing; they have not been run against a scope's card here.

## Before you start

- **Image the whole card, not a partition.** The card holds a loader, the partition table of the stock loader (no MBR,
  no GPT) and partitions that Windows and macOS cannot read. A copy of what a computer shows as partitions misses all
  that, and a scope does not start from it. Pick the whole device: `disk4` (macOS, not `disk4s1`), `/dev/sdb` or
  `/dev/mmcblk0` (Linux, not `sdb1` or `mmcblk0p1`), the physical disk in a Windows tool.
- **Windows cannot read the card and offers to format it** ("You need to format the disk in drive E: before you can
  use it"); Disk Management may offer to initialise it. Always choose **Cancel**. Never format, initialise, repair or
  "fix" the card: each of these writes onto it.
- **Check the device twice before you write.** Writing an image to the wrong disk destroys that disk.
- The image is as large as the card; the computer needs that much free space.
- **Verify the backup** (below), keep the image in two places, and **put the original card away unchanged**. Work only
  with the second card from then on. The original holds the factory calibration and licences of your scope.
- The second card should be the same size or larger. Cards of the same nominal size differ by a few MB; for a smaller
  card see `tools/sdcard-clone.sh fit` (README, step 2).

## Windows

**Win32 Disk Imager** (free):

1. Card in the reader (Cancel the offer to format it). Device: the card's drive letter. Image File: a new name ending
   in `.img`.
2. Leave **Read Only Allocated Partitions** unticked: the whole card has to be read.
3. **Read**.
4. Verify: with the original card still in the reader, the same image file and device, **Verify Only**. It has to end
   with "Verify Successful".
5. Clone: second card in the reader, its drive letter, the image, **Write**, then **Verify Only** again.

Win32 Disk Imager lists the card by drive letter. Windows usually gives a card in a reader one even when it cannot read
it; if none shows, take one of the tools below, which open the physical disk.

Other tools that copy a whole disk: **HDD Raw Copy Tool** (source: the card, target: FILE, as `.img`; for the clone,
source: the file, target: the second card), **WinHex** (create a disk image of the physical medium, not of a
partition; Tools > Disk Tools > Clone Disk for the clone) and **USBImager** (Read to make an image, Write with
Verify ticked for the clone). A checksum of the image, to compare later copies with: `certutil -hashfile
sd-original.img SHA256`.

## macOS

```sh
tools/sdcard-clone.sh list                                       # the card's disk number, e.g. disk4
tools/sdcard-clone.sh read disk4 ~/oos-backup/sd-original.img     # reads, verifies, writes sd-original.img.sha256
tools/sdcard-clone.sh write ~/oos-backup/sd-original.img disk5    # the second card; asks for the disk id, verifies
```

The tool refuses the system disk and any disk that is not a removable SD or USB medium. By hand:

```sh
diskutil list                                    # find the card by its size: /dev/diskN
diskutil unmountDisk /dev/diskN
sudo dd if=/dev/rdiskN of=sd-original.img bs=4m status=progress
sudo shasum -a 256 /dev/rdiskN; shasum -a 256 sd-original.img      # the two must be equal
```

Use `rdiskN` (the raw device), not `diskN`: many times faster.

## Linux

```sh
lsblk -o NAME,SIZE,MODEL,TRAN                    # the card: /dev/sdX, or /dev/mmcblkN in a built-in reader
sudo umount /dev/sdX?* 2>/dev/null
sudo dd if=/dev/sdX of=sd-original.img bs=4M status=progress conv=fsync
sudo sha256sum /dev/sdX sd-original.img          # the two must be equal
```

The clone, to the second card `/dev/sdY`, and its check over the length of the image:

```sh
sudo dd if=sd-original.img of=/dev/sdY bs=4M status=progress conv=fsync
sudo cmp -n "$(stat -c %s sd-original.img)" sd-original.img /dev/sdY && echo same
```

`tools/sdcard-clone.sh` needs macOS for `list`, `read`, `write` and `verify`; its `fit` (a copy of the image for a
smaller card) works on Linux with the card's size in bytes (`lsblk -bdno SIZE /dev/sdY`).
