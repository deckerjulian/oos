// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! NMRA DCC (S-9.2.1) packet interpretation into a monitor “yard”: locomotives, turnouts, signals,
//! idle/broadcast/advanced traffic, and track status (active vs emergency stop).
//!
//! Contributed by Arjen Halma (pull request #3). Checked against generated packets only, not yet against a track.

/// Rail / command-station status derived from packets on the bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackStatus {
    /// No valid DCC packets seen yet
    Off,
    /// Idle or normal traffic; track is live
    Active,
    /// Emergency stop (broadcast or loco e-stop)
    Estop,
}

impl Default for TrackStatus {
    fn default() -> Self {
        Self::Off
    }
}

impl TrackStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "No signal",
            Self::Active => "Track active",
            Self::Estop => "Emergency stop",
        }
    }
}

/// One multi-function decoder (locomotive / train).
#[derive(Clone, Debug, PartialEq)]
pub struct Loco {
    pub addr: u16,
    /// true = forward
    pub forward: bool,
    /// 0 = stop; for 128-step mode up to 126; for 28-step NMRA mid-value mapped to 0..28
    pub speed: u8,
    /// 14, 28 or 128
    pub steps: u8,
    /// F0..F28 as a bit mask (bit 0 = F0 / FL)
    pub functions: u32,
    /// last speed packet was NMRA emergency stop
    pub estop: bool,
}

/// One accessory / turnout (basic accessory decoder output pair).
#[derive(Clone, Debug, PartialEq)]
pub struct Turnout {
    /// NMRA turnout address (1..)
    pub addr: u16,
    /// true = thrown / diverging / “left”; false = closed / normal / “right”
    pub thrown: bool,
    /// last C bit (activate)
    pub active: bool,
}

/// Extended accessory: signal head / aspect decoder.
#[derive(Clone, Debug, PartialEq)]
pub struct Signal {
    pub addr: u16,
    /// aspect byte (XXX XXXXX from the extended accessory packet)
    pub aspect: u8,
}

/// Advanced extended packet (address partition 253 / 254).
#[derive(Clone, Debug, PartialEq)]
pub struct Advanced {
    /// 253 or 254
    pub kind: u8,
    /// payload bytes after the partition address (before XOR)
    pub data: Vec<u8>,
}

/// Yard snapshot: every DCC device class seen on the rail.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Yard {
    pub status: TrackStatus,
    pub locos: Vec<Loco>,
    pub turnouts: Vec<Turnout>,
    pub signals: Vec<Signal>,
    pub advanced: Vec<Advanced>,
    /// idle packets observed (address 255)
    pub idle_count: u32,
    /// broadcast packets observed (address 0)
    pub broadcast_count: u32,
}

impl Yard {
    /// Applies one DCC packet (`addr` = first address byte, `data` = following bytes before XOR).
    pub fn apply(&self, addr: u8, data: &[u8]) -> Yard {
        // idle: 11111111 + typically one zero data byte
        if addr == 255 {
            let mut y = self.clone();
            y.idle_count = y.idle_count.saturating_add(1);
            if y.status != TrackStatus::Estop {
                y.status = TrackStatus::Active;
            }
            return y;
        }
        if data.is_empty() {
            return self.clone();
        }
        // accessory partition 10xxxxxx
        if addr & 0xc0 == 0x80 {
            if data.len() >= 2 && data[0] & 0x80 == 0 {
                return self.apply_signal(addr, data[0], data[1]);
            }
            if data.len() == 1 && data[0] & 0x80 != 0 {
                return self.with_active(self.apply_turnout(addr, data[0]));
            }
            return self.clone();
        }
        // advanced extended partitions
        if addr == 253 || addr == 254 {
            return self.apply_advanced(addr, data);
        }
        if (192..=231).contains(&addr) {
            if data.len() < 2 {
                return self.clone();
            }
            let long = ((addr as u16 & 0x3f) << 8) | data[0] as u16;
            return self.apply_loco(long, &data[1..], false);
        }
        if addr == 0 {
            let mut y = self.clone();
            y.broadcast_count = y.broadcast_count.saturating_add(1);
            let estop = is_estop_instruction(data);
            if estop {
                y.status = TrackStatus::Estop;
            }
            for i in 0..y.locos.len() {
                let a = y.locos[i].addr;
                y = y.apply_loco(a, data, true);
            }
            if !estop && matches!(speed_kind(data), SpeedKind::Stop | SpeedKind::Run) {
                y.status = TrackStatus::Active;
            } else if !estop && y.status != TrackStatus::Estop {
                y.status = TrackStatus::Active;
            }
            return y;
        }
        if addr <= 127 {
            return self.apply_loco(addr as u16, data, false);
        }
        self.clone()
    }

