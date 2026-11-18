# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""No code for a host that is not Unix (ADR 0033, AGENTS.md "Never", milestone M1 #184): the tracked Rust code, the
Cargo manifests, the tools and the workflows hold no Windows branch, and the tree no Windows script."""
import re
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

# a Windows branch: cfg(windows), cfg(not(unix)), target_os = "windows", a Windows target or runner, an OS test in Python
BRANCH = re.compile(
    r'cfg\(\s*(not\(\s*unix\s*\)|windows)'
    r'|target_os\s*=\s*"windows"'
    r'|target\.\'?cfg\(windows'
    r'|[a-z0-9_]+-pc-windows-|windows-latest|windows-20\d\d'
    r'|os\.name\s*==\s*["\']nt["\']|sys\.platform\s*(==|\.startswith\()\s*["\']win'
    r'|platform\.system\(\)\s*==\s*["\']Windows["\']'
)
# where the rule is stated or its history told: the decision records and the agents' rules, and this test
TOLD = ("docs/adr/", "AGENTS.md", "tools/tests/test_no_windows_code.py")
SCRIPTS = (".bat", ".cmd", ".ps1")


def tracked():
    out = subprocess.run(["git", "ls-files"], cwd=ROOT, capture_output=True, text=True, check=True).stdout
    return [p for p in out.splitlines() if p]


class NoWindowsCode(unittest.TestCase):
    def test_the_pattern_finds_what_it_is_for(self):
        for s in ['#[cfg(windows)]', '#[cfg(not(unix))]', 'cfg!(target_os = "windows")',
                  "[target.'cfg(windows)'.dependencies]", 'runs-on: windows-latest', 'if os.name == "nt":',
                  "sys.platform == 'win32'", "sys.platform.startswith('win')", '--target x86_64-pc-windows-msvc',
                  'aarch64-pc-windows-msvc', 'i686-pc-windows-gnu', 'if platform.system() == "Windows":']:
            self.assertTrue(BRANCH.search(s), s)
        for s in ['#[cfg(unix)]', '#[cfg(target_os = "linux")]', 'cfg(not(target_os = "linux"))', 'WSL 2', 'windowed']:
            self.assertFalse(BRANCH.search(s), s)

    def test_no_windows_branch_or_script_in_the_tree(self):
        found = []
        for p in tracked():
            if p.startswith(TOLD) or p == TOLD[1]:
                continue
            if p.lower().endswith(SCRIPTS):
                found.append(f"{p}: a Windows script")
                continue
            if not p.endswith((".rs", ".toml", ".py", ".sh", ".yml", ".yaml")):
                continue
            try:
                text = (ROOT / p).read_text(encoding="utf-8")
            except (UnicodeDecodeError, FileNotFoundError):
                continue
            for n, line in enumerate(text.splitlines(), 1):
                if BRANCH.search(line):
                    found.append(f"{p}:{n}: {line.strip()}")
        self.assertEqual(found, [], "code for a host that is not Unix (ADR 0033):\n" + "\n".join(found))


if __name__ == "__main__":
    unittest.main()
