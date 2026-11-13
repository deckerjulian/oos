# Ethernet, date and time, remote services

How OOS brings up Ethernet, what it does about a link that comes up at 10 Mbit/s, how it sets the clock from the
network, and which services it offers on it. WLAN through a USB stick is in [wlan.md](wlan.md).

The settings of this page live in `/oos/system.conf` (section 5), not in the setups: a saved or recalled setup does not
change them.

## 1. Ethernet

| | |
|---|---|
| MAC | GMAC of the RK3399 (`fe300000`), RMII, 50 MHz reference clock into the MAC (`clock_in_out = "input"`) |
| PHY | Motorcomm YT8512 at MDIO address 0, 10/100 Mbit/s, reset line GPIO3_B7 (`snps,reset-gpio`, 10 ms low, then 50 ms) |
| Kernel | `dwmac-rk` + `stmmac`, PHY driver `motorcomm` (both built in), no PHY interrupt: phylib reads the PHY once a second |
| MAC address | fixed per SD card: `02:` and 5 bytes of the MD5 of the card's CID (rcS, step 2, `oos-mac`); `eth.mac` (section 6) names another |
| Address | none without a cable; with one DHCP (`udhcpc`, 5 tries), else a link-local address 169.254.x.y/16 (RFC 3927: from the MAC, checked with `arping -D`, another one if it is taken; the top bar says `LAN · local`). A computer on the same cable without DHCP takes such an address itself, so it reaches the scope without any set-up (with `net.allow` only if 169.254.0.0/16 is listed, section 6). A server that comes later still gives DHCP |

System > Network shows the address of eth0 and the speed of the link (`192.168.1.20 · 100 Mbit/s`).

## 2. A link at 10 Mbit/s

A user reported that Ethernet came up at 10 Mbit/s after every cold start, and at 100 Mbit/s every time after
unplugging and plugging the cable in again.

**What OOS does (System > Network > 10 Mbit retry, on by default).** oos-ui reads `/sys/class/net/eth0/carrier` and
`speed` once a second. When a link has been up at 10 Mbit/s for 3 s, it restarts the autonegotiation once: the
ethtool request `ETHTOOL_NWAY_RST` (what `ethtool -r eth0` sends), which sets the restart bit of the PHY's control
register. The link drops for about two seconds and comes back with whatever the two sides agree on. It does not take
the interface down (`ip link set eth0 down` would delete the default route, and the DHCP client of rcS is gone by then),
so addresses and routes stay. Nothing goes out to the network: it only concerns the cable to the next switch. A DHCP
request of rcS that falls into the break is answered at one of its next tries, at the latest by the try every 30 s that
follows the fallback address.

Limits, so that a partner that really runs 10 Mbit/s sees no loop:

- one restart per link-up; a link that comes up within 20 s after a restart is its result and gets none,
- at most 3 restarts per start of oos-ui (a cable plugged in again later counts as a new link-up).

The log `/tmp/oos-ui.log` shows each restart with the PHY's registers before it, and the result:

```
oos-ui: eth0 up at 10 Mbit/s full, restarting the autonegotiation (1 of 3; phy 0 bmcr=1100 bmsr=786d anar=01e1 anlpar=0061 aner=0001)
oos-ui: eth0 after the restart: 100 Mbit/s full (phy 0 bmcr=3100 bmsr=786d anar=01e1 anlpar=45e1 aner=0001)
```

(The register values above show the format, they are not a measurement.) `anar` is what the scope offers (`01e1`:
10 and 100, half and full duplex), `anlpar` what the partner offered, `aner` bit 0 whether the partner negotiates at all.
With `anlpar` 0 or only 10 Mbit bits, the partner either did not negotiate (parallel detection) or offered only 10.

**Not verified on hardware:** the restart, and whether it ends at 100 Mbit/s where a replug does. If it does not, the
log lines above are what is needed to find out why.

### 2.1 Possible causes in the kernel (not verified on hardware)

Read from the sources of the OOS kernel (6.12 with Rockchip's tree), not measured:

1. **Auto sleep of the PHY.** The YT8512 driver (`yt8512_config_init`) switches the PHY's auto sleep off (extended
   register 0x2027 bit 15), but only when eth0 is brought up (`ip link set eth0 up` in rcS), seconds after power-on.
   Until then the PHY runs with its power-on defaults. If its link pulses in that state look like plain 10BASE-T link
   pulses to the switch, the switch picks 10 Mbit/s by parallel detection and keeps it while the link stays up. A replug,
   and probably the restart above, makes both sides negotiate from the start. This fits the report (every cold start,
   never after a replug) best. A check: `aner` bit 0 and `anlpar` in the log line of a cold start.
2. **No soft reset with this clock set-up.** `yt8512_clk_init` returns early when the MAC's clock is `"input"`, as in the
   OOS device tree. Then the PHY gets no soft reset after the driver's settings, and the negotiation that started at
   the hardware reset (at boot, when the MDIO bus registers, long before eth0 is up) stays, unless phylib finds the
   advertisement register different from what it wants (pause bits, for example) and restarts it when eth0 comes up.
