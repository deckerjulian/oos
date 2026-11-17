#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""privacy-scan.py: what a range of commits would publish that belongs to a person or a unit (AGENTS.md, "Never").

    tools/privacy-scan.py [RANGE]      RANGE: commits as git names them, default origin/main..HEAD

Reads every commit of the range on its own: the lines it adds and its message. A line that a later commit of the
same range removes again still counts, because the commit stays readable in the pull request. Looks for

    mac      a MAC address
    serial   a serial number of a scope
    key      a private key: its head, followed by the key itself on the same or the next line (a head alone, as a
             test or a check for one uses it, is no key)
    token    an access token of GitHub and of a few other services
    home     a path inside somebody's home directory
    address  an IPv4 address of a host in a private network
    mail     an e-mail address (not in a Signed-off-by or Co-Authored-By line, not a no-reply address)

Third-party files that are in the tree as they come (kernel/, the licence texts) are not read. A value that is meant
to be in the tree, like the address in an example, goes into tools/privacy-allow.txt as `rule value`, in the same
pull request, where the reviewer sees it. Exit 1 when something was found. Python's standard library only; reads
the repository, touches no scope and no network.
"""
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SKIP = ("kernel/", "rootfs/initramfs/etc/licenses/", "LICENSE", "THIRD-PARTY-LICENSES.md", "ui/Cargo.lock",
        "hal/Cargo.lock", "dsp/Cargo.lock", "fpgad/Cargo.lock")
RULES = {
    "mac": re.compile(r"(?<![0-9A-Fa-f:])(?:[0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}(?![0-9A-Fa-f:])"),
    "serial": re.compile(r"\b[DM]HO[0-9][A-Z][A-Z0-9]{9,}\b"),
    "key": re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
    "token": re.compile(r"\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|AKIA[0-9A-Z]{16}"
                        r"|xox[abprs]-[A-Za-z0-9-]{10,}|sk-[A-Za-z0-9_-]{24,})"),
    "home": re.compile(r"(?:/Users/|/home/|[A-Za-z]:\\Users\\)([A-Za-z0-9._-]+)"),
    "address": re.compile(r"(?<![0-9.])(?:10\.\d{1,3}|192\.168|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}(?![0-9.])"),
    "mail": re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}\b"),
}
# values that are nobody's: examples, and the accounts of the tools
FINE = {
    "mac": re.compile(r"^(?:00:){5}00$|^(?:ff:){5}ff$|^[0-9a-f]{2}:11:22:33:44:[0-9a-f]{2}$|^11:22:33:44:55:66$"
                      r"|^aa:bb:cc:[0-9a-f]{2}:[0-9a-f]{2}:[0-9a-f]{2}$|^02:00:00:00:00:[0-9a-f]{2}$", re.I),
    # a made-up number: nothing but zeros after the model and its letter, and at most a short count
    "serial": re.compile(r"^[DM]HO[0-9][A-Z]0{7,}[0-9]{0,2}$"),
    "home": re.compile(r"^(?:runner|ubuntu|user|you|me|name|USER|\$USER|username)$"),
    # a network, not a host
    "address": re.compile(r"\.0$"),
    "mail": re.compile(r"@(?:users\.noreply\.github\.com|example\.(?:com|org|net|invalid)|noreply\.[a-z.]+)$|^noreply@"
                       r"|^git@|^sk-[a-z0-9-]+@openssh\.com$"),
}
# the head of a hunk: where its added lines start in the new file
HUNK = re.compile(r"^@@ -\S+ \+(\d+)")
# the key itself: a long run of base64, as the lines of a key in PEM or OpenSSH form are
KEY_BODY = re.compile(r"[A-Za-z0-9+/]{40,}")
TRAILER = re.compile(r"^(?:Signed-off-by|Co-authored-by|Reviewed-by|Tested-by|Reported-by):", re.I)


def git(*args):
    return subprocess.run(("git", "-C", ROOT) + args, check=True, capture_output=True).stdout.decode("utf-8", "replace")


def allowed():
    out = set()
    try:
        with open(os.path.join(ROOT, "tools", "privacy-allow.txt"), encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if line and not line.startswith("#"):
                    rule, _, value = line.partition(" ")
                    out.add((rule, value.strip()))
    except FileNotFoundError:
        pass
    return out


def find(text, allow, message=False, following=""):
    """(rule, value) for everything in one line of text that a rule names and nothing excuses; `following` is the
    next added line of the same file, which a key's head needs to be a key."""
    if message and TRAILER.match(text):
        return []
    hits = []
    for rule, pattern in RULES.items():
        for m in pattern.finditer(text):
            value = m.group(1) if rule == "home" else m.group(0)
            if rule == "key" and not (KEY_BODY.search(text[m.end():]) or KEY_BODY.search(following)):
                continue
            if rule in FINE and FINE[rule].search(value):
                continue
            if (rule, value) in allow:
                continue
            hits.append((rule, value))
    return hits


def scan(rng):
    allow = allowed()
    hits = []
    for commit in git("rev-list", "--no-merges", rng).split():
        short = commit[:9]
        message = git("log", "-1", "--format=%B", commit).splitlines()
        for i, line in enumerate(message):
            after = message[i + 1] if i + 1 < len(message) else ""
            hits += [(short, "message", rule, value) for rule, value in find(line, allow, True, after)]
        hits += [(short, path, rule, value) for path, rule, value in scan_diff(commit, allow)]
    return hits


def scan_diff(commit, allow):
    """(path, rule, value) for the lines a commit adds. A key's head and the key itself are read together even when
    only one of them is new: a head added above a key already in the file, or a key added below a head."""
    hits = []
    path, number, files = None, 0, {}

    def line_of(n):  # line n (from 1) of the file at this commit
        if path not in files:
            try:
                files[path] = git("show", f"{commit}:{path}").splitlines()
            except subprocess.CalledProcessError:
                files[path] = []
        return files[path][n - 1] if 0 < n <= len(files[path]) else ""

    diff = git("show", "--format=", "--unified=0", "--no-color", commit).splitlines()
    added = [line.startswith("+") and not line.startswith("+++ ") for line in diff]
    for i, line in enumerate(diff):
        if line.startswith("+++ "):
            path = line[6:] if line.startswith("+++ b/") else None
        elif line.startswith("@@") and HUNK.match(line):
            number = int(HUNK.match(line).group(1))
        elif added[i] and path:
            text = line[1:]
            if not path.startswith(SKIP):
                next_added = i + 1 < len(diff) and added[i + 1]
                after = diff[i + 1][1:] if next_added else ""
                if not next_added and RULES["key"].search(text):
                    after = line_of(number + 1)
                hits += [(path, rule, value) for rule, value in find(text, allow, following=after)]
                if not (i > 0 and added[i - 1]) and KEY_BODY.search(text):
                    head = RULES["key"].search(line_of(number - 1))
                    if head and ("key", head.group(0)) not in allow:
                        hits.append((path, "key", head.group(0)))
            number += 1
    return hits


def main(argv):
    rng = argv[1] if len(argv) > 1 else "origin/main..HEAD"
    hits = scan(rng)
    for commit, where, rule, value in hits:
        print(f"{commit} {where}: {rule}: {value}")
    if hits:
        print(f"{len(hits)} found in {rng}: take them out, in a commit range without them, or, for a value that is "
              "meant to be public, add `rule value` to tools/privacy-allow.txt")
        return 1
    print(f"nothing found in {rng}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
