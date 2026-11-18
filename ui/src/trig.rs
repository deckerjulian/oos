// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The software trigger of all sources: the trigger types of oos-dsp, and the protocol trigger (TrigType::Bus), which
//! decodes the record with the Decode menu's settings and triggers at the end of the matching field.

use crate::model::*;
use oos_dsp::decode::{Kind, Packet};
use oos_dsp::trigger::Input;

/// Packets of the decoder settings in `m` on the channels of `inp` (times = fractional sample indices).
pub fn decode(m: &Model, ch: [&[f32]; NCH], dt: f64) -> Result<Vec<Packet>, &'static str> {
    if m.dec_src >= NCH {
        return Err("stored decoder inputs need the reference adapter");
    }
    decode_at(m, ch, dt, m.dec_thr, 0.1 * m.ch[m.dec_src].volts_per_div())
}

/// Packets of logic analyser lines (`Model::dec_la`, the decoder's roles source / second / third / fourth line). The
/// lines become 0 / 1 waves in the channel slots 0..3 and the normal decoders run on them, threshold 0.5; the LA's
/// own comparators have already decided the levels. Times = fractional sample indices of the LA record.
pub fn decode_la(m: &Model, words: &[u16], dt: f64) -> Result<Vec<Packet>, &'static str> {
    let wave = |n: u8| -> Vec<f32> { words.iter().map(|w| (w >> n & 1) as f32).collect() };
    let (thr, hy) = (0.5, 0.1);
    let la = m.dec_la;
    if m.dec_proto == Proto::PARALLEL {
        // up to 16 data lines upwards from the main source, skipping the clock line
        let first = la[0].unwrap_or(0);
        let lines: Vec<Vec<f32>> = (0..16u8)
            .map(|k| (first + k) % 16)
            .filter(|&n| Some(n) != la[2])
            .take(m.dec_bits.clamp(1, 16) as usize)
            .map(wave)
            .collect();
        let refs: Vec<&[f32]> = lines.iter().map(|l| &l[..]).collect();
        let clk = la[2].map(wave);
        return Ok(oos_dsp::decode::parallel(&refs, clk.as_deref(), oos_dsp::trigger::Slope::Rising, thr, hy));
    }
    let slots: [Vec<f32>; NCH] = std::array::from_fn(|i| la.get(i).copied().flatten().map_or(Vec::new(), &wave));
    let mut s = m.clone();
    (s.dec_src, s.dec_src2, s.dec_cs, s.dec_src4, s.dec_diff) = (0, 1, la[2].map(|_| 2), la[3].map(|_| 3), None);
    decode_at(&s, std::array::from_fn(|c| &slots[c][..]), dt, thr, hy)
}

/// `decode` with the threshold and hysteresis given.
pub(crate) fn decode_at(m: &Model, ch: [&[f32]; NCH], dt: f64, thr: f32, hy: f32) -> Result<Vec<Packet>, &'static str> {
    use oos_dsp::decode as x1;
    let get = |c: usize| ch.get(c).copied().filter(|v| !v.is_empty()).ok_or("source channel off");
    let opt = |c: Option<usize>| -> Result<Option<&[f32]>, &'static str> { c.map(get).transpose() };
    // RS-485 / differential: A - B, threshold around 0 V like the pair's receiver
    let diff;
    let line: &[f32] = match (m.dec_proto.diff_ok(), m.dec_diff) {
        (true, Some(b)) => {
            diff = x1::diff(get(m.dec_src)?, get(b)?);
            &diff
        }
        _ => get(m.dec_src)?,
    };
    // the protocol module: the lines of its roles; a role it names must have a source (the third and fourth may be
    // "none")
    let Proto::Module(i) = m.dec_proto;
    let p = crate::protocols::get(i);
    let r = p.roles();
    let lines = [
        Some(line),
        if r[1].is_some() { Some(get(m.dec_src2)?) } else { None },
        if r[2].is_some() { opt(m.dec_cs)? } else { None },
        if r[3].is_some() { opt(m.dec_src4)? } else { None },
    ];
    let chans: [Option<&[f32]>; 4] = std::array::from_fn(|c| ch.get(c).copied().filter(|v| !v.is_empty()));
    let src = [Some(m.dec_src), Some(m.dec_src2), m.dec_cs, m.dec_src4];
    p.decode(&crate::protocols::Lines {
        lines,
        dt,
        thr,
        hy,
        opt: m.dec_opts(),
        sh: crate::protocols::Shared::of(m),
        chans,
        src,
    })
}

