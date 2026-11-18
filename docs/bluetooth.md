# Bluetooth through a USB stick

OOS can use Bluetooth mice and keyboards through a combined WLAN and Bluetooth stick in the front USB port. The
decision and its reasons are in [ADR 0057](adr/system/0057-bluetooth-input-through-bluez.md).

**Verified on the unit** (DHO924S): on 2026-11-15, in a development image started by kexec, the stick below switched
out of its driver-CD mode, the firmware loaded, `hci0`, the system bus and `bluetoothd` came up, the adapter on with
the scope's name, and an LE scan listed the devices around. On 2026-11-17 and 18, in an installed image with oos-ui's
Bluetooth page: a BLE keyboard with a touchpad paired with its passkey, connected, typed and moved the pointer, and
reconnected by itself after a power cycle. **Not yet verified on hardware:** a classic (BR/EDR) mouse or keyboard,
two devices at once, long runs.

oos-ui talks to bluetoothd with a D-Bus client of its own (`hal/src/dbus.rs`, `hal/src/bluez.rs`; no libdbus) and
registers itself as the pairing agent (`KeyboardDisplay`): it shows a passkey or the PIN `0000` for an old keyboard,
accepts numeric comparison and service authorisations for the device being paired, and refuses to type a passkey
(the scope has no keypad). "The device being paired" is the one whose pairing oos-ui started: every agent call for
another device, or when no pairing runs, is refused (`org.bluez.Error.Rejected`), so nothing around can pair with the
scope or connect a service in the 180 s it stays pairable after a scan (`PairableTimeout`). A paired device is
trusted, so its reconnection asks the agent nothing.

## On the scope

System > Network > Bluetooth:

| Row | What |
|---|---|
| Bluetooth | `on · <name> · n connected`, or why not (no stick, starting) |
| Find devices | listens for 10 s, then lists the mice and keyboards heard in that time (and other devices that give a name), the strongest first; a tap pairs, trusts and connects it. A keyboard shows a passkey on the scope: type it on the keyboard, then Enter. A device leaves the list when it is paired or bluetoothd forgets it (BlueZ's `TemporaryTimeout`, by default 30 s after the scan; assumed, not yet verified on hardware) |
| Paired devices | a tap connects a paired device, or disconnects a connected one |
| Forget device | removes a pairing |
| Bluetooth details | the adapter and every paired device (the pickers of Paired devices and Forget device show the first 18: a picker does not scroll) |

While a Bluetooth stick is there, the top bar has a box `BT` beside the WLAN box: its dot is green while a mouse or
keyboard is connected through it (counted from `/sys/class/input`, bus `0005`, one per address; this sysfs layout is
assumed from the kernel's input core, not yet read on the unit), and a tap opens the page. A paired device is trusted, so it connects again by itself when it wakes up, also after a power
cycle. Its input reaches oos-ui like a USB mouse or keyboard (`ui/src/hid.rs`; what the mouse and the keys do:
the wiki, First Steps).

## Sticks

| Stick | USB id | Bluetooth | WLAN |
|---|---|---|---|
| TP-Link Archer TX10UB Nano (Realtek RTL8851BU) | `0bda:1a2b` (driver CD), then `3625:010b` | `btusb` + `btrtl`, `rtl_bt/rtl8851bu_fw.bin` | yes: `rtw89` of Linux 6.18 as a module ([wlan.md](wlan.md), ADR 0027) |

Another stick works when `btusb` knows its id (or it is a Realtek one with the standard Bluetooth interface class)
and its firmware is in `lib/firmware/rtl_bt/` (or the directory its driver loads from). A stick with its own
driver-CD mode needs its id in `tools/hw/oos-usbmode.c`.

## How it runs

1. **Driver CD.** Realtek combo sticks first show up as a mass-storage "DISK" with the Windows driver (`0bda:1a2b`).
   The kernel ignores that mode on purpose. `oos-usbmode` sends the stick a SCSI eject (bulk-only CBW, START STOP
   UNIT with LoEj), as usb_modeswitch does; the stick comes back with its real ids. Nothing is written to it, and
   unplugging it brings the CD mode back.
2. **Driver.** A USB interface of class `e0/01/01` (Bluetooth) makes `oos-bt` load `bluetooth`, `hidp`, `btrtl`,
   `btintel`, `btbcm` and `btusb`. `btrtl` loads the firmware; the log has `RTL: fw version ...`.
3. **Daemons.** With `hci0` there, `oos-bt` starts a system bus of its own (`dbus-daemon`, `/etc/oos-bt/dbus.conf`:
   root only, no service activation) and `bluetoothd` (`/etc/bluetooth/main.conf`, with the scope's name put in for
   `%h`). The adapter is switched on by itself (`AutoEnable`). When the stick goes, `bluetoothd` is stopped.
4. **Pairings** are kept in `/oos/bluetooth` on the data partition (`/var/lib/bluetooth` points there), so a paired
   device stays paired across power cycles. Without the data partition they go to `/tmp/bluetooth`.

A scope without such a stick loads no module and starts no daemon.

| Part | Where |
|---|---|
| kernel | `CONFIG_BT*` in `kernel/config`, modules listed in `rootfs/board/dho/etc/oos-board` |
| firmware | `lib/firmware/rtl_bt/rtl8851bu_fw.bin`, `rtl8851bu_config.bin`: from linux-firmware, unchanged (the config is the file `rtl8761bu_config.bin` that linux-firmware links), with its licence beside it |
| `/bin/oos-usbmode` | `tools/hw/oos-usbmode.c`, static C |
| `/bin/dbus-daemon`, `/bin/bluetoothd`, `/bin/btmon` | `rootfs/build-bluez.sh`: dbus and BlueZ of `rootfs/deps.lock`, static, input profiles only; expat, glib, pcre2 and libintl linked in from Alpine (THIRD-PARTY-LICENSES.md) |
| `/bin/oos-bt` | started by `rcS` (5c2) after the start-up; log in `/tmp/oos-bt.log`, `bluetoothd`'s in `/tmp/bluetoothd.log` |

## By hand

On the shell of the scope (SSH): `btmon` shows every HCI packet, for a device that does not pair; its `-T` gives the
time of each. The image has no `bluetoothctl`: it would bring readline, and oos-ui pairs with a client of its own.
bluetoothd's log is `/tmp/bluetoothd.log`, oos-bt's `/tmp/oos-bt.log`.

A connected mouse or keyboard appears as an input device (`/proc/bus/input/devices`, bus `0005`).
