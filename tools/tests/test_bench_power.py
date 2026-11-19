# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/bench-power.sh against stand-ins for the outlet (curl, or a program) and the scope (ssh): what it switches,
and when it refuses to cut the power (ADR 0070). No network, no outlet, no scope."""
import os
import shutil
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

# curl URL...: appends the URL to $LOG and answers like a Shelly or a Tasmota plug that is on
CURL = r'''#!/bin/sh
for a; do url=$a; done
echo "$url" >> "$LOG"
case $url in
*Switch.GetStatus*) echo '{"id":0,"output":true}' ;;
*cmnd=Power) echo '{"POWER":"OFF"}' ;;
*) echo '{}' ;;
esac
'''
# ssh ... COMMAND: the scope answers with $SSH_ANSWER, or not at all when $SSH_DOWN is set
SSH = r'''#!/bin/sh
[ -z "${SSH_DOWN:-}" ] || exit 255
printf '%s' "${SSH_ANSWER:-}"
'''
# the program of `cmd:`: logs what it was asked
OUTLET = r'''#!/bin/sh
echo "outlet $1" >> "$LOG"
[ "$1" != status ] || echo on
'''


class BenchPower(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="oos-power-"))
        self.addCleanup(shutil.rmtree, self.dir)
        for name, text in (("curl", CURL), ("ssh", SSH), ("outlet", OUTLET)):
            p = self.dir / name
            p.write_text(text)
            p.chmod(p.stat().st_mode | stat.S_IXUSR)
        self.log = self.dir / "log"

    def run_tool(self, *args, power="shelly:plug", scope="scope", **env):
        e = dict(os.environ, PATH=f"{self.dir}:{os.environ['PATH']}", LOG=str(self.log), OOS_POWER=power)
        e.pop("OOS_SCOPE", None)
        if scope:
            e["OOS_SCOPE"] = scope
        e.update(env)
        out = subprocess.run(["sh", str(ROOT / "tools" / "bench-power.sh"), *args], env=e, capture_output=True,
                             text=True)
        calls = self.log.read_text().split() if self.log.exists() else []
        self.log.unlink(missing_ok=True)
        return out.returncode, out.stdout + out.stderr, calls

    def test_status_and_switching(self):
        rc, out, calls = self.run_tool("status")
        self.assertEqual((rc, out.strip()), (0, "on"))
        self.assertEqual(calls, ["http://plug/rpc/Switch.GetStatus?id=0"])
        rc, out, calls = self.run_tool("status", power="tasmota:plug")
        self.assertEqual((rc, out.strip()), (0, "off"))
        rc, _, calls = self.run_tool("on")
        self.assertEqual((rc, calls), (0, ["http://plug/rpc/Switch.Set?id=0&on=true"]))
        rc, _, calls = self.run_tool("off")
        self.assertEqual((rc, calls), (0, ["http://plug/rpc/Switch.Set?id=0&on=false"]))
        rc, _, calls = self.run_tool("cycle", "--wait", "0")
        self.assertEqual((rc, calls), (0, ["http://plug/rpc/Switch.Set?id=0&on=false",
                                           "http://plug/rpc/Switch.Set?id=0&on=true"]))
        rc, _, calls = self.run_tool("cycle", "--wait", "0", power=f"cmd:{self.dir / 'outlet'}")
        self.assertEqual((rc, calls), (0, ["outlet", "off", "outlet", "on"]))

    def test_no_power_cut_while_an_update_may_write(self):
        rc, out, calls = self.run_tool("off", SSH_ANSWER="412")
        self.assertEqual((rc, calls), (2, []), "oos-update runs: nothing switched")
        self.assertIn("oos-update runs", out)
        rc, out, calls = self.run_tool("cycle", "--wait", "0", SSH_DOWN="1")
        self.assertEqual((rc, calls), (2, []), "the scope does not answer: it may be installing")
        rc, out, calls = self.run_tool("off", scope=None)
        self.assertEqual((rc, calls), (2, []), "nobody to ask")
        rc, _, calls = self.run_tool("off", "--force", SSH_DOWN="1")
        self.assertEqual((rc, calls), (0, ["http://plug/rpc/Switch.Set?id=0&on=false"]), "--force switches anyway")
        rc, _, calls = self.run_tool("on", SSH_DOWN="1")
        self.assertEqual(rc, 0, "switching on asks nobody")

    def test_wrong_use(self):
        self.assertEqual(self.run_tool("status", power="")[0], 2)
        self.assertEqual(self.run_tool("status", power="zigbee:x")[0], 2)
        self.assertEqual(self.run_tool("cycle", "--wait", "x")[0], 2)
        self.assertEqual(self.run_tool("reboot")[0], 2)


if __name__ == "__main__":
    unittest.main()
