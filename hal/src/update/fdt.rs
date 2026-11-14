// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Device trees for the trial start of an update that brings another tree (module `kexec`).
//!
//! The tree in an image (`rk-kernel.dtb` in its RSCE section) is not the tree the kernel gets: the stock loader adds
//! to it at every start (reserved memory for the ramdisk and the boot logo, the command line, the display mode it
//! set up). A kexec start with the new image's tree needs those too, or it would try something else than a power-on
//! does. So the new tree gets what the loader changed in the running one: the difference between the tree in the
//! image that started (`BOOTED_DT`) and the tree the kernel got (`/sys/firmware/fdt`), applied by node path.
//!
//! What the loader writes is plain values (addresses, sizes, strings), no phandles: a phandle of the old tree would
//! name another node in the new one (dtc numbers them anew). A difference in a `phandle` property, or one that cannot
//! be put into the new tree (its node is not there), refuses the kexec start; the install then needs `--new-dt`.

/// A node: its name (with the unit address), properties in order, children in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub name: String,
    pub props: Vec<(String, Vec<u8>)>,
    pub kids: Vec<Node>,
}

impl Node {
    pub fn prop(&self, name: &str) -> Option<&[u8]> {
        self.props.iter().find(|p| p.0 == name).map(|p| p.1.as_slice())
    }

    fn set(&mut self, name: &str, v: &[u8]) {
        match self.props.iter_mut().find(|p| p.0 == name) {
            Some(p) => p.1 = v.to_vec(),
            None => self.props.push((name.to_string(), v.to_vec())),
        }
    }

    fn kid(&self, name: &str) -> Option<&Node> {
        self.kids.iter().find(|k| k.name == name)
    }

    fn kid_mut(&mut self, name: &str) -> Option<&mut Node> {
        self.kids.iter_mut().find(|k| k.name == name)
    }
}

/// A flattened device tree: the memory reservations, the boot CPU, the nodes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fdt {
    pub rsv: Vec<(u64, u64)>,
    pub boot_cpu: u32,
    pub root: Node,
}

const MAGIC: u32 = 0xd00d_feed;
const BEGIN: u32 = 1;
const END: u32 = 2;
const PROP: u32 = 3;
const NOP: u32 = 4;
const FINISH: u32 = 9;

fn be32(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).map(|x| u32::from_be_bytes([x[0], x[1], x[2], x[3]])).ok_or_else(|| "device tree cut short".to_string())
}

fn cstr(b: &[u8], at: usize) -> Result<&str, String> {
    let rest = b.get(at..).ok_or("device tree: a name out of bounds")?;
    let n = rest.iter().position(|&c| c == 0).ok_or("device tree: a name without its end")?;
    std::str::from_utf8(&rest[..n]).map_err(|_| "device tree: a name that is not text".to_string())
}

impl Fdt {
    pub fn parse(b: &[u8]) -> Result<Fdt, String> {
        if be32(b, 0)? != MAGIC {
            return Err("not a device tree".into());
        }
        let total = be32(b, 4)? as usize;
        let (off_struct, off_strings, off_rsv) = (be32(b, 8)? as usize, be32(b, 12)? as usize, be32(b, 16)? as usize);
        if total > b.len() || be32(b, 20)? < 16 {
            return Err("device tree: wrong size or version".into());
        }
        let b = &b[..total];
        let boot_cpu = be32(b, 28)?;
        let mut rsv = Vec::new();
        let mut at = off_rsv;
        loop {
            let a = (be32(b, at)? as u64) << 32 | be32(b, at + 4)? as u64;
            let s = (be32(b, at + 8)? as u64) << 32 | be32(b, at + 12)? as u64;
            at += 16;
            if a == 0 && s == 0 {
                break;
            }
            rsv.push((a, s));
        }
        let mut stack: Vec<Node> = Vec::new();
        let mut root = None;
        let mut p = off_struct;
        loop {
            let tok = be32(b, p)?;
            p += 4;
            match tok {
                BEGIN => {
                    let name = cstr(b, p)?.to_string();
                    p += (name.len() + 1).div_ceil(4) * 4;
                    stack.push(Node { name, ..Default::default() });
                }
                END => {
                    let n = stack.pop().ok_or("device tree: an end without a node")?;
                    match stack.last_mut() {
                        Some(parent) => parent.kids.push(n),
                        None => root = Some(n),
                    }
                }
                PROP => {
                    let (len, nameoff) = (be32(b, p)? as usize, be32(b, p + 4)? as usize);
                    let v = b.get(p + 8..p + 8 + len).ok_or("device tree: a property out of bounds")?.to_vec();
                    let name = cstr(b, off_strings + nameoff)?.to_string();
                    p += 8 + len.div_ceil(4) * 4;
                    stack.last_mut().ok_or("device tree: a property outside a node")?.props.push((name, v));
                }
                NOP => {}
                FINISH => break,
                t => return Err(format!("device tree: unknown token {}", t)),
            }
        }
        let root = root.ok_or("device tree without a root node")?;
        if !stack.is_empty() {
            return Err("device tree: a node without its end".into());
        }
        Ok(Fdt { rsv, boot_cpu, root })
    }

