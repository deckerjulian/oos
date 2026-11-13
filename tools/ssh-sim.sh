#!/bin/sh
# Copyright (C) 2026 OOS contributors
# SPDX-License-Identifier: GPL-2.0-or-later
# ssh-sim.sh - tests /bin/oos-ssh (docs/network.md 7) without a scope: an arm64 container whose busybox is the image's
# busybox, with the image's oos-conf, oos-firewall, nft and Dropbear. An OpenSSH client logs in over loopback with the
# password and with a key, copies a key with ssh-copy-id and a file with scp -O, and checks the guards: no listener
# without remote.ssh, net.allow or the loaded firewall; password login only off while a trusted key exists; password
# and host key the same after a "power cycle". Needs Docker and one finished image build (its busybox), and
# rootfs/out/nft and rootfs/out/dropbearmulti (rootfs/build-nft.sh, rootfs/build-dropbear.sh).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
out="$root/rootfs/out"
w="${TMPDIR:-/tmp}/oos-ssh-sim"
rm -rf "$w"; mkdir -p "$w"
docker image inspect oos-initramfs:ubuntu >/dev/null 2>&1 || { echo "build the image once first (boot/build-dev-image.sh)" >&2; exit 1; }
for f in nft dropbearmulti; do [ -x "$out/$f" ] || { echo "no $out/$f" >&2; exit 1; }; done
docker run --rm --platform linux/arm64 oos-initramfs:ubuntu cat /bin/busybox > "$w/busybox"
chmod +x "$w/busybox"

cat > "$w/test" <<'TEST'
set -u
ok=1
fail() { echo "FAIL: $*"; ok=0; }
st() { cat /tmp/oos-ssh.state; }
conf() { printf '%s\n' "$@" > /oos/system.conf; }
run() { /bin/oos-ssh "$@" 2>> /tmp/oos-ssh.log; }
H="-o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/c/known_hosts -o BatchMode=no -o ConnectTimeout=5"
pw_login() { sshpass -f /oos/ssh/password ssh $H -o PubkeyAuthentication=no -o PreferredAuthentications=password root@127.0.0.1 "$@"; }
key_login() { ssh $H -o PasswordAuthentication=no -o PreferredAuthentications=publickey -i /c/id root@127.0.0.1 "$@"; }
listening() { netstat -ltn 2>/dev/null | grep -q ':22 '; }

mkdir -p /oos/setups /etc/oos /run /c
echo "net.allow = 192.0.2.0/24" > /etc/oos/system.conf

echo "== off by default"
conf "# nothing"
run start; [ "$(st)" = off ] || fail "default: $(st)"
listening && fail "listening while off"

echo "== remote.ssh on, but no net.allow"
: > /etc/oos/system.conf; conf "remote.ssh = on"
run start; [ "$(st)" = "off: needs net.allow" ] || fail "no net.allow: $(st)"

echo "== net.allow, but no firewall"
echo "net.allow = 192.0.2.0/24" > /etc/oos/system.conf
run start; [ "$(st)" = "off: firewall not loaded" ] || fail "no firewall: $(st)"
listening && fail "listening without a firewall"

echo "== firewall, first start"
/bin/oos-firewall; nft list table inet oos_fw >/dev/null || fail "firewall"
run start; [ "$(st)" = "on: password and keys" ] || fail "first start: $(st)"
listening || fail "not listening"
grep -Eqx '[a-hj-km-np-z2-9]{4}(-[a-hj-km-np-z2-9]{4}){3}' /oos/ssh/password || fail "password format: $(cat /oos/ssh/password)"
[ "$(stat -c %a /oos/ssh)" = 700 ] || fail "folder mode $(stat -c %a /oos/ssh)"
for f in password dropbear_ed25519_host_key dropbear_ecdsa_host_key authorized_keys; do
	[ "$(stat -c %a /oos/ssh/$f)" = 600 ] || fail "$f mode $(stat -c %a /oos/ssh/$f)"
done
[ -L /root/.ssh ] && [ "$(readlink /root/.ssh)" = /oos/ssh ] || fail "/root/.ssh"
grep -q '^ssh-ed25519 ' /oos/ssh/dropbear_ed25519_host_key.pub || fail "host key .pub"

