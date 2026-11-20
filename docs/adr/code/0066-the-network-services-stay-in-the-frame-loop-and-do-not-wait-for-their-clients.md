# ADR 0066: The network services stay in the frame loop, and they do not wait for their clients

- Status: proposed
- Date: 2026-11-19
- Category: code

## Context

The servers of the remote control (SCPI on TCP, VXI-11, HiSLIP, the web server) run in the frame loop of `oos-ui`:
every pass reads what the clients sent, runs complete commands on the screen's state and writes the answers. Issue
#192 showed what that costs when a server waits for a client: a reply was written at once, with a blocking write, and
a client that took a little of it every 2 s held the loop for up to 30 s per reply; the screen dropped below one frame
a second. Before that, the servers read with a timeout of 1 ms, which the kernel rounds up to whole timer ticks (4 to
8 ms), paid per connection and pass (`ui/system/src/net.rs`).

ADR 0064 left open how the services run ("a thread of their own, non-blocking I/O, an async runtime"), until there
were measurements. A contributor offered to compare an async runtime with threads; those measurements have not come.
#269 fixed #192 in the frame loop instead, and that is the decision this record writes down.

## Decision

**The network services stay in the frame loop, and their reads and writes there do not wait for a client. What a
client has not taken yet waits in its connection, a client that takes nothing is given up, and the work that takes
long runs on a thread of its own. Where a server still waits (the web server's short answers, below), that is a gap
to close the same way, not a second rule.**

- Reads take what has arrived (`net::read_now`), writes what the socket takes now (`net::write_now`). Neither waits.
- Each connection of SCPI, VXI-11 and HiSLIP keeps an `Outbox` of what is still to be written. One pass writes at
  most `FLUSH_MAX` (1 MiB) per connection. A client that took no byte for `SEND_STALL` (2 s), or whose reply is not
  out after `SEND_LIMIT` (30 s), is closed. A client's next command runs only after its replies are out, so replies
  keep their order and do not pile up.
- A connection whose client closed its side is still answered for what it sent before.
- The web server sends a body over 48 KiB on one of at most two sender threads, parses an uploaded waveform on a
  thread, and encodes screenshots on a thread of its own (`ui/src/http.rs`). The commands themselves run in the frame
  loop, on the state of the screen, as before.
- Every server has a limit of connections (8 each; VXI-11 also 16 links).
- **Not yet:** the web server writes its short answers on the frame loop at once, with a write timeout of 1 or 2 s:
  pages and data up to 48 KiB, the refusals 403 and 413, "busy", and the result of a waveform upload. A client that
  takes none of it holds the loop for up to that time per answer. These go to an `Outbox` per connection as well.

## Alternatives

- **A thread per service, the screen's state behind a lock or a channel.** Every remote command reads or changes the
  screen's state, so each one would still wait for the frame loop; the waiting moves into the lock, and the state of
  the screen (`Ui`) would have to be shared between threads, which it is not built for.
- **An async runtime (Tokio).** A dependency beyond `libc` (ADR 0013). It gives non-blocking sockets, which the
  loop now has without it, and the commands would still run on the frame loop's state.
- **One `poll` over all sockets** instead of one read per connection and pass. With at most 8 connections per server
  the reads cost little; worth doing when that number grows.

## Consequences

- A slow or stuck client costs a pass a few system calls, not a frame. A client that reads a large reply slowly gets
  it at the speed it reads, as long as it takes a byte every 2 s.
- SCPI, VXI-11 and HiSLIP never block the loop by a write; a new server follows the same pattern (`Outbox`,
  `read_now`). The web server reads that way and still writes its short answers with a timeout (above).
- A client that reads nothing for 2 s while a reply waits loses its connection. The old blocking write gave it up to
  30 s per write.
- Not yet measured on a unit: the frame rate with clients that read slowly. The tests check states, not times (ADR
  0020). Worth a second look when such a measurement shows that the loop still pays for its clients, or when the
  web server's answers move off the frame loop's state (ADR 0064, `http`).

## References

ADR 0013, ADR 0020, ADR 0022, ADR 0064; issue #192; pull request #269; `ui/system/src/net.rs` (`read_now`,
`write_now`, `Outbox`), `ui/src/scpi.rs`, `ui/remote/src/vxi11.rs`, `ui/remote/src/hislip.rs`, `ui/src/http.rs`.
