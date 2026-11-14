// SPDX-License-Identifier: GPL-2.0-or-later
//! Direct calibrated conversion and bounded, ordered CPU processing of DMA batches.
use super::hw::FrameDecoder;
use crate::board::Frame;
use oos_hal::dho::{hz, seq::Settings};
use std::sync::{Arc, mpsc};

#[derive(Default)]
pub(super) struct Converter {
    keys: [Option<(u64, u64)>; 4],
    tables: [Vec<f32>; 4],
}
impl Converter {
    /// No intermediate whole-record u16 vectors. Exclusive output buffers keep
    /// their capacity; persistent workers also reuse exact calibration tables.
    pub(super) fn convert(&mut self, bytes: &[u8], settings: &Settings, out: &mut [Vec<f32>; 4]) {
        let ratio = hz::ratio(settings.mask).max(1) as usize;
        let words = bytes.len() / 2;
        let mut rank = 0usize;
        for c in 0..4 {
            out[c].clear();
            if settings.mask & (1 << c) == 0 { continue; }
            let lane = if ratio == 4 { c } else { rank };
            rank += 1;
            let count = words.saturating_sub(lane).div_ceil(ratio);
            out[c].reserve(count);
            let (scale, offset) = (settings.ch[c].scale, settings.ch[c].offset);
            let table = count >= 65_536;
            if table {
                let key = (scale.to_bits(), offset.to_bits());
                if self.keys[c] != Some(key) {
                    self.tables[c] = (0..=u16::MAX).map(|code| ((code as f64 - 32768.0) / 7500.0 * scale - offset) as f32).collect();
                    self.keys[c] = Some(key);
                }
            }
            for word in (lane..words).step_by(ratio) {
                let at = word * 2;
                let code = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
                out[c].push(if table { self.tables[c][code as usize] }
                    else { ((code as f64 - 32768.0) / 7500.0 * scale - offset) as f32 });
            }
        }
    }
}

struct Job {
    bytes: Arc<Vec<u8>>, offset: usize, length: usize,
    settings: Arc<Settings>, params: hz::Params, index: usize, sequence: u64, stamp: u64,
}
struct Completed { worker: usize, index: usize, frame: Arc<Frame> }

