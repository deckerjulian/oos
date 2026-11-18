# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/duplicates.py on small made-up trees: exact and loose copies, test code, the minimum, maximal blocks."""
import contextlib
import importlib.util
import io
import shutil
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("duplicates", ROOT / "tools/duplicates.py")
dup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dup)


def body(n, name="a", base=0):
    """n statements, each on its own line, with the names and numbers given; up to 21 of them differ even with the
    names and numbers left out (the loose mode)."""
    kinds = ("let ", "let mut ", "")
    return "".join(f"    {kinds[i // 7 % 3]}{name}{i} = {name}{i + 1}"
                   + "".join(f".step({base + i + k})" for k in range(i % 7 + 1)) + ";\n" for i in range(n))


def func(fname, inner):
    return f"pub fn {fname}() {{\n{inner}}}\n"


class Duplicates(unittest.TestCase):
    def tree(self, files):
        root = Path(tempfile.mkdtemp(prefix="oos-dup-"))
        self.addCleanup(shutil.rmtree, root)
        for crate in ("hal", "dsp", "ui", "fpgad"):
            (root / crate / "src").mkdir(parents=True)
        for rel, text in files.items():
            (root / rel).parent.mkdir(parents=True, exist_ok=True)
            (root / rel).write_text(text)
        return str(root)

    def run_tool(self, *argv):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = dup.main(["duplicates.py", *argv])
        self.assertEqual(code, 0, "a report: exit 0 whatever it finds")
        return out.getvalue()

    def blocks(self, root, minimum=8, loose=False):
        return dup.find(dup.read_tree(root), minimum, loose)

    def test_an_exact_copy_in_two_files(self):
        # the second copy has other comments, white space and brackets on lines of their own: the same code
        copy = "pub fn g() {\n" + body(10).replace(";\n", "; // why\n").replace("    let", "  let") + "\n\n}\n"
        root = self.tree({"dsp/src/a.rs": "// a file\n" + func("f", body(10)), "ui/src/b.rs": copy})
        blocks, lines = self.blocks(root)
        self.assertEqual(blocks, [(10, [("dsp/src/a.rs", 3, 12), ("ui/src/b.rs", 2, 11)])])
        self.assertEqual(lines, 10, "the lines of the places but the first")
        out = self.run_tool(root)
        self.assertEqual(out, "duplicates: 1 blocks, 10 duplicated lines (exact, >= 8 lines); "
                              "loose mode: 1 blocks, 11 duplicated lines (loose, >= 8 lines)\n", "loose: `fn f` is `fn g`")
        listed = self.run_tool("--list", root)
        self.assertIn("10 lines, 2 places:\n  dsp/src/a.rs:3-12\n  ui/src/b.rs:2-11\n", listed)

    def test_a_near_copy_only_in_the_loose_mode(self):
        root = self.tree({"hal/src/a.rs": func("f", body(9)), "fpgad/src/b.rs": func("g", body(9, "x", 100))})
        self.assertEqual(self.blocks(root), ([], 0))
        blocks, lines = self.blocks(root, loose=True)
        # the head of the function too: `pub fn f() {` is `pub fn g() {` with the names left out
        self.assertEqual(blocks, [(10, [("fpgad/src/b.rs", 1, 10), ("hal/src/a.rs", 1, 10)])])
        self.assertEqual(lines, 10)
        self.assertIn("1 blocks, 10 duplicated lines (loose", self.run_tool(root))
        self.assertIn("10 lines, 2 places:\n  fpgad/src/b.rs:1-10\n", self.run_tool("--list", "--loose", root))

    def test_test_code_is_not_read(self):
        test_mod = "#[cfg(test)]\nmod tests {\n" + func("t", body(10)) + "}\n"
        inner_mod = "pub mod inner {\n    #[cfg(test)]\n    mod tests {\n" + func("t", body(10)) + "    }\n}\n"
        root = self.tree({
            "ui/src/lib.rs": func("f", body(10)) + "#[cfg(test)]\nmod checks;\n" + test_mod + inner_mod,
            "ui/src/checks.rs": func("c", body(10)),  # built for tests only, declared in its parent
            "ui/src/tests.rs": func("t", body(10)),
            "ui/src/ui/menu_tests.rs": func("t", body(10)),
            "ui/src/scpi/tests/measure.rs": func("t", body(10)),
            "ui/tests/it.rs": func("t", body(10)),
        })
        self.assertEqual(sorted(dup.read_tree(root)), ["ui/src/lib.rs"])
        self.assertEqual(self.blocks(root), ([], 0))
        self.assertEqual(self.blocks(root, loose=True), ([], 0))

    def test_attributes_between_the_test_attribute_and_its_module(self):
        # `#[cfg(test)]`, then `#[path]` or another attribute, then `mod`: test code as without them; the file a
        # `#[path]` names is built for tests only, whatever its name
        by_path = '#[cfg(test)]\n#[path = "helper.rs"]\nmod checks;\n'
        allowed = "#[cfg(test)]\n#[allow(dead_code)]\n#[rustfmt::skip]\nmod tests {\n" + func("t", body(10)) + "}\n"
        own = '#[path = "own.rs"]\nmod own;\n'  # no test: `#[cfg(test)]` is not above it
        root = self.tree({
            "ui/src/lib.rs": func("f", body(10)) + by_path + own + allowed,
            "ui/src/helper.rs": func("h", body(10)),
            "ui/src/checks.rs": "pub fn not_declared_by_that_name() {}\n",
            "ui/src/own.rs": "pub fn own() {}\n",
            "ui/src/ui/page.rs": '#[cfg(test)]\n#[path = "../shared_checks.rs"]\nmod shared;\n',
            "ui/src/shared_checks.rs": func("s", body(10)),
        })
        texts = dup.read_tree(root)
        self.assertEqual(sorted(texts), ["ui/src/checks.rs", "ui/src/lib.rs", "ui/src/own.rs", "ui/src/ui/page.rs"])
        lines = texts["ui/src/lib.rs"].split("\n")
        self.assertEqual(lines[12:20], ["", "", "mod checks;", '#[path = "own.rs"]', "mod own;", "", "", ""],
                         "the attributes of a test module are blanked with it, the lines keep their numbers")
        self.assertNotIn("fn t()", texts["ui/src/lib.rs"])
        self.assertEqual(self.blocks(root), ([], 0))

    def test_the_test_attribute_does_not_lengthen_a_block(self):
        # the same 8 lines, each followed by `#[cfg(test)]` on a test-only function that differs: the attribute is
        # test syntax and must not count as a ninth equal line
        root = self.tree({
            "dsp/src/a.rs": body(8) + "#[cfg(test)]\nfn ta() { one(); }\n",
            "dsp/src/b.rs": body(8) + "#[cfg(test)]\nfn tb() { two(); }\n",
        })
        self.assertEqual(self.blocks(root)[1], 8)

    def test_a_block_under_the_minimum_is_not_reported(self):
        root = self.tree({"dsp/src/a.rs": func("f", body(7)), "dsp/src/b.rs": func("g", body(7))})
        self.assertEqual(self.blocks(root), ([], 0))
        self.assertEqual(len(self.blocks(root, minimum=7)[0]), 1)
        self.assertIn("1 blocks, 7 duplicated lines (exact, >= 7 lines)", self.run_tool("--min", "7", root))

    def test_a_match_grows_to_its_full_length(self):
        # 20 equal lines are one block of 20, not thirteen windows of 8; a third place with only the first 12 is a
        # block of its own with all three places, and its lines count once
        twenty = body(20)
        root = self.tree({
            "ui/src/a.rs": func("f", twenty),
            "ui/src/b.rs": func("g", "    let other = 1;\n" + twenty + "    let more = 2;\n"),
            "ui/src/c.rs": func("h", body(12) + "    let end = 3;\n"),
        })
        blocks, lines = self.blocks(root)
        self.assertEqual(blocks, [
            (20, [("ui/src/a.rs", 2, 21), ("ui/src/b.rs", 3, 22)]),
            (12, [("ui/src/a.rs", 2, 13), ("ui/src/b.rs", 3, 14), ("ui/src/c.rs", 2, 13)]),
        ])
        self.assertEqual(lines, 20 + 12)

    def test_two_places_in_one_file_do_not_overlap(self):
        # the same 10 lines twice in a row in one file: one block with two places, not a longer one with itself
        root = self.tree({"hal/src/a.rs": func("f", body(10) + body(10))})
        blocks, _ = self.blocks(root)
        self.assertEqual(blocks, [(10, [("hal/src/a.rs", 2, 11), ("hal/src/a.rs", 12, 21)])])

    def test_rows_of_a_table_are_no_copy(self):
        rows = "".join(f"    row(\n        {i},\n        \"name {i}\",\n        Kind::K{i},\n    ),\n" for i in range(30))
        root = self.tree({"ui/src/table.rs": "static T: [Row; 30] = [\n" + rows + "];\n"})
        self.assertEqual(self.blocks(root, loose=True), ([], 0))

    def test_comments_and_strings(self):
        # a `//` inside a string is no comment; block comments and raw strings span lines
        a = func("f", '    let u = "http://x";\n    /* a\n  comment */\n    let r = r#"two\nlines"#;\n' + body(8))
        b = func("g", '    let u = "http://y";\n    let r = r#"two\nlines"#;\n' + body(8))
        blocks, _ = self.blocks(self.tree({"dsp/src/a.rs": a, "dsp/src/b.rs": b}))
        self.assertEqual(blocks, [(9, [("dsp/src/a.rs", 5, 14), ("dsp/src/b.rs", 3, 12)])])


if __name__ == "__main__":
    unittest.main()
