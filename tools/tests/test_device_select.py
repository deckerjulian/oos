# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/device_select.py: which device tests a change runs, how retries and a known run judge a result, and how the
register sequences of the unit are compared with this computer's. No scope."""
import re
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
import device_select as d  # noqa: E402

# every test of device-test.py, by name, as the runner lists them
SRC = (ROOT / "tools" / "device-test.py").read_text()
NAMES = re.findall(r"^@test\([^\n]*\)\n(?:@\w+\n)*def (\w+)\(", SRC, re.M)


def chosen(*paths):
    return set(d.select(NAMES, d.areas_for(paths)[0]))


class Selection(unittest.TestCase):
    def test_every_area_names_a_test_that_exists(self):
        for area, rxs in d.AREAS.items():
            for rx in rxs:
                self.assertTrue(any(re.search(rx, n) for n in NAMES), f"{area}: {rx} matches no test")

    def test_every_test_is_found_here(self):
        self.assertEqual(len(NAMES), SRC.count("\n@test("))

    def test_the_smoke_tests_exist(self):
        for n in d.SMOKE:
            self.assertIn(n, NAMES)

    def test_documents_and_ci_run_the_smoke_tests_only(self):
        self.assertEqual(chosen("README.md", "docs/scpi.md", "wiki/Home.md", ".github/workflows/ci.yml",
                                "tools/check.sh"), set(d.SMOKE))

    def test_a_roll_change_runs_roll_and_the_time_axis_not_the_web(self):
        c = chosen("hal/src/dho/roll.rs")
        self.assertIn("time_axis_at_every_channel_count", c)
        self.assertIn("roll_and_scan_show_the_signal", c)
        self.assertIn("plan_matches_this_build", c)
        self.assertNotIn("web_pages_and_downloads", c)
        self.assertLess(len(c), len(NAMES) // 3)

    def test_the_image_the_update_and_the_test_itself_run_everything(self):
        for p in ("rootfs/etc/init.d/rcS", "hal/src/update/hash.rs", "ui/Cargo.toml", "tools/device-test.py"):
            self.assertEqual(chosen(p), set(NAMES), p)

    def test_a_file_no_rule_knows_runs_everything(self):
        self.assertEqual(chosen("somewhere/new.bin"), set(NAMES))

    def test_scpi_runs_the_remote_tests(self):
        c = chosen("ui/src/scpi.rs")
        self.assertIn("vxi11_answers", c)
        self.assertNotIn("roll_with_all_loops", c)


class Judge(unittest.TestCase):
    def test_a_pass_on_the_second_attempt_is_flaky(self):
        res, detail = d.judge("t", [("FAIL", "1 of 24"), ("PASS", "")], {})
        self.assertEqual(res, "FLAKY")
        self.assertIn("1 of 24", detail)

    def test_a_failure_the_known_run_has_too_is_known(self):
        self.assertEqual(d.judge("t", [("FAIL", "x"), ("FAIL", "y")], {"t": "FAIL"})[0], "KNOWN")
        self.assertEqual(d.judge("t", [("FAIL", "x"), ("FAIL", "y")], {"t": "KNOWN"})[0], "KNOWN")

    def test_a_failure_the_known_run_passed_stays_a_failure_with_every_attempt(self):
        res, detail = d.judge("t", [("FAIL", "a"), ("FAIL", "b")], {"t": "PASS"})
        self.assertEqual(res, "FAIL")
        self.assertIn("a / b", detail)

    def test_one_pass_is_a_pass(self):
        self.assertEqual(d.judge("t", [("PASS", "ok")], {"t": "FAIL"}), ("PASS", "ok"))


HOST = """### plan 1 ch
  wr 0x1000 0x1
   # /home/me/oos/local/cal/cal_adc.hex: 1876 bytes, crc ok
### afg sine
afg Sine 1000 Hz: 0x3070 0x5121 (no cal file)
  Bar1 { off: 12400, val: 20769 }
### la thr
la: zero codes [1, 2]
"""
UNIT = """### plan 1 ch
  wr 0x1000 0x1
   # /rigol/data/cal_adc.hex: 1876 bytes, crc ok
### afg sine
afg Sine 1000 Hz: 0x3070 0x523f (cal_afg.hex)
  Bar1 { off: 12400, val: 21055 }
### la thr
la: zero codes [1, 2]
probe present, thresholds 0x01500150
"""


class Plan(unittest.TestCase):
    def test_the_setup_is_no_difference(self):
        self.assertEqual(d.plan_compare(HOST, UNIT), ([], ["### afg sine"]))

    def test_a_changed_register_is_named_with_its_section(self):
        diff, _ = d.plan_compare(HOST, UNIT.replace("0x1000 0x1", "0x1000 0x2"))
        self.assertEqual(len(diff), 1)
        self.assertIn("### plan 1 ch line 1", diff[0])

    def test_a_missing_section_is_a_difference(self):
        diff, _ = d.plan_compare(HOST, UNIT.split("### la thr")[0])
        self.assertEqual(diff, ["### la thr: only on the host"])


if __name__ == "__main__":
    unittest.main()