    /// The tree as a blob (version 17), property names stored once.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut strings: Vec<u8> = Vec::new();
        let mut names: Vec<(String, u32)> = Vec::new();
        let mut st: Vec<u8> = Vec::new();
        fn put(n: &Node, st: &mut Vec<u8>, strings: &mut Vec<u8>, names: &mut Vec<(String, u32)>) {
            st.extend_from_slice(&BEGIN.to_be_bytes());
            st.extend_from_slice(n.name.as_bytes());
            st.push(0);
            while st.len() % 4 != 0 {
                st.push(0);
            }
            for (k, v) in &n.props {
                let off = match names.iter().find(|x| &x.0 == k) {
                    Some(x) => x.1,
                    None => {
                        let o = strings.len() as u32;
                        strings.extend_from_slice(k.as_bytes());
                        strings.push(0);
                        names.push((k.clone(), o));
                        o
                    }
                };
                st.extend_from_slice(&PROP.to_be_bytes());
                st.extend_from_slice(&(v.len() as u32).to_be_bytes());
                st.extend_from_slice(&off.to_be_bytes());
                st.extend_from_slice(v);
                while st.len() % 4 != 0 {
                    st.push(0);
                }
            }
            for k in &n.kids {
                put(k, st, strings, names);
            }
            st.extend_from_slice(&END.to_be_bytes());
        }
        put(&self.root, &mut st, &mut strings, &mut names);
        st.extend_from_slice(&FINISH.to_be_bytes());
        let off_rsv = 40usize;
        let off_struct = off_rsv + (self.rsv.len() + 1) * 16;
        let off_strings = off_struct + st.len();
        let total = off_strings + strings.len();
        let mut b = Vec::with_capacity(total);
        for v in [MAGIC, total as u32, off_struct as u32, off_strings as u32, off_rsv as u32, 17, 16, self.boot_cpu, strings.len() as u32, st.len() as u32] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        for &(a, s) in self.rsv.iter().chain(std::iter::once(&(0, 0))) {
            b.extend_from_slice(&a.to_be_bytes());
            b.extend_from_slice(&s.to_be_bytes());
        }
        b.extend_from_slice(&st);
        b.extend_from_slice(&strings);
        b
    }
}

/// A file of a Rockchip resource container (RSCE, the `second` section of an image).
pub fn rsce_file<'a>(blob: &'a [u8], name: &str) -> Result<&'a [u8], String> {
    if blob.get(..4) != Some(b"RSCE") || blob.len() < 16 {
        return Err("the device tree section is no RSCE container".into());
    }
    let (toff, eblk) = (blob[9] as usize, blob[10] as usize);
    let n = be32_le(blob, 12)? as usize;
    for i in 0..n {
        let at = (toff + i * eblk) * 512;
        let row = blob.get(at..at + 268).ok_or("RSCE: entry out of bounds")?;
        let end = row[4..260].iter().position(|&c| c == 0).unwrap_or(256);
        if &row[4..4 + end] == name.as_bytes() {
            let (first, size) = (be32_le(row, 260)? as usize, be32_le(row, 264)? as usize);
            return blob.get(first * 512..first * 512 + size).ok_or_else(|| format!("RSCE: {} out of bounds", name));
        }
    }
    Err(format!("RSCE: no {}", name))
}

