use super::*;

impl Ui {
    pub(super) fn trigger_source_label(&self) -> String {
        if self.m.trig_ref.is_none() {
            if let Some(line) = self.m.trig_line() { return format!("D{line}"); }
        }
        Self::trigger_wave_label(self.m.trigger_wave())
    }

    pub(super) fn trigger_wave_label(id: WaveId) -> String {
        match id { WaveId::Ch(c) => format!("CH{}", c + 1), WaveId::Ref(i) => format!("R{}", i + 1) }
    }

    pub(super) fn trigger_sources(&self) -> Vec<WaveId> {
        (0..n_ch()).map(WaveId::Ch).chain((0..NREF).filter(|&i| self.refs[i].is_some()).map(WaveId::Ref)).collect()
    }

    pub(crate) fn set_trigger_source(&mut self, id: WaveId, secondary: bool) -> bool {
        if matches!(id, WaveId::Ref(i) if i >= NREF || self.refs[i].is_none()) { return false; }
        if matches!(id, WaveId::Ch(c) if c >= n_ch()) { return false; }
        let running=self.m.run==RunState::Run;
        let previous_context=self.record_mode();
        let primary_changed=!secondary && self.m.trigger_wave()!=id;
        if !secondary {
            match id {
                WaveId::Ch(c) if self.record_mode() => self.activate_live_channel(c),
                WaveId::Ref(i) => {
                    if !self.record_mode() { self.remember_live(); self.set_offline(true); }
                    self.select_workspace_source(WaveId::Ref(i));
                    if !self.session_is_sim { self.offline_ref = i; }
                }
                _ => {}
            }
        }
        self.m.set_trigger_wave(id, secondary);
        if !secondary && (primary_changed || previous_context!=self.record_mode()) {
            // Choosing the acquisition source preserves the user's Run/Stop
            // intent. It never turns a stopped source choice into an acquisition.
            self.sim_cancel_generation();self.cancel_bus_candidate(true);
            self.m.run=if running{RunState::Run}else{RunState::Stop};
            self.sim_play=self.record_mode() && running;
            self.m.status=if running{TrigStatus::Wait}else{TrigStatus::Stop};
            if running && self.m.trig_mode==TrigMode::Single{self.single_armed=false;self.scpi_arm_operation();}
            if !running && previous_context && !self.record_mode(){self.cleared=true;}
            self.replay.reset_trigger_source();
            self.replay.next=self.now;self.replay.sim_next=self.now;
        } else { self.replay.reset_search(); }
        true
    }

    /// Enable a physical input without discarding imported overlays or their settings.
    pub(super) fn activate_live_channel(&mut self, channel: usize) {
        if channel >= n_ch() { return; }
        self.workflow_epoch = self.workflow_epoch.wrapping_add(1);
        self.sim_cancel_generation();
        self.offline = false;
        self.offline_hold = None;
        self.session_is_sim = false;
        self.sim_play = false;
        self.m.ch[channel].on = true;
        self.select_workspace_source(WaveId::Ch(channel));
        self.m.set_trigger_wave(WaveId::Ch(channel), false);
        self.m.run = RunState::Run;
        self.m.status = TrigStatus::Wait;
        self.m.floor_live_tdiv();
        self.last[0] = None;
        self.dirty_map = true;
        self.relayout();
    }

    pub(super) fn trigger_vpd(&self, secondary: bool) -> f32 {
        let id = if secondary { self.m.trigger_wave2() } else { self.m.trigger_wave() };
        match id {
            WaveId::Ch(c) => self.m.ch[c].volts_per_div(),
            WaveId::Ref(i) => self.refs[i].as_ref().map_or(1.0, |r| r.vpd),
        }.max(1e-9)
    }

    fn reference_trigger_range(&self) -> Option<(f32, f32)> {
        let r = self.refs.get(self.m.trig_ref?)?.as_ref()?;
        let summary = crate::wave::autoset_summary(&r.v, r.dt);
        let (lo, hi) = (summary.vmin, summary.vmax);
        (lo.is_finite() && hi.is_finite() && hi > lo).then_some((lo, hi))
    }

