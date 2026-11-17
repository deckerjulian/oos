// SPDX-License-Identifier: GPL-2.0-or-later
//! SCPI coverage extensions, tier 2: reference, mask and SIM actions, storage and the whole-setup round trip, Studio
//! zoom and per-reference horizontal, the bandwidth test, the data logger, and the setup tables of the analysers
//! (spectrum, eye, power, magnetic, inverter, double pulse, Bode, SIM). See `scpiext.rs`.

use super::scpiext::*;
use super::*;
use crate::scpi::{e as en, kw, num, parse_bool, parse_wave};
use super::workspace::Trace;

pub(super) const T2: &[&str] = &[
    "REFerence:EXPort", "REFerence:CLEar", "REFerence:LOOK",
    "REFerence:HORizontal:MODE", "REFerence:HORizontal:SCALe", "REFerence:HORizontal:OFFSet", "REFerence:HORizontal:FIT",
    "MASK:CREate:REF", "MASK:CLEar", "MASK:INVert", "MASK:OUTPut", "MASK:SAVE", "MASK:LOAD",
    "SIM:DEFault", "SIM:ADDRef", "SIM:LOAD", "SIM:LIVE",
    "*SAV", "*RCL", "SYSTem:SETup", "SAVE:SETup", "LOAD:SETup", "SAVE:DESTination", "SAVE:SLOT",
    "SAVE:WAVeform", "SAVE:IMAGe", "SAVE:FFRame", "LOAD:FFRame",
    "TIMebase:DELay:SOURce", "TIMebase:DELay:VZOom", "TIMebase:DELay:VZOom:RESet", "TIMebase:DELay:FIT", "TIMebase:DELay:CENTer",
    "BANDwidth:STATe", "BANDwidth:SOURce", "BANDwidth:FREQuency", "BANDwidth:BINS",
    "SYSTem:LOG", "SYSTem:LOG:INTerval", "SYSTem:LOG:FILE", "SYSTem:LOG:INFO",
    "BODE:RUN",
    "EYE:RATE", "POWer:REFerence", "INVerter:ENCoder:INDex",
];

/// How a setup header reaches its menu item.
#[derive(Clone, Copy)]
pub(super) enum K {
    /// the field of `Model` the value item changes, with its limits
    F64(fn(&mut Model) -> &mut f64, f64, f64),
    F32(fn(&mut Model) -> &mut f32, f32, f32),
    U32(fn(&mut Model) -> &mut u32, u32, u32),
    U8(fn(&mut Model) -> &mut u8, u8, u8),
    Usize(fn(&mut Model) -> &mut usize, usize, usize),
    /// a choice item: its options are the SCPI values (sources by CHANn / Rn)
    Sel(Item),
}

