// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! SHA-1 (only for the image id the stock loader checks), SHA-256 and SHA-512 (FIPS 180-4), streaming, no dependencies. The round constants and initial values were generated
//! from the prime cube / square roots (not typed in) and the tests check the FIPS / NIST example digests.

use std::ops::{BitAnd, BitXor, Not, Shr};

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
    0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
    0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
    0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];
const H256: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
const K512: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];
const H512: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

/// The word of one SHA-2 function and what differs with it (FIPS 180-4, 4.1.2 / 4.1.3, 4.2.2 / 4.2.3, 5.3.3 /
/// 5.3.5): the round constants, the initial hash value and the rotations; the rest of the algorithm is the same.
/// Public only because `Sha2` names it; `u32` and `u64` are the two words.
pub trait Word:
    'static + Copy + Default + BitAnd<Output = Self> + BitXor<Output = Self> + Not<Output = Self> + Shr<u32, Output = Self>
{
    /// One per round (64 or 80).
    const K: &'static [Self];
    const H: [Self; 8];
    /// Σ0 of a, Σ1 of e (three rotations each), σ0 and σ1 of the schedule (two rotations and a shift each).
    const ROT: [[u32; 3]; 4];
    fn rotr(self, n: u32) -> Self;
    fn add(self, o: Self) -> Self;
    /// From big-endian bytes (exactly the word's size).
    fn from_be(b: &[u8]) -> Self;
    /// To big-endian bytes (exactly the word's size).
    fn put_be(self, out: &mut [u8]);
}

macro_rules! word {
    ($t:ty, $k:expr, $h:expr, $rot:expr) => {
        impl Word for $t {
            const K: &'static [Self] = &$k;
            const H: [Self; 8] = $h;
            const ROT: [[u32; 3]; 4] = $rot;
            fn rotr(self, n: u32) -> Self {
                self.rotate_right(n)
            }
            fn add(self, o: Self) -> Self {
                self.wrapping_add(o)
            }
            fn from_be(b: &[u8]) -> Self {
                let mut x = [0u8; size_of::<$t>()];
                x.copy_from_slice(b);
                <$t>::from_be_bytes(x)
            }
            fn put_be(self, out: &mut [u8]) {
                out.copy_from_slice(&self.to_be_bytes());
            }
        }
    };
}
word!(u32, K256, H256, [[2, 13, 22], [6, 11, 25], [7, 18, 3], [17, 19, 10]]);
word!(u64, K512, H512, [[28, 34, 39], [14, 18, 41], [1, 8, 7], [19, 61, 6]]);

/// SHA-2 state over the word `W`, with blocks of `B` bytes (16 words) and a digest of `O` bytes (8 words).
#[derive(Clone)]
pub struct Sha2<W, const B: usize, const O: usize> {
    h: [W; 8],
    buf: [u8; B],
    n: usize,
    len: u64,
}

/// SHA-256 state.
pub type Sha256 = Sha2<u32, 64, 32>;
/// SHA-512 state.
pub type Sha512 = Sha2<u64, 128, 64>;

impl<W: Word, const B: usize, const O: usize> Default for Sha2<W, B, O> {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: Word, const B: usize, const O: usize> Sha2<W, B, O> {
    /// Bytes of the word.
    const S: usize = size_of::<W>();

    pub fn new() -> Self {
        const { assert!(B == 16 * Self::S && O == 8 * Self::S) };
        Sha2 { h: W::H, buf: [0; B], n: 0, len: 0 }
    }