    pub(super) fn fit_reference_trigger_levels(&mut self) {
        let Some((lo, hi)) = self.reference_trigger_range() else { return };
        if matches!(self.m.trig_type, TrigType::Runt | TrigType::Slope | TrigType::Window) {
            self.m.trig_level = lo + 0.8 * (hi - lo);
            self.m.trig_level2 = lo + 0.2 * (hi - lo);
        } else { self.m.trig_level = 0.5 * (lo + hi); }
    }

    pub(crate) fn trigger_type_set(&mut self, kind: TrigType) { self.set_trig_type(kind); }

    pub(super) fn set_reference_trigger_type(&mut self, kind: TrigType) -> bool {
        if self.m.trig_ref.is_none() { return false; }
        let previous = self.m.trig_type;
        self.m.trig_type = kind;
        if kind == TrigType::Bus {
            self.m.dec_on = true;
            if Self::decoder_uses_refs(&self.m) {
                if let Some(source) = decode_wave(self.m.dec_src) { self.m.set_trigger_wave(source, false); }
            }
        }
        let invalid = self.reference_trigger_range().is_some_and(|(lo, hi)| {
            !(lo..hi).contains(&self.m.trig_level) ||
                (matches!(kind, TrigType::Runt | TrigType::Slope | TrigType::Window) &&
                (!(lo..hi).contains(&self.m.trig_level2) || self.m.trig_level == self.m.trig_level2))
        });
        if invalid || (kind != previous && matches!(kind, TrigType::Runt | TrigType::Slope | TrigType::Window)) {
            self.fit_reference_trigger_levels();
        }
        if kind == TrigType::Runt { self.m.trig_when = WhenKind::Off; }
        else if self.m.trig_when == WhenKind::Off { self.m.trig_when = WhenKind::Gt; }
        self.replay.reset_search();
        true
    }

    pub(super) fn draw_compact_trigger(&self, cv: &mut Canvas, rect: Rect) {
        btn(cv, rect, BAR, if self.menu_open && self.menu_root() == MenuId::Trigger { ACCENT } else { CARD_BORDER });
        cv.set_clip(rect.inset(3));
        cv.text(&self.fonts.sb, rect.x + 7, rect.y + 12,
            &format!("{} · {} · {:?}", self.trigger_source_label(), self.m.trig_type.name(), self.m.trig_mode), ACCENT);
        let levels = if matches!(self.m.trig_type, TrigType::Runt | TrigType::Slope | TrigType::Window) {
            format!("{} … {}", si_short(self.m.trig_level2 as f64, "V"), si_short(self.m.trig_level as f64, "V"))
        } else { si_short(self.m.trig_level as f64, "V") };
        cv.text(&self.fonts.s, rect.x + 7, rect.y + 25, &levels, TEXT);
        cv.no_clip();
    }

    pub(super) fn compact_trigger_tap(&mut self, x: i32, y: i32, rect: Rect) -> bool {
        if !rect.contains(x, y) { return false; }
        self.toggle_menu(MenuId::Trigger);
        true
    }

    pub(super) fn trigger_absolute_level_visible(&self, secondary: bool) -> bool {
        let id = if secondary { self.m.trigger_wave2() } else { self.m.trigger_wave() };
        !matches!(id, WaveId::Ch(c) if self.m.ch[c].coupling == Coupling::Ac)
    }

    pub(super) fn trigger_level_y(&self, level: f32, secondary: bool) -> Option<i32> {
        if !self.trigger_absolute_level_visible(secondary) { return None; }
        let id = if secondary { self.m.trigger_wave2() } else { self.m.trigger_wave() };
        let area = self.source_area(id)?;
        let (vpd, offset) = match id {
            WaveId::Ref(i) => { let r = self.refs[i].as_ref()?; (r.vpd, r.offset) }
            WaveId::Ch(c) => (self.m.ch[c].volts_per_div(), self.m.ch[c].offset),
        };
        Some(self.zoom_y_of_divs(area, (level + offset) / vpd).round() as i32)
    }

