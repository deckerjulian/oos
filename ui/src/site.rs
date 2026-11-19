// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What the web remote answers from the screen (ADR 0049, the command layer, beside `cmd` and `scpi`): its state, the
//! last record as a file, an update package, a waveform file for a reference slot, and MCP. http.rs is the transport:
//! it reads the requests, keeps the limits and sends the answers, and hands a request here through `Ctx`, the one
//! thing of the screen it holds. It names nothing of the screen itself, so it can leave oos-ui (ADR 0064).

use crate::gfx::Canvas;
use crate::scpi::Scpi;
use crate::shots::Live;
use crate::ui::Ui;
use std::net::TcpStream;

/// What an answer of the web server may use: the screen, its last picture, the SCPI interpreter and the live picture,
/// for one pass of the frame loop.
pub struct Ctx<'a> {
    ui: &'a mut Ui,
    cv: &'a Canvas,
    scpi: &'a mut Scpi,
    live: Option<&'a Live>,
}

impl<'a> Ctx<'a> {
    pub fn new(ui: &'a mut Ui, cv: &'a Canvas, scpi: &'a mut Scpi) -> Ctx<'a> {
        Ctx { ui, cv, scpi, live: None }
    }

    /// With the live picture of the frame loop: without it `/screen.png` and `/screen.jpg` are answered 503 (tests
    /// that ask for no picture).
    pub fn with_live(mut self, live: &'a Live) -> Ctx<'a> {
        self.live = Some(live);
        self
    }

    /// `Ui::waveform_epoch`: a waveform upload of an older session does not go into the slot.
    pub fn waveform_epoch(&self) -> u64 {
        self.ui.waveform_epoch()
    }
}

/// Every pass of the web server, before its requests: the live picture's turn (`Live::tick`).
pub fn pass(cx: &Ctx) {
    if let Some(live) = cx.live {
        live.tick(cx.cv);
    }
}

/// `GET /screen.png` or `/screen.jpg` on `s` (`have`: the ETag of If-None-Match): the live picture answers it.
pub fn picture(cx: &Ctx, s: &TcpStream, have: Option<String>, jpeg: bool) {
    match cx.live {
        Some(live) => live.request(s, have, cx.cv, jpeg),
        None => crate::net::busy_answer(s),
    }
}

/// Whether `GET /cmd` may run the text command `line` (keys, knobs, taps: `cmd::remote_ok`).
pub fn remote_ok(_cx: &Ctx, line: &str) -> bool {
    crate::cmd::remote_ok(line)
}

/// The longest waveform file `POST /waveform` takes.
pub const MAX_WAVEFORM: usize = crate::store::MAX_WAVEFORM_BYTES;

/// The traces of a waveform file, or why it was not read.
pub type Traces = Result<Vec<crate::store::WaveTrace>, String>;

/// Reads a waveform file of `POST /waveform` (on the web server's thread for it, not the frame loop).
pub fn parse_upload(file: &[u8], name: &str) -> Traces {
    crate::store::parse_wave(file, name)
}

/// Puts the traces of an upload into reference slot `slot` (R1 is 0) when the session is still the one of `epoch`:
/// (taken, the text for the page).
pub fn take_upload(cx: &mut Ctx, parsed: Traces, name: &str, slot: usize, epoch: u64) -> (bool, String) {
    let ui = &mut *cx.ui;
    let previous = ui.ref_slot();
    let result = if ui.waveform_epoch() != epoch {
        Err("The session changed while the file was read; load it again.".to_string())
    } else {
        parsed.and_then(|traces| ui.commit_waveform_traces_at(traces, name, slot))
    };
    match result {
        Ok(()) => {
            let hint = if ui.record_mode() {
                "Use Run/Stop or Single to acquire from this record; Return to live restores the live source."
            } else {
                "It is on with the live channels. Fit shows its full record; Take over enters stored acquisition."
            };
            (true, format!("Loaded {} into R{}. {}", name, ui.ref_slot() + 1, hint))
        }
        Err(error) => {
            ui.ref_slot_set(previous);
            (false, error)
        }
    }
}

/// What `GET /update` shows: the state as `oos-update status --quick` gives it, and the result of the last upload.
pub fn update_state(cx: &Ctx) -> (String, String) {
    let (status, last) = cx.ui.update_state();
    (status, last.to_string())
}

/// The answer of a route of the screen: (content type, body, whether the body is the reason it failed and not the
/// data asked for); None for a route that is not one of them. An empty content type is 202 with no body (an MCP
/// notification).
pub fn answer(cx: &mut Ctx, route: &str, post: bool, body: &[u8]) -> Option<(&'static str, Vec<u8>, bool)> {
    let ok = |ctype: &'static str, b: Vec<u8>| Some((ctype, b, false));
    let file = |ctype: &'static str, r: Result<Vec<u8>, String>| match r {
        Ok(b) => Some((ctype, b, false)),
        Err(e) => Some(("text/plain", e.into_bytes(), true)),
    };
    let ui = &mut *cx.ui;
    match route {
        "/ui-state" if !post => ok("application/json", ui.workflow_state().into_bytes()),
        // the scope's local time for the names of the page's downloads; empty while its clock is not set
        "/stamp" => ok("text/plain", ui.file_stamp().unwrap_or_default().into_bytes()),
        "/wave.csv" => file("text/csv; charset=utf-8", ui.web_wave_csv().map(String::into_bytes)),
        "/la.csv" => file("text/csv; charset=utf-8", ui.web_la_csv().map(String::into_bytes)),
        "/wave.bin" => file("application/octet-stream", ui.web_wave_bin()),
        "/update" if post => match ui.update_upload(body) {
            Ok(m) => ok("text/plain", m.into_bytes()),
            Err(e) => ok("text/plain", format!("REFUSED: {}", e).into_bytes()),
        },
        "/screen.bmp" => ok("image/bmp", crate::scpi::bmp(cx.cv)),
        "/mcp" => match mcp(&String::from_utf8_lossy(body), cx) {
            Some(r) => ok("application/json", r.into_bytes()),
            None => ok("", Vec::new()),
        },
        _ => None,
    }
}

/// One MCP JSON-RPC message; None for notifications (answered with 202 and no body).
pub fn mcp(body: &str, cx: &mut Ctx) -> Option<String> {
    let (ui, cv, scpi) = (&mut *cx.ui, cx.cv, &mut *cx.scpi);
    use crate::json::{parse, J};
    let err = |id: J, code: f64, msg: &str| {
        J::obj(&[
            ("jsonrpc", J::s("2.0")),
            ("id", id),
            ("error", J::obj(&[("code", J::Num(code)), ("message", J::s(msg))])),
        ])
        .to_string()
    };
    let Some(req) = parse(body) else { return Some(err(J::Null, -32700.0, "parse error")) };
    let id = req.get("id").cloned()?;
    let method = req.get("method").and_then(J::str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(J::Null);
    let text = |s: &str| J::obj(&[("content", J::Arr(vec![J::obj(&[("type", J::s("text")), ("text", J::s(s))])]))]);
    let result = match method {
        "initialize" => {
            let ver = params.get("protocolVersion").and_then(J::str).unwrap_or("2025-06-18").to_string();
            J::obj(&[
                ("protocolVersion", J::Str(ver)),
                ("capabilities", J::obj(&[("tools", J::obj(&[]))])),
                ("serverInfo", J::obj(&[("name", J::s("oos-ui")), ("version", J::s(env!("CARGO_PKG_VERSION")))])),
                ("instructions", J::s("Rigol DHO924S running OOS. Use the scpi tool with Rigol-style commands (e.g. :TIM:SCAL 1e-3, :CHAN1:SCAL 0.5, :MEAS:ITEM? VPP,CHAN1, :WAV:DATA?) and screenshot to see the screen.")),
            ])
        }
        "ping" => J::obj(&[]),
        "tools/list" => {
            let schema = |props: J, req: Vec<J>| {
                J::obj(&[("type", J::s("object")), ("properties", props), ("required", J::Arr(req))])
            };
            J::obj(&[(
                "tools",
                J::Arr(vec![
                    J::obj(&[
                        ("name", J::s("scpi")),
                        ("description", J::s("Run one SCPI program message on the oscilloscope (commands separated by ';'); returns the response of the queries.")),
                        ("inputSchema", schema(J::obj(&[("command", J::obj(&[("type", J::s("string"))]))]), vec![J::s("command")])),
                    ]),
                    J::obj(&[
                        ("name", J::s("screenshot")),
                        ("description", J::s("The current oscilloscope screen as a PNG image.")),
                        ("inputSchema", schema(J::obj(&[]), vec![])),
                    ]),
                ]),
            )])
        }
        "tools/call" => match params.get("name").and_then(J::str) {
            Some("scpi") => {
                let c = params.get("arguments").and_then(|a| a.get("command")).and_then(J::str).unwrap_or("");
                match scpi.exec_message_checked(ui, cv, c) {
                    Ok((r, has_reply)) => text(if r.is_empty() && !has_reply {
                        "ok"
                    } else {
                        std::str::from_utf8(&r).unwrap_or("(binary response)")
                    }),
                    Err(error) => J::obj(&[
                        ("isError", J::Bool(true)),
                        ("content", J::Arr(vec![J::obj(&[("type", J::s("text")), ("text", J::s(error))])])),
                    ]),
                }
            }
            Some("screenshot") => J::obj(&[(
                "content",
                J::Arr(vec![J::obj(&[
                    ("type", J::s("image")),
                    ("mimeType", J::s("image/png")),
                    ("data", J::Str(crate::base64::encode(&crate::gfx::png::png_of(cv)))),
                ])]),
            )]),
            _ => return Some(err(id, -32602.0, "unknown tool")),
        },
        _ => return Some(err(id, -32601.0, "method not found")),
    };
    Some(J::obj(&[("jsonrpc", J::s("2.0")), ("id", id), ("result", result)]).to_string())
}
