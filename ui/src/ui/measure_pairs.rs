// SPDX-License-Identifier: GPL-2.0-or-later
//! Bounded pair readouts: metadata-only waiters, one job holding shared analyses.
use super::*;
use super::analysis::RefMeasureKey;
use super::tools::{PairKind, PAIRS, MEAS_MAX};
use oos_dsp::measure::{Analysis, Pair, delay, phase, skew, setup_time_at, hold_time_at, setup_hold_violations_at};
use std::{collections::VecDeque, sync::{Arc, mpsc}};

const DIRECT_EDGES: usize = 8_192;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Key {
    kind: usize,
    a: WaveId,
    b: WaveId,
    ai: (RefMeasureKey, usize),
    bi: (RefMeasureKey, usize),
    setup: u64,
    hold: u64,
}
struct Job { key: Key, a: Arc<Analysis>, b: Arc<Analysis>, ta: f64, tb: f64 }
struct Worker { input: mpsc::SyncSender<Job>, output: mpsc::Receiver<(Key,f64)>, busy: bool, submitted: usize }
#[derive(Default)]
pub(super) struct State { cache: VecDeque<(Key,f64)>, pending: VecDeque<Key>, worker: Option<Worker> }

// Only the two mid-edge vectors are needed by pair maths. Copies stay off the
// event thread for long records; jobs retain shared input until they complete.
fn shifted(a: &Analysis, t0: f64) -> Analysis {
    Analysis { valid:a.valid, rises:a.rises.iter().map(|time|time+t0).collect(),
        falls:a.falls.iter().map(|time|time+t0).collect(), ..Default::default() }
}
fn compute(job: &Job) -> f64 {
    let (_,kind,ea,eb)=PAIRS[job.key.kind];
    let (a,b)=(shifted(&job.a,job.ta),shifted(&job.b,job.tb));
    match kind {
        PairKind::Delay=>delay(&a,&b,Pair(ea,eb)), PairKind::Phase=>phase(&a,&b,Pair(ea,eb)),
        PairKind::Skew=>skew(&a,&b,Pair(ea,eb)), PairKind::Setup=>setup_time_at(&a,&b,ea),
        PairKind::Hold=>hold_time_at(&a,&b,ea),
        PairKind::SetupAndHold=>setup_hold_violations_at(&a,&b,f64::from_bits(job.key.setup),f64::from_bits(job.key.hold),ea),
    }
}
impl Worker {
    fn open() -> Self {
        let (input,jobs)=mpsc::sync_channel::<Job>(1);let (results,output)=mpsc::channel();
        std::thread::Builder::new().name("measurement-pairs".into()).spawn(move||{
            while let Ok(job)=jobs.recv() {
                let value=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||compute(&job))).unwrap_or(f64::NAN);
                if results.send((job.key,value)).is_err(){break;}
            }
        }).expect("start bounded pair worker");
        Self{input,output,busy:false,submitted:0}
    }
}
impl State {
    fn put(&mut self,key:Key,value:f64) {
        self.cache.retain(|(old,_)|*old!=key);
        self.cache.push_front((key,value));self.cache.truncate(MEAS_MAX);
        self.pending.retain(|old|*old!=key);
    }
}
impl Ui {
    fn pair_key(&self,kind:usize,a:WaveId,b:WaveId)->Option<Key> {
        Some(Key{kind:kind.min(PAIRS.len()-1),a,b,ai:self.measurement_identity(a)?,bi:self.measurement_identity(b)?,
            setup:self.m.meas_tsetup.to_bits(),hold:self.m.meas_thold.to_bits()})
    }
    pub(super) fn cached_pair(&self,kind:usize,a:WaveId,b:WaveId)->f64 {
        let (Some((aa,ta)),Some((bb,tb)))=(self.meas_analysis(a),self.meas_analysis(b)) else{return f64::NAN};
        let Some(key)=self.pair_key(kind,a,b) else{return f64::NAN};
        let mut state=self.azr.measure_pairs.borrow_mut();
        if let Some((_,value))=state.cache.iter().find(|(old,_)|*old==key){return *value;}
        let job=Job{key,a:aa,b:bb,ta,tb};
        if job.a.rises.len()+job.a.falls.len()+job.b.rises.len()+job.b.falls.len()<=DIRECT_EDGES {
            let value=compute(&job);state.put(key,value);return value;
        }
        if !state.pending.contains(&key){state.pending.push_back(key);while state.pending.len()>MEAS_MAX{state.pending.pop_front();}}
        let worker=state.worker.get_or_insert_with(Worker::open);
        if !worker.busy && worker.input.try_send(job).is_ok(){worker.busy=true;worker.submitted+=1;}
        f64::NAN
    }
    pub(super) fn pair_measurement_computing(&self,kind:usize,a:WaveId,b:WaveId)->bool {
        let Some(key)=self.pair_key(kind,a,b) else{return false};
        self.azr.measure_pairs.borrow().pending.contains(&key)
    }
    pub(super) fn pair_measurement_busy(&self)->bool {
        self.azr.measure_pairs.borrow().pending.iter().any(|key|self.pair_key(key.kind,key.a,key.b)==Some(*key))
    }
    pub(super) fn poll_pair_measurements(&mut self) {
        let mut completed=Vec::new();
        {
            let mut state=self.azr.measure_pairs.borrow_mut();
            if let Some(worker)=state.worker.as_mut(){while let Ok(result)=worker.output.try_recv(){worker.busy=false;completed.push(result);}}
        }
        let mut changed=false;
        for (key,value) in completed {
            if self.pair_key(key.kind,key.a,key.b)==Some(key){self.azr.measure_pairs.borrow_mut().put(key,value);changed=true;}
        }
        let next={
            let mut state=self.azr.measure_pairs.borrow_mut();
            state.pending.retain(|key|self.pair_key(key.kind,key.a,key.b)==Some(*key));
            state.pending.front().copied()
        };
        if let Some(key)=next {let _=self.cached_pair(key.kind,key.a,key.b);}
        if changed {self.finish_deferred_capture_statistics();self.dirty_map=true;}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration,Instant};
    fn fixture()->Ui {
        let mut ui=Ui::new(Source::Demo{t:0.0});ui.m.ch.iter_mut().for_each(|ch|ch.on=false);
        ui.m.run=RunState::Stop;ui.m.meas_full_record=true;ui.m.meas.clear();ui.m.meas2=vec![(0,WaveId::Ref(0),WaveId::Ref(1))];
        let values:Vec<f32>=(0..400_000).map(|i|if i%40<20{-0.5}else{0.5}).collect();
        for i in 0..2 {ui.refs[i]=Some(RefWave{src:0,color:i,label:String::new(),t0:0.001+i as f64*3e-9,dt:1e-9,v:values.clone(),vpd:0.2,offset:0.0,on:true});}
        let deadline=Instant::now()+Duration::from_secs(10);
        loop {ui.measure_refs();ui.poll_reference_measurements();
            if ui.meas_analysis(WaveId::Ref(0)).is_some()&&ui.meas_analysis(WaveId::Ref(1)).is_some(){break;}
            assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(2));
        }
        ui
    }
    fn wait(ui:&mut Ui,kind:usize)->f64 {
        let deadline=Instant::now()+Duration::from_secs(10);
        loop {ui.poll_reference_measurements();let value=ui.pair_at(kind,WaveId::Ref(0),WaveId::Ref(1));
            if !ui.pair_measurement_computing(kind,WaveId::Ref(0),WaveId::Ref(1))&&value.is_finite(){return value;}
            assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn large_pair_is_background_cached_in_physical_time_and_full_pan_does_not_resubmit() {
        let mut ui=fixture();let before=ui.measurement_statistics_snapshot_for_test();
        let start=Instant::now();assert!(ui.pair_at(0,WaveId::Ref(0),WaveId::Ref(1)).is_nan());
        for _ in 0..100 {assert!(ui.pair_at(0,WaveId::Ref(0),WaveId::Ref(1)).is_nan());}
        assert!(start.elapsed()<Duration::from_millis(100));assert!(ui.measurement_busy());
        assert!(ui.meas_rows()[0].computing);
        assert_eq!(ui.azr.measure_pairs.borrow().worker.as_ref().unwrap().submitted,1);
        let value=wait(&mut ui,0);assert!((value-3e-9).abs()<1e-15,"{value}");
        ui.m.delay=123.0;
        assert_eq!(ui.pair_at(0,WaveId::Ref(0),WaveId::Ref(1)),value);
        assert_eq!(ui.azr.measure_pairs.borrow().worker.as_ref().unwrap().submitted,1);
        assert_eq!(ui.measurement_statistics_snapshot_for_test(),before);
        assert_eq!((ui.m.run,ui.replay.captures),(RunState::Stop,0));
    }
    #[test]
    fn pair_result_rejects_stale_setup_hold_settings_and_source_reload() {
        let mut ui=fixture();ui.m.meas_thold=4e-9;
        assert!(ui.fact_setup_hold(WaveId::Ref(0),WaveId::Ref(1)).is_nan());
        ui.m.meas_thold=1e-9;
        assert_eq!(wait(&mut ui,14),0.0);
        ui.refs[1].as_mut().unwrap().t0+=5e-9;
        let value=wait(&mut ui,0);assert!((value-8e-9).abs()<1e-15,"{value}");
        assert!(ui.azr.measure_pairs.borrow().cache.len()<=MEAS_MAX);
    }
}
