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
| NTP server | **DHCP** | DHCP: the server the DHCP lease names (see below); the picker also lists a few public servers; Other takes a name or an address on the keyboard |
| NTP state | | Off / Waiting for a network link / Asking the server / Synced at 14:30:05 (+0.03 s) / the last failure and the last sync. With DHCP: the server asked and where it came from, `Synced 14:30:05, 192.168.1.1 (DHCP)` or `no answer, pool.ntp.org (no DHCP)` |
| Sync now | | asks the server at once |
| Time zone | UTC | DHCP: the zone the DHCP lease names (see below), shown with its source, `CET (DHCP)` or `CET (no DHCP)`; 22 common zones with their daylight saving rules; Other takes a POSIX time zone string |
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

**Time zone = DHCP** (`tz = dhcp`). The DHCP client also asks for option 100 (a POSIX `TZ` string, RFC 4833; `udhcpc
-O tzstr`); `udhcpc.script` writes it as it came to `/tmp/oos-dhcp-tz.eth0` or `.wlan0`, never runs it. oos-ui takes
eth0's string, else wlan0's, only when it is a valid zone (ASCII, at most 60 characters, the full POSIX grammar of
`clock::Tz::parse`); otherwise `tz.fallback` (default `UTC0`). It looks again once a minute, so a new lease counts within
a minute, and logs a change. Option 101 (a tz database name such as `Europe/Amsterdam`) is not asked for: the image has
no zoneinfo to turn a name into rules. Other on the keyboard starts from the fallback.

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
| SSH | 22 | a root shell with the scope's own password or a trusted key (section 7); **SSH settings** next to it |

Web remote, SCPI and VXI-11 are on by default, SSH is off unless the image or the user switches it on. A service
switched off closes its port at once, with the connections on it. Switching off the web remote also takes away the
update page: an update then comes from a USB stick. SSH is started by `rcS` at power-on (`/bin/oos-ssh`), which reads
`remote.ssh` from `/oos/system.conf`; oos-ui starts or stops it when the switch changes.

This fork ships no telnet: rcS does not start `telnetd`, the switch is gone, and a `remote.telnet` line in
`/oos/system.conf` is kept but starts nothing. The serial console (UART2) keeps its shell without a login.

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
| `<name>.local` | A | the address of the interface the question came in on (no AAAA: the web remote, SCPI and VXI-11 listen on IPv4 only, and a computer would try the IPv6 address first) |
| `y.x.254.169.in-addr.arpa` (the IPv4 address reversed) | PTR | `<name>.local` |
| `_services._dns-sd._udp.local` | PTR | each service type below that is switched on |
| `_http._tcp.local` | PTR, SRV, TXT | web remote: port 8080, `path=/` |
| `_scpi-raw._tcp.local` | PTR, SRV, TXT | SCPI: port 5555 |
| `_vxi-11._tcp.local` | PTR, SRV, TXT | VXI-11: port 111 |
| `_ssh._tcp.local` | PTR, SRV, TXT | SSH (this fork, section 7): port 22, only while oos-ssh listens (`remote.ssh = on`, `net.allow` set, firewall loaded) |

The instance name a computer lists is `Rigol DHO924S OOS (204527)` (with a name of its own: `Rigol DHO924S OOS
(lab-scope-3)`). A service switched off is not announced; its records get a goodbye. A name of the scope without the
type asked for is answered with an NSEC record of the types it has (A, so a question for AAAA gets a quick "none").

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

