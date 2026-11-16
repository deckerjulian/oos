# Downloaded sources of the image

Every source tarball the image builds from is listed once, in `rootfs/deps.lock`: name, owner, version, SHA-256, URL
(and, where the URL's directory has no listing, a page that lists the releases).

- The fork's own builds read it: `rootfs/build-dropbear.sh` (Dropbear), `rootfs/build-sftp-server.sh` (OpenSSH,
  `sftp-server` only), `rootfs/build-nft.sh` (libmnl, libnftnl, nftables). They source `rootfs/deps.sh`, which
  downloads once into `rootfs/out/`, refuses a file whose SHA-256 differs, and stamps each binary with the manifest
  lines and the build script it came from (`<binary>.stamp`). `boot/unit-hook.sh` builds a binary again when it is
  missing or its stamp no longer matches, so a version bump or a changed build option takes effect at the next image
  build without deleting anything by hand.
- `gpu/build.sh` reads the Mesa line the same way, and keeps Mesa's build in `rootfs/out/mesa` stamped with that line.
- Upstream's builds (`rootfs/build-kexec.sh`, `rootfs/build-fsck.sh`, `rootfs/build-wlan-tools.sh`)
  keep their own `ver=` and `sum=`: changing them would conflict at every upstream sync. Their lines in the manifest
  (`upstream:<script>`) document them, and `tools/deps-check.sh` reports when a script and its line differ.
- `libnl` and `libnl-musl-patch` (owner `alpine:3.22`) are not built: `wpa_supplicant` links the `libnl3-static` of the
  Alpine image. The two lines are the files Alpine builds that package from. `rootfs/build-wlan-tools.sh` fetches them
  into `rootfs/out/` for the release and stops when Alpine installs another version.
- What a release has to carry next to its image is listed by `tools/release-sources.sh list`, from this manifest and
  `kernel/base.sha256`; `tools/release-sources.sh check <tag>` compares a release on GitHub with it
  ([THIRD-PARTY-LICENSES.md](../../THIRD-PARTY-LICENSES.md)).
- Not in the manifest: the Docker base images (`alpine:3.22`, `ubuntu:26.04`, `rust:1-alpine`), busybox (Ubuntu's
  `busybox-static`, its version and the glibc it was built with recorded in `busybox.version` of each build), the
  kernel base archive
  (`kernel/base.sha256`) and Rust crates (`Cargo.lock`).

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
candidates); exit status 1 when one is newer or an upstream script differs from its line. It only reads public
listing pages.

## Updating one

1. Read the release notes of the new version (security fixes, changed build options).
2. Set the version and the URL in its `rootfs/deps.lock` line.
3. Get the SHA-256 from a second source, not only from the downloaded file: OpenSSH's release notes (base64
   SHA256), Alpine's aports or Debian's `.dsc` for the same tarball, or a signature check. Write it into the line.
4. Build: `rootfs/build-<name>.sh` (or the next image build, which sees the stale stamp). A wrong checksum stops the
   build and deletes the download.
5. Run the sims that cover it: `tools/ssh-sim.sh` (Dropbear, sftp-server), `tools/fw-sim.sh` and `tools/ssh-sim.sh`
   (nft), then an image build and the unit checks of the feature (docs/network.md 6, 7).
6. Commit the manifest line (and a build script change, if the new version needs one) with the sources' release in
   the message.

For an `upstream:` line: change it only to follow upstream's script after a sync, or bump the script in an upstream
contribution.

## Pending updates (2026-11-13)

`tools/deps-check.sh` found newer releases that were not taken in this round: libnftnl 1.2.9 -> 1.3.2, nftables
1.1.3 -> 1.1.7 (fork, `rootfs/build-nft.sh`); e2fsprogs 1.47.2 -> 1.47.4, wpa_supplicant 2.11 -> 2.12, Mesa 26.0.8 ->
26.3.0 (upstream's scripts: follow upstream). Dropbear, OpenSSH, libmnl and kexec-tools are current.
