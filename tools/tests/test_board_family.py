# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/lib/board.sh and the tools that take the board family from OOS_BOARD (ADR 0067): the family, `dho` without
it, a name that is no family refused before anything is built or sent. No network, no build; needs sh."""
import os
import shutil
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

    def test_check_builds_for_the_families(self):
        # the layers clippy, test and target build for OOS_FAMILIES, else every family; a wrong name stops the run
        def check(families):
            return tool(["sh", str(ROOT / "tools/check.sh"), "families"], "", OOS_FAMILIES=families)
        self.assertEqual(check("").stdout, "dho\n")
        self.assertEqual(check("dho").stdout, "dho\n")
        r = check("dho nope")
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn("check.sh: OOS_FAMILIES: nope", r.stderr)


# a stand-in for cargo: one line per call, the crate's folder, OOS_BOARD, CARGO_TARGET_DIR and the subcommand
FAKE_CARGO = '''#!/bin/sh
echo "$(basename "$PWD") ${OOS_BOARD:--} ${CARGO_TARGET_DIR:--} $1" >> "$CARGO_LOG"
'''


class CheckPerFamily(unittest.TestCase):
    """tools/check.sh in a made-up tree with a second family `mho` and a cargo that only writes down how it is called:
    each family is built with its own OOS_BOARD and target folder, dho as without OOS_BOARD."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name) / "tree"
        for f in ("tools/check.sh", "tools/lib/board.sh"):
            (self.root / f).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(ROOT / f, self.root / f)
        for crate in ("hal", "dsp", "ui", "fpgad"):
            (self.root / crate).mkdir()
        for name in ("mho", "dho"):
            (self.root / "rootfs/board" / name / "etc").mkdir(parents=True)
            (self.root / "rootfs/board" / name / "etc/oos-board").write_text(f"OOS_BOARD={name}\n")
        bin_dir = Path(self.tmp.name) / "bin"
        bin_dir.mkdir()
        (bin_dir / "cargo").write_text(FAKE_CARGO)
        (bin_dir / "cargo").chmod(0o755)
        self.log = Path(self.tmp.name) / "cargo.log"
        # HOME without a .cargo/bin: check.sh puts that in front of PATH
        self.env = dict(os.environ, HOME=self.tmp.name, PATH=f"{bin_dir}:{os.environ['PATH']}",
                        CARGO_LOG=str(self.log), OOS_BOARD="mho")
        self.env.pop("CARGO_TARGET_DIR", None)
        self.env.pop("OOS_FAMILIES", None)

    def tearDown(self):
        self.tmp.cleanup()

    def calls(self, *layers, families=None):
        env = dict(self.env, **({"OOS_FAMILIES": families} if families is not None else {}))
        self.log.write_text("")
        r = subprocess.run(["sh", str(self.root / "tools/check.sh"), *layers], env=env, capture_output=True,
                           text=True, timeout=60)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        return self.log.read_text().splitlines()

    def test_test(self):
        self.assertEqual(self.calls("test:hal"), ["hal - - test", "hal mho target/board-mho test"],
                         "dho without OOS_BOARD, though the caller set one")
        self.assertEqual(self.calls("test:dsp"), ["dsp - - test"], "dsp knows no board")
        self.assertEqual(self.calls("test:ui", families="mho"), ["ui mho target/board-mho test"])

    def test_clippy(self):
        calls = [c for c in self.calls("clippy") if c.split()[0] == "fpgad"]
        self.assertEqual(calls[:2], ["fpgad - - clippy", "fpgad mho target/board-mho clippy"])

    def test_target(self):
        calls = [c for c in self.calls("target", families="dho") if c.endswith(" check")]
        self.assertEqual(calls, ["ui - - check", "fpgad - - check"])
        calls = [c for c in self.calls("target") if c.endswith(" check")]
        self.assertEqual(calls, ["ui - - check", "fpgad - - check", "ui mho target/board-mho check",
                                 "fpgad mho target/board-mho check"])


if __name__ == "__main__":
    unittest.main()
