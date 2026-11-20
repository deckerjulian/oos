# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/link-check.py on a small made-up tree: links, named files, and the item a document names behind a file."""
import importlib.util
import shutil
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("link_check", ROOT / "tools/link-check.py")
lc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lc)

RUST = """fn helper() {}

#[test]
fn every_row_is_complete() {}

pub(super) static ANALYSERS: &[u8] = &[];
"""


class Check(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        self.addCleanup(setattr, lc, "ROOT", lc.ROOT)
        lc.ROOT = str(self.root)
        (self.root / "ui/src").mkdir(parents=True)
        (self.root / "ui/src/tests.rs").write_text(RUST)
        (self.root / "tools").mkdir()
        (self.root / "tools/check.py").write_text("def main():\n    pass\n")

    def problems(self, text):
        (self.root / "PAGE.md").write_text(text)
        return [message for _, message in lc.check("PAGE.md", set())]

    def test_a_named_file_and_a_link_resolve(self):
        self.assertEqual(self.problems("`ui/src/tests.rs`, `ui/src/tests.rs:3` and [it](ui/src/tests.rs).\n"), [])
        self.assertEqual(self.problems("`ui/src/gone.rs`\n"), ["`ui/src/gone.rs`: no such file or folder"])

    def test_an_item_behind_a_file_is_declared_there(self):
        fine = "`ui/src/tests.rs::every_row_is_complete`, `ui/src/tests.rs::tests::helper`, " \
               "`ui/src/tests.rs::ANALYSERS` and `tools/check.py::main`.\n"
        self.assertEqual(self.problems(fine), [])
        self.assertEqual(self.problems("`ui/src/tests.rs::every_row`\n"),
                         ["`ui/src/tests.rs::every_row`: ui/src/tests.rs declares no every_row"])
        self.assertEqual(self.problems("`ui/src/gone.rs::helper`\n"), ["`ui/src/gone.rs`: no such file or folder"])

    def test_a_placeholder_and_a_fenced_block_are_not_checked(self):
        self.assertEqual(self.problems("`ui/src/<name>.rs::helper`\n```\n`ui/src/gone.rs`\n```\n"), [])


if __name__ == "__main__":
    unittest.main()
