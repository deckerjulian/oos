# OOS HTTP reference

The web remote of `oos-ui` answers on TCP port 8080 (`--web-port` on a computer; System > Network > Remote services
switches it, [network.md](network.md) 4). Every route is one arm of the `match` in `Http::answer`
(`ui/src/http.rs`); the MCP tools are in `mcp` of the same file. This page lists each of them with what it takes and
what it answers, read from the code. SCPI through MCP is the interpreter of [scpi.md](scpi.md).

Each entry carries one mark of [ADR 0055](adr/code/0055-the-remote-interface-is-stable-and-versioned.md) (the
marks below are its first ones, and the interface has no version query yet):

- **stable**: a client may build on it; a change that can break a client comes only after a deprecation;
- **experimental**: may change or go in any release;
- **internal**: listed so that this page and its check know every route; no part of the interface.

The text of an error message is not part of the interface; its status code is. `tools/http-coverage.py` (layer
`ratchet` of `tools/check.sh`) fails when a route of `Http::answer` or an MCP tool has no entry here, when an entry
has no mark, or when an entry names a route the code does not answer.

## Requests

- HTTP/1.1, one request per connection: every answer says `Connection: close`. A body is read by its
  `Content-Length` (none: no body); chunked bodies are not read.
- Only `POST` is told apart. Every other method (`GET`, `HEAD`, `PUT`, ...) is taken as `GET`, and a `HEAD` gets the
  body too. A route without a method of its own below answers every method alike, `POST` included.
- The route is the path before `?`, as sent (no percent-decoding). Of the query parameters, `name` of
  `POST /waveform` and `c` of `/cmd` are percent-decoded (`+` is a space); `slot` is read as sent, so `slot=%31` is
  refused.
- Answers carry `Content-Type` and `Content-Length` (not a 304) and `Cache-Control: no-store` (not a 413, nor a 503
  of `busy_answer`). An answer above 48 KiB is sent from a thread of its own on the slow cores, at most two at once.
  A smaller one, a 403 or 413 and the result of an upload go out as the client takes them, from an outbox that the
  frame loop writes from on every pass without waiting (ADR 0066); a client that takes nothing for 2 s, or a ninth
  such answer, closes the connection.

Limits (constants at the top of `ui/src/http.rs`):

| What | Limit | Beyond it |
|---|---|---|
| request head | 16 KiB | connection closed, no answer |
| body of `POST /update` | 70 MiB | 413 |
| body of `POST /waveform` | 128 MiB (`store::MAX_WAVEFORM_BYTES`) | 413 |
| any other body | 1 MiB | 413 |
| a body above 1 MiB | one at a time (a waveform being parsed counts) | 503 as soon as the head is in |
| open connections | 8 | the next ones are closed at accept |
| time | 10 s without data, 120 s for the whole request | connection closed, no answer |

## Common answers

Every route can answer these; the body is `text/plain`.

| Status | Body | When |
|---|---|---|
| 403 | `refused` | a request another site made through a browser (`foreign`, below), on every route |
| 403 | `refused` | `POST /update` and `/mcp` from a peer outside `net.allow`, when the head is in (body not read) |
| 404 | `not found` | a path that is no route, and a route asked with a method it does not take |
| 413 | `request too large` | a body above its limit (announced or sent) |
| 503 | `busy, try again` | a 2nd body above 1 MiB or waveform upload, a 3rd answer above 48 KiB, a 9th picture asked |

**Foreign** (`foreign` in `ui/src/http.rs`): a request is refused when its `Host` names the scope neither by an
address nor by `localhost`, a single label or a `.local` name (DNS rebinding); when it has an `Origin` whose host and
port differ from `Host`; or when `Sec-Fetch-Site` is `cross-site` or `same-site`, except for a `GET` of `/`,
`/style.css`, `/update` and `/lxi/identification`, which a link may open. A client that sends neither `Origin` nor
`Sec-Fetch-Site` (curl, a script) is never foreign.

**net.allow** ([network.md](network.md) 6, `ui/system/src/netacl.rs`): without a list everyone is allowed; loopback
always is. Each request is checked, so a change of Allowed counts from the next one. Only `POST /update` and `/mcp` look
at it; the firewall of the scope applies to every port.

## Routes

The status of an answer is chosen in `Http::answer`: 409 where the route says so, 404 and 403 for the bodies `not
found` and `refused`, 202 for an MCP notification, else 200.

