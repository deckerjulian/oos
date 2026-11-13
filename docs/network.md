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
| MAC address | fixed per SD card: `02:` and 5 bytes of the MD5 of the card's CID (rcS, step 2) |
| Address | none without a cable; with one DHCP (`udhcpc`, 5 tries), else a link-local address 169.254.x.y/16 (RFC 3927: from the MAC, checked with `arping -D`, another one if it is taken; the top bar says `LAN · local`). A computer on the same cable without DHCP takes such an address itself, so it reaches the scope without any set-up. A server that comes later still gives DHCP |

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
| NTP server | **DHCP** | DHCP: the server the DHCP lease names (see below); the picker also lists a few public servers; Other takes a name or an address on the keyboard |
| NTP state | | Off / Waiting for a network link / Asking the server / Synced at 14:30:05 (+0.03 s) / the last failure and the last sync. With DHCP: the server asked and where it came from, `Synced 14:30:05, 192.168.1.1 (DHCP)` or `no answer, pool.ntp.org (no DHCP)` |
| Sync now | | asks the server at once |
| Time zone | UTC | 22 common zones with their daylight saving rules; Other takes a POSIX time zone string |
| Set the clock | | the local date and time on the keyboard, `2026-11-11 14:30` |

**Off by default.** With Time from NTP off, OOS sends nothing to the outside. On, it sends one UDP packet of 48 bytes to
port 123 of the server per sync (and a DNS request for its name to the server from DHCP), once after a link came up and
then every hour.

**NTP server = DHCP.** The DHCP client asks for option 42 (NTP servers, RFC 2132) on eth0 and wlan0 (`udhcpc -O
ntpsrv`); `udhcpc.script` writes the servers of a lease, space separated, to `/tmp/oos-dhcp-ntp.eth0` or
`/tmp/oos-dhcp-ntp.wlan0` and removes the file when the lease ends or names none. Each request takes the first server of
eth0's file, else of wlan0's, and `pool.ntp.org` when there is neither; the files are read at the request, so a new
lease counts from the next one. This is the default for a new settings file (`ntp.server = dhcp`); a file that names a
server keeps it. Time from NTP stays off by default. In a lab whose DHCP server hands out its own NTP server the scope
then asks that one and nothing outside. (A server whose name is the single word `dhcp` cannot be chosen.)

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

### 4.1 The scope by name: mDNS and DNS-SD

The last row of the page: **mDNS** (on by default) and **Name**, which shows where the scope is reached,
`dho924s-204527.local`. A computer on the same network opens the web remote at `http://dho924s-204527.local:8080`,
also when the scope only has a link-local address (169.254.x.y, a cable straight to a computer without DHCP), and
VISA reaches it at `TCPIP::dho924s-204527.local::INSTR`. macOS, iOS, Linux with Avahi and Windows 10 and newer resolve
`.local` names by themselves.

**The name.** At the first start oos-ui makes it from the model and the last six digits of the serial number
(`dho924s-204527`) and writes it to `/oos/system.conf` (`net.name`). The Name picker offers it, the name set, and
Other: the keyboard takes 1 to 63 letters, digits and hyphens (one DNS label, not starting or ending with a hyphen;
upper case is taken as lower case). The name is also the kernel's host name: rcS sets it at power-on and the DHCP
client sends it (`udhcpc -x hostname:`), so a router lists the scope by it. A new name counts for mDNS at once, for the
router from the next lease (the next start, or the next WLAN connection). Without `net.name` (the very first start)
rcS uses `oos`. An SD card moved to another scope keeps its name: choose the one from model and serial in the picker.

**What it answers.** oos-ui has its own responder (`ui/src/mdns.rs`, RFC 6762 and RFC 6763), in a thread of its own
that sleeps in `poll` between packets; there is no avahi on the image. It listens on UDP port 5353 at 224.0.0.251 and,
where the kernel has IPv6, at ff02::fb, on every interface that is up, has an address and takes multicast (eth0, wlan0),
and looks at the interfaces every 2 s: a cable plugged in later, DHCP after the link-local address, a WLAN stick.

| name | type | data |
|---|---|---|
| `<name>.local` | A, AAAA | the addresses of the interface the question came in on |
| `y.x.254.169.in-addr.arpa` (the IPv4 address reversed) | PTR | `<name>.local` |
| `_services._dns-sd._udp.local` | PTR | each service type below that is switched on |
| `_http._tcp.local` | PTR, SRV, TXT | web remote: port 8080, `path=/` |
| `_scpi-raw._tcp.local` | PTR, SRV, TXT | SCPI: port 5555 |
| `_vxi-11._tcp.local` | PTR, SRV, TXT | VXI-11: port 111 |

The instance name a computer lists is `Rigol DHO924S OOS (204527)` (with a name of its own: `Rigol DHO924S OOS
(lab-scope-3)`). A service switched off is not announced; its records get a goodbye. A name of the scope without the
type asked for is answered with an NSEC record of the types it has (no AAAA on an interface without IPv6).

Before it answers, the responder probes for the name three times 250 ms apart and announces its records twice, a second
apart (RFC 6762 §8). It does that again after each change of the name, the services or the interfaces. When another
host already has the name, the scope takes `<name>-2`, `<name>-3` and so on, and the Name row shows the one in use
(the setting stays; a conflict over the instance name gives `... (2)`). The log `/tmp/oos-ui.log` has a line when the
records are announced and one for each conflict:

```
oos-ui: mDNS: dho924s-204527.local on eth0 wlan0, services _http._tcp _scpi-raw._tcp _vxi-11._tcp
oos-ui: mDNS: dho924s-204527.local is taken (dho924s-204527.local type 1), now dho924s-204527-2.local
```

Questions with the unicast-response bit get a unicast answer; a question from a port other than 5353 (a plain DNS
resolver) gets a legacy unicast answer with its id and question and a TTL of at most 10 s, so these work as well:

```
dig @224.0.0.251 -p 5353 dho924s-204527.local
dig @224.0.0.251 -p 5353 -x 169.254.7.9
dns-sd -B _scpi-raw._tcp            (macOS: browse; avahi-browse -r _scpi-raw._tcp on Linux)
dns-sd -G v4v6 dho924s-204527.local
```

**Switching it off.** mDNS = Off (`remote.mdns = off`) sends a goodbye (TTL 0) for every record and closes port 5353;
the scope then answers nothing on it. It is on by default because it only works on the local link: routers do not
forward 224.0.0.251 and ff02::fb, and it sends nothing but the answers, the probes and the announcements. oos-ui also
sends the goodbye when it ends.

**Verified on the host only** (macOS): the packets, compression, probing, conflicts, the answers (unit tests), and the
responder on en0 with mDNSResponder (`dns-sd -G` and `-B` found the test name and its services). **Not verified on
hardware:** the responder on the scope, the host name in the DHCP request.

## 5. `/oos/system.conf`

`key = value` lines, written by oos-ui when a setting changes (one step, then a sync: the scope is switched off at the
mains). Without `/oos` the file is `/tmp/oos-system.conf` and is lost at power-off. A value that does not parse keeps
its default and is named in `/tmp/oos-ui.log`; lines of other keys stay in the file.

```
# OOS system settings: key = value (oos-ui, System menu)
ntp = off
ntp.server = dhcp             # or a server name or address
tz = UTC0
eth.retry_10m = on
remote.web = on
remote.scpi = on
remote.vxi11 = on
remote.telnet = on
remote.mdns = on              # mDNS and DNS-SD (4.1)
net.name = dho924s-204527     # the name on the network, written at the first start
graphics = gpu                # Display > Graphics: gpu or cpu (gpu/README.md)
```
