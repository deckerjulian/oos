// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What a menu item does: turning the knob on it or tapping its arrows (`Ui::adjust`, the segment items in
//! `Ui::seg_adjust`), pressing it (`Ui::activate`), and switching the decoder protocol and the trigger type with the
//! settings they bring along. A new item gets its action here and its text in `menu_text.rs`.

use super::*;

impl Ui {
    /// Switches the decoder protocol. The demo takes the sources, channels and trigger that match its bus traffic
    /// (`Protocol::setup`). On every other source (the hardware, Live, Replay of a recorded file) one setting must not
    /// change another (the forum, eTobey): the channels and the trigger stay as they are, the sources the protocols
    /// share stay, and a role the old protocol did not have takes a channel that is on and no other role uses, or none.
    /// The protocol's own parameters (threshold, baud, bits ...) are set on both.
    pub(super) fn set_dec_proto(&mut self, p: Proto) {
        let Proto::Module(i) = p;
        let s = crate::protocols::get(i).setup();
        if Self::decoder_uses_refs(&self.m) {
            self.m.dec_proto = p;
            if s.la_off {
                (self.m.dec_thr, self.m.dec_la_in) = (s.thr, false);
            }
            return;
        }
        let demo = self.src.is_demo();
        let m = &mut self.m;
        let changed = m.dec_proto != p;
        let old = m.dec_proto.roles();
        m.dec_proto = p;
        // its threshold and shared rows; its own options from their defaults when it is newly chosen
        m.dec_thr = s.thr;
        let r = p.roles();
        if demo {
            // its demo channels and LA lines; roles the protocol doesn't have keep their sources
            m.dec_la = s.la;
            m.dec_src = s.src[0].unwrap_or(2);
            if r[2].is_some() {
                m.dec_cs = s.src[2];
            }
            if r[3].is_some() {
                m.dec_src4 = s.src[3];
            }
            if let Some(c) = s.src[1] {
                m.dec_src2 = c;
            }
        } else {
            // a new role: a channel that is on and that no other role of the protocol uses
            let mut used = vec![m.dec_src];
            if r[1].is_some() && old[1].is_some() && m.dec_src2 != m.dec_src {
                used.push(m.dec_src2);
            }
            used.extend(
                [(r[2], old[2], m.dec_cs), (r[3], old[3], m.dec_src4)]
                    .into_iter()
                    .filter_map(|(n, o, c)| n.and(o).and(c)),
            );
            let mut free = |m: &Model| {
                let c = (0..NCH).find(|&c| m.ch[c].on && !used.contains(&c));
                used.extend(c);
                c
            };
            if r[1].is_some() && (old[1].is_none() || m.dec_src2 == m.dec_src) {
                if let Some(c) = free(m) {
                    m.dec_src2 = c;
                }
            }
            if r[2].is_some() && (old[2].is_none() || m.dec_cs.is_none()) {
                m.dec_cs = free(m);
            }
            if r[3].is_some() && (old[3].is_none() || m.dec_src4.is_none()) {
                m.dec_src4 = free(m);
            }
        }
        if s.la_off {
            m.dec_la_in = false;
        }
        use crate::protocols::Row;
        for &(r, v) in s.rows {
            match r {
                Row::Baud => m.dec_baud = v as usize,
                Row::Bits => m.dec_bits = v,
                Row::Parity => {
                    m.dec_parity =
                        [oos_dsp::decode::Parity::None, oos_dsp::decode::Parity::Even, oos_dsp::decode::Parity::Odd]
                            [(v as usize).min(2)]
                }
                Row::SpiMode => m.dec_mode = v as u8,
                Row::CanRate => m.dec_canrate = v as usize,
                Row::CanH => m.dec_canh = v != 0,
                Row::UsbSpeed => m.dec_uspeed = v as usize,
                Row::CphyRate | Row::Fmt => {}
            }
        }
        if changed {
            m.dec_opt = crate::protocols::opt_default(i);
            // a module with a view of its own: open it, when no other pane is open
            if m.pane == 0
                && crate::protocols::get(i).view(&[], &crate::protocols::opt_fit(i, m.dec_opt), &mut None).is_some()
            {
                m.pane = tables::PANE_BUS;
            }
        }
        if !demo {
            self.dirty_map = true;
            return;
        }
        // the demo: every source the protocol uses; the traffic of some is on all its channels or on every channel
        m.ch[m.dec_src].on = true;
        if r[1].is_some() {
            m.ch[m.dec_src2].on = true;
        }
        if let (Some(_), Some(c)) = (r[2], m.dec_cs) {
            m.ch[c].on = true;
        }
        if let (Some(_), Some(c)) = (r[3], m.dec_src4) {
            m.ch[c].on = true;
        }
        if s.show {
            for c in s.src.into_iter().flatten().filter(|&c| c < NCH) {
                m.ch[c].on = true;
            }
        }
        if s.show_all {
            for channel in &mut m.ch {
                channel.on = true;
            }
        }
        // the demo: a trigger that holds its traffic still, on the channel of the role
        if let Some(t) = s.trig.filter(|_| demo) {
            let c = [Some(m.dec_src), Some(m.dec_src2), m.dec_cs, m.dec_src4][t.role.min(3)];
            if let Some(c) = c.filter(|&c| c < NCH) {
                (m.trig_src, m.slope, m.trig_level) = (c, t.slope, t.level);
                if t.idle > 0.0 {
                    (m.trig_type, m.nth_idle, m.nth_n) = (TrigType::NthEdge, t.idle, 1);
                } else {
                    m.trig_type = TrigType::Edge;
                }
            }
        }
        self.dirty_map = true;
    }

