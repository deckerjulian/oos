# ADR 0057: Bluetooth mice and keyboards through BlueZ on a system bus of their own

- Status: accepted (2026-11-18, by the maintainer)
- Date: 2026-11-18 (drafted 2026-11-15 as 0026; 0026 and 0055 went to other records meanwhile)
- Category: system (touches process: ADR 0013, ADR 0033)

## Context

A mouse and a keyboard make the scope easier to use on a bench: menus, the measurement page, typing a WLAN password
or a name. Most of them today are Bluetooth Low Energy devices; some keyboards are classic Bluetooth. A combined
WLAN and Bluetooth stick in the front port can serve them.

The stick at hand is a TP-Link Archer TX10UB Nano (verified on the unit, 2026-11-15): it first shows up as a driver
CD, USB id `0bda:1a2b`, which the kernel ignores on purpose (usb-storage, `US_FL_IGNORE_DEVICE`); after a SCSI eject
it is `3625:010b` with a Bluetooth interface pair (class `e0/01/01`) and a WLAN interface (class `ff`). Linux 6.12's
`btusb` lists `3625:010b` as a Realtek 8851BU and `btrtl` loads `rtl_bt/rtl8851bu_fw.bin`. Its WLAN needs `rtw89`
with USB, which 6.12 does not have (ADR 0027 brings it as a module).

A host for Bluetooth input needs more than the kernel: pairing (SSP, LE Secure Connections, a passkey shown for a
keyboard), the link keys kept across power cycles, SDP and the HID profile for classic devices (the kernel's `hidp`
then makes the input device), GATT and HID over GATT for LE devices (bluetoothd hands the reports to the kernel
through `uhid`), and reconnection when a sleeping device wakes up. BlueZ's `bluetoothd` does all of it and talks to
its clients over D-Bus.

## Decision

- The image carries BlueZ (`bluetoothd`, `btmon`) and `dbus-daemon`, built from their release tarballs (pinned in
  `rootfs/deps.lock`) as static binaries, with the input profiles only: no audio, network, OBEX, mesh, udev or
  systemd, and no `bluetoothctl` (it would bring readline; oos-ui is the client). expat, glib, pcre2 and libintl are
  linked in from Alpine's static packages, whose sources a release carries, as for libnl. Their licences go into
  `/etc/licenses` and THIRD-PARTY-LICENSES.md, as for `wpa_supplicant`.
- `bin/oos-bt`, started by rcS after the start-up, switches a stick out of its driver-CD mode (`oos-usbmode`), loads
  `bluetooth`, `hidp`, `btrtl` and `btusb` when a Bluetooth interface is there, and runs a system bus of its own
  (`/etc/oos-bt/dbus.conf`, root only, no activation) and `bluetoothd` while `hci0` exists. A scope without a stick
  loads nothing.
- Pairings live in `/oos/bluetooth` (the data partition), so a paired device stays paired.
- Mice and keyboards reach oos-ui as evdev devices, whatever brought them (USB, `hidp`, `uhid`); oos-ui reads them as
  it reads the touch screen. oos-ui drives pairing on the system bus with a D-Bus client of its own
  (`oos_hal::dbus`, `oos_hal::bluez`) and is the pairing agent.

## Alternatives

- **An own Bluetooth host in Rust** (management socket, SMP through the kernel, SDP, HIDP, ATT/GATT, uhid): in line
  with ADR 0013 and the own mDNS responder, but several thousand lines for a stack the kernel and BlueZ already have
  right, and every device's quirks would be found again.
- **Only USB mice and keyboards:** no stick needed, but a cable across the bench, and a USB port the WLAN stick also
  wants.
- **BlueZ with the distribution's shared libraries:** smaller to build, but the image has no dynamic loader for
  musl; static binaries keep the image as it is (only the GPU helper brings its own loader, ADR 0003).

## Consequences

- About 5 MB more in the initramfs (three static binaries, before compression); nothing runs on a scope without a
  stick.
- ADR 0013 holds for the Rust crates: BlueZ is a program in the image, like `wpa_supplicant`, `dropbear` and `nft`,
  not a dependency of oos-ui. oos-ui speaks to it over the bus, not through a library.
- A new BlueZ or dbus release is a line of `rootfs/deps.lock` and a test with a mouse and a keyboard.
- How it is known (ADR 0015): verified on the unit (DHO924S, 2026-11-15 to 18): the stick's ids and interfaces, the
  modules and the firmware load, a BLE keyboard with touchpad paired, used and reconnected after a power cycle. Not
  yet verified on hardware: a classic (BR/EDR) device, two at once.
- Worth a second look if BlueZ's size in the image becomes a problem. The WLAN half of the stick: ADR 0027.

## References

`rootfs/build-bluez.sh`, `rootfs/initramfs/bin/oos-bt`, `tools/hw/oos-usbmode.c`, `rootfs/initramfs/etc/oos-bt/`,
`rootfs/initramfs/etc/bluetooth/main.conf`, `kernel/config` (`CONFIG_BT*`), [bluetooth.md](../../bluetooth.md),
ADR 0003, ADR 0013, ADR 0027, ADR 0033.
