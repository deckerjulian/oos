# Boot medium and boot chain of the DHO924S

## Medium

- The RK3399 boots from the **SD card** on `dwmmc@fe320000` (Android fstab: `/dev/block/platform/fe320000.dwmmc/by-name/*`).
  `sdhci@fe330000` (eMMC controller) is enabled in the DT but not used for the system.
- Linux sees the card as `/dev/block/mmcblk0`; a tool that works on the card should also handle `mmcblk1`.

## Card layout (Rockchip legacy "parameter" layout)

Partition table: the stock PARM CMDLINE. Offsets there are relative to LBA 0x2000; the table below is absolute. Verified on
the unit against a card image: the IDB, vendor storage, `uboot` through `backup`, `verity_mode`, `baseparamer` and `frp`.

| LBA | Sectors | Size | Partition / content |
|---:|---:|---:|---|
| 0 | | | no MBR signature, no GPT |
| 64 | | | IDB: Rockchip TPL/SPL (DDR init + miniloader) |
| 7168 | 4 x 128 | 4 x 64 KiB | vendor storage: LAN MAC (id 3), BT MAC (id 4), id 26 (8 bytes) |
| 8192 | 0x2000 | 4 MiB | `parameter` (PARM, copies up to LBA 16383) |
| 16384 | 0x2000 | 4 MiB | `uboot` (LOADER, U-Boot 2014.10-RK3399, 2024-08-29) |
| 24576 | 0x2000 | 4 MiB | `trust` (BL3X, TF-A v1.3) |
| 32768 | 0x2000 | 4 MiB | `misc` (BCB, holds `boot-recovery`) |
| 40960 | 0x8000 | 16 MiB | `resource` (RSCE: `rk-kernel.dtb`, `logo.bmp`, `logo_kernel.bmp`) |
| 73728 | 0xC000 | 24 MiB | `kernel` (KRNL + raw arm64 Image, Linux 4.4.126) |
| 122880 | 0x10000 | 32 MiB | `boot` (KRNL + gzip ramdisk) |
| 188416 | 0x20000 | 64 MiB | `recovery` (KRNL + gzip ramdisk) |
| 319488 | 0x38000 | 112 MiB | `backup` (**all zero** on the original card) |
| 548864 | 0x40000 | 128 MiB | `cache` (ext4) |
| 811008 | 0x400000 | 2 GiB | `system` (ext4) |
| 5005312 | 0x8000 | 16 MiB | `metadata` (ext4) |
| 5038080 | 0x40 | 32 KiB | `verity_mode` |
| 5038144 | 0x2000 | 4 MiB | `baseparamer` |
| 5046336 | 0x400 | 512 KiB | `frp` |
| 5047360 | 0xFA000 | 500 MiB | `rigol` (ext4, `/rigol`: app files, FPGA images, calibration in `/rigol/data`) |
| 6299648 | rest | ~26.8 GiB on 32 GB | `userdata` (ext4, `/data`) |

Kernel command line words added by the stock loader: `storagemedia=sd androidboot.oem_unlocked=0
uboot_logo=0x02000000@0xf5c00000 loader.timestamp=<build time> SecureBootCheckOk=0`;
for a `recovery`/`backup` boot it prefixes the partition list with `0x00002000@0x00000000(parameter),`.

## Boot chain

1. BootROM -> IDB at LBA 64 (TPL/SPL) -> `uboot` partition (Rockchip U-Boot 2014.10) + `trust` (BL31).
2. U-Boot reads `parameter`, the reboot flag and boot keys; normally it boots `boot`, falling back to `recovery`, then `backup`.
   It loads the kernel from `kernel` and the DTB from `resource` (it can also load the FDT from the recovery image).
3. Rockchip "KRNL" images are `KRNL` + u32 length + payload (+ CRC). `boot` and `recovery` hold only ramdisks.
4. U-Boot 2014 RK also boots Android boot images (`ANDROID!` header).

### How the stock loader chooses

Verified on the unit: with `boot-recovery` at LBA 32800 plus the OOS image in `recovery`, power-on and `reboot recovery`
both booted Android from `boot` (kernel command line without the `(parameter)` prefix, `androidboot.mode=emmc`).
The `misc` boot request is not on the power-on path.

