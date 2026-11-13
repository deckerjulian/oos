// SPDX-License-Identifier: GPL-2.0-or-later
//! Bounded catalog jobs over immutable sources; no waveform DSP on the render thread.
use super::*;
use std::{cell::RefCell,collections::VecDeque,sync::{Arc,Weak,mpsc}};

type Row=(u16,WaveId,WaveId,WaveId,WaveId);
const MAX_POINTS:usize=32_000_000;
const MAX_ALIGNED_BYTES:usize=256<<20;
const MAX_SMALL_COPY:usize=262_144;
#[derive(Clone,Copy,PartialEq,Eq,Debug)]
enum Family{Rate,Eye,Common,Quality,Harmonics,Inrush,InputC,Turn,Switch,Magnetic,Ripple,LineRipple,SwRipple,Ratio,Motor,Mechanical,Rail,RailTurn,Pulse}
fn family(c:Cat)->Family{
    use Cat::*;
    match c{
        UnitInterval|DataRate=>Family::Rate,DcCommon|AcCommon|DiffCross=>Family::Common,
        PowerQuality|PowerFactor=>Family::Quality,Harmonics=>Family::Harmonics,Inrush=>Family::Inrush,InputC=>Family::InputC,
        TurnOn|TurnOff=>Family::Turn,EOn|EOff|ECond|DvdtRise|DvdtFall|DidtRise|DidtFall|RdsOn=>Family::Switch,
        Inductance|MagProp|MagLoss=>Family::Magnetic,Ripple=>Family::Ripple,LineRipple=>Family::LineRipple,SwRipple=>Family::SwRipple,
        Psrr|Impedance=>Family::Ratio,Dq0|Speed|Accel|Direction|Angle=>Family::Motor,Torque|MechPower|SysEff=>Family::Mechanical,
        RailOver|RailUnder|DcRail=>Family::Rail,RailTurn=>Family::RailTurn,
        Vpeak|Ipeak|TdOn|Tr|Ton|TdOff|Tf|Toff|DpdtV|DpdtI|Tdt|Trr|Qrr|Err|Irrm|DiodeDdt|Qoss=>Family::Pulse,
        _=>Family::Eye,
    }
}
#[derive(Clone,PartialEq,Eq,Debug)]
struct SourceKey{id:WaveId,version:u64,lo:usize,hi:usize,len:usize,t0:u64,dt:u64,capture:u64,conditioning:(u8,bool,usize,u8)}
#[derive(Clone,PartialEq,Eq,Debug)]
struct Key{family:Family,sources:Vec<SourceKey>,roles:Vec<WaveId>,settings:Vec<u64>,epoch:u64,full:bool}
#[derive(Clone)]
enum Samples{Reference(Arc<Vec<f32>>),Native(Arc<crate::board::Frame>,usize),Small(Arc<Vec<f32>>)}
impl Samples{fn values(&self)->&[f32]{match self{Self::Reference(v)|Self::Small(v)=>v,Self::Native(f,c)=>&f.ch[*c]}}}
#[derive(Clone)]
struct Input{id:WaveId,samples:Samples,lo:usize,hi:usize,t0:f64,dt:f64}
impl Input{fn values(&self)->&[f32]{&self.samples.values()[self.lo..self.hi]}fn start(&self)->f64{self.t0+self.lo as f64*self.dt}fn end(&self)->f64{self.t0+(self.hi-1)as f64*self.dt}}
enum Owner{Reference(Weak<Vec<f32>>),Native(Weak<crate::board::Frame>),Small(usize,u64,u64)}
struct Version{id:WaveId,owner:Owner,version:u64}
impl Owner{fn same(&self,other:&Self)->bool{match(self,other){(Self::Reference(a),Self::Reference(b))=>Weak::ptr_eq(a,b),(Self::Native(a),Self::Native(b))=>Weak::ptr_eq(a,b),(Self::Small(a,ac,ag),Self::Small(b,bc,bg))=>a==b&&ac==bc&&ag==bg,_=>false}}}
struct Job{key:Key,row:Row,inputs:Vec<Input>,model:Model,capture:u64,stats_epoch:u64}
struct Batch{values:Vec<(Cat,f64)>,quality:&'static str}
impl std::ops::Deref for Batch{type Target=[(Cat,f64)];fn deref(&self)->&Self::Target{&self.values}}
struct Finished{key:Key,result:Result<Batch,&'static str>,capture:u64,stats_epoch:u64}
struct Worker{send:mpsc::SyncSender<Job>,receive:mpsc::Receiver<Finished>,busy:bool,submitted:usize}
impl Worker{fn new()->Self{let(send,requests)=mpsc::sync_channel::<Job>(1);let(results,receive)=mpsc::sync_channel(1);
    std::thread::Builder::new().name("catalog-measurements".into()).spawn(move||while let Ok(job)=requests.recv(){
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||compute(&job))).unwrap_or(Err("Catalog analysis failed"));
        if results.send(Finished{key:job.key,result,capture:job.capture,stats_epoch:job.stats_epoch}).is_err(){break;}
    }).expect("start catalog worker");Self{send,receive,busy:false,submitted:0}}}
