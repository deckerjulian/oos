# Ethernet, date and time

How OOS brings up Ethernet, what it does about a link that comes up at 10 Mbit/s, and how it sets the clock from the
network. WLAN through a USB stick is in [wlan.md](wlan.md).

The settings of this page live in `/oos/system.conf` (section 4), not in the setups: a saved or recalled setup does not
change them.

## 1. Ethernet

| | |
|---|---|
| MAC | GMAC of the RK3399 (`fe300000`), RMII, 50 MHz reference clock into the MAC (`clock_in_out = "input"`) |
| PHY | Motorcomm YT8512 at MDIO address 0, 10/100 Mbit/s, reset line GPIO3_B7 (`snps,reset-gpio`, 10 ms low, then 50 ms) |
| Kernel | `dwmac-rk` + `stmmac`, PHY driver `motorcomm` (both built in), no PHY interrupt: phylib reads the PHY once a second |
| MAC address | fixed per SD card: `02:` and 5 bytes of the MD5 of the card's CID (rcS, step 2) |
| Address | DHCP (`udhcpc`, 5 tries), else 192.168.177.64/24; a cable or a server that comes later still gets DHCP |

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
restarted oos-ui that the clock is right.

**Not verified on hardware:** setting the clock, the sync over Ethernet and WLAN, DNS through the name server of DHCP,
the zone for FAT time stamps. Verified on the host: the SNTP packets, an exchange with a local server, the time zone
rules, the schedule, the settings file, the page.

## 4. `/oos/system.conf`

`key = value` lines, written by oos-ui when a setting changes (one step, then a sync: the scope is switched off at the
mains). Without `/oos` the file is `/tmp/oos-system.conf` and is lost at power-off. A value that does not parse keeps
its default and is named in `/tmp/oos-ui.log`; lines of other keys stay in the file.

```
# OOS system settings: key = value (oos-ui, System menu)
ntp = off
ntp.server = pool.ntp.org
tz = UTC0
eth.retry_10m = on
```
