# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/scpi-coverage.py on a small made-up tree: which words, queries and fields count, and what is missing."""
import contextlib
import importlib.util
import io
import shutil
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("scpi_coverage", ROOT / "tools/scpi-coverage.py")
cov = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cov)

SCPI_RS = """fn kw(tok: &str, long: &str) -> bool { true }
fn run(nodes: &[&str], arg: &str, ui: &mut Ui) {
    let n0 = nodes[0];
    if kw(n0, "CHANnel") && kw(nodes[1], "SCALe") { ui.m.vscale = 1; }
    if kw(nodes[1], "OFFSet") {}
    if kw(n[2], "POSition") {}
    if kw(suffix(n0).0, "SOURce") {}
    if kw(which, "CAX") || kw(which, "AXValue") {}
    if kw(arg, "ON") || kw(first, "MAXimum") {}
    if kw(last, "SECRet") || kw(last, "secret") {}
    let t = ui.m.tdiv;
}
"""

SCPI_MD = """# Reference

| Command | Meaning |
| --- | --- |
| `:CHANnel<n>:SCALe[?]` | Volts. |
| `:CHANnel<n>:OFFSet[?] <V>` | Volts. |
| `:CURSor:{MANual|TRACk}:{AXValue|BXValue}?` | Value. |
| `:CURSor:CAX[?]` / `:CBX[?]` | Time. |
| `:TIMebase[:MAIN]:SCALe[?]` | Seconds. |
| `:RUN` | Run, also `:CHAN1:POSition?` in the text. |
| `:MEASure:ITEM? <item>` | A measurement. |
| `:MEASure:ITEM? <pair>,<A>,<B>` | Twice in the table, counted once. |
"""

SCPI_TXT = """# answers
frames 5
scpi $S/a.txt :CHAN1:SCAL?
scpi $S/a.txt :timebase:main:scale?
scpi $S/a.txt :CURSor:TRACk:AXValue?
scpi $S/a.txt :MEASure:ITEM? VPP,CHAN1
scpi $S/a.txt :CHANnel1:OFFSet 0.5
"""

STORE_RS = """fn helper(m: &Model, kv: &mut dyn FnMut(&str, String)) {
    kv("bus", m.bus.to_string());
}

pub fn setup_text(m: &Model) -> String {
    kv("vdiv", m.vscale.to_string());
    kv("tdiv", m.tdiv.to_string());
    kv("view", m.dec_view(0).to_string());
    kv("grid", m.grid.to_string());
    helper(m, &mut kv);
    s
}

pub fn other(m: &Model) {
    let _ = m.not_written;
}
"""


