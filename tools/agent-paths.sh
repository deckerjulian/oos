#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# agent-paths.sh - the files a standing agent must not change (docs/adr/process/0034): the rules, the checks, what
# decides how the scope starts, and the register sequences. Reads file names on standard input, one a line, prints
# those an agent may not change, and exits 1 when there is one.
#
#   git diff --name-only origin/main...HEAD | tools/agent-paths.sh
bad=0
while IFS= read -r f; do
	case $f in
	docs/adr/README.md) ;;
	.github/* | AGENTS.md | CLAUDE.md | docs/adr/* | LICENSE | THIRD-PARTY-LICENSES.md | \
		tools/check.sh | tools/agent-paths.sh | tools/agent-target.py | tools/agent-start.sh | tools/scout.py | \
		tools/pr-gate.sh | tools/pr-labels.sh | tools/ratchet.sh | tools/privacy-allow.txt | \
		tools/compare-base.sh | tools/plan-matrix.sh | \
		ui/tools/layout-check.sh | boot/* | kernel/* | rootfs/* | hal/src/dho/*)
		echo "$f"
		bad=1
		;;
	esac
done
exit $bad
