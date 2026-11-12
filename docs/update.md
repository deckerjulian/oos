# Signed updates with rollback

Built and tested on the host (unit tests, a real image on a card-shaped file, the start-up watcher in an arm64 Linux
container: stable start, crash loop, install during the watch, hand-installed image). Run on the unit (DHO924S,
2026-11-07/08): installs from the web page and the shell, the kexec trial start, the trial start at the next power-on
and its confirmation. **Not yet verified on hardware: the automatic rollback of a bad update.**

The kexec trial start (below) ran on the unit (2026-11-08): from `install --execute` to the installed build about 20 s,
then the power-on as trial start, confirmed. Its first run had shown that the network links and the display must stop
first (step 3): the package arrived damaged and the display stayed dark. In the kexec start the kernel warns twice
while the PCIe driver loads (`its_build_mapti_cmd`, `its_build_inv_cmd`: the interrupt translation tables of the
previous kernel are still there); the display and the acquisition work, and the power-on after it has no warning.

Code: `hal/src/update/` (hashes, Ed25519, package, card, state, kexec), `fpgad/src/bin/oos-update.rs` (command line),
`ui/src/ui/update.rs` and `ui/src/http.rs` (GUI, web upload), `rootfs/initramfs/etc/rcS` (start count, watcher).

## What it protects against

| Failure | What happens |
|---|---|
| package damaged in transfer, or not signed by a trusted key (the community key is public, so this stops mistakes, not an attacker: see "Keys") | refused before anything is written |
| image the stock loader would refuse (wrong SHA-1 id, section length not a multiple of 4) | refused before anything is written |
| power lost while `recovery` is written | the boot magic is set to `KRNL` before the write and back to `OOSK` only after the read-back, so the next power-on starts the intact stock image in `boot`; the previous OOS image is still in the rollback slot. The stock copy in `backup` is not needed for this |
| new kernel or initramfs hangs **before** rcS (panics, cannot find the card) | the new image is first started by kexec, with `recovery` untouched (below): power-cycle, and the previous OOS build starts again (stock in one-shot mode). The next OOS start reports it. A new device tree is tried too, from 0.1.7 on (below). Not covered for `--new-dt`, packages without `trial kexec` (older builds) and `--no-kexec` |
| new build hangs in rcS after `oos-oneshot` and the start count (the first three commands of rcS) | `KRNL` is back, so the next power-on boots stock; when OOS is started again, the trial image's second start rolls back |
| new oos-ui crashes in a loop (3 restarts) or never runs 20 s stable within 180 s | the watcher writes the previous image back right away; the notice is shown on the panel; power-cycle starts the previous build |
| cold boot ends in `abort` or hangs while oos-ui runs stable (hardware trouble) | counts as a good start (no automatic rollback); the screen names the stage, System > Roll back update is available |

## Trial start by kexec

OOS has no loader of its own, so nothing counts the starts before rcS. An image written to `recovery` and selected with
`OOSK` that hangs in the kernel would start again at every power-on. So `install --execute` does not write it:

1. All checks of the install, nothing written. The package must say `trial kexec` (a signed header line that
   `oos-update pack` writes: its image knows how to finish such a start), and the image must bring the same device
   tree (`second`) as the one the scope started with. Every start saves that tree to `/tmp/oos-booted.dt`
   (`oos-update boot`, before anything writes `recovery`).
2. The state names the build (`kexec <build>`). The boot selection stays as it is: `recovery` still holds the running
   build.
3. Its kernel and initramfs are loaded with `kexec_file_load`, or, when the image brings another device tree, with
   `kexec_load` through kexec-tools (`/bin/kexec -c --dtb`, see "Another device tree" below). A second, uncompressed
   archive in the initramfs carries the package (`/oos-kexec.oosupd`), the key that verified it (`/oos-kexec.key`:
   the new build may trust other keys) and the device tree section the start runs with (`/oos-kexec.dt`); the command line gets `oos.kexec=install` and `initramfs_async=0`
   (the archive is unpacked before any driver probes). Under the update lock the start-up watcher and oos-ui are
   stopped. Then what writes to memory on its own stops: every network link goes down (the Ethernet controller has
   no shutdown hook, its receive DMA would write packets into memory of the next kernel) and the display subsystem is
   unbound (it lets go of the VOPs and their IOMMUs, which the next kernel cannot take over otherwise). `/oos` is
   remounted read only, then the kernel starts.
