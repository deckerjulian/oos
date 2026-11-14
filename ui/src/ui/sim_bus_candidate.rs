//! Fresh SIM bus records are decoded before they become an accepted reference.
use super::*;
use super::super::reference_decode::{BorrowedWave,decode_borrowed,sources};
use oos_dsp::decode::Packet;

pub(super) struct Decoded{pub primary_slot:usize,pub packets:Arc<Vec<Packet>>,pub hits:Arc<Vec<f64>>,pub channels:[bool;NCH]}
enum Data{Generated,Reference(Arc<Vec<f32>>),Native(Arc<crate::board::Frame>,usize),Small(Arc<Vec<f32>>)}
struct Role{source:usize,data:Data,t0:f64,dt:f64}
pub(super) struct Plan{model:Model,roles:Vec<Role>,primary_slot:usize,hysteresis:f32}
impl Plan{
    pub(super) fn evaluate(&self,generated:&[f32],dt:f64)->Result<Decoded,&'static str>{
        let waves=self.roles.iter().map(|role|{let(v,step)=match &role.data{
            Data::Generated=>(generated,dt),Data::Reference(v)|Data::Small(v)=>(v.as_slice(),role.dt),Data::Native(frame,c)=>(frame.ch[*c].as_slice(),role.dt)};
            BorrowedWave{source:role.source,v,t0:role.t0,dt:step}
        }).collect::<Vec<_>>();
        let packets=decode_borrowed(&self.model,&waves,self.hysteresis)?;
        let mut hits=packets.iter().filter(|packet|crate::trig::matches(&self.model,packet)).map(|packet|packet.t1).collect::<Vec<_>>();
        hits.sort_by(f64::total_cmp);Ok(Decoded{primary_slot:self.primary_slot,packets,hits:Arc::new(hits),channels:std::array::from_fn(|c|self.roles.iter().any(|role|role.source==c))})
    }
}
impl Ui{
    pub(in crate::ui::sim) fn prepare_sim_bus(&self,model:&Model,slot:usize,generated_vpd:f32)->Result<Plan,&'static str>{
        if model.dec_la_in||model.bus_on_la(){return Err("SIM Bus qualification on logic-analyzer inputs is unavailable");}
        let mut roles=Vec::new();let mut primary_vpd=generated_vpd;
        for source in sources(model)?{
            let id=decode_wave(source).ok_or("Invalid SIM Bus role")?;
            let(data,t0,dt,vpd)=match id{
                WaveId::Ref(i)if i==slot=>(Data::Generated,self.source_time_origin(id,0.0),1.0,generated_vpd),
                WaveId::Ref(i)=>{let r=self.refs.get(i).and_then(Option::as_ref).ok_or("SIM Bus auxiliary reference is empty")?;
                    (Data::Reference(Arc::clone(&r.v)),self.source_time_origin(id,r.t0),r.dt,r.vpd)},
                WaveId::Ch(c)=>{if c>=n_ch()||!self.m.ch[c].on{return Err("SIM Bus auxiliary channel is off");}
                    let record=self.last[0].as_ref().ok_or("SIM Bus auxiliary channel has no accepted record")?;
                    if let Some(raw)=record.raw.as_ref().filter(|raw|!raw.frame.envelope&&raw.frame.ch[c].len()>=2){
                        (Data::Native(Arc::clone(&raw.frame),c),self.source_time_origin(id,raw.t0-raw.i0 as f64*raw.frame.dt),raw.frame.dt,self.m.ch[c].volts_per_div())
                    }else{if record.ch[c].len()>262_144{return Err("SIM Bus needs an immutable native auxiliary channel record");}
                        (Data::Small(Arc::new(record.ch[c].clone())),self.source_time_origin(id,record.t0),record.dt,self.m.ch[c].volts_per_div())}
                }
            };
            if source==model.dec_src{primary_vpd=vpd;}
            roles.push(Role{source,data,t0,dt});
        }
        Ok(Plan{model:model.clone(),roles,primary_slot:model.trig_ref.unwrap_or(slot),hysteresis:0.1*primary_vpd.max(1e-9)})
    }
    /// Complete role identity, excluding the installed old SIM waveform that a
    /// generated candidate will replace. Owners are retained by the request.
    pub(in crate::ui::sim) fn sim_bus_guard(&self,model:&Model,slot:usize)->String{
        if model.trig_type!=TrigType::Bus{return String::new();}
        let mut key=format!("|BUS:{:?}:{:?}:{}:{}|",model.dec_cfg(),model.trig_bus,model.trig_bus_val,self.workflow_epoch);
        match sources(model){Err(error)=>key.push_str(error),Ok(ids)=>for source in ids{
            let stamp=match decode_wave(source){
                Some(WaveId::Ref(i))if i==slot=>format!("generated:{}",self.source_time_origin(WaveId::Ref(i),0.0).to_bits()),
                Some(id@WaveId::Ref(i))=>self.refs.get(i).and_then(Option::as_ref).map_or("missing".into(),|r|format!("{:p}:{}:{}:{}:{}",Arc::as_ptr(&r.v),r.v.len(),r.dt.to_bits(),self.source_time_origin(id,r.t0).to_bits(),r.vpd.to_bits())),
                Some(id@WaveId::Ch(c))=>self.last[0].as_ref().map_or("missing".into(),|record|{
                    if let Some(raw)=record.raw.as_ref().filter(|raw|!raw.frame.envelope&&raw.frame.ch[c].len()>=2){format!("{:p}:{}:{}:{}:{}:{}:{}:{:?}",Arc::as_ptr(&raw.frame),raw.frame.seq,raw.frame.ch[c].len(),raw.frame.dt.to_bits(),self.source_time_origin(id,raw.t0-raw.i0 as f64*raw.frame.dt).to_bits(),self.m.ch[c].on,self.m.ch[c].volts_per_div().to_bits(),(self.m.ch[c].coupling,self.m.ch[c].invert,self.m.ch[c].bw))}
                    else{format!("{:p}:{}:{}:{}:{}:{}:{}:{:?}",record.ch[c].as_ptr(),self.wfm_total,record.ch[c].len(),record.dt.to_bits(),self.source_time_origin(id,record.t0).to_bits(),self.m.ch[c].on,self.m.ch[c].volts_per_div().to_bits(),(self.m.ch[c].coupling,self.m.ch[c].invert,self.m.ch[c].bw))}
                }),None=>"invalid".into()};
            key.push_str(&format!("{source}:{stamp}|"));
        }}key
    }
}