pub(super) const SETUP: &[(&str, K)] = &[
    ("SPECtrum:SOURce", K::Sel(Item::SpecSrc)), ("SPECtrum:CENTer", K::F64(|m| &mut m.spec_center, 0.0, 1e12)),
    ("SPECtrum:SPAN", K::F64(|m| &mut m.spec_span, 0.0, 1e12)), ("SPECtrum:RBW", K::F64(|m| &mut m.spec_rbw, 0.0, 1e12)),
    ("SPECtrum:WINDow", K::Sel(Item::SpecWin)), ("SPECtrum:TRACe", K::Sel(Item::SpecTrace)),
    ("SPECtrum:RLEVel", K::F32(|m| &mut m.spec_ref, super::ranges::SPEC_REF_DBV.0, super::ranges::SPEC_REF_DBV.1)), ("SPECtrum:WATerfall", K::Sel(Item::SpecWf)),
    ("SPECtrum:DISTortion", K::Sel(Item::SpecDist)), ("SPECtrum:PEAK:HARMonics", K::Sel(Item::SpecHarm)),
    ("SPECtrum:ACPR:WIDTh", K::F64(|m| &mut m.spec_acpr_w, 1.0, 1e12)), ("SPECtrum:ACPR:SPACing", K::F64(|m| &mut m.spec_acpr_s, 1.0, 1e12)),
    ("EYE:SOURce", K::Sel(Item::EyeSrc)), ("EYE:CLOCk", K::Sel(Item::EyeB)),
    ("EYE:BER", K::F64(|m| &mut m.eye_ber, 1e-18, 1e-1)), ("EYE:PN:LOW", K::F64(|m| &mut m.eye_pn_lo, 1.0, 1e12)),
    ("EYE:PN:HIGH", K::F64(|m| &mut m.eye_pn_hi, 1.0, 1e12)), ("EYE:NPJ", K::Sel(Item::EyeNpj)), ("EYE:RAIL", K::Sel(Item::EyeRail)),
    ("EYE:NOTCh", K::F64(|m| &mut m.eye_notch, 0.0, 1e12)), ("EYE:HPASs", K::Sel(Item::EyeHp)),
    ("EYE:PLL:BWIDth", K::F64(|m| &mut m.eye_pll_bw, 0.0, super::ranges::EYE_PLL.1)), ("EYE:PLACe", K::Sel(Item::EyePlace)),
    ("POWer:VOLTage:SOURce", K::Sel(Item::PowV)), ("POWer:CURRent:SOURce", K::Sel(Item::PowI)), ("POWer:PAGE", K::Sel(Item::PowMode)),
    ("POWer:FLINe", K::F64(|m| &mut m.pw_fline, 0.0, 1e9)), ("POWer:FSWitch", K::F64(|m| &mut m.pw_fsw, 0.0, 1e9)),
    ("POWer:VIN:SOURce", K::Sel(Item::PowVin)), ("POWer:CLOCk:SOURce", K::Sel(Item::PowClk)),
    ("MAGnetic:VOLTage:SOURce", K::Sel(Item::MagV)), ("MAGnetic:CURRent:SOURce", K::Sel(Item::MagI)),
    ("MAGnetic:TURNs", K::F64(|m| &mut m.mag_n, super::ranges::MAG_TURNS.0 as f64, super::ranges::MAG_TURNS.1 as f64)), ("MAGnetic:AREA", K::F64(|m| &mut m.mag_ae, 0.0, 1e3)),
    ("MAGnetic:LENGth", K::F64(|m| &mut m.mag_le, 0.0, 1e3)), ("MAGnetic:RCU", K::F64(|m| &mut m.mag_rcu, 0.0, 1e6)),
    ("MAGnetic:PLOT", K::Sel(Item::MagPlot)), ("MAGnetic:CURSor", K::Usize(|m| &mut m.mag_cur, 0, 1_000_000)),
    ("INVerter:PAGE", K::Sel(Item::InvPage)), ("INVerter:PHASe1:SOURce", K::Sel(Item::InvA)), ("INVerter:PHASe2:SOURce", K::Sel(Item::InvB)),
    ("INVerter:PHASe3:SOURce", K::Sel(Item::InvC)), ("INVerter:ANGLe:SOURce", K::Sel(Item::InvAng)),
    ("INVerter:POLEs", K::U32(|m| &mut m.inv_poles, 1, 32)), ("INVerter:ANALog:SOURce", K::Sel(Item::InvAn)),
    ("INVerter:ANALog:SCALe", K::F64(|m| &mut m.inv_scale, 1e-3, 1e3)), ("INVerter:ANALog:DEGRees", K::F64(|m| &mut m.inv_degv, 1e-3, 1e4)),
    ("INVerter:ANGLe:OFFSet", K::F64(|m| &mut m.inv_off, -3600.0, 3600.0)), ("INVerter:QUANtity", K::Sel(Item::InvQty)),
    ("INVerter:TORQue:SOURce", K::Sel(Item::InvTq)), ("INVerter:BUS:VOLTage:SOURce", K::Sel(Item::InvEv)),
    ("INVerter:BUS:CURRent:SOURce", K::Sel(Item::InvEi)), ("INVerter:ELECtrical", K::Sel(Item::InvElec)),
    ("INVerter:ENCoder:CPR", K::F64(|m| &mut m.inv_cpr, 1.0, 100_000.0)), ("INVerter:ENCoder:SWAP", K::Sel(Item::InvSwap)),
    ("INVerter:ENCoder:A", K::U8(|m| &mut m.inv_qa, 0, 15)), ("INVerter:ENCoder:B", K::U8(|m| &mut m.inv_qb, 0, 15)),
    ("INVerter:HALL1:LINE", K::U8(|m| &mut m.inv_ha, 0, 15)), ("INVerter:HALL2:LINE", K::U8(|m| &mut m.inv_hb, 0, 15)),
    ("INVerter:HALL3:LINE", K::U8(|m| &mut m.inv_hc, 0, 15)),
    ("DPT:VDS:SOURce", K::Sel(Item::DptV)), ("DPT:ID:SOURce", K::Sel(Item::DptI)), ("DPT:VGS:SOURce", K::Sel(Item::DptG)),
    ("DPT:FOURth", K::Sel(Item::DptFourth)), ("DPT:FOURth:SOURce", K::Sel(Item::Dpt4)),
    ("DPT:GATE", K::F64(|m| &mut m.dpt_pct, 1.0, 49.0)), ("DPT:ZOOM", K::Sel(Item::DptZoom)),
    ("BODE:STARt", K::F64(|m| &mut m.bode_start, 1e-3, 1e9)), ("BODE:STOP", K::F64(|m| &mut m.bode_stop, 1e-3, 1e9)),
    ("BODE:POINts", K::Usize(|m| &mut m.bode_points, 5, 200)), ("BODE:AMPLitude", K::F64(|m| &mut m.bode_amp, super::ranges::BODE_AMP.0, super::ranges::BODE_AMP.1)),
    ("BODE:INPut", K::Sel(Item::BodeIn)), ("BODE:OUTPut", K::Sel(Item::BodeOut)),
    ("BODE:SWEep:TIME", K::F64(|m| &mut m.bode_sw_time, 1e-3, 500.0)), ("BODE:SWEep:LOG", K::Sel(Item::BodeSwLog)),
    ("BODE:SYNC:SOURce", K::Sel(Item::BodeSync)), ("BODE:SYNC:EDGE", K::Sel(Item::BodeSyncEdge)),
    ("BODE:SYNC:LEVel", K::F32(|m| &mut m.bode_sync_level, -1e3, 1e3)), ("BODE:REFLevel", K::F64(|m| &mut m.bode_ref, -300.0, 300.0)),
    ("SIM:RATE", K::F64(|m| &mut m.sim_rate, 1e8, 64e9)), ("SIM:LEVel:HIGH", K::F64(|m| &mut m.sim_vhigh, -2.0, 2.0)),
    ("SIM:LEVel:LOW", K::F64(|m| &mut m.sim_vlow, -2.0, 2.0)), ("SIM:RISE", K::F64(|m| &mut m.sim_rise, 5e-12, 2e-9)),
    ("SIM:BWLimit", K::F64(|m| &mut m.sim_bw, 10e6, 50e9)), ("SIM:DEEMphasis", K::F64(|m| &mut m.sim_deemph, 0.0, 12.0)),
    ("SIM:NOISe", K::F64(|m| &mut m.sim_noise, 0.0, 50e-3)), ("SIM:JITTer:RANDom", K::F64(|m| &mut m.sim_rj, 0.0, 50e-12)),
    ("SIM:JITTer:PERiodic:AMPLitude", K::F64(|m| &mut m.sim_pj, 0.0, 50e-12)),
    ("SIM:JITTer:PERiodic:FREQuency", K::F64(|m| &mut m.sim_pjhz, 1e5, 50e6)),
];

