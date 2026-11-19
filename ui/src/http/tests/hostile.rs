// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hostile input for the web remote (ADR 0047): whole requests over a socket, as a client sends them, so the head,
//! the routes, the queries, the upload paths and MCP are all read the way they are on a scope.

use super::*;

/// One request whose sender closes its side after the last byte; the server is polled until it has closed the
/// connection. What came back, or why the answer did not end: a server that keeps the connection open fails after
/// 10 s instead of holding the test.
fn exchange(
    h: &mut Http,
    ui: &mut Ui,
    cv: &Canvas,
    scpi: &mut crate::scpi::Scpi,
    req: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let port = h.port().unwrap();
    let client = std::thread::spawn(move || {
        let mut c = TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("connect: {e}"))?;
        c.set_read_timeout(Some(std::time::Duration::from_secs(10))).map_err(|e| e.to_string())?;
        let _ = c.write_all(&req);
        let _ = c.shutdown(std::net::Shutdown::Write);
        let mut out = Vec::new();
        c.read_to_end(&mut out).map_err(|e| format!("the answer did not end ({e}) after {} bytes", out.len()))?;
        Ok(out)
    });
    while !client.is_finished() {
        h.poll(&mut Ctx::new(ui, cv, scpi));
        std::thread::yield_now();
    }
    client.join().unwrap()
}

/// Commands the test keeps away from MCP so it stays inside the test process (`hostile::remote::sandboxed` for the
/// interpreter's own test): the valid requests hold none, and a mutation would have to write one.
fn sandboxed(req: &[u8]) -> bool {
    let text = String::from_utf8_lossy(req).to_ascii_uppercase();
    [":BODE", ":SIM", ":CAL", "LOAD", ":IMP", "MAP", "LANE"].iter().any(|w| text.contains(w))
}

/// `Http::poll` and `Http::answer` (the web remote on TCP 8080: the page's requests, `/cmd`, the uploads to
/// `/waveform` and `/update`, MCP): return for any bytes; a request whose head and body are whole is answered; an
/// answer is a status line the server knows, a head, and a body as long as its Content-Length says.
#[test]
fn hostile_requests() {
    let _one_at_a_time = sends();
    let mut h = Http::open(0);
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
    let post = |path: &str, body: &[u8]| {
        let head = format!("POST {path} HTTP/1.1\r\nHost: 192.0.2.10:8080\r\nContent-Length: {}\r\n\r\n", body.len());
        [head.as_bytes(), body].concat()
    };
    let mcp = |method: &str, params: &str| {
        post("/mcp", format!(r#"{{"jsonrpc":"2.0","id":3,"method":"{method}","params":{params}}}"#).as_bytes())
    };
    let files = [
        b"GET /stamp HTTP/1.1\r\nHost: 192.0.2.10:8080\r\nOrigin: http://192.0.2.10:8080\r\n\r\n".to_vec(),
        b"GET /cmd?c=key+run&c=touch%20100%20200&x=%zz HTTP/1.1\r\nHost: scope.local\r\nSec-Fetch-Site: same-origin\r\n\
          \r\n"
        .to_vec(),
        b"GET /ui-state HTTP/1.1\r\n\r\n".to_vec(),
        b"GET /lxi/identification HTTP/1.1\r\nHost: [2001:db8::7]:8080\r\n\r\n".to_vec(),
        b"GET /nothing/here?a=b HTTP/1.1\r\nHost: x\r\nIf-None-Match: \"0\"\r\n\r\n".to_vec(),
        post("/waveform?slot=2&name=..%2Fclock%0A.csv", b"0,1\n1e-6,2\n2e-6,3\n"),
        post("/update", b"OOSUPD1\0 not a package"),
        mcp("tools/call", r#"{"name":"scpi","arguments":{"command":"*IDN?;:CHAN1:DISP ON;:CHAN1:DISP?"}}"#),
        mcp("initialize", r#"{"protocolVersion":"2025-06-18"}"#),
        mcp("tools/list", "{}"),
    ];
    let seeds: Vec<&[u8]> = files.iter().map(|f| f.as_slice()).collect();
    // every request is a connection, and a closed one keeps its port for a while: at most 4000 of them in a run
    let div = (crate::hostile::iters(1) / 4000).max(8);
    crate::hostile::run("http::poll", &seeds, div, |b| {
        if sandboxed(b) {
            return Ok(());
        }
        let answer = exchange(&mut h, &mut ui, &cv, &mut scpi, b.to_vec())?;
        // whole: the head ends, and the body is exactly as long as the head says (as the server reads the length)
        let head_end = b.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4);
        let whole = head_end.is_some_and(|e| {
            let head = String::from_utf8_lossy(&b[..e]).to_ascii_lowercase();
            let said = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok());
            e <= 16384 && b.len() - e == said.unwrap_or(0)
        });
        if answer.is_empty() {
            return if whole { Err("a whole request without an answer".into()) } else { Ok(()) };
        }
        const STATUS: [&str; 8] = [
            "200 OK",
            "202 Accepted",
            "304 Not Modified",
            "403 Forbidden",
            "404 Not Found",
            "409 Conflict",
            "413 Payload Too Large",
            "503 Service Unavailable",
        ];
        let text = String::from_utf8_lossy(&answer);
        let known = text.strip_prefix("HTTP/1.1 ").is_some_and(|rest| STATUS.iter().any(|s| rest.starts_with(s)));
        let Some(end) = answer.windows(4).position(|w| w == b"\r\n\r\n").filter(|_| known) else {
            return Err(format!("the answer starts {:?}", &text[..text.len().min(60)]));
        };
        let length = String::from_utf8_lossy(&answer[..end])
            .lines()
            .find_map(|l| l.strip_prefix("Content-Length: ").and_then(|v| v.parse::<usize>().ok()));
        // an answer to a request with bytes left unread can be cut short by the reset of the connection
        if whole && length.is_some_and(|n| n != answer.len() - end - 4) {
            return Err(format!("Content-Length {:?} before a body of {} bytes", length, answer.len() - end - 4));
        }
        Ok(())
    });
}
