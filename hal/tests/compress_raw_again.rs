// SPDX-License-Identifier: GPL-2.0-or-later

//! The live compression's second read (fork, docs/fork/compress.md): a batch read with FPGA compression, then the
//! newest frame of the same batch read again raw (`seq::read_raw_again`), against the host model of the FPGA.
//! Break it catches: the raw read leaving the compression on, reading another frame than asked, or a wrong length.

use oos_hal::dho::bus::DmaBuf;
use oos_hal::dho::pairs;
use oos_hal::dho::seg::sim::{engine, Engine};
use oos_hal::dho::seq::{acquire_batch, compressed_len, frame_bytes, raw_settings, read_raw_again, Settings, State};

fn words(b: &[u8]) -> Vec<u16> {
    b.chunks_exact(2).map(|x| u16::from_le_bytes([x[0], x[1]])).collect()
}

#[test]
fn compressed_batch_then_the_newest_frame_raw() {
    let k = 4u32;
    let s = Settings { tdiv: 10e-6, frames: 10, compress: k, window: None, ..Settings::default() };
    let mut st = State::handover(1, None);
    let p = st.configure(&s, None, None).unwrap().1;
    assert_eq!(p.ratio, 1);
    // every frame recorded at the first look at the done flag
    let (eng, mut bar, mut c2h) = engine(Engine::new(0, 1_000_000_000, 10, p.ratio));
    let mut dma = DmaBuf::new();
    let n = acquire_batch(&mut st, &s, &p, &mut bar, &mut c2h, &mut dma, false, 25, |_| {}).unwrap();
    assert_eq!(n, 10);
    let fb = frame_bytes(&s, &p);
    assert_eq!(fb, 2 * compressed_len(&s, &p) as usize);
    assert_eq!(dma.data().len(), n as usize * fb);
    // the newest compressed frame: min/max pairs of its samples
    let len = compressed_len(&s, &p) as usize;
    let raw_newest: Vec<u16> = (0..len * k as usize).map(|i| Engine::word(n - 1, i)).collect();
    let comp = words(&dma.data()[(n as usize - 1) * fb..]);
    assert_eq!(comp, pairs::model(&raw_newest, k as usize, pairs::LAYOUT)[..len]);

    let mut raw = DmaBuf::new();
    read_raw_again(&mut st, &s, &p, &mut bar, &mut c2h, &mut raw, n - 1, |_| {}).unwrap();
    let rs = raw_settings(&s);
    assert_eq!(raw.data().len(), frame_bytes(&rs, &p));
    assert_eq!(frame_bytes(&rs, &p), 2 * p.memsel as usize);
    let got = words(raw.data());
    assert!(got.iter().enumerate().all(|(i, &w)| w == Engine::word(n - 1, i)), "not frame {} raw", n - 1);
    // compression off again, and the compressed read below it still matches the raw samples
    assert_eq!(eng.borrow().regs.get(&0x1170).copied(), Some(0x4000_0001));
    // every compressed point stands for samples inside the record (`compressed_len`): all of them match
    assert!(len * k as usize <= p.memsel as usize);
    assert_eq!(comp, pairs::model(&got, k as usize, pairs::LAYOUT)[..len]);
}