echo "== password login"
[ "$(pw_login 'echo in; id -u')" = "$(printf 'in\n0')" ] || fail "password login"
ssh $H -o PubkeyAuthentication=no -o PreferredAuthentications=password -o NumberOfPasswordPrompts=1 root@127.0.0.1 true < /dev/null 2>/dev/null && fail "login without the password"
sshpass -p wrong-password ssh $H -o PubkeyAuthentication=no -o PreferredAuthentications=password root@127.0.0.1 true 2>/dev/null && fail "wrong password taken"
# the host key the client saw is the one in the SSH folder
ssh-keygen -lf /oos/ssh/dropbear_ed25519_host_key.pub | awk '{print $2}' > /c/fp
ssh-keygen -lF 127.0.0.1 -f /c/known_hosts | grep -q "$(cat /c/fp)" || fail "host key seen by the client"

echo "== ssh-copy-id, key login, scp -O"
ssh-keygen -q -t ed25519 -N '' -C tester@host -f /c/id
key_login true 2>/dev/null && fail "key login before the key was trusted"
SSHPASS=$(cat /oos/ssh/password) sshpass -e ssh-copy-id $H -i /c/id.pub root@127.0.0.1 > /c/copy.log 2>&1 || { cat /c/copy.log; fail "ssh-copy-id"; }
grep -q 'tester@host' /oos/ssh/authorized_keys || fail "ssh-copy-id did not reach /oos/ssh/authorized_keys"
[ "$(key_login 'echo key')" = key ] || fail "key login"
echo payload > /c/f
scp -O $H -o PasswordAuthentication=no -i /c/id /c/f root@127.0.0.1:/tmp/f >/dev/null || fail "scp -O to the scope"
[ "$(cat /tmp/f 2>/dev/null)" = payload ] || fail "scp content"

echo "== keys only"
conf "remote.ssh = on" "ssh.password = off"
run restart; [ "$(st)" = "on: keys only" ] || fail "keys only: $(st)"
pw_login true 2>/dev/null && fail "password taken with ssh.password = off"
key_login true || fail "key login with keys only"

echo "== no trusted key: password stays on"
cp /oos/ssh/authorized_keys /c/ak
printf '# ssh-ed25519 AAAAC3 commented out\n\nnot a key\n' > /oos/ssh/authorized_keys
run restart; [ "$(st)" = "on: password kept, no key" ] || fail "lockout guard: $(st)"
pw_login true || fail "password login with the lockout guard"
cp /c/ak /oos/ssh/authorized_keys

echo "== power cycle: password and host key kept"
cp /oos/ssh/password /c/pw.before
run stop; [ "$(st)" = off ] || fail "stop: $(st)"
pidof dropbear >/dev/null && fail "dropbear left after stop"
rm -f /etc/passwd /etc/shadow /root/.ssh /run/dropbear.pid
conf "remote.ssh = on"
run start; [ "$(st)" = "on: password and keys" ] || fail "second start: $(st)"
cmp -s /c/pw.before /oos/ssh/password || fail "password changed"
pw_login true || fail "password login after the power cycle (host key changed?)"
key_login true || fail "key login after the power cycle"

echo "== restart keeps sessions, stop ends them"
key_login 'sleep 30' & s=$!
sleep 1
run restart
kill -0 $s 2>/dev/null || fail "session ended by restart"
run stop
sleep 1
kill -0 $s 2>/dev/null && fail "session survived stop"
wait $s 2>/dev/null

echo "== switched off"
conf "remote.ssh = off"
run restart; [ "$(st)" = off ] || fail "off: $(st)"
listening && fail "listening while off"

echo "---- /tmp/oos-ssh.log"; cat /tmp/oos-ssh.log
grep -q "$(cat /oos/ssh/password)" /tmp/oos-ssh.log && fail "the password is in the log"
[ $ok = 1 ] && echo "ssh-sim: all passed" || { echo "ssh-sim: FAILED"; exit 1; }
TEST

docker run --rm --platform linux/arm64 --cap-add NET_ADMIN \
	-v "$w:/w:ro" -v "$root/rootfs/initramfs/bin:/src:ro" -v "$out:/out:ro" alpine:3.22 sh -euc '
	apk add --no-cache openssh-client openssh-keygen sshpass >/dev/null
	# the image busybox runs every applet from here on (the OpenSSH client and sshpass are the computer side)
	cp /w/busybox /bin/busybox
	for f in oos-ssh oos-conf oos-firewall; do cp /src/$f /bin/; done
	cp /out/nft /out/dropbearmulti /bin/
	for p in dropbear dropbearkey scp; do ln -sf dropbearmulti /bin/$p; done
	rm -f /etc/passwd /etc/shadow /etc/group
	sh /w/test
'
