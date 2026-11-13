#!/bin/sh
# Copyright (C) 2026 OOS contributors
# SPDX-License-Identifier: GPL-2.0-or-later
# fw-sim.sh - tests oos-firewall's IPv6 part, udhcpc.script and oos-net6 (docs/network.md 6, 8) without a scope: an
# arm64 container with the image's busybox, oos-conf, oos-firewall and nft. Checks which routes count as prefixes on
# the link, the ruleset, the set `onlink6` loaded in the kernel and kept up to date by `watch`, the lease's time zone
# and that a DHCP lease leaves the IPv6 addresses alone. Needs Docker, one finished image build (its busybox) and
# rootfs/out/nft (rootfs/build-nft.sh; OOS_OUT names another rootfs/out).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
out="${OOS_OUT:-$root/rootfs/out}"
w="${TMPDIR:-/tmp}/oos-fw-sim"
rm -rf "$w"; mkdir -p "$w"
docker image inspect oos-initramfs:ubuntu >/dev/null 2>&1 || { echo "build the image once first (boot/build-dev-image.sh)" >&2; exit 1; }
[ -x "$out/nft" ] || { echo "no $out/nft" >&2; exit 1; }
docker run --rm --platform linux/arm64 oos-initramfs:ubuntu cat /bin/busybox > "$w/busybox"
chmod +x "$w/busybox"

cat > "$w/routes" <<'R'
default via fe80::1 dev eth0  metric 1024  expires 1790sec
2001:db8:5:7::/64 dev eth0  metric 256  expires 86390sec
2001:DB8:77::/64 dev wlan0  metric 1024
fd12:3456::/48 dev eth0  metric 256
2001:db8:99::/64 via fe80::1 dev eth0  metric 1024
fe80::/64 dev eth0  metric 256
2000::/3 dev eth0  metric 1024
2001:db8:40::/40 dev eth0  metric 1024
::1/128 dev lo  metric 256
ff00::/8 dev eth0  metric 256
unreachable ::/96 dev lo  metric 1024
R

cat > "$w/test" <<'TEST'
set -u
ok=1
fail() { echo "FAIL: $*"; ok=0; }
conf() { printf '%s\n' "$@" > /oos/system.conf; }
mkdir -p /oos/setups /etc/oos
: > /etc/oos/system.conf
export OOS_ROUTES6=/c/routes
cp /w/routes /c/routes

echo "== prefixes on the link"
want="2001:db8:5:7::/64
2001:db8:77::/64
fd12:3456::/48"
got=$(/bin/oos-firewall --prefixes)
[ "$got" = "$want" ] || fail "prefixes: $got"

echo "== ruleset"
conf "net.allow = 192.0.2.0/24 fe80::/10 onlink6"
r=$(/bin/oos-firewall --print)
echo "$r" | grep -q 'elements = { 2001:db8:5:7::/64, 2001:db8:77::/64, fd12:3456::/48 }' || fail "set elements"
echo "$r" | grep -q 'ip6 saddr @onlink6 accept' || fail "set rule"
echo "$r" | grep -q 'ip6 saddr { fe80::/10 } accept' || fail "link-local"
echo "$r" | grep -q 'ip saddr { 192.0.2.0/24 } accept' || fail "IPv4"
conf "net.allow = 192.0.2.0/24"
/bin/oos-firewall --print | grep -q onlink6 && fail "set without onlink6"
conf "net.allow = onlink6"
/bin/oos-firewall --print | grep -q '@onlink6' || fail "onlink6 alone"
conf "ntp = on"
[ -z "$(/bin/oos-firewall --print)" ] || fail "a firewall without net.allow"

echo "== loaded"
conf "net.allow = 192.0.2.0/24 fe80::/10 onlink6"
/bin/oos-firewall || fail "apply"
member() { nft get element inet oos_fw onlink6 "{ $1 }" >/dev/null 2>&1; }
member 2001:db8:5:7::abcd || fail "on-link address not in the set"
member fd12:3456:0:9::1 || fail "ULA /48 not in the set"
member 2001:db8:99::1 && fail "routed prefix in the set"
member 2a00:1450:4001::1 && fail "internet address in the set"
[ "$(cat /tmp/oos-net6.prefixes)" = "$want" ] || fail "prefix file: $(cat /tmp/oos-net6.prefixes)"

