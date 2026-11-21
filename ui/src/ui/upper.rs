// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! System > Opt. memory and Probe optional memory: the frame memory above 256 MiB for hardware segments
//! (docs/protocols/horizontal-config.md 1.8). The probe: the acquisition thread probes it for every segment size the
//! Acquire menu's memory depths give (`Board::upper_plan`); every size held switches it on, one that did not switches
//! it off. The switch: the user turns it off and on again; on without a yes of a probe on this unit probes first. Both
//! are settings of the scope in `/oos/system.conf` (`mem.upper`, `mem.upper.found`), off by default (ADR 0010). System
//! info shows them ("Optional memory").

use super::*;
use crate::board::{board, Board};

/// The state of the probe: the request sent and not answered yet, and the number of the last one.
#[derive(Default)]
pub(super) struct State {
    pub asked: Option<u64>,
    pub n: u64,
}

impl Ui {
    pub(super) fn upper_kind(it: Item) -> Option<Kind> {
        match it {
            Item::UpperProbe => Some(Kind::Action),
            Item::UpperUse => Some(Kind::Choice),
            _ => None,
        }
    }

    pub(super) fn upper_label(it: Item) -> Option<&'static str> {
        match it {
            Item::UpperProbe => Some("Probe optional memory"),
            Item::UpperUse => Some("Opt. memory"),
            _ => None,
        }
    }

    pub(super) fn upper_value(&self, it: Item) -> Option<String> {
        (it == Item::UpperUse).then(|| if self.sys.mem_upper { "On" } else { "Off" }.to_string())
    }

    pub(super) fn upper_options(&self, it: Item) -> Option<Vec<String>> {
        (it == Item::UpperUse).then(|| vec!["Off".into(), "On".into()])
    }

    /// The probe button's text: what it does, or that the probe runs.
    pub(super) fn upper_button(&self) -> &'static str {
        if self.upper.asked.is_some() {
            "Probing memory ..."
        } else {
            "Probe optional memory"
        }
    }

    /// The switch: off at once; on at once where a probe on this unit found the memory, else the probe first (its yes
    /// switches it on).
    pub(super) fn upper_adjust(&mut self, it: Item, _d: i32) -> bool {
        if it != Item::UpperUse {
            return false;
        }
        if self.sys.mem_upper {
            self.upper_set(false);
            self.toast("Optional memory: off, the lower 256 MiB only");
        } else if self.sys.mem_upper_found == Some(true) {
            self.upper_set(true);
            self.toast("Optional memory: on");
        } else {
            self.upper_probe();
        }
        true
    }

    /// The switch set to `on`, kept and given to the board.
    fn upper_set(&mut self, on: bool) {
        self.sys.mem_upper = on;
        board().set_upper_mem(on, self.sys.mem_upper_found);
        self.sys_save();
    }

    pub(super) fn upper_activate(&mut self, it: Item) -> bool {
        if it != Item::UpperProbe {
            return false;
        }
        self.upper_probe();
        true
    }

    /// Asks the thread for the probe, unless one runs, the source is not the hardware or hardware segments are kept.
    fn upper_probe(&mut self) {
        if self.upper.asked.is_some() {
            self.toast("The optional memory is being probed");
            return;
        }
        // the probe writes frames: segments recorded in the hardware would be lost
        let kept = match &self.src {
            Source::Hw { shared, .. } => crate::wave::hw(shared).seg.phase != crate::board::SegPhase::Idle,
            _ => false,
        };
        if kept {
            self.toast("Probe: switch the hardware segments off first");
            return;
        }
        let n = self.upper.n + 1;
        if self.src.hw_upper(Some((n, board().upper_plan(&self.m)))).is_none() {
            self.toast("The probe needs the hardware source");
            return;
        }
        (self.upper.asked, self.upper.n) = (Some(n), n);
        self.toast("Probing the optional memory ...");
    }

    /// Per frame: the probe's answer, kept as `mem.upper.found` and the switch, and told in a toast.
    pub(super) fn upper_poll(&mut self) {
        let Some(n) = self.upper.asked else { return };
        let Some(Some((k, got))) = self.src.hw_upper(None) else { return };
        if k != n {
            return;
        }
        self.upper.asked = None;
        let text = match got {
            Ok(sizes) if sizes.is_empty() => "Optional memory: no segment size to probe".to_string(),
            Ok(sizes) => {
                let no: Vec<String> = sizes.iter().filter(|e| !e.1).map(|e| e.0.to_string()).collect();
                let yes = no.is_empty();
                self.sys.mem_upper_found = Some(yes);
                self.upper_set(yes);
                if yes {
                    format!("Optional memory: on ({} segment sizes probed)", sizes.len())
                } else {
                    format!("Optional memory: not present, off (segments of {} bursts)", no.join(", "))
                }
            }
            Err(e) => format!("Optional memory: probe stopped: {}", e),
        };
        self.toast(&text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A screen on a hardware source whose thread is the test: it answers the probe's request itself.
    fn hw_ui() -> (Ui, crate::board::HwShared) {
        let shared: crate::board::HwShared = Default::default();
        let mut u = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        u.sys_path = None;
        (u, shared)
    }

    /// The request the screen sent to the thread, answered with `sizes` (or why it stopped) and taken by the poll.
    fn answer(u: &mut Ui, shared: &crate::board::HwShared, got: Result<Vec<(u32, bool)>, String>) {
        let (n, plan) = crate::wave::hw(shared).upper_want.take().expect("a request to the thread");
        assert!(!plan.is_empty() && plan.iter().all(|s| s.segments.is_some()), "{} set-ups", plan.len());
        assert_eq!(u.item_label(Item::UpperProbe), "Probing memory ...");
        crate::wave::hw(shared).upper_probed = Some((n, got));
        u.upper_poll();
        assert_eq!(u.upper.asked, None);
    }

    // Break it catches: a System menu without the button or the switch, the memory on by default, a press that asks
    // the thread for nothing or for no segment size, a yes that is not kept or does not switch it on, a no that is not
    // kept as off, an answer to an older request taken for the new one.
    #[test]
    fn the_button_probes_and_keeps_the_answer() {
        let (mut u, shared) = hw_ui();
        let items = u.items(MenuId::Utility);
        assert!(items.contains(&Item::UpperProbe) && items.contains(&Item::UpperUse));
        assert!(Ui::kind(Item::UpperProbe) == Kind::Action);
        assert_eq!(u.item_label(Item::UpperProbe), "Probe optional memory");
        assert_eq!((u.sys.mem_upper, u.sys.mem_upper_found, u.value(Item::UpperUse)), (false, None, "Off".into()));
        assert_eq!(board().upper_mem(), crate::board::UpperMem::NotProbed);
        u.activate(Item::UpperProbe);
        let n = u.upper.asked.expect("asked");
        // an answer to another request is not this one's
        crate::wave::hw(&shared).upper_probed = Some((n + 7, Ok(vec![(256, false)])));
        u.upper_poll();
        assert_eq!((u.upper.asked, u.sys.mem_upper_found), (Some(n), None));
        answer(&mut u, &shared, Ok(vec![(256, true), (2048, true)]));
        assert_eq!((u.sys.mem_upper, u.sys.mem_upper_found), (true, Some(true)));
        assert!(u.sys.text().contains("\nmem.upper = on\nmem.upper.found = yes\n"));
        assert_eq!((board().upper_mem(), u.value(Item::UpperUse)), (crate::board::UpperMem::InUse, "On".into()));
        assert!(u.toast.as_ref().is_some_and(|t| t.0.contains("on (2 segment")), "{:?}", u.toast);
        // a no for one size: off, and not present
        u.activate(Item::UpperProbe);
        answer(&mut u, &shared, Ok(vec![(256, true), (2048, false)]));
        assert_eq!((u.sys.mem_upper, u.sys.mem_upper_found), (false, Some(false)));
        assert!(u.sys.text().contains("\nmem.upper = off\nmem.upper.found = no\n"));
        assert_eq!(board().upper_mem(), crate::board::UpperMem::Missing);
        assert!(u.toast.as_ref().is_some_and(|t| t.0.contains("2048 bursts")), "{:?}", u.toast);
        // a probe that stopped changes nothing
        u.activate(Item::UpperProbe);
        answer(&mut u, &shared, Err("configure: no".into()));
        assert_eq!((u.sys.mem_upper, u.sys.mem_upper_found), (false, Some(false)));
        board().set_upper_mem(false, None);
    }

    // Break it catches: a switch that cannot turn the memory off again, one that turns it on without a probe's yes on
    // this unit, or that probes again where a probe found it.
    #[test]
    fn the_switch_turns_it_off_and_on_again() {
        let (mut u, shared) = hw_ui();
        // never probed: On probes first, and its yes switches it on
        u.select_option(Item::UpperUse, "On");
        assert!(!u.sys.mem_upper && u.upper.asked.is_some(), "on only after the probe");
        answer(&mut u, &shared, Ok(vec![(256, true)]));
        assert_eq!(u.value(Item::UpperUse), "On");
        u.select_option(Item::UpperUse, "Off");
        assert_eq!((u.sys.mem_upper, u.sys.mem_upper_found), (false, Some(true)));
        assert!(u.sys.text().contains("\nmem.upper = off\nmem.upper.found = yes\n"));
        assert_eq!(board().upper_mem(), crate::board::UpperMem::Off, "present, switched off");
        // found before: on again at once, without a probe
        u.select_option(Item::UpperUse, "On");
        assert!(u.sys.mem_upper && u.upper.asked.is_none() && crate::wave::hw(&shared).upper_want.is_none());
        assert_eq!(board().upper_mem(), crate::board::UpperMem::InUse);
        // found missing: On probes again, and a no keeps it off
        u.select_option(Item::UpperUse, "Off");
        u.sys.mem_upper_found = Some(false);
        u.select_option(Item::UpperUse, "On");
        answer(&mut u, &shared, Ok(vec![(256, false)]));
        assert_eq!((u.value(Item::UpperUse), u.sys.mem_upper_found), ("Off".into(), Some(false)));
        board().set_upper_mem(false, None);
    }

    // Break it catches: a probe on another source, or one that writes over hardware segments still kept.
    #[test]
    fn no_probe_without_the_hardware_or_over_kept_segments() {
        let mut u = crate::ui::testkit::demo_ui();
        u.activate(Item::UpperProbe);
        u.select_option(Item::UpperUse, "On");
        assert_eq!((u.upper.asked, u.sys.mem_upper), (None, false));
        let (mut u, shared) = hw_ui();
        crate::wave::hw(&shared).seg.phase = crate::board::SegPhase::Done;
        u.activate(Item::UpperProbe);
        assert!(u.upper.asked.is_none() && crate::wave::hw(&shared).upper_want.is_none());
    }
}
