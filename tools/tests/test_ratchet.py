# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/ratchet.sh in a small repository made here: what counts, what fails it, and that --lower only lowers.
Needs sh, git and awk."""
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class Ratchet(unittest.TestCase):
    def setUp(self):
        self.repo = Path(tempfile.mkdtemp(prefix="oos-ratchet-"))
        self.addCleanup(shutil.rmtree, self.repo)
        (self.repo / "tools").mkdir()
        shutil.copy(ROOT / "tools" / "ratchet.sh", self.repo / "tools" / "ratchet.sh")
        for crate in ("hal", "dsp", "ui", "fpgad"):
            (self.repo / crate / "src").mkdir(parents=True)
            (self.repo / crate / "src" / "lib.rs").write_text("pub fn f() {}\n")
        self.big = self.repo / "ui" / "src" / "big.rs"
        self.write_big(code=2100, tests=300)
        (self.repo / "hal" / "src" / "lib.rs").write_text(
            "#[allow(dead_code)]\nfn a() {}\nfn b() { unsafe { c() } }\n// unsafe in a comment\nunsafe fn c() {}\n")
        self.git("init", "-q")
        self.git("add", "-A")

    def write_big(self, code, tests):
        body = "".join(f"fn f{i}() {{}}\n" for i in range(code))
        module = "#[cfg(test)]\nmod tests {\n" + "".join(f"    fn t{i}() {{}}\n" for i in range(tests)) + "}\n"
        self.big.write_text(body + module)

    def git(self, *args):
        subprocess.run(("git", "-C", str(self.repo)) + args, check=True, capture_output=True)

    def ratchet(self, *args):
        out = subprocess.run(["sh", "tools/ratchet.sh", *args], cwd=self.repo, capture_output=True, text=True)
        return out.returncode, out.stdout + out.stderr

    def numbers(self):
        return dict(line.split(" ", 1)[::-1] for line in (self.repo / "tools" / "ratchet.txt").read_text().splitlines())

    def test_what_counts(self):
        self.assertEqual(self.ratchet("--init")[0], 0)
        n = self.numbers()
        self.assertEqual(n["lines ui/src/big.rs"], "2100", "the test module inside the file is not counted")
        self.assertEqual(n["allow hal"], "1")
        self.assertEqual(n["unsafe hal"], "2", "not the one in a comment")
        self.assertNotIn("lines hal/src/lib.rs", n)
        self.assertEqual(self.ratchet()[0], 0)

    def test_up_fails_and_a_test_costs_nothing(self):
        self.ratchet("--init")
        self.write_big(code=2100, tests=900)
        self.assertEqual(self.ratchet()[0], 0, "more tests in the file")
        self.write_big(code=2101, tests=300)
        code, out = self.ratchet()
        self.assertEqual(code, 1)
        self.assertIn("up: lines ui/src/big.rs 2100 -> 2101", out)
        self.assertEqual(self.ratchet("--lower")[0], 1, "--lower does not raise")
        self.assertEqual(self.numbers()["lines ui/src/big.rs"], "2100")

    def test_down_fails_until_lowered(self):
        self.ratchet("--init")
        self.write_big(code=2050, tests=300)
        code, out = self.ratchet()
        self.assertEqual(code, 1)
        self.assertIn("down: lines ui/src/big.rs 2100 -> 2050", out)
        self.assertEqual(self.ratchet("--lower")[0], 0)
        self.assertEqual(self.numbers()["lines ui/src/big.rs"], "2050")
        self.assertEqual(self.ratchet()[0], 0)
        self.write_big(code=1900, tests=300)
        self.assertEqual(self.ratchet("--lower")[0], 0)
        self.assertNotIn("lines ui/src/big.rs", self.numbers(), "at or under the limit: no line")

    def test_a_new_large_file_and_a_new_allow(self):
        self.ratchet("--init")
        other = self.repo / "dsp" / "src" / "other.rs"
        other.write_text("".join(f"fn g{i}() {{}}\n" for i in range(2001)))
        self.git("add", "-A")
        code, out = self.ratchet()
        self.assertEqual(code, 1)
        self.assertIn("lines dsp/src/other.rs is new with 2001 lines", out)
        other.unlink()
        self.git("add", "-A")
        (self.repo / "dsp" / "src" / "lib.rs").write_text("#[allow(unused)]\npub fn f() {}\n")
        code, out = self.ratchet()
        self.assertEqual(code, 1)
        self.assertIn("up: allow dsp 0 -> 1", out)

    def test_init_only_once(self):
        self.assertEqual(self.ratchet("--init")[0], 0)
        self.assertEqual(self.ratchet("--init")[0], 2)


if __name__ == "__main__":
    unittest.main()
