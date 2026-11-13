# Downloaded sources of the image

Every source tarball the image builds from is listed once, in `rootfs/deps.lock`: name, owner, version, SHA-256, URL
(and, where the URL's directory has no listing, a page that lists the releases).

- The fork's own builds read it: `rootfs/build-dropbear.sh` (Dropbear), `rootfs/build-sftp-server.sh` (OpenSSH,
  `sftp-server` only), `rootfs/build-nft.sh` (libmnl, libnftnl, nftables). They source `rootfs/deps.sh`, which
  downloads once into `rootfs/out/`, refuses a file whose SHA-256 differs, and stamps each binary with the manifest
  lines and the build script it came from (`<binary>.stamp`). `boot/unit-hook.sh` builds a binary again when it is
  missing or its stamp no longer matches, so a version bump or a changed build option takes effect at the next image
  build without deleting anything by hand.
- Upstream's builds (`rootfs/build-kexec.sh`, `rootfs/build-fsck.sh`, `rootfs/build-wlan-tools.sh`, `gpu/build.sh`)
  keep their own `ver=` and `sum=`: changing them would conflict at every upstream sync. Their lines in the manifest
  (`upstream:<script>`) document them, and `tools/deps-check.sh` reports when a script and its line differ.
- Not in the manifest: the Docker base images (`alpine:3.22`, `ubuntu:26.04`, `rust:1-alpine`), busybox (Ubuntu's
  `busybox-static`, its version recorded in `busybox.version` of each build), the kernel base archive
  (`kernel/src/SHA256SUMS`) and Rust crates (`Cargo.lock`).

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
