# ADR 0027: A WLAN driver of a newer kernel comes as a module beside 6.12, with a patch of its own

- Status: accepted (2026-11-18, by the maintainer)
- Date: 2026-11-16
- Category: system (touches ADR 0013, ADR 0026)

## Context

The TP-Link Archer TX10UB Nano (Realtek RTL8851BU) brings Bluetooth and WLAN in one stick. Its WLAN needs
`rtw89` with USB, which came with Linux 6.17; OOS runs 6.12 with the SoC's BSP (`kernel/README.md`). Moving the whole
kernel to a newer release for one stick would move the BSP, the 12 patches and every driver the acquisition depends
on.

`rtw89` of Linux 6.18.63 (the long-term release), only the files of the 8851BU, built against the 6.12 tree: three
places in `mac80211.c` and `wow.c` follow interfaces of mac80211 that changed in between (the radio index of 6.17,
`link_sta_rc_update` of 6.13, `ieee80211_gtk_rekey_add`); with them it builds without a warning (2026-11-16).

Verified on the unit (DHO924S, 2026-11-16, development image started by kexec, the stick in the front port beside
Bluetooth): `oos-wlan` chose `rtw89` by the stick's id, the firmware 0.29.41.5 loaded, `wlan0` joined a saved WPA2
network on 5 GHz (Wi-Fi 6, 80 MHz, 540 Mbit/s link rate at -59 dBm), DHCP, 80 MiB sent to a computer on the same
network in 8 s (about 84 Mbit/s).

## Decision

- A driver a newer kernel has and 6.12 lacks comes as an external module: its files from a named stable release,
  unchanged, in `kernel/modules/<driver>/` (only those the board needs), a `Kbuild` of ours, and one patch
  `kernel/modules/<driver>-linux-6.12.patch` for the interfaces of 6.12. `kernel/build-base.sh` builds it as it builds
  `xdma`; the modules go into `updates/`.
- `rtw89` is the first: `rtw89_core`, `rtw89_usb`, `rtw89_8851b`, `rtw89_8851bu`, from Linux 6.18.63, with
  `rtw89/rtw8851b_fw.bin` from linux-firmware. PCI and the other chips stay out.
- `oos-wlan` picks the driver by `modules.alias`: a vendor-specific USB interface whose id one of `rtw89`'s aliases
  matches gets `rtw89`, every other one `rtl8xxxu` as before.

## Alternatives

- **Linux 6.18 for the whole system:** the driver with every fix, and no patch of ours; but a new BSP merge, the
  patches again, and every function of the scope tested again on a unit. Worth it when other reasons come together.
- **Realtek's own out-of-tree driver for the 8851BU:** a large vendor driver with its own WLAN stack, not the one
  mainline maintains; outside what ADR 0013 accepts.
- **No WLAN on that stick:** a second stick for WLAN; the front port has one place.

## Consequences

- The driver is in the tree twice in effect: fixes to `rtw89` in newer releases reach OOS only when the files are
  taken again (a new release name in `kernel/README.md`, the files copied, the patch checked). The patch stays small
  as long as only interfaces differ; a patch that grows into logic is the sign to move the kernel instead.
- The image grows by about 1 MB compressed (2.4 MB unpacked: 1.25 MB modules, 1.16 MB firmware). A scope without such
  a stick loads nothing of it.
- How it is known (ADR 0015): driver choice, firmware, joining, DHCP and throughput verified on the unit; long runs,
  2.4 GHz, WPA3 and the acquisition rate under WLAN traffic not yet.
- Worth a second look when OOS moves to a kernel that has the driver: the module and its patch go then.

## References

`kernel/modules/rtw89/`, `kernel/modules/rtw89-linux-6.12.patch`, `kernel/build-base.sh`,
`rootfs/initramfs/bin/oos-wlan`, [wlan.md](../../wlan.md), ADR 0013, ADR 0026.