    pub(super) fn draw_reference_trigger_markers(&self, cv: &mut Canvas) {
        for (id, level, tag) in [(self.m.trigger_wave(), self.m.trig_level, "T"),
            (if matches!(self.m.trig_type, TrigType::Delay | TrigType::SetupHold) { self.m.trigger_wave2() } else { self.m.trigger_wave() }, self.m.trig_level2, "t")] {
            if tag == "t" && !matches!(self.m.trig_type, TrigType::Runt | TrigType::Slope | TrigType::Window | TrigType::Delay | TrigType::SetupHold) { continue; }
            let WaveId::Ref(i) = id else { continue };
            let Some(r) = self.refs[i].as_ref().filter(|r| r.on) else { continue };
            let Some(a) = self.source_area(id) else { continue };
            let y = self.zoom_y_of_divs(a, (level + r.offset) / r.vpd).round() as i32;
            let margin = 9.min(a.h.max(0) / 2);
            let y = y.clamp(a.y + margin, a.y + a.h - margin);
            let x = a.x + a.w - 22;
            cv.triangle([(x, y), (x + 8, y - 6), (x + 8, y + 6)], ACCENT);
            cv.text(&self.fonts.sb, x + 10, y + 5, tag, ACCENT);
            if (tag == "T" && self.touch.drag == Drag::Level) || (tag == "t" && self.touch.drag == Drag::Level2) {
                cv.hline(a.x, a.x + a.w - 1, y, ACCENT);
            }
            let text = format!("{} {}", Self::trigger_wave_label(id), si_short(level as f64, "V"));
            backed_text(cv, &self.fonts.s, x - self.fonts.s.width(&text) - 4, y + 4, &text, ACCENT);
        }
    }
}
#[cfg(test)]
mod acquisition_source_tests {
    use super::*;
    use std::sync::Arc;
    fn fixture(running:bool)->Ui {
        let mut ui=Ui::new(Source::Demo{t:0.0});
        ui.m.run=if running{RunState::Run}else{RunState::Stop};
        ui.m.status=if running{TrigStatus::Auto}else{TrigStatus::Stop};
        ui.m.trig_type=TrigType::Edge;ui.m.trig_mode=TrigMode::Normal;ui.m.trig_level=0.;
        ui.m.meas=vec![(Meas::Vpp,WaveId::Ref(0))];ui.m.meas2.clear();ui.m.cat.clear();
        for i in 0..2{ui.refs[i]=Some(RefWave{src:0,color:i,label:format!("R{}",i+1),t0:0.,dt:1e-9,
            v:(0..5000).map(|k|if i==1||k%40<20{-1.0}else{1.0}).collect::<Vec<_>>().into(),vpd:0.5,offset:0.,on:true});}
        ui
    }
    #[test]
    fn choosing_primary_ref_while_live_running_arms_and_accepts_without_an_extra_run(){
        let mut ui=fixture(true);let samples=Arc::clone(&ui.refs[0].as_ref().unwrap().v);
        assert!(ui.set_trigger_source(WaveId::Ref(0),false));
        assert!(ui.record_mode()&&ui.sim_play);assert_eq!((ui.m.run,ui.m.status),(RunState::Run,TrigStatus::Wait));
        ui.step(0.0);assert_eq!(ui.replay.captures,1);assert_eq!(ui.m.status,TrigStatus::Triggered);
        assert!(Arc::ptr_eq(&samples,&ui.refs[0].as_ref().unwrap().v));assert_eq!(ui.mstat[0].n,1);
    }
    #[test]
    fn choosing_primary_ref_while_stopped_holds_until_single_accepts_exactly_once(){
        let mut ui=fixture(false);assert!(ui.set_trigger_source(WaveId::Ref(0),false));
        assert_eq!((ui.m.run,ui.m.status),(RunState::Stop,TrigStatus::Stop));assert!(!ui.sim_play);
        ui.step(0.2);assert_eq!(ui.replay.captures,0);
        ui.key(key::SINGLE);ui.step(0.0);assert_eq!(ui.replay.captures,1);assert_eq!(ui.m.run,RunState::Stop);
        ui.step(0.2);assert_eq!(ui.replay.captures,1);
    }
    #[test]
    fn returning_to_primary_channel_preserves_run_or_stop_without_stopped_preview_acquisition(){
        for running in[false,true]{let mut ui=fixture(running);ui.set_trigger_source(WaveId::Ref(0),false);
            ui.m.run=if running{RunState::Run}else{RunState::Stop};ui.sim_play=running;
            assert!(ui.set_trigger_source(WaveId::Ch(0),false));assert!(!ui.record_mode()&&!ui.sim_play);
            assert_eq!(ui.m.run,if running{RunState::Run}else{RunState::Stop});
            assert_eq!(ui.m.status,if running{TrigStatus::Wait}else{TrigStatus::Stop});
            if !running{let count=ui.wfm_total;ui.step(0.2);assert_eq!(ui.wfm_total,count,"choosing a source while Stop cannot acquire a preview");}
        }
    }
    #[test]
    fn same_context_ref_change_arms_fresh_condition_without_reusing_old_hits_or_force(){
        let mut ui=fixture(true);ui.set_trigger_source(WaveId::Ref(0),false);ui.key(key::RUNSTOP);
        if ui.m.run==RunState::Stop{ui.key(key::RUNSTOP);}ui.step(0.2);assert!(ui.replay.trigger_time.is_some());
        let count=ui.replay.captures;ui.replay.request_force();
        assert!(ui.set_trigger_source(WaveId::Ref(1),false));assert_eq!((ui.m.run,ui.m.status),(RunState::Run,TrigStatus::Wait));
        assert_eq!(ui.replay.trigger_time,None);assert!(!ui.replay.force_requested());
        ui.step(0.0);assert_eq!(ui.replay.captures,count);assert_eq!(ui.m.status,TrigStatus::Wait);assert!(ui.search_events().is_empty());
    }
    #[test]
    fn switching_source_while_single_is_armed_preserves_single_and_stops_on_one_hit(){
        let mut ui=fixture(true);ui.m.trig_mode=TrigMode::Single;ui.single_armed=true;
        assert!(ui.set_trigger_source(WaveId::Ref(0),false));assert_eq!(ui.m.trig_mode,TrigMode::Single);
        assert_eq!(ui.m.run,RunState::Run);assert!(!ui.single_armed);
        ui.step(0.0);assert_eq!((ui.m.run,ui.replay.captures),(RunState::Stop,1));ui.step(0.2);assert_eq!(ui.replay.captures,1);
    }
    #[test]
    fn secondary_source_focus_and_overlay_import_keep_live_acquisition_context(){
        let mut ui=fixture(true);let before=(ui.m.run,ui.m.status,ui.m.trigger_wave(),ui.record_mode());
        assert!(ui.set_trigger_source(WaveId::Ref(1),true));assert_eq!((ui.m.run,ui.m.status,ui.m.trigger_wave(),ui.record_mode()),before);
        ui.select_workspace_source(WaveId::Ref(0));assert_eq!((ui.m.run,ui.m.status,ui.m.trigger_wave(),ui.record_mode()),before);
        let trace=crate::store::WaveTrace{t0:0.,dt:1e-9,v:vec![0.,1.,0.,1.],label:"overlay".into()};
        ui.commit_waveform_traces_at(vec![trace],"overlay",2).unwrap();
        assert_eq!((ui.m.run,ui.m.status,ui.m.trigger_wave(),ui.record_mode()),before);assert!(ui.m.ch[0].on);
    }
}