/// Whether a packet satisfies the protocol trigger condition of `m` on its own. UART and SPI data take the relation,
/// limit and mask of `m.ser`; I2C addresses and data need the frame around them (`points`).
pub fn matches(m: &Model, p: &Packet) -> bool {
    let v = m.trig_bus_val;
    if let Some(r) = m.dec_proto.module().and_then(|x| x.trigger_match(m.trig_bus, v as u32, p)) {
        return r;
    }
    let v32 = v as u32;
    if matches!(m.dec_proto, Proto::UART | Proto::SPI) && m.trig_bus == BusWhat::Data {
        return p.kind == Kind::Data && m.ser.rel.test(p.value as u64, v, m.ser.max, m.ser.mask);
    }
    if m.dec_proto == Proto::I2C && matches!(m.trig_bus, BusWhat::Id | BusWhat::Data) {
        return false;
    }
    let v = v32;
    match m.trig_bus {
        BusWhat::Frame => match p.kind {
            Kind::Start | Kind::Break | Kind::Word { cmd: true } => true,
            Kind::Data => matches!(m.dec_proto, Proto::UART | Proto::SPI | Proto::PARALLEL),
            Kind::Sample { right: false } => true,
            // the first field of a frame of the extended decoders
            Kind::Field(l) => matches!(l, "!RESET" | "PRE" | "TSS" | "CMD" | "REQ" | "BLK" | "IR" | "DR" | "SYNC"),
            _ => false,
        },
        // USB: the token address or the PID byte
        BusWhat::Id => {
            (matches!(p.kind, Kind::Id { .. } | Kind::Addr { .. } | Kind::Word { cmd: true })
                || p.kind == Kind::Field("ROM")
                || p.kind == Kind::Field("CMD")
                || p.kind == Kind::Field("HDR")
                || p.kind == Kind::Field("STAT"))
                && p.value == v
        }
        BusWhat::Data => matches!(p.kind, Kind::Data | Kind::Sample { .. } | Kind::Word { cmd: false }) && p.value == v,
        BusWhat::Error => !p.ok,
    }
}

/// The trigger points (packet ends) of `m`'s protocol condition among `packets`: `matches` per packet, and for I2C
/// the address (relation, direction, 10-bit) and the first `ser.bytes` data bytes after it, big-endian, as the TPU's
/// receiver compares them.
pub fn points(m: &Model, packets: &[Packet]) -> Vec<f64> {
    if m.dec_proto != Proto::I2C || !matches!(m.trig_bus, BusWhat::Id | BusWhat::Data) {
        return packets.iter().filter(|p| matches(m, p)).map(|p| p.t1).collect();
    }
    let s = &m.ser;
    let mut out = Vec::new();
    // per frame: the address (10-bit: its high bits until the second byte), then the data bytes so far
    let (mut addr, mut hi10, mut read, mut n, mut acc) = (None::<u32>, None::<u32>, false, 0u8, 0u64);
    for p in packets {
        match p.kind {
            Kind::Start | Kind::RepStart | Kind::Stop => (addr, hi10, n, acc) = (None, None, 0, 0),
            Kind::Addr { read: r } => {
                read = r;
                (n, acc) = (0, 0);
                if s.ten_bit && p.value & 0x7c == 0x78 {
                    (addr, hi10) = (None, Some(p.value & 3));
                } else {
                    (addr, hi10) = (Some(p.value), None);
                    if m.trig_bus == BusWhat::Id && addr_ok(m, p.value, read) {
                        out.push(p.t1);
                    }
                }
            }
            Kind::Data => {
                if let Some(h) = hi10.take() {
                    let a = h << 8 | (p.value & 0xff);
                    addr = Some(a);
                    if m.trig_bus == BusWhat::Id && addr_ok(m, a, read) {
                        out.push(p.t1);
                    }
                    continue;
                }
                if m.trig_bus != BusWhat::Data || addr.is_none() {
                    continue;
                }
                // any data byte of the frame
                if s.bytes == 0 {
                    if s.rel.test((p.value & 0xff) as u64, m.trig_bus_val, s.max, s.mask & 0xff) {
                        out.push(p.t1);
                    }
                    continue;
                }
                if n >= s.bytes.min(5) {
                    continue;
                }
                acc = acc << 8 | (p.value & 0xff) as u64;
                n += 1;
                let mask = s.mask & ((1u64 << (8 * n as u32)) - 1);
                if n == s.bytes.min(5) && s.rel.test(acc, m.trig_bus_val, s.max, mask) {
                    out.push(p.t1);
                }
            }
            _ => {}
        }
    }
    out
}

/// The I2C address condition: relation against the value and `addr_max`, and the direction.
fn addr_ok(m: &Model, a: u32, read: bool) -> bool {
    let s = &m.ser;
    let dir = match s.dir {
        I2cDir::Either => true,
        I2cDir::Read => read,
        I2cDir::Write => !read,
    };
    dir && s.addr_rel.test(a as u64, m.trig_bus_val, s.addr_max as u64, 0x3ff)
}

