// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What the acquisition thread hands the draw pipeline (drawpipe.rs, ADR 0059) with each batch, declared here so the
//! board names no signal code (ADR 0074): the read-out buffer (`RawBatch`), how a frame is decoded or where its
//! channels are (`DecodeInto`, `CodesInto`, `CodeLayout`), the frame counters, and the deposit workers as the thread
//! sees them (`Workers`). `drawpipe` implements `Workers` and registers itself when the hardware becomes the source
//! (`register`); before that, and with every other source, the thread finds no workers.

use crate::board::Frame;
use crate::model::NCH;
use std::any::Any;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

/// Where a frame's read-out holds each channel (`fastdraw::Codes`), as the board gives it (`CodesInto`).
#[derive(Clone, Copy)]
pub struct CodeLayout {
    pub stride: usize,
    pub word: [Option<usize>; NCH],
    pub affine: [(f32, f32); NCH],
    pub volt: fn(u16, (f32, f32)) -> f32,
    /// volts of samples a..b of one channel (bytes, stride, word, a, b, affine, out): the board's decode loop
    pub volts: VoltsFn,
}

/// The board's decode loop: (bytes, stride, word, a, b, affine, out).
pub type VoltsFn = fn(&[u8], usize, usize, usize, usize, (f32, f32), &mut Vec<f32>);

/// The board's view of a frame without decoding it: sets every field of the frame but the channel vectors (left
/// empty) as the decode would, and says where the channels are; None where only the decode will do (compressed or
/// with a header).
pub type CodesInto = Arc<dyn Fn(&[u8], &mut Frame) -> Option<CodeLayout> + Send + Sync>;

/// Frames acquired, used (drawn into the picture) and dropped, since the start.
#[derive(Default)]
pub struct Counters {
    pub acquired: AtomicU64,
    pub used: AtomicU64,
    pub dropped: AtomicU64,
    /// frames whose cheap measurements went into the statistics (framestats.rs)
    pub measured: AtomicU64,
}

/// The process's counters (the acquisition thread, the workers and the UI add to them).
pub fn counters() -> &'static Counters {
    static C: OnceLock<Counters> = OnceLock::new();
    C.get_or_init(Counters::default)
}

impl Counters {
    pub fn add(c: &AtomicU64, n: u64) {
        c.fetch_add(n, Ordering::Relaxed);
    }
    /// (acquired, used, dropped)
    pub fn read(&self) -> (u64, u64, u64) {
        (self.acquired.load(Ordering::Relaxed), self.used.load(Ordering::Relaxed), self.dropped.load(Ordering::Relaxed))
    }
}

/// A read-out buffer of the board (the acquisition thread's DMA target), lent to the workers.
pub trait RawBatch: Send + Sync {
    fn bytes(&self) -> &[u8];
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send>;
}

/// Decodes the frame bytes into a kept frame (the board's decoder with the batch's settings).
pub type DecodeInto = Arc<dyn Fn(&[u8], &mut Frame) + Send + Sync>;

/// One batch for the workers (`Workers::feed`): frames `first..frames` of `fb` bytes in `raw`, recorded under settings
/// stamp `stamp`, decoded with `decode` or drawn from their words with `codes`. `frames` is the end of that range, not
/// a count from `first`: the frames of the batch from frame 0 that the workers take (the acquisition thread passes all
/// but the newest, which it queues itself), and `first` how many of them at the start are left out.
pub struct Feed {
    pub raw: Box<dyn RawBatch>,
    pub fb: usize,
    pub first: usize,
    pub frames: usize,
    pub stamp: u64,
    pub decode: DecodeInto,
    pub codes: Option<CodesInto>,
}

/// The acquisition thread's share of the deposit work, kept by the thread between batches: the workers' own state,
/// made by them on the first `Workers::help`.
#[derive(Default)]
pub struct Helper(pub Option<Box<dyn Any + Send>>);

/// The deposit workers as the acquisition thread sees them (drawpipe.rs: `Pipe`).
pub trait Workers: Sync {
    /// They take batches: the UI published a view the fast path applies to.
    fn accepting(&self) -> bool;
    /// A buffer back from the workers, to read the next batch into (None: all are still out).
    fn take_buffer(&self) -> Option<Box<dyn Any + Send>>;
    /// A batch for the workers; its buffer comes back through `take_buffer` when it is drawn.
    fn feed(&self, feed: Feed);
    /// The thread draws a few frames while it waits for the next batch; false when there was nothing to draw.
    fn help(&self, h: &mut Helper) -> bool;
    /// The wait is over: the thread hands on what it drew.
    fn help_done(&self, h: &mut Helper);
}

/// The workers' getter, registered by the draw pipeline (`register`).
static WORKERS: OnceLock<fn() -> Option<&'static dyn Workers>> = OnceLock::new();

/// Registers the getter of the workers (drawpipe.rs, when the hardware becomes the source); the first one stays.
pub fn register(get: fn() -> Option<&'static dyn Workers>) {
    let _ = WORKERS.set(get);
}

/// The workers while the source is the hardware, else None.
pub fn workers() -> Option<&'static dyn Workers> {
    WORKERS.get().and_then(|get| get())
}
