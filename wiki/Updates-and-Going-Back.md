# Updates and going back

## Installing an update

Take `oos-<version>.oosupd` from the [latest release](https://github.com/dekajoule/oos/releases/latest), or build a
package yourself (`boot/out/oos-dev.oosupd`, [[Building]]). Open `http://<scope>:8080/update` and choose the file, or:

```sh
curl --data-binary @oos-<version>.oosupd http://<scope>:8080/update
```

From 0.1.1 on, the scope first restarts into the new build by itself (kexec), and the new build installs itself from there; nothing is written before that. Then switch off at the mains, wait 30 seconds and switch on: that start is the trial. If the screen stays dark or shows a kernel panic after the restart, do the same: the previous OOS build starts (stock in one-shot mode). Coming from 0.1.0, which installs directly: switch off at the mains, wait 30 seconds and switch on after the install.

**From 0.1.6 or older, install 0.1.7 first** (its file is in the [0.1.7 release](https://github.com/dekajoule/oos/releases/tag/v0.1.7)), then the latest. From 0.1.8 on the image brings another device tree (the GPU and the video encoder on). 0.1.7 and later start such an image by kexec with its own tree before they install it; 0.1.6 cannot and refuses it. The USB stick ([[Installation]]) writes any version directly.

**From 0.1.13 or older to 0.1.14 or later, not over the web page:** the package (47 MB) is larger than the restart into
a new build of 0.1.13 and older can carry, and the web page's install ends in a kernel panic (nothing is written;
switch off at the mains and on again, and the old build starts). Install it from a shell on the scope (SSH, or telnet
in 0.1.13) with `--no-kexec`, which writes the image directly:

```sh
oos-update install /mnt/usb/oos-0.1.14.oosupd                        # dry run: checks the package
oos-update install /mnt/usb/oos-0.1.14.oosupd --no-kexec --execute   # writes it
```

The package can stay on a USB stick in the front port, which OOS mounts at `/mnt/usb`; copying it to `/tmp` first
needs SCP or SFTP on the computer. Then switch off at the mains, wait 30 seconds and switch on: that start is the
trial of what runs after the start-up scripts. Unlike the web page's install, this direct install has no safety net
before them: a new build whose kernel panics or hangs that early starts again on every power-on, and the card has to
come out ([[Installation]], "On a second card"). This way has not been tried from 0.1.13 on a unit yet; it is the
path 0.1.13 already has for packages without a kexec trial. The other way is the USB stick install of the stock
system ([[Installation]]). From 0.1.14 on, updates over the web page carry packages of this size again.

The new build runs on trial. If it crashes in a loop or never becomes stable, the previous image is written back.
**System > Update** shows the state, **System > Roll back update** (or `oos-update rollback --execute` in a shell)
returns to the previous image.

Signatures protect against damaged files, not against an attacker: the community key is public. Details: [docs/update.md](https://github.com/dekajoule/oos/blob/main/docs/update.md).

## Going back to stock

- **One-shot mode:** switch off and on. **System > Back to stock** says the same.
- **Persist mode:** **System > Boot mode > One-shot**, then switch off and on. Or, with the card in a reader:
  `sudo python3 boot/switch.py stock /dev/rdiskM --execute`
- **For good:** put your original card back. After an install without opening the scope there is no original card in a
  drawer: OOS stays in `recovery` and does nothing there. Putting the stock `recovery` back needs the card in a reader
  and `OOS-rescue/recovery.bin` from the stick, a dry run first, then the same with `--execute`:
  `sudo python3 boot/card-install.py restore /dev/rdiskM --backup OOS-rescue/recovery.bin`

OOS never writes the FPGA flash, `/rigol/data` or the `uboot`, `trust` and `parameter` partitions.
