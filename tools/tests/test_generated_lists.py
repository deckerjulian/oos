# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/adr-index.py and tools/changelog.py (ADR 0065) on small made-up trees and on this tree: the status of a
record in the words of the index, the index between its markers, a change file and its faults, the merge of changes
into `## Unreleased`, a release."""
import contextlib
import importlib.util
import io
import os
import shutil
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, ROOT / "tools" / file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


idx = load("adr_index", "adr-index.py")
log = load("changelog", "changelog.py")


class Tree(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.root)

    def write(self, rel, text):
        path = os.path.join(self.root, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)


class Status(unittest.TestCase):
    def test_brackets_go_and_amendments_are_listed(self):
        self.assertEqual(idx.status(" accepted (2026-11-17, by the maintainer)"), "accepted")
        self.assertEqual(idx.status(" proposed"), "proposed")
        self.assertEqual(idx.status(" superseded by ADR 0059 (Batch is the only draw path)"), "superseded by ADR 0059")
        self.assertEqual(
            idx.status(" accepted (2026-11-17, with (nested) words), amended by ADR 0039 (the agent) by ADR 0041 (x)"
                       " and by ADR 0042 (y)"),
            "accepted, amended by ADR 0039, ADR 0041 and ADR 0042")
        self.assertEqual(idx.status(" accepted, amended by ADR 0058 (a decoder)"), "accepted, amended by ADR 0058")


class Index(Tree):
    def record(self, folder, number, title, status):
        self.write("docs/adr/%s/%s-x.md" % (folder, number),
                   "# ADR %s: %s\n\n- Status: %s\n- Date: 2026-11-18\n" % (number, title, status))

    def test_rows_by_number_with_folder_and_status(self):
        self.record("process", "0002", "Second", "accepted (2026-11-18,\n  by the maintainer)")
        self.record("code", "0001", "First", "proposed")
        self.assertEqual(idx.table(self.root), idx.HEAD + "| [0001](code/0001-x.md) | code | First | proposed |\n"
                         "| [0002](process/0002-x.md) | process | Second | accepted |\n")

    def test_a_number_twice_or_a_record_without_status_is_refused(self):
        self.record("code", "0001", "First", "proposed")
        self.record("system", "0001", "Again", "proposed")
        with self.assertRaisesRegex(ValueError, "0001"):
            idx.rows(self.root)
        shutil.rmtree(os.path.join(self.root, "docs/adr/system"))
        self.write("docs/adr/code/0003-y.md", "# ADR 0003: No status\n")
        with self.assertRaisesRegex(ValueError, "Status"):
            idx.rows(self.root)

    def test_written_between_the_markers_only(self):
        text = "intro\n%s\nold rows\n%s\nafter\n" % (idx.BEGIN, idx.END)
        self.assertEqual(idx.written(text, "new\n"), "intro\n%s\nnew\n%s\nafter\n" % (idx.BEGIN, idx.END))
        with self.assertRaises(ValueError):
            idx.written("no markers\n", "new\n")

    def test_this_tree_is_as_written(self):
        with open(ROOT / idx.README, encoding="utf-8") as f:
            text = f.read()
        self.assertEqual(idx.written(text, idx.table(str(ROOT))), text, "run tools/adr-index.py")


class Changes(Tree):
    LOG = "# Changes\n\n## Unreleased\n\nIntro.\n\nAdded:\n\n- a\n\nFixed:\n\n- b\n\n## 0.1.0\n\nAdded:\n\n- old\n"

    def test_a_change_and_its_faults(self):
        self.assertEqual(log.parse("x.md", "Fixed:\n\n- one\n  more\n- two\n"),
                         ("Fixed:", ["- one", "  more", "- two"]))
        for text, fault in [("Fixes:\n\n- a\n", "first line"), ("Fixed:\n- a\n", "empty line"),
                            ("Fixed:\n\n- a", "newline"), ("Fixed:\n\n- a\nb\n", "neither"),
                            ("Fixed:\n\n- \n", "empty line"), ("Fixed:\n\n- a\n- \n", "without text"),
                            ("Fixed:\n\n- " + "x" * 120 + "\n", "columns")]:
            with self.assertRaisesRegex(ValueError, fault):
                log.parse("x.md", text)

    def test_items_go_to_the_end_of_their_part_and_a_missing_part_is_added_in_order(self):
        _, section, _ = log.split(self.LOG)
        out = log.merged(section, [("1.md", "Fixed:", ["- c"]), ("2.md", "Removed:", ["- r"]),
                                   ("3.md", "Added:", ["- d"])])
        self.assertEqual("\n".join(out), "Intro.\n\nAdded:\n\n- a\n- d\n\nRemoved:\n\n- r\n\nFixed:\n\n- b\n- c")

    def test_a_release_writes_the_section_and_removes_the_files(self):
        self.write(log.LOG, self.LOG)
        self.write("changelog.d/README.md", "# how\n")
        self.write("changelog.d/fix.md", "Fixed:\n\n- c\n")
        old = log.ROOT
        log.ROOT = self.root
        try:
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(log.main(["changelog.py", "--release", "0.2.0"]), 0)
        finally:
            log.ROOT = old
        with open(os.path.join(self.root, log.LOG), encoding="utf-8") as f:
            self.assertEqual(f.read(), "# Changes\n\n## 0.2.0\n\nIntro.\n\nAdded:\n\n- a\n\n"
                                       "Fixed:\n\n- b\n- c\n\n## 0.1.0\n\nAdded:\n\n- old\n")
        self.assertEqual(os.listdir(os.path.join(self.root, "changelog.d")), ["README.md"])

    def test_without_an_unreleased_section_the_release_goes_above_the_newest(self):
        self.write(log.LOG, "# Changes\n\n## 0.1.0\n\nAdded:\n\n- old\n")
        self.write("changelog.d/b.md", "Fixed:\n\n- c\n")
        self.write("changelog.d/a.md", "Added:\n\n- a\n")
        old = log.ROOT
        log.ROOT = self.root
        try:
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(log.main(["changelog.py"]), 0)
                self.assertEqual(log.main(["changelog.py", "--release", "0.2.0"]), 0)
        finally:
            log.ROOT = old
        self.assertTrue(out.getvalue().startswith("## Unreleased\n\nAdded:\n\n- a\n\nFixed:\n\n- c\n"))
        with open(os.path.join(self.root, log.LOG), encoding="utf-8") as f:
            self.assertEqual(f.read(), "# Changes\n\n## 0.2.0\n\nAdded:\n\n- a\n\nFixed:\n\n- c\n\n"
                                       "## 0.1.0\n\nAdded:\n\n- old\n")

    def test_this_tree_has_good_changes(self):
        log.fragments(str(ROOT))


if __name__ == "__main__":
    unittest.main()
