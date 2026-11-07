// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Ed25519 signatures (RFC 8032), no dependencies. Written for clarity, not speed or constant time: OOS only
//! verifies update packages on the unit (a public operation), and signing runs on the developer's machine.
//!
//! Field GF(2^255 - 19) in five 51-bit limbs, points in extended coordinates (X:Y:Z:T) on -x^2 + y^2 = 1 + d x^2 y^2,
//! the complete addition law for doubling too. d, sqrt(-1) and the base point are computed once from their
//! definitions (d = -121665/121666, sqrt(-1) = 2^((p-1)/4), B = the point with y = 4/5 and even x), so no long
//! constant is typed in. Scalars mod L by plain shift-and-subtract. The tests check the RFC 8032 vectors.

use super::hash::Sha512;
use std::sync::OnceLock;

const MASK: u64 = (1 << 51) - 1;

#[derive(Clone, Copy, Debug)]
struct Fe([u64; 5]);

impl Fe {
    const ZERO: Fe = Fe([0; 5]);
    const ONE: Fe = Fe([1, 0, 0, 0, 0]);

    fn small(v: u64) -> Fe {
        Fe([v, 0, 0, 0, 0])
    }

    /// Low 255 bits of 32 little-endian bytes (the top bit is ignored).
    fn from_bytes(b: &[u8; 32]) -> Fe {
        let mut w = [0u64; 4];
        for (i, x) in w.iter_mut().enumerate() {
            let mut q = [0u8; 8];
            q.copy_from_slice(&b[8 * i..8 * i + 8]);
            *x = u64::from_le_bytes(q);
        }
        Fe([
            w[0] & MASK,
            ((w[0] >> 51) | (w[1] << 13)) & MASK,
            ((w[1] >> 38) | (w[2] << 26)) & MASK,
            ((w[2] >> 25) | (w[3] << 39)) & MASK,
            (w[3] >> 12) & MASK,
        ])
    }

    /// Limbs back below 2^51 (plus a small carry into limb 0).
    fn carry(mut h: [u64; 5]) -> Fe {
        for i in 0..4 {
            h[i + 1] += h[i] >> 51;
            h[i] &= MASK;
        }
        h[0] += 19 * (h[4] >> 51);
        h[4] &= MASK;
        h[1] += h[0] >> 51;
        h[0] &= MASK;
        Fe(h)
    }

    /// Canonical 32-byte encoding (fully reduced mod p).
    fn to_bytes(&self) -> [u8; 32] {
        let mut h = Fe::carry(self.0).0;
        // q = 1 exactly when h >= p
        let mut q = (h[0] + 19) >> 51;
        for x in &h[1..] {
            q = (x + q) >> 51;
        }
        h[0] += 19 * q;
        for i in 0..4 {
            h[i + 1] += h[i] >> 51;
            h[i] &= MASK;
        }
        h[4] &= MASK;
        let w = [
            h[0] | (h[1] << 51),
            (h[1] >> 13) | (h[2] << 38),
            (h[2] >> 26) | (h[3] << 25),
            (h[3] >> 39) | (h[4] << 12),
        ];
        let mut out = [0u8; 32];
        for (i, x) in w.iter().enumerate() {
            out[8 * i..8 * i + 8].copy_from_slice(&x.to_le_bytes());
        }
        out
    }

    fn add(&self, b: &Fe) -> Fe {
        let mut h = [0u64; 5];
        for i in 0..5 {
            h[i] = self.0[i] + b.0[i];
        }
        Fe::carry(h)
    }

    fn sub(&self, b: &Fe) -> Fe {
        // + 4p so every limb stays positive (inputs are carried: limbs < 2^52)
        const P4: [u64; 5] = [4 * ((1 << 51) - 19), 4 * MASK, 4 * MASK, 4 * MASK, 4 * MASK];
        let mut h = [0u64; 5];
        for i in 0..5 {
            h[i] = self.0[i] + P4[i] - b.0[i];
        }
        Fe::carry(h)
    }

    fn neg(&self) -> Fe {
        Fe::ZERO.sub(self)
    }

