# ADR 0055: The remote interface (SCPI and HTTP) is stable and versioned

- Status: accepted (2026-11-18, by the maintainer)
- Date: 2026-11-18
- Category: code

Touches the system (the network services, ADR 0024 and ADR 0025) and the process (the checks, ADR 0020 and ADR 0032).

## Context

The maintainer's goal for milestone M5 (issue #205): OOS offers a stable, documented interface, so anyone can build a
GUI or an integration without touching the core. SCPI and the HTTP API are where it starts.

What there is, read on 2026-11-18 (main at dd2eadc6):

- **SCPI.** One interpreter (`ui/src/scpi.rs`, `ui/src/scpi/`) behind every transport: the raw socket (TCP 5555),
  VXI-11, HiSLIP and the `scpi` tool of MCP (ADR 0022). [scpi.md](../../scpi.md) is the reference; a command that is
  not in it is not promised, and a released command name or wire format does not change (ADR 0023).
  `tools/scpi-coverage.py` fails when a command word is not in the reference or a documented query is not asked in
  `ui/tests/scpi.txt`, and `tools/compare-base.sh` compares the answers of that script with the base, so a changed
  answer shows in a pull request (ADR 0032). What the reference does not say is which commands a client may build on:
  it calls the answers of the development commands (`:DEBug:SLOTs?`, `:SYSTem:DEBug:*`) "no stable interface", and
  the keys of `:SYSTem:GET?` and `:SYSTem:SET` "implementation-specific", and says nothing of the rest.
- **HTTP.** The web remote (`ui/src/http.rs`, port 8080) answers these routes, all in one `match` in `Http::answer`.
  Only `/ui-state`, `/waveform` and `/update` look at the method; the others answer whatever it is. Any other path
  answers 404 `not found`.

  - `GET /`: the page of the web remote (HTML, with its script); `GET /style.css`: its style sheet.
  - `GET /screen.png`: the screen as PNG, with an ETag (`If-None-Match` gets 304); `GET /screen.jpg`: as JPEG from
    the video encoder, PNG where there is none; `GET /screen.bmp`: as 24-bit BMP (1.8 MB).
  - `GET /wave.csv`: the last record as Storage > CSV writes it; `GET /wave.bin`: in the binary form of Storage;
    `GET /la.csv`: its logic lines, one row per change. Each answers 409 with a text when there is no such record
    (the CSV also when the record is too long for it).
  - `POST /waveform?name=...&slot=1..10`: loads a CSV or binary waveform into reference R1..R10; 200 or 409 with a
    text, 503 while another one is parsed.
  - `GET /ui-state`: JSON for the page's controls (`mode`, `running`, `busy`, `measurementBusy`, `source`,
    `captures`, `status`, `gate`).
  - `GET /stamp`: the scope's local time as `YYYYMMDD-HHMMSS` for the names of downloads; empty while the clock is
    not set.
  - `GET /cmd?c=...`: front-panel commands of `ui/src/cmd.rs` (`key`, `knob`, `tap`, `drag`, and `workflow` with
    `live`, `sim`, `load`, `measure` or `settings`); `ok`, or 403 `refused` for any other command.
  - `GET /lxi/identification`: the LXI identification XML.
  - `POST /mcp`: MCP over JSON-RPC 2.0 with `initialize`, `ping`, `tools/list` and `tools/call`; the tools `scpi`
    (argument `command`, a program message; answers its text) and `screenshot` (a PNG in base64). A notification
    gets 202.
  - `GET /update`: the update page (HTML); `POST /update`: installs a signed package (`.oosupd`) and answers a text
    that says what happens, or `REFUSED: ...`.

  Every route answers 403 to a request another site made through a browser (`foreign`), 413 to a body above its
  limit, 503 when too many large answers or bodies are under way. `POST /update` and `/mcp` are refused unless the
  peer is in `net.allow` (ADR 0025). These routes have no reference page of their own (the list is in the comment at
  the top of `ui/src/http.rs`, the ports in [network.md](../../network.md) 4 and in
  [Remote-Control.md](../../../wiki/Remote-Control.md)), no check that a route is documented, and no comparison of
  their answers. They are a mix: some serve the page's own work, some are what a script downloads.
