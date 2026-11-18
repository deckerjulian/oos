// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The item lists of the menus: which rows a menu has (`Ui::items`, `Ui::menu_items`), the rows of the decoder and the
//! trigger menus for the selected protocol and trigger type, the start menu's groups and the channels a page reads
//! that are switched off. A new row goes into the list of its menu here, its text into `menu_text.rs` and its action
//! into `menu_actions.rs`.

use super::*;

impl Ui {
    pub(super) fn items(&self, menu: MenuId) -> Vec<Item> {
        if let Some(v) = self.features_items(menu) {
            return v;
        }
        let mut v = self.menu_items(menu);
        if !self.off_sources(menu).is_empty() {
            v.push(Item::SrcOn);
        }
        v
    }

    /// Channels a page reads that are switched off: the page has nothing to work with, and says so with a button.
    /// A ref is stored memory, so a switched-off channel does not blank it.
    pub(super) fn off_sources(&self, menu: MenuId) -> Vec<usize> {
        let m = &self.m;
        let ch = WaveId::Ch;
        let want: Vec<WaveId> = match menu {
            MenuId::Power if matches!(m.pw_mode, PowerMode::Ripple | PowerMode::Rail) => vec![m.pw_v],
            MenuId::Power => vec![m.pw_v, m.pw_i],
            MenuId::Spectrum => vec![m.spec_src],
            MenuId::Eye => vec![m.eye_src],
            MenuId::Bode if m.bode_gen == bode::FREE => {
                [Some(m.bode_out), m.bode_sw_in.checked_sub(1).map(ch), Some(ch(m.bode_sync))]
                    .into_iter()
                    .flatten()
                    .collect()
            }
            MenuId::Bode => vec![m.bode_in, m.bode_out],
            MenuId::Magnetic => vec![m.mag_v, m.mag_i],
            MenuId::Inverter if m.inv_page == InvPage::Dq0 => {
                let mut v = vec![m.inv_a, m.inv_b, m.inv_c];
                if m.inv_ang == InvAngle::Analog {
                    v.push(m.inv_an);
                }
                v
            }
            MenuId::Inverter => {
                let mut v = match m.inv_ang {
                    InvAngle::Analog => vec![m.inv_an],
                    InvAngle::Quad | InvAngle::Hall | InvAngle::Off => Vec::new(),
                    InvAngle::Phase => vec![m.inv_a, m.inv_b, m.inv_c],
                };
                v.extend(m.inv_tq);
                v.extend(m.inv_ev);
                v.extend(m.inv_ei);
                v
            }
            MenuId::Dpt => {
                let mut v = vec![m.dpt_v, m.dpt_i, m.dpt_g];
                if m.dpt_4th != DptFourth::Off {
                    v.push(m.dpt_4ch);
                }
                v
            }
            MenuId::BwTest => vec![ch(m.bw_src)],
            MenuId::Mask => vec![ch(m.mask_src)],
            MenuId::Dvm if self.offline => Vec::new(),
            MenuId::Dvm => {
                [m.dvm_on.then_some(ch(m.dvm_src)), m.cnt_on.then_some(ch(m.cnt_src))].into_iter().flatten().collect()
            }
            MenuId::Decode if !m.dec_on_la() => decode_wave(m.dec_src).into_iter().collect(),
            _ => Vec::new(),
        };
        let mut v: Vec<usize> = want
            .into_iter()
            .filter_map(|id| match id {
                WaveId::Ch(c) if c < n_ch() && !m.ch[c].on => Some(c),
                _ => None,
            })
            .collect();
        v.dedup();
        v
    }