    fn block(h: &mut [W; 8], b: &[u8]) {
        let [r0, r1, s0r, s1r] = W::ROT;
        let mut w = [W::default(); 80];
        for (i, x) in w[..16].iter_mut().enumerate() {
            *x = W::from_be(&b[i * Self::S..][..Self::S]);
        }
        for i in 16..W::K.len() {
            let s0 = w[i - 15].rotr(s0r[0]) ^ w[i - 15].rotr(s0r[1]) ^ (w[i - 15] >> s0r[2]);
            let s1 = w[i - 2].rotr(s1r[0]) ^ w[i - 2].rotr(s1r[1]) ^ (w[i - 2] >> s1r[2]);
            w[i] = w[i - 16].add(s0).add(w[i - 7]).add(s1);
        }
        let [mut a, mut b_, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
        for (k, wi) in W::K.iter().zip(&w) {
            let s1 = e.rotr(r1[0]) ^ e.rotr(r1[1]) ^ e.rotr(r1[2]);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.add(s1).add(ch).add(*k).add(*wi);
            let s0 = a.rotr(r0[0]) ^ a.rotr(r0[1]) ^ a.rotr(r0[2]);
            let maj = (a & b_) ^ (a & c) ^ (b_ & c);
            let t2 = s0.add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.add(t1);
            d = c;
            c = b_;
            b_ = a;
            a = t1.add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b_, c, d, e, f, g, hh]) {
            *x = x.add(y);
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        if self.n > 0 {
            let k = (B - self.n).min(data.len());
            self.buf[self.n..self.n + k].copy_from_slice(&data[..k]);
            self.n += k;
            data = &data[k..];
            if self.n < B {
                return;
            }
            let b = self.buf;
            Self::block(&mut self.h, &b);
            self.n = 0;
        }
        let (blocks, r) = data.as_chunks::<B>();
        for c in blocks {
            Self::block(&mut self.h, c);
        }
        self.buf[..r.len()].copy_from_slice(r);
        self.n = r.len();
    }

    pub fn finish(mut self) -> [u8; O] {
        // the length field is two words (8 or 16 bytes); messages here are far below 2^61 bytes, so for SHA-512 the
        // upper 64 bits are zero, and SHA-256 keeps the lower 64 bits of the bit count (FIPS 180-4, 5.1)
        let lb = 2 * Self::S;
        let bits = (self.len as u128) * 8;
        let mut pad = [0u8; 256];
        pad[0] = 0x80;
        let k = if self.n < B - lb { B - lb - self.n } else { 2 * B - lb - self.n };
        pad[k..k + lb].copy_from_slice(&bits.to_be_bytes()[16 - lb..]);
        let len = self.len;
        self.update(&pad[..k + lb]);
        self.len = len;
        debug_assert_eq!(self.n, 0);
        let mut out = [0u8; O];
        for (i, v) in self.h.iter().enumerate() {
            v.put_be(&mut out[i * Self::S..][..Self::S]);
        }
        out
    }
}

/// SHA-256 of one buffer.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut s = Sha256::new();
    s.update(data);
    s.finish()
}

/// SHA-512 of one buffer (the tests check the streaming hasher against it).
#[cfg(test)]
fn sha512(data: &[u8]) -> [u8; 64] {
    let mut s = Sha512::new();
    s.update(data);
    s.finish()
}

/// SHA-1 of the given parts in order (the stock loader's boot image id; not used for any security decision).
pub fn sha1(parts: &[&[u8]]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];
    let total: u64 = parts.iter().map(|p| p.len() as u64).sum();
    let mut tail = [0u8; 64 + 72];
    let mut buf = [0u8; 64];
    let mut n = 0usize;
    let block = |h: &mut [u32; 5], b: &[u8]| {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b_, mut c, mut d, mut e] = *h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i / 20 {
                0 => ((b_ & c) | (!b_ & d), 0x5a827999),
                1 => (b_ ^ c ^ d, 0x6ed9eba1),
                2 => ((b_ & c) | (b_ & d) | (c & d), 0x8f1bbcdc),
                _ => (b_ ^ c ^ d, 0xca62c1d6u32),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b_.rotate_left(30);
            b_ = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b_, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    };
    for p in parts {
        for &x in p.iter() {
            buf[n] = x;
            n += 1;
            if n == 64 {
                block(&mut h, &buf);
                n = 0;
            }
        }
    }
    tail[..n].copy_from_slice(&buf[..n]);
    tail[n] = 0x80;
    let len = if n < 56 { 64 } else { 128 };
    tail[len - 8..len].copy_from_slice(&(total * 8).to_be_bytes());
    for c in tail[..len].chunks(64) {
        block(&mut h, c);
    }
    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// Lower-case hex of a byte string.
pub fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(2 * b.len());
    for x in b {
        s.push_str(&format!("{:02x}", x));
    }
    s
}

