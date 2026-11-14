// SPDX-License-Identifier: GPL-2.0-or-later
//! Zoom is a display transform, not a change to acquired samples or source controls.
use super::*;

fn mixed() -> Ui {
    let mut ui = Ui::new(Source::Demo { t: 0.0 });
    ui.m.run = RunState::Stop;
    ui.m.tscale = tscales().iter().position(|&s| s >= 1e-6).unwrap();
    ui.m.tfine = 1.0;
    ui.m.meas.clear();
    ui.m.meas2.clear();
    ui.refs[0] = Some(RefWave { src: 0, color: 1, label: "Known record".into(),
        t0: -40e-6, dt: 20e-9, v: ((0..4000).map(|k| (k as f32 * 0.1).sin() * 0.4).collect::<Vec<_>>()).into(),
        vpd: 0.2, offset: 0.1, on: true });
    ui.relayout();
    ui
}

fn pointer(ui: &mut Ui, phase: Phase, x: i32, y: i32) {
    ui.handle(Ev::Touch { id: 0, phase, x, y });
}
fn tap(ui: &mut Ui, rect: Rect) {
    let (x,y) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
    pointer(ui,Phase::Down,x,y); pointer(ui,Phase::Up,x,y);
}
fn drag(ui: &mut Ui, from: (i32,i32), to: (i32,i32)) {
    pointer(ui,Phase::Down,from.0,from.1); pointer(ui,Phase::Move,to.0,to.1); pointer(ui,Phase::Up,to.0,to.1);
}
fn control(ui: &mut Ui, name: &str) {
    let rect = ui.zoom_controls().into_iter().find(|(_,text)| *text==name).unwrap().0;
    tap(ui,rect);
}
fn stacked(ui: &mut Ui) {
    let time = ui.card_layout().time;
    tap(ui,Rect::new(time.x-62,BOTTOM.y+3,58,30));
    assert!(ui.stacked());
}

#[test]
fn corner_magnifier_toggles_display_and_keeps_distinct_touch_targets() {
    let mut ui=mixed();
    let before=ui.m.clone();
    let (full,full_hit)=ui.full_btn(); let (zoom,hit)=ui.zoom_btn();
    assert!(!overlaps(full,zoom) && !overlaps(full_hit,hit));
    tap(&mut ui,hit); assert!(ui.m.zoom && !ui.zoom_arm && !ui.full);
    let hit=ui.zoom_btn().1;tap(&mut ui,hit);assert!(!ui.m.zoom && !ui.zoom_arm);
    ui.m.zscale=before.zscale;ui.m.zdelay=before.zdelay;
    assert_eq!(ui.m,before);
    assert_eq!(ui.replay.captures,0);
}

#[test]
fn repeated_boxes_magnify_both_axes_and_tap_menu_escape_cancel_only_drawing_first() {
    let mut ui=mixed();ui.set_zoom(true);
    let source=(ui.refs[0].as_ref().unwrap().vpd,ui.refs[0].as_ref().unwrap().offset);
    let main=(ui.m.tscale,ui.m.tfine,ui.m.delay);
    control(&mut ui,"Box");assert!(ui.zoom_arm);
    let a=ui.wave_area();let old=ui.view().0;
    drag(&mut ui,(a.x+a.w/4,a.y+a.h/4),(a.x+a.w/2,a.y+3*a.h/4));
    assert!(ui.m.zoom && ui.zoom_arm && ui.view().0<old);
    assert!((ui.zoom_vfactor-2.0).abs()<0.02);
    let first=ui.zoom_vfactor;let a=ui.wave_area();
    drag(&mut ui,(a.x+a.w/4,a.y+a.h/4),(a.x+a.w/2,a.y+3*a.h/4));
    assert!(ui.zoom_arm && ui.zoom_vfactor>first);
    let a=ui.wave_area();tap(&mut ui,Rect::new(a.x+80,a.y+80,2,2));
    assert!(ui.m.zoom && !ui.zoom_arm);
    control(&mut ui,"Box");ui.open_menu(MenuId::Cursor);assert!(!ui.zoom_arm && ui.m.zoom);
    ui.close_menu();control(&mut ui,"Box");ui.dismiss_context();assert!(!ui.zoom_arm && ui.m.zoom);
    ui.dismiss_context();assert!(!ui.m.zoom);
    assert_eq!((ui.m.tscale,ui.m.tfine,ui.m.delay),main);
    assert_eq!((ui.refs[0].as_ref().unwrap().vpd,ui.refs[0].as_ref().unwrap().offset),source);
    assert_eq!((ui.m.run,ui.replay.captures),(RunState::Stop,0));
}

