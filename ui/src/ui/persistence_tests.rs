// SPDX-License-Identifier: GPL-2.0-or-later
use super::*;

fn queued(batch: usize, persistence: usize, color: bool) -> Ui {
    let shared: crate::board::HwShared = Default::default();
    for seq in 1..=batch {
        let (n,dt,pre)=(12_500usize,0.8e-9,6250usize);
        let amplitude=if seq==batch {1.0} else {2.0};
        let values=(0..n).map(|k| (2.0*std::f64::consts::PI*200e3*(k as f64-pre as f64+0.3)*dt).sin() as f32*amplitude).collect();
        let frame=crate::board::Frame { basic:Default::default(),seq:seq as u64,dt,pre,ch:[values,Vec::new(),Vec::new(),Vec::new()],roll:false,envelope:false,scan:false,partial:false,acq:(1.25e9,n),stamp:0,la:None };
        shared.lock().unwrap().queue.push_back(frame.into());
    }
    let mut ui=Ui::new(Source::Hw{shared,seen:0});
    ui.m.tscale=tscales().iter().position(|&t|(t-1e-6).abs()<1e-12).unwrap();
    ui.m.ch[0].offset=0.0;ui.m.persist=persistence;ui.m.color_grade=color;
    ui.m.meas=vec![(Meas::Vpp,WaveId::Ch(0))];
    // A prior bright batch must not dim the current Min image while it decays.
    ui.vmax=[50_000;NCH];ui.vmax_cg=50_000;
    ui.step(0.05);ui
}

#[test]
fn minimum_persistence_picture_and_brightness_do_not_depend_on_queue_batch_size() {
    for color in [false,true] {
        let one=queued(1,0,color);let many=queued(10,0,color);
        assert_eq!(many.wfm_total,10);
        assert_eq!(many.records,10);
        assert_eq!(many.mstat[0].n,10,"display dropping must never drop acquisition statistics");
        assert_eq!(many.ph.map[0],one.ph.map[0],"only the newest accepted waveform supplies the Min picture");
        let peak=many.ph.map[0].iter().copied().max().unwrap() as u32;
        assert_eq!(many.vmax[0],peak.max(1));assert_eq!(many.vmax[0],one.vmax[0]);
        if color {assert_eq!(many.vmax_cg,one.vmax_cg);}
        let area=many.wave_area();let mut a=Canvas::new(W,H);let mut b=Canvas::new(W,H);
        one.draw_traces(&mut a,area,&one.ph,&one.vmax,one.vmax_cg,false);
        many.draw_traces(&mut b,area,&many.ph,&many.vmax,many.vmax_cg,false);
        assert_eq!(a.px,b.px,"rendered Min brightness is independent of previous peak and batch size");
    }
}

#[test]
fn timed_persistence_retains_intermediate_waveforms_while_min_keeps_latest() {
    let min=queued(10,0,false);let timed=queued(10,1,false);
    let lit=|ui:&Ui|ui.ph.map[0].iter().filter(|&&v|v>0).count();
    assert!(lit(&timed)>lit(&min));
    assert_eq!(timed.mstat[0].n,min.mstat[0].n);
    assert_eq!(timed.mstat[0].n,10);
}
