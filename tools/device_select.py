# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""What tools/device-test.py runs for a change, and how it judges a run. No scope, no network: pure functions, tested by
tools/tests/test_device_select.py.

- Which tests: the files a change touches (`--changed BASE`) name areas, an area names tests. A file in no area
  (documents, CI, host tools) runs the smoke tests only; a file whose effect reaches everything (the image, the update
  path, the build) runs all of them.
- Noise: a test that fails runs again (`--retries`). One that passes then is FLAKY: shown, not a failure.
- What was broken before: a failure that the known run (`--known`, the results.json of a run of main on the same unit)
  has too is KNOWN: shown, not a failure of this change.
- The register sequences: the unit's `plan-matrix.sh` against this computer's, section by section, with the
  differences of the setup taken out (where the calibration files lie, a logic probe plugged in, a calibration file
  that only one side has).
"""

import json
import os
import re
import subprocess

# Always: is it the unit meant, is it safe, is it alive, and where does the generator reach.
SMOKE = ("identity", "protected_areas_before", "protected_areas_after", "build_on_the_unit", "log_has_no_panic",
         "generator_reaches")

# area -> the tests (regular expressions on the name)
AREAS = {
    "registers": (r"^plan_matches_this_build$", r"^depth_and_rate_matrix$", r"^every_time_base_runs$"),
    "acq": (r"^depth_and_rate_matrix$", r"^every_time_base_runs$", r"^gnd_is_flat$", r"^time_axis_at_every",
            r"^fine_time_base", r"^delay_moves_the_record$", r"^single_takes_one_new_record$", r"^deep_memory",
            r"^peak_detect", r"^high_res", r"^rates_for_the_record$", r"^two_channels_agree$", r"^deskew"),
    "frontend": (r"^zero_of_every_scale$", r"^gnd_is_flat$", r"^every_scale_reads_true$", r"^fine_scale",
                 r"^offset_", r"^probe_ratio", r"^ac_coupling", r"^invert_", r"^bandwidth_limit", r"^response_over",
                 r"^two_channels_agree$", r"^auto_"),
    "trigger": (r"^normal_and_single", r"^edge_trigger", r"^trigger_types", r"^runt_trigger$", r"^holdoff",
                r"^either_slope", r"^invert_and_the_trigger$", r"^protocol_trigger", r"^hidden_trigger",
                r"^delay_trigger", r"^single_takes", r"^la_trigger", r"^la_pattern", r"^rf_trigger"),
    "roll": (r"^roll_", r"^time_axis_at_every", r"^la_has_no_gaps", r"^la_in_scan_mode$", r"^la_time_axis"),
    "generator": (r"^generator_", r"^bode_of_the_cable$", r"^uart_from_the_generator"),
    "la": (r"^la_",),
    "rf": (r"^rf_",),
    "screen": (r"^menus_open_and_close$", r"^frame_rate_by", r"^zoom_and_views$", r"^analysers_",
               r"^settings_survive", r"^random_settings"),
    "remote": (r"^web_", r"^no_telnet_shell$", r"^mcp_tools$", r"^vxi11_answers$", r"^limits_hold$",
               r"^settings_survive", r"^random_settings", r"^exports_match"),
    "measure": (r"^every_scale_reads_true$", r"^formula_runs$", r"^analysers_", r"^logger_writes$",
                r"^exports_match", r"^uart_from_the_generator", r"^rf_frequency"),
    "decode": (r"^uart_from_the_generator", r"^protocol_trigger", r"^la_decoders$", r"^la_protocol"),
    "store": (r"^exports_match", r"^logger_writes$", r"^settings_survive", r"^web_pages_and_downloads$",
              r"^la_csv"),
}

# changed path -> areas; the first rule that matches decides. ALL: every test.
ALL = ("ALL",)
RULES = (
    # what the unit does not run, and the tests' own helpers that only a run of everything proves
    (r"^tools/(device-test\.py|device_select\.py|oosclient\.py|plan-matrix\.sh|device-deploy\.sh)$", ALL),
    (r"\.md$|^docs/|^wiki/|^\.github/|^tools/|^ui/tests/|^dsp/tests/|^hal/tests/|^LICENSE|^\.[a-z]", ()),
    # the image, the update path and the build: anything can follow
    (r"^(boot|kernel|rootfs|hal/src/update)/|Cargo\.(toml|lock)$|^rustfmt\.toml$", ALL),
    (r"^hal/src/dho/roll", ("roll", "acq", "registers")),
    (r"^hal/src/dho/la", ("la", "registers")),
    (r"^hal/src/dho/(tpu|trig|mask|zone|auxout)", ("trigger", "registers")),
    (r"^hal/src/dho/(afe|adc|cal|levels|lmx|pll|relay|vga|offset)", ("frontend", "acq", "registers")),
    (r"^hal/src/dho/(afg|gen)", ("generator", "registers")),
    (r"^hal/src/dho/boot/", ALL),
    (r"^hal/src/dho/", ("acq", "registers")),
    (r"^hal/", ("acq", "registers")),
    (r"^fpgad/", ("registers",)),
    # the board runs every acquisition: what it sets up and what it reads back
    (r"^ui/src/board/", ("acq", "roll", "trigger", "frontend", "registers")),
    # the measurement code: nearly every test of a1 reads its answer through a measurement (#118 was found by the
    # time axis test, through FREQ)
    (r"^dsp/src/measure\.rs$|^ui/src/ui/analysis/\w+\.rs$"
     r"|^ui/src/(framestats|ui/(analysis|measure_own|measure_pairs|records|acquire|tools|facts))\.rs$",
     ("measure", "acq", "roll", "frontend", "generator", "trigger")),
    (r"^ui/src/(scpi|vxi|hislip|http|mcp|net|mdns|announce|ssh|web)", ("remote",)),
    (r"^ui/src/(store|compress|logger)", ("store",)),
    (r"^ui/src/(measures|meas)", ("measure",)),
    (r"^ui/src/(protocols|decode)", ("decode",)),
    (r"^ui/src/(gen|afg|bode)", ("generator",)),
    (r"^ui/src/", ("screen", "remote")),
    (r"^gpu/", ("screen",)),
    (r"^dsp/src/(decode|uart|spi|i2c|can|lin|proto)", ("decode", "measure")),
    (r"^dsp/", ("measure", "decode")),
)


def changed_files(base, cwd=None):
    """The files this tree changes against `base`: commits since the common ancestor, what is not yet committed, and
    new files git does not know yet."""
    def git(*a):
        return subprocess.run(("git",) + a, cwd=cwd, capture_output=True, text=True, check=True).stdout

    anc = git("merge-base", base, "HEAD").strip()
    names = set(git("diff", "--name-only", anc).split())
    names |= set(git("ls-files", "--others", "--exclude-standard").split())
    return sorted(names)


def areas_for(paths):
    """(areas, why): the areas the paths reach, and for each path the rule's areas. "ALL" among the areas: run all."""
    areas, why = set(), {}
    for p in paths:
        for rx, a in RULES:
            if re.search(rx, p):
                areas |= set(a)
                why[p] = a
                break
        else:
            # a file no rule knows: safe side
            areas |= set(ALL)
            why[p] = ALL
    return areas, why