- **No version.** `*IDN?` answers `OOS,<model>,<serial>,oos-ui <version>` (ADR 0023), the MCP `serverInfo` and the
  LXI identification the same program version. A release number changes with every fix and says nothing about
  whether a client's commands still mean the same.

A person who writes a GUI today cannot tell which commands and routes will still be there, and in which form, in the
next release; the tree cannot tell whether a pull request breaks such a client.

## Decision

- **Every entry gets one of three marks** in its reference: every SCPI command in [scpi.md](../../scpi.md), every
  HTTP route of `Http::answer` in a reference page of its own (`docs/http.md`, to be written with the
  implementation), every MCP tool.
  - *stable*: what a client may build on, with the promises below;
  - *experimental*: may change or go in any release; the change is still listed (below);
  - *internal*: listed so that the reference and its check know every entry, and no part of the interface.
- **The public interface** of OOS is the entries marked stable or experimental: the SCPI commands with their
  parameters, answers and error numbers on every transport (raw socket, VXI-11, HiSLIP, the `scpi` tool of MCP) and
  the framing of those transports; the HTTP routes with their method, request, status codes and answer; the MCP tools
  with their names and arguments; and the formats these hand out or take (the CSV and binary forms of a record, the
  logic CSV, the waveform upload, the pictures, the SCPI wire formats of ADR 0023, the LXI identification, the
  DNS-SD service types). An entry becomes stable by a minor version of the interface; new entries come as
  experimental unless the pull request says why not.
- **Internal**, and so not part of the interface: the development commands (`:DEBug:*`, `:SYSTem:DEBug:*`),
  `:SYSTem:STATe?`, `:SYSTem:GET?` and `:SYSTem:SET` (their keys follow the setup file; the reference may mark a key
  stable later), and the routes of the web page itself: `/`, `/style.css` and `GET /update`, whose HTML, CSS and
  script change with the page. Also not part of it, and not entries of a reference: the text of error messages
  (their numbers and status codes are part), timing, the control FIFO and `--script` commands of `oos-ui`, the
  protocol between `oos-ui` and `oos-gpu` (ADR 0003), the files on the card. As a first proposal, to be settled in
  the reference: `/ui-state`, `/stamp` and `/cmd` start as experimental (`tap` and `drag` name points of a screen
  whose layout changes); the pictures, the downloads, `POST /waveform`, `/lxi/identification`, `POST /mcp` and
  `POST /update` as stable.
- **What stable promises.** The interface has a version `MAJOR.MINOR`, after semantic versioning, independent of the
  release number of OOS. A new command, route, parameter, answer field or tool raises MINOR, and so does any change
  of an experimental entry, so a client that uses one can name the exact version it was written against (ADR 0056).
  A change that can break a client of a stable entry (a removed or renamed command or route, a changed answer format,
  unit, default or meaning, a parameter that is no longer taken) raises MAJOR, and is made only after the entry was
  *deprecated* for at least one release: it still works, the reference says since when, what replaces it and in
  which release it goes, and `CHANGELOG.md` says so. An HTTP route that is deprecated also answers with a
  `Deprecation` header (RFC 9745); SCPI has no place for a warning, so a deprecated command is not made to queue an
  error. A fix that makes an entry do what its reference says is no change of the interface. The stock command names
  stay as ADR 0023 has them: a released stock name is never deprecated.
- **A client reads the version** with an SCPI query and an HTTP route, which come with the implementation, not with
  this record. Proposed: `:SYSTem:INTerface:VERSion?` answers `<major>.<minor>` (SCPI's `:SYSTem:VERSion?` names the
  version of the SCPI standard and is left to that; the name is checked against the Programming Guide for a
  collision, ADR 0023), and `GET /interface` answers JSON with the interface version and the program version.
  `*IDN?` keeps its form. The version is one constant in `oos-ui`; a test holds the references to it. The first
  version that carries the marks is 1.0.