    fn mul(&self, b: &Fe) -> Fe {
        let a = self.0.map(|x| x as u128);
        let b = b.0.map(|x| x as u128);
        let b19 = b.map(|x| 19 * x);
        let r = [
            a[0] * b[0] + a[1] * b19[4] + a[2] * b19[3] + a[3] * b19[2] + a[4] * b19[1],
            a[0] * b[1] + a[1] * b[0] + a[2] * b19[4] + a[3] * b19[3] + a[4] * b19[2],
            a[0] * b[2] + a[1] * b[1] + a[2] * b[0] + a[3] * b19[4] + a[4] * b19[3],
            a[0] * b[3] + a[1] * b[2] + a[2] * b[1] + a[3] * b[0] + a[4] * b19[4],
            a[0] * b[4] + a[1] * b[3] + a[2] * b[2] + a[3] * b[1] + a[4] * b[0],
        ];
        let mut h = [0u64; 5];
        let mut c: u128 = 0;
        for i in 0..5 {
            let v = r[i] + c;
            h[i] = (v as u64) & MASK;
            c = v >> 51;
        }
        h[0] += 19 * (c as u64);
        Fe::carry(h)
    }

    fn sq(&self) -> Fe {
        self.mul(self)
    }

    /// self^e, e as 32 little-endian bytes.
    fn pow(&self, e: &[u8; 32]) -> Fe {
        let mut r = Fe::ONE;
        for i in (0..256).rev() {
            r = r.sq();
            if (e[i / 8] >> (i % 8)) & 1 == 1 {
                r = r.mul(self);
            }
        }
        r
    }

    fn invert(&self) -> Fe {
        // p - 2 = 2^255 - 21
        let mut e = [0xffu8; 32];
        e[0] = 0xeb;
        e[31] = 0x7f;
        self.pow(&e)
    }

    fn eq(&self, b: &Fe) -> bool {
        self.to_bytes() == b.to_bytes()
    }

    fn is_odd(&self) -> bool {
        self.to_bytes()[0] & 1 == 1
    }
}

#[derive(Clone, Copy, Debug)]
struct Point {
    x: Fe,
    y: Fe,
    z: Fe,
    t: Fe,
}

struct Consts {
    d: Fe,
    d2: Fe,
    sqrt_m1: Fe,
    base: Point,
}

fn consts() -> &'static Consts {
    static C: OnceLock<Consts> = OnceLock::new();
    C.get_or_init(|| {
        let d = Fe::small(121665).neg().mul(&Fe::small(121666).invert());
        // (p - 1) / 4 = 2^253 - 5
        let mut e = [0xffu8; 32];
        e[0] = 0xfb;
        e[31] = 0x1f;
        let sqrt_m1 = Fe::small(2).pow(&e);
        let mut c = Consts { d, d2: d.add(&d), sqrt_m1, base: Point::IDENTITY };
        // B: y = 4/5, x even -> encoding 0x58 0x66 .. 0x66
        let mut b = [0x66u8; 32];
        b[0] = 0x58;
        c.base = Point::decode_with(&b, &c).expect("Ed25519 base point");
        c
    })
}

impl Point {
    const IDENTITY: Point = Point { x: Fe::ZERO, y: Fe::ONE, z: Fe::ONE, t: Fe::ZERO };

    fn add(&self, q: &Point) -> Point {
        let c = consts();
        let a = self.y.sub(&self.x).mul(&q.y.sub(&q.x));
        let b = self.y.add(&self.x).mul(&q.y.add(&q.x));
        let cc = self.t.mul(&c.d2).mul(&q.t);
        let d = self.z.add(&self.z).mul(&q.z);
        let (e, f, g, h) = (b.sub(&a), d.sub(&cc), d.add(&cc), b.add(&a));
        Point { x: e.mul(&f), y: g.mul(&h), z: f.mul(&g), t: e.mul(&h) }
    }

    fn neg(&self) -> Point {
        Point { x: self.x.neg(), y: self.y, z: self.z, t: self.t.neg() }
    }

    /// [k]P, k as 32 little-endian bytes (variable time).
    fn mul(&self, k: &[u8; 32]) -> Point {
        let mut r = Point::IDENTITY;
        for i in (0..256).rev() {
            r = r.add(&r);
            if (k[i / 8] >> (i % 8)) & 1 == 1 {
                r = r.add(self);
            }
        }
        r
    }

    fn encode(&self) -> [u8; 32] {
        let zi = self.z.invert();
        let x = self.x.mul(&zi);
        let mut out = self.y.mul(&zi).to_bytes();
        if x.is_odd() {
            out[31] |= 0x80;
        }
        out
    }

    fn decode(b: &[u8; 32]) -> Option<Point> {
        Point::decode_with(b, consts())
    }

