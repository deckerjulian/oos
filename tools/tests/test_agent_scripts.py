# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""tools/agent-target.py (which clean-up task comes next) and tools/scout.py (what is new in the forum and the fork),
with their readers replaced by made-up answers. No network, no cargo."""
import collections
import importlib.util
import io
import contextlib
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), ROOT / "tools" / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Target(unittest.TestCase):
    def setUp(self):
        self.t = load("agent-target")

    def task(self, per_crate):
        counted = {c: {lint: collections.Counter(files) for lint, files in f.items()} for c, f in per_crate.items()}
        self.t.findings = lambda crate: counted.get(crate, {})
        return self.t.lint_task()

    def test_the_crate_closest_to_clean_and_its_largest_lint(self):
        title, text = self.task({
            "ui": {"clippy::a": {"ui/src/x.rs": 30}},
            "hal": {"clippy::b": {"hal/src/y.rs": 2}, "clippy::c": {"hal/src/y.rs": 3, "hal/src/z.rs": 1}},
        })
        self.assertEqual(title, "Lints: clippy::c in hal (4)")
        self.assertIn("| `hal/src/y.rs` | 3 |", text)
        self.assertIn("**Behaviour:** unchanged.", text)
        self.assertIn("reports no `clippy::c` any more", text)

    def test_a_large_lint_is_given_file_by_file(self):
        files = {f"dsp/src/f{i}.rs": 30 - i for i in range(6)}  # 30 + 29 + 28 + 27 + 26 + 25 = 165
        title, text = self.task({"dsp": {"clippy::big": files}})
        self.assertEqual(title, "Lints: clippy::big in dsp (59 of 165)", "the files with the most, up to 80")
        self.assertIn("f0.rs", text)
        self.assertIn("f1.rs", text)
        self.assertNotIn("f2.rs", text)
        self.assertIn("in these files", text)

    def test_one_file_over_the_limit_is_still_given(self):
        title, _ = self.task({"dsp": {"clippy::big": {"dsp/src/huge.rs": 200}}})
        self.assertEqual(title, "Lints: clippy::big in dsp (200)")

    def test_nothing_to_do(self):
        self.assertIsNone(self.task({}))

    def test_the_paths_that_need_a_unit_come_last(self):
        title, text = self.task({
            "fpgad": {"clippy::a": {"fpgad/src/main.rs": 1}},
            "hal": {"clippy::b": {"hal/src/dho/seq.rs": 2, "hal/src/fsutil.rs": 1}},
            "ui": {"clippy::c": {"ui/src/board/dho/hw.rs": 9, "ui/src/ui.rs": 4}},
        })
        self.assertEqual(title, "Lints: clippy::b in hal (1)", "the one finding outside the unit paths, not fpgad")
        self.assertIn("fsutil.rs", text)
        self.assertNotIn("seq.rs", text)
        title, _ = self.task({"fpgad": {"clippy::a": {"fpgad/src/main.rs": 1}},
                              "hal": {"clippy::b": {"hal/src/dho/seq.rs": 2}}})
        self.assertEqual(title, "Lints: clippy::a in fpgad (1)", "only unit paths left: the smallest of them")

    def test_findings_are_read_from_cargo_and_counted_once(self):
        def message(code, file, line, level="warning"):
            return json.dumps({"reason": "compiler-message", "message": {
                "code": {"code": code}, "level": level,
                "spans": [{"is_primary": True, "file_name": file, "line_start": line, "column_start": 1}]}})
        out = "\n".join([
            message("clippy::a", "src/x.rs", 3), message("clippy::a", "src/x.rs", 3),  # the same, in two targets
            message("clippy::a", "src/x.rs", 9), message("dead_code", "src/y.rs", 1),
            json.dumps({"reason": "compiler-message", "message": {"code": None, "level": "warning", "spans": []}}),
            json.dumps({"reason": "build-finished", "success": True}), "not json",
        ])
        real = subprocess.run
        self.t.subprocess.run = lambda *a, **k: subprocess.CompletedProcess(a, 0, stdout=out, stderr="")
        try:
            found = self.t.findings("hal")
        finally:
            self.t.subprocess.run = real
        self.assertEqual(dict(found["clippy::a"]), {"hal/src/x.rs": 2})
        self.assertEqual(dict(found["dead_code"]), {"hal/src/y.rs": 1})

    def test_the_largest_file(self):
        repo = Path(tempfile.mkdtemp(prefix="oos-target-"))
        self.addCleanup(shutil.rmtree, repo)
        (repo / "ui").mkdir()
        (repo / "ui" / "small.rs").write_text("fn a() {}\n" * 10)
        subprocess.run(("git", "-C", str(repo), "init", "-q"), check=True)
        subprocess.run(("git", "-C", str(repo), "add", "-A"), check=True)
        self.t.ROOT = str(repo)
        self.assertIsNone(self.t.file_task(), "no file over the limit")
        (repo / "ui" / "big.rs").write_text("fn a() {}\n" * 2500)
        (repo / "fpgad").mkdir()
        (repo / "fpgad" / "huge.rs").write_text("fn a() {}\n" * 4000)
        subprocess.run(("git", "-C", str(repo), "add", "-A"), check=True)
        title, text = self.t.file_task()
        self.assertEqual(title, "Refactor: one part out of ui/big.rs (2500 lines)", "not the larger file in fpgad")
        self.assertIn("**Behaviour:** unchanged.", text)
        (repo / "ui" / "big.rs").write_text("fn a() {}\n" * 10)
        subprocess.run(("git", "-C", str(repo), "add", "-A"), check=True)
        self.assertEqual(self.t.file_task()[0], "Refactor: one part out of fpgad/huge.rs (4000 lines)")


def page(replies, slug="oos-an-open-operating-system-for-the-rigol-scopes"):
    """A forum page as far as the scout reads it: a link and a reply number for each post."""
    base = f"https://www.eevblog.com/forum/testgear/{slug}"
    return "".join(
        f'<h5 id="subject_{600 + n}"><a href="{base}/msg{600 + n}/#msg{600 + n}" rel="nofollow">Re: OOS</a></h5>'
        f'<div class="smalltext">&#171; <strong>Reply #{n} on:</strong> <strong>Today</strong> at 01:00 &#187;</div>'
        f"<div>text of post {n} that the scout never reads into the issue</div>" for n in replies)


class Scout(unittest.TestCase):
    def setUp(self):
        self.s = load("scout")
        self.pages = {}
        self.asked = []

        class Answer(io.BytesIO):
            def __enter__(self): return self
            def __exit__(self, *a): return False

        def urlopen(request, timeout=0):
            self.asked.append(request.full_url)
            start = int(request.full_url.rstrip("/").rsplit("/", 1)[1])
            if start not in self.pages:
                raise OSError("no such page")
            return Answer(page(self.pages[start]).encode())
        self.s.urllib.request.urlopen = urlopen

    def test_titles_are_plain_text(self):
        self.assertEqual(self.s.clean("fix: `x` @someone #12 [a](b) <i>"), "fix:  x   someone  12  a (b)  i")

    def test_new_replies_page_by_page(self):
        self.pages = {125: list(range(125, 150)), 150: list(range(150, 154))}
        replies, error = self.s.forum(139)
        self.assertIsNone(error)
        self.assertEqual([n for n, _ in replies], list(range(140, 154)))
        self.assertTrue(replies[0][1].endswith("/msg740/#msg740"))
        self.assertEqual(len(self.asked), 2)

    def test_nothing_new_and_a_forum_that_cannot_be_read(self):
        self.pages = {125: list(range(125, 140))}
        self.assertEqual(self.s.forum(139), ([], None))
        self.pages = {}
        replies, error = self.s.forum(139)
        self.assertEqual(replies, [])
        self.assertIn("could not be read", error)

    def run_main(self, state, pulls, issues, argv=("scout.py", "--print")):
        def gh(*args):
            if args[:2] == ("issue", "list"):
                return json.dumps([{"body": state}] if state is not None else [])
            if "pulls" in args[1]:
                return json.dumps(pulls)
            return json.dumps(issues)
        self.s.gh = gh
        self.s.sys.argv = list(argv)
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = self.s.main()
        return code, out.getvalue()

    def test_the_first_run_reports_nothing_old(self):
        self.pages = {0: list(range(1, 25)), 25: list(range(25, 31))}
        code, out = self.run_main(None, [], [])
        self.assertEqual(code, 0)
        self.assertIn("the forum is at reply 30", out)
        self.assertNotIn("msg6", out, "no link to an old post")
        self.assertRegex(out, r"<!-- scout: reply=30 fork=\S+ -->")

    def test_a_later_run_lists_links_and_titles_without_links(self):
        self.pages = {25: list(range(25, 34))}
        state = "text\n<!-- scout: reply=30 fork=2026-11-10T00:00:00Z -->\n"
        pulls = [{"number": 40, "title": "test(ui): wait @x on #3", "merged_at": "2026-11-12T00:00:00Z"},
                 {"number": 41, "title": "old", "merged_at": "2026-11-01T00:00:00Z"},
                 {"number": 42, "title": "closed, not merged", "merged_at": None}]
        issues = [{"number": 7, "title": "A new thing", "created_at": "2026-11-11T00:00:00Z"},
                  {"number": 8, "title": "A pull request", "created_at": "2026-11-11T00:00:00Z", "pull_request": {}}]
        code, out = self.run_main(state, pulls, issues)
        self.assertEqual(code, 0)
        self.assertIn("3 new replies", out)
        self.assertIn("- [reply 31](", out)
        self.assertIn("- merged, pull request 40: test(ui): wait  x on  3", out)
        self.assertNotIn("old", out)
        self.assertNotIn("closed, not merged", out)
        self.assertIn("- new issue 7: A new thing", out)
        self.assertNotIn("A pull request", out)
        self.assertNotIn("yottabytesolutions/oos#", out, "no link that would notify the fork")
        self.assertNotIn("github.com/yottabytesolutions", out)
        self.assertNotIn("text of post", out, "nothing of a post's text")
        self.assertRegex(out, r"<!-- scout: reply=33 fork=\S+ -->")

    def test_nothing_new_opens_nothing(self):
        self.pages = {25: list(range(25, 31))}
        code, out = self.run_main("<!-- scout: reply=30 fork=2026-11-10T00:00:00Z -->", [], [])
        self.assertEqual((code, out.strip()), (0, "nothing new"))


if __name__ == "__main__":
    unittest.main()