fn be32_le(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]])).ok_or_else(|| "RSCE cut short".to_string())
}

/// The tree file in an image's device tree section.
pub const TREE: &str = "rk-kernel.dtb";

/// One change the loader made, by node path.
#[derive(Clone, Debug, PartialEq)]
enum Change {
    Set(String, String, Vec<u8>),
    Remove(String, String),
    AddNode(String, Node),
    RemoveNode(String, String),
}

fn diff(path: &str, old: &Node, run: &Node, out: &mut Vec<Change>) {
    for (k, v) in &run.props {
        if old.prop(k) != Some(v.as_slice()) {
            out.push(Change::Set(path.into(), k.clone(), v.clone()));
        }
    }
    for (k, _) in &old.props {
        if run.prop(k).is_none() {
            out.push(Change::Remove(path.into(), k.clone()));
        }
    }
    let sub = |n: &str| if path == "/" { format!("/{}", n) } else { format!("{}/{}", path, n) };
    for r in &run.kids {
        match old.kid(&r.name) {
            Some(o) => diff(&sub(&r.name), o, r, out),
            None => out.push(Change::AddNode(path.into(), r.clone())),
        }
    }
    for o in &old.kids {
        if run.kid(&o.name).is_none() {
            out.push(Change::RemoveNode(path.into(), o.name.clone()));
        }
    }
}

fn node_at<'a>(root: &'a mut Node, path: &str) -> Option<&'a mut Node> {
    let mut n = root;
    for part in path.split('/').filter(|p| !p.is_empty()) {
        n = n.kid_mut(part)?;
    }
    Some(n)
}

/// The ramdisk the kernel of this tree got: [start, end) from /chosen.
fn ramdisk(root: &Node) -> Option<(u64, u64)> {
    let c = root.kid("chosen")?;
    let num = |v: &[u8]| match v.len() {
        4 => Some(u32::from_be_bytes([v[0], v[1], v[2], v[3]]) as u64),
        8 => Some(u64::from_be_bytes([v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]])),
        _ => None,
    };
    Some((num(c.prop("linux,initrd-start")?)?, num(c.prop("linux,initrd-end")?)?))
}

fn has_phandle(n: &Node) -> bool {
    n.prop("phandle").is_some() || n.prop("linux,phandle").is_some() || n.kids.iter().any(has_phandle)
}