    /// The rows of `menu`; a menu of more than one line has its own helper below, in this order.
    fn menu_items(&self, menu: MenuId) -> Vec<Item> {
        match menu {
            MenuId::Main => self.main_groups().into_iter().flat_map(|g| g.1).collect(),
            MenuId::Channel(c) => self.channel_items(c),
            MenuId::ChMore(c) => self.ch_more_items(c),
            MenuId::Horizontal => self.horizontal_items(),
            MenuId::Trigger => self.trigger_items(),
            // the Horizontal Menu key opens Acquire; time base and zoom one level down
            MenuId::Acquire => self.acquire_items(),
            MenuId::MeasPair => (0..tools::PAIRS.len()).map(Item::MeasPair).collect(),
            MenuId::Dvm => self.dvm_items(),
            // in the order of the groups of `measure_layout`
            MenuId::Measure => self.measure_items(),
            MenuId::MeasHold => vec![Item::AddMeas],
            MenuId::Decode => self.decode_items(),
            MenuId::Bus => self.bus_items(),
            MenuId::Logger => vec![Item::LogOn, Item::LogIvl, Item::LogInfo],
            MenuId::Ref => self.ref_items(),
            MenuId::MeasAdd => Vec::new(),
            MenuId::MeasSelected => self.measurement_catalog(),
            MenuId::Cursor => self.cursor_items(),
            MenuId::Display => self.display_items(),
            MenuId::Storage => self.storage_items(),
            MenuId::Spectrum => self.spectrum_items(),
            MenuId::Eye => self.eye_items(),
            MenuId::Power => self.power_items(),
            MenuId::Identify => self.identify_items(),
            MenuId::SelfCal => self.cal_items(),
            MenuId::Network => self.network_items(),
            MenuId::Features | MenuId::Decoders(_) => self.features_items(menu).unwrap_or_default(),
            MenuId::Remote => self.remote_items(),
            MenuId::Ssh => self.ssh_items(),
            MenuId::Time => self.time_items(),
            MenuId::Bode => self.bode_items(),
            MenuId::Sim => self.sim_items(),
            MenuId::SimAdvanced => self.sim_advanced_items(),
            MenuId::Magnetic => self.magnetic_items(),
            MenuId::Inverter => self.inverter_items(),
            MenuId::Dpt => self.dpt_items(),
            MenuId::Afg => self.afg_items(),
            MenuId::BwTest => self.bw_items(),
            MenuId::ViewMore => self.view_more_items(),
            MenuId::La => self.la_items(),
            MenuId::LaLines => self.la_line_items(),
            MenuId::LaPat => (0..16).map(Item::LaPat).collect(),
            MenuId::Windows => self.win_items(),
            MenuId::Zone => self.zone_items(),
            MenuId::Mask => self.mask_items(),
            MenuId::Utility => self.utility_items(),
            MenuId::Pick => {
                let n = self.pick.map_or(0, |p| self.options(p.it).len());
                (0..n).map(Item::PickOpt).collect()
            }
            MenuId::Math => self.math_items(),
        }
    }

    /// The channel menu of channel `c`.
    fn channel_items(&self, c: usize) -> Vec<Item> {
        vec![
            Item::ChOn(c),
            Item::ChScale(c),
            Item::ChOffset(c),
            Item::ChCoupling(c),
            Item::ChBw(c),
            Item::ChProbe(c),
            Item::ChInvert(c),
            Item::ChFine,
            Item::ChLabel(c),
            Item::Open(MenuId::ChMore(c)),
        ]
    }

    /// The second level of the channel menu of channel `c`.
    fn ch_more_items(&self, c: usize) -> Vec<Item> {
        vec![Item::ChUnit(c), Item::ChBias(c), Item::ChBiasZero(c), Item::ChSkew(c), Item::AutoDeskew, Item::ChExpand]
    }

    /// The time base menu, with the zoom rows while zoomed.
    fn horizontal_items(&self) -> Vec<Item> {
        let mut v = vec![
            Item::HScale,
            Item::HFine,
            Item::HExpandI,
            Item::HDelay,
            Item::HZero,
            Item::AutoDeskew,
            Item::ClearDeskew,
            Item::Zoom,
        ];
        if self.m.zoom {
            v.extend([Item::ZScale, Item::ZDelay]);
        }
        v.push(Item::AutoRoll);
        v
    }

