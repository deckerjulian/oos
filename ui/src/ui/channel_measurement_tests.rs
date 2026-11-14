// SPDX-License-Identifier: GPL-2.0-or-later
use super::*;
use std::time::{Duration, Instant};

fn native(count: usize) -> Ui {
    let mut ui=Ui::new(Source::Demo{t:0.0});
    ui.m.run=RunState::Stop;ui.m.ch.iter_mut().enumerate().for_each(|(c,ch)|ch.on=c==0);
    ui.m.meas=vec![(Meas::Vpp,WaveId::Ch(0))];ui.m.meas2.clear();ui.m.meas_full_record=true;
    let values:Vec<f32>=(0..count).map(|k|if k%40<20 {-0.25}else{0.25}).collect();
    let basic=crate::board::BasicStats::of(&values);
    let frame=Arc::new(crate::board::Frame{seq:1,dt:1e-9,pre:count/2,ch:[values,Vec::new(),Vec::new(),Vec::new()],
        basic:[basic,Default::default(),Default::default(),Default::default()],..Default::default()});
    let mut record=crate::board::to_record(&frame,&ui.m,3600,count as f64*1e-9,-(count as f64)*0.5e-9);
    // A physical origin away from trigger zero catches timestamp/gate errors.
    record.raw.as_mut().unwrap().t0=0.01+record.raw.as_ref().unwrap().i0 as f64*frame.dt;
    ui.last[0]=Some(record);ui
}

fn wait(ui: &mut Ui) -> (Arc<oos_dsp::measure::Analysis>,f64) {
    let deadline=Instant::now()+Duration::from_secs(10);
    loop {
        ui.poll_reference_measurements();
        if let Some(result)=ui.channel_measurement(0,true) {return result;}
        assert!(Instant::now()<deadline,"CH measurement worker did not finish");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn native_full_basic_values_are_inline_and_do_not_start_edge_analysis() {
    let mut ui=native(600_000);
    let before=ui.measurement_statistics_snapshot_for_test();let start=Instant::now();
    ui.measure_last();
    for _ in 0..100 {
        assert_eq!(ui.meas_at(WaveId::Ch(0),Meas::Vpp),0.5);
        assert!(ui.fact_meas(Meas::Vavg,WaveId::Ch(0)).abs()<1e-15);
        assert_eq!(ui.meas_at(WaveId::Ch(0),Meas::Vrms),0.25);
        assert!((ui.meas_at(WaveId::Ch(0),Meas::StdDev)-0.25).abs()<1e-15);
    }
    assert!(start.elapsed()<Duration::from_millis(250));
    assert!(ui.azr.ref_measure_worker.borrow().is_none());
    assert_eq!(ui.measurement_statistics_snapshot_for_test(),before,"polling never counts an acquisition");
    assert!(ui.ana[0].rises.is_empty());assert!(!ui.measurement_busy());
}

#[test]
fn native_full_edge_job_is_nonblocking_shared_and_reused_after_pan() {
    let mut ui=native(600_000);ui.m.meas.push((Meas::Freq,WaveId::Ch(0)));
    let start=Instant::now();ui.measure_last();
    assert!(start.elapsed()<Duration::from_millis(250));
    assert!(ui.measurement_busy());assert!(ui.meas_at(WaveId::Ch(0),Meas::Freq).is_nan());
    let rows=ui.meas_rows();assert!(!rows[0].computing && rows[1].computing);
    assert_eq!(ui.azr.ref_measure_worker.borrow().as_ref().unwrap().submitted,1);
    let (analysis,t0)=wait(&mut ui);assert!((t0-0.01).abs()<1e-12);
    assert!((ui.meas_at(WaveId::Ch(0),Meas::Freq)-25e6).abs()<1.0);
    let reads=Instant::now();
    for _ in 0..1_000 {
        assert!((ui.fact_meas(Meas::PWidth,WaveId::Ch(0))-20e-9).abs()<1e-12);
        assert!((ui.meas_at(WaveId::Ch(0),Meas::PDuty)-50.0).abs()<1e-6);
    }
    assert!(reads.elapsed()<Duration::from_millis(150),"cached width/duty reads took {:?}",reads.elapsed());
    ui.m.delay=1.0;
    let (again,again_t0)=ui.channel_measurement(0,true).unwrap();
    assert!(Arc::ptr_eq(&analysis,&again));assert_eq!(again_t0,t0);
    assert_eq!(ui.azr.ref_measure_worker.borrow().as_ref().unwrap().submitted,1);
    assert_eq!((ui.m.run,ui.records),(RunState::Stop,0));
}

#[test]
fn large_screen_pan_rejects_old_gate_and_preserves_native_timebase() {
    let mut ui=native(1_200_000);ui.m.meas_full_record=false;
    ui.m.meas=vec![(Meas::Freq,WaveId::Ch(0))];
    ui.m.tscale=tscales().iter().position(|&t|(t-50e-6).abs()<1e-10).unwrap();
    ui.m.delay=0.0103;
    let first=ui.channel_measurement_key(0,false).unwrap().0;
    assert!(first.len>BACKGROUND_MEASUREMENT_POINTS);assert!(ui.channel_measurement(0,true).is_none());
    ui.m.delay=0.0109;
    let second=ui.channel_measurement_key(0,false).unwrap().0;
    assert!(first!=second);assert!(ui.channel_measurement(0,true).is_none());
    let (analysis,t0)=wait(&mut ui);
    assert_eq!(ui.azr.ch_measure[0].borrow().as_ref().unwrap().key,second);
    assert!(t0>0.0106 && t0<0.0107);
    assert!((analysis.get(Meas::Period)-40e-9).abs()<1e-12);
    assert_eq!(ui.last[0].as_ref().unwrap().raw.as_ref().unwrap().frame.dt,1e-9);
    assert_eq!((ui.m.run,ui.records),(RunState::Stop,0));
}

#[test]
fn native_cache_rejects_conditioning_changes_and_new_acceptance_without_mutating_samples() {
    let mut ui=native(600_000);ui.m.meas=vec![(Meas::Freq,WaveId::Ch(0))];
    let frame=Arc::clone(&ui.last[0].as_ref().unwrap().raw.as_ref().unwrap().frame);
    let original=frame.ch[0][0].to_bits();
    let first=ui.channel_measurement_key(0,false).unwrap().0;
    assert!(ui.channel_measurement(0,true).is_none());
    ui.m.ch[0].invert=true;ui.m.ch[0].coupling=Coupling::Ac;ui.m.ch[0].bw=BW_20M;
    let changed=ui.channel_measurement_key(0,false).unwrap().0;assert!(first!=changed);
    let _=wait(&mut ui);
    assert_eq!(ui.azr.ch_measure[0].borrow().as_ref().unwrap().key,changed);
    assert_eq!(frame.ch[0][0].to_bits(),original,"settings invalidate results, not immutable captures");
    // Replay can keep seq=1 and an allocator may repeat addresses. The accepted
    // acquisition counter must still invalidate that otherwise identical key.
    ui.wfm_total+=1;
    let accepted=ui.channel_measurement_key(0,false).unwrap().0;assert!(accepted!=changed);
    assert!(ui.channel_measurement(0,true).is_none());let _=wait(&mut ui);
    assert_eq!(ui.azr.ch_measure[0].borrow().as_ref().unwrap().key,accepted);
    assert_eq!(frame.ch[0][0].to_bits(),original);
    assert_eq!((ui.m.run,ui.records),(RunState::Stop,0));
}