**With the fork's firewall** (section 6): mDNS questions come in from the asking computer's own address, so a
computer inside `net.allow` reaches the responder and one outside does not, like every other service. Questions over
IPv6 (from fe80:: addresses) are dropped unless `fe80::/10` is listed; computers ask over IPv4 as well, and the
responder has no AAAA anyway. With mDNS the web remote is at `http://<name>.local:8080` (the web server's
rebinding guard takes `.local` host names); the unit's name is `dho924s-<last six digits of the serial>`.

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
tz = UTC0                     # or dhcp (section 3)
tz.fallback = UTC0            # with tz = dhcp, while no lease names a valid zone; written when not UTC0
eth.retry_10m = on
remote.web = on
remote.scpi = on
remote.vxi11 = on
remote.ssh = off
ssh.password = on
remote.mdns = on              # mDNS and DNS-SD (4.1)
net.name = dho924s-204527     # the name on the network, written at the first start
graphics = gpu                # System > Graphics: gpu or cpu (gpu/README.md)
```

## 6. Firewall, remote access and the MAC address

Three keys of the system settings that rcS reads before the network comes up (`oos-conf`). Each one is looked up in
`/etc/oos/system.conf` first (the defaults of an image built for one unit: `OOS_UNIT`, docs/models.md 4.2), then in
`/oos/system.conf`; the last line wins, also one with an empty value. oos-ui keeps these lines when it writes the file.
Without `/etc/oos/system.conf` and without the keys everything is as before. The same image default applies to the keys
of section 5 (oos-ui and rcS read it first), so a unit image can ship `remote.ssh = on`: SSH at power-on without a
setting on the scope.

| Key | Default | What |
|---|---|---|
| `net.allow` | (none) | address prefixes, `192.0.2.0/24 fd00::/8`, and the word `onlink6` for the IPv6 prefixes on the link (section 8). Firewall: inbound only from them; oos-ui: `POST /update`, `/mcp`, SCPI and VXI-11 only from them |
| `net.ipv6` | `off` | `on`: stable IPv6 addresses, the services and mDNS on IPv6 too (section 8) |
| `eth.mac` | `cid` | `cid`: from the card's CID as before; `vendor`: the LAN MAC in vendor.bin, as the stock system uses it; or an address `02:00:00:00:00:01` |

**Firewall** (`oos-firewall`, nftables, table `inet oos_fw`, before SSH and oos-ui start). Inbound passes: loopback,
answers to what the scope sent (DHCP, DNS, NTP; connection tracking), IPv6 neighbour discovery, DHCP replies, and
anything from a listed prefix. The rest is dropped, on Ethernet and WLAN alike. Outbound is not limited. An entry that
does not parse is left out and logged (`/tmp/oos-net.log`, kernel log); a list of which nothing parses closes everything
but loopback rather than opening it. `nft` is a static build in the image (`rootfs/build-nft.sh`, nftables 1.1.3).
The link-local fallback address (169.254.x.y) is only reachable from a listed prefix: add `169.254.0.0/16` to
`net.allow` for a computer on a direct cable.

**oos-ui** reads `net.allow` once at start and applies it a second time per connection: `POST /update` and `/mcp`
answer 403 to others, SCPI and VXI-11 close the connection. The page, the live picture, `/cmd` and the downloads stay
open to the firewall's prefixes. Loopback is always allowed.

**MAC**: an address that is not unicast or not six colon-separated bytes falls back to `cid` (logged). `vendor` asks the board
(`oos-board mac`; on the DHO800/DHO900 `oos-fpgad model`, which needs the keys of vendor.bin, docs/models.md 1); without
them it falls back to `cid` as well.

Not verified on hardware: all of this section. Verified on the host: the ruleset loads into a Linux kernel with the
static `nft` (arm64 container), `oos-conf` and `oos-mac` under busybox, the address lists of oos-ui (`netacl` tests).

## 7. SSH

Dropbear 2026.94 (`rootfs/build-dropbear.sh`: static, arm64, built in Docker, the tarball checked against a pinned
SHA-256 that matches Alpine's SHA-512; no sftp-server, no X11 or agent forwarding). `/bin/oos-ssh` starts it from rcS
after the firewall, in the background, and oos-ui restarts or stops it when a setting changes. Log: `/tmp/oos-ssh.log`.

**When it listens.** Only with `remote.ssh = on`, a non-empty `net.allow` (section 6) and the firewall table loaded.
Without `net.allow`, or when `nft` refused the ruleset, port 22 stays closed (state `off: needs net.allow`,
`off: firewall not loaded`), so SSH never opens to a network that was not named. Built-in default: off. The DHO804 unit
image sets `remote.ssh = on` together with its `net.allow`, so that unit has SSH from its first start: on a LAN that is
already limited to one prefix, a random password is an acceptable first login, and it is what lets `ssh-copy-id` add the
first key.

**First start.** The scope makes, once, in `/oos/ssh` on the data partition (kept over updates and power-offs; without
the partition in `/tmp/oos-ssh`, made again at every start):

| file | what |
|---|---|
| `password` | the root password: 16 characters from 31 that cannot be mixed up (no i, l, o, 0, 1), about 79 bits, from `/dev/random`, in four groups: `k7m2-p9xq-4tnv-hc8w` |
| `dropbear_ed25519_host_key`, `dropbear_ecdsa_host_key` (and `.pub`) | the host keys |
| `authorized_keys` | the trusted keys, empty at first |

The folder is `0700`, the files `0600`. At every start oos-ssh writes `/etc/passwd`, `/etc/shadow` (SHA-512 crypt of the
password) and `/etc/shells` into the RAM root, and links `/root/.ssh` to the SSH folder. Root is the only user: the
image has no other accounts, and every OOS program runs as root.

**System > Network > Remote services.** The SSH switch stands with the other services; **SSH settings** next to it
opens:

| row | what |
|---|---|
| State | what oos-ssh did: `on: password and keys`, `on: keys only`, `on: password kept, no key`, `off: ...` |
| Password login | On: password or key. Off: keys only. **Refused while no trusted key is there** ("Import a trusted key first: password login off without one locks root out"). oos-ssh checks the same at every start: with `ssh.password = off` and no key line in the file it keeps password login on |
| Show password | a dialog with `ssh root@<address>`, the password and the host key fingerprint; closed by a tap. The password is never on the page itself. The web remote mirrors the screen, so a viewer of the web remote sees the dialog while it is open |
| Host key | the start of the ed25519 host key's SHA-256 fingerprint, the same that `ssh` shows at the first connection |
| Trusted keys | how many |
| List keys | comment and fingerprint of each key |
| Import keys from USB | every `*.pub` file and `authorized_keys` in the stick's top folder and in `OOS/`: plain keys (ed25519, ECDSA, RSA from 2048 bits, FIDO `sk-` keys), each once, at most 100. Lines with options (`from=`, `command=`) and private keys are not taken; the toast says how many were added, already trusted, or refused and why |
| Remove a key | a picker of the keys. **The last key cannot go while password login is off** |

Changes to the keys apply to the next login; Dropbear reads the file at every one. Password login on or off restarts the
listener: sessions stay. SSH off ends the listener and every session.

**From a computer**, with the password from Show password:

```sh
ssh root@192.0.2.10
ssh-copy-id root@192.0.2.10            # adds ~/.ssh/id_*.pub to /oos/ssh/authorized_keys (through /root/.ssh)
ssh root@192.0.2.10 sync               # ext2 without a journal: flush before the scope is switched off
scp -O file root@192.0.2.10:/tmp/      # -O: the scp protocol; there is no sftp-server
```

Then Password login > Off, if keys only is wanted.

**Not over the web remote.** Key management stays on the scope's screen and the stick. The web remote has no login: a
key page on port 8080 would let anyone in `net.allow` add a key and get a root shell without the password.

**A new password**: `rm /oos/ssh/password` and power-cycle. New host keys: remove `/oos/ssh/dropbear_*` the same way;
clients then warn about a changed host key.

Not verified on hardware: all of this section. Verified on the host (`tools/ssh-sim.sh`, arm64 container with the
image's busybox, `nft` and Dropbear, an OpenSSH client): the guards (off, no `net.allow`, no firewall), the files and
their modes, password and key login, a wrong or empty password refused, `ssh-copy-id`, `scp -O`, keys only, the
lockout guard of the script, password and host key unchanged after a restart, restart keeping sessions and stop ending
them. The rules and the page in `ui` tests (`ssh::tests`, `ui::ssh::tests`).

## 8. IPv6

Off by default (`net.ipv6`, section 6): then the services listen on IPv4 only, as upstream, and the kernel makes its
usual SLAAC addresses. The DHO804 unit image sets `net.ipv6 = on` and `net.allow = 192.0.2.0/24 169.254.0.0/16
fe80::/10 onlink6`.

**Addresses.** The kernel takes them from the router's advertisements (SLAAC; the firewall lets router advertisements
in). With `net.ipv6 = on`, `oos-net6` sets before eth0 comes up: stable-privacy interface identifiers (RFC 7217,
`addr_gen_mode = 2`) from a 128-bit secret made once and kept in `/oos/net/ipv6.secret`, no temporary addresses
(`use_tempaddr = 0`), and `bindv6only = 0`. The address for a prefix is the same at every start and does not carry the
MAC, so it can go into DNS. A new card or `rm /oos/net/ipv6.secret` gives a new one. The secret is synced to the card
as soon as it is made: `/oos` is ext2 without a journal, and the first version left it in the page cache, so a reboot
soon after the first start lost it and every start made new addresses. `/tmp/oos-net.log` says `secret made` or
`secret kept`. A WLAN stick gets the same mode.
There is no DHCPv6. `udhcpc.script` flushes only the IPv4 addresses of an interface: it used to flush all, which took
the IPv6 link-local address with it (the kernel makes that one only when the link comes up), so the scope had no IPv6
at all after its first DHCP lease.

**DNS.** The name servers stay those of the IPv4 lease. The kernel hands RDNSS (RFC 8106) from a router advertisement
only to a program that listens for it, and the image has none; on a network with IPv4 that changes nothing.

**Services.** The web remote (8080), SCPI (5555) and VXI-11 (111, 9009) listen on `[::]`, which takes IPv4 too
(`net6.rs`; an IPv4 peer appears as `::ffff:a.b.c.d`, which `net.allow` maps back). Dropbear (22) listens on both
anyway. mDNS publishes an AAAA record for each IPv6 address of the interface asked on, link-local included, and
answers questions over IPv6 as before. Without `net.ipv6` it publishes none (a computer tries IPv6 first). The web
remote's guard against DNS rebinding takes an IPv6 literal, `http://[2001:db8::5]:8080`.