    /// Switches the trigger type and sets starting values that trigger on the demo signals.
    pub(super) fn set_trig_type(&mut self, t: TrigType) {
        let was_bus = self.m.trig_type == TrigType::Bus;
        // leaving Bus: off again what entering it switched on (the decoder kept the live picture raw, its demo sources
        // stayed on); a type set elsewhere since leaves nothing to undo
        let undo = if t != TrigType::Bus { self.bus_auto.take().filter(|_| was_bus) } else { None };
        if let Some((chans, dec)) = undo {
            for c in (0..NCH).filter(|&c| chans[c]) {
                self.m.ch[c].on = false;
            }
            if dec {
                self.m.dec_on = false;
            }
        }
        if t == TrigType::Bus && Self::decoder_uses_refs(&self.m) {
            if let Some(source) = decode_wave(self.m.dec_src) {
                self.set_trigger_source(source, false);
                self.m.trig_type = t;
                self.m.dec_on = true;
                self.replay.reset_search();
                return;
            }
        }
        if self.set_reference_trigger_type(t) {
            return;
        }
        if t == TrigType::Bus {
            // the decoder defines the protocol: switch it on with its demo sources and a value its traffic contains
            let before: [bool; NCH] = std::array::from_fn(|c| self.m.ch[c].on);
            let dec_before = self.m.dec_on;
            self.m.dec_on = true;
            let p = self.m.dec_proto;
            self.set_dec_proto(p);
            // Bus chosen again keeps what the first choice switched on
            if !was_bus {
                self.bus_auto = Some((std::array::from_fn(|c| self.m.ch[c].on && !before[c]), !dec_before));
            }
            let m = &mut self.m;
            m.trig_type = t;
            // a value the protocol's demo traffic contains
            let Proto::Module(i) = p;
            let (what, val) = crate::protocols::get(i).info().trigger_default;
            (m.trig_bus, m.trig_bus_val, m.ser) = (what, val as u64, SerialTrig::default());
            self.dirty_map = true;
            return;
        }
        // the presets below suit the demo signals (each type on a channel that shows it). Every other source (the
        // hardware, a live or replayed record) keeps what the user set: the channels, source, level and slope, and
        // also the width, the pattern, the lower level and the rest that several types share (the forum, eTobey: one
        // setting must not change another). A SIM session triggers on its reference: set_reference_trigger_type
        let demo = self.src.is_demo();
        let m = &mut self.m;
        m.trig_type = t;
        let on = |c: usize, m: &mut Model| m.ch[c].on = true;
        match t {
            _ if !demo => {}
            TrigType::Bus => {} // handled above
            TrigType::Edge => {
                m.trig_src = 0;
                m.trig_level = 1.5;
            }
            TrigType::Pulse => {
                on(3, m);
                (m.trig_src, m.trig_pol, m.trig_when, m.trig_t1, m.trig_level) =
                    (3, Pol::Pos, WhenKind::Lt, 30e-6, 1.65);
            }
            TrigType::Slope => {
                (m.trig_src, m.slope, m.trig_when, m.trig_t1, m.trig_level, m.trig_level2) =
                    (0, Slope::Rise, WhenKind::Gt, 1e-6, 2.0, 0.5);
            }
            TrigType::Video => {
                on(1, m);
                (m.trig_src, m.trig_level) = (1, 0.15);
                m.ch[1].vscale = VSCALES.iter().position(|&v| (v - 0.2).abs() < 1e-6).unwrap_or(8);
                m.ch[1].offset = -0.5;
            }
            TrigType::Pattern => {
                on(3, m);
                m.pat = std::array::from_fn(|i| {
                    if i == 0 {
                        Bit::H
                    } else if i == NCH - 1 {
                        Bit::R
                    } else {
                        Bit::X
                    }
                });
                m.trig_src = 0;
            }
            TrigType::Duration => {
                m.pat = std::array::from_fn(|i| if i == 0 { Bit::H } else { Bit::X });
                (m.trig_src, m.trig_when, m.trig_t1) = (0, WhenKind::Gt, 200e-6);
            }
            TrigType::Timeout => {
                (m.trig_src, m.slope, m.trig_t1, m.trig_level) = (0, Slope::Rise, 1.2e-3, 1.5);
            }
            TrigType::Runt => {
                (m.trig_src, m.trig_pol, m.trig_when, m.trig_level, m.trig_level2) =
                    (0, Pol::Pos, WhenKind::Off, 2.0, 0.5);
            }
            TrigType::Window => {
                on(1, m);
                (m.trig_src, m.win_mode, m.trig_level, m.trig_level2) = (1, WinMode::Enter, 0.5, -0.5);
            }
            TrigType::Delay => {
                on(3, m);
                (
                    m.trig_src,
                    m.slope,
                    m.trig_src2,
                    m.trig_slope2,
                    m.trig_when,
                    m.trig_t1,
                    m.trig_level,
                    m.trig_level2,
                ) = (0, Slope::Rise, 3, Slope::Rise, WhenKind::Gt, 40e-6, 1.5, 1.65);
            }
            TrigType::SetupHold => {
                on(2, m);
                on(3, m);
                (m.trig_src, m.trig_src2, m.trig_slope2, m.setup, m.hold, m.trig_level, m.trig_level2) =
                    (2, 3, Slope::Rise, 2e-6, 2e-6, 1.65, 1.65);
            }
            TrigType::NthEdge => {
                on(2, m);
                (m.trig_src, m.slope, m.nth_idle, m.nth_n, m.trig_level) = (2, Slope::Fall, 50e-6, 1, 1.65);
            }
        }
        if m.trig_type != TrigType::Runt && m.trig_when == WhenKind::Off {
            m.trig_when = WhenKind::Lt;
        }
        if !demo {
            // the second source of Delay and Setup/Hold on a channel that is on (PR #8, W4R)
            let src = m.trig_src;
            if matches!(m.trig_type, TrigType::Delay | TrigType::SetupHold)
                && (m.trig_src2 == src || !m.ch[m.trig_src2.min(NCH - 1)].on)
            {
                if let Some(c) = (0..NCH).find(|&k| k != src && m.ch[k].on) {
                    m.trig_src2 = c;
                }
            }
        }
        // the lower level the type finds (Delay's level B, say) not above the shared upper one
        m.order_trigger_levels();
        self.dirty_map = true;
    }

    /// The segment items of the Acquire menu; false for any other item.
    fn seg_adjust(&mut self, it: Item, d: i32) -> bool {
        match it {
            Item::AcqSeg => {
                let o = self.seg_options();
                let cur = if self.m.segments == 0 { 0 } else { self.seg_count() };
                let i = o.iter().position(|&n| n >= cur).unwrap_or(0);
                self.m.segments = o[cycle(i, o.len(), d)];
            }
            Item::AcqSegIvl => {
                let i = SEG_IVLS.iter().position(|&x| x >= self.m.seg_ivl).unwrap_or(0);
                self.m.seg_ivl = SEG_IVLS[cycle(i, SEG_IVLS.len(), d)];
            }
            _ => return false,
        }
        true
    }

