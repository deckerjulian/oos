# Updates and going back

## Installing an update

Open `http://<scope>:8080/update` and choose a `.oosupd` file from the [releases](https://github.com/dekajoule/oos/releases), or:

```sh
curl --data-binary @oos-0.1.12.oosupd http://<scope>:8080/update
```

From 0.1.1 on, the scope first restarts into the new build by itself (kexec), and the new build installs itself from there; nothing is written before that. Then switch off at the mains, wait 30 seconds and switch on: that start is the trial. If the screen stays dark after the restart, do the same: the previous OOS build starts (stock in one-shot mode). Coming from 0.1.0, which installs directly: switch off at the mains, wait 30 seconds and switch on after the install.

**From 0.1.6 or older, install 0.1.7 first** (its file is in the [0.1.7 release](https://github.com/dekajoule/oos/releases/tag/v0.1.7)), then the latest. From 0.1.8 on the image brings another device tree (the GPU and the video encoder on). 0.1.7 and later start such an image by kexec with its own tree before they install it; 0.1.6 cannot and refuses it. The USB stick ([[Installation]]) writes any version directly.

The new build runs on trial. If it crashes in a loop, the previous image comes back. **System > Update** shows the state, **System > Roll back update** returns to the previous image.

Signatures protect against damaged files, not against an attacker: the community key is public. Details: [docs/update.md](https://github.com/dekajoule/oos/blob/main/docs/update.md).

## Going back to stock

- **One-shot mode:** switch off and on.
- **Persist mode:** **System > Boot mode > One-shot**, then switch off and on.
- **For good:** put your original card back.

OOS never writes the FPGA flash, `/rigol/data` or the `uboot`, `trust` and `parameter` partitions.