#[test]
fn vertical_zoom_is_shared_by_detail_lanes_and_has_exact_forward_inverse_mapping() {
    for stack in [false,true] {
        let mut ui=mixed();ui.set_zoom(true);if stack {stacked(&mut ui);}
        ui.zoom_vfactor=4.0;ui.zoom_vcenter=0.75;
        let overview=Ui::split_zoom(ui.grid).0;
        assert_eq!(ui.zoom_vertical_transform(overview),(1.0,0.0));
        for area in ui.wins.iter().copied() {
            assert_eq!(ui.zoom_vertical_transform(area),(4.0,0.75));
            for d in [-0.1,0.5,1.0] {
                let y=ui.zoom_y_of_divs(area,d);
                let inverse=ui.zoom_divs_at(area,(y-area.y as f32)/area.h as f32);
                assert!((inverse-d).abs()<1e-5);
            }
        }
        let display=ui.zoom_display_model();
        assert_eq!(display.ch[0].volts_per_div(),ui.m.ch[0].volts_per_div()/4.0);
        assert_eq!(display.ch[0].offset,ui.m.ch[0].offset-0.75*ui.m.ch[0].volts_per_div());
        let original=(ui.m.ch[0].vfine,ui.m.ch[0].offset);
        control(&mut ui,"V+");assert_eq!(ui.zoom_vfactor,8.0);
        control(&mut ui,"V-");assert_eq!(ui.zoom_vfactor,4.0);
        assert_eq!((ui.m.ch[0].vfine,ui.m.ch[0].offset),original);
        ui.reset_zoom();assert_eq!((ui.zoom_vfactor,ui.zoom_vcenter),(1.0,0.0));
    }
}

#[test]
fn detail_pinch_and_ctrl_wheel_keep_time_anchor_without_source_mutations() {
    let mut ui=mixed();ui.set_zoom(true);stacked(&mut ui);
    let area=ui.source_area(WaveId::Ref(0)).unwrap();
    let model=crate::store::setup_text(&ui.m);
    let ref_settings=(ui.refs[0].as_ref().unwrap().vpd,ui.refs[0].as_ref().unwrap().offset);
    let (x,y)=(area.x+area.w/3,area.y+area.h/3);
    let fy=(y-area.y) as f32/area.h as f32;
    let (scale,delay)=ui.view();
    ui.handle(Ev::Wheel{x,y,steps:1,ctrl:false,shift:false,alt:false});
    assert_eq!(ui.view().0,scale);assert!(ui.view().1<delay);
    let v=ui.zoom_divs_at(area,fy);
    let fx=(x-area.x) as f64/area.w as f64;
    let before=ui.view();let anchor=before.1 as f64+(fx-0.5)*HDIV as f64*before.0 as f64;
    let vertical=(ui.zoom_vfactor,ui.zoom_vcenter);
    ui.handle(Ev::Wheel{x,y,steps:1,ctrl:true,shift:false,alt:false});
    let after=ui.view();let held=after.1 as f64+(fx-0.5)*HDIV as f64*after.0 as f64;
    assert!(after.0<before.0 && (held-anchor).abs()<before.0 as f64*1e-5);
    assert_eq!((ui.zoom_vfactor,ui.zoom_vcenter),vertical);
    assert!((ui.zoom_divs_at(area,fy)-v).abs()<1e-5);
    // Spread actual fingers in the same detail lane: both axes are display magnification.
    let (x0,y0)=(area.x+200,area.y+area.h/3);
    let (x1,y1)=(area.x+400,area.y+2*area.h/3);
    let previous=(ui.m.zscale,ui.zoom_vfactor);
    pointer(&mut ui,Phase::Down,x0,y0);
    ui.handle(Ev::Touch{id:1,phase:Phase::Down,x:x1,y:y1});
    ui.handle(Ev::Touch{id:1,phase:Phase::Move,x:x1+100,y:y1+50});
    ui.handle(Ev::Touch{id:1,phase:Phase::Up,x:x1+100,y:y1+50});
    pointer(&mut ui,Phase::Up,x0,y0);
    assert!(ui.m.zscale<previous.0 && ui.zoom_vfactor>previous.1);
    // Only the two display zoom model fields may differ.
    let z=(ui.m.zscale,ui.m.zdelay);let mut baseline=ui.m.clone();
    let mut old=Model::default();crate::store::parse_setup(&model).map(|m|old=m).unwrap();
    baseline.zscale=old.zscale;baseline.zdelay=old.zdelay;
    assert_eq!(crate::store::setup_text(&baseline),model);
    assert_ne!(z.0,old.zscale);
    assert_eq!((ui.refs[0].as_ref().unwrap().vpd,ui.refs[0].as_ref().unwrap().offset),ref_settings);
    assert_eq!((ui.m.run,ui.replay.captures),(RunState::Stop,0));
}