4. The new image's rcS: `oos-oneshot`, `/oos` mounted, then `oos-update boot` finds the word and the package and
   runs the ordinary install from there: rollback slot, `recovery`, read back, `OOSK`. This start is no trial start:
   its device tree and loader path are the old build's, and the display comes up from another state. The watcher
   judges nothing in it. The next power-on is the first trial start, confirmed or rolled back as for any trial. If
   the install fails after it began writing, the previous build is written back at once. A package that fails its
   check stays at `/oos-kexec.oosupd` (RAM) until the next start, for a look at what damaged it.

If the kernel hangs in step 4, the scope is power-cycled and the previous OOS build starts (stock in one-shot mode,
as after every OOS start). The next OOS start finds the mark in the state and says that the update did not finish; so
does a power cut during the install in step 4 (then stock starts, `KRNL` was set before the write). A failed load or
start (step 3) puts everything back and starts oos-ui again.

What it does not cover:

- A device tree change before 0.1.7: those builds have no kexec-tools and refuse an image with another tree for this
  path (`--new-dt` writes it directly, without a safety net before rcS). Install 0.1.7 first.
- A kernel that runs from kexec but hangs before rcS from a cold start (the kexec start runs on warm hardware) starts
  again at every power-on, as with a direct install.
- The install from 0.1.0 to a later build: 0.1.0 has no kexec start and writes the package directly.
- The command line of the first power-on comes from the device tree's `bootargs`, as the kexec start's did; a
  different command line in the image header is not used by this loader.
- A start whose device tree is unknown (`/tmp/oos-booted.dt` missing: started before a build with this, or
  recovery unreadable at start-up) takes the tree from `recovery` only while the state names the running build
  there; otherwise power-cycle once or use `--new-dt`.

### Another device tree

`kexec_file_load` gives the new kernel the running tree, so it cannot try an image with another one. From 0.1.7 the
image has kexec-tools (`rootfs/build-kexec.sh`, static), and `oos-update` loads such an image with `kexec_load`, which
takes a tree. Not the image's tree as it is: the stock loader adds to it at every power-on (reserved memory for the
ramdisk and the boot logo, `bootargs`, the display mode it set up for the logo, the logo's memory). The new tree gets
those changes: the difference between the tree in the image that started (`/tmp/oos-booted.dt`) and the tree the
kernel got (`/sys/firmware/fdt`), applied by node path (`hal/src/update/fdt.rs`). So the kexec start runs with the
tree a power-on of the new image would give it. The loader writes plain values; a change in a `phandle` (dtc numbers
them anew in every tree) or to a node the new tree does not have refuses the kexec start, and the install then needs
`--new-dt`. Left out on purpose: the running kernel's ramdisk (`linux,initrd-*` in `/chosen` and its reservation):
kexec-tools names the new one, and the kernel drops a ramdisk that lies in reserved memory (seen on the unit: the
55 MB of the kexec start fell into the 13 MB the loader had reserved for its own, and the kernel found no root; the
power-cycle brought the running build back, as it should). On the unit 33 changes of the loader carry over.