/// The setup headers as a list of names (for `resolve`).
pub(super) const T2B: &[&str] = &[];

fn norm(s: &str) -> String { s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase() }

impl Ui {
    /// The option of a choice item that `want` names: `CHANnel2` for CH2, case, spaces and dashes do not matter.
    fn sel_option(&self, it: Item, want: &str) -> Option<String> {
        let w = want.trim().trim_matches('"');
        let opts = self.options(it);
        if let Some(id) = parse_wave(w) {
            let n = self.wave_name(id);
            if let Some(o) = opts.iter().find(|o| **o == n) { return Some(o.clone()); }
        }
        let n = norm(w);
        opts.into_iter().find(|o| norm(o) == n || (n == "NONE" && norm(o) == "OFF"))
    }

    /// A choice item as a SCPI value: set by option text, query by the shown value (On/Off items answer 1 and 0).
    pub(super) fn sel(&mut self, it: Item, q: bool, arg: &str) -> Reply {
        let opts = self.options(it);
        let onoff = opts.len() == 2 && opts[0] == "Off" && opts[1] == "On";
        if q {
            let v = self.value(it);
            return t(if onoff { b01(v == "On").to_string() } else { norm_value(&v) });
        }
        let want = if onoff { parse_bool(arg).map(|b| if b { "On" } else { "Off" }.to_string()) } else { self.sel_option(it, arg) };
        match want.and_then(|w| self.sel_option(it, &w)) {
            Some(o) => { self.select_option(it, &o); DONE }
            None => Err(bad(arg)),
        }
    }