#[test]
fn overview_resize_keeps_opposite_edge_fixed_after_scale_snapping() {
    for left in [true,false] {
        let mut ui=mixed();ui.set_zoom(true);
        let area=Ui::split_zoom(ui.grid).0;
        let half=ui.view().0*HDIV*0.5;
        let edge=if left {ui.m.zdelay-half}else{ui.m.zdelay+half};
        let fixed=if left {ui.m.zdelay+half}else{ui.m.zdelay-half};
        let x=area.x+area.w/2+((edge-ui.m.delay)/ui.m.time_per_div()*area.w as f32/HDIV) as i32;
        drag(&mut ui,(x,area.y+area.h/2),(x+if left {-60}else{60},area.y+area.h/2));
        let after=if left {ui.m.zdelay+ui.view().0*HDIV*0.5}else{ui.m.zdelay-ui.view().0*HDIV*0.5};
        assert!((after-fixed).abs()<ui.view().0*1e-4,"{after} != {fixed}");
        assert_eq!((ui.m.run,ui.replay.captures),(RunState::Stop,0));
    }
}

#[test]
fn overview_pinch_changes_main_timebase_and_close_retains_that_explicit_change() {
    let mut ui=mixed();ui.set_zoom(true);
    let old=ui.m.tscale;let a=Ui::split_zoom(ui.grid).0;
    let (x,y)=(a.x+a.w/3,a.y+a.h/2);
    pointer(&mut ui,Phase::Down,x,y);
    ui.handle(Ev::Touch{id:1,phase:Phase::Down,x:x+200,y});
    ui.handle(Ev::Touch{id:1,phase:Phase::Move,x:x+300,y});
    ui.handle(Ev::Touch{id:1,phase:Phase::Up,x:x+300,y});
    pointer(&mut ui,Phase::Up,x,y);
    assert!(ui.m.tscale<old);
    let main=(ui.m.tscale,ui.m.tfine,ui.m.delay);
    let close=ui.zoom_close();tap(&mut ui,close);
    assert!(!ui.m.zoom);assert_eq!((ui.m.tscale,ui.m.tfine,ui.m.delay),main);
}