3. **Speed read before it is resolved.** `yt8512_read_status` takes the link from the standard status register and the
   speed from the PHY-specific status register (0x11, bits 15:14), without looking at that register's "speed and
   duplex resolved" bit (bit 11) or its own link bit (bit 10); the YT8521 code takes link and speed from register 0x11
   together. (That the YT8512 has the same bits as the YT8521 is assumed from the driver.) Read in the moment the link
   comes up, the field may still say 10 Mbit/s. phylink sets the MAC from the speed at link-up and does not change it
   while the link stays up, while `/sys/class/net/eth0/speed` follows the PHY at every read. This would rather show as a
   link that does not work than as a slow one, and the retry of this section would not see it.
4. **A stray register write.** `yt8512_led_init` writes the LED1 value to extended register 0x0010
   (`YT8512_LED1_BT_ON_EN` used as the address) instead of 0x40c3. What register 0x0010 of the YT8512 does is not
   known here.
5. **Reset timing.** The reset pulse comes from the deprecated `snps,reset-gpio` of the MAC node (10 ms low, 50 ms
   wait). Whether the YT8512 needs a longer wait after reset is not known; `reset-gpios` with `reset-assert-us` and
   `reset-deassert-us` in the PHY node would be the current way.

Next steps on a unit with the problem: the two log lines of a cold start; `dmesg | grep -i -e eth0 -e yt85` (driver
name, speed at link-up); what the switch shows for the port before and after the restart. If the restart ends at
10 Mbit/s again where a replug gives 100, the next thing to try is a longer break: the PHY's power-down bit for a second
or two, which the switch sees like a pulled cable. It is not done now because it is not known whether the YT8512 keeps
its clock outputs running in power-down.

## 3. Date and time

The DHO800/DHO900 keeps no time while it is off: without an RTC battery the clock starts in 2013 at every power-on.
System > Date and time sets it from an NTP server or by hand.

| Row | Default | What |
|---|---|---|
| Now | | local date and time, and the zone; "(not set)" until NTP or a hand entry set the clock in this boot |
| Time from NTP | **Off** | On: the clock is set from the server once a network link is up |
| NTP server | `pool.ntp.org` | the picker lists a few public servers; Other takes a name or an address on the keyboard |
| NTP state | | Off / Waiting for a network link / Asking the server / Synced at 14:30:05 (+0.03 s) / the last failure and the last sync |
| Sync now | | asks the server at once |
| Time zone | UTC | 22 common zones with their daylight saving rules; Other takes a POSIX time zone string |
| Set the clock | | the local date and time on the keyboard, `2026-11-11 14:30` |

**Off by default.** With Time from NTP off, OOS sends nothing to the outside. On, it sends one UDP packet of 48 bytes to
port 123 of the server per sync (and a DNS request for its name to the server from DHCP), once after a link came up and
then every hour.

