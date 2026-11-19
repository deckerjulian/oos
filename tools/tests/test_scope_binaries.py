# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""What tools/scope-binaries.sh leaves out has to stay data the programs never act on (ADR 0068): no Rust code of the
tree reads the place of a panic. With `-Zlocation-detail=none` such code would compare equal while it does something
else on the scope, and a change would pass `unit` without a run. Reads the sources; builds nothing."""
import re
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
# what hands a program the file, line or column of a panic or of its caller
READS_A_PLACE = re.compile(r"Location::caller|panic::set_hook|panic::take_hook|PanicHookInfo|PanicInfo|\.location\(\)"
                           r"|#\[track_caller\]")


class ScopeBinaries(unittest.TestCase):
    def test_no_code_reads_the_place_of_a_panic(self):
        files = subprocess.run(["git", "ls-files", "*.rs"], cwd=ROOT, capture_output=True, text=True,
                               check=True).stdout.split()
        self.assertGreater(len(files), 100, "the tree's Rust files")
        found = [f"{f}:{n}: {line.strip()}" for f in files
                 for n, line in enumerate((ROOT / f).read_text(errors="replace").splitlines(), 1)
                 if READS_A_PLACE.search(line)]
        self.assertEqual(found, [], "code that reads a panic's place: tools/scope-binaries.sh would not see a change "
                                    "of it (ADR 0068); such code needs ADR 0068 looked at again first")

    def test_the_pattern_finds_what_it_is_for(self):
        for text in ("let at = std::panic::Location::caller();", "std::panic::set_hook(Box::new(f));",
                     "#[track_caller]", "info.location().unwrap()"):
            self.assertRegex(text, READS_A_PLACE)
        self.assertNotRegex("let location = 3; // a plain word", READS_A_PLACE)


if __name__ == "__main__":
    unittest.main()
