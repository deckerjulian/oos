#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Framing regression tests of the SCPI transport (tools/oosclient.py) and the report of scpi_validate.py; no network
or running application."""
import io
import json
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from oosclient import Link, LinkError, NoBlock  # noqa: E402
from scpi_validate import Results  # noqa: E402


class Socket:
    def sendall(self, data):
        pass

    def settimeout(self, t):
        pass


class Fragmented(io.BytesIO):
    def read(self, count=-1):
        return super().read(min(count, 2) if count >= 0 else count)


class FramingTests(unittest.TestCase):
    def client(self, data, stream=io.BytesIO):
        client = Link.__new__(Link)
        client.sock, client.stream, client.timeout = Socket(), stream(data), 5
        return client

    def test_empty_reply_does_not_consume_next_line(self):
        client = self.client(b"\n42\n")
        self.assertEqual(client.query(":MEAS:STRIP?"), "")
        self.assertEqual(client.query(":WAV:POINTS?"), "42")

    def test_binary_payload_newline_and_short_reads_preserve_next_reply(self):
        client = self.client(b"#14a\nb\x00\n0\n", Fragmented)
        self.assertEqual(client.query_bytes(":WAV:DATA?"), b"#14a\nb\x00")
        self.assertEqual(client.query(":COUN:ENAB?"), "0")

    def test_block_gives_the_payload_and_a_line_is_no_block(self):
        client = self.client(b"#212hello\nworld!\n-1\n", Fragmented)
        self.assertEqual(client.block(":DISP:DATA?"), b"hello\nworld!")
        with self.assertRaises(NoBlock):
            client.block(":DISP:DATA?")

    def test_a_line_that_starts_with_a_hash_is_a_line(self):
        client = self.client(b"#ffffff\n#\n1\n")
        self.assertEqual(client.query(":DISP:COL?"), "#ffffff")
        self.assertEqual(client.query(":X?"), "#")
        self.assertEqual(client.query(":Y?"), "1")

    def test_binary_assertions_can_be_written_to_the_json_report(self):
        result = Results()
        result.equal("PNG signature", b"\x89PNG", b"\x89PNG")
        self.assertEqual(json.loads(json.dumps(result.checks))[0]["actual"], {"hex": "89504e47"})

    def test_rejects_invalid_oversized_and_truncated_blocks(self):
        for payload in [b"#0", b"#1x", b"#14abc", b"#90067108865", b"#11x!", b""]:
            with self.subTest(payload=payload), self.assertRaises(LinkError):
                self.client(payload, Fragmented).query_bytes(":WAV:DATA?")


if __name__ == "__main__":
    unittest.main()
