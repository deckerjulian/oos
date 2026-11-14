// SPDX-License-Identifier: GPL-2.0-or-later
//! Actual default PCIe4 SIM data; independent reference-crossing and bit-lattice oracles.
use oos_dsp::{measure::{analyze_refs,Meas},serial};
fn at(v:&[f32],level:f64,rise:bool,dt:f64)->Vec<f64>{
 v.windows(2).enumerate().filter_map(|(i,p)|{
  let(a,b)=(p[0]as f64,p[1]as f64);
  if (rise&&a<level&&b>=level)||(!rise&&a>level&&b<=level){Some((i as f64+(level-a)/(b-a))*dt)}else{None}
 }).collect()
}
fn oracle(v:&[f32],dt:f64,lo:f64,hi:f64)->(Vec<f64>,Vec<f64>){
 let mid=(lo+hi)*0.5;let band=(hi-lo)*0.125;
 let mut mids=Vec::new();let mut state=v.first().is_some_and(|x|*x as f64>mid);
 let mut last=0.0;
 for(i,p)in v.windows(2).enumerate(){let(a,b)=(p[0]as f64,p[1]as f64);
  if (a<mid)!=(b<mid){last=(i as f64+(mid-a)/(b-a))*dt;}
  if !state&&b>mid+band*0.5 {mids.push((last,true));state=true;}
  else if state&&b<mid-band*0.5{mids.push((last,false));state=false;}
 }
 let edges=[at(v,lo,true,dt),at(v,hi,true,dt),at(v,hi,false,dt),at(v,lo,false,dt)];
 let(mut r,mut f)=(Vec::new(),Vec::new());
 for(k,&(t,up))in mids.iter().enumerate(){let before=if k>0{mids[k-1].0}else{0.0};let after=mids.get(k+1).map_or(v.len()as f64*dt,|e|e.0);
  let(a,b)=if up{(&edges[0],&edges[1])}else{(&edges[2],&edges[3])};
  let i=a.partition_point(|x|*x<=t);let j=b.partition_point(|x|*x<t);
  if i>0&&j<b.len()&&a[i-1]>=before&&b[j]<after{if up{r.push(b[j]-a[i-1]);}else{f.push(b[j]-a[i-1]);}}
 }
 (r,f)
}

#[test]
fn pcie4_default_ssc_record_ui_is_not_its_prbs_cycle_period_and_edges_use_selected_levels(){
 let lane=serial::LANES[3];let dt=serial::dt_of(&lane);let samples=serial::try_generate(&lane).unwrap();
 assert_eq!(samples.len(),3_878_788);assert_eq!(dt,1.0/128e9);
 let lo=(28.35e-6/dt)as usize;let hi=(28.85e-6/dt)as usize;let v=&samples[lo..hi];
 let mut means=Vec::new();
 for(lp,hp)in[(0.1,0.9),(0.2,0.8)]{
  let a=analyze_refs(v,dt,0.05,lp,hp);let low=a.base+lp*(a.top-a.base);let high=a.base+hp*(a.top-a.base);
  let(r,f)=oracle(v,dt,low,high);let mean=|values:&[f64]|values.iter().sum::<f64>()/values.len()as f64;
  assert_eq!((a.rise_count,a.fall_count),(r.len(),f.len()));assert!(a.rise_count>1900);
  assert!((a.rise_time-mean(&r)).abs()<1e-16);assert!((a.fall_time-mean(&f)).abs()<1e-16);
  assert!(a.est_ui>62e-12&&a.est_ui<63e-12);assert!(a.pll_ui>62e-12&&a.pll_ui<63e-12);
  assert!(a.period()>3.8*a.est_ui&&a.period()<4.2*a.est_ui);
  assert!(a.get(Meas::Freq)>3.8e9&&a.get(Meas::Freq)<4.2e9);
  println!("PCIe4 screen {lp}/{hp}: UI={}ps PLL={}ps Period={}ps Freq={}GHz rise/fall={}/{}ps qualified={}/{}",
   a.est_ui*1e12,a.pll_ui*1e12,a.period()*1e12,a.get(Meas::Freq)/1e9,a.rise_time*1e12,a.fall_time*1e12,a.rise_count,a.fall_count);
  means.push((a.rise_time,a.fall_time));
 }
 assert!(means[0].0>90e-12&&means[0].1>90e-12);assert!(means[1].0<45e-12&&means[1].1<45e-12);
 let full=analyze_refs(&samples,dt,0.05,0.1,0.9);
 assert!(full.est_ui>62.4e-12&&full.est_ui<62.6e-12);
 assert!(full.pll_ui.is_nan(),"full edge population exceeds the existing optional PLL work limit");
 assert!(full.rise_count<full.rises.len()&&full.fall_count<full.falls.len(),"partial global-level edges are excluded, not invented");
}

#[test]
fn pcie4_twenty_ps_knob_is_the_isolated_unfiltered_twenty_eighty_edge(){
 let mut lane=serial::LANES[3];lane.ssc=false;lane.bw=0.0;lane.deemph_db=0.0;lane.noise_rms=0.0;
 lane.rj_rms=0.0;lane.pj_peak=0.0;lane.dcd_pp=0.0;lane.corr_rj_rms=0.0;
 let v=serial::try_generate(&lane).unwrap();let a=analyze_refs(&v,serial::dt_of(&lane),0.05,0.2,0.8);
 assert!((a.rise_time-lane.rise).abs()<2.5e-12,"{}",a.rise_time);
 assert!((a.fall_time-lane.rise).abs()<2.5e-12,"{}",a.fall_time);
}

#[test]
fn incomplete_global_reference_transitions_are_missing_with_zero_qualified_count(){
 let mut v=vec![0.0];v.extend((0..4000).map(|i|if i%2==0{0.4}else{0.6}));v.push(1.0);
 let a=analyze_refs(&v,1e-9,0.01,0.1,0.9);
 assert!(a.rises.len()>1000&&a.falls.len()>1000);
 assert_eq!((a.rise_count,a.fall_count),(0,0));assert!(a.rise_time.is_nan()&&a.fall_time.is_nan());
}
