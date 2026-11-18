// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What a menu shows: the title of a menu, the kind of control each item has, its label and its value as text. A new
//! item gets its line in `Ui::kind`, `Ui::label` and `Ui::value` here, and what it does in `menu_actions.rs`.

use super::*;

impl Ui {
    /// Row label, with the trigger rows named after their role in the selected trigger type.
    pub(super) fn item_label(&self, it: Item) -> &'static str {
        if it == Item::SimPlay {
            if self.sim_generating() {
                return "Cancel generation";
            }
            return if !self.session_is_sim {
                "Start simulation"
            } else if self.m.run == RunState::Run {
                "Stop simulation"
            } else {
                "Run simulation"
            };
        }
        let t = self.m.trig_type;
        let two_level = matches!(t, TrigType::Slope | TrigType::Runt | TrigType::Window);
        match it {
            Item::CurA if self.m.cursor == CursorMode::Xy => "X1",
            Item::CurB if self.m.cursor == CursorMode::Xy => "X2",
            Item::TrigLevel if two_level => "Upper",
            Item::TrigLevel2 if two_level => "Lower",
            Item::TrigLevel if t == TrigType::Delay => "Level A",
            Item::TrigLevel2 if t == TrigType::Delay => "Level B",
            Item::TrigLevel if t == TrigType::SetupHold => "Data level",
            Item::TrigLevel2 if t == TrigType::SetupHold => "Clock level",
            Item::TrigSrc if t == TrigType::Delay => "Source A",
            Item::TrigSrc if t == TrigType::SetupHold => "Data",
            Item::TrigSrc if matches!(t, TrigType::Pattern | TrigType::Duration) => "Level of",
            Item::TrigSrc2 if t == TrigType::SetupHold => "Clock",
            Item::TrigSlope if t == TrigType::Delay => "Slope A",
            Item::TrigSlope2 if t == TrigType::SetupHold => "Clock edge",
            Item::TrigT1 if t == TrigType::Timeout => "Timeout",
            Item::DecOpt(k) => self.m.dec_proto.module().and_then(|p| p.options().get(k)).map_or("Option", |o| o.name),
            Item::DecSrc if self.m.dec_proto.roles()[0] != Some("Source") => {
                self.m.dec_proto.roles()[0].unwrap_or("Source")
            }
            Item::DecSrc2 if self.m.dec_proto.roles()[1].is_some() => self.m.dec_proto.roles()[1].unwrap_or("Clock"),
            Item::DecCs if self.m.dec_proto.roles()[2].is_some() => self.m.dec_proto.roles()[2].unwrap_or("CS"),
            Item::DecSrc4 if self.m.dec_proto.roles()[3].is_some() => self.m.dec_proto.roles()[3].unwrap_or("Source 4"),
            Item::DecThr if self.m.dec_proto == Proto::CPHY => "Min level gap",
            Item::TrigT1 if matches!(self.m.trig_when, WhenKind::In | WhenKind::Out) => "Lower limit",
            Item::NavHist if self.hist_pos.is_some() => "Live",
            _ => Self::label(it),
        }
    }

    pub(super) fn menu_title(&self, menu: MenuId) -> String {
        match menu {
            // where the files go now; /tmp is in RAM: say so
            MenuId::Storage if self.save_dir() == std::path::Path::new(crate::store::USB_DIR) => {
                "Storage \u{b7} USB stick".into()
            }
            MenuId::Storage if self.save_dir().starts_with("/tmp") => {
                format!("Storage \u{b7} {} (lost at power-off)", self.save_dir().display())
            }
            MenuId::Storage => format!("Storage \u{b7} {}", self.save_dir().display()),
            MenuId::Mask => "Mask test".into(),
            MenuId::Zone => "Zone trigger".into(),
            MenuId::Spectrum => "Spectrum analyser".into(),
            MenuId::Eye => "Eye diagram / jitter".into(),
            MenuId::Power => "Power analysis".into(),
            MenuId::Identify => "Identify: tap to apply".into(),
            MenuId::SelfCal => "Self-calibration".into(),
            MenuId::Network => "Network".into(),
            MenuId::Features => "Features".into(),
            MenuId::Decoders(k) => format!("Decoders {} of {}", k + 1, PROTOS.len().div_ceil(features::DEC_PAGE)),
            MenuId::Remote => "Remote services".into(),
            MenuId::Ssh => "SSH".into(),
            MenuId::Time => "Date and time".into(),
            MenuId::Bode => "Bode plot".into(),
            MenuId::Sim => "SIM".into(),
            MenuId::SimAdvanced => "SIM advanced".into(),
            MenuId::Magnetic => "Magnetic".into(),
            MenuId::Inverter => "Inverter".into(),
            MenuId::Dpt => "Double pulse".into(),
            MenuId::Afg => "Generator (GI/GO)".into(),
            MenuId::BwTest => "Bandwidth test (step response)".into(),
            MenuId::ViewMore => self.view_name().into(),
            MenuId::La => self.la_title(),
            MenuId::LaLines => "Logic analyser \u{b7} lines shown".into(),
            MenuId::LaPat => "Pattern \u{b7} D0-D15".into(),
            MenuId::Main => "Menu".into(),
            MenuId::Channel(c) => format!("CH{}", c + 1),
            MenuId::Horizontal => "Horizontal".into(),
            MenuId::ChMore(c) => format!("CH{} more", c + 1),
            MenuId::Trigger => "Trigger".into(),
            MenuId::Acquire => "Acquire".into(),
            MenuId::Measure => "Measure settings".into(),
            MenuId::MeasAdd => "Add measurement".into(),
            MenuId::MeasSelected => format!("Selected measurements ({})", self.measurement_count()),
            MenuId::MeasHold => "Add".into(),
            MenuId::MeasPair => format!(
                "Pairs {} to {}",
                self.pair_source_caption(self.meas_src),
                self.pair_source_caption(self.meas_src_b)
            ),
            MenuId::Dvm => "DVM and counter".into(),
            MenuId::Logger => "Data logger".into(),
            MenuId::Cursor => format!(
                "Cursor · {}",
                self.selected_workspace_source().map_or("No source".into(), |id| self.wave_name(id))
            ),
            MenuId::Display => "Display".into(),
            MenuId::Windows => "Windows".into(),
            MenuId::Utility => "System".into(),
            MenuId::Math => "Math".into(),
            MenuId::Ref => {
                self.refs[self.ref_sel].as_ref().map_or(format!("Reference R{} · empty", self.ref_sel + 1), |r| {
                    format!("Reference R{}{}{}", self.ref_sel + 1, if r.label.is_empty() { "" } else { " · " }, r.label)
                })
            }
            MenuId::Decode => "Decode".into(),
            MenuId::Bus => "Bus · I2C".into(),
            MenuId::Pick => match self.pick {
                Some(p) if Self::hex_item(p.it) => format!("{} \u{b7} 0x{}_", self.item_label(p.it), self.keypad),
                Some(p) if p.it == Item::AfgFreqEntry => {
                    format!("Frequency \u{b7} {}_ (max {})", self.keypad, si_short(self.m.gen_max_hz(), "Hz"))
                }
                Some(p) => format!("{} \u{b7} {}", self.menu_title(p.menu), self.item_label(p.it)),
                None => String::new(),
            },
        }
    }

    pub(super) fn kind(it: Item) -> Kind {
        if matches!(it, Item::AutoDeskew | Item::ClearDeskew) {
            return Kind::Action;
        }
        if let Some(k) = PARTS.iter().find_map(|p| p.kind.and_then(|f| f(it))) {
            return k;
        }
        match it {
            Item::LogIvl
            | Item::ChBias(_)
            | Item::ChSkew(_)
            | Item::RefColor
            | Item::CurC
            | Item::CurD
            | Item::RefSel
            | Item::RefVpd
            | Item::RefOffset
            | Item::MathLinA
            | Item::MathLinB
            | Item::MathScale
            | Item::MathOffset
            | Item::ZScale
            | Item::ZDelay
            | Item::DecThr => Kind::Value,
            Item::MathAuto | Item::ZoneBox | Item::ZoneShape | Item::ZoneClear => Kind::Action,
            Item::DecBits => Kind::Value,
            Item::TrigLevel2 | Item::VidLine | Item::PatLevel | Item::NthN => Kind::Value,
            Item::ChScale(_)
            | Item::ChOffset(_)
            | Item::HScale
            | Item::HDelay
            | Item::TrigLevel
            | Item::CurA
            | Item::CurB => Kind::Value,
            Item::RefSave | Item::RefClear => Kind::Action,
            Item::StoreSlot | Item::MaskX | Item::MaskY => Kind::Value,
            Item::StoreSaveSetup
            | Item::StoreRecall
            | Item::StoreCsv
            | Item::StorePng
            | Item::StoreFfSave
            | Item::StoreFfLoad
            | Item::MaskCreate
            | Item::MaskFromRef
            | Item::MaskReset
            | Item::MaskClear => Kind::Action,
            Item::Open(_)
            | Item::MathExprEdit
            | Item::ChBiasZero(_)
            | Item::RefLabel
            | Item::RefExport
            | Item::RefImport
            | Item::RefLoad
            | Item::DecTable
            | Item::DecExport
            | Item::SearchTable
            | Item::SearchSave
            | Item::StoreBin
            | Item::MeasPair(_)
            | Item::CntClear
            | Item::MaskSave
            | Item::MaskLoad
            | Item::ChLabel(_)
            | Item::SrcOn
            | Item::WlanInfo
            | Item::WlanScan
            | Item::WlanForget
            | Item::HZero
            | Item::Trig50
            | Item::MeasAdd(_)
            | Item::AddMeas
            | Item::MeasCfg(_)
            | Item::MeasRemove(_)
            | Item::MeasClear
            | Item::AcqSegTable
            | Item::ClearDisp
            | Item::About
            | Item::SysInfo
            | Item::Shot
            | Item::Stock
            | Item::Update
            | Item::UpdateRollback => Kind::Action,
            Item::PickOpt(_) | Item::ViewExit | Item::ViewSettings => Kind::Action,
            Item::SpiTimeout => Kind::Value,
            Item::RunStop | Item::SingleAct | Item::AutosetAct | Item::DefaultAct | Item::SlopeAct | Item::ForceAct => {
                Kind::Action
            }
            Item::NavHist | Item::NavBack | Item::NavFwd | Item::QuickRun | Item::LockAct | Item::PowerAct => {
                Kind::Action
            }
            _ => Kind::Choice,
        }
    }

    fn label(it: Item) -> &'static str {
        if let Some(l) = PARTS.iter().find_map(|p| p.label.and_then(|f| f(it))) {
            return l;
        }
        Self::label_menus(it)
            .or_else(|| Self::label_decode(it))
            .or_else(|| Self::label_inputs(it))
            .unwrap_or_else(|| Self::label_system(it))
    }

    /// Labels of the menu entries, the storage and mask rows and the measurement display rows.
    fn label_menus(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Acquire) => "Acquire",
            Item::Open(MenuId::Trigger) => "Trigger",
            Item::Open(MenuId::Horizontal) => "Horizontal",
            Item::Open(MenuId::ChMore(_)) => "More",
            Item::Open(MenuId::Measure) => "Measure",
            Item::AddMeas => "Add meas",
            Item::Open(MenuId::Cursor) => "Cursor",
            Item::Open(MenuId::Display) => "Display",
            Item::Open(MenuId::Utility) => "System",
            Item::Open(MenuId::MeasPair) => "Channel pair",
            Item::Open(MenuId::Dvm) => "DVM / counter",
            Item::Open(MenuId::Logger) => "Data logger",
            Item::Open(MenuId::Network) => "Network",
            Item::Open(MenuId::Math) => "Math",
            Item::Open(MenuId::Ref) => "Reference",
            Item::Open(MenuId::Storage) => "Storage",
            Item::Open(MenuId::Mask) => "Mask",
            Item::Open(MenuId::Zone) => "Zones",
            Item::Open(MenuId::Magnetic) => "Magnetic",
            Item::Open(MenuId::Inverter) => "Inverter",
            Item::Open(MenuId::Dpt) => "Double pulse",
            Item::StoreSlot => "Setup slot",
            Item::StoreDest => "Save to",
            Item::StoreSaveSetup => "Save setup",
            Item::StoreRecall => "Recall setup",
            Item::StoreCsv => "Save waveform (CSV)",
            Item::StorePng => "Save screenshot (PNG)",
            Item::StoreFfSave => "Save fast-frame",
            Item::StoreFfLoad => "Load fast-frame",
            Item::MaskOn => "Mask test",
            Item::MaskSrc => "Source",
            Item::MaskX => "Tolerance X",
            Item::MaskY => "Tolerance Y",
            Item::MaskAction => "On fail",
            Item::MaskInvert => "Invert",
            Item::MaskCreate => "Create from CH",
            Item::MaskFromRef => "Create from ref",
            Item::MaskReset => "Reset counts",
            Item::MaskClear => "Clear mask",
            Item::MeasStats => "Statistics",
            Item::MeasLabels => "Labels on trace",
            Item::StripDock => "Dock",
            Item::Zoom => "Zoom",
            Item::AutoRoll => "Auto Roll",
            _ => return None,
        })
    }

    /// Labels of the decoder, event, bus trigger, zoom, reference and math rows; "" for the other menu entries.
    fn label_decode(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Decode) => "Decode",
            Item::DecOn => "Decoder",
            Item::DecSel => "Decoder no.",
            Item::DecTable => "Event table",
            Item::DecExport => "Export events",
            Item::SearchTable => "Event list",
            Item::SearchSave => "Save events",
            Item::StoreBin => "Save waveform (binary)",
            Item::PaneSel => "Lower",
            Item::TrendSel(0) => "Trend chart 1",
            Item::TrendSel(_) => "Trend chart 2",
            Item::DecSrc => "Source",
            Item::DecBaud => "Baud",
            Item::DecCphyRate => "Symbol rate",
            Item::DecCanRate => "Bit rate",
            Item::TrigBusWhat => "Match",
            Item::TrigBusVal => "Value",
            Item::SerRel => "Relation",
            Item::SerMax => "Max",
            Item::SerMask => "Mask",
            Item::UartPol => "Polarity",
            Item::UartStop => "Stop bits",
            Item::UartErr => "Error type",
            Item::SpiCs => "CS",
            Item::SpiTimeout => "Timeout",
            Item::I2cDir => "Direction",
            Item::I2cAddrBits => "Address",
            Item::I2cBytes => "Bytes",
            Item::DecCanH => "Signal",
            Item::DecUsbSpeed => "Speed",
            Item::DecOpt(_) => "Option",
            Item::DecSrc4 => "Source 4",
            Item::DecDiff => "B line (A-B)",
            Item::DecThr => "Threshold",
            Item::DecFmt => "Format",
            Item::DecProto => "Protocol",
            Item::DecSrc2 => "Clock",
            Item::DecCs => "CS",
            Item::DecMode => "Mode",
            Item::DecBits => "Data bits",
            Item::DecParity => "Parity",
            Item::ZScale => "Zoom time/div",
            Item::ZDelay => "Zoom position",
            Item::XyMode => "Format",
            Item::RefSrc => "Save from",
            Item::RefSave => "Save to next R",
            Item::RefShow(i) => ["R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8", "R9", "R10"][i.min(NREF - 1)],
            Item::RefClear => "Clear all",
            Item::MathOpI => "Operation",
            Item::MathFam => "Category",
            Item::MathA => "Source A",
            Item::MathB => "Source B",
            Item::MathScale => "Scale",
            Item::MathOffset => "Offset",
            Item::MathSel => "Channel",
            Item::MathAuto => "Auto scale",
            Item::Open(MenuId::MeasSelected) => "Selected",
            Item::Open(_) => "",
            _ => return None,
        })
    }

    /// Labels of the channel, horizontal, trigger, acquire, measure, logger, slot and catalogue rows.
    fn label_inputs(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::ChCoupling(_) => "Coupling",
            Item::ChProbe(_) => "Probe",
            Item::ChScale(_) => "Scale",
            Item::ChOffset(_) => "Offset",
            Item::HScale => "Time/div",
            Item::HDelay => "Delay",
            Item::HZero => "Delay to 0",
            Item::AutoDeskew => "Auto deskew",
            Item::ClearDeskew => "Clear deskew",
            Item::TrigSrc => "Source",
            Item::TrigLevel => "Level",
            Item::Trig50 => "Level 50 %",
            Item::TrigType => "Type",
            Item::SearchOn => "Mark events",
            Item::TrigLevel2 => "Level 2",
            Item::TrigSrc2 => "Source B",
            Item::TrigWhen => "When",
            Item::VidLine => "Line",
            Item::Pat(0) => "CH1",
            Item::Pat(1) => "CH2",
            Item::Pat(2) => "CH3",
            Item::Pat(_) => "CH4",
            Item::PatLevel => "Level",
            Item::NthN => "Edge no.",
            Item::AcqSeg => "Segments",
            Item::AcqSegIvl => "Seg. interval",
            Item::AcqSegTable => "Segment times",
            Item::AcqDepth => "Memory",
            Item::AcqAlias => "Aliasing",
            Item::MeasSrc => "Source A",
            Item::MeasCopyTo => "Copy meas. to",
            Item::MeasSrcB => "Source B",
            Item::MeasPair(k) => tools::PAIRS[k.min(tools::PAIRS.len() - 1)].0,
            Item::DvmSrc => "DVM source",
            Item::CntSrc => "Count source",
            Item::CntClear => "Clear counter",
            Item::ChLabel(_) => "Name",
            Item::MathExprI => "Formula",
            Item::MathExprEdit => "Edit formula",
            Item::LogOn => "Logging",
            Item::LogIvl => "Interval",
            Item::LogInfo => "File",
            Item::ChBias(_) => "Zero bias",
            Item::ChBiasZero(_) => "Zero this input",
            Item::ChSkew(_) => "Deskew",
            Item::HFine => "Fine time base",
            Item::HExpandI => "Expand around",
            Item::RefColor => "Slot color",
            Item::RefLabel => "Slot name",
            Item::RefSel => "Slot",
            Item::RefVpd => "Slot scale",
            Item::RefOffset => "Slot offset",
            Item::RefExport => "Export slot",
            Item::RefImport => "Import slot",
            Item::RefLoad => "Load file",
            Item::MaskSave => "Save mask",
            Item::MaskLoad => "Load mask",
            Item::MathLinA => "A",
            Item::MathLinB => "B",
            Item::MeasAdd(k) => k.name(),
            Item::MeasCfg(_) => "Set",
            Item::MeasClear => "Clear all",
            Item::MeasRemove(_) => "Remove",
            Item::CatSourceC => "Source C",
            Item::CatSourceD => "Source D",
            _ => return None,
        })
    }

    /// Labels of the cursor, display, zone, system, network and key rows; "" for the rest.
    fn label_system(it: Item) -> &'static str {
        match it {
            Item::CurMode => "Mode",
            Item::CurSrcB => "Cursor B source",
            Item::CurA => "Cursor A",
            Item::CurB => "Cursor B",
            Item::CurC => "Y1",
            Item::CurD => "Y2",
            Item::ClearDisp => "Clear",
            Item::Hdmi => "HDMI",
            Item::TextSize => "Text size",
            Item::ZoneSel => "Zone",
            Item::ZoneOn => "Enable",
            Item::ZoneSrc => "Source",
            Item::ZoneMode => "Condition",
            Item::ZoneBox => "Draw box",
            Item::ZoneShape => "Draw shape",
            Item::ZoneClear => "Delete",
            Item::ZInput => "Z input",
            Item::Histogram => "Histogram",
            Item::About => "About OOS",
            Item::SysInfo => "System info",
            Item::Shot => "Screenshot",
            Item::AuxOut => "AUX out",
            Item::Stock => "Back to stock",
            Item::Update => "Update",
            Item::BootMode => "Boot mode",
            Item::Overclock => "Overclock",
            Item::ViewExit => "Exit",
            Item::ViewSettings => "Settings",
            Item::SrcOn => "Switch on source",
            Item::WlanInfo => "WLAN details",
            Item::WlanScan => "Find WLAN networks",
            Item::WlanForget => "Forget WLAN network",
            Item::NetEth => "Ethernet",
            Item::NetMac => "MAC",
            Item::NetPorts => "Other access",
            Item::NetWlan => "WLAN",
            Item::WlanNet => "Networks",
            Item::WlanForgetNet => "Forget",
            Item::UpdateRollback => "Roll back update",
            Item::QuickAct => "Quick key",
            // drawn with the option text of the picker
            Item::PickOpt(_) => "",
            Item::RunStop => "Run/Stop",
            Item::SingleAct => "Single",
            Item::AutosetAct => "Autoset",
            Item::DefaultAct => "Default",
            Item::SlopeAct => "Slope",
            Item::ForceAct => "Force",
            Item::NavHist => "History",
            Item::NavBack => "Older",
            Item::NavFwd => "Newer",
            // drawn with the action (see draw_menu)
            Item::QuickRun => "Quick",
            Item::LockAct => "Touch lock",
            Item::PowerAct => "Power off",
            // analysers: ana_label
            _ => "",
        }
    }

    pub(super) fn value(&self, it: Item) -> String {
        if it == Item::CatSourceC {
            return self.wave_name(self.cat_c);
        }
        if it == Item::CatSourceD {
            return self.wave_name(self.cat_d);
        }
        if let Some(v) = PARTS.iter().find_map(|p| p.value.and_then(|f| f(self, it))) {
            return v;
        }
        self.value_inputs(it)
            .or_else(|| self.value_measure(it))
            .or_else(|| self.value_decode(it))
            .unwrap_or_else(|| self.value_system(it))
    }

    /// Values of the channel, horizontal, trigger and acquire rows.
    fn value_inputs(&self, it: Item) -> Option<String> {
        let m = &self.m;
        let onoff = |b: bool| if b { "On".to_string() } else { "Off".to_string() };
        Some(match it {
            Item::ChCoupling(c) => m.ch[c].coupling.name().into(),
            Item::ChProbe(c) => format!("{}x", m.ch[c].probe),
            Item::ChScale(c) => si_short(m.ch[c].volts_per_div() as f64, m.unit(c)),
            Item::ChOffset(c) => si(m.ch[c].offset as f64, m.unit(c)),
            Item::ChBias(c) => si(m.ch[c].bias as f64, m.unit(c)),
            Item::ChSkew(c) => si(m.ch[c].skew as f64, "s"),
            Item::HFine => onoff(m.fine_t),
            Item::HExpandI => format!("{:?}", m.h_expand),
            Item::RefColor => {
                self.refs[self.ref_sel].as_ref().map_or("-", |r| REF_COLORS[r.color.min(REF_COLORS.len() - 1)].0).into()
            }
            Item::HScale => si_short(m.time_per_div() as f64, "s"),
            Item::HDelay => si(m.delay as f64, "s"),
            Item::TrigSrc => self.trigger_source_label(),
            Item::TrigLevel => si(m.trig_level as f64, m.unit(m.trig_src)),
            Item::TrigType => m.trig_type.name().into(),
            Item::SearchOn => {
                if self.search_on {
                    "On".into()
                } else {
                    "Off".into()
                }
            }
            Item::TrigLevel2 => si(m.trig_level2 as f64, m.unit(m.trig_src)),
            Item::TrigSrc2 => Self::trigger_wave_label(m.trigger_wave2()),
            Item::TrigWhen => match m.trig_when {
                WhenKind::Off => "Off".into(),
                WhenKind::Gt => ">".into(),
                WhenKind::Lt => "<".into(),
                WhenKind::In => "Inside".into(),
                WhenKind::Out => "Outside".into(),
            },
            Item::VidLine => format!("{}", m.video_line),
            Item::Pat(c) => match m.pat[c] {
                Bit::H => "H".into(),
                Bit::L => "L".into(),
                Bit::X => "X".into(),
                Bit::R => "Rise".into(),
                Bit::F => "Fall".into(),
            },
            Item::PatLevel => si(m.pat_levels[m.trig_src] as f64, "V"),
            Item::NthN => format!("{}", m.nth_n),
            Item::AcqSeg => {
                if m.segments == 0 {
                    "Off".into()
                } else {
                    self.seg_count().to_string()
                }
            }
            Item::AcqSegIvl => seg_ivl_name(m.seg_ivl),
            Item::AcqDepth => mdepths()[m.mdepth].0.into(),
            Item::AcqAlias => {
                self.alias_above().map_or(String::new(), |f| format!("above {}, Peak shows it", si_short(f, "Hz")))
            }
            _ => return None,
        })
    }

    /// Values of the measure, cursor, logger, slot, display, zone, math operator and view rows.
    fn value_measure(&self, it: Item) -> Option<String> {
        let m = &self.m;
        let onoff = |b: bool| if b { "On".to_string() } else { "Off".to_string() };
        Some(match it {
            Item::MeasSrc => self.wave_name(self.meas_src),
            Item::MeasCopyTo => "Pick".into(),
            Item::MeasSrcB => self.wave_name(self.meas_src_b),
            Item::CurSrcB => self.wave_name(self.cursor_b_source()),
            Item::MathExprI => {
                if m.math_expr[m.math_sel].is_empty() {
                    "none".to_string()
                } else {
                    m.math_expr[m.math_sel].clone()
                }
            }
            Item::LogOn => onoff(self.log.file.is_some()),
            Item::LogIvl => si_short(logger::LOG_STEPS[m.log_ivl.min(logger::LOG_STEPS.len() - 1)], "s"),
            Item::LogInfo => self.log_info(),
            Item::CurC => self.cursor_values().get(2).map_or(format!("{:.0} %", m.cur_c * 100.0), |v| v.1.clone()),
            Item::CurD => self.cursor_values().get(3).map_or(format!("{:.0} %", m.cur_d * 100.0), |v| v.1.clone()),
            Item::RefSel => format!("R{}", self.ref_sel + 1),
            Item::RefVpd => {
                self.refs[self.ref_sel].as_ref().map_or("-".into(), |r| format!("{}/div", si_short(r.vpd as f64, "V")))
            }
            Item::RefOffset => self.refs[self.ref_sel].as_ref().map_or("-".into(), |r| si_short(r.offset as f64, "V")),
            Item::DvmSrc => format!("CH{}", m.dvm_src + 1),
            Item::CntSrc => format!("CH{}", m.cnt_src + 1),
            Item::MathLinA => format!("{:.3}", m.math_lin.0),
            Item::MathLinB => format!("{:.3}", m.math_lin.1),
            Item::CurMode => format!("{:?}", m.cursor),
            // where the cursor is, as the read-out under the plot says it; the percentage only without a record
            Item::CurA => self.cursor_values().first().map_or(format!("{:.0} %", m.cur_a * 100.0), |v| v.1.clone()),
            Item::CurB => self
                .cursor_values()
                .get(if m.cursor == CursorMode::Track { 2 } else { 1 })
                .map_or(format!("{:.0} %", m.cur_b * 100.0), |v| v.1.clone()),
            Item::Hdmi => format!("{:?}", m.hdmi),
            Item::TextSize => m.text_size.name().into(),
            Item::ZoneSel => format!("Z{}", self.zone_sel + 1),
            Item::ZoneOn => onoff(m.zones[self.zone_sel].on),
            Item::ZoneSrc => format!("CH{}", m.zones[self.zone_sel].src + 1),
            Item::ZoneMode => {
                if m.zones[self.zone_sel].inside {
                    "Intersect".into()
                } else {
                    "Not intersect".into()
                }
            }
            Item::ZInput => m.z_src.map_or("Off".to_string(), |c| format!("CH{}", c + 1)),
            Item::Histogram => format!("{:?}", m.hist),
            Item::AuxOut => m.aux_out.name().into(),
            Item::MathOpI => m.math.name().into(),
            Item::MathFam => self.math_fam.name().into(),
            Item::StoreDest => self.store_dest.name().into(),
            Item::MaskInvert => onoff(m.mask_inv),
            Item::MeasStats => onoff(self.stats_on),
            Item::MeasLabels => onoff(self.m.meas_labels),
            Item::StripDock => self.m.strip_dock.label().into(),
            Item::Zoom => onoff(m.zoom),
            Item::AutoRoll => onoff(m.auto_roll),
            _ => return None,
        })
    }

    /// Values of the decoder, pane, trend, bus trigger, zoom and XY rows.
    fn value_decode(&self, it: Item) -> Option<String> {
        let m = &self.m;
        let onoff = |b: bool| if b { "On".to_string() } else { "Off".to_string() };
        Some(match it {
            Item::DecOn => onoff(m.dec_on),
            Item::DecSel => format!("D{}", m.dec_sel + 1),
            Item::PaneSel => tables::PANES[m.pane.min(tables::PANES.len() - 1)].into(),
            Item::TrendSel(k) => self.trend_options().get(m.trend_sel[k]).cloned().unwrap_or_else(|| "Auto".into()),
            Item::DecSrc => decode_source_name(m.dec_src),
            Item::DecBaud => format!("{}", crate::decode::BAUDS[m.dec_baud]),
            Item::DecCphyRate => crate::decode::cphy_rate_label(m.dec_cphy_rate),
            Item::DecCanRate => si(crate::decode::CAN_RATES[m.dec_canrate], "bit/s"),
            Item::TrigBusWhat => m.trig_bus.name().into(),
            Item::TrigBusVal => format!("0x{:X}", m.trig_bus_val),
            Item::SerRel
            | Item::SerMax
            | Item::SerMask
            | Item::UartPol
            | Item::UartStop
            | Item::UartErr
            | Item::SpiCs
            | Item::SpiTimeout
            | Item::I2cDir
            | Item::I2cAddrBits
            | Item::I2cBytes => self.serial_value(it),
            Item::DecCanH => {
                if m.dec_canh {
                    "CAN_H".into()
                } else {
                    "RX/TX".into()
                }
            }
            Item::DecUsbSpeed => USPEED_NAMES[m.dec_uspeed].into(),
            Item::DecSrc4 => m.dec_src4.map_or("None".into(), decode_source_name),
            Item::DecDiff => m.dec_diff.map_or("None".into(), decode_source_name),
            Item::DecThr => si(m.dec_thr as f64, "V"),
            Item::DecFmt => {
                if m.dec_ascii {
                    "ASCII".into()
                } else {
                    "Hex".into()
                }
            }
            Item::DecProto => m.dec_proto.name().into(),
            Item::DecOpt(k) => m
                .dec_proto
                .module()
                .and_then(|p| p.options().get(k))
                .map_or(String::new(), |o| o.choices[m.dec_opts()[k]].into()),
            Item::DecSrc2 => decode_source_name(m.dec_src2),
            Item::DecCs => m.dec_cs.map_or("None".into(), decode_source_name),
            Item::DecMode => format!("{}", m.dec_mode),
            Item::DecBits => format!("{}", m.dec_bits),
            Item::DecParity => match m.dec_parity {
                oos_dsp::decode::Parity::None => "None".into(),
                oos_dsp::decode::Parity::Even => "Even".into(),
                oos_dsp::decode::Parity::Odd => "Odd".into(),
            },
            Item::ZScale => si_short(tscales()[m.zscale] as f64, "s"),
            Item::ZDelay => si(m.zdelay as f64, "s"),
            Item::XyMode => {
                if self.xy {
                    "XY".into()
                } else {
                    "YT".into()
                }
            }
            _ => return None,
        })
    }

    /// Values of the system, network, reference, math, storage and mask rows; empty for the rest.
    fn value_system(&self, it: Item) -> String {
        let m = &self.m;
        let onoff = |b: bool| if b { "On".to_string() } else { "Off".to_string() };
        match it {
            Item::BootMode => self.boot_mode().into(),
            Item::Overclock => overclock::label(self.clock_next),
            Item::NetEth | Item::NetMac | Item::NetPorts | Item::NetWlan => self.net_value(it),
            Item::RefSrc => format!("CH{}", self.ref_src + 1),
            Item::RefShow(i) => match &self.refs[i] {
                None => "empty".into(),
                Some(r) => {
                    let src = if r.label.is_empty() { format!("CH{}", r.src + 1) } else { r.label.clone() };
                    format!("{} {}", src, if r.on { "on" } else { "off" })
                }
            },
            Item::MathA => src_name(m.math_a),
            Item::MathB => src_name(m.math_b),
            Item::MathSel => format!("M{}", m.math_sel + 1),
            Item::MathScale => self.math_scale_text(),
            Item::MathOffset => format!("{:.2} div", m.math_offset),
            Item::StoreSlot => format!("{}  (setup{}.txt)", self.store_slot, self.store_slot),
            Item::MaskOn => onoff(m.mask_on),
            Item::MaskSrc => format!("CH{}", m.mask_src + 1),
            Item::MaskX => format!("{:.2} div", m.mask_x),
            Item::MaskY => format!("{:.2} div", m.mask_y),
            Item::MaskAction => {
                if m.mask_stop {
                    "Stop".into()
                } else {
                    "None".into()
                }
            }
            Item::QuickAct => m.quick.name().into(),
            _ => String::new(),
        }
    }
}