    fn ana_changed(&mut self) {
        self.azr.spec_avg = None;
        if let Some(w) = &mut self.azr.wf { w.clear(); }
        self.analyzer_update();
    }

    fn setup_cmd(&mut self, k: K, q: bool, arg: &str) -> Reply {
        let r = match k {
            K::Sel(it) => return self.sel(it, q, arg),
            K::F64(f, lo, hi) => numd(q, arg, f(&mut self.m), lo, hi),
            K::F32(f, lo, hi) => numf(q, arg, f(&mut self.m), lo, hi),
            K::U32(f, lo, hi) => intf(q, arg, f(&mut self.m), lo, hi),
            K::U8(f, lo, hi) => { let mut v = *f(&mut self.m) as u32; let r = intf(q, arg, &mut v, lo as u32, hi as u32); if !q && r.is_ok() { *f(&mut self.m) = v as u8; } r }
            K::Usize(f, lo, hi) => { let mut v = *f(&mut self.m) as u32; let r = intf(q, arg, &mut v, lo as u32, hi as u32); if !q && r.is_ok() { *f(&mut self.m) = v as usize; } r }
        };
        if !q && r.is_ok() { self.ana_changed(); }
        r
    }

    fn out_path(&self, arg: &str) -> std::path::PathBuf {
        let p = std::path::PathBuf::from(text_arg(arg));
        if p.is_absolute() { p } else { self.save_dir().join(p) }
    }

