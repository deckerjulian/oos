# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/layers.py on small made-up trees: paths and use trees, the three crossings of the crates, test code, comments
and strings, the dependencies of Cargo.toml, the numbers of --enforce and --lower; the layers inside oos-ui: where a
module is placed, which layer may name which, and a module without a layer."""
import contextlib
import importlib.util
import io
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("layers", ROOT / "tools/layers.py")
lay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lay)


# the counts inside oos-ui, in the order of their layers
UI_KEYS = ["ui-base-up", "ui-model-up", "ui-settings-up", "ui-board-up", "ui-signal-up", "ui-store-up", "ui-screen-up",
           "ui-command-up", "ui-transport-skip", "ui-unplaced"]
UI_NONE = "layers of oos-ui: " + ", ".join(key + " 0" for key in UI_KEYS) + " (--list names them)\n"


def paths(text):
    return [(line, "::".join(names)) for line, names in lay.paths(list(lay.dup.tokens(text)))]


def made_up(case, files):
    """A tree of `files` ({path: text}) in a folder that goes with the test `case`; its top."""
    root = Path(tempfile.mkdtemp(prefix="oos-layers-"))
    case.addCleanup(shutil.rmtree, root)
    for rel, text in files.items():
        (root / rel).parent.mkdir(parents=True, exist_ok=True)
        (root / rel).write_text(text)
    return root


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
        self.root = made_up(self, self.FILES)

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

    def test_the_crates_cross_no_layer_of_the_ui(self):
        self.assertEqual([self.result()[key] for key in UI_KEYS], [[]] * len(UI_KEYS),
                         "the crate root is the wiring, board/ one layer, and hal, dsp and fpgad have none")

    def test_prints_two_lines_and_exits_0(self):
        self.assertEqual(self.run_tool(), (0, "layers: board 4, hal-up 2, dsp-hw 7 (--list names them)\n" + UI_NONE))
        code, out = self.run_tool("--list")
        self.assertEqual(code, 0)
        self.assertIn("hal-up: 2 names of the crates above hal in hal\n  hal/src/lib.rs:2 use oos_dsp::fft;", out)

    @staticmethod
    def full(text):
        """A file of the numbers: `text` (the three of the crates), then the counts inside oos-ui, 0 in this tree."""
        return text + "".join("0 %s\n" % key for key in sorted(UI_KEYS))

    def numbers(self, text=None):
        path = self.root / lay.NUMBERS
        if text is not None:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(self.full(text))
        return path.read_text()

    def test_enforce_and_lower(self):
        self.numbers("4 board\n7 dsp-hw\n2 hal-up\n")
        self.assertEqual(self.run_tool("--enforce"),
                         (0, "layers: board 4, hal-up 2, dsp-hw 7 (--list names them)\n" + UI_NONE
                          + "layers: 13 numbers as recorded in tools/layers.txt\n"))
        self.numbers("3 board\n9 dsp-hw\n2 hal-up\n")
        code, out = self.run_tool("--enforce")
        self.assertEqual(code, 1)
        self.assertIn("up: board 3 -> 4 (--list names them)\n", out)
        self.assertIn("down: dsp-hw 9 -> 7 (run tools/layers.py --lower)\n", out)
        code, out = self.run_tool("--lower")
        self.assertEqual(code, 1, "board went up: --lower does not make that pass")
        self.assertEqual(self.numbers(), self.full("3 board\n7 dsp-hw\n2 hal-up\n"), "the rise stays out of the file")


class UiTable(unittest.TestCase):
    def test_a_layer_names_only_layers_below_it(self):
        order = [layer for layer, _, _, _ in lay.UI_LAYERS]
        self.assertEqual(order, ["base", "model", "settings", "board", "signal", "store", "screen", "command",
                                 "transport", "main"])
        for layer, (key, may) in lay.MAY.items():
            self.assertEqual(sorted(may - set(order[:order.index(layer)])), [], "%s names a layer above it" % layer)
        self.assertEqual([key for key, _ in lay.MAY.values()] + [lay.UNPLACED], UI_KEYS, "one count per layer")
        self.assertNotIn("main", lay.MAY, "the wiring may name everything and has no count")
        self.assertEqual(lay.MAY["transport"][1], {"base", "command"}, "a transport hands programs over (ADR 0022)")

    def test_a_module_is_in_one_row(self):
        modules = [module for _, _, _, row in lay.UI_LAYERS for module in row.split()]
        self.assertEqual(sorted(modules), sorted(set(modules)))

    def test_the_longest_row_places_a_name(self):
        tops = {"ui", "model", "newthing"}
        self.assertEqual(lay.ui_layer(["ui", "la", "LaRec"], tops), "screen")
        self.assertEqual(lay.ui_layer(["model"], tops), "model")
        self.assertEqual(lay.ui_layer([], tops), "main", "the crate root")
        self.assertEqual(lay.ui_layer(["frame_loop"], tops), "main", "a name of the crate root that is no module")
        self.assertIsNone(lay.ui_layer(["newthing", "f"], tops), "a module without a row")
        with mock.patch.dict(lay.PLACE, {"ui::la": "signal"}):
            self.assertEqual(lay.ui_layer(["ui", "la", "LaRec"], tops), "signal")
            self.assertEqual(lay.ui_layer(["ui", "lab", "f"], tops), "screen", "names are compared, not letters")

    def test_a_visibility_names_nothing(self):
        def left(text):
            return "".join(piece for _, _, piece in lay.without_visibility(list(lay.dup.tokens(text))))
        self.assertEqual(left("pub(crate) fn f(); pub(in crate::ui) mod m; pub(super) use a::b;"),
                         "pubfnf();pubmodm;pubusea::b;")
        self.assertEqual(left("struct P(pub (crate::a::A, B));"), "structP(pub(crate::a::A,B));", "a tuple as a field")

    def test_the_modules_written_out_in_a_file(self):
        text = "mod a {\n    fn f() {\n    }\n    pub mod b {\n    }\n}\nfn g() {\n}\nmod c;\n"
        self.assertEqual(lay.inner_modules(list(lay.dup.tokens(text))), [(4, 5, ["a", "b"]), (1, 6, ["a"])])

    def test_a_file_declared_with_a_path_is_the_module_its_declaration_says(self):
        files = {
            "ui/src/main.rs": "mod store;\nmod board;\n",
            "ui/src/store.rs": ('#[path = "odd.rs"]\nmod odd;\nmod outer {\n    pub mod deep {\n'
                                '        #[path = "part.rs"]\n        pub(crate) mod inner;\n    }\n}\n'),
            "ui/src/odd.rs": "",
            "ui/src/store/outer/deep/part.rs": "",
            "ui/src/board/mod.rs": 'mod a {\n    #[path = "b.rs"]\n    mod c;\n}\n',
            "ui/src/board/a/b.rs": "",
        }
        modules = lay.ui_modules({rel: list(lay.dup.tokens(text)) for rel, text in files.items()})
        self.assertEqual(modules, {
            "ui/src/main.rs": [], "ui/src/store.rs": ["store"], "ui/src/board/mod.rs": ["board"],
            "ui/src/odd.rs": ["store", "odd"],
            "ui/src/store/outer/deep/part.rs": ["store", "outer", "deep", "inner"],
            "ui/src/board/a/b.rs": ["board", "a", "c"],
        }, "with the modules written out around the declaration, and the file where Rust looks for it")

    def test_a_path_is_read_from_its_module(self):
        target = lay.ui_target
        self.assertEqual(target(["store", "png"], ["crate", "ui", "Ui"]), ["ui", "Ui"])
        self.assertEqual(target(["store", "png"], ["self", "crc"]), ["store", "png", "crc"])
        self.assertEqual(target(["store", "png"], ["super", "paths", "dir"]), ["store", "paths", "dir"])
        self.assertEqual(target(["store", "png"], ["super", "super", "ui", "*"]), ["ui"])
        self.assertIsNone(target(["store"], ["super", "super", "ui"]), "above the crate root")
        self.assertIsNone(target(["store"], ["super", "*"]), "the crate root itself")
        self.assertIsNone(target(["store"], ["crate"]))
        self.assertIsNone(target(["store"], ["ui", "Ui"]), "a name of a `use`, or of another crate")
        self.assertIsNone(target(["store"], ["std", "fs", "read"]))


class UiTree(unittest.TestCase):
    """Every layer in a made-up oos-ui: what it may name is not listed, what it may not is, with its rule."""
    FILES = {
        "ui/src/main.rs": "mod gfx;\nuse ui::Ui;\nfn main() { crate::http::serve(); crate::model::Model::new(); }\n",
        "ui/src/input.rs": "use crate::board::Board;\nuse crate::ui::Ev;\n",
        "ui/src/gfx.rs": ("use crate::font::Font;\nuse crate::model::{Model,\n    NCH};\n"
                          "fn f() { crate::frame_loop(); crate::newthing::f(); }\n"),
        "ui/src/font.rs": "pub struct Font;\n",
        "ui/src/sysconf.rs": "pub struct Flag;\n",
        "ui/src/model.rs": ("//! The model; `crate::ui::Ui` in a comment is no path.\n"
                            "use crate::gfx::Rgb;\nuse crate::math::MathOp;\npub(crate) mod channels;\n"
                            "pub(in crate::ui) fn f() {}\n"
                            "fn g(u: &crate::ui::Ui) { crate::ui::la::available(); crate::ui::la::available(); }\n"
                            'fn h() -> &str { "crate::store::save" }\n'
                            "#[cfg(test)]\nmod tests {\n    use crate::store;\n}\n"),
        "ui/src/model/channels.rs": ("use super::*;\nuse super::super::board::board;\n"
                                     "fn f(&self) { self::helper(); super::defaults(); }\n"),
        "ui/src/math.rs": "pub enum MathOp {}\n",
        "ui/src/settings.rs": "use crate::model::Model;\nfn f() { crate::scpi::kw(); crate::store::p_bool(); }\n",
        "ui/src/board/mod.rs": ("use crate::model::Model;\nuse crate::sysconf::Flag;\npub struct Frame;\n"
                                "fn f() -> crate::wave::Record { crate::ui::la::LaRec }\n"),
        "ui/src/wave.rs": ("use crate::board::Frame;\nuse crate::model::*;\n"
                           "mod imp {\n    use super::Frame;\n    use super::super::ui::la;\n"
                           "    pub mod deep {\n        use super::super::super::store::save;\n    }\n}\n"
                           "use super::settings::Setting;\n"),
        "ui/src/store.rs": ("use crate::settings::Setting;\nuse crate::wave::Record;\n"
                            "use crate::{board, ui::VIEWS_MAX};\n#[path = \"odd.rs\"]\nmod odd;\n"),
        "ui/src/store/png.rs": "use crate::ui::Ui;\n",
        "ui/src/odd.rs": "use super::super::http::base64;\n",
        "ui/src/ui.rs": ("use crate::{board::Board, model::*, settings, store, wave::*};\n"
                         "fn f() { crate::cmd::run(); crate::vxi11::CORE_PORT; crate::gpu::Gpu; }\n"),
        "ui/src/ui/page.rs": ("use super::*;\nuse super::super::http::STYLE;\n"
                              "#[path = \"page_part.rs\"]\npub(super) mod part;\n"),
        "ui/src/ui/page_part.rs": "use super::super::Ui;\nuse super::super::super::scpi::Scpi;\n",
        "ui/src/ui/page_tests.rs": "use crate::http::Server;\n",
        "ui/src/scpi.rs": ("use crate::model::*;\nuse crate::ui::Ui;\n"
                           "fn f() { crate::http::base64(); crate::input::Ev; }\n"),
        "ui/src/cmd.rs": "use crate::ui::*;\n",
        "ui/src/http.rs": ("use crate::gfx::Canvas;\nuse crate::hislip::PORT;\nuse crate::scpi::{Program, Scpi};\n"
                           "use crate::ui::Ui;\nfn f() { crate::store::png(); crate::model::NCH; }\n"),
        "ui/src/hislip.rs": "pub const PORT: u16 = 4880;\n",
        "ui/src/vxi11.rs": "pub const CORE_PORT: u16 = 1024;\n",
        "ui/src/gpu.rs": "use crate::ui::TraceJob;\n",
        "ui/src/newthing.rs": "//! New.\nuse crate::ui::Ui;\n",
        "ui/src/newdir/part.rs": "//! A part.\n",
    }

    def setUp(self):
        self.result = {key: items for key, _, items in lay.counts(str(made_up(self, self.FILES)))}

    def test_base_names_no_other_layer(self):
        self.assertEqual(self.result["ui-base-up"], [
            "ui/src/gfx.rs:2 use crate::model::{Model,  (base names model: crate::model::Model)",
            "ui/src/gfx.rs:3 NCH};  (base names model: crate::model::NCH)",
            "ui/src/gfx.rs:4 fn f() { crate::frame_loop(); crate::newthing::f(); }  "
            "(base names main: crate::frame_loop)",
        ], "a use tree is its leaves, each on its line; a module without a row is no layer to name")

    def test_the_model_names_base(self):
        self.assertEqual(self.result["ui-model-up"], [
            "ui/src/model.rs:6 fn g(u: &crate::ui::Ui) { crate::ui::la::available(); crate::ui::la::available(); }  "
            "(model names screen: crate::ui::Ui)",
            "ui/src/model.rs:6 fn g(u: &crate::ui::Ui) { crate::ui::la::available(); crate::ui::la::available(); }  "
            "(model names screen: crate::ui::la::available)",
            "ui/src/model/channels.rs:2 use super::super::board::board;  "
            "(model names board: super::super::board::board)",
        ], "its own layer (math), a visibility, a comment, a string, a test and `self` do not count")

    def test_settings_and_board_name_base_and_the_model(self):
        self.assertEqual(self.result["ui-settings-up"], [
            "ui/src/settings.rs:2 fn f() { crate::scpi::kw(); crate::store::p_bool(); }  "
            "(settings names command: crate::scpi::kw)",
            "ui/src/settings.rs:2 fn f() { crate::scpi::kw(); crate::store::p_bool(); }  "
            "(settings names store: crate::store::p_bool)",
        ])
        self.assertEqual(self.result["ui-board-up"], [
            "ui/src/board/mod.rs:4 fn f() -> crate::wave::Record { crate::ui::la::LaRec }  "
            "(board names screen: crate::ui::la::LaRec)",
            "ui/src/board/mod.rs:4 fn f() -> crate::wave::Record { crate::ui::la::LaRec }  "
            "(board names signal: crate::wave::Record)",
        ])

    def test_signal_names_the_board_and_reads_super_from_a_module_written_out(self):
        self.assertEqual(self.result["ui-signal-up"], [
            "ui/src/wave.rs:5 use super::super::ui::la;  (signal names screen: super::super::ui::la)",
            "ui/src/wave.rs:7 use super::super::super::store::save;  "
            "(signal names store: super::super::super::store::save)",
            "ui/src/wave.rs:10 use super::settings::Setting;  (signal names settings: super::settings::Setting)",
        ], "`super::Frame` in `mod imp` is wave's own")

    def test_the_store_names_settings_and_signal_and_a_file_is_placed_by_its_folder_or_its_declaration(self):
        self.assertEqual(self.result["ui-store-up"], [
            "ui/src/odd.rs:1 use super::super::http::base64;  (store names transport: super::super::http::base64)",
            "ui/src/store.rs:3 use crate::{board, ui::VIEWS_MAX};  (store names board: crate::board)",
            "ui/src/store.rs:3 use crate::{board, ui::VIEWS_MAX};  (store names screen: crate::ui::VIEWS_MAX)",
            "ui/src/store/png.rs:1 use crate::ui::Ui;  (store names screen: crate::ui::Ui)",
        ], "odd.rs is `store::odd` by `#[path]`, so it has a layer and its `super::super` is the crate root")

    def test_the_screen_names_what_is_below_it(self):
        self.assertEqual(self.result["ui-screen-up"], [
            "ui/src/ui.rs:2 fn f() { crate::cmd::run(); crate::vxi11::CORE_PORT; crate::gpu::Gpu; }  "
            "(screen names command: crate::cmd::run)",
            "ui/src/ui.rs:2 fn f() { crate::cmd::run(); crate::vxi11::CORE_PORT; crate::gpu::Gpu; }  "
            "(screen names main: crate::gpu::Gpu)",
            "ui/src/ui.rs:2 fn f() { crate::cmd::run(); crate::vxi11::CORE_PORT; crate::gpu::Gpu; }  "
            "(screen names transport: crate::vxi11::CORE_PORT)",
            "ui/src/ui/page.rs:2 use super::super::http::STYLE;  (screen names transport: super::super::http::STYLE)",
            "ui/src/ui/page_part.rs:2 use super::super::super::scpi::Scpi;  "
            "(screen names command: super::super::super::scpi::Scpi)",
        ], "page_part.rs is `ui::page::part` by `#[path]`: its `super::super::Ui` is the screen's")

    def test_an_interpreter_names_the_screen_and_a_transport_only_the_interpreters(self):
        self.assertEqual(self.result["ui-command-up"], [
            "ui/src/scpi.rs:3 fn f() { crate::http::base64(); crate::input::Ev; }  "
            "(command names main: crate::input::Ev)",
            "ui/src/scpi.rs:3 fn f() { crate::http::base64(); crate::input::Ev; }  "
            "(command names transport: crate::http::base64)",
        ])
        self.assertEqual(self.result["ui-transport-skip"], [
            "ui/src/http.rs:4 use crate::ui::Ui;  (transport names screen: crate::ui::Ui)",
            "ui/src/http.rs:5 fn f() { crate::store::png(); crate::model::NCH; }  "
            "(transport names model: crate::model::NCH)",
            "ui/src/http.rs:5 fn f() { crate::store::png(); crate::model::NCH; }  "
            "(transport names store: crate::store::png)",
        ], "another transport (hislip), base and the interpreter are its own")

    def test_super_in_a_path_module_inside_a_module_written_out(self):
        root = made_up(self, {
            "ui/src/main.rs": "mod ui;\nmod wave;\n",
            "ui/src/ui.rs": "pub struct Ui;\n",
            "ui/src/wave.rs": 'pub struct Record;\nmod outer {\n    #[path = "../part.rs"]\n    mod inner;\n}\n',
            "ui/src/wave/part.rs": "use super::super::Record;\nuse super::super::super::ui::Ui;\n",
        })
        self.assertEqual([items for _, _, items in lay.counts(str(root)) if items], [[
            "ui/src/wave/part.rs:2 use super::super::super::ui::Ui;  "
            "(signal names screen: super::super::super::ui::Ui)",
        ]], "part.rs is `wave::outer::inner`, not `wave::part`: two `super` are wave, three the crate root")

    def test_a_module_without_a_layer_is_counted_and_its_paths_are_not(self):
        self.assertEqual(self.result["ui-unplaced"], [
            "ui/src/newdir/part.rs:1 //! A part.  (no row for the module newdir::part in UI_LAYERS of tools/layers.py)",
            "ui/src/newthing.rs:1 //! New.  (no row for the module newthing in UI_LAYERS of tools/layers.py)",
        ])
        self.assertEqual(sum(len(items) for items in self.result.values()), 3 + 3 + 2 + 2 + 3 + 4 + 5 + 2 + 3 + 2,
                         "the wiring (main.rs, input.rs, gpu.rs) names everything")


class ThisTree(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.result = lay.counts(str(ROOT))

    def test_the_layers_of_this_tree_are_counted(self):
        keys = [key for key, _, _ in self.result]
        self.assertEqual(keys, ["board", "hal-up", "dsp-hw"] + UI_KEYS)
        self.assertEqual(sorted(keys), sorted(lay.numbers.read(str(ROOT), lay.NUMBERS)))

    def test_every_ui_module_has_a_layer(self):
        unplaced = [items for key, _, items in self.result if key == lay.UNPLACED]
        self.assertEqual(unplaced, [[]], "a new module of oos-ui gets a row in UI_LAYERS of tools/layers.py (ADR 0049)")

    def test_every_row_is_a_module_of_this_tree(self):
        texts = lay.dup.read_tree(str(ROOT))
        modules = lay.ui_modules({rel: list(lay.dup.tokens(texts[rel])) for rel in texts if rel.startswith(lay.UI)})
        there = {"::".join(module) for module in modules.values()} | lay.crate_modules(texts)
        self.assertEqual(sorted(module for module in lay.PLACE if module not in there), [],
                         "a row of UI_LAYERS for a module that is gone")


if __name__ == "__main__":
    unittest.main()
