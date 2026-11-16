#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# wlan-sim.sh - tests the WLAN path without a stick: a virtual machine with simulated radios (mac80211_hwsim) and access
# points (hostapd), running the image's wpa_supplicant, /bin/oos-wlan, /etc/udhcpc.script and the join code of oos-ui
# (test `live_radio` in ui/src/ui/wlan.rs). The stick's own driver and the USB port are not part of this.
# Needs Docker and qemu-system-aarch64.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
. "$root/tools/lib/sim.sh"
w="${TMPDIR:-/tmp}/oos-wlan-sim"
mkdir -p "$w"
[ -x "$root/rootfs/out/wpa_supplicant" ] || "$root/rootfs/build-wlan-tools.sh"
# the access points' hostapd, from rootfs/deps.lock (checked), built once into $w
DEPS_LOCK="$root/rootfs/deps.lock"
. "$root/rootfs/deps.sh"
hostapd=$(dep_ver hostapd)
[ -x "$w/hostapd" ] || dep_fetch hostapd "$w" >/dev/null

(cd "$root/ui" && PATH="$HOME/.cargo/bin:$PATH" cargo test --target aarch64-unknown-linux-musl --no-run --message-format=json 2>/dev/null) \
	| python3 -c '
import sys, json
for l in sys.stdin:
    m = json.loads(l)
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m["target"]["name"] == "oos-ui" and m["profile"]["test"]:
        print(m["executable"])' > "$w/test-binary"
cp "$(cat "$w/test-binary")" "$w/oos-ui-test"

# the busybox of the image
sim_busybox "$w"

cat > "$w/init" <<'INIT'
#!/bin/sh
export PATH=/bin
mkdir -p /proc /sys /dev /run /tmp /oos/setups
mount -t proc proc /proc; mount -t sysfs sys /sys; mount -t devtmpfs dev /dev
for m in /mods/*.ko; do
	case $m in *mac80211_hwsim.ko) insmod $m radios=4 ;; *) insmod $m ;; esac
done
sleep 1
ls /sys/class/net
ip link set lo up
ap() {
	printf 'interface=%s\ndriver=nl80211\nssid=%s\nhw_mode=g\nchannel=%s\n%s\n' "$1" "$2" "$3" "$4" > /tmp/$1.conf
	hostapd -B /tmp/$1.conf > /tmp/$1.log 2>&1 || cat /tmp/$1.log
}
ap wlan1 OOS-Test 1 'wpa=2
wpa_key_mgmt=WPA-PSK
rsn_pairwise=CCMP
wpa_passphrase=se"cr#t p\ss'
ap wlan2 OOS-Open 6 ''
ap wlan3 OOS-Hidden 11 'ignore_broadcast_ssid=1
wpa=2
wpa_key_mgmt=WPA-PSK
rsn_pairwise=CCMP
wpa_passphrase=hidden-pass'
n=1
for i in wlan1 wlan2 wlan3; do
	ip addr add 192.168.5$n.1/24 dev $i 2>/dev/null
	n=$((n + 1))
done
ip addr flush dev wlan2; ip addr flush dev wlan3
ip addr add 192.168.50.1/24 dev wlan1
ip addr add 192.168.50.2/24 dev wlan2
ip addr add 192.168.50.3/24 dev wlan3
# no DHCP server on wlan3: a network that does not answer must not stop oos-wlan
k=4
for i in wlan1 wlan2; do
	printf 'interface %s\nstart 192.168.50.1%s0\nend 192.168.50.1%s9\nlease_file /tmp/%s.leases\npidfile /tmp/%s.pid\nopt router 192.168.50.1\nopt subnet 255.255.255.0\n' $i $k $k $i $i > /tmp/$i.dhcp
	: > /tmp/$i.leases
	udhcpd /tmp/$i.dhcp
	k=$((k + 1))
done
printf '1.0\tend\t\t\n' > /tmp/oos-boot.status
/bin/oos-wlan > /tmp/oos-wlan.log 2>&1 &
sleep 2
cd /tmp
timeout 200 /bin/oos-ui-test --ignored --exact --nocapture --test-threads=1 ui::wlan::tests::live_radio 2>&1
echo "exit code $?"
# the stick is pulled
rmmod mac80211_hwsim
sleep 4
echo ---- oos-wlan.log; cat /tmp/oos-wlan.log
echo ---- wpa_supplicant.log; tail -15 /tmp/wpa_supplicant.log
echo ---- routes; ip route
echo ---- end
poweroff -f
INIT

# mac80211_hwsim and what it needs, and the ciphers and packet sockets (built in on the scope, modules here)
sim_vm_initramfs "$w" "build-base linux-headers pkgconf libnl3-dev libnl3-static" "af_packet ccm gcm cmac ctr mac80211_hwsim" '
	if [ ! -x /w/hostapd ]; then
		cd /tmp && tar -xzf /w/hostapd-$HOSTAPD.tar.gz && cd hostapd-$HOSTAPD/hostapd
		printf "CONFIG_DRIVER_NL80211=y\nCONFIG_LIBNL32=y\nCONFIG_TLS=internal\nCONFIG_INTERNAL_LIBTOMMATH=y\nCONFIG_NO_RANDOM_POOL=y\nCONFIG_GETRANDOM=y\nCONFIG_IEEE80211N=y\n" > .config
		LDFLAGS=-static make -j"$(nproc)" hostapd >/tmp/b.log 2>&1 || { tail -20 /tmp/b.log; exit 1; }
		cp hostapd /w/hostapd
	fi
	cp /rootfs/out/wpa_supplicant /rootfs/out/wpa_cli /rootfs/initramfs/bin/oos-wlan /w/hostapd /w/oos-ui-test $r/bin/
	cp /rootfs/initramfs/etc/udhcpc.script $r/etc/
' -e HOSTAPD="$hostapd" -v "$root/rootfs:/rootfs:ro"
sim_qemu "$w" 1024
grep -q 'LIVE RADIO TEST PASSED' "$w/log" && grep -q 'oos-wlan: stick gone' "$w/log"