#[test]
fn vertical_detail_phosphor_and_reference_pixels_follow_the_same_transform_as_markers() {
    let mut ui=mixed();ui.set_zoom(true);ui.zoom_vfactor=2.0;ui.zoom_vcenter=0.4;
    let area=ui.wave_area();let (scale,delay)=ui.view();let span=scale*HDIV;
    let values=vec![0.1;21];let t0=(delay-span*0.5) as f64;let dt=span as f64/20.0;
    let model=ui.zoom_display_model();let mut ph=Phosphor::new(area.w as usize,area.h as usize);
    ph.draw_ex(0,&model,(scale,delay),t0,dt,&values,24,None);
    let channel_y=ui.y_of_volts(0,0.1)-area.y;
    assert!(ph.map[0][channel_y as usize*ph.w..(channel_y as usize+1)*ph.w].iter().any(|&p|p>0));
    let reference=ui.refs[0].as_mut().unwrap();reference.t0=t0;reference.dt=dt;reference.v=values.into();
    let expected=ui.y_of_wave(WaveId::Ref(0),0.1);
    let mut cv=Canvas::new(W,H);ui.draw_refs_at(&mut cv,area,(scale,delay),true);
    let color=REF_COLORS[1].1;
    assert!((area.x+area.w/2-20..area.x+area.w/2+20).all(|x|cv.px[(expected*W+x) as usize]==color));
    assert_eq!(ui.trigger_level_y(0.1,false),Some(ui.y_of_volts(0,0.1)));
}

#[test]
fn ac_source_has_no_absolute_trigger_marker_and_dc_drag_draws_thin_guide() {
    for coupling in [Coupling::Ac,Coupling::Dc] {
        let mut ui=mixed();ui.refs[0]=None;ui.relayout();ui.set_zoom(true);
        ui.m.ch[0].coupling=coupling;ui.m.trig_src=0;ui.m.trig_ref=None;
        ui.m.trig_type=TrigType::Edge;ui.m.trig_level=0.0;
        let area=ui.wave_area();let y=ui.trigger_level_y(0.0,false);
        assert_eq!(y.is_some(),coupling==Coupling::Dc);
        let from=(area.x+area.w-30,area.y+area.h/2);
        pointer(&mut ui,Phase::Down,from.0,from.1);pointer(&mut ui,Phase::Move,from.0,from.1+24);
        let mut cv=Canvas::new(W,H);ui.draw_markers(&mut cv);
        if coupling==Coupling::Dc {
            assert!(ui.touch.drag==Drag::Level);assert_ne!(ui.m.trig_level,0.0);
            let y=ui.trigger_level_y(ui.m.trig_level,false).unwrap();
            assert!((200..W-200).all(|x|cv.px[(y*W+x) as usize]==ACCENT));
            assert!((200..W-200).all(|x|cv.px[((y+1)*W+x) as usize]!=ACCENT),"guide is exactly one pixel thick");
        } else {
            assert!(ui.touch.drag!=Drag::Level);assert_eq!(ui.m.trig_level,0.0);
        }
        pointer(&mut ui,Phase::Up,from.0,from.1+24);
        assert_eq!((ui.m.run,ui.replay.captures),(RunState::Stop,0));
    }
    // An imported reference has an absolute voltage axis even when CH1 is AC.
    let mut reference=mixed();reference.m.ch[0].coupling=Coupling::Ac;
    reference.m.set_trigger_wave(WaveId::Ref(0),false);reference.set_zoom(true);
    assert!(reference.trigger_level_y(0.1,false).is_some());
}