    /// The acquire menu: mode, average, depth, segments, cores, draw path, alias.
    fn acquire_items(&self) -> Vec<Item> {
        let mut v = vec![Item::AcqMode];
        if self.m.acq == AcqMode::Average {
            v.push(Item::AcqAvg);
            if self.src.is_hw() {
                v.push(Item::AcqAvgHw);
            }
        }
        v.push(Item::AcqDepth);
        v.push(Item::AcqSeg);
        if self.m.segments > 0 {
            v.push(Item::AcqSegHw);
        }
        if self.seg_hw_on() {
            v.extend([Item::AcqSegIvl, Item::AcqSegTable]);
        }
        v.push(Item::AcqCores);
        v.push(Item::AcqDrawPath);
        if self.alias_above().is_some() {
            v.push(Item::AcqAlias);
        }
        v.push(Item::Open(MenuId::Horizontal));
        v
    }

    /// The voltmeter and counter menu.
    fn dvm_items(&self) -> Vec<Item> {
        vec![
            Item::DvmOn,
            Item::DvmSrc,
            Item::DvmMode,
            Item::CntOn,
            Item::CntSrc,
            Item::CntMode,
            Item::CntDigits,
            Item::CntClear,
        ]
    }

    /// The measurement menu.
    fn measure_items(&self) -> Vec<Item> {
        vec![
            Item::MeasSrc,
            Item::MeasSrcB,
            Item::MeasCopyTo,
            Item::MeasGate,
            Item::MeasRefs,
            Item::MeasStats,
            Item::MeasLabels,
            Item::StripDock,
            Item::MeasIdle,
            Item::MeasTsetup,
            Item::MeasThold,
            Item::MeasOutside,
            Item::MeasN,
            Item::MeasEdges,
            Item::Open(MenuId::MeasPair),
            Item::Open(MenuId::Dvm),
            Item::MeasClear,
        ]
    }

    /// The reference menu; Load only with its feature on.
    fn ref_items(&self) -> Vec<Item> {
        vec![
            Item::RefSrc,
            Item::RefSave,
            Item::RefClear,
            Item::RefSel,
            Item::RefVpd,
            Item::RefOffset,
            Item::RefColor,
            Item::RefLabel,
            Item::RefLoad,
            Item::RefExport,
            Item::RefImport,
        ]
        .into_iter()
        .filter(|&it| it != Item::RefLoad || self.feature_on("load"))
        .chain((0..NREF).map(Item::RefShow))
        .collect()
    }

    /// The cursor menu, with the rows of the cursor mode.
    fn cursor_items(&self) -> Vec<Item> {
        if self.m.cursor == CursorMode::Off {
            vec![Item::CurMode]
        } else if self.m.cursor == CursorMode::Xy {
            vec![Item::CurMode, Item::CurA, Item::CurB, Item::CurC, Item::CurD]
        } else if self.m.cursor == CursorMode::Track {
            vec![Item::CurMode, Item::CurA, Item::CurB, Item::CurSrcB]
        } else {
            vec![Item::CurMode, Item::CurA, Item::CurB]
        }
    }

    /// The display menu; the trend selectors while the trend pane is shown.
    fn display_items(&self) -> Vec<Item> {
        let mut v = vec![
            Item::Open(MenuId::Windows),
            Item::Views,
            Item::Persist,
            Item::ColorGrade,
            Item::Intensity,
            Item::GridStyle,
            Item::GridBright,
            Item::GridFrame,
            Item::Dots,
            Item::BottomBar,
            Item::ZInput,
            Item::Histogram,
            Item::XyMode,
            Item::Hdmi,
            Item::HdmiView,
            Item::Backlight,
            Item::TextSize,
            Item::ClearDisp,
        ];
        if self.m.pane == tables::PANE_TREND {
            v.splice(1..1, [Item::TrendSel(0), Item::TrendSel(1)]);
        }
        v
    }

    /// The storage menu.
    fn storage_items(&self) -> Vec<Item> {
        vec![
            Item::StoreDest,
            Item::StoreSlot,
            Item::StoreSaveSetup,
            Item::StoreRecall,
            Item::StoreCsv,
            Item::StoreBin,
            Item::StoreFfSave,
            Item::StoreFfLoad,
            Item::StorePng,
        ]
    }

