# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/lib/board.sh and the tools that take the board family from OOS_BOARD (ADR 0067): the family, `dho` without
it, a name that is no family refused before anything is built or sent. No network, no build; needs sh."""
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LIB = ROOT / "tools/lib/board.sh"


def sh(script, root, board=None):
    env = {k: v for k, v in os.environ.items() if k != "OOS_BOARD"}
    if board is not None:
        env["OOS_BOARD"] = board
    return subprocess.run(["sh", "-euc", f'. "$0"; {script} "$1"', str(LIB), str(root)], env=env,
                          capture_output=True, text=True)


def tool(args, board, **extra):
    env = dict({k: v for k, v in os.environ.items() if k != "OOS_BOARD"}, OOS_BOARD=board, **extra)
    return subprocess.run(args, env=env, capture_output=True, text=True, timeout=60)


class Family(unittest.TestCase):
    def test_dho_without_oos_board(self):
        for board in (None, ""):
            r = sh("oos_board", ROOT, board)
            self.assertEqual((r.returncode, r.stdout), (0, "dho\n"), r.stderr)

    def test_a_family_of_the_tree(self):
        r = sh("oos_board", ROOT, "dho")
        self.assertEqual((r.returncode, r.stdout), (0, "dho\n"), r.stderr)

    def test_no_family_is_refused(self):
        for board in ("nope", "../dho", "-x", "DHO", "a b"):
            r = sh("oos_board", ROOT, board)
            self.assertNotEqual(r.returncode, 0, board)
            self.assertEqual(r.stdout, "", board)
            self.assertIn(f"OOS_BOARD='{board}'", r.stderr)
        self.assertIn("(has: dho", sh("oos_board", ROOT, "nope").stderr, "names the families there are")

    def test_the_families_dho_first(self):
        with tempfile.TemporaryDirectory() as d:
            for name in ("zz", "dho", "abc", "half"):
                (Path(d) / "rootfs/board" / name / "etc").mkdir(parents=True)
                if name != "half":
                    (Path(d) / "rootfs/board" / name / "etc/oos-board").write_text(f"OOS_BOARD={name}\n")
            r = sh("oos_families", d)
            self.assertEqual(r.stdout.split(), ["dho", "abc", "zz"], "a folder without etc/oos-board is none")
            self.assertEqual(sh("oos_board", d, "abc").stdout, "abc\n")
            self.assertNotEqual(sh("oos_board", d, "half").returncode, 0)

    def test_the_families_of_this_tree(self):
        names = sorted(p.name for p in (ROOT / "rootfs/board").iterdir() if p.is_dir())
        out = sh("oos_families", ROOT).stdout.split()
        self.assertEqual(out[0], "dho")
        self.assertEqual(sorted(out), names)


class Tools(unittest.TestCase):
    """Each tool stops at an unknown family before it builds, runs or sends anything."""

    def test_plan_matrix(self):
        r = tool(["sh", str(ROOT / "tools/plan-matrix.sh"), "/nonexistent/oos-fpgad"], "nope")
        self.assertEqual(r.returncode, 2, r.stderr)
        self.assertEqual(r.stdout, "")
        self.assertIn("OOS_BOARD='nope'", r.stderr)

    def test_compare_base(self):
        r = tool(["sh", str(ROOT / "tools/compare-base.sh"), "HEAD"], "nope")
        self.assertEqual(r.returncode, 2, r.stderr)
        self.assertIn("compare-base.sh: no board family to compare", r.stderr)
        self.assertNotIn("== base", r.stdout)

    def test_device_deploy(self):
        # the programs are there (any executable will do): the family is the next check, before the first ssh
        r = tool(["sh", str(ROOT / "tools/device-deploy.sh"), "scope.invalid"], "nope",
                 OOS_DEPLOY_UI="/bin/sh", OOS_DEPLOY_FPGAD="/bin/sh")
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn("OOS_BOARD='nope'", r.stderr)


if __name__ == "__main__":
    unittest.main()
