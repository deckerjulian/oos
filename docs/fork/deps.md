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
- Not in the manifest: the Rust tool chain (`rust-toolchain.toml`) and the Docker base images (`alpine:3.22` and
  `ubuntu:26.04`, named once in `tools/lib/docker.sh` with their digests), both pinned (below), busybox (Ubuntu's
  `busybox-static`, its version and the glibc it was built with recorded in `busybox.version` of each build), the
  kernel base archive (`kernel/base.sha256`) and Rust crates (`Cargo.lock`).

## Rust crates

The four crates depend on `libc` and on each other only; `tools/check.sh target` fails otherwise
([ADR 0021](../adr/process/0021-libc-only-on-the-scope-platform-crates-on-the-desktop.md)). There is no build with
platform crates any more: the Windows desktop build, which used `minifb` and `windows-sys`, is gone
([ADR 0033](../adr/process/0033-development-on-linux-macos-and-wsl.md)).

## The Rust tool chain and the base images

Both are pinned, so a build takes the same compiler and the same base images until a pull request changes them
([ADR 0045](../adr/process/0045-the-toolchain-and-the-base-images-are-pinned.md)).

- **Rust:** `rust-toolchain.toml` at the root of the tree names the release (`channel`), the components (rustfmt,
  Clippy) and the target of the scope. rustup reads it from the folder of a crate upwards: `cargo` anywhere in the
  tree runs that release, and `rustup toolchain install` in the tree installs it (CI does that; on a computer rustup
  also fetches it when cargo first runs in the tree). Nothing installs Rust inside a container: the Rust programs
  are built on the host.

  To bump: set `channel` to the new release, run `tools/check.sh`, fix what it finds (a new Clippy lint, a changed
  layout of rustfmt), all in one pull request. The weekly run tries the newest stable release against the tree (job
  `canary`) and opens an issue when it fails.
- **Base images:** `OOS_ALPINE` and `OOS_UBUNTU` in `tools/lib/docker.sh` are `name:tag@sha256:<digest>`. Docker takes
  the image by its digest; the tag only says which release that is. The digest is the one of the index for all
  platforms, so the same name serves an arm64 and an x86-64 host. It pins the base image, not what a build installs
  on top: `apt-get install` and `apk add` take the packages of that release as they are on the day of the build.

  To bump: `docker buildx imagetools inspect alpine:3.22` (and `ubuntu:26.04`) prints the `Digest:` the tag points
  to now. Write it behind the tag, then build the image and run the sims (`boot/build-dev-image.sh`,
  `tools/check.sh sim`); the build images are made again by themselves, their tag is a hash of their Dockerfile. A
  new release of the distribution is the same edit with another tag. Nothing looks for a newer digest by itself.

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
