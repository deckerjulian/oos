# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/ci-parts.sh: which parts of CI a set of changed files reaches. No network, no build; needs sh."""
import json
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def parts(*files, all_parts=False):
    args = ["sh", str(ROOT / "tools/ci-parts.sh")] + (["--all"] if all_parts else [])
    out = subprocess.run(args, input="".join(f + "\n" for f in files), capture_output=True, text=True, check=True)
    lines = dict(line.split("=", 1) for line in out.stdout.splitlines())
    layers = " ".join(entry["layers"] for entry in json.loads(lines["matrix"]))
    return set(layers.split()), lines["compare"] == "true"


EVERYTHING = {"test:hal", "test:ui", "fmt", "test:dsp", "test:fpgad", "target", "decoders:core", "decoders:each/1",
              "decoders:each/2"}
UI = {"test:ui", "fmt", "target", "decoders:core", "decoders:each/1", "decoders:each/2"}


class Parts(unittest.TestCase):
    def test_everything(self):
        self.assertEqual(parts(all_parts=True), (EVERYTHING, True))
        for f in (".github/workflows/ci.yml", "tools/check.sh", "tools/ci-parts.sh", "rustfmt.toml",
                  "ui/tools/decoder-features.sh", "a-file-nobody-knows"):
            self.assertEqual(parts(f), (EVERYTHING, True), f)

    def test_what_depends_on_what(self):
        self.assertEqual(parts("hal/src/dho/seq.rs"), (EVERYTHING - {"test:dsp"}, True), "hal reaches ui and fpgad")
        self.assertEqual(parts("keys/update.pub"), parts("hal/src/lib.rs"), "keys/ is compiled into hal")
        self.assertEqual(parts("dsp/src/fft.rs"), (UI | {"test:dsp"}, True), "dsp reaches ui")
        self.assertEqual(parts("ui/src/scpi.rs"), (UI, True))
        self.assertEqual(parts("fpgad/src/main.rs"), ({"fmt", "test:fpgad", "target"}, True), "fpgad reaches nothing")

    def test_what_reaches_less(self):
        self.assertEqual(parts("rootfs/init"), ({"test:hal"}, False), "read by the tests of hal, built into nothing")
        self.assertEqual(parts("tools/compare-base.sh"), (set(), True), "only the comparison")
        for f in ("docs/scpi.md", "wiki/Home.md", "README.md", "tools/scout.py", "tools/tests/test_x.py",
                  ".github/workflows/image.yml", "gpu/oos-gpu.c", "boot/build-dev-image.sh", "kernel/config"):
            self.assertEqual(parts(f), (set(), False), f)

    def test_together(self):
        self.assertEqual(parts("docs/scpi.md", "fpgad/src/main.rs", "dsp/src/fft.rs"),
                         (UI | {"test:dsp", "test:fpgad"}, True))


if __name__ == "__main__":
    unittest.main()
