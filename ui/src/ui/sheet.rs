// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio Settings Sheet: one menu pattern for the Studio layout. A docked inspector at the right of the screen
//! (the plot stays visible), a fixed header (Back, title, x), at most four tabs, a single column of at most seven
//! rows of 44 px, and a pinned footer with at most two actions. The sheet never moves while it is used.
//!
//! Nothing about a setting is decided here: every row is an `Item` of the existing menus and is read and changed
//! with `value`, `options`, `select_option`, `adjust` and `activate`. This file maps items to row types, splits
//! long pages into tabs and draws them. A page that does not fit the pattern (more than seven rows in a tab, more
//! than four tabs, keypad lists) is not converted: `sheet_spec` returns None and the classic dialog is used.
use super::meascatalog::{AddEntry, Preview, ADD_GROUPS};
use super::studio::StudioView;
use super::*;

/// Width of the docked sheet, and the row, header, tab strip and footer heights.
pub(super) const SHEET_W: i32 = 380;
pub(super) const SHEET_ROW: i32 = 44;
pub(super) const SHEET_MAX_ROWS: usize = 7;
pub(super) const SHEET_MAX_TABS: usize = 4;
pub(super) const SHEET_HEAD: i32 = 52;
pub(super) const SHEET_TABS: i32 = 40;
pub(super) const SHEET_FOOT: i32 = 56;
const PAD: i32 = 12;
/// Width of the label column of a row.
const LABEL_W: i32 = 132;

/// The five kinds of row a sheet has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum RowKind {
    /// two to four segments
    Choice,
    /// a value that opens the list of its options
    Pick,
    /// - value +
    Number,
    /// a switch
    Toggle,
    /// opens another page or runs an action; links go last
    Link,
}

/// One tab: its name, its rows (at most seven) and its pinned actions (at most two, destructive last).
#[derive(Clone, Debug)]
pub(super) struct SheetTab {
    pub name: &'static str,
    pub rows: Vec<Item>,
    pub foot: Vec<Item>,
}

#[derive(Clone, Debug)]
pub(super) struct SheetSpec {
    pub tabs: Vec<SheetTab>,
    /// tab shown when the sheet opens
    pub first: usize,
}

/// Where a menu item goes on the sheet.
#[derive(Clone, Copy)]
enum Slot {
    Tab(usize),
    Foot(usize),
    Skip,
}

/// What the sheet remembers between frames: the tab, the page of a long list, the filter of the add flow.
#[derive(Default)]
pub(super) struct SheetState {
    menu: Option<MenuId>,
    tab: usize,
    page_key: Option<(MenuId, Option<Item>)>,
    page: usize,
    pub add_group: usize,
    pub search: String,
}

/// Geometry of an items page.
pub(super) struct SheetRowGeo {
    pub it: Item,
    pub kind: RowKind,
    pub row: Rect,
    pub label: Rect,
    pub ctrl: Rect,
}

pub(super) struct SheetGeo {
    pub r: Rect,
    pub back: Option<Rect>,
    pub close: Rect,
    pub tabs: Vec<Rect>,
    pub rows: Vec<SheetRowGeo>,
    pub foot: Vec<(Item, Rect)>,
}

/// The cells of an add-panel tile and what each one opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum PanelKind {
    Measurements,
    Decode,
    Spectrum,
    Eye,
    Histogram,
    Trend,
    Cursor,
    Sim,
}

pub(super) const PANEL_KINDS: [(PanelKind, &str, &str); 8] = [
    (PanelKind::Measurements, "Measurements table", "All values, docked"),
    (PanelKind::Decode, "Decode table", "Packets under the plot"),
    (PanelKind::Spectrum, "Spectrum", "FFT under the plot"),
    (PanelKind::Eye, "Eye", "Eye diagram beside the plot"),
    (PanelKind::Histogram, "Histogram", "Vertical, in the plot"),
    (PanelKind::Trend, "Trend", "Values over time"),
    (PanelKind::Cursor, "Cursor readout", "Cursors on, with values"),
    (PanelKind::Sim, "SIM", "Generated signals"),
];

/// Items whose list is a keypad or a file list with its own handling: the classic picker keeps them.
fn keypad_pick(it: Item) -> bool {
    matches!(it, Item::TrigBusVal | Item::AfgFreqEntry | Item::WlanNet | Item::WlanForgetNet | Item::NtpServer | Item::TimeZone | Item::RemName | Item::RefImport | Item::MeasCopyTo)
}

fn destructive(it: Item) -> bool {
    matches!(it, Item::RefClear | Item::MeasClear | Item::UpdateRollback | Item::Stock | Item::PowerAct | Item::ClearDisp | Item::DefaultAct)
}

impl Ui {
    // ------------------------------------------------------------------------------------------------ the plan

