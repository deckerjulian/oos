# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""oosclient.Link against a local stand-in server: replies of one line, of several lines, and blocks keep their order."""
import importlib.util
from pathlib import Path
import socket
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("oosclient", ROOT / "tools/oosclient.py")
oosclient = importlib.util.module_from_spec(spec)
spec.loader.exec_module(oosclient)


def serve(replies):
    """A server that answers the n-th received line with replies[n] (bytes, sent at once) and records the lines."""
    srv = socket.socket()
    srv.bind(("127.0.0.1", 0))
    srv.listen(1)
    got = []

    def run():
        c, _ = srv.accept()
        f = c.makefile("rb")
        for r in replies:
            got.append(f.readline())
            c.sendall(r)
        time.sleep(0.2)
        c.close()
        srv.close()

    threading.Thread(target=run, daemon=True).start()
    return srv.getsockname()[1], got


class TextReply(unittest.TestCase):
    def test_a_reply_of_several_lines_is_read_whole_and_the_next_reply_stays_in_place(self):
        port, _ = serve([b"1e-3,D1 UART CH2,4F,ok\n2e-3,D1 UART CH2,4F,ok\n3e-3,D1 UART CH2,4F,ok\n", b"0,\"No error\"\n"])
        link = oosclient.Link("127.0.0.1", port, timeout=5)
        link.send(":DEC:EVEN?")
        rows = link.text_reply().splitlines()
        self.assertEqual(len(rows), 3)
        self.assertTrue(all(r.endswith("4F,ok") for r in rows))
        link.send(":SYST:ERR?")
        self.assertEqual(link.text_reply(), '0,"No error"')
        link.close()

    def test_a_line_and_a_block_are_unchanged(self):
        port, _ = serve([b"RIGOL,OOS\n", b"#14abcd\n", b"\n"])
        link = oosclient.Link("127.0.0.1", port, timeout=5)
        link.send("*IDN?")
        self.assertEqual(link.text_reply(), "RIGOL,OOS")
        self.assertEqual(link.block(":WAV:DATA?"), b"abcd")
        link.send(":X?")
        self.assertEqual(link.text_reply(), "")
        link.close()


if __name__ == "__main__":
    unittest.main()