    fn with_active(&self, mut y: Yard) -> Yard {
        if y.status != TrackStatus::Estop {
            y.status = TrackStatus::Active;
        }
        y
    }

    fn apply_loco(&self, addr: u16, data: &[u8], from_broadcast: bool) -> Yard {
        let Some(&b) = data.first() else {
            return self.clone();
        };
        let mut y = self.clone();
        if !from_broadcast && y.status != TrackStatus::Estop {
            y.status = TrackStatus::Active;
        }
        let i = y.locos.iter().position(|l| l.addr == addr).unwrap_or_else(|| {
            y.locos.push(Loco { addr, forward: true, speed: 0, steps: 28, functions: 0, estop: false });
            y.locos.len() - 1
        });
        let mut loco = y.locos[i].clone();
        let ccc = b >> 5;
        match ccc {
            0b001 if b & 0x1f == 0x1f => {
                if let Some(&sp) = data.get(1) {
                    loco.forward = sp & 0x80 != 0;
                    let v = sp & 0x7f;
                    match v {
                        1 => {
                            loco.speed = 0;
                            loco.estop = true;
                            y.status = TrackStatus::Estop;
                        }
                        0 => {
                            loco.speed = 0;
                            loco.estop = false;
                            if !from_broadcast {
                                y.status = TrackStatus::Active;
                            }
                        }
                        v => {
                            loco.speed = v.saturating_sub(1);
                            loco.estop = false;
                            y.status = TrackStatus::Active;
                        }
                    }
                    loco.steps = 128;
                }
            }
            0b010 | 0b011 => {
                loco.forward = ccc == 0b011;
                let d = b & 0x1f;
                let v = ((d & 0x0f) << 1) | ((d >> 4) & 1);
                // S-9.2 speed table, C S3..S0: x0000 stop, x0001 emergency stop, 00010 step 1, 10010 step 2 ...;
                // v = SSSS << 1 | C, so 0 / 1 = stop and 2 / 3 = emergency stop
                match v {
                    2 | 3 => {
                        loco.speed = 0;
                        loco.estop = true;
                        y.status = TrackStatus::Estop;
                    }
                    0 | 1 => {
                        loco.speed = 0;
                        loco.estop = false;
                        if !from_broadcast {
                            y.status = TrackStatus::Active;
                        }
                    }
                    s => {
                        loco.speed = (s - 3).min(28);
                        loco.estop = false;
                        y.status = TrackStatus::Active;
                    }
                }
                loco.steps = 28;
            }
            0b100 => {
                let fl = (b >> 4) & 1;
                let f1 = b & 1;
                let f2 = (b >> 1) & 1;
                let f3 = (b >> 2) & 1;
                let f4 = (b >> 3) & 1;
                loco.functions = (loco.functions & !0x1f)
                    | fl as u32
                    | ((f1 as u32) << 1)
                    | ((f2 as u32) << 2)
                    | ((f3 as u32) << 3)
                    | ((f4 as u32) << 4);
            }
            0b101 => {
                let hi = b & 0x10 != 0;
                let nibble = b & 0x0f;
                if hi {
                    loco.functions = (loco.functions & !(0x0f << 5)) | ((nibble as u32) << 5);
                } else {
                    loco.functions = (loco.functions & !(0x0f << 9)) | ((nibble as u32) << 9);
                }
            }
            0b110 if b == 0b1101_1110 || b == 0b1101_1111 => {
                if let Some(&fb) = data.get(1) {
                    if b & 1 == 0 {
                        loco.functions = (loco.functions & !(0xff << 13)) | ((fb as u32) << 13);
                    } else {
                        loco.functions = (loco.functions & !(0xff << 21)) | ((fb as u32) << 21);
                    }
                }
            }
            _ => {}
        }
        y.locos[i] = loco;
        y.locos.sort_by_key(|l| l.addr);
        y
    }