**Decision logic of the stock loader:**
- A reboot flag in a PMU register (0x5242C3xx: normal, rockusb, fastboot, recover, wipe data, charge) and boot keys
  ("recovery key pressed", "rockusb key pressed"). The stock Android restarts through the front-panel MCU (power
  cycle), so the flag from `reboot recovery` does not survive.
- When an image fails its check (magic and sha): `boot` failed -> "try to start recovery"; `recovery` failed -> "try
  to start backup"; else Rockusb.

**Switch used by OOS:** replace the magic `KRNL` at the start of `boot` (LBA 122880) with `OOSK`. The loader then
boots `recovery`, and if it refuses that image it boots `backup`, which holds a copy of the stock boot image
(`card-install.py --fallback-stock`), in its first 32 MiB. Back to stock: write `KRNL` again. `boot/switch.py` does this
on a card in a host computer. The OOS initramfs restores `KRNL` at start (one-shot), and `oos-boot-oos-again` sets `OOSK`.

### Ramdisk length must be a multiple of 4

Verified on the unit: the loader refuses an image whose ramdisk length is not a multiple of 4. It hashes with the RK3399
crypto engine (`init` mode 0xa0), which works on 32-bit words. Images built from stock parts (with `boot` = `OOSK`,
`backup` = `BAKK`, so a refusal shows as Rockusb 2207:330c) gave: stock kernel + stock ramdisk + stock DTB boots (command
line then starts with `0x00002000@0x00000000(parameter)`); the same padded to 29.5 MB boots; the stock kernel padded to
the 6.12 size boots; stock kernel + stock DTB + an initramfs of 1 181 602 bytes is refused. `oos-bootimg.py` pads the
ramdisk to 4 bytes; the padded image boots Linux 6.12.115+ on the unit (panel, SD card as mmcblk0, Ethernet, one-shot).
The switch (`OOSK` in `boot` -> loader boots `recovery`) is verified on the unit.

### Image refusal

With `boot` = `OOSK` and the fallback in `backup` disabled (`BAKK`), a refused image leaves the Rigol logo on screen and
the scope enumerates on USB as Rockchip 2207:330c (Rockusb): the loader refused `recovery` and `backup` and went to "try
to start rockusb". With a stock copy in `backup`, a refusal of `recovery` boots Android instead.

The loader's check:
- secure mode flag set -> needs `ANDROID!` and "SIGN" at +0x400 plus an RSA signature ("signature mismatch!");
- otherwise `ANDROID!` magic, then SHA-1 over kernel, le32 size, ramdisk, size, second, size, header[32:576] compared
  with the id at +0x240 ("boot or recovery image sha mismatch!"). `oos-bootimg.py` computes exactly this.

The loader's messages go to UART2 (115200 8N1).

## Image format the stock loader accepts in `recovery`

- Android boot image, header version 0, page size 2048: `ANDROID!`, sizes and load addresses of kernel, ramdisk and
  `second`, `name[16]`, `cmdline[512]`, `id[32]`.
- kernel = uncompressed arm64 `Image`, loaded and entered at 0x00280000 (limit 0x00200000 + 0x3800000).
- ramdisk = gzip newc cpio, loaded at 0x04BF0000 (ramdisk + second together < 0x3000000).
- second = Rockchip RSCE container with `rk-kernel.dtb`.
- id = SHA-1 over kernel + le32 size, ramdisk + le32 size, second + le32 size, then header bytes 32..575; the loader
  refuses an image whose id does not match.
- The kernel command line is built by the loader: DTB `/chosen/bootargs` + the PARM CMDLINE + the loader's own words.
- Whole image must fit `recovery` (64 MiB).

## Consequences for OOS

- `boot/oos-bootimg.py` is OOS's own packer for this format (from scratch, no base image needed).
- The stock `kernel` partition stays untouched; OOS carries its kernel in the `recovery` boot image.
- The switch between stock and OOS is the magic of `boot`: `OOSK` = OOS (via the fallback to `recovery`), `KRNL` = stock.
- The PARM table stays as it is. Assumed, not yet verified on hardware: U-Boot checks a CRC over the first 942 bytes of
  the PARM copy, and a changed table drops into Rockusb.
