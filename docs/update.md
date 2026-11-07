# Signed updates with rollback

Built and tested on the host (unit tests, a real image on a card-shaped file, the start-up watcher in an arm64 Linux
container: stable start, crash loop, install during the watch, hand-installed image). **Not yet verified on hardware.**

Code: `hal/src/update/` (hashes, Ed25519, package, card, state), `fpgad/src/bin/oos-update.rs` (command line),
`ui/src/ui/update.rs` and `ui/src/http.rs` (GUI, web upload), `rootfs/initramfs/etc/rcS` (start count, watcher).

## What it protects against

| Failure | What happens |
|---|---|
| package damaged in transfer, or not signed by a trusted key (the community key is public, so this stops mistakes, not an attacker: see "Keys") | refused before anything is written |
| image the stock loader would refuse (wrong SHA-1 id, section length not a multiple of 4) | refused before anything is written |
| power lost while `recovery` is written | the boot magic is set to `KRNL` before the write and back to `OOSK` only after the read-back, so the next power-on starts the intact stock image in `boot`; the previous OOS image is still in the rollback slot. The stock copy in `backup` is not needed for this |
| new build hangs in rcS after `oos-oneshot` and the start count (the first three commands of rcS) | `KRNL` is back, so the next power-on boots stock; when OOS is started again, the trial image's second start rolls back |
| new kernel hangs **before** rcS | **not covered**: `OOSK` stays set and every power-on starts the new image again; put the card into a host computer (`card-install.py restore` or `switch.py`), which means opening the scope. Needs a loader-side counter (own U-Boot) |
| new oos-ui crashes in a loop (3 restarts) or never runs 20 s stable within 180 s | the watcher writes the previous image back right away; the notice is shown on the panel; power-cycle starts the previous build |
| cold boot ends in `abort` or hangs while oos-ui runs stable (hardware trouble) | counts as a good start (no automatic rollback); the screen names the stage, System > Roll back update is available |

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

Then power-cycle. The new build runs on trial; System > Update shows the state.

| command | |
|---|---|
| `oos-update status` | boot magic, recovery and slot (build, SHA-256, loader id), trial, mode, last event |
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
- **A new kernel or device tree has no safety net.** The trial and the watcher run in the initramfs: a kernel that panics,
  hangs or cannot reach the SD card before it starts again with OOS selected on every power-on, and the card has to come
  out. Test a kernel change on a card you can take out before it goes into a package.
- Running the USB install again to start OOS replaces what `recovery` holds with the stick's build, also a newer one
  installed over the network since, and the rollback slot with it.
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