    /// RFC 8032 5.1.3 (non-canonical y refused).
    fn decode_with(b: &[u8; 32], c: &Consts) -> Option<Point> {
        let sign = b[31] >> 7 == 1;
        let y = Fe::from_bytes(b);
        let mut canon = *b;
        canon[31] &= 0x7f;
        if y.to_bytes() != canon {
            return None;
        }
        let yy = y.sq();
        let u = yy.sub(&Fe::ONE);
        let v = c.d.mul(&yy).add(&Fe::ONE);
        // x = u v^3 (u v^7)^((p-5)/8), (p - 5) / 8 = 2^252 - 3
        let mut e = [0xffu8; 32];
        e[0] = 0xfd;
        e[31] = 0x0f;
        let v3 = v.sq().mul(&v);
        let v7 = v3.sq().mul(&v);
        let mut x = u.mul(&v3).mul(&u.mul(&v7).pow(&e));
        let vxx = v.mul(&x.sq());
        if vxx.eq(&u) {
        } else if vxx.eq(&u.neg()) {
            x = x.mul(&c.sqrt_m1);
        } else {
            return None;
        }
        let zero = x.eq(&Fe::ZERO);
        if zero && sign {
            return None;
        }
        if x.is_odd() != sign {
            x = x.neg();
        }
        Some(Point { x, y, z: Fe::ONE, t: x.mul(&y) })
    }
}

/// Group order L = 2^252 + 27742317777372353535851937790883648493, little-endian 64-bit words.
const L: [u64; 4] = [0x5812631a5cf5d3ed, 0x14def9dea2f79cd6, 0, 0x1000000000000000];

fn ge(a: &[u64; 4], b: &[u64; 4]) -> bool {
    for i in (0..4).rev() {
        if a[i] != b[i] {
            return a[i] > b[i];
        }
    }
    true
}

fn sub_l(a: &mut [u64; 4]) {
    let mut borrow = 0u64;
    for i in 0..4 {
        let (v, b1) = a[i].overflowing_sub(L[i]);
        let (v, b2) = v.overflowing_sub(borrow);
        a[i] = v;
        borrow = (b1 | b2) as u64;
    }
}

