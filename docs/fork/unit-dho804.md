# Unit support: DHO804

Private fork notes for one unit. The mechanisms are generic and documented in the upstream docs; this page lists what
this unit uses, how to build its image, and what is still unverified.

## The unit

- Physically a DHO804: 4 channels, no logic analyser connector, no generator board (an AWG board is planned).
- `vendor.bin` names a DHO924S. OOS takes the identity from it, so it would start with generator and LA.
- Boot card: 64 GB, runs SDR104 under the patched stock kernel.
- LAN 192.0.2.0/24, fixed MAC `02:00:00:00:00:01`.

## Changes

| Area | What | Where |
|---|---|---|
| Capabilities | `/etc/oos/limits.conf` in the image: capability keys that apply from the first start, between vendor.bin and `/oos/limits.conf`. Not experimental; identity unchanged | `hal/src/model.rs` (`UNIT_CONF`, `Platform::build_unit`), docs/models.md 4.2 |
| Generator lines | `afg::Io::open` refuses without a generator; `oos-fpgad gpio` refuses `afg_in1..5` and `FG_M2C_GPIO` without `--force`; `oos-fpgad afg --force` uses `Io::open_forced` | `hal/src/dho/afg.rs` (`LINES`, `line_allowed`), `fpgad/src/main.rs` |
| Boot card | opt-in device tree with `sd-uhs-sdr104`, `sd-uhs-sdr50`, `regulator-settling-time-down-us = <30000>` on `vccio_sd`, no `regulator-init-microvolt` | `kernel/dts/rk3399-rigol-dho900-uhs.dts`, `kernel/build-base.sh` copies every board DTB |
| Firewall | `net.allow` prefixes: nftables ruleset before any service starts; static `nft` in the image | `rootfs/initramfs/bin/oos-firewall`, `rootfs/build-nft.sh`, rcS |
| Remote ACL | the same prefixes in oos-ui: `POST /update`, `/mcp` get 403, SCPI and VXI-11 connections are closed | `ui/src/netacl.rs`, `http.rs`, `scpi.rs`, `vxi11.rs` |
| Telnet | `telnet = off` leaves telnetd out | rcS |
| MAC | `eth.mac = cid / vendor / <address>` | `rootfs/initramfs/bin/oos-mac`, `oos-board mac`, rcS |
| Settings lookup | `/etc/oos/system.conf` (image default), then `/oos/system.conf`; last line wins | `rootfs/initramfs/bin/oos-conf`, `netacl::Acl::from_texts` |
| Unit files | `limits.conf` (`afg = no`, `la = no`), `system.conf` (`net.allow = 192.0.2.0/24`, `telnet = off`, `eth.mac = 02:00:00:00:00:01`), `build.env` (UHS DTB) | `rootfs/unit/dho804/` |

Without `OOS_UNIT` the image is upstream-identical in behaviour, apart from carrying `nft` (about 1 MB).

## Building the image

Needs Docker, rustup with `aarch64-unknown-linux-musl`, and the kernel base archive in `kernel/src/` (from an OOS
release, `kernel/README.md`).

```sh
kernel/build-base.sh                                   # kernel + rk3399-rigol-dho900{,-uhs}.dtb into kernel/out/base
OOS_UNIT=dho804 boot/build-dev-image.sh     # boot/out/oos-dev.img, .oosupd, GEL
```

Check the result before it goes anywhere near the scope:

- `boot/out/initramfs.cpio.gz` contains `etc/oos/limits.conf`, `etc/oos/system.conf`, `bin/nft`, `bin/oos-firewall`,
  `bin/oos-conf`, `bin/oos-mac` (`gzip -dc ... | cpio -t`).
- The DTB in the image is the UHS one: `fdtget -p boot/out/oos-dho900.dtb /mmc@fe320000` lists `sd-uhs-sdr104`.

The GEL install accepts the model vendor.bin names (DHO924S), so the unit passes the model check. Install on the second SD card only
(private RE notes `docs/oos-review.md`, "If we try it").

When the AWG board is fitted: `afg = yes` in `/oos/limits.conf` (shows as experimental), or remove the line from
`rootfs/unit/dho804/limits.conf` and rebuild. `cal_afg.hex` will be missing until the stock software
calibrates the new board.

## Verified on the host

- `cargo test` in `hal`, `dsp`, `ui`, `fpgad`: all pass (new tests: `model::unit_defaults_tests`,
  `afg::line_tests`, `netacl::tests`).
- `cargo check` of `hal`, `ui`, `fpgad` on linux/arm64 musl (container), so the `target_os = "linux"` paths compile.
- Both device trees preprocess and compile with dtc 1.8.1 against `rk3399.dtsi` and the headers of Rockchip
  `develop-6.12` at 470f9dcc (fetched file by file; not the release archive with the 6.12.115 merges). The
  decompiled difference is exactly the three properties.
- In the same Rockchip tree: RK808 LDO_REG4 has no ramp delay and no `set_voltage_time_sel`, so
  `_regulator_set_voltage_time` returns `settling_time_down` on a falling switch. The 30 ms apply.
- The firewall ruleset loads with the static `nft` into a Linux kernel (arm64 container with `NET_ADMIN`), including
  mixed IPv4/IPv6 lists, host bits in a prefix and an all-invalid list. Packet filtering itself was not exercised.
- `oos-conf`, `oos-firewall --print` and `oos-mac` under busybox `sh`: precedence, empty values, bad prefixes, bad and
  multicast MACs, missing CID; the CID path gives the same MAC as the old rcS code.

## Needs verification on hardware

1. First start: `oos-fpgad model` shows `DHO924S`, caps `250 MHz rated, 4 ch`, `unit afg = no`, `unit la = no`;
   System > System info shows "Unit defaults"; `/sys/kernel/debug/gpio` shows no consumer on `afg_in1..5` and `FG_M2C_GPIO`; no
   `cal_afg.hex` warning in `/tmp/oos-cold.log`.
2. Boot card under the OOS kernel with the UHS DTB: `/sys/kernel/debug/mmc0/ios` with debugfs mounted (timing SDR104,
   1.80 V, 150 MHz), read rate, `dmesg | grep mmc0` without errors, over several cold starts. A failure here can hang the
   kernel before rcS, which leaves `OOSK` set: recovery is card out, `switch.py stock` on a host. Use cold power
   cycles only; a warm restart from stock with the card at 1.8 V is not tested.
3. Firewall on the OOS kernel: `nft list ruleset`, then from 192.0.2.x the web page, SCPI 5555, VXI-11 work; from
   another subnet (or the 192.168.177.64 fallback) they do not; DHCP renewals and NTP still work.
4. oos-ui ACL: `POST /update` and `/mcp` from an allowed host work. For the refusal path the firewall has to let an
   outside host through: on the scope `nft delete table inet oos_fw`, then `POST /mcp` from another subnet gives 403
   and SCPI 5555 closes at once. Power-cycle afterwards to restore the firewall.
5. MAC: `ip link show eth0` gives `02:00:00:00:00:01`, the DHCP server hands out the expected address.
6. Telnet: port 23 closed; `telnet = on` in `/oos/system.conf` plus a power cycle opens it.
7. Fallback path (risk 2 in `docs/oos-review.md`): unchanged by this work, still open.
