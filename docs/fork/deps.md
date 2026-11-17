# Downloaded sources of the image

Every source archive OOS builds from is listed once, in `rootfs/deps.lock`: name, version, SHA-256, URL (and, where
the URL's directory has no listing, a page that lists the releases). No build script carries a version or a checksum
of its own.

- Every build script that downloads a source reads its lines through `rootfs/deps.sh`: `rootfs/build-dropbear.sh`
  (Dropbear), `rootfs/build-sftp-server.sh` (OpenSSH, `sftp-server` only), `rootfs/build-nft.sh` (libmnl, libnftnl,
  nftables), `rootfs/build-kexec.sh` (kexec-tools), `rootfs/build-fsck.sh` (e2fsprogs), `rootfs/build-wlan-tools.sh`
  (wpa_supplicant, libnl), `gpu/build.sh` (Mesa). `dep_fetch` downloads once into `rootfs/out/` and refuses a file
  whose SHA-256 differs; `dep_stamp` records what a binary was built from (`<binary>.stamp`: the deps.lock lines and
  the content of the build script and of the tree's sources it compiles).
- `boot/build-dev-image.sh` and `boot/unit-hook.sh` build a binary again when it is missing or its stamp no longer
  matches (`dep_stale`), so a version bump or a changed build option takes effect at the next image build without
  deleting anything by hand. Mesa's own build is kept in `rootfs/out/mesa`, stamped with its line; a change of its
  build options in `gpu/build.sh` needs that directory removed.
- `hostapd` is not in the image: `tools/wlan-sim.sh` builds it for the access points of its test, from the same file.
- `libnl` and `libnl-musl-patch` are not built: `wpa_supplicant` links the `libnl3-static` of the
  Alpine image. The two lines are the files Alpine builds that package from. `rootfs/build-wlan-tools.sh` fetches them
  into `rootfs/out/` for the release and stops when Alpine installs another version.
- What a release has to carry next to its image is listed by `tools/release-sources.sh list`, from this manifest and
  `kernel/base.sha256`; `tools/release-sources.sh check <tag>` compares a release on GitHub with it
  ([THIRD-PARTY-LICENSES.md](../../THIRD-PARTY-LICENSES.md)).
- Not in the manifest: the Docker base images (`alpine:3.22` and `ubuntu:26.04`, named once in `tools/lib/docker.sh`),
  busybox (Ubuntu's `busybox-static`, its version and the glibc it was built with recorded in `busybox.version` of
  each build), the kernel base archive (`kernel/base.sha256`) and Rust crates (`Cargo.lock`).

## Rust crates

What runs on the scope, `oos-hal` and `oos-dsp` depend on `libc` and the crates of the tree only; `tools/check.sh
target` fails otherwise ([ADR 0021](../adr/process/0021-libc-only-on-the-scope-platform-crates-on-the-desktop.md)).
A build that only runs on a computer may use its platform's crates; each is listed here:

| Crate | Build | For | Versions |
|---|---|---|---|
| `minifb` | Windows desktop (`cfg(windows)`) | the window and its keyboard and mouse input (`ui/src/desktop.rs`) | `ui/Cargo.toml`, `ui/Cargo.lock` |
| `windows-sys` | Windows desktop (`cfg(windows)`) | network interfaces, sockets, file dialogs (`ui/src/announce/windows.rs`, `ui/src/desktop/`) | `ui/Cargo.toml`, `ui/Cargo.lock` |

## Checking for updates

`tools/deps-check.sh` reads each line's release listing and prints the pinned and the newest release (no release
candidates); exit status 1 when one is newer. It only reads public listing pages.

## Updating one

1. Read the release notes of the new version (security fixes, changed build options).
2. Set the version and the URL in its `rootfs/deps.lock` line.
3. Get the SHA-256 from a second source, not only from the downloaded file: OpenSSH's release notes (base64
   SHA256), Alpine's aports or Debian's `.dsc` for the same tarball, or a signature check. Write it into the line.
4. Build: `rootfs/build-<name>.sh` or `gpu/build.sh` (or the next image build, which sees the stale stamp). A wrong
   checksum stops the build and deletes the download.
5. Run the sims that cover it: `tools/ssh-sim.sh` (Dropbear, sftp-server), `tools/fw-sim.sh` and `tools/ssh-sim.sh`
   (nft), `tools/wlan-sim.sh` (wpa_supplicant), `gpu/test-host.sh` (Mesa), then an image build and the unit checks of
   the feature (docs/network.md 6, 7).
6. Commit the manifest line (and a build script change, if the new version needs one) with the sources' release in
   the message.

## Pending updates (2026-11-13)

`tools/deps-check.sh` found newer releases that were not taken in this round: libnftnl 1.2.9 -> 1.3.2, nftables
1.1.3 -> 1.1.7, e2fsprogs 1.47.2 -> 1.47.4, wpa_supplicant 2.11 -> 2.12, Mesa 26.0.8 -> 26.3.0. Dropbear,
OpenSSH, libmnl and kexec-tools were current.
