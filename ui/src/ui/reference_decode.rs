// SPDX-License-Identifier: GPL-2.0-or-later
//! Stored protocol inputs share their physical time coordinates with live inputs.
//! One worker and a bounded result cache serve bands, tables, export and Search.
use super::*;
use oos_dsp::decode::Packet;
use std::sync::{Arc, mpsc};

const BACKGROUND_POINTS: usize = 262_144;
const MAX_POINTS: usize = 32_000_000;
const MAX_PACKETS: usize = 250_000;
pub(super) const COMPUTING: &str = "Decode computing";
type Packets = Result<Arc<Vec<Packet>>, &'static str>;

struct Wave { source: usize, v: Vec<f32>, dt: f64, t0: f64 }
struct Job { key: String, model: Model, waves: Vec<Wave>, hysteresis: f32 }
struct Finished { key: String, packets: Packets }
struct Worker { input: mpsc::SyncSender<Job>, output: mpsc::Receiver<Finished>, busy: bool }
impl Worker {
    fn open() -> Self {
        let (input, requests) = mpsc::sync_channel::<Job>(1);
        let (results, output) = mpsc::sync_channel(1);
        std::thread::Builder::new().name("reference-decode".into()).spawn(move || {
            while let Ok(job) = requests.recv() {
                let finished = Finished { key: job.key.clone(), packets: job.run() };
                if results.send(finished).is_err() { break; }
            }
        }).expect("start bounded reference decode worker");
        Self { input, output, busy: false }
    }
}

struct Entry { key: String, packets: Packets, hits: Option<(BusWhat, u32, Arc<Vec<f64>>)> }
#[derive(Default)]
pub(super) struct Cache { entries: Vec<Entry>, worker: Option<Worker>, scans: usize }
impl Cache {
    fn put(&mut self, key: String, packets: Packets) {
        if self.entries.len() >= DECODERS * 2 { self.entries.remove(0); }
        self.entries.push(Entry { key, packets, hits: None });
    }
}

/// The required analogue roles, including the implicit Parallel data bank.
fn sources(m: &Model) -> Result<Vec<usize>, &'static str> {
    let mut ids = vec![m.dec_src];
    if m.dec_proto == Proto::Parallel {
        let (base, count) = if m.dec_src >= DEC_REF_BASE { (DEC_REF_BASE, NREF) } else { (0, NCH) };
        ids = (0..count).map(|i| base + (m.dec_src - base + i) % count)
            .filter(|id| Some(*id) != m.dec_cs)
            .take((m.dec_bits.clamp(1, NCH as u32) as usize).min(NCH - usize::from(m.dec_cs.is_some())))
            .collect();
    } else {
        let roles = m.dec_proto.roles();
        if roles[1].is_some() { ids.push(m.dec_src2); }
        if roles[3].is_some() { ids.extend(m.dec_src4); }
        if m.dec_proto.diff_ok() { ids.extend(m.dec_diff); }
    }
    if m.dec_proto.roles()[2].is_some() { ids.extend(m.dec_cs); }
    let mut unique = Vec::new();
    for id in ids {
        if decode_wave(id).is_none() { return Err("Invalid decoder source"); }
        if !unique.contains(&id) { unique.push(id); }
    }
    if unique.len() > NCH { return Err("Too many decoder roles"); }
    Ok(unique)
}