    /// Which tab (or footer) each item of a menu goes to. `names` are the tab names, in order.
    fn sheet_plan(&self, menu: MenuId) -> Option<(&'static [&'static str], fn(Item) -> Slot)> {
        use Item::*;
        use MenuId as M;
        Some(match menu {
            M::Trigger => (&["Type and source", "Settings"], |it| match it {
                TrigType | TrigSrc | TrigSrc2 | TrigSlope | TrigSlope2 | TrigPol | DecProto | TrigBusWhat | TrigBusVal | Pat(_) | WinModeI
                    | VidStd | VidSync | VidLine | TrigLevel | TrigLevel2 | Trig50 | PatLevel | Open(M::LaPat) => Slot::Tab(0),
                SearchTable | SearchSave => Slot::Foot(1),
                Open(M::Zone) => Slot::Skip,
                _ => Slot::Tab(1),
            }),
            M::Channel(_) | M::ChMore(_) => (&["Vertical", "More"], |it| match it {
                ChOn(_) | ChScale(_) | ChOffset(_) | ChCoupling(_) | ChProbe(_) | ChBw(_) | ChInvert(_) => Slot::Tab(0),
                Open(M::ChMore(_)) => Slot::Skip,
                ChBiasZero(_) | AutoDeskew => Slot::Foot(1),
                SrcOn => Slot::Foot(0),
                _ => Slot::Tab(1),
            }),
            M::Measure => (&["Settings", "Sources", "Parameters", "Levels"], |it| match it {
                MeasGate | Open(M::MeasStat) | MeasLabels | Open(M::MeasPair) | Open(M::Dvm) => Slot::Tab(0),
                MeasSrc | MeasSrcB | MeasCopyTo => Slot::Tab(1),
                MeasLvType | MeasRefs | MeasLow | MeasMid | MeasHigh | MeasHyst | MeasBaseTop => Slot::Tab(3),
                Open(M::MeasFall) => Slot::Foot(3),
                MeasClear => Slot::Foot(0),
                StripDock => Slot::Skip,
                _ => Slot::Tab(2),
            }),
            M::MeasFall => (&["Fall levels"], |_| Slot::Tab(0)),
            M::MeasStat => (&["Statistics"], |it| match it { MeasStatReset => Slot::Foot(0), _ => Slot::Tab(0) }),
            M::Display => (&["Waveform", "Grid and views", "Screen"], |it| match it {
                Persist | ColorGrade | Intensity | Dots | Histogram | XyMode | ZInput | TrendSel(_) => Slot::Tab(0),
                Open(M::Windows) | Views | GridStyle | GridBright | GridFrame | Layout => Slot::Tab(1),
                ClearDisp => Slot::Foot(2),
                _ => Slot::Tab(2),
            }),
            M::Acquire => (&["Acquire"], |it| match it {
                Open(M::Horizontal) => Slot::Tab(0),
                _ => Slot::Tab(0),
            }),
            M::Horizontal => (&["Time base", "Zoom"], |it| match it {
                Zoom | ZScale | ZDelay => Slot::Tab(1),
                AutoDeskew | ClearDeskew => Slot::Foot(0),
                _ => Slot::Tab(0),
            }),
            M::Cursor => (&["Cursor"], |_| Slot::Tab(0)),
            M::Ref => (&["Reference", "Scale and files"], |it| match it {
                RefSel | RefSrc | RefLabel | RefColor => Slot::Tab(0),
                RefShow(_) => Slot::Skip,
                RefSave | RefClear => Slot::Foot(0),
                _ => Slot::Tab(1),
            }),
            M::Decode | M::Bus => (&["Decode", "Settings", "Bus"], |it| match it {
                DecSel | DecOn | DecProto | DecIn | DecSrc | DecSrc2 | DecCs | DecSrc4 | DecDiff => Slot::Tab(0),
                DecTable | DecExport => Slot::Foot(1),
                BusLabel => Slot::Foot(2),
                BusOn | BusPos | BusClk | BusClkThr | BusData | BusDataThr | BusRw => Slot::Tab(2),
                _ => Slot::Tab(1),
            }),
            M::Math => (&["Function", "Scale"], |it| match it {
                MathSel | MathFam | MathOpI | MathA | MathB | MathExprEdit | FftWin => Slot::Tab(0),
                MathExprI => Slot::Skip,
                MathAuto => Slot::Foot(1),
                _ => Slot::Tab(1),
            }),
            M::Utility => (&["Connectivity", "Device", "Service", "Advanced"], |it| match it {
                Open(M::Network) | Open(M::Time) => Slot::Tab(0),
                Offline | QuickAct | TouchLock | Graphics | Shot => Slot::Tab(1),
                Open(M::SelfCal) | SysInfo | About | Update => Slot::Tab(2),
                UpdateRollback => Slot::Foot(2),
                Stock => Slot::Foot(3),
                _ => Slot::Tab(3),
            }),
            _ => return None,
        })
    }

    /// The items a sheet of `menu` draws from: the menus it merges.
    fn sheet_source_items(&self, menu: MenuId) -> Vec<Item> {
        match menu {
            MenuId::Channel(c) | MenuId::ChMore(c) => self.items(MenuId::Channel(c)).into_iter().chain(self.items(MenuId::ChMore(c))).collect(),
            MenuId::Decode | MenuId::Bus => self.items(MenuId::Decode).into_iter().chain(self.items(MenuId::Bus)).collect(),
            m => self.items(m),
        }
    }

    /// The start menu of the Studio sheet: short, with nothing the top bar already has.
    fn sheet_main_spec(&self) -> SheetSpec {
        use Item::*;
        use MenuId as M;
        let names = ["Setup", "Analyze", "Analysers", "More"];
        let place = |it: Item| -> Slot {
            match it {
                AutosetAct | DefaultAct | ClearDisp | ForceAct | Open(M::Horizontal) | Open(M::Display) | NavHist => Slot::Tab(0),
                NavBack | NavFwd => Slot::Foot(0),
                Open(M::Math) | Open(M::Ref) | Open(M::Decode) | Open(M::Mask) | Open(M::Zone) | Open(M::Sim) | Open(M::Dvm) => Slot::Tab(1),
                Open(M::Spectrum) | Open(M::Eye) | Open(M::Power) | Open(M::Bode) | Open(M::BwTest) | IdentifyAct | Open(M::Magnetic) => Slot::Tab(2),
                Open(M::Inverter) | Open(M::Dpt) | Open(M::Afg) | Open(M::La) | Open(M::Logger) | Open(M::Storage) | Open(M::Utility) => Slot::Tab(3),
                LockAct | PowerAct => Slot::Foot(3),
                // what the top bar has, and what System has
                _ => Slot::Skip,
            }
        };
        let mut tabs: Vec<SheetTab> = names.iter().map(|n| SheetTab { name: n, rows: Vec::new(), foot: Vec::new() }).collect();
        for it in self.items(M::Main) {
            match place(it) {
                Slot::Tab(i) => tabs[i].rows.push(it),
                Slot::Foot(i) => tabs[i].foot.push(it),
                Slot::Skip => {}
            }
        }
        SheetSpec { tabs, first: 0 }
    }

    /// The sheet of a menu, or None when the menu has none yet or does not fit the pattern.
    pub(super) fn sheet_spec(&self, menu: MenuId) -> Option<SheetSpec> {
        if self.m.layout != Layout::Studio {
            return None;
        }
        let spec = if menu == MenuId::Main {
            self.sheet_main_spec()
        } else if menu == MenuId::MeasRow {
            let tabs = self.measrow_sheet_tabs();
            if tabs.is_empty() { return None; }
            SheetSpec { tabs, first: 0 }
        } else {
            let (names, place) = self.sheet_plan(menu)?;
            let mut tabs: Vec<SheetTab> = names.iter().map(|n| SheetTab { name: n, rows: Vec::new(), foot: Vec::new() }).collect();
            let mut items = self.sheet_source_items(menu);
            // the add-source button belongs to the page it fixes
            items.retain(|it| !matches!(self.ctl(*it, 200), Ctl::Text));
            for it in items {
                match place(it) {
                    Slot::Tab(i) => tabs[i].rows.push(it),
                    Slot::Foot(i) => tabs[i].foot.push(it),
                    Slot::Skip => {}
                }
            }
            // Ref: the ten slots are one Pick, and the shown switch is the one of the selected slot
            if matches!(menu, MenuId::Ref) {
                let sel = self.ref_sel;
                let at = 2.min(tabs[0].rows.len());
                tabs[0].rows.insert(at, Item::RefShow(sel));
            }
            let first = match menu {
                MenuId::ChMore(_) => 1,
                MenuId::Bus => 2,
                _ => 0,
            };
            SheetSpec { tabs, first }
        };
        if spec.tabs.len() > SHEET_MAX_TABS {
            return None;
        }
        // a third action does not fit the footer, an eighth row does not fit the body
        if spec.tabs.iter().any(|t| t.foot.len() > 2 || t.rows.len() > SHEET_MAX_ROWS) {
            return None;
        }
        Some(SheetSpec {
            tabs: spec.tabs.into_iter().map(|mut t| { t.foot.sort_by_key(|it| destructive(*it)); t }).collect(),
            first: spec.first,
        })
    }

    // ------------------------------------------------------------------------------------------------ state

    /// The sheet is the menu on screen: Studio, a menu open, a plan for it.
    pub(super) fn sheet_active(&self) -> bool {
        if self.m.layout != Layout::Studio || !self.menu_open || self.full_on() {
            return false;
        }
        match self.menu {
            MenuId::MeasAdd | MenuId::AddPanel => true,
            MenuId::Pick => self.pick.is_some_and(|p| !keypad_pick(p.it) && !self.options(p.it).is_empty() && self.sheet_spec(p.menu).is_some()),
            m => self.sheet_spec(m).is_some(),
        }
    }

    pub(super) fn sheet_dock_w(&self) -> i32 {
        if self.sheet_active() { SHEET_W + 4 } else { 0 }
    }

    /// The rectangle of the menu on screen, sheet or classic dialog.
    pub(super) fn menu_rect(&self) -> Option<Rect> {
        if self.sheet_active() { Some(self.sheet_rect()) } else { self.dlg_layout().map(|d| d.r) }
    }

    /// The sheet's place: the right edge of the body, from the top bar to the bottom bar. It is the same for every page.
    pub(super) fn sheet_rect(&self) -> Rect {
        let f = self.frame();
        let y = f.top.bottom();
        Rect::new(self.scr.w - SHEET_W, y, SHEET_W, (f.bottom.y - y).max(SHEET_HEAD + SHEET_FOOT))
    }

    fn sheet_tab(&self, spec: &SheetSpec) -> usize {
        let n = spec.tabs.len().max(1);
        if self.sheet.menu == Some(self.menu) { self.sheet.tab.min(n - 1) } else { spec.first.min(n - 1) }
    }

    fn sheet_page(&self, key: Option<Item>) -> usize {
        if self.sheet.page_key == Some((self.menu, key)) { self.sheet.page } else { 0 }
    }

    fn sheet_set_tab(&mut self, i: usize) {
        self.sheet.menu = Some(self.menu);
        self.sheet.tab = i;
    }

    fn sheet_set_page(&mut self, key: Option<Item>, page: usize) {
        self.sheet.page_key = Some((self.menu, key));
        self.sheet.page = page;
    }

    // ------------------------------------------------------------------------------------------------ rows

    /// The row type of an item, from the control the classic dialog would give it.
    pub(super) fn sheet_row_kind(&self, it: Item) -> RowKind {
        let w = SHEET_W - 2 * PAD - LABEL_W;
        if it == Item::TrigType {
            return RowKind::Choice;
        }
        if let Item::RefShow(_) = it {
            return RowKind::Toggle;
        }
        match self.ctl(it, w) {
            Ctl::Seg(o, _) if o.len() == 2 && o[0] == "Off" && o[1] == "On" => RowKind::Toggle,
            Ctl::Seg(o, _) if o.len() <= 4 => RowKind::Choice,
            Ctl::Seg(..) | Ctl::Pick => RowKind::Pick,
            Ctl::Value => RowKind::Number,
            Ctl::Button | Ctl::Text => RowKind::Link,
        }
    }

    /// Options and selection of a Choice row. The trigger type shows its common types inline and the rest behind More.
    pub(super) fn sheet_choice(&self, it: Item) -> (Vec<String>, Option<usize>) {
        if it == Item::TrigType {
            let common = ["Edge", "Pulse", "Slope"];
            let cur = self.value(Item::TrigType);
            let mut v: Vec<String> = common.iter().map(|s| s.to_string()).collect();
            return match common.iter().position(|c| *c == cur) {
                Some(i) => { v.push("More".into()); (v, Some(i)) }
                None => { v.push(cur); (v, Some(3)) }
            };
        }
        match self.ctl(it, SHEET_W - 2 * PAD - LABEL_W) {
            Ctl::Seg(o, s) => (o, s),
            _ => (Vec::new(), None),
        }
    }

    fn sheet_on(&self, it: Item) -> bool {
        match it {
            Item::RefShow(i) => self.refs.get(i).and_then(|r| r.as_ref()).is_some_and(|r| r.on),
            _ => self.value(it) == "On",
        }
    }

    /// What a row says: its label.
    fn sheet_label(&self, it: Item) -> String {
        match it {
            Item::SrcOn => {
                let names: Vec<String> = self.off_sources(self.menu).iter().map(|c| format!("CH{}", c + 1)).collect();
                if names.is_empty() { "Switch on source".into() } else { format!("Turn on {}", names.join(", ")) }
            }
            Item::RefShow(_) => "Shown".into(),
            Item::RowF(measrow::RF::Used) => self.measrow_text(it).unwrap_or_default(),
            _ => self.item_label(it).to_string(),
        }
    }

    /// Chrome shared by every page: Back (always at the left), the close button, the header rectangle.
    fn sheet_chrome(&self) -> (Rect, Option<Rect>, Rect) {
        let r = self.sheet_rect();
        let back = self.sheet_has_back().then(|| Rect::new(r.x + 8, r.y + 4, 76, 44));
        (r, back, Rect::new(r.right() - 52, r.y + 4, 44, 44))
    }

    fn sheet_has_back(&self) -> bool {
        self.parent.is_some() || self.menu == MenuId::Pick || !self.menu_history.is_empty()
    }

    /// Geometry of an items page.
    pub(super) fn sheet_geo(&self) -> Option<SheetGeo> {
        let spec = self.sheet_spec(self.menu)?;
        let (r, back, close) = self.sheet_chrome();
        let multi = spec.tabs.len() > 1;
        let tab = self.sheet_tab(&spec);
        let tw = (r.w - 2 * PAD) / spec.tabs.len().max(1) as i32;
        let tabs = if multi {
            (0..spec.tabs.len()).map(|i| Rect::new(r.x + PAD + i as i32 * tw, r.y + SHEET_HEAD, tw, SHEET_TABS)).collect()
        } else {
            Vec::new()
        };
        let body_y = r.y + SHEET_HEAD + if multi { SHEET_TABS } else { 0 };
        let t = &spec.tabs[tab];
        let mut rows = Vec::new();
        for (i, &it) in t.rows.iter().enumerate() {
            let row = Rect::new(r.x + PAD, body_y + i as i32 * SHEET_ROW, r.w - 2 * PAD, SHEET_ROW);
            let kind = self.sheet_row_kind(it);
            let label = Rect::new(row.x, row.y, LABEL_W, row.h);
            let ctrl = match kind {
                RowKind::Link => Rect::new(row.x, row.y + 2, row.w, 40),
                RowKind::Toggle => Rect::new(row.right() - 56, row.y + 9, 52, 26),
                _ => Rect::new(row.x + LABEL_W, row.y + 2, row.w - LABEL_W, 40),
            };
            rows.push(SheetRowGeo { it, kind, row, label, ctrl });
        }
        let foot = self.sheet_foot_rects(r, &t.foot);
        Some(SheetGeo { r, back, close, tabs, rows, foot })
    }

    fn sheet_foot_rects(&self, r: Rect, foot: &[Item]) -> Vec<(Item, Rect)> {
        let y = r.bottom() - SHEET_FOOT + 6;
        let w = r.w - 2 * PAD;
        match foot.len() {
            0 => Vec::new(),
            1 => vec![(foot[0], Rect::new(r.x + PAD, y, w, 44))],
            _ => {
                let half = (w - 8) / 2;
                vec![(foot[0], Rect::new(r.x + PAD, y, half, 44)), (foot[1], Rect::new(r.x + PAD + half + 8, y, half, 44))]
            }
        }
    }

    // ------------------------------------------------------------------------------------------------ drawing

    fn sheet_draw_chrome(&self, cv: &mut Canvas, title: &str) {
        let f = self.fonts();
        let (r, back, close) = self.sheet_chrome();
        cv.round(r, 8, ACCENT, 255);
        cv.round(r.inset(2), 7, DLG_BG, 255);
        let left = back.map_or(r.x + 14, |b| b.right() + 8);
        let mut t = title.to_string();
        while f.mb.width(&t) > close.x - left - 6 && t.chars().count() > 2 {
            let mut c = t.chars();
            c.next();
            c.next();
            t = format!("\u{2026}{}", c.as_str());
        }
        cv.text_in(&f.mb, Rect::new(left, r.y + 4, close.x - left - 4, 44), 0, &t, TEXT);
        if let Some(b) = back {
            btn(cv, b, BTN, BTN_BORDER);
            cv.text_in(&f.sb, b, 1, "Back", TEXT);
        }
        btn(cv, close, BTN, BTN_BORDER);
        cv.text_in(&f.mb, close, 1, "\u{d7}", TEXT);
        cv.hline(r.x + 6, r.right() - 7, r.y + SHEET_HEAD - 1, LINE);
    }

    fn sheet_draw_footer(&self, cv: &mut Canvas, r: Rect, foot: &[(Item, Rect)]) {
        let f = self.fonts();
        cv.hline(r.x + 6, r.right() - 7, r.bottom() - SHEET_FOOT, LINE);
        for &(it, b) in foot {
            let red = destructive(it);
            btn(cv, b, if red { mix(DLG_BG, RED, 30) } else { BTN }, if red { RED } else { BTN_BORDER });
            let label = self.sheet_label(it);
            cv.text_in(&f.mb, b, 1, &label, if red { RED } else { TEXT });
        }
    }

    pub(super) fn draw_sheet(&self, cv: &mut Canvas) {
        if !self.sheet_active() {
            return;
        }
        match self.menu {
            MenuId::MeasAdd => return self.draw_sheet_add(cv),
            MenuId::AddPanel => return self.draw_sheet_add_panel(cv),
            MenuId::Pick => return self.draw_sheet_pick(cv),
            _ => {}
        }
        let Some(g) = self.sheet_geo() else { return };
        let f = self.fonts();
        self.sheet_draw_chrome(cv, &self.menu_title(self.menu));
        let spec = self.sheet_spec(self.menu).expect("sheet");
        let cur = self.sheet_tab(&spec);
        for (i, r) in g.tabs.iter().enumerate() {
            let on = i == cur;
            if on {
                cv.round(*r, 5, rgb(0x2e, 0x2e, 0x32), 255);
                cv.round_outline(*r, 5, ACCENT);
            }
            cv.text_in(&f.sb, *r, 1, spec.tabs[i].name, if on { TEXT } else { DIM });
        }
        for row in &g.rows {
            self.sheet_draw_row(cv, row);
        }
        self.sheet_draw_footer(cv, g.r, &g.foot);
    }

    fn sheet_draw_row(&self, cv: &mut Canvas, row: &SheetRowGeo) {
        let f = self.fonts();
        let it = row.it;
        let label = self.sheet_label(it);
        let c = row.ctrl;
        if row.kind != RowKind::Link {
            let font = if f.m.width(&label) > row.label.w - 6 { &f.s } else { &f.m };
            cv.set_clip(Rect::new(row.label.x, row.label.y, row.label.w - 4, row.label.h));
            cv.text_in(font, row.label, 0, &label, LABEL);
            cv.no_clip();
        }
        match row.kind {
            RowKind::Choice => {
                let (opts, sel) = self.sheet_choice(it);
                let n = opts.len().max(1) as i32;
                let sw = (c.w - (n - 1) * 4) / n;
                for (k, o) in opts.iter().enumerate() {
                    let r = Rect::new(c.x + k as i32 * (sw + 4), c.y, sw, c.h);
                    if sel == Some(k) {
                        cv.round(r, 5, ACCENT, 255);
                        cv.text_in(&f.sb, r, 1, o, DARK_TEXT);
                    } else {
                        btn(cv, r, BTN, BTN_BORDER);
                        cv.text_in(&f.s, r, 1, o, TEXT);
                    }
                }
            }
            RowKind::Pick => {
                btn(cv, c, BTN, BTN_BORDER);
                let v = self.value(it);
                cv.set_clip(Rect::new(c.x + 6, c.y, c.w - 30, c.h));
                cv.text_in(&f.m, Rect::new(c.x + 10, c.y, c.w - 34, c.h), 0, &v, TEXT);
                cv.no_clip();
                chevron_down(cv, c.x + c.w - 18, c.y + c.h / 2 - 3, DIM);
            }
            RowKind::Number => {
                let (mi, mid, pl) = Self::value_parts(c);
                btn(cv, mi, BTN, BTN_BORDER);
                cv.text_in(&f.mb, mi, 1, "-", TEXT);
                btn(cv, mid, BTN, BTN_BORDER);
                cv.set_clip(mid);
                cv.text_in(&f.m, mid, 1, &self.value(it), TEXT);
                cv.no_clip();
                btn(cv, pl, BTN, BTN_BORDER);
                cv.text_in(&f.mb, pl, 1, "+", TEXT);
            }
            RowKind::Toggle => {
                let on = self.sheet_on(it);
                cv.round(c, c.h / 2, if on { ACCENT } else { BTN_BORDER }, 255);
                cv.circle(if on { c.right() - c.h / 2 } else { c.x + c.h / 2 }, c.y + c.h / 2, c.h / 2 - 3, if on { DARK_TEXT } else { DIM });
            }
            RowKind::Link => {
                let open = matches!(it, Item::Open(_));
                btn(cv, c, BTN, BTN_BORDER);
                let tail = if it == Item::MathExprEdit { self.value(Item::MathExprI) } else { String::new() };
                let room = c.w - 40 - if tail.is_empty() { 0 } else { (f.s.width(&tail) + 12).min(c.w / 2) };
                cv.set_clip(Rect::new(c.x + 8, c.y, room, c.h));
                cv.text_in(&f.m, Rect::new(c.x + 14, c.y, room, c.h), 0, &label, TEXT);
                cv.no_clip();
                if !tail.is_empty() {
                    cv.set_clip(Rect::new(c.x + room, c.y, c.w - room - 30, c.h));
                    cv.text_in(&f.s, Rect::new(c.x + room, c.y, c.w - room - 30, c.h), 2, &tail, DIM);
                    cv.no_clip();
                }
                if open {
                    chevron(cv, c.right() - 16, c.y + c.h / 2, 1, DIM);
                }
            }
        }
    }

    // ------------------------------------------------------------------------------------------------ tapping

    /// The Back button: to the page before, or out of a picker.
    fn sheet_back(&mut self) {
        if self.menu == MenuId::Pick {
            return self.close_picker();
        }
        if let Some((menu, focus)) = self.menu_history.pop() {
            self.menu = menu;
            self.focus = focus;
            self.parent = self.menu_history.last().map(|e| e.0);
        } else {
            self.menu = self.parent.take().unwrap_or(MenuId::Main);
            self.focus = 0;
        }
    }

    /// A tap while the sheet is the menu. False: outside it.
    pub(super) fn sheet_tap(&mut self, x: i32, y: i32) -> bool {
        let (r, back, close) = self.sheet_chrome();
        if !r.contains(x, y) {
            return false;
        }
        if close.contains(x, y) {
            self.close_menu();
            return true;
        }
        if back.is_some_and(|b| b.contains(x, y)) {
            self.sheet_back();
            return true;
        }
        match self.menu {
            MenuId::MeasAdd => return self.sheet_add_tap(x, y),
            MenuId::AddPanel => return self.sheet_add_panel_tap(x, y),
            MenuId::Pick => return self.sheet_pick_tap(x, y),
            _ => {}
        }
        let Some(g) = self.sheet_geo() else { return true };
        for (i, t) in g.tabs.iter().enumerate() {
            if t.contains(x, y) {
                self.sheet_set_tab(i);
                return true;
            }
        }
        for &(it, b) in &g.foot {
            if b.contains(x, y) {
                self.activate(it);
                return true;
            }
        }
        for row in &g.rows {
            if !row.row.contains(x, y) {
                continue;
            }
            self.sheet_row_tap(row, x, y);
            break;
        }
        true
    }

    fn sheet_row_tap(&mut self, row: &SheetRowGeo, x: i32, y: i32) {
        let it = row.it;
        match row.kind {
            RowKind::Choice => {
                if !row.ctrl.contains(x, y) {
                    return;
                }
                let (opts, _) = self.sheet_choice(it);
                let n = opts.len().max(1) as i32;
                let k = ((x - row.ctrl.x) * n / row.ctrl.w).clamp(0, n - 1) as usize;
                if it == Item::TrigType && k == 3 {
                    return self.open_picker(it);
                }
                self.select_option(it, &opts[k]);
            }
            RowKind::Pick => {
                if row.ctrl.contains(x, y) || row.label.contains(x, y) {
                    self.open_picker(it);
                }
            }
            RowKind::Number => {
                let (mi, _, pl) = Self::value_parts(row.ctrl);
                if mi.contains(x, y) {
                    self.adjust(it, -1);
                } else if pl.contains(x, y) {
                    self.adjust(it, 1);
                }
            }
            RowKind::Toggle => {
                if let Item::RefShow(_) = it {
                    self.activate(it);
                } else {
                    let on = self.sheet_on(it);
                    self.select_option(it, if on { "Off" } else { "On" });
                }
            }
            RowKind::Link => self.activate(it),
        }
    }

    // ------------------------------------------------------------------------------------------------ pick page

    /// Cells of the picker: two columns of up to seven rows.
    pub(super) fn sheet_pick_cells(&self) -> (Vec<(usize, Rect)>, usize, usize) {
        let (r, _, _) = self.sheet_chrome();
        let n = self.pick.map_or(0, |p| self.options(p.it).len());
        let per = SHEET_MAX_ROWS * 2;
        let pages = n.div_ceil(per).max(1);
        let page = self.sheet_page(self.pick.map(|p| p.it)).min(pages - 1);
        let w = (r.w - 2 * PAD - 8) / 2;
        let cells = (page * per..n.min((page + 1) * per)).enumerate().map(|(i, k)| {
            let (col, row) = ((i / SHEET_MAX_ROWS) as i32, (i % SHEET_MAX_ROWS) as i32);
            (k, Rect::new(r.x + PAD + col * (w + 8), r.y + SHEET_HEAD + 6 + row * SHEET_ROW, w, 40))
        }).collect();
        (cells, page, pages)
    }

    fn sheet_pager(&self, page: usize, pages: usize) -> [Rect; 2] {
        let r = self.sheet_rect();
        let y = r.bottom() - SHEET_FOOT + 6;
        let half = (r.w - 2 * PAD - 8) / 2;
        let _ = (page, pages);
        [Rect::new(r.x + PAD, y, half, 44), Rect::new(r.x + PAD + half + 8, y, half, 44)]
    }

    fn draw_sheet_pager(&self, cv: &mut Canvas, page: usize, pages: usize) {
        if pages < 2 {
            return;
        }
        let f = self.fonts();
        let r = self.sheet_rect();
        cv.hline(r.x + 6, r.right() - 7, r.bottom() - SHEET_FOOT, LINE);
        for (b, label) in self.sheet_pager(page, pages).into_iter().zip(["Previous".to_string(), format!("Next \u{b7} {}/{}", page + 1, pages)]) {
            btn(cv, b, BTN, BTN_BORDER);
            cv.text_in(&f.m, b, 1, &label, TEXT);
        }
    }

    fn draw_sheet_pick(&self, cv: &mut Canvas) {
        let Some(p) = self.pick else { return };
        let f = self.fonts();
        self.sheet_draw_chrome(cv, &self.menu_title(MenuId::Pick));
        let opts = self.options(p.it);
        let cur = self.value(p.it);
        let (cells, page, pages) = self.sheet_pick_cells();
        for (k, c) in cells {
            let on = opts[k] == cur;
            if on {
                cv.round(c, 5, ACCENT, 255);
            } else {
                btn(cv, c, BTN, BTN_BORDER);
            }
            let text = &opts[k];
            let font = if f.m.width(text) + 10 <= c.w { &f.m } else { &f.s };
            cv.set_clip(c);
            cv.text_in(font, Rect::new(c.x + 4, c.y, c.w - 8, c.h), 1, text, if on { DARK_TEXT } else { TEXT });
            cv.no_clip();
        }
        self.draw_sheet_pager(cv, page, pages);
    }

    fn sheet_pick_tap(&mut self, x: i32, y: i32) -> bool {
        let (cells, page, pages) = self.sheet_pick_cells();
        for (k, c) in cells {
            if c.contains(x, y) {
                self.activate(Item::PickOpt(k));
                return true;
            }
        }
        if pages > 1 {
            let [prev, next] = self.sheet_pager(page, pages);
            let key = self.pick.map(|p| p.it);
            if prev.contains(x, y) && page > 0 {
                self.sheet_set_page(key, page - 1);
            } else if next.contains(x, y) && page + 1 < pages {
                self.sheet_set_page(key, page + 1);
            }
        }
        true
    }

    // ------------------------------------------------------------------------------------------------ add measurement

    /// The sources on the screen: channels that are on and references that are shown.
    pub(super) fn sheet_shown_sources(&self) -> Vec<WaveId> {
        self.wave_ids().into_iter().filter(|id| match *id {
            WaveId::Ch(c) => c < n_ch() && self.m.ch[c].on,
            WaveId::Ref(i) => self.refs.get(i).and_then(|r| r.as_ref()).is_some_and(|r| r.on),
        }).collect()
    }

    /// Opens the add flow with a source that is on the screen.
    pub(super) fn sheet_add_open(&mut self) {
        let shown = self.sheet_shown_sources();
        if !shown.contains(&self.meas_src) {
            if let Some(&first) = shown.first() {
                self.sheet_pick_source(first);
            }
        }
        self.sheet.search.clear();
    }

    fn sheet_pick_source(&mut self, id: WaveId) {
        let name = self.wave_name(id);
        self.select_option(Item::MeasSrc, &name);
    }

    /// The entries of the list the filter leaves: the search text, else the category chip.
    pub(super) fn sheet_add_entries(&self) -> Vec<AddEntry> {
        let q = self.sheet.search.trim().to_ascii_lowercase();
        super::meascatalog::add_entries().into_iter().filter(|e| {
            if q.is_empty() { e.group == self.sheet.add_group } else { e.name.to_ascii_lowercase().contains(&q) }
        }).collect()
    }

    /// Geometry of the add page: source chips, filter chips, the list rows and the pager.
    pub(super) fn sheet_add_geo(&self) -> AddGeo {
        let (r, _, _) = self.sheet_chrome();
        let shown = self.sheet_shown_sources();
        let y0 = r.y + SHEET_HEAD;
        let w = r.w - 2 * PAD;
        let n = shown.len().clamp(1, 8) as i32;
        let cw = (w - (n - 1) * 4) / n;
        let chips = shown.iter().take(8).enumerate().map(|(i, &id)| (id, Rect::new(r.x + PAD + i as i32 * (cw + 4), y0 + 2, cw, 40))).collect();
        let gw = (w - 4 * 4) / 5;
        let groups = (0..ADD_GROUPS.len()).map(|i| Rect::new(r.x + PAD + i as i32 * (gw + 4), y0 + 46, gw, 40)).collect();
        let find = Rect::new(r.x + PAD + 4 * (gw + 4), y0 + 46, gw, 40);
        let list_y = y0 + 90;
        let fit = (((r.bottom() - SHEET_FOOT - list_y) / SHEET_ROW).max(1) as usize).min(SHEET_MAX_ROWS);
        let entries = self.sheet_add_entries();
        let pages = entries.len().div_ceil(fit).max(1);
        let page = self.sheet_page(None).min(pages - 1);
        let rows = (page * fit..entries.len().min((page + 1) * fit)).enumerate()
            .map(|(i, k)| (k, Rect::new(r.x + PAD, list_y + i as i32 * SHEET_ROW, w, SHEET_ROW))).collect();
        let [prev, next] = self.sheet_pager(page, pages);
        AddGeo { chips, groups, find, rows, page, pages, prev, next }
    }

    pub(super) fn draw_sheet_add(&self, cv: &mut Canvas) {
        let f = self.fonts();
        self.sheet_draw_chrome(cv, "Add measurement");
        let g = self.sheet_add_geo();
        if g.chips.is_empty() {
            let (r, _, _) = self.sheet_chrome();
            cv.text_in(&f.m, Rect::new(r.x + PAD, r.y + SHEET_HEAD + 2, r.w - 2 * PAD, 40), 0, "No source is on: turn on a channel", DIM);
        }
        for &(id, c) in &g.chips {
            let on = id == self.meas_src;
            if on {
                cv.round(c, 5, self.wave_color(id), 255);
            } else {
                btn(cv, c, BTN, self.wave_color(id));
            }
            cv.text_in(&f.mb, c, 1, &self.wave_name(id), if on { DARK_TEXT } else { TEXT });
        }
        let searching = !self.sheet.search.trim().is_empty();
        for (i, c) in g.groups.iter().enumerate() {
            let on = !searching && i == self.sheet.add_group;
            if on {
                cv.round(*c, 5, rgb(0x2e, 0x2e, 0x32), 255);
                cv.round_outline(*c, 5, ACCENT);
            } else {
                btn(cv, *c, BTN, BTN_BORDER);
            }
            cv.text_in(&f.s, *c, 1, ADD_GROUPS[i], if on { TEXT } else { DIM });
        }
        btn(cv, g.find, if searching { ACCENT } else { BTN }, if searching { ACCENT } else { BTN_BORDER });
        let label = if searching { format!("\u{d7} {}", self.sheet.search.trim()) } else { "Find".to_string() };
        cv.set_clip(g.find);
        cv.text_in(&f.s, Rect::new(g.find.x + 3, g.find.y, g.find.w - 6, g.find.h), 1, &label, if searching { DARK_TEXT } else { TEXT });
        cv.no_clip();
        let entries = self.sheet_add_entries();
        if entries.is_empty() {
            let (r, _, _) = self.sheet_chrome();
            cv.text_in(&f.m, Rect::new(r.x + PAD, r.y + SHEET_HEAD + 90, r.w - 2 * PAD, 44), 0, "Nothing matches", DIM);
        }
        for &(k, row) in &g.rows {
            let e = &entries[k];
            let added = self.add_is_on(e.cat);
            let preview = self.add_preview(e.cat);
            cv.hline(row.x, row.right() - 1, row.bottom() - 1, mix(DLG_BG, LINE, 160));
            let mut x = row.x + 4;
            if added {
                let (kx, ky) = (row.x + 8, row.y + row.h / 2);
                cv.line(kx, ky, kx + 4, ky + 4, ACCENT);
                cv.line(kx + 4, ky + 4, kx + 11, ky - 5, ACCENT);
                cv.line(kx, ky + 1, kx + 4, ky + 5, ACCENT);
                cv.line(kx + 4, ky + 5, kx + 11, ky - 4, ACCENT);
                x = row.x + 26;
            }
            let (sub, fix, value): (Option<String>, Option<Rect>, Option<String>) = match &preview {
                Preview::Value(v) => (None, None, Some(v.clone())),
                Preview::Computing => (None, None, Some("...".into())),
                Preview::Off(ch) => (Some(format!("Needs CH{} on", ch + 1)), Some(Rect::new(row.right() - 112, row.y + 6, 110, 32)), None),
                Preview::Reason(s) => (Some(s.clone()), None, None),
            };
            let vw = value.as_ref().map_or(0, |v| f.mb.width(v) + 8).max(if fix.is_some() { 118 } else { 0 });
            let name_w = row.right() - x - vw - 6;
            cv.set_clip(Rect::new(x, row.y, name_w.max(1), row.h));
            let col = if sub.is_some() { DIM } else if added { ACCENT } else { TEXT };
            match &sub {
                Some(s) => {
                    cv.text(&f.m, x, row.y + 19, e.name, col);
                    cv.text(&f.s, x, row.y + 37, s, if fix.is_some() { AMBER } else { DIM });
                }
                None => cv.text_in(&f.m, Rect::new(x, row.y, name_w.max(1), row.h), 0, e.name, col),
            }
            cv.no_clip();
            if let Some(v) = value {
                cv.text_in(&f.mb, Rect::new(row.right() - vw - 2, row.y, vw, row.h), 2, &v, if added { ACCENT } else { TEXT });
            }
            if let Some(b) = fix {
                btn(cv, b, BTN, ACCENT);
                if let Preview::Off(ch) = preview {
                    cv.text_in(&f.s, b, 1, &format!("Turn on CH{}", ch + 1), TEXT);
                }
            }
        }
        self.draw_sheet_pager(cv, g.page, g.pages);
    }

    pub(super) fn sheet_add_tap(&mut self, x: i32, y: i32) -> bool {
        let g = self.sheet_add_geo();
        for &(id, c) in &g.chips {
            if c.contains(x, y) {
                self.sheet_pick_source(id);
                return true;
            }
        }
        for (i, c) in g.groups.iter().enumerate() {
            if c.contains(x, y) {
                self.sheet.add_group = i;
                self.sheet.search.clear();
                self.sheet_set_page(None, 0);
                return true;
            }
        }
        if g.find.contains(x, y) {
            if self.sheet.search.trim().is_empty() {
                self.wlan.osk = Some(wlan::Osk::for_search(""));
            } else {
                self.sheet.search.clear();
                self.sheet_set_page(None, 0);
            }
            return true;
        }
        if g.pages > 1 {
            if g.prev.contains(x, y) && g.page > 0 {
                self.sheet_set_page(None, g.page - 1);
                return true;
            }
            if g.next.contains(x, y) && g.page + 1 < g.pages {
                self.sheet_set_page(None, g.page + 1);
                return true;
            }
        }
        let entries = self.sheet_add_entries();
        for &(k, row) in &g.rows {
            if !row.contains(x, y) {
                continue;
            }
            let cat = entries[k].cat;
            match self.add_preview(cat) {
                Preview::Off(ch) => {
                    if x >= row.right() - 116 {
                        // the fix, in place
                        self.select_option(Item::ChOn(ch), "On");
                    } else {
                        self.toast(&format!("Needs CH{} on", ch + 1));
                    }
                }
                Preview::Reason(s) => self.toast(&s),
                _ => self.sheet_add_toggle(cat),
            }
            break;
        }
        true
    }

    /// One tap adds the measurement (a second one removes it); a new one is pinned so it shows at once.
    pub(super) fn sheet_add_toggle(&mut self, cat: super::meascatalog::Cat) {
        let was = self.add_is_on(cat);
        self.catalog_add(cat);
        if !was && self.add_is_on(cat) {
            let key = self.add_key(cat);
            self.studio_set_pin(key, true);
        }
    }

    // ------------------------------------------------------------------------------------------------ add panel

    /// The tiles of the add-panel page: two columns of four.
    pub(super) fn sheet_panel_tiles(&self) -> Vec<(PanelKind, Rect)> {
        let (r, _, _) = self.sheet_chrome();
        let w = (r.w - 2 * PAD - 8) / 2;
        let h = ((r.h - SHEET_HEAD - 8 - 4 * 8) / 4).clamp(60, 92);
        PANEL_KINDS.iter().enumerate().map(|(i, (k, _, _))| {
            let (col, row) = ((i % 2) as i32, (i / 2) as i32);
            (*k, Rect::new(r.x + PAD + col * (w + 8), r.y + SHEET_HEAD + 8 + row * (h + 8), w, h))
        }).collect()
    }

    /// Whether the panel of a tile is on the screen.
    pub(super) fn panel_shown(&self, k: PanelKind) -> bool {
        match k {
            PanelKind::Measurements => matches!(self.studio.view, StudioView::Docked | StudioView::Floating | StudioView::Only),
            PanelKind::Decode => self.table == Some(tables::TableKind::Decode),
            PanelKind::Spectrum => self.m.pane == tables::PANE_SPECTRUM || self.m.view == AView::Spectrum,
            PanelKind::Eye => self.m.view == AView::Eye,
            PanelKind::Histogram => self.m.hist != Hist::Off,
            PanelKind::Trend => self.m.pane == tables::PANE_TREND,
            PanelKind::Cursor => self.m.cursor != CursorMode::Off,
            PanelKind::Sim => self.studio.view == StudioView::Sim,
        }
    }

    pub(super) fn draw_sheet_add_panel(&self, cv: &mut Canvas) {
        let f = self.fonts();
        self.sheet_draw_chrome(cv, "Add panel");
        for ((_, name, sub), (k, r)) in PANEL_KINDS.iter().zip(self.sheet_panel_tiles()) {
            let on = self.panel_shown(k);
            btn(cv, r, BTN, if on { ACCENT } else { BTN_BORDER });
            let font = if f.mb.width(name) + 16 <= r.w { &f.mb } else { &f.m };
            cv.set_clip(r);
            cv.text(font, r.x + 10, r.y + r.h / 2 - 2, name, TEXT);
            cv.text(&f.s, r.x + 10, r.y + r.h / 2 + 20, if on { "On screen" } else { sub }, if on { ACCENT } else { DIM });
            cv.no_clip();
        }
    }

    pub(super) fn sheet_add_panel_tap(&mut self, x: i32, y: i32) -> bool {
        for (k, r) in self.sheet_panel_tiles() {
            if r.contains(x, y) {
                self.studio_add_panel(k);
                break;
            }
        }
        true
    }

    /// Opens a panel docked, with the pages that already exist; the sheet closes so the panel is seen.
    pub(super) fn studio_add_panel(&mut self, k: PanelKind) {
        self.close_menu();
        match k {
            PanelKind::Measurements => self.studio_set_view(StudioView::Docked),
            PanelKind::Decode => {
                if self.table != Some(tables::TableKind::Decode) {
                    self.activate(Item::DecTable);
                }
            }
            PanelKind::Spectrum => self.select_option(Item::PaneSel, "Spectrum"),
            PanelKind::Trend => self.select_option(Item::PaneSel, "Trend"),
            PanelKind::Eye => {
                self.select_option(Item::EyePlace, "Side");
                if self.m.view != AView::Eye {
                    self.adjust(Item::EyeOn, 1);
                }
            }
            PanelKind::Histogram => self.select_option(Item::Histogram, "Vertical"),
            PanelKind::Cursor => {
                if self.m.cursor == CursorMode::Off {
                    self.select_option(Item::CurMode, "Time");
                }
            }
            PanelKind::Sim => self.studio_set_view(StudioView::Sim),
        }
        self.relayout();
    }

}

/// The parts of the add-measurement page.
pub(super) struct AddGeo {
    pub chips: Vec<(WaveId, Rect)>,
    pub groups: Vec<Rect>,
    pub find: Rect,
    pub rows: Vec<(usize, Rect)>,
    pub page: usize,
    pub pages: usize,
    pub prev: Rect,
    pub next: Rect,
}

impl Ui {
    /// The search text of the add list, from the keyboard.
    pub(super) fn sheet_set_search(&mut self, text: &str) {
        self.sheet.search = text.to_string();
        self.sheet_set_page(None, 0);
    }
}
