// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The decoders: the protocol of a decoder, the settings of one decoder slot, and the methods that select, load and
//! store the slots the menu edits. A new decoder setting goes here, with its field in `DecCfg`.

use super::*;

/// Protocol of the decoder: a protocol module (`protocols::MODULES[i]`). The core decoders stand first at fixed places
/// (`Proto::UART` ...), the code that knows them by name uses those.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Proto {
    Module(u8),
}

impl Proto {
    pub const UART: Proto = Proto::Module(0);
    pub const I2C: Proto = Proto::Module(1);
    pub const SPI: Proto = Proto::Module(2);
    pub const CAN: Proto = Proto::Module(3);
    pub const LIN: Proto = Proto::Module(4);
    pub const PARALLEL: Proto = Proto::Module(5);
    pub const CPHY: Proto = Proto::Module(6);

    /// What oos-dsp says of the protocol's decoder: its `ModuleInfo`, at the same place in
    /// `oos_dsp::protocols::MODULES` as the module in `protocols::MODULES` (`protocols::every_decoder_is_complete`).
    /// What the scope shows of it (menu rows, labels, demo traffic) is the module's, `Proto::module`.
    fn info(self) -> &'static oos_dsp::protocols::ModuleInfo {
        let Proto::Module(i) = self;
        oos_dsp::protocols::MODULES[i as usize]
    }

    /// Name in saved setups: the module's key.
    pub fn key(self) -> String {
        self.info().key.to_string()
    }

    pub fn from_key(k: &str) -> Option<Proto> {
        (0..oos_dsp::protocols::MODULES.len()).map(|i| Proto::Module(i as u8)).find(|p| p.key() == k)
    }

    pub fn name(self) -> &'static str {
        self.info().name
    }

    /// The role whose line frames start on, where the hardware trigger in front of the protocol trigger looks.
    pub fn start_role(self) -> usize {
        self.info().start.0
    }

    /// Role names of the sources (main, second, third optional, fourth optional); None = not used.
    pub fn roles(self) -> [Option<&'static str>; 4] {
        self.info().roles
    }

    /// Serial protocols on one line that can also be decoded from a differential pair (RS-485: A - B).
    pub fn diff_ok(self) -> bool {
        self.info().diff_ok
    }
}

/// Settings of one decoder (the `dec_*` fields of `Model` are the one the menu edits).
#[derive(Clone, PartialEq, Debug)]
pub struct DecCfg {
    pub on: bool,
    pub src: usize,
    pub baud: usize,
    pub canrate: usize,
    pub canh: bool,
    pub fdrate: usize,
    pub arate: usize,
    pub tick: usize,
    pub nibbles: u32,
    pub uspeed: usize,
    pub cphy_rate: f64,
    pub src4: Option<usize>,
    pub la_in: bool,
    pub la: [Option<u8>; 4],
    pub diff: Option<usize>,
    pub thr: f32,
    pub ascii: bool,
    pub proto: Proto,
    pub src2: usize,
    pub cs: Option<usize>,
    pub mode: u8,
    pub bits: u32,
    pub parity: oos_dsp::decode::Parity,
    pub opt: [u8; oos_dsp::protocols::OPTS],
}

/// Decoders at once.
pub const DECODERS: usize = 4;

impl Model {
    /// The settings of the decoder the menu edits.
    pub fn dec_cfg(&self) -> DecCfg {
        DecCfg {
            on: self.dec_on,
            src: self.dec_src,
            baud: self.dec_baud,
            canrate: self.dec_canrate,
            canh: self.dec_canh,
            fdrate: self.dec_fdrate,
            arate: self.dec_arate,
            tick: self.dec_tick,
            nibbles: self.dec_nibbles,
            uspeed: self.dec_uspeed,
            cphy_rate: self.dec_cphy_rate,
            src4: self.dec_src4,
            la_in: self.dec_la_in,
            la: self.dec_la,
            diff: self.dec_diff,
            thr: self.dec_thr,
            ascii: self.dec_ascii,
            proto: self.dec_proto,
            src2: self.dec_src2,
            cs: self.dec_cs,
            mode: self.dec_mode,
            bits: self.dec_bits,
            parity: self.dec_parity,
            opt: self.dec_opt,
        }
    }

    fn dec_load(&mut self, c: &DecCfg) {
        self.dec_on = c.on;
        self.dec_src = c.src;
        self.dec_baud = c.baud;
        self.dec_canrate = c.canrate;
        self.dec_canh = c.canh;
        self.dec_fdrate = c.fdrate;
        self.dec_arate = c.arate;
        self.dec_tick = c.tick;
        self.dec_nibbles = c.nibbles;
        self.dec_uspeed = c.uspeed;
        self.dec_cphy_rate = c.cphy_rate;
        self.dec_src4 = c.src4;
        self.dec_la_in = c.la_in;
        self.dec_la = c.la;
        self.dec_diff = c.diff;
        self.dec_thr = c.thr;
        self.dec_ascii = c.ascii;
        self.dec_proto = c.proto;
        self.dec_src2 = c.src2;
        self.dec_cs = c.cs;
        self.dec_mode = c.mode;
        self.dec_bits = c.bits;
        self.dec_parity = c.parity;
        self.dec_opt = c.opt;
    }

    /// Settings of decoder `k`.
    pub fn dec_slot(&self, k: usize) -> DecCfg {
        if k == self.dec_sel {
            self.dec_cfg()
        } else {
            self.decs[k].clone().unwrap_or_else(|| Model::default().dec_cfg())
        }
    }

    pub fn dec_is_on(&self, k: usize) -> bool {
        if k == self.dec_sel {
            self.dec_on
        } else {
            self.decs[k].as_ref().is_some_and(|c| c.on)
        }
    }

    /// The model as decoder `k` sees it, for the code that reads the `dec_*` fields.
    pub fn dec_view(&self, k: usize) -> Model {
        let mut m = self.clone();
        if k != self.dec_sel {
            m.dec_load(&self.dec_slot(k));
        }
        m
    }

    /// Makes decoder `k` the one the menu edits.
    pub fn dec_select(&mut self, k: usize) {
        let k = k.min(DECODERS - 1);
        if k != self.dec_sel {
            let c = self.dec_slot(k);
            self.decs[self.dec_sel] = Some(self.dec_cfg());
            self.dec_load(&c);
            self.dec_sel = k;
        }
    }

    /// Writes `c` into slot `k` (setups).
    pub fn dec_store(&mut self, k: usize, c: DecCfg) {
        if k == self.dec_sel {
            self.dec_load(&c);
        } else {
            self.decs[k] = Some(c);
        }
    }

    /// The decoder reads logic analyser lines (the choice only counts while the LA is on).
    pub fn dec_on_la(&self) -> bool {
        self.dec_la_in && self.la_on() && self.dec_proto != Proto::CPHY
    }
}