    /// The spectrum menu.
    fn spectrum_items(&self) -> Vec<Item> {
        vec![
            Item::SpecOn,
            Item::SpecSrc,
            Item::SpecCenter,
            Item::SpecSpan,
            Item::SpecHarm,
            Item::SpecRbw,
            Item::SpecWin,
            Item::SpecTrace,
            Item::SpecRef,
            Item::SpecWf,
            Item::SpecDist,
            Item::SpecAcprW,
            Item::SpecAcprS,
            Item::SpecPeak,
            Item::SpecMargin,
            Item::SpecLimitSet,
            Item::SpecLimitClear,
        ]
    }

    /// The eye diagram menu.
    fn eye_items(&self) -> Vec<Item> {
        vec![
            Item::EyeAct,
            Item::EyeOn,
            Item::EyePlace,
            Item::EyeSrc,
            Item::EyeRate,
            Item::EyeBer,
            Item::EyeB,
            Item::EyeHp,
            Item::EyePnLo,
            Item::EyePnHi,
            Item::EyeNpj,
            Item::EyeRail,
            Item::EyeNotch,
            Item::EyeClear,
        ]
    }

    /// The power analysis menu, with the rows of its mode.
    fn power_items(&self) -> Vec<Item> {
        let mut v = vec![Item::PowOn, Item::PowV, Item::PowI, Item::PowMode];
        match self.m.pw_mode {
            PowerMode::Ripple => v.extend([Item::PowFline, Item::PowFsw, Item::PowVin]),
            PowerMode::Rail => v.extend([Item::PowRef, Item::PowVin, Item::PowClk]),
            _ => {}
        }
        v
    }

    /// The network menu.
    fn network_items(&self) -> Vec<Item> {
        vec![
            Item::NetEth,
            Item::NetRetry10,
            Item::NetMac,
            Item::Open(MenuId::Remote),
            Item::NetPorts,
            Item::NetWlan,
            Item::WlanScan,
            Item::WlanForget,
            Item::WlanInfo,
        ]
    }

    /// The zone trigger menu.
    fn zone_items(&self) -> Vec<Item> {
        vec![
            Item::ZoneSel,
            Item::ZoneOn,
            Item::ZoneSrc,
            Item::ZoneMode,
            Item::ZoneBox,
            Item::ZoneShape,
            Item::ZoneClear,
        ]
    }

    /// The mask test menu.
    fn mask_items(&self) -> Vec<Item> {
        vec![
            Item::MaskOn,
            Item::MaskSrc,
            Item::MaskInvert,
            Item::MaskX,
            Item::MaskY,
            Item::MaskAction,
            Item::MaskCreate,
            Item::MaskFromRef,
            Item::MaskReset,
            Item::MaskClear,
            Item::MaskSave,
            Item::MaskLoad,
        ]
    }

    /// The utility menu; Overclock only when it is switched on.
    fn utility_items(&self) -> Vec<Item> {
        let mut v = vec![
            Item::Offline,
            Item::QuickAct,
            Item::AuxOut,
            Item::BootMode,
            Item::Overclock,
            Item::Graphics,
            Item::Shot,
            Item::Open(MenuId::SelfCal),
            Item::Open(MenuId::Network),
            Item::Open(MenuId::Features),
            Item::Open(MenuId::Time),
            Item::SysInfo,
            Item::About,
            Item::Update,
            Item::UpdateRollback,
            Item::Stock,
        ];
        // overclocking loses samples at the full rate: only with `adc.overclock = on` (overclock.rs)
        if !overclock::shown(overclock::switch_on(), self.clock_next) {
            v.retain(|&i| i != Item::Overclock);
        }
        v
    }

