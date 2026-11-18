# SSH access and trusted keys

The SSH server, its switches and its page are described in [network.md](network.md) section 7: Dropbear started by
`/bin/oos-ssh`, off by default (`remote.ssh`), listening only with `net.allow` and the firewall loaded, root with the
scope's own password or a trusted key, System > Network > Remote services > SSH settings. telnet stays beside it
(`remote.telnet`, on by default); switch it off once SSH is set up. `net.allow` is set on the same page, **Allowed**
(network.md section 6); it applies at once: with SSH switched on, a list set starts the server, and a list removed
stops it with its sessions.

This page is about getting trusted keys onto a scope without the password.

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