    pub(super) fn ext_b(&mut self, key: &str, idx: Option<usize>, q: bool, arg: &str) -> Option<Reply> {
        let _ = idx;
        if let Some((_, k)) = SETUP.iter().find(|(h, _)| *h == key) { return Some(self.setup_cmd(*k, q, arg)); }
        let action = |q: bool| if q { Err(undefined(key)) } else { DONE };
        Some(match key {
            // ---- reference
            "REFerence:EXPort" => {
                if q { return Some(Err(undefined(key))); }
                let path = (!arg.trim().is_empty()).then(|| self.out_path(arg));
                self.ref_export_to(path);
                DONE
            }
            "REFerence:CLEar" => {
                if q { return Some(Err(undefined(key))); }
                if arg.trim().is_empty() { self.ref_clear_all(); self.mark_changed(); return Some(DONE); }
                match crate::scpi::suffix(arg.trim()) {
                    (k, Some(n)) if (k.is_empty() || kw(k, "R") || kw(k, "REF")) && (1..=NREF).contains(&n) => { self.ref_clear_slot(n - 1); self.mark_changed(); DONE }
                    _ => match arg.trim().parse::<usize>() {
                        Ok(n) if (1..=NREF).contains(&n) => { self.ref_clear_slot(n - 1); self.mark_changed(); DONE }
                        _ => Err(bad(arg)),
                    },
                }
            }
            "REFerence:LOOK" => {
                const L: [(reflook::RefLook, &str); 3] = [(reflook::RefLook::Live, "LIVE"), (reflook::RefLook::TakeOver, "TAKEover"), (reflook::RefLook::Waves, "OFFLine")];
                let mut cur = self.look;
                let r = pick(q, arg, &mut cur, &L);
                if !q && r.is_ok() { self.set_look(cur); }
                r
            }
            "REFerence:HORizontal:MODE" => {
                let slot = self.ref_sel;
                if q { return Some(t(if self.studio.sz.ref_h.contains_key(&slot) { "OWN" } else { "FOLL" })); }
                if kw(arg.trim(), "OWN") { self.studio_ref_set_own(slot, true); DONE }
                else if kw(arg.trim(), "FOLLow") { self.studio_ref_set_own(slot, false); DONE }
                else { Err(bad(arg)) }
            }
            "REFerence:HORizontal:SCALe" | "REFerence:HORizontal:OFFSet" => {
                let slot = self.ref_sel;
                let scale = key.ends_with("SCALe");
                if q {
                    let (tdiv, pos) = self.studio.sz.ref_h.get(&slot).copied().unwrap_or((self.m.time_per_div(), self.m.delay));
                    return Some(t(en(if scale { tdiv } else { pos } as f64)));
                }
                let Some(v) = num(arg).filter(|v| if scale { *v > 0.0 && *v <= 1e4 } else { v.abs() <= 1e6 }) else { return Some(Err(bad(arg))) };
                self.studio_ref_set_own(slot, true);
                if let Some(h) = self.studio.sz.ref_h.get_mut(&slot) { if scale { h.0 = v } else { h.1 = v } }
                self.dirty_map = true;
                DONE
            }
            "REFerence:HORizontal:FIT" => {
                if q { return Some(Err(undefined(key))); }
                if self.studio_ref_fit(self.ref_sel) { DONE } else { Err(conflict("Studio lanes and a reference with samples are needed")) }
            }
            // ---- mask
            "MASK:CREate:REF" => {
                if q { return Some(Err(undefined(key))); }
                let Some(WaveId::Ref(i)) = parse_wave(arg) else { return Some(Err(bad(arg))) };
                if self.refs[i].is_none() { return Some(Err(conflict("that reference is empty"))); }
                self.ref_sel = i;
                self.activate(Item::MaskFromRef);
                DONE
            }
            "MASK:CLEar" => { self.activate(Item::MaskClear); action(q) }
            "MASK:INVert" => return Some(self.sel(Item::MaskInvert, q, arg)),
            "MASK:OUTPut" => {
                if q { return Some(t(if self.m.mask_stop { "STOP" } else { "CONT" })); }
                if kw(arg.trim(), "STOP") { self.select_option(Item::MaskAction, "Stop"); DONE }
                else if kw(arg.trim(), "CONTinue") || kw(arg.trim(), "NONE") { self.select_option(Item::MaskAction, "None"); DONE }
                else { Err(bad(arg)) }
            }
            "MASK:SAVE" => {
                if q { return Some(Err(undefined(key))); }
                if self.mask_ref.is_none() { return Some(Err(conflict("no mask yet"))); }
                let path = (!arg.trim().is_empty()).then(|| self.out_path(arg));
                self.mask_save_to(path);
                DONE
            }
            "MASK:LOAD" => {
                if q { return Some(Err(undefined(key))); }
                let path = (!arg.trim().is_empty()).then(|| self.out_path(arg));
                self.mask_load_from(path);
                if self.mask_ref.is_none() { Err(conflict("no mask loaded")) } else { DONE }
            }
            // ---- SIM
            "SIM:DEFault" => { self.sim_activate(Item::SimDefaults); action(q) }
            "SIM:ADDRef" => { self.sim_activate(Item::SimAddRef); action(q) }
            "SIM:LIVE" => { self.sim_activate(Item::SimLive); action(q) }
            "SIM:LOAD" => {
                if q { return Some(Err(undefined(key))); }
                self.ref_sel = self.m.sim_slot;
                match self.ref_load_named(arg) { Ok(()) => { self.mark_changed(); DONE } Err(e) => Err(bad(&e)) }
            }
            // ---- storage
            "*SAV" | "*RCL" => {
                let Some(n) = arg.trim().parse::<u32>().ok().filter(|n| (1..=9).contains(n)) else { return Some(Err(bad(arg))) };
                self.store_slot = n;
                self.activate(if key == "*SAV" { Item::StoreSaveSetup } else { Item::StoreRecall });
                DONE
            }
            "SAVE:SETup" | "LOAD:SETup" => {
                if q { return Some(Err(undefined(key))); }
                let save = key.starts_with("SAVE");
                if arg.trim().is_empty() { self.activate(if save { Item::StoreSaveSetup } else { Item::StoreRecall }); return Some(DONE); }
                let p = self.out_path(arg);
                if save {
                    let r = crate::store::write(&p, self.setup_text_full().as_bytes());
                    self.toast_saved(&p, r);
                    DONE
                } else {
                    match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| self.apply_setup_text(&t)) {
                        Ok(()) => { self.mark_changed(); DONE }
                        Err(e) => Err(bad(&e)),
                    }
                }
            }
            "SYSTem:SETup" => {
                if q { return Some(Ok(Some(crate::scpi::block(self.setup_text_full().as_bytes())))); }
                let Some(text) = setup_block(arg) else { return Some(Err(bad("setup block"))) };
                match self.apply_setup_text(&text) { Ok(()) => { self.mark_changed(); DONE } Err(e) => Err(bad(&e)) }
            }
            "SAVE:DESTination" => {
                const D: [(crate::store::StoreDest, &str); 3] = [(crate::store::StoreDest::Auto, "AUTO"), (crate::store::StoreDest::Stick, "USB"), (crate::store::StoreDest::Local, "DESKtop")];
                pick(q, arg, &mut self.store_dest, &D)
            }
            "SAVE:SLOT" => intf(q, arg, &mut self.store_slot, 1, 9),
            "SAVE:WAVeform" => {
                if q { return Some(Err(undefined(key))); }
                let mut it = arg.split(',');
                let path = it.next().map(str::trim).filter(|p| !p.is_empty()).map(|p| self.out_path(p));
                let csv = match it.next().map(str::trim) { None | Some("") => true, Some(f) if kw(f, "CSV") => true, Some(f) if kw(f, "BIN") => false, Some(f) => return Some(Err(bad(f))) };
                if self.last[0].is_none() { return Some(Err(conflict("no waveform to save"))); }
                self.store_wave(csv, path);
                DONE
            }
            "SAVE:IMAGe" => {
                if q { return Some(Err(undefined(key))); }
                if arg.trim().is_empty() { self.activate(Item::StorePng); } else { let p = self.out_path(arg); self.close_menu(); self.shot_request = Some(p.to_string_lossy().into_owned()); }
                DONE
            }
            "SAVE:FFRame" => { self.activate(Item::StoreFfSave); action(q) }
            "LOAD:FFRame" => { self.activate(Item::StoreFfLoad); action(q) }
            // ---- Studio zoom
            "TIMebase:DELay:SOURce" => {
                if q { return Some(t(self.studio.sz.lane.map_or("ALL".to_string(), |l| match l { Trace::Channel(c) => format!("CHAN{}", c + 1), Trace::Reference(i) => format!("R{}", i + 1), Trace::Math(k) => format!("MATH{}", k + 1) }))); }
                let a = arg.trim();
                let lane = if kw(a, "ALL") { None } else if let Some(id) = parse_wave(a) {
                    match id { WaveId::Ch(c) if c < n_ch() => Some(Trace::Channel(c)), WaveId::Ref(i) => Some(Trace::Reference(i)), _ => return Some(Err(bad(arg))) }
                } else if let (k, Some(n)) = crate::scpi::suffix(a) {
                    if kw(k, "MATH") && (1..=NMATH).contains(&n) { Some(Trace::Math(n - 1)) } else { return Some(Err(bad(arg))) }
                } else { return Some(Err(bad(arg))) };
                self.studio.sz.lane = lane;
                if !self.m.zoom { self.set_zoom(true); }
                self.dirty_map = true;
                DONE
            }
            "TIMebase:DELay:VZOom" => {
                if q { return Some(t(en(self.zoom_vfactor as f64))); }
                let Some(v) = num(arg).filter(|v| *v >= 1.0 && *v <= 1024.0) else { return Some(Err(bad(arg))) };
                if !self.m.zoom { self.set_zoom(true); }
                let want = v.log2().round() as i32;
                let have = self.zoom_vfactor.log2().round() as i32;
                for _ in 0..(want - have).abs() { self.studio_zoom_do(if want > have { studio::ZoomRow::VPlus } else { studio::ZoomRow::VMinus }); }
                DONE
            }
            "TIMebase:DELay:VZOom:RESet" => { self.studio_zoom_do(studio::ZoomRow::ResetY); action(q) }
            "TIMebase:DELay:FIT" => { self.studio_zoom_do(studio::ZoomRow::Fit); action(q) }
            "TIMebase:DELay:CENTer" => { self.studio_zoom_do(studio::ZoomRow::Center); action(q) }
            // ---- bandwidth test
            "BANDwidth:STATe" => return Some(self.sel(Item::BwOn, q, arg)),
            "BANDwidth:SOURce" => {
                if q { return Some(t(format!("CHAN{}", self.m.bw_src + 1))); }
                match parse_wave(arg) { Some(WaveId::Ch(c)) if c < n_ch() => { self.m.bw_src = c; self.azr.bw = None; DONE } _ => Err(bad(arg)) }
            }
            "BANDwidth:FREQuency" => {
                if q { return Some(t(en(self.m.bw_hint))); }
                let Some(v) = num(arg).filter(|v| *v >= 0.0) else { return Some(Err(bad(arg))) };
                let i = bwtest::HINTS.iter().enumerate().min_by(|a, b| (a.1 - v as f64).abs().total_cmp(&(b.1 - v as f64).abs())).map_or(0, |x| x.0);
                self.m.bw_hint = bwtest::HINTS[i];
                self.azr.bw = None;
                DONE
            }
            "BANDwidth:BINS" => {
                if q { return Some(t(self.m.bw_bins.to_string())); }
                let Some(v) = num(arg).filter(|v| *v >= 1.0) else { return Some(Err(bad(arg))) };
                let i = bwtest::BINS.iter().enumerate().min_by(|a, b| (*a.1 as f32 - v).abs().total_cmp(&(*b.1 as f32 - v).abs())).map_or(0, |x| x.0);
                self.m.bw_bins = bwtest::BINS[i];
                self.azr.bw = None;
                DONE
            }
            // ---- data logger
            "SYSTem:LOG" => {
                if q { return Some(t(b01(self.log.file.is_some()))); }
                match parse_bool(arg) { Some(on) => { self.log_set(on); DONE } None => Err(bad(arg)) }
            }
            "SYSTem:LOG:INTerval" => {
                if q { return Some(t(en(logger::LOG_STEPS[self.m.log_ivl.min(logger::LOG_STEPS.len() - 1)]))); }
                let Some(v) = num(arg).filter(|v| *v > 0.0) else { return Some(Err(bad(arg))) };
                self.m.log_ivl = crate::scpi::nearest(&logger::LOG_STEPS.map(|s| s as f32), v);
                DONE
            }
            "SYSTem:LOG:FILE" => if q { t(self.log.file.as_ref().map_or(String::new(), |p| p.to_string_lossy().into_owned())) } else { Err(undefined(key)) },
            "SYSTem:LOG:INFO" => if q { t(self.log_info()) } else { Err(undefined(key)) },
            "BODE:RUN" => {
                if q { return Some(t(b01(self.bode.running))); }
                match parse_bool(arg).or_else(|| arg.trim().is_empty().then_some(true)) { Some(on) => { self.bode_run(on); DONE } None => Err(bad(arg)) }
            }
            "EYE:RATE" => {
                if q { return Some(t(en(self.m.eye_rate))); }
                let Some(v) = arg.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0 && *v <= 1e12) else { return Some(Err(bad(arg))) };
                (self.m.eye_rate, self.m.eye_from_record) = (v, false);
                self.ana_changed();
                DONE
            }
            "INVerter:ENCoder:INDex" => {
                if q { return Some(t(self.m.inv_qi.map_or("OFF".to_string(), |n| n.to_string()))); }
                if kw(arg.trim(), "OFF") { self.m.inv_qi = None; self.ana_changed(); return Some(DONE); }
                match arg.trim().parse::<u8>().ok().filter(|n| *n < 16) { Some(n) => { self.m.inv_qi = Some(n); self.ana_changed(); DONE } None => Err(bad(arg)) }
            }
            "POWer:REFerence" => {
                if q { return Some(t(en(self.m.pw_ref.unwrap_or(0.0)))); }
                let Some(v) = arg.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0 && *v <= 1e6) else { return Some(Err(bad(arg))) };
                self.m.pw_ref = if v == 0.0 { None } else { Some(v) };
                self.ana_changed();
                DONE
            }
            _ => return None,
        })
    }
}

/// The setup text out of the argument of `:SYSTem:SETup`: a definite-length block, or the text itself.
fn setup_block(arg: &str) -> Option<String> {
    let a = arg.trim_start();
    let Some(r) = a.strip_prefix('#') else { return (!a.trim().is_empty()).then(|| a.to_string()) };
    let n = r.chars().next()?.to_digit(10).filter(|&n| n >= 1)? as usize;
    let len: usize = r.get(1..1 + n)?.parse().ok()?;
    // the interpreter trims the argument: a block that ends in the newline of the text may come one short
    let data = r.get(1 + n..)?;
    (data.len() + 2 >= len && data.len() <= len).then(|| data.to_string())
}

fn norm_value(v: &str) -> String {
    v.trim().to_ascii_uppercase().replace(' ', "_")
}

