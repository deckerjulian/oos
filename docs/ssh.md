# SSH access and trusted keys

The SSH server, its switches and its page are described in [network.md](network.md) section 7: Dropbear started by
`/bin/oos-ssh`, off by default (`remote.ssh`), listening only with `net.allow` and the firewall loaded, root with the
scope's own password or a trusted key, System > Network > Remote services > SSH settings. SSH is the only shell on
the network: there is no telnet (ADR 0054); the serial console keeps its shell. `net.allow` is set on the same page,
**Allowed** (network.md section 6); it applies at once: with SSH switched on, a list set starts the server, and a list
removed stops it with its sessions.

This page is about the first login and about getting trusted keys onto a scope without the password.

## The first login

On a scope whose image does not switch SSH on already (most images):

1. System > Network > Remote services > **Allowed**: pick the network the scope is on (`192.0.2.0/24`), or Other for
   the prefix of your computer. SSH listens only with an address list, so it never opens to a network nobody named.
2. **SSH** on. SSH settings > State shows `on: password and keys`.
3. SSH settings > **Show password**: `ssh root@<address>`, the password and the host key fingerprint. On the computer,
   `ssh root@<address>`, compare the fingerprint it asks about with the one on the scope, answer `yes`, and give the
   password.
4. With a key from then on: `ssh-copy-id root@<address>` (or one of the ways below), and Password login off once
   `ssh -o BatchMode=yes root@<address> true` works without the password.

The password, the host key and the trusted keys stay on the data partition (`/oos/ssh`) over updates and power
cycles. The tools of the tree use the same way in (below).

## Keys built into an image

```
OOS_SSH_AUTHORIZED_KEYS=/path/to/id_ed25519.pub boot/build-dev-image.sh
```

The build checks the file with `tools/ssh-keys.py validate` (bare OpenSSH public keys, one per line; private keys are
refused) and puts it into the image as `/etc/oos-ssh/authorized_keys`. At its first start `oos-ssh` copies it into a
new SSH folder (`/oos/ssh/authorized_keys`). An update never replaces the keys there, and a key removed from the folder
does not come back from the image.

## Keys written to a card on a computer

With the scope's data partition mounted on a computer:

```
python3 tools/ssh-keys.py --data-dir /mounted/oos add ~/.ssh/id_ed25519.pub
python3 tools/ssh-keys.py --data-dir /mounted/oos list
python3 tools/ssh-keys.py --data-dir /mounted/oos remove SHA256:EXACT_FINGERPRINT
```

The tool edits `ssh/authorized_keys` in that folder only: it mounts, formats and connects to nothing, keeps lines it
does not know, writes atomically and refuses symbolic links. New imports are bare Ed25519 keys; `list` shows SHA-256
fingerprints, `remove` takes one of them.

The other ways in, on the running scope: Import keys from USB on the SSH settings page, or `ssh-copy-id` with the
password (network.md 7).

## Tools over SSH

`tools/device-deploy.sh` and `tools/device-test.py` reach the scope over SSH only, with a trusted key and strict host
key checking: compare the fingerprint on the SSH settings page with the first `ssh root@<ip>` prompt first.
`OOS_SSH_IDENTITY` names a private key, `OOS_SSH_PORT` another port. The steps for a bench unit are in
[device-testing.md](device-testing.md#ssh-on-a-bench-unit).

Offline tests of the key tool: `python3 -m unittest discover -s tools/tests -p test_ssh_keys.py -v`.
