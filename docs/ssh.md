# SSH access and public keys

New scope images use Dropbear SSH on port 22 in place of the development telnet shell. Password authentication,
agent/X11 forwarding and TCP/Unix forwarding are disabled in the build. No SSH listener starts until a structurally
valid Ed25519 public key is provisioned and `/oos` is mounted. Desktop OOS does not start an SSH server.

The shell is still root: a provisioned key grants full control of the development scope. SCPI, the web remote, MCP
and the update interface retain their existing authentication behavior. Use a trusted bench network.

## First public key

Use an existing OpenSSH Ed25519 key, or create one on your computer with `ssh-keygen -t ed25519`. Keep the private
file on your computer. Only the `.pub` file goes to the scope or image builder.

Choose one local provisioning route:

- Build a personal image with `OOS_SSH_AUTHORIZED_KEYS=/path/to/id_ed25519.pub boot/build-dev-image.sh`.
  The build validates the public key file before packaging it. Several bare public-key lines are supported.
- At the scope's local/serial console, with the public file on USB, run `oos-ssh add /mnt/usb/id_ed25519.pub`,
  then `oos-ssh start`.
- With the OOS data filesystem mounted on a computer, run
  `python3 tools/ssh-keys.py --data-dir /mounted/oos add /path/to/id_ed25519.pub`.
  This edits a directory only; the tool does not mount, partition, format or connect to anything.

The image's public seed initializes a missing `/oos/ssh/authorized_keys` only. Updating an image does not replace
existing keys, and deleting a seeded key does not cause it to return at the next boot. Use the console or offline
tool to add a newly trusted key to an already provisioned scope.

## Verify the device identity

`oos-ssh fingerprint` at the local console shows the device host fingerprint. Compare it with the first
`ssh root@SCOPE_IP` prompt before accepting the connection. Each scope generates its own Ed25519 host key once,
stored as `/oos/ssh/dropbear_ed25519_host_key` with mode 0600. Image updates preserve it. No private host key is
shipped in an image, and no transient identity is generated when the persistent filesystem is unavailable.

After accepting the verified identity, use `ssh -i /path/to/private_key root@SCOPE_IP` (or your usual SSH agent).
`tools/device-deploy.sh` and the read-only shell checks in `tools/device-test.py` use SSH with strict known-host
checking. `OOS_SSH_IDENTITY` optionally selects the local private-key path; `OOS_SSH_PORT` defaults to 22.
Deployment streams files through SSH and verifies SHA256; it needs no unauthenticated upload port or SFTP server.

## Manage access

On the scope's console or an authenticated SSH session:

```
oos-ssh list
oos-ssh add /mnt/usb/another_ed25519.pub
oos-ssh remove BASE64_KEY_FROM_LIST
oos-ssh disable
oos-ssh enable
oos-ssh status
```

Imports accept one bare OpenSSH Ed25519 public key per file. Existing RSA/ECDSA keys, restrictions and comments
are preserved, but the start check requires at least one bare Ed25519 key. Removal targets its exact base64 key,
not a changing row number. Removing the last managed key stops the listener; already authenticated sessions can
remain until they exit. `disable` persists across boots and preserves all keys. Keep console access when removing
the last remote key.

The offline host tool lists SHA256 fingerprints instead of key blobs:

```
python3 tools/ssh-keys.py --data-dir /mounted/oos list
python3 tools/ssh-keys.py --data-dir /mounted/oos remove SHA256:EXACT_FINGERPRINT
```

Both managers write atomically and preserve unrelated existing key lines. The offline tool refuses symlink paths.
SSH logs are in `/tmp/oos-ssh.log`; provisioning and service failures do not stop acquisition startup.

## Build and local validation

`rootfs/build-ssh-tools.sh` builds static arm64 Dropbear and dropbearkey with the same Docker/QEMU path as WLAN
tools. Version 2026.94 is pinned to SHA256
`e098034a843699200c8c977a991fff73159735bf795d5f72ef672c41a6b1ae81`.
The official [release](https://github.com/mkj/dropbear/releases/tag/DROPBEAR_2026.94) links the upstream and mirror
tarballs; the digest is also recorded by [Buildroot](https://github.com/buildroot/buildroot/blob/master/package/dropbear/dropbear.hash).
The [Dropbear manual](https://github.com/mkj/dropbear/blob/DROPBEAR_2026.94/manpages/dropbear.8) describes key files
and daemon options. The image includes Dropbear and bundled crypto licenses.

Run `python3 -m unittest discover -s tools/tests -p test_ssh_keys.py -v` for offline key parsing, preservation,
identity fingerprint, service gating and daemon-argument tests. Shell service tests redirect every path and mock
the daemon/key generator; they never open a listener or contact hardware. Building an arm64 image requires Docker
and the kernel artifacts described in [boot.md](boot.md). These offline tests do not prove a hardware SSH login.
