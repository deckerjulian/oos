// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The protocol decoders as modules: what each one is, independent of how a screen shows it. A decoder's
//! [`ModuleInfo`] stands next to its decoder, in the file of its family (`decode/<family>.rs`; C-PHY and the line
//! codes in `cphy.rs` and `linecode.rs`), and [`MODULES`] lists them all in the order setups and SCPI count them.
//! The scope's side of a decoder (menu rows, labels, demo traffic, its view) is a protocol module of `oos-ui` that
//! names its `ModuleInfo` (`ui/src/protocols/mod.rs`, ADR 0002).

use crate::cphy::CPHY;
use crate::decode::*;
use crate::linecode::{MANCHESTER, MLT3};

/// A protocol decoder as setups, SCPI and the menus know it.
#[derive(Clone, Copy, Debug)]
pub struct ModuleInfo {
    /// Stable name in saved setups and over SCPI: letters and digits only, never changed once released (ADR 0002).
    pub key: &'static str,
    /// Name in the Decode menu, in the event table and on the decoder's label.
    pub name: &'static str,
    /// The file of its decoder, `decode/<family>.rs` or `<family>.rs` in `dsp/src`, where this info stands too.
    pub family: &'static str,
    /// The lines the decoder takes, in this order: main, second, third (optional), fourth (optional); `None` = not
    /// taken. The Decode menu names its source rows after them.
    pub roles: [Option<&'static str>; 4],
    /// Whether the main line may be a differential pair (RS-485 style A - B).
    pub diff_ok: bool,
}

impl ModuleInfo {
    /// A decoder of one single-ended line, the main one ("Source").
    pub const fn new(key: &'static str, name: &'static str, family: &'static str) -> ModuleInfo {
        ModuleInfo { key, name, family, roles: [Some("Source"), None, None, None], diff_ok: false }
    }

    /// The decoder with these lines.
    pub const fn roles(self, roles: [Option<&'static str>; 4]) -> ModuleInfo {
        ModuleInfo { roles, ..self }
    }

    /// The decoder with a main line that may be a differential pair.
    pub const fn differential(self) -> ModuleInfo {
        ModuleInfo { diff_ok: true, ..self }
    }
}

/// Every decoder, in the order setups and SCPI count them: the seven core protocols first, at fixed places, then
/// the others in the order they were added. A new one goes to the end; a key is never reused.
pub static MODULES: [&ModuleInfo; 29] = [
    &UART,
    &I2C,
    &SPI,
    &CAN,
    &LIN,
    &PARALLEL,
    &CPHY,
    &DMX512,
    &SPDIF,
    &USB_PD,
    &DCC,
    &MANCHESTER,
    &MLT3,
    &UART9,
    &MODBUS_RTU,
    &MODBUS_ASCII,
    &ONE_WIRE,
    &DS2482,
    &SWD,
    &JTAG,
    &QSPI,
    &I3C,
    &CAN_FD,
    &FLEXRAY,
    &SENT,
    &ARINC429,
    &MIL1553,
    &I2S,
    &USB,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The source file of `family`, and its text.
    fn family_file(family: &str) -> Option<(String, String)> {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        [format!("decode/{}.rs", family), format!("{}.rs", family)]
            .into_iter()
            .find_map(|f| std::fs::read_to_string(src.join(&f)).ok().map(|t| (f, t)))
    }

    /// The keys of every `ModuleInfo::new` in the files below `dir`.
    fn declared(dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().map(Result::unwrap) {
            let p = e.path();
            if p.is_dir() {
                declared(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).unwrap();
                for part in text.split("ModuleInfo::new(\"").skip(1) {
                    out.push(part.split('"').next().unwrap().to_string());
                }
            }
        }
    }

    // Break it catches: a key or a name used twice, a key a setup file cannot hold, a decoder without a main line,
    // an info that does not stand in the file of its family
    #[test]
    fn modules_are_well_formed() {
        let mut keys = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        for m in MODULES {
            assert!(!m.key.is_empty() && m.key.chars().all(|c| c.is_ascii_alphanumeric()), "key {:?}", m.key);
            assert!(keys.insert(m.key), "{}: key used twice", m.key);
            assert!(names.insert(m.name), "{}: name {} used twice", m.key, m.name);
            assert!(m.roles[0].is_some(), "{}: no main line", m.key);
            let (file, text) = family_file(m.family)
                .unwrap_or_else(|| panic!("{}: family {}: no dsp/src/decode/{}.rs", m.key, m.family, m.family));
            let decl = format!("ModuleInfo::new(\"{}\"", m.key);
            assert!(
                text.contains(&decl),
                "{}: its ModuleInfo is not in dsp/src/{}, the file of its family",
                m.key,
                file
            );
        }
    }

    // Break it catches: a decoder whose ModuleInfo is declared but not in MODULES, so the scope never offers it
    #[test]
    fn every_declared_module_is_registered() {
        let mut keys = Vec::new();
        declared(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut keys);
        assert!(keys.len() >= MODULES.len(), "{:?}", keys);
        for k in keys {
            assert!(MODULES.iter().any(|m| m.key == k), "{}: its ModuleInfo is not in oos_dsp::protocols::MODULES", k);
        }
    }
}
