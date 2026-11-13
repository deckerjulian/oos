# Time zone from DHCP, IPv6 on the unit

Branch `net/tz-ipv6`. Working file: fold into docs/network.md and docs/fork/unit-dho804.md when merged, then delete.

## Plan

Time zone from DHCP (all images; the default stays `tz = UTC0`):

- udhcpc asks `-O tzstr` (option 100, busybox 1.37 name, checked in the image's busybox). udhcpc.script writes
  `$tzstr` to `/tmp/oos-dhcp-tz.<iface>` like the NTP servers. Option 101 (`tzdbstr`) is not used: the image has no
  zoneinfo to map a tz database name.
- sysconf: `tz = dhcp` takes the lease's string; `tz.fallback` (POSIX, default `UTC0`) when no lease has a valid one.
  `Tz::parse` validates (ASCII, 60 characters, full POSIX grammar) before use: the string comes from the network.
- time.rs: the zone in use is resolved with the lease at load, every minute and on change; the page shows the zone and
  its source (`CET (DHCP)`, `CET (no DHCP)`), the picker gets a DHCP entry.
- Unit: `tz = dhcp`, `tz.fallback = CET-1CEST,M3.5.0,M10.5.0/3`.

IPv6 (only with `net.ipv6 = on`, unit image on; other images unchanged):

- rcS before eth0 comes up: a stable secret kept in `/oos/net/ipv6.secret` (made once) as `stable_secret` on default
  and eth0 (RFC 7217 stable-privacy addresses, addr_gen_mode 2), `use_tempaddr = 0`. SLAAC by the kernel.
- DNS: the kernel passes RDNSS only to user space and busybox has no consumer; DNS stays with the IPv4 lease.
- Services bind `[::]` (dual stack, IPv4-mapped peers) with a fallback to `0.0.0.0`: web, SCPI, VXI-11. Dropbear
  already listens on both. mDNS publishes AAAA for the interface's IPv6 addresses.
- Firewall: `net.allow` token `onlink6` = the IPv6 prefixes on the link (routes without a gateway from RA, length
  48..128, not fe80::/64). nft named set `onlink6` filled by `oos-firewall`, refreshed by `oos-firewall watch` (10 s)
  which writes `/tmp/oos-net6.prefixes` for netacl. ICMPv6: ND/RA rule present; errors for the scope's own traffic
  pass as `related`; everything from allowed sources passes.
- netacl: `onlink6` reads `/tmp/oos-net6.prefixes`.
- Web guard: IPv6 literals already accepted; add tests for global literals and `[addr]` without a port.
- Tests: sysconf/time (tz dhcp), netacl (onlink6), http guard, mdns AAAA, `tools/fw-sim.sh` (rules, prefix parsing,
  set update under nft in a container).

## Status

- done: time zone from DHCP (e58354a); IPv6 switch, dual-stack binds, AAAA, `onlink6` in firewall and netacl,
  `oos-net6`, udhcpc.script flushes IPv4 only (it removed the link-local address: the unit had no IPv6 at all),
  `tools/fw-sim.sh`, docs/network.md 3, 5, 6, 8.
- next: report to team-lead, merge after approval, then unitlab installs and sets `tz = dhcp` in /oos/system.conf.
- not done: RDNSS (no consumer in the image), option 101 (no zoneinfo), IPv6 address on the Network page.
