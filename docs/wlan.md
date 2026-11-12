# WLAN through a USB stick

The unit has no built-in WLAN. OOS joins a network through a stick in the front USB port (host port 3-1, controller
fe900000, `protocols/frontpanel.md`).

Tested without hardware: wpa_supplicant, `oos-wlan`, `udhcpc.script` and the join code of oos-ui ran against access points
on simulated radios (`tools/wlan-sim.sh`, section 6). **Not yet verified on hardware:** the stick's driver and firmware
under OOS, and the front USB port with kernel 6.12.

## 1. What works

| | |
|---|---|
| Sticks | RTL8188EU (USB id 0bda:8179 and the other ids of the kernel driver `rtl8xxxu` that need `rtl8188eufw.bin`) |
| Band | what the stick has: 2.4 GHz for the RTL8188EU |
| Networks | open, WPA and WPA2 personal, WPA2/WPA3 mixed, hidden names |
| Not supported | WPA3 only, enterprise (802.1X), WEP. The list says so, a join is refused |
| Address | DHCP only |

Another chip of `rtl8xxxu` needs its firmware file under `rootfs/initramfs/lib/firmware/rtlwifi/`, and some need the kernel
option `RTL8XXXU_UNTESTED`. Another driver needs its modules in `boot/build-dev-image.sh` and in `oos-wlan`.

## 2. Use

The network box at the top right, or System > Network:

- **Find WLAN networks:** scans for a few seconds, then lists the 17 strongest names with their signal. Tap one and enter the
  password. A saved network joins without one. **Other network** takes a hidden name, then a password (empty: open).
- **Forget WLAN network:** lists the saved networks, a tap removes one.
- **WLAN details:** connection, signal, address, MAC of the stick.

The page also shows the state and speed of Ethernet, its MAC, the address of the web remote and the other ways in, and
the 10 Mbit retry ([network.md](network.md)).

The keyboard takes every password character: Shift for one capital, `#+=` for the symbols, Show for the plain text. Touch
or any knob work (turn to choose, push to press).

A network is saved only after a join worked. A wrong password shows as "wrong password?", usually within seconds; a network that
does not answer after 30 s. Saved networks join by themselves at every start. The top bar shows `WLAN` and the address
when the network runs over the stick. With both, Ethernet stays the default route.

## 3. Parts

| Part | Where | Does |
|---|---|---|
| kernel | `cfg80211`, `mac80211`, `rtl8xxxu`, `libarc4` modules | already in the base kernel, loaded by `oos-wlan` |
| firmware | `lib/firmware/rtlwifi/rtl8188eufw.bin` | from linux-firmware, unchanged, with its licence beside it |
| `/bin/wpa_supplicant`, `/bin/wpa_cli` | `rootfs/build-wlan-tools.sh`, into `rootfs/out/` | version 2.11, static, internal crypto, no EAP, no SAE, no P2P |
| `/bin/oos-wlan` | started by `rcS` (5c) | driver, wpa_supplicant, DHCP, section 4 |
| `oos_hal::wlan` | `hal/src/wlan.rs` | control socket, parsers, key derivation |
| GUI | `ui/src/ui/wlan.rs` | worker thread, menu, keyboard |

## 4. `oos-wlan`

Waits for `end` or `abort` in the start-up status (at most 60 s), so nothing of this runs beside the acquisition bring-up.
Then every 2 s:

1. A USB device there and no driver: load the modules. A scope without a stick never loads them.
2. `wlan0` there: wpa_supplicant runs, started again if it died.
3. Joined a network: `udhcpc` runs in the background and renews the lease. Moving between access points of one network
   keeps the address; a lost link or another network drops it.
4. `wlan0` gone (stick pulled): wpa_supplicant and DHCP stop.

`ignore_routes_with_linkdown` is set, so that a route of eth0 whose cable was pulled does not take the traffic of a WLAN
in that same network. `wlan0` gets route metric 20.

Logs: `/tmp/oos-wlan.log`, `/tmp/wpa_supplicant.log`. By hand: `wpa_cli -p /run/wpa_supplicant -i wlan0 status`.

## 5. Stored data

`/oos/wlan/wpa_supplicant.conf`, mode 0600, written by wpa_supplicant (SAVE_CONFIG) followed by a sync. It holds the
names and the 256-bit key derived from each password, not the password. That key is enough to join that network, so
treat the card as you would the password. Without `/oos` the file is in `/tmp/wlan` and is lost at power-off.

oos-ui passes only hexadecimal forms to wpa_supplicant (SSID bytes, the key), so a name or password cannot break a command
or the file. Names from a scan are untrusted: they are cut, and control characters are replaced on the screen.

**Everything open on Ethernet is open on the WLAN too:** telnet without a password, SCPI, VXI-11, the web remote, MCP and the
update page. Join only networks in which you trust every device.

## 6. Test

`tools/wlan-sim.sh` (Docker and qemu-system-aarch64) boots Linux 6.12.115 in a virtual machine with four `mac80211_hwsim`
radios and three access points on hostapd, then runs the test `live_radio` of `ui/src/ui/wlan.rs` with the image's busybox,
wpa_supplicant, `oos-wlan` and `udhcpc.script`:

scan with security types and no hidden name, a wrong password (found, not saved), WPA2 with `"`, `#`, a space and `\` in
the password, DHCP, the file holding the key and not the password, a second network, back without a password, a hidden
network, forgetting the current one and falling back to a saved one, wpa_supplicant killed and rejoining by itself, the stick
pulled.

Not covered: `rtl8xxxu`, the firmware load, the USB port, radio conditions, the on-screen steps on the panel (unit tests only).

## 7. To check on the unit

1. Stick in the front port, start OOS. `/tmp/oos-wlan.log`: "driver loaded", "wpa_supplicant runs". `dmesg`: firmware loaded.
2. Network box > Find WLAN networks, join a WPA2 network; Status shows the address; SCPI and the web remote work over it.
3. A wrong password; a pulled and replugged stick; a power cycle (joins by itself).
4. Stock had sticks missing after some reboots: count how often it happens here.
5. Acquisition rate with WLAN traffic against without (USB interrupts on the same SoC).
