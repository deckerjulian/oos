# Building

You only need a build of your own to change OOS or to try something that is not released yet. To install, the files of the [latest release](https://github.com/dekajoule/oos/releases/latest) are enough: see [[Installation]].

A build makes three files in `boot/out/`: `oos-dev.img` for a second card, `oos-dev.oosupd`, the signed update package, and `DHO800_DHO900_OOS_Install.GEL` for the USB stick.

## Which computer

| Host | How |
|---|---|
| macOS on Apple silicon | where OOS grew up |
| Linux on arm64 | the same, natively |
| Linux on x86-64, an ordinary PC | the kernel and `oos-hw` cross-compiled in the build container, the Rust programs through rustup's target, busybox, wpa_supplicant and e2fsck from arm64 containers through QEMU |

Windows: in a Linux virtual machine or WSL 2 with Docker, as on x86-64 Linux (untested).

Everything that needs a Linux tool chain runs in Docker. You need on the computer itself: Docker, [Rust from rustup](https://rustup.rs), Python 3, git and curl.

## On Linux, also on x86-64

Debian or Ubuntu; other distributions need the same things. Run everything as yourself, **not with `sudo`**: under sudo, rustup's cargo is not found and the results belong to root.

1. **Docker without sudo:**

   ```sh
   sudo apt install docker.io git python3 curl
   sudo usermod -aG docker $USER
   ```

   Log out and in again. `docker run --rm hello-world` has to work without sudo.

2. **Rust from rustup**, not from apt. The packages of the distribution lack the target and `rust-lld`.

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup target add aarch64-unknown-linux-musl
   ```

3. **x86-64 only: arm64 containers through QEMU.** Again after every reboot:

   ```sh
   docker run --privileged --rm tonistiigi/binfmt --install arm64
   docker run --rm --platform linux/arm64 alpine:3.22 uname -m      # has to print aarch64
   ```

   You install no cross compiler yourself: the build container brings Ubuntu's `gcc-aarch64-linux-gnu`.

4. **The source and the kernel archive.** The archive is too large for git; it is a file of each release, and `kernel/src/` is not in a clone:

   ```sh
   git clone https://github.com/dekajoule/oos.git && cd oos
   mkdir -p kernel/src
   curl -fL -o kernel/src/linux-6.12.115-base.tar.xz \
     https://github.com/dekajoule/oos/releases/latest/download/linux-6.12.115-base.tar.xz
   ```

5. **Build:**

   ```sh
   kernel/build-base.sh          # the kernel, only again when it changes
   boot/build-dev-image.sh       # the image, the update package and the USB file
   ```

   The first build takes longest: the build containers are made, and on x86-64 wpa_supplicant and e2fsck are compiled under QEMU. They are built once.

## On macOS

Docker Desktop, running, and Rust from rustup with `rustup target add aarch64-unknown-linux-musl`. Then steps 4 and 5 above. The kernel takes about 5 minutes on an M-series Mac.

## Signing

Update packages are signed. Without a key of your own the build uses the community key of the repository, which every OOS trusts; how to make your own: [README, step 4](https://github.com/dekajoule/oos#4-signing-key). **Only install packages from people you trust.**

## When it fails

| The script says | What to do |
|---|---|
| `do not run this with sudo` | step 1, then run it as yourself |
| `no cargo` or `missing Rust target` | step 2 |
| `no rustup` | Rust from the distribution: remove it, take rustup |
| `Docker is not running` / `permission denied` on the Docker socket | start Docker; on Linux, step 1 and log in again |
| `Docker cannot run arm64 containers`, or `exec format error` | step 3 (after a reboot again) |
| `missing kernel/src/linux-6.12.115-base.tar.xz` or `wrong checksum` | step 4; download it again |
| `run kernel/build-base.sh first` | the kernel comes before the image |

If a step fails, the previous image stays in `boot/out/`: check the time of the file. For anything else, [open an issue](https://github.com/dekajoule/oos/issues) with the host, the distribution and the last lines of the output.

## On GitHub

The workflow [Image](https://github.com/dekajoule/oos/actions/workflows/image.yml) builds everything on an arm64 and on an x86-64 Linux runner for every release, and compares what the two put into the image: kernel, device tree, every file of the initramfs. The files of a run can be downloaded from its page.
