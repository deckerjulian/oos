"""Offline SSH provisioning tests. No sockets, listeners or hardware access."""
import base64
import importlib.util
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


keys = load("oos_ssh_keys", ROOT / "tools/ssh-keys.py")
device_test = load("oos_device_test", ROOT / "tools/device-test.py")


def public(byte=1, comment="offline test"):
    blob = struct.pack(">I", 11) + b"ssh-ed25519" + struct.pack(">I", 32) + bytes([byte]) * 32
    return "ssh-ed25519 " + base64.b64encode(blob).decode() + " " + comment


class HostKeys(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(dir=ROOT / "local")
        self.root = Path(self.tmp.name)
        self.file = self.root / "public.pub"
        self.file.write_text(public() + "\n")

    def tearDown(self):
        self.tmp.cleanup()

    def test_import_deduplicates_and_preserves_existing_key_and_host_identity(self):
        state = self.root / "ssh"
        state.mkdir()
        old = "# existing restrictions\nno-pty ssh-rsa AAAA legacy\n"
        (state / "authorized_keys").write_text(old)
        (state / "dropbear_ed25519_host_key").write_bytes(b"existing private identity")
        self.assertEqual(keys.edit(self.root, "add", self.file), ["Added 1 public key(s)"])
        self.assertEqual(keys.edit(self.root, "add", self.file), ["Added 0 public key(s)"])
        self.assertTrue((state / "authorized_keys").read_text().startswith(old))
        self.assertEqual((state / "dropbear_ed25519_host_key").read_bytes(), b"existing private identity")
        fingerprint = keys.public_key(public())[1]
        self.assertIn(fingerprint, "\n".join(keys.edit(self.root, "list")))
        keys.edit(self.root, "remove", fingerprint)
        self.assertEqual((state / "authorized_keys").read_text(), old)

    def test_malformed_private_options_and_embedded_type_are_rejected_without_writing(self):
        for invalid in ["-----BEGIN OPENSSH PRIVATE KEY-----", "command=evil " + public(),
                        "ssh-ed25519 !!!", "ssh-ed25519 " + base64.b64encode(b"wrong structure").decode(),
                        public(comment="bad\x1bcomment")]:
            with self.subTest(invalid=invalid):
                self.file.write_text(invalid + "\n")
                with self.assertRaises(ValueError):
                    keys.edit(self.root, "add", self.file)
        self.assertFalse((self.root / "ssh/authorized_keys").exists())

    def test_crlf_bom_seed_is_canonical_and_duplicate_fingerprints_are_stable(self):
        self.file.write_bytes(("\ufeff# public seed\r\n" + public() + "\r\n").encode())
        parsed = keys.import_keys(self.file)
        self.assertEqual(parsed, [keys.public_key(public())])
        keys.edit(self.root, "add", self.file)
        self.assertEqual((self.root / "ssh/authorized_keys").read_bytes(), (public() + "\n").encode())

    def test_unknown_remove_does_not_change_content(self):
        keys.edit(self.root, "add", self.file)
        path = self.root / "ssh/authorized_keys"
        before = path.read_bytes()
        with self.assertRaises(ValueError):
            keys.edit(self.root, "remove", "SHA256:unknown")
        self.assertEqual(path.read_bytes(), before)

    def test_fingerprint_matches_independent_openssh_when_available(self):
        executable = shutil.which("ssh-keygen")
        if not executable:
            self.skipTest("OpenSSH keygen unavailable")
        output = subprocess.check_output([executable, "-lf", str(self.file)], text=True)
        self.assertIn(keys.public_key(public())[1], output)

    def test_device_shell_uses_key_auth_and_strict_identity_without_network(self):
        scope = object.__new__(device_test.Scope)
        scope.host, scope.sim = "example.invalid", False
        result = subprocess.CompletedProcess([], 0, "read-only result\n", "")
        with mock.patch.dict(os.environ, {"OOS_SSH_IDENTITY": "local/test identity", "OOS_SSH_PORT": "2222"}):
            with mock.patch.object(subprocess, "run", return_value=result) as run:
                self.assertEqual(scope.shell("uname -r"), "read-only result")
                args = run.call_args.args[0]
                self.assertIn("StrictHostKeyChecking=yes", args)
                self.assertIn("BatchMode=yes", args)
                self.assertEqual(args[-2:], ["root@example.invalid", "uname -r"])
                self.assertEqual(args[args.index("-p") + 1], "2222")
                self.assertEqual(args[args.index("-i") + 1], "local/test identity")
                with self.assertRaises(AssertionError):
                    scope.shell("dd of=/dev/mmcblk0")
                self.assertEqual(run.call_count, 1)


def posix(path):
    path = Path(path).resolve()
    return "/" + path.drive[0].lower() + path.as_posix()[2:] if path.drive else str(path)


class DeviceScript(unittest.TestCase):
    """Run the actual shell manager with paths redirected and daemon/keygen mocked."""
    def setUp(self):
        self.shell = shutil.which("sh")
        if not self.shell and Path("C:/msys64/usr/bin/sh.exe").exists():
            self.shell = "C:/msys64/usr/bin/sh.exe"
        if not self.shell:
            self.skipTest("POSIX shell unavailable")
        self.tmp = tempfile.TemporaryDirectory(dir=ROOT / "local")
        self.root = Path(self.tmp.name)
        self.state = self.root / "ssh"
        self.seed = self.root / "seed"
        self.seed.write_text(public() + "\n")
        self.public = self.root / "import.pub"
        self.public.write_text(public(2) + "\n")
        self.bin = self.root / "bin"
        self.bin.mkdir()
        log = posix(self.root / "daemon.args")
        (self.bin / "dropbear").write_text(f"#!/bin/sh\nprintf '%s\\n' \"$@\" > '{log}'\n")
        (self.bin / "dropbearkey").write_text("#!/bin/sh\nif [ \"$1\" = -t ]; then printf 'fixture host identity' > \"$4\"; else echo 'fixture fingerprint'; fi\n")
        (self.bin / "sync").write_text("#!/bin/sh\nexit 0\n")
        (self.bin / "chown").write_text("#!/bin/sh\nexit 0\n")
        for path in self.bin.iterdir():
            path.chmod(0o755)
        script = (ROOT / "rootfs/initramfs/bin/oos-ssh").read_text()
        script = script.replace("state=/oos/ssh", f"state='{posix(self.state)}'")
        script = script.replace("seed=/etc/oos-ssh/authorized_keys", f"seed='{posix(self.seed)}'")
        script = script.replace("pidfile=/run/oos-ssh.pid", f"pidfile='{posix(self.root / 'pid')}'")
        script = script.replace("mounted() { grep -q ' /oos ' /proc/mounts; }", "mounted() { return 0; }")
        script = script.replace("/bin/dropbear", posix(self.bin / "dropbear"))
        script = script.replace("/tmp/oos-ssh.log", posix(self.root / "log"))
        script = script.replace("set -eu", f"set -eu\nexport PATH='{posix(self.bin)}:/usr/bin:/bin'")
        self.script = self.root / "oos-ssh"
        self.script.write_text(script, newline="\n")

    def tearDown(self):
        if hasattr(self, "tmp"):
            self.tmp.cleanup()

    def run_script(self, *args, check=True):
        result = subprocess.run([self.shell, posix(self.script), *args], capture_output=True, text=True)
        if check:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def test_boot_without_a_valid_key_never_invokes_daemon(self):
        self.seed.write_text("ssh-ed25519 invalid\n")
        result = self.run_script("start")
        self.assertIn("SSH off", result.stdout)
        self.assertFalse((self.root / "daemon.args").exists())
        self.assertFalse((self.state / "dropbear_ed25519_host_key").exists())

    def test_valid_provision_starts_key_only_and_preserves_host_identity(self):
        self.run_script("start")
        args = (self.root / "daemon.args").read_text().splitlines()
        for required in ["-s", "-j", "-k", "-D", "-r"]:
            self.assertIn(required, args)
        host = self.state / "dropbear_ed25519_host_key"
        host.write_bytes(b"existing unique host key")
        self.run_script("start")
        self.assertEqual(host.read_bytes(), b"existing unique host key")

    def test_add_remove_preserve_existing_lines_and_removed_seed_is_not_resurrected(self):
        self.run_script("list")
        path = self.state / "authorized_keys"
        with path.open("a") as stream:
            stream.write("no-pty ssh-rsa AAAA legacy\n")
        self.run_script("add", posix(self.public))
        self.run_script("add", posix(self.public))
        self.assertEqual(path.read_text().count(public(2).split()[1]), 1)
        self.run_script("remove", public().split()[1])
        self.run_script("list")
        self.assertNotIn(public().split()[1], path.read_text())
        self.assertIn("no-pty ssh-rsa AAAA legacy", path.read_text())

    def test_disable_survives_start_and_invalid_import_is_atomic(self):
        self.run_script("disable")
        self.run_script("start")
        self.assertFalse((self.root / "daemon.args").exists())
        path = self.state / "authorized_keys"
        before = path.read_bytes()
        self.public.write_text("-----BEGIN OPENSSH PRIVATE KEY-----\n")
        self.assertNotEqual(self.run_script("add", posix(self.public), check=False).returncode, 0)
        self.assertEqual(path.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