/// A little-endian number of up to 512 bits mod L.
fn reduce(w: &[u64; 8]) -> [u8; 32] {
    let mut r = [0u64; 4];
    for i in (0..512).rev() {
        // r < L < 2^253, so 2r + 1 fits in 4 words
        for j in (1..4).rev() {
            r[j] = (r[j] << 1) | (r[j - 1] >> 63);
        }
        r[0] = (r[0] << 1) | ((w[i / 64] >> (i % 64)) & 1);
        if ge(&r, &L) {
            sub_l(&mut r);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in r.iter().enumerate() {
        out[8 * i..8 * i + 8].copy_from_slice(&x.to_le_bytes());
    }
    out
}

fn words<const N: usize>(b: &[u8]) -> [u64; N] {
    let mut w = [0u64; N];
    for (i, x) in w.iter_mut().enumerate() {
        let mut q = [0u8; 8];
        q.copy_from_slice(&b[8 * i..8 * i + 8]);
        *x = u64::from_le_bytes(q);
    }
    w
}

fn reduce64(h: &[u8; 64]) -> [u8; 32] {
    reduce(&words::<8>(h))
}

/// (r + k a) mod L.
fn mul_add(k: &[u8; 32], a: &[u8; 32], r: &[u8; 32]) -> [u8; 32] {
    let (k, a, r) = (words::<4>(k), words::<4>(a), words::<4>(r));
    let mut p = [0u128; 9];
    for i in 0..4 {
        let mut c: u128 = 0;
        for j in 0..4 {
            let v = p[i + j] + (k[i] as u128) * (a[j] as u128) + c;
            p[i + j] = v & (u64::MAX as u128);
            c = v >> 64;
        }
        p[i + 4] += c;
    }
    let mut c: u128 = 0;
    for i in 0..8 {
        let v = p[i] + if i < 4 { r[i] as u128 } else { 0 } + c;
        p[i] = v & (u64::MAX as u128);
        c = v >> 64;
    }
    debug_assert_eq!(c, 0);
    let w: [u64; 8] = std::array::from_fn(|i| p[i] as u64);
    reduce(&w)
}

fn hash(parts: &[&[u8]]) -> [u8; 64] {
    let mut h = Sha512::new();
    for p in parts {
        h.update(p);
    }
    h.finish()
}

fn expand(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let h = hash(&[seed]);
    let mut a = [0u8; 32];
    a.copy_from_slice(&h[..32]);
    a[0] &= 248;
    a[31] &= 127;
    a[31] |= 64;
    let mut prefix = [0u8; 32];
    prefix.copy_from_slice(&h[32..]);
    (a, prefix)
}

/// Public key of a 32-byte secret seed.
pub fn public_key(seed: &[u8; 32]) -> [u8; 32] {
    consts().base.mul(&expand(seed).0).encode()
}

/// Signature of `msg` with the secret seed.
pub fn sign(seed: &[u8; 32], msg: &[u8]) -> [u8; 64] {
    let (a, prefix) = expand(seed);
    let b = &consts().base;
    let pk = b.mul(&a).encode();
    let r = reduce64(&hash(&[&prefix, msg]));
    let rr = b.mul(&r).encode();
    let k = reduce64(&hash(&[&rr, &pk, msg]));
    let s = mul_add(&k, &a, &r);
    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&rr);
    sig[32..].copy_from_slice(&s);
    sig
}

/// True when `sig` is a valid signature of `msg` under the public key `pk` ([S]B = R + [k]A, S < L).
pub fn verify(pk: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> bool {
    let Some(a) = Point::decode(pk) else { return false };
    let mut rr = [0u8; 32];
    rr.copy_from_slice(&sig[..32]);
    let mut s = [0u8; 32];
    s.copy_from_slice(&sig[32..]);
    if ge(&words::<4>(&s), &L) {
        return false;
    }
    let k = reduce64(&hash(&[&rr, pk, msg]));
    let check = consts().base.mul(&s).add(&a.neg().mul(&k));
    check.encode() == rr
}

#[cfg(test)]
mod tests {
    use super::super::hash::{hex, unhex};
    use super::*;

    fn arr<const N: usize>(s: &str) -> [u8; N] {
        unhex(s).unwrap().try_into().unwrap()
    }

    // RFC 8032 7.1, TEST 1..3 and TEST SHA(abc)
    const VECTORS: [(&str, &str, &str, &str); 4] = [
        (
            "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
            "",
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        ),
        (
            "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
            "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
            "72",
            "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
        ),
        (
            "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
            "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
            "af82",
            "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
        ),
        (
            "833fe62409237b9d62ec77587520911e9a759cec1d19755b7da901b96dca3d42",
            "ec172b93ad5e563bf4932c70e1245034c35467ef2efd4d64ebf819683467e2bf",
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
            "dc2a4459e7369633a52b1bf277839a00201009a3efbf3ecb69bea2186c26b58909351fc9ac90b3ecfdfbc7c66431e0303dca179c138ac17ad9bef1177331a704",
        ),
    ];

    #[test]
    fn rfc8032_vectors() {
        for (sk, pk, msg, sig) in VECTORS {
            let sk: [u8; 32] = arr(sk);
            let pk: [u8; 32] = arr(pk);
            let msg = unhex(msg).unwrap();
            let sig: [u8; 64] = arr(sig);
            assert_eq!(hex(&public_key(&sk)), hex(&pk));
            assert_eq!(hex(&sign(&sk, &msg)), hex(&sig));
            assert!(verify(&pk, &msg, &sig));
        }
    }

    #[test]
    fn tampering_is_refused() {
        let (sk, pk, _, _) = VECTORS[2];
        let sk: [u8; 32] = arr(sk);
        let pk: [u8; 32] = arr(pk);
        let msg = b"OOS update package".to_vec();
        let sig = sign(&sk, &msg);
        assert!(verify(&pk, &msg, &sig));
        let mut m2 = msg.clone();
        m2[3] ^= 1;
        assert!(!verify(&pk, &m2, &sig));
        for i in [0, 31, 32, 63] {
            let mut s2 = sig;
            s2[i] ^= 0x10;
            assert!(!verify(&pk, &msg, &s2), "signature byte {} flipped", i);
        }
        // another key
        let other = public_key(&[7; 32]);
        assert!(!verify(&other, &msg, &sig));
        // S + L (malleability) refused
        let mut s = words::<4>(&sig[32..]);
        let mut c = 0u128;
        for i in 0..4 {
            let v = s[i] as u128 + L[i] as u128 + c;
            s[i] = v as u64;
            c = v >> 64;
        }
        let mut s3 = sig;
        for i in 0..4 {
            s3[32 + 8 * i..40 + 8 * i].copy_from_slice(&s[i].to_le_bytes());
        }
        assert!(!verify(&pk, &msg, &s3));
        // a key that is not a curve point
        let mut bad = pk;
        bad[0] ^= 1;
        let _ = verify(&bad, &msg, &sig); // must not panic
    }

    #[test]
    fn order_of_the_base_point_is_l() {
        let mut l = [0u8; 32];
        for i in 0..4 {
            l[8 * i..8 * i + 8].copy_from_slice(&L[i].to_le_bytes());
        }
        let p = consts().base.mul(&l);
        assert_eq!(p.encode(), Point::IDENTITY.encode());
    }
}