def select(names, areas):
    """The tests of `names` that the areas ask for, the smoke tests, and every test for "ALL"."""
    if "ALL" in areas:
        return list(names)
    rx = [r for a in areas for r in AREAS.get(a, ())]
    return [n for n in names if n in SMOKE or any(re.search(r, n) for r in rx)]


BAD = ("FAIL", "ERROR")
# a failure of these is never "known": a register sequence or the safety of the unit is not noise of a measurement
NEVER_KNOWN = SMOKE + ("plan_matches_this_build",)


def judge(name, attempts, known):
    """The result of a test from its attempts [(result, detail)], with the known run {name: result}."""
    res, detail = attempts[-1]
    if res == "PASS" and len(attempts) > 1:
        return "FLAKY", f"passed on attempt {len(attempts)}; before: {attempts[0][1]}"
    if res in BAD and name not in NEVER_KNOWN and known.get(name) in BAD + ("KNOWN",):
        return "KNOWN", f"fails in the known run too: {detail}"
    if res in BAD and len(attempts) > 1:
        return res, f"{len(attempts)} attempts: " + " / ".join(d[:120] for _, d in attempts)
    return res, detail


def load_known(path):
    """{name: result} of a results.json, {} when there is none."""
    if not path or not os.path.exists(path):
        return {}
    return {r["test"]: r["result"] for r in json.load(open(path))}


def sections(text):
    """{header: [lines]} of a plan-matrix output; the lines before the first header under ""."""
    out, cur = {}, ""
    out[cur] = []
    for line in text.replace("\r", "").split("\n"):
        if line.startswith("### "):
            cur = line
            out.setdefault(cur, [])
            continue
        # where the calibration files lie is the setup, not the build
        line = re.sub(r"# \S*/(cal_\w+\.hex)", r"# \1", line)
        # a logic probe plugged in or not is the setup too
        if re.match(r"probe (present|absent)\b", line):
            continue
        out[cur].append(line.rstrip())
    # the shell on the unit drops the last empty line of its output
    for lines in out.values():
        while lines and not lines[-1]:
            lines.pop()
    return out


def plan_compare(host, unit):
    """(differences, not comparable): the sections whose lines differ, with the first differing line; and the sections
    left out because a calibration file is on one side only (the host has no cal_afg.hex, for example)."""
    a, b = sections(host), sections(unit)
    diff, skipped = [], []
    for h in sorted(set(a) | set(b)):
        x, y = a.get(h), b.get(h)
        if x is None or y is None:
            diff.append(f"{h or '(start)'}: only on the {'unit' if x is None else 'host'}")
            continue
        if x == y:
            continue
        if any("(no cal file)" in l for l in x) != any("(no cal file)" in l for l in y):
            skipped.append(h)
            continue
        i = next((i for i, (p, q) in enumerate(zip(x, y)) if p != q), min(len(x), len(y)))
        show = lambda l: l[i][:100] if i < len(l) else "(end)"
        diff.append(f"{h or '(start)'} line {i + 1}: host {show(x)!r}, unit {show(y)!r}")
    return diff, skipped
