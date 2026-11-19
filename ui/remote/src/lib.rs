// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! oos-remote: the remote transports of oos-ui (ADR 0064): VXI-11 on the scope, HiSLIP on a computer, the web server,
//! and their announcement by mDNS / DNS-SD and LXI (ADR 0024). A transport only frames messages (ADR 0022). VXI-11 and
//! HiSLIP take a program message from their client, hand it to the one SCPI interpreter of oos-ui as a `Program`, and
//! send back what the interpreter answered. The web server reads HTTP requests, keeps their limits and sends the
//! answers; what a route answers from the screen (its state, files, pictures, MCP) it asks oos-ui through
//! `http::Site`. None of them names anything of oos-ui; the compiler keeps it that way.

pub mod announce;
pub mod hislip;
pub mod http;
pub mod vxi11;

/// One program message of a client, as a transport hands it to the interpreter and keeps it while the interpreter is
/// not done with it (a `*OPC?` that waits for an operation). The interpreter of oos-ui (`scpi::Program`) is the one
/// that runs on the scope; the tests of this crate have their own.
pub trait Program {
    /// The message as it came, one or more commands.
    fn new(text: &str) -> Self;
    /// The length of the message as it came, which a transport counts against its limits.
    fn bytes(&self) -> usize;
    /// Whether the message has an answer to send, also an empty one (a query that answered nothing).
    fn has_reply(&self) -> bool;
}

/// A program of the tests: its commands are run at once by a closure, as the interpreter would run them without
/// waiting. The separators of the answers are those of oos-ui's interpreter: `;` between the answers of one line,
/// a newline between lines.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Line {
    commands: std::collections::VecDeque<(String, bool)>,
    out: Vec<u8>,
    separator: u8,
    bytes: usize,
    has_reply: bool,
}

#[cfg(test)]
impl Program for Line {
    fn new(text: &str) -> Self {
        let mut commands = std::collections::VecDeque::new();
        for line in text.split(['\n', '\r']).filter(|line| !line.trim().is_empty()) {
            for (index, command) in line.split(';').map(str::trim).filter(|s| !s.is_empty()).enumerate() {
                commands.push_back((command.to_string(), index == 0));
            }
        }
        Line { commands, separator: b';', bytes: text.len(), ..Line::default() }
    }
    fn bytes(&self) -> usize {
        self.bytes
    }
    fn has_reply(&self) -> bool {
        self.has_reply
    }
}

#[cfg(test)]
impl Line {
    /// Runs every command with `exec`; the answers of the queries, joined.
    pub(crate) fn immediate(&mut self, exec: &mut dyn FnMut(&str) -> Vec<u8>) -> Vec<u8> {
        while let Some((command, new_line)) = self.commands.pop_front() {
            if new_line && self.has_reply {
                self.separator = b'\n';
            }
            let reply = exec(&command);
            if !reply.is_empty() || command.split_whitespace().next().is_some_and(|header| header.ends_with('?')) {
                if self.has_reply {
                    self.out.push(self.separator);
                }
                self.out.extend_from_slice(&reply);
                self.separator = b';';
                self.has_reply = true;
            }
        }
        std::mem::take(&mut self.out)
    }
}

// the generator of the hostile-input tests (ADR 0047), one file for every crate
#[cfg(test)]
#[path = "../../../hal/src/hostile/gen.rs"]
mod hostile;
