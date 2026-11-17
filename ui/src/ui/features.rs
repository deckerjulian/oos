// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! System > Features: what the menus offer. A scope is for measuring; who does not need the analyser pages, the
//! simulator or most of the decoders can leave them out (asked for in the forum: ygi, Sorama, eTobey, rdtsc). What
//! is left out is gone from the start menu, the top bar and the pickers, and it runs nothing: an analyser page that
//! was shown goes back to the scope, a decoder whose protocol is left out is switched off. A setting of the scope
//! (`/oos/system.conf`: `features.off`, `decoders.off`), nothing left out by default. Setups that name something left
//! out still load (`store::parse_setup_lenient`) and simply do not show it.

use super::*;

/// A feature that can be left out: its id in system.conf, its name, the analyser page and the menu it is.
pub(super) struct Feature {
    pub id: &'static str,
    pub name: &'static str,
    pub view: Option<AView>,
    pub menu: Option<MenuId>,
}

/// Decoders on a page of System > Features > Decoders.
pub(crate) const DEC_PAGE: usize = 18;

/// The features: every analyser page (`analysers::ANALYSERS`, in its order), then the simulator and Load file.
pub(super) fn features() -> &'static [Feature] {
    static F: std::sync::OnceLock<Vec<Feature>> = std::sync::OnceLock::new();
    F.get_or_init(|| {
        let pages = super::analysers::ANALYSERS.iter().map(|a| Feature {
            id: a.feature().0,
            name: a.feature().1,
            view: Some(a.view()),
            menu: Some(a.menu()),
        });
        pages
            .chain([
                Feature { id: "sim", name: "SIM", view: None, menu: Some(MenuId::Sim) },
                Feature { id: "load", name: "Load file", view: None, menu: None },
            ])
            .collect()
    })
}

impl Ui {
    /// Whether the feature `id` is offered.
    pub(super) fn feature_on(&self, id: &str) -> bool {
        !self.sys.features_off.iter().any(|x| x == id)
    }

    /// Whether the menu `id` is left out (System > Features).
    pub(super) fn menu_left_out(&self, id: MenuId) -> bool {
        features().iter().any(|f| f.menu == Some(id) && !self.feature_on(f.id))
    }

    /// Whether the analyser page `v` is left out.
    pub(super) fn view_left_out(&self, v: AView) -> bool {
        features().iter().any(|f| f.view == Some(v) && !self.feature_on(f.id))
    }

    /// Whether the decoder `p` is offered.
    pub(super) fn proto_on(&self, p: Proto) -> bool {
        let k = p.key();
        !self.sys.decoders_off.iter().any(|x| *x == k)
    }

