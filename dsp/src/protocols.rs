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

/// What the protocol trigger looks for in the decoded packets ([`ModuleInfo::trigger_match`]); the trigger point is
/// the end of the matching field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BusWhat {
    /// every frame start (SOF, break, I2C start, 1553 command word, UART / SPI word)
    Frame,
    /// identifier / address / 1553 command word equal to the value
    Id,
    /// data byte / word / audio sample equal to the value
    Data,
    /// any packet with a failed check (CRC, parity, checksum, NACK, missing ACK)
    Error,
}

/// A protocol's own rule of the protocol trigger: whether a packet satisfies "`what` = `value`"; `None` = the generic
/// rule.
pub type TriggerRule = fn(BusWhat, u32, &Packet) -> Option<bool>;

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
    /// The protocol's own rule of the protocol trigger, before the generic one ([`ModuleInfo::trigger_match`]).
    pub trigger: TriggerRule,
    /// What the protocol trigger looks for when it is switched on: a value the demo traffic contains.
    pub trigger_default: (BusWhat, u32),
}

impl ModuleInfo {
    /// A decoder of one single-ended line, the main one ("Source").
    pub const fn new(key: &'static str, name: &'static str, family: &'static str) -> ModuleInfo {
        ModuleInfo {
            key,
            name,
            family,
            roles: [Some("Source"), None, None, None],
            diff_ok: false,
            trigger: |_, _, _| None,
            trigger_default: (BusWhat::Frame, 0x40),
        }
    }

    /// The decoder with these lines.
    pub const fn roles(self, roles: [Option<&'static str>; 4]) -> ModuleInfo {
        ModuleInfo { roles, ..self }
    }

    /// The decoder with a main line that may be a differential pair.
    pub const fn differential(self) -> ModuleInfo {
        ModuleInfo { diff_ok: true, ..self }
    }

    /// The decoder with a trigger rule of its own.
    pub const fn trigger(self, trigger: TriggerRule) -> ModuleInfo {
        ModuleInfo { trigger, ..self }
    }

    /// The decoder with the protocol trigger starting on "`what` = `value`".
    pub const fn starts_on(self, what: BusWhat, value: u32) -> ModuleInfo {
        ModuleInfo { trigger_default: (what, value), ..self }
    }

    /// Whether `p` satisfies the protocol trigger "`what` = `value`" on its own: the module's rule, else the generic
    /// one (frame starts, IDs and addresses, data, failed checks).
    pub fn trigger_match(&self, what: BusWhat, value: u32, p: &Packet) -> bool {
        (self.trigger)(what, value, p).unwrap_or_else(|| generic_match(what, value, p))
    }
}

/// The trigger rule of a decoder whose every word starts a frame (UART, SPI, a parallel bus).
pub(crate) fn every_word_is_a_frame(what: BusWhat, _value: u32, p: &Packet) -> Option<bool> {
    (what == BusWhat::Frame && p.kind == Kind::Data).then_some(true)
}

/// The generic rule of the protocol trigger.
fn generic_match(what: BusWhat, v: u32, p: &Packet) -> bool {
    match what {
        BusWhat::Frame => match p.kind {
            Kind::Start | Kind::Break | Kind::Word { cmd: true } => true,
            Kind::Sample { right: false } => true,
            // the first field of a frame of the extended decoders
            Kind::Field(l) => matches!(l, "!RESET" | "PRE" | "TSS" | "CMD" | "REQ" | "BLK" | "IR" | "DR" | "SYNC"),
            _ => false,
        },
        // an identifier, an address, a command word, a ROM code, a header or a status
        BusWhat::Id => {
            (matches!(p.kind, Kind::Id { .. } | Kind::Addr { .. } | Kind::Word { cmd: true })
                || p.kind == Kind::Field("ROM")
                || p.kind == Kind::Field("CMD")
                || p.kind == Kind::Field("HDR")
                || p.kind == Kind::Field("STAT"))
                && p.value == v
        }
        BusWhat::Data => matches!(p.kind, Kind::Data | Kind::Sample { .. } | Kind::Word { cmd: false }) && p.value == v,
        BusWhat::Error => !p.ok,
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

    // Break it catches: the protocol trigger missing a frame start, an ID, a value or a failed check, or finding one
    // in the field of another protocol (the rules of each decoder are tried on its decoded traffic in dsp/tests/decode)
    #[test]
    fn the_generic_trigger_rule_and_the_word_rule() {
        use BusWhat::*;
        let pk = |kind, value, ok| Packet::new(kind, 0.0, 1.0, value, ok);
        let (data, start) = (pk(Kind::Data, 0x41, true), pk(Kind::Start, 0, true));
        // a UART, SPI or parallel word starts a frame; an I2C or CAN byte does not, its start does
        for m in [&UART, &SPI, &PARALLEL, &UART9] {
            assert!(m.trigger_match(Frame, 0, &data) && m.trigger_match(Frame, 0, &start), "{}", m.key);
        }
        assert!(!I2C.trigger_match(Frame, 0, &data) && I2C.trigger_match(Frame, 0, &start));
        assert!(!CAN.trigger_match(Frame, 0, &data));
        for f in ["!RESET", "PRE", "TSS", "CMD", "REQ", "BLK", "IR", "DR", "SYNC"] {
            assert!(SENT.trigger_match(Frame, 0, &pk(Kind::Field(f), 0, true)), "{}", f);
        }
        assert!(!SENT.trigger_match(Frame, 0, &pk(Kind::Field("CRC"), 0, true)));
        assert!(I2S.trigger_match(Frame, 0, &pk(Kind::Sample { right: false }, 0, true)));
        assert!(!I2S.trigger_match(Frame, 0, &pk(Kind::Sample { right: true }, 0, true)));
        // an ID is an identifier, an address, a command word or one of the ID fields, with the value
        let id = pk(Kind::Id { ext: false, rtr: false }, 0x123, true);
        assert!(CAN.trigger_match(Id, 0x123, &id) && !CAN.trigger_match(Id, 0x124, &id));
        assert!(MIL1553.trigger_match(Id, 0x0c23, &pk(Kind::Word { cmd: true }, 0x0c23, true)));
        assert!(ONE_WIRE.trigger_match(Id, 0x33, &pk(Kind::Field("ROM"), 0x33, true)));
        assert!(!ONE_WIRE.trigger_match(Id, 0x33, &pk(Kind::Field("ADDR"), 0x33, true)));
        // data is a byte, a sample or a data word with the value; an error any failed check
        assert!(I2S.trigger_match(Data, 0x7fff, &pk(Kind::Sample { right: true }, 0x7fff, true)));
        assert!(UART.trigger_match(Data, 0x41, &data) && !UART.trigger_match(Data, 0x42, &data));
        assert!(SPI.trigger_match(Error, 0, &pk(Kind::Data, 0, false)) && !SPI.trigger_match(Error, 0, &data));
        // a module without a rule of its own starts on a frame
        assert_eq!(SPDIF.trigger_default, (Frame, 0x40));
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
