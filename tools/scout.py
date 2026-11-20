#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""scout.py: what is new where OOS is talked about and worked on, as one issue (docs/adr/process/0039).

    tools/scout.py            open the issue, if there is something new
    tools/scout.py --print    print it, change nothing

Two places:
  - the thread in the EEVblog forum: the replies since the last one reported, as links. Nothing of their text is
    read into the issue: a person reads the posts and decides what becomes an issue.
  - the fork yottabytesolutions/oos: the pull requests it merged and the issues it opened since the last report, as
    titles. They are written without links, so the fork gets no notices from here.

Where the last report ended is in a comment line at its end. No model reads or writes anything here. Python's
standard library and gh (GH_TOKEN); reads the forum and GitHub, writes one issue with the label `scout`.
"""
import datetime
import json
import os
import re
import subprocess
import sys
import urllib.request

REPO = os.environ.get("OOS_REPO") or os.environ.get("GITHUB_REPOSITORY") or "dekajoule/oos"
FORK = "yottabytesolutions/oos"
THREAD = "https://www.eevblog.com/forum/testgear/oos-an-open-operating-system-for-the-rigol-scopes/"
PAGE = 25
REPLY = re.compile(r'href="(https://www\.eevblog\.com/forum/[a-z0-9-]+/[a-z0-9-]+/msg(\d+)/#msg\2)"[^>]*>[^<]*</a>'
                   r'.{0,400}?Reply #(\d+) on:', re.S)
STATE = re.compile(r"<!-- scout: reply=(\d+) fork=(\S+) -->")


def gh(*args):
    return subprocess.run(("gh",) + args, check=True, capture_output=True, text=True).stdout


def last_state():
    """(last reply reported, time of the last report on the fork) from the newest scout issue."""
    found = json.loads(gh("issue", "list", "--repo", REPO, "--label", "scout", "--state", "all", "--limit", "1",
                          "--json", "body"))
    m = STATE.search(found[0]["body"]) if found else None
    return (int(m.group(1)), m.group(2)) if m else (None, None)


def forum(after):
    """([(reply, link)], error): the replies with a number above `after`, page by page."""
    out, start = [], (after + 1) // PAGE * PAGE
    for _ in range(8):
        request = urllib.request.Request(f"{THREAD}{start}/", headers={"User-Agent": "oos-scout"})
        try:
            with urllib.request.urlopen(request, timeout=30) as answer:
                page = answer.read().decode("utf-8", "replace")
        except OSError as e:
            return out, f"the forum could not be read from here ({e})"
        replies = sorted({(int(n), link) for link, _, n in REPLY.findall(page)})
        new = [r for r in replies if r[0] > after and r not in out]
        out += new
        if len(replies) < PAGE - 1 or not new:
            break
        start += PAGE
    return out, None


def clean(title):
    """A title as plain text: no mention, no link, no markup."""
    return re.sub(r"[`@#<>\[\]|]", " ", title).strip()


def fork(since):
    pulls = json.loads(gh("api", f"repos/{FORK}/pulls?state=closed&sort=updated&direction=desc&per_page=100"))
    issues = json.loads(gh("api", f"repos/{FORK}/issues?state=all&sort=created&direction=desc&per_page=100"))
    merged = [(p["number"], clean(p["title"])) for p in pulls if p.get("merged_at") and p["merged_at"] > since]
    opened = [(i["number"], clean(i["title"])) for i in issues
              if "pull_request" not in i and i["created_at"] > since]
    return sorted(merged), sorted(opened)


def main():
    now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    reply, since = last_state()
    first = reply is None
    lines = []
    if first:
        # nothing reported yet: start from here, report nothing old
        replies, error = forum(0)
        reply = max((n for n, _ in replies), default=0)
        lines.append(f"First run: the forum is at reply {reply}; from now on new replies are listed here.")
        since = now
    else:
        replies, error = forum(reply)
        if replies:
            lines.append(f"**Forum:** {len(replies)} new replies in [the thread]({THREAD}).\n")
            lines += [f"- [reply {n}]({link})" for n, link in replies]
            reply = replies[-1][0]
        merged, opened = fork(since)
        if merged or opened:
            lines.append(f"\n**Fork {FORK.split('/')[0]}:** since {since[:10]}.\n")
            lines += [f"- merged, pull request {n}: {t}" for n, t in merged]
            lines += [f"- new issue {n}: {t}" for n, t in opened]
            since = now
    if error:
        lines.append(f"\n{error}.")
    if not lines:
        print("nothing new")
        return 0
    body = ("What is new where OOS is talked about and worked on. Read it, turn what matters into issues, and close "
            "this one.\n\n" + "\n".join(lines) + f"\n\n<!-- scout: reply={reply} fork={since} -->\n")
    title = f"Scout: forum and fork, {now[:10]}"
    if "--print" in sys.argv[1:]:
        print(title + "\n\n" + body)
        return 0
    subprocess.run(("gh", "label", "create", "scout", "--repo", REPO, "--color", "ededed", "--force", "--description",
                    "What is new in the forum and in the fork, to be read"), check=True, capture_output=True)
    print(gh("issue", "create", "--repo", REPO, "--label", "scout", "--title", title, "--body", body).strip())
    return 0


if __name__ == "__main__":
    sys.exit(main())