    /// The math menu, with the rows of the operation.
    fn math_items(&self) -> Vec<Item> {
        if self.m.math == MathOp::Off {
            vec![Item::MathSel, Item::MathFam, Item::MathOpI]
        } else if self.m.math == MathOp::Expr {
            vec![
                Item::MathSel,
                Item::MathFam,
                Item::MathOpI,
                Item::MathExprI,
                Item::MathExprEdit,
                Item::MathScale,
                Item::MathOffset,
                Item::MathAuto,
            ]
        } else {
            let mut v = vec![Item::MathSel, Item::MathFam, Item::MathOpI, Item::MathA];
            if self.m.math.two_sources() {
                v.push(Item::MathB);
            }
            if self.m.math == MathOp::Lin {
                v.extend([Item::MathLinA, Item::MathLinB]);
            }
            if self.m.math.has_fc() {
                v.push(Item::MathFc);
            }
            if self.m.math.has_bw() {
                v.push(Item::MathBw);
            }
            v.extend([Item::MathScale, Item::MathOffset]);
            if self.m.math == MathOp::Fft {
                v.push(Item::FftWin);
            }
            if self.m.math != MathOp::Off {
                v.push(Item::MathAuto);
            }
            v
        }
    }

    /// The start menu (Menu button, Multi 1 with no menu open): every front-panel key function and every menu, in
    /// groups (label, entries). The logic analyser and the generator only on models that have them; the history
    /// steps appear while a stored record is shown.
    pub(super) fn main_groups(&self) -> Vec<(&'static str, Vec<Item>)> {
        use Item::*;
        let mut disp = vec![Open(MenuId::Display), Open(MenuId::Horizontal), NavHist];
        if self.hist_pos.is_some() {
            disp.extend([NavBack, NavFwd]);
        }
        // System > Features: what is left out is not offered
        let ana: Vec<Item> = analysers::ANALYSERS
            .iter()
            .flat_map(|a| a.start())
            .filter(|it| !matches!(it, Open(m) if self.menu_left_out(*m)))
            .collect();
        // the generator page only on models with one (S models), the LA only with the port
        let mut ins = Vec::new();
        if oos_hal::model::platform().caps.has_gen() {
            ins.push(Open(MenuId::Afg));
        }
        if la::available() {
            ins.push(Open(MenuId::La));
        }
        ins.push(Open(MenuId::Dvm));
        ins.push(Open(MenuId::Logger));
        let mut g = vec![
            ("Acquisition", vec![RunStop, SingleAct, AutosetAct, ClearDisp, DefaultAct, Open(MenuId::Acquire)]),
            ("Trigger", vec![Open(MenuId::Trigger), SlopeAct, ForceAct, Open(MenuId::Zone)]),
            (
                "Analysis",
                [
                    Open(MenuId::Measure),
                    Open(MenuId::Cursor),
                    Open(MenuId::Math),
                    Open(MenuId::Ref),
                    Open(MenuId::Sim),
                    Open(MenuId::Decode),
                    Open(MenuId::Bus),
                    Open(MenuId::Mask),
                ]
                .into_iter()
                .filter(|it| !matches!(it, Open(m) if self.menu_left_out(*m)))
                .collect(),
            ),
            ("Analysers", ana),
            ("Instruments", ins),
            ("Display", disp),
            ("System", vec![Open(MenuId::Utility), Open(MenuId::Storage), Shot, QuickRun, LockAct, PowerAct]),
        ];
        g.retain(|x| !x.1.is_empty());
        g
    }

    /// Decode menu rows for the selected protocol.
    pub(super) fn decode_items(&self) -> Vec<Item> {
        use Item::*;
        let p = self.m.dec_proto;
        let r = p.roles();
        let on_la = self.m.dec_on_la();
        let mut v = vec![DecSel, DecOn, DecProto];
        if self.m.la_on() && p != Proto::CPHY {
            v.push(DecIn);
        }
        v.push(DecSrc);
        if r[1].is_some() {
            v.push(DecSrc2);
        }
        if r[2].is_some() {
            v.push(DecCs);
        }
        if r[3].is_some() {
            v.push(DecSrc4);
        }
        if p.diff_ok() && !on_la {
            v.push(DecDiff);
        }
        // the shared rows the protocol names, then its own options
        use crate::protocols::Row;
        for r in p.module().map_or(&[][..], |x| x.shared()) {
            v.push(match r {
                Row::Baud => DecBaud,
                Row::Bits => DecBits,
                Row::Parity => DecParity,
                Row::SpiMode => DecMode,
                Row::CanRate => DecCanRate,
                Row::CanH => DecCanH,
                Row::UsbSpeed => DecUsbSpeed,
                Row::CphyRate => DecCphyRate,
                Row::Fmt => DecFmt,
            });
        }
        v.extend((0..p.module().map_or(0, |x| x.options().len())).map(DecOpt));
        // LA lines: the probe's comparators set the levels (LA page)
        if !on_la {
            v.push(DecThr);
        }
        // what it decoded, as a table, and as a file
        v.extend([DecTable, DecExport]);
        v
    }

