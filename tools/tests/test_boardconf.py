# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""The check of the stock partition table that guards every write to a card (boot/boardconf.py check_parm), and the
scripts that use it: card-install.py, switch.py, sdcard-layout.py and the image's oos-oneshot. Offline, on card
images in a temporary folder; no card, no scope."""
import contextlib
import importlib.util
import io
import os
import struct
import subprocess
import sys
import tempfile
import unittest

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
sys.path.insert(0, os.path.join(ROOT, "boot"))
import boardconf  # noqa: E402

BOARD = boardconf.load()
BOARD_FILE = os.path.join(ROOT, "rootfs", "board", BOARD["OOS_BOARD"], "etc", "oos-board")
SECTOR = 512
# the partitions of the stock table up to the data partition, as its CMDLINE writes them (upper case hex)
STOCK = ("0x00002000@0x00002000(uboot),0x00002000@0x00004000(trust),0x00002000@0x00006000(misc),"
         "0x00008000@0x00008000(resource),0x0000C000@0x00010000(kernel),0x00010000@0x0001C000(boot),"
         "0x00020000@0x0002C000(recovery),0x00038000@0x0004C000(backup),0x00400000@0x00084000(userdata)")


def parm(parts=STOCK, magic=b"PARM"):
    text = ("FIRMWARE_VER:7.1\nMACHINE: 3399\nCMDLINE: console=ttyFIQ0 mtdparts=rk29xxsd:" + parts + "\n").encode()
    return (magic + struct.pack("<I", len(text)) + text).ljust(2 * SECTOR, b"\0")


def load_script(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


class CheckParm(unittest.TestCase):
    def test_the_stock_table_passes(self):
        self.assertEqual(boardconf.check_parm(parm()), [])

    def test_the_marks_are_the_union_of_what_the_tools_checked(self):
        marks = BOARD["OOS_PARM_MARKS"].split()
        for name in ("misc", "boot", "recovery"):
            self.assertTrue(any(m.endswith(f"({name})") for m in marks), name)

    def test_every_missing_mark_is_named(self):
        for m in BOARD["OOS_PARM_MARKS"].split():
            moved = STOCK.lower().replace(m.lower(), m.lower().replace("@0x", "@0x1"))
            self.assertEqual(boardconf.check_parm(parm(moved)), [m], m)

    def test_a_block_without_the_magic_fails(self):
        self.assertIn("PARM magic", boardconf.check_parm(parm(magic=b"KRNL")))
        self.assertIn("PARM magic", boardconf.check_parm(b"\0" * SECTOR))

    def test_case_and_nul_bytes_do_not_count(self):
        raw = parm(STOCK.lower(), magic=b"PaRM")
        self.assertEqual(boardconf.check_parm(raw), [])
        i = raw.index(b"(recovery)")
        self.assertEqual(boardconf.check_parm(raw[:i] + b"\0\0" + raw[i:]), [])


class Scripts(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def card(self, table, boot_magic=b"KRNL"):
        """A sparse card image with the PARM block and the start of `boot`."""
        p = os.path.join(self.tmp.name, "card.img")
        lba = int(BOARD["OOS_BOOT_LBA"])
        with open(p, "wb") as f:
            f.truncate((lba + 1) * SECTOR)
            f.seek(int(BOARD["OOS_PARM_LBA"]) * SECTOR)
            f.write(table)
            f.seek(lba * SECTOR)
            f.write(boot_magic.ljust(SECTOR, b"\0"))
        return p

    def test_the_tools_read_the_table_where_the_board_says(self):
        want = int(BOARD["OOS_PARM_LBA"])
        for name, path in (("card_install", "boot/card-install.py"), ("switch", "boot/switch.py"),
                           ("sdcard_layout", "tools/sdcard-layout.py")):
            self.assertEqual(load_script(name, os.path.join(ROOT, path)).PARM_LBA, want, path)

    def test_card_install_refuses_a_table_without_recovery(self):
        ci = load_script("card_install", os.path.join(ROOT, "boot", "card-install.py"))
        with open(self.card(parm()), "rb") as f, contextlib.redirect_stdout(io.StringIO()):
            ci.check_parm(f)
        with open(self.card(parm(STOCK.replace("(recovery)", "(other)"))), "rb") as f:
            with self.assertRaises(SystemExit) as e:
                ci.check_parm(f)
        self.assertIn("(recovery)", str(e.exception))

    def test_switch_refuses_a_table_without_recovery(self):
        run = [sys.executable, "-I", os.path.join(ROOT, "boot", "switch.py"), "show"]
        ok = subprocess.run(run + [self.card(parm())], capture_output=True, text=True)
        self.assertEqual(ok.returncode, 0, ok.stderr)
        bad = subprocess.run(run + [self.card(parm(STOCK.replace("(recovery)", "(other)")))],
                             capture_output=True, text=True)
        self.assertNotEqual(bad.returncode, 0)
        self.assertIn("(recovery)", bad.stderr)

    def oneshot(self, table, boot_magic):
        img = self.card(table, boot_magic)
        log = os.path.join(self.tmp.name, "log")
        env = dict(os.environ, OOS_BOARD_FILE=BOARD_FILE, OOS_DEV=img, OOS_LOG=log, OOS_TEST="1")
        r = subprocess.run(["sh", os.path.join(ROOT, "rootfs", "initramfs", "bin", "oos-oneshot")], env=env,
                           capture_output=True, text=True)
        with open(img, "rb") as f:
            f.seek(int(BOARD["OOS_BOOT_LBA"]) * SECTOR)
            magic = f.read(4)
        with open(log) as f:
            return r.returncode, f.read(), magic

    def test_oneshot_passes_the_stock_table(self):
        # boot already KRNL: the check passes and nothing is written
        code, log, magic = self.oneshot(parm(), b"KRNL")
        self.assertIn("boot already valid", log)
        self.assertEqual(magic, b"KRNL")

    def test_oneshot_refuses_a_table_without_recovery(self):
        code, log, magic = self.oneshot(parm(STOCK.replace("(recovery)", "(other)")), b"OOSK")
        self.assertEqual(code, 1)
        self.assertIn("unexpected partition table", log)
        self.assertEqual(magic, b"OOSK")


if __name__ == "__main__":
    unittest.main()
