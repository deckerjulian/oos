# OOS kernel

Everything the kernel of OOS is built from. `build-base.sh` needs nothing outside this directory.

| Part | What |
|---|---|
| `src/linux-6.12.115-base.tar.xz` | Linux 6.12.115 with Rockchip's BSP: [Rockchip's `develop-6.12`](https://github.com/rockchip-linux/kernel) at `470f9dccbdc42e7b8a824d0a5c5640a10e9457d2` with the kernel.org 6.12 stable releases up to v6.12.115 merged in. Too large for git, `base.sha256` names it. Compared file by file with both: of 92,158 files 83,877 equal Rockchip's, 8,092 equal the stable tree, 189 are merges of the two, none is new. 37 lines in 20 of the merged files are in neither, mostly the merge following changed interfaces (syscore, reset controls) and build fixes in `bcmdhd`. 13 netfilter files whose names differ from others only in case are left out |
| `series`, `patches/` | 10 patches of ours on that tree, in this order: bug fixes the scope needs |
| `dts/` | device tree of the DHO800/DHO900 board |
| `config` | the kernel configuration: only what OOS uses, with 7 modules beside `xdma` |
| `firmware/` | `regulatory.db` for the WLAN stack, built into the kernel |
| `modules/xdma/` | Xilinx' XDMA driver, unchanged: [dma_ip_drivers](https://github.com/Xilinx/dma_ip_drivers) at `f09dce768433b2809e7aa6b9d41779265357a2b4`, `XDMA/linux-kernel` |
| `tools/` | answers the `git log` queries of the Rockchip drivers, so the build needs no git tree |

`build-base.sh` always gives the same bits for the same input: fixed time stamp, build names and paths. Compiler: gcc 15.2.0.

Licences: kernel and patches GPL-2.0, device tree GPL-2.0+ OR MIT, `xdma` as in its files, `firmware/` as in `firmware/LICENSE.wireless-regdb`.

Whoever passes on an image must offer this source, the archive in `src/` included.
