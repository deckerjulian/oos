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
            ":CURSor:CAX[?]", ":TIMebase[:MAIN]:SCALe[?]", ":MEASure:ITEM?"])

    def test_queries_not_asked(self):
        # the OFFSet line sets, it does not ask
        self.assertEqual(self.result()["queries-tests"], ([":CHANnel<n>:OFFSet[?]", ":CURSor:CAX[?]"], 6))

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

    def test_prints_and_exits_0(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(cov.main([str(self.root)]), 0)
        self.assertEqual(out.getvalue(), "scpi-coverage: words-docs 2/8, words-tests 4/8, queries-tests 2/6, "
                                         "fields-scpi 2/4 missing (--list names them)\n")
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(cov.main(["--list", str(self.root)]), 0)
        self.assertIn("fields-scpi: 2 of 4 setup fields", out.getvalue())
        self.assertIn("\n  grid\n  bus\n", out.getvalue())


if __name__ == "__main__":
    unittest.main()