    /// Trigger menu rows for the selected type.
    pub(super) fn trigger_items(&self) -> Vec<Item> {
        use crate::model::TrigType as TT;
        use Item::*;
        let m = &self.m;
        let when = |v: &mut Vec<Item>| {
            v.push(TrigWhen);
            if m.trig_when != WhenKind::Off {
                v.push(TrigT1);
            }
            if matches!(m.trig_when, WhenKind::In | WhenKind::Out) {
                v.push(TrigT2);
            }
        };
        let mut v = vec![Item::TrigType];
        match m.trig_type {
            // an LA line has no level of its own (LA page: pod threshold)
            TT::Edge if m.trig_line().is_some() => v.extend([TrigSrc, TrigSlope]),
            TT::Edge => v.extend([TrigSrc, TrigSlope, TrigLevel, Trig50, TrigCoup]),
            // the protocol and its sources come from the Decode menu
            TT::Bus => {
                v.extend([DecProto, TrigBusWhat]);
                if matches!(m.trig_bus, BusWhat::Id | BusWhat::Data) {
                    v.push(TrigBusVal);
                }
                v.extend(self.serial_items());
            }
            TT::Pulse => {
                v.extend([TrigSrc, TrigPol]);
                when(&mut v);
                v.push(TrigLevel);
            }
            TT::Slope => {
                v.extend([TrigSrc, TrigSlope]);
                when(&mut v);
                v.extend([TrigLevel, TrigLevel2]);
            }
            TT::Video => {
                v.extend([TrigSrc, VidStd, VidSync]);
                if m.video_sync == VSync::Line {
                    v.push(VidLine);
                }
                v.push(TrigLevel);
            }
            TT::Pattern => {
                v.extend([Pat(0), Pat(1), Pat(2), Pat(3), TrigSrc, PatLevel]);
                if m.la_on() {
                    v.push(Open(MenuId::LaPat));
                }
            }
            TT::Duration => {
                v.extend([Pat(0), Pat(1), Pat(2), Pat(3)]);
                when(&mut v);
                v.extend([TrigSrc, PatLevel]);
                if m.la_on() {
                    v.push(Open(MenuId::LaPat));
                }
            }
            TT::Timeout => v.extend([TrigSrc, TrigSlope, TrigT1, TrigLevel]),
            TT::Runt => {
                v.extend([TrigSrc, TrigPol]);
                when(&mut v);
                v.extend([TrigLevel, TrigLevel2]);
            }
            TT::Window => {
                v.extend([TrigSrc, WinModeI]);
                if m.win_mode == WinMode::Inside {
                    when(&mut v);
                }
                v.extend([TrigLevel, TrigLevel2]);
            }
            TT::Delay => {
                v.extend([TrigSrc, TrigSlope, TrigSrc2, TrigSlope2]);
                when(&mut v);
                v.extend([TrigLevel, TrigLevel2]);
            }
            TT::SetupHold => v.extend([TrigSrc, TrigSrc2, TrigSlope2, Setup, Hold, TrigLevel, TrigLevel2]),
            TT::NthEdge => v.extend([TrigSrc, TrigSlope, NthIdle, NthN, TrigLevel]),
        }
        // the analog comparators: not for a protocol or an LA line
        if m.trig_type != TT::Bus && m.trig_line().is_none() {
            v.push(TrigNoise);
        }
        v.push(TrigAutoNoise);
        v.push(TrigMode);
        v.push(SearchOn);
        if self.search_on {
            v.extend([SearchTable, SearchSave]);
        }
        v.push(Open(MenuId::Zone));
        v.push(TrigHoldoff);
        v
    }
}