Where the segments go: above 1 GiB (`kexec --mem-min`), and above every GIC table the running kernel's log names. The
GICv3 keeps its LPI and ITS tables across kexec (it cannot be given new ones); the started kernel takes them over and
writes to them, and `/proc/iomem` does not show them. On the unit they lie at 0x2400000 and 0xa320000..0xa400000,
where kexec-tools put the 55 MB initramfs at first: the new kernel found it damaged ("rootfs image is not initramfs
(uncompression error)", read from pstore after a start with `panic=5`), with every other bus master stopped as well.

`oos-update kexec-check FILE` loads a package's kernel and initramfs and unloads them again, nothing written, no
restart: it shows that the running kernel takes the image for a kexec start.

## Card layout used

| LBA | size | content | written by the update |
|---:|---:|---|---|
| 122880 | 1 sector | `boot` magic `KRNL` / `OOSK` | yes (to `KRNL` before `recovery` is written, to `OOSK` after) |
| 188416 | 64 MiB | `recovery`: running OOS image | yes |
| 319488 | 32 MiB | `backup` start: stock boot copy (the loader's last fallback) | never |
| 385024 | 64 MiB | `backup` + 32 MiB: **rollback slot**, the previous OOS image | yes |
| 516096 | 16 MiB | `backup` end: /oos data partition | never (state file through the file system) |

The slot is all zero on an original card (the whole `backup` is, see `boot.md`). Before the first write the tool
checks that the slot is either empty or holds an Android boot image; anything else stops the install. Every image
write is padded to whole sectors, synced, the cached pages dropped, and the region read back from the card.

The state is written before the `recovery` write, after a write test of the state file before any card write. The slot
entry is cleared while the slot is written. A rollback changes the state only after its write, and "already holds" reads
the card, so an interrupted rollback does not look finished.

## Package `.oosupd`

```
OOS-UPDATE 1
target dho900-recovery
build 20260101-120000 kernel 6.12.115+
kernel 6.12.115+
size 31141888
sha256 <image>
key <key id, 16 hex digits>
sig <Ed25519 over every byte above this line>

<image>
```

`head -9 file.oosupd` shows it. The signature covers the header, the header names the image's SHA-256. The `build`
starts with the UTC build time; an older build than the installed one is refused unless `--downgrade`.

## Keys

- Community key: `keys/community-signing.key` is in the repository **on purpose**, and every build trusts it, so that
  anyone can build an update that installs on any OOS scope. `boot/build-dev-image.sh` uses it when there is no own key.
  It proves nothing about who built a package.
- Own keys: `oos-update keygen` creates one, mode 0600, outside the repository. Back it up (password manager or an
  offline copy) and never publish it. Whoever wants only their own builds on a scope adds the public line of their key
  to `keys/update.pub`, removes the community line and the other lines they do not want, and installs that build. From
  then on that scope refuses community packages. If the own key is lost, a scope that trusts only this key needs an
  image installed by hand (`card-install.py` on a host, or `dd` over telnet).
- Public keys: `keys/update.pub`, compiled into every build (`oos_hal::update::TRUSTED_KEYS`). A second key (e.g. a CI
  key) is added as another line; a key is retired by an update that no longer lists it.
- Ed25519 and SHA-2 are implemented in the tree (no crates beyond libc): RFC 8032 test vectors, FIPS examples, and a
  cross-check against Python `cryptography` in both directions (Rust signs / Python verifies and the reverse).
- The signature guards the install path only. Telnet, SCPI and the web remote stay unauthenticated (bench LAN): anyone
  on the LAN can still write the card by hand. With the community key trusted, anyone on the LAN can also sign and
  install a package. What the signature buys then: a damaged package, or one not made for OOS, is never installed by
  the web page or the GUI. Keeping foreign packages out takes a build without the community key.

## Use

Build (host): `boot/build-dev-image.sh` writes `boot/out/oos-dev.img` and, when the key exists,
`boot/out/oos-dev.oosupd`.

Install on a running OOS:

- browser: `http://<scope>:8080/update`, choose the file, Install; or
  `curl --data-binary @boot/out/oos-dev.oosupd http://<scope>:8080/update`
- shell: upload the file to /tmp, `oos-update install /tmp/oos-dev.oosupd` (dry run), then `--execute`

The scope restarts into the new build by kexec, which installs itself (about a minute); System > Update shows the
result. Then power-cycle: that start is its trial. If the screen stays dark after the restart, power-cycle too: the
previous build starts if the install did not finish. A package without `trial kexec`, `--no-kexec` and `--new-dt`
write the image directly: then power-cycle to start it on trial.

| command | |
|---|---|
| `oos-update status` | boot magic, recovery and slot (build, SHA-256, loader id), trial, mode, last event |
| `oos-update install FILE [--execute] [--downgrade] [--force] [--no-kexec \| --new-dt]` | see above |
| `oos-update kexec-check FILE` | load and unload the package's kernel for a kexec start; nothing written |
| `oos-update rollback [--execute]` | slot -> recovery (checked against its SHA-256 first), arm OOS |
| `oos-update confirm` | end a trial by hand (after stopping oos-ui for development, `/tmp/oos-ui.off`) |
| `oos-update mode persist` / `oneshot` | persist: OOS arms itself again after each confirmed start, and is selected for the next power-on at once; oneshot: stock is selected at once. The GUI has this as System > Boot mode |
| `oos-update verify FILE` | signature and image check, anywhere (`--keys` only here and for card-file tests) |
| `oos-update pack ...`, `keygen` | host side |

Logs: `/tmp/oos-update.log` (start count, watcher), `/tmp/oos-update.job` (GUI / web job), state `/oos/update/state`.

## Trial rules (`oos_hal::update::judge`)

- good: oos-ui has run 20 s without a restart and the start-up wrote `end` or `abort` (no acquisition hardware in the
  image: oos-ui alone); at 180 s also with a start-up still running, if oos-ui runs stable.
- bad: oos-ui restarted 3 times, or no oos-ui that ran 20 s stable by 180 s. The oos-ui process is followed by pid, so a
  forked child does not count as a restart.
- The trial is about the running image: the watcher only judges when `/etc/oos-build` equals the build on trial
  (`pack --build` must get the image's own build id, as build-dev-image.sh does), and applies its verdict only if the
  state still names the same image (an install from the web page during the watch stays on trial).
- Powering off before "confirmed" (about 20 s after the start-up) counts as an unconfirmed start: the next OOS start
  rolls back. System > Update shows "on trial" until then.
- `/tmp/oos-ui.off` (oos-ui stopped by hand): no verdict, the trial stays open.
- rcS `oos-update boot`: a trial image that starts a second time without confirmation is rolled back at that start, with
  a notice on the panel first; a running image that is not the one on trial (recovery written by hand) drops the trial
  instead.
- All state changes take a lock (`/oos/update/state.lock`), so rcS, the watcher, the GUI job and a shell call never
  interleave.
- Power off, restart and a rollback tap are refused while an update job runs. The panel's own power key cannot be refused.

Install keeps the image in `recovery` as the rollback image only when it is confirmed or was put there by hand
(`untracked-<sha>`); over an image still on trial the slot keeps the last good one. `--force` with the image already in
`recovery` does not overwrite the rollback image.

## Boot mode

Default stays **one-shot**: every OOS start puts `KRNL` back. `persist` makes OOS the normal system: after every
confirmed start the watcher sets `OOSK` again, checked and under the lock. An unconfirmed start leaves `KRNL`, so the
next power-on boots stock: a boot counter with one try, without an own U-Boot.

## Not covered

- Rolling forward after a rollback: the failed image is not kept; install the package again.
- Updating the stock loader, `trust`, the FPGA flash or the partition table: never part of an update.
- A power cut during the slot write before `recovery` is written leaves the old image in `recovery`: harmless, run
  the install again.
- The downgrade check only knows the state's `current`: with no state, an untracked image or after a rollback, any signed
  build can be installed from the LAN without authentication. It still runs on trial.
- **`--new-dt` has no safety net**, and neither has a direct install (`--no-kexec`, packages without `trial kexec`, the
  USB stick): a kernel that panics, hangs or cannot reach the SD card before rcS starts again with OOS selected on
  every power-on, and the card has to come out. Test such a change on a card you can take out first.
- The watcher judges a start by oos-ui staying up and the start-up checklist finishing. A build that runs but measures
  wrongly is confirmed and becomes the rollback image at the next install.
- A full `/oos` stops the trial bookkeeping: an update then neither confirms nor rolls back. The next power-on in one-shot
  mode starts stock.

Selecting OOS by hand (`oos-boot-oos-again`, `oos-update arm`) checks that `recovery` holds an image the loader takes and
that no install or rollback runs; after a cut job the next start stays stock. `oos-update confirm` ends a trial only when
the image on trial is the one running. Images whose load addresses or sizes are outside what the loader handles
(docs/boot.md) are refused at install.

## Web upload limits (ui/src/http.rs)

At most 8 connections; bodies up to 70 MiB only for `POST /update` (1 MiB elsewhere), an announced larger
`Content-Length` gets 413 at once; 10 s idle and 120 s per request. The page runs `oos-update status --quick`
(headers only, no hashing of 2 x 31 MB).

## To check on the unit

1. Install `boot/out/oos-dev.img` once by hand (it brings `oos-update`), power-cycle.
2. `oos-update status`: layout ok, recovery untracked, slot empty, trusted key listed.
3. Build again, install the `.oosupd` from the web page, power-cycle: trial, then "confirmed" in
   `/tmp/oos-update.log` after ~20 s past the start-up.
4. Rollback test: `oos-update rollback --execute`, power-cycle, check the build in About.
5. Time of `install` (two 31 MB writes plus read-back on the SD card) and of the upload over 100 Mbit.