#[test]
fn desktop_modifier_events_pan_and_control_zoom_time_without_vertical_changes() {
    let mut ui=mixed();stacked(&mut ui);
    let area=ui.source_area(WaveId::Ref(0)).unwrap();let (x,y)=(area.x+area.w/2,area.y+area.h/2);
    let source=ui.refs[0].as_ref().unwrap();let original=(source.vpd,source.offset,source.v.as_ptr());
    let ch=ui.m.ch.clone();let trigger=ui.m.trigger();let scale=ui.m.tscale;let time=ui.m.delay;
    ui.handle(Ev::Wheel{x,y,steps:2,ctrl:false,shift:false,alt:false});
    assert_eq!(ui.m.tscale,scale);assert!(ui.m.delay<time);
    let source=ui.refs[0].as_ref().unwrap();assert_eq!((source.vpd,source.offset,source.v.as_ptr()),original);
    assert_eq!(ui.m.ch,ch);assert_eq!(ui.m.trigger(),trigger);
    ui.handle(Ev::Wheel{x,y,steps:1,ctrl:true,shift:false,alt:false});
    assert!(ui.m.tscale<scale);assert_eq!(ui.refs[0].as_ref().unwrap().vpd,original.0);assert_eq!(ui.ref_sel,0);
    assert_eq!(ui.refs[0].as_ref().unwrap().offset,original.1);assert_eq!(ui.m.ch,ch);
    assert_eq!(ui.m.trigger(),trigger);assert_eq!((ui.m.run,ui.records,ui.replay.captures),(RunState::Stop,0,0));
    ui.set_zoom(true);stacked(&mut ui);let a=ui.source_area(WaveId::Ch(0)).unwrap();
    let before=ui.m.ch.clone();let factor=ui.zoom_vfactor;let zscale=ui.m.zscale;
    ui.handle(Ev::Wheel{x:a.x+a.w/2,y:a.y+a.h/2,steps:1,ctrl:true,shift:false,alt:false});
    assert_eq!(ui.zoom_vfactor,factor);assert_eq!(ui.m.ch,before);assert!(ui.m.zscale<zscale);
}

#[test]
fn mixed_demo_reference_stacked_subnanosecond_detail_has_live_channel_pixels() {
    let mut ui=Ui::new(Source::Demo{t:0.0});ui.m.mdepth=0;
    ui.m.ch[0].on=true;ui.m.ch[1].on=true;ui.m.ch[0].offset=-1.25;ui.m.ch[1].offset=-1.25;
    ui.m.tscale=tscales().iter().position(|&s|(s-200e-9).abs()<1e-14).unwrap();ui.m.delay=819.2e-9;
    ui.m.hist=Hist::Horizontal;
    ui.refs[0]=Some(RefWave{src:0,color:2,label:"R1".into(),t0:0.0,dt:5e-12,
        v: ((0..20_000).map(|i|(i as f32*0.03).sin()*0.4).collect::<Vec<_>>()).into(),vpd:0.2,offset:0.0,on:true});
    ui.set_zoom(true);ui.m.zscale=tscales().iter().position(|&s|(s-200e-12).abs()<1e-16).unwrap();ui.m.zdelay=32.604365e-9;
    stacked(&mut ui);ui.m.run=RunState::Run;ui.step(0.02);
    assert!(!ui.record_mode());assert_eq!(ui.wins.len(),3);
    for c in 0..2 {assert!(ui.phz.map[c].iter().any(|&v|v>0),"CH{} detail blank, range {:?}, dt {:?}",c+1,(ui.stats[c].vmin,ui.stats[c].vmax),ui.last[0].as_ref().map(|r|r.dt));}
    assert_eq!((ui.phz.w,ui.phz.h),(ui.wins[0].w as usize,ui.wins[0].h as usize));
    // The reported late-session offset can clip a vertically magnified detail,
    // while the same enabled channel remains visible in the unity overview.
    ui.m.run=RunState::Stop;ui.m.ch[0].vscale=15;ui.m.ch[0].offset=-20.81701;
    ui.zoom_vfactor=8.0;ui.zoom_vcenter=0.0;ui.step(0.02);
    assert!(ui.ph.map[0].iter().any(|&v|v>0));assert!(ui.phz.map[0].iter().all(|&v|v==0));
    let close=ui.zoom_close();tap(&mut ui,Rect::new(close.x-76,close.y,70,close.h));ui.step(0.02);
    assert!(ui.phz.map[0].iter().any(|&v|v>0),"Reset must recover clipped detail without changing captured samples");
    assert_eq!((ui.m.ch[0].vscale,ui.m.ch[0].offset),(15,-20.81701));
}