#[derive(Default)]
pub(in crate::ui) struct State{versions:Vec<Version>,next_version:u64,cache:VecDeque<Finished>,pending:VecDeque<(Key,Row)>,worker:Option<Worker>,stats_epoch:u64,stats:Vec<(Row,u64,Key,u64)>,small:Vec<(WaveId,usize,u64,u64,Arc<Vec<f32>>)>}
pub(in crate::ui) struct Reading{pub value:f64,pub computing:bool,pub reason:Option<&'static str>,pub quality:&'static str}
impl State{
    fn version(&mut self,id:WaveId,owner:Owner)->u64{
        if let Some(old)=self.versions.iter().find(|old|old.id==id&&old.owner.same(&owner)){return old.version;}
        self.next_version=self.next_version.checked_add(1).expect("catalog source version exhausted");
        self.versions.retain(|old|old.id!=id);self.versions.push(Version{id,owner,version:self.next_version});self.next_version
    }
    fn put(&mut self,answer:Finished){self.cache.retain(|old|old.key!=answer.key);self.pending.retain(|(key,_)|*key!=answer.key);
        self.cache.push_front(answer);self.cache.truncate(tools::MEAS_MAX);}
}
fn settings(m:&Model,f:Family)->Vec<u64>{
    let mut v=vec![];let mut put=|x:f64|v.push(x.to_bits());
    match f{
        Family::Eye=>{for x in[m.eye_rate,m.eye_ber,m.eye_pn_lo,m.eye_pn_hi,m.eye_notch]{put(x);}v.extend([m.eye_hp as u64,m.eye_npj as u64]);}
        Family::Common=>v.push(m.eye_hp as u64),
        Family::Magnetic=>for x in[m.mag_n,m.mag_ae,m.mag_le,m.mag_rcu]{put(x);},
        Family::LineRipple|Family::Quality|Family::Harmonics|Family::Ratio=>put(m.pw_fline),Family::SwRipple=>put(m.pw_fsw),
        Family::Motor|Family::Mechanical=>{put(m.inv_scale);v.push(m.inv_poles as u64);for id in[m.inv_ev,m.inv_ei]{v.push(id.map_or(u64::MAX,wave_number));}}
        Family::Rail|Family::RailTurn=>v.push(m.pw_ref.map_or(u64::MAX,f64::to_bits)),
        Family::Pulse=>{put(m.dpt_pct);v.push(m.dpt_4th as u64);v.push(wave_number(m.dpt_4ch));},
        _=>{}
    }v
}
fn wave_number(id:WaveId)->u64{match id{WaveId::Ch(c)=>c as u64,WaveId::Ref(r)=>(NCH+r)as u64}}
fn role_ids(c:Cat,row:[WaveId;4],m:&Model)->Result<Vec<WaveId>,&'static str>{
    let mut ids=catalog_roles(c).iter().map(|&r|row[r]).collect::<Vec<_>>();
    match family(c){
        Family::Motor|Family::Mechanical=>{if row[0]==row[1]||row[0]==row[2]||row[1]==row[2]{return Err("Three distinct phase sources required");}
            if family(c)==Family::Mechanical{if m.inv_ev.is_some()!=m.inv_ei.is_some(){return Err("Electrical voltage and current sources required together");}ids.extend(m.inv_ev);ids.extend(m.inv_ei);}},
        Family::Pulse=>{if m.dpt_4th!=DptFourth::Off{ids.push(m.dpt_4ch);}},_=>{}
    }
    if ids.is_empty(){return Err("Catalog source roles missing");}Ok(ids)
}
impl Ui{
    fn catalog_input(&self,id:WaveId,state:&mut State)->Result<(Input,SourceKey),&'static str>{
        let(samples,len,t0,dt,owner,capture,conditioning)=match id{
            WaveId::Ref(slot)=>{let r=self.refs.get(slot).and_then(Option::as_ref).ok_or("Reference source empty")?;
                let revision=if self.session_is_sim&&self.m.sim_slot==slot{self.replay.sim_revision}else{0};
                (Samples::Reference(Arc::clone(&r.v)),r.v.len(),self.source_time_origin(id,r.t0),r.dt,Owner::Reference(Arc::downgrade(&r.v)),revision,(0,false,0,0))},
            WaveId::Ch(c)=>{if c>=n_ch()||!self.m.ch[c].on{return Err("Channel source is off");}let record=self.last[0].as_ref().ok_or("No acquired record")?;
                let config=&self.m.ch[c];let conditioning=(config.coupling as u8,config.invert,config.bw,self.m.acq as u8);
                if let Some(raw)=record.raw.as_ref().filter(|raw|self.m.acq!=AcqMode::HiRes&&!raw.frame.envelope&&raw.frame.ch[c].len()>=2){
                    (Samples::Native(Arc::clone(&raw.frame),c),raw.frame.ch[c].len(),self.source_time_origin(id,raw.t0-raw.i0 as f64*raw.frame.dt),raw.frame.dt,
                        Owner::Native(Arc::downgrade(&raw.frame)),raw.frame.seq,conditioning)
                }else{let values=&record.ch[c];if values.len()>MAX_SMALL_COPY{return Err("Large processed channel needs an immutable captured record");}
                    // Processed fallback is bounded to 1MiB. A Clear/reset generation is part
                    // of both owner and snapshot identities because capture numbers can restart.
                    // Native long records never enter this copy path.
                    ({
                        let pointer=values.as_ptr()as usize;
                        if state.small.iter().all(|(id,p,capture,generation,_)|*id!=WaveId::Ch(c)||*p!=pointer||*capture!=self.wfm_total||*generation!=state.stats_epoch){
                            state.small.retain(|(id,_,_,_,_)|*id!=WaveId::Ch(c));state.small.push((WaveId::Ch(c),pointer,self.wfm_total,state.stats_epoch,Arc::new(values.clone())));
                        }
                        Samples::Small(Arc::clone(&state.small.iter().find(|(id,_,_,_,_)|*id==WaveId::Ch(c)).unwrap().4))
                    },values.len(),self.source_time_origin(id,record.t0),record.dt,
                        Owner::Small(values.as_ptr()as usize,self.wfm_total,state.stats_epoch),self.wfm_total,conditioning)}
            }
        };
        if len<2||len>MAX_POINTS||!t0.is_finite()||!dt.is_finite()||dt<=0.0{return Err("Invalid waveform source");}
        let(lo,hi)=self.measurement_bounds(len,t0,dt);if hi<lo+2{return Err("No samples in measurement gate");}
        let version=state.version(id,owner);
        Ok((Input{id,samples,lo,hi,t0,dt},SourceKey{id,version,lo,hi,len,t0:t0.to_bits(),dt:dt.to_bits(),capture,conditioning}))
    }
    fn catalog_snapshot(&self,c:Cat,row:[WaveId;4])->Result<(Key,Vec<Input>),&'static str>{
        if !matches!(meta(c).kind,CatKind::Wave){return Err("Catalog entry uses a legacy measurement");}
        let roles=role_ids(c,row,&self.m)?;let mut state=self.azr.catalog_measurements.borrow_mut();let mut inputs=Vec::new();let mut keys=Vec::new();
        for &id in &roles{if inputs.iter().any(|input:&Input|input.id==id){continue;}let(input,key)=self.catalog_input(id,&mut state)?;inputs.push(input);keys.push(key);}
        Ok((Key{family:family(c),sources:keys,roles,settings:settings(&self.m,family(c)),epoch:self.workflow_epoch,full:self.m.meas_full_record},inputs))
    }
    pub(in crate::ui) fn catalog_reading(&self,c:Cat,row:[WaveId;4])->Reading{
        let quality=if family(c)==Family::Eye{"NRZ estimate / jitter model"}else{"Derived from scaled source roles"};
        let(key,inputs)=match self.catalog_snapshot(c,row){Ok(value)=>value,Err(reason)=>return Reading{value:f64::NAN,computing:false,reason:Some(reason),quality}};
        let mut state=self.azr.catalog_measurements.borrow_mut();
        if let Some(answer)=state.cache.iter().find(|answer|answer.key==key){return match &answer.result{
            Ok(values)=>Reading{value:values.iter().find(|(id,_)|*id==c).map_or(f64::NAN,|(_,v)|*v),computing:false,reason:None,quality:values.quality},
            Err(reason)=>Reading{value:f64::NAN,computing:false,reason:Some(*reason),quality}};}
        let tuple=(c as u16,row[0],row[1],row[2],row[3]);
        if !state.pending.iter().any(|(old,_)|*old==key){state.pending.push_back((key.clone(),tuple));while state.pending.len()>tools::MEAS_MAX{state.pending.pop_front();}}
        let stats_epoch=state.stats_epoch;
        let worker=state.worker.get_or_insert_with(Worker::new);
        if !worker.busy{let job=Job{key,row:tuple,inputs,model:self.m.clone(),capture:self.wfm_total,stats_epoch};if worker.send.try_send(job).is_ok(){worker.busy=true;worker.submitted+=1;}}
        Reading{value:f64::NAN,computing:true,reason:None,quality}
    }
    pub(in crate::ui) fn catalog_measurement_busy(&self)->bool{let pending=self.azr.catalog_measurements.borrow().pending.iter().map(|(_,r)|*r).collect::<Vec<_>>();
        pending.iter().any(|&(id,a,b,c,d)|cat_from(id).is_some_and(|cat|self.catalog_snapshot(cat,[a,b,c,d]).is_ok()))}
    pub(in crate::ui) fn queue_catalog_statistics(&self,row:Row,capture:u64){
        let(id,a,b,c,d)=row;let Some(cat)=cat_from(id)else{return};let Ok((key,_))=self.catalog_snapshot(cat,[a,b,c,d])else{return};
        let mut state=self.azr.catalog_measurements.borrow_mut();state.stats.retain(|(stored,_,_,_)|*stored!=row);let epoch=state.stats_epoch;state.stats.push((row,capture,key,epoch));state.stats.truncate(tools::MEAS_MAX);
    }
    pub(in crate::ui) fn reset_catalog_statistics(&self){let mut state=self.azr.catalog_measurements.borrow_mut();state.stats.clear();state.stats_epoch=state.stats_epoch.checked_add(1).expect("catalog statistics generation exhausted");}
    pub(in crate::ui) fn poll_catalog_measurements(&mut self){
        let finished={let mut state=self.azr.catalog_measurements.borrow_mut();state.worker.as_mut().and_then(|worker|worker.receive.try_recv().ok().map(|answer|{worker.busy=false;answer}))};
        let mut changed=false;
        if let Some(answer)=finished{
            let rows=self.m.cat.clone();let current=rows.iter().any(|&(id,a,b,c,d)|cat_from(id).is_some_and(|cat|self.catalog_snapshot(cat,[a,b,c,d]).is_ok_and(|(key,_)|key==answer.key)));
            let mut accepted=Vec::new();
            {let mut state=self.azr.catalog_measurements.borrow_mut();state.pending.retain(|(key,_)|*key!=answer.key);
                if current{
                    if let Ok(values)=&answer.result{for (row,capture,key,epoch)in &state.stats{if *key==answer.key&&*capture==answer.capture&&*capture==self.wfm_total&&*epoch==answer.stats_epoch&&*epoch==state.stats_epoch{
                        if let Some((_,value))=values.iter().find(|(cat,_)|*cat as u16==row.0){accepted.push((*row,*value,*capture));}
                    }}}
                    state.stats.retain(|(_,capture,key,_)|*capture==self.wfm_total&&*key!=answer.key);state.put(answer);changed=true;
                }}
            for (row,value,capture)in accepted{self.accept_catalog_stat(row,value,capture);}
        }
        let pending=self.azr.catalog_measurements.borrow().pending.iter().map(|(_,row)|*row).collect::<Vec<_>>();
        for row@(id,a,b,c,d) in pending{let current=cat_from(id).and_then(|cat|self.catalog_snapshot(cat,[a,b,c,d]).ok().map(|(key,_)|key));
            let mut state=self.azr.catalog_measurements.borrow_mut();state.pending.retain(|(key,stored)|*stored!=row||Some(&*key)==current.as_ref());}
        let next=self.azr.catalog_measurements.borrow().pending.front().cloned();
        if let Some((_,(id,a,b,c,d)))=next{if let Some(cat)=cat_from(id){let _=self.catalog_reading(cat,[a,b,c,d]);}}
        if changed{self.finish_deferred_capture_statistics();self.dirty_map=true;}
    }
}
fn snap(x:f64)->f64{if(x-x.round()).abs()<1e-7{x.round()}else{x}}
#[derive(Debug)]
struct Aligned{dt:f64,t0:f64,inputs:Vec<(WaveId,Vec<f32>)>}
fn align_inputs(inputs:&[Input])->Result<Aligned,&'static str>{
    let slow=inputs.iter().max_by(|a,b|a.dt.total_cmp(&b.dt)).ok_or("No source roles")?;
    let start=inputs.iter().map(Input::start).fold(f64::NEG_INFINITY,f64::max);
    let end=inputs.iter().map(Input::end).fold(f64::INFINITY,f64::min);let dt=slow.dt;
    let first=snap((start-slow.t0)/dt).ceil();let last=snap((end-slow.t0)/dt).floor();
    if !first.is_finite()||!last.is_finite()||last-first<1.0{return Err("Catalog sources do not overlap");}
    let n=(last-first+1.0)as usize;
    if n>MAX_POINTS||n.checked_mul(inputs.len()).and_then(|n|n.checked_mul(4)).is_none_or(|bytes|bytes>MAX_ALIGNED_BYTES){return Err("Catalog alignment exceeds memory bound; shorten measurement gate");}
    let t0=slow.t0+first*dt;let mut out=Vec::new();
    for input in inputs{let values=input.samples.values();let mut data=Vec::with_capacity(n);
        for i in 0..n{let x=snap((t0+i as f64*dt-input.t0)/input.dt);if x<input.lo as f64||x>(input.hi-1)as f64{return Err("Source outside common physical interval");}
            let k=x.floor()as usize;let fraction=(x-k as f64)as f32;let value=if fraction==0.0{values[k]}else{values[k]+(values[k+1]-values[k])*fraction};data.push(value);}
        out.push((input.id,data));}
    Ok(Aligned{dt,t0,inputs:out})
}
struct Evaluator{quality:RefCell<Option<oos_dsp::power::Quality>>,rail:RefCell<Option<oos_dsp::power::Rail>>,rate:RefCell<Option<oos_dsp::measure::Analysis>>,model:Model,t0:f64,dt:f64,inputs:Vec<(WaveId,Vec<f32>)>,eye:RefCell<Option<Option<EyeSnap>>>,switching:RefCell<Option<oos_dsp::power::Switching>>,
    magnetic:RefCell<Option<Arc<oos_dsp::magnetic::Magnetic>>>,shaft:RefCell<Option<Arc<oos_dsp::motor::Shaft>>>,mechanical:RefCell<Option<Arc<oos_dsp::motor::Mech>>>,pulse:RefCell<Option<Arc<oos_dsp::dpt::Pulse>>>}