    /// A `decoders.off` that leaves out every decoder (a hand-edited system.conf) keeps the one in use, as System >
    /// Features keeps one: its name then.
    pub(super) fn keep_one_decoder(&mut self) -> Option<&'static str> {
        if !self.protos_on().is_empty() {
            return None;
        }
        let k = self.m.dec_proto.key();
        self.sys.decoders_off.retain(|x| *x != k);
        Some(self.m.dec_proto.name())
    }

    /// The decoders offered, in the order of the picker.
    pub(super) fn protos_on(&self) -> Vec<Proto> {
        PROTOS.iter().copied().filter(|&p| self.proto_on(p)).collect()
    }

    /// What is left out runs nothing: once a step, an analyser page shown goes back to the scope, the spectrum pane
    /// closes, a decoder whose protocol is left out goes off.
    pub(super) fn features_enforce(&mut self) {
        if self.m.view != AView::Scope && self.view_left_out(self.m.view) {
            self.m.view = AView::Scope;
            self.relayout();
        }
        if self.m.pane == tables::PANE_SPECTRUM && !self.feature_on("spectrum") {
            self.m.pane = 0;
            self.relayout();
        }
        if self.m.dec_on && !self.proto_on(self.m.dec_proto) {
            self.m.dec_on = false;
        }
        if (self.sim_play || self.session_is_sim) && !self.feature_on("sim") {
            self.sim_play = false;
        }
    }

    // ------------------------------------------------------------------------------------------------ the pages

    pub(super) fn features_items(&self, id: MenuId) -> Option<Vec<Item>> {
        match id {
            MenuId::Features => Some(
                (0..features().len())
                    .map(Item::FeatOn)
                    .chain(std::iter::once(Item::Open(MenuId::Decoders(0))))
                    .collect(),
            ),
            // a page of decoders, the next page after them (the dialog holds about twenty rows)
            MenuId::Decoders(k) => {
                let (a, b) = (k * DEC_PAGE, ((k + 1) * DEC_PAGE).min(PROTOS.len()));
                let mut v: Vec<Item> = (a..b).map(Item::FeatDec).collect();
                v.push(Item::Open(MenuId::Decoders(if b < PROTOS.len() { k + 1 } else { 0 })));
                Some(v)
            }
            _ => None,
        }
    }

    pub(super) fn features_kind(it: Item) -> Option<Kind> {
        matches!(it, Item::FeatOn(_) | Item::FeatDec(_)).then_some(Kind::Choice)
    }

    pub(super) fn features_label(it: Item) -> Option<&'static str> {
        match it {
            Item::FeatOn(i) => features().get(i).map(|f| f.name),
            Item::FeatDec(i) => PROTOS.get(i).map(|p| p.name()),
            Item::Open(MenuId::Features) => Some("Features"),
            Item::Open(MenuId::Decoders(0)) => Some("Decoders"),
            Item::Open(MenuId::Decoders(_)) => Some("More decoders"),
            _ => None,
        }
    }

    pub(super) fn features_value(&self, it: Item) -> Option<String> {
        let on = match it {
            Item::FeatOn(i) => self.feature_on(features().get(i)?.id),
            Item::FeatDec(i) => self.proto_on(*PROTOS.get(i)?),
            _ => return None,
        };
        Some(if on { "On" } else { "Off" }.into())
    }

    pub(super) fn features_options(&self, it: Item) -> Option<Vec<String>> {
        matches!(it, Item::FeatOn(_) | Item::FeatDec(_)).then(|| vec!["Off".into(), "On".into()])
    }

    pub(super) fn features_adjust(&mut self, it: Item, _d: i32) -> bool {
        let (list, key, name) = match it {
            Item::FeatOn(i) => match features().get(i) {
                Some(f) => (&mut self.sys.features_off, f.id.to_string(), f.name.to_string()),
                None => return true,
            },
            Item::FeatDec(i) => match PROTOS.get(i) {
                Some(p) => (&mut self.sys.decoders_off, p.key(), p.name().to_string()),
                None => return true,
            },
            _ => return false,
        };
        let on = match list.iter().position(|x| *x == key) {
            Some(k) => {
                list.remove(k);
                true
            }
            None => {
                list.push(key);
                false
            }
        };
        // the last decoder stays: the Decode page needs one to show
        if !on && matches!(it, Item::FeatDec(_)) && self.protos_on().is_empty() {
            if let Item::FeatDec(i) = it {
                let k = PROTOS[i].key();
                self.sys.decoders_off.retain(|x| *x != k);
            }
            self.toast("One decoder stays");
            return true;
        }
        self.sys_save();
        self.features_enforce();
        self.toast(&format!("{} {}", name, if on { "offered" } else { "left out" }));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Break it catches: a feature left out still offered somewhere, still running, or not kept in system.conf
    #[test]
    fn what_is_left_out_is_gone_and_runs_nothing() {
        let mut u = crate::ui::testkit::demo_ui();
        assert!(u.items(MenuId::Utility).contains(&Item::Open(MenuId::Features)));
        let has = |u: &Ui, it: Item| u.main_groups().iter().any(|g| g.1.contains(&it));
        assert!(has(&u, Item::Open(MenuId::Magnetic)) && has(&u, Item::Open(MenuId::Sim)));
        // Magnetic off while it is shown: gone from the start menu, the page back to the scope
        u.m.view = AView::Magnetic;
        let i = features().iter().position(|f| f.id == "magnetic").unwrap();
        u.select_option(Item::FeatOn(i), "Off");
        assert!(!has(&u, Item::Open(MenuId::Magnetic)));
        assert_eq!(u.m.view, AView::Scope);
        // SIM and Load file: gone from the start menu and the Reference menu
        assert!(u.items(MenuId::Ref).contains(&Item::RefLoad));
        for id in ["sim", "load"] {
            let i = features().iter().position(|f| f.id == id).unwrap();
            u.select_option(Item::FeatOn(i), "Off");
        }
        assert!(!has(&u, Item::Open(MenuId::Sim)));
        assert!(!u.items(MenuId::Ref).contains(&Item::RefLoad));
        // a decoder off: not in the picker, switched off while it decodes
        u.m.dec_on = true;
        u.m.dec_proto = Proto::UART;
        let k = PROTOS.iter().position(|&p| p == Proto::UART).unwrap();
        u.select_option(Item::FeatDec(k), "Off");
        assert!(!u.m.dec_on);
        assert!(!u.options(Item::DecProto).contains(&Proto::UART.name().to_string()));
        // kept in the settings of the scope
        let (c, bad) = crate::sysconf::SysConf::parse(&u.sys.text());
        assert!(
            bad.is_empty() && c.features_off == ["magnetic", "sim", "load"] && c.decoders_off == [Proto::UART.key()],
            "{:?}",
            c.features_off
        );
        // and back
        u.select_option(Item::FeatOn(i), "On");
        assert!(u.feature_on("magnetic"));
    }

    // Break it catches: a hand-edited system.conf that leaves out every decoder, which System > Features does not
    // allow, made Decode > Protocol divide by zero
    #[test]
    fn a_system_conf_without_any_decoder_keeps_one() {
        let all = PROTOS.iter().map(|p| p.key()).collect::<Vec<_>>().join(" ");
        let (c, bad) = crate::sysconf::SysConf::parse(&format!("decoders.off = {}\n", all));
        assert!(bad.is_empty());
        let mut u = crate::ui::testkit::demo_ui();
        u.sys.decoders_off = c.decoders_off.clone();
        u.open_menu(MenuId::Decode);
        u.adjust(Item::DecProto, 1);
        u.adjust(Item::DecProto, -1);
        // as it is loaded at the start: the decoder in use stays
        let proto = u.m.dec_proto;
        assert_eq!(u.keep_one_decoder(), Some(proto.name()));
        assert_eq!(u.protos_on(), vec![proto]);
        assert_eq!(u.keep_one_decoder(), None);
    }
}