    pub(super) fn adjust(&mut self, it: Item, d: i32) {
        if matches!(it, Item::CatSourceC | Item::CatSourceD) {
            let current = if it == Item::CatSourceC { self.cat.c } else { self.cat.d };
            let next = self.step_wave(current, d);
            if it == Item::CatSourceC {
                self.cat.c = next;
            } else {
                self.cat.d = next;
            }
            return;
        }
        if matches!(it, Item::MathA | Item::MathB) {
            let sources = self.math_source_ids(self.m.math_sel);
            let current = if it == Item::MathA { self.m.math_a } else { self.m.math_b };
            if !sources.is_empty() {
                let i = sources.iter().position(|&s| s == current).unwrap_or(0);
                let source = sources[(i as i32 + d).rem_euclid(sources.len() as i32) as usize];
                if it == Item::MathA {
                    self.m.math_a = source;
                } else {
                    self.m.math_b = source;
                }
                self.settings_changed();
            }
            return;
        }
        if PARTS.iter().any(|p| p.adjust.is_some_and(|f| f(self, it, d))) {
            return;
        }
        if matches!(it, Item::TrigSrc | Item::TrigSrc2) {
            let secondary = it == Item::TrigSrc2;
            let sources = self.trigger_sources();
            let selected = if secondary { self.m.trigger_wave2() } else { self.m.trigger_wave() };
            if !sources.is_empty() {
                let pos = sources.iter().position(|&id| id == selected).unwrap_or(0);
                let id = sources[(pos as i32 + d).rem_euclid(sources.len() as i32) as usize];
                self.set_trigger_source(id, secondary);
            }
            return;
        }
        if matches!(it, Item::TrigLevel | Item::TrigLevel2) && self.m.trig_ref.is_some() {
            let secondary = it == Item::TrigLevel2 && matches!(self.m.trig_type, TrigType::Delay | TrigType::SetupHold);
            let step = d as f32 * self.trigger_vpd(secondary) / 25.0;
            if it == Item::TrigLevel {
                self.m.trig_level += step;
            } else {
                self.m.trig_level2 += step;
            }
            self.replay.reset_search();
            return;
        }
        if self.seg_adjust(it, d) {
            return;
        }
        let m = &mut self.m;
        match it {
            Item::ChCoupling(c) => {
                let all = board().couplings();
                let i = all.iter().position(|&x| x == m.ch[c].coupling).unwrap_or(0);
                m.ch[c].coupling = all[cycle(i, all.len(), d)];
            }
            Item::ChProbe(c) => {
                let all = [1.0, 10.0, 100.0];
                let i = all.iter().position(|&x| x == m.ch[c].probe).unwrap_or(0);
                m.ch[c].probe = all[cycle(i, 3, d)];
            }
            Item::ChScale(c) => {
                let old = m.ch[c].volts_per_div();
                if m.fine_v {
                    // row by row through the calibration table: each scale has its own record
                    let rows = board().vscale_steps_nv();
                    let now = VSCALES[m.ch[c].vscale] as f64 * m.ch[c].vfine as f64 * 1e9 * 0.999999;
                    let at = rows.iter().position(|&k| k as f64 >= now).unwrap_or(rows.len() - 1) as i32;
                    let want = (rows[(at + d).clamp(0, rows.len() as i32 - 1) as usize] as f64 * 1e-9) as f32;
                    let i = VSCALES.iter().rposition(|&s| s <= want * 1.000001).unwrap_or(0);
                    let f = want / VSCALES[i];
                    (m.ch[c].vscale, m.ch[c].vfine) = (i, if (f - 1.0).abs() < 1e-6 { 1.0 } else { f });
                } else {
                    m.ch[c].vscale = step_index(m.ch[c].vscale, VSCALES.len(), d);
                    m.ch[c].vfine = 1.0;
                }
                // around ground: the zero line keeps its place on the screen
                if m.v_expand_gnd {
                    m.ch[c].offset *= m.ch[c].volts_per_div() / old;
                }
            }
            Item::ChBias(c) => m.ch[c].bias += 0.01 * m.ch[c].volts_per_div() * d as f32,
            Item::ChSkew(c) => m.ch[c].skew = (m.ch[c].skew + 600e-12 * d as f32).clamp(-SKEW_MAX, SKEW_MAX),
            Item::HFine => m.fine_t = !m.fine_t,
            Item::HExpandI => {
                let all = [HExpand::Center, HExpand::Left, HExpand::Right, HExpand::Trigger];
                let i = all.iter().position(|&x| x == m.h_expand).unwrap_or(0);
                m.h_expand = all[cycle(i, all.len(), d)];
            }
            Item::LogOn => {
                if self.log.file.is_some() {
                    self.log_stop("Log ended");
                } else {
                    self.log_start();
                }
            }
            Item::LogIvl => m.log_ivl = step_index(m.log_ivl, logger::LOG_STEPS.len(), d),
            Item::ChOffset(c) => m.ch[c].offset += d as f32 * m.ch[c].volts_per_div() / 25.0,
            Item::HScale => {
                let old = m.time_per_div();
                if m.fine_t {
                    (m.tscale, m.tfine) = fine_time(old, d);
                } else {
                    m.tscale = step_index(m.tscale, tscales().len(), d);
                    m.tfine = 1.0;
                }
                keep_expand_point(m, old);
            }
            Item::HDelay => m.delay += d as f32 * m.time_per_div() / 25.0,
            Item::TrigSrc => {
                m.trig_src = src_step(m.trig_src, NCH, d);
            }
            Item::TrigLevel => {
                m.trig_level += d as f32 * m.ch[m.trig_src].volts_per_div() / 25.0;
                // the upper level of a two-level type stops at the lower one, as Bode's start stops at its stop
                if m.trig_type.two_level() {
                    m.trig_level = m.trig_level.max(m.trig_level2);
                }
            }
            Item::TrigType => {
                let i = TRIG_TYPES.iter().position(|&x| x == m.trig_type).unwrap_or(0);
                let t = TRIG_TYPES[cycle(i, TRIG_TYPES.len(), d)];
                self.set_trig_type(t);
            }
            Item::TrigLevel2 => {
                let c =
                    if matches!(m.trig_type, TrigType::Delay | TrigType::SetupHold) { m.trig_src2 } else { m.trig_src };
                m.trig_level2 += d as f32 * m.ch[c].volts_per_div() / 25.0;
                if m.trig_type.two_level() {
                    m.trig_level2 = m.trig_level2.min(m.trig_level);
                }
            }
            Item::TrigSrc2 => m.trig_src2 = src_step(m.trig_src2, NCH, d),
            Item::SearchOn => {
                self.search_on = !self.search_on;
                return;
            }
            Item::TrigWhen => {
                let all: &[WhenKind] = if m.trig_type == TrigType::Runt {
                    &[WhenKind::Off, WhenKind::Gt, WhenKind::Lt, WhenKind::In, WhenKind::Out]
                } else {
                    &[WhenKind::Gt, WhenKind::Lt, WhenKind::In, WhenKind::Out]
                };
                let i = all.iter().position(|&x| x == m.trig_when).unwrap_or(0);
                m.trig_when = all[cycle(i, all.len(), d)];
            }
            Item::VidLine => {
                m.video_line = (m.video_line as i32 + d).clamp(1, m.video_std.lines() as i32 / 2 - 10) as u32
            }
            Item::Pat(c) => {
                let all = if m.trig_type == TrigType::Duration {
                    &[Bit::H, Bit::L, Bit::X][..]
                } else {
                    &[Bit::H, Bit::L, Bit::X, Bit::R, Bit::F][..]
                };
                let i = all.iter().position(|&x| x == m.pat[c]).unwrap_or(0);
                m.pat[c] = all[cycle(i, all.len(), d)];
            }
            Item::PatLevel => {
                let c = m.trig_src;
                m.pat_levels[c] += d as f32 * m.ch[c].volts_per_div() / 25.0;
            }
            Item::NthN => m.nth_n = (m.nth_n as i32 + d).clamp(1, 65535) as u32,
            Item::AcqDepth => m.mdepth = cycle(m.mdepth, mdepths().len(), d),
            Item::MeasSrc => {
                let cur = self.meas_src;
                self.meas_src = self.step_wave(cur, d);
            }
            Item::MeasSrcB => {
                let cur = self.meas_src_b;
                self.meas_src_b = self.step_wave(cur, d);
            }
            // Track cursor B on its own source (follows cursor A's until set)
            Item::CurSrcB => {
                let cur = self.cursor_b_source();
                let next = self.step_wave(cur, d);
                self.set_cursor_b_source(next);
            }
            Item::DvmSrc => m.dvm_src = src_step(m.dvm_src, NCH, d),
            Item::CntSrc => m.cnt_src = src_step(m.cnt_src, NCH, d),
            Item::MathLinA => m.math_lin.0 = (m.math_lin.0 + 0.1 * d as f32).clamp(-1e6, 1e6),
            Item::MathLinB => m.math_lin.1 = (m.math_lin.1 + 0.1 * d as f32).clamp(-1e6, 1e6),
            Item::CurMode => {
                let all = [CursorMode::Off, CursorMode::Time, CursorMode::Volt, CursorMode::Track, CursorMode::Xy];
                let i = all.iter().position(|&x| x == m.cursor).unwrap();
                m.cursor = all[cycle(i, all.len(), d)];
            }
            Item::CurA => m.cur_a = (m.cur_a + d as f32 * 0.005).clamp(0.0, 1.0),
            Item::CurB => m.cur_b = (m.cur_b + d as f32 * 0.005).clamp(0.0, 1.0),
            Item::CurC => m.cur_c = (m.cur_c + d as f32 * 0.005).clamp(0.0, 1.0),
            Item::CurD => m.cur_d = (m.cur_d + d as f32 * 0.005).clamp(0.0, 1.0),
            Item::RefSel => {
                let slot = (self.ref_sel as i32 + d).rem_euclid(NREF as i32) as usize;
                self.select_workspace_source(WaveId::Ref(slot));
            }
            Item::RefColor => {
                if let Some(r) = &mut self.refs[self.ref_sel] {
                    r.color = cycle(r.color.min(REF_COLORS.len() - 1), REF_COLORS.len(), d);
                }
            }
            Item::RefVpd => {
                if let Some(r) = &mut self.refs[self.ref_sel] {
                    let i = VSCALES.iter().position(|&s| s >= r.vpd * 0.999).unwrap_or(VSCALES.len() - 1);
                    r.vpd = VSCALES[step_index(i, VSCALES.len(), d)];
                }
                self.measure_refs();
            }
            Item::RefOffset => {
                if let Some(r) = &mut self.refs[self.ref_sel] {
                    r.offset += 0.1 * r.vpd * d as f32;
                }
            }
            Item::ZoneSel => self.zone_sel = cycle(self.zone_sel, 2, d),
            Item::ZoneOn => {
                let z = &mut m.zones[self.zone_sel];
                z.on = !z.on;
            }
            Item::ZoneSrc => {
                let z = &mut m.zones[self.zone_sel];
                z.src = src_step(z.src, NCH, d);
            }
            Item::ZoneMode => {
                let z = &mut m.zones[self.zone_sel];
                z.inside = !z.inside;
            }
            Item::Hdmi => {
                let i = HDMI_MODES.iter().position(|&x| x == m.hdmi).unwrap_or(0);
                m.hdmi = HDMI_MODES[step_index(i, HDMI_MODES.len(), d)];
                if self.m.hdmi == HdmiMode::Layout {
                    self.toast("HDMI Layout: a second picture each monitor frame, the panel may get fewer frames");
                }
            }
            Item::TextSize => {
                let i = m.text_size.index();
                m.text_size = TEXT_SIZES[step_index(i, TEXT_SIZES.len(), d)];
            }
            Item::ZInput => {
                let i = m.z_src.map_or(0, |c| c + 1);
                let i = opt_src_step(i, d);
                m.z_src = if i == 0 { None } else { Some(i - 1) };
                self.dirty_map = true;
            }
            Item::AuxOut => {
                let all = AuxOut::choices(board().mask_hw() || board().zone_hw());
                let i = all.iter().position(|&a| a == m.aux_out).unwrap_or(0);
                m.aux_out = all[cycle(i, all.len(), d)];
            }
            Item::QuickAct => {
                let i = QUICKS.iter().position(|&q| q == m.quick).unwrap_or(0);
                m.quick = QUICKS[cycle(i, QUICKS.len(), d)];
            }
            Item::MathOpI => {
                let fam = self.math_fam;
                let mut ops = vec![MathOp::Off];
                ops.extend_from_slice(fam.ops());
                let i = ops.iter().position(|&x| x == m.math).unwrap_or(0);
                m.math = ops[cycle(i, ops.len(), d)];
                m.math_offset = 0.0;
                self.math_autoscale();
            }
            Item::MathFam => {
                let i = MathFamily::ALL.iter().position(|&f| f == self.math_fam).unwrap_or(0);
                self.math_fam = MathFamily::ALL[cycle(i, MathFamily::ALL.len(), d)];
                if m.math != MathOp::Off && m.math.family() != self.math_fam {
                    m.math = self.math_fam.ops()[0];
                    m.math_offset = 0.0;
                    self.math_autoscale();
                }
            }
            // sources: CH1..4 and the math channels below the selected one
            Item::MathA => m.math_a = src_step(m.math_a.min(NCH + m.math_sel - 1), NCH + m.math_sel, d),
            Item::MathB => m.math_b = src_step(m.math_b.min(NCH + m.math_sel - 1), NCH + m.math_sel, d),
            Item::MathSel => {
                let k = cycle(m.math_sel, NMATH, d);
                m.math_select(k);
                if m.math != MathOp::Off {
                    self.math_fam = m.math.family();
                }
            }
            Item::MathScale => {
                let n = if m.math == MathOp::Fft { FFT_DB.len() } else { math_scales().len() };
                m.math_scale = step_index(m.math_scale, n, d);
            }
            Item::MathOffset => m.math_offset += d as f32 * 0.1,
            Item::MeasStats => self.stats_on = !self.stats_on,
            Item::MeasLabels => self.m.meas_labels = !self.m.meas_labels,
            Item::StripDock => {
                let all = StripDock::ALL;
                let i = all.iter().position(|&s| s == m.strip_dock).unwrap_or(0);
                m.strip_dock = all[step_index(i, all.len(), d)];
                // the statistics card follows the dock the strip uses
                self.workspace.measure_docked = true;
                self.workspace.measure_float = None;
            }
            Item::DecOn => self.m.dec_on = !self.m.dec_on,
            Item::DecSel => {
                let k = (self.m.dec_sel as i32 + d).rem_euclid(DECODERS as i32) as usize;
                self.m.dec_select(k);
            }
            Item::PaneSel => {
                self.m.pane = (self.m.pane as i32 + d).rem_euclid(tables::PANES.len() as i32) as usize;
                if self.m.density == Density::Split {
                    self.m.split_pane = self.m.pane;
                    let prev = self.m.win_layout;
                    self.m.win_layout = if self.m.pane == 0 { WinLayout::Rows } else { WinLayout::One };
                    if self.m.win_layout != prev && self.m.win_layout != WinLayout::One {
                        self.spread_wins();
                        if self.m.zoom {
                            self.toast("Windows show while zoom is off");
                        }
                    }
                } else if self.m.pane != 0 && self.pane_h() == 0 {
                    self.toast("No room for a second pane here: fewer LA lanes or the scope view");
                }
            }
            Item::TrendSel(k) => {
                let n = self.trend_options().len() as i32;
                self.m.trend_sel[k] = (self.m.trend_sel[k].min(n as usize - 1) as i32 + d).rem_euclid(n) as usize;
            }
            Item::DecSrc => {
                let next = self.decoder_source_step(Some(self.m.dec_src), false, d);
                self.configure_decoder_source(0, next);
            }
            Item::DecBaud => self.m.dec_baud = step_index(self.m.dec_baud, crate::decode::BAUDS.len(), d),
            Item::DecCphyRate => {
                let rates = crate::decode::CPHY_RATES;
                let index = rates.iter().position(|&rate| rate >= self.m.dec_cphy_rate).unwrap_or(rates.len() - 1);
                self.m.dec_cphy_rate = rates[cycle(index, rates.len(), d)];
            }
            Item::DecCanRate => self.m.dec_canrate = step_index(self.m.dec_canrate, crate::decode::CAN_RATES.len(), d),
            Item::TrigBusWhat => {
                let i = BUS_WHATS.iter().position(|&x| x == self.m.trig_bus).unwrap_or(0);
                self.m.trig_bus = BUS_WHATS[cycle(i, BUS_WHATS.len(), d)];
            }
            Item::TrigBusVal => {
                self.m.trig_bus_val = (self.m.trig_bus_val as i64 + d as i64).clamp(0, BUS_VAL_MAX as i64) as u64
            }
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
            | Item::I2cBytes => self.serial_adjust(it, d),
            Item::DecCanH => self.m.dec_canh = !self.m.dec_canh,
            Item::DecThr => self.m.dec_thr += d as f32 * self.decoder_vpd() / 25.0,
            Item::DecFmt => self.m.dec_ascii = !self.m.dec_ascii,
            Item::DecProto => {
                // only the decoders System > Features offers (none: a hand-edited system.conf, see `load_system`)
                let on = self.protos_on();
                if !on.is_empty() {
                    let i = on.iter().position(|&x| x == self.m.dec_proto).unwrap_or(0);
                    self.set_dec_proto(on[cycle(i, on.len(), d)]);
                }
            }
            Item::DecUsbSpeed => self.m.dec_uspeed = cycle(self.m.dec_uspeed, crate::decode::USB_SPEEDS.len(), d),
            Item::DecOpt(k) => {
                if let Some(o) = self.m.dec_proto.module().and_then(|p| p.options().get(k)) {
                    self.m.dec_opt[k] = cycle(self.m.dec_opts()[k], o.choices.len(), d) as u8;
                }
            }
            Item::DecSrc4 => {
                let next = self.decoder_source_step(self.m.dec_src4, true, d);
                self.configure_decoder_source(3, next);
            }
            Item::DecDiff => {
                let next = self.decoder_source_step(self.m.dec_diff, true, d);
                self.configure_decoder_source(4, next);
            }
            Item::DecSrc2 => {
                let next = self.decoder_source_step(Some(self.m.dec_src2), false, d);
                self.configure_decoder_source(1, next);
            }
            Item::DecCs => {
                let next = self.decoder_source_step(self.m.dec_cs, self.m.dec_proto != Proto::CPHY, d);
                self.configure_decoder_source(2, next);
            }
            Item::DecMode => self.m.dec_mode = cycle(self.m.dec_mode as usize, 4, d) as u8,
            Item::DecBits => {
                let (lo, hi) = if self.m.dec_proto == Proto::UART { (5, 9) } else { (4, 32) };
                self.m.dec_bits = (self.m.dec_bits as i32 + d).clamp(lo, hi) as u32;
            }
            Item::DecParity => {
                use oos_dsp::decode::Parity;
                let all = [Parity::None, Parity::Even, Parity::Odd];
                let i = all.iter().position(|&x| x == self.m.dec_parity).unwrap();
                self.m.dec_parity = all[cycle(i, 3, d)];
            }
            Item::Zoom => {
                let on = !self.m.zoom;
                self.set_zoom(on);
            }
            Item::AutoRoll => self.m.auto_roll = !self.m.auto_roll,
            Item::ZScale => {
                self.m.zscale = step_index(self.m.zscale, tscales().len(), d);
                self.clamp_zoom();
            }
            Item::ZDelay => {
                self.m.zdelay += d as f32 * tscales()[self.m.zscale] / 25.0;
                self.clamp_zoom();
            }
            Item::XyMode => self.xy = !self.xy,
            Item::BootMode => self.boot_mode_toggle(),
            Item::Overclock => self.overclock_step(d),
            Item::StoreSlot => self.store_slot = (self.store_slot as i32 + d).clamp(1, 9) as u32,
            Item::StoreDest => {
                let i = crate::store::StoreDest::ALL.iter().position(|&x| x == self.store_dest).unwrap_or(0);
                self.store_dest = crate::store::StoreDest::ALL[cycle(i, crate::store::StoreDest::ALL.len(), d)];
            }
            Item::MaskSrc => {
                // a mask belongs to its channel: the new source needs a new one
                self.m.mask_src = src_step(self.m.mask_src, NCH, d);
                self.mask = None;
                self.mask_ref = None;
                self.m.mask_on = false;
            }
            Item::MaskX => {
                self.m.mask_x = ((self.m.mask_x + d as f32 * 0.04) * 100.0).round().clamp(0.0, 200.0) / 100.0;
                self.build_mask();
            }
            Item::MaskY => {
                self.m.mask_y = ((self.m.mask_y + d as f32 * 0.04) * 100.0).round().clamp(4.0, 400.0) / 100.0;
                self.build_mask();
            }
            Item::RefSrc => self.ref_src = src_step(self.ref_src, NCH, d),
            Item::RefShow(i) => {
                if let Some(r) = &mut self.refs[i] {
                    r.on = !r.on;
                }
            }
            _ => {}
        }
        // moving the strip does not change what was measured
        if it == Item::StripDock {
            self.relayout();
            return;
        }
        self.settings_changed();
        if it == Item::Hdmi {
            self.sync_full();
        }
    }

