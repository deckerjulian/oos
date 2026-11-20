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
        for v in AView::STORED {
            let m = Model { view: v, ..Default::default() };
            assert_eq!(crate::store::parse_setup(&crate::store::setup_text(&m)).unwrap().view, v);
        }
        let bw = of(AView::Step).unwrap();
        assert!(bw.scpi().is_none() && !AView::STORED.contains(&AView::Step), "the bandwidth test is a procedure");
        for a in ANALYSERS.iter().filter(|a| a.view() != AView::Step) {
            assert!(
                AView::STORED.contains(&a.view()),
                "{:?}: a page a setup keeps, missing in AView::STORED",
                a.view()
            );
        }
    }

    /// Every page with what is stored of it: its feature id (`features.off` in system.conf), the word a setup keeps
    /// its view as (`view = <word>`, the variant's name) and its keyword of `:VIEW`. What was released never changes:
    /// system.conf, setups and scripts hold it. A new page gets its row here.
    const PAGES: [(&str, AView, &str, Option<&str>); 8] = [
        ("spectrum", AView::Spectrum, "Spectrum", Some("SPECtrum")),
        ("eye", AView::Eye, "Eye", Some("EYE")),
        ("power", AView::Power, "Power", Some("POWer")),
        ("bode", AView::Bode, "Bode", Some("BODE")),
        ("bwtest", AView::Step, "Step", None),
        ("magnetic", AView::Magnetic, "Magnetic", Some("MAGnetic")),
        ("inverter", AView::Inverter, "Inverter", Some("INVerter")),
        ("dpt", AView::Dpt, "Dpt", Some("DPT")),
    ];

    /// What a page needs besides its `Analyser`, each failure with its name and the part that is missing: its row
    /// in `PAGES` with its feature id, the word of its view and its `:VIEW` keyword as they are, the keyword in the
    /// `:VIEW` rows of docs/scpi.md and of the wiki, its name under Analysers in wiki/Features.md, and its menu in
    /// the list of the layout check.
    #[test]
    fn every_page_is_complete() {
        use crate::ui::testkit::{document, names, section};
        let view_row = |path: &str| {
            let text = document(path);
            text.lines()
                .find(|l| l.starts_with("| `:VIEW[?]`"))
                .unwrap_or_else(|| panic!("{}: no row of :VIEW", path))
                .to_string()
        };
        let (scpi_md, wiki_scpi) = (view_row("docs/scpi.md"), view_row("wiki/SCPI-and-VXI-11.md"));
        let wiki = section(&document("wiki/Features.md"), "**Analysers**");
        // the menus the layout check opens (`every_menu_fits`)
        let fits = include_str!("lint/menu_layout.rs");
        let fits = fits.split_once("fn menus_fit_at(").expect("the walk of the menus").1;
        let fits = fits.split_once("for m in [").and_then(|x| x.1.split_once("] {")).expect("its list of menus").0;
        assert_eq!(ANALYSERS.len(), PAGES.len(), "PAGES lists the pages of ANALYSERS");
        for a in ANALYSERS {
            let (id, fname) = a.feature();
            let row = PAGES.iter().find(|p| p.1 == a.view()).unwrap_or_else(|| panic!("{}: no row in PAGES", a.name()));
            assert_eq!(id, row.0, "{}: a feature id never changes", a.name());
            assert_eq!(format!("{:?}", a.view()), row.2, "{}: the word a setup stores never changes", a.name());
            assert_eq!(a.scpi(), row.3, "{}: its :VIEW keyword", a.name());
            if let Some(k) = a.scpi() {
                assert!(names(&scpi_md, k), "{}: {} is not in the :VIEW row of docs/scpi.md", a.name(), k);
                assert!(names(&wiki_scpi, k), "{}: {} is not in the :VIEW row of wiki/SCPI-and-VXI-11.md", a.name(), k);
            }
            assert!(names(&wiki, fname), "{}: {} is not named under Analysers in wiki/Features.md", a.name(), fname);
            // the layout check opens every page of the table of pages (ADR 0060) and the menus of its own list
            let menu = format!("{:?},", a.menu());
            assert!(
                super::pages::page(a.menu()).is_some() || fits.lines().any(|l| l.trim() == menu),
                "{}: its menu {} is neither in the table of pages nor in the list of menus_fit_at \
                 (ui/src/ui/lint/menu_layout.rs)",
                a.name(),
                menu
            );
        }
        // the features that are no page keep their ids too
        let ids: Vec<&str> = features::features().iter().map(|f| f.id).collect();
        let want: Vec<&str> = PAGES.iter().map(|p| p.0).chain(["sim", "load"]).collect();
        assert_eq!(ids, want, "the ids of System > Features");
    }
}