impl Evaluator{
    fn wave_vec(&self,id:WaveId)->Option<(&[f32],f64,f64)>{self.inputs.iter().find(|(source,_)|*source==id).map(|(_,v)|(v.as_slice(),self.t0,self.dt))}
    fn align(&self,ids:&[WaveId])->Option<(f64,Vec<&[f32]>)>{Some((self.dt,ids.iter().map(|&id|self.wave_vec(id).map(|w|w.0)).collect::<Option<_>>()?))}
}
fn compute(job:&Job)->Result<Batch,&'static str>{
    let aligned=align_inputs(&job.inputs)?;let eval=Evaluator{quality:RefCell::new(None),rail:RefCell::new(None),rate:RefCell::new(None),model:job.model.clone(),t0:aligned.t0,dt:aligned.dt,inputs:aligned.inputs,eye:RefCell::new(None),switching:RefCell::new(None),magnetic:RefCell::new(None),shaft:RefCell::new(None),mechanical:RefCell::new(None),pulse:RefCell::new(None)};
    let(_,a,b,c,d)=job.row;
    let values=listed().into_iter().filter(|&cat|matches!(meta(cat).kind,CatKind::Wave)&&family(cat)==job.key.family&&catalog_roles(cat)==catalog_roles(cat_from(job.row.0).unwrap()))
        .map(|cat|(cat,eval.cat_number(cat,a,b,c,d))).collect();
    let quality=if job.key.family==Family::Eye{
        eval.eye.borrow().as_ref().and_then(|eye|eye.as_ref()).map_or("NRZ estimate: insufficient transitions or sampling",|eye|eye.quality)
    }else if job.key.family==Family::Pulse{
        eval.pulse.borrow().as_ref().map_or("DPT: no qualifying pulse",|pulse|if pulse.sample_limit{"DPT estimate: sample limited"}else if pulse.diode_from_id||pulse.err_from_vds||pulse.qoss_approx{"DPT estimate: inferred recovery or charge"}else{"Derived DPT result"})
    }else{"Derived from scaled source roles"};
    Ok(Batch{values,quality})
}
#[derive(Clone)]
struct EyeSnap {
    quality: &'static str,
    period: f64,
    rate: f64,
    tj: f64,
    tie_pp: f64,
    rj: f64,
    dj: f64,
    pj: f64,
    sj: f64,
    ddj: f64,
    dcd: f64,
    srj: f64,
    j2: f64,
    pn: f64,
    one: f64,
    zero: f64,
    amp: f64,
    height: f64,
    width: f64,
    height_ber: f64,
    width_ber: f64,
    q: f64,
    s1: f64,
    s0: f64,
    ssc_ppm: f64,
    ssc_hz: f64,
    tnt: f64,
}

