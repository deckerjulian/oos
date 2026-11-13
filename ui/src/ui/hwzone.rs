// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! The zone trigger in the FPGA (`zone.hw = on` in system.conf, docs/fork/hwzone.md): the zones go to the zone tables
//! of the mask unit, which qualifies the trigger, so only qualifying frames arrive and the fast display path stays on.
//! The software zone filter stays the fallback and the reference: it still runs on every full record that reaches the
//! UI, and alone where the hardware cannot qualify.

use super::*;
use crate::board::ZoneWant;

impl Ui {
    /// The zone trigger runs in the hardware: enabled on this board, live hardware frames, at least one usable zone,
    /// all of them on one acquired channel, and the mask unit not taken by the hardware mask test.
    pub(super) fn zone_hw_active(&self) -> bool {
        board().zone_hw() && self.src.is_hw() && self.zone_hw_src().is_some_and(|c| self.m.ch[c].on) && !self.mask_hw_active() && self.unit_blocked.is_none()
    }

    /// The one source channel of the usable zones; None without usable zones or with zones on different channels.
    fn zone_hw_src(&self) -> Option<usize> {
        let mut src = self.m.zones.iter().filter(|z| z.usable()).map(|z| z.src);
        let first = src.next()?;
        src.all(|c| c == first).then_some(first)
    }

    /// Hands the zones to the acquisition thread and takes its latest reading.
    pub(super) fn zone_hw_poll(&mut self) {
        let want = self.zone_hw_active().then(|| ZoneWant {
            ch: self.zone_hw_src().unwrap_or(0),
            zones: self.m.zones.iter().filter(|z| z.usable()).map(|z| (z.polygon(), z.inside)).collect(),
            clear: self.zone_hw_clear,
        });
        if let Some(r) = self.src.hw_zone(want) {
            self.zone_hw = r;
        }
    }

    /// Fork, `:SYSTem:DEBug:ZONE?`: the zone unit's raw status and counter next to the software filter and the frame
    /// counters of the pipeline (docs/fork/hwzone.md).
    pub fn zone_debug_text(&self) -> String {
        let (acq, used, _) = crate::drawpipe::counters().read();
        let sw = format!("sw_drops={},acquired={},used={},records={}", self.zone_drops, acq, used, self.records);
        let Some(r) = self.zone_hw else {
            return format!("off,enabled={},active={},{}", board().zone_hw() as u8, self.zone_hw_active() as u8, sw);
        };
        format!(
            "ch={},slot={},ctrl={:#010x},s4030={:#010x},s4038={:#010x},frames_lo={:#010x},frames_hi={:#010x},fails_lo={:#010x},fails_hi={:#010x},armed={},pass={},cycle={},state={},frames={},n={},batch={}/{},batch_ms={:.1},{}",
            r.ch + 1,
            r.slot,
            r.ctrl,
            r.raw[0],
            r.raw[1],
            r.raw[2],
            r.raw[3],
            r.raw[4],
            r.raw[5],
            r.armed as u8,
            r.pass as u8,
            r.in_cycle as u8,
            r.state,
            r.frames,
            r.n,
            r.batch.0,
            r.batch.1,
            r.batch.2,
            sw
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zone::Zone;

    #[test]
    fn one_source_for_the_hardware() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let z = |src| Zone { on: true, src, inside: true, pts: vec![(0.0, 0.0), (1e-6, 1.0)] };
        assert_eq!(ui.zone_hw_src(), None);
        ui.m.zones[0] = z(1);
        assert_eq!(ui.zone_hw_src(), Some(1));
        ui.m.zones[1] = z(1);
        assert_eq!(ui.zone_hw_src(), Some(1));
        ui.m.zones[1] = z(2);
        assert_eq!(ui.zone_hw_src(), None);
        ui.m.zones[1].on = false;
        assert_eq!(ui.zone_hw_src(), Some(1));
        // the demo source is no hardware
        assert!(!ui.zone_hw_active());
        assert!(ui.zone_debug_text().starts_with("off,enabled=0,active=0,sw_drops=0,"));
        ui.zone_hw = Some(crate::board::ZoneRead { ch: 1, slot: 1, armed: true, pass: true, frames: 5, n: 2, ..Default::default() });
        assert!(ui.zone_debug_text().starts_with("ch=2,slot=1,ctrl=0x00000000,"), "{}", ui.zone_debug_text());
        assert!(ui.zone_debug_text().contains(",armed=1,pass=1,cycle=0,state=0,frames=5,n=2,batch=0/0,batch_ms=0.0,sw_drops=0,"));
    }
}