    pub(super) fn clear_measurement_statistics(&mut self) {
        self.mstat.clear();
        self.replay.clear_deferred_statistics();
        self.reset_catalog_statistics();
    }

    pub(super) fn settings_changed(&mut self) {
        // whatever moved the upper level (50 %, SCPI's LEVel, a fit) leaves the two levels in order
        self.m.order_trigger_levels();
        self.cancel_bus_candidate(true);
        if self.m.trig_mode == TrigMode::Single && self.m.run == RunState::Run {
            self.single_armed = false;
            self.scpi_arm_operation();
        }
        if self.m.run == RunState::Run {
            self.dirty_map = true;
        }
        self.clear_measurement_statistics();
    }

    pub(super) fn activate(&mut self, it: Item) {
        if Self::kind(it) == Kind::Action && PARTS.iter().any(|p| p.activate.is_some_and(|f| f(self, it))) {
            return;
        }
        match Self::kind(it) {
            Kind::Choice => self.adjust(it, 1),
            Kind::Value => {}
            Kind::Action => match it {
                Item::Open(MenuId::La) if !self.m.la.on && !self.la_probe_ok() => self.toast("No LA probe connected"),
                // start menu > Measure: the measurement page, as the Measure button and key open it
                Item::Open(MenuId::Measure) if self.menu == MenuId::Main => {
                    self.ensure_meas_own();
                    self.open_submenu(MenuId::MeasAdd);
                }
                Item::Open(id) => {
                    if id == MenuId::Cursor && self.m.cursor == CursorMode::Off {
                        // like the CURSOR key
                        self.m.cursor = CursorMode::Time;
                    }
                    self.open_submenu(id);
                }
                // start menu: the front-panel key functions; one-shot actions close the menu, stepping ones keep it
                Item::RunStop
                | Item::SingleAct
                | Item::AutosetAct
                | Item::DefaultAct
                | Item::ForceAct
                | Item::LockAct => {
                    self.close_menu();
                    self.key(match it {
                        Item::RunStop => key::RUNSTOP,
                        Item::SingleAct => key::SINGLE,
                        Item::AutosetAct => key::AUTO,
                        Item::DefaultAct => key::DEFAULT,
                        Item::ForceAct => key::FORCE,
                        _ => key::TOUCHLOCK,
                    });
                }
                Item::SlopeAct => self.key(key::SLOPE),
                Item::NavHist => self.key(key::NAVIGATE),
                Item::NavBack => self.key(0x08),
                Item::NavFwd => self.key(0x06),
                Item::QuickRun => {
                    self.close_menu();
                    self.quick();
                }
                Item::PowerAct => {
                    // the dialog of the POWER key, without the MCU answer that belongs to a key press
                    self.close_menu();
                    self.dialog = None;
                    self.power_dlg = true;
                }
                Item::HZero => {
                    self.m.delay = 0.0;
                    self.settings_changed();
                }
                Item::AutoDeskew => self.auto_deskew(),
                Item::ClearDeskew => self.clear_deskew(),
                Item::Trig50 => self.level_50(),
                Item::PickOpt(k) if self.pick.is_some_and(|p| Self::hex_item(p.it)) => {
                    // keypad: digits edit the entry, OK sets the value and goes back
                    match KEYPAD.get(k).copied() {
                        Some("OK") => {
                            let v = u64::from_str_radix(if self.keypad.is_empty() { "0" } else { &self.keypad }, 16)
                                .unwrap_or(0)
                                .min(BUS_VAL_MAX);
                            if let Some(p) = self.pick {
                                self.set_hex(p.it, v);
                            }
                            self.close_picker();
                        }
                        Some("Clear") => self.keypad.clear(),
                        Some("Del") => {
                            self.keypad.pop();
                        }
                        Some(d) if self.keypad.len() < 10 => {
                            if self.keypad == "0" {
                                self.keypad.clear();
                            }
                            self.keypad.push_str(d);
                        }
                        _ => {}
                    }
                }
                Item::PickOpt(k) if self.pick.is_some_and(|p| p.it == Item::AfgFreqEntry) => self.freq_key(k),
                Item::PickOpt(k) if self.pick.is_some_and(|p| p.it == Item::NumEntry) => self.numpad_key(k),
                // a picker its page answers for (ADR 0060)
                Item::PickOpt(k) if self.pick.is_some_and(|p| Self::page_of_pick(p.it).is_some()) => {
                    if let Some((it, page)) = self.pick.and_then(|p| Some((p.it, Self::page_of_pick(p.it)?))) {
                        self.close_picker();
                        (page.pick)(self, it, k);
                    }
                }
                Item::PickOpt(k) if self.pick.is_some_and(|p| p.it == Item::RefImport) => {
                    let p = self.ref_files.get(k).cloned();
                    self.close_picker();
                    if let Some(p) = p {
                        self.ref_load_path(&p);
                    }
                }
                Item::PickOpt(k) => {
                    if let Some(p) = self.pick {
                        let opts = self.options(p.it);
                        self.close_picker();
                        if let Some(o) = opts.get(k) {
                            self.select_option(p.it, o);
                        }
                    }
                }
                Item::AddMeas => {
                    if let Some(k) = self.next_usual_meas() {
                        self.activate(Item::MeasAdd(k));
                    }
                }
                Item::MeasCfg(i) => self.open_meas_cfg(i as usize),
                Item::MeasAdd(k) => {
                    let e = (k, self.meas_src);
                    if let Some(index) = self.m.meas.iter().position(|item| *item == e) {
                        self.m.meas.remove(index);
                        self.meas_own.remove(&e);
                        if self.meas_cfg == Some(index as u16) {
                            self.meas_cfg = None;
                        } else if self.meas_cfg.is_some_and(|n| n as usize > index) {
                            self.meas_cfg = self.meas_cfg.map(|n| n - 1);
                        }
                        self.clear_measurement_statistics();
                        self.toast(&format!(
                            "{} {} removed",
                            meascatalog::meas_long_name(k),
                            self.wave_name(self.meas_src)
                        ));
                        return;
                    } else {
                        if !self.meas_make_room() {
                            return;
                        }
                        self.m.meas.push(e);
                        self.clear_measurement_statistics();
                    }
                    if k == Meas::Burst && !(self.m.meas_idle > 0.0) {
                        if let Some((a, _)) = self.meas_analysis(self.meas_src) {
                            let p = a.period();
                            if p.is_finite() && p > 0.0 {
                                self.m.meas_idle = p;
                            }
                        }
                    }
                    self.ensure_meas_own();
                    self.toast(&format!("{} {} added", meascatalog::meas_long_name(k), self.wave_name(self.meas_src)));
                }
                Item::MeasPair(k) => self.meas_pair_toggle(k),
                Item::CntClear => self.counter_clear(),
                Item::DecTable => self.table_toggle(tables::TableKind::Decode),
                Item::DecExport => self.events_export(tables::TableKind::Decode),
                Item::SearchTable => self.table_toggle(tables::TableKind::Search),
                Item::SearchSave => self.events_export(tables::TableKind::Search),
                Item::StoreBin => match &self.last[0] {
                    Some(rec) => {
                        let p = self.new_file(&self.save_dir(), "wave", "bin");
                        let r = crate::store::write(&p, &crate::store::wave_bin(rec, &self.m));
                        self.toast_saved(&p, r);
                    }
                    None => self.toast("No waveform to save"),
                },
                Item::RefExport => self.ref_export(),
                Item::RefImport => self.ref_import(),
                Item::RefLoad => self.ref_load(),
                Item::MaskSave => self.mask_save(),
                Item::MaskLoad => self.mask_load(),
                Item::ChLabel(c) => self.wlan.osk = Some(wlan::Osk::for_label(c, &self.m.ch_label[c])),
                Item::BusLabel => self.wlan.osk = Some(wlan::Osk::for_bus(&self.m.bus.label)),
                Item::RefLabel => match &self.refs[self.ref_sel] {
                    Some(r) => self.wlan.osk = Some(wlan::Osk::for_ref(self.ref_sel, &r.label)),
                    None => self.toast("That slot is empty"),
                },
                Item::ChBiasZero(c) => self.zero_input(c),
                Item::MathExprEdit => {
                    self.wlan.osk = Some(wlan::Osk::for_expr(self.m.math_sel, &self.m.math_expr[self.m.math_sel]))
                }
                Item::MeasRemove(index) => self.remove_measurement(index as usize),
                Item::MeasClear => {
                    self.m.meas.clear();
                    self.m.meas2.clear();
                    self.m.cat.clear();
                    self.meas_own.clear();
                    self.meas_cfg = None;
                    self.clear_measurement_statistics();
                }
                Item::RefSave => {
                    let c = self.ref_src;
                    let rec = self.last[0].clone();
                    let saved = rec.as_ref().filter(|r| !r.ch[c].is_empty()).map(|r| {
                        let (v, dt) = self.meas_wave(r, c);
                        let t0 =
                            if v.len() > r.ch[c].len() { r.raw.as_ref().map(|x| x.t0).unwrap_or(r.t0) } else { r.t0 };
                        RefWave {
                            src: c,
                            color: 0,
                            label: String::new(),
                            t0,
                            dt,
                            v: v.into(),
                            vpd: self.m.ch[c].volts_per_div(),
                            offset: self.m.ch[c].offset,
                            on: true,
                        }
                    });
                    match saved {
                        Some(w) => {
                            let slot = self.refs.iter().position(|r| r.is_none()).unwrap_or(NREF - 1);
                            self.m.deskew_ref[slot] = self.m.deskew_ch[c];
                            self.refs[slot] = Some(w);
                            self.toast(&format!("CH{} saved to R{}", c + 1, slot + 1));
                            self.measure_refs();
                            self.analyzer_update();
                        }
                        None => self.toast("Source channel has no data"),
                    }
                }
                Item::RefClear => {
                    self.refs = std::array::from_fn(|_| None);
                    self.measure_refs();
                    self.analyzer_update();
                }
                Item::ClearDisp => {
                    if self.menu == MenuId::Main {
                        self.close_menu();
                    }
                    self.clear_display();
                }
                Item::SysInfo => self.sysinfo_open(),
                Item::ViewExit => self.view_exit(),
                Item::ViewSettings => self.view_settings(),
                Item::About => {
                    let p = oos_hal::model::platform();
                    let build = std::fs::read_to_string("/etc/oos-build").unwrap_or_default();
                    let mut d = vec![
                        "OOS".into(),
                        "An open operating system for the DHO800/DHO900".into(),
                        if build.is_empty() {
                            format!("oos-ui {}", env!("CARGO_PKG_VERSION"))
                        } else {
                            format!("Build {}", oos_hal::update::short(build.trim()))
                        },
                        self.info.clone(),
                        format!("Rigol {}", p.ident()),
                        p.caps_line(),
                    ];
                    // experimental limits (limits.conf) and detection problems: the details are on System info
                    if p.experimental() {
                        d.push(format!("EXPERIMENTAL limits ({}), see System info", p.overrides.len()));
                    }
                    if !p.warnings.is_empty() {
                        d.push(format!("{} warning(s), see System info", p.warnings.len()));
                    }
                    d.extend([
                        format!("Source: {}   {:.0} fps", self.src.name(), self.fps),
                        self.start_up_summary().unwrap_or_default(),
                        "Tap to close".into(),
                    ]);
                    d.retain(|l| !l.is_empty());
                    self.dialog = Some(d)
                }
                Item::Shot => {
                    // without the dialog on it
                    self.close_menu();
                    self.shot_request = Some(String::new());
                }
                Item::MathAuto => self.math_autoscale(),
                Item::ZoneBox | Item::ZoneShape => {
                    self.zone_arm = Some((self.zone_sel, it == Item::ZoneShape));
                    self.close_menu();
                    self.toast(if it == Item::ZoneShape {
                        "Draw the zone with one finger"
                    } else {
                        "Drag a box for the zone"
                    });
                }
                Item::ZoneClear => {
                    self.m.zones[self.zone_sel] =
                        crate::zone::Zone { src: self.m.zones[self.zone_sel].src, ..Default::default() };
                }
                Item::StoreSaveSetup => {
                    let p = self.setup_path();
                    let r = crate::store::write(
                        &p,
                        format!("{}{}", crate::store::setup_text(&self.m), self.ref_setup_text()).as_bytes(),
                    );
                    self.toast_saved(&p, r);
                }
                Item::StoreRecall => self.recall_setup(),
                Item::StoreCsv => match &self.last[0] {
                    Some(rec) => {
                        let p = self.new_file(&self.save_dir(), "wave", "csv");
                        let mut r = crate::store::write(&p, crate::store::wave_csv(rec, &self.m).as_bytes());
                        // the LA lines go to their own file: another sample rate, and only the changes
                        if let (Ok(()), Some(la), true) = (&r, &rec.la, self.m.la_on()) {
                            let q = self.new_file(&self.save_dir(), "la", "csv");
                            r = crate::store::write(&q, crate::store::la_csv(la, &self.m.la.lines()).as_bytes());
                        }
                        self.toast_saved(&p, r);
                    }
                    None => self.toast("No waveform to save"),
                },
                Item::StorePng => {
                    // without the dialog: the screen is saved after the next frame (main loop / script runner)
                    let p = self.new_file(&self.save_dir(), "shot", "png");
                    self.close_menu();
                    self.shot_request = Some(p.to_string_lossy().into_owned());
                }
                Item::MaskCreate => self.create_mask(),
                Item::MaskFromRef => self.create_mask_from_ref(),
                Item::MaskClear => {
                    self.mask = None;
                    self.mask_ref = None;
                    self.m.mask_on = false;
                    self.mask_counts_reset();
                    self.toast("Mask cleared");
                }
                Item::MaskReset => self.mask_counts_reset(),
                Item::StoreFfSave => self.ff_save(),
                Item::StoreFfLoad => self.ff_load(),
                Item::Stock => self.stock_tap(),
                Item::BootAgain => self.boot_again_tap(),
                Item::Update => self.dialog = Some(self.update_lines()),
                Item::AcqSegTable => self.table_toggle(tables::TableKind::Segments),
                Item::SrcOn => {
                    let v = self.off_sources(self.menu);
                    for &c in &v {
                        self.m.ch[c].on = true;
                    }
                    self.focus = 0;
                    self.settings_changed();
                    let names: Vec<String> = v.iter().map(|c| format!("CH{}", c + 1)).collect();
                    self.toast(&format!("{} on", names.join(", ")));
                }
                Item::WlanInfo => self.dialog = Some(self.wlan_lines()),
                Item::WlanScan => self.wlan_scan(),
                Item::WlanForget => self.wlan_forget_tap(),
                Item::UpdateRollback => self.update_rollback_tap(),
                _ => {}
            },
        }
    }
}