#[test]
fn fit_record_recovers_selected_reference_time_and_voltage_without_changing_trigger_or_other_sources() {
    let mut ui=mixed();ui.refs[1]=ui.refs[0].clone();ui.select_workspace_source(WaveId::Ref(0));
    ui.refs[0].as_mut().unwrap().vpd=10.0;ui.refs[0].as_mut().unwrap().offset=50.0;
    ui.m.delay=123.0;ui.set_zoom(true);ui.zoom_vfactor=16.0;ui.zoom_vcenter=10.0;
    let record=ui.refs[0].as_ref().unwrap();let metadata=(record.v.as_ptr(),record.v.len(),record.dt,record.t0);
    let other=(ui.refs[1].as_ref().unwrap().vpd,ui.refs[1].as_ref().unwrap().offset);let trigger=ui.m.trigger();let channels=ui.m.ch.clone();
    assert!(ui.fit_selected_record());assert!(!ui.m.zoom && !ui.zoom_arm);
    let record=ui.refs[0].as_ref().unwrap();assert_eq!((record.v.as_ptr(),record.v.len(),record.dt,record.t0),metadata);
    assert!(record.vpd<10.0 && record.offset.abs()<0.01);
    let half=ui.m.time_per_div() as f64*HDIV as f64*0.5;let center=ui.m.delay as f64;
    assert!(center-half<=record.t0 && center+half>=record.t0+record.dt*(record.v.len()-1) as f64);
    assert_eq!((ui.refs[1].as_ref().unwrap().vpd,ui.refs[1].as_ref().unwrap().offset),other);
    assert_eq!(ui.m.trigger(),trigger);assert_eq!(ui.m.ch,channels);assert_eq!((ui.m.run,ui.records,ui.replay.captures),(RunState::Stop,0,0));
}

#[test]
fn fit_record_exposes_native_capture_extent_without_copying_or_reconfiguring_the_adc() {
    let mut ui=mixed();ui.select_workspace_source(WaveId::Ch(0));
    let frame=std::sync::Arc::new(crate::board::Frame{dt:1e-9,pre:50_000,ch:[vec![0.1;100_000],Vec::new(),Vec::new(),Vec::new()],..Default::default()});
    let mut record=crate::board::to_record(&frame,&ui.m,3600,1e-6,-0.5e-6);
    let raw=record.raw.as_mut().unwrap();raw.t0=0.001+raw.i0 as f64*frame.dt;
    ui.last[0]=Some(record);ui.m.delay=-100.0;ui.set_zoom(true);
    let channels=ui.m.ch.clone();let trigger=ui.m.trigger();let pointer=frame.ch[0].as_ptr();
    assert!(ui.fit_selected_record());let (n,dt,t0)=ui.raw_info(0).unwrap();
    assert_eq!((n,dt),(100_000,1e-9));assert!((t0-0.001).abs()<1e-15);assert_eq!(pointer,frame.ch[0].as_ptr());
    let half=ui.m.time_per_div() as f64*HDIV as f64*0.5;let center=ui.m.delay as f64;
    assert!(center-half<=t0 && center+half>=t0+(n-1) as f64*dt);
    assert_eq!(ui.m.ch,channels);assert_eq!(ui.m.trigger(),trigger);assert!(!ui.m.zoom);
    assert_eq!((ui.m.run,ui.records,ui.replay.captures),(RunState::Stop,0,0));
}

#[test]
fn close_corner_and_escape_remain_closed_with_histogram_box_and_overlapping_summary_headers() {
    for running in [false,true] {for armed in [false,true] {for exit in 0..3 {
        let mut ui=mixed();ui.m.run=RunState::Run;ui.step(0.02);
        ui.m.run=if running{RunState::Run}else{RunState::Stop};ui.m.hist=Hist::Horizontal;
        ui.m.ch[0].vscale=15;ui.m.ch[0].offset=-20.81701;ui.m.delay=819.2e-9;
        ui.set_zoom(true);ui.zoom_arm=armed;
        let close=ui.zoom_close();ui.stats_on=true;ui.workspace.measure_float=Some((close.x-10,close.y));
        let trigger=ui.m.trigger();let counters=(ui.records,ui.replay.captures);let mode=ui.m.run;
        if exit==0 {tap(&mut ui,close);} else if exit==1 {let corner=ui.zoom_btn().1;tap(&mut ui,corner);}
        else {ui.dismiss_context();if ui.m.zoom{ui.dismiss_context();}}
        assert!(!ui.m.zoom && !ui.zoom_arm,"exit{exit},armed{armed},running{running}");
        assert_eq!((ui.records,ui.replay.captures),counters);assert_eq!(ui.m.trigger(),trigger);assert_eq!(ui.m.run,mode);
        for _ in 0..4 {ui.step(0.02);assert!(!ui.m.zoom,"a step reopened zoom");}
    }}}
}

