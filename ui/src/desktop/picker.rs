// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The native Open dialog. Canceling never changes the session.

use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::io::Read;
use std::sync::mpsc::{self, Receiver, Sender};
use windows_sys::Win32::UI::Controls::Dialogs::*;

type Parsed = Result<(Vec<crate::store::WaveTrace>, String), String>;

struct Job { id: u64, epoch: u64, slot: usize, path: PathBuf }
struct Worker { requests: Sender<Job>, results: Receiver<(Job, Parsed)> }

/// One reader/parser worker, with at most one pending choice after the active
/// file. Superseded/canceled results never replace a working view.
#[derive(Default)]
pub struct Loader {
    worker: Option<Worker>,
    next: u64,
    active: Option<u64>,
}

impl Loader {
    pub fn new() -> Self { Self::default() }
    pub fn busy(&self) -> bool { self.active.is_some() }
    pub fn cancel(&mut self) { self.active = None; }

    pub fn submit(&mut self, path: PathBuf, epoch: u64, slot: usize) -> Result<String, String> {
        if slot >= crate::model::NREF { return Err("Choose a reference slot from R1 to R10".into()); }
        if self.worker.is_none() {
            let (requests, input) = mpsc::channel::<Job>();
            let (output, results) = mpsc::channel();
            std::thread::Builder::new().name("oos-wave-load".into()).spawn(move || {
                while let Ok(mut job) = input.recv() {
                    while let Ok(newer) = input.try_recv() { job = newer; }
                    let parsed = read_waveform(&job.path);
                    if output.send((job, parsed)).is_err() { break; }
                }
            }).map_err(|e| format!("Could not start waveform reader: {e}"))?;
            self.worker = Some(Worker { requests, results });
        }
        let name = file_name(&path);
        self.next = self.next.wrapping_add(1);
        self.active = Some(self.next);
        if self.worker.as_ref().unwrap().requests.send(Job { id: self.next, epoch, slot, path }).is_err() {
            self.active = None;
            self.worker = None;
            return Err("Waveform reader stopped; choose the file again".into());
        }
        Ok(name)
    }

    pub fn poll(&mut self, epoch: u64) -> Option<(usize, Parsed)> {
        loop {
            let worker = self.worker.as_ref()?;
            match worker.results.try_recv() {
                Ok((job, result)) => {
                    if self.active != Some(job.id) { continue; }
                    self.active = None;
                    if job.epoch != epoch { continue; }
                    return Some((job.slot, result));
                }
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker = None;
                    return self.active.take().map(|_| (0, Err("Waveform reader stopped; choose the file again".into())));
                }
            }
        }
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "waveform".into())
}

/// Bounded reading also handles a file growing after metadata was checked.
fn read_waveform(path: &std::path::Path) -> Parsed {
    let name = file_name(path);
    let file = std::fs::File::open(path).map_err(|e| format!("Cannot read {name}: {e}"))?;
    let size = file.metadata().map_err(|e| format!("Cannot read {name}: {e}"))?.len();
    let maximum = crate::store::MAX_WAVEFORM_BYTES;
    if size > maximum as u64 { return Err(format!("{name} exceeds the {} MiB waveform limit", maximum >> 20)); }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(maximum as u64 + 1).read_to_end(&mut bytes).map_err(|e| format!("Cannot read {name}: {e}"))?;
    if bytes.len() > maximum { return Err(format!("{name} exceeds the {} MiB waveform limit", maximum >> 20)); }
    let traces = crate::store::parse_wave(&bytes, &name)?;
    Ok((traces, name))
}

pub fn waveform(owner: *mut std::ffi::c_void, slot: usize) -> Result<Option<PathBuf>, String> {
    let filter: Vec<u16> = "Waveforms (*.csv;*.bin;*.wfm;*.trc;*.isf;*.tr0;*.wfmx)\0*.csv;*.bin;*.wfm;*.trc;*.isf;*.tr0;*.wfmx\0All files (*.*)\0*.*\0\0".encode_utf16().collect();
    let title: Vec<u16> = format!("Open waveform into R{}\0", slot + 1).encode_utf16().collect();
    let mut name = vec![0u16; 32_768];
    let mut options = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: filter.as_ptr(),
        nFilterIndex: 1,
        lpstrFile: name.as_mut_ptr(),
        nMaxFile: name.len() as u32,
        lpstrTitle: title.as_ptr(),
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if unsafe { GetOpenFileNameW(&mut options) } == 0 {
        let error = unsafe { CommDlgExtendedError() };
        return if error == 0 { Ok(None) } else { Err(format!("The file dialog could not open (error {error:#x}).")) };
    }
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Ok(Some(std::ffi::OsString::from_wide(&name[..end]).into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn fixture() -> PathBuf {
        let directory = std::env::temp_dir().join(format!("oos-native-load-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("clock.csv");
        std::fs::write(&path, b"Time (s),Volts\n0,-1\n1e-9,1\n2e-9,-1\n").unwrap();
        path
    }

    #[test]
    fn background_reader_commits_only_the_latest_choice_in_the_same_workflow() {
        let path = fixture();
        let mut loader = Loader::new();
        loader.submit(path.clone(), 1, 0).unwrap();
        loader.submit(path.clone(), 2, 8).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        let (traces, name) = loop {
            if let Some((slot, result)) = loader.poll(2) { assert_eq!(slot, 8); break result.unwrap(); }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(name, "clock.csv");
        assert_eq!(traces[0].v, [-1.0, 1.0, -1.0]);
        assert!(!loader.busy());
        loader.submit(path.clone(), 2, 1).unwrap();
        while loader.busy() {
            assert!(loader.poll(3).is_none(), "a changed source workflow must reject the late load");
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(2));
        }
        loader.submit(path.clone(), 3, 2).unwrap();
        loader.cancel();
        assert!(!loader.busy());
        assert!(loader.poll(3).is_none());
        let _ = std::fs::remove_file(path);
    }

    /// Explicit local audit, never part of ordinary CI. The original captures
    /// are read only; pass paths separated by semicolons in OOS_NATIVE_WAVEFORMS.
    #[test]
    #[ignore = "requires explicitly supplied local waveform paths"]
    fn actual_native_waveform_files_parse_at_their_recorded_intervals() {
        let paths = std::env::var("OOS_NATIVE_WAVEFORMS").expect("set OOS_NATIVE_WAVEFORMS to semicolon-separated capture paths");
        for path in paths.split(';') {
            let start = Instant::now();
            let (traces, name) = read_waveform(std::path::Path::new(path)).unwrap();
            assert!(!traces.is_empty());
            for trace in traces {
                assert!(trace.v.len() >= 2 && trace.dt.is_finite() && trace.dt > 0.0 && trace.t0.is_finite());
                assert!(trace.v.iter().any(|value| value.is_finite()));
                eprintln!("{name}: {} points dt={}s t0={}s label={} read+parse={:?}", trace.v.len(), trace.dt, trace.t0, trace.label, start.elapsed());
            }
        }
    }
}
