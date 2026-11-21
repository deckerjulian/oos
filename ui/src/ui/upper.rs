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

/// The state of the probe: the request sent and not answered yet, the number of the last one, and whether the switch
/// was set to Off while it ran (Off wins over the probe's yes).
#[derive(Default)]
pub(super) struct State {
    pub asked: Option<u64>,
    pub n: u64,
    pub off: bool,
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
        (it == Item::UpperUse).then(|| if self.upper_shown_on() { "On" } else { "Off" }.to_string())
    }

    pub(super) fn upper_options(&self, it: Item) -> Option<Vec<String>> {
        (it == Item::UpperUse).then(|| vec!["Off".into(), "On".into()])
    }

    /// The optional memory switched on: `mem.upper = on` after a probe's yes on this unit; `on` alone (an image's
    /// default, a line by hand) is off until a probe said yes.
    fn upper_on(&self) -> bool {
        self.sys.mem_upper && self.sys.mem_upper_found == Some(true)
    }

    /// What the switch shows: on, or on once the probe that runs says yes (unless Off was chosen meanwhile).
    fn upper_shown_on(&self) -> bool {
        self.upper_on() || (self.upper.asked.is_some() && !self.upper.off)
    }

    /// The probe button's text: what it does, or that the probe runs.
    pub(super) fn upper_button(&self) -> &'static str {
        if self.upper.asked.is_some() {
            "Probing memory ..."
        } else {
            "Probe optional memory"
        }
    }

    /// The switch: off at once (also over a probe that runs: its yes then leaves it off); on at once where a probe on
    /// this unit found the memory, else the probe first (its yes switches it on).
    pub(super) fn upper_adjust(&mut self, it: Item, _d: i32) -> bool {
        if it != Item::UpperUse {
            return false;
        }
        if self.upper_shown_on() {
            self.upper.off = self.upper.asked.is_some();
            self.upper_set(false);
            self.toast("Optional memory: off, the lower 256 MiB only");
        } else if self.sys.mem_upper_found == Some(true) {
            self.upper.off = false;
            self.upper_set(true);
            self.toast("Optional memory: on");
        } else {
            self.upper.off = false;
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
        // the probe writes frames: segments recorded in the hardware and still set up would be lost (switched off,
        // they go at the next configuration anyway, also in STOP)
        let kept = match &self.src {
            Source::Hw { shared, .. } => {
                self.seg_hw_on() && crate::wave::hw(shared).seg.phase != crate::board::SegPhase::Idle
            }
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
            Err(e) => {
                self.upper.off = false;
                format!("Optional memory: probe stopped: {}", e)
            }
            Ok(sizes) if sizes.is_empty() => "Optional memory: no segment size to probe".to_string(),
            Ok(sizes) => {
                let no: Vec<String> = sizes.iter().filter(|e| !e.1).map(|e| e.0.to_string()).collect();
                let yes = no.is_empty();
                self.sys.mem_upper_found = Some(yes);
                // Off chosen while the probe ran wins over its yes
                let on = yes && !std::mem::take(&mut self.upper.off);
                self.upper_set(on);
                if yes && !on {
                    format!("Optional memory: present, off ({} segment sizes probed)", sizes.len())
                } else if yes {
                    format!("Optional memory: on ({} segment sizes probed)", sizes.len())
                } else {
                    format!("Optional memory: not present, off (segments of {} bursts)", no.join(", "))
                }
            }
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

    // Break it catches: a probe on another source, one that writes over hardware segments still set up and kept, or
    // one refused after the hardware segments were switched off in STOP (the thread forgets them only at the next
    // configuration).
    #[test]
    fn no_probe_without_the_hardware_or_over_kept_segments() {
        let mut u = crate::ui::testkit::demo_ui();
        u.activate(Item::UpperProbe);
        u.select_option(Item::UpperUse, "On");
        assert_eq!((u.upper.asked, u.sys.mem_upper), (None, false));
        let (mut u, shared) = hw_ui();
        (u.m.seg_hw, u.m.segments) = (true, 10);
        u.m.tscale = tscales().iter().position(|&x| (x as f64 - 1e-6).abs() < 1e-9).unwrap();
        assert!(u.seg_hw_on());
        crate::wave::hw(&shared).seg.phase = crate::board::SegPhase::Done;
        u.activate(Item::UpperProbe);
        assert!(u.upper.asked.is_none() && crate::wave::hw(&shared).upper_want.is_none());
        // switched off (`:ACQ:SEGM:HARD OFF` in STOP): the phase is still Done, the probe goes ahead
        u.m.run = RunState::Stop;
        u.m.seg_hw = false;
        u.activate(Item::UpperProbe);
        assert!(u.upper.asked.is_some() && crate::wave::hw(&shared).upper_want.is_some());
    }

    // Break it catches: `mem.upper = on` without a probe's yes (an image's default) taken as on, or System info that
    // says "in use" before a yes; On then not probing first.
    #[test]
    fn on_without_a_yes_is_off_until_the_probe() {
        let (mut u, shared) = hw_ui();
        (u.sys.mem_upper, u.sys.mem_upper_found) = (true, None);
        board().set_upper_mem(true, None);
        assert_eq!(u.value(Item::UpperUse), "Off");
        assert_eq!(board().upper_mem(), crate::board::UpperMem::NotProbed);
        u.select_option(Item::UpperUse, "On");
        assert!(u.upper.asked.is_some(), "On probes first");
        answer(&mut u, &shared, Ok(vec![(256, true)]));
        assert_eq!((u.value(Item::UpperUse), board().upper_mem()), ("On".into(), crate::board::UpperMem::InUse));
        board().set_upper_mem(false, None);
    }

    // Break it catches: a probe's yes that switches the memory on after the user chose Off while it ran.
    #[test]
    fn off_while_the_probe_runs_wins() {
        let (mut u, shared) = hw_ui();
        u.activate(Item::UpperProbe);
        u.select_option(Item::UpperUse, "Off");
        answer(&mut u, &shared, Ok(vec![(256, true), (2048, true)]));
        assert_eq!((u.sys.mem_upper, u.sys.mem_upper_found), (false, Some(true)));
        assert_eq!((u.value(Item::UpperUse), board().upper_mem()), ("Off".into(), crate::board::UpperMem::Off));
        assert!(u.toast.as_ref().is_some_and(|t| t.0.contains("present, off")), "{:?}", u.toast);
        // the next probe's yes switches it on again (Off was for that run only)
        u.activate(Item::UpperProbe);
        answer(&mut u, &shared, Ok(vec![(256, true)]));
        assert!(u.sys.mem_upper);
        // On during a probe that Off was chosen in: the yes switches it on
        u.select_option(Item::UpperUse, "Off");
        u.activate(Item::UpperProbe);
        u.select_option(Item::UpperUse, "Off");
        u.select_option(Item::UpperUse, "On");
        answer(&mut u, &shared, Ok(vec![(256, true)]));
        assert!(u.sys.mem_upper);
        board().set_upper_mem(false, None);
    }
}