/// Bytes from hex (even length, only the digits 0-9, a-f and A-F); None on anything else.
pub fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    // `from_str_radix` takes a sign too: "+f" would be 0x0f
    if !s.len().is_multiple_of(2) || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The NIST example messages (FIPS 180-4 examples; the 448- and 896-bit ones and a million 'a').
    const M448: &[u8] = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    const M896: &[u8] = concat!(
        "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmn",
        "hijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
    )
    .as_bytes();

    #[test]
    fn fips_examples() {
        let million = vec![b'a'; 1_000_000];
        let cases: [(&[u8], &str, &str); 5] = [
            (
                b"",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce\
                 47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e",
            ),
            (
                b"abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
                 2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
            ),
            (
                M448,
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
                "204a8fc6dda82f0a0ced7beb8e08a41657c16ef468b228a8279be331a703c335\
                 96fd15c13b1b07f9aa1d3bea57789ca031ad85c7a71dd70354ec631238ca3445",
            ),
            (
                M896,
                "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
                "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018\
                 501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909",
            ),
            (
                &million,
                "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
                "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973eb\
                 de0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b",
            ),
        ];
        for (m, d256, d512) in cases {
            assert_eq!(hex(&sha256(m)), d256, "sha256 of {} bytes", m.len());
            assert_eq!(hex(&sha512(m)), d512, "sha512 of {} bytes", m.len());
        }
    }

    #[test]
    fn streaming_matches_one_shot() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 7 + 3) as u8).collect();
        for cut in [0, 1, 55, 56, 63, 64, 65, 111, 112, 127, 128, 129, 500, 1000] {
            let mut a = Sha256::new();
            a.update(&data[..cut]);
            a.update(&data[cut..]);
            assert_eq!(a.finish(), sha256(&data), "sha256 cut {}", cut);
            let mut b = Sha512::new();
            for c in data[..cut].chunks(3) {
                b.update(c);
            }
            b.update(&data[cut..]);
            assert_eq!(b.finish(), sha512(&data), "sha512 cut {}", cut);
        }
    }

    /// A megabyte and 77 bytes of xorshift64 output, fed in pieces of 1 to 997 bytes; the digests are Python's
    /// `hashlib` of the same bytes.
    #[test]
    fn large_pseudo_random_input() {
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let data: Vec<u8> = (0..(1 << 20) + 77)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 32) as u8
            })
            .collect();
        let (mut a, mut b) = (Sha256::new(), Sha512::new());
        let (mut rest, mut piece) = (&data[..], 1);
        while !rest.is_empty() {
            let (p, r) = rest.split_at(piece.min(rest.len()));
            a.update(p);
            b.update(p);
            rest = r;
            piece = piece % 997 + 1;
        }
        assert_eq!(hex(&a.finish()), "960c399057973f18c12722efef778d948de553267837cb884ff2fb858c86c8e0");
        assert_eq!(
            hex(&b.finish()),
            "1fa9c19cf34257c9c09ca3b215bc101b35c87a2f96a8948097bc94094df8cede\
             2a936cb416e34d0fc69e24496d11050d23894a46a121fabfacfa7ce2abc1b0c2"
        );
    }

    #[test]
    fn sha1_examples() {
        assert_eq!(hex(&sha1(&[b"abc"])), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hex(&sha1(&[b""])), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(&sha1(&[b"abcdbcdecdefdefgefghfghighij", b"hijkijkljklmklmnlmnomnopnopq"])),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(unhex("00ff10"), Some(vec![0, 255, 16]));
        assert_eq!(unhex("0"), None);
        assert_eq!(unhex("zz"), None);
        assert_eq!(hex(&[0xab, 1]), "ab01");
    }
}
