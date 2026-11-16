# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""The values of a board for the scripts in boot/: rootfs/board/<OOS_BOARD>/etc/oos-board, plain KEY=value lines."""
import os


def load(board=None):
    here = os.path.dirname(os.path.abspath(__file__))
    name = board or os.environ.get("OOS_BOARD", "dho")
    values = {}
    with open(os.path.join(here, "..", "rootfs", "board", name, "etc", "oos-board")) as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                key, value = line.split("=", 1)
                values[key] = value.strip('"')
    return values


def check_parm(raw, values=None):
    """What the PARM block `raw` (read from OOS_PARM_LBA, at least its first 1 KiB) lacks of the stock layout of the
    board: the `PARM` magic and every mark of OOS_PARM_MARKS, as the scope's own check (oos-oneshot, oos-hal) sees it:
    NUL bytes left out, case ignored. An empty list: the stock layout, the card may be written."""
    values = values or load()
    text = raw.replace(b"\0", b"").decode("latin1").lower()
    missing = [] if text.startswith("parm") else ["PARM magic"]
    return missing + [m for m in values["OOS_PARM_MARKS"].split() if m.lower() not in text]