**How a sync works.** oos-ui has its own SNTP client (`ui/src/sntp.rs`, RFC 4330: version 4, client mode, one request and
one answer, the originate time checked, kiss-o'-death and unsynchronised servers refused); the busybox of the image has
no `ntpd`. A worker thread does the exchange, so the screen never waits on the network. The clock is stepped
(`clock_settime`) when it is off by 50 ms or more. When a link comes up (Ethernet with a cable and an address, or WLAN
with an address), the first request goes out 3 s later, after DHCP set the route and the name server. A failure is
tried again after 15 s, 30 s, 1 min and so on up to every 15 min; a success again after an hour. The log
`/tmp/oos-ui.log` has a line per sync with the server's address, its stratum, the offset and the delay.

**Time zone.** A POSIX `TZ` string: `std offset [dst [offset] [,start[/time],end[/time]]]`, the offset counted west of
Greenwich as in POSIX. Examples: `CET-1CEST,M3.5.0,M10.5.0/3` (Berlin), `EST5EDT,M3.2.0,M11.1.0` (New York),
`IST-5:30` (India), `<+04>-4` (4 hours east, no daylight saving time). `Mm.w.d` is day d (0 = Sunday) of week w (5 = the
last) of month m; the start time is in standard time, the end time in daylight saving time, 02:00 if left out. Without
rules, daylight saving time follows the US rules. The zone applies to everything the screen shows and to file names.
oos-ui also gives the current offset to the kernel (`settimeofday` with a zone only), which uses it for the time stamps
on a FAT stick: those are local time, so a computer shows them right. exFAT and ext4 keep UTC.

**File names.** Once the clock was set in this boot (by NTP or by hand), new files carry the local date and time:
`shot-20261111-143005.png`, `wave-20261111-143005.csv`, `la-...`, `log-...`, `events-...`, with `-2`, `-3` for a second
file in the same second. Before that they are numbered as before (`shot-001.png`). A file `/tmp/oos-clock.set` tells a
restarted oos-ui that the clock is right. The web page names its downloads the same way (`oos-20261111-143005.png`):
it asks the scope for its local time (`/stamp`), and takes the computer's own time while the scope's clock is not set.

**Not verified on hardware:** setting the clock, the sync over Ethernet and WLAN, DNS through the name server of DHCP,
the zone for FAT time stamps. Verified on the host: the SNTP packets, an exchange with a local server, the time zone
rules, the schedule, the settings file, the page.

## 4. Remote services

System > Network > Remote services has a switch for each service, next to where it is reached:

| service | port | what goes with it |
|---|---|---|
| Web remote | 8080 | live picture, keys and touch, downloads, LXI identification, MCP, **the update page** |
| SCPI | 5555 (TCP) | the SCPI commands |
| VXI-11 | 111 (portmapper, TCP and UDP) and 9009 | SCPI through VISA (LXI instruments) |
| telnet | 23 | a shell **without password** (development image) |

All are on by default. A service switched off closes its port at once, with the connections on it. Switching off the
web remote also takes away the update page: an update then comes from a USB stick. telnet is started by `rcS` at
power-on, which reads `remote.telnet` from `/oos/system.conf`; oos-ui ends or starts it when the switch changes.

## 5. `/oos/system.conf`

`key = value` lines, written by oos-ui when a setting changes (one step, then a sync: the scope is switched off at the
mains). Without `/oos` the file is `/tmp/oos-system.conf` and is lost at power-off. A value that does not parse keeps
its default and is named in `/tmp/oos-ui.log`; lines of other keys stay in the file.

```
# OOS system settings: key = value (oos-ui, System menu)
ntp = off
ntp.server = pool.ntp.org
tz = UTC0
eth.retry_10m = on
remote.web = on
remote.scpi = on
remote.vxi11 = on
remote.telnet = on
graphics = gpu                # Display > Graphics: gpu or cpu (gpu/README.md)
```

## 6. Firewall, remote access, telnet and the MAC address

Three keys of the system settings that rcS reads before the network comes up (`oos-conf`). Each one is looked up in
`/etc/oos/system.conf` first (the defaults of an image built for one unit: `OOS_UNIT`, docs/models.md 4.2), then in
`/oos/system.conf`; the last line wins, also one with an empty value. oos-ui keeps these lines when it writes the file.
Without `/etc/oos/system.conf` and without the keys everything is as before. The same image default applies to the keys
of section 5 (oos-ui and rcS read it first), so a unit image can ship `remote.telnet = off`: no telnetd at power-on, and
oos-ui does not start it either until System > Network > Remote services switches it on.

| Key | Default | What |
|---|---|---|
| `net.allow` | (none) | address prefixes, `192.0.2.0/24 fd00::/8`. Firewall: inbound only from them; oos-ui: `POST /update`, `/mcp`, SCPI and VXI-11 only from them |
| `eth.mac` | `cid` | `cid`: from the card's CID as before; `vendor`: the LAN MAC in vendor.bin, as the stock system uses it; or an address `02:00:00:00:00:01` |

**Firewall** (`oos-firewall`, nftables, table `inet oos_fw`, before telnetd and oos-ui start). Inbound passes: loopback,
answers to what the scope sent (DHCP, DNS, NTP; connection tracking), IPv6 neighbour discovery, DHCP replies, and
anything from a listed prefix. The rest is dropped, on Ethernet and WLAN alike. Outbound is not limited. An entry that
does not parse is left out and logged (`/tmp/oos-net.log`, kernel log); a list of which nothing parses closes everything
but loopback rather than opening it. `nft` is a static build in the image (`rootfs/build-nft.sh`, nftables 1.1.3).
The link-local fallback address (169.254.x.y) is only reachable from a listed prefix: add `169.254.0.0/16` to
`net.allow` for a computer on a direct cable.

**oos-ui** reads `net.allow` once at start and applies it a second time per connection: `POST /update` and `/mcp`
answer 403 to others, SCPI and VXI-11 close the connection. The page, the live picture, `/cmd` and the downloads stay
open to the firewall's prefixes. Loopback is always allowed.

**Telnet on again** with `remote.telnet = off` in the image: System > Network > Remote services > telnet, or
`remote.telnet = on` in `/oos/system.conf`. Or build the image without that line.

**MAC**: an address that is not unicast or not six colon-separated bytes falls back to `cid` (logged). `vendor` asks the board
(`oos-board mac`; on the DHO800/DHO900 `oos-fpgad model`, which needs the keys of vendor.bin, docs/models.md 1); without
them it falls back to `cid` as well.

Not verified on hardware: all of this section. Verified on the host: the ruleset loads into a Linux kernel with the
static `nft` (arm64 container), `oos-conf` and `oos-mac` under busybox, the address lists of oos-ui (`netacl` tests).
