# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/code-size.py on a small tree made here: what counts as code, and the table of two trees."""
import contextlib
import importlib.util
import io
import shutil
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("code_size", ROOT / "tools/code-size.py")
size = importlib.util.module_from_spec(spec)
spec.loader.exec_module(size)

LIB = """// a comment
pub fn a() {}

mod part;
#[cfg(test)]
mod checks;
#[cfg(test)]
mod tests {
    fn t() {}
}
pub fn b() {} // code with a comment after it
"""


class Size(unittest.TestCase):
    def tree(self, extra=""):
        root = Path(tempfile.mkdtemp(prefix="oos-size-"))
        self.addCleanup(shutil.rmtree, root)
        for crate in size.CRATES:
            (root / crate / "src").mkdir(parents=True)
            (root / crate / "src" / "lib.rs").write_text("pub fn f() {}\n")
        src = root / "ui" / "src"
        (src / "lib.rs").write_text(LIB + extra)
        (src / "part.rs").write_text("pub fn p() {}\npub fn q() {}\n")
        (src / "checks.rs").write_text("fn only_for_tests() {}\n" * 50)
        (root / "ui" / "tests").mkdir()
        (root / "ui" / "tests" / "it.rs").write_text("fn t() {}\n" * 50)
        (root / "ui" / "target").mkdir()
        (root / "ui" / "target" / "gen.rs").write_text("fn g() {}\n" * 50)
        return root

    def test_what_counts(self):
        s = size.sizes(str(self.tree()))
        # lib.rs: a, mod part, b; part.rs: p, q. Not the comment, the empty line, the test module, the file that is
        # built for tests only, the folder tests/, the folder target/.
        self.assertEqual(s["ui"], 5)
        self.assertEqual(s["hal"], 1)

    def test_the_table_of_two_trees(self):
        base, new = self.tree(), self.tree(extra="pub fn c() {}\n#[cfg(test)]\nmod more {\n    fn t() {}\n}\n")
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            size.main(["code-size.py", str(base), str(new)])
        self.assertIn("| ui | 5 | 6 | +1 |", out.getvalue(), "one line of code more; the new test module costs nothing")
        self.assertIn("| all | 8 | 9 | +1 |", out.getvalue())


if __name__ == "__main__":
    unittest.main()