/// At most four persistent workers, one queued job per worker, shared input
/// bytes once, and output restored to original acquisition sequence order.
pub(super) struct DecoderPool {
    inputs: Vec<mpsc::SyncSender<Job>>,
    output: Option<mpsc::Receiver<Completed>>,
    threads: Vec<std::thread::JoinHandle<()>>,
}
impl DecoderPool {
    pub(super) fn new(workers: usize) -> Self {
        let workers = workers.clamp(1, 4);
        let (results, output) = mpsc::sync_channel::<Completed>(workers);
        let mut inputs = Vec::new();
        let mut threads = Vec::new();
        for worker in 0..workers {
            let (input, requests) = mpsc::sync_channel::<Job>(1);
            let results = results.clone();
            inputs.push(input);
            threads.push(std::thread::Builder::new().name(format!("frame-decode-{worker}")).spawn(move || {
                let mut decoder = FrameDecoder::default();
                while let Ok(job) = requests.recv() {
                    let mut frame = decoder.decode(&job.bytes[job.offset..job.offset + job.length], &job.settings, &job.params, job.sequence);
                    frame.stamp = job.stamp;
                    if results.send(Completed { worker, index: job.index, frame: Arc::new(frame) }).is_err() { break; }
                }
            }).expect("start bounded frame decoder"));
        }
        Self { inputs, output: Some(output), threads }
    }
    pub(super) fn decode_batch(&mut self, bytes: Vec<u8>, settings: &Settings, params: &hz::Params,
        first_sequence: u64, stamp: u64, count: usize) -> Result<Vec<Arc<Frame>>, String> {
        let length = oos_hal::dho::seq::frame_bytes(settings, params);
        if length == 0 || count > 4096 || count.checked_mul(length).is_none_or(|needed| needed != bytes.len()) {
            return Err("invalid or oversized DMA frame batch".into());
        }
        if count == 0 { return Ok(Vec::new()); }
        let bytes = Arc::new(bytes);
        let settings = Arc::new(settings.clone());
        let job = |index| Job { bytes: Arc::clone(&bytes), offset: index * length, length,
            settings: Arc::clone(&settings), params: *params, index,
            sequence: first_sequence.wrapping_add(index as u64), stamp };
        let mut next = 0;
        for sender in self.inputs.iter().take(count) {
            sender.send(job(next)).map_err(|_| "frame decoder worker stopped")?;
            next += 1;
        }
        let mut ordered: Vec<Option<Arc<Frame>>> = vec![None; count];
        for _ in 0..count {
            let result = self.output.as_ref().unwrap().recv().map_err(|_| "frame decoder results stopped")?;
            ordered[result.index] = Some(result.frame);
            if next < count {
                self.inputs[result.worker].send(job(next)).map_err(|_| "frame decoder worker stopped")?;
                next += 1;
            }
        }
        Ok(ordered.into_iter().map(Option::unwrap).collect())
    }
}
impl Drop for DecoderPool {
    fn drop(&mut self) {
        self.inputs.clear();
        self.output.take();
        for thread in self.threads.drain(..) { let _ = thread.join(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::BasicStats;
    fn settings(mask: u32) -> Settings {
        let mut settings = Settings { mask, ..Settings::default() };
        for (c, channel) in settings.ch.iter_mut().enumerate() { channel.scale = 0.002 * (c + 1) as f64; channel.offset = -0.00013 * c as f64; }
        settings
    }
    fn legacy(bytes: &[u8], s: &Settings) -> [Vec<f32>; 4] {
        let words = oos_hal::dho::seq::deinterleave(bytes, s.mask);
        std::array::from_fn(|c| oos_hal::dho::seq::volts(&words[c], s.ch[c].scale, s.ch[c].offset))
    }
    #[test]
    fn fused_conversion_is_bit_exact_for_all_adc_codes_masks_and_tail_words() {
        let mut converter = Converter::default();
        let bytes: Vec<u8> = (0..=u16::MAX).cycle().take(262_147).flat_map(u16::to_le_bytes).chain(std::iter::once(0xff)).collect();
        let mut output = std::array::from_fn(|_| Vec::new());
        for mask in 1..16 {
            let mut s = settings(mask);
            for _ in 0..2 {
                converter.convert(&bytes, &s, &mut output);
                let expected = legacy(&bytes, &s);
                for c in 0..4 {
                    assert_eq!(output[c].len(), expected[c].len(), "mask{mask} CH{c}");
                    assert!(output[c].iter().zip(&expected[c]).all(|(a,b)| a.to_bits() == b.to_bits()), "mask{mask} CH{c}");
                }
                s.ch[0].scale *= 3.0; s.ch[0].offset = 0.14;
            }
        }
    }
    #[test]
    fn fused_conversion_reuses_exclusive_output_capacity_and_keeps_peak_pairs() {
        let mut converter = Converter::default();
        let s = settings(5);
        let bytes: Vec<u8> = [0u16, u16::MAX, 32768, 32769, 12345, 54321].into_iter().flat_map(u16::to_le_bytes).collect();
        let mut output = std::array::from_fn(|_| Vec::with_capacity(16));
        converter.convert(&bytes, &s, &mut output);
        let pointers = output.each_ref().map(|v| v.as_ptr());
        assert_eq!(output, legacy(&bytes, &s));
        converter.convert(&bytes, &s, &mut output);
        assert_eq!(output.each_ref().map(|v| v.as_ptr()), pointers);
        assert_eq!(output, legacy(&bytes, &s), "min/max envelope pair order stays intact");
        assert!(output.iter().flatten().all(|v| v.is_finite()));
    }
    #[test]
    fn worker_batch_is_bounded_ordered_and_contains_identical_basic_packets() {
        let mut pool = DecoderPool::new(4);
        assert_eq!(pool.inputs.len(), 4);
        let params = hz::Params { ratio: 1, rate: 1_000_000, memsel: 16, dot_fs: 1_000_000_000, seg: 0, max_frames: 12, pre: 8, post: 8 };
        let s = Settings { header: false, window: None, compress: 1, frames: 12, ..settings(1) };
        let length = oos_hal::dho::seq::frame_bytes(&s, &params);
        let bytes: Vec<u8> = (0..12).flat_map(|frame| std::iter::repeat(32768u16 + frame).take(length / 2).flat_map(u16::to_le_bytes)).collect();
        let frames = pool.decode_batch(bytes, &s, &params, 41, 77, 12).unwrap();
        assert_eq!(frames.len(), 12);
        for (i, frame) in frames.iter().enumerate() {
            assert_eq!((frame.seq, frame.stamp), (41 + i as u64, 77));
            assert_eq!(frame.basic[0].values, BasicStats::of(&frame.ch[0]).values);
            assert_eq!(frame.basic[0].count as usize, length / 2);
            assert!(frame.basic[0].valid);
            assert_eq!(frame.ch[0][0].to_bits(), (((i as f64) / 7500.0 * s.ch[0].scale - s.ch[0].offset) as f32).to_bits());
        }
        assert!(pool.decode_batch(vec![0; 2], &s, &params, 0, 0, 12).is_err());
        assert!(pool.decode_batch(vec![0; length * 12 + 8], &s, &params, 0, 0, 12).is_err(), "unexpected trailing bytes must not silently disappear");
    }
    #[test]
    fn basic_statistics_ignore_nonfinite_and_preserve_small_variance_at_dc() {
        let values = [f32::NAN, 1000.0, 1000.001, 999.999, f32::INFINITY];
        let summary = BasicStats::of(&values);
        assert_eq!(summary.count, 3);
        assert_eq!(summary.reading(crate::model::Meas::Vavg), Some(1000.0));
        assert!((summary.values[5] - 0.0007973599423122325).abs() < 1e-12);
        assert!(!BasicStats::of(&[f32::NAN]).valid);
        assert_eq!(summary.reading(crate::model::Meas::Freq), None);
    }

    #[test]
    fn hardware_latest_queue_fast_path_and_retained_record_share_the_same_frame() {
        use crate::{board::{Shared, Frame}, model::Model, wave::{Source, Rng}};
        let frame = Arc::new(Frame { seq: 9, dt: 1e-9, pre: 50_000,
            ch: [vec![0.25; 100_000], Vec::new(), Vec::new(), Vec::new()], ..Frame::default() });
        let shared = Arc::new(std::sync::Mutex::new(Shared::default()));
        { let mut state = shared.lock().unwrap(); state.frame = Arc::clone(&frame);
          state.queue.push_back(Arc::clone(&frame)); state.queue.push_back(Arc::clone(&frame)); }
        let mut source = Source::Hw { shared: Arc::clone(&shared), seen: 0 };
        let fast = source.drain_but_last();
        assert_eq!(fast.len(), 1);
        assert!(Arc::ptr_eq(&fast[0], &frame));
        let mut model = Model::default();
        model.tscale = crate::model::tscales().iter().position(|&v| v >= 1e-6).unwrap();
        let record = source.acquire(&model, 0.01, &mut Rng::new(1)).unwrap();
        assert!(Arc::ptr_eq(&record.raw.unwrap().frame, &frame));
        assert!(Arc::ptr_eq(&shared.lock().unwrap().frame, &frame));
        assert_eq!(source.probe(0, 0.0, 1e-9, 8), vec![0.25; 8]);
    }

    #[test]
    #[ignore = "explicit local host timing; not a CI performance assertion"]
    fn benchmark_hardware_decode_pipeline() {
        use std::{hint::black_box, time::Instant};
        fn median(mut action: impl FnMut()) -> f64 {
            let mut times = Vec::new();
            for _ in 0..5 { let started = Instant::now(); action(); times.push(started.elapsed().as_secs_f64() * 1000.0); }
            times.sort_by(f64::total_cmp); times[2]
        }
        let points = 500_000usize;
        let s = Settings { header: false, window: None, compress: 1, frames: 12, ..settings(15) };
        let params = hz::Params { ratio: 4, rate: 1_000_000, memsel: points as u32, dot_fs: 1_000_000_000,
            seg: 0, max_frames: 12, pre: (points / 2) as u32, post: (points / 2) as u32 };
        let bytes: Vec<u8> = (0..points * 4).map(|i| (i.wrapping_mul(13007) & 65535) as u16).flat_map(u16::to_le_bytes).collect();
        let mut converter = Converter::default();
        let mut reusable = std::array::from_fn(|_| Vec::new());
        converter.convert(&bytes, &s, &mut reusable);
        let baseline = legacy(&bytes, &s);
        assert_eq!(baseline, reusable);
        let legacy_allocate = median(|| { black_box(legacy(black_box(&bytes), black_box(&s))); });
        let fused_allocate = median(|| { let mut output = std::array::from_fn(|_| Vec::new()); converter.convert(black_box(&bytes), black_box(&s), &mut output); black_box(output); });
        let fused_reuse = median(|| { converter.convert(black_box(&bytes), black_box(&s), &mut reusable); black_box(&reusable); });
        let legacy_stats = median(|| { let values = legacy(black_box(&bytes), black_box(&s)); black_box(values.each_ref().map(|v| BasicStats::of(v))); });
        let mut decoder = FrameDecoder::default();
        let mut frame = Frame::default();
        decoder.decode_into(&bytes, &s, &params, 1, &mut frame);
        let fused_stats = median(|| { decoder.decode_into(black_box(&bytes), black_box(&s), &params, 1, &mut frame); black_box(&frame); });
        let batch = bytes.repeat(12);
        assert_eq!(oos_hal::dho::seq::frame_bytes(&s, &params), bytes.len());
        let mut one = DecoderPool::new(1);
        let mut four = DecoderPool::new(4);
        // Warm persistent calibration tables before measuring identical batches.
        let warm = one.decode_batch(batch.clone(), &s, &params, 0, 0, 12).unwrap();
        assert!(warm.iter().all(|frame| frame.ch.iter().all(|channel| channel.len() == points)));
        drop(warm);
        drop(four.decode_batch(batch.clone(), &s, &params, 0, 0, 12).unwrap());
        let mut one_times = Vec::new(); let mut four_times = Vec::new();
        for _ in 0..3 {
            let input = batch.clone(); let started = Instant::now();
            let output = one.decode_batch(input, &s, &params, 0, 0, 12).unwrap();
            one_times.push(started.elapsed().as_secs_f64() * 1000.0); black_box(&output); drop(output);
            let input = batch.clone(); let started = Instant::now();
            let output = four.decode_batch(input, &s, &params, 0, 0, 12).unwrap();
            four_times.push(started.elapsed().as_secs_f64() * 1000.0); black_box(&output); drop(output);
        }
        one_times.sort_by(f64::total_cmp); four_times.sort_by(f64::total_cmp);
        let report = format!("{{\n  \"scope\":\"host CPU only; no hardware or GUI; optimized build\",\n  \"points_per_channel\":{points},\n  \"channels\":4,\n  \"batch_frames\":12,\n  \"legacy_convert_allocate_median_ms\":{legacy_allocate},\n  \"fused_convert_allocate_median_ms\":{fused_allocate},\n  \"fused_convert_reuse_median_ms\":{fused_reuse},\n  \"legacy_convert_plus_six_stats_median_ms\":{legacy_stats},\n  \"fused_reuse_plus_six_stats_median_ms\":{fused_stats},\n  \"pool_one_worker_median_ms\":{},\n  \"pool_four_workers_median_ms\":{},\n  \"pool_host_speedup\":{}\n}}\n", one_times[1], four_times[1], one_times[1] / four_times[1]);
        if let Ok(path) = std::env::var("OOS_DECODE_BENCH_PATH") { std::fs::write(path, &report).unwrap(); }
        println!("{report}");
    }
}
