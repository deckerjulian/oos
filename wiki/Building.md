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

4. **The source and the kernel archive.** The archive is too large for git; it is a file of each release
   ([kernel/README.md](https://github.com/dekajoule/oos/blob/main/kernel/README.md)), and `kernel/src/` is not in a
   clone:

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

   The kernel builds in Docker in 5 to 30 minutes, depending on the machine, into `kernel/out/base`; you only build it
   again when the kernel changes. The scripts say what is missing before they start: Docker not reachable, no rustup
   or no target, no arm64 containers. The first build takes longest: the build containers are made, and on x86-64
   wpa_supplicant and e2fsck are compiled under QEMU. They are built once. A finished build leaves the same files as on
   macOS. To put the image on a second card from Linux, follow [[Installation]], with `/dev/sdX` for the card.

## On macOS

Docker Desktop, running, and Rust from rustup with `rustup target add aarch64-unknown-linux-musl`. Then steps 4 and 5 above. The kernel takes about 5 minutes on an M-series Mac.

## Signing

Update packages must be signed. To let everyone build updates for every OOS scope, the repository includes a **public
community key**, `keys/community-signing.key`, and all builds trust it. You do not have to do anything: the build uses
it automatically.

The community key only protects against damaged packages. It says nothing about who built one, so **only install
packages from people you trust.**

If you want a scope to accept only your own builds, create your own key before you build:

```sh
cd fpgad && cargo run --release --bin oos-update -- keygen && cd ..
```

Replace all keys in `keys/update.pub` with the line it prints. The secret key is in
`~/.config/oos/update-signing.key`. **Back it up and never publish it.** Details:
[docs/update.md](https://github.com/dekajoule/oos/blob/main/docs/update.md).

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

## Try it without a scope

On Linux, on macOS, or on Windows inside WSL 2, with Rust installed, start OOS without a display and without hardware
and open `http://localhost:8080/`. You need no Docker, no kernel and no image for this:

```sh
cd ui && cargo run --release -- --serve
```

The web remote shows the screen with demo signals: a click is a touch, a drag is a swipe, and the page has the keys
and knobs of the front panel. The SCPI socket is on port 5555 and HiSLIP on 4880. `--offline` starts with the analog
channels off for stored waveforms; `--web-port`, `--scpi-port` and `--hislip-port` choose other ports, and
`--store-dir` the folder for setups, CSV and screenshots. There is no native Windows build
([ADR 0033](https://github.com/dekajoule/oos/blob/main/docs/adr/process/0033-development-on-linux-macos-and-wsl.md)).

The web remote accepts CSV, OOS BIN and WFM files by file selection and drag-and-drop. Loaded sources appear as
**R1–R10** cards. **SIM** (start menu, Analysis) starts a simulated reference; **Run/Stop**, **Single**, and trigger
settings control its acquisitions. **Live** restores the live session. Measurements follow each captured window and
recompute when a stopped reference is panned. **Measure** opens categorized choices with Source, Settings, and
Selected; large readout lists have page controls. A host build announces its SCPI and HiSLIP listeners over mDNS.

**SIM > Advanced** adds SSC profile, deviation and modulation frequency, duty-cycle distortion, and correlated random
jitter with its corner frequency. PRBS-13 is available alongside the other patterns. With SSC enabled, the stored
record covers a complete modulation cycle at eight samples per bit. Long records generate in the background; cancel
or return to Live to keep the previous session. Configurations exceeding the generation budget report an error
without replacing the current waveform. The HSS down-spread profile removes its DC term; centered spread uses
plus/minus the configured deviation.

Choose **Auto Eye/Jitter** in the analysis menu/settings to analyze the selected live or reference source. Automatic
setup recovers an NRZ clock/rate, chooses voltage thresholds from the signal and opens the eye with its TIE trend,
histogram, spectrum and bathtub. The source/rate header keeps manual rate and explicit-clock settings accessible. Flat,
insufficient or sparsely sampled data shows its limit; model estimates are labeled. The Eye analyzes a full stored
reference once per changed record or setting, including while the trace is zoomed; large files show Computing without
blocking navigation. Live captures accumulate; stopped panning does not add observations. Ordinary trace measurements
use the selected Screen or Full record gate.

The same advanced settings are available through OOS SCPI. Values use seconds, hertz and fractional deviation (0.005
means 0.5%). Configure them together, then start generation and poll `:SIM:BUSY?`:

```text
:SIM:PRESET "PCIe 3.0"
:SIM:SSC:MODE CENTER
:SIM:SSC:DEVIATION 0.005
:SIM:SSC:FREQUENCY 33000
:SIM:JITTER:DCD 4e-12
:SIM:JITTER:CORRELATED 2e-12
:SIM:JITTER:CORNER 2e6
:SIM:PATTERN PRBS13
:SIM:PLAY ON
:SIM:BUSY?
:SYSTEM:ERROR?
```

OOS can also run scripted without hardware, with its simulated demo signals. It writes a screenshot in PPM format next
to the script:

```sh
cat > /tmp/demo.txt <<'EOF'
frames 30
shot demo.ppm
EOF
cd ui && cargo run -- --script /tmp/demo.txt
```

The commands are listed at the top of [ui/src/cmd.rs](https://github.com/dekajoule/oos/blob/main/ui/src/cmd.rs). Add
`--model DHO804` to act as another model.

To try the web remote with the same demo signals for 5 minutes, run this and open `http://localhost:8080/`:

```sh
cd ui && cargo test web_demo -- --ignored --nocapture
```

## On GitHub

The workflow [Image](https://github.com/dekajoule/oos/actions/workflows/image.yml) builds everything on an arm64 and on an x86-64 Linux runner for every release, and compares what the two put into the image: kernel, device tree, every file of the initramfs. The files of a run can be downloaded from its page.
