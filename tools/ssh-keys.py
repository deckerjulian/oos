#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""Manage public keys in an offline mounted /oos directory, or validate build seeds.

  ssh-keys.py --data-dir /mounted/oos add ~/.ssh/id_ed25519.pub
  ssh-keys.py --data-dir /mounted/oos list
  ssh-keys.py --data-dir /mounted/oos remove SHA256:...
  ssh-keys.py validate id_ed25519.pub --out local/authorized_keys

Never connects to a scope or generates private keys. Private key inputs are rejected.
New imports are bare Ed25519 public keys. Unrecognized existing lines survive edits.
"""
import argparse
import base64
import hashlib
import os
from pathlib import Path
import struct
import tempfile


def public_key(line):
    line = line.strip()
    if any(ord(c) < 32 or ord(c) == 127 for c in line):
        raise ValueError("control characters are not allowed in public keys")
    fields = line.split(None, 2)
    if len(fields) < 2 or fields[0] != "ssh-ed25519":
        raise ValueError("expected a bare OpenSSH Ed25519 public key, not a private key/options")
    try:
        blob = base64.b64decode(fields[1], validate=True)
    except ValueError as error:
        raise ValueError("invalid public-key base64") from error
    prefix = struct.pack(">I", 11) + b"ssh-ed25519" + struct.pack(">I", 32)
    if len(blob) != 51 or not blob.startswith(prefix):
        raise ValueError("invalid Ed25519 key structure")
    canonical = "ssh-ed25519 " + base64.b64encode(blob).decode()
    if len(fields) == 3:
        canonical += " " + fields[2]
    fingerprint = "SHA256:" + base64.b64encode(hashlib.sha256(blob).digest()).decode().rstrip("=")
    return canonical, fingerprint


def import_keys(path):
    raw = Path(path).read_bytes()
    if len(raw) > 65536:
        raise ValueError("public-key file exceeds 64 KiB")
    lines = raw.decode("utf-8-sig").splitlines()
    keys = [public_key(line) for line in lines if line.strip() and not line.lstrip().startswith("#")]
    if not keys:
        raise ValueError("no Ed25519 public keys found")
    return keys


def atomic_write(path, text):
    path = Path(path)
    # A symlink anywhere in the supplied path must not redirect a key write.
    for part in [path, *path.parents]:
        if part.is_symlink():
            raise ValueError("refusing symlink key path")
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(path.parent, 0o700)
    descriptor, tmp = tempfile.mkstemp(prefix=".keys-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(text)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(tmp, 0o600)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def edit(data_dir, action, value=None):
    path = Path(data_dir) / "ssh" / "authorized_keys"
    for part in [path, *path.parents]:
        if part.is_symlink():
            raise ValueError("refusing symlink key path")
    original = path.read_text(encoding="utf-8") if path.exists() else ""
    lines = original.splitlines()
    known = {}
    for i, line in enumerate(lines):
        try:
            canonical, fingerprint = public_key(line)
            known[i] = (canonical, fingerprint)
        except ValueError:
            pass  # Preserve existing RSA/ECDSA keys, restrictions and comments verbatim.
    if action == "list":
        return [f"{entry[1]} {entry[0]}" for entry in known.values()] + (["Other existing lines preserved"] if len(known) < len([x for x in lines if x.strip()]) else [])
    if action == "add":
        imported = import_keys(value)
        existing = {entry[1] for entry in known.values()}
        additions = []
        for line, fingerprint in imported:
            if fingerprint not in existing:
                additions.append(line)
                existing.add(fingerprint)
        if additions:
            text = original + ("\n" if original and not original.endswith("\n") else "") + "\n".join(additions) + "\n"
            atomic_write(path, text)
        return [f"Added {len(additions)} public key(s)"]
    if action == "remove":
        matched = {i for i, entry in known.items() if entry[1] == value}
        if not matched:
            raise ValueError("fingerprint not found; list keys first")
        atomic_write(path, "".join(line + "\n" for i, line in enumerate(lines) if i not in matched))
        return [f"Removed {len(matched)} public key(s)"]
    raise ValueError("unknown key action")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--data-dir", type=Path, help="offline mounted OOS data directory; never a raw disk")
    sub = parser.add_subparsers(dest="action", required=True)
    sub.add_parser("list")
    sub.add_parser("add").add_argument("file", type=Path)
    sub.add_parser("remove").add_argument("fingerprint")
    validate = sub.add_parser("validate")
    validate.add_argument("file", type=Path)
    validate.add_argument("--out", type=Path)
    args = parser.parse_args()
    try:
        if args.action == "validate":
            keys = import_keys(args.file)
            if args.out:
                atomic_write(args.out, "".join(key + "\n" for key, _ in keys))
            for _, fingerprint in keys:
                print(fingerprint)
        else:
            if args.data_dir is None:
                parser.error("--data-dir is required for offline key management")
            for line in edit(args.data_dir, args.action, getattr(args, "file", getattr(args, "fingerprint", None))):
                print(line)
    except (ValueError, OSError, UnicodeError) as error:
        parser.exit(1, f"ssh-keys: {error}\n")


if __name__ == "__main__":
    main()