#[test]
fn explicit_zoom_close_survives_pending_sim_setup_and_fresh_running_or_stopped_captures() {
    for running in [false,true] {
        let mut ui=Ui::new(Source::Demo{t:0.0});assert!(ui.sim_preset_set("USB 2.0 HS"));ui.set_zoom(true);
        ui.sim_play_set(running).unwrap();let close=ui.zoom_close();tap(&mut ui,close);assert!(!ui.m.zoom);
        ui.wait_sim_generation();assert!(!ui.m.zoom,"late initial generation reopened zoom");
        for _ in 0..6 {ui.step(0.2);std::thread::sleep(std::time::Duration::from_millis(2));assert!(!ui.m.zoom);}
        assert_eq!(ui.m.run,if running{RunState::Run}else{RunState::Stop});
    }
}

#[test]
fn former_eye_plot_target_is_plain_waveform_and_auto_eye_remains_in_the_menu() {
    let mut ui=mixed();let a=ui.grid;
    tap(&mut ui,Rect::new(a.x+MARK_W+6,a.y+22,96,28));assert_eq!(ui.m.view,AView::Scope);
    ui.open_menu(MenuId::Eye);assert!(ui.items(MenuId::Eye).contains(&Item::EyeAct));
    assert!(ui.fonts().s.width("Bathtub (model)")<=120);
    ui.m.eye_rate=64e9;ui.azr.eye_auto.enabled=true;assert!(ui.eye_rate_caption().starts_with("Est."));
    ui.azr.eye_auto.enabled=false;assert!(!ui.eye_rate_caption().starts_with("Est."));
}