/// All trigger points of `m`'s condition in `inp` (fractional sample indices).
pub fn find_all(m: &Model, inp: &Input) -> Vec<f64> {
    if m.trig_type == TrigType::Bus {
        return points(m, &decode(m, inp.ch, inp.dt).unwrap_or_default());
    }
    oos_dsp::trigger::find_all(&m.trigger(), inp)
}

/// The first trigger point after `from`.
pub fn find_first(m: &Model, inp: &Input, from: f64) -> Option<f64> {
    find_all(m, inp).into_iter().find(|&t| t > from)
}

#[cfg(test)]
mod serial_tests {
    use super::*;

    fn pk(t: f64, kind: Kind, value: u32) -> Packet {
        Packet { t0: t, t1: t, kind, value, value2: None, ok: true }
    }

    #[test]
    fn relations_mask_and_limits() {
        // (relation, x, value, max, mask) -> match
        let cases = [
            (Rel::Eq, 0x41, 0x41, 0, u64::MAX, true),
            (Rel::Ne, 0x41, 0x41, 0, u64::MAX, false),
            (Rel::Gt, 0x42, 0x41, 0, u64::MAX, true),
            (Rel::Lt, 0x41, 0, 0x41, u64::MAX, false),
            (Rel::In, 0x35, 0x30, 0x39, u64::MAX, true),
            (Rel::Out, 0x35, 0x30, 0x39, u64::MAX, false),
            (Rel::Out, 0x40, 0x30, 0x39, u64::MAX, true),
            // the mask applies to x and both limits
            (Rel::Eq, 0x4f, 0x40, 0, 0xf0, true),
        ];
        for (r, x, v, max, mask, want) in cases {
            assert_eq!(r.test(x, v, max, mask), want, "{:?} {:#x}", r, x);
        }
    }

    #[test]
    fn uart_and_spi_data_take_the_relation() {
        let mut m = Model { dec_proto: Proto::UART, trig_bus: BusWhat::Data, trig_bus_val: 0x30, ..Model::default() };
        m.ser.rel = Rel::In;
        m.ser.max = 0x39;
        let p = [pk(1.0, Kind::Data, 0x2f), pk(2.0, Kind::Data, 0x35), pk(3.0, Kind::Data, 0x3a)];
        assert_eq!(points(&m, &p), [2.0]);
        m.dec_proto = Proto::SPI;
        m.ser.rel = Rel::Ne;
        assert_eq!(points(&m, &p), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn i2c_address_direction_ten_bit_and_data_bytes() {
        use Kind::*;
        let mut m = Model { dec_proto: Proto::I2C, trig_bus: BusWhat::Id, trig_bus_val: 0x50, ..Model::default() };
        // write to 0x50: 0x12 0x34 0x56; read from 0x50: 0xab; 10-bit write to 0x2a5: 0x01
        let p = [
            pk(0.0, Start, 0),
            pk(1.0, Addr { read: false }, 0x50),
            pk(2.0, Data, 0x12),
            pk(3.0, Data, 0x34),
            pk(4.0, Data, 0x56),
            pk(5.0, RepStart, 0),
            pk(6.0, Addr { read: true }, 0x50),
            pk(7.0, Data, 0xab),
            pk(8.0, Stop, 0),
            pk(9.0, Start, 0),
            pk(10.0, Addr { read: false }, 0x7a),
            pk(11.0, Data, 0xa5),
            pk(12.0, Data, 0x01),
            pk(13.0, Stop, 0),
        ];
        assert_eq!(points(&m, &p), [1.0, 6.0]);
        m.ser.dir = I2cDir::Read;
        assert_eq!(points(&m, &p), [6.0]);
        // 10-bit: the address completes with the second byte
        (m.ser.dir, m.ser.ten_bit, m.trig_bus_val) = (I2cDir::Either, true, 0x2a5);
        assert_eq!(points(&m, &p), [11.0]);
        // data: any byte, then the first two bytes after the address, big-endian
        (m.ser.ten_bit, m.trig_bus, m.trig_bus_val) = (false, BusWhat::Data, 0x34);
        assert_eq!(points(&m, &p), [3.0]);
        (m.ser.bytes, m.trig_bus_val) = (2, 0x1234);
        assert_eq!(points(&m, &p), [3.0]);
        (m.ser.bytes, m.trig_bus_val) = (3, 0x12_3456);
        assert_eq!(points(&m, &p), [4.0]);
        // a mask over the second byte only, and a range over the first byte
        (m.ser.bytes, m.trig_bus_val, m.ser.mask) = (2, 0x0034, 0x00ff);
        assert_eq!(points(&m, &p), [3.0]);
        (m.ser.bytes, m.ser.mask, m.ser.rel, m.trig_bus_val, m.ser.max) = (1, u64::MAX, Rel::In, 0x10, 0xaf);
        // the first byte of each frame: 0x12, 0xab and 0xa5 (the 7-bit reading of the third frame's address)
        assert_eq!(points(&m, &p), [2.0, 7.0, 11.0]);
    }
}