/// The new image's tree (`new`, a blob) with what the loader changed from `old` (the tree in the image that started) to
/// `running` (the tree the kernel got): the tree a power-on of the new image would give its kernel. Also the number of
/// changes carried over.
pub fn with_loader_changes(new: &[u8], old: &[u8], running: &[u8]) -> Result<(Vec<u8>, usize), String> {
    let (mut t, o, r) = (Fdt::parse(new)?, Fdt::parse(old)?, Fdt::parse(running)?);
    let mut ch = Vec::new();
    diff("/", &o.root, &r.root, &mut ch);
    // the running kernel's ramdisk is not the kexec start's: kexec-tools names the new one in /chosen. Its old place
    // stays out, also as a reservation: the kernel drops a ramdisk that lies in reserved memory (seen: the 55 MB of
    // the kexec start fell into the 13 MB the loader had reserved for its own, and the kernel found no root).
    let initrd = ramdisk(&r.root);
    ch.retain(|c| !matches!(c, Change::Set(p, k, _) if p == "/chosen" && k.starts_with("linux,initrd-")));
    let n = ch.len();
    for c in ch {
        let refuse = |what: &str| Err(format!("the loader's change to {} cannot go into the new device tree", what));
        match c {
            Change::Set(p, k, v) => {
                if k == "phandle" || k == "linux,phandle" {
                    return refuse(&format!("{}:{}", p, k));
                }
                match node_at(&mut t.root, &p) {
                    Some(n) => n.set(&k, &v),
                    None => return refuse(&p),
                }
            }
            Change::Remove(p, k) => {
                if let Some(n) = node_at(&mut t.root, &p) {
                    n.props.retain(|x| x.0 != k);
                }
            }
            Change::AddNode(p, node) => {
                if has_phandle(&node) {
                    return refuse(&format!("{}/{}", p, node.name));
                }
                match node_at(&mut t.root, &p) {
                    Some(n) => match n.kid_mut(&node.name) {
                        Some(k) => *k = node,
                        None => n.kids.push(node),
                    },
                    None => return refuse(&p),
                }
            }
            Change::RemoveNode(p, name) => {
                if let Some(n) = node_at(&mut t.root, &p) {
                    n.kids.retain(|k| k.name != name);
                }
            }
        }
    }
    // the reservations the loader added, except the one of its ramdisk
    let added: Vec<(u64, u64)> =
        r.rsv.iter().filter(|x| !o.rsv.contains(x) && !initrd.is_some_and(|(a, e)| x.0 < e && a < x.0 + x.1)).copied().collect();
    let rsv_n = added.len();
    for x in added {
        if !t.rsv.contains(&x) {
            t.rsv.push(x);
        }
    }
    t.boot_cpu = r.boot_cpu;
    Ok((t.to_bytes(), n + rsv_n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, props: &[(&str, &[u8])], kids: Vec<Node>) -> Node {
        Node { name: name.into(), props: props.iter().map(|(k, v)| (k.to_string(), v.to_vec())).collect(), kids }
    }

    fn tree(root: Node, rsv: Vec<(u64, u64)>) -> Vec<u8> {
        Fdt { rsv, boot_cpu: 0, root }.to_bytes()
    }

    #[test]
    fn a_tree_reads_back() {
        let t = Fdt {
            rsv: vec![(0x460_0000, 0x17000)],
            boot_cpu: 0,
            root: node("", &[("compatible", b"rigol,dho900\0"), ("#address-cells", &[0, 0, 0, 2])], vec![node("chosen", &[("bootargs", b"console=ttyS2\0")], vec![]), node("gpu@ff9a0000", &[("status", b"okay\0"), ("phandle", &[0, 0, 0, 7]), ("compatible", b"x\0")], vec![])]),
        };
        let b = t.to_bytes();
        assert_eq!(Fdt::parse(&b).unwrap(), t);
        assert_eq!(&b[..4], &MAGIC.to_be_bytes());
    }

    #[test]
    fn the_loaders_changes_go_into_the_new_tree() {
        // the image that started, what the loader made of it, the new image (another phandle numbering, a new node)
        let old = node("", &[("model", b"DHO\0")], vec![node("chosen", &[("bootargs", b"a\0")], vec![]), node("route", &[("phandle", &[0, 0, 0, 5])], vec![node("dsi", &[("logo,mode", &[0])], vec![])]), node("gone", &[], vec![])]);
        let run = node("", &[("model", b"DHO\0")], vec![node("chosen", &[("bootargs", b"a b\0"), ("linux,initrd-start", &[0, 0, 0, 0, 4, 0xbf, 0, 0]), ("linux,initrd-end", &[0, 0, 0, 0, 5, 0x17, 0x59, 0x44])], vec![]), node("route", &[("phandle", &[0, 0, 0, 5])], vec![node("dsi", &[("logo,width", &[2, 88])], vec![])]), node("memory", &[("reg", &[1])], vec![])]);
        let new = node("", &[("model", b"DHO\0")], vec![node("chosen", &[("bootargs", b"a\0")], vec![]), node("route", &[("phandle", &[0, 0, 0, 6])], vec![node("dsi", &[("logo,mode", &[0])], vec![])]), node("gone", &[], vec![]), node("gpu", &[("status", b"okay\0")], vec![])]);
        let (m, n) = with_loader_changes(&tree(new, vec![]), &tree(old, vec![]), &tree(run, vec![(0x460_0000, 0x17000), (0x4bf_0000, 0x58_5944)])).unwrap();
        let m = Fdt::parse(&m).unwrap();
        let chosen = m.root.kid("chosen").unwrap();
        assert_eq!(chosen.prop("bootargs"), Some(&b"a b\0"[..]));
        assert_eq!((chosen.prop("linux,initrd-start"), chosen.prop("linux,initrd-end")), (None, None), "the running ramdisk is not the new one");
        let dsi = m.root.kid("route").unwrap().kid("dsi").unwrap();
        assert_eq!((dsi.prop("logo,width"), dsi.prop("logo,mode")), (Some(&[2u8, 88][..]), None));
        assert_eq!(m.root.kid("route").unwrap().prop("phandle"), Some(&[0u8, 0, 0, 6][..]), "the new tree's numbering stays");
        assert!(m.root.kid("memory").is_some() && m.root.kid("gone").is_none() && m.root.kid("gpu").is_some());
        assert_eq!(m.rsv, vec![(0x460_0000, 0x17000)], "nor its reservation");
        assert_eq!(n, 6);
    }

    #[test]
    fn a_phandle_change_is_refused() {
        let old = node("", &[], vec![node("a", &[("phandle", &[0, 0, 0, 1])], vec![])]);
        let run = node("", &[], vec![node("a", &[("phandle", &[0, 0, 0, 2])], vec![])]);
        assert!(with_loader_changes(&tree(old.clone(), vec![]), &tree(old.clone(), vec![]), &tree(run, vec![])).is_err());
        // a change to a node the new tree does not have
        let run = node("", &[], vec![node("a", &[("phandle", &[0, 0, 0, 1]), ("x", &[1])], vec![])]);
        let new = node("", &[], vec![]);
        assert!(with_loader_changes(&tree(new, vec![]), &tree(old, vec![]), &tree(run, vec![])).is_err());
    }

    #[test]
    fn the_tree_of_an_rsce_container() {
        let dtb = tree(node("", &[("model", b"x\0")], vec![]), vec![]);
        // header block, one entry, the file from block 2 on
        let mut b = vec![0u8; 1024];
        b[..4].copy_from_slice(b"RSCE");
        b[8..12].copy_from_slice(&[1, 1, 1, 0]);
        b[12..16].copy_from_slice(&1u32.to_le_bytes());
        b[512..516].copy_from_slice(b"ENTR");
        b[516..516 + TREE.len()].copy_from_slice(TREE.as_bytes());
        b[512 + 260..512 + 264].copy_from_slice(&2u32.to_le_bytes());
        b[512 + 264..512 + 268].copy_from_slice(&(dtb.len() as u32).to_le_bytes());
        b.extend_from_slice(&dtb);
        assert_eq!(rsce_file(&b, TREE).unwrap(), &dtb[..]);
        assert!(rsce_file(&b, "other").is_err());
        assert!(rsce_file(&dtb, TREE).is_err());
    }

    /// The trees of the unit (0.1.6's image, the tree its kernel got, the image with the GPU and the VPU): run with
    /// OOS_DT_OLD, OOS_DT_RUNNING, OOS_DT_NEW set to the files.
    #[test]
    #[ignore]
    fn the_units_trees() {
        let f = |v: &str| std::fs::read(std::env::var(v).unwrap()).unwrap();
        let (old, run, new) = (f("OOS_DT_OLD"), f("OOS_DT_RUNNING"), f("OOS_DT_NEW"));
        for b in [&old, &run, &new] {
            let t = Fdt::parse(b).unwrap();
            assert_eq!(Fdt::parse(&t.to_bytes()).unwrap(), t);
        }
        let (m, n) = with_loader_changes(&new, &old, &run).unwrap();
        println!("{} changes of the loader carried over", n);
        if let Ok(p) = std::env::var("OOS_DT_OUT") {
            std::fs::write(p, &m).unwrap();
        }
        // the old image with the loader's changes is the running tree (order aside)
        fn norm(n: &mut Node) {
            n.props.sort();
            n.kids.iter_mut().for_each(norm);
            n.kids.sort_by(|a, b| a.name.cmp(&b.name));
        }
        let (same, _) = with_loader_changes(&old, &old, &run).unwrap();
        let (mut a, mut b) = (Fdt::parse(&same).unwrap(), Fdt::parse(&run).unwrap());
        // all but the running kernel's ramdisk
        let rd = ramdisk(&b.root).unwrap();
        b.root.kid_mut("chosen").unwrap().props.retain(|p| !p.0.starts_with("linux,initrd-"));
        b.rsv.retain(|x| x.0 != rd.0);
        norm(&mut a.root);
        norm(&mut b.root);
        a.rsv.sort();
        b.rsv.sort();
        assert_eq!(a, b);
    }
}
