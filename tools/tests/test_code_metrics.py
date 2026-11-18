# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/code-metrics.py on small made-up trees: function spans, `.unwrap()`, module headers, test code, and the
numbers of --enforce and --lower."""
import contextlib
import importlib.util
import io
import shutil
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("code_metrics", ROOT / "tools/code-metrics.py")
met = importlib.util.module_from_spec(spec)
spec.loader.exec_module(met)


def func(name, n, indent=""):
    """A function of n lines (n >= 3): its signature, n - 2 statements, its closing brace."""
    return f"{indent}fn {name}() {{\n" + f"{indent}    x();\n" * (n - 2) + f"{indent}}}\n"


class Functions(unittest.TestCase):
    def test_braces_in_strings_chars_comments_and_raw_strings_do_not_count(self):
        text = ('fn f() {\n'
                '    let s = "}}} {";\n'
                "    let c = '}';\n"
                "    let b = b'{';\n"
                '    // }\n'
                '    /* } /* nested } */ { */\n'
                '    let r = r#"}"# ;\n'
                '    let q = "a \\" }";\n'
                "    let l: &'static str = s;\n"
                '}\n'
                'fn g() {}\n')
        self.assertEqual(met.functions(text), [(10, 1, "f"), (1, 11, "g")])

    def test_a_nested_function_counts_on_its_own_and_for_the_outer_one(self):
        text = "fn outer() {\n    x();\n" + func("inner", 4, "    ") + "}\n"
        self.assertEqual(met.functions(text), [(4, 3, "outer::inner"), (7, 1, "outer")])

    def test_methods_are_named_with_their_type(self):
        text = ("impl<T: Fn() -> u8> Default for Model<T> where T: Copy {\n" + func("default", 3, "    ") + "}\n"
                "trait Draw {\n    fn size(&self) -> u32;\n" + func("draw", 3, "    ") + "}\n"
                "impl crate::a::Ui {\n" + func("step", 3, "    ") + "}\n")
        self.assertEqual([name for _, _, name in met.functions(text)], ["Model::default", "Draw::draw", "Ui::step"])

    def test_declarations_types_and_impl_trait_arguments_are_no_bodies(self):
        text = ('extern "C" {\n    fn c_call(x: [u8; 4]);\n}\n'
                "fn f(cb: fn(u8) -> u8, it: impl Iterator<Item = u8>) -> impl Fn() {\n    move || ()\n}\n")
        self.assertEqual(met.functions(text), [(3, 4, "f")])


class Unwraps(unittest.TestCase):
    def test_only_calls_in_code(self):
        text = ('// a.unwrap() in a comment\n'
                '/// b.unwrap() in a doc comment\n'
                'fn f() {\n'
                '    let s = "c.unwrap()";\n'
                '    let r = r#"d.unwrap()"#;\n'
                '    /* e.unwrap() */\n'
                '    x.unwrap();\n'
                '    y\n        .unwrap();\n'
                '    z.unwrap_or(0);\n'
                '    w.0.unwrap();\n'
                '}\n')
        self.assertEqual(met.unwraps(text), 3)


class Headers(unittest.TestCase):
    def test_a_header_before_the_first_line_of_code(self):
        spdx = "// SPDX-License-Identifier: GPL-2.0-or-later\n"
        self.assertTrue(met.has_header(spdx + "\n//! What it is for.\nfn f() {}\n"))
        self.assertTrue(met.has_header("#![allow(dead_code)]\n//! What it is for.\n"))
        self.assertFalse(met.has_header(spdx + "use a::b;\n//! too late\n"))
        self.assertFalse(met.has_header("/// a doc comment of an item\nfn f() {}\n"))
        self.assertFalse(met.has_header(""))


class Tree(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="oos-metrics-"))
        self.addCleanup(shutil.rmtree, self.root)
        (self.root / "tools").mkdir()
        for crate in ("hal", "dsp", "fpgad"):
            (self.root / crate / "src").mkdir(parents=True)
            (self.root / crate / "src" / "lib.rs").write_text("//! A crate.\n")
        long_test = func("long_test", 200).replace("x();", "x.unwrap();")
        files = {
            "ui/src/lib.rs": ("//! The crate.\n" + func("long", 151) + func("short", 150)
                              + "fn u() {\n    a.unwrap();\n}\n"
                              + "#[cfg(test)]\nmod checks;\n#[cfg(test)]\nmod tests {\n" + long_test + "}\n"
                              + "pub mod inner {\n    #[cfg(test)]\n    mod tests {\n" + long_test + "    }\n}\n"),
            "ui/src/checks.rs": long_test,  # built for tests only, declared in its parent
            "ui/src/part_tests.rs": long_test,  # a test file by its name
            "ui/src/tests/more.rs": long_test,
            "ui/src/plain.rs": "// SPDX-License-Identifier: GPL-2.0-or-later\nfn p() {\n    b.unwrap();\n}\n",
        }
        for rel, text in files.items():
            (self.root / rel).parent.mkdir(parents=True, exist_ok=True)
            (self.root / rel).write_text(text)

    def run_tool(self, *argv):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = met.main([*argv, str(self.root)])
        return code, out.getvalue()

    def test_counts_without_test_code(self):
        result = {key: (n, items) for key, _, n, items in met.counts(str(self.root))}
        self.assertEqual(result["long-fns"], (1, ["151 ui/src/lib.rs:2 long"]), "150 lines are not long; tests not")
        self.assertEqual(result["unwraps"], (2, ["1 ui/src/lib.rs", "1 ui/src/plain.rs"]))
        self.assertEqual(result["headers"], (1, ["ui/src/plain.rs"]))

    def test_prints_one_line_and_exits_0(self):
        self.assertEqual(self.run_tool(), (0, "code-metrics: long-fns 1, unwraps 2, headers 1 (--list names them)\n"))
        code, out = self.run_tool("--list")
        self.assertEqual(code, 0)
        self.assertIn("long-fns: 1 functions over 150 lines\n  151 ui/src/lib.rs:2 long\n", out)
        self.assertIn("headers: 1 files without a //! header, of 5\n  ui/src/plain.rs\n", out)

    def numbers(self, text=None):
        path = self.root / met.NUMBERS
        if text is not None:
            path.write_text(text)
        return path.read_text()

    def test_enforce_passes_when_every_count_is_its_number(self):
        self.numbers("1 headers\n1 long-fns\n2 unwraps\n")
        code, out = self.run_tool("--enforce")
        self.assertEqual(code, 0, out)
        self.assertIn("code-metrics: 3 numbers as recorded in tools/code-metrics.txt\n", out)

    def test_enforce_fails_on_a_count_above_or_below_its_number_and_writes_nothing(self):
        self.numbers("1 headers\n0 long-fns\n5 unwraps\n")
        code, out = self.run_tool("--enforce")
        self.assertEqual(code, 1)
        self.assertIn("up: long-fns 0 -> 1 (--list names them)\n", out)
        self.assertIn("down: unwraps 5 -> 2 (run tools/code-metrics.py --lower)\n", out)
        self.assertEqual(self.numbers(), "1 headers\n0 long-fns\n5 unwraps\n", "--enforce writes nothing")

    def test_enforce_fails_on_a_missing_or_unknown_key(self):
        self.numbers("1 headers\n1 long-fns\n2 unwraps\n3 todos\n")
        code, out = self.run_tool("--enforce")
        self.assertEqual(code, 1)
        self.assertIn("unknown: todos 3 is no count of this tool: remove its line\n", out)
        self.numbers("1 headers\n1 long-fns\n")
        code, out = self.run_tool("--enforce")
        self.assertEqual(code, 1)
        self.assertIn("up: unwraps is new with 2: add `2 unwraps` to tools/code-metrics.txt\n", out)

    def test_lower_writes_what_went_down_and_never_raises(self):
        self.numbers("1 headers\n0 long-fns\n5 unwraps\n")
        code, out = self.run_tool("--lower")
        self.assertEqual(code, 1, "long-fns went up: --lower does not make that pass")
        self.assertIn("lowered: unwraps 5 -> 2\n", out)
        self.assertEqual(self.numbers(), "1 headers\n0 long-fns\n2 unwraps\n", "the rise stays out of the file")
        # once the rise is gone (here: the number raised by hand), --lower and --enforce pass
        self.numbers("1 headers\n1 long-fns\n2 unwraps\n")
        self.assertEqual(self.run_tool("--lower")[0], 0)
        self.assertEqual(self.run_tool("--enforce")[0], 0)

    def test_a_bad_line_of_the_numbers_is_an_error(self):
        self.numbers("many unwraps\n")
        with self.assertRaises(ValueError):
            met.read_numbers(str(self.root))


if __name__ == "__main__":
    unittest.main()