class Coverage(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="oos-scpi-coverage-"))
        self.addCleanup(shutil.rmtree, self.root)
        for rel, text in ((cov.SCPI_RS, SCPI_RS), (cov.SCPI_MD, SCPI_MD), (cov.SCPI_TXT, SCPI_TXT),
                          (cov.STORE_RS, STORE_RS)):
            (self.root / rel).parent.mkdir(parents=True, exist_ok=True)
            (self.root / rel).write_text(text)

    def result(self):
        return {key: (missing, total) for key, _, missing, total in cov.counts(str(self.root))}

    def test_words_are_header_nodes_once_each(self):
        words = cov.command_words(SCPI_RS)
        self.assertEqual(words, ["CHANnel", "SCALe", "OFFSet", "POSition", "SOURce", "CAX", "AXValue", "SECRet"])

    def test_words_missing_from_docs(self):
        # POSition is named only as `:CHAN1:POSition?` in a text, which counts; `{AXValue|` counts as a choice
        self.assertEqual(self.result()["words-docs"], (["SOURce", "SECRet"], 8))

    def test_words_missing_from_tests(self):
        # the short form `:CHAN1:SCAL?` names neither CHANnel nor SCALe; `:CHANnel1:OFFSet` names CHANnel with its
        # number; `:timebase:main:scale?` names SCALe
        missing, total = self.result()["words-tests"]
        self.assertEqual(total, 8)
        self.assertEqual(missing, ["POSition", "SOURce", "CAX", "SECRet"])

    def test_numbered_forms_and_sequences(self):
        # a number after the word names it (`:CHANnel1`, `:MATH2`); not a longer word
        self.assertTrue(cov.named("CHANnel", ":CHANnel1:SCALe 1"))
        self.assertTrue(cov.named("MATH", "x :MATH2:DISPlay?"))
        self.assertFalse(cov.named("CHAN", ":CHANnel1"))
        # every command of a `;` sequence is asked, a `;` inside quotes splits nothing
        self.assertEqual(cov.commands(':CHAN1:SCAL 1;:CHAN1:SCAL?;:DISP:TEXT "a;b";*OPC?'),
                         [":CHAN1:SCAL", ":CHAN1:SCAL?", ":DISP:TEXT", "*OPC?"])
        self.assertEqual(cov.asked("scpi a.txt :RUN;:TRIG:MODE?\nscpi b.txt *IDN?\n"), [":RUN", ":TRIG:MODE?", "*IDN?"])

    def test_documented_queries(self):
        self.assertEqual(cov.documented_queries(SCPI_MD), [
            ":CHANnel<n>:SCALe[?]", ":CHANnel<n>:OFFSet[?]", ":CURSor:{MANual|TRACk}:{AXValue|BXValue}?",
            ":CURSor:CAX[?]", ":CURSor:CBX[?]", ":TIMebase[:MAIN]:SCALe[?]", ":MEASure:ITEM?"])

    def test_every_command_of_the_first_cell(self):
        # a relative command replaces as many last nodes of the first one as it has, after `, ` or ` / `; one with as
        # many nodes or more is absolute; a `|` inside backticks is no end of the cell; the other cells do not count
        row = cov.row_commands
        self.assertEqual(row("| `:TIMebase:DELay:ENABle[?]`, `:SCALe[?]`, `:OFFSet[?]` | Zoom, `:TIM:X?`. |"),
                         [":TIMebase:DELay:ENABle[?]", ":TIMebase:DELay:SCALe[?]", ":TIMebase:DELay:OFFSet[?]"])
        self.assertEqual(row("| `:TRIGger:RUNT:UPPer[?]` / `:LOWer[?]` | Volts. |"),
                         [":TRIGger:RUNT:UPPer[?]", ":TRIGger:RUNT:LOWer[?]"])
        self.assertEqual(row("| `:BUS<n>:IIC:SCLK:SOURce[?]`, `:SDA:SOURce[?]` | Roles. |"),
                         [":BUS<n>:IIC:SCLK:SOURce[?]", ":BUS<n>:IIC:SDA:SOURce[?]"])
        self.assertEqual(row("| `:CURSor:{MANual|TRACk}:CAX[?]` / `:CBX[?]` | Time. |"),
                         [":CURSor:{MANual|TRACk}:CAX[?]", ":CURSor:{MANual|TRACk}:CBX[?]"])
        self.assertEqual(row("| `:REFerence:VSCale[?] [<slot>,]<V>` / `:VOFFset[?] [<slot>,]<V>` | Scale. |"),
                         [":REFerence:VSCale[?]", ":REFerence:VOFFset[?]"])
        self.assertEqual(row("| `:TFORce` or `:TRIGger:FORCe` | Force. |"), [":TFORce", ":TRIGger:FORCe"])
        self.assertEqual(row("| `:DVM:SOURce[?]`, `:COUNter:SOURce[?]` | Sources. |"),
                         [":DVM:SOURce[?]", ":COUNter:SOURce[?]"])
        self.assertEqual(row("| `:AUToscale` / `:AUToscale?` | Autoset. |"), [":AUToscale", ":AUToscale?"])
        self.assertEqual(cov.nodes(":TIMebase[:MAIN]:SCALe[?]"), ["TIMebase[:MAIN]", "SCALe[?]"])
        self.assertEqual(row("| Command | Meaning |"), [])

    def test_queries_not_asked(self):
        # the OFFSet line sets, it does not ask; `:CBX[?]` is the second command of the CAX row
        self.assertEqual(self.result()["queries-tests"],
                         ([":CHANnel<n>:OFFSet[?]", ":CURSor:CAX[?]", ":CURSor:CBX[?]"], 7))

    def test_query_pattern(self):
        pat = cov.query_pattern(":TIMebase[:MAIN]:SCALe[?]")
        for asked in (":TIMebase:SCALe?", ":TIM:MAIN:SCAL?", ":timebase:main:scale?"):
            self.assertRegex(asked, "(?i)^" + pat + "$")
        for other in (":TIMebase:SCALe", ":TIMeb:SCALe?", ":TIMebase:DELay:SCALe?"):
            self.assertNotRegex(other, "(?i)^" + pat + "$")

    def test_setup_fields(self):
        # m.dec_view(0) is a method, m.not_written is outside what setup_text reaches, m.bus is in its helper
        self.assertEqual(cov.setup_fields(STORE_RS), ["vscale", "tdiv", "grid", "bus"])
        self.assertEqual(self.result()["fields-scpi"], (["grid", "bus"], 4))

    def test_the_files_of_the_folder_count(self):
        # a subsystem in its own file of ui/src/scpi/ (also one folder down) is the interpreter as much as scpi.rs
        sub = self.root / cov.SCPI_DIR / "tests"
        sub.mkdir(parents=True)
        (self.root / cov.SCPI_DIR / "grid.rs").write_text('fn g(n0: &str) { if kw(n0, "GRID") { m.grid = 1; } }\n')
        (sub / "bus.rs").write_text("fn t() { ui.m.bus = 2; }\n")
        self.assertEqual(self.result()["words-docs"], (["SOURce", "SECRet", "GRID"], 9))
        self.assertEqual(self.result()["fields-scpi"], ([], 4))

    def test_the_rows_of_the_settings_table_count(self):
        # a row with a command adds its nodes as words and its fields as reached; a row without one only its fields
        folder = self.root / cov.SETTINGS_DIR
        folder.mkdir(parents=True)
        (folder / "display.rs").write_text(
            'const T: [Setting; 2] = [\n'
            '    Setting { key: "grid", scpi: Scpi::Cmd("GRID:STYLe"), get: |m| m.grid, put: |m, v| m.grid = v },\n'
            '    Setting { key: "bar", scpi: Scpi::None { why: "x" }, get: |m| m.bar, put: |m, v| m.bar = v },\n'
            '];\n')
        (folder / "tests.rs").write_text('fn t() { let s = Setting { scpi: Scpi::Cmd("TEST") }; }\n')
        self.assertEqual(cov.settings_rows(str(self.root)), [("GRID:STYLe", ["grid"]), (None, ["bar"])])
        self.assertEqual(self.result()["words-docs"], (["SOURce", "SECRet", "GRID", "STYLe"], 10))
        self.assertEqual(self.result()["fields-scpi"], (["bus", "bar"], 5))

    def test_prints_and_exits_0(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(cov.main([str(self.root)]), 0)
        self.assertEqual(out.getvalue(), "scpi-coverage: words-docs 2/8, words-tests 4/8, queries-tests 3/7, "
                                         "fields-scpi 2/4 missing (--list names them)\n")
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(cov.main(["--list", str(self.root)]), 0)
        self.assertIn("fields-scpi: 2 of 4 setup fields", out.getvalue())
        self.assertIn("\n  grid\n  bus\n", out.getvalue())

    def numbers(self, text=None):
        """Writes tools/scpi-coverage.txt of the made-up tree (when `text` is given) and returns what is in it."""
        path = self.root / cov.NUMBERS
        if text is not None:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        return path.read_text() if path.exists() else None

    def enforce(self, *opts):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = cov.main(list(opts) + [str(self.root)])
        return code, out.getvalue()

    # the made-up tree counts words-docs 2, words-tests 4, queries-tests 3, fields-scpi 2
    AS_IS = "2 fields-scpi\n3 queries-tests\n2 words-docs\n4 words-tests\n"

    def test_enforce_passes_when_every_count_is_its_number(self):
        self.numbers(self.AS_IS)
        code, out = self.enforce("--enforce")
        self.assertEqual(code, 0)
        self.assertIn("scpi-coverage: 4 numbers as recorded in tools/scpi-coverage.txt\n", out)

    def test_enforce_fails_on_a_count_above_its_number(self):
        # a command word the docs do not name, one more than the file allows
        self.numbers(self.AS_IS.replace("2 words-docs", "1 words-docs"))
        code, out = self.enforce("--enforce")
        self.assertEqual(code, 1)
        self.assertIn("up: words-docs 1 -> 2 (--list names them)\n", out)
        self.assertNotIn("as recorded", out)

    def test_enforce_fails_on_a_count_below_its_number_and_asks_to_lower_it(self):
        self.numbers(self.AS_IS.replace("4 words-tests", "9 words-tests"))
        code, out = self.enforce("--enforce")
        self.assertEqual(code, 1)
        self.assertIn("down: words-tests 9 -> 4 (run tools/scpi-coverage.py --lower)\n", out)
        self.assertEqual(self.numbers(), self.AS_IS.replace("4 words-tests", "9 words-tests"),
                         "--enforce writes nothing")

    def test_lower_writes_what_went_down_and_never_raises(self):
        self.numbers("2 fields-scpi\n7 queries-tests\n1 words-docs\n4 words-tests\n")
        code, out = self.enforce("--lower")
        self.assertEqual(code, 1, "words-docs went up: --lower does not make that pass")
        self.assertIn("lowered: queries-tests 7 -> 3\n", out)
        self.assertIn("up: words-docs 1 -> 2", out)
        self.assertEqual(self.numbers(), "2 fields-scpi\n3 queries-tests\n1 words-docs\n4 words-tests\n")
        # once the rise is gone (here: the number raised by hand), --lower and --enforce pass
        self.numbers("2 fields-scpi\n7 queries-tests\n2 words-docs\n4 words-tests\n")
        self.assertEqual(self.enforce("--lower")[0], 0)
        self.assertEqual(self.numbers(), self.AS_IS)
        self.assertEqual(self.enforce("--enforce")[0], 0)

    def test_a_missing_or_unknown_key_fails(self):
        code, out = self.enforce("--enforce")
        self.assertEqual(code, 1, "no file: every count is new")
        self.assertIn("up: words-docs is new with 2: add `2 words-docs` to tools/scpi-coverage.txt\n", out)
        self.numbers(self.AS_IS + "3 words-wiki\n")
        code, out = self.enforce("--enforce")
        self.assertEqual(code, 1)
        self.assertIn("unknown: words-wiki 3 is no count of this tool", out)

    def test_a_line_that_is_no_number_and_key_is_an_error(self):
        self.numbers("two words-docs\n")
        with self.assertRaises(ValueError):
            cov.read_numbers(str(self.root))

    def test_enforce_and_lower_exclude_each_other(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            cov.main(["--enforce", "--lower", str(self.root)])


if __name__ == "__main__":
    unittest.main()