    fn apply_turnout(&self, addr1: u8, addr2: u8) -> Yard {
        let turnout_addr = accessory_addr(addr1, addr2);
        let thrown = addr2 & 1 == 0;
        let active = addr2 & 0x08 != 0;
        let mut y = self.clone();
        if let Some(t) = y.turnouts.iter_mut().find(|t| t.addr == turnout_addr) {
            if active {
                t.thrown = thrown;
            }
            t.active = active;
        } else {
            y.turnouts.push(Turnout { addr: turnout_addr, thrown, active });
            y.turnouts.sort_by_key(|t| t.addr);
        }
        y
    }

    fn apply_signal(&self, addr1: u8, addr2: u8, aspect: u8) -> Yard {
        // extended accessory: second byte 0AAA0AA1
        if addr2 & 0x89 != 0x01 {
            return self.clone();
        }
        let addr = accessory_addr(addr1, addr2);
        let mut y = self.with_active(self.clone());
        if let Some(s) = y.signals.iter_mut().find(|s| s.addr == addr) {
            s.aspect = aspect;
        } else {
            y.signals.push(Signal { addr, aspect });
            y.signals.sort_by_key(|s| s.addr);
        }
        y
    }

    fn apply_advanced(&self, kind: u8, data: &[u8]) -> Yard {
        let mut y = self.with_active(self.clone());
        if let Some(a) = y.advanced.iter_mut().find(|a| a.kind == kind) {
            a.data = data.to_vec();
        } else {
            y.advanced.push(Advanced { kind, data: data.to_vec() });
            y.advanced.sort_by_key(|a| a.kind);
        }
        y
    }
}

fn accessory_addr(addr1: u8, addr2: u8) -> u16 {
    let board = (addr1 as u16 & 0x3f) | ((!(addr2 as u16) & 0x70) << 2);
    let pair = (addr2 >> 1) & 0x03;
    ((board.saturating_sub(1)) << 2) + pair as u16 + 1
}

#[derive(Clone, Copy)]
enum SpeedKind {
    None,
    Estop,
    Stop,
    Run,
}

fn speed_kind(data: &[u8]) -> SpeedKind {
    let Some(&b) = data.first() else {
        return SpeedKind::None;
    };
    let ccc = b >> 5;
    match ccc {
        0b001 if b & 0x1f == 0x1f => match data.get(1).map(|s| s & 0x7f) {
            Some(1) => SpeedKind::Estop,
            Some(0) => SpeedKind::Stop,
            Some(_) => SpeedKind::Run,
            None => SpeedKind::None,
        },
        0b010 | 0b011 => {
            let d = b & 0x1f;
            let v = ((d & 0x0f) << 1) | ((d >> 4) & 1);
            match v {
                2 | 3 => SpeedKind::Estop,
                0 | 1 => SpeedKind::Stop,
                _ => SpeedKind::Run,
            }
        }
        _ => SpeedKind::None,
    }
}

fn is_estop_instruction(data: &[u8]) -> bool {
    matches!(speed_kind(data), SpeedKind::Estop)
}

impl Loco {
    pub fn dir_text(&self) -> &'static str {
        if self.estop {
            "E-Stop"
        } else if self.forward {
            "Fwd"
        } else {
            "Rev"
        }
    }

    pub fn speed_text(&self) -> String {
        if self.estop {
            "E-Stop".into()
        } else {
            format!("{}/{}", self.speed, self.steps)
        }
    }
}

impl Turnout {
    pub fn side_text(&self) -> &'static str {
        if self.thrown { "Left" } else { "Right" }
    }
}

impl Signal {
    pub fn aspect_text(&self) -> String {
        format!("Aspect {:02X}", self.aspect)
    }
}

/// Common function labels for the monitor (F0 light, F1 bell, F2 horn; others as Fn).
pub fn function_label(n: u8) -> &'static str {
    match n {
        0 => "Light",
        1 => "Bell",
        2 => "Horn",
        3 => "F3",
        4 => "F4",
        _ => "",
    }
}

