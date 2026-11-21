# Third-party software in OOS

OOS itself is free software under the GNU General Public License, version 2 or (at your option) any later version
([LICENSE](LICENSE)). An OOS image (`oos-<version>.img`, `oos-<version>.oosupd`, `DHO800_DHO900_OOS_Install.GEL`)
also holds the parts listed here. Each keeps its own licence.

- **Licence texts.** An image built from this tree holds them under `/etc/licenses`, together with this file. In the
  tree they are under [rootfs/initramfs/etc/licenses](rootfs/initramfs/etc/licenses) and the places the table names;
  the texts that come out of a build (`e2fsck`, `mesa`, `glibc` and the like) are written by the build scripts.
- **Sources.** Every [release](https://github.com/dekajoule/oos/releases) carries, next to the image, the source
  archives of the binaries in it: the files of the column "Source with a release". The rest of the corresponding
  source is this tree at the tag of the release (GitHub's "Source code" archive): the OOS programs, the kernel
  patches, configuration and device tree, and the scripts that build everything (`boot/`, `kernel/`, `rootfs/`,
  `gpu/`). `tools/release-sources.sh` lists the files a release needs and checks a release for them.
- If you pass an image on, pass these sources and this file on with it, or point to the release you took it from.

## Kernel and firmware

| Part | Version | Licence | In the image | Licence text | Source with a release |
|---|---|---|---|---|---|
| Linux kernel | 6.12.115, Rockchip `develop-6.12` with the stable releases merged ([kernel/README.md](kernel/README.md)) | GPL-2.0-only WITH Linux-syscall-note | the kernel of the boot image and its modules | `GPL-2.0.txt` | `linux-6.12.115-base.tar.xz`; patches, configuration and build script in `kernel/` |
| Device tree of the board | this tree | GPL-2.0-or-later OR MIT | in the boot image | `GPL-2.0.txt` | `kernel/dts/` |
| XDMA driver (Xilinx `dma_ip_drivers`) | commit `f09dce7` | Dual BSD/GPL, as its files state | module `xdma.ko` | in its files | `kernel/modules/xdma/` |
| wireless-regdb (`regulatory.db`) | as in `kernel/firmware/` | ISC | built into the kernel | `wireless-regdb.txt` | `kernel/firmware/` (data, with its signature) |
| RTL8851BU Bluetooth firmware (`rtl8851bu_fw.bin`, `rtl8851bu_config.bin`, from linux-firmware) | as in the tree | Realtek's licence for redistribution in binary form | `/lib/firmware/rtl_bt/` | `/lib/firmware/rtl_bt/LICENCE.rtlwifi_firmware.txt` | none: binary firmware, passed on unchanged with its licence |
| rtw89 driver (Linux, `rtw89_core`, `rtw89_usb`, `rtw89_8851b`, `rtw89_8851bu`) | Linux 6.18.63, with `kernel/modules/rtw89-linux-6.12.patch` | GPL-2.0 OR BSD-3-Clause, as its files state | modules in `/lib/modules/*/updates/` | in its files | `kernel/modules/rtw89/` |
| RTL8851B firmware (`rtw8851b_fw.bin`, from linux-firmware) | as in the tree | Realtek's licence for redistribution in binary form | `/lib/firmware/rtw89/` | `/lib/firmware/rtw89/LICENCE.rtlwifi_firmware.txt` | none: binary firmware, passed on unchanged with its licence |
| RTL8188EU firmware (`rtl8188eufw.bin`, from linux-firmware) | as in the tree | Realtek's licence for redistribution in binary form | `/lib/firmware/rtlwifi/` | `/lib/firmware/rtlwifi/LICENCE.rtlwifi_firmware.txt` | none: binary firmware, passed on unchanged with its licence |

## Programs and libraries

| Part | Version | Licence | In the image | Licence text | Source with a release |
|---|---|---|---|---|---|
| BusyBox | 1.37.0 (Ubuntu `busybox-static` 1:1.37.0-7ubuntu1) | GPL-2.0-only | `/bin/busybox`, static | `GPL-2.0.txt` | `busybox_1.37.0.orig.tar.bz2`, `busybox_1.37.0-7ubuntu1.debian.tar.xz`, `.dsc` |
| GNU C library (glibc), in BusyBox | 2.42 (Ubuntu 2.42-2ubuntu4: the one Ubuntu built `busybox-static` with, its `Built-Using`) | LGPL-2.1-or-later; parts under other licences, see `glibc` | linked into `busybox` | `glibc`, `LGPL-2.1.txt` | `glibc_2.42.orig.tar.xz`, `glibc_2.42-2ubuntu4.debian.tar.xz`, `.dsc` |
| GNU C library (glibc), elsewhere | 2.43 (Ubuntu 2.43-2ubuntu2.4) | LGPL-2.1-or-later; parts under other licences, see `glibc` | linked into `oos-hw`; shared libraries of the GPU helper | `glibc`, `LGPL-2.1.txt` | `glibc_2.43.orig.tar.xz`, `glibc_2.43-2ubuntu2.4.debian.tar.xz`, `.dsc` |
| GCC runtime (libgcc, libstdc++) | GCC 16 (Ubuntu 16-20260322-1ubuntu1) | GPL-3.0-or-later WITH GCC-exception-3.1 | shared libraries of the GPU helper; libgcc parts in the C programs | `libstdc++`, `GPL-3.0.txt`, `GCC-exception-3.1.txt` | `gcc-16_16-20260322.orig.tar.gz`, `gcc-16_16-20260322-1ubuntu1.debian.tar.xz`, `.dsc` |
| Mesa | 26.0.8 | MIT, and the licences its file lists | `libEGL`, `libGLESv2`, `libgbm`, `libgallium` of the GPU helper | `mesa` | `mesa_26.0.8.orig.tar.xz` |
| libdrm | 2.4.131 (Ubuntu 2.4.131-1) | MIT | shared library of the GPU helper | `libdrm` | `libdrm_2.4.131.orig.tar.xz`, `libdrm_2.4.131-1.debian.tar.xz`, `.dsc` |
| zlib | 1.3.1 (Ubuntu 1.3.dfsg+really1.3.1-1ubuntu3.2) | Zlib | shared library of the GPU helper | `zlib` | `zlib_1.3.dfsg+really1.3.1.orig.tar.gz`, `…-1ubuntu3.2.debian.tar.xz`, `.dsc` |
| e2fsprogs (`e2fsck`) | 1.47.2 | GPL-2.0-only; its libraries LGPL-2.0-only, BSD-3-Clause and MIT, as its notice says | `/bin/e2fsck`, static | `e2fsck`, `GPL-2.0.txt` | `e2fsprogs-1.47.2.tar.xz` |
| kexec-tools | 2.0.32 | GPL-2.0-only | `/bin/kexec`, static | `kexec-tools` | `kexec-tools-2.0.32.tar.xz` |
| wpa_supplicant (`wpa_supplicant`, `wpa_cli`) | 2.11 | BSD-3-Clause | `/bin`, static | `wpa_supplicant` | `wpa_supplicant-2.11.tar.gz` |
| libnl | 3.11.0 (Alpine `libnl3-static` 3.11.0-r0) | LGPL-2.1-or-later | linked into `wpa_supplicant` and `wpa_cli` | `LGPL-2.1.txt` | `libnl-3.11.0.tar.gz`, `libnl3-musl.patch` (Alpine's one patch) |
| BlueZ (`bluetoothd`, `btmon`) | 5.82 | GPL-2.0-or-later; its libraries LGPL-2.1-or-later | `/bin`, static | `bluez`, `GPL-2.0.txt`, `LGPL-2.1.txt` | `bluez-5.82.tar.xz` |
| D-Bus (`dbus-daemon`) | 1.16.2 | AFL-2.1 OR GPL-2.0-or-later | `/bin/dbus-daemon`, static | `dbus` | `dbus-1.16.2.tar.xz` |
| expat | 2.10.0 (Alpine `expat-static` 2.10.0-r0) | MIT | linked into `dbus-daemon` | `expat` | `expat-2.10.0.tar.xz` |
| GLib | 2.84.4 (Alpine `glib-static` 2.84.4-r0) | LGPL-2.1-or-later | linked into `bluetoothd` | `LGPL-2.1.txt` | `glib-2.84.4.tar.xz`, `tests-machine-id.patch`, `flaky-tests.patch` (Alpine's patches) |
| PCRE2 | 10.46 (Alpine `pcre2-static` 10.46-r1) | BSD-3-Clause WITH PCRE2-exception | linked into `bluetoothd` (through GLib) | `pcre2` | `pcre2-10.46.tar.bz2` |
| GNU gettext (`libintl`) | 0.24.1 (Alpine `gettext-static` 0.24.1-r0) | LGPL-2.1-or-later | linked into `bluetoothd` (through GLib) | `LGPL-2.1.txt` | `gettext-0.24.1.tar.xz`, `cwd.patch`, `skip-tests-musl.patch`, `skip-meaningless-pthread-rwlock-test.patch` (Alpine's patches) |
| nftables (`nft`) | 1.1.3 | GPL-2.0-only; its mini-gmp LGPL-3.0-or-later OR GPL-2.0-or-later | `/bin/nft`, static | `nftables` | `nftables-1.1.3.tar.xz` |
| libmnl | 1.0.5 | LGPL-2.1-or-later | linked into `nft` | `LGPL-2.1.txt` | `libmnl-1.0.5.tar.bz2` |
| libnftnl | 1.2.9 | GPL-2.0-or-later | linked into `nft` | `GPL-2.0.txt` | `libnftnl-1.2.9.tar.xz` |
| Dropbear (`dropbear`, `dropbearkey`, `scp`) | 2026.94 | MIT, and the licences of its parts its file lists | `/bin/dropbearmulti`, static | `dropbear` | `dropbear-2026.94.tar.bz2` |
| OpenSSH (`sftp-server` only) | 10.6p2 | BSD licences, as its file lists | `/bin/sftp-server`, static | `openssh-sftp-server` | `openssh-10.6p2.tar.gz` |
| musl | the one of Alpine 3.22 (C programs) and of Rust's musl target (Rust programs) | MIT | linked into `e2fsck`, `kexec`, `wpa_supplicant`, `wpa_cli`, `nft`, `dropbearmulti`, `sftp-server`, `dbus-daemon`, `bluetoothd`, `btmon`, `oos-ui`, `oos-fpgad`, `oos-update` | `musl.txt` | not required |
| Rust standard library | the toolchain of the build | MIT OR Apache-2.0; parts under other permissive licences | linked into `oos-ui`, `oos-fpgad`, `oos-update` | `rust.txt`, `Apache-2.0.txt` | not required |
| `libc` crate | as in `ui/Cargo.lock` | MIT OR Apache-2.0 | linked into `oos-ui`, `oos-fpgad`, `oos-update` | `rust.txt`, `Apache-2.0.txt` | not required |
| Roboto (roboto-classic) | as in `ui/tools/fonts/` | Apache-2.0 | its glyphs, drawn into `oos-ui` | `Apache-2.0.txt` ([ui/tools/fonts/LICENSE-Roboto.txt](ui/tools/fonts/LICENSE-Roboto.txt)) | not required |

The versions are those of release 0.1.15. `rootfs/deps.lock` pins the archives the image builds from, with their
checksums; the Ubuntu and Alpine packages come from the container images the build scripts name (`ubuntu:26.04`,
`alpine:3.22`), and a release carries the source packages of the versions that were in its image. A package that
links a library in keeps the library's version of the day it was built: `busybox.version` of a build names the one
of BusyBox. The releases up to 0.1.6 were built before the Ubuntu revision of glibc was recorded; they carry the
source of 2.43-2ubuntu2.4, the revision of the next build.

## Not in the image

- The stock software of the scope, its FPGA image and the calibration files are not part of OOS and not in any OOS
  image; OOS reads them on the scope.