impl Job {
    fn run(&self) -> Packets {
        let start = self.waves.iter().map(|w| w.t0).fold(f64::NEG_INFINITY, f64::max);
        let stop = self.waves.iter().map(|w| w.t0 + (w.v.len() - 1) as f64 * w.dt).fold(f64::INFINITY, f64::min);
        let finest = self.waves.iter().min_by(|a, b| a.dt.total_cmp(&b.dt)).ok_or("Decoder source empty")?;
        let dt = finest.dt;
        let snap = |value: f64| if (value - value.round()).abs() <= 1e-7 { value.round() } else { value };
        let first = snap((start - finest.t0) / dt).ceil();
        let last = snap((stop - finest.t0) / dt).floor();
        if !first.is_finite() || !last.is_finite() || last - first < 1.0 { return Err("Decoder sources do not overlap"); }
        let count = (last - first + 1.0) as usize;
        if count > MAX_POINTS { return Err("Decoder aligned record exceeds 32M samples"); }
        let t0 = finest.t0 + first * dt;
        let aligned: Vec<Option<Vec<f32>>> = self.waves.iter().map(|w| {
            if w.dt == dt && w.t0 == t0 && w.v.len() == count { return None; }
            Some((0..count).map(|i| {
                let index = snap((t0 + i as f64 * dt - w.t0) / w.dt);
                if index < 0.0 || index > (w.v.len() - 1) as f64 { return f32::NAN; }
                let k = index.floor() as usize;
                if k + 1 == w.v.len() { w.v[k] }
                else { w.v[k] + index.fract() as f32 * (w.v[k + 1] - w.v[k]) }
            }).collect())
        }).collect();
        let ch: [&[f32]; NCH] = std::array::from_fn(|i| self.waves.get(i).map_or(&[][..], |w| aligned[i].as_deref().unwrap_or(&w.v)));
        let slot = |source| self.waves.iter().position(|w| w.source == source);
        let mut model = self.model.clone();
        model.dec_src = slot(model.dec_src).unwrap_or(0);
        model.dec_src2 = slot(model.dec_src2).unwrap_or(0);
        model.dec_cs = model.dec_cs.and_then(slot);
        model.dec_src4 = model.dec_src4.and_then(slot);
        model.dec_diff = model.dec_diff.and_then(slot);
        // Parallel's selected bank is already ordered in the four DSP slots.
        if model.dec_proto == Proto::Parallel { model.dec_src = 0; }
        let mut packets = crate::trig::decode_at(&model, ch, dt, model.dec_thr, self.hysteresis)?;
        if packets.len() > MAX_PACKETS { return Err("Decode exceeds 250000 packets; shorten the record"); }
        for packet in &mut packets {
            packet.t0 = t0 + packet.t0 * dt;
            packet.t1 = t0 + packet.t1 * dt;
        }
        Ok(Arc::new(packets))
    }
}