- The card runs at 3.3 V high speed (`cap-sd-highspeed`). `kernel/dts/rk3399-rigol-dho900-uhs.dts` is an opt-in variant
  with UHS-I (SDR104, SDR50) and a 30 ms settling time on `vccio_sd`; the image takes it with
  `OOS_KERNEL_DTB=rk3399-rigol-dho900-uhs.dtb` (or a unit's `build.env`). Why each setting: the comment in that file.
  Not yet verified under the OOS kernel. A warm restart from a stock system that left the card at 1.8 V into an OOS
  image without the variant is not verified either: use a power cycle.

## OOS start-up after the kernel (rcS)

Not yet verified on hardware in this order.

Order in `rootfs/initramfs/etc/rcS`, each step logged to `/tmp/oos-boot.status` (`oos_hal::bootstatus`: uptime,
state, stage, detail; the boot-time profile of every start). What needs the hardware rcS asks of the board
(`rootfs/board/dho/bin/oos-board`); the commands named below are what that script runs on the DHO800/DHO900. Addresses come
from `/etc/oos-board` ([porting.md](porting.md)):

1. `Linux` (uptime at the start of rcS).
2. `oos-oneshot` (boot magic back to `KRNL`), as the very first program, so a hanging new build on trial is followed
   by stock at the next power-on (`update.md`); only when it succeeded the automatic restarts (panic restart and the
   hardware watchdog `oos-watchdog`, below), then `/oos`, the
   crash records of the last start in the background (`oos-crash-save`, below) and the start count of 4c. Then **`oos-fpgad fpga-boot`**: only the
   BOOT command (spidev1.0, slot 0x400000), its uptime to `/tmp/oos-fpga-booted`. The bitstream loads while the rest of
   rcS runs; the fixed 3 s FPGA wait is counted from this command.
3. Network in the background (`udhcpc -t 5` would block the start-up up to 15 s without a DHCP answer), SSH when
   switched on (password or key, [network.md](network.md) 7), banner, `/oos`, touch and beeper modules.
4. **oos-ui** with `--hw --afe --start-up /tmp/oos-boot.status`: the start-up screen (`ui/src/ui/startup.rs`; logo or
   `/oos/splash.ppm`, checklist with times) while its hardware thread waits for the `end` line. No demo signals and
   no restart of oos-ui.
4c. `oos-update boot` (`update.md`), early in rcS: counts the start of an image on trial; one that already started once
   without confirmation is rolled back here. After oos-ui: `oos-update watch` in the background confirms the trial
   (oos-ui 20 s stable + start-up finished) or writes the previous image back.
4d. `oos-wlan` in the background: waits for the end of the start-up, then WLAN through a USB stick (`wlan.md`).
5. In the background: `oos-mount-rigol`, then **`oos-fpgad cold-boot --execute --power-on --booted --progress`**:
   calibration files check (warn if one is missing or damaged), sample clock and AFEs during the FPGA wait (GPIO
   only, no FPGA needed), the rest of the wait, PCIe polled every 20 ms, the cold-boot steps grouped into the screen's stages (`boot::stage`: FPGA image, sample clock, analog front ends,
   ADC and memory set-up, ADC data lanes, signal path; line-open errors as "GPIO lines", cold-boot warnings as
   "Cold-boot checks"), then the temperatures (SoC, FPGA XADC; warn above the guard limits). rcS writes `end` or `abort`
   only after oos-fpgad exited, so its GPIO lines are free for oos-ui.

The start-up screen goes away 2 s after a clean start (5 s with warnings); a failed start stays until a tap and names
the stage; System > About shows the last result. After 60 s without a new status line a tap leaves the screen (hung
mount or driver load). The power key and its dialog work over the start-up screen. The first status read happens at once.

The FPGA self-checks (0x1008 bit 29, 0x1210) are part of the cold boot; the ADC self-test stays out (never-do list in
`protocols/cold-boot.md`, not needed).

Time until acquisition, measured with the status file on a DHO924S: 8.1 s from the kernel's clock (Linux 3 s, calibration
files 0.9 s, sample clock 0.4 s, AFEs 2.7 s, the rest 1.1 s). Sample clock and AFEs run during the 3 s FPGA wait; `/oos/fpga-wait`
overrides it for trials.

## Restart after a panic or a hang

The scope runs unattended, so a kernel that stops restarts by itself, but only once a restart cannot lead back into
the image that failed ([ADR 0027](adr/system/0027-automatic-restart-after-the-boot-selection.md)). Verified on a DHO804
(sysrq panic, killed kicker, warm reboot) with `panic=10` on the command line, before the restart moved behind
`oos-oneshot`; the gating itself is tested in `tools/selfheal-sim.sh`, not yet on hardware.

