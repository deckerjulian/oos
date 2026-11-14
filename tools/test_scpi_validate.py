#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Local framing regression tests; no network or running application."""
import io
import json
import unittest
from scpi_validate import OOS, Results

class Socket:
    def sendall(self, data):
        pass

class Fragmented(io.BytesIO):
    def read(self, count=-1):
        return super().read(min(count, 2) if count >= 0 else count)

class FramingTests(unittest.TestCase):
    def client(self, data, stream=io.BytesIO):
        client = OOS.__new__(OOS)
        client.sock, client.stream = Socket(), stream(data)
        return client

    def test_empty_reply_does_not_consume_next_line(self):
        client = self.client(b"\n42\n")
        self.assertEqual(client.query(":MEAS:STRIP?"), "")
        self.assertEqual(client.query(":WAV:POINTS?"), "42")

    def test_binary_payload_newline_and_short_reads_preserve_next_reply(self):
        client = self.client(b"#14a\nb\x00\n0\n", Fragmented)
        self.assertEqual(client.query_bytes(":WAV:DATA?"), b"#14a\nb\x00")
        self.assertEqual(client.query(":COUN:ENAB?"), "0")

    def test_binary_assertions_can_be_written_to_the_json_report(self):
        result = Results()
        result.equal("PNG signature", b"\x89PNG", b"\x89PNG")
        self.assertEqual(json.loads(json.dumps(result.checks))[0]["actual"], {"hex": "89504e47"})

    def test_rejects_invalid_oversized_and_truncated_blocks(self):
        for payload in [b"#0", b"#1x", b"#14abc", b"#90067108865", b"#11x!"]:
            with self.subTest(payload=payload), self.assertRaises(RuntimeError):
                self.client(payload, Fragmented).query_bytes(":WAV:DATA?")

if __name__ == "__main__":
    unittest.main()