### `GET /`

- Mark: **internal**
- Request: nothing.
- Answer: 200 `text/html; charset=utf-8`, the page of the web remote with its script (`PAGE`). Its script uses
  `/style.css`, the pictures, the downloads, `/stamp`, `/ui-state`, `/cmd` and `POST /waveform`.
- net.allow: no.

### `GET /style.css`

- Mark: **internal**
- Request: nothing.
- Answer: 200 `text/css`, the style sheet of the page and of the update page (`STYLE`).
- net.allow: no.

### `GET /screen.png`

- Mark: **stable**
- Request: optional header `If-None-Match` with the `ETag` of the picture the client has.
- Answer: 200 `image/png` with an `ETag`, the screen (60 to 175 KB). A client without the newest picture gets it at
  once when it is at most 300 ms old, else after a fresh copy. A client that has the newest one waits until the
  screen changes, at most 2 s (long poll), then gets 304 with no body. 503 while 8 requests wait for a picture or
  two large answers are being sent.
- Limits: pictures are made off the frame loop, only of a changed screen, at most 20 a second (`Shots`).
- net.allow: no.

### `GET /screen.jpg`

- Mark: **stable**
- Request: as `GET /screen.png`.
- Answer: as `GET /screen.png`, the picture as `image/jpeg` from the video encoder (`ui/src/vpu.rs`). Where there is
  none, or after it failed (tried again after 60 s), the answer is `image/png`: the `Content-Type` says which.
- net.allow: no.

### `GET /screen.bmp`

- Mark: **stable**
- Request: nothing.
- Answer: 200 `image/bmp`, the screen as a 24-bit BMP (1.8 MB), made at the request (`bmp` in
  `ui/src/scpi/display.rs`, the picture of `:DISPlay:DATA?`). No `ETag`. 503 while two large answers are being sent.
- net.allow: no.

### `GET /wave.csv`

- Mark: **stable**
- Request: nothing.
- Answer: 200 `text/csv; charset=utf-8`, the last record as Storage > CSV writes it (`store::wave_csv`): a header
  `Time (s),CH1 (V),...` with the channels that are on, then one row per sample, the time as `%.9e`. 409
  `text/plain`: `No waveform yet`, or a record above 2 000 000 points (`WEB_CSV_MAX`), which goes as `/wave.bin`.
- net.allow: no.

### `GET /wave.bin`

