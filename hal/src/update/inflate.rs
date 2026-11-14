// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! gzip (RFC 1952) and deflate (RFC 1951) unpacking, for reading a file out of an image's initramfs: the build of the
//! image on the card (`image_build`), which the install from a USB stick compares with its own.

struct Bits<'a> {
    b: &'a [u8],
    at: usize,
    bit: u32,
    n: u32,
}

impl<'a> Bits<'a> {
    fn need(&mut self, k: u32) -> Result<(), String> {
        while self.n < k {
            let byte = *self.b.get(self.at).ok_or("deflate: data cut short")?;
            self.at += 1;
            self.bit |= (byte as u32) << self.n;
            self.n += 8;
        }
        Ok(())
    }

    fn get(&mut self, k: u32) -> Result<u32, String> {
        if k == 0 {
            return Ok(0);
        }
        self.need(k)?;
        let v = self.bit & ((1u32 << k) - 1);
        self.bit >>= k;
        self.n -= k;
        Ok(v)
    }

    fn align(&mut self) {
        self.bit = 0;
        self.n = 0;
    }
}

/// A canonical Huffman code: the count of codes per length, the symbols in code order.
struct Huff {
    count: [u16; 16],
    sym: Vec<u16>,
}

impl Huff {
    fn new(lens: &[u8]) -> Result<Huff, String> {
        let mut count = [0u16; 16];
        for &l in lens {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut offs = [0u16; 16];
        for i in 1..16 {
            offs[i] = offs[i - 1] + count[i - 1];
        }
        let mut sym = vec![0u16; lens.len()];
        for (s, &l) in lens.iter().enumerate() {
            if l != 0 {
                sym[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        Ok(Huff { count, sym })
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.get(1)? as i32;
            let count = self.count[len] as i32;
            if code - count < first {
                return Ok(self.sym[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("deflate: bad code".into())
    }
}

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

fn codes(bits: &mut Bits, out: &mut Vec<u8>, lit: &Huff, dist: &Huff, limit: usize) -> Result<(), String> {
    loop {
        let s = lit.decode(bits)? as usize;
        if s < 256 {
            out.push(s as u8);
        } else if s == 256 {
            return Ok(());
        } else {
            let s = s - 257;
            if s >= 29 {
                return Err("deflate: bad length".into());
            }
            let len = LBASE[s] as usize + bits.get(LEXT[s] as u32)? as usize;
            let d = dist.decode(bits)? as usize;
            if d >= 30 {
                return Err("deflate: bad distance".into());
            }
            let back = DBASE[d] as usize + bits.get(DEXT[d] as u32)? as usize;
            if back > out.len() {
                return Err("deflate: distance too far".into());
            }
            let from = out.len() - back;
            for i in 0..len {
                let c = out[from + i];
                out.push(c);
            }
        }
        if out.len() > limit {
            return Err("deflate: more data than allowed".into());
        }
    }
}

/// Raw deflate data, at most `limit` bytes out. Also the number of input bytes it took.
pub fn inflate(b: &[u8], limit: usize) -> Result<(Vec<u8>, usize), String> {
    let mut bits = Bits { b, at: 0, bit: 0, n: 0 };
    let mut out = Vec::new();
    loop {
        let last = bits.get(1)?;
        match bits.get(2)? {
            0 => {
                bits.align();
                let at = bits.at;
                let h = b.get(at..at + 4).ok_or("deflate: stored block cut short")?;
                let (len, nlen) = (u16::from_le_bytes([h[0], h[1]]), u16::from_le_bytes([h[2], h[3]]));
                if len != !nlen {
                    return Err("deflate: stored block length".into());
                }
                let d = b.get(at + 4..at + 4 + len as usize).ok_or("deflate: stored block cut short")?;
                out.extend_from_slice(d);
                bits.at = at + 4 + len as usize;
            }
            1 => {
                let mut l = [0u8; 288];
                l[..144].fill(8);
                l[144..256].fill(9);
                l[256..280].fill(7);
                l[280..].fill(8);
                codes(&mut bits, &mut out, &Huff::new(&l)?, &Huff::new(&[5u8; 30])?, limit)?;
            }
            2 => {
                let (nlen, ndist, ncode) = (bits.get(5)? as usize + 257, bits.get(5)? as usize + 1, bits.get(4)? as usize + 4);
                const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                let mut cl = [0u8; 19];
                for &o in ORDER.iter().take(ncode) {
                    cl[o] = bits.get(3)? as u8;
                }
                let ch = Huff::new(&cl)?;
                let mut lens = vec![0u8; nlen + ndist];
                let mut i = 0;
                while i < nlen + ndist {
                    let s = ch.decode(&mut bits)?;
                    let (v, rep) = match s {
                        0..=15 => (s as u8, 1),
                        16 => (*lens.get(i.wrapping_sub(1)).ok_or("deflate: repeat of nothing")?, 3 + bits.get(2)? as usize),
                        17 => (0, 3 + bits.get(3)? as usize),
                        _ => (0, 11 + bits.get(7)? as usize),
                    };
                    if i + rep > lens.len() {
                        return Err("deflate: too many lengths".into());
                    }
                    lens[i..i + rep].fill(v);
                    i += rep;
                }
                codes(&mut bits, &mut out, &Huff::new(&lens[..nlen])?, &Huff::new(&lens[nlen..])?, limit)?;
            }
            _ => return Err("deflate: bad block type".into()),
        }
        if out.len() > limit {
            return Err("deflate: more data than allowed".into());
        }
        if last == 1 {
            return Ok((out, bits.at));
        }
    }
}

/// A gzip file (one member), at most `limit` bytes out.
pub fn gunzip(b: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    if b.len() < 18 || b[0] != 0x1f || b[1] != 0x8b || b[2] != 8 {
        return Err("not gzip".into());
    }
    let flags = b[3];
    let mut at = 10;
    if flags & 4 != 0 {
        let x = u16::from_le_bytes([b[at], b[at + 1]]) as usize;
        at += 2 + x;
    }
    for f in [8u8, 16] {
        if flags & f != 0 {
            at += b.get(at..).ok_or("gzip cut short")?.iter().position(|&c| c == 0).ok_or("gzip cut short")? + 1;
        }
    }
    if flags & 2 != 0 {
        at += 2;
    }
    let (out, _) = inflate(b.get(at..).ok_or("gzip cut short")?, limit)?;
    Ok(out)
}

/// A file of a newc cpio archive (with or without a leading "./").
pub fn cpio_file<'a>(mut b: &'a [u8], name: &str) -> Option<&'a [u8]> {
    while b.len() >= 110 && &b[..6] == b"070701" {
        let f = |i: usize| std::str::from_utf8(&b[6 + i * 8..14 + i * 8]).ok().and_then(|s| u32::from_str_radix(s, 16).ok());
        let (size, nlen) = (f(6)? as usize, f(11)? as usize);
        let n = b.get(110..110 + nlen.saturating_sub(1))?;
        let at = (110 + nlen).div_ceil(4) * 4;
        let data = b.get(at..at + size)?;
        if n == b"TRAILER!!!" {
            return None;
        }
        if n == name.as_bytes() || n.strip_prefix(b"./") == Some(name.as_bytes()) {
            return Some(data);
        }
        b = b.get((at + size).div_ceil(4) * 4..)?;
    }
    None
}

/// The build of an OOS image (its initramfs' /etc/oos-build, as `pack --build` names it), when it can be read.
pub fn image_build(img: &[u8]) -> Option<String> {
    let (_, ramdisk, _) = super::kexec::parts(img).ok()?;
    let rd = gunzip(ramdisk, 512 << 20).ok()?;
    let b = cpio_file(&rd, "etc/oos-build")?;
    Some(String::from_utf8_lossy(b).trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// gzip with stored blocks (what a test can make without a compressor).
    pub fn gzip_stored(data: &[u8]) -> Vec<u8> {
        let mut b = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3];
        let chunks: Vec<&[u8]> = if data.is_empty() { vec![&[][..]] } else { data.chunks(65535).collect() };
        for (i, c) in chunks.iter().enumerate() {
            b.push((i + 1 == chunks.len()) as u8);
            b.extend_from_slice(&(c.len() as u16).to_le_bytes());
            b.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
            b.extend_from_slice(c);
        }
        b.extend_from_slice(&[0; 8]);
        b
    }

    #[test]
    fn stored_blocks() {
        let d: Vec<u8> = (0..200_000u32).map(|i| (i * 7) as u8).collect();
        assert_eq!(gunzip(&gzip_stored(&d), 1 << 20).unwrap(), d);
        assert!(gunzip(&gzip_stored(&d), 1000).is_err());
    }

    #[test]
    fn fixed_and_dynamic_codes() {
        // Python's gzip.compress(..., 9, mtime=0): a short text (fixed codes) and a longer one (dynamic codes)
        let fixed = [31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 203, 72, 205, 201, 201, 87, 200, 64, 39, 185, 0, 0, 136, 89, 11, 24, 0, 0, 0];
        assert_eq!(gunzip(&fixed, 1 << 20).unwrap(), b"hello hello hello hello\n");
        let dynamic = [0x1f,0x8b,0x08,0x00,0x00,0x00,0x00,0x00,0x02,0xff,0x95,0xd5,0x39,0x0e,0xc2,0x50,0x0c,0x04,0xd0,0x9e,0x53,0xf8,0x08,0x78,0x63,0x3b,0x0e,0x28,0x88,0x40,0x48,0xd8,0xa2,0x70,0x7c,0x84,0x68,0x52,0x20,0xdb,0x53,0x7b,0x7e,0x31,0x7a,0xf2,0x77,0xd7,0xf6,0x0d,0x2d,0x77,0xf4,0x3a,0x35,0x74,0x1f,0xdb,0xc3,0x85,0xf6,0x8f,0x61,0xea,0xe9,0x38,0xbc,0x69,0xd1,0x7d,0x87,0xfc,0x7f,0x78,0x1e,0xaf,0xb7,0xe7,0x2f,0x21,0x41,0x62,0x16,0xd3,0x2c,0x36,0xcb,0x5a,0x29,0x3b,0x7b,0xe0,0x51,0x85,0x55,0x5a,0x61,0x5d,0xab,0xb0,0x01,0x2a,0x6c,0xd1,0x0a,0x1c,0x33,0xe4,0x0e,0x5c,0x84,0x60,0x44,0x82,0x61,0x0a,0x0e,0x2d,0x38,0xc7,0xe0,0xa2,0x06,0x23,0x1c,0x0c,0x7b,0x48,0xe8,0x21,0x85,0xbd,0x28,0x7a,0x08,0xe2,0x21,0xb0,0x87,0x84,0x1e,0x92,0x7b,0x48,0xd1,0x43,0x10,0x0f,0x81,0x3d,0x34,0xf4,0xd0,0xdc,0x43,0xab,0x1f,0x15,0xe2,0xa1,0xb0,0x87,0x86,0x1e,0x9a,0x7b,0x68,0xd1,0x43,0x11,0x0f,0x85,0x3d,0x2c,0xf4,0xb0,0xdc,0xc3,0x8a,0x1e,0x06,0x5d,0x0e,0xd8,0xc3,0x42,0x0f,0xcb,0x3d,0xac,0xe8,0x61,0x88,0x87,0xc1,0x1e,0x1e,0x7a,0x78,0xee,0xe1,0x45,0x0f,0x47,0x3c,0x1c,0x3f,0xe5,0xa1,0x87,0xe7,0x1e,0x5e,0xf4,0x70,0xc4,0xc3,0x11,0x8f,0x0f,0xea,0x0f,0xcf,0x53,0x56,0x09,0x00,0x00];
        let text: String = (0..60).map(|i| format!("line {}: the quick brown fox {}\n", i, "jumps".repeat(i % 5))).collect();
        assert_eq!(gunzip(&dynamic, 1 << 20).unwrap(), text.as_bytes());
        assert!(gunzip(&dynamic, 100).is_err(), "the limit holds");
    }

    #[test]
    fn a_file_of_an_archive() {
        let a = crate::update::kexec::newc(&[("etc/oos-build", 0o100644, b"20261112-121847 kernel 6.12.115+\n"), ("bin/x", 0o100755, b"")]);
        assert_eq!(cpio_file(&a, "etc/oos-build"), Some(&b"20261112-121847 kernel 6.12.115+\n"[..]));
        assert_eq!(cpio_file(&a, "etc/other"), None);
    }

    /// An image's build: OOS_IMG set to a boot image (boot/out/oos-dev.img).
    #[test]
    #[ignore]
    fn the_build_of_an_image() {
        let img = std::fs::read(std::env::var("OOS_IMG").unwrap()).unwrap();
        let t = std::time::Instant::now();
        let b = image_build(&img).expect("a build");
        println!("{} ({} ms)", b, t.elapsed().as_millis());
    }
}
