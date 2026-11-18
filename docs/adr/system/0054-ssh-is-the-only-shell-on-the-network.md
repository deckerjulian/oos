# ADR 0054: SSH is the only shell on the network; there is no telnet

- Status: proposed
- Date: 2026-11-18
- Category: system

Supersedes the telnet part of ADR 0025: telnet as a service on by default, with its switch under Remote services and
`remote.telnet`. The rest of ADR 0025 holds.

## Context

ADR 0025 kept telnet from bring-up: busybox `telnetd` on port 23, started by rcS unless `remote.telnet = off`, on by
default, a root shell without a password for everyone who can reach the scope. Since then SSH has come
([network.md](../../network.md) 7, [ssh.md](../../ssh.md)): Dropbear with the scope's own password or trusted keys,
listening only with `net.allow` and the firewall loaded, with its page under Remote services, `scp` and `sftp`. The
documents told the user to switch telnet off once SSH was set up; a scope on which nobody did stayed open to a root
shell. The host tools (`tools/device-deploy.sh`, `tools/device-test.py`) reached the unit over telnet by default and
over SSH with `OOS_SHELL=ssh`.

## Decision

- The image starts no `telnetd`, oos-ui has no telnet switch and starts or stops nothing on port 23. SSH is the only
  shell on the network; the serial console keeps its shell.
- `remote.telnet` in `/oos/system.conf` or `/etc/oos/system.conf` is read with any value and dropped: a file of an
  earlier version loads without a refused line, and the key is not written again.
- The host tools reach a unit over SSH only, with a trusted key and strict host key checking.

## Alternatives

- **Telnet off by default, the switch kept:** the shell without a password stays one tap away, and every document and
  tool keeps two ways in. Nothing needs it that SSH cannot do.
- **Telnet only with `net.allow`, as SSH:** still no authentication; on a listed prefix everyone gets root.
- **SSH on by default:** it would listen before anyone chose an address list, which ADR 0025 and network.md 7 rule
  out; SSH stays off until the image or the user switches it on.

## Consequences

- A scope has no shell on the network until SSH is switched on and Allowed holds a list. The first login is on the
  screen (Remote services > Allowed, SSH on, Show password; ssh.md, "The first login"); a bench unit for the tools
  needs that once, with a key ([device-testing.md](../../device-testing.md)).
- What a shell was used for after an installation (`oos-boot-oos-again` to start OOS once more, reading the logs) needs
  SSH or the serial console; System > Boot mode = Persist and running the install again from stock do not need a
  shell.
- The busybox of the image still holds the `telnetd` applet (the Ubuntu `busybox-static` package): nothing starts it,
  and taking it out would need a busybox of our own.
- How it is known (ADR 0015): the settings file and the page are tested on the host (`sysconf::tests`,
  `ui::remote::tests`), the tools with a stand-in `ssh` (`tools/tests/test_ssh_keys.py`); a start without telnet, a
  login over SSH and the tools over SSH are not yet verified on hardware (`no_telnet_shell` of
  `tools/device-test.py` checks it on a unit).

## References

`rootfs/initramfs/etc/rcS`, `ui/src/sysconf.rs`, `ui/src/ui/remote.rs`, `ui/src/remote.rs`, `ui/src/main.rs`,
`tools/device-deploy.sh`, `tools/device-test.py`, [network.md](../../network.md) 4 and 7, [ssh.md](../../ssh.md),
[device-testing.md](../../device-testing.md), ADR 0025, issue #185.