#[test]
fn ctrl_wheel_anchors_main_detail_and_overview_for_reference_native_and_mixed_records() {
    use std::sync::Arc;
    for source in 0..3 { for stack in [false,true] { for surface in 0..3 {
        let mut ui=mixed();
        if source==0 { ui.set_offline(true); }
        for (c,ch) in ui.m.ch.iter_mut().enumerate() { ch.on=source!=0 && c==0; }
        let values:Vec<f32>=(0..20_000).map(|i|(i as f32*0.003).sin()*0.4).collect();
        let native=Arc::new(crate::board::Frame { seq: 42, dt:5e-12,
            ch:[values.clone(),Vec::new(),Vec::new(),Vec::new()],..Default::default() });
        let mut record=Ui::blank_record();record.ch[0]=values[5000..6000].to_vec();
        record.dt=5e-12;record.t0=-25e-9;
        record.raw=Some(crate::wave::Raw { frame:Arc::clone(&native),i0:5000,i1:6000,t0:-25e-9,conditioned:true });
        ui.last[0]=Some(record);
        ui.refs[0]=if source==1 {None} else {Some(RefWave {src:0,color:1,label:"Native-equivalent reference".into(),
            v:values.into(),dt:5e-12,t0:-50e-9,vpd:0.2,offset:0.1,on:true})};
        let reference=ui.refs[0].as_ref().map(|r|Arc::clone(&r.v));
        ui.m.tscale=tscales().iter().position(|&t|t==5e-9f32).unwrap();ui.m.tfine=1.0;ui.m.delay=0.0;
        ui.records=11;ui.wfm_total=11;ui.replay.captures=7;ui.m.run=RunState::Stop;
        ui.relayout();if surface!=0 {ui.set_zoom(true);}if stack {stacked(&mut ui);}
        let selected=if source==1 {WaveId::Ch(0)} else {WaveId::Ref(0)};
        let area=if surface==2 {Ui::split_zoom(ui.grid).0} else {ui.source_area(selected).unwrap()};
        let (x,y)=(area.x+area.w/3,area.y+area.h/3);
        let fraction=(x-area.x) as f64/area.w as f64;
        let before=if surface==1 {ui.view()} else {(ui.m.time_per_div(),ui.m.delay)};
        let anchor=before.1 as f64+(fraction-0.5)*HDIV as f64*before.0 as f64;
        let channels=ui.m.ch.clone();let trigger=ui.m.trigger();
        let vertical=(ui.zoom_vfactor,ui.zoom_vcenter);
        let ref_settings=ui.refs[0].as_ref().map(|r|(r.vpd,r.offset,r.t0,r.dt));
        let detail_before=(ui.m.zscale,ui.m.zdelay);
        let main_before=(ui.m.tscale,ui.m.delay);
        ui.handle(Ev::Wheel{x,y,steps:1,ctrl:true,shift:false,alt:false});
        let after=if surface==1 {ui.view()} else {(ui.m.time_per_div(),ui.m.delay)};
        let held=after.1 as f64+(fraction-0.5)*HDIV as f64*after.0 as f64;
        assert!(after.0<before.0,"source{source} stack{stack} surface{surface}");
        assert!((held-anchor).abs()<before.0 as f64*HDIV as f64*2e-7+1e-18,"anchor{anchor} held{held}");
        if surface==1 {assert_eq!((ui.m.tscale,ui.m.delay),main_before);}
        if surface==2 {assert_eq!((ui.m.zscale,ui.m.zdelay),detail_before);}
        assert_eq!((ui.zoom_vfactor,ui.zoom_vcenter),vertical);
        assert_eq!(ui.m.ch,channels);assert_eq!(ui.m.trigger(),trigger);
        assert_eq!(ui.refs[0].as_ref().map(|r|(r.vpd,r.offset,r.t0,r.dt)),ref_settings);
        ui.handle(Ev::Wheel{x,y,steps:-1,ctrl:true,shift:false,alt:false});
        let restored=if surface==1 {ui.view()} else {(ui.m.time_per_div(),ui.m.delay)};
        assert_eq!(restored.0,before.0);
        assert!((restored.1-before.1).abs()<before.0*1e-5);
        ui.handle(Ev::Wheel{x,y,steps:2,ctrl:false,shift:false,alt:false});
        let panned=if surface==1 {ui.view()} else {(ui.m.time_per_div(),ui.m.delay)};
        assert_eq!(panned.0,restored.0);assert!(panned.1<restored.1);
        ui.step(0.0);
        assert!(Arc::ptr_eq(&native,&ui.last[0].as_ref().unwrap().raw.as_ref().unwrap().frame));
        assert_eq!(native.seq,42);assert_eq!(native.ch[0][5050],(5050f32*0.003).sin()*0.4);
        if let Some(owner)=reference {assert!(Arc::ptr_eq(&owner,&ui.refs[0].as_ref().unwrap().v));}
        assert_eq!((ui.m.run,ui.records,ui.wfm_total,ui.replay.captures),(RunState::Stop,11,11,7));
    }}}
}

#[test]
fn shift_wheel_and_vertical_controls_remain_vertical_while_ctrl_takes_time_zoom() {
    let mut ui=mixed();ui.set_zoom(true);let area=ui.wave_area();
    let (x,y)=(area.x+area.w/3,area.y+area.h/3);
    let time=ui.view();let factor=ui.zoom_vfactor;
    ui.handle(Ev::Wheel{x,y,steps:1,ctrl:false,shift:true,alt:false});
    assert_eq!(ui.view(),time);assert!(ui.zoom_vfactor>factor);
    let vertical=(ui.zoom_vfactor,ui.zoom_vcenter);let time=ui.view();
    ui.handle(Ev::Wheel{x,y,steps:1,ctrl:true,shift:true,alt:false});
    assert!(ui.view().0<time.0);assert_eq!((ui.zoom_vfactor,ui.zoom_vcenter),vertical);
    let time=ui.view();control(&mut ui,"V+");assert_eq!(ui.view(),time);assert!(ui.zoom_vfactor>vertical.0);
}
