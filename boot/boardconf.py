# Copyright (C) 2026 Julian Decker
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