- rcS arms both only when `oos-oneshot` succeeded: any restore write and sync must succeed, and a complete,
  successful read with `dd iflag=direct` must show `KRNL` (also for an already-stock selection). A restored sector
  must also match the intended sector in full:
  `echo 10 > /proc/sys/kernel/panic`, then `oos-watchdog`. Before that line, and when the magic could not be
  restored, a panic halts as it always did, and `/tmp/oos-boot.status` carries a boot warning. rcS sets
  `kernel.softlockup_panic` in the same branch; neither panic setting is armed in `OOS_BOOTARGS`, and kexec discards
  the old build's `panic=` and `softlockup_panic=` words: a soft lockup panics; hard lockups and tasks hung for
  120 s already did (kernel config). On the unit: back with SSH 39 s after `echo c > /proc/sysrq-trigger`.
- Hardware watchdog: `dw_wdt` at `ff848000` (enabled in `rk3399-base.dtsi`). rcS starts `oos-watchdog` right after a
  successful `oos-oneshot`: busybox `watchdog -F -T 30 -t 5`, nice -20, `oom_score_adj` -1000. If it exits it is started again
  for as long as the system runs: after 1 s, doubling up to 60 s, back to 1 s after 10 min of running; each restart is
  logged to the kernel log. The backoff sleep is interruptible, so a kexec handoff (`update.md`) is answered at once. It does not depend on oos-ui. The hardware period is at most 21.5 s (2^31 cycles of
  `pclk_wdt` at 100 MHz); the watchdog core pings the hardware on until the 30 s since the last user ping run out. On
  the unit a killed kicker reset the SoC 41 s later. Log: `/tmp/oos-watchdog.log`. Before it opens the device it logs
  one `before open:` line (also in the kernel log): whether the hardware counter already runs (control register from
  the driver's debugfs), its period, the hardware maximum, sysfs `state`/`timeout`/`timeleft`/`bootstatus` and whether
  this is a kexec start.
- The device tree gives the watchdog no reset line, so the kernel cannot stop it once started. A reboot or a panic
  restart (PSCI system reset) clears it with the SoC (on the unit: `hw stopped` after a warm reboot), so a stock
  Android start after it sees it off. A kexec start keeps it running: nothing services it between the last ping before
  the jump and the registration of `dw_wdt` in the new kernel; from then on the kernel pings it
  (`CONFIG_WATCHDOG_HANDLE_BOOT_ENABLED`) until `oos-watchdog` opens it again. If the rest of the hardware period does
  not cover that gap, the SoC resets and the previous build starts again.
- Which system comes back follows the boot magic: OOS in persist mode, else stock Android. A build on trial that
  panics is rolled back at its next start (`update.md`).
- A panic before `oos-oneshot` (kernel, initramfs, the first lines of rcS) halts, as before: `OOSK` is still set
  there, and a restart would start the same image again with neither the trial count nor the rollback run. A kexec
  start carries the running watchdog into the new kernel; a hang there resets the SoC into the previous build, which
  `recovery` still holds. If `oos-oneshot` fails in a kexec start, the new kernel keeps pinging the inherited watchdog
  itself (`CONFIG_WATCHDOG_HANDLE_BOOT_ENABLED`), so hangs then go undetected.
- Crash records: ramoops keeps the oops/panic record and the console across a warm reset, not across a power cut.
  rcS starts `oos-crash-save` in the background, so a slow write never delays oos-ui. Only with 4 MiB free on `/oos`,
  it copies `dmesg-ramoops-*` and the last 64 KiB of `console-ramoops` into `/oos/crash/.tmp`, checks every copy,
  fsyncs each file and the directory, compares each file with its source, renames it to `/oos/crash/<n>/` and fsyncs
  `/oos/crash` and `/oos` (the parent of a newly created crash directory). Only then are the pstore entries removed and the oldest directories beyond 4 pruned. Any failure
  leaves pstore as it was. A `.tmp` from an interrupted save is kept as `.partial-<n>` and counts towards the 4. The
  start-up screen shows a `Crash record` warning; log `/tmp/oos-crash.log`. A watchdog reset leaves no dmesg record,
  so nothing is saved for it. `tools/selfheal-sim.sh` runs both scripts against a fake pstore and device.
