# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/layers.py on small made-up trees: paths and use trees, the three crossings, test code, comments and strings,
the dependencies of Cargo.toml, and the numbers of --enforce and --lower."""
import contextlib
import importlib.util
import io
import shutil
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("layers", ROOT / "tools/layers.py")
lay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lay)


def paths(text):
    return [(line, "::".join(names)) for line, names in lay.paths(list(lay.dup.tokens(text)))]


class Paths(unittest.TestCase):
    def test_a_path_as_written_on_the_line_of_its_last_name(self):
        self.assertEqual(paths("let x = a::b::\n    c(d);"), [(1, "let"), (1, "x"), (2, "a::b::c"), (2, "d")])
        self.assertEqual(paths("::oos_hal::dho::seq"), [(1, "oos_hal::dho::seq")])

    def test_a_use_tree_is_its_leaves_written_out(self):
        text = "use oos_hal::dho::{hz,\n    seq::{self, Settings as S}, tpu::*};\nuse crate::board;"
        self.assertEqual(paths(text), [(1, "oos_hal::dho::hz"), (2, "oos_hal::dho::seq"),
                                       (2, "oos_hal::dho::seq::Settings"), (2, "oos_hal::dho::tpu::*"),
                                       (3, "crate::board")])
        self.assertEqual(paths("pub(crate) use std::{fs, os::unix};"),
                         [(1, "pub"), (1, "crate"), (1, "std::fs"), (1, "std::os::unix")])


class Tree(unittest.TestCase):
    FILES = {
        "ui/src/lib.rs": ("//! The crate; `board::dho` in a comment is no path.\n"
                          "use crate::board::dho::hw;\n"
                          "use oos_hal::{dho, model};\n"
                          "fn f() { oos_hal::dho::seq::frame_bytes(); oos_hal::dho::seq::frame_bytes(); }\n"
                          'fn g() -> &str { "oos_hal::dho" }\n'
                          "fn h() { oos_hal::model::platform(); }\n"
                          "#[cfg(test)]\nmod tests {\n    use crate::board::dho::hw::settings;\n}\n"),
        "ui/src/board/mod.rs": "pub mod dho;\npub type Active = dho::Dho;\nuse crate::board::dho::hw;\n"
                               "use oos_hal::dho::seq;\n",
        "ui/src/board/dho/hw.rs": "use oos_hal::dho::{seq, tpu};\n",
        "ui/src/ui/la_tests.rs": "use oos_hal::dho::la;\n",
        "hal/src/lib.rs": "//! The hal.\nuse oos_dsp::fft;\nuse crate::dho;\nfn f() { libc::ioctl(); }\n",
        "dsp/src/lib.rs": ("//! The dsp; `/dev/mem` in a comment.\n"
                           "use std::{fs, os::unix::io::RawFd, f32::consts::PI};\n"
                           "fn f() { libc::ioctl(0, 1); std::fs::read(\"/dev/mem\"); }\n"
                           "fn g(m: &Mmap) { m.mmap(); }\n"
                           "#[cfg(test)]\nmod tests {\n    use std::fs;\n}\n"),
        "fpgad/src/main.rs": "//! The tool.\nuse oos_hal::dho::seq;\n",
        "hal/Cargo.toml": ('[package]\nname = "oos-hal"\n\n[dependencies]\nlibc = "0.2"\n'
                           'oos-dsp = { path = "../dsp" }\n\n[dev-dependencies]\nserde = "1"\n'),
        "dsp/Cargo.toml": ('[package]\nname = "oos-dsp"\n\n[dependencies]\n\n'
                           '[target.\'cfg(unix)\'.dependencies.libc]\nversion = "0.2"\n'),
    }

    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="oos-layers-"))
        self.addCleanup(shutil.rmtree, self.root)
        for rel, text in self.FILES.items():
            (self.root / rel).parent.mkdir(parents=True, exist_ok=True)
            (self.root / rel).write_text(text)

    def result(self):
        return {key: items for key, _, items in lay.counts(str(self.root))}

    def test_board(self):
        self.assertEqual(self.result()["board"], [
            "ui/src/board/mod.rs:4 use oos_hal::dho::seq;  (oos_hal::dho::seq)",
            "ui/src/lib.rs:2 use crate::board::dho::hw;  (crate::board::dho::hw)",
            "ui/src/lib.rs:3 use oos_hal::{dho, model};  (oos_hal::dho)",
            "ui/src/lib.rs:4 fn f() { oos_hal::dho::seq::frame_bytes(); oos_hal::dho::seq::frame_bytes(); }  "
            "(oos_hal::dho::seq::frame_bytes)",
        ], "board/ may name board::dho, board/dho/ oos_hal::dho; tests, comments, strings and fpgad do not count")

    def test_hal_up(self):
        self.assertEqual(self.result()["hal-up"], [
            "hal/src/lib.rs:2 use oos_dsp::fft;  (oos_dsp::fft)",
            'hal/Cargo.toml:6 oos-dsp = { path = "../dsp" }  (dependency oos-dsp)',
        ], "libc is hal's own; a dev-dependency serves the tests")

    def test_dsp_hw(self):
        self.assertEqual(self.result()["dsp-hw"], [
            "dsp/src/lib.rs:2 use std::{fs, os::unix::io::RawFd, f32::consts::PI};  (std::fs)",
            "dsp/src/lib.rs:2 use std::{fs, os::unix::io::RawFd, f32::consts::PI};  (std::os::unix::io::RawFd)",
            'dsp/src/lib.rs:3 fn f() { libc::ioctl(0, 1); std::fs::read("/dev/mem"); }  ("/dev/mem")',
            'dsp/src/lib.rs:3 fn f() { libc::ioctl(0, 1); std::fs::read("/dev/mem"); }  (libc::ioctl)',
            'dsp/src/lib.rs:3 fn f() { libc::ioctl(0, 1); std::fs::read("/dev/mem"); }  (std::fs::read)',
            "dsp/src/lib.rs:4 fn g(m: &Mmap) { m.mmap(); }  (mmap)",
            "dsp/Cargo.toml:6 [target.'cfg(unix)'.dependencies.libc]  (dependency libc)",
        ])

    def run_tool(self, *argv):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = lay.main([*argv, str(self.root)])
        return code, out.getvalue()

    def test_prints_one_line_and_exits_0(self):
        self.assertEqual(self.run_tool(), (0, "layers: board 4, hal-up 2, dsp-hw 7 (--list names them)\n"))
        code, out = self.run_tool("--list")
        self.assertEqual(code, 0)
        self.assertIn("hal-up: 2 names of the crates above hal in hal\n  hal/src/lib.rs:2 use oos_dsp::fft;", out)

    def numbers(self, text=None):
        path = self.root / lay.NUMBERS
        if text is not None:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        return path.read_text()

    def test_enforce_and_lower(self):
        self.numbers("4 board\n7 dsp-hw\n2 hal-up\n")
        self.assertEqual(self.run_tool("--enforce"), (0, "layers: board 4, hal-up 2, dsp-hw 7 (--list names them)\n"
                                                         "layers: 3 numbers as recorded in tools/layers.txt\n"))
        self.numbers("3 board\n9 dsp-hw\n2 hal-up\n")
        code, out = self.run_tool("--enforce")
        self.assertEqual(code, 1)
        self.assertIn("up: board 3 -> 4 (--list names them)\n", out)
        self.assertIn("down: dsp-hw 9 -> 7 (run tools/layers.py --lower)\n", out)
        code, out = self.run_tool("--lower")
        self.assertEqual(code, 1, "board went up: --lower does not make that pass")
        self.assertEqual(self.numbers(), "3 board\n7 dsp-hw\n2 hal-up\n", "the rise stays out of the file")


class ThisTree(unittest.TestCase):
    def test_the_layers_of_this_tree_are_counted(self):
        keys = [key for key, _, _ in lay.counts(str(ROOT))]
        self.assertEqual(keys, ["board", "hal-up", "dsp-hw"])
        self.assertEqual(sorted(keys), sorted(lay.numbers.read(str(ROOT), lay.NUMBERS)))


if __name__ == "__main__":
    unittest.main()
