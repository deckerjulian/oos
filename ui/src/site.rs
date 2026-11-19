// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What the web remote answers from the screen (ADR 0049, the command layer, beside `cmd` and `scpi`): its state, the
//! last record as a file, an update package, a waveform file for a reference slot, the live picture, and MCP. The
//! transport is `oos_remote::http` (ADR 0064): it reads the requests, keeps the limits and sends the answers, and asks
//! the screen through `oos_remote::http::Site`, which `Ctx` is for one pass of the frame loop.

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
}

/// The waveform files `POST /waveform` takes, read on the web server's thread.
pub type Upload = Vec<crate::store::WaveTrace>;

/// The web server of oos-ui: the transport of oos-remote with the screen's uploads.
pub type Web = oos_remote::http::Http<Upload>;

impl oos_remote::http::Site for Ctx<'_> {
    type Upload = Upload;
    const MAX_WAVEFORM: usize = crate::store::MAX_WAVEFORM_BYTES;

    /// The live picture's turn (`Live::tick`).
    fn pass(&mut self) {
        if let Some(live) = self.live {
            live.tick(self.cv);
        }
    }

    /// The live picture answers it; 503 without one (tests that ask for no picture).
    fn picture(&mut self, s: &TcpStream, have: Option<String>, jpeg: bool) {
        match self.live {
            Some(live) => live.request(s, have, self.cv, jpeg),
            None => crate::net::busy_answer(s),
        }
    }

    /// Keys, knobs and taps (`cmd::remote_ok`).
    fn remote_ok(&self, line: &str) -> bool {
        crate::cmd::remote_ok(line)
    }

    /// /ui-state, /stamp, /wave.csv, /la.csv, /wave.bin, POST /update, /screen.bmp and /mcp.
    fn answer(&mut self, route: &str, post: bool, body: &[u8]) -> Option<(&'static str, Vec<u8>, bool)> {
        let ok = |ctype: &'static str, b: Vec<u8>| Some((ctype, b, false));
        let file = |ctype: &'static str, r: Result<Vec<u8>, String>| match r {
            Ok(b) => Some((ctype, b, false)),
            Err(e) => Some(("text/plain", e.into_bytes(), true)),
        };
        let ui = &mut *self.ui;
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
            "/screen.bmp" => ok("image/bmp", crate::scpi::bmp(self.cv)),
            "/mcp" => match mcp(&String::from_utf8_lossy(body), self) {
                Some(r) => ok("application/json", r.into_bytes()),
                None => ok("", Vec::new()),
            },
            _ => None,
        }
    }

    /// The state as `oos-update status --quick` gives it, and the result of the last upload.
    fn update_state(&mut self) -> (String, String) {
        let (status, last) = self.ui.update_state();
        (status, last.to_string())
    }

    /// `Ui::waveform_epoch`.
    fn waveform_epoch(&self) -> u64 {
        self.ui.waveform_epoch()
    }

    fn parse_upload(file: &[u8], name: &str) -> Result<Upload, String> {
        crate::store::parse_wave(file, name)
    }

    fn take_upload(&mut self, parsed: Result<Upload, String>, name: &str, slot: usize, epoch: u64) -> (bool, String) {
        let ui = &mut *self.ui;
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

#[cfg(test)]
mod tests;