/// Human-readable list of active functions, e.g. `Light Horn F5`; empty when none is on.
pub fn functions_text(mask: u32) -> String {
    let mut v = Vec::new();
    for n in 0..29u8 {
        if mask & (1 << n) != 0 {
            let l = function_label(n);
            if l.is_empty() {
                v.push(format!("F{n}"));
            } else {
                v.push(l.to_string());
            }
        }
    }
    v.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loco_28_speed_and_functions() {
        let y = Yard::default().apply(3, &[0b011_11111]);
        assert_eq!(y.locos[0].speed, 28);
        assert_eq!(y.locos[0].functions & 1, 0, "28-step C bit is not F0");
        assert_eq!(y.status, TrackStatus::Active);
        let y = y.apply(3, &[0b100_10101]);
        let l = &y.locos[0];
        assert_eq!(l.functions & 0x1f, 0b01011);
        assert!(functions_text(l.functions).contains("Light"));
        assert!(functions_text(l.functions).contains("Bell"));
    }

    #[test]
    fn loco_28_stop_and_first_step() {
        let stop = Yard::default().apply(3, &[0b011_00000]);
        assert_eq!(stop.locos[0].speed, 0);
        assert!(!stop.locos[0].estop);
        let first = Yard::default().apply(3, &[0b011_00010]);
        assert_eq!(first.locos[0].speed, 1);
        // the whole S-9.2 table: C S3..S0 -> stop / emergency stop / step
        for (b, speed, estop) in [(0b0_0000u8, 0u8, false), (0b1_0000, 0, false), (0b0_0001, 0, true), (0b1_0001, 0, true), (0b1_0010, 2, false), (0b0_0011, 3, false), (0b1_1111, 28, false)] {
            let l = Yard::default().apply(3, &[0b011_00000 | b]).locos[0].clone();
            assert_eq!((l.speed, l.estop), (speed, estop), "{:05b}", b);
        }
    }

    #[test]
    fn broadcast_estop_sets_red_status() {
        // 01DCSSSS with SSSS=1 (C either) = emergency stop
        let y = Yard::default().apply(3, &[0b011_10110]).apply(0, &[0b011_00001]);
        assert_eq!(y.status, TrackStatus::Estop);
        assert!(y.locos[0].estop);
        assert!(y.broadcast_count >= 1);
        let y = y.apply(0, &[0b011_00000]); // broadcast stop clears
        assert_eq!(y.status, TrackStatus::Active);
        assert!(!y.locos[0].estop);
    }

    #[test]
    fn idle_marks_track_active() {
        let y = Yard::default().apply(255, &[0]);
        assert_eq!(y.status, TrackStatus::Active);
        assert_eq!(y.idle_count, 1);
    }

    #[test]
    fn broadcast_does_not_create_loco_zero() {
        let y = Yard::default().apply(0, &[0b011_00000]);
        assert!(y.locos.is_empty());
        let y = Yard::default().apply(3, &[0b011_10110]).apply(0, &[0b010_00000]);
        assert_eq!(y.locos.len(), 1);
        assert_eq!(y.locos[0].addr, 3);
        assert!(!y.locos[0].forward);
    }

    #[test]
    fn loco_128_speed() {
        let y = Yard::default().apply(5, &[0b001_11111, 0b1000_0100]);
        let l = &y.locos[0];
        assert!(l.forward);
        assert_eq!(l.steps, 128);
        assert_eq!(l.speed, 3);
    }

    #[test]
    fn turnout_left_and_right() {
        let y = Yard::default().apply(0b1000_0001, &[0b1111_1000]);
        assert_eq!(y.turnouts.len(), 1);
        assert!(y.turnouts[0].thrown);
        assert!(y.turnouts[0].active);
        let y = y.apply(0b1000_0001, &[0b1111_1001]);
        assert!(!y.turnouts[0].thrown);
    }

    #[test]
    fn signal_aspect() {
        // extended accessory address 1: addr1=0x81, addr2=0x71 (0AAA0AA1), aspect 0x24
        let y = Yard::default().apply(0b1000_0001, &[0b0111_0001, 0x24]);
        assert_eq!(y.signals.len(), 1);
        assert_eq!(y.signals[0].addr, 1);
        assert_eq!(y.signals[0].aspect, 0x24);
        assert_eq!(y.status, TrackStatus::Active);
    }

    #[test]
    fn advanced_partition() {
        let y = Yard::default().apply(253, &[0x11, 0x22]);
        assert_eq!(y.advanced.len(), 1);
        assert_eq!(y.advanced[0].kind, 253);
        assert_eq!(y.advanced[0].data, vec![0x11, 0x22]);
    }
}