impl Ui {
    pub(super) fn decoder_uses_refs(m: &Model) -> bool {
        !m.dec_on_la() && sources(m).is_ok_and(|ids| ids.iter().any(|&id| id >= DEC_REF_BASE))
    }
    fn decoder_samples(&self, id: usize) -> Result<(&[f32], f64, f64, f32), &'static str> {
        self.decoder_samples_at(id, None)
    }
    fn decoder_samples_at<'a>(&'a self, id: usize, candidate: Option<&'a Record>) -> Result<(&'a [f32], f64, f64, f32), &'static str> {
        let (v, dt, t0, vpd) = match decode_wave(id).ok_or("Invalid decoder source")? {
            WaveId::Ref(i) => {
                let r = self.refs[i].as_ref().ok_or("Reference source empty")?;
                (r.v.as_slice(), r.dt, self.source_time_origin(WaveId::Ref(i), r.t0), r.vpd)
            }
            WaveId::Ch(c) => {
                let r = candidate.or(self.last[0].as_ref()).ok_or("Live decoder source has no record")?;
                if let Some(raw) = r.raw.as_ref().filter(|raw| !raw.frame.envelope && raw.frame.ch[c].len() >= 2) {
                    if !raw.frame.dt.is_finite() || raw.frame.dt <= 0.0 || !raw.t0.is_finite() { return Err("Invalid decoder record"); }
                    return Ok((&raw.frame.ch[c], raw.frame.dt, self.source_time_origin(WaveId::Ch(c), raw.t0 - raw.i0 as f64 * raw.frame.dt), self.m.ch[c].volts_per_div()));
                }
                (r.ch[c].as_slice(), r.dt, self.source_time_origin(WaveId::Ch(c), r.t0), self.m.ch[c].volts_per_div())
            }
        };
        if v.len() < 2 || !dt.is_finite() || dt <= 0.0 || !t0.is_finite() {
            return Err("Invalid decoder record");
        }
        Ok((v, dt, t0, vpd))
    }
    fn decoder_key(&self, m: &Model) -> Result<String, &'static str> {
        self.decoder_key_at(m, None)
    }
    pub(super) fn decoder_key_at(&self, m: &Model, candidate: Option<&Record>) -> Result<String, &'static str> {
        let mut key = format!("{:?}|{}|", m.dec_cfg(), self.workflow_epoch);
        for id in sources(m)? {
            let (v, dt, t0, vpd) = self.decoder_samples_at(id, candidate)?;
            let revision = if id < NCH { candidate.or(self.last[0].as_ref()).and_then(|rec| rec.raw.as_ref()).map_or(self.wfm_total, |raw| raw.frame.seq) }
                else if self.session_is_sim && id == DEC_REF_BASE + self.m.sim_slot { self.replay.sim_attempt }
                else { 0 };
            key.push_str(&format!("{id}:{:p}:{}:{}:{}:{}:{revision}|", v.as_ptr(), v.len(), dt.to_bits(), t0.to_bits(), vpd.to_bits()));
        }
        Ok(key)
    }
    pub(super) fn reference_decode_packets(&self, m: &Model) -> Packets {
        self.reference_decode_packets_at(m, None)
    }
    pub(super) fn decoder_candidate_key(&self, m: &Model, record: &Record) -> Result<String, &'static str> {
        Ok(format!("candidate:{}|{}", self.bus_candidate.epoch, self.decoder_key_at(m, Some(record))?))
    }
    pub(super) fn reference_decode_packets_at(&self, m: &Model, candidate: Option<&Record>) -> Packets {
        let key = if let Some(record) = candidate { self.decoder_candidate_key(m, record)? } else { self.decoder_key(m)? };
        let mut cache = self.azr.reference_decode.borrow_mut();
        let finished = cache.worker.as_mut().and_then(|w| w.output.try_recv().ok().map(|result| { w.busy = false; result }));
        if let Some(result) = finished {
            // The worker survives imports. A completed old source/settings job
            // must never repopulate the current workspace's accepted results.
            let current = (0..DECODERS).any(|k| self.decoder_key(&self.m.dec_view(k)).ok().as_ref() == Some(&result.key))
                || self.bus_candidate.record.as_ref().is_some_and(|record| self.decoder_candidate_key(&self.m, record).ok().as_ref() == Some(&result.key));
            if current { cache.put(result.key, result.packets); }
        }
        if let Some(entry) = cache.entries.iter().find(|entry| entry.key == key) { return entry.packets.clone(); }
        if cache.worker.as_ref().is_some_and(|worker| worker.busy) { return Err(COMPUTING); }
        let ids = sources(m)?;
        let left = ids.iter().map(|&id| self.decoder_samples_at(id, candidate).map(|(_, _, t0, _)| t0)).collect::<Result<Vec<_>, _>>()?
            .into_iter().fold(f64::NEG_INFINITY, f64::max);
        let right = ids.iter().map(|&id| self.decoder_samples_at(id, candidate).map(|(v, dt, t0, _)| t0 + (v.len() - 1) as f64 * dt)).collect::<Result<Vec<_>, _>>()?
            .into_iter().fold(f64::INFINITY, f64::min);
        let finest = ids.iter().map(|&id| self.decoder_samples_at(id, candidate).map(|(_, dt, _, _)| dt)).collect::<Result<Vec<_>, _>>()?
            .into_iter().fold(f64::INFINITY, f64::min);
        if right <= left { return Err("Decoder sources do not overlap"); }
        if (right - left) / finest + 1.0 > MAX_POINTS as f64 + 1e-7 { return Err("Decoder aligned record exceeds 32M samples"); }
        let mut waves = Vec::new();
        for id in ids {
            let (v, dt, t0, _) = self.decoder_samples_at(id, candidate)?;
            // Only the physical intersection is copied, with one neighbour for
            // interpolation. A 500 MB CH record overlapping a short R record
            // must not create another full-size live-data allocation.
            let lo = (((left - t0) / dt).floor().max(0.0) as usize).min(v.len() - 1);
            let hi = ((((right - t0) / dt).ceil().max(0.0) as usize).saturating_add(1)).min(v.len());
            waves.push(Wave { source: id, v: v[lo..hi].to_vec(), dt, t0: t0 + lo as f64 * dt });
        }
        let (_, _, _, vpd) = self.decoder_samples_at(m.dec_src, candidate)?;
        let large = waves.iter().any(|wave| wave.v.len() > BACKGROUND_POINTS);
        let job = Job { key: key.clone(), model: m.clone(), waves, hysteresis: 0.1 * vpd.max(1e-9) };
        cache.scans += 1;
        if large {
            let worker = cache.worker.get_or_insert_with(Worker::open);
            worker.input.try_send(job).map_err(|_| "Decode worker unavailable")?;
            worker.busy = true;
            Err(COMPUTING)
        } else {
            let packets = job.run();
            cache.put(key, packets.clone());
            packets
        }
    }
    pub(super) fn seed_accepted_decode(&self, record: &Record, packets: Arc<Vec<Packet>>) {
        if let Ok(key) = self.decoder_key_at(&self.m, Some(record)) {
            self.azr.reference_decode.borrow_mut().put(key, Ok(packets));
        }
    }
    pub(super) fn decoder_trigger_hits(&self, m: &Model) -> Option<Arc<Vec<f64>>> {
        let packets = self.reference_decode_packets(m).ok()?;
        let key = self.decoder_key(m).ok()?;
        let mut cache = self.azr.reference_decode.borrow_mut();
        let entry = cache.entries.iter_mut().find(|entry| entry.key == key)?;
        if let Some((kind, value, hits)) = &entry.hits {
            if *kind == m.trig_bus && *value == m.trig_bus_val { return Some(Arc::clone(hits)); }
        }
        let mut times: Vec<_> = packets.iter().filter(|packet| crate::trig::matches(m, packet)).map(|packet| packet.t1).collect();
        times.sort_by(f64::total_cmp);
        let hits = Arc::new(times);
        entry.hits = Some((m.trig_bus, m.trig_bus_val, Arc::clone(&hits)));
        Some(hits)
    }
    pub fn decode_busy(&self) -> bool {
        for k in 0..DECODERS {
            let model = self.m.dec_view(k);
            if model.dec_on && Self::decoder_uses_refs(&model) { let _ = self.reference_decode_packets(&model); }
        }
        self.azr.reference_decode.borrow().worker.as_ref().is_some_and(|worker| worker.busy)
    }
    pub fn decode_events_result(&self) -> Result<String, &'static str> {
        for k in 0..DECODERS {
            if self.m.dec_is_on(k) { self.decode_packets_of(&self.m.dec_view(k))?; }
        }
        Ok(self.events_text())
    }
    pub(super) fn decoder_sources(&self) -> Vec<usize> {
        (0..n_ch()).chain((0..NREF).filter(|&i| self.refs[i].is_some()).map(|i| DEC_REF_BASE + i)).collect()
    }
    pub(super) fn decoder_vpd(&self) -> f32 {
        self.decoder_samples(self.m.dec_src).map_or(1.0, |(_, _, _, vpd)| vpd.max(1e-9))
    }
    #[cfg(test)]
    pub(super) fn decoder_worker_state_for_test(&self) -> (usize, usize) {
        let cache = self.azr.reference_decode.borrow();
        (usize::from(cache.worker.is_some()), cache.scans)
    }
    pub(super) fn decoder_source_step(&self, source: Option<usize>, optional: bool, delta: i32) -> Option<usize> {
        let mut sources: Vec<_> = self.decoder_sources().into_iter().map(Some).collect();
        if optional { sources.insert(0, None); }
        let at = sources.iter().position(|value| *value == source).unwrap_or(0);
        sources[(at as i32 + delta).rem_euclid(sources.len() as i32) as usize]
    }
    pub(crate) fn configure_decoder_source(&mut self, role: usize, source: Option<usize>) -> bool {
        if role > 4 || ((role < 2 || (role == 2 && self.m.dec_proto == Proto::Cphy)) && source.is_none())
            || source.is_some_and(|id| !self.decoder_sources().contains(&id)) { return false; }
        self.m.dec_la_in = false;
        match role {
            0 => self.m.dec_src = source.unwrap(), 1 => self.m.dec_src2 = source.unwrap(),
            2 => self.m.dec_cs = source, 3 => self.m.dec_src4 = source, _ => self.m.dec_diff = source,
        }
        if self.m.trig_type == TrigType::Bus {
            if let Some(wave) = decode_wave(self.m.dec_src) {
                if matches!(wave, WaveId::Ref(_)) != self.record_mode() { self.set_trigger_source(wave, false); }
                else { self.m.set_trigger_wave(wave, false); }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn reference(v: Vec<f32>, dt: f64, t0: f64, vpd: f32) -> RefWave {
        RefWave { src: 0, color: 0, label: "decoder fixture".into(), t0, dt, v, vpd, offset: 0.0, on: true }
    }
    fn uart(byte: u8, count: usize) -> (Vec<f32>, f64) {
        let mut values = vec![3.3; count];
        let bits: Vec<bool> = std::iter::once(false).chain((0..8).map(|b| byte >> b & 1 != 0)).chain(std::iter::once(true)).collect();
        for (bit, high) in bits.into_iter().enumerate() {
            values[100 + bit * 20..100 + (bit + 1) * 20].fill(if high { 3.3 } else { 0.0 });
        }
        (values, 1.0 / (115_200.0 * 20.0))
    }
    fn uart_ui(count: usize) -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let (v, dt) = uart(0xa5, count);
        ui.refs[0] = Some(reference(v, dt, -0.005, 0.5));
        ui.m.dec_proto = Proto::Uart;
        ui.m.dec_on = true;
        ui.m.dec_src = DEC_REF_BASE;
        ui.m.dec_baud = crate::decode::BAUDS.iter().position(|&v| v == 115_200.0).unwrap();
        ui.m.dec_thr = 1.65;
        ui.m.dec_bits = 8;
        ui.m.dec_parity = oos_dsp::decode::Parity::None;
        ui.m.run = RunState::Stop;
        ui.m.set_trigger_wave(WaveId::Ref(0), false);
        ui
    }
    fn wait(ui: &Ui) -> Arc<Vec<Packet>> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match ui.reference_decode_packets(&ui.m) {
                Ok(packets) => return packets,
                Err(COMPUTING) => { assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(1)); }
                Err(error) => panic!("{error}"),
            }
        }
    }

    #[test]
    fn reference_uart_uses_original_time_and_shared_search_event_cache() {
        let mut ui = uart_ui(1000);
        assert!(ui.last[0].is_none());
        let packets = wait(&ui);
        let data = packets.iter().find(|p| p.kind == oos_dsp::decode::Kind::Data).unwrap();
        assert_eq!(data.value, 0xa5);
        assert!(data.t0 > -0.005 && data.t1 < -0.004);
        ui.trigger_type_set(TrigType::Bus);
        assert!(ui.record_mode(), "a reference Bus trigger must arm the stored source");
        ui.m.trig_bus = BusWhat::Data;
        ui.m.trig_bus_val = 0xa5;
        let hits = ui.search_events();
        assert_eq!(hits.as_ref(), &vec![data.t1]);
        ui.m.delay = 0.123;
        ui.m.zdelay = -0.7;
        assert!(Arc::ptr_eq(&hits, &ui.search_events()));
        assert!(ui.events_text().contains("D1 UART R1"));
        assert!(ui.decode_events_result().unwrap().contains("A5"));
        assert_eq!(ui.azr.reference_decode.borrow().scans, 1);
        assert_eq!(ui.replay.captures, 0);
        assert_eq!(ui.m.run, RunState::Stop);
        ui.m.trig_bus_val = 0x5a;
        assert!(ui.search_events().is_empty());
        assert_eq!(ui.azr.reference_decode.borrow().scans, 1);
    }

    #[test]
    fn loaded_sources_are_selectable_in_every_decoder_role_and_survive_setup() {
        let mut ui = uart_ui(1000);
        ui.refs[9] = Some(reference(vec![0.0; 1000], 1e-9, -1e-6, 0.2));
        for (item, role) in [(Item::DecSrc, 0), (Item::DecSrc2, 1), (Item::DecCs, 2), (Item::DecSrc4, 3), (Item::DecDiff, 4)] {
            assert!(ui.options(item).contains(&"R10".to_string()));
            assert!(ui.configure_decoder_source(role, Some(DEC_REF_BASE + 9)));
            assert_eq!(ui.value(item), "R10");
        }
        assert!(!ui.configure_decoder_source(0, Some(DEC_REF_BASE + 8)), "empty refs are not a valid selection");
        let text = crate::store::setup_text(&ui.m);
        let mut restored = crate::store::parse_setup(&text).unwrap();
        assert_eq!((restored.dec_src, restored.dec_src2, restored.dec_cs, restored.dec_src4, restored.dec_diff),
            (DEC_REF_BASE + 9, DEC_REF_BASE + 9, Some(DEC_REF_BASE + 9), Some(DEC_REF_BASE + 9), Some(DEC_REF_BASE + 9)));
        assert!(crate::store::set(&mut restored, "dec_src", "4").is_err(), "reserved LA IDs must not become CH indices");
        ui.set_dec_proto(Proto::Jtag);
        assert_eq!(ui.m.dec_src, DEC_REF_BASE + 9, "protocol changes must preserve stored roles");
        let mut canvas = Canvas::new(W, H);
        ui.render(&mut canvas);
    }

    #[test]
    fn four_wire_and_mixed_inputs_align_at_native_physical_times() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let clk = |t: f64| if (0..8).any(|bit| (5.0 + 8.0 * bit as f64..9.0 + 8.0 * bit as f64).contains(&t)) { 3.3 } else { 0.0 };
        let data = |t: f64| { let bit = ((t - 3.0).max(0.0) / 8.0) as usize; if bit < 8 && 0xa5u8 >> (7 - bit) & 1 != 0 { 3.3 } else { 0.0 } };
        ui.refs[0] = Some(reference((0..100).map(|i| data(-3.0 + i as f64)).collect(), 1e-6, -3e-6, 0.5));
        ui.refs[1] = Some(reference((0..220).map(|i| clk(-5.0 + i as f64 * 0.5)).collect(), 0.5e-6, -5e-6, 0.5));
        ui.m.dec_proto = Proto::Spi;
        ui.m.dec_src = DEC_REF_BASE;
        ui.m.dec_src2 = DEC_REF_BASE + 1;
        ui.m.dec_cs = None;
        ui.m.dec_thr = 1.65;
        ui.m.dec_bits = 8;
        let packets = wait(&ui);
        assert!(packets.iter().any(|packet| packet.kind == oos_dsp::decode::Kind::Data && packet.value == 0xa5), "{packets:?}");
        assert!(packets.iter().all(|packet| packet.t0 >= -3e-6 && packet.t1 <= 96e-6));
        // Use the very same physical clock as a captured CH input instead.
        let clock = ui.refs[1].as_ref().unwrap();
        let mut rec = Record { ch: std::array::from_fn(|_| Vec::new()), dt: clock.dt, t0: clock.t0,
            triggered: true, partial: false, raw: None, la: None };
        rec.ch[1] = clock.v.clone(); rec.dt = clock.dt; rec.t0 = clock.t0;
        ui.last[0] = Some(rec);
        ui.m.dec_src2 = 1;
        assert_eq!(wait(&ui).as_ref(), packets.as_ref(), "mixed live/ref alignment must preserve packets");
        let full_clock = Arc::new(crate::board::Frame { seq: 19, dt: clock.dt,
            ch: [Vec::new(), clock.v.clone(), Vec::new(), Vec::new()], ..crate::board::Frame::default() });
        let record = ui.last[0].as_mut().unwrap();
        record.ch[1] = vec![0.0; 4]; record.dt *= 50.0;
        record.raw = Some(crate::wave::Raw { frame: full_clock, i0: 10, i1: 14, t0: clock.t0 + 10.0 * clock.dt, conditioned: false });
        assert_eq!(wait(&ui).as_ref(), packets.as_ref(), "a decimated screen must not replace retained CH samples during mixed decode");
        // All four JTAG roles are valid stored roles; non-overlap is explicit.
        for i in 2..4 { ui.refs[i] = Some(reference(vec![0.0; 100], 1e-6, 1.0, 0.5)); }
        ui.m.dec_proto = Proto::Jtag;
        ui.m.dec_src2 = DEC_REF_BASE + 1;
        ui.m.dec_cs = Some(DEC_REF_BASE + 2);
        ui.m.dec_src4 = Some(DEC_REF_BASE + 3);
        assert_eq!(ui.reference_decode_packets(&ui.m).unwrap_err(), "Decoder sources do not overlap");
    }

    #[test]
    fn large_decode_is_nonblocking_single_worker_and_discards_changed_source_results() {
        let mut ui = uart_ui(BACKGROUND_POINTS + 1000);
        let started = Instant::now();
        assert_eq!(ui.reference_decode_packets(&ui.m).unwrap_err(), COMPUTING);
        assert!(started.elapsed() < Duration::from_millis(250), "UI only queues a bounded copy");
        assert_eq!(ui.azr.reference_decode.borrow().scans, 1);
        let (v, dt) = uart(0x5a, BACKGROUND_POINTS + 1000);
        ui.refs[1] = Some(reference(v, dt, -0.008, 0.5));
        ui.m.dec_src = DEC_REF_BASE + 1;
        ui.workflow_epoch += 1;
        ui.replay = super::super::workflow::Replay::default();
        let packets = wait(&ui);
        assert!(packets.iter().any(|p| p.value == 0x5a));
        assert!(!packets.iter().any(|p| p.value == 0xa5));
        assert!(packets.iter().all(|p| p.t0 < -0.007));
        let cache = ui.azr.reference_decode.borrow();
        assert_eq!(cache.scans, 2);
        assert!(cache.worker.is_some());
        assert!(!cache.worker.as_ref().unwrap().busy);
        assert_eq!(cache.entries.len(), 1, "stale source result must not enter the accepted cache");
        drop(cache);
        assert!(Arc::ptr_eq(&packets, &wait(&ui)));
        assert_eq!(ui.replay.captures, 0);
    }

    #[test]
    fn all_twenty_seven_protocol_dispatchers_match_the_same_live_samples_as_refs() {
        for protocol in PROTOS.into_iter().filter(|protocol| *protocol != Proto::Cphy) {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.set_dec_proto(protocol);
            ui.m.dec_on = true;
            for channel in &mut ui.m.ch { channel.on = true; }
            let tdiv = (ui.m.bus_bit() * 20.0) as f32;
            ui.m.tscale = tscales().iter().position(|&scale| scale >= tdiv).unwrap();
            ui.m.trig_type = TrigType::Edge;
            ui.m.trig_mode = TrigMode::Auto;
            ui.step(0.05);
            let expected = ui.decode_packets().unwrap_or_else(|error| panic!("{protocol:?} live fixture: {error}"));
            let rec = ui.last[0].as_ref().unwrap();
            for c in 0..NCH {
                if !rec.ch[c].is_empty() { ui.refs[c] = Some(reference(rec.ch[c].clone(), rec.dt, rec.t0, ui.m.ch[c].volts_per_div())); }
            }
            ui.m.dec_src += DEC_REF_BASE;
            ui.m.dec_src2 += DEC_REF_BASE;
            ui.m.dec_cs = ui.m.dec_cs.map(|c| DEC_REF_BASE + c);
            ui.m.dec_src4 = ui.m.dec_src4.map(|c| DEC_REF_BASE + c);
            ui.m.dec_diff = ui.m.dec_diff.map(|c| DEC_REF_BASE + c);
            ui.last[0] = None;
            let actual = ui.decode_packets().unwrap_or_else(|error| panic!("{protocol:?} reference fixture: {error}"));
            assert_eq!(actual, expected, "{protocol:?} native-interval packets changed after source remapping");
        }
    }

    #[test]
    fn cphy_three_reference_roles_decode_the_published_synchronized_word() {
        let mut symbols = vec![3; 12];
        symbols.extend([3, 4, 4, 4, 4, 4, 3]);
        symbols.extend([0, 2, 2, 4, 1, 0, 0]);
        symbols.extend([4; 7]);
        let states = oos_dsp::cphy::states_for_symbols(0, &symbols);
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let dt = 5e-12;
        let t0 = -12e-9;
        for wire in 0..3 {
            let values = states.iter().flat_map(|&state| std::iter::repeat(oos_dsp::cphy::wire_values(state)[wire]).take(20)).collect();
            ui.refs[wire] = Some(reference(values, dt, t0, 0.1));
        }
        ui.m.dec_src = DEC_REF_BASE;
        ui.set_dec_proto(Proto::Cphy);
        assert!(ui.configure_decoder_source(1, Some(DEC_REF_BASE + 1)));
        assert!(ui.configure_decoder_source(2, Some(DEC_REF_BASE + 2)));
        assert!(!ui.configure_decoder_source(2, None), "the third C-PHY wire is required");
        ui.m.dec_on = true;
        ui.m.trig_type = TrigType::Bus;
        ui.m.trig_bus = BusWhat::Data;
        ui.m.trig_bus_val = 0x7290;
        assert_eq!(ui.value(Item::DecSrc), "R1");
        assert_eq!(ui.value(Item::DecSrc2), "R2");
        assert_eq!(ui.value(Item::DecCs), "R3");
        assert_eq!(ui.item_label(Item::DecCs), "C");
        assert_eq!(ui.item_label(Item::DecThr), "Min level gap");
        let packets = wait(&ui);
        let word = packets.iter().find(|packet| packet.kind == (oos_dsp::decode::Kind::Word { cmd: false }) && packet.value == 0x7290).unwrap();
        assert!(word.ok && word.t0 >= t0 && word.t1 < 0.0);
        assert_eq!(ui.search_events().as_ref(), &vec![word.t1]);
        assert!(ui.events_text().contains("7290"));
        assert_eq!(ui.value(Item::DecCphyRate), "Auto");
        assert!(ui.decode_items().contains(&Item::DecCphyRate));
        assert!(ui.options(Item::DecCphyRate).contains(&"2.25 Gsym/s".to_string()));
        ui.select_option(Item::DecCphyRate, "2.25 Gsym/s");
        assert_eq!(ui.m.dec_cphy_rate, 2.25e9);
        assert!(!wait(&ui).iter().any(|packet| packet.kind == (oos_dsp::decode::Kind::Word { cmd: false }) && packet.value == 0x7290),
            "an intentionally slower known rate must change stability filtering, not remain an ignored UI label");
        ui.select_option(Item::DecCphyRate, "10 Gsym/s");
        assert!(wait(&ui).iter().any(|packet| packet.kind == (oos_dsp::decode::Kind::Word { cmd: false }) && packet.value == 0x7290));
        ui.m.dec_cphy_rate = 1.234567890123e9;
        let restored = crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap();
        assert_eq!(restored.dec_cphy_rate, ui.m.dec_cphy_rate);
        ui.m.dec_select(1); assert_eq!(ui.m.dec_cphy_rate, 0.0);
        ui.m.dec_select(0); assert_eq!(ui.m.dec_cphy_rate, 1.234567890123e9);
        ui.select_option(Item::DecCphyRate, "Auto");
        assert_eq!(ui.m.dec_cphy_rate, 0.0, "the picker must also reset an arbitrary SCPI-provided rate");
        let mut canvas = Canvas::new(W, H);
        ui.render(&mut canvas);
    }
}
