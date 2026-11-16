// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Display > Monitor view: with HDMI Layout the monitor shows one of the views (Display > Views) instead of the panel's,
//! for a second screen with its own arrangement next to the scope (asked for in the forum): the panel keeps the
//! controls in one view, the monitor shows another one, e.g. the spectrum, the eye or four windows with a large
//! measurement table. The monitor view is set up on the panel like any view: switch the panel to it, arrange it, switch
//! back. While the panel shows the same view, the monitor shows it as it is.
//!
//! What the monitor draws is the view's arrangement (windows, measurements, cursors, DVM and counter, histogram,
//! second pane, analyser page, XY, full screen) on the channels, time base and trigger every view shares. Its analyser
//! page is worked out beside the panel's. The trace maps are the panel's: a zoom of the monitor view is drawn only
//! while the panel shows the same zoom, and the measurement statistics only while both measure the same rows.
//! Setup: `hdmi_view` (0: the panel's view). SCPI: `:DISPlay:VIEW:MONitor <n>` (0: the panel's).

use super::*;

impl Ui {
    /// The view the monitor shows in place of the panel's, as a model and the Ui's XY and full screen; None: the
    /// panel's (not HDMI Layout, no other view chosen, or the panel shows that view).
    pub(super) fn monitor_view(&self) -> Option<(Model, bool, bool)> {
        let j = self.m.hdmi_view.checked_sub(1)?;
        if self.m.hdmi != HdmiMode::Layout || self.m.views < 2 || j >= self.m.views || j == self.m.view_cur {
            return None;
        }
        let mut m = self.m.clone();
        let (mut xy, mut full) = (false, false);
        let empty = views::plot_alone();
        let slot = if self.m.view_slots[j].trim().is_empty() { &empty } else { &self.m.view_slots[j] };
        for part in slot.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let Some((k, v)) = part.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "xy" => xy = v == "true",
                "full" => full = v == "true",
                _ if views::is_view_key(k) => {
                    let _ = crate::store::set(&mut m, k, v);
                }
                _ => {}
            }
        }
        if m.zscale > m.tscale {
            m.zscale = m.tscale;
        }
        // the zoom maps are drawn for the panel's zoom only
        if m.zoom && !(self.m.zoom && m.zscale == self.m.zscale && m.zdelay == self.m.zdelay) {
            m.zoom = false;
        }
        Some((m, xy, full))
    }

    /// Runs `f` with the monitor's view in place of the panel's, then puts the panel's back; None when the monitor
    /// shows the panel's view.
    pub(super) fn with_monitor_view<R>(&mut self, f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let (m, xy, full) = self.monitor_view()?;
        let stats = self.stats_on && m.meas == self.m.meas && m.meas2 == self.m.meas2;
        let j = self.m.hdmi_view - 1;
        let panel = std::mem::replace(&mut self.m, m);
        // the monitor's top bar marks its own view; the panel's notes stay on the panel
        self.m.view_cur = j;
        let toast = self.toast.take();
        let keep = (self.xy, self.full, self.stats_on);
        (self.xy, self.full, self.stats_on) = (xy, full, stats);
        let r = f(self);
        self.m = panel;
        (self.xy, self.full, self.stats_on) = keep;
        self.toast = toast;
        Some(r)
    }

    /// The monitor view's analyser page or second pane, where it is not the panel's: worked out beside it.
    pub(super) fn monitor_analyzer_update(&mut self) {
        let Some((m, _, _)) = self.monitor_view() else { return };
        if m.view == self.m.view && m.pane == self.m.pane {
            return;
        }
        self.with_monitor_view(|u| u.analyzer_update());
    }

    // ------------------------------------------------------------------------------------------------ the menu item

    pub(super) fn monitor_label(it: Item) -> Option<&'static str> {
        (it == Item::HdmiView).then_some("Monitor view")
    }

    pub(super) fn monitor_options(&self, it: Item) -> Option<Vec<String>> {
        (it == Item::HdmiView).then(|| std::iter::once("Panel's".to_string()).chain((1..=self.m.views.max(1)).map(|n| format!("View {}", n))).collect())
    }

    pub(super) fn monitor_value(&self, it: Item) -> Option<String> {
        (it == Item::HdmiView).then(|| match self.m.hdmi_view {
            0 => "Panel's".to_string(),
            n => format!("View {}", n),
        })
    }

    pub(super) fn monitor_adjust(&mut self, it: Item, d: i32) -> bool {
        if it != Item::HdmiView {
            return false;
        }
        let n = self.m.views.max(1) as i32 + 1;
        self.m.hdmi_view = (self.m.hdmi_view as i32 + d).rem_euclid(n) as usize;
        if self.m.hdmi_view > 0 && self.m.hdmi != HdmiMode::Layout {
            self.toast("Monitor view: with Display > HDMI = Layout");
        } else if self.m.hdmi_view > 0 && self.m.views < 2 {
            self.toast("Monitor view: set Display > Views to 2 or more");
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui() -> Ui {
        let mut u = crate::ui::testkit::demo_ui();
        u.m.hdmi = HdmiMode::Layout;
        u.views_set(2);
        u
    }

    // Break it catches: the monitor drawing the panel's view, or the panel's model changed by the monitor's frame
    #[test]
    fn the_monitor_shows_its_own_view_and_the_panel_keeps_its_own() {
        let mut u = ui();
        // view 2: the spectrum and Freq only
        u.view_go(1);
        u.m.view = AView::Spectrum;
        u.m.meas = vec![(Meas::Freq, WaveId::Ch(0))];
        u.view_go(0);
        u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0))];
        assert!(u.monitor_view().is_none(), "Panel's by default");
        u.m.hdmi_view = 2;
        let (m, _, _) = u.monitor_view().expect("view 2 on the monitor");
        assert_eq!((m.view, m.meas.clone()), (AView::Spectrum, vec![(Meas::Freq, WaveId::Ch(0))]));
        // the panel's model stays as it was after a monitor frame
        let before = crate::store::setup_text(&u.m);
        let mut cv = Canvas::new(1280, 720);
        u.render_view(&mut cv);
        assert_eq!(crate::store::setup_text(&u.m), before);
        assert_eq!(u.m.view, AView::Scope);
        // the panel showing view 2 too: the monitor shows it as it is
        u.view_go(1);
        assert!(u.monitor_view().is_none());
        // only with HDMI Layout
        u.view_go(0);
        u.m.hdmi = HdmiMode::Mirror;
        assert!(u.monitor_view().is_none());
    }

    #[test]
    fn the_monitor_view_goes_with_a_setup_and_by_scpi() {
        let mut u = ui();
        let cv = Canvas::new(crate::ui::PANEL_W, crate::ui::PANEL_H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let mut q = |u: &mut Ui, c: &str| crate::ui::testkit::q(&mut scpi, u, &cv, c);
        assert_eq!(q(&mut u, ":DISP:VIEW:MON?"), "0");
        q(&mut u, ":DISPlay:VIEW:MONitor 2");
        assert_eq!(u.m.hdmi_view, 2);
        q(&mut u, ":DISP:VIEW:MON 3");
        assert!(q(&mut u, ":SYST:ERR?").starts_with("-224"), "only views there are");
        let back = crate::store::parse_setup(&crate::store::setup_text(&u.m)).unwrap();
        assert_eq!(back.hdmi_view, 2);
        q(&mut u, ":DISP:VIEW:MON 0");
        assert_eq!(u.m.hdmi_view, 0);
    }
}