echo "== watch"
OOS_FW6_PERIOD=1 /bin/oos-firewall watch 2> /c/watch.log &
wp=$!
sed -i 's/^2001:db8:5:7::/2001:db8:6:1::/' /c/routes
n=0; until grep -q '2001:db8:6:1::/64' /tmp/oos-net6.prefixes 2>/dev/null || [ $n -ge 10 ]; do sleep 1; n=$((n + 1)); done
member 2001:db8:6:1::5 || fail "new prefix not in the set"
member 2001:db8:5:7::abcd && fail "old prefix still in the set"
grep -v -e '^fd12' -e '^2001:DB8:77' /w/routes | grep -v '^2001:db8:5:7' > /c/routes
sed -i '/^2001:db8:6:1::/d' /c/routes
sed -i '/ dev /!d' /c/routes
n=0; until [ -z "$(cat /tmp/oos-net6.prefixes)" ] || [ $n -ge 10 ]; do sleep 1; n=$((n + 1)); done
[ -z "$(cat /tmp/oos-net6.prefixes)" ] || fail "prefixes gone: $(cat /tmp/oos-net6.prefixes)"
member 2001:db8:6:1::5 && fail "set not emptied"
kill $wp
grep -q 'IPv6 prefixes on the link: 2001:db8:6:1::/64' /c/watch.log || fail "watch log: $(cat /c/watch.log)"
conf "net.allow = 192.0.2.0/24"
timeout 3 /bin/oos-firewall watch || fail "watch without onlink6 does not end"

echo "== DHCP lease: time zone, IPv6 kept"
ip link add d0 type dummy && ip link set d0 up
n=0; until ip -6 addr show dev d0 | grep -q 'inet6 fe80'; do sleep 1; n=$((n + 1)); [ $n -ge 5 ] && break; done
ip -6 addr add 2001:db8:5:7::9/64 dev d0
cp /etc/resolv.conf /c/resolv
interface=d0 ip=10.9.9.9 mask=24 tzstr='CET-1CEST,M3.5.0,M10.5.0/3' ntpsrv=10.9.9.1 sh /etc/udhcpc.script bound
[ "$(cat /tmp/oos-dhcp-tz.d0)" = 'CET-1CEST,M3.5.0,M10.5.0/3' ] || fail "tz file"
[ "$(cat /tmp/oos-dhcp-ntp.d0)" = 10.9.9.1 ] || fail "ntp file"
ip -4 addr show dev d0 | grep -q 'inet 10.9.9.9/24' || fail "IPv4 address"
ip -6 addr show dev d0 | grep -q 'inet6 fe80' || fail "link-local address flushed"
ip -6 addr show dev d0 | grep -q 'inet6 2001:db8:5:7::9' || fail "SLAAC address flushed"
# a hostile string is stored as it came (oos-ui parses it) and runs nothing
interface=d0 ip=10.9.9.8 mask=24 tzstr='$(touch /c/pwned);`touch /c/pwned`' sh /etc/udhcpc.script bound
[ -e /c/pwned ] && fail "the lease ran a command"
interface=d0 ip=10.9.9.8 mask=24 sh /etc/udhcpc.script renew
[ -e /tmp/oos-dhcp-tz.d0 ] && fail "tz file kept without option 100"
interface=d0 sh /etc/udhcpc.script deconfig
cp /c/resolv /etc/resolv.conf

echo "== IPv6 secret"
s1=$(/bin/oos-net6 --secret /c/net/ipv6.secret); s2=$(/bin/oos-net6 --secret /c/net/ipv6.secret)
echo "$s1" | grep -Eqx '([0-9a-f]{4}:){7}[0-9a-f]{4}' || fail "secret format: $s1"
[ "$s1" = "$s2" ] || fail "secret not kept"
[ "$(stat -c %a /c/net/ipv6.secret)" = 600 ] || fail "secret mode $(stat -c %a /c/net/ipv6.secret)"

[ $ok = 1 ] && echo "fw-sim: all passed" || { echo "fw-sim: FAILED"; exit 1; }
TEST

docker run --rm --platform linux/arm64 --cap-add NET_ADMIN --sysctl net.ipv6.conf.all.disable_ipv6=0 \
	--sysctl net.ipv6.conf.default.disable_ipv6=0 \
	-v "$w:/w:ro" -v "$root/rootfs/initramfs:/src:ro" -v "$out:/out:ro" alpine:3.22 sh -euc '
	cp /w/busybox /bin/busybox
	for a in sh ip awk sed grep sort cat od tr mv mkdir chmod dirname sleep timeout stat cp; do ln -sf busybox /bin/$a; done
	for f in oos-conf oos-firewall oos-net6; do cp /src/bin/$f /bin/; done
	cp /src/etc/udhcpc.script /etc/udhcpc.script
	cp /out/nft /bin/
	mkdir -p /c
	sh /w/test
'