**Firewall.** IPv6 follows the IPv4 policy: inbound only from link-local (`fe80::/10`) and from the prefixes the router
announces on the LAN (`onlink6`), plus neighbour discovery, router advertisements and the answers to what the scope sent
(connection tracking, which also passes ICMPv6 errors such as packet too big). A global address of the scope is not
reachable from the internet, whatever the router lets through. `onlink6` is the routes without a gateway that the
kernel made from router advertisements, /48 to /128, without `fe80::/64`: a shorter on-link prefix (`2000::/3` from a
rogue router) is ignored. They live in the nft set `onlink6` of table `inet oos_fw`; `oos-firewall watch` (started by
rcS, busybox `ip` has no monitor) checks them every 10 s, replaces the set when they changed, logs it
(`/tmp/oos-net.log`) and writes them to `/tmp/oos-net6.prefixes`, from which oos-ui's second check (section 6) reads them
per connection. Until the first advertisement only link-local is allowed.

**The router** must send router advertisements with the LAN prefix (on-link and autonomous flags set) for addresses;
RDNSS in them is not used. For the time zone the DHCPv4 server sends option 100 with a POSIX string, on Kea option
`pcode`:

```json
"option-data": [ { "name": "pcode", "data": "CET-1CEST,M3.5.0,M10.5.0/3" } ]
```

Not verified on hardware. Verified on the host: the prefixes taken from a route list, the ruleset, the set loaded into
a Linux kernel and kept up to date by `watch`, membership of on-link, routed and internet addresses, the lease's time
zone file, a DHCP lease that keeps the IPv6 addresses, a hostile option string that runs nothing, the secret
(`tools/fw-sim.sh`, arm64 container with the image's busybox and `nft`); the zone's validation and source on the page,
`onlink6` in oos-ui, AAAA records, dual-stack sockets and the web guard in `ui` tests.