- Mark: **stable**
- Request: nothing.
- Answer: 200 `application/octet-stream`, the last record in the binary form of Storage (`store::wave_bin`, `OOSW`
  and its header as [First Steps](../wiki/First-Steps.md#saving) describes it). 409 `text/plain`: `No waveform yet`.
- net.allow: no.

### `GET /la.csv`

- Mark: **stable**
- Request: nothing.
- Answer: 200 `text/csv; charset=utf-8`, the logic lines of the last record (`store::la_csv`): a header `Time
  (s),D<n>,...` with the lines shown, a row at the start of the record and one for every change, `0` or `1` per line,
  and a last row at its end. 409 `text/plain`: `No logic analyser record` (none, or the logic analyser is off).
- net.allow: no.

### `POST /waveform`

- Mark: **stable**
- Request: query `slot`, 1 to 10, the reference R1 to R10; `name`, the file's name for the display (its last path
  part, without control characters, at most 128 characters; `waveform.csv` without one). Body: the file as it is, any
  `Content-Type`; the formats `store::parse_wave` recognises by their header (`ui/src/store/import.rs`).
- Answer: `text/plain; charset=utf-8`. 200 `Loaded <name> into R<n>. ...` with a hint for the next step. 409 without
  a slot from 1 to 10, for a file that is refused, or when the session changed during the parse; the slot chosen
  before stays. 503 while another upload is parsed. The file is parsed on a thread of its own and the answer comes when
  the frame loop has taken the traces (seconds for a large file). Nothing is written to storage.
- Limits: 128 MiB.
- net.allow: no.

### `GET /ui-state`

- Mark: **experimental**
- Request: nothing; a `POST` gets 404.
- Answer: 200 `application/json`, one object (`Ui::workflow_state`, `ui/src/ui/workflow.rs`): `mode` (`live`,
  `reference`, `sim`), `running`, `busy` (SIM generating), `measurementBusy` (booleans), `source` (`CH<n>` or
  `R<n>`), `captures` (a number), `status` (`TRIGGERED`, `WAIT`, `AUTO`, `STOP`), `gate` (`screen`, `full-record`).
- net.allow: no.

### `GET /stamp`

- Mark: **experimental**
- Request: nothing.
- Answer: 200 `text/plain`, the scope's local time as `YYYYMMDD-HHMMSS` for the names of downloads
  (`Ui::file_stamp`); an empty body while the clock was not set in this boot ([network.md](network.md) 3).
- net.allow: no.

### `GET /cmd`

- Mark: **experimental** (`tap` and `drag` name points of a screen whose layout changes)
- Request: query `c`, once or more, each a front-panel command of `ui/src/cmd.rs` that `cmd::remote_ok` lets
  through: `key <name|0xNN>`, `knob <name> <+n|-n|push>`, `tap <x> <y>`, `drag <x0> <y0> <x1> <y1> [hold]`,
  `workflow live|sim|load|measure|settings`.
- Answer: 200 `text/plain` `ok`; 403 `refused` when one `c` is another command. The commands that are let through
  run either way, in the frame loop after the answer; `ok` says they were taken, not what they did. Without `c`: `ok`.
- net.allow: no (keys and knobs, ADR 0025).

### `GET /lxi/identification`

- Mark: **stable**
- Request: nothing.
- Answer: 200 `text/xml`, the LXI identification (LXI Device Specification 2011, 10.2; `lxi_identification`):
  manufacturer `OOS`, model `DHO924S`, serial number `0`, firmware `oos-ui <version>`, and an instrument address per
  service offered: `TCPIP::INSTR` (VXI-11), `TCPIP::<port>::SOCKET` (the SCPI socket), `TCPIP::hislip0[,<port>]::INSTR`
  (HiSLIP). LXI version 1.5.
- net.allow: no.

### `POST /mcp`

- Mark: **stable**
- Request: one JSON-RPC 2.0 message (no batch), Model Context Protocol over streamable HTTP with plain JSON answers:
  no session, no event stream. Methods: `initialize` (answers the `protocolVersion` asked, `2025-06-18` without one,
  capability `tools`, `serverInfo` `oos-ui` and its version), `ping`, `tools/list`, `tools/call` with the tools below.
  Any method of HTTP is taken alike; a `GET` has no body and gets the parse error.
- Answer: 200 `application/json`, the JSON-RPC answer; errors as JSON-RPC errors with status 200: -32700 parse error,
  -32601 method not found, -32602 unknown tool. A message without `id` (a notification) gets 202 with no body
  and an empty `Content-Type`.
- Limits: 1 MiB.
- net.allow: yes, as soon as the head is in, for every method.

### MCP tool `scpi`

- Mark: **stable**
- Request: argument `command`, a string: one SCPI program message (commands separated by `;`) for the interpreter
  (`Scpi::exec_message_checked`, [scpi.md](scpi.md)); a missing one is an empty message.
- Answer: one `text` content item: the answer of the queries, `ok` for a message without a query, an empty text for a
  query that answers nothing, `(binary response)` for an answer that is not UTF-8. A message that waits on `*OPC?`
  answers `isError: true` with a text, queues error -221, and drops the commands after it (the ones before it ran).

### MCP tool `screenshot`

- Mark: **stable**
- Request: no arguments.
- Answer: one `image` content item, `mimeType` `image/png`, `data` the screen as PNG in base64 (`gfx::png::png_of`,
  made at the call).

### `GET /update`

- Mark: **internal**
- Request: nothing.
- Answer: 200 `text/html; charset=utf-8`, the update page: the state that `oos-update status --quick` prints, the
  result of the last job (`Ui::update_state`, `ui/src/ui/update.rs`), and the upload form (`update_page`,
  `ui/src/http.rs`).
- net.allow: no.

### `POST /update`

- Mark: **stable**
- Request: body: a signed package (`.oosupd`, [update.md](update.md)) as it is, for example `curl --data-binary
  @oos-dev.oosupd http://<scope>:8080/update`.
- Answer: 200 `text/plain` that says what happens (`signature ok, installing build ...`, or the start by kexec), or
  `REFUSED: <reason>`, also with status 200, when a job runs, the signature or the image hash does not hold, or the
  package cannot be written or `oos-update` started (`Ui::update_upload`). The signature is checked before anything
  is written.
- Limits: 70 MiB.
- net.allow: yes, as soon as the head is in.