- **How it is guarded.**
  - The answers of the stable SCPI queries are in `ui/tests/scpi.txt` and the comparison with the base, as now.
  - The HTTP routes get the same: a check in the layer `ratchet`, as `tools/scpi-coverage.py` does for SCPI (or that
    tool grown by a part), fails when a route of `Http::answer` is not in the reference, or an entry of either
    reference has none of the three marks; and a script of requests against `oos-ui --serve` writes status, type
    and body of each stable route (a hash for a picture) to a text file that `tools/compare-base.sh` compares, so a
    change of an answer shows in a pull request.
  - `CHANGELOG.md` gets a section `Interface:` in each release, next to `Added:`, `Changed:` and `Removed:`, with
    every change of the public interface, its mark, and the new version. A pull request whose comparison shows a
    changed answer of a stable entry has its line there, and a breaking one has a major version and its deprecation
    in an earlier release; review asks for both until a check can.

## Alternatives

- **A new REST API next to SCPI** (`/api/v2/...` with JSON for every setting). A second command set that drifts from
  the interpreter, against ADR 0022; every function would be written twice. HTTP stays a transport and a place for
  downloads; a JSON framing of SCPI programs, as MCP already is, can be added as a transport.
- **gRPC or another IDL with generated clients.** It needs HTTP/2, protobuf and their crates in `oos-ui`, against
  ADR 0013, and VISA programs and the stock scripts do not speak it.
- **An own command tree instead of the stock one.** ADR 0023 rejected it: every existing script would break. SCPI with
  the stock names stays the base, and what OOS adds is versioned on top.
- **No version; the reference is the promise** (today). A client cannot tell which commands a scope has, and a
  breaking change is seen only when a client fails.
- **The program version as the interface version.** It changes with every fix and every release, so a client would
  need a table of releases; it cannot say "nothing you use changed".
- **One version per command or route, or a list of capabilities.** Finer, and harder to keep true; a list of the
  entries a scope offers can be added later as a query of the same version.
- **Everything stable at once.** It would freeze the page's own routes and the development commands, which change with
  the screen and the hardware work.

## Consequences

- A GUI or an integration can be written against the reference and checked against `oos-ui --serve` on any host,
  without a scope and without touching the core; it can refuse a scope whose major version it does not know.
- Every change of a remote command or route costs more: its mark, its `Interface:` line, and for a stable entry a
  deprecation period. Experimental is the way to try something without that.
- The comparison already holds the SCPI answers; the HTTP answers join it, and with them some minutes per pull request.
- `docs/scpi.md` gets a column or a mark per row, and the tables written from the settings table
  (`ui/src/settings/`) get it from the row, so the mark has one home (ADR 0035, ADR 0043).
- The interface stays an unauthenticated bench-LAN service (ADR 0025); being stable changes nothing about who may use
  it. ADR 0056 builds the extension interface for add-ons on this version.
- Not decided here: the reference page's form, the names of the version query and route, the first marks of each
  entry. They are the work of issue #205.
- Worth a second look when an authenticated transport comes (ADR 0025), or when clients need a stream (records as
  they are taken) that neither SCPI nor the routes give.

## References

Issue #205; ADR 0002, ADR 0003, ADR 0013, ADR 0022, ADR 0023, ADR 0024, ADR 0025, ADR 0032, ADR 0035, ADR 0043,
ADR 0046, ADR 0056; [scpi.md](../../scpi.md) (Generator, logic and diagnostic extensions: the development commands),
[network.md](../../network.md) 4, [Remote-Control.md](../../../wiki/Remote-Control.md); `ui/src/http.rs` (`answer`,
`mcp`, `foreign`), `ui/src/cmd.rs` (`remote_ok`), `ui/src/scpi.rs`, `ui/src/scpi/dispatch.rs` (`*IDN?`),
`ui/tests/scpi.txt`, `tools/scpi-coverage.py`, `tools/compare-base.sh`, `CHANGELOG.md`.
