// SPDX-License-Identifier: GPL-2.0-or-later

//! `oos-fpgad compress-check`: a compressed read against a raw read of the same record (`run --compress k --all
//! --raw-too`), per channel: which min/max pair layout explains it (`oos_hal::dho::pairs::check`). Offline, runs
//! anywhere (fork, docs/fork/compress.md).

use oos_hal::dho::pairs;
use oos_hal::dho::seq;

/// The report for `<raw.bin> <compressed.bin> --k K [--mask M]`.
pub fn run(args: &[String]) -> Result<String, String> {
    let (mut files, mut k, mut mask) = (Vec::new(), None, 1u32);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--k" => {
                k = Some(args.get(i + 1).ok_or("--k needs a value")?.parse::<usize>().map_err(|e| format!("--k: {}", e))?);
                i += 1;
            }
            "--mask" => {
                let v = args.get(i + 1).ok_or("--mask needs a value")?;
                mask = match v.strip_prefix("0x") {
                    Some(h) => u32::from_str_radix(h, 16),
                    None => v.parse(),
                }
                .map_err(|e| format!("--mask: {}", e))?;
                i += 1;
            }
            f => files.push(f.to_string()),
        }
        i += 1;
    }
    let [raw, comp] = files.as_slice() else { return Err("compress-check <raw.bin> <compressed.bin> --k K [--mask M]".into()) };
    let k = k.filter(|&k| k >= 2).ok_or("--k K (2 or more) is needed")?;
    if !(1..=15).contains(&mask) {
        return Err("--mask 1..0xf".into());
    }
    let read = |f: &String| std::fs::read(f).map_err(|e| format!("{}: {}", f, e));
    Ok(report(&read(raw)?, &read(comp)?, k, mask))
}

/// The report for the two reads (read-out bytes, channels interleaved as `mask` sets them).
pub fn report(raw: &[u8], comp: &[u8], k: usize, mask: u32) -> String {
    let (r, c) = (seq::deinterleave(raw, mask), seq::deinterleave(comp, mask));
    let mut out = String::new();
    for ch in 0..4 {
        if mask & 1 << ch == 0 {
            continue;
        }
        let (r, c) = (&r[ch], &c[ch]);
        out += &format!("CH{}: {} raw points, {} compressed points (raw / k = {})\n", ch + 1, r.len(), c.len(), r.len().div_ceil(k));
        let fits = pairs::check(r, c, k);
        for f in fits.iter().take(3) {
            let l = f.layout;
            out += &format!(
                "  block {}k, {:?} order, shift {:+}: {} of {} points within one code, largest difference {}\n",
                l.block_k,
                l.order,
                l.shift,
                f.within,
                f.points,
                f.max_err
            );
        }
        let best = fits.first();
        let verdict = match best {
            Some(f) if f.points > 0 && f.within == f.points && f.layout == pairs::LAYOUT => "PASS: the assumed layout (pairs::LAYOUT) explains every point".to_string(),
            Some(f) if f.points > 0 && f.within == f.points => format!("LAYOUT DIFFERS: set pairs::BLOCK_K = {}, ORDER = {:?}, shift {:+}", f.layout.block_k, f.layout.order, f.layout.shift),
            _ => "FAIL: no candidate layout explains the read (not the same record, another k, or another kind of compression)".to_string(),
        };
        out += &format!("  {}\n", verdict);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(w: &[u16]) -> Vec<u8> {
        w.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    #[test]
    fn report_names_the_layout() {
        let raw: Vec<u16> = (0..40_000u32).map(|i| (30_000 + (i * 7919) % 3001 + if (i / 900) % 2 == 0 { 0 } else { 2000 }) as u16).collect();
        let ok = report(&bytes(&raw), &bytes(&pairs::model(&raw, 16, pairs::LAYOUT)), 16, 1);
        assert!(ok.contains("PASS"), "{}", ok);
        let other = pairs::Layout { block_k: 1, order: pairs::Order::MinFirst, ..pairs::LAYOUT };
        let differs = report(&bytes(&raw), &bytes(&pairs::model(&raw, 16, other)), 16, 1);
        assert!(differs.contains("BLOCK_K = 1"), "{}", differs);
        let fixed = pairs::Layout { order: pairs::Order::MinFirst, ..pairs::LAYOUT };
        let differs = report(&bytes(&raw), &bytes(&pairs::model(&raw, 16, fixed)), 16, 1);
        assert!(differs.contains("ORDER = MinFirst"), "{}", differs);
        let wrong = report(&bytes(&raw), &bytes(&pairs::model(&raw, 8, pairs::LAYOUT)), 16, 1);
        assert!(wrong.contains("FAIL"), "{}", wrong);
    }
}