fn eye_of(v: &[f32], dt: f64, ber: f64, pn_lo: f64, pn_hi: f64, rate: f64) -> Option<EyeSnap> {
    if v.len() < 16 || !(dt > 0.0) {
        return None;
    }
    let finite = v.iter().copied().filter(|x| x.is_finite());
    if finite.clone().take(16).count() < 16 {
        return None;
    }
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for x in finite {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    if !(hi > lo) {
        return None;
    }
    let (summary, clk) = super::super::eyepage::automatic::infer(v, dt, (rate>0.0).then_some(rate), false).ok()?;
    let tie = oos_dsp::eye::tie(&clk.edges, &clk);
    let j = oos_dsp::eye::jitter(&tie);
    let mut means = oos_dsp::eye::EdgeMeans::default();
    means.add(&tie, &clk);
    let trend = oos_dsp::eye::tie_trend(&clk.edges, &tie, &clk);
    let (df, spec) = oos_dsp::eye::tie_spectrum(&trend, clk.period);
    let lines = oos_dsp::eye::pj_lines(df, &spec, 12);
    let pj = lines.iter().map(|(_, r)| r * r).sum::<f64>().sqrt();
    let sj = lines.first().map(|x| x.1).unwrap_or(f64::NAN);
    let pad = 0.15 * (hi - lo);
    let mut diagram = oos_dsp::eye::Eye::new(72, 40, 2, lo - pad, hi + pad);
    diagram.accumulate(v, dt, &clk);
    let meas = diagram.measure(clk.period);
    let (ssc_ppm, _) = oos_dsp::eye::ssc_deviation(&clk);
    let snap = EyeSnap {
        quality: if summary.samples_per_ui<10.0{"NRZ estimate: limited sampling"}else{match j.fit{
            oos_dsp::eye::JitterFit::TooFew=>"NRZ estimate: too few edges for jitter model",
            oos_dsp::eye::JitterFit::Limit=>"NRZ estimate: dual-Dirac upper limits",
            oos_dsp::eye::JitterFit::Quiet=>"NRZ estimate: jitter below time resolution",
            oos_dsp::eye::JitterFit::Tails=>"NRZ estimate: dual-Dirac tail model",
        }},
        period: clk.period,
        rate: clk.rate(),
        tj: oos_dsp::eye::tj_at(&j, ber),
        tie_pp: j.pp,
        rj: j.rj_rms,
        dj: j.dj_pp,
        pj,
        sj,
        ddj: means.ddj(),
        dcd: means.dcd(),
        srj: oos_dsp::eye::srj_pp(&trend, clk.period),
        j2: oos_dsp::eye::tj_at(&j, oos_dsp::eye::BER_J2),
        pn: oos_dsp::eye::phase_noise(&trend, clk.period, pn_lo, pn_hi),
        one: meas.one,
        zero: meas.zero,
        amp: meas.amplitude,
        height: meas.height,
        width: meas.width,
        height_ber: meas.height_at(ber),
        width_ber: oos_dsp::eye::eye_width_at(&j, clk.period, ber),
        q: meas.q_factor,
        s1: meas.sigma_one,
        s0: meas.sigma_zero,
        ssc_ppm,
        ssc_hz: oos_dsp::eye::ssc_rate(&clk),
        tnt: oos_dsp::eye::tnt_ratio(v, dt, &clk),
    };
    Some(snap)
}


impl Evaluator {
    fn cat_number(&self, c: Cat, a: WaveId, b: WaveId, csrc: WaveId, d: WaveId) -> f64 {
        let nan = f64::NAN;
        let one = |id| self.wave_vec(id).map(|w| w);
        match c {
            Cat::UnitInterval => {
                let Some((v, _, dt)) = one(a) else { return nan };
                if self.rate.borrow().is_none(){*self.rate.borrow_mut()=Some(oos_dsp::measure::analyze(v,dt));}
                let an=self.rate.borrow();let an=an.as_ref().unwrap();
                let u = an.get(Meas::EstUi);
                if u.is_finite() && u > 0.0 { u } else { an.period() }
            }
            Cat::DataRate => {
                let Some((v, _, dt)) = one(a) else { return nan };
                if self.rate.borrow().is_none(){*self.rate.borrow_mut()=Some(oos_dsp::measure::analyze(v,dt));}
                let an=self.rate.borrow();let an=an.as_ref().unwrap();
                let p = an.period();
                if p.is_finite() && p > 0.0 {
                    1.0 / p
                } else {
                    let u = an.get(Meas::EstUi);
                    if u.is_finite() && u > 0.0 { 1.0 / u } else { nan }
                }
            }
            Cat::JitterSummary | Cat::TjBer => self.eye_num(a, |e| e.tj),
            Cat::Tie => self.eye_num(a, |e| e.tie_pp),
            Cat::PhaseNoise => self.eye_num(a, |e| e.pn),
            Cat::RjDd => self.eye_num(a, |e| e.rj),
            Cat::DjDd => self.eye_num(a, |e| e.dj),
            Cat::SjFreq => self.eye_num(a, |e| e.sj),
            Cat::Pj => self.eye_num(a, |e| e.pj),
            Cat::Ddj => self.eye_num(a, |e| e.ddj),
            Cat::Dcd => self.eye_num(a, |e| e.dcd),
            Cat::Srj => self.eye_num(a, |e| e.srj),
            Cat::J2 => self.eye_num(a, |e| e.j2),
            Cat::EyeHeight => self.eye_num(a, |e| e.height),
            Cat::EyeWidth => self.eye_num(a, |e| e.width),
            Cat::EyeHigh => self.eye_num(a, |e| e.one - 3.0 * e.s1),
            Cat::EyeHeightBer => self.eye_num(a, |e| e.height_ber),
            Cat::EyeWidthBer => self.eye_num(a, |e| e.width_ber),
            Cat::EyeLow => self.eye_num(a, |e| e.zero + 3.0 * e.s0),
            Cat::QFactor => self.eye_num(a, |e| e.q),
            Cat::BitHigh => self.eye_num(a, |e| e.one),
            Cat::BitLow => self.eye_num(a, |e| e.zero),
            Cat::BitAmplitude => self.eye_num(a, |e| e.amp),
            Cat::Tnt => self.eye_num(a, |e| e.tnt),
            Cat::BitRate => self.eye_num(a, |e| e.rate),
            Cat::SscDev => self.eye_num(a, |e| e.ssc_ppm),
            Cat::SscRate => self.eye_num(a, |e| e.ssc_hz),
            Cat::DcCommon => {
                if a == b { return nan; }
                let Some((dt, v)) = self.align(&[a, b]) else { return nan };
                let _ = dt;
                oos_dsp::eye::dc_common(&v[0], &v[1])
            }
            Cat::AcCommon => {
                let Some((v, _, dt)) = one(a) else { return nan };
                if a == b {
                    oos_dsp::eye::ac_common_pp(&v, None, dt, self.model.eye_hp)
                } else if let Some((aligned_dt, w)) = self.align(&[a, b]) {
                    oos_dsp::eye::ac_common_pp(&w[0], Some(&w[1]), aligned_dt, self.model.eye_hp)
                } else {
                    nan
                }
            }
            Cat::DiffCross => {
                if a == b { return nan; }
                let Some((_, v)) = self.align(&[a, b]) else { return nan };
                oos_dsp::eye::diff_crossover(&v[0], &v[1])
            }
            Cat::PowerQuality => self.power_quality(a,b).map_or(nan,|q|q.p_real),
            Cat::Harmonics => one(b).map(|(i,_,dt)| {
                let h = oos_dsp::power::harmonics(i,dt,(self.model.pw_fline>0.0).then_some(self.model.pw_fline),40);
                let fund = h.iter().find(|x| x.0 == 1).map(|x| x.1).unwrap_or(0.0);
                let rest = h.iter().filter(|x| x.0 > 1).map(|x| x.1 * x.1).sum::<f64>().sqrt();
                if fund > 0.0 { 100.0 * rest / fund } else { nan }
            }).unwrap_or(nan),
            Cat::Inrush => one(b).map(|(i,_,dt)|oos_dsp::power::inrush(i,dt).0).unwrap_or(nan),
            Cat::InputC => self.vi(a, b, |v, i, dt| oos_dsp::power::input_capacitance(v, i, dt)),
            Cat::TurnOn => self.vi(a, b, |vout, vin, dt| oos_dsp::power::turn_on_time(vin, vout, dt)),
            Cat::TurnOff => self.vi(a, b, |vout, vin, dt| oos_dsp::power::turn_off_time(vin, vout, dt)),
            Cat::EOn => self.sw(a, b, |s| s.e_on),
            Cat::EOff => self.sw(a, b, |s| s.e_off),
            Cat::ECond => self.sw(a, b, |s| s.e_cond),
            Cat::DvdtRise => self.sw(a, b, |s| s.dvdt_rise),
            Cat::DvdtFall => self.sw(a, b, |s| s.dvdt_fall),
            Cat::DidtRise => self.sw(a, b, |s| s.didt_rise),
            Cat::DidtFall => self.sw(a, b, |s| s.didt_fall),
            Cat::RdsOn => self.sw(a, b, |s| s.rds_on),
            Cat::Inductance => self.mag(a, b).and_then(|m| m.l).unwrap_or(nan),
            Cat::MagProp => self.mag(a, b).and_then(|m| m.mu_r).unwrap_or(nan),
            Cat::MagLoss => self.mag(a, b).and_then(|m| m.loss).unwrap_or(nan),
            Cat::Ripple => one(a).map(|(v, _, _)| oos_dsp::power::ripple(&v).0).unwrap_or(nan),
            Cat::LineRipple => one(a).map(|(v, _, dt)| {
                let f = if self.model.pw_fline > 0.0 { self.model.pw_fline } else { oos_dsp::power::line_hz(&v, dt) };
                oos_dsp::power::tone_ripple(&v, dt, f).pp
            }).unwrap_or(nan),
            Cat::SwRipple => one(a).map(|(v, _, dt)| {
                let f = if self.model.pw_fsw > 0.0 { self.model.pw_fsw } else { oos_dsp::power::switch_hz(&v, dt) };
                oos_dsp::power::switching_ripple(&v, dt, f).pp
            }).unwrap_or(nan),
            Cat::Psrr => self.ratio(a, b, oos_dsp::bode::Mode::Psrr),
            Cat::Impedance => self.ratio(a, b, oos_dsp::bode::Mode::Z),
            Cat::PowerFactor => self.power_quality(a,b).map_or(nan,|q|q.pf),
            Cat::Dq0 => self.phases(a, b, csrc).and_then(|(dt, v)| oos_dsp::motor::dq0(&v[0], &v[1], &v[2], dt, oos_dsp::motor::Theta::Vector).map(|q| q.d)).unwrap_or(nan),
            Cat::Speed => self.shaft(a, b, csrc).and_then(|s| s.rpm).unwrap_or(nan),
            Cat::Accel => self.shaft(a, b, csrc).and_then(|s| s.accel).unwrap_or(nan),
            Cat::Direction => self.shaft(a, b, csrc).map(|s| match s.direction {
                oos_dsp::motor::Direction::Forward => 1.0,
                oos_dsp::motor::Direction::Stopped => 0.0,
                oos_dsp::motor::Direction::Reverse => -1.0,
            }).unwrap_or(nan),
            Cat::Angle => self.shaft(a, b, csrc).and_then(|s| s.angle_deg).unwrap_or(nan),
            Cat::Torque | Cat::MechPower | Cat::SysEff => self.mech(a, b, csrc, d).map(|m| match c {
                Cat::Torque => m.torque.unwrap_or(nan),
                Cat::MechPower => m.p_mech.unwrap_or(nan),
                _ => m.eff.unwrap_or(nan),
            }).unwrap_or(nan),
            Cat::RailOver | Cat::RailUnder | Cat::RailTurn | Cat::DcRail => {
                let Some((v, _, dt)) = one(a) else { return nan };
                let vin = if a != b { self.wave_vec(b).map(|w| w.0) } else { None };
                if self.rail.borrow().is_none(){*self.rail.borrow_mut()=Some(oos_dsp::power::rail(v,dt,self.model.pw_ref.filter(|v|v.is_finite()&&*v!=0.0),vin.as_deref(),None));}
                let rail=self.rail.borrow();let rail=rail.as_ref().unwrap();
                match c {
                    Cat::RailOver => rail.overshoot,
                    Cat::RailUnder => rail.undershoot,
                    Cat::RailTurn => rail.turn_on,
                    _ => rail.dc,
                }
            }
            Cat::Vpeak | Cat::Ipeak | Cat::TdOn | Cat::TdOff | Cat::Tr | Cat::Tf | Cat::Ton | Cat::Toff | Cat::DpdtV | Cat::DpdtI | Cat::Tdt | Cat::Trr | Cat::Qrr | Cat::Err | Cat::Irrm | Cat::DiodeDdt | Cat::Qoss => {
                self.pulse(a, b, csrc).map(|p| match c {
                    Cat::Vpeak => p.vpeak.unwrap_or(nan),
                    Cat::Ipeak => p.ipeak.unwrap_or(nan),
                    Cat::TdOn => p.td_on.unwrap_or(nan),
                    Cat::TdOff => p.td_off.unwrap_or(nan),
                    Cat::Tr => p.tr.unwrap_or(nan),
                    Cat::Tf => p.tf.unwrap_or(nan),
                    Cat::Ton => p.ton.unwrap_or(nan),
                    Cat::Toff => p.toff.unwrap_or(nan),
                    Cat::DpdtV => p.dvdt.unwrap_or(nan),
                    Cat::DpdtI => p.didt.unwrap_or(nan),
                    Cat::Tdt => p.tdt.unwrap_or(nan),
                    Cat::Trr => p.trr.unwrap_or(nan),
                    Cat::Qrr => p.qrr.unwrap_or(nan),
                    Cat::Err => p.err.unwrap_or(nan),
                    Cat::Irrm => p.irrm.unwrap_or(nan),
                    Cat::DiodeDdt => p.diode_didt.unwrap_or(nan),
                    _ => p.qoss.unwrap_or(nan),
                }).unwrap_or(nan)
            }
            _ => nan,
        }
    }

    fn eye_num(&self, id: WaveId, f: impl FnOnce(&EyeSnap) -> f64) -> f64 {
        let Some((v, _, dt)) = self.wave_vec(id) else { return f64::NAN };
        if self.eye.borrow().is_none() { *self.eye.borrow_mut() = Some(eye_of(&v, dt, self.model.eye_ber, self.model.eye_pn_lo, self.model.eye_pn_hi,self.model.eye_rate)); }
        self.eye.borrow().as_ref().and_then(|e| e.as_ref()).map(f).unwrap_or(f64::NAN)
    }

    fn power_quality(&self,a:WaveId,b:WaveId)->Option<oos_dsp::power::Quality>{
        if self.quality.borrow().is_none(){let(dt,v)=self.align(&[a,b])?;*self.quality.borrow_mut()=Some(oos_dsp::power::quality(v[0],v[1],dt,(self.model.pw_fline>0.0).then_some(self.model.pw_fline)));}
        *self.quality.borrow()
    }
    fn vi(&self, a: WaveId, b: WaveId, f: impl FnOnce(&[f32], &[f32], f64) -> f64) -> f64 {
        self.align(&[a, b]).map(|(dt, v)| f(&v[0], &v[1], dt)).unwrap_or(f64::NAN)
    }

    fn sw(&self, a: WaveId, b: WaveId, f: impl FnOnce(&oos_dsp::power::Switching) -> f64) -> f64 {
        if self.switching.borrow().is_none() {
            let Some((dt,v)) = self.align(&[a,b]) else { return f64::NAN };
            *self.switching.borrow_mut() = Some(oos_dsp::power::switching(v[0],v[1],dt,10.0));
        }
        self.switching.borrow().as_ref().map(f).unwrap_or(f64::NAN)
    }

    fn mag(&self, a: WaveId, b: WaveId) -> Option<Arc<oos_dsp::magnetic::Magnetic>> {
        if let Some(value) = self.magnetic.borrow().as_ref() { return Some(Arc::clone(value)); }
        let (dt, v) = self.align(&[a, b])?;
        let geom = oos_dsp::magnetic::Geom {
            turns: self.model.mag_n.max(1.0),
            area: (self.model.mag_ae > 0.0).then_some(self.model.mag_ae),
            length: (self.model.mag_le > 0.0).then_some(self.model.mag_le),
            rcu: self.model.mag_rcu.max(0.0),
        };
        let value=Arc::new(oos_dsp::magnetic::measure(v[0],v[1],dt,geom));
        *self.magnetic.borrow_mut()=Some(Arc::clone(&value)); Some(value)
    }

    fn ratio(&self, a: WaveId, b: WaveId, mode: oos_dsp::bode::Mode) -> f64 {
        let Some((dt, v)) = self.align(&[a, b]) else { return f64::NAN };
        let f=if self.model.pw_fline>0.0{self.model.pw_fline}else{oos_dsp::power::line_hz(v[0],dt)};
        oos_dsp::bode::response(mode, &v[0], &v[1], dt, f).map(|p| p.0).unwrap_or(f64::NAN)
    }

    fn phases(&self, a: WaveId, b: WaveId, c: WaveId) -> Option<(f64, Vec<&[f32]>)> {
        if a == b || a == c || b == c {
            return None;
        }
        self.align(&[a, b, c])
    }

    fn shaft(&self, a: WaveId, b: WaveId, c: WaveId) -> Option<Arc<oos_dsp::motor::Shaft>> {
        if let Some(value)=self.shaft.borrow().as_ref(){return Some(Arc::clone(value));}
        let (dt,v)=self.phases(a,b,c)?;
        let value=Arc::new(oos_dsp::motor::shaft_phase(v[0],v[1],v[2],dt,self.t0,self.model.inv_poles.max(1)));
        *self.shaft.borrow_mut()=Some(Arc::clone(&value));Some(value)
    }

    fn mech(&self, a: WaveId, b: WaveId, c: WaveId, d: WaveId) -> Option<Arc<oos_dsp::motor::Mech>> {
        if let Some(value)=self.mechanical.borrow().as_ref(){return Some(Arc::clone(value));}
        let shaft = self.shaft(a, b, c)?;
        let torque = self.wave_vec(d).map(|w| w.0);
        let ev = self.model.inv_ev.and_then(|id| self.wave_vec(id)).map(|w| w.0);
        let ei = self.model.inv_ei.and_then(|id| self.wave_vec(id)).map(|w| w.0);
        let value=Arc::new(oos_dsp::motor::mechanical(shaft.as_ref().clone(),torque.as_deref(),self.model.inv_scale,ev.as_deref(),ei.as_deref()));
        *self.mechanical.borrow_mut()=Some(Arc::clone(&value));Some(value)
    }

    fn pulse(&self, a: WaveId, b: WaveId, g: WaveId) -> Option<Arc<oos_dsp::dpt::Pulse>> {
        if let Some(value)=self.pulse.borrow().as_ref(){return Some(Arc::clone(value));}
        let (dt,v)=self.align(&[a,b,g])?;
        let fourth=self.wave_vec(self.model.dpt_4ch).map(|w|w.0);
        let fourth=match (self.model.dpt_4th,fourth) {
            (DptFourth::Gate,Some(v))=>oos_dsp::dpt::Fourth::Gate(v),
            (DptFourth::DiodeV,Some(v))=>oos_dsp::dpt::Fourth::DiodeV(v),
            (DptFourth::DiodeI,Some(v))=>oos_dsp::dpt::Fourth::DiodeI(v),
            _=>oos_dsp::dpt::Fourth::Off,
        };
        let value=Arc::new(oos_dsp::dpt::measure(v[0],v[1],Some(v[2]),fourth,dt,self.model.dpt_pct));
        *self.pulse.borrow_mut()=Some(Arc::clone(&value));Some(value)
    }
}
#[cfg(test)]
mod tests{
    use super::*;use std::time::{Duration,Instant};
    fn input(id:usize,t0:f64,dt:f64,v:Vec<f32>)->Input{let hi=v.len();Input{id:WaveId::Ref(id),samples:Samples::Reference(Arc::new(v)),lo:0,hi,t0,dt}}
    fn fixture()->Ui{let mut ui=Ui::new(Source::Demo{t:0.0});ui.m.run=RunState::Stop;ui.m.ch.iter_mut().for_each(|c|c.on=false);ui.m.meas.clear();ui.m.meas2.clear();ui.m.cat.clear();ui.m.meas_full_record=true;ui}
    fn put(ui:&mut Ui,r:usize,v:Vec<f32>,t0:f64,dt:f64){ui.refs[r]=Some(RefWave{src:0,color:r,label:String::new(),t0,dt,v:v.into(),vpd:1.0,offset:0.0,on:true});}
    fn row(c:Cat)->Row{(c as u16,WaveId::Ref(0),WaveId::Ref(1),WaveId::Ref(2),WaveId::Ref(3))}
    fn wait(ui:&mut Ui,c:Cat)->Reading{let end=Instant::now()+Duration::from_secs(20);loop{ui.poll_catalog_measurements();let r=ui.catalog_reading(c,[WaveId::Ref(0),WaveId::Ref(1),WaveId::Ref(2),WaveId::Ref(3)]);if !r.computing{return r;}assert!(Instant::now()<end,"catalog never completed");std::thread::sleep(Duration::from_millis(1));}}
    #[test]
    fn alignment_uses_common_intersection_slowest_grid_and_subsample_origins(){
        let a=input(0,-2.0,1.0,vec![0.,1.,2.,3.,4.,5.]);let b=input(1,-1.5,0.5,vec![0.,1.,2.,3.,4.,5.,6.,7.]);
        let v=align_inputs(&[a,b]).unwrap();assert_eq!((v.t0,v.dt),(-1.0,1.0));
        assert_eq!(v.inputs[0].1,[1.,2.,3.,4.]);assert_eq!(v.inputs[1].1,[1.,3.,5.,7.]);
        assert!(align_inputs(&[input(0,0.,1.,vec![0.,1.]),input(1,10.,1.,vec![0.,1.])]).unwrap_err().contains("overlap"));
    }
    #[test]
    fn gate_and_fourth_probe_join_all_dpt_roles_before_alignment(){
        let mut ui=fixture();for r in 0..4{put(&mut ui,r,vec![1.;if r==2{4}else{20}],if r==2{5.0}else{0.0},1.0);}
        ui.m.dpt_4th=DptFourth::Gate;ui.m.dpt_4ch=WaveId::Ref(3);
        let(_,inputs)=ui.catalog_snapshot(Cat::Vpeak,[WaveId::Ref(0),WaveId::Ref(1),WaveId::Ref(2),WaveId::Ref(3)]).unwrap();
        assert_eq!(inputs.len(),4);let aligned=align_inputs(&inputs).unwrap();assert_eq!((aligned.t0,aligned.dt,aligned.inputs[0].1.len()),(5.0,1.0,4));
        ui.refs[3].as_mut().unwrap().t0=20.0;let(_,inputs)=ui.catalog_snapshot(Cat::Vpeak,[WaveId::Ref(0),WaveId::Ref(1),WaveId::Ref(2),WaveId::Ref(3)]).unwrap();assert!(align_inputs(&inputs).is_err());
    }
    #[test]
    fn families_queue_once_share_immutable_data_and_never_count_stopped_reads(){
        let mut ui=fixture();put(&mut ui,0,(0..400_000).map(|i|if i%40<20{-0.5}else{0.5}).collect(),0.0,1e-9);
        ui.m.cat=vec![row(Cat::EyeHeight),row(Cat::Tie)];
        let started=Instant::now();for _ in 0..100{let _=ui.cat_row(row(Cat::EyeHeight).0,row(Cat::EyeHeight).1,row(Cat::EyeHeight).2,row(Cat::EyeHeight).3,row(Cat::EyeHeight).4);let _=ui.cat_row(row(Cat::Tie).0,row(Cat::Tie).1,row(Cat::Tie).2,row(Cat::Tie).3,row(Cat::Tie).4);}
        assert!(started.elapsed()<Duration::from_millis(500));assert_eq!(ui.azr.catalog_measurements.borrow().worker.as_ref().unwrap().submitted,1);
        assert!(Arc::strong_count(&ui.refs[0].as_ref().unwrap().v)>=2,"worker must hold the same immutable source");
        let height=wait(&mut ui,Cat::EyeHeight);assert!(height.value.is_finite());assert!(height.quality.contains("NRZ"));assert!(wait(&mut ui,Cat::Tie).value.is_finite());
        assert_eq!(ui.azr.catalog_measurements.borrow().worker.as_ref().unwrap().submitted,1);
        assert_eq!((ui.wfm_total,ui.replay.captures,ui.m.run),(0,0,RunState::Stop));assert!(ui.measurement_statistics_snapshot_for_test().iter().all(|(n,_)|*n==0));
        ui.m.delay=100.;assert!(!ui.catalog_reading(Cat::Tie,[WaveId::Ref(0),WaveId::Ref(1),WaveId::Ref(2),WaveId::Ref(3)]).computing);
        assert_eq!(ui.azr.catalog_measurements.borrow().worker.as_ref().unwrap().submitted,1);
    }
    #[test]
    fn replacing_unsampled_interior_data_discards_old_answer_and_has_unique_owner_version(){
        let mut ui=fixture();put(&mut ui,0,vec![0.;400_000],0.0,1e-9);ui.m.cat=vec![row(Cat::Ripple)];
        assert!(ui.catalog_reading(Cat::Ripple,[WaveId::Ref(0),WaveId::Ref(1),WaveId::Ref(2),WaveId::Ref(3)]).computing);
        let old=ui.catalog_snapshot(Cat::Ripple,[WaveId::Ref(0),WaveId::Ref(1),WaveId::Ref(2),WaveId::Ref(3)]).unwrap().0;
        Arc::make_mut(&mut ui.refs[0].as_mut().unwrap().v)[123_456]=7.;
        let new=ui.catalog_snapshot(Cat::Ripple,[WaveId::Ref(0),WaveId::Ref(1),WaveId::Ref(2),WaveId::Ref(3)]).unwrap().0;assert_ne!(old,new);
        let reading=wait(&mut ui,Cat::Ripple);assert_eq!(reading.value,7.);assert!(ui.azr.catalog_measurements.borrow().cache.iter().all(|entry|entry.key!=old));
        // No full source owner is held by the compact result cache. A retained Weak
        // still forces make_mut to disassociate and receive a new monotonic ID.
        Arc::make_mut(&mut ui.refs[0].as_mut().unwrap().v)[123_457]=9.;
        let newest=ui.catalog_snapshot(Cat::Ripple,[WaveId::Ref(0),WaveId::Ref(1),WaveId::Ref(2),WaveId::Ref(3)]).unwrap().0;
        assert_ne!(newest,new);assert_eq!(wait(&mut ui,Cat::Ripple).value,9.);
        assert_eq!(ui.wfm_total,0);
    }
    #[test]
    fn screen_pan_changes_physical_key_full_pan_and_unused_roles_do_not(){
        let mut ui=fixture();put(&mut ui,0,vec![0.;10_000],-5e-6,1e-9);
        let ids=[WaveId::Ref(0),WaveId::Ref(1),WaveId::Ref(2),WaveId::Ref(3)];let full=ui.catalog_snapshot(Cat::Ripple,ids).unwrap().0;
        ui.m.delay=1e-6;assert_eq!(ui.catalog_snapshot(Cat::Ripple,ids).unwrap().0,full);
        ui.m.meas_full_record=false;ui.m.tscale=tscales().iter().position(|&v|(v-100e-9).abs()<1e-14).unwrap();ui.m.delay=0.;
        let screen=ui.catalog_snapshot(Cat::Ripple,ids).unwrap().0;ui.m.delay=1e-6;assert_ne!(ui.catalog_snapshot(Cat::Ripple,ids).unwrap().0,screen);
        assert_eq!(ui.catalog_snapshot(Cat::Ripple,[ids[0],WaveId::Ch(3),ids[2],ids[3]]).unwrap().0,ui.catalog_snapshot(Cat::Ripple,ids).unwrap().0);
    }
    #[test]
    fn missing_data_and_alignment_budget_are_explicit_not_zero(){
        let mut ui=fixture();put(&mut ui,0,vec![0.;4000],0.,1e-9);ui.m.cat=vec![row(Cat::EyeHeight)];let r=wait(&mut ui,Cat::EyeHeight);assert!(r.value.is_nan());
        let missing=ui.catalog_reading(Cat::Vpeak,[WaveId::Ref(0),WaveId::Ref(1),WaveId::Ref(2),WaveId::Ref(3)]);assert!(!missing.computing&&missing.value.is_nan()&&missing.reason.is_some());
        // Metadata-only impossible span verifies the guard runs before indexing/allocation.
        let a=Input{id:WaveId::Ref(0),samples:Samples::Reference(Arc::new(vec![0.,1.])),lo:0,hi:MAX_POINTS+1,t0:0.,dt:1.};
        assert!(align_inputs(&[a]).unwrap_err().contains("bound"));
    }
    #[test]
    fn every_wave_id_completes_through_backend_with_missing_domain_data_not_placeholder_values(){
        let mut ui=fixture();for r in 0..4{put(&mut ui,r,vec![1.;4000],0.,1e-9);}
        let catalog=listed().into_iter().filter(|&c|matches!(meta(c).kind,CatKind::Wave)).collect::<Vec<_>>();assert_eq!(catalog.len(),83);
        ui.m.cat=catalog.iter().map(|&c|row(c)).collect();
        for &cat in &catalog{let r=wait(&mut ui,cat);assert!(!r.computing);}
        assert_eq!(wait(&mut ui,Cat::Ripple).value,0.);assert_eq!(wait(&mut ui,Cat::Inrush).value,1.);assert_eq!(wait(&mut ui,Cat::DcCommon).value,1.);
        assert!(wait(&mut ui,Cat::EyeHeight).value.is_nan());assert!(wait(&mut ui,Cat::TdOn).value.is_nan());
        assert!(ui.azr.catalog_measurements.borrow().cache.len()<catalog.len(),"family results must be shared");
    }
    #[test]
    fn only_explicit_accepted_capture_queue_counts_a_completed_result_once(){
        let mut ui=fixture();put(&mut ui,0,(0..400_000).map(|i|if i%20<10{0.0}else{2.0}).collect(),0.,1e-9);ui.m.cat=vec![row(Cat::Ripple)];ui.wfm_total=1;
        let ids=[WaveId::Ref(0),WaveId::Ref(1),WaveId::Ref(2),WaveId::Ref(3)];assert!(ui.catalog_reading(Cat::Ripple,ids).computing);
        ui.queue_catalog_statistics(row(Cat::Ripple),1);assert_eq!(wait(&mut ui,Cat::Ripple).value,2.);
        assert_eq!(ui.measurement_statistics_snapshot_for_test()[0].0,1);
        for _ in 0..20{ui.poll_catalog_measurements();let _=ui.catalog_reading(Cat::Ripple,ids);}
        assert_eq!(ui.measurement_statistics_snapshot_for_test()[0].0,1);
        ui.m.meas_full_record=false;ui.m.tscale=tscales().iter().position(|&v|(v-100e-9).abs()<1e-14).unwrap();ui.m.delay=1e-6;
        assert_eq!(wait(&mut ui,Cat::Ripple).value,2.);assert_eq!(ui.measurement_statistics_snapshot_for_test()[0].0,1,"stopped pan is not a capture");
    }

    #[test]
    fn mixed_origin_interval_readouts_match_independent_common_mode_math(){
        let mut ui=fixture();put(&mut ui,0,vec![0.,1.,2.,3.,4.,5.,6.,7.],-1.5,0.5);put(&mut ui,1,vec![0.,1.,2.,3.,4.,5.],-2.0,1.0);
        ui.m.cat=vec![row(Cat::DcCommon),row(Cat::AcCommon)];ui.m.eye_hp=false;
        assert_eq!(wait(&mut ui,Cat::DcCommon).value,3.25);
        assert_eq!(wait(&mut ui,Cat::AcCommon).value,4.5);
        assert_eq!(ui.azr.catalog_measurements.borrow().worker.as_ref().unwrap().submitted,1);
    }

    #[test]
    fn clear_cancels_old_accounting_even_when_capture_number_is_reused(){
        let mut ui=fixture();put(&mut ui,0,(0..400_000).map(|i|if i%20<10{0.0}else{2.0}).collect(),0.,1e-9);ui.m.cat=vec![row(Cat::Ripple)];ui.wfm_total=1;
        let ids=[WaveId::Ref(0),WaveId::Ref(1),WaveId::Ref(2),WaveId::Ref(3)];let _=ui.catalog_reading(Cat::Ripple,ids);ui.queue_catalog_statistics(row(Cat::Ripple),1);
        // Hold a finished answer before UI delivery, then clear and reuse sequence1.
        let answer=ui.azr.catalog_measurements.borrow().worker.as_ref().unwrap().receive.recv_timeout(Duration::from_secs(20)).unwrap();
        ui.clear_display();ui.wfm_total=1;ui.queue_catalog_statistics(row(Cat::Ripple),1);
        let(send,receive)=mpsc::sync_channel(1);ui.azr.catalog_measurements.borrow_mut().worker.as_mut().unwrap().receive=receive;assert!(send.send(answer).is_ok());
        ui.poll_catalog_measurements();assert_eq!(ui.catalog_reading(Cat::Ripple,ids).value,2.0,"a valid value may still cache");
        assert!(ui.measurement_statistics_snapshot_for_test().is_empty(),"pre-Clear capture must never repopulate statistics");
    }

    #[test]
    fn processed_channel_reused_buffer_and_capture_after_clear_cannot_reuse_old_samples(){
        let mut ui=fixture();ui.m.ch[0].on=true;ui.wfm_total=1;
        ui.last[0]=Some(Record{t0:0.0,dt:1e-9,ch:[vec![0.0,2.0,0.0,2.0],Vec::new(),Vec::new(),Vec::new()],triggered:true,partial:false,raw:None,la:None});
        let ids=[WaveId::Ch(0),WaveId::Ref(1),WaveId::Ref(2),WaveId::Ref(3)];ui.m.cat=vec![(Cat::Ripple as u16,ids[0],ids[1],ids[2],ids[3])];
        let read=|ui:&mut Ui|{let deadline=Instant::now()+Duration::from_secs(20);loop{ui.poll_catalog_measurements();let r=ui.catalog_reading(Cat::Ripple,ids);if !r.computing{return r.value;}assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(1));}};
        assert_eq!(read(&mut ui),2.0);let pointer=ui.last[0].as_ref().unwrap().ch[0].as_ptr();
        // Clear's reset generation changes while the accepted sequence is reused.
        // Mutate the same allocation to make allocator reuse deterministic.
        ui.reset_catalog_statistics();ui.wfm_total=1;ui.last[0].as_mut().unwrap().ch[0][1]=9.0;
        assert_eq!(ui.last[0].as_ref().unwrap().ch[0].as_ptr(),pointer);
        assert_eq!(read(&mut ui),9.0,"pre-clear processed snapshot must not survive reused pointer/capture");
    }

}
