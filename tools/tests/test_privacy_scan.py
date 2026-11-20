# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""The rules of tools/privacy-scan.py on single lines. The samples are put together here, so this file holds none."""
import importlib.util
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("privacy_scan", ROOT / "tools/privacy-scan.py")
scan = importlib.util.module_from_spec(spec)
spec.loader.exec_module(scan)

MAC = ":".join(["3c", "5a", "b4", "01", "02", "03"])
SERIAL = "DHO9" + "A" + "254" + "100123"
ADDRESS = ".".join(["192", "168", "7", "23"])
HOME = "/Us" + "ers/" + "somebody/Documents"
MAIL = "somebody" + "@" + "mailbox.invalid"


class Rules(unittest.TestCase):
    def rules(self, text, **kw):
        return [rule for rule, _ in scan.find(text, kw.pop("allow", set()), **kw)]

    def test_what_is_found(self):
        self.assertEqual(self.rules(f"eth.mac = {MAC}"), ["mac"])
        self.assertEqual(self.rules(f"serial {SERIAL} of the unit"), ["serial"])
        head = "-----BEGIN OPENSSH " + "PRIVATE KEY-----"
        body = "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQ" + "AAAAAAAAABAAAAMwAAAAtzc2gtZW"
        self.assertEqual(self.rules(head, following=body), ["key"], "a head with the key on the next line")
        self.assertEqual(self.rules(f'"{head}\\n{body}"'), ["key"], "a head with the key in the same string")
        self.assertEqual(self.rules("token ghp_" + "a1B2" * 9), ["token"])
        self.assertEqual(self.rules(f"cd {HOME}"), ["home"])
        self.assertEqual(self.rules(f"ssh root@{ADDRESS}"), ["address"])
        self.assertEqual(self.rules(f"write to {MAIL}"), ["mail"])

    def test_what_is_not(self):
        for text in ("the DHO924S and the DHO804", "sha256 " + "ab" * 32, "00:11:22:33:44:55", "version 10.2.3",
                     "the network 192.168." + "1.0/24",
                     "/home/runner/work", "10848788+dekajoule@users.noreply.github.com", "12:34:56 on the clock",
                     "port 8.8.8.8", "at 1.25 GHz", "TCPIP0::127.0.0.1::5555::SOCKET", "git@github.com:a/b.git",
                     # a key's head alone, as a test of a check for keys writes it
                     'write("-----BEGIN OPENSSH ' + 'PRIVATE KEY-----\\n")',
                     # made-up values of the tests
                     "DHO9A" + "000000001", "DHO9S" + "000000000", "aa:bb:cc:00:00:01", "02:11:22:33:44:5A",
                     "root@example.invalid",
                     "sk-ssh-ed25519@" + "openssh.com"):
            self.assertEqual(self.rules(text), [], text)

    def test_trailers_and_the_allow_list(self):
        self.assertEqual(self.rules(f"Signed-off-by: A Person <{MAIL}>", message=True), [])
        self.assertEqual(self.rules(f"Signed-off-by: A Person <{MAIL}>"), ["mail"])
        self.assertEqual(self.rules(f"ping {ADDRESS}", allow={("address", ADDRESS)}), [])


class Commits(unittest.TestCase):
    """scan() over the commits of a repository made up here: how a diff's added lines are paired."""

    HEAD = "-----BEGIN OPENSSH " + "PRIVATE KEY-----"
    BODY = "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQ" + "AAAAAAAAABAAAAMwAAAAtzc2gtZW"

    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix="oos-privacy-")
        self.addCleanup(shutil.rmtree, self.dir)
        self.git("init", "-q")
        self.commit("start", {"README.md": "a file\n"})
        self.root, scan.ROOT = scan.ROOT, self.dir
        self.addCleanup(setattr, scan, "ROOT", self.root)

    def git(self, *args):
        env = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@example.com", GIT_COMMITTER_NAME="t",
                   GIT_COMMITTER_EMAIL="t@example.com")
        subprocess.run(("git", "-C", self.dir) + args, check=True, capture_output=True, env=env)

    def commit(self, message, files):
        for name, text in files.items():
            with open(os.path.join(self.dir, name), "w", encoding="utf-8") as f:
                f.write(text)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)

    def rules(self):
        return [rule for _, _, rule, _ in scan.scan("HEAD~1..HEAD")]

    def test_a_head_and_its_key_added_together(self):
        self.commit("add", {"k": f"{self.HEAD}\n{self.BODY}\n"})
        self.assertEqual(self.rules(), ["key"])

    def test_a_head_added_above_a_key_already_there(self):
        self.commit("body", {"k": f"x\n{self.BODY}\n"})
        self.assertEqual(self.rules(), [], "a run of base64 alone is no key")
        self.commit("head", {"k": f"{self.HEAD}\n{self.BODY}\n"})
        self.assertEqual(self.rules(), ["key"])

    def test_a_key_added_below_a_head_already_there(self):
        self.commit("head", {"k": f"{self.HEAD}\nx\n"})
        self.assertEqual(self.rules(), [], "a head alone is no key")
        self.commit("body", {"k": f"{self.HEAD}\n{self.BODY}\nx\n"})
        self.assertEqual(self.rules(), ["key"])

    def test_a_key_in_a_commit_message(self):
        self.commit(f"subject\n\n{self.HEAD}\n{self.BODY}\n", {"README.md": "changed\n"})
        self.assertEqual(self.rules(), ["key"])


if __name__ == "__main__":
    unittest.main()
