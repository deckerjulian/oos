// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Analyser pages: the views that replace the waveform (spectrum, eye, power, Bode, ...), behind one registration.
//!
//! An analyser page implements [`Analyser`] next to its own code (`bode.rs`, `magnetic.rs` ...) and has one line in
//! [`ANALYSERS`]. The scope asks the registry for everything around a page: the start menu entries (Analysers), the
//! view bar's name and its settings menu, the overflow menu, System > Features (leaving it out), SCPI `:VIEW`, the
//! `view` key of setups, which record it is computed from and when, and drawing it into the plot. The page itself
//! keeps its settings in the model, its results in `Ui::azr`, and its menu rows as `Item`s of its menu.
//!
//! `docs/analyser-modules.md` walks through adding one; the tests below check every registered page the same way.

use super::*;

/// An analyser page. `view`, `feature`, `name`, `menu` and `draw` are required.
pub(super) trait Analyser: Sync {
    /// The view it fills the plot with.
    fn view(&self) -> AView;

    /// Its id in System > Features (`features.off` in system.conf; never changed once released) and its name there.
    fn feature(&self) -> (&'static str, &'static str);

    /// Its name on the view bar.
    fn name(&self) -> &'static str;

    /// Its settings menu: the view bar's Settings, and the start menu entry.
    fn menu(&self) -> MenuId;

    /// The keyword of SCPI `:VIEW` (SCPI long form: the short part in capitals); `None` = not over SCPI.
    fn scpi(&self) -> Option<&'static str> {
        None
    }

    /// Whether setups keep it as the view (`view = <AView>`). A page that runs a procedure says no.
    fn stored(&self) -> bool {
        true
    }

    /// Its entries in the start menu (Analysers group).
    fn start(&self) -> Vec<Item> {
        vec![Item::Open(self.menu())]
    }

    /// What the overflow menu of the view offers before Settings and Exit.
    fn more(&self, _ui: &Ui) -> Vec<Item> {
        Vec::new()
    }

    /// Whether it computes without a record of the acquisition (a loaded reference, a capture of its own).
    fn without_record(&self, _ui: &Ui) -> bool {
        false
    }

    /// Computes its results from a new record (every captured sample of the window). `pane`: it runs in the pane
    /// beside the waveform, not as the page.
    fn update(&self, _ui: &mut Ui, _rec: &Record, _pane: bool) {}

    /// Draws the page into the plot.
    fn draw(&self, ui: &Ui, cv: &mut Canvas);
}

/// The registered pages, in the order of the start menu and of System > Features.
pub(super) static ANALYSERS: &[&dyn Analyser] = &[
    &analysis::SpectrumPage,
    &eyepage::EyePage,
    &analysis::PowerPage,
    &bode::BodePage,
    &bwtest::BandwidthPage,
    &magnetic::MagneticPage,
    &motor::InverterPage,
    &dpt::DoublePulsePage,
];

/// The page of view `v` (none for the waveform).
pub(super) fn of(v: AView) -> Option<&'static dyn Analyser> {
    ANALYSERS.iter().copied().find(|a| a.view() == v)
}

/// The views SCPI `:VIEW` sets, with their keywords: the waveform and every page that has one.
pub(crate) fn scpi_views() -> Vec<(&'static str, AView)> {
    std::iter::once(("SCOPe", AView::Scope))
        .chain(ANALYSERS.iter().filter_map(|a| a.scpi().map(|k| (k, a.view()))))
        .collect()
}

/// The views a setup may keep.
pub(crate) fn stored_views() -> Vec<AView> {
    std::iter::once(AView::Scope).chain(ANALYSERS.iter().filter(|a| a.stored()).map(|a| a.view())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Every view but the waveform has one page; views, feature ids, names, menus and SCPI keywords are unique.
    #[test]
    fn analysers_are_well_formed() {
        let (mut views, mut ids, mut fnames, mut names, mut menus, mut scpi) =
            (HashSet::new(), HashSet::new(), HashSet::new(), HashSet::new(), HashSet::new(), HashSet::new());
        for a in ANALYSERS {
            assert!(views.insert(format!("{:?}", a.view())), "{:?} twice", a.view());
            let (id, fname) = a.feature();
            assert!(
                !id.is_empty() && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "feature id {id:?}"
            );
            assert!(ids.insert(id) && fnames.insert(fname), "feature {id} / {fname} twice");
            assert!(names.insert(a.name()), "name {} twice", a.name());
            assert!(menus.insert(format!("{:?}", a.menu())), "menu {:?} twice", a.menu());
            if let Some(k) = a.scpi() {
                assert!(scpi.insert(k.to_ascii_uppercase()) && k.chars().all(|c| c.is_ascii_alphabetic()), "SCPI {k}");
            }
            assert!(a.start().contains(&Item::Open(a.menu())), "{}: start menu", a.name());
        }
        for v in AView::ALL {
            assert_eq!(of(v).is_none(), v == AView::Scope, "{v:?}");
        }
    }

    /// Every page on the demo signals: it computes and draws without panicking, the view bar names it, its overflow
    /// menu leads to its settings and out again; System > Features leaves it out everywhere.
    #[test]
    fn every_page_runs_and_is_reached_the_same_way() {
        for a in ANALYSERS {
            let mut ui = crate::ui::testkit::demo_ui();
            ui.m.ch[1].on = true;
            ui.set_analysis_view(a.view());
            let mut cv = Canvas::new(PANEL_W, PANEL_H);
            for _ in 0..5 {
                ui.step(1.0 / 30.0);
                ui.render(&mut cv);
            }
            assert_eq!(ui.m.view, a.view(), "{}", a.name());
            assert_eq!(ui.view_name(), a.name());
            let more = ui.view_more_items();
            assert!(more.ends_with(&[Item::ViewSettings, Item::ViewExit]), "{}: {more:?}", a.name());
            ui.activate(Item::ViewSettings);
            assert!(ui.menu_open && ui.menu == a.menu() && !ui.items(a.menu()).is_empty(), "{}", a.name());
            ui.close_menu();
            ui.activate(Item::ViewExit);
            assert_eq!(ui.m.view, AView::Scope, "{}", a.name());
            // left out: not in the start menu
            ui.sys.features_off = vec![a.feature().0.to_string()];
            assert!(ui.view_left_out(a.view()) && ui.menu_left_out(a.menu()), "{}", a.name());
            assert!(!ui.main_groups().iter().any(|g| g.1.contains(&Item::Open(a.menu()))), "{}", a.name());
        }
    }

    /// SCPI `:VIEW` reaches every page that has a keyword, and setups keep every page that is stored.
    #[test]
    fn scpi_and_setups_reach_the_pages() {
        for (k, v) in scpi_views() {
            let (mut s, mut ui, cv) = crate::ui::testkit::scpi_session();
            s.exec_line(&mut ui, &cv, &format!(":VIEW {k}"));
            assert_eq!(ui.m.view, v, ":VIEW {k}");
        }
        for v in stored_views() {
            let m = Model { view: v, ..Default::default() };
            assert_eq!(crate::store::parse_setup(&crate::store::setup_text(&m)).unwrap().view, v);
        }
        let bw = of(AView::Step).unwrap();
        assert!(bw.scpi().is_none() && !bw.stored(), "the bandwidth test is a procedure, not a stored view");
    }
}
