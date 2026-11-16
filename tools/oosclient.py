# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""oosclient.py: the SCPI transport of the host tools (device-test.py, scpi_validate.py, sim_validate.py).

One raw TCP SCPI connection (port 5555): a command is a line, a reply is a line or an IEEE 488.2 definite block
(`#<one digit 1-9><length><payload>` and its line end), read exactly, so binary payloads with line ends inside and
short reads leave the next reply where it is. Python's standard library only; nothing here knows a command.
"""
import socket
import time

# no reply of OOS is longer (the largest waveform and screenshot blocks)
BLOCK_LIMIT = 64 << 20
LINE_LIMIT = 1 << 20


class LinkError(RuntimeError):
    """The connection failed: closed, no reply in time, or a reply that is not SCPI framing."""


class NoBlock(Exception):
    """A block was asked for and a line came back; the line is the exception's text."""


class Link:
    def __init__(self, host, port, timeout=10.0, tries=1, pause=2.0, nodelay=False):
        """Connect to host:port, `tries` times `pause` seconds apart; `timeout` is the default for every reply."""
        self.host, self.port, self.timeout = host, port, timeout
        error = None
        for attempt in range(tries):
            try:
                self.sock = socket.create_connection((host, port), timeout=timeout)
                break
            except OSError as e:
                error = e
                if attempt + 1 < tries:
                    time.sleep(pause)
        else:
            raise LinkError(f"no SCPI on {host}:{port}: {error}")
        if nodelay:
            self.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self.stream = self.sock.makefile("rb")

    def close(self):
        self.stream.close()
        self.sock.close()

    def send(self, command):
        try:
            self.sock.sendall(command.encode() + b"\n")
        except OSError as e:
            raise LinkError(f"SCPI write: {e}")

    def reply(self, timeout=None, what="a command"):
        """The next reply as bytes: a line without its line end (empty for an empty reply), or a whole definite block
        with its header and without the line end after it."""
        t = self.timeout if timeout is None else timeout
        self.sock.settimeout(t)
        try:
            first = self._exact(1, what)
            if first == b"\n":
                return b""
            width = self._exact(1, what) if first == b"#" else b""
            if width not in b"123456789" or not width:
                # a line, also one that starts with `#` but not with a block header (`#ffffff`)
                line = first + width + (b"" if width == b"\n" else self.stream.readline(LINE_LIMIT))
                if not line.endswith(b"\n"):
                    raise LinkError(f"incomplete reply to {what}")
                return line[:-1].rstrip(b"\r")
            digits = self._exact(int(width), what)
            if not digits.isdigit():
                raise LinkError(f"bad block length from {what}: {digits!r}")
            length = int(digits)
            if length > BLOCK_LIMIT:
                raise LinkError(f"block from {what} exceeds the OOS response limit")
            payload = self._exact(length, what)
            if self._exact(1, what) != b"\n":
                raise LinkError(f"block from {what} was not terminated")
            return first + width + digits + payload
        except socket.timeout:
            raise LinkError(f"no reply to {what} in {t} s")
        except OSError as e:
            raise LinkError(f"SCPI read: {e}")

    def _exact(self, count, what):
        data = bytearray()
        while len(data) < count:
            chunk = self.stream.read(count - len(data))
            if not chunk:
                raise LinkError(f"connection closed during the reply to {what}")
            data.extend(chunk)
        return bytes(data)

    def query_bytes(self, command, timeout=None):
        self.send(command)
        return self.reply(timeout, repr(command))

    def query(self, command, timeout=None):
        return self.query_bytes(command, timeout).decode(errors="replace").strip()

    def block(self, command, timeout=None):
        """The payload of the definite block `command` answers; NoBlock when it answers a line."""
        r = self.query_bytes(command, timeout)
        if r[:1] != b"#":
            raise NoBlock(r.decode(errors="replace"))
        return r[2 + r[1] - 48:]