/// Text on a dark backing, for labels drawn over the waveforms; returns the x after the text.
/// `fine_time`: next fine time base from `now`, by `d` hundredths on three digits, as (step, factor). The step is the
/// one at or above the value; from the roll time base on the screen is the record, so there only the steps remain.
fn fine_time(now: f32, d: i32) -> (usize, f32) {
    let ts = tscales();
    let raw = now as f64 * (1.0 + 0.01 * d as f64);
    let q = 10f64.powf(raw.max(1e-12).log10().floor() - 2.0);
    let want = (((raw / q).round() * q) as f32).clamp(ts[0], ts[ts.len() - 1]);
    let i = ts.iter().position(|&t| t >= want * 0.99999).unwrap_or(ts.len() - 1);
    if ts[i] as f64 >= board().roll_tdiv() * 0.999 {
        let at = ts.iter().position(|&t| t >= now * 0.99999).unwrap_or(ts.len() - 1) as i32;
        return ((at + d.signum()).clamp(0, ts.len() as i32 - 1) as usize, 1.0);
    }
    let f = want / ts[i];
    (i, if (f - 1.0).abs() < 1e-5 { 1.0 } else { f })
}

/// After the time base changed from `old` s/div: the zoom stays inside it, and the delay, the time in the middle of
/// the screen, moves so the expand point keeps its place (the knob and the number pad).
pub(super) fn keep_expand_point(m: &mut Model, old: f32) {
    if m.zscale > m.tscale {
        m.zscale = m.tscale;
    }
    let new = m.time_per_div();
    let half = HDIV / 2.0;
    match m.h_expand {
        HExpand::Center => {}
        HExpand::Left => m.delay -= half * (old - new),
        HExpand::Right => m.delay += half * (old - new),
        HExpand::Trigger => m.delay *= new / old,
    }
}
